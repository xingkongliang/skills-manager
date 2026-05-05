use serde::Serialize;
use std::collections::{HashMap, HashSet};

use super::{
    error::AppError,
    skill_store::SkillStore,
    tool_adapters::{self, CustomToolDef},
};

#[derive(Debug, Clone, Serialize)]
pub struct ToolInfo {
    pub key: String,
    pub display_name: String,
    pub installed: bool,
    pub skills_dir: String,
    pub enabled: bool,
    pub is_custom: bool,
    pub has_path_override: bool,
    pub project_relative_skills_dir: Option<String>,
}

pub fn get_disabled_tools(store: &SkillStore) -> Vec<String> {
    store
        .get_setting("disabled_tools")
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default()
}

pub fn disabled_tools_set(store: &SkillStore) -> HashSet<String> {
    get_disabled_tools(store).into_iter().collect()
}

pub fn set_disabled_tools(store: &SkillStore, disabled: &[String]) -> Result<(), AppError> {
    let json = serde_json::to_string(disabled)
        .map_err(|e| AppError::internal(format!("Failed to serialize: {e}")))?;
    store
        .set_setting("disabled_tools", &json)
        .map_err(AppError::db)
}

pub fn get_custom_tool_paths(store: &SkillStore) -> HashMap<String, String> {
    tool_adapters::custom_tool_paths(store)
}

pub fn set_custom_tool_paths(
    store: &SkillStore,
    paths: &HashMap<String, String>,
) -> Result<(), AppError> {
    let json = serde_json::to_string(paths)
        .map_err(|e| AppError::internal(format!("Failed to serialize: {e}")))?;
    store
        .set_setting("custom_tool_paths", &json)
        .map_err(AppError::db)
}

pub fn get_custom_tools(store: &SkillStore) -> Vec<CustomToolDef> {
    tool_adapters::custom_tools(store)
}

pub fn set_custom_tools(store: &SkillStore, custom_tools: &[CustomToolDef]) -> Result<(), AppError> {
    let json = serde_json::to_string(custom_tools)
        .map_err(|e| AppError::internal(format!("Failed to serialize: {e}")))?;
    store
        .set_setting("custom_tools", &json)
        .map_err(AppError::db)
}

pub fn normalize_skills_dir_input(path: &str) -> Result<String, AppError> {
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

pub fn normalize_project_relative_skills_dir_input(path: &str) -> Result<Option<String>, AppError> {
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

pub fn list_tool_info(store: &SkillStore) -> Vec<ToolInfo> {
    let disabled = disabled_tools_set(store);
    tool_adapters::all_tool_adapters(store)
        .into_iter()
        .map(|adapter| ToolInfo {
            key: adapter.key.clone(),
            display_name: adapter.display_name.clone(),
            installed: adapter.is_installed(),
            skills_dir: adapter.skills_dir().to_string_lossy().to_string(),
            enabled: !disabled.contains(&adapter.key),
            is_custom: adapter.is_custom,
            has_path_override: adapter.has_path_override(),
            project_relative_skills_dir: if adapter.relative_skills_dir.is_empty() {
                None
            } else {
                Some(adapter.relative_skills_dir.clone())
            },
        })
        .collect()
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

    if changed || store.has_tool_key_references(OLD_KEY).map_err(AppError::db)? {
        store
            .remap_tool_key_references(OLD_KEY, NEW_KEY)
            .map_err(AppError::db)?;
    }
    if changed {
        log::info!("Migrated legacy tool key {OLD_KEY} -> {NEW_KEY}");
    }
    Ok(())
}
