use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

use crate::core::{
    skill_store::{ScenarioRecord, SkillStore},
    sync_engine, tool_adapters,
};

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
pub fn get_scenarios(store: State<'_, Arc<SkillStore>>) -> Result<Vec<ScenarioDto>, String> {
    let scenarios = store.get_all_scenarios().map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for s in scenarios {
        let count = store
            .count_skills_for_scenario(&s.id)
            .unwrap_or(0);
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
}

#[tauri::command]
pub fn get_active_scenario(store: State<'_, Arc<SkillStore>>) -> Result<Option<ScenarioDto>, String> {
    let active_id = store
        .get_active_scenario_id()
        .map_err(|e| e.to_string())?;

    if let Some(id) = active_id {
        let scenarios = store.get_all_scenarios().map_err(|e| e.to_string())?;
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
}

#[tauri::command]
pub fn create_scenario(
    name: String,
    description: Option<String>,
    icon: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ScenarioDto, String> {
    let now = chrono::Utc::now().timestamp_millis();
    let id = uuid::Uuid::new_v4().to_string();
    let previous_active_id = store.get_active_scenario_id().map_err(|e| e.to_string())?;

    let record = ScenarioRecord {
        id: id.clone(),
        name: name.clone(),
        description: description.clone(),
        icon: icon.clone(),
        sort_order: 999,
        created_at: now,
        updated_at: now,
    };

    store
        .insert_scenario(&record)
        .map_err(|e| e.to_string())?;

    if let Some(previous_id) = previous_active_id.as_deref() {
        unsync_scenario_skills(&store, previous_id)?;
    }
    store.set_active_scenario(&id).map_err(|e| e.to_string())?;

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
}

#[tauri::command]
pub fn update_scenario(
    id: String,
    name: String,
    description: Option<String>,
    icon: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), String> {
    store
        .update_scenario(&id, &name, description.as_deref(), icon.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_scenario(id: String, store: State<'_, Arc<SkillStore>>) -> Result<(), String> {
    let was_active = store
        .get_active_scenario_id()
        .map_err(|e| e.to_string())?
        .as_deref()
        == Some(id.as_str());

    if was_active {
        unsync_scenario_skills(&store, &id)?;
    }

    store.delete_scenario(&id).map_err(|e| e.to_string())?;

    if was_active {
        let remaining = store.get_all_scenarios().map_err(|e| e.to_string())?;
        if let Some(first) = remaining.first() {
            store
                .set_active_scenario(&first.id)
                .map_err(|e| e.to_string())?;
            sync_scenario_skills(&store, &first.id)?;
        }
    }

    Ok(())
}

#[tauri::command]
pub fn switch_scenario(id: String, store: State<'_, Arc<SkillStore>>) -> Result<(), String> {
    // Unsync old scenario skills
    if let Ok(Some(old_id)) = store.get_active_scenario_id() {
        if old_id != id {
            unsync_scenario_skills(&store, &old_id)?;
        }
    }

    // Set new active
    store
        .set_active_scenario(&id)
        .map_err(|e| e.to_string())?;

    // Sync new scenario skills
    sync_scenario_skills(&store, &id)?;

    Ok(())
}

#[tauri::command]
pub fn add_skill_to_scenario(
    skill_id: String,
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), String> {
    store
        .add_skill_to_scenario(&scenario_id, &skill_id)
        .map_err(|e| e.to_string())?;

    // If this is the active scenario, sync the skill
    if let Ok(Some(active_id)) = store.get_active_scenario_id() {
        if active_id == scenario_id {
            // Sync to all installed tools
            let adapters = tool_adapters::default_tool_adapters();
            let configured_mode = store.get_setting("sync_mode").map_err(|e| e.to_string())?;
            if let Ok(Some(skill)) = store.get_skill_by_id(&skill_id) {
                let source = PathBuf::from(&skill.central_path);
                for adapter in &adapters {
                    if adapter.is_installed() {
                        let target = adapter.skills_dir().join(&skill.name);
                        let mode = sync_engine::sync_mode_for_tool(&adapter.key, configured_mode.as_deref());
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
    }

    Ok(())
}

#[tauri::command]
pub fn remove_skill_from_scenario(
    skill_id: String,
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), String> {
    store
        .remove_skill_from_scenario(&scenario_id, &skill_id)
        .map_err(|e| e.to_string())?;

    // If this is the active scenario, unsync the skill
    if let Ok(Some(active_id)) = store.get_active_scenario_id() {
        if active_id == scenario_id {
            // Check if skill is in any other active scenario
            let other_scenarios = store
                .get_scenarios_for_skill(&skill_id)
                .unwrap_or_default();
            if !other_scenarios.contains(&active_id) {
                // Unsync from all tools
                let targets = store
                    .get_targets_for_skill(&skill_id)
                    .unwrap_or_default();
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
}

#[tauri::command]
pub fn reorder_scenarios(
    ids: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), String> {
    store
        .reorder_scenarios(&ids)
        .map_err(|e| e.to_string())
}

// ── Internal helpers ──

pub(crate) fn sync_scenario_skills(store: &SkillStore, scenario_id: &str) -> Result<(), String> {
    let skills = store
        .get_skills_for_scenario(scenario_id)
        .map_err(|e| e.to_string())?;
    let adapters = tool_adapters::default_tool_adapters();
    let configured_mode = store.get_setting("sync_mode").map_err(|e| e.to_string())?;

    for skill in &skills {
        let source = PathBuf::from(&skill.central_path);
        for adapter in &adapters {
            if !adapter.is_installed() {
                continue;
            }
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

pub(crate) fn unsync_scenario_skills(store: &SkillStore, scenario_id: &str) -> Result<(), String> {
    let skill_ids = store
        .get_skill_ids_for_scenario(scenario_id)
        .map_err(|e| e.to_string())?;

    for skill_id in &skill_ids {
        let targets = store
            .get_targets_for_skill(skill_id)
            .unwrap_or_default();
        for target in &targets {
            let path = PathBuf::from(&target.target_path);
            sync_engine::remove_target(&path).ok();
            store.delete_target(skill_id, &target.tool).ok();
        }
    }

    Ok(())
}
