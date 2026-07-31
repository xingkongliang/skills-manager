use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::Instant;

use super::{
    error::AppError,
    skill_store::{ScenarioRecord, SkillStore, SkillTargetRecord},
    sync_engine, tool_adapters,
    tool_service,
};

/// UI 与托盘会同时修改 `skill_targets` 和对应磁盘目录，必须共用同一把进程内锁。
///
/// UI 调用会等待锁，保证连续点击的 Preset 完整执行；托盘调用使用
/// `try_lock_preset_apply`，保留原有“已有任务时忽略重复点击”的交互语义。
static PRESET_APPLY_LOCK: Mutex<()> = Mutex::new(());

pub fn lock_preset_apply() -> MutexGuard<'static, ()> {
    PRESET_APPLY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn try_lock_preset_apply() -> Option<MutexGuard<'static, ()>> {
    match PRESET_APPLY_LOCK.try_lock() {
        Ok(guard) => Some(guard),
        Err(TryLockError::WouldBlock) => None,
        Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
    }
}

/// 将路径中最近的已存在祖先解析为真实位置，再接回尚未创建的尾段。
///
/// Agent 根可能位于 junction/symlink 下，而中间目录尚未创建。只尝试解析立即
/// 父目录会漏掉这种别名；逐级向上解析既保留同一物理根，也不会跟随目标 Skill
/// 自身的链接（否则两个独立 Agent 的链接都会被折叠到中央 Library）。
fn canonicalize_with_missing_tail(path: &Path) -> Option<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let mut cursor = absolute.as_path();
    let mut tail = Vec::new();

    loop {
        if let Ok(mut resolved) = std::fs::canonicalize(cursor) {
            for component in tail.iter().rev() {
                resolved.push(component);
            }
            return Some(resolved);
        }
        let name = cursor.file_name()?.to_os_string();
        tail.push(name);
        cursor = cursor.parent()?;
    }
}

#[cfg(windows)]
fn windows_path_key(path: &Path, fold_case: bool) -> String {
    let raw = path.to_string_lossy().replace('/', "\\");
    // `canonicalize` 返回 extended-length path，而词法回退通常没有该前缀；
    // 统一去除后，同一物理路径不会因为 `\\?\` 表示法不同而被拆成两组。
    let normalized = if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = raw.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        raw
    };
    if fold_case {
        normalized.to_lowercase()
    } else {
        normalized
    }
}

