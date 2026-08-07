use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, State};

use crate::core::error::AppError;
use crate::core::scenario_service;
use crate::core::scenario_service::sync_scenario_skills;
use crate::core::skill_store::SkillStore;
use crate::core::sync_engine;
use crate::core::timing::should_log_first_or_slow;
use crate::core::tool_adapters::{self, CustomToolDef, ToolCategory};
use crate::core::tool_service::{
    self, get_custom_tool_paths, get_custom_tool_project_paths, get_custom_tools,
    get_disabled_tools, get_tool_order, normalize_project_relative_skills_dir_input,
    normalize_skills_dir_input, set_custom_tool_paths, set_custom_tool_project_paths,
    set_custom_tools, set_disabled_tools, set_tool_order, ToolInfo,
};

#[derive(Debug, Serialize)]
pub struct ToolInfoDto {
    pub key: String,
    pub display_name: String,
    pub installed: bool,
    pub skills_dir: String,
    pub enabled: bool,
    pub is_custom: bool,
    pub has_path_override: bool,
    pub project_relative_skills_dir: Option<String>,
    pub has_project_path_override: bool,
    pub category: ToolCategory,
}

/// Sync active scenario skills to a single tool.
fn sync_active_scenario_to_tool(store: &SkillStore, tool_key: &str) {
    scenario_service::sync_active_scenario_to_tool(store, tool_key)
}

/// Remove all synced skill files and target records for a given tool.
fn unsync_all_for_tool(store: &SkillStore, tool_key: &str) {
    let targets = store.get_all_targets().unwrap_or_default();
    for target in targets.iter().filter(|t| t.tool == tool_key) {
        sync_engine::remove_target(&PathBuf::from(&target.target_path)).ok();
        store.delete_target(&target.skill_id, tool_key).ok();
    }
}

fn reconcile_tool_sync_after_path_change(store: &SkillStore, tool_key: &str) {
    // Remove existing synced artifacts/records (old path), then re-sync to current adapter path.
    unsync_all_for_tool(store, tool_key);
    let disabled = get_disabled_tools(store);
    if !disabled.contains(&tool_key.to_string()) {
        sync_active_scenario_to_tool(store, tool_key);
    }
}

static GET_TOOL_STATUS_FIRST_CALL: AtomicBool = AtomicBool::new(true);

#[tauri::command]
pub async fn get_tool_status(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<ToolInfoDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let start = Instant::now();
        let infos = tool_service::list_tool_info(&store);
        let count = infos.len();
        let result: Vec<ToolInfoDto> = infos
            .into_iter()
            .map(|info: ToolInfo| ToolInfoDto {
                key: info.key,
                display_name: info.display_name,
                installed: info.installed,
                skills_dir: info.skills_dir,
                enabled: info.enabled,
                is_custom: info.is_custom,
                has_path_override: info.has_path_override,
                project_relative_skills_dir: info.project_relative_skills_dir,
                has_project_path_override: info.has_project_path_override,
                category: info.category,
            })
            .collect();
        let elapsed_ms = start.elapsed().as_millis();
        if should_log_first_or_slow(&GET_TOOL_STATUS_FIRST_CALL, elapsed_ms, 100) {
            log::info!("get_tool_status: {count} tools in {elapsed_ms} ms");
        }
        Ok(result)
    })
    .await?
}

fn refresh_tray_menu_best_effort(app: &AppHandle) {
    if let Err(err) = crate::refresh_tray_menu(app) {
        log::warn!("Failed to refresh tray menu after tool mutation: {err}");
    }
}

#[tauri::command]
pub async fn set_tool_enabled(
    app: AppHandle,
    key: String,
    enabled: bool,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut disabled = get_disabled_tools(&store);
        if enabled {
            disabled.retain(|k| k != &key);
            set_disabled_tools(&store, &disabled)?;
            sync_active_scenario_to_tool(&store, &key);
            Ok(())
        } else {
            if !disabled.contains(&key) {
                disabled.push(key.clone());
            }
            unsync_all_for_tool(&store, &key);
            set_disabled_tools(&store, &disabled)
        }
    })
    .await?;
    if result.is_ok() {
        refresh_tray_menu_best_effort(&app);
    }
    result
}

