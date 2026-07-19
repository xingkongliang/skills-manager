use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use super::{
    audit_log::AuditDraft,
    central_repo, content_hash,
    error::AppError,
    skill_store::{ScenarioRecord, SkillStore, SkillTargetRecord},
    sync_engine, tool_adapters,
    tool_service,
};

#[derive(Debug, Clone)]
pub struct ScenarioSyncTarget {
    pub skill_id: String,
    pub skill_name: String,
    pub tool: String,
    pub source: PathBuf,
    pub target: PathBuf,
    pub mode: sync_engine::SyncMode,
    /// Current content hash of the central skill source, copied from
    /// `SkillRecord.content_hash`. Compared against the previously
    /// synced `SkillTargetRecord.source_hash` to skip redundant
    /// Copy-mode resyncs at startup (issue #153).
    pub source_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncPreviewTarget {
    pub skill_id: String,
    pub skill_name: String,
    pub tool: String,
    pub target_path: String,
    pub mode: String,
}

pub fn ensure_scenario_exists(store: &SkillStore, scenario_id: &str) -> Result<(), AppError> {
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

pub fn enabled_installed_adapters_for_scenario_skill(
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

pub fn collect_scenario_sync_targets(
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
        let adapters = enabled_installed_adapters_for_scenario_skill(store, scenario_id, &skill.id)?;
        for adapter in &adapters {
            let target = adapter.skills_dir().join(&target_name);
            let mode = sync_engine::sync_mode_for_tool(&adapter.key, configured_mode.as_deref());
            targets.push(ScenarioSyncTarget {
                skill_id: skill.id.clone(),
                skill_name: skill.name.clone(),
                tool: adapter.key.clone(),
                source: source.clone(),
                target,
                mode,
                source_hash: skill.content_hash.clone(),
            });
        }
    }

    Ok(targets)
}

pub fn preview_scenario_sync(
    store: &SkillStore,
    scenario_id: &str,
) -> Result<Vec<SyncPreviewTarget>, AppError> {
    collect_scenario_sync_targets(store, scenario_id).map(|targets| {
        targets
            .into_iter()
            .map(|target| SyncPreviewTarget {
                skill_id: target.skill_id,
                skill_name: target.skill_name,
                tool: target.tool,
                target_path: target.target.to_string_lossy().to_string(),
                mode: target.mode.as_str().to_string(),
            })
            .collect()
    })
}

/// Decide which `SyncMode` `is_target_current` should compare against, or
/// `None` if the existing target's mode is incompatible with the desired
/// mode and the skip path must be refused.
///
/// Returns `Some(existing)` when both modes match exactly. Also returns
/// `Some(Copy)` when the existing record is `"copy"` but the desired
/// mode is `Symlink` — this is the Windows fallback case (issue #153):
/// `symlink_dir()` failed on a prior run and we landed in copy mode, so
/// every subsequent startup would re-attempt symlink, fail again, and
/// trigger a full recursive copy. Treating the existing copy as
/// compatible lets the hash gate skip when the source hasn't changed.
///
/// The reverse direction (existing `"symlink"`, desired `Copy`) returns
/// `None` because the user actively changed the `sync_mode` setting and
/// the on-disk symlink doesn't reflect that intent.
fn skip_check_mode(existing_mode: &str, desired: sync_engine::SyncMode) -> Option<sync_engine::SyncMode> {
    match (existing_mode, desired) {
        ("symlink", sync_engine::SyncMode::Symlink) => Some(sync_engine::SyncMode::Symlink),
        ("copy", sync_engine::SyncMode::Copy) => Some(sync_engine::SyncMode::Copy),
        ("copy", sync_engine::SyncMode::Symlink) => Some(sync_engine::SyncMode::Copy),
        _ => None,
    }
}

/// Slack (ms) allowed between a target's newest content mtime and its last
/// recorded `synced_at` before we treat the target as "possibly edited". Filesystem
/// mtime granularity and the small delay between writing files and recording
/// `synced_at` mean an equal-or-slightly-later mtime does not imply a real edit.
/// Kept in sync with the 1000ms convention used elsewhere in the sync path.
const SPOKE_MTIME_THRESHOLD_MS: i64 = 1000;

/// What the deploy guard decides to do with an about-to-be-overwritten Copy target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeployGuardDecision {
    /// Overwrite the target with central content (original behavior).
    Deploy,
    /// Target was edited by the user since our last sync: snapshot it once,
    /// flag the record, and skip the deploy.
    PreserveAndFlag,
    /// Target was edited but is already flagged: preserve and skip without
    /// taking another snapshot.
    PreserveNoBackup,
}

/// Decide whether an existing Copy target may be overwritten, protected with a
/// fresh snapshot, or protected without one.
///
/// `target_mtime` is the newest content-file mtime of the spoke (ms), `synced_at`
/// is when we last deployed to it, `baseline_hash` is the content hash we last
/// deployed (== the central source hash at that time), and `status` is the
/// current target record status.
///
/// The (expensive) `compute_target_hash` closure is only invoked when the cheap
/// mtime pre-filter says the target *might* have changed. This keeps the #248
/// hot path intact: a target whose newest file predates our last sync (within
/// `SPOKE_MTIME_THRESHOLD_MS` slack) can't have been edited since we wrote it, so
/// we return `Deploy` without ever hashing it.
fn deploy_guard_decision(
    target_mtime: Option<i64>,
    synced_at: i64,
    baseline_hash: &str,
    status: &str,
    compute_target_hash: impl FnOnce() -> Option<String>,
) -> DeployGuardDecision {
    // Cheap pre-filter — no hashing on the hot path.
    let possibly_edited = match target_mtime {
        None => false,
        Some(mtime) => mtime > synced_at + SPOKE_MTIME_THRESHOLD_MS,
    };
    if !possibly_edited {
        return DeployGuardDecision::Deploy;
    }

    // mtime moved: hash to tell a real edit apart from a bare touch.
    match compute_target_hash() {
        // Content still equals what we deployed: not a real edit, overwrite.
        Some(hash) if hash == baseline_hash => DeployGuardDecision::Deploy,
        // Couldn't hash (target vanished/unreadable): fall back to overwrite so
        // we never protect a target we can't reason about.
        None => DeployGuardDecision::Deploy,
        // Genuine divergence from what we deployed → protect the spoke. Snapshot
        // only the first time (status flips to target_ahead) so repeated preset
        // applies don't keep flooding backups/.
        Some(_) => {
            if status == "target_ahead" {
                DeployGuardDecision::PreserveNoBackup
            } else {
                DeployGuardDecision::PreserveAndFlag
            }
        }
    }
}

pub fn sync_desired_targets(
    store: &SkillStore,
    desired_targets: &[ScenarioSyncTarget],
) -> Result<(), AppError> {
    let batch_start = Instant::now();
    let existing_targets: HashMap<(String, String), SkillTargetRecord> = store
        .get_all_targets()
        .map_err(AppError::db)?
        .into_iter()
        .map(|target| ((target.skill_id.clone(), target.tool.clone()), target))
        .collect();

    let mut synced_count = 0usize;
    let mut skipped_count = 0usize;
    let mut preserved_count = 0usize;
    let mut failed_count = 0usize;

    for desired in desired_targets {
        let target_start = Instant::now();
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
            } else {
                if existing.status == "ok" {
                    if let Some(check_mode) = skip_check_mode(&existing.mode, desired.mode) {
                        if sync_engine::is_target_current(
                            &desired.source,
                            &desired.target,
                            check_mode,
                            existing.source_hash.as_deref(),
                            desired.source_hash.as_deref(),
                        ) {
                            // Surface the Windows fallback case in logs so operators
                            // can tell when a target is permanently on Copy because
                            // an earlier symlink_dir() failed (issue #153). Helpful
                            // when a user later enables Developer Mode and wonders
                            // why Symlink isn't being re-attempted.
                            if existing.mode == "copy"
                                && matches!(desired.mode, sync_engine::SyncMode::Symlink)
                            {
                                log::debug!(
                                    "sync_desired_targets: skill {} ({}) staying on copy fallback for {} (content unchanged); trigger a manual resync to retry symlink",
                                    desired.skill_id,
                                    desired.skill_name,
                                    desired.tool
                                );
                            }
                            skipped_count += 1;
                            continue;
                        }
                    }
                }

                // Deploy guard (Problem B): we're about to overwrite an existing
                // target. Only Copy targets have an independent spoke that can
                // hold user edits — Symlink targets *are* the central dir, so
                // there's nothing to protect and the guard is skipped. We also
                // need a baseline (both synced_at and source_hash present) to
                // tell a real edit from the content we last deployed.
                if matches!(desired.mode, sync_engine::SyncMode::Copy) {
                    if let (Some(synced_at), Some(baseline)) =
                        (existing.synced_at, existing.source_hash.as_deref())
                    {
                        // Cheap mtime read first; the closure only hashes when
                        // the pre-filter says the target might have changed.
                        let target_mtime =
                            content_hash::latest_modified_millis(&desired.target);
                        let decision = deploy_guard_decision(
                            target_mtime,
                            synced_at,
                            baseline,
                            &existing.status,
                            || content_hash::hash_directory(&desired.target).ok(),
                        );
                        match decision {
                            DeployGuardDecision::Deploy => {}
                            DeployGuardDecision::PreserveAndFlag => {
                                let skill_dir_name = sync_engine::target_dir_name(
                                    &desired.source,
                                    &desired.skill_name,
                                );
                                let central_root = central_repo::base_dir();
                                // In-place snapshot (Copy) — the spoke stays put.
                                match central_repo::backup_directory(
                                    &central_root,
                                    &desired.target,
                                    &skill_dir_name,
                                    "spoke-ahead",
                                    central_repo::BackupKind::Copy,
                                ) {
                                    Ok(dest) => log::info!(
                                        "sync_desired_targets: preserved user-edited target for skill {} ({}) / {}; snapshot at {}",
                                        desired.skill_id,
                                        desired.skill_name,
                                        desired.tool,
                                        dest.display()
                                    ),
                                    Err(e) => log::warn!(
                                        "sync_desired_targets: failed to snapshot user-edited target for skill {} / {}: {e}",
                                        desired.skill_id,
                                        desired.tool
                                    ),
                                }
                                // Flag the record so a later run takes the
                                // PreserveNoBackup path and doesn't re-snapshot.
                                if let Err(e) = store.update_target_status(
                                    &desired.skill_id,
                                    &desired.tool,
                                    "target_ahead",
                                    Some("preserved user-edited target, skipped deploy"),
                                ) {
                                    log::warn!(
                                        "sync_desired_targets: failed to flag target_ahead for skill {} / {}: {e}",
                                        desired.skill_id,
                                        desired.tool
                                    );
                                }
                                store.log_audit(
                                    AuditDraft::new("sync")
                                        .skill(&desired.skill_id, &desired.skill_name)
                                        .tool(&desired.tool)
                                        .ok()
                                        .detail("preserved user-edited target, skipped deploy"),
                                );
                                preserved_count += 1;
                                continue;
                            }
                            DeployGuardDecision::PreserveNoBackup => {
                                // Already snapshotted on a prior run — keep the
                                // spoke, skip silently (no backup spam).
                                preserved_count += 1;
                                continue;
                            }
                        }
                    }
                }
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
                    // Record the hash that was just synced so the next
                    // run of this loop can short-circuit when the central
                    // skill content has not changed (issue #153).
                    source_hash: desired.source_hash.clone(),
                };
                if let Err(e) = store.insert_target(&target_record) {
                    log::warn!(
                        "Failed to insert sync target for skill {}: {e}",
                        desired.skill_id
                    );
                }
                synced_count += 1;
                let elapsed = target_start.elapsed().as_millis();
                if elapsed >= 200 {
                    log::warn!(
                        "sync_desired_targets: slow sync ({elapsed} ms, mode={}) for skill {} ({}) -> {}",
                        actual_mode.as_str(),
                        desired.skill_id,
                        desired.skill_name,
                        desired.target.display()
                    );
                }
            }
            Err(e) => {
                failed_count += 1;
                log::warn!(
                    "Failed to sync skill {} ({}) to {} after {} ms: {e}",
                    desired.skill_id,
                    desired.skill_name,
                    desired.target.display(),
                    target_start.elapsed().as_millis()
                );
            }
        }
    }

    log::info!(
        "sync_desired_targets: {} targets in {} ms (synced={synced_count}, skipped={skipped_count}, preserved={preserved_count}, failed={failed_count})",
        desired_targets.len(),
        batch_start.elapsed().as_millis()
    );

    Ok(())
}