/// 返回用于共享目标判定的稳定键。
///
/// 已解析路径保留文件系统返回的精确大小写，避免把启用 per-directory case
/// sensitivity 的 NTFS 目录错误合并；只有完全无法解析祖先时才对 Windows 的
/// 词法回退做大小写折叠。
fn normalized_target_key(path: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_default()
            .join(path)
    };
    let canonical_parent = absolute
        .parent()
        .and_then(canonicalize_with_missing_tail)
        .and_then(|parent| absolute.file_name().map(|name| parent.join(name)))
        .map(|path| lexical_normalize(&path));
    #[cfg(windows)]
    {
        match canonical_parent {
            Some(path) => windows_path_key(&path, false),
            None => windows_path_key(&lexical_normalize(&absolute), true),
        }
    }
    #[cfg(not(windows))]
    {
        // Unix 允许反斜杠作为普通文件名字符，不能将它误当作分隔符，
        // 否则 `a/b` 与 `a\b` 会错误共享同一个同步目标。
        canonical_parent
            .unwrap_or_else(|| lexical_normalize(&absolute))
            .to_string_lossy()
            .into_owned()
    }
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

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
            } else if existing.status == "ok" {
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
        "sync_desired_targets: {} targets in {} ms (synced={synced_count}, skipped={skipped_count}, failed={failed_count})",
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BatchApplyFailure {
    pub skill_id: String,
    pub tool_key: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BatchApplyReport {
    pub applied: usize,
    pub skipped: usize,
    pub failures: Vec<BatchApplyFailure>,
}

impl BatchApplyReport {
    fn push_failure(
        &mut self,
        skill_id: impl Into<String>,
        tool_key: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.failures.push(BatchApplyFailure {
            skill_id: skill_id.into(),
            tool_key: tool_key.into(),
            message: message.into(),
        });
    }
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

    let report = apply_skills_to_tools_with_report(store, skill_ids, tool_keys, mode)?;
    for failure in &report.failures {
        log::warn!(
            "apply_skills_to_tools: skill {} / tool {} failed: {}",
            failure.skill_id,
            failure.tool_key,
            failure.message
        );
    }
    Ok(())
}

/// 精确批量应用并返回逐项结果。与托盘保留的兼容入口不同，这个入口会在
/// 任何磁盘写入之前完整校验目标 Agent，避免混合有效/无效目标导致部分提交。
pub fn apply_skills_to_tools_with_report(
    store: &SkillStore,
    skill_ids: &[String],
    tool_keys: &[String],
    mode: BatchApplyMode,
) -> Result<BatchApplyReport, AppError> {
    let unique_skill_ids = deduplicate_strings(skill_ids);
    if unique_skill_ids.is_empty() {
        return Ok(BatchApplyReport::default());
    }

    match mode {
        BatchApplyMode::Add => {
            let adapters = validate_batch_tools_for_add(store, tool_keys)?;
            apply_add_with_report(store, &unique_skill_ids, &adapters)
        }
        BatchApplyMode::Remove => {
            let unique_tool_keys = validate_batch_tools_for_remove(store, tool_keys)?;
            apply_remove_with_report(store, &unique_skill_ids, &unique_tool_keys)
        }
    }
}

fn deduplicate_strings(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .iter()
        .filter(|value| seen.insert((*value).clone()))
        .cloned()
        .collect()
}

fn validate_batch_tools_for_add(
    store: &SkillStore,
    tool_keys: &[String],
) -> Result<Vec<tool_adapters::ToolAdapter>, AppError> {
    let unique_keys = deduplicate_strings(tool_keys);
    if unique_keys.is_empty() {
        return Err(AppError::invalid_input(
            "At least one target Agent is required",
        ));
    }

    let disabled = tool_service::get_disabled_tools(store);
    let mut adapters = Vec::with_capacity(unique_keys.len());
    for key in unique_keys {
        let adapter = tool_adapters::find_adapter_with_store(store, &key)
            .ok_or_else(|| AppError::invalid_input(format!("Unknown Agent: {key}")))?;
        if !adapter.is_installed() {
            return Err(AppError::invalid_input(format!(
                "{} is not installed",
                adapter.display_name
            )));
        }
        if disabled.contains(&key) {
            return Err(AppError::invalid_input(format!(
                "{} is disabled",
                adapter.display_name
            )));
        }
        adapters.push(adapter);
    }
    Ok(adapters)
}

/// Remove 依据数据库中的既有目标执行，不要求 Agent 仍被检测为已安装或启用。
/// 这样用户禁用 Agent、卸载客户端或临时断开网络盘后，仍能清理其受管记录。
fn validate_batch_tools_for_remove(
    store: &SkillStore,
    tool_keys: &[String],
) -> Result<Vec<String>, AppError> {
    let unique_keys = deduplicate_strings(tool_keys);
    if unique_keys.is_empty() {
        return Err(AppError::invalid_input(
            "At least one target Agent is required",
        ));
    }
    for key in &unique_keys {
        if tool_adapters::find_adapter_with_store(store, key).is_none() {
            return Err(AppError::invalid_input(format!("Unknown Agent: {key}")));
        }
    }
    Ok(unique_keys)
}

#[derive(Debug)]
struct StagedTarget {
    original: PathBuf,
    staged: PathBuf,
}

/// 在 Agent 的 Skills 扫描根之外创建同卷暂存位置。
///
/// 暂存项必须与目标同卷，才能用 `rename` 完成原子切换；同时不能放在
/// `skills_dir` 内，否则清理失败后残留的 `SKILL.md` 仍可能被 Agent 发现。
fn unique_staging_path(target: &Path, purpose: &str) -> Result<PathBuf, AppError> {
    let skills_root = target
        .parent()
        .ok_or_else(|| AppError::invalid_input("Target path has no Skills root"))?;
    // Agent 根可能是跨卷 junction/symlink。必须跟随根目录本身，确保暂存项
    // 和真实目标位于同一卷；继续使用词法父目录会让 Windows rename 返回
    // ERROR_NOT_SAME_DEVICE。
    let absolute_root = if skills_root.is_absolute() {
        skills_root.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_default()
            .join(skills_root)
    };
    let physical_root = canonicalize_with_missing_tail(&absolute_root)
        .unwrap_or_else(|| lexical_normalize(&absolute_root));
    let staging_parent = physical_root
        .parent()
        .ok_or_else(|| AppError::invalid_input("Skills root has no safe staging parent"))?;
    let staging_root = staging_parent.join(".skills-manager-staging");
    std::fs::create_dir_all(&staging_root).map_err(AppError::io)?;
    Ok(staging_root.join(format!(
        "{purpose}-{}",
        uuid::Uuid::new_v4()
    )))
}

fn path_exists_strict(path: &Path) -> Result<bool, AppError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(AppError::io(error)),
    }
}

