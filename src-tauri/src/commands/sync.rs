use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

use crate::core::{
    error::AppError,
    skill_store::{SkillStore, SkillTargetRecord},
    sync_engine, sync_metadata, tool_adapters,
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
    store
        .get_setting("disabled_tools")
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default()
}

fn sync_skill_to_tool_internal(
    store: &SkillStore,
    skill_id: &str,
    tool: &str,
) -> Result<(), AppError> {
    let adapter = tool_adapters::find_adapter_with_store(store, tool)
        .ok_or_else(|| AppError::not_found(format!("Unknown tool: {}", tool)))?;

    if !adapter.is_installed() {
        return Err(AppError::not_found(format!(
            "{} is not installed",
            adapter.display_name
        )));
    }

    if disabled_tools(store).contains(&tool.to_string()) {
        return Err(AppError::invalid_input(format!(
            "{} is disabled",
            adapter.display_name
        )));
    }

    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;

    let source = PathBuf::from(&skill.central_path);
    let target = adapter
        .skills_dir()
        .join(sync_engine::target_dir_name(&source, &skill.name));
    let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
    let mode = sync_engine::sync_mode_for_tool(tool, configured_mode.as_deref());
    let actual_mode = sync_engine::sync_skill(&source, &target, mode).map_err(AppError::io)?;

    let now = chrono::Utc::now().timestamp_millis();
    let target_record = SkillTargetRecord {
        id: uuid::Uuid::new_v4().to_string(),
        skill_id: skill_id.to_string(),
        tool: tool.to_string(),
        target_path: target.to_string_lossy().to_string(),
        mode: actual_mode.as_str().to_string(),
        status: "ok".to_string(),
        synced_at: Some(now),
        last_error: None,
    };

    store.insert_target(&target_record).map_err(AppError::db)?;
    Ok(())
}

#[tauri::command]
pub async fn sync_skill_to_tool(
    skill_id: String,
    tool: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        sync_skill_to_tool_internal(&store, &skill_id, &tool)?;

        if let Ok(Some(active_id)) = store.get_active_scenario_id() {
            let skill_ids = store
                .get_skill_ids_for_scenario(&active_id)
                .map_err(AppError::db)?;
            if skill_ids.contains(&skill_id) {
                let adapter_keys: Vec<String> = tool_adapters::enabled_installed_adapters(&store)
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
    })
    .await?
}

#[tauri::command]
pub async fn unsync_skill_from_tool(
    skill_id: String,
    tool: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let targets = store
            .get_targets_for_skill(&skill_id)
            .map_err(AppError::db)?;

        if let Some(target) = targets.iter().find(|t| t.tool == tool) {
            let target_path = PathBuf::from(&target.target_path);
            sync_engine::remove_target(&target_path).ok();
        }

        store
            .delete_target(&skill_id, &tool)
            .map_err(AppError::db)?;

        if let Ok(Some(active_id)) = store.get_active_scenario_id() {
            let skill_ids = store
                .get_skill_ids_for_scenario(&active_id)
                .map_err(AppError::db)?;
            if skill_ids.contains(&skill_id) {
                let adapter_keys: Vec<String> = tool_adapters::enabled_installed_adapters(&store)
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
    })
    .await?
}

#[tauri::command]
pub async fn get_skill_tool_toggles(
    skill_id: String,
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillToolToggleDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill_ids = store
            .get_skill_ids_for_scenario(&scenario_id)
            .map_err(AppError::db)?;
        if !skill_ids.contains(&skill_id) {
            return Err(AppError::not_found("Skill is not enabled in this scenario"));
        }

        let disabled = disabled_tools(&store);
        let all_adapters = tool_adapters::all_tool_adapters(&store);
        let default_enabled_keys: Vec<String> = all_adapters
            .iter()
            .filter(|adapter| adapter.is_installed() && !disabled.contains(&adapter.key))
            .map(|adapter| adapter.key.clone())
            .collect();
        store
            .ensure_scenario_skill_tool_defaults(&scenario_id, &skill_id, &default_enabled_keys)
            .map_err(AppError::db)?;

        let toggles = store
            .get_scenario_skill_tool_toggles(&scenario_id, &skill_id)
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
    skill_id: String,
    scenario_id: String,
    tool: String,
    enabled: bool,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill_ids = store
            .get_skill_ids_for_scenario(&scenario_id)
            .map_err(AppError::db)?;
        if !skill_ids.contains(&skill_id) {
            return Err(AppError::not_found("Skill is not enabled in this scenario"));
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
            store.set_scenario_skill_tool_enabled(&scenario_id, &skill_id, &tool, enabled)?;
            sync_metadata::write_all_from_db_unlocked(&store)
        })
        .map_err(AppError::db)?;

        let is_active = store
            .get_active_scenario_id()
            .map_err(AppError::db)?
            .as_deref()
            == Some(scenario_id.as_str());
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
    .await?
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