#[tauri::command]
pub async fn set_all_tools_enabled(
    app: AppHandle,
    enabled: bool,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        if enabled {
            set_disabled_tools(&store, &[])?;
            // Re-sync active scenario skills to all (now-enabled) installed tools
            if let Ok(Some(active_id)) = store.get_active_scenario_id() {
                sync_scenario_skills(&store, &active_id).ok();
            }
            Ok(())
        } else {
            let adapters = tool_adapters::all_tool_adapters(&store);
            let all_keys: Vec<String> = adapters.iter().map(|a| a.key.clone()).collect();
            for adapter in &adapters {
                unsync_all_for_tool(&store, &adapter.key);
            }
            set_disabled_tools(&store, &all_keys)
        }
    })
    .await?;
    if result.is_ok() {
        refresh_tray_menu_best_effort(&app);
    }
    result
}

#[tauri::command]
pub async fn get_tool_order_cmd(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<String>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || Ok(get_tool_order(&store))).await?
}

#[tauri::command]
pub async fn set_tool_order_cmd(
    order: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || set_tool_order(&store, &order)).await?
}

#[tauri::command]
pub async fn set_custom_tool_path(
    key: String,
    path: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let key = key.trim().to_string();
        let path = normalize_skills_dir_input(&path)?;
        if key.is_empty() || path.is_empty() {
            return Err(AppError::invalid_input("Key and path are required"));
        }

        let old_adapter = tool_adapters::find_adapter_with_store(&store, &key)
            .ok_or_else(|| AppError::not_found(format!("Unknown tool: {key}")))?;
        let old_skills_dir = old_adapter.skills_dir();

        let mut customs = get_custom_tools(&store);
        if let Some(custom) = customs.iter_mut().find(|c| c.key == key) {
            custom.skills_dir = path;
            set_custom_tools(&store, &customs)?;
        } else {
            let mut paths = get_custom_tool_paths(&store);
            paths.insert(key.clone(), path);
            set_custom_tool_paths(&store, &paths)?;
        }

        let new_adapter = tool_adapters::find_adapter_with_store(&store, &key)
            .ok_or_else(|| AppError::not_found(format!("Unknown tool: {key}")))?;
        if old_skills_dir != new_adapter.skills_dir() {
            reconcile_tool_sync_after_path_change(&store, &key);
        }
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn reset_custom_tool_path(
    key: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let old_adapter = tool_adapters::find_adapter_with_store(&store, &key)
            .ok_or_else(|| AppError::not_found(format!("Unknown tool: {key}")))?;
        let old_skills_dir = old_adapter.skills_dir();

        let mut paths = get_custom_tool_paths(&store);
        paths.remove(&key);
        set_custom_tool_paths(&store, &paths)?;

        let new_adapter = tool_adapters::find_adapter_with_store(&store, &key)
            .ok_or_else(|| AppError::not_found(format!("Unknown tool: {key}")))?;
        if old_skills_dir != new_adapter.skills_dir() {
            reconcile_tool_sync_after_path_change(&store, &key);
        }
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn set_custom_tool_project_path(
    key: String,
    project_relative_skills_dir: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let key = key.trim().to_string();
        if key.is_empty() {
            return Err(AppError::invalid_input("Key is required"));
        }
        let normalized = normalize_project_relative_skills_dir_input(
            project_relative_skills_dir.as_deref().unwrap_or_default(),
        )?;

        // Custom tools store the project path on their definition; clearing it
        // (None) drops project-workspace support for that agent.
        let mut customs = get_custom_tools(&store);
        if let Some(custom) = customs.iter_mut().find(|c| c.key == key) {
            custom.project_relative_skills_dir = normalized;
            return set_custom_tools(&store, &customs);
        }

        // Built-in tools keep overrides in a side map keyed by tool key.
        // Resolve the built-in default project path (no store overrides) to
        // validate the key and to detect no-op edits: an empty value, or one
        // equal to the default, removes the override and restores the default.
        let default_project_path = tool_adapters::default_tool_adapters()
            .into_iter()
            .find(|a| a.key == key)
            .map(|a| a.project_relative_skills_dir().to_string())
            .ok_or_else(|| AppError::not_found(format!("Unknown tool: {key}")))?;
        let mut project_paths = get_custom_tool_project_paths(&store);
        match normalized {
            Some(path) if path != default_project_path => {
                project_paths.insert(key, path);
            }
            _ => {
                project_paths.remove(&key);
            }
        }
        set_custom_tool_project_paths(&store, &project_paths)
    })
    .await?
}

