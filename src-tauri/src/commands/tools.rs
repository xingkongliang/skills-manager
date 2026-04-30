use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

use crate::core::error::AppError;
use crate::core::skill_store::{SkillStore, SkillTargetRecord};
use crate::core::sync_engine;
use crate::core::tool_adapters::{self, CustomToolDef};

use super::scenarios::{enabled_installed_adapters_for_scenario_skill, sync_scenario_skills};

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
}

fn get_disabled_tools(store: &SkillStore) -> Vec<String> {
    store
        .get_setting("disabled_tools")
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default()
}

fn set_disabled_tools(store: &SkillStore, disabled: &[String]) -> Result<(), AppError> {
    let json = serde_json::to_string(disabled)
        .map_err(|e| AppError::internal(format!("Failed to serialize: {e}")))?;
    store
        .set_setting("disabled_tools", &json)
        .map_err(AppError::db)
}

fn get_custom_tool_paths(store: &SkillStore) -> HashMap<String, String> {
    tool_adapters::custom_tool_paths(store)
}

fn set_custom_tool_paths(
    store: &SkillStore,
    paths: &HashMap<String, String>,
) -> Result<(), AppError> {
    let json = serde_json::to_string(paths)
        .map_err(|e| AppError::internal(format!("Failed to serialize: {e}")))?;
    store
        .set_setting("custom_tool_paths", &json)
        .map_err(AppError::db)
}

fn get_custom_tools(store: &SkillStore) -> Vec<CustomToolDef> {
    tool_adapters::custom_tools(store)
}

fn set_custom_tools(store: &SkillStore, custom_tools: &[CustomToolDef]) -> Result<(), AppError> {
    let json = serde_json::to_string(custom_tools)
        .map_err(|e| AppError::internal(format!("Failed to serialize: {e}")))?;
    store
        .set_setting("custom_tools", &json)
        .map_err(AppError::db)
}

fn normalize_skills_dir_input(path: &str) -> Result<String, AppError> {
    let raw = path.trim();
    if raw.is_empty() {
        return Err(AppError::invalid_input("Path is required"));
    }

    let expanded = if raw == "~" {
        dirs::home_dir()
            .ok_or_else(|| AppError::internal("Cannot determine home directory"))?
            .to_string_lossy()
            .to_string()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        dirs::home_dir()
            .ok_or_else(|| AppError::internal("Cannot determine home directory"))?
            .join(rest)
            .to_string_lossy()
            .to_string()
    } else if !std::path::Path::new(raw).is_absolute() {
        return Err(AppError::invalid_input(
            "Skills path must be absolute (or start with ~/)",
        ));
    } else {
        raw.to_string()
    };

    Ok(expanded)
}

fn normalize_project_relative_skills_dir_input(path: &str) -> Result<Option<String>, AppError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let candidate = std::path::Path::new(trimmed);
    if candidate.is_absolute() {
        return Err(AppError::invalid_input(
            "Project skills path must be relative to the project root",
        ));
    }
    if candidate
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(AppError::invalid_input(
            "Project skills path cannot contain parent directory segments",
        ));
    }
    Ok(Some(trimmed.trim_matches('/').to_string()))
}

