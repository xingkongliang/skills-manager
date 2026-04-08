use rayon::prelude::*;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, State};

use crate::core::{
    error::AppError,
    skill_store::{ScenarioRecord, SkillStore, SkillTargetRecord},
    sync_engine, tool_adapters,
};
use std::collections::{HashMap, HashSet};

fn refresh_tray_menu_best_effort(app: &tauri::AppHandle) {
    if let Err(err) = crate::refresh_tray_menu(app) {
        log::warn!("Failed to refresh tray menu after scenario mutation: {err}");
    }
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
        let t_total = std::time::Instant::now();
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
        log::info!("[get_scenarios] TOTAL ({} scenarios): {:?}", result.len(), t_total.elapsed());
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
        let t_total = std::time::Instant::now();
        let active_id = store.get_active_scenario_id().map_err(AppError::db)?;

        if let Some(id) = active_id {
            let scenarios = store.get_all_scenarios().map_err(AppError::db)?;
            if let Some(s) = scenarios.into_iter().find(|s| s.id == id) {
                let count = store.count_skills_for_scenario(&s.id).unwrap_or(0);
                log::info!("[get_active_scenario] TOTAL: {:?}", t_total.elapsed());
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
        log::info!("[get_active_scenario] TOTAL (no active): {:?}", t_total.elapsed());
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
            prompt_template: None,
            created_at: now,
            updated_at: now,
        };

        store.insert_scenario(&record).map_err(AppError::db)?;

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
        store
            .update_scenario(&id, &name, description.as_deref(), icon.as_deref())
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

        store.delete_scenario(&id).map_err(AppError::db)?;

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

#[tauri::command]
pub async fn switch_scenario(
    app: tauri::AppHandle,
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let store_bg = store.clone();
    let id_bg = id.clone();

    // Check if global sync scope — skip file operations if so
    let is_global = store
        .get_setting("skill_sync_scope")
        .unwrap_or(None)
        .map_or(false, |v| v == "global");

    // Phase 1: DB-only operations (fast) — synchronous
    tauri::async_runtime::spawn_blocking(move || {
        ensure_scenario_exists(&store, &id)?;

        if !is_global {
            // Unsync old scenario skills — file removal
            if let Ok(Some(old_id)) = store.get_active_scenario_id() {
                if old_id != id {
                    let t0 = std::time::Instant::now();
                    unsync_scenario_skills(&store, &old_id)?;
                    log::info!("unsync_scenario_skills took {:?}", t0.elapsed());
                }
            }
        }

        // Set new active scenario in DB
        store.set_active_scenario(&id).map_err(AppError::db)?;

        Ok::<(), AppError>(())
    })
    .await??;

    // Phase 2: File sync in background — don't block the UI
    // In global mode, all skills are already synced — skip file operations
    let app_bg = app.clone();
    if !is_global {
        tauri::async_runtime::spawn(async move {
            tauri::async_runtime::spawn_blocking(move || {
                let t0 = std::time::Instant::now();
                if let Err(e) = sync_scenario_skills(&store_bg, &id_bg) {
                    log::error!("Background sync_scenario_skills failed: {e}");
                }
                log::info!("sync_scenario_skills (background) took {:?}", t0.elapsed());
            })
            .await
            .ok();
            // Notify frontend that file sync is done so it can refresh targets
            if let Err(e) = app_bg.emit("scenario-sync-complete", ()) {
                log::warn!("Failed to emit scenario-sync-complete: {e}");
            }
        });
    } else {
        // Global mode: just notify frontend to refresh (scenario metadata changed)
        if let Err(e) = app_bg.emit("scenario-sync-complete", ()) {
            log::warn!("Failed to emit scenario-sync-complete: {e}");
        }
    }

    refresh_tray_menu_best_effort(&app);
    Ok(())
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
        store
            .add_skill_to_scenario(&scenario_id, &skill_id)
            .map_err(AppError::db)?;

        // If this is the active scenario AND not in global sync scope, sync the skill
        let is_global = store
            .get_setting("skill_sync_scope")
            .unwrap_or(None)
            .map_or(false, |v| v == "global");

        if !is_global {
            if let Ok(Some(active_id)) = store.get_active_scenario_id() {
                if active_id == scenario_id {
                    let adapters =
                        enabled_installed_adapters_for_scenario_skill(&store, &scenario_id, &skill_id)?;
                    let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
                    if let Ok(Some(skill)) = store.get_skill_by_id(&skill_id) {
                        let source = PathBuf::from(&skill.central_path);
                        for adapter in &adapters {
                            let target = adapter.skills_dir().join(&skill.name);
                            let mode = sync_engine::sync_mode_for_tool(
                                &adapter.key,
                                configured_mode.as_deref(),
                            );
                            match sync_engine::sync_skill(&source, &target, mode) {
                                Ok(_actual_mode) => {
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
                                    if let Err(e) = store.insert_target(&target_record) {
                                        log::warn!(
                                            "Failed to insert sync target for skill {skill_id}: {e}"
                                        );
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
pub async fn remove_skill_from_scenario(
    app: tauri::AppHandle,
    skill_id: String,
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        store
            .remove_skill_from_scenario(&scenario_id, &skill_id)
            .map_err(AppError::db)?;

        // If this is the active scenario AND not in global sync scope, unsync the skill
        let is_global = store
            .get_setting("skill_sync_scope")
            .unwrap_or(None)
            .map_or(false, |v| v == "global");

        if !is_global {
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
        store.reorder_scenarios(&ids).map_err(AppError::db)
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
        store
            .reorder_scenario_skills(&scenario_id, &skill_ids)
            .map_err(AppError::db)
    })
    .await?
}

// ── Internal helpers ──

pub(crate) fn sync_scenario_skills(store: &SkillStore, scenario_id: &str) -> Result<(), AppError> {
    let t_total = std::time::Instant::now();

    let skills = store
        .get_skills_for_scenario(scenario_id)
        .map_err(AppError::db)?;
    if skills.is_empty() {
        return Ok(());
    }

    let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;

    // 1. Pre-compute: get all enabled & installed adapters once
    let t0 = std::time::Instant::now();
    let all_adapters = tool_adapters::enabled_installed_adapters(store);
    let adapter_keys: Vec<String> = all_adapters.iter().map(|a| a.key.clone()).collect();
    log::info!("[sync] step1 adapters ({} enabled): {:?}", all_adapters.len(), t0.elapsed());

    // Pre-create adapter skills directories to avoid repeated create_dir_all calls
    let t0 = std::time::Instant::now();
    for adapter in &all_adapters {
        let skills_dir = adapter.skills_dir();
        if let Some(parent) = skills_dir.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    log::info!("[sync] pre-create adapter dirs: {:?}", t0.elapsed());

    // 2. Batch ensure defaults for all skills in one transaction
    let t0 = std::time::Instant::now();
    let skill_ids: Vec<String> = skills.iter().map(|s| s.id.clone()).collect();
    store
        .ensure_scenario_skill_tool_defaults_batch(scenario_id, &skill_ids, &adapter_keys)
        .map_err(AppError::db)?;
    log::info!("[sync] step2 ensure defaults: {:?}", t0.elapsed());

    // 3. Batch query: which tools are enabled for each skill
    let t0 = std::time::Instant::now();
    let enabled_map: HashMap<String, Vec<String>> = store
        .get_enabled_tools_for_scenario_skills_batch(scenario_id, &skill_ids)
        .map_err(AppError::db)?;
    log::info!("[sync] step3 query enabled tools: {:?}", t0.elapsed());

    // 4. Build sync tasks
    struct SyncTask {
        source: PathBuf,
        target: PathBuf,
        mode: sync_engine::SyncMode,
        skill_id: String,
        tool_key: String,
    }

    let t0 = std::time::Instant::now();
    let mut sync_tasks = Vec::new();
    for skill in &skills {
        let source = PathBuf::from(&skill.central_path);
        let enabled_tools = enabled_map.get(&skill.id);
        if let Some(tools) = enabled_tools {
            let enabled_set: HashSet<&str> = tools.iter().map(|s| s.as_str()).collect();
            for adapter in &all_adapters {
                if enabled_set.contains(adapter.key.as_str()) {
                    let target = adapter.skills_dir().join(&skill.name);
                    let mode = sync_engine::sync_mode_for_tool(
                        &adapter.key,
                        configured_mode.as_deref(),
                    );
                    sync_tasks.push(SyncTask {
                        source: source.clone(),
                        target,
                        mode,
                        skill_id: skill.id.clone(),
                        tool_key: adapter.key.clone(),
                    });
                }
            }
        }
    }
    let symlink_count = sync_tasks.iter().filter(|t| matches!(t.mode, sync_engine::SyncMode::Symlink)).count();
    let copy_count = sync_tasks.iter().filter(|t| matches!(t.mode, sync_engine::SyncMode::Copy)).count();
    log::info!("[sync] step4 build tasks: {} total ({} symlink, {} copy): {:?}",
        sync_tasks.len(), symlink_count, copy_count, t0.elapsed());

    // 5. Parallel file sync (optimized: no create_dir_all per task, no remove before symlink if target doesn't exist)
    let t0 = std::time::Instant::now();
    let symlink_time = std::sync::atomic::AtomicU64::new(0);
    let copy_time = std::sync::atomic::AtomicU64::new(0);
    let sync_results: Vec<_> = sync_tasks
        .par_iter()
        .map(|task| {
            let t_op = std::time::Instant::now();
            let result = sync_engine::sync_skill_fast(&task.source, &task.target, task.mode);
            let elapsed = t_op.elapsed().as_micros() as u64;
            match task.mode {
                sync_engine::SyncMode::Symlink => symlink_time.fetch_add(elapsed, std::sync::atomic::Ordering::Relaxed),
                sync_engine::SyncMode::Copy => copy_time.fetch_add(elapsed, std::sync::atomic::Ordering::Relaxed),
            };
            (task, result)
        })
        .collect();
    let sym_us = symlink_time.load(std::sync::atomic::Ordering::Relaxed);
    let cp_us = copy_time.load(std::sync::atomic::Ordering::Relaxed);
    log::info!("[sync] step5 parallel file sync: {:?} (symlink cumulative: {}ms, copy cumulative: {}ms)",
        t0.elapsed(), sym_us / 1000, cp_us / 1000);
    log::info!("[sync] step5 avg: symlink={:.1}ms/op, copy={:.1}ms/op",
        sym_us as f64 / symlink_count.max(1) as f64 / 1000.0,
        cp_us as f64 / copy_count.max(1) as f64 / 1000.0);

    // 6. Batch insert target records in one transaction
    let t0 = std::time::Instant::now();
    let now = chrono::Utc::now().timestamp_millis();
    let target_records: Vec<SkillTargetRecord> = sync_results
        .iter()
        .filter_map(|(task, result)| match result {
            Ok(actual_mode) => Some(SkillTargetRecord {
                id: uuid::Uuid::new_v4().to_string(),
                skill_id: task.skill_id.clone(),
                tool: task.tool_key.clone(),
                target_path: task.target.to_string_lossy().to_string(),
                mode: actual_mode.as_str().to_string(),
                status: "ok".to_string(),
                synced_at: Some(now),
                last_error: None,
            }),
            Err(e) => {
                log::warn!(
                    "Failed to sync skill {} to {}: {e}",
                    task.skill_id,
                    task.target.display()
                );
                None
            }
        })
        .collect();

    if let Err(e) = store.insert_targets_batch(&target_records) {
        log::warn!("Failed to batch-insert sync targets: {e}");
    }
    log::info!("[sync] step6 batch insert DB: {:?}", t0.elapsed());
    log::info!("[sync] TOTAL: {:?}", t_total.elapsed());

    Ok(())
}

pub(crate) fn unsync_scenario_skills(
    store: &SkillStore,
    scenario_id: &str,
) -> Result<(), AppError> {
    let skill_ids = store
        .get_skill_ids_for_scenario(scenario_id)
        .map_err(AppError::db)?;
    if skill_ids.is_empty() {
        return Ok(());
    }

    // 1. Batch query: get all targets for all skills at once
    let all_targets = store
        .get_targets_for_skills(&skill_ids)
        .unwrap_or_default();

    // 2. Parallel file removal
    all_targets.par_iter().for_each(|target| {
        let path = PathBuf::from(&target.target_path);
        if let Err(e) = sync_engine::remove_target(&path) {
            log::warn!("Failed to remove sync target {}: {e}", path.display());
        }
    });

    // 3. Batch delete DB records in one transaction
    if let Err(e) = store.delete_targets_for_skills_batch(&skill_ids) {
        log::warn!("Failed to batch-delete target records: {e}");
    }

    Ok(())
}

/// Sync ALL managed skills to all enabled/installed tool adapters (global mode).
pub(crate) fn sync_all_skills(store: &SkillStore) -> Result<(), AppError> {
    let t_total = std::time::Instant::now();

    let skills = store.get_all_skills().map_err(AppError::db)?;
    if skills.is_empty() {
        return Ok(());
    }

    let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
    let all_adapters = tool_adapters::enabled_installed_adapters(store);

    // Pre-create adapter skills directories
    for adapter in &all_adapters {
        let skills_dir = adapter.skills_dir();
        if let Some(parent) = skills_dir.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    // Build sync tasks: every skill × every adapter
    struct SyncTask {
        source: PathBuf,
        target: PathBuf,
        mode: sync_engine::SyncMode,
        skill_id: String,
        tool_key: String,
    }

    let mut sync_tasks = Vec::new();
    for skill in &skills {
        let source = PathBuf::from(&skill.central_path);
        for adapter in &all_adapters {
            let target = adapter.skills_dir().join(&skill.name);
            let mode =
                sync_engine::sync_mode_for_tool(&adapter.key, configured_mode.as_deref());
            sync_tasks.push(SyncTask {
                source: source.clone(),
                target,
                mode,
                skill_id: skill.id.clone(),
                tool_key: adapter.key.clone(),
            });
        }
    }
    log::info!(
        "[sync_all] {} tasks ({} skills × {} adapters)",
        sync_tasks.len(),
        skills.len(),
        all_adapters.len()
    );

    // Parallel file sync
    let sync_results: Vec<_> = sync_tasks
        .par_iter()
        .map(|task| {
            let result = sync_engine::sync_skill_fast(&task.source, &task.target, task.mode);
            (task, result)
        })
        .collect();

    // Batch insert target records
    let now = chrono::Utc::now().timestamp_millis();
    let target_records: Vec<SkillTargetRecord> = sync_results
        .iter()
        .filter_map(|(task, result)| match result {
            Ok(actual_mode) => Some(SkillTargetRecord {
                id: uuid::Uuid::new_v4().to_string(),
                skill_id: task.skill_id.clone(),
                tool: task.tool_key.clone(),
                target_path: task.target.to_string_lossy().to_string(),
                mode: actual_mode.as_str().to_string(),
                status: "ok".to_string(),
                synced_at: Some(now),
                last_error: None,
            }),
            Err(e) => {
                log::warn!(
                    "Failed to sync skill {} to {}: {e}",
                    task.skill_id,
                    task.target.display()
                );
                None
            }
        })
        .collect();

    if let Err(e) = store.insert_targets_batch(&target_records) {
        log::warn!("Failed to batch-insert sync targets: {e}");
    }
    log::info!("[sync_all] TOTAL: {:?}", t_total.elapsed());

    Ok(())
}

/// Remove ALL sync targets from the filesystem and database.
fn unsync_all_skills(store: &SkillStore) -> Result<(), AppError> {
    let all_targets = store.get_all_targets().unwrap_or_default();

    all_targets.par_iter().for_each(|target| {
        let path = PathBuf::from(&target.target_path);
        if let Err(e) = sync_engine::remove_target(&path) {
            log::warn!("Failed to remove sync target {}: {e}", path.display());
        }
    });

    // Delete all target records — collect all unique skill IDs
    let skill_ids: Vec<String> = all_targets
        .iter()
        .map(|t| t.skill_id.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    if !skill_ids.is_empty() {
        if let Err(e) = store.delete_targets_for_skills_batch(&skill_ids) {
            log::warn!("Failed to batch-delete target records: {e}");
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn set_skill_sync_scope(
    app: tauri::AppHandle,
    scope: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let app_bg = app.clone();

    tauri::async_runtime::spawn_blocking(move || {
        // Save the new scope setting
        store
            .set_setting("skill_sync_scope", &scope)
            .map_err(AppError::db)?;

        if scope == "global" {
            // Switching to global: sync ALL skills to all adapters
            let t0 = std::time::Instant::now();
            sync_all_skills(&store)?;
            log::info!("set_skill_sync_scope → global: sync_all_skills took {:?}", t0.elapsed());
        } else {
            // Switching to scenario: unsync everything, then sync active scenario only
            unsync_all_skills(&store)?;
            if let Ok(Some(active_id)) = store.get_active_scenario_id() {
                sync_scenario_skills(&store, &active_id)?;
            }
        }

        Ok::<(), AppError>(())
    })
    .await??;

    if let Err(e) = app_bg.emit("scenario-sync-complete", ()) {
        log::warn!("Failed to emit scenario-sync-complete: {e}");
    }
    refresh_tray_menu_best_effort(&app);
    Ok(())
}

#[tauri::command]
pub async fn save_scenario_prompt_template(
    scenario_id: String,
    template: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_scenario_exists(&store, &scenario_id)?;
        store
            .save_scenario_prompt_template(&scenario_id, template.as_deref())
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn get_scenario_prompt_template(
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Option<String>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .get_scenario_prompt_template(&scenario_id)
            .map_err(AppError::db)
    })
    .await?
}
