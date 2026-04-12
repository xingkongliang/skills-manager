use std::sync::Arc;
use tauri::State;

use crate::core::{
    error::AppError,
    plugins,
    skill_store::{ManagedPluginRecord, ScenarioPluginRecord, SkillStore},
};

#[tauri::command]
pub async fn get_managed_plugins(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<ManagedPluginRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.get_all_managed_plugins().map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn scan_plugins(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<ManagedPluginRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        plugins::scan_and_register_plugins(&store).map_err(AppError::io)
    })
    .await?
}

#[tauri::command]
pub async fn get_scenario_plugins(
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<ScenarioPluginRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .get_scenario_plugins(&scenario_id)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn set_scenario_plugin_enabled(
    scenario_id: String,
    plugin_id: String,
    enabled: bool,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .set_scenario_plugin_enabled(&scenario_id, &plugin_id, enabled)
            .map_err(AppError::db)?;

        // If this is the active scenario, apply the change immediately
        if let Ok(Some(active_id)) = store.get_active_scenario_id() {
            if active_id == scenario_id {
                plugins::apply_scenario_plugins(&store, &scenario_id).map_err(AppError::io)?;
            }
        }

        Ok(())
    })
    .await?
}
