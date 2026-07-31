use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;
use tauri::State;

use crate::core::{
    error::AppError,
    scenario_service::{self, BatchApplyMode},
    skill_store::{ScenarioRecord, SkillStore},
    sync_metadata, tool_adapters,
    timing::should_log_first_or_slow,
};

fn refresh_tray_menu_best_effort(app: &tauri::AppHandle) {
    if let Err(err) = crate::refresh_tray_menu(app) {
        log::warn!("Failed to refresh tray menu after preset mutation: {err}");
    }
}

/// Sync a skill's files to all enabled tool adapter directories for the given preset.
/// Only performs sync if the preset is the currently active one.
pub(crate) fn sync_skill_to_active_preset(
    store: &SkillStore,
    scenario_id: &str,
    skill_id: &str,
) -> Result<(), AppError> {
    scenario_service::sync_skill_to_active_scenario(store, scenario_id, skill_id)
}

#[derive(Debug, Serialize)]
pub struct PresetDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub sort_order: i32,
    pub skill_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

static GET_PRESETS_FIRST_CALL: AtomicBool = AtomicBool::new(true);

#[tauri::command]
pub async fn get_presets(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<PresetDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let start = Instant::now();
        let scenarios = store.get_all_scenarios().map_err(AppError::db)?;
        let count = scenarios.len();
        let mut result = Vec::new();
        for s in scenarios {
            let skill_count = store.count_skills_for_scenario(&s.id).unwrap_or(0);
            result.push(PresetDto {
                id: s.id,
                name: s.name,
                description: s.description,
                icon: s.icon,
                sort_order: s.sort_order,
                skill_count,
                created_at: s.created_at,
                updated_at: s.updated_at,
            });
        }
        let elapsed_ms = start.elapsed().as_millis();
        if should_log_first_or_slow(&GET_PRESETS_FIRST_CALL, elapsed_ms, 100) {
            log::info!("get_presets: {count} presets in {elapsed_ms} ms");
        }
        Ok(result)
    })
    .await?
}