pub fn unsync_obsolete_scenario_targets(
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

pub fn unsync_scenario_skills(store: &SkillStore, scenario_id: &str) -> Result<(), AppError> {
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

pub fn sync_scenario_skills(store: &SkillStore, scenario_id: &str) -> Result<(), AppError> {
    let desired_targets = collect_scenario_sync_targets(store, scenario_id)?;
    sync_desired_targets(store, &desired_targets)
}

pub fn apply_scenario_to_default(store: &SkillStore, scenario_id: &str) -> Result<(), AppError> {
    ensure_scenario_exists(store, scenario_id)?;
    let desired_targets = collect_scenario_sync_targets(store, scenario_id)?;

    if let Ok(Some(old_id)) = store.get_active_scenario_id() {
        if old_id != scenario_id {
            unsync_obsolete_scenario_targets(store, &old_id, &desired_targets)?;
        }
    }

    store.set_active_scenario(scenario_id).map_err(AppError::db)?;
    sync_desired_targets(store, &desired_targets)
}

pub fn sync_skill_to_active_scenario(
    store: &SkillStore,
    scenario_id: &str,
    skill_id: &str,
) -> Result<(), AppError> {
    if let Ok(Some(active_id)) = store.get_active_scenario_id() {
        if active_id == scenario_id {
            let adapters = enabled_installed_adapters_for_scenario_skill(store, scenario_id, skill_id)?;
            let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
            let Ok(Some(skill)) = store.get_skill_by_id(skill_id) else {
                return Ok(());
            };
            let source = PathBuf::from(&skill.central_path);
            let target_name = sync_engine::target_dir_name(&source, &skill.name);
            let old_targets = store.get_targets_for_skill(skill_id).unwrap_or_default();
            for adapter in &adapters {
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
                let mode = sync_engine::sync_mode_for_tool(&adapter.key, configured_mode.as_deref());
                match sync_engine::sync_skill(&source, &target, mode) {
                    Ok(actual_mode) => {
                        let now = chrono::Utc::now().timestamp_millis();
                        let target_record = super::skill_store::SkillTargetRecord {
                            id: uuid::Uuid::new_v4().to_string(),
                            skill_id: skill_id.to_string(),
                            tool: adapter.key.clone(),
                            target_path: target.to_string_lossy().to_string(),
                            mode: actual_mode.as_str().to_string(),
                            status: "ok".to_string(),
                            synced_at: Some(now),
                            last_error: None,
                            source_hash: skill.content_hash.clone(),
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

pub fn ensure_default_startup_scenario(store: &SkillStore) -> Result<(), AppError> {
    let mut scenarios = store.get_all_scenarios().map_err(AppError::db)?;
    if scenarios.is_empty() {
        let now = chrono::Utc::now().timestamp_millis();
        let default_scenario = ScenarioRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Default".to_string(),
            description: Some("Default startup scenario".to_string()),
            icon: None,
            sort_order: 0,
            created_at: now,
            updated_at: now,
        };
        store.insert_scenario(&default_scenario).map_err(AppError::db)?;
        scenarios.push(default_scenario);
    }

    let current_active = store.get_active_scenario_id().map_err(AppError::db)?;
    let preferred_default = store.get_setting("default_scenario").ok().flatten();

    let desired_active = preferred_default
        .filter(|id| scenarios.iter().any(|scenario| scenario.id == *id))
        .or_else(|| {
            current_active
                .clone()
                .filter(|id| scenarios.iter().any(|scenario| scenario.id == *id))
        })
        .unwrap_or_else(|| scenarios[0].id.clone());

    if current_active.as_deref() != Some(desired_active.as_str()) {
        if let Some(old_active) = current_active.as_deref() {
            unsync_scenario_skills(store, old_active)?;
        }
        store
            .set_active_scenario(&desired_active)
            .map_err(AppError::db)?;
    }

    sync_scenario_skills(store, &desired_active)
}

pub fn ensure_cli_scenario_state(store: &SkillStore) -> Result<(), AppError> {
    let mut scenarios = store.get_all_scenarios().map_err(AppError::db)?;
    if scenarios.is_empty() {
        let now = chrono::Utc::now().timestamp_millis();
        let default_scenario = ScenarioRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Default".to_string(),
            description: Some("Default startup scenario".to_string()),
            icon: None,
            sort_order: 0,
            created_at: now,
            updated_at: now,
        };
        store.insert_scenario(&default_scenario).map_err(AppError::db)?;
        scenarios.push(default_scenario);
    }

    let current_active = store.get_active_scenario_id().map_err(AppError::db)?;
    if current_active
        .as_deref()
        .is_some_and(|id| scenarios.iter().any(|scenario| scenario.id == id))
    {
        return Ok(());
    }

    let preferred_default = store.get_setting("default_scenario").ok().flatten();
    let desired_active = preferred_default
        .filter(|id| scenarios.iter().any(|scenario| scenario.id == *id))
        .unwrap_or_else(|| scenarios[0].id.clone());

    store
        .set_active_scenario(&desired_active)
        .map_err(AppError::db)
}

pub fn restore_all_skills_sync_included(store: &SkillStore) -> Result<bool, AppError> {
    let mut changed = false;
    for skill in store.get_all_skills().map_err(AppError::db)? {
        if !skill.enabled {
            store
                .update_skill_enabled(&skill.id, true)
                .map_err(AppError::db)?;
            changed = true;
        }
    }
    Ok(changed)
}

pub fn sync_active_scenario_to_tool(store: &SkillStore, tool_key: &str) {
    if let Ok(Some(active_id)) = store.get_active_scenario_id() {
        let Ok(skill_ids) = store.get_skill_ids_for_scenario(&active_id) else {
            return;
        };
        for skill_id in skill_ids {
            if let Ok(adapters) = enabled_installed_adapters_for_scenario_skill(store, &active_id, &skill_id)
            {
                if adapters.iter().any(|adapter| adapter.key == tool_key) {
                    let _ = sync_skill_to_active_scenario(store, &active_id, &skill_id);
                }
            }
        }
    }
}

pub fn sync_single_skill_to_tool(
    store: &SkillStore,
    skill_id: &str,
    tool: &str,
) -> Result<(), AppError> {
    let adapter = tool_adapters::find_adapter_with_store(store, tool)
        .ok_or_else(|| AppError::not_found(format!("Unknown tool: {}", tool)))?;

    if !adapter.is_installed() {
        return Err(AppError::not_found(format!(
            "{} is not installed",
            adapter.display_name
        )));
    }

    if tool_service::get_disabled_tools(store).contains(&tool.to_string()) {
        return Err(AppError::invalid_input(format!(
            "{} is disabled",
            adapter.display_name
        )));
    }

    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;

    let source = PathBuf::from(&skill.central_path);
    let target = adapter
        .skills_dir()
        .join(sync_engine::target_dir_name(&source, &skill.name));
    let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
    let mode = sync_engine::sync_mode_for_tool(tool, configured_mode.as_deref());
    let actual_mode = sync_engine::sync_skill(&source, &target, mode).map_err(AppError::io)?;

    let now = chrono::Utc::now().timestamp_millis();
    let target_record = SkillTargetRecord {
        id: uuid::Uuid::new_v4().to_string(),
        skill_id: skill_id.to_string(),
        tool: tool.to_string(),
        target_path: target.to_string_lossy().to_string(),
        mode: actual_mode.as_str().to_string(),
        status: "ok".to_string(),
        synced_at: Some(now),
        last_error: None,
        source_hash: skill.content_hash.clone(),
    };

    store.insert_target(&target_record).map_err(AppError::db)?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub enum BatchApplyMode {
    Add,
    Remove,
}

/// Apply a batch of `(skill_id × tool_key)` pairs in either Add or Remove mode
/// without touching `active_scenario_id` or `scenario_skill_tools` toggles.
///
/// This is the tray-side preset apply primitive. Unlike [`sync_single_skill_to_tool`]
/// (which is wrapped by the `sync_skill_to_tool` Tauri command and carries the
/// implicit active-preset toggle side-effect), this batch is a pure
/// "write/remove files + maintain `skill_targets` rows" operation.
///
/// Remove mode handles shared physical paths: a `target_path` may be referenced
/// by multiple `(skill_id, tool)` records when several tools resolve to the same
/// skills directory. The filesystem path is only removed when no remaining
/// `skill_targets` row references it after the batch deletions, so removing one
/// preset's tools never wipes another tool's still-active files.
pub fn apply_skills_to_tools(
    store: &SkillStore,
    skill_ids: &[String],
    tool_keys: &[String],
    mode: BatchApplyMode,
) -> Result<(), AppError> {
    if skill_ids.is_empty() || tool_keys.is_empty() {
        return Ok(());
    }

    match mode {
        BatchApplyMode::Add => apply_add(store, skill_ids, tool_keys),
        BatchApplyMode::Remove => apply_remove(store, skill_ids, tool_keys),
    }
}

fn apply_add(
    store: &SkillStore,
    skill_ids: &[String],
    tool_keys: &[String],
) -> Result<(), AppError> {
    let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
    let disabled = tool_service::get_disabled_tools(store);

    let mut adapters: HashMap<String, tool_adapters::ToolAdapter> = HashMap::new();
    for key in tool_keys {
        if disabled.contains(key) {
            log::debug!("apply_skills_to_tools: skipping disabled tool {key}");
            continue;
        }
        let Some(adapter) = tool_adapters::find_adapter_with_store(store, key) else {
            log::warn!("apply_skills_to_tools: unknown tool {key}");
            continue;
        };
        if !adapter.is_installed() {
            log::debug!(
                "apply_skills_to_tools: skipping uninstalled tool {} ({key})",
                adapter.display_name
            );
            continue;
        }
        adapters.insert(key.clone(), adapter);
    }

    let mut synced = 0usize;
    let mut failed = 0usize;
    for skill_id in skill_ids {
        let Ok(Some(skill)) = store.get_skill_by_id(skill_id) else {
            log::warn!("apply_skills_to_tools: skill {skill_id} not found");
            continue;
        };
        let source = PathBuf::from(&skill.central_path);
        let target_name = sync_engine::target_dir_name(&source, &skill.name);
        for (tool_key, adapter) in &adapters {
            let target = adapter.skills_dir().join(&target_name);
            let mode = sync_engine::sync_mode_for_tool(tool_key, configured_mode.as_deref());
            match sync_engine::sync_skill(&source, &target, mode) {
                Ok(actual_mode) => {
                    let now = chrono::Utc::now().timestamp_millis();
                    let target_record = SkillTargetRecord {
                        id: uuid::Uuid::new_v4().to_string(),
                        skill_id: skill_id.clone(),
                        tool: tool_key.clone(),
                        target_path: target.to_string_lossy().to_string(),
                        mode: actual_mode.as_str().to_string(),
                        status: "ok".to_string(),
                        synced_at: Some(now),
                        last_error: None,
                        source_hash: skill.content_hash.clone(),
                    };
                    if let Err(e) = store.insert_target(&target_record) {
                        log::warn!(
                            "apply_skills_to_tools: failed to insert target for skill {skill_id} / {tool_key}: {e}"
                        );
                        failed += 1;
                    } else {
                        synced += 1;
                    }
                }
                Err(e) => {
                    failed += 1;
                    log::warn!(
                        "apply_skills_to_tools: failed to sync skill {skill_id} ({}) to {}: {e}",
                        skill.name,
                        target.display()
                    );
                }
            }
        }
    }

    log::info!(
        "apply_skills_to_tools(Add): skills={} tools={} synced={synced} failed={failed}",
        skill_ids.len(),
        adapters.len(),
    );
    Ok(())
}

fn apply_remove(
    store: &SkillStore,
    skill_ids: &[String],
    tool_keys: &[String],
) -> Result<(), AppError> {
    let tool_set: HashSet<&String> = tool_keys.iter().collect();

    let mut to_delete: Vec<(String, String, PathBuf)> = Vec::new();
    for skill_id in skill_ids {
        let targets = store.get_targets_for_skill(skill_id).unwrap_or_default();
        for target in targets {
            if tool_set.contains(&target.tool) {
                to_delete.push((
                    skill_id.clone(),
                    target.tool.clone(),
                    PathBuf::from(&target.target_path),
                ));
            }
        }
    }

    if to_delete.is_empty() {
        return Ok(());
    }

    // Phase 1: drop the DB rows first so the post-delete recount below sees
    // the new ground truth when deciding which filesystem paths to keep.
    for (skill_id, tool, _) in &to_delete {
        if let Err(e) = store.delete_target(skill_id, tool) {
            log::warn!(
                "apply_skills_to_tools(Remove): failed to delete target record for skill {skill_id} / {tool}: {e}"
            );
        }
    }

    // Phase 2: gather the paths the batch wanted to remove, then keep any path
    // a remaining (skill_id, tool) row still points at. This prevents wiping a
    // directory another adapter is sharing.
    let candidate_paths: HashSet<PathBuf> = to_delete.iter().map(|(_, _, p)| p.clone()).collect();
    let still_referenced: HashSet<PathBuf> = store
        .get_all_targets()
        .unwrap_or_default()
        .into_iter()
        .map(|t| PathBuf::from(&t.target_path))
        .collect();

    let mut removed = 0usize;
    for path in candidate_paths {
        if still_referenced.contains(&path) {
            log::debug!(
                "apply_skills_to_tools(Remove): keeping {} (still referenced by another target)",
                path.display()
            );
            continue;
        }
        if let Err(e) = sync_engine::remove_target(&path) {
            log::warn!(
                "apply_skills_to_tools(Remove): failed to remove {}: {e}",
                path.display()
            );
        } else {
            removed += 1;
        }
    }

    log::info!(
        "apply_skills_to_tools(Remove): pairs={} fs_removed={removed}",
        to_delete.len(),
    );
    Ok(())
}

#[cfg(test)]
mod sync_desired_targets_tests {
    use super::*;
    use crate::core::central_repo;
    use crate::core::skill_store::{SkillRecord, SkillStore, SkillTargetRecord};
    use std::fs;
    use tempfile::tempdir;

    /// Issue #153 regression: when the existing target was written in
    /// Copy mode (Windows symlink fallback) but the configured mode is
    /// Symlink, and the source content hash hasn't changed, the sync
    /// must be skipped. Prior to the fix the mode-equality guard would
    /// reject the skip branch and re-attempt the full recursive copy
    /// every startup.
    #[test]
    fn copy_fallback_target_with_matching_hash_is_skipped() {
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();

        // Real source dir with one file (the central skill).
        let source = central_repo::skills_dir().join("skill-a");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "real source").unwrap();

        // Pre-existing target dir with a marker file that would be wiped
        // by copy_dir_recursive's pre-clean step if a re-sync ran.
        let target = tmp.path().join("agent-skills").join("skill-a");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("MARKER.txt"), "do not wipe me").unwrap();

        // DB rows: skill content_hash = "h1"; existing target also at "h1",
        // mode "copy" (i.e. previously fell back from Symlink).
        let skill = SkillRecord {
            id: "skill-a".to_string(),
            name: "skill-a".to_string(),
            description: None,
            source_type: "import".to_string(),
            source_ref: Some(source.to_string_lossy().to_string()),
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: source.to_string_lossy().to_string(),
            content_hash: Some("h1".to_string()),
            enabled: true,
            created_at: 1,
            updated_at: 1,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: None,
            last_check_error: None,
        };
        store.insert_skill(&skill).unwrap();

        store
            .insert_target(&SkillTargetRecord {
                id: "target-1".to_string(),
                skill_id: "skill-a".to_string(),
                tool: "claude-code".to_string(),
                target_path: target.to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: Some("h1".to_string()),
            })
            .unwrap();

        // Desired target: same source/target/hash but Symlink mode
        // (the configured default that originally fell back to Copy).
        let desired = vec![ScenarioSyncTarget {
            skill_id: "skill-a".to_string(),
            skill_name: "skill-a".to_string(),
            tool: "claude-code".to_string(),
            source: source.clone(),
            target: target.clone(),
            mode: sync_engine::SyncMode::Symlink,
            source_hash: Some("h1".to_string()),
        }];

        sync_desired_targets(&store, &desired).unwrap();

        // The marker file proves no re-sync ran (a real re-sync would
        // have called copy_dir_recursive after wiping the target).
        assert!(
            target.join("MARKER.txt").exists(),
            "target dir was wiped — skip did not fire"
        );
        // The skill's actual SKILL.md should NOT have been copied in,
        // because we skipped the sync entirely.
        assert!(
            !target.join("SKILL.md").exists(),
            "SKILL.md appeared — sync ran instead of skipping"
        );

        central_repo::set_test_base_dir_override(None);
    }

    /// Companion: if the target has been manually deleted, even with a
    /// matching hash, we must NOT skip — the user's agent dir is
    /// otherwise left broken.
    #[test]
    fn deleted_target_with_matching_hash_forces_resync() {
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();

        let source = central_repo::skills_dir().join("skill-b");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "real source").unwrap();

        // Target path that does NOT exist on disk.
        let target = tmp.path().join("agent-skills").join("skill-b");

        let skill = SkillRecord {
            id: "skill-b".to_string(),
            name: "skill-b".to_string(),
            description: None,
            source_type: "import".to_string(),
            source_ref: Some(source.to_string_lossy().to_string()),
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: source.to_string_lossy().to_string(),
            content_hash: Some("h1".to_string()),
            enabled: true,
            created_at: 1,
            updated_at: 1,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: None,
            last_check_error: None,
        };
        store.insert_skill(&skill).unwrap();

        store
            .insert_target(&SkillTargetRecord {
                id: "target-2".to_string(),
                skill_id: "skill-b".to_string(),
                tool: "claude-code".to_string(),
                target_path: target.to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: Some("h1".to_string()),
            })
            .unwrap();

        let desired = vec![ScenarioSyncTarget {
            skill_id: "skill-b".to_string(),
            skill_name: "skill-b".to_string(),
            tool: "claude-code".to_string(),
            source: source.clone(),
            target: target.clone(),
            mode: sync_engine::SyncMode::Copy,
            source_hash: Some("h1".to_string()),
        }];

        sync_desired_targets(&store, &desired).unwrap();

        // Sync must have run — target should now exist with the source content.
        assert!(target.join("SKILL.md").exists(), "missing target was not re-synced");

        central_repo::set_test_base_dir_override(None);
    }

    // ── Problem B: deploy guard for user-edited Copy targets ──

    use std::path::Path;

    /// Force a content file's mtime to a fixed epoch-ms value so the
    /// touched/untouched decision doesn't depend on wall-clock timing.
    fn set_mtime_ms(path: &Path, ms: i64) {
        let time = std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms as u64);
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(time).unwrap();
    }

    fn guard_skill(id: &str, source: &Path, content_hash: &str) -> SkillRecord {
        SkillRecord {
            id: id.to_string(),
            name: id.to_string(),
            description: None,
            source_type: "import".to_string(),
            source_ref: Some(source.to_string_lossy().to_string()),
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: source.to_string_lossy().to_string(),
            content_hash: Some(content_hash.to_string()),
            enabled: true,
            created_at: 1,
            updated_at: 1,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: None,
            last_check_error: None,
        }
    }

    fn count_backups(base: &Path, skill_dir: &str) -> usize {
        let dir = base.join("backups").join(skill_dir);
        match fs::read_dir(&dir) {
            Ok(rd) => rd.filter_map(|e| e.ok()).count(),
            Err(_) => 0,
        }
    }

    /// Case 1 (no regression): the target was NOT edited since the last sync
    /// (mtime <= synced_at) and the central content changed, so the target is
    /// overwritten exactly as before — the guard must not fire.
    #[test]
    fn untouched_target_is_overwritten_when_central_changed() {
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();

        let source = central_repo::skills_dir().join("skill-a");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "central v2").unwrap();

        let target = tmp.path().join("agent-skills").join("skill-a");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "old deployed v1").unwrap();
        fs::write(target.join("MARKER.txt"), "should be wiped").unwrap();
        // Target last modified at 5000; we "synced" at 10000 → untouched.
        set_mtime_ms(&target.join("SKILL.md"), 5_000);
        set_mtime_ms(&target.join("MARKER.txt"), 5_000);

        store.insert_skill(&guard_skill("skill-a", &source, "h2")).unwrap();
        store
            .insert_target(&SkillTargetRecord {
                id: "t1".to_string(),
                skill_id: "skill-a".to_string(),
                tool: "claude-code".to_string(),
                target_path: target.to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "ok".to_string(),
                synced_at: Some(10_000),
                last_error: None,
                source_hash: Some("h1".to_string()), // baseline differs from central h2
            })
            .unwrap();

        let desired = vec![ScenarioSyncTarget {
            skill_id: "skill-a".to_string(),
            skill_name: "skill-a".to_string(),
            tool: "claude-code".to_string(),
            source: source.clone(),
            target: target.clone(),
            mode: sync_engine::SyncMode::Copy,
            source_hash: Some("h2".to_string()),
        }];

        sync_desired_targets(&store, &desired).unwrap();

        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "central v2",
            "untouched target should have been overwritten with central content"
        );
        assert!(
            !target.join("MARKER.txt").exists(),
            "overwrite should have wiped the stale marker"
        );
        assert_eq!(count_backups(&base, "skill-a"), 0, "no backup for untouched target");

        central_repo::set_test_base_dir_override(None);
    }

    /// Case 2 (the fix): the target was edited since the last sync (mtime >
    /// synced_at AND content hash != baseline) and central changed — the
    /// user's edit must be preserved, a snapshot taken, and status flagged.
    #[test]
    fn edited_target_is_preserved_snapshotted_and_flagged() {
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();

        let source = central_repo::skills_dir().join("skill-b");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "central v2").unwrap();

        // Deploy an original target, capture its real hash as the baseline.
        let target = tmp.path().join("agent-skills").join("skill-b");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "original deployed").unwrap();
        let baseline = content_hash::hash_directory(&target).unwrap();

        // Now the user edits the spoke; mtime is "now" (>> synced_at + slack).
        fs::write(target.join("SKILL.md"), "USER EDIT keep me").unwrap();

        store.insert_skill(&guard_skill("skill-b", &source, "central-h2")).unwrap();
        store
            .insert_target(&SkillTargetRecord {
                id: "t2".to_string(),
                skill_id: "skill-b".to_string(),
                tool: "claude-code".to_string(),
                target_path: target.to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: Some(baseline.clone()),
            })
            .unwrap();

        let desired = vec![ScenarioSyncTarget {
            skill_id: "skill-b".to_string(),
            skill_name: "skill-b".to_string(),
            tool: "claude-code".to_string(),
            source: source.clone(),
            target: target.clone(),
            mode: sync_engine::SyncMode::Copy,
            source_hash: Some("central-h2".to_string()),
        }];

        sync_desired_targets(&store, &desired).unwrap();

        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "USER EDIT keep me",
            "user-edited target must NOT be overwritten"
        );
        assert_eq!(
            count_backups(&base, "skill-b"),
            1,
            "one spoke-ahead snapshot must exist"
        );
        let rec = store
            .get_targets_for_skill("skill-b")
            .unwrap()
            .into_iter()
            .find(|t| t.tool == "claude-code")
            .unwrap();
        assert_eq!(rec.status, "target_ahead", "record must be flagged target_ahead");

        central_repo::set_test_base_dir_override(None);
    }

    /// Case 3 (no false protection): the target's mtime was bumped but its
    /// content still equals the baseline — this is not a real edit, so the
    /// overwrite proceeds.
    #[test]
    fn touched_but_unchanged_target_is_overwritten() {
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();

        let source = central_repo::skills_dir().join("skill-c");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "central v2").unwrap();

        let target = tmp.path().join("agent-skills").join("skill-c");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "deployed content").unwrap();
        let baseline = content_hash::hash_directory(&target).unwrap();
        // "Touch": rewrite identical content so mtime moves but hash is stable.
        fs::write(target.join("SKILL.md"), "deployed content").unwrap();

        store.insert_skill(&guard_skill("skill-c", &source, "central-h2")).unwrap();
        store
            .insert_target(&SkillTargetRecord {
                id: "t3".to_string(),
                skill_id: "skill-c".to_string(),
                tool: "claude-code".to_string(),
                target_path: target.to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: Some(baseline.clone()),
            })
            .unwrap();

        let desired = vec![ScenarioSyncTarget {
            skill_id: "skill-c".to_string(),
            skill_name: "skill-c".to_string(),
            tool: "claude-code".to_string(),
            source: source.clone(),
            target: target.clone(),
            mode: sync_engine::SyncMode::Copy,
            source_hash: Some("central-h2".to_string()),
        }];

        sync_desired_targets(&store, &desired).unwrap();

        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "central v2",
            "content-unchanged target must still be overwritten"
        );
        assert_eq!(count_backups(&base, "skill-c"), 0, "no backup when content unchanged");

        central_repo::set_test_base_dir_override(None);
    }

    /// Case 4: a target already flagged target_ahead is preserved again on a
    /// second run, but no additional snapshot is written.
    #[test]
    fn already_flagged_target_is_not_snapshotted_again() {
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();

        let source = central_repo::skills_dir().join("skill-d");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "central v2").unwrap();

        let target = tmp.path().join("agent-skills").join("skill-d");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "original deployed").unwrap();
        let baseline = content_hash::hash_directory(&target).unwrap();
        fs::write(target.join("SKILL.md"), "USER EDIT keep me").unwrap();

        store.insert_skill(&guard_skill("skill-d", &source, "central-h2")).unwrap();
        store
            .insert_target(&SkillTargetRecord {
                id: "t4".to_string(),
                skill_id: "skill-d".to_string(),
                tool: "claude-code".to_string(),
                target_path: target.to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "target_ahead".to_string(), // already flagged
                synced_at: Some(1),
                last_error: Some("preserved user-edited target, skipped deploy".to_string()),
                source_hash: Some(baseline.clone()),
            })
            .unwrap();

        let desired = vec![ScenarioSyncTarget {
            skill_id: "skill-d".to_string(),
            skill_name: "skill-d".to_string(),
            tool: "claude-code".to_string(),
            source: source.clone(),
            target: target.clone(),
            mode: sync_engine::SyncMode::Copy,
            source_hash: Some("central-h2".to_string()),
        }];

        sync_desired_targets(&store, &desired).unwrap();

        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "USER EDIT keep me",
            "already-flagged target must remain preserved"
        );
        assert_eq!(
            count_backups(&base, "skill-d"),
            0,
            "already-flagged target must NOT be snapshotted again"
        );

        central_repo::set_test_base_dir_override(None);
    }

    /// Case 5: Symlink-mode targets are physically the central dir, so the
    /// guard must not engage — no snapshot, no target_ahead flag — even when
    /// the on-disk target looks "edited".
    #[test]
    fn symlink_mode_target_is_not_guarded() {
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();

        let source = central_repo::skills_dir().join("skill-e");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "central v2").unwrap();

        let target = tmp.path().join("agent-skills").join("skill-e");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "USER EDIT keep me").unwrap();
        let baseline = content_hash::hash_directory(&target).unwrap();
        // edited afterwards to differ from baseline
        fs::write(target.join("SKILL.md"), "USER EDIT changed again").unwrap();

        store.insert_skill(&guard_skill("skill-e", &source, "central-h2")).unwrap();
        store
            .insert_target(&SkillTargetRecord {
                id: "t5".to_string(),
                skill_id: "skill-e".to_string(),
                tool: "claude-code".to_string(),
                target_path: target.to_string_lossy().to_string(),
                mode: "symlink".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: Some(baseline.clone()),
            })
            .unwrap();

        let desired = vec![ScenarioSyncTarget {
            skill_id: "skill-e".to_string(),
            skill_name: "skill-e".to_string(),
            tool: "claude-code".to_string(),
            source: source.clone(),
            target: target.clone(),
            mode: sync_engine::SyncMode::Symlink,
            source_hash: Some("central-h2".to_string()),
        }];

        sync_desired_targets(&store, &desired).unwrap();

        // The guard is Copy-only: no spoke-ahead snapshot, and the record is
        // never flagged target_ahead for a symlink target.
        assert_eq!(
            count_backups(&base, "skill-e"),
            0,
            "symlink target must never be snapshotted by the guard"
        );
        let rec = store
            .get_targets_for_skill("skill-e")
            .unwrap()
            .into_iter()
            .find(|t| t.tool == "claude-code")
            .unwrap();
        assert_ne!(
            rec.status, "target_ahead",
            "symlink target must never be flagged target_ahead"
        );

        central_repo::set_test_base_dir_override(None);
    }

    /// NB-1 (real-data smoke): the whole guard rests on one invariant — an
    /// *unedited* spoke hashes back to the `source_hash` baseline. Every other
    /// guard test hand-seeds that baseline, so a future drift between
    /// `copy_dir_recursive` (what deploy writes) and `hash_directory` (what the
    /// guard reads) would slip through silently. Here the baseline and the
    /// deployed bytes are produced by a *real* `sync_desired_targets` run, so
    /// the two code paths must genuinely agree: a faithful copy that was never
    /// touched by the user must be overwritten (Deploy), not falsely protected.
    ///
    /// Mutation check: if the guard compared the target hash against a baseline
    /// that can never equal it (e.g. a hard-coded sentinel), this test would go
    /// red — the untouched target would be preserved instead of overwritten.
    #[test]
    fn real_deploy_then_unedited_target_is_overwritten() {
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();

        // Real central skill with a couple of content files.
        let source = central_repo::skills_dir().join("skill-real");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "central v1").unwrap();
        fs::write(source.join("ref.txt"), "shared reference").unwrap();
        // The genuine content hash a real skill record would carry.
        let h1 = content_hash::hash_directory(&source).unwrap();

        let target = tmp.path().join("agent-skills").join("skill-real");

        store.insert_skill(&guard_skill("skill-real", &source, &h1)).unwrap();

        // ── Run 1: REAL deploy. No hand-seeded target row/hash — the record's
        // source_hash and synced_at, and the target bytes, are all produced by
        // sync_desired_targets copying the real source. ──
        let desired_v1 = vec![ScenarioSyncTarget {
            skill_id: "skill-real".to_string(),
            skill_name: "skill-real".to_string(),
            tool: "claude-code".to_string(),
            source: source.clone(),
            target: target.clone(),
            mode: sync_engine::SyncMode::Copy,
            source_hash: Some(h1.clone()),
        }];
        sync_desired_targets(&store, &desired_v1).unwrap();

        // Sanity: the deploy really happened and produced a baseline record.
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "central v1"
        );
        let rec_after_deploy = store
            .get_targets_for_skill("skill-real")
            .unwrap()
            .into_iter()
            .find(|t| t.tool == "claude-code")
            .unwrap();
        assert_eq!(rec_after_deploy.source_hash.as_deref(), Some(h1.as_str()));
        let synced_at = rec_after_deploy.synced_at.unwrap();

        // ── Central content changes; the target dir is NOT edited by the user.
        // We only push the target mtime past synced_at so the cheap pre-filter
        // can't short-circuit — forcing the guard down the hash branch, where
        // it must find hash(faithful copy) == baseline and decide Deploy. ──
        fs::write(source.join("SKILL.md"), "central v2 UPDATED").unwrap();
        let h2 = content_hash::hash_directory(&source).unwrap();
        assert_ne!(h1, h2, "central content must have actually changed");
        set_mtime_ms(&target.join("SKILL.md"), synced_at + 10_000);
        set_mtime_ms(&target.join("ref.txt"), synced_at + 10_000);

        let desired_v2 = vec![ScenarioSyncTarget {
            skill_id: "skill-real".to_string(),
            skill_name: "skill-real".to_string(),
            tool: "claude-code".to_string(),
            source: source.clone(),
            target: target.clone(),
            mode: sync_engine::SyncMode::Copy,
            source_hash: Some(h2.clone()),
        }];
        sync_desired_targets(&store, &desired_v2).unwrap();

        // The faithful, unedited copy must be overwritten with the new central
        // content — the guard must NOT mistake a bumped mtime for a real edit.
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "central v2 UPDATED",
            "unedited spoke (hash == baseline) must be overwritten, not protected"
        );
        assert_eq!(
            count_backups(&base, "skill-real"),
            0,
            "no backup for an unedited faithful copy"
        );
        let rec_after = store
            .get_targets_for_skill("skill-real")
            .unwrap()
            .into_iter()
            .find(|t| t.tool == "claude-code")
            .unwrap();
        assert_ne!(
            rec_after.status, "target_ahead",
            "unedited target must not be flagged target_ahead"
        );

        central_repo::set_test_base_dir_override(None);
    }

    /// NB-4 (defensive branch): the guard only engages when BOTH `synced_at`
    /// and `source_hash` are present (a usable baseline). A row with a partial
    /// baseline — here `synced_at = Some`, `source_hash = None` (e.g. written
    /// before the hash column existed) — must fall straight through to a normal
    /// deploy, even if the on-disk target looks edited. No protection, no
    /// snapshot, no flag.
    #[test]
    fn partial_baseline_skips_guard_and_deploys() {
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();

        let source = central_repo::skills_dir().join("skill-partial");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "central v2").unwrap();

        let target = tmp.path().join("agent-skills").join("skill-partial");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "USER EDIT would-be-protected").unwrap();

        store
            .insert_skill(&guard_skill("skill-partial", &source, "central-h2"))
            .unwrap();
        store
            .insert_target(&SkillTargetRecord {
                id: "tp".to_string(),
                skill_id: "skill-partial".to_string(),
                tool: "claude-code".to_string(),
                target_path: target.to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: None, // partial baseline: no hash to compare against
            })
            .unwrap();

        let desired = vec![ScenarioSyncTarget {
            skill_id: "skill-partial".to_string(),
            skill_name: "skill-partial".to_string(),
            tool: "claude-code".to_string(),
            source: source.clone(),
            target: target.clone(),
            mode: sync_engine::SyncMode::Copy,
            source_hash: Some("central-h2".to_string()),
        }];

        sync_desired_targets(&store, &desired).unwrap();

        // Guard skipped (no baseline) → ordinary overwrite.
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "central v2",
            "partial-baseline target must be overwritten, guard must not engage"
        );
        assert_eq!(
            count_backups(&base, "skill-partial"),
            0,
            "guard skipped → no snapshot"
        );
        let rec = store
            .get_targets_for_skill("skill-partial")
            .unwrap()
            .into_iter()
            .find(|t| t.tool == "claude-code")
            .unwrap();
        assert_ne!(rec.status, "target_ahead", "guard skipped → never flagged");

        central_repo::set_test_base_dir_override(None);
    }
}