fn stage_existing_target(path: &Path, purpose: &str) -> Result<Option<StagedTarget>, AppError> {
    if !path_exists_strict(path)? {
        return Ok(None);
    }
    let staged = unique_staging_path(path, purpose)?;
    std::fs::rename(path, &staged).map_err(AppError::io)?;
    Ok(Some(StagedTarget {
        original: path.to_path_buf(),
        staged,
    }))
}

fn remove_path_if_present(path: &Path) -> Result<(), AppError> {
    if path_exists_strict(path)? {
        sync_engine::remove_target(path).map_err(AppError::io)?;
    }
    Ok(())
}

fn cleanup_staging_parent(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::remove_dir(parent);
    }
}

fn discard_staged_target(staged: StagedTarget) {
    if let Err(error) = remove_path_if_present(&staged.staged) {
        // 暂存位置不在任何 Agent 的扫描根内；清理失败不会重新启用 Skill，
        // 但需要保留日志，便于用户手动处理权限或占用问题。
        log::warn!(
            "Failed to clean Preset staging path {}: {error}",
            staged.staged.display()
        );
    }
    cleanup_staging_parent(&staged.staged);
}

fn restore_staged_target(staged: &StagedTarget) -> Result<(), AppError> {
    remove_path_if_present(&staged.original)?;
    std::fs::rename(&staged.staged, &staged.original).map_err(AppError::io)?;
    cleanup_staging_parent(&staged.staged);
    Ok(())
}

fn restore_staged_targets(staged: &[StagedTarget]) -> Vec<String> {
    staged
        .iter()
        .rev()
        .filter_map(|entry| {
            restore_staged_target(entry).err().map(|error| {
                format!(
                    "failed to restore {}: {error}",
                    entry.original.display()
                )
            })
        })
        .collect()
}

fn target_record_is_current(
    record: &SkillTargetRecord,
    source: &Path,
    desired_target: &Path,
    desired_mode: sync_engine::SyncMode,
    current_source_hash: Option<&str>,
) -> bool {
    record.status == "ok"
        && normalized_target_key(Path::new(&record.target_path))
            == normalized_target_key(desired_target)
        && skip_check_mode(&record.mode, desired_mode).is_some_and(|check_mode| {
            sync_engine::is_target_current(
                source,
                Path::new(&record.target_path),
                check_mode,
                record.source_hash.as_deref(),
                current_source_hash,
            )
        })
}

fn push_group_failure(
    report: &mut BatchApplyReport,
    skill_id: &str,
    members: &[(&tool_adapters::ToolAdapter, PathBuf)],
    message: impl Into<String>,
) {
    let message = message.into();
    for (adapter, _) in members {
        report.push_failure(skill_id, &adapter.key, &message);
    }
}

fn restore_after_add_failure(
    desired_backup: Option<&StagedTarget>,
    old_backups: &[StagedTarget],
    desired_target: &Path,
) -> Vec<String> {
    let mut failures = Vec::new();
    if let Err(error) = remove_path_if_present(desired_target) {
        failures.push(format!(
            "failed to remove the uncommitted target {}: {error}",
            desired_target.display()
        ));
    }
    if let Some(backup) = desired_backup {
        if let Err(error) = restore_staged_target(backup) {
            failures.push(format!(
                "failed to restore the previous target {}: {error}",
                backup.original.display()
            ));
        }
    }
    failures.extend(restore_staged_targets(old_backups));
    failures
}

