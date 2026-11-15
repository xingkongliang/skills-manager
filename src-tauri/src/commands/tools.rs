use serde::Serialize;
use std::path::{Path, PathBuf};
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
    release_tool_targets(store, tool_key, None);
}

/// Drop every target record of `tool_key`. With `new_dir`, the tool has just
/// moved there: deployments outside it sit at a path the tool no longer reads
/// and are released (copies moved into the library) rather than removed. Without it, the user is
/// taking the skills away from the tool, so they are removed.
fn release_tool_targets(store: &SkillStore, tool_key: &str, new_dir: Option<&Path>) {
    let targets = store.get_all_targets().unwrap_or_default();
    for target in targets.iter().filter(|t| t.tool == tool_key) {
        // Tools can share one skills directory; another tool's live
        // deployment at this exact path is not ours to remove.
        let still_referenced = targets
            .iter()
            .any(|other| other.tool != tool_key && other.target_path == target.target_path);
        if !still_referenced {
            let path = PathBuf::from(&target.target_path);
            match new_dir {
                Some(dir) if path.parent() != Some(dir) => {
                    scenario_service::release_abandoned_target(&path, &target.mode, tool_key);
                }
                _ => {
                    sync_engine::remove_recorded_target_or_warn(&path, &target.mode);
                }
            }
        }
        store.delete_target(&target.skill_id, tool_key).ok();
    }
}

fn reconcile_tool_sync_after_path_change(store: &SkillStore, tool_key: &str) {
    // Release the deployments at the old path, then re-sync to the current one.
    let new_dir = tool_adapters::find_adapter_with_store(store, tool_key).map(|a| a.skills_dir());
    release_tool_targets(store, tool_key, new_dir.as_deref());
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
        set_tool_enabled_internal(&store, &key, enabled)
    })
    .await?;
    if result.is_ok() {
        refresh_tray_menu_best_effort(&app);
    }
    result
}

