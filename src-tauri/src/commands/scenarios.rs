use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

use crate::core::{
    error::AppError,
    skill_store::{ScenarioRecord, SkillStore, SkillTargetRecord},
    sync_engine, sync_metadata, tool_adapters,
};
use std::collections::{HashMap, HashSet};

fn refresh_tray_menu_best_effort(app: &tauri::AppHandle) {
    if let Err(err) = crate::refresh_tray_menu(app) {
        log::warn!("Failed to refresh tray menu after scenario mutation: {err}");
    }
}

/// Sync a skill's files to all enabled tool adapter directories for the given scenario.
/// Only performs sync if the scenario is the currently active one.
pub(crate) fn sync_skill_to_active_scenario(
    store: &SkillStore,
    scenario_id: &str,
    skill_id: &str,
) -> Result<(), AppError> {
    if let Ok(Some(active_id)) = store.get_active_scenario_id() {
        if active_id == scenario_id {
            let adapters =
                enabled_installed_adapters_for_scenario_skill(store, scenario_id, skill_id)?;
            let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
            let Ok(Some(skill)) = store.get_skill_by_id(skill_id) else {
                return Ok(());
            };
            let source = PathBuf::from(&skill.central_path);
            let target_name = sync_engine::target_dir_name(&source, &skill.name);
            let old_targets = store.get_targets_for_skill(skill_id).unwrap_or_default();
            for adapter in &adapters {
                // Remove stale target from a previous sync if the skill name changed
                if let Some(old) = old_targets.iter().find(|t| t.tool == adapter.key) {
                    let old_path = PathBuf::from(&old.target_path);
                    if old_path != adapter.skills_dir().join(&target_name) {
                        if let Err(e) = sync_engine::remove_target(&old_path) {
                            log::warn!("Failed to remove stale target {}: {e}", old_path.display());
                        }
                        let _ = store.delete_target(skill_id, &adapter.key);
                    }
                }

                let target = adapter.skills_dir().join(&target_name);
                let mode =
                    sync_engine::sync_mode_for_tool(&adapter.key, configured_mode.as_deref());
                match sync_engine::sync_skill(&source, &target, mode) {
                    Ok(actual_mode) => {
                        let now = chrono::Utc::now().timestamp_millis();
                        let target_record = crate::core::skill_store::SkillTargetRecord {
                            id: uuid::Uuid::new_v4().to_string(),
                            skill_id: skill_id.to_string(),
                            tool: adapter.key.clone(),
                            target_path: target.to_string_lossy().to_string(),
                            mode: actual_mode.as_str().to_string(),
                            status: "ok".to_string(),
                            synced_at: Some(now),
                            last_error: None,
                        };
                        if let Err(e) = store.insert_target(&target_record) {
                            log::warn!("Failed to insert sync target for skill {skill_id}: {e}");
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to sync skill {skill_id} to {}: {e}",
                            target.display()
                        );
                    }
                }
            }
        }
    }
    Ok(())
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

#[derive(Debug, Clone)]
struct ScenarioSyncTarget {
    skill_id: String,
    tool: String,
    source: PathBuf,
    target: PathBuf,
    mode: sync_engine::SyncMode,
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
        };

        sync_metadata::with_repo_lock("create scenario", || {
            store.insert_scenario(&record)?;
            sync_metadata::write_all_from_db_unlocked(&store)
        })
        .map_err(AppError::db)?;

        if let Some(previous_id) = previous_active_id.as_deref() {
            unsync_scenario_skills(&store, previous_id)?;
        }
        store.set_active_scenario(&id).map_err(AppError::db)?;

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
        sync_metadata::with_repo_lock("update scenario", || {
            store.update_scenario(&id, &name, description.as_deref(), icon.as_deref())?;
            sync_metadata::write_all_from_db_unlocked(&store)
        })
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

        sync_metadata::with_repo_lock("delete scenario", || {
            store.delete_scenario(&id)?;
            sync_metadata::write_all_from_db_unlocked(&store)
        })
        .map_err(AppError::db)?;

        if was_active {
            let remaining = store.get_all_scenarios().map_err(AppError::db)?;
            if let Some(first) = remaining.first() {
                store.set_active_scenario(&first.id).map_err(AppError::db)?;
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

/// Apply a scenario to the default targets (all enabled agent globals).
///
/// This is the explicit user-initiated action introduced in v1.16. It performs
/// the same disk-writing work as the legacy [`switch_scenario`] command but is
/// only invoked when the user clicks "Apply to Default" — sidebar/command-palette
/// scenario clicks no longer call this.
#[tauri::command]
pub async fn apply_scenario_to_default(
    app: tauri::AppHandle,
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    apply_scenario_to_default_impl(app, id, store.inner().clone()).await
}

/// Legacy command kept for the tray menu and backward compatibility. Frontend
/// callers should use [`apply_scenario_to_default`] instead.
#[tauri::command]
pub async fn switch_scenario(
    app: tauri::AppHandle,
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    apply_scenario_to_default_impl(app, id, store.inner().clone()).await
}

async fn apply_scenario_to_default_impl(
    app: tauri::AppHandle,
    id: String,
    store: Arc<SkillStore>,
) -> Result<(), AppError> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        ensure_scenario_exists(&store, &id)?;
        let desired_targets = collect_scenario_sync_targets(&store, &id)?;

        // Remove only targets that are not also needed by the new scenario.
        if let Ok(Some(old_id)) = store.get_active_scenario_id() {
            if old_id != id {
                unsync_obsolete_scenario_targets(&store, &old_id, &desired_targets)?;
            }
        }

        // Mark this scenario as the one currently applied to default targets.
        store.set_active_scenario(&id).map_err(AppError::db)?;

        // Sync missing or stale targets for the new scenario.
        sync_desired_targets(&store, &desired_targets)?;

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
        sync_metadata::with_repo_lock("add skill to scenario", || {
            store.add_skill_to_scenario(&scenario_id, &skill_id)?;
            sync_metadata::write_all_from_db_unlocked(&store)
        })
        .map_err(AppError::db)?;

        sync_skill_to_active_scenario(&store, &scenario_id, &skill_id)?;

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
        sync_metadata::with_repo_lock("remove skill from scenario", || {
            store.remove_skill_from_scenario(&scenario_id, &skill_id)?;
            sync_metadata::write_all_from_db_unlocked(&store)
        })
        .map_err(AppError::db)?;

        // If this is the active scenario, unsync the skill
        if let Ok(Some(active_id)) = store.get_active_scenario_id() {
            if active_id == scenario_id {
                let targets = store.get_targets_for_skill(&skill_id).unwrap_or_default();
                for target in &targets {
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
        sync_metadata::with_repo_lock("reorder scenarios", || {
            store.reorder_scenarios(&ids)?;
            sync_metadata::write_all_from_db_unlocked(&store)
        })
        .map_err(AppError::db)
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
        sync_metadata::with_repo_lock("reorder scenario skills", || {
            store.reorder_scenario_skills(&scenario_id, &skill_ids)?;
            sync_metadata::write_all_from_db_unlocked(&store)
        })
        .map_err(AppError::db)
    })
    .await?
}

// ── Internal helpers ──

pub(crate) fn sync_scenario_skills(store: &SkillStore, scenario_id: &str) -> Result<(), AppError> {
    let desired_targets = collect_scenario_sync_targets(store, scenario_id)?;
    sync_desired_targets(store, &desired_targets)
}

fn collect_scenario_sync_targets(
    store: &SkillStore,
    scenario_id: &str,
) -> Result<Vec<ScenarioSyncTarget>, AppError> {
    let skills = store
        .get_skills_for_scenario(scenario_id)
        .map_err(AppError::db)?;
    let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
    let mut targets = Vec::new();

    for skill in &skills {
        let source = PathBuf::from(&skill.central_path);
        let target_name = sync_engine::target_dir_name(&source, &skill.name);
        let adapters =
            enabled_installed_adapters_for_scenario_skill(store, scenario_id, &skill.id)?;
        for adapter in &adapters {
            let target = adapter.skills_dir().join(&target_name);
            let mode = sync_engine::sync_mode_for_tool(&adapter.key, configured_mode.as_deref());
            targets.push(ScenarioSyncTarget {
                skill_id: skill.id.clone(),
                tool: adapter.key.clone(),
                source: source.clone(),
                target,
                mode,
            });
        }
    }

    Ok(targets)
}

fn sync_desired_targets(
    store: &SkillStore,
    desired_targets: &[ScenarioSyncTarget],
) -> Result<(), AppError> {
    let existing_targets: HashMap<(String, String), SkillTargetRecord> = store
        .get_all_targets()
        .map_err(AppError::db)?
        .into_iter()
        .map(|target| ((target.skill_id.clone(), target.tool.clone()), target))
        .collect();

    for desired in desired_targets {
        let key = (desired.skill_id.clone(), desired.tool.clone());
        if let Some(existing) = existing_targets.get(&key) {
            let target_path = PathBuf::from(&existing.target_path);
            if target_path != desired.target {
                if let Err(e) = sync_engine::remove_target(&target_path) {
                    log::warn!(
                        "Failed to remove stale target {}: {e}",
                        target_path.display()
                    );
                }
                if let Err(e) = store.delete_target(&desired.skill_id, &desired.tool) {
                    log::warn!(
                        "Failed to delete stale target record for skill {}, tool {}: {e}",
                        desired.skill_id,
                        desired.tool
                    );
                }
            } else if existing.mode == desired.mode.as_str()
                && existing.status == "ok"
                && sync_engine::is_target_current(&desired.source, &desired.target, desired.mode)
            {
                continue;
            }
        }

        match sync_engine::sync_skill(&desired.source, &desired.target, desired.mode) {
            Ok(actual_mode) => {
                let now = chrono::Utc::now().timestamp_millis();
                let target_record = SkillTargetRecord {
                    id: uuid::Uuid::new_v4().to_string(),
                    skill_id: desired.skill_id.clone(),
                    tool: desired.tool.clone(),
                    target_path: desired.target.to_string_lossy().to_string(),
                    mode: actual_mode.as_str().to_string(),
                    status: "ok".to_string(),
                    synced_at: Some(now),
                    last_error: None,
                };
                if let Err(e) = store.insert_target(&target_record) {
                    log::warn!(
                        "Failed to insert sync target for skill {}: {e}",
                        desired.skill_id
                    );
                }
            }
            Err(e) => {
                log::warn!(
                    "Failed to sync skill {} to {}: {e}",
                    desired.skill_id,
                    desired.target.display()
                );
            }
        }
    }

    Ok(())
}

fn unsync_obsolete_scenario_targets(
    store: &SkillStore,
    old_scenario_id: &str,
    desired_targets: &[ScenarioSyncTarget],
) -> Result<(), AppError> {
    let desired_paths: HashMap<(String, String), PathBuf> = desired_targets
        .iter()
        .map(|target| {
            (
                (target.skill_id.clone(), target.tool.clone()),
                target.target.clone(),
            )
        })
        .collect();

    let old_skill_ids = store
        .get_skill_ids_for_scenario(old_scenario_id)
        .map_err(AppError::db)?;
    for skill_id in &old_skill_ids {
        let targets = store.get_targets_for_skill(skill_id).unwrap_or_default();
        for target in &targets {
            let path = PathBuf::from(&target.target_path);
            let key = (skill_id.clone(), target.tool.clone());
            if desired_paths.get(&key) == Some(&path) {
                continue;
            }

            if let Err(e) = sync_engine::remove_target(&path) {
                log::warn!("Failed to remove sync target {}: {e}", path.display());
            }
            if let Err(e) = store.delete_target(skill_id, &target.tool) {
                log::warn!(
                    "Failed to delete target record for skill {skill_id}, tool {}: {e}",
                    target.tool
                );
            }
        }
    }

    Ok(())
}

pub(crate) fn unsync_scenario_skills(
    store: &SkillStore,
    scenario_id: &str,
) -> Result<(), AppError> {
    let skill_ids = store
        .get_skill_ids_for_scenario(scenario_id)
        .map_err(AppError::db)?;

    for skill_id in &skill_ids {
        let targets = store.get_targets_for_skill(skill_id).unwrap_or_default();
        for target in &targets {
            let path = PathBuf::from(&target.target_path);
            if let Err(e) = sync_engine::remove_target(&path) {
                log::warn!("Failed to remove sync target {}: {e}", path.display());
            }
            if let Err(e) = store.delete_target(skill_id, &target.tool) {
                log::warn!(
                    "Failed to delete target record for skill {skill_id}, tool {}: {e}",
                    target.tool
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::skill_store::SkillRecord;
    use crate::core::tool_adapters::CustomToolDef;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
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

    fn write_skill_dir(base: &std::path::Path, name: &str) -> PathBuf {
        let dir = base.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), format!("---\nname: {name}\n---\n")).unwrap();
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
    }

    #[cfg(unix)]
    #[test]
    fn switching_scenarios_keeps_overlapping_skill_target() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let source_base = tmp.path().join("central");
        let target_base = tmp.path().join("agent-skills");
        fs::create_dir_all(&source_base).unwrap();
        fs::create_dir_all(&target_base).unwrap();

        configure_single_custom_tool(&store, &target_base);

        store
            .insert_scenario(&sample_scenario("old", "Old"))
            .unwrap();
        store
            .insert_scenario(&sample_scenario("new", "New"))
            .unwrap();

        let shared_dir = write_skill_dir(&source_base, "shared");
        let old_only_dir = write_skill_dir(&source_base, "old-only");
        let new_only_dir = write_skill_dir(&source_base, "new-only");
        store
            .insert_skill(&sample_skill("shared", "shared", &shared_dir))
            .unwrap();
        store
            .insert_skill(&sample_skill("old-only", "old-only", &old_only_dir))
            .unwrap();
        store
            .insert_skill(&sample_skill("new-only", "new-only", &new_only_dir))
            .unwrap();

        store.add_skill_to_scenario("old", "shared").unwrap();
        store.add_skill_to_scenario("old", "old-only").unwrap();
        store.add_skill_to_scenario("new", "shared").unwrap();
        store.add_skill_to_scenario("new", "new-only").unwrap();

        store.set_active_scenario("old").unwrap();
        sync_scenario_skills(&store, "old").unwrap();

        let shared_target = target_base.join("shared");
        let old_only_target = target_base.join("old-only");
        let new_only_target = target_base.join("new-only");
        assert_eq!(fs::read_link(&shared_target).unwrap(), shared_dir);
        assert!(old_only_target.is_symlink());
        let shared_inode_before = fs::symlink_metadata(&shared_target).unwrap().ino();

        let desired_targets = collect_scenario_sync_targets(&store, "new").unwrap();
        unsync_obsolete_scenario_targets(&store, "old", &desired_targets).unwrap();
        store.set_active_scenario("new").unwrap();
        sync_desired_targets(&store, &desired_targets).unwrap();

        assert_eq!(fs::read_link(&shared_target).unwrap(), shared_dir);
        assert_eq!(
            fs::symlink_metadata(&shared_target).unwrap().ino(),
            shared_inode_before
        );
        assert!(!old_only_target.exists());
        assert_eq!(fs::read_link(&new_only_target).unwrap(), new_only_dir);

        let targets = store.get_all_targets().unwrap();
        assert_eq!(targets.len(), 2);
        assert!(targets
            .iter()
            .any(|target| target.skill_id == "shared" && target.tool == "test_agent"));
        assert!(targets
            .iter()
            .any(|target| target.skill_id == "new-only" && target.tool == "test_agent"));
    }

    #[test]
    fn scenario_sync_keeps_duplicate_skill_names_separate() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let source_base = tmp.path().join("central");
        let target_base = tmp.path().join("agent-skills");
        fs::create_dir_all(&source_base).unwrap();
        fs::create_dir_all(&target_base).unwrap();
        configure_single_custom_tool(&store, &target_base);
        store.set_setting("sync_mode", "copy").unwrap();

        store
            .insert_scenario(&sample_scenario("active", "Active"))
            .unwrap();

        let first_dir = write_skill_dir(&source_base, "skill123");
        let second_dir = write_skill_dir(&source_base, "skill123-2");
        fs::write(first_dir.join("unique.txt"), "first").unwrap();
        fs::write(second_dir.join("unique.txt"), "second").unwrap();

        store
            .insert_skill(&sample_skill("first", "skill123", &first_dir))
            .unwrap();
        store
            .insert_skill(&sample_skill("second", "skill123", &second_dir))
            .unwrap();
        store.add_skill_to_scenario("active", "first").unwrap();
        store.add_skill_to_scenario("active", "second").unwrap();

        sync_scenario_skills(&store, "active").unwrap();

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