fn apply_add_with_report(
    store: &SkillStore,
    skill_ids: &[String],
    adapters: &[tool_adapters::ToolAdapter],
) -> Result<BatchApplyReport, AppError> {
    let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
    let mut report = BatchApplyReport::default();

    for skill_id in skill_ids {
        let skill = match store.get_skill_by_id(skill_id).map_err(AppError::db)? {
            Some(skill) => skill,
            None => {
                for adapter in adapters {
                    report.push_failure(skill_id, &adapter.key, "Skill not found");
                }
                continue;
            }
        };

        let source = PathBuf::from(&skill.central_path);
        let target_name = sync_engine::target_dir_name(&source, &skill.name);
        let mut groups: Vec<(String, Vec<(&tool_adapters::ToolAdapter, PathBuf)>)> = Vec::new();
        for adapter in adapters {
            let target = adapter.skills_dir().join(&target_name);
            let target_key = normalized_target_key(&target);
            if let Some((_, members)) = groups.iter_mut().find(|(key, _)| *key == target_key) {
                members.push((adapter, target));
            } else {
                groups.push((target_key, vec![(adapter, target)]));
            }
        }

        for (target_key, members) in groups {
            let all_targets = store.get_all_targets().map_err(AppError::db)?;
            let first_target = &members[0].1;
            let first_mode = sync_engine::sync_mode_for_tool(
                &members[0].0.key,
                configured_mode.as_deref(),
            );
            if let Err(error) = sync_engine::ensure_dst_not_inside_src(&source, first_target) {
                push_group_failure(
                    &mut report,
                    skill_id,
                    &members,
                    format!("Unsafe source/target layout: {error}"),
                );
                continue;
            }

            let physical_records: Vec<&SkillTargetRecord> = all_targets
                .iter()
                .filter(|record| {
                    normalized_target_key(Path::new(&record.target_path)) == target_key
                })
                .collect();
            if physical_records
                .iter()
                .any(|record| record.skill_id != *skill_id)
            {
                push_group_failure(
                    &mut report,
                    skill_id,
                    &members,
                    "Target path is already managed by another Skill",
                );
                continue;
            }

            let mut current_tools = HashSet::new();
            for (adapter, target) in &members {
                if all_targets.iter().any(|record| {
                    record.skill_id == *skill_id
                        && record.tool == adapter.key
                        && target_record_is_current(
                            record,
                            &source,
                            target,
                            sync_engine::sync_mode_for_tool(
                                &adapter.key,
                                configured_mode.as_deref(),
                            ),
                            skill.content_hash.as_deref(),
                        )
                }) {
                    current_tools.insert(adapter.key.clone());
                }
            }
            report.skipped += current_tools.len();
            let pending_members: Vec<(&tool_adapters::ToolAdapter, PathBuf)> = members
                .iter()
                .filter(|(adapter, _)| !current_tools.contains(&adapter.key))
                .map(|(adapter, target)| (*adapter, target.clone()))
                .collect();
            if pending_members.is_empty() {
                continue;
            }

            // 另一个共享同一路径的 Agent 已证明物理内容是当前版本时，只需补齐
            // 本组记录，不应重复替换该目录。
            let current_physical = physical_records.iter().find(|record| {
                target_record_is_current(
                    record,
                    &source,
                    first_target,
                    first_mode,
                    skill.content_hash.as_deref(),
                )
            });

            let target_exists = match path_exists_strict(first_target) {
                Ok(exists) => exists,
                Err(error) => {
                    push_group_failure(&mut report, skill_id, &pending_members, error.to_string());
                    continue;
                }
            };
            if current_physical.is_none() && target_exists && physical_records.is_empty() {
                push_group_failure(
                    &mut report,
                    skill_id,
                    &pending_members,
                    "Target directory already exists but is not managed by Skills Manager",
                );
                continue;
            }

            // 路径迁移时先把不再被其他记录引用的旧目标移出扫描根；数据库提交
            // 失败会按相反顺序恢复，避免“旧副本已删、新副本未登记”。
            let pending_tool_keys: HashSet<&str> = pending_members
                .iter()
                .map(|(adapter, _)| adapter.key.as_str())
                .collect();
            let mut old_backups = Vec::new();
            let mut staged_old_keys = HashSet::new();
            let mut staging_error = None;
            for record in all_targets.iter().filter(|record| {
                record.skill_id == *skill_id && pending_tool_keys.contains(record.tool.as_str())
            }) {
                let old_path = PathBuf::from(&record.target_path);
                let old_key = normalized_target_key(&old_path);
                if old_key == target_key || !staged_old_keys.insert(old_key.clone()) {
                    continue;
                }
                let referenced_elsewhere = all_targets.iter().any(|other| {
                    other.id != record.id
                        && normalized_target_key(Path::new(&other.target_path)) == old_key
                        && !(other.skill_id == *skill_id
                            && pending_tool_keys.contains(other.tool.as_str()))
                });
                if referenced_elsewhere {
                    continue;
                }
                match stage_existing_target(&old_path, "old") {
                    Ok(Some(staged)) => old_backups.push(staged),
                    Ok(None) => {}
                    Err(error) => {
                        staging_error = Some(error);
                        break;
                    }
                }
            }
            if let Some(error) = staging_error {
                let restore_failures = restore_staged_targets(&old_backups);
                let suffix = if restore_failures.is_empty() {
                    String::new()
                } else {
                    format!("; {}", restore_failures.join("; "))
                };
                push_group_failure(
                    &mut report,
                    skill_id,
                    &pending_members,
                    format!("Failed to stage the previous target: {error}{suffix}"),
                );
                continue;
            }

            let mut desired_backup = None;
            let mut incoming_path = None;
            let mode_name = if let Some(existing) = current_physical {
                existing.mode.clone()
            } else {
                let incoming = match unique_staging_path(first_target, "incoming") {
                    Ok(path) => path,
                    Err(error) => {
                        let restore_failures = restore_staged_targets(&old_backups);
                        push_group_failure(
                            &mut report,
                            skill_id,
                            &pending_members,
                            format!(
                                "Failed to prepare the new target: {error}{}",
                                if restore_failures.is_empty() {
                                    String::new()
                                } else {
                                    format!("; {}", restore_failures.join("; "))
                                }
                            ),
                        );
                        continue;
                    }
                };
                let actual_mode = match sync_engine::sync_skill(&source, &incoming, first_mode) {
                    Ok(mode) => mode,
                    Err(error) => {
                        let _ = remove_path_if_present(&incoming);
                        cleanup_staging_parent(&incoming);
                        let restore_failures = restore_staged_targets(&old_backups);
                        push_group_failure(
                            &mut report,
                            skill_id,
                            &pending_members,
                            format!(
                                "Failed to build the new target: {error}{}",
                                if restore_failures.is_empty() {
                                    String::new()
                                } else {
                                    format!("; {}", restore_failures.join("; "))
                                }
                            ),
                        );
                        continue;
                    }
                };

                if let Some(parent) = first_target.parent() {
                    if let Err(error) = std::fs::create_dir_all(parent) {
                        let _ = remove_path_if_present(&incoming);
                        cleanup_staging_parent(&incoming);
                        let restore_failures = restore_staged_targets(&old_backups);
                        push_group_failure(
                            &mut report,
                            skill_id,
                            &pending_members,
                            format!(
                                "Failed to create the Agent Skills directory: {error}{}",
                                if restore_failures.is_empty() {
                                    String::new()
                                } else {
                                    format!("; {}", restore_failures.join("; "))
                                }
                            ),
                        );
                        continue;
                    }
                }

                desired_backup = match stage_existing_target(first_target, "replaced") {
                    Ok(staged) => staged,
                    Err(error) => {
                        let _ = remove_path_if_present(&incoming);
                        cleanup_staging_parent(&incoming);
                        let restore_failures = restore_staged_targets(&old_backups);
                        push_group_failure(
                            &mut report,
                            skill_id,
                            &pending_members,
                            format!(
                                "Failed to stage the current target: {error}{}",
                                if restore_failures.is_empty() {
                                    String::new()
                                } else {
                                    format!("; {}", restore_failures.join("; "))
                                }
                            ),
                        );
                        continue;
                    }
                };
                if let Err(error) = std::fs::rename(&incoming, first_target) {
                    let mut restore_failures = Vec::new();
                    if let Some(backup) = desired_backup.as_ref() {
                        if let Err(restore_error) = restore_staged_target(backup) {
                            restore_failures.push(restore_error.to_string());
                        }
                    }
                    restore_failures.extend(restore_staged_targets(&old_backups));
                    let _ = remove_path_if_present(&incoming);
                    cleanup_staging_parent(&incoming);
                    push_group_failure(
                        &mut report,
                        skill_id,
                        &pending_members,
                        format!(
                            "Failed to activate the new target: {error}{}",
                            if restore_failures.is_empty() {
                                String::new()
                            } else {
                                format!("; restore failed: {}", restore_failures.join("; "))
                            }
                        ),
                    );
                    continue;
                }
                incoming_path = Some(incoming);
                actual_mode.as_str().to_string()
            };

            let now = chrono::Utc::now().timestamp_millis();
            let expected_records: Vec<SkillTargetRecord> = all_targets
                .iter()
                .filter(|record| {
                    record.skill_id == *skill_id
                        && pending_tool_keys.contains(record.tool.as_str())
                })
                .cloned()
                .collect();
            let records: Vec<SkillTargetRecord> = pending_members
                .iter()
                .map(|(adapter, target)| {
                    let existing = all_targets.iter().find(|record| {
                        record.skill_id == *skill_id && record.tool == adapter.key
                    });
                    SkillTargetRecord {
                        id: existing
                            .map(|record| record.id.clone())
                            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                        skill_id: skill_id.clone(),
                        tool: adapter.key.clone(),
                        target_path: target.to_string_lossy().to_string(),
                        mode: mode_name.clone(),
                        status: "ok".to_string(),
                        synced_at: Some(now),
                        last_error: None,
                        source_hash: skill.content_hash.clone(),
                    }
                })
                .collect();

            if let Err(error) =
                store.upsert_targets_if_unchanged(&records, &expected_records)
            {
                let restore_failures = if current_physical.is_some() {
                    restore_staged_targets(&old_backups)
                } else {
                    restore_after_add_failure(
                        desired_backup.as_ref(),
                        &old_backups,
                        first_target,
                    )
                };
                if let Some(incoming) = incoming_path.as_ref() {
                    cleanup_staging_parent(incoming);
                }
                push_group_failure(
                    &mut report,
                    skill_id,
                    &pending_members,
                    format!(
                        "Failed to record target: {error}{}",
                        if restore_failures.is_empty() {
                            String::new()
                        } else {
                            format!("; restore failed: {}", restore_failures.join("; "))
                        }
                    ),
                );
                continue;
            }

            if let Some(backup) = desired_backup {
                discard_staged_target(backup);
            }
            for backup in old_backups {
                discard_staged_target(backup);
            }
            if let Some(incoming) = incoming_path.as_ref() {
                cleanup_staging_parent(incoming);
            }
            report.applied += records.len();
        }
    }

    log::info!(
        "apply_skills_to_tools_with_report(Add): skills={} tools={} applied={} skipped={} failed={}",
        skill_ids.len(),
        adapters.len(),
        report.applied,
        report.skipped,
        report.failures.len(),
    );
    Ok(report)
}