/// Shared GUI/CLI implementation for the global agent toggle.
pub fn set_tool_enabled_internal(
    store: &SkillStore,
    key: &str,
    enabled: bool,
) -> Result<(), AppError> {
    if tool_adapters::find_adapter_with_store(store, key).is_none() {
        return Err(AppError::not_found(format!("Unknown agent: {key}")));
    }

    let mut disabled = get_disabled_tools(store);
    if enabled {
        disabled.retain(|item| item != key);
        set_disabled_tools(store, &disabled)?;
        sync_active_scenario_to_tool(store, key);
        Ok(())
    } else {
        if !disabled.iter().any(|item| item == key) {
            disabled.push(key.to_string());
        }
        unsync_all_for_tool(store, key);
        set_disabled_tools(store, &disabled)
    }
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
                // Enabling the tools succeeded either way; a target we may not
                // replace is reported, not fatal (#363).
                match sync_scenario_skills(&store, &active_id) {
                    Ok(refusals) => {
                        for refusal in refusals {
                            log::warn!("enable-all-tools sync skipped a target: {refusal}");
                        }
                    }
                    Err(e) => log::warn!("enable-all-tools sync failed: {e}"),
                }
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

/// Whether a built-in adapter claims this key. Resolution consults built-ins
/// before custom tools, so a key that answers `true` here must have its
/// overrides written to the side maps, never onto a custom tool definition.
fn is_builtin_key(key: &str) -> bool {
    tool_adapters::default_tool_adapters()
        .iter()
        .any(|a| a.key == key)
}

/// Store side of [`set_custom_tool_path`], separated so the write can be
/// tested against a real store without a Tauri runtime.
pub(crate) fn apply_tool_skills_dir(
    store: &SkillStore,
    key: &str,
    path: &str,
) -> Result<(), AppError> {
    let key = key.trim().to_string();
    let path = normalize_skills_dir_input(path)?;
    if key.is_empty() || path.is_empty() {
        return Err(AppError::invalid_input("Key and path are required"));
    }

    let old_adapter = tool_adapters::find_adapter_with_store(store, &key)
        .ok_or_else(|| AppError::not_found(format!("Unknown tool: {key}")))?;
    let old_skills_dir = old_adapter.skills_dir();

    // Resolution prefers a built-in over a custom tool of the same key
    // (`find_adapter_with_store`), so the write has to agree. A custom agent
    // whose key later became a built-in one otherwise stores the new path on a
    // definition nothing reads, and the save reports success while the
    // displayed path never moves (#378).
    let mut customs = get_custom_tools(store);
    match customs.iter_mut().find(|c| c.key == key) {
        Some(custom) if !is_builtin_key(&key) => {
            custom.skills_dir = path;
            set_custom_tools(store, &customs)?;
        }
        _ => {
            let mut paths = get_custom_tool_paths(store);
            paths.insert(key.clone(), path);
            set_custom_tool_paths(store, &paths)?;
        }
    }

    let new_adapter = tool_adapters::find_adapter_with_store(store, &key)
        .ok_or_else(|| AppError::not_found(format!("Unknown tool: {key}")))?;
    if old_skills_dir != new_adapter.skills_dir() {
        reconcile_tool_sync_after_path_change(store, &key);
    }
    Ok(())
}

#[tauri::command]
pub async fn set_custom_tool_path(
    key: String,
    path: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || apply_tool_skills_dir(&store, &key, &path)).await?
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

/// Store side of [`set_custom_tool_project_path`], separated so the write can
/// be tested against a real store without a Tauri runtime.
pub(crate) fn apply_tool_project_skills_dir(
    store: &SkillStore,
    key: &str,
    project_relative_skills_dir: Option<&str>,
) -> Result<(), AppError> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err(AppError::invalid_input("Key is required"));
    }
    let normalized = normalize_project_relative_skills_dir_input(
        project_relative_skills_dir.unwrap_or_default(),
    )?;

    // Built-in tools keep overrides in a side map keyed by tool key. Resolve
    // the built-in default project path (no store overrides) to validate the
    // key and to detect no-op edits: an empty value, or one equal to the
    // default, removes the override and restores the default.
    //
    // Checked before custom tools, matching how resolution reads them back
    // (#378) — see `apply_tool_skills_dir`.
    let default_project_path = match tool_adapters::default_tool_adapters()
        .into_iter()
        .find(|a| a.key == key)
        .map(|a| a.project_relative_skills_dir().to_string())
    {
        Some(default_project_path) => default_project_path,
        // Custom tools store the project path on their definition; clearing it
        // (None) drops project-workspace support for that agent.
        None => {
            let mut customs = get_custom_tools(store);
            let custom = customs
                .iter_mut()
                .find(|c| c.key == key)
                .ok_or_else(|| AppError::not_found(format!("Unknown tool: {key}")))?;
            custom.project_relative_skills_dir = normalized;
            return set_custom_tools(store, &customs);
        }
    };
    let mut project_paths = get_custom_tool_project_paths(store);
    match normalized {
        Some(path) if path != default_project_path => {
            project_paths.insert(key, path);
        }
        _ => {
            project_paths.remove(&key);
        }
    }
    set_custom_tool_project_paths(store, &project_paths)
}

#[tauri::command]
pub async fn set_custom_tool_project_path(
    key: String,
    project_relative_skills_dir: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        apply_tool_project_skills_dir(&store, &key, project_relative_skills_dir.as_deref())
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
        add_custom_tool_internal(
            &store,
            &key,
            &display_name,
            &skills_dir,
            project_relative_skills_dir.as_deref(),
        )
    })
    .await?
}

/// Shared by the app and the CLI (`agents add-custom`, #501).
pub fn add_custom_tool_internal(
    store: &SkillStore,
    key: &str,
    display_name: &str,
    skills_dir: &str,
    project_relative_skills_dir: Option<&str>,
) -> Result<(), AppError> {
    let key = key.trim().to_string();
    let display_name = display_name.trim().to_string();
    let skills_dir = normalize_skills_dir_input(skills_dir)?;
    let project_relative_skills_dir = normalize_project_relative_skills_dir_input(
        project_relative_skills_dir.unwrap_or_default(),
    )?;
    if key.is_empty() || display_name.is_empty() || skills_dir.is_empty() {
        return Err(AppError::invalid_input(
            "Agent key, name and skills path are required",
        ));
    }

    // Validate key uniqueness
    let all = tool_adapters::all_tool_adapters(store);
    if all.iter().any(|a| a.key == key) {
        return Err(AppError::invalid_input(format!(
            "Agent key \"{key}\" already exists"
        )));
    }
    let mut customs = get_custom_tools(store);
    customs.push(CustomToolDef {
        key: key.clone(),
        display_name,
        skills_dir,
        project_relative_skills_dir,
        category: Default::default(),
    });
    set_custom_tools(store, &customs)?;
    reconcile_tool_sync_after_path_change(store, &key);
    Ok(())
}

