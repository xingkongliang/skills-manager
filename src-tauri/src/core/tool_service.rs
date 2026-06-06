use serde::Serialize;
use std::collections::{HashMap, HashSet};

use super::{
    error::AppError,
    skill_store::SkillStore,
    tool_adapters::{self, CustomToolDef, ToolCategory},
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
    pub has_project_path_override: bool,
    pub category: ToolCategory,
}

pub fn get_disabled_tools(store: &SkillStore) -> Vec<String> {
    store
        .get_setting("disabled_tools")
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default()
}

const DEFAULT_PRIORITY_ORDER: &[&str] = &[
    "claude_code",
    "codex",
    "grok",
    "gemini_cli",
    "cursor",
    "opencode",
    "hermes",
    "openclaw",
];

pub fn get_tool_order(store: &SkillStore) -> Vec<String> {
    store
        .get_setting("tool_order")
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default()
}

pub fn set_tool_order(store: &SkillStore, order: &[String]) -> Result<(), AppError> {
    let json = serde_json::to_string(order)
        .map_err(|e| AppError::internal(format!("Failed to serialize: {e}")))?;
    store.set_setting("tool_order", &json).map_err(AppError::db)
}

/// Merge a saved tool order with the actual list of available tool keys.
/// - Keeps saved entries in their saved order (filtering out keys that no longer exist).
/// - If saved is empty, seeds with the built-in default priority list.
/// - Slots a newly-registered priority agent into its canonical position
///   (right after the previous priority agent already present) so e.g. a new
///   built-in `grok` lands next to `codex` even for users who already have a
///   saved order, instead of being dumped at the bottom.
/// - Appends any remaining keys (non-priority new agents) at the end in their
///   natural adapter order.
fn merge_order(saved: &[String], all_keys: &[String]) -> Vec<String> {
    let all_set: HashSet<&str> = all_keys.iter().map(|s| s.as_str()).collect();
    let mut out: Vec<String> = Vec::with_capacity(all_keys.len());

    for k in saved {
        if all_set.contains(k.as_str()) && !out.iter().any(|x| x == k) {
            out.push(k.clone());
        }
    }

    if out.is_empty() {
        for k in DEFAULT_PRIORITY_ORDER {
            if all_set.contains(*k) {
                out.push((*k).to_string());
            }
        }
    }

    let mut anchor: Option<usize> = None;
    for key in DEFAULT_PRIORITY_ORDER {
        if !all_set.contains(*key) {
            continue;
        }
        match out.iter().position(|x| x == key) {
            Some(idx) => anchor = Some(idx),
            None => {
                let insert_at = anchor.map_or(0, |a| a + 1);
                out.insert(insert_at, (*key).to_string());
                anchor = Some(insert_at);
            }
        }
    }

    for k in all_keys {
        if !out.iter().any(|x| x == k) {
            out.push(k.clone());
        }
    }

    out
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

pub fn get_custom_tool_project_paths(store: &SkillStore) -> HashMap<String, String> {
    tool_adapters::custom_tool_project_paths(store)
}

pub fn set_custom_tool_project_paths(
    store: &SkillStore,
    paths: &HashMap<String, String>,
) -> Result<(), AppError> {
    let json = serde_json::to_string(paths)
        .map_err(|e| AppError::internal(format!("Failed to serialize: {e}")))?;
    store
        .set_setting("custom_tool_project_paths", &json)
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
    let project_overrides = get_custom_tool_project_paths(store);
    let infos: Vec<ToolInfo> = tool_adapters::all_tool_adapters(store)
        .into_iter()
        .map(|adapter| ToolInfo {
            key: adapter.key.clone(),
            display_name: adapter.display_name.clone(),
            installed: adapter.is_installed(),
            skills_dir: adapter.skills_dir().to_string_lossy().to_string(),
            enabled: !disabled.contains(&adapter.key),
            is_custom: adapter.is_custom,
            has_path_override: adapter.has_path_override(),
            project_relative_skills_dir: {
                let project_dir = adapter.project_relative_skills_dir();
                if project_dir.is_empty() {
                    None
                } else {
                    Some(project_dir.to_string())
                }
            },
            // Only built-in adapters have a default project path to reset back to;
            // custom tools clear their path instead of resetting.
            has_project_path_override: !adapter.is_custom
                && project_overrides.contains_key(&adapter.key),
            category: adapter.category,
        })
        .collect();

    let saved = get_tool_order(store);
    let all_keys: Vec<String> = infos.iter().map(|i| i.key.clone()).collect();
    let ordered_keys = merge_order(&saved, &all_keys);

    let mut by_key: HashMap<String, ToolInfo> = infos.into_iter().map(|i| (i.key.clone(), i)).collect();
    ordered_keys
        .into_iter()
        .filter_map(|k| by_key.remove(&k))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn v(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn fresh_install_uses_default_priority_order() {
        let all = v(&["cursor", "claude_code", "codex", "grok", "gemini_cli", "opencode"]);
        let order = merge_order(&[], &all);
        // Priority list comes first, then remaining adapters in their natural order.
        assert_eq!(order[0], "claude_code");
        assert_eq!(order[1], "codex");
        assert_eq!(order[2], "grok");
    }

    #[test]
    fn new_priority_agent_slots_after_its_predecessor() {
        // Existing user whose saved order predates `grok`.
        let saved = v(&["claude_code", "codex", "gemini_cli", "cursor", "opencode"]);
        let all = v(&["cursor", "claude_code", "codex", "grok", "gemini_cli", "opencode"]);
        let order = merge_order(&saved, &all);
        let codex = order.iter().position(|k| k == "codex").unwrap();
        assert_eq!(order[codex + 1], "grok", "grok must land right after codex");
        // Existing entries keep their relative order.
        assert!(
            order.iter().position(|k| k == "gemini_cli").unwrap()
                > order.iter().position(|k| k == "grok").unwrap()
        );
    }

    #[test]
    fn non_priority_new_agent_appends_at_end() {
        let saved = v(&["claude_code", "codex"]);
        let all = v(&["claude_code", "codex", "some_new_tool"]);
        let order = merge_order(&saved, &all);
        assert_eq!(order.last().unwrap(), "some_new_tool");
    }
}