fn apply_remove_with_report(
    store: &SkillStore,
    skill_ids: &[String],
    tool_keys: &[String],
) -> Result<BatchApplyReport, AppError> {
    let mut report = BatchApplyReport::default();
    let all_targets = store.get_all_targets().map_err(AppError::db)?;
    let requested_pairs: HashSet<(String, String)> = skill_ids
        .iter()
        .flat_map(|skill_id| {
            tool_keys
                .iter()
                .map(move |tool_key| (skill_id.clone(), tool_key.clone()))
        })
        .collect();
    let selected: Vec<SkillTargetRecord> = all_targets
        .iter()
        .filter(|record| {
            requested_pairs.contains(&(record.skill_id.clone(), record.tool.clone()))
        })
        .cloned()
        .collect();
    report.skipped = requested_pairs.len().saturating_sub(selected.len());

    let selected_ids: HashSet<String> = selected.iter().map(|record| record.id.clone()).collect();
    let mut groups: Vec<(String, Vec<SkillTargetRecord>)> = Vec::new();
    for record in selected {
        let key = normalized_target_key(Path::new(&record.target_path));
        if let Some((_, records)) = groups.iter_mut().find(|(existing, _)| *existing == key) {
            records.push(record);
        } else {
            groups.push((key, vec![record]));
        }
    }

    for (path_key, records) in groups {
        let physical_path = PathBuf::from(&records[0].target_path);
        let referenced_elsewhere = all_targets.iter().any(|record| {
            !selected_ids.contains(&record.id)
                && normalized_target_key(Path::new(&record.target_path)) == path_key
        });
        let staged = if referenced_elsewhere {
            None
        } else {
            match stage_existing_target(&physical_path, "removed") {
                Ok(staged) => staged,
                Err(error) => {
                    for record in &records {
                        report.push_failure(&record.skill_id, &record.tool, error.to_string());
                    }
                    continue;
                }
            }
        };

        if let Err(error) = store.delete_targets_exact_atomic(&records) {
            let restore_note = staged
                .as_ref()
                .and_then(|entry| restore_staged_target(entry).err())
                .map(|restore| format!("; restore failed: {restore}"))
                .unwrap_or_default();
            for record in &records {
                report.push_failure(
                    &record.skill_id,
                    &record.tool,
                    format!("Failed to delete target record: {error}{restore_note}"),
                );
            }
            continue;
        }

        if let Some(staged) = staged {
            // 数据库提交后再检查一次，若有新的共享引用出现，则把物理目标恢复；
            // 读取失败时恢复原记录和目录，破坏性操作必须 fail closed。
            match store.get_all_targets() {
                Ok(remaining) => {
                    let newly_referenced = remaining.iter().any(|record| {
                        normalized_target_key(Path::new(&record.target_path)) == path_key
                    });
                    if newly_referenced {
                        match path_exists_strict(&physical_path) {
                            Ok(false) => {
                                if let Err(error) = restore_staged_target(&staged) {
                                    let row_restore = store
                                        .upsert_targets_if_unchanged(&records, &[])
                                        .err();
                                    for record in &records {
                                        report.push_failure(
                                            &record.skill_id,
                                            &record.tool,
                                            format!(
                                                "Failed to restore a newly referenced shared target: {error}{}",
                                                row_restore
                                                    .as_ref()
                                                    .map(|restore| format!(
                                                        "; record restore failed: {restore}"
                                                    ))
                                                    .unwrap_or_default()
                                            ),
                                        );
                                    }
                                    continue;
                                }
                            }
                            Ok(true) => discard_staged_target(staged),
                            Err(error) => {
                                let row_restore = store
                                    .upsert_targets_if_unchanged(&records, &[])
                                    .err();
                                let path_restore = restore_staged_target(&staged).err();
                                let mut suffix = Vec::new();
                                if let Some(restore) = row_restore {
                                    suffix.push(format!("record restore failed: {restore}"));
                                }
                                if let Some(restore) = path_restore {
                                    suffix.push(format!("path restore failed: {restore}"));
                                }
                                for record in &records {
                                    report.push_failure(
                                        &record.skill_id,
                                        &record.tool,
                                        format!(
                                            "Failed to verify a newly referenced shared target: {error}{}",
                                            if suffix.is_empty() {
                                                String::new()
                                            } else {
                                                format!("; {}", suffix.join("; "))
                                            }
                                        ),
                                    );
                                }
                                continue;
                            }
                        }
                    } else {
                        discard_staged_target(staged);
                    }
                }
                Err(error) => {
                    let row_restore = store
                        .upsert_targets_if_unchanged(&records, &[])
                        .err();
                    let path_restore = restore_staged_target(&staged).err();
                    let mut suffix = Vec::new();
                    if let Some(restore) = row_restore {
                        suffix.push(format!("record restore failed: {restore}"));
                    }
                    if let Some(restore) = path_restore {
                        suffix.push(format!("path restore failed: {restore}"));
                    }
                    for record in &records {
                        report.push_failure(
                            &record.skill_id,
                            &record.tool,
                            format!(
                                "Failed to verify shared target after removal: {error}{}",
                                if suffix.is_empty() {
                                    String::new()
                                } else {
                                    format!("; {}", suffix.join("; "))
                                }
                            ),
                        );
                    }
                    continue;
                }
            }
        }
        report.applied += records.len();
    }

    log::info!(
        "apply_skills_to_tools_with_report(Remove): skills={} tools={} applied={} skipped={} failed={}",
        skill_ids.len(),
        tool_keys.len(),
        report.applied,
        report.skipped,
        report.failures.len(),
    );
    Ok(report)
}

