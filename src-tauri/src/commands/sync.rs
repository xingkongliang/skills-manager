use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

use crate::core::{
    error::AppError,
    skill_store::{SkillStore, SkillTargetRecord},
    sync_engine, tool_adapters,
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
    let target = adapter.skills_dir().join(&skill.name);
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
        let is_effective = store
            .is_skill_in_effective_scenario(&scenario_id, &skill_id)
            .map_err(AppError::db)?;
        if !is_effective {
            return Err(AppError::not_found("Skill is not in the effective skill set for this scenario"));
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
        let is_effective = store
            .is_skill_in_effective_scenario(&scenario_id, &skill_id)
            .map_err(AppError::db)?;
        if !is_effective {
            return Err(AppError::not_found("Skill is not in the effective skill set for this scenario"));
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

        store
            .set_scenario_skill_tool_enabled(&scenario_id, &skill_id, &tool, enabled)
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