/// Sync active scenario skills to a single tool.
fn sync_active_scenario_to_tool(store: &SkillStore, tool_key: &str) {
    let active_id = match store.get_active_scenario_id() {
        Ok(Some(id)) => id,
        _ => return,
    };
    let skills = match store.get_skills_for_scenario(&active_id) {
        Ok(s) => s,
        _ => return,
    };
    let adapter = match tool_adapters::find_adapter_with_store(store, tool_key) {
        Some(a) if a.is_installed() => a,
        _ => return,
    };
    let configured_mode = store.get_setting("sync_mode").ok().flatten();
    for skill in &skills {
        let allowed_adapters =
            match enabled_installed_adapters_for_scenario_skill(store, &active_id, &skill.id) {
                Ok(adapters) => adapters,
                Err(_) => continue,
            };
        if !allowed_adapters
            .iter()
            .any(|adapter| adapter.key == tool_key)
        {
            continue;
        }
        let source = PathBuf::from(&skill.central_path);
        let target = adapter
            .skills_dir()
            .join(sync_engine::target_dir_name(&source, &skill.name));
        let mode = sync_engine::sync_mode_for_tool(&adapter.key, configured_mode.as_deref());
        match sync_engine::sync_skill(&source, &target, mode) {
            Ok(actual_mode) => {
                let now = chrono::Utc::now().timestamp_millis();
                let target_record = SkillTargetRecord {
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
            Err(e) => {
                log::warn!(
                    "Failed to sync skill {} to {} for tool {}: {e}",
                    skill.id,
                    target.display(),
                    adapter.key
                );
            }
        }
    }
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

#[tauri::command]
pub async fn get_tool_status(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<ToolInfoDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let adapters = tool_adapters::all_tool_adapters(&store);
        let disabled = get_disabled_tools(&store);
        let result: Vec<ToolInfoDto> = adapters
            .into_iter()
            .map(|a| ToolInfoDto {
                key: a.key.clone(),
                display_name: a.display_name.clone(),
                installed: a.is_installed(),
                skills_dir: a.skills_dir().to_string_lossy().to_string(),
                enabled: !disabled.contains(&a.key),
                is_custom: a.is_custom,
                has_path_override: a.has_path_override(),
                project_relative_skills_dir: if a.relative_skills_dir.is_empty() {
                    None
                } else {
                    Some(a.relative_skills_dir.clone())
                },
            })
            .collect();
        Ok(result)
    })
    .await?
}

#[tauri::command]
pub async fn set_tool_enabled(
    key: String,
    enabled: bool,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
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
    .await?
}

#[tauri::command]
pub async fn set_all_tools_enabled(
    enabled: bool,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
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
    .await?
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

        let mut customs = get_custom_tools(&store);
        let custom = customs
            .iter_mut()
            .find(|c| c.key == key)
            .ok_or_else(|| AppError::not_found(format!("Custom tool not found: {key}")))?;
        custom.project_relative_skills_dir = normalized;
        set_custom_tools(&store, &customs)
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
    const OLD_KEY: &str = "clawdbot";
    const NEW_KEY: &str = "openclaw";

    let mut changed = false;

    let mut disabled = get_disabled_tools(store);
    if disabled.iter().any(|k| k == OLD_KEY) {
        for key in &mut disabled {
            if key == OLD_KEY {
                *key = NEW_KEY.to_string();
            }
        }
        disabled.sort();
        disabled.dedup();
        set_disabled_tools(store, &disabled)?;
        changed = true;
    }

    let mut custom_paths = get_custom_tool_paths(store);
    if let Some(old_path) = custom_paths.remove(OLD_KEY) {
        custom_paths.entry(NEW_KEY.to_string()).or_insert(old_path);
        set_custom_tool_paths(store, &custom_paths)?;
        changed = true;
    }

    // Backward compatibility: normalize any persisted "~" path forms.
    let mut normalized_path_changed = false;
    for value in custom_paths.values_mut() {
        if let Ok(normalized) = normalize_skills_dir_input(value) {
            if *value != normalized {
                *value = normalized;
                normalized_path_changed = true;
            }
        }
    }
    if normalized_path_changed {
        set_custom_tool_paths(store, &custom_paths)?;
        changed = true;
    }

    let custom_tools = get_custom_tools(store);
    let mut custom_tools_changed = false;
    let custom_tools = if custom_tools.iter().any(|c| c.key == OLD_KEY) {
        let has_new = custom_tools.iter().any(|c| c.key == NEW_KEY);
        let mut migrated = Vec::with_capacity(custom_tools.len());
        let mut seen_keys = std::collections::HashSet::new();
        for mut custom in custom_tools {
            if custom.key == OLD_KEY {
                if has_new {
                    continue;
                }
                custom.key = NEW_KEY.to_string();
            }
            if seen_keys.insert(custom.key.clone()) {
                migrated.push(custom);
            }
        }
        custom_tools_changed = true;
        changed = true;
        migrated
    } else {
        custom_tools
    };

    let mut normalized_customs = custom_tools;
    for custom in &mut normalized_customs {
        if let Ok(normalized) = normalize_skills_dir_input(&custom.skills_dir) {
            if custom.skills_dir != normalized {
                custom.skills_dir = normalized;
                custom_tools_changed = true;
            }
        }
    }
    if custom_tools_changed {
        set_custom_tools(store, &normalized_customs)?;
    }

    if changed
        || store
            .has_tool_key_references(OLD_KEY)
            .map_err(AppError::db)?
    {
        // Migrate historical per-tool records in DB tables only when needed.
        store
            .remap_tool_key_references(OLD_KEY, NEW_KEY)
            .map_err(AppError::db)?;
    }
    if changed {
        log::info!("Migrated legacy tool key {OLD_KEY} -> {NEW_KEY}");
    }
    Ok(())
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