fn ensure_remove_report_succeeded(report: BatchApplyReport) -> Result<BatchApplyReport, AppError> {
    if report.failures.is_empty() {
        return Ok(report);
    }
    Err(AppError::io(
        report
            .failures
            .iter()
            .map(|failure| {
                format!(
                    "{}/{}: {}",
                    failure.skill_id, failure.tool_key, failure.message
                )
            })
            .collect::<Vec<_>>()
            .join("; "),
    ))
}

/// 从单个 Agent 移除一个受管 Skill，并保留仍被其他 Agent 引用的共享目录。
/// 调用方必须持有 [`lock_preset_apply`]，以便与批量 Preset 写入串行。
pub fn remove_skill_target_preserving_shared_path(
    store: &SkillStore,
    skill_id: &str,
    tool_key: &str,
) -> Result<bool, AppError> {
    let report = apply_remove_with_report(
        store,
        &[skill_id.to_string()],
        &[tool_key.to_string()],
    )?;
    let report = ensure_remove_report_succeeded(report)?;
    Ok(report.applied == 1)
}

/// 安全移除一个 Agent 的全部受管目标；共享物理目录只删除该 Agent 的记录。
/// 调用方必须持有 [`lock_preset_apply`]。
pub fn remove_all_skill_targets_for_tool(
    store: &SkillStore,
    tool_key: &str,
) -> Result<(), AppError> {
    let skill_ids = store
        .get_all_targets()
        .map_err(AppError::db)?
        .into_iter()
        .filter(|target| target.tool == tool_key)
        .map(|target| target.skill_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if skill_ids.is_empty() {
        return Ok(());
    }
    let report = apply_remove_with_report(store, &skill_ids, &[tool_key.to_string()])?;
    ensure_remove_report_succeeded(report).map(|_| ())
}

#[cfg(test)]
mod preset_apply_lock_tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn preset_apply_lock_serializes_waiting_callers() {
        let first_guard = lock_preset_apply();
        let (acquired_tx, acquired_rx) = mpsc::channel();

        let waiter = thread::spawn(move || {
            let _second_guard = lock_preset_apply();
            acquired_tx.send(()).unwrap();
        });

        // 第二个调用方必须在首个批处理释放锁之前保持等待，避免目录和
        // skill_targets 被两个桌面命令交错改写。
        assert!(acquired_rx.recv_timeout(Duration::from_millis(100)).is_err());
        drop(first_guard);
        acquired_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("waiting preset apply did not acquire the released lock");
        waiter.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn normalized_target_key_preserves_backslash_filename_on_unix() {
        let tmp = tempfile::tempdir().unwrap();
        let nested_parent = tmp.path().join("agent");
        std::fs::create_dir_all(&nested_parent).unwrap();

        let slash_path = nested_parent.join("skill");
        let backslash_path = tmp.path().join(r"agent\skill");

        assert_ne!(
            normalized_target_key(&slash_path),
            normalized_target_key(&backslash_path),
            "Unix backslashes are filename characters, not separators"
        );
    }
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
