use std::path::PathBuf;
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
        // sharing this skills directory keeps its deployment.
        let still_referenced = targets
            .iter()
            .any(|other| other.tool != tool && other.target_path == target.target_path);
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
        let still_referenced = store
            .get_all_targets()
            .map_err(AppError::db)?
            .into_iter()
            .any(|other| {
                other.target_path == target.target_path
                    && !(other.skill_id == skill_id && other.tool == tool)
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

        let disabled = disabled_tools(&store);
        let all_adapters = tool_adapters::all_tool_adapters(&store);
        let default_enabled_keys: Vec<String> = all_adapters
            .iter()
            .filter(|adapter| adapter.is_installed() && !disabled.contains(&adapter.key))
            .map(|adapter| adapter.key.clone())
            .collect();
        store
            .ensure_scenario_skill_tool_defaults(&preset_id, &skill_id, &default_enabled_keys)
            .map_err(AppError::db)?;

        let toggles = store
            .get_scenario_skill_tool_toggles(&preset_id, &skill_id)
            .map_err(AppError::db)?;
        let enabled_map: std::collections::HashMap<String, bool> = toggles
            .into_iter()
            .map(|toggle| (toggle.tool, toggle.enabled))
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
                    tool: adapter.key.clone(),
                    display_name: adapter.display_name.clone(),
                    installed: adapter.is_installed(),
                    globally_enabled,
                }
            })
            .collect())
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
    use crate::core::skill_store::{ScenarioRecord, SkillRecord};
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
            icon: None,
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
        let custom_tools: Vec<CustomToolDef> = ["agent_a", "agent_b"]
            .into_iter()
            .map(|key| CustomToolDef {
                key: key.to_string(),
                display_name: key.to_string(),
                skills_dir: shared.to_string_lossy().to_string(),
                project_relative_skills_dir: None,
                category: Default::default(),
                icon: None,
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
}
