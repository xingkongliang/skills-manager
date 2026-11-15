use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, State};

use crate::core::{
    error::AppError,
    scenario_service,
    skill_store::SkillStore,
    sync_engine, sync_metadata, tool_adapters,
    tool_service,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SkillToolToggleDto {
    pub tool: String,
    pub display_name: String,
    pub installed: bool,
    pub globally_enabled: bool,
    pub enabled: bool,
    /// Whether a `skill_targets` row actually backs this tool — i.e. the
    /// skill has been deployed to it. `enabled` is only the per-preset
    /// intent flag, which `ensure_scenario_skill_tool_defaults` seeds as
    /// `1` for every installed adapter on membership alone (#384).
    pub deployed: bool,
}

fn disabled_tools(store: &SkillStore) -> Vec<String> {
    tool_service::get_disabled_tools(store)
}

/// Sync commands fire one call per `(skill, agent)` pair when PresetBar
/// applies a preset from the in-app workspace view. Route through the
/// coalescing refresh so a burst rebuilds the tray at most once per window
/// instead of once per row.
fn schedule_tray_refresh(app: &AppHandle) {
    crate::schedule_tray_refresh(app);
}

fn sync_skill_to_tool_internal(
    store: &SkillStore,
    skill_id: &str,
    tool: &str,
) -> Result<(), AppError> {
    scenario_service::sync_single_skill_to_tool(
        store,
        skill_id,
        tool,
        scenario_service::DeployIntent::Managed,
    )
}

/// The disk-side half of turning a preset toggle off for one tool: the
/// `skill_targets` record always goes, while whatever is at the recorded path
/// is only removed when it still matches what we deployed (#435).
fn unsync_skill_for_tool_in_preset(
    store: &SkillStore,
    skill_id: &str,
    tool: &str,
) -> Result<(), AppError> {
    let targets = store
        .get_targets_for_skill(skill_id)
        .map_err(AppError::db)?;
    if let Some(target) = targets.iter().find(|target| target.tool == tool) {
        // Same survivor check as `unsync_skill_from_tool`: another tool
        // sharing this skills directory keeps its deployment, under whatever
        // spelling of the path it recorded.
        let mut memo = std::collections::HashMap::new();
        let key = scenario_service::ownership_key(Path::new(&target.target_path), &mut memo);
        let still_referenced = targets.iter().any(|other| {
            other.tool != tool
                && scenario_service::ownership_key(Path::new(&other.target_path), &mut memo) == key
        });
        if still_referenced {
            return store.delete_target(skill_id, tool).map_err(AppError::db);
        }
        sync_engine::remove_recorded_target_or_warn(
            &PathBuf::from(&target.target_path),
            &target.mode,
        );
    }
    store.delete_target(skill_id, tool).map_err(AppError::db)
}

/// The one-pair case of [`scenario_service::set_active_preset_tool_toggles`].
fn set_active_preset_tool_toggle(
    store: &SkillStore,
    skill_id: &str,
    tool: &str,
    enabled: bool,
) -> Result<(), AppError> {
    scenario_service::set_active_preset_tool_toggles(
        store,
        &[skill_id.to_string()],
        &[tool.to_string()],
        enabled,
    )
}

#[tauri::command]
pub async fn sync_skill_to_tool(
    app: AppHandle,
    skill_id: String,
    tool: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let outcome = sync_skill_to_tool_internal(&store, &skill_id, &tool)
            .and_then(|()| set_active_preset_tool_toggle(&store, &skill_id, &tool, true));
        log_sync_outcome(&store, "enable", &skill_id, &tool, outcome.as_ref());
        outcome
    })
    .await?;
    if result.is_ok() {
        schedule_tray_refresh(&app);
    }
    result
}

fn unsync_skill_from_tool_internal(
    store: &SkillStore,
    skill_id: &str,
    tool: &str,
) -> Result<(), AppError> {
    let targets = store
        .get_targets_for_skill(skill_id)
        .map_err(AppError::db)?;

    // Toggling a skill off is the GUI twin of `skills undeploy`, so it
    // needs the same protection: the row says we deployed here, but if
    // the user has since replaced our artifact with content of their
    // own, that content is not ours to delete (#363).
    if let Some(target) = targets.iter().find(|t| t.tool == tool) {
        let target_path = PathBuf::from(&target.target_path);
        // Several tools can resolve to one skills directory, so this
        // exact path may still be deployed for another (skill, tool)
        // that is staying. `apply_remove` has this survivor check;
        // without it here, switching agent A off deletes agent B's
        // live deployment.
        // Compared as directory entries, the way deployment pools them: an
        // agent reaching this directory through a symlinked parent still uses it.
        let mut memo = std::collections::HashMap::new();
        let key = scenario_service::ownership_key(&target_path, &mut memo);
        let still_referenced = store
            .get_all_targets()
            .map_err(AppError::db)?
            .into_iter()
            .any(|other| {
                !(other.skill_id == skill_id && other.tool == tool)
                    && scenario_service::ownership_key(Path::new(&other.target_path), &mut memo)
                        == key
            });
        if still_referenced {
            log::debug!(
                "unsync: keeping {} (still referenced by another target)",
                target_path.display()
            );
        } else {
            match sync_engine::remove_recorded_target(&target_path, &target.mode) {
                Ok(true) => {}
                Ok(false) => log::warn!(
                    "unsync: preserving {} — no longer matches its recorded {} deployment; \
                     removing the record only",
                    target_path.display(),
                    target.mode
                ),
                Err(e) => {
                    log::warn!("unsync: failed to remove {}: {e}", target_path.display())
                }
            }
        }
    }

    store.delete_target(skill_id, tool).map_err(AppError::db)?;

    set_active_preset_tool_toggle(store, skill_id, tool, false)
}

#[tauri::command]
pub async fn unsync_skill_from_tool(
    app: AppHandle,
    skill_id: String,
    tool: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let outcome = unsync_skill_from_tool_internal(&store, &skill_id, &tool);
        log_sync_outcome(&store, "disable", &skill_id, &tool, outcome.as_ref());
        outcome
    })
    .await?;
    if result.is_ok() {
        schedule_tray_refresh(&app);
    }
    result
}

