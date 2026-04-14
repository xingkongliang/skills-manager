use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

use crate::core::{
    central_repo, dedup,
    error::AppError,
    installer,
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

#[tauri::command]
pub async fn import_discovered_skill(
    discovered_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let discovered = store
            .get_discovered_by_id(&discovered_id)
            .map_err(AppError::db)?
            .ok_or_else(|| {
                AppError::not_found(format!("Discovered skill '{}' not found", discovered_id))
            })?;

        let source_path = PathBuf::from(&discovered.found_path);
        if !source_path.exists() {
            return Err(AppError::not_found(format!(
                "Source path no longer exists: {}",
                discovered.found_path
            )));
        }

        let name = discovered
            .name_guess
            .as_deref()
            .unwrap_or("unnamed-skill");

        let central_path = central_repo::skills_dir().join(name);

        // Check if already exists in central store
        if let Some(existing) = store
            .get_skill_by_central_path(&central_path.to_string_lossy())
            .map_err(AppError::db)?
        {
            // Link discovered to existing skill
            store
                .link_discovered_to_skill(&discovered_id, &existing.id)
                .map_err(AppError::db)?;

            // Add to active scenario
            if let Ok(Some(scenario_id)) = store.get_active_scenario_id() {
                store.add_skill_to_scenario(&scenario_id, &existing.id).ok();
            }
            return Ok(());
        }

        // Install to central store
        let result = installer::install_from_local_to_destination(
            &source_path,
            Some(name),
            &central_path,
        )
        .map_err(AppError::io)?;

        let now = chrono::Utc::now().timestamp_millis();
        let skill_id = uuid::Uuid::new_v4().to_string();
        let record = crate::core::skill_store::SkillRecord {
            id: skill_id.clone(),
            name: result.name,
            description: result.description,
            source_type: "import".to_string(),
            source_ref: Some(discovered.found_path),
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: result.central_path.to_string_lossy().to_string(),
            content_hash: Some(result.content_hash),
            enabled: true,
            created_at: now,
            updated_at: now,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: Some(now),
            last_check_error: None,
        };
        store.insert_skill(&record).map_err(AppError::db)?;
        store
            .link_discovered_to_skill(&discovered_id, &skill_id)
            .map_err(AppError::db)?;

        // Add to active scenario
        if let Ok(Some(scenario_id)) = store.get_active_scenario_id() {
            store.add_skill_to_scenario(&scenario_id, &skill_id).ok();
        }

        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn mark_skill_as_native(
    discovered_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .mark_discovered_as_native(&discovered_id)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn unmark_skill_as_native(
    discovered_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .unmark_discovered_as_native(&discovered_id)
            .map_err(AppError::db)
    })
    .await?
}