#[tauri::command]
pub async fn get_active_preset(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Option<PresetDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let active_id = store.get_active_scenario_id().map_err(AppError::db)?;

        if let Some(id) = active_id {
            let scenarios = store.get_all_scenarios().map_err(AppError::db)?;
            if let Some(s) = scenarios.into_iter().find(|s| s.id == id) {
                let count = store.count_skills_for_scenario(&s.id).unwrap_or(0);
                return Ok(Some(PresetDto {
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
pub async fn create_preset(
    app: tauri::AppHandle,
    name: String,
    description: Option<String>,
    icon: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<PresetDto, AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        // 创建新 Preset 会撤销旧活动 Preset 的磁盘目标；必须与 UI、托盘
        // 及其他 Agent 配置命令串行，避免只完成一半的切换。
        let _guard = scenario_service::lock_preset_apply();
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

        Ok(PresetDto {
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
pub async fn update_preset(
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
pub async fn delete_preset(
    app: tauri::AppHandle,
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        // 删除活动 Preset 同时包含撤销旧目标和应用后继 Preset，两步必须
        // 作为一个受锁保护的工作区变更完成。
        let _guard = scenario_service::lock_preset_apply();
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

/// Apply a preset to the default targets (all enabled agent globals).
///
/// This is the explicit user-initiated action introduced in v1.16. It performs
/// the same disk-writing work as the legacy [`switch_preset`] command but is
/// only invoked when the user clicks "Apply to Default" — sidebar/command-palette
/// preset clicks no longer call this.
#[tauri::command]
pub async fn apply_preset_to_default(
    app: tauri::AppHandle,
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    apply_preset_to_default_impl(app, id, store.inner().clone()).await
}

/// Legacy command kept for the CLI and backward compatibility. New callers
/// should use [`apply_preset_to_default`] (or [`apply_preset_to_coding_agents`]
/// for the workspace-scoped variant the tray now uses).
#[tauri::command]
pub async fn switch_preset(
    app: tauri::AppHandle,
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    apply_preset_to_default_impl(app, id, store.inner().clone()).await
}

async fn apply_preset_to_default_impl(
    app: tauri::AppHandle,
    id: String,
    store: Arc<SkillStore>,
) -> Result<(), AppError> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        // 旧的全局应用入口同样会改写 `skill_targets` 与磁盘目录，必须与
        // PresetBar 和托盘共用互斥锁，不能留下可交错的兼容路径。
        let _guard = scenario_service::lock_preset_apply();
        scenario_service::apply_scenario_to_default(&store, &id)
    })
    .await?;
    if result.is_ok() {
        refresh_tray_menu_best_effort(&app);
    }
    result
}

#[tauri::command]
pub async fn add_skill_to_preset(
    app: tauri::AppHandle,
    skill_id: String,
    preset_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        sync_metadata::with_repo_lock("add skill to scenario", || {
            store.add_skill_to_scenario(&preset_id, &skill_id)?;
            sync_metadata::write_all_from_db_unlocked(&store)
        })
        .map_err(AppError::db)?;
        // Membership-only edit. We intentionally do NOT sync to disk here,
        // even when this preset happens to be the legacy `active_scenario_id`,
        // because in the post-v1.16 model presets are curation labels, not
        // implicit deployment switches. Users apply presets explicitly via
        // PresetBar / the tray, which is where the actual write happens.
        Ok(())
    })
    .await?;
    if result.is_ok() {
        refresh_tray_menu_best_effort(&app);
    }
    result
}

#[tauri::command]
pub async fn remove_skill_from_preset(
    app: tauri::AppHandle,
    skill_id: String,
    preset_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        sync_metadata::with_repo_lock("remove skill from scenario", || {
            store.remove_skill_from_scenario(&preset_id, &skill_id)?;
            sync_metadata::write_all_from_db_unlocked(&store)
        })
        .map_err(AppError::db)?;
        // Same rationale as add_skill_to_preset: editing preset membership
        // never wipes on-disk skill targets. To remove a skill from a coding
        // agent the caller goes through PresetBar / the tray (or the explicit
        // per-skill unsync command).
        Ok(())
    })
    .await?;
    if result.is_ok() {
        refresh_tray_menu_best_effort(&app);
    }
    result
}

#[tauri::command]
pub async fn reorder_presets(
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
pub async fn get_preset_skill_order(
    preset_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<String>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .get_skill_ids_for_scenario(&preset_id)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn reorder_preset_skills(
    preset_id: String,
    skill_ids: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        sync_metadata::with_repo_lock("reorder scenario skills", || {
            store.reorder_scenario_skills(&preset_id, &skill_ids)?;
            sync_metadata::write_all_from_db_unlocked(&store)
        })
        .map_err(AppError::db)
    })
    .await?
}

// ── Internal helpers ──

pub(crate) fn sync_scenario_skills(store: &SkillStore, scenario_id: &str) -> Result<(), AppError> {
    scenario_service::sync_scenario_skills(store, scenario_id)
}

pub(crate) fn unsync_scenario_skills(
    store: &SkillStore,
    scenario_id: &str,
) -> Result<(), AppError> {
    scenario_service::unsync_scenario_skills(store, scenario_id)
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetApplyMode {
    Add,
    Remove,
}

impl From<PresetApplyMode> for BatchApplyMode {
    fn from(value: PresetApplyMode) -> Self {
        match value {
            PresetApplyMode::Add => BatchApplyMode::Add,
            PresetApplyMode::Remove => BatchApplyMode::Remove,
        }
    }
}

/// Apply (or remove) every skill in `preset_id` against every enabled coding
/// agent (`ToolCategory::Coding`). Mirrors the PresetBar behavior in the
/// global workspace view but covers all enabled coding agents at once.
///
/// Lobster agents are intentionally excluded — they have their own workspace
/// and their own preset bar.
#[tauri::command]
pub async fn apply_preset_to_coding_agents(
    app: tauri::AppHandle,
    preset_id: String,
    mode: PresetApplyMode,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        // 这个兼容命令虽然已不由新 PresetBar 调用，仍要与托盘和精确
        // Agent 批处理串行，避免旧前端或 CLI 触发并发写入。
        let _guard = scenario_service::lock_preset_apply();
        scenario_service::ensure_scenario_exists(&store, &preset_id)?;
        let skill_ids = store
            .get_skill_ids_for_scenario(&preset_id)
            .map_err(AppError::db)?;
        if skill_ids.is_empty() {
            return Ok(());
        }
        let tool_keys: Vec<String> = tool_adapters::enabled_installed_adapters(&store)
            .into_iter()
            .filter(|adapter| matches!(adapter.category, tool_adapters::ToolCategory::Coding))
            .map(|adapter| adapter.key)
            .collect();
        if tool_keys.is_empty() {
            return Ok(());
        }
        scenario_service::apply_skills_to_tools(&store, &skill_ids, &tool_keys, mode.into())
    })
    .await?;
    if result.is_ok() {
        refresh_tray_menu_best_effort(&app);
    }
    result
}

/// 将一个 Preset 精确应用到调用方指定的 Agent，不扩展到其他 Coding Agent。
/// 工具校验、去重和逐项容错由 scenario_service 的报告型批处理统一完成。
#[tauri::command]
pub async fn apply_preset_to_tools(
    app: tauri::AppHandle,
    preset_id: String,
    tool_keys: Vec<String>,
    mode: PresetApplyMode,
    store: State<'_, Arc<SkillStore>>,
) -> Result<scenario_service::BatchApplyReport, AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        // UI 的 FIFO 只覆盖单个 WebView；此锁还需与托盘共用，避免两个入口
        // 同时改写同一组目录和 `skill_targets`。
        let _guard = scenario_service::lock_preset_apply();
        apply_preset_to_tools_impl(&store, &preset_id, &tool_keys, mode)
    })
    .await?;
    if result.is_ok() {
        refresh_tray_menu_best_effort(&app);
    }
    result
}

