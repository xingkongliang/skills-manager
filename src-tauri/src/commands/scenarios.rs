use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

use crate::core::{
    error::AppError,
    skill_store::{ScenarioRecord, SkillStore},
    sync_engine, tool_adapters,
};
use std::collections::HashSet;

fn refresh_tray_menu_best_effort(app: &tauri::AppHandle) {
    if let Err(err) = crate::refresh_tray_menu(app) {
        log::warn!("Failed to refresh tray menu after scenario mutation: {err}");
    }
}

fn ensure_scenario_exists(store: &SkillStore, scenario_id: &str) -> Result<(), AppError> {
    let exists = store
        .get_all_scenarios()
        .map_err(AppError::db)?
        .iter()
        .any(|s| s.id == scenario_id);
    if !exists {
        return Err(AppError::not_found("Scenario not found"));
    }
    Ok(())
}

pub(crate) fn enabled_installed_adapters_for_scenario_skill(
    store: &SkillStore,
    scenario_id: &str,
    skill_id: &str,
) -> Result<Vec<tool_adapters::ToolAdapter>, AppError> {
    let adapters = tool_adapters::enabled_installed_adapters(store);
    let adapter_keys: Vec<String> = adapters.iter().map(|a| a.key.clone()).collect();

    store
        .ensure_scenario_skill_tool_defaults(scenario_id, skill_id, &adapter_keys)
        .map_err(AppError::db)?;

    let enabled = store
        .get_enabled_tools_for_scenario_skill(scenario_id, skill_id)
        .map_err(AppError::db)?;
    let enabled_set: HashSet<String> = enabled.into_iter().collect();

    Ok(adapters
        .into_iter()
        .filter(|adapter| enabled_set.contains(&adapter.key))
        .collect())
}

#[derive(Debug, Serialize)]
pub struct ScenarioDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub sort_order: i32,
    pub skill_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[tauri::command]
pub async fn get_scenarios(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<ScenarioDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let scenarios = store.get_all_scenarios().map_err(AppError::db)?;
        let mut result = Vec::new();
        for s in scenarios {
            let count = store.count_skills_for_scenario(&s.id).unwrap_or(0);
            result.push(ScenarioDto {
                id: s.id,
                name: s.name,
                description: s.description,
                icon: s.icon,
                sort_order: s.sort_order,
                skill_count: count,
                created_at: s.created_at,
                updated_at: s.updated_at,
            });
        }
        Ok(result)
    })
    .await?
}

#[tauri::command]
pub async fn get_active_scenario(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Option<ScenarioDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let active_id = store.get_active_scenario_id().map_err(AppError::db)?;

        if let Some(id) = active_id {
            let scenarios = store.get_all_scenarios().map_err(AppError::db)?;
            if let Some(s) = scenarios.into_iter().find(|s| s.id == id) {
                let count = store.count_skills_for_scenario(&s.id).unwrap_or(0);
                return Ok(Some(ScenarioDto {
                    id: s.id,
                    name: s.name,
                    description: s.description,
                    icon: s.icon,
                    sort_order: s.sort_order,
                    skill_count: count,
                    created_at: s.created_at,
                    updated_at: s.updated_at,
                }));
            }
        }
        Ok(None)
    })
    .await?
}

#[tauri::command]
pub async fn create_scenario(
    app: tauri::AppHandle,
    name: String,
    description: Option<String>,
    icon: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ScenarioDto, AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let now = chrono::Utc::now().timestamp_millis();
        let id = uuid::Uuid::new_v4().to_string();
        let previous_active_id = store.get_active_scenario_id().map_err(AppError::db)?;

        let record = ScenarioRecord {
            id: id.clone(),
            name: name.clone(),
            description: description.clone(),
            icon: icon.clone(),
            sort_order: 999,
            created_at: now,
            updated_at: now,
        };

        store.insert_scenario(&record).map_err(AppError::db)?;

        if let Some(previous_id) = previous_active_id.as_deref() {
            unsync_scenario_skills(&store, previous_id)?;
        }
        store.set_active_scenario(&id).map_err(AppError::db)?;

        Ok(ScenarioDto {
            id,
            name,
            description,
            icon,
            sort_order: 999,
            skill_count: 0,
            created_at: now,
            updated_at: now,
        })
    })
    .await?;
    if result.is_ok() {
        refresh_tray_menu_best_effort(&app);
    }
    result
}

#[tauri::command]
pub async fn update_scenario(
    app: tauri::AppHandle,
    id: String,
    name: String,
    description: Option<String>,
    icon: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        store
            .update_scenario(&id, &name, description.as_deref(), icon.as_deref())
            .map_err(AppError::db)
    })
    .await?;
    if result.is_ok() {
        refresh_tray_menu_best_effort(&app);
    }
    result
}

#[tauri::command]
pub async fn delete_scenario(
    app: tauri::AppHandle,
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let was_active = store
            .get_active_scenario_id()
            .map_err(AppError::db)?
            .as_deref()
            == Some(id.as_str());

        if was_active {
            unsync_scenario_skills(&store, &id)?;
        }

        store.delete_scenario(&id).map_err(AppError::db)?;

        if was_active {
            let remaining = store.get_all_scenarios().map_err(AppError::db)?;
            if let Some(first) = remaining.first() {
                store.set_active_scenario(&first.id).map_err(AppError::db)?;
                sync_scenario_skills(&store, &first.id)?;
            }
        }

        Ok(())
    })
    .await?;
    if result.is_ok() {
        refresh_tray_menu_best_effort(&app);
    }
    result
}