fn log_sync_outcome(
    store: &SkillStore,
    action: &str,
    skill_id: &str,
    tool: &str,
    outcome: Result<&(), &AppError>,
) {
    let name = store
        .get_skill_by_id(skill_id)
        .ok()
        .flatten()
        .map(|s| s.name)
        .unwrap_or_default();
    let mut draft = crate::core::audit_log::AuditDraft::new(action)
        .skill(skill_id.to_string(), name)
        .tool(tool.to_string());
    draft = match outcome {
        Ok(_) => draft.ok(),
        Err(e) => draft.fail(e.to_string()),
    };
    store.log_audit(draft);
}

fn build_skill_tool_toggles(
    store: &SkillStore,
    preset_id: &str,
    skill_id: &str,
) -> Result<Vec<SkillToolToggleDto>, AppError> {
    let disabled = disabled_tools(store);
    let all_adapters = tool_adapters::all_tool_adapters(store);
    let default_enabled_keys: Vec<String> = all_adapters
        .iter()
        .filter(|adapter| adapter.is_installed() && !disabled.contains(&adapter.key))
        .map(|adapter| adapter.key.clone())
        .collect();
    store
        .ensure_scenario_skill_tool_defaults(preset_id, skill_id, &default_enabled_keys)
        .map_err(AppError::db)?;

    let toggles = store
        .get_scenario_skill_tool_toggles(preset_id, skill_id)
        .map_err(AppError::db)?;
    let enabled_map: std::collections::HashMap<String, bool> = toggles
        .into_iter()
        .map(|toggle| (toggle.tool, toggle.enabled))
        .collect();

    // `enabled` is the preset's intent; `deployed` is the fact on disk as
    // recorded in `skill_targets`. Reporting intent as synced is what made
    // freshly-added preset skills show green checks for every agent (#384).
    let deployed_tools: std::collections::HashSet<String> = store
        .get_targets_for_skill(skill_id)
        .map_err(AppError::db)?
        .into_iter()
        .map(|target| target.tool)
        .collect();

    Ok(all_adapters
        .into_iter()
        .map(|adapter| {
            let globally_enabled = !disabled.contains(&adapter.key);
            let available = adapter.is_installed() && globally_enabled;
            SkillToolToggleDto {
                // Unavailable tools are always presented as disabled in UI.
                enabled: if available {
                    enabled_map.get(&adapter.key).copied().unwrap_or(false)
                } else {
                    false
                },
                deployed: deployed_tools.contains(&adapter.key),
                tool: adapter.key.clone(),
                display_name: adapter.display_name.clone(),
                installed: adapter.is_installed(),
                globally_enabled,
            }
        })
        .collect())
}

#[tauri::command]
pub async fn get_skill_tool_toggles(
    skill_id: String,
    preset_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillToolToggleDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill_ids = store
            .get_skill_ids_for_scenario(&preset_id)
            .map_err(AppError::db)?;
        if !skill_ids.contains(&skill_id) {
            return Err(AppError::not_found("Skill is not enabled in this preset"));
        }

        build_skill_tool_toggles(&store, &preset_id, &skill_id)
    })
    .await?
}