#[cfg(test)]
mod deploy_guard_decision_tests {
    use super::{deploy_guard_decision, DeployGuardDecision, SPOKE_MTIME_THRESHOLD_MS};

    // Hot-path contract (#248): a target whose newest content file predates
    // our last sync cannot have been edited since we wrote it, so the decision
    // must be Deploy *without* ever invoking the (expensive) hash closure.
    #[test]
    fn untouched_target_deploys_without_hashing() {
        let mut hashed = false;
        let decision = deploy_guard_decision(Some(1_000), 1_000, "baseline", "ok", || {
            hashed = true;
            Some("whatever".to_string())
        });
        assert_eq!(decision, DeployGuardDecision::Deploy);
        assert!(!hashed, "hot path must not hash an untouched target");
    }

    #[test]
    fn missing_mtime_deploys_without_hashing() {
        let mut hashed = false;
        let decision = deploy_guard_decision(None, 1_000, "baseline", "ok", || {
            hashed = true;
            Some("x".to_string())
        });
        assert_eq!(decision, DeployGuardDecision::Deploy);
        assert!(!hashed, "no mtime → treat as untouched, do not hash");
    }

    #[test]
    fn mtime_within_threshold_is_still_untouched() {
        let mut hashed = false;
        let decision = deploy_guard_decision(
            Some(1_000 + SPOKE_MTIME_THRESHOLD_MS),
            1_000,
            "baseline",
            "ok",
            || {
                hashed = true;
                Some("x".to_string())
            },
        );
        assert_eq!(decision, DeployGuardDecision::Deploy);
        assert!(
            !hashed,
            "mtime within THRESHOLD of synced_at counts as untouched"
        );
    }

