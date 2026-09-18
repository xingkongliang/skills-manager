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

#[tauri::command]
pub async fn sync_skill_to_tool(
    app: AppHandle,
    skill_id: String,
    tool: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let outcome = (|| -> Result<(), AppError> {
            sync_skill_to_tool_internal(&store, &skill_id, &tool)?;

            if let Ok(Some(active_id)) = store.get_active_scenario_id() {
                let skill_ids = store
                    .get_skill_ids_for_scenario(&active_id)
                    .map_err(AppError::db)?;
                if skill_ids.contains(&skill_id) {
                    let adapter_keys: Vec<String> =
                        tool_adapters::enabled_installed_adapters(&store)
                            .iter()
                            .map(|a| a.key.clone())
                            .collect();
                    store
                        .ensure_scenario_skill_tool_defaults(&active_id, &skill_id, &adapter_keys)
                        .map_err(AppError::db)?;
                    store
                        .set_scenario_skill_tool_enabled(&active_id, &skill_id, &tool, true)
                        .map_err(AppError::db)?;
                }
            }

            Ok(())
        })();
        log_sync_outcome(&store, "enable", &skill_id, &tool, outcome.as_ref());
        outcome
    })
    .await?;
    if result.is_ok() {
        schedule_tray_refresh(&app);
    }
    result
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
        let outcome = (|| -> Result<(), AppError> {
            let targets = store
                .get_targets_for_skill(&skill_id)
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

            store
                .delete_target(&skill_id, &tool)
                .map_err(AppError::db)?;

            if let Ok(Some(active_id)) = store.get_active_scenario_id() {
                let skill_ids = store
                    .get_skill_ids_for_scenario(&active_id)
                    .map_err(AppError::db)?;
                if skill_ids.contains(&skill_id) {
                    let adapter_keys: Vec<String> =
                        tool_adapters::enabled_installed_adapters(&store)
                            .iter()
                            .map(|a| a.key.clone())
                            .collect();
                    store
                        .ensure_scenario_skill_tool_defaults(&active_id, &skill_id, &adapter_keys)
                        .map_err(AppError::db)?;
                    store
                        .set_scenario_skill_tool_enabled(&active_id, &skill_id, &tool, false)
                        .map_err(AppError::db)?;
                }
            }

            Ok(())
        })();
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
                let targets = store
                    .get_targets_for_skill(&skill_id)
                    .map_err(AppError::db)?;
                if let Some(target) = targets.iter().find(|target| target.tool == tool) {
                    // Safe because the app currently guarantees a single active scenario.
                    sync_engine::remove_target(&PathBuf::from(&target.target_path)).ok();
                }
                store
                    .delete_target(&skill_id, &tool)
                    .map_err(AppError::db)?;
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
    use crate::core::skill_store::SkillRecord;
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
