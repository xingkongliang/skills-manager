use serde::Serialize;
use std::sync::Arc;
use tauri::State;

use crate::core::{
    dedup,
    error::AppError,
    skill_store::{AgentSkillOwnership, PackRecord, SkillRecord, SkillStore},
    tool_adapters,
};

use super::scenarios;

#[derive(Debug, Serialize)]
pub struct AgentConfigDto {
    pub tool_key: String,
    pub display_name: String,
    pub scenario_id: Option<String>,
    pub scenario_name: Option<String>,
    pub managed: bool,
    pub installed: bool,
    pub effective_skill_count: usize,
    pub extra_pack_count: usize,
}

#[tauri::command]
pub async fn get_all_agent_configs(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<AgentConfigDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let configs = store.get_all_agent_configs().map_err(AppError::db)?;
        let scenarios = store.get_all_scenarios().map_err(AppError::db)?;
        let all_adapters = tool_adapters::all_tool_adapters(&store);

        let mut result = Vec::new();
        for config in configs {
            let adapter = all_adapters.iter().find(|a| a.key == config.tool_key);
            let display_name = adapter
                .map(|a| a.display_name.clone())
                .unwrap_or_else(|| config.tool_key.clone());
            let installed = adapter.map(|a| a.is_installed()).unwrap_or(false);
            let scenario_name = config.scenario_id.as_ref().and_then(|sid| {
                scenarios.iter().find(|s| &s.id == sid).map(|s| s.name.clone())
            });
            let effective_skill_count = store
                .get_effective_skills_for_agent(&config.tool_key)
                .map(|s| s.len())
                .unwrap_or(0);
            let extra_pack_count = store
                .get_agent_extra_packs(&config.tool_key)
                .map(|p| p.len())
                .unwrap_or(0);

            result.push(AgentConfigDto {
                tool_key: config.tool_key,
                display_name,
                scenario_id: config.scenario_id,
                scenario_name,
                managed: config.managed,
                installed,
                effective_skill_count,
                extra_pack_count,
            });
        }
        Ok(result)
    })
    .await?
}

#[tauri::command]
pub async fn get_agent_config(
    tool_key: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Option<AgentConfigDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let config = match store.get_agent_config(&tool_key).map_err(AppError::db)? {
            Some(c) => c,
            None => return Ok(None),
        };
        let scenarios = store.get_all_scenarios().map_err(AppError::db)?;
        let adapter = tool_adapters::find_adapter_with_store(&store, &tool_key);
        let display_name = adapter
            .as_ref()
            .map(|a| a.display_name.clone())
            .unwrap_or_else(|| config.tool_key.clone());
        let installed = adapter.map(|a| a.is_installed()).unwrap_or(false);
        let scenario_name = config.scenario_id.as_ref().and_then(|sid| {
            scenarios.iter().find(|s| &s.id == sid).map(|s| s.name.clone())
        });
        let effective_skill_count = store
            .get_effective_skills_for_agent(&config.tool_key)
            .map(|s| s.len())
            .unwrap_or(0);
        let extra_pack_count = store
            .get_agent_extra_packs(&config.tool_key)
            .map(|p| p.len())
            .unwrap_or(0);

        Ok(Some(AgentConfigDto {
            tool_key: config.tool_key,
            display_name,
            scenario_id: config.scenario_id,
            scenario_name,
            managed: config.managed,
            installed,
            effective_skill_count,
            extra_pack_count,
        }))
    })
    .await?
}

#[tauri::command]
pub async fn set_agent_scenario(
    tool_key: String,
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Unsync old skills for this agent first
        scenarios::unsync_agent_skills(&store, &tool_key)?;

        store
            .set_agent_scenario(&tool_key, &scenario_id)
            .map_err(AppError::db)?;

        // Re-sync this agent with its new scenario's skills
        scenarios::sync_agent_skills(&store, &tool_key)?;
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn set_agent_managed(
    tool_key: String,
    managed: bool,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .set_agent_managed(&tool_key, managed)
            .map_err(AppError::db)?;

        if managed {
            // Re-sync when re-enabling management
            scenarios::sync_agent_skills(&store, &tool_key)?;
        } else {
            // Unsync when disabling management
            scenarios::unsync_agent_skills(&store, &tool_key)?;
        }
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn add_agent_extra_pack(
    tool_key: String,
    pack_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .add_agent_extra_pack(&tool_key, &pack_id)
            .map_err(AppError::db)?;

        // Re-sync the agent to pick up the new pack's skills
        scenarios::sync_agent_skills(&store, &tool_key)?;
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn remove_agent_extra_pack(
    tool_key: String,
    pack_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Unsync first, then remove the pack, then re-sync
        scenarios::unsync_agent_skills(&store, &tool_key)?;

        store
            .remove_agent_extra_pack(&tool_key, &pack_id)
            .map_err(AppError::db)?;

        // Re-sync with the updated skill set
        scenarios::sync_agent_skills(&store, &tool_key)?;
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn get_effective_skills_for_agent(
    tool_key: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .get_effective_skills_for_agent(&tool_key)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn get_agent_extra_packs(
    tool_key: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<PackRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .get_agent_extra_packs(&tool_key)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn get_agent_skill_ownership(
    tool_key: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<AgentSkillOwnership, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let adapter = tool_adapters::find_adapter_with_store(&store, &tool_key)
            .ok_or_else(|| AppError::not_found(format!("Unknown tool: {tool_key}")))?;
        store
            .scan_agent_skill_ownership(&tool_key, &adapter.skills_dir())
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn dedup_agent_skills(
    tool_key: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<dedup::DedupResult, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let adapter = tool_adapters::find_adapter_with_store(&store, &tool_key)
            .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", tool_key)))?;
        let skills_dir = adapter.skills_dir();
        dedup::dedup_agent_skills(&store, &tool_key, &skills_dir, false)
            .map_err(AppError::internal)
    })
    .await
    .map_err(AppError::internal)?
}

#[tauri::command]
pub async fn dedup_all_agents(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<(String, dedup::DedupResult)>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let adapters = tool_adapters::enabled_installed_adapters(&store);
        Ok(dedup::dedup_all_agents(&store, &adapters, false))
    })
    .await
    .map_err(AppError::internal)?
}