    #[test]
    fn touched_but_content_matches_baseline_deploys() {
        // mtime bumped but content identical to what we deployed → not a real
        // edit, keep the original overwrite behavior (no false protection).
        let decision = deploy_guard_decision(Some(10_000), 1_000, "baseline", "ok", || {
            Some("baseline".to_string())
        });
        assert_eq!(decision, DeployGuardDecision::Deploy);
    }

    #[test]
    fn touched_and_edited_first_time_preserves_and_flags() {
        let decision = deploy_guard_decision(Some(10_000), 1_000, "baseline", "ok", || {
            Some("user-edit".to_string())
        });
        assert_eq!(decision, DeployGuardDecision::PreserveAndFlag);
    }

    #[test]
    fn touched_and_edited_when_already_flagged_skips_backup() {
        // Second run over an already-protected target: still preserve, but do
        // not snapshot again (avoid flooding backups/ on every preset apply).
        let decision =
            deploy_guard_decision(Some(10_000), 1_000, "baseline", "target_ahead", || {
                Some("user-edit".to_string())
            });
        assert_eq!(decision, DeployGuardDecision::PreserveNoBackup);
    }

    #[test]
    fn touched_but_unhashable_target_falls_back_to_deploy() {
        // If we can't hash the target (removed/unreadable between the mtime
        // read and the hash), fall back to the original overwrite behavior
        // rather than protecting a target we can't reason about.
        let decision =
            deploy_guard_decision(Some(10_000), 1_000, "baseline", "ok", || None);
        assert_eq!(decision, DeployGuardDecision::Deploy);
    }
}