#[tauri::command]
pub async fn remove_custom_tool(
    key: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Remove synced targets for this tool
        unsync_all_for_tool(&store, &key);
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
    use crate::core::central_repo;
    use crate::core::skill_store::{ScenarioRecord, SkillRecord, SkillTargetRecord};
    use std::fs;
    use tempfile::tempdir;

    /// #378: a user names a custom agent after one that later ships built-in.
    /// The key is derived from the display name, so "DeepSeek Harness" becomes
    /// `deepseek_harness` — and the collision check at creation time only knew
    /// the keys that existed then. Resolution now prefers the built-in, so a
    /// write that landed on the custom definition was stored where nothing
    /// reads it: the save reported success and the path never moved.
    fn store_with_colliding_custom_tool(dir: &std::path::Path, key: &str) -> SkillStore {
        let store = SkillStore::new(&dir.join("test.db")).unwrap();
        let customs = vec![CustomToolDef {
            key: key.to_string(),
            display_name: "DeepSeek Harness".to_string(),
            skills_dir: "/tmp/whatever-they-had".to_string(),
            project_relative_skills_dir: Some(".old/skills".to_string()),
            category: ToolCategory::Coding,
        }];
        store
            .set_setting("custom_tools", &serde_json::to_string(&customs).unwrap())
            .unwrap();
        store
    }

    #[test]
    fn a_path_edit_survives_a_custom_tool_shadowed_by_a_builtin() {
        let tmp = tempdir().unwrap();
        let store = store_with_colliding_custom_tool(tmp.path(), "deepseek_harness");
        let chosen = tmp.path().join("chosen");

        apply_tool_skills_dir(
            &store,
            "deepseek_harness",
            &chosen.to_string_lossy(),
        )
        .unwrap();

        let adapter = tool_adapters::find_adapter_with_store(&store, "deepseek_harness").unwrap();
        assert_eq!(adapter.skills_dir(), chosen, "the edit must be what resolves");
        assert!(adapter.has_path_override());
    }

    #[test]
    fn a_project_path_edit_survives_the_same_shadowing() {
        let tmp = tempdir().unwrap();
        let store = store_with_colliding_custom_tool(tmp.path(), "deepseek_harness");

        apply_tool_project_skills_dir(&store, "deepseek_harness", Some(".mine/skills")).unwrap();

        let adapter = tool_adapters::find_adapter_with_store(&store, "deepseek_harness").unwrap();
        assert_eq!(adapter.project_relative_skills_dir(), ".mine/skills");
    }

    /// A genuine custom agent — no built-in claims its key — still keeps both
    /// paths on its own definition, which is where its adapter reads them.
    #[test]
    fn a_real_custom_tool_still_stores_its_paths_on_its_definition() {
        let tmp = tempdir().unwrap();
        let store = store_with_colliding_custom_tool(tmp.path(), "my_own_agent");
        let chosen = tmp.path().join("chosen");

        apply_tool_skills_dir(&store, "my_own_agent", &chosen.to_string_lossy()).unwrap();
        apply_tool_project_skills_dir(&store, "my_own_agent", Some(".mine/skills")).unwrap();

        let customs = get_custom_tools(&store);
        let custom = customs.iter().find(|c| c.key == "my_own_agent").unwrap();
        assert_eq!(std::path::Path::new(&custom.skills_dir), chosen);
        assert_eq!(
            custom.project_relative_skills_dir.as_deref(),
            Some(".mine/skills")
        );

        let adapter = tool_adapters::find_adapter_with_store(&store, "my_own_agent").unwrap();
        assert_eq!(adapter.skills_dir(), chosen);
        assert_eq!(adapter.project_relative_skills_dir(), ".mine/skills");
    }

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

    /// Insert a minimal skill row (the `skill_targets` foreign key needs it)
    /// plus one target row pointing at `target_path`.
    fn insert_skill_and_target(
        store: &SkillStore,
        target_path: &std::path::Path,
        tool: &str,
        mode: &str,
    ) {
        store
            .insert_skill(&SkillRecord {
                id: "s1".to_string(),
                name: "my-skill".to_string(),
                description: None,
                source_type: "import".to_string(),
                source_ref: None,
                source_ref_resolved: None,
                source_subpath: None,
                source_branch: None,
                source_revision: None,
                remote_revision: None,
                central_path: "unused".to_string(),
                content_hash: None,
                enabled: true,
                created_at: 1,
                updated_at: 1,
                status: "ok".to_string(),
                update_status: "local_only".to_string(),
                last_checked_at: None,
                last_check_error: None,
            })
            .unwrap();
        store
            .insert_target(&SkillTargetRecord {
                id: "t1".to_string(),
                skill_id: "s1".to_string(),
                tool: tool.to_string(),
                target_path: target_path.to_string_lossy().to_string(),
                mode: mode.to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: None,
            })
            .unwrap();
    }

    /// #435: a `skill_targets` row is a statement about the past. When the
    /// user replaced our symlink with a real directory of their own,
    /// disabling the tool must preserve it — the row only authorizes
    /// removing what still looks like what we deployed.
    #[test]
    fn disabling_a_tool_preserves_user_content_that_replaced_a_recorded_link() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let target = tmp.path().join("agent-skills").join("my-skill");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("mine.txt"), "DO_NOT_OVERWRITE").unwrap();
        insert_skill_and_target(&store, &target, "test_agent", "symlink");

        unsync_all_for_tool(&store, "test_agent");

        assert_eq!(
            fs::read_to_string(target.join("mine.txt")).unwrap(),
            "DO_NOT_OVERWRITE",
            "the user's directory must survive the disable"
        );
        assert!(
            store.get_targets_for_skill("s1").unwrap().is_empty(),
            "the stale record must not survive the disable"
        );
    }

    /// A copy-mode row still vouches for a real directory — that is what a
    /// copy deployment is — so disabling the tool removes it. Guards #435's
    /// fix against overcorrecting into refusing every real directory.
    #[test]
    fn disabling_a_tool_still_removes_a_recorded_copy_deployment() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let target = tmp.path().join("agent-skills").join("my-skill");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "---\nname: my-skill\n---\n").unwrap();
        insert_skill_and_target(&store, &target, "test_agent", "copy");

        unsync_all_for_tool(&store, "test_agent");

        assert!(
            !target.exists(),
            "a recorded copy deployment is ours to remove"
        );
        assert!(store.get_targets_for_skill("s1").unwrap().is_empty());
    }

    /// Two tools resolving to one skills directory share the deployment at a
    /// path. Disabling one must leave the other's copy in place.
    #[test]
    fn disabling_a_tool_keeps_a_deployment_another_tool_shares() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let target = tmp.path().join("agent-skills").join("my-skill");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "---\nname: my-skill\n---\n").unwrap();
        insert_skill_and_target(&store, &target, "agent_a", "copy");
        store
            .insert_target(&SkillTargetRecord {
                id: "t2".to_string(),
                skill_id: "s1".to_string(),
                tool: "agent_b".to_string(),
                target_path: target.to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: None,
            })
            .unwrap();

        unsync_all_for_tool(&store, "agent_a");

        assert!(target.exists(), "agent_b still deploys this path");
        let remaining = store.get_targets_for_skill("s1").unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].tool, "agent_b");
    }

    /// One skill in the active preset, deployed to `old_base` with `mode`. A
    /// copy deployment also gets a file the user added to it there. The
    /// library lives in the temp dir, since kept copies are moved into it.
    struct MovedTool {
        _tmp: tempfile::TempDir,
        _lock: std::sync::MutexGuard<'static, ()>,
        store: SkillStore,
        old_base: PathBuf,
        old_copy: PathBuf,
        new_base: PathBuf,
        kept_root: PathBuf,
    }

    impl Drop for MovedTool {
        fn drop(&mut self) {
            central_repo::set_test_base_dir_override(None);
        }
    }

    fn deploy_before_the_move(mode: &str) -> MovedTool {
        let lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        central_repo::set_test_base_dir_override(Some(tmp.path().join("library")));
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let source_base = tmp.path().join("central");
        let old_base = tmp.path().join("old-skills");
        let new_base = tmp.path().join("new-skills");
        for dir in [&source_base, &old_base, &new_base] {
            fs::create_dir_all(dir).unwrap();
        }
        configure_single_custom_tool(&store, &old_base);
        store.set_setting("sync_mode", mode).unwrap();
        store
            .insert_scenario(&sample_scenario("active", "Active"))
            .unwrap();
        store.set_active_scenario("active").unwrap();
        let source = write_skill_dir(&source_base, "skill-a", "library");
        store
            .insert_skill(&sample_skill("a", "skill-a", &source))
            .unwrap();
        store.add_skill_to_scenario("active", "a").unwrap();
        sync_active_scenario_to_tool(&store, "test_agent");

        let old_copy = old_base.join("skill-a");
        if mode == "copy" {
            fs::write(old_copy.join("notes.md"), "edited by the user").unwrap();
        }
        let kept_root = tmp.path().join("library/kept-copies/test_agent");
        MovedTool {
            _tmp: tmp,
            _lock: lock,
            store,
            old_base,
            old_copy,
            new_base,
            kept_root,
        }
    }

    /// The tool's directory changes underneath the records, as when an app
    /// update moves a built-in adapter (#593), without the settings-page
    /// reconcile running.
    fn move_tool_without_reconcile(moved: &MovedTool, mode: &str) {
        configure_single_custom_tool(&moved.store, &moved.new_base);
        moved.store.set_setting("sync_mode", mode).unwrap();
    }

    fn assert_kept_and_redeployed(moved: &MovedTool) {
        assert_eq!(
            fs::read_to_string(moved.kept_root.join("skill-a/notes.md")).unwrap(),
            "edited by the user",
            "the edited copy was not kept in the library"
        );
        // Moved out, not left behind: another agent may read the old
        // directory, and moving back must not find it in the way.
        assert!(
            !moved.old_copy.exists(),
            "the copy was left at the old path"
        );
        assert_eq!(
            fs::read_to_string(moved.new_base.join("skill-a/unique.txt")).unwrap(),
            "library",
            "the skill was not deployed to the new path"
        );
        let targets = moved.store.get_targets_for_skill("a").unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(
            PathBuf::from(&targets[0].target_path),
            moved.new_base.join("skill-a")
        );
    }

    /// #593: changing a tool's skills directory in Settings must not delete
    /// the copy left at the old path; the user may have edited it there.
    #[test]
    fn moving_a_tool_keeps_an_edited_copy_at_the_old_path() {
        let moved = deploy_before_the_move("copy");

        apply_tool_skills_dir(
            &moved.store,
            "test_agent",
            &moved.new_base.to_string_lossy(),
        )
        .unwrap();

        assert_kept_and_redeployed(&moved);
    }

    /// Only copies are kept. A link at the old path carries no data of its
    /// own, so it is still removed.
    #[test]
    fn moving_a_tool_still_removes_a_link_at_the_old_path() {
        let moved = deploy_before_the_move("symlink");
        assert!(
            fs::symlink_metadata(&moved.old_copy)
                .unwrap()
                .file_type()
                .is_symlink(),
            "fixture: expected a link deployment"
        );

        apply_tool_skills_dir(
            &moved.store,
            "test_agent",
            &moved.new_base.to_string_lossy(),
        )
        .unwrap();

        assert!(
            fs::symlink_metadata(&moved.old_copy).is_err(),
            "the link at the old path was left behind"
        );
        assert!(moved.new_base.join("skill-a/unique.txt").exists());
    }

    /// The next full sync after the adapter moved (`sync_desired_targets`).
    #[test]
    fn a_full_sync_after_the_tool_moved_keeps_the_old_copy() {
        let moved = deploy_before_the_move("copy");
        move_tool_without_reconcile(&moved, "copy");

        scenario_service::sync_scenario_skills(&moved.store, "active").unwrap();

        assert_kept_and_redeployed(&moved);
    }

    /// Re-syncing one skill after the adapter moved
    /// (`sync_skill_to_active_scenario`).
    #[test]
    fn a_single_skill_sync_after_the_tool_moved_keeps_the_old_copy() {
        let moved = deploy_before_the_move("copy");
        move_tool_without_reconcile(&moved, "copy");

        scenario_service::sync_skill_to_active_scenario(&moved.store, "active", "a").unwrap();

        assert_kept_and_redeployed(&moved);
    }

    /// Switching presets after the adapter moved: a skill the new preset still
    /// deploys keeps its old copy, but one the new preset drops is removed,
    /// because the user is taking it away from the tool.
    #[test]
    fn switching_presets_after_the_tool_moved_keeps_only_skills_still_deployed() {
        let moved = deploy_before_the_move("copy");
        let source_b = write_skill_dir(&moved._tmp.path().join("central"), "skill-b", "b");
        moved
            .store
            .insert_skill(&sample_skill("b", "skill-b", &source_b))
            .unwrap();
        moved.store.add_skill_to_scenario("active", "b").unwrap();
        sync_active_scenario_to_tool(&moved.store, "test_agent");
        // Re-syncing at the same path replaces the copy, so the user's edit
        // has to come after it. That replacement is a separate, known gap.
        fs::write(moved.old_copy.join("notes.md"), "edited by the user").unwrap();
        let old_b = moved.old_copy.with_file_name("skill-b");
        assert!(old_b.join("unique.txt").exists(), "fixture: b not deployed");

        moved
            .store
            .insert_scenario(&sample_scenario("next", "Next"))
            .unwrap();
        moved.store.add_skill_to_scenario("next", "a").unwrap();
        move_tool_without_reconcile(&moved, "copy");

        scenario_service::apply_scenario_to_default(&moved.store, "next").unwrap();

        assert_kept_and_redeployed(&moved);
        assert!(
            !old_b.exists(),
            "a skill the new preset drops must be removed"
        );
    }

    /// Moving a tool and then moving it back: the first move must not leave
    /// an unrecorded directory at the original path, or the way back refuses
    /// to deploy there while the save reports success.
    #[test]
    fn moving_a_tool_away_and_back_redeploys_at_the_original_path() {
        let moved = deploy_before_the_move("copy");
        let new_base = moved.new_base.to_string_lossy().to_string();
        let old_base = moved.old_base.to_string_lossy().to_string();

        apply_tool_skills_dir(&moved.store, "test_agent", &new_base).unwrap();
        apply_tool_skills_dir(&moved.store, "test_agent", &old_base).unwrap();

        assert_eq!(
            fs::read_to_string(moved.old_copy.join("unique.txt")).unwrap(),
            "library",
            "the skill was not redeployed at the original path"
        );
        let targets = moved.store.get_targets_for_skill("a").unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(PathBuf::from(&targets[0].target_path), moved.old_copy);
        assert_eq!(
            fs::read_to_string(moved.kept_root.join("skill-a/notes.md")).unwrap(),
            "edited by the user"
        );
    }

    /// A copy deployed from the library's current content, with the hash
    /// recorded as a real install records it, then edited in the agent's folder.
    fn edited_copy() -> MovedTool {
        let deployed = deploy_before_the_move("copy");
        set_library_hash(&deployed);
        sync_active_scenario_to_tool(&deployed.store, "test_agent");
        fs::write(
            deployed.old_copy.join("SKILL.md"),
            "edited in the agent's copy",
        )
        .unwrap();
        deployed
    }

    fn set_library_hash(deployed: &MovedTool) {
        let skill = deployed.store.get_skill_by_id("a").unwrap().unwrap();
        let hash =
            crate::core::content_hash::hash_directory(Path::new(&skill.central_path)).unwrap();
        deployed
            .store
            .update_skill_after_install("a", "skill-a", None, None, None, Some(&hash), "local_only")
            .unwrap();
    }

    fn copy_still_edited(deployed: &MovedTool) -> bool {
        fs::read_to_string(deployed.old_copy.join("SKILL.md")).unwrap()
            == "edited in the agent's copy"
    }

    type Resync = fn(&SkillStore) -> Result<(), AppError>;

    /// Every way the app re-syncs one skill to an agent that already has it.
    const RESYNCS: [(&str, Resync); 5] = [
        ("full sync", |store| {
            scenario_service::sync_scenario_skills(store, "active").map(drop)
        }),
        ("single skill to the active preset", |store| {
            scenario_service::sync_skill_to_active_scenario(store, "active", "a")
        }),
        ("single skill to one agent", |store| {
            scenario_service::sync_single_skill_to_tool(
                store,
                "a",
                "test_agent",
                scenario_service::DeployIntent::Managed,
            )
        }),
        ("batch deploy", |store| {
            scenario_service::apply_skills_to_tools(
                store,
                &["a".to_string()],
                &["test_agent".to_string()],
                scenario_service::BatchApplyMode::Add,
            )
        }),
        ("copies after an update", |store| {
            crate::commands::skills::resync_copy_targets(store, "a")
        }),
    ];

    /// Applying a preset from the PresetBar, a batch deploy or the sync after
    /// an install rewrote the copy and deleted edits made in it, although the
    /// library had not changed (F1's third case).
    #[test]
    fn a_resync_leaves_an_edited_copy_alone_while_the_library_is_unchanged() {
        for (name, resync) in RESYNCS {
            let deployed = edited_copy();
            resync(&deployed.store).unwrap();
            assert!(
                copy_still_edited(&deployed),
                "{name} overwrote the edited copy"
            );
        }
    }

    /// The user swapped the copy for a link to the library. That serves the
    /// same content, so the link is left as it is, as `main` did.
    #[cfg(unix)]
    #[test]
    fn a_link_to_the_library_in_place_of_the_copy_is_left_alone() {
        for (name, resync) in RESYNCS {
            let deployed = edited_copy();
            let skill = deployed.store.get_skill_by_id("a").unwrap().unwrap();
            fs::remove_dir_all(&deployed.old_copy).unwrap();
            std::os::unix::fs::symlink(&skill.central_path, &deployed.old_copy).unwrap();

            resync(&deployed.store).unwrap();

            assert!(
                fs::symlink_metadata(&deployed.old_copy)
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "{name} replaced the user's link with a copy"
            );
        }
    }

    /// A copy is current only while it is still a directory (or a link to the
    /// library). A file the user put in its place stays, and deploying over it
    /// is refused.
    #[test]
    fn a_file_in_place_of_the_copy_is_not_taken_for_a_current_copy() {
        for (name, resync) in RESYNCS {
            let deployed = edited_copy();
            fs::remove_dir_all(&deployed.old_copy).unwrap();
            fs::write(&deployed.old_copy, "the user's file").unwrap();

            let result = resync(&deployed.store);

            assert_eq!(
                fs::read_to_string(&deployed.old_copy).unwrap(),
                "the user's file",
                "{name} replaced the user's file"
            );
            if name == "single skill to one agent" {
                assert!(
                    result.is_err(),
                    "{name} reported success over the user's file"
                );
            }
        }
    }

    #[test]
    fn a_resync_still_delivers_a_library_change() {
        for (name, resync) in RESYNCS {
            let deployed = edited_copy();
            let skill = deployed.store.get_skill_by_id("a").unwrap().unwrap();
            fs::write(
                Path::new(&skill.central_path).join("unique.txt"),
                "library v2",
            )
            .unwrap();
            set_library_hash(&deployed);

            resync(&deployed.store).unwrap();

            assert_eq!(
                fs::read_to_string(deployed.old_copy.join("unique.txt")).unwrap(),
                "library v2",
                "{name} did not deliver the library change"
            );
        }
    }
}