#[tauri::command]
pub async fn switch_scenario(
    app: tauri::AppHandle,
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        ensure_scenario_exists(&store, &id)?;

        // Unsync old scenario skills
        if let Ok(Some(old_id)) = store.get_active_scenario_id() {
            if old_id != id {
                unsync_scenario_skills(&store, &old_id)?;
            }
        }

        // Set new active
        store.set_active_scenario(&id).map_err(AppError::db)?;

        // Sync new scenario skills
        sync_scenario_skills(&store, &id)?;

        Ok(())
    })
    .await?;
    if result.is_ok() {
        refresh_tray_menu_best_effort(&app);
    }
    result
}

#[tauri::command]
pub async fn add_skill_to_scenario(
    app: tauri::AppHandle,
    skill_id: String,
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        store
            .add_skill_to_scenario(&scenario_id, &skill_id)
            .map_err(AppError::db)?;

        // If this is the active scenario, sync the skill
        if let Ok(Some(active_id)) = store.get_active_scenario_id() {
            if active_id == scenario_id {
                let adapters =
                    enabled_installed_adapters_for_scenario_skill(&store, &scenario_id, &skill_id)?;
                let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
                if let Ok(Some(skill)) = store.get_skill_by_id(&skill_id) {
                    let source = PathBuf::from(&skill.central_path);
                    for adapter in &adapters {
                        let target = adapter.skills_dir().join(&skill.name);
                        let mode = sync_engine::sync_mode_for_tool(
                            &adapter.key,
                            configured_mode.as_deref(),
                        );
                        if sync_engine::sync_skill(&source, &target, mode).is_ok() {
                            let now = chrono::Utc::now().timestamp_millis();
                            let target_record = crate::core::skill_store::SkillTargetRecord {
                                id: uuid::Uuid::new_v4().to_string(),
                                skill_id: skill_id.clone(),
                                tool: adapter.key.clone(),
                                target_path: target.to_string_lossy().to_string(),
                                mode: mode.as_str().to_string(),
                                status: "ok".to_string(),
                                synced_at: Some(now),
                                last_error: None,
                            };
                            store.insert_target(&target_record).ok();
                        }
                    }
                }
            }
        }

        Ok(())
    })
    .await?;
    if result.is_ok() {
        refresh_tray_menu_best_effort(&app);
    }
    result
}

#[tauri::command]
pub async fn remove_skill_from_scenario(
    app: tauri::AppHandle,
    skill_id: String,
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        store
            .remove_skill_from_scenario(&scenario_id, &skill_id)
            .map_err(AppError::db)?;

        // If this is the active scenario, unsync the skill
        if let Ok(Some(active_id)) = store.get_active_scenario_id() {
            if active_id == scenario_id {
                // Check if skill is in any other active scenario
                let other_scenarios = store.get_scenarios_for_skill(&skill_id).unwrap_or_default();
                if !other_scenarios.contains(&active_id) {
                    // Unsync from all tools
                    let targets = store.get_targets_for_skill(&skill_id).unwrap_or_default();
                    for target in &targets {
                        let path = PathBuf::from(&target.target_path);
                        sync_engine::remove_target(&path).ok();
                    }
                    // Remove all targets for this skill
                    for target in &targets {
                        store.delete_target(&skill_id, &target.tool).ok();
                    }
                }
            }
        }

        Ok(())
    })
    .await?;
    if result.is_ok() {
        refresh_tray_menu_best_effort(&app);
    }
    result
}

#[tauri::command]
pub async fn reorder_scenarios(
    app: tauri::AppHandle,
    ids: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        store.reorder_scenarios(&ids).map_err(AppError::db)
    })
    .await?;
    if result.is_ok() {
        refresh_tray_menu_best_effort(&app);
    }
    result
}

#[tauri::command]
pub async fn get_scenario_skill_order(
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<String>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .get_skill_ids_for_scenario(&scenario_id)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn reorder_scenario_skills(
    scenario_id: String,
    skill_ids: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .reorder_scenario_skills(&scenario_id, &skill_ids)
            .map_err(AppError::db)
    })
    .await?
}

// ── Internal helpers ──

pub(crate) fn sync_scenario_skills(store: &SkillStore, scenario_id: &str) -> Result<(), AppError> {
    let skills = store
        .get_skills_for_scenario(scenario_id)
        .map_err(AppError::db)?;
    let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;

    for skill in &skills {
        let source = PathBuf::from(&skill.central_path);
        let adapters =
            enabled_installed_adapters_for_scenario_skill(store, scenario_id, &skill.id)?;
        for adapter in &adapters {
            let target = adapter.skills_dir().join(&skill.name);
            let mode = sync_engine::sync_mode_for_tool(&adapter.key, configured_mode.as_deref());
            if let Ok(actual_mode) = sync_engine::sync_skill(&source, &target, mode) {
                let now = chrono::Utc::now().timestamp_millis();
                let target_record = crate::core::skill_store::SkillTargetRecord {
                    id: uuid::Uuid::new_v4().to_string(),
                    skill_id: skill.id.clone(),
                    tool: adapter.key.clone(),
                    target_path: target.to_string_lossy().to_string(),
                    mode: actual_mode.as_str().to_string(),
                    status: "ok".to_string(),
                    synced_at: Some(now),
                    last_error: None,
                };
                store.insert_target(&target_record).ok();
            }
        }
    }

    Ok(())
}

pub(crate) fn unsync_scenario_skills(
    store: &SkillStore,
    scenario_id: &str,
) -> Result<(), AppError> {
    let skill_ids = store
        .get_skill_ids_for_scenario(scenario_id)
        .map_err(AppError::db)?;

    for skill_id in &skill_ids {
        let targets = store.get_targets_for_skill(skill_id).unwrap_or_default();
        for target in &targets {
            let path = PathBuf::from(&target.target_path);
            sync_engine::remove_target(&path).ok();
            store.delete_target(skill_id, &target.tool).ok();
        }
    }

    Ok(())
}