#[cfg(test)]
mod skip_check_mode_tests {
    use super::skip_check_mode;
    use super::sync_engine::SyncMode;

    #[test]
    fn matching_modes_are_compatible() {
        assert!(matches!(
            skip_check_mode("symlink", SyncMode::Symlink),
            Some(SyncMode::Symlink)
        ));
        assert!(matches!(
            skip_check_mode("copy", SyncMode::Copy),
            Some(SyncMode::Copy)
        ));
    }

    #[test]
    fn copy_existing_with_symlink_desired_treated_as_copy() {
        // Windows fallback case (issue #153): record says copy because
        // symlink_dir failed previously. We accept that and let the hash
        // gate decide freshness, instead of re-attempting symlink and
        // triggering a full recopy on every startup.
        assert!(matches!(
            skip_check_mode("copy", SyncMode::Symlink),
            Some(SyncMode::Copy)
        ));
    }

    #[test]
    fn symlink_existing_with_copy_desired_is_incompatible() {
        // User flipped sync_mode setting from symlink to copy — the
        // on-disk symlink no longer reflects intent, must resync.
        assert!(skip_check_mode("symlink", SyncMode::Copy).is_none());
    }

    #[test]
    fn unknown_existing_mode_is_incompatible() {
        assert!(skip_check_mode("garbage", SyncMode::Symlink).is_none());
        assert!(skip_check_mode("", SyncMode::Copy).is_none());
    }
}
