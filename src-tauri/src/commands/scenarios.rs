use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

use crate::core::{
    error::AppError,
    plugins,
    skill_store::{DisclosureMode, ScenarioRecord, SkillStore},
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
            disclosure_mode: DisclosureMode::Full,
        };

        store.insert_scenario(&record).map_err(AppError::db)?;

        if let Some(previous_id) = previous_active_id.as_deref() {
            unsync_scenario_skills(&store, previous_id)?;
        }
        store.set_active_scenario(&id).map_err(AppError::db)?;

        // Update all managed agents to use this new scenario
        let configs = store.get_all_agent_configs().map_err(AppError::db)?;
        for config in &configs {
            if config.managed {
                store.set_agent_scenario(&config.tool_key, &id).map_err(AppError::db)?;
            }
        }

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
                // Update all managed agents to the fallback scenario
                let configs = store.get_all_agent_configs().map_err(AppError::db)?;
                for config in &configs {
                    if config.managed {
                        store.set_agent_scenario(&config.tool_key, &first.id).map_err(AppError::db)?;
                    }
                }
                sync_scenario_skills(&store, &first.id)?;
            } else {
                store.clear_active_scenario().map_err(AppError::db)?;
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

        // Update all managed agents to use this scenario
        let configs = store.get_all_agent_configs().map_err(AppError::db)?;
        for config in &configs {
            if config.managed {
                store.set_agent_scenario(&config.tool_key, &id).map_err(AppError::db)?;
            }
        }

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

        // Re-sync all managed agents that are on this scenario
        let agent_configs = store.get_all_agent_configs().map_err(AppError::db)?;
        for config in &agent_configs {
            if !config.managed {
                continue;
            }
            if config.scenario_id.as_deref() == Some(scenario_id.as_str()) {
                sync_agent_skills(&store, &config.tool_key)?;
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

        // For each managed agent on this scenario, check if the skill is still
        // in the effective set (might still be inherited via a pack). If not,
        // remove the sync target for that agent's tool.
        let agent_configs = store.get_all_agent_configs().map_err(AppError::db)?;
        for config in &agent_configs {
            if !config.managed {
                continue;
            }
            if config.scenario_id.as_deref() != Some(scenario_id.as_str()) {
                continue;
            }
            let still_effective = store
                .is_skill_in_effective_scenario(&scenario_id, &skill_id)
                .unwrap_or(false);
            if !still_effective {
                let targets = store.get_targets_for_skill(&skill_id).unwrap_or_default();
                for target in &targets {
                    if target.tool != config.tool_key {
                        continue;
                    }
                    let path = PathBuf::from(&target.target_path);
                    if let Err(e) = sync_engine::remove_target(&path) {
                        log::warn!("Failed to remove sync target {}: {e}", path.display());
                    }
                    if let Err(e) = store.delete_target(&skill_id, &target.tool) {
                        log::warn!(
                            "Failed to delete target record for skill {skill_id}, tool {}: {e}",
                            target.tool
                        );
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

/// Sync a single agent's effective skills to its tool directory.
pub(crate) fn sync_agent_skills(
    store: &SkillStore,
    tool_key: &str,
) -> Result<(), AppError> {
    let adapter = match tool_adapters::find_adapter_with_store(store, tool_key) {
        Some(a) if a.is_installed() => a,
        _ => return Ok(()),
    };

    let skills = store
        .get_effective_skills_for_agent(tool_key)
        .map_err(AppError::db)?;
    let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;

    // Get the agent's scenario_id for per-skill tool toggle lookups
    let agent_scenario_id = store
        .get_agent_config(tool_key)
        .map_err(AppError::db)?
        .and_then(|c| c.scenario_id);

    for skill in &skills {
        let source = PathBuf::from(&skill.central_path);
        if !source.exists() {
            continue;
        }

        // If agent has a scenario, check per-skill tool toggles
        if let Some(ref scenario_id) = agent_scenario_id {
            let adapter_keys = vec![adapter.key.clone()];
            store
                .ensure_scenario_skill_tool_defaults(scenario_id, &skill.id, &adapter_keys)
                .map_err(AppError::db)?;

            let enabled = store
                .get_enabled_tools_for_scenario_skill(scenario_id, &skill.id)
                .map_err(AppError::db)?;
            if !enabled.contains(&adapter.key) {
                continue;
            }
        }

        let target = adapter.skills_dir().join(&skill.name);
        let mode = sync_engine::sync_mode_for_tool(&adapter.key, configured_mode.as_deref());
        match sync_engine::sync_skill(&source, &target, mode) {
            Ok(actual_mode) => {
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
                if let Err(e) = store.insert_target(&target_record) {
                    log::warn!("Failed to insert sync target for skill {}: {e}", skill.id);
                }
            }
            Err(e) => {
                log::warn!(
                    "Failed to sync skill {} to {}: {e}",
                    skill.id,
                    target.display()
                );
            }
        }
    }

    Ok(())
}

/// Unsync a single agent's effective skills from its tool directory.
pub(crate) fn unsync_agent_skills(
    store: &SkillStore,
    tool_key: &str,
) -> Result<(), AppError> {
    // Remove all targets for this tool
    let all_targets = store.get_all_targets().map_err(AppError::db)?;
    for target in &all_targets {
        if target.tool != tool_key {
            continue;
        }
        let path = PathBuf::from(&target.target_path);
        if let Err(e) = sync_engine::remove_target(&path) {
            log::warn!("Failed to remove sync target {}: {e}", path.display());
        }
        if let Err(e) = store.delete_target(&target.skill_id, &target.tool) {
            log::warn!(
                "Failed to delete target record for skill {}, tool {}: {e}",
                target.skill_id,
                target.tool
            );
        }
    }
    Ok(())
}

/// Sync scenario skills across all managed agents.
/// Each agent gets its own effective skill list based on its per-agent scenario
/// (and any extra packs assigned to it).
pub(crate) fn sync_scenario_skills(store: &SkillStore, scenario_id: &str) -> Result<(), AppError> {
    let agent_configs = store.get_all_agent_configs().map_err(AppError::db)?;

    for config in &agent_configs {
        if !config.managed {
            continue;
        }
        // Only sync agents that are on this scenario
        if config.scenario_id.as_deref() != Some(scenario_id) {
            continue;
        }
        sync_agent_skills(store, &config.tool_key)?;
    }

    // Apply per-scenario plugin state to installed_plugins.json
    if let Err(e) = plugins::apply_scenario_plugins(store, scenario_id) {
        log::warn!("Failed to apply scenario plugin state: {e}");
    }

    Ok(())
}

pub(crate) fn unsync_scenario_skills(
    store: &SkillStore,
    scenario_id: &str,
) -> Result<(), AppError> {
    let agent_configs = store.get_all_agent_configs().map_err(AppError::db)?;

    for config in &agent_configs {
        if !config.managed {
            continue;
        }
        // Only unsync agents that are on this scenario
        if config.scenario_id.as_deref() != Some(scenario_id) {
            continue;
        }
        unsync_agent_skills(store, &config.tool_key)?;
    }

    // Restore all plugins when unsyncing (back to "all enabled" default)
    if let Err(e) = plugins::restore_all_plugins(store) {
        log::warn!("Failed to restore plugins during unsync: {e}");
    }

    Ok(())
}

#[tauri::command]
pub async fn set_scenario_disclosure_mode(
    scenario_id: String,
    mode: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .set_scenario_disclosure_mode(&scenario_id, &mode)
            .map_err(AppError::db)
    })
    .await?
}