fn apply_preset_to_tools_impl(
    store: &SkillStore,
    preset_id: &str,
    tool_keys: &[String],
    mode: PresetApplyMode,
) -> Result<scenario_service::BatchApplyReport, AppError> {
    scenario_service::ensure_scenario_exists(store, preset_id)?;
    let skill_ids = store
        .get_skill_ids_for_scenario(preset_id)
        .map_err(AppError::db)?;
    scenario_service::apply_skills_to_tools_with_report(
        store,
        &skill_ids,
        tool_keys,
        mode.into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::core::scenario_service::{
        collect_scenario_sync_targets, sync_desired_targets, unsync_obsolete_scenario_targets,
    };
    use crate::core::skill_store::SkillRecord;
    use crate::core::tool_adapters::{self, CustomToolDef};
    use std::fs;
    use std::path::PathBuf;
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
            content_hash: Some(format!("hash-{id}")),
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
    }

    fn configure_custom_tools(
        store: &SkillStore,
        tools: &[(&str, &std::path::Path)],
    ) {
        let custom_tools: Vec<CustomToolDef> = tools
            .iter()
            .map(|(key, target_base)| CustomToolDef {
                key: (*key).to_string(),
                display_name: format!("Test {key}"),
                skills_dir: target_base.to_string_lossy().to_string(),
                project_relative_skills_dir: None,
                category: Default::default(),
            })
            .collect();
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

    #[test]
    fn exact_preset_apply_deduplicates_tools_and_reports_skips() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let source_base = tmp.path().join("central");
        let first_target = tmp.path().join("agent-one");
        let second_target = tmp.path().join("agent-two");
        fs::create_dir_all(&source_base).unwrap();
        configure_custom_tools(
            &store,
            &[
                ("agent_one", first_target.as_path()),
                ("agent_two", second_target.as_path()),
            ],
        );
        store.set_setting("sync_mode", "copy").unwrap();

        store
            .insert_scenario(&sample_scenario("preset", "Preset"))
            .unwrap();
        store
            .insert_scenario(&sample_scenario("active", "Active"))
            .unwrap();
        store.set_active_scenario("active").unwrap();

        for name in ["alpha", "beta"] {
            let source = write_skill_dir(&source_base, name);
            store
                .insert_skill(&sample_skill(name, name, &source))
                .unwrap();
            store.add_skill_to_scenario("preset", name).unwrap();
        }

        let add = apply_preset_to_tools_impl(
            &store,
            "preset",
            &[
                "agent_one".to_string(),
                "agent_one".to_string(),
            ],
            PresetApplyMode::Add,
        )
        .unwrap();
        assert_eq!(add.applied, 2);
        assert_eq!(add.skipped, 0);
        assert!(add.failures.is_empty());
        assert!(first_target.join("alpha/SKILL.md").exists());
        assert!(first_target.join("beta/SKILL.md").exists());
        assert!(!second_target.exists());
        assert_eq!(
            store.get_active_scenario_id().unwrap().as_deref(),
            Some("active")
        );
        assert!(store
            .get_all_targets()
            .unwrap()
            .iter()
            .all(|target| target.tool == "agent_one"));

        let duplicate_add = apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string()],
            PresetApplyMode::Add,
        )
        .unwrap();
        assert_eq!(duplicate_add.applied, 0);
        assert_eq!(duplicate_add.skipped, 2);
        assert!(duplicate_add.failures.is_empty());

        let remove = apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string()],
            PresetApplyMode::Remove,
        )
        .unwrap();
        assert_eq!(remove.applied, 2);
        assert_eq!(remove.skipped, 0);
        assert!(remove.failures.is_empty());

        let duplicate_remove = apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string()],
            PresetApplyMode::Remove,
        )
        .unwrap();
        assert_eq!(duplicate_remove.applied, 0);
        assert_eq!(duplicate_remove.skipped, 2);
        assert!(duplicate_remove.failures.is_empty());
    }

    #[test]
    fn exact_preset_apply_rebuilds_missing_recorded_target() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let source_base = tmp.path().join("central");
        let target_base = tmp.path().join("agent-one");
        fs::create_dir_all(&source_base).unwrap();
        configure_custom_tools(&store, &[("agent_one", target_base.as_path())]);
        store.set_setting("sync_mode", "copy").unwrap();
        store
            .insert_scenario(&sample_scenario("preset", "Preset"))
            .unwrap();
        let source = write_skill_dir(&source_base, "alpha");
        store
            .insert_skill(&sample_skill("alpha", "alpha", &source))
            .unwrap();
        store.add_skill_to_scenario("preset", "alpha").unwrap();

        let first = apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string()],
            PresetApplyMode::Add,
        )
        .unwrap();
        assert_eq!(first.applied, 1);
        fs::remove_dir_all(target_base.join("alpha")).unwrap();

        let repaired = apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string()],
            PresetApplyMode::Add,
        )
        .unwrap();
        assert_eq!(repaired.applied, 1);
        assert_eq!(repaired.skipped, 0);
        assert!(repaired.failures.is_empty());
        assert!(target_base.join("alpha/SKILL.md").exists());
        assert_eq!(store.get_targets_for_skill("alpha").unwrap().len(), 1);
    }

    #[test]
    fn exact_preset_apply_moves_stale_record_to_current_agent_path() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let source_base = tmp.path().join("central");
        let old_target = tmp.path().join("agent-old");
        let new_target = tmp.path().join("agent-new");
        fs::create_dir_all(&source_base).unwrap();
        configure_custom_tools(&store, &[("agent_one", old_target.as_path())]);
        store.set_setting("sync_mode", "copy").unwrap();
        store
            .insert_scenario(&sample_scenario("preset", "Preset"))
            .unwrap();
        let source = write_skill_dir(&source_base, "alpha");
        store
            .insert_skill(&sample_skill("alpha", "alpha", &source))
            .unwrap();
        store.add_skill_to_scenario("preset", "alpha").unwrap();

        apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string()],
            PresetApplyMode::Add,
        )
        .unwrap();
        configure_custom_tools(&store, &[("agent_one", new_target.as_path())]);

        let moved = apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string()],
            PresetApplyMode::Add,
        )
        .unwrap();
        assert_eq!(moved.applied, 1);
        assert_eq!(moved.skipped, 0);
        assert!(moved.failures.is_empty());
        assert!(!old_target.join("alpha").exists());
        assert!(new_target.join("alpha/SKILL.md").exists());
        let rows = store.get_targets_for_skill("alpha").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(PathBuf::from(&rows[0].target_path), new_target.join("alpha"));
    }

    #[test]
    fn exact_preset_apply_refreshes_stale_copy_hash() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let source_base = tmp.path().join("central");
        let target_base = tmp.path().join("agent-one");
        fs::create_dir_all(&source_base).unwrap();
        configure_custom_tools(&store, &[("agent_one", target_base.as_path())]);
        store.set_setting("sync_mode", "copy").unwrap();
        store
            .insert_scenario(&sample_scenario("preset", "Preset"))
            .unwrap();
        let source = write_skill_dir(&source_base, "alpha");
        fs::write(source.join("version.txt"), "v1").unwrap();
        let mut skill = sample_skill("alpha", "alpha", &source);
        skill.content_hash = Some("h1".to_string());
        store.insert_skill(&skill).unwrap();
        store.add_skill_to_scenario("preset", "alpha").unwrap();

        apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string()],
            PresetApplyMode::Add,
        )
        .unwrap();
        fs::write(source.join("version.txt"), "v2").unwrap();
        skill.content_hash = Some("h2".to_string());
        store.upsert_skill(&skill).unwrap();

        let refreshed = apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string()],
            PresetApplyMode::Add,
        )
        .unwrap();
        assert_eq!(refreshed.applied, 1);
        assert_eq!(refreshed.skipped, 0);
        assert!(refreshed.failures.is_empty());
        assert_eq!(
            fs::read_to_string(target_base.join("alpha/version.txt")).unwrap(),
            "v2"
        );
        let rows = store.get_targets_for_skill("alpha").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source_hash.as_deref(), Some("h2"));
    }

    #[test]
    fn exact_preset_apply_keeps_previous_copy_when_source_build_fails() {
        let tmp = tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        let store = SkillStore::new(&db_path).unwrap();
        let source_base = tmp.path().join("central");
        let target_base = tmp.path().join("agent-one");
        fs::create_dir_all(&source_base).unwrap();
        configure_custom_tools(&store, &[("agent_one", target_base.as_path())]);
        store.set_setting("sync_mode", "copy").unwrap();
        store
            .insert_scenario(&sample_scenario("preset", "Preset"))
            .unwrap();
        let source = write_skill_dir(&source_base, "alpha");
        fs::write(source.join("version.txt"), "v1").unwrap();
        let mut skill = sample_skill("alpha", "alpha", &source);
        skill.content_hash = Some("h1".to_string());
        store.insert_skill(&skill).unwrap();
        store.add_skill_to_scenario("preset", "alpha").unwrap();
        apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string()],
            PresetApplyMode::Add,
        )
        .unwrap();

        fs::remove_dir_all(&source).unwrap();
        skill.content_hash = Some("h2".to_string());
        store.upsert_skill(&skill).unwrap();
        let failed = apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string()],
            PresetApplyMode::Add,
        )
        .unwrap();

        assert_eq!(failed.applied, 0);
        assert_eq!(failed.failures.len(), 1);
        assert_eq!(
            fs::read_to_string(target_base.join("alpha/version.txt")).unwrap(),
            "v1"
        );
        let rows = store.get_targets_for_skill("alpha").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source_hash.as_deref(), Some("h1"));
    }

    #[test]
    fn exact_preset_apply_restores_previous_copy_when_database_commit_fails() {
        let tmp = tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        let store = SkillStore::new(&db_path).unwrap();
        let source_base = tmp.path().join("central");
        let target_base = tmp.path().join("agent-one");
        fs::create_dir_all(&source_base).unwrap();
        configure_custom_tools(&store, &[("agent_one", target_base.as_path())]);
        store.set_setting("sync_mode", "copy").unwrap();
        store
            .insert_scenario(&sample_scenario("preset", "Preset"))
            .unwrap();
        let source = write_skill_dir(&source_base, "alpha");
        fs::write(source.join("version.txt"), "v1").unwrap();
        let mut skill = sample_skill("alpha", "alpha", &source);
        skill.content_hash = Some("h1".to_string());
        store.insert_skill(&skill).unwrap();
        store.add_skill_to_scenario("preset", "alpha").unwrap();
        apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string()],
            PresetApplyMode::Add,
        )
        .unwrap();

        fs::write(source.join("version.txt"), "v2").unwrap();
        skill.content_hash = Some("h2".to_string());
        store.upsert_skill(&skill).unwrap();
        let trigger = rusqlite::Connection::open(&db_path).unwrap();
        trigger
            .execute_batch(
                "CREATE TRIGGER fail_target_upsert BEFORE INSERT ON skill_targets
                 BEGIN SELECT RAISE(ABORT, 'forced target write failure'); END;",
            )
            .unwrap();

        let failed = apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string()],
            PresetApplyMode::Add,
        )
        .unwrap();
        assert_eq!(failed.applied, 0);
        assert_eq!(failed.failures.len(), 1);
        assert_eq!(
            fs::read_to_string(target_base.join("alpha/version.txt")).unwrap(),
            "v1"
        );
        let rows = store.get_targets_for_skill("alpha").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source_hash.as_deref(), Some("h1"));
        assert!(!tmp.path().join(".skills-manager-staging").exists());
    }

    #[test]
    fn exact_preset_remove_restores_target_when_database_commit_fails() {
        let tmp = tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        let store = SkillStore::new(&db_path).unwrap();
        let source_base = tmp.path().join("central");
        let target_base = tmp.path().join("agent-one");
        fs::create_dir_all(&source_base).unwrap();
        configure_custom_tools(&store, &[("agent_one", target_base.as_path())]);
        store.set_setting("sync_mode", "copy").unwrap();
        store
            .insert_scenario(&sample_scenario("preset", "Preset"))
            .unwrap();
        let source = write_skill_dir(&source_base, "alpha");
        store
            .insert_skill(&sample_skill("alpha", "alpha", &source))
            .unwrap();
        store.add_skill_to_scenario("preset", "alpha").unwrap();
        apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string()],
            PresetApplyMode::Add,
        )
        .unwrap();
        let trigger = rusqlite::Connection::open(&db_path).unwrap();
        trigger
            .execute_batch(
                "CREATE TRIGGER fail_target_delete BEFORE DELETE ON skill_targets
                 BEGIN SELECT RAISE(ABORT, 'forced target delete failure'); END;",
            )
            .unwrap();

        let failed = apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string()],
            PresetApplyMode::Remove,
        )
        .unwrap();
        assert_eq!(failed.applied, 0);
        assert_eq!(failed.failures.len(), 1);
        assert!(target_base.join("alpha/SKILL.md").exists());
        assert_eq!(store.get_targets_for_skill("alpha").unwrap().len(), 1);
        assert!(!tmp.path().join(".skills-manager-staging").exists());
    }

    #[test]
    fn exact_preset_apply_preserves_shared_physical_agent_directory() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let source_base = tmp.path().join("central");
        let shared_target = tmp.path().join("shared-agent-skills");
        fs::create_dir_all(&source_base).unwrap();
        fs::create_dir_all(&shared_target).unwrap();

        // 第二个 Agent 故意使用带 `.` 的等价路径，验证分组和删除判定使用
        // 规范化后的物理路径，而不是原始字符串。
        let shared_target_alias = shared_target.join(".");
        configure_custom_tools(
            &store,
            &[
                ("agent_one", shared_target.as_path()),
                ("agent_two", shared_target_alias.as_path()),
            ],
        );
        store.set_setting("sync_mode", "copy").unwrap();
        store
            .insert_scenario(&sample_scenario("preset", "Preset"))
            .unwrap();
        let source = write_skill_dir(&source_base, "alpha");
        store
            .insert_skill(&sample_skill("alpha", "alpha", &source))
            .unwrap();
        store.add_skill_to_scenario("preset", "alpha").unwrap();

        let add = apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string(), "agent_two".to_string()],
            PresetApplyMode::Add,
        )
        .unwrap();
        assert_eq!(add.applied, 2);
        assert!(add.failures.is_empty());
        assert!(shared_target.join("alpha/SKILL.md").exists());
        assert_eq!(store.get_all_targets().unwrap().len(), 2);

        let remove_first = apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string()],
            PresetApplyMode::Remove,
        )
        .unwrap();
        assert_eq!(remove_first.applied, 1);
        assert!(remove_first.failures.is_empty());
        assert!(shared_target.join("alpha/SKILL.md").exists());
        assert_eq!(store.get_all_targets().unwrap().len(), 1);

        let remove_second = apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_two".to_string()],
            PresetApplyMode::Remove,
        )
        .unwrap();
        assert_eq!(remove_second.applied, 1);
        assert!(remove_second.failures.is_empty());
        assert!(!shared_target.join("alpha").exists());
        assert!(store.get_all_targets().unwrap().is_empty());
    }

    #[test]
    fn exact_preset_apply_validates_all_tools_before_writing() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let source_base = tmp.path().join("central");
        let target_base = tmp.path().join("agent-one");
        fs::create_dir_all(&source_base).unwrap();
        configure_custom_tools(&store, &[("agent_one", target_base.as_path())]);
        store.set_setting("sync_mode", "copy").unwrap();
        store
            .insert_scenario(&sample_scenario("preset", "Preset"))
            .unwrap();
        let source = write_skill_dir(&source_base, "alpha");
        store
            .insert_skill(&sample_skill("alpha", "alpha", &source))
            .unwrap();
        store.add_skill_to_scenario("preset", "alpha").unwrap();

        let error = apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string(), "missing_agent".to_string()],
            PresetApplyMode::Add,
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            crate::core::error::ErrorKind::InvalidInput
        ));
        assert!(!target_base.exists());
        assert!(store.get_all_targets().unwrap().is_empty());
    }

    #[test]
    fn exact_preset_apply_continues_after_a_single_skill_failure() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let source_base = tmp.path().join("central");
        let target_base = tmp.path().join("agent-one");
        fs::create_dir_all(&source_base).unwrap();
        configure_custom_tools(&store, &[("agent_one", target_base.as_path())]);
        store.set_setting("sync_mode", "copy").unwrap();
        store
            .insert_scenario(&sample_scenario("preset", "Preset"))
            .unwrap();

        let valid_source = write_skill_dir(&source_base, "valid");
        store
            .insert_skill(&sample_skill("valid", "valid", &valid_source))
            .unwrap();
        let missing_source = source_base.join("missing");
        store
            .insert_skill(&sample_skill("missing", "missing", &missing_source))
            .unwrap();
        store.add_skill_to_scenario("preset", "valid").unwrap();
        store.add_skill_to_scenario("preset", "missing").unwrap();

        let report = apply_preset_to_tools_impl(
            &store,
            "preset",
            &["agent_one".to_string()],
            PresetApplyMode::Add,
        )
        .unwrap();
        assert_eq!(report.applied, 1);
        assert_eq!(report.skipped, 0);
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].skill_id, "missing");
        assert_eq!(report.failures[0].tool_key, "agent_one");
        assert!(target_base.join("valid/SKILL.md").exists());
    }
}