#[tauri::command]
pub async fn set_skill_tool_toggle(
    app: AppHandle,
    skill_id: String,
    preset_id: String,
    tool: String,
    enabled: bool,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let skill_ids = store
            .get_skill_ids_for_scenario(&preset_id)
            .map_err(AppError::db)?;
        if !skill_ids.contains(&skill_id) {
            return Err(AppError::not_found("Skill is not enabled in this preset"));
        }

        let adapter = tool_adapters::find_adapter_with_store(&store, &tool)
            .ok_or_else(|| AppError::not_found(format!("Unknown tool: {}", tool)))?;
        let disabled = disabled_tools(&store);
        let globally_enabled = !disabled.contains(&tool);

        if enabled {
            if !adapter.is_installed() {
                return Err(AppError::not_found(format!(
                    "{} is not installed",
                    adapter.display_name
                )));
            }
            if !globally_enabled {
                return Err(AppError::invalid_input(format!(
                    "{} is disabled",
                    adapter.display_name
                )));
            }
        }

        sync_metadata::with_repo_lock("set skill tool toggle", || {
            store.set_scenario_skill_tool_enabled(&preset_id, &skill_id, &tool, enabled)?;
            sync_metadata::write_all_from_db_unlocked(&store)
        })
        .map_err(AppError::db)?;

        let is_active = store
            .get_active_scenario_id()
            .map_err(AppError::db)?
            .as_deref()
            == Some(preset_id.as_str());
        if is_active {
            if enabled {
                sync_skill_to_tool_internal(&store, &skill_id, &tool)?;
            } else {
                unsync_skill_for_tool_in_preset(&store, &skill_id, &tool)?;
            }
        }

        Ok(())
    })
    .await?;
    if result.is_ok() {
        schedule_tray_refresh(&app);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::central_repo;
    use crate::core::skill_store::{ScenarioRecord, SkillRecord, SkillTargetRecord};
    use crate::core::tool_adapters::CustomToolDef;
    use std::fs;
    use tempfile::tempdir;

    fn sample_skill(id: &str, name: &str, central_path: &std::path::Path) -> SkillRecord {
        SkillRecord {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            source_type: "import".to_string(),
            source_ref: Some(central_path.to_string_lossy().to_string()),
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: central_path.to_string_lossy().to_string(),
            content_hash: None,
            enabled: true,
            created_at: 1,
            updated_at: 1,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: None,
            last_check_error: None,
        }
    }

    fn write_skill_dir(base: &std::path::Path, dir_name: &str, marker: &str) -> PathBuf {
        let dir = base.join(dir_name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {dir_name}\n---\n"),
        )
        .unwrap();
        fs::write(dir.join("unique.txt"), marker).unwrap();
        dir
    }

    fn configure_single_custom_tool(store: &SkillStore, target_base: &std::path::Path) {
        let custom_tools = vec![CustomToolDef {
            key: "test_agent".to_string(),
            display_name: "Test Agent".to_string(),
            skills_dir: target_base.to_string_lossy().to_string(),
            project_relative_skills_dir: None,
            category: Default::default(),
        }];
        store
            .set_setting(
                "custom_tools",
                &serde_json::to_string(&custom_tools).unwrap(),
            )
            .unwrap();
        let disabled_builtin_tools: Vec<String> = tool_adapters::default_tool_adapters()
            .into_iter()
            .map(|adapter| adapter.key)
            .collect();
        store
            .set_setting(
                "disabled_tools",
                &serde_json::to_string(&disabled_builtin_tools).unwrap(),
            )
            .unwrap();
        store.set_setting("sync_mode", "copy").unwrap();
    }

    /// Two agents resolving to one skills directory, which is what makes a
    /// single filesystem object be claimed by several `skill_targets` rows
    /// (the table is unique on `(skill_id, tool)`, not on `target_path`).
    fn configure_two_custom_tools_sharing_a_dir(store: &SkillStore, shared: &std::path::Path) {
        configure_custom_tools(store, &[("agent_a", shared), ("agent_b", shared)]);
    }

    /// Custom agents `(key, skills dir)`, every built-in disabled, copy mode.
    fn configure_custom_tools(store: &SkillStore, tools: &[(&str, &std::path::Path)]) {
        let custom_tools: Vec<CustomToolDef> = tools
            .iter()
            .map(|(key, dir)| CustomToolDef {
                key: key.to_string(),
                display_name: key.to_string(),
                skills_dir: dir.to_string_lossy().to_string(),
                project_relative_skills_dir: None,
                category: Default::default(),
            })
            .collect();
        store
            .set_setting(
                "custom_tools",
                &serde_json::to_string(&custom_tools).unwrap(),
            )
            .unwrap();
        let disabled_builtin_tools: Vec<String> = tool_adapters::default_tool_adapters()
            .into_iter()
            .map(|adapter| adapter.key)
            .collect();
        store
            .set_setting(
                "disabled_tools",
                &serde_json::to_string(&disabled_builtin_tools).unwrap(),
            )
            .unwrap();
        store.set_setting("sync_mode", "copy").unwrap();
    }

    /// The toggle DTO must separate intent (`enabled`, seeded as `1` for
    /// every installed adapter by `ensure_scenario_skill_tool_defaults`)
    /// from fact (`deployed`, a `skill_targets` row). Adding a skill to a
    /// preset is a membership edit only, so the detail sheet used to report
    /// green checks and "synced 6/6" for agents that never received
    /// files (#384).
    #[test]
    fn toggle_dtos_report_deployed_only_with_skill_target_rows() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let agent_dir = tmp.path().join("test-agent-skills");
        fs::create_dir_all(&agent_dir).unwrap();
        configure_single_custom_tool(&store, &agent_dir);

        store
            .insert_skill(&sample_skill(
                "s1",
                "demo-skill",
                &tmp.path().join("central"),
            ))
            .unwrap();
        store
            .insert_scenario(&ScenarioRecord {
                id: "sc1".to_string(),
                name: "Preset".to_string(),
                description: None,
                icon: None,
                sort_order: 0,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
        store.add_skill_to_scenario("sc1", "s1").unwrap();

        // Defaults seeded enabled=1 for the one available agent, but nothing
        // has been synced to disk yet.
        let toggles = build_skill_tool_toggles(&store, "sc1", "s1").unwrap();
        let toggle = toggles.iter().find(|t| t.tool == "test_agent").unwrap();
        assert!(toggle.enabled);
        assert!(
            !toggle.deployed,
            "preset membership alone must not read as deployed (#384)"
        );

        // After a real sync writes the `skill_targets` row, it reads deployed.
        store
            .insert_target(&SkillTargetRecord {
                id: "t1".to_string(),
                skill_id: "s1".to_string(),
                tool: "test_agent".to_string(),
                target_path: agent_dir.join("demo-skill").to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: None,
            })
            .unwrap();
        let toggles = build_skill_tool_toggles(&store, "sc1", "s1").unwrap();
        let toggle = toggles.iter().find(|t| t.tool == "test_agent").unwrap();
        assert!(toggle.enabled);
        assert!(toggle.deployed);
    }

    /// Deploying to two agents that share a skills directory must succeed. The
    /// second pair has no row of its own when the batch starts, so without
    /// batch-level evidence it would refuse the directory the first pair just
    /// wrote (#363 review, round 2).
    #[test]
    fn shared_skills_dir_deploys_to_both_agents_in_one_batch() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let source_base = tmp.path().join("central");
        let shared = tmp.path().join("shared-agent-skills");
        fs::create_dir_all(&source_base).unwrap();
        fs::create_dir_all(&shared).unwrap();
        configure_two_custom_tools_sharing_a_dir(&store, &shared);

        let dir = write_skill_dir(&source_base, "shared-skill", "content");
        store
            .insert_skill(&sample_skill("s1", "shared-skill", &dir))
            .unwrap();

        scenario_service::apply_skills_to_tools(
            &store,
            &["s1".to_string()],
            &["agent_a".to_string(), "agent_b".to_string()],
            scenario_service::BatchApplyMode::Add,
        )
        .expect("a shared target directory must not make the second agent refuse");

        assert_eq!(
            fs::read_to_string(shared.join("shared-skill/unique.txt")).unwrap(),
            "content"
        );
        let rows = store.get_targets_for_skill("s1").unwrap();
        assert_eq!(rows.len(), 2, "both agents should be recorded: {rows:?}");
    }

    /// Contradictory rows for one path are ambiguous evidence, and a fix whose
    /// purpose is preservation must refuse rather than guess. Regression for the
    /// hole that survived three review rounds: evidence has to be pooled from
    /// every row on the path, including rows this batch did not select.
    #[test]
    fn contradictory_rows_on_a_shared_path_refuse_to_replace_user_content() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let source_base = tmp.path().join("central");
        let shared = tmp.path().join("shared-agent-skills");
        fs::create_dir_all(&source_base).unwrap();
        fs::create_dir_all(&shared).unwrap();
        configure_two_custom_tools_sharing_a_dir(&store, &shared);

        let dir = write_skill_dir(&source_base, "shared-skill", "content");
        store
            .insert_skill(&sample_skill("s1", "shared-skill", &dir))
            .unwrap();

        // A real directory of the user's now sits at the shared target.
        let target = shared.join("shared-skill");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("mine.txt"), "DO_NOT_OVERWRITE").unwrap();

        // Two rows disagree about what we put there. Only agent_a is selected
        // below, so the contradicting agent_b row is exactly the "unselected
        // row" that pooling from the batch alone would have missed.
        for (tool, mode) in [("agent_a", "copy"), ("agent_b", "symlink")] {
            store
                .insert_target(&crate::core::skill_store::SkillTargetRecord {
                    id: format!("t-{tool}"),
                    skill_id: "s1".to_string(),
                    tool: tool.to_string(),
                    target_path: target.to_string_lossy().to_string(),
                    mode: mode.to_string(),
                    status: "ok".to_string(),
                    synced_at: Some(1),
                    last_error: None,
                    source_hash: Some("h1".to_string()),
                })
                .unwrap();
        }

        let result = scenario_service::apply_skills_to_tools(
            &store,
            &["s1".to_string()],
            &["agent_a".to_string()],
            scenario_service::BatchApplyMode::Add,
        );

        assert!(
            result.is_err(),
            "contradictory records must refuse, not guess a mode"
        );
        assert_eq!(
            fs::read_to_string(target.join("mine.txt")).unwrap(),
            "DO_NOT_OVERWRITE"
        );
    }

    /// #435: unchecking a skill for one tool in the active preset used to
    /// delete whatever sat at the recorded path. A real directory that
    /// replaced our symlink is the user's — preserve it and drop only the
    /// record.
    #[test]
    fn preset_unsync_preserves_user_content_that_replaced_a_recorded_link() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let target = tmp.path().join("agent-skills").join("my-skill");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("mine.txt"), "DO_NOT_OVERWRITE").unwrap();
        store
            .insert_skill(&sample_skill("s1", "my-skill", &tmp.path().join("central")))
            .unwrap();
        store
            .insert_target(&crate::core::skill_store::SkillTargetRecord {
                id: "t1".to_string(),
                skill_id: "s1".to_string(),
                tool: "agent_a".to_string(),
                target_path: target.to_string_lossy().to_string(),
                mode: "symlink".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: None,
            })
            .unwrap();

        unsync_skill_for_tool_in_preset(&store, "s1", "agent_a").unwrap();

        assert_eq!(
            fs::read_to_string(target.join("mine.txt")).unwrap(),
            "DO_NOT_OVERWRITE",
            "the user's directory must survive the uncheck"
        );
        assert!(store.get_targets_for_skill("s1").unwrap().is_empty());
    }

    /// Unchecking one tool must not delete a deployment another tool shares.
    #[test]
    fn preset_unsync_keeps_a_deployment_another_tool_shares() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let target = tmp.path().join("agent-skills").join("my-skill");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "---\nname: my-skill\n---\n").unwrap();
        store
            .insert_skill(&sample_skill("s1", "my-skill", &tmp.path().join("central")))
            .unwrap();
        for (id, tool) in [("t1", "agent_a"), ("t2", "agent_b")] {
            store
                .insert_target(&crate::core::skill_store::SkillTargetRecord {
                    id: id.to_string(),
                    skill_id: "s1".to_string(),
                    tool: tool.to_string(),
                    target_path: target.to_string_lossy().to_string(),
                    mode: "copy".to_string(),
                    status: "ok".to_string(),
                    synced_at: Some(1),
                    last_error: None,
                    source_hash: None,
                })
                .unwrap();
        }

        unsync_skill_for_tool_in_preset(&store, "s1", "agent_a").unwrap();

        assert!(target.exists(), "agent_b still deploys this path");
        let remaining = store.get_targets_for_skill("s1").unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].tool, "agent_b");
    }

    struct ActivePresetRepo {
        _lock: std::sync::MutexGuard<'static, ()>,
        _tmp: tempfile::TempDir,
        store: SkillStore,
        deployed: PathBuf,
    }

    impl Drop for ActivePresetRepo {
        fn drop(&mut self) {
            central_repo::set_test_base_dir_override(None);
        }
    }

    /// One skill in the active preset, deployed to `test_agent` by the startup
    /// sync, with the sync metadata written: the state of a running app.
    fn deployed_active_preset_repo() -> ActivePresetRepo {
        let lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();
        let target_base = tmp.path().join("agent-skills");
        fs::create_dir_all(&target_base).unwrap();
        configure_single_custom_tool(&store, &target_base);

        let dir = write_skill_dir(&central_repo::skills_dir(), "my-skill", "content");
        store
            .insert_skill(&sample_skill("s1", "my-skill", &dir))
            .unwrap();
        store
            .insert_scenario(&ScenarioRecord {
                id: "active".to_string(),
                name: "Active".to_string(),
                description: None,
                icon: None,
                sort_order: 0,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
        store.add_skill_to_scenario("active", "s1").unwrap();
        store.set_active_scenario("active").unwrap();
        scenario_service::ensure_default_startup_scenario(&store).unwrap();
        sync_metadata::write_all_from_db(&store).unwrap();

        let deployed = target_base.join("my-skill");
        assert!(deployed.exists());
        ActivePresetRepo {
            _lock: lock,
            _tmp: tmp,
            store,
            deployed,
        }
    }

    /// What the next launch does before the window opens.
    fn restart(store: &SkillStore) {
        sync_metadata::reindex_from_metadata(store).unwrap();
        scenario_service::ensure_default_startup_scenario(store).unwrap();
    }

    /// #549: removing an active-preset skill from one agent in the workspace
    /// view turned its toggle off in the database only. The next launch
    /// reindexed the old toggle from the sync metadata, and the startup sync
    /// deployed the skill again.
    #[test]
    fn removed_active_preset_skill_stays_removed_after_restart() {
        let repo = deployed_active_preset_repo();

        unsync_skill_from_tool_internal(&repo.store, "s1", "test_agent").unwrap();
        assert!(!repo.deployed.exists());

        restart(&repo.store);

        assert!(
            !repo.deployed.exists(),
            "the removed skill was deployed again on restart"
        );
    }

    /// The same gap in the other direction: adding it back was forgotten on
    /// restart, leaving the agent's toggle off while the skill is deployed.
    #[test]
    fn readded_active_preset_skill_stays_enabled_after_restart() {
        let repo = deployed_active_preset_repo();
        // Off and persisted, the way the skill detail panel's toggle does it.
        repo.store
            .set_scenario_skill_tool_enabled("active", "s1", "test_agent", false)
            .unwrap();
        sync_metadata::write_all_from_db(&repo.store).unwrap();
        unsync_skill_for_tool_in_preset(&repo.store, "s1", "test_agent").unwrap();

        sync_skill_to_tool_internal(&repo.store, "s1", "test_agent").unwrap();
        set_active_preset_tool_toggle(&repo.store, "s1", "test_agent", true).unwrap();
        restart(&repo.store);

        assert_eq!(
            repo.store
                .get_enabled_tools_for_scenario_skill("active", "s1")
                .unwrap(),
            vec!["test_agent".to_string()]
        );
    }

    /// The tray's remove action and the CLI's `presets undeploy` / `skills
    /// undeploy` go through the batch remove, which left the active preset's
    /// toggles on: the next launch deployed the skill again (#549).
    #[test]
    fn batch_removed_active_preset_skill_stays_removed_after_restart() {
        let repo = deployed_active_preset_repo();

        scenario_service::apply_skills_to_tools(
            &repo.store,
            &["s1".to_string()],
            &["test_agent".to_string()],
            scenario_service::BatchApplyMode::Remove,
        )
        .unwrap();
        assert!(!repo.deployed.exists());

        restart(&repo.store);

        assert!(
            !repo.deployed.exists(),
            "the removed skill was deployed again on restart"
        );
    }

    /// Removing from an agent the skill was never deployed to (the tray
    /// removes from every coding agent) must also keep the next launch from
    /// deploying it there.
    #[test]
    fn batch_remove_from_an_undeployed_agent_keeps_it_off_after_restart() {
        let repo = deployed_active_preset_repo();
        repo.store.delete_target("s1", "test_agent").unwrap();
        fs::remove_dir_all(&repo.deployed).unwrap();

        scenario_service::apply_skills_to_tools(
            &repo.store,
            &["s1".to_string()],
            &["test_agent".to_string()],
            scenario_service::BatchApplyMode::Remove,
        )
        .unwrap();
        restart(&repo.store);

        assert!(!repo.deployed.exists());
    }

    /// Deploying it again must turn the toggle back on, or the preview and the
    /// startup sync keep ignoring a skill that is deployed.
    #[test]
    fn batch_redeployed_active_preset_skill_is_back_in_the_preset_after_restart() {
        let repo = deployed_active_preset_repo();
        let skill = ["s1".to_string()];
        let agent = ["test_agent".to_string()];
        scenario_service::apply_skills_to_tools(
            &repo.store,
            &skill,
            &agent,
            scenario_service::BatchApplyMode::Remove,
        )
        .unwrap();

        scenario_service::apply_skills_to_tools(
            &repo.store,
            &skill,
            &agent,
            scenario_service::BatchApplyMode::Add,
        )
        .unwrap();
        restart(&repo.store);

        assert!(repo.deployed.exists());
        let preview = scenario_service::preview_scenario_sync(&repo.store, "active").unwrap();
        assert!(
            preview
                .iter()
                .any(|target| target.skill_id == "s1" && target.tool == "test_agent"),
            "the redeployed skill is missing from the preset: {preview:?}"
        );
    }

    #[test]
    fn sync_skill_to_tool_keeps_duplicate_skill_names_separate() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let source_base = tmp.path().join("central");
        let target_base = tmp.path().join("agent-skills");
        fs::create_dir_all(&source_base).unwrap();
        fs::create_dir_all(&target_base).unwrap();
        configure_single_custom_tool(&store, &target_base);

        let first_dir = write_skill_dir(&source_base, "skill123", "first");
        let second_dir = write_skill_dir(&source_base, "skill123-2", "second");
        store
            .insert_skill(&sample_skill("first", "skill123", &first_dir))
            .unwrap();
        store
            .insert_skill(&sample_skill("second", "skill123", &second_dir))
            .unwrap();

        sync_skill_to_tool_internal(&store, "first", "test_agent").unwrap();
        sync_skill_to_tool_internal(&store, "second", "test_agent").unwrap();

        assert_eq!(
            fs::read_to_string(target_base.join("skill123/unique.txt")).unwrap(),
            "first"
        );
        assert_eq!(
            fs::read_to_string(target_base.join("skill123-2/unique.txt")).unwrap(),
            "second"
        );
    }

    /// Two agents on one skills directory, copy mode, and skill `s1` whose
    /// deployment directory is `shared/demo`.
    fn shared_dir_with_one_skill() -> (tempfile::TempDir, SkillStore, PathBuf) {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let shared = tmp.path().join("shared");
        fs::create_dir_all(&shared).unwrap();
        configure_two_custom_tools_sharing_a_dir(&store, &shared);
        let dir = write_skill_dir(&tmp.path().join("central"), "demo", "library");
        store
            .insert_skill(&sample_skill("s1", "demo", &dir))
            .unwrap();
        (tmp, store, shared.join("demo"))
    }

    fn recorded_tools(store: &SkillStore, skill_id: &str) -> Vec<String> {
        let mut tools: Vec<String> = store
            .get_targets_for_skill(skill_id)
            .unwrap()
            .into_iter()
            .map(|row| row.tool)
            .collect();
        tools.sort();
        tools
    }

    /// Cluster 37 A5: deploying one agent at a time to two agents that share
    /// a skills directory refused the second one in copy mode — the
    /// directory the first one wrote has no row of the second agent's. The
    /// skill detail sheet then showed "pending sync" for it forever.
    #[test]
    fn shared_skills_dir_deploys_to_both_agents_one_at_a_time() {
        let (_tmp, store, target) = shared_dir_with_one_skill();

        sync_skill_to_tool_internal(&store, "s1", "agent_a").unwrap();
        sync_skill_to_tool_internal(&store, "s1", "agent_b")
            .expect("the directory agent_a's deployment wrote must not be refused");

        assert_eq!(recorded_tools(&store, "s1"), ["agent_a", "agent_b"]);
        for row in store.get_targets_for_skill("s1").unwrap() {
            assert_eq!(PathBuf::from(&row.target_path), target);
            assert_eq!(row.mode, "copy");
        }
        assert_eq!(
            fs::read_to_string(target.join("unique.txt")).unwrap(),
            "library"
        );
    }

    fn add_active_preset(store: &SkillStore, skill_ids: &[&str]) {
        store
            .insert_scenario(&ScenarioRecord {
                id: "p".to_string(),
                name: "P".to_string(),
                description: None,
                icon: None,
                sort_order: 0,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
        for skill_id in skill_ids {
            store.add_skill_to_scenario("p", skill_id).unwrap();
        }
        store.set_active_scenario("p").unwrap();
    }

    /// The preset paths deploy pair by pair too: switching to a preset, the
    /// startup sync, and adding a skill to the active preset.
    #[test]
    fn shared_skills_dir_preset_sync_deploys_to_both_agents() {
        for active_only in [false, true] {
            let (_tmp, store, target) = shared_dir_with_one_skill();
            add_active_preset(&store, &["s1"]);

            if active_only {
                scenario_service::sync_skill_to_active_scenario(&store, "p", "s1").unwrap();
            } else {
                let refusals = scenario_service::sync_scenario_skills(&store, "p").unwrap();
                assert!(refusals.is_empty(), "{refusals:?}");
            }

            assert_eq!(
                recorded_tools(&store, "s1"),
                ["agent_a", "agent_b"],
                "active_only={active_only}"
            );
            assert!(target.join("unique.txt").exists());
        }

        // `presets sync --tool agent_b` (and its `--dry-run` preflight) after
        // agent_a alone was deployed.
        let (_tmp, store, _target) = shared_dir_with_one_skill();
        add_active_preset(&store, &["s1"]);
        sync_skill_to_tool_internal(&store, "s1", "agent_a").unwrap();
        let only_b: Vec<_> = scenario_service::collect_scenario_sync_targets(&store, "p")
            .unwrap()
            .into_iter()
            .filter(|target| target.tool == "agent_b")
            .collect();
        scenario_service::preflight_scenario_sync_targets(&store, &only_b)
            .expect("the dry run must agree with the sync");
        let refusals = scenario_service::sync_desired_targets(&store, &only_b).unwrap();
        assert!(refusals.is_empty(), "{refusals:?}");
        assert_eq!(recorded_tools(&store, "s1"), ["agent_a", "agent_b"]);
    }

    /// Another agent's row vouches for the directory only when it records
    /// this skill at this path. Content we have no record of, or another
    /// skill's deployment on the same path, is still refused and left alone.
    #[test]
    fn shared_skills_dir_still_refuses_content_that_is_not_this_skills_deployment() {
        // Nothing recorded there at all: the user's own directory.
        let (_tmp, store, target) = shared_dir_with_one_skill();
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("mine.txt"), "USER").unwrap();
        assert!(sync_skill_to_tool_internal(&store, "s1", "agent_b").is_err());
        assert_eq!(fs::read_to_string(target.join("mine.txt")).unwrap(), "USER");
        assert!(recorded_tools(&store, "s1").is_empty());

        // Another library skill deployed by agent_a to the same path (its
        // central directory has the same basename).
        let (tmp, store, target) = shared_dir_with_one_skill();
        let other = write_skill_dir(&tmp.path().join("central-2"), "demo", "other skill");
        store
            .insert_skill(&sample_skill("s2", "demo", &other))
            .unwrap();
        sync_skill_to_tool_internal(&store, "s2", "agent_a").unwrap();
        assert!(
            sync_skill_to_tool_internal(&store, "s1", "agent_b").is_err(),
            "s2's row must not vouch for deploying s1 over it"
        );
        // The batch accepts any agreeing row of another agent, so only the
        // same-skill requirement stands between s2's deployment and s1.
        assert!(
            scenario_service::apply_skills_to_tools(
                &store,
                &["s1".to_string()],
                &["agent_b".to_string()],
                scenario_service::BatchApplyMode::Add,
            )
            .is_err(),
            "s2's row must not vouch for a batch deploy of s1 either"
        );
        assert_eq!(
            fs::read_to_string(target.join("unique.txt")).unwrap(),
            "other skill"
        );
        assert!(recorded_tools(&store, "s1").is_empty());
        assert_eq!(recorded_tools(&store, "s2"), ["agent_a"]);

        // The same two skills in one preset sync: whichever deploys first owns
        // the path for the rest of the run, the other is refused.
        let (tmp, store, target) = shared_dir_with_one_skill();
        let other = write_skill_dir(&tmp.path().join("central-2"), "demo", "other skill");
        store
            .insert_skill(&sample_skill("s2", "demo", &other))
            .unwrap();
        add_active_preset(&store, &["s1", "s2"]);
        let refusals = scenario_service::sync_scenario_skills(&store, "p").unwrap();
        assert!(
            !refusals.is_empty(),
            "one of the two skills must be refused"
        );
        let s1_tools = recorded_tools(&store, "s1");
        let s2_tools = recorded_tools(&store, "s2");
        let (winner, loser) = if s1_tools.is_empty() {
            (s2_tools, s1_tools)
        } else {
            (s1_tools, s2_tools)
        };
        assert_eq!(winner, ["agent_a", "agent_b"]);
        assert!(loser.is_empty(), "the refused skill must have no rows");
        let on_disk = fs::read_to_string(target.join("unique.txt")).unwrap();
        let expected = if recorded_tools(&store, "s1").is_empty() {
            "other skill"
        } else {
            "library"
        };
        assert_eq!(
            on_disk, expected,
            "the directory must hold the recorded skill"
        );
    }

    /// Removing one agent of a shared directory keeps the directory and the
    /// other agent's record; removing the last one deletes the directory.
    /// Every removal path: the workspace/list toggle, the detail-sheet toggle
    /// of the active preset, and the batch remove behind the tray and CLI.
    #[test]
    fn shared_skills_dir_removal_keeps_the_other_agents_deployment() {
        type Remove = fn(&SkillStore, &str, &str) -> Result<(), AppError>;
        let removals: [(&str, Remove); 3] = [
            ("unsync_skill_from_tool", unsync_skill_from_tool_internal),
            ("preset toggle off", unsync_skill_for_tool_in_preset),
            ("batch remove", |store, skill_id, tool| {
                scenario_service::apply_skills_to_tools(
                    store,
                    &[skill_id.to_string()],
                    &[tool.to_string()],
                    scenario_service::BatchApplyMode::Remove,
                )
            }),
        ];
        for (name, remove) in removals {
            let (_tmp, store, target) = shared_dir_with_one_skill();
            sync_skill_to_tool_internal(&store, "s1", "agent_a").unwrap();
            sync_skill_to_tool_internal(&store, "s1", "agent_b").unwrap();

            remove(&store, "s1", "agent_a").unwrap();
            assert_eq!(recorded_tools(&store, "s1"), ["agent_b"], "{name}");
            assert_eq!(
                fs::read_to_string(target.join("unique.txt")).unwrap(),
                "library",
                "{name}: agent_b's deployment must survive"
            );

            remove(&store, "s1", "agent_b").unwrap();
            assert!(recorded_tools(&store, "s1").is_empty(), "{name}");
            assert!(
                fs::symlink_metadata(&target).is_err(),
                "{name}: the last agent's removal must delete the directory"
            );
        }
    }

    /// `.DS_Store` is left out of the content hash, so it does not stop an
    /// adoption, and a rewrite of the directory would drop it.
    fn plant_sentinel(target: &std::path::Path) {
        fs::write(target.join(".DS_Store"), "sentinel").unwrap();
    }

    fn sentinel_survived(target: &std::path::Path) -> bool {
        target.join(".DS_Store").exists()
    }

    /// H review item 1. Agent A's copy is current, so A's own skip check
    /// keeps the edits the user made in it. Agent B shares the directory and
    /// has no row. Borrowing A's row must not let the unattended startup sync
    /// (or a single-agent deploy) delete those edits; on `main` B refused.
    #[test]
    fn borrowed_row_never_overwrites_an_edited_shared_copy() {
        for per_pair in [false, true] {
            let (_tmp, store, target) = shared_dir_with_one_skill();
            let mut skill = store.get_skill_by_id("s1").unwrap().unwrap();
            skill.content_hash = Some("h1".to_string());
            store.upsert_skill(&skill).unwrap();
            add_active_preset(&store, &["s1"]);
            sync_skill_to_tool_internal(&store, "s1", "agent_a").unwrap();
            fs::write(target.join("unique.txt"), "USER EDIT").unwrap();

            if per_pair {
                assert!(sync_skill_to_tool_internal(&store, "s1", "agent_b").is_err());
            } else {
                let refusals = scenario_service::sync_scenario_skills(&store, "p").unwrap();
                assert_eq!(refusals.len(), 1, "B must be refused: {refusals:?}");
            }

            assert_eq!(
                fs::read_to_string(target.join("unique.txt")).unwrap(),
                "USER EDIT",
                "per_pair={per_pair}"
            );
            assert_eq!(
                recorded_tools(&store, "s1"),
                ["agent_a"],
                "per_pair={per_pair}"
            );
        }
    }

    /// When the shared copy is exactly the library's content, the agent
    /// without a row adopts it: a row is added, the directory is not rebuilt.
    #[test]
    fn borrowed_row_adopts_an_identical_shared_copy_without_rewriting_it() {
        type Deploy = fn(&SkillStore) -> Result<(), AppError>;
        let deploys: [(&str, Deploy); 3] = [
            ("single agent", |store| {
                sync_skill_to_tool_internal(store, "s1", "agent_b")
            }),
            ("preset sync", |store| {
                let refusals = scenario_service::sync_scenario_skills(store, "p")?;
                assert!(refusals.is_empty(), "{refusals:?}");
                Ok(())
            }),
            ("active preset add", |store| {
                scenario_service::sync_skill_to_active_scenario(store, "p", "s1")
            }),
        ];
        for (name, deploy) in deploys {
            let (_tmp, store, target) = shared_dir_with_one_skill();
            // Recorded hash = library hash, so A's own copy is current and the
            // preset sync skips it: only B's step can touch the directory.
            let mut skill = store.get_skill_by_id("s1").unwrap().unwrap();
            skill.content_hash = Some("h1".to_string());
            store.upsert_skill(&skill).unwrap();
            add_active_preset(&store, &["s1"]);
            sync_skill_to_tool_internal(&store, "s1", "agent_a").unwrap();
            plant_sentinel(&target);

            deploy(&store).unwrap();

            assert_eq!(
                recorded_tools(&store, "s1"),
                ["agent_a", "agent_b"],
                "{name}"
            );
            let b = store
                .get_targets_for_skill("s1")
                .unwrap()
                .into_iter()
                .find(|row| row.tool == "agent_b")
                .unwrap();
            assert_eq!(b.mode, "copy", "{name}");
            assert_eq!(PathBuf::from(&b.target_path), target, "{name}");
            assert!(
                sentinel_survived(&target),
                "{name}: the directory was rewritten"
            );
        }
    }

    /// A copy that fell behind the library (not edited by the user) is
    /// refreshed by its own agent and then adopted by the other, whichever
    /// comes first in the startup sync. Order A→B settles in one run; B→A
    /// refuses B once and adopts on the next run.
    #[test]
    fn a_stale_shared_copy_converges_in_either_order() {
        for b_first in [false, true] {
            let (_tmp, store, target) = shared_dir_with_one_skill();
            let mut skill = store.get_skill_by_id("s1").unwrap().unwrap();
            skill.content_hash = Some("h1".to_string());
            store.upsert_skill(&skill).unwrap();
            add_active_preset(&store, &["s1"]);
            sync_skill_to_tool_internal(&store, "s1", "agent_a").unwrap();
            // The library moves on; A's copy is now stale but unedited.
            fs::write(PathBuf::from(&skill.central_path).join("unique.txt"), "v2").unwrap();
            skill.content_hash = Some("h2".to_string());
            store.upsert_skill(&skill).unwrap();

            let run = || {
                let mut desired =
                    scenario_service::collect_scenario_sync_targets(&store, "p").unwrap();
                desired.sort_by(|x, y| x.tool.cmp(&y.tool));
                if b_first {
                    desired.reverse();
                }
                scenario_service::sync_desired_targets(&store, &desired).unwrap()
            };
            let first = run();
            if b_first {
                assert_eq!(first.len(), 1, "B refuses the stale copy once");
                assert_eq!(recorded_tools(&store, "s1"), ["agent_a"]);
                assert!(run().is_empty(), "the next run adopts");
            } else {
                assert!(first.is_empty(), "{first:?}");
            }
            assert_eq!(
                recorded_tools(&store, "s1"),
                ["agent_a", "agent_b"],
                "b_first={b_first}"
            );
            assert_eq!(fs::read_to_string(target.join("unique.txt")).unwrap(), "v2");
        }
    }

    /// Contradictory rows for this skill (one agent recorded copy, another
    /// symlink) vouch for nothing. A third agent sharing the directory must
    /// not replace the real directory there — neither in the batch, where
    /// any agreeing row would authorize it, nor by adopting it.
    #[test]
    fn contradictory_rows_refuse_a_third_agent_on_a_shared_dir() {
        for content in ["library", "USER"] {
            let tmp = tempdir().unwrap();
            let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
            let shared = tmp.path().join("shared");
            fs::create_dir_all(&shared).unwrap();
            configure_custom_tools(
                &store,
                &[
                    ("agent_a", &shared),
                    ("agent_b", &shared),
                    ("agent_c", &shared),
                ],
            );
            let dir = write_skill_dir(&tmp.path().join("central"), "demo", "library");
            store
                .insert_skill(&sample_skill("s1", "demo", &dir))
                .unwrap();
            let target = write_skill_dir(&shared, "demo", content);
            plant_sentinel(&target);
            for (tool, mode) in [("agent_a", "copy"), ("agent_b", "symlink")] {
                store
                    .insert_target(&SkillTargetRecord {
                        id: format!("t-{tool}"),
                        skill_id: "s1".to_string(),
                        tool: tool.to_string(),
                        target_path: target.to_string_lossy().to_string(),
                        mode: mode.to_string(),
                        status: "ok".to_string(),
                        synced_at: Some(1),
                        last_error: None,
                        source_hash: None,
                    })
                    .unwrap();
            }

            assert!(
                sync_skill_to_tool_internal(&store, "s1", "agent_c").is_err(),
                "content={content}: single agent"
            );
            assert!(
                scenario_service::apply_skills_to_tools(
                    &store,
                    &["s1".to_string()],
                    &["agent_c".to_string()],
                    scenario_service::BatchApplyMode::Add,
                )
                .is_err(),
                "content={content}: batch"
            );
            assert!(sentinel_survived(&target), "content={content}");
            assert_eq!(
                fs::read_to_string(target.join("unique.txt")).unwrap(),
                content
            );
            assert_eq!(recorded_tools(&store, "s1"), ["agent_a", "agent_b"]);
        }
    }

    /// The dry run must refuse what the real run refuses: two skills whose
    /// directories share a name, aimed at one path that does not exist yet.
    /// The disk shows nothing in the way; the real run writes the first one
    /// and then refuses the second.
    #[test]
    fn preset_preflight_refuses_what_the_run_will_refuse() {
        let (tmp, store, _target) = shared_dir_with_one_skill();
        let other = write_skill_dir(&tmp.path().join("central-2"), "demo", "other skill");
        store
            .insert_skill(&sample_skill("s2", "demo", &other))
            .unwrap();
        add_active_preset(&store, &["s1", "s2"]);
        let desired = scenario_service::collect_scenario_sync_targets(&store, "p").unwrap();

        let preflight = scenario_service::preflight_scenario_sync_targets(&store, &desired);
        let refusals = scenario_service::sync_desired_targets(&store, &desired).unwrap();

        assert!(!refusals.is_empty());
        assert!(
            preflight.is_err(),
            "the dry run passed a sync that refused {refusals:?}"
        );
    }

    /// H review item 2. Deployment treats `/alias/skills/demo` and
    /// `/real/skills/demo` as one directory when `/alias` links to `/real`, so
    /// removal must too: removing A must not delete what B still uses.
    #[cfg(unix)]
    #[test]
    fn removal_through_a_symlinked_parent_keeps_the_other_agents_directory() {
        type Remove = fn(&SkillStore, &str, &str) -> Result<(), AppError>;
        let removals: [(&str, Remove); 3] = [
            ("unsync_skill_from_tool", unsync_skill_from_tool_internal),
            ("preset toggle off", unsync_skill_for_tool_in_preset),
            ("batch remove", |store, skill_id, tool| {
                scenario_service::apply_skills_to_tools(
                    store,
                    &[skill_id.to_string()],
                    &[tool.to_string()],
                    scenario_service::BatchApplyMode::Remove,
                )
            }),
        ];
        for (name, remove) in removals {
            let tmp = tempdir().unwrap();
            let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
            let real = tmp.path().join("real").join("skills");
            fs::create_dir_all(&real).unwrap();
            std::os::unix::fs::symlink(tmp.path().join("real"), tmp.path().join("alias")).unwrap();
            let alias = tmp.path().join("alias").join("skills");
            configure_custom_tools(&store, &[("agent_a", &real), ("agent_b", &alias)]);
            let dir = write_skill_dir(&tmp.path().join("central"), "demo", "library");
            store
                .insert_skill(&sample_skill("s1", "demo", &dir))
                .unwrap();
            sync_skill_to_tool_internal(&store, "s1", "agent_a").unwrap();
            sync_skill_to_tool_internal(&store, "s1", "agent_b").unwrap();
            assert_eq!(
                recorded_tools(&store, "s1"),
                ["agent_a", "agent_b"],
                "{name}"
            );

            remove(&store, "s1", "agent_a").unwrap();
            assert!(
                real.join("demo/unique.txt").exists(),
                "{name}: agent_b's directory was deleted"
            );
            assert_eq!(recorded_tools(&store, "s1"), ["agent_b"], "{name}");

            remove(&store, "s1", "agent_b").unwrap();
            assert!(fs::symlink_metadata(real.join("demo")).is_err(), "{name}");
        }
    }
}