#[tauri::command]
pub async fn reset_custom_tool_project_path(
    key: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let key = key.trim().to_string();
        if key.is_empty() {
            return Err(AppError::invalid_input("Key is required"));
        }
        if tool_adapters::find_adapter_with_store(&store, &key).is_none() {
            return Err(AppError::not_found(format!("Unknown tool: {key}")));
        }
        let mut project_paths = get_custom_tool_project_paths(&store);
        if project_paths.remove(&key).is_some() {
            set_custom_tool_project_paths(&store, &project_paths)?;
        }
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn add_custom_tool(
    key: String,
    display_name: String,
    skills_dir: String,
    project_relative_skills_dir: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let key = key.trim().to_string();
        let display_name = display_name.trim().to_string();
        let skills_dir = normalize_skills_dir_input(&skills_dir)?;
        let project_relative_skills_dir = normalize_project_relative_skills_dir_input(
            project_relative_skills_dir.as_deref().unwrap_or_default(),
        )?;
        if key.is_empty() || display_name.is_empty() || skills_dir.is_empty() {
            return Err(AppError::invalid_input(
                "Agent key, name and skills path are required",
            ));
        }

        // Validate key uniqueness
        let all = tool_adapters::all_tool_adapters(&store);
        if all.iter().any(|a| a.key == key) {
            return Err(AppError::invalid_input(format!(
                "Agent key \"{key}\" already exists"
            )));
        }
        let mut customs = get_custom_tools(&store);
        customs.push(CustomToolDef {
            key: key.clone(),
            display_name,
            skills_dir,
            project_relative_skills_dir,
            category: Default::default(),
        });
        set_custom_tools(&store, &customs)?;
        reconcile_tool_sync_after_path_change(&store, &key);
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn remove_custom_tool(
    key: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Remove synced targets for this tool
        let targets = store.get_all_targets().unwrap_or_default();
        for target in targets.iter().filter(|t| t.tool == key) {
            crate::core::sync_engine::remove_target(&PathBuf::from(&target.target_path)).ok();
            store.delete_target(&target.skill_id, &key).ok();
        }
        // Remove from custom_tools list
        let mut customs = get_custom_tools(&store);
        customs.retain(|c| c.key != key);
        set_custom_tools(&store, &customs)?;
        // Remove any stale override for this key.
        let mut custom_paths = get_custom_tool_paths(&store);
        custom_paths.remove(&key);
        set_custom_tool_paths(&store, &custom_paths)?;
        // Also remove from disabled_tools if present
        let mut disabled = get_disabled_tools(&store);
        disabled.retain(|k| k != &key);
        set_disabled_tools(&store, &disabled)
    })
    .await?
}

pub fn migrate_legacy_tool_keys(store: &SkillStore) -> Result<(), AppError> {
    tool_service::migrate_legacy_tool_keys(store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::skill_store::{ScenarioRecord, SkillRecord};
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

    fn sample_scenario(id: &str, name: &str) -> ScenarioRecord {
        ScenarioRecord {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            icon: None,
            sort_order: 0,
            created_at: 1,
            updated_at: 1,
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
    fn active_scenario_tool_sync_keeps_duplicate_skill_names_separate() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let source_base = tmp.path().join("central");
        let target_base = tmp.path().join("agent-skills");
        fs::create_dir_all(&source_base).unwrap();
        fs::create_dir_all(&target_base).unwrap();
        configure_single_custom_tool(&store, &target_base);

        store
            .insert_scenario(&sample_scenario("active", "Active"))
            .unwrap();
        store.set_active_scenario("active").unwrap();

        let first_dir = write_skill_dir(&source_base, "skill123", "first");
        let second_dir = write_skill_dir(&source_base, "skill123-2", "second");
        store
            .insert_skill(&sample_skill("first", "skill123", &first_dir))
            .unwrap();
        store
            .insert_skill(&sample_skill("second", "skill123", &second_dir))
            .unwrap();
        store.add_skill_to_scenario("active", "first").unwrap();
        store.add_skill_to_scenario("active", "second").unwrap();

        sync_active_scenario_to_tool(&store, "test_agent");

        assert_eq!(
            fs::read_to_string(target_base.join("skill123/unique.txt")).unwrap(),
            "first"
        );
        assert_eq!(
            fs::read_to_string(target_base.join("skill123-2/unique.txt")).unwrap(),
            "second"
        );
        let targets = store.get_all_targets().unwrap();
        assert!(targets.iter().any(|target| {
            target.skill_id == "first" && target.target_path.ends_with("skill123")
        }));
        assert!(targets.iter().any(|target| {
            target.skill_id == "second" && target.target_path.ends_with("skill123-2")
        }));
    }
}
