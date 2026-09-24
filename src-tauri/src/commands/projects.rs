use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use serde::Serialize;
use tauri::State;

use crate::core::skill_store::{ProjectRecord, SkillRecord, SkillStore};
use crate::core::timing::should_log_first_or_slow;
use crate::core::{error::AppError, installer, project_scanner, sync_engine};

#[derive(Serialize, Default)]
pub struct SyncHealthDto {
    pub in_sync: usize,
    pub project_newer: usize,
    pub center_newer: usize,
    pub diverged: usize,
    pub project_only: usize,
}

#[derive(Serialize)]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    pub path: String,
    pub workspace_type: String,
    pub linked_agent_name: Option<String>,
    pub supports_skill_toggle: bool,
    pub sort_order: i32,
    pub skill_count: usize,
    pub sync_health: SyncHealthDto,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
pub struct ProjectSkillDocumentDto {
    pub skill_name: String,
    pub filename: String,
    pub content: String,
}

#[derive(Serialize, Clone)]
pub struct ProjectAgentTargetDto {
    pub key: String,
    pub display_name: String,
    pub enabled: bool,
    pub installed: bool,
    pub is_custom: bool,
}

fn agent_skill_configs(store: &SkillStore) -> Vec<project_scanner::AgentSkillConfig> {
    crate::core::project_skill_service::project_agent_configs(store)
}

fn linked_workspace_agent_key(rec: &ProjectRecord) -> String {
    crate::core::project_skill_service::linked_workspace_agent(rec).0
}

fn read_workspace_skills(
    rec: &ProjectRecord,
    configs: &[project_scanner::AgentSkillConfig],
) -> Vec<project_scanner::ProjectSkillInfo> {
    crate::core::project_skill_service::read_workspace_skills(rec, configs)
}

/// Resolve the enabled and disabled skills root directories for a given agent in a workspace.
fn resolve_agent_skills_roots(
    store: &SkillStore,
    rec: &ProjectRecord,
    agent: &str,
) -> Option<(PathBuf, Option<PathBuf>)> {
    crate::core::project_skill_service::resolve_agent_skills_roots(store, rec, agent)
}

fn project_agent_targets_for_record(
    store: &SkillStore,
    rec: &ProjectRecord,
) -> Vec<ProjectAgentTargetDto> {
    crate::core::project_skill_service::list_project_agent_targets(store, &rec.id)
        .unwrap_or_default()
        .into_iter()
        .map(|target| ProjectAgentTargetDto {
            key: target.key,
            display_name: target.display_name,
            enabled: target.enabled,
            installed: target.installed,
            is_custom: target.is_custom,
        })
        .collect()
}

/// Convert a project record into its DTO, folding the copies of one logical
/// skill across agents together by relative path.
fn project_to_dto(
    rec: &ProjectRecord,
    all_managed: &[SkillRecord],
    configs: &[project_scanner::AgentSkillConfig],
) -> ProjectDto {
    let skills = read_workspace_skills(rec, configs);
    let mut grouped_statuses: HashMap<String, String> = HashMap::new();

    for skill in &skills {
        let matched = find_best_center_match(skill, all_managed);
        let status = classify_sync_status(skill, matched);
        let key = skill.relative_path.to_lowercase();
        let existing = grouped_statuses
            .entry(key)
            .or_insert_with(|| status.clone());
        if sync_status_priority(&status) > sync_status_priority(existing) {
            *existing = status;
        }
    }

    let skill_count = grouped_statuses.len();
    let mut health = SyncHealthDto::default();
    for status in grouped_statuses.values() {
        match status.as_str() {
            "in_sync" => health.in_sync += 1,
            "project_newer" => health.project_newer += 1,
            "center_newer" => health.center_newer += 1,
            "diverged" => health.diverged += 1,
            _ => health.project_only += 1,
        }
    }

    ProjectDto {
        id: rec.id.clone(),
        name: rec.name.clone(),
        path: rec.path.clone(),
        workspace_type: rec.workspace_type.clone(),
        linked_agent_name: rec.linked_agent_name.clone(),
        supports_skill_toggle: rec.workspace_type != "linked" || rec.disabled_path.is_some(),
        sort_order: rec.sort_order,
        skill_count,
        sync_health: health,
        created_at: rec.created_at,
        updated_at: rec.updated_at,
    }
}

/// Severity of a sync status, used to reduce one logical skill's per-agent
/// copies to a single verdict: the worst one the group carries.
fn sync_status_priority(status: &str) -> u8 {
    match status {
        "diverged" => 5,
        "project_newer" => 4,
        "center_newer" => 3,
        "project_only" => 2,
        "in_sync" => 1,
        _ => 0,
    }
}

pub(crate) fn ensure_safe_skill_relative_path(skill_relative_path: &str) -> Result<(), AppError> {
    crate::core::project_skill_service::ensure_safe_skill_relative_path(skill_relative_path)
}

pub(crate) fn ensure_dir_within_root(path: &Path, root: &Path) -> Result<(), AppError> {
    crate::core::project_skill_service::ensure_dir_within_root(path, root)
}

// Walks upward from `start`, removing each empty directory until reaching
// (and including) `root`. Stops at the first non-empty directory or any
// other error. `fs::remove_dir` only succeeds on empty directories, so this
// will never delete a directory that still holds skills.
fn cleanup_empty_dirs_up_to(start: &Path, root: &Path) {
    let Ok(root_canonical) = std::fs::canonicalize(root) else {
        return;
    };
    let mut current = start.to_path_buf();
    loop {
        let Ok(current_canonical) = std::fs::canonicalize(&current) else {
            return;
        };
        if !current_canonical.starts_with(&root_canonical) {
            return;
        }
        if std::fs::remove_dir(&current).is_err() {
            return;
        }
        if current_canonical == root_canonical {
            return;
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => return,
        }
    }
}

fn remove_symlink_entry(path: &Path) -> Result<(), AppError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(AppError::io(err)),
    };
    if !metadata.file_type().is_symlink() {
        return Err(AppError::invalid_input(
            "Duplicate skill entry is not a symlink — resolve manually",
        ));
    }
    sync_engine::remove_target(path).map_err(AppError::io)
}

fn set_project_skill_enabled_state(
    skills_dir: &Path,
    disabled_dir: &Path,
    skill_relative_path: &str,
    enabled: bool,
) -> Result<(), AppError> {
    ensure_safe_skill_relative_path(skill_relative_path)?;

    let enabled_path = skills_dir.join(skill_relative_path);
    let disabled_path = disabled_dir.join(skill_relative_path);

    if enabled {
        if enabled_path.is_dir() {
            ensure_dir_within_root(&enabled_path, skills_dir)?;
            if disabled_path.exists() {
                ensure_dir_within_root(&disabled_path, disabled_dir)?;
                remove_symlink_entry(&disabled_path)?;
                if let Some(parent) = disabled_path.parent() {
                    cleanup_empty_dirs_up_to(parent, disabled_dir);
                }
            }
            return Ok(());
        }

        if !disabled_path.is_dir() {
            return Err(AppError::not_found(
                "Skill directory not found in skills-disabled",
            ));
        }
        ensure_dir_within_root(&disabled_path, disabled_dir)?;
        if let Some(parent) = enabled_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if enabled_path.exists() {
            return Err(AppError::invalid_input(
                "Skill already exists in skills directory",
            ));
        }
        std::fs::rename(&disabled_path, &enabled_path)?;
        if let Some(parent) = disabled_path.parent() {
            cleanup_empty_dirs_up_to(parent, disabled_dir);
        }
        return Ok(());
    }

    if disabled_path.is_dir() {
        ensure_dir_within_root(&disabled_path, disabled_dir)?;
        if enabled_path.exists() {
            ensure_dir_within_root(&enabled_path, skills_dir)?;
            remove_symlink_entry(&enabled_path)?;
        }
        return Ok(());
    }

    if !enabled_path.is_dir() {
        return Err(AppError::not_found("Skill directory not found"));
    }
    ensure_dir_within_root(&enabled_path, skills_dir)?;
    if let Some(parent) = disabled_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if disabled_path.exists() {
        return Err(AppError::invalid_input(
            "Skill already exists in skills-disabled directory",
        ));
    }
    std::fs::rename(&enabled_path, &disabled_path)?;
    Ok(())
}

fn ensure_distinct_linked_workspace_roots(
    skills_root: &Path,
    disabled_root: &Path,
) -> Result<(), AppError> {
    let skills_canonical = std::fs::canonicalize(skills_root)?;
    let disabled_canonical = std::fs::canonicalize(disabled_root)?;

    if skills_canonical == disabled_canonical
        || skills_canonical.starts_with(&disabled_canonical)
        || disabled_canonical.starts_with(&skills_canonical)
    {
        return Err(AppError::invalid_input(
            "Skills directory and disabled skills directory must not overlap",
        ));
    }

    Ok(())
}

pub(crate) fn slugify_skill_dir_name(name: &str) -> String {
    crate::core::project_skill_service::slugify_skill_dir_name(name)
}

pub(crate) fn source_ref_matches_skill_path(
    skill_path: &str,
    skill_canonical: Option<&PathBuf>,
    managed: &SkillRecord,
) -> bool {
    crate::core::project_skill_service::source_ref_matches_skill_path(
        skill_path,
        skill_canonical.map(|path| path.as_path()),
        managed,
    )
}

pub(crate) fn find_best_center_match<'a>(
    skill: &project_scanner::ProjectSkillInfo,
    all_managed: &'a [SkillRecord],
) -> Option<&'a SkillRecord> {
    crate::core::project_skill_service::find_best_center_match(skill, all_managed)
}


pub(crate) fn classify_sync_status(
    skill: &project_scanner::ProjectSkillInfo,
    managed: Option<&SkillRecord>,
) -> String {
    let Some(managed) = managed else {
        return "project_only".to_string();
    };

    // Fast path: compare project hash against DB-stored center hash
    if skill.content_hash.is_some()
        && managed.content_hash.as_deref() == skill.content_hash.as_deref()
    {
        return "in_sync".to_string();
    }

    // The DB hash may be stale, and `updated_at` is the wrong clock for the
    // comparison further down, so read the center from disk once and answer
    // both questions from the same walk.
    let center_entries =
        crate::core::content_hash::list_content_files(Path::new(&managed.central_path));

    if let Some(project_hash) = skill.content_hash.as_deref() {
        if project_hash == crate::core::content_hash::hash_entries(&center_entries) {
            return "in_sync".to_string();
        }
    }

    let Some(project_modified_at) = skill.last_modified_at else {
        return "diverged".to_string();
    };

    // The project side is a filesystem mtime, so the center has to be one too.
    // `updated_at` is a database column stamped when the row was written:
    // editing files in the library does not move it, and a metadata-only write
    // moves it while no content changed. Comparing the two rulers reported
    // "center is newer" for a project copy the user had just edited, and that
    // status invites a pull, which overwrites the edit — the diagnosis behind
    // #328.
    let Some(center_modified_at) = crate::core::content_hash::latest_modified_ms(&center_entries)
    else {
        return "diverged".to_string();
    };
    let threshold_ms = 1_000;
    if project_modified_at > center_modified_at + threshold_ms {
        "project_newer".to_string()
    } else if center_modified_at > project_modified_at + threshold_ms {
        "center_newer".to_string()
    } else {
        "diverged".to_string()
    }
}

static GET_PROJECTS_FIRST_CALL: AtomicBool = AtomicBool::new(true);

#[tauri::command]
pub async fn get_projects(store: State<'_, Arc<SkillStore>>) -> Result<Vec<ProjectDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let start = Instant::now();
        let records = store.get_all_projects().map_err(AppError::db)?;
        let all_managed = store.get_all_skills().map_err(AppError::db)?;
        let configs = agent_skill_configs(&store);
        let count = records.len();
        let dtos: Vec<ProjectDto> = records
            .iter()
            .map(|r| project_to_dto(r, &all_managed, &configs))
            .collect();
        let elapsed_ms = start.elapsed().as_millis();
        if should_log_first_or_slow(&GET_PROJECTS_FIRST_CALL, elapsed_ms, 100) {
            log::info!("get_projects: {count} projects in {elapsed_ms} ms");
        }
        Ok(dtos)
    })
    .await?
}

#[tauri::command]
pub async fn add_project(
    store: State<'_, Arc<SkillStore>>,
    path: String,
) -> Result<ProjectDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let record = crate::core::project_service::add_project(&store, Path::new(&path))?;
        let all_managed = store.get_all_skills().map_err(AppError::db)?;
        let configs = agent_skill_configs(&store);
        Ok(project_to_dto(&record, &all_managed, &configs))
    })
    .await?
}

#[tauri::command]
pub async fn add_linked_workspace(
    store: State<'_, Arc<SkillStore>>,
    name: String,
    path: String,
    disabled_path: Option<String>,
) -> Result<ProjectDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(AppError::invalid_input("Workspace name is required"));
        }

        let skills_root = PathBuf::from(path.trim());
        if !skills_root.is_dir() {
            return Err(AppError::invalid_input("Skills directory does not exist"));
        }

        let disabled_path = disabled_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let disabled_path = if let Some(disabled) = disabled_path {
            let disabled_root = PathBuf::from(&disabled);
            if !disabled_root.is_dir() {
                return Err(AppError::invalid_input(
                    "Disabled skills directory does not exist",
                ));
            }
            ensure_distinct_linked_workspace_roots(&skills_root, &disabled_root)?;
            Some(disabled)
        } else {
            let mut disabled_root = skills_root.clone();
            let derived = disabled_root
                .file_name()
                .and_then(|n| n.to_str())
                .map(|name| format!("{}-disabled", name));
            match derived {
                Some(name) => {
                    disabled_root.set_file_name(name);
                    match std::fs::create_dir_all(&disabled_root) {
                        Ok(()) => {
                            ensure_distinct_linked_workspace_roots(&skills_root, &disabled_root)?;
                            Some(disabled_root.to_string_lossy().to_string())
                        }
                        Err(_) => None,
                    }
                }
                None => None,
            }
        };

        let now = chrono::Utc::now().timestamp_millis();
        let record = ProjectRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.clone(),
            path: skills_root.to_string_lossy().to_string(),
            workspace_type: "linked".to_string(),
            linked_agent_key: Some(slugify_skill_dir_name(&name)),
            linked_agent_name: Some(name),
            disabled_path,
            sort_order: 0,
            created_at: now,
            updated_at: now,
        };

        store.insert_project(&record).map_err(AppError::db)?;
        let all_managed = store.get_all_skills().map_err(AppError::db)?;
        let configs = agent_skill_configs(&store);
        Ok(project_to_dto(&record, &all_managed, &configs))
    })
    .await?
}

#[tauri::command]
pub async fn remove_project(store: State<'_, Arc<SkillStore>>, id: String) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.delete_project(&id).map_err(AppError::db))
        .await?
}

#[tauri::command]
pub async fn reorder_projects(
    ids: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.reorder_projects(&ids).map_err(AppError::db))
        .await?
}

#[tauri::command]
pub async fn scan_projects(
    root: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<String>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root_path = Path::new(&root);
        if !root_path.is_dir() {
            return Err(AppError::invalid_input("Directory does not exist"));
        }
        let configs = agent_skill_configs(&store);
        Ok(project_scanner::scan_projects_in_dir(
            root_path, 4, &configs,
        ))
    })
    .await?
}

#[tauri::command]
pub async fn get_project_agent_targets(
    store: State<'_, Arc<SkillStore>>,
    project_id: String,
) -> Result<Vec<ProjectAgentTargetDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let record = store
            .get_project_by_id(&project_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Workspace not found"))?;
        Ok(project_agent_targets_for_record(&store, &record))
    })
    .await?
}

#[tauri::command]
pub async fn get_project_skills(
    store: State<'_, Arc<SkillStore>>,
    project_id: String,
) -> Result<Vec<project_scanner::ProjectSkillInfo>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let record = store
            .get_project_by_id(&project_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Workspace not found"))?;

        let configs = agent_skill_configs(&store);
        let mut skills = read_workspace_skills(&record, &configs);

        let all_managed = store.get_all_skills().unwrap_or_default();
        let tags_map = store.get_tags_map().unwrap_or_default();
        for skill in &mut skills {
            let matched = find_best_center_match(skill, &all_managed);
            skill.in_center = matched.is_some();
            skill.center_skill_id = matched.map(|m| m.id.clone());
            skill.tags = skill
                .center_skill_id
                .as_ref()
                .and_then(|skill_id| tags_map.get(skill_id).cloned())
                .unwrap_or_default();
            skill.sync_status = classify_sync_status(skill, matched);
        }

        Ok(skills)
    })
    .await?
}

#[tauri::command]
pub async fn get_project_skill_document(
    project_id: String,
    skill_relative_path: String,
    agent: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ProjectSkillDocumentDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_safe_skill_relative_path(&skill_relative_path)?;

        let record = store
            .get_project_by_id(&project_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Workspace not found"))?;

        let (skills_root, disabled_root) = resolve_agent_skills_roots(&store, &record, &agent)
            .ok_or_else(|| AppError::not_found(format!("Unknown workspace agent: {}", agent)))?;
        let disabled_root_copy = disabled_root.clone();
        let skill_dir = skills_root.join(&skill_relative_path);
        let skill_dir = if skill_dir.is_dir() {
            ensure_dir_within_root(&skill_dir, &skills_root)?;
            skill_dir
        } else if let Some(disabled_root) = disabled_root {
            let disabled = disabled_root.join(&skill_relative_path);
            if disabled.is_dir() {
                ensure_dir_within_root(&disabled, &disabled_root)?;
                disabled
            } else {
                return Err(AppError::not_found("Skill directory not found"));
            }
        } else {
            return Err(AppError::not_found("Skill directory not found"));
        };

        // Collect all allowed roots for symlink target validation
        let mut allowed_roots: Vec<PathBuf> = vec![skills_root.clone()];
        if let Some(dr) = disabled_root_copy {
            allowed_roots.push(dr);
        }
        // For project workspaces, also allow the project root itself
        if record.workspace_type != "linked" {
            allowed_roots.push(PathBuf::from(&record.path));
        }

        let candidates = ["SKILL.md", "skill.md", "CLAUDE.md", "README.md"];
        for candidate in &candidates {
            let file_path = skill_dir.join(candidate);
            if !file_path.exists() {
                continue;
            }
            // For symlinks, verify the resolved target stays within an allowed root
            if let Ok(meta) = std::fs::symlink_metadata(&file_path) {
                if meta.file_type().is_symlink() {
                    let resolved = match std::fs::canonicalize(&file_path) {
                        Ok(r) => r,
                        Err(_) => continue, // broken symlink
                    };
                    let in_allowed_root = allowed_roots.iter().any(|root| {
                        std::fs::canonicalize(root)
                            .map(|canon| resolved.starts_with(&canon))
                            .unwrap_or(false)
                    });
                    if !in_allowed_root {
                        continue;
                    }
                }
            }
            if file_path.is_file() {
                let content = std::fs::read_to_string(&file_path)?;
                return Ok(ProjectSkillDocumentDto {
                    skill_name: skill_relative_path,
                    filename: candidate.to_string(),
                    content,
                });
            }
        }

        Err(AppError::not_found(
            "No document file found in skill directory",
        ))
    })
    .await?
}

#[tauri::command]
pub async fn import_project_skill_to_center(
    store: State<'_, Arc<SkillStore>>,
    project_id: String,
    skill_relative_path: String,
    agent: String,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_safe_skill_relative_path(&skill_relative_path)?;

        let record = store
            .get_project_by_id(&project_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Workspace not found"))?;

        let configs = agent_skill_configs(&store);
        let skills = read_workspace_skills(&record, &configs);
        let skill = skills
            .iter()
            .find(|s| s.relative_path == skill_relative_path && s.agent == agent)
            .ok_or_else(|| AppError::not_found("Skill not found in workspace"))?;

        let source_path = PathBuf::from(&skill.path);
        let all_managed = store.get_all_skills().unwrap_or_default();
        // Use the same matching logic as the UI (find_best_center_match) to
        // stay consistent with sync-status display. After updating, bind
        // source_ref so future imports match by exact path.
        if let Some(existing) = find_best_center_match(skill, &all_managed) {
            let result = installer::install_from_local_to_destination(
                &source_path,
                Some(&existing.name),
                Path::new(&existing.central_path),
            )
            .map_err(AppError::io)?;
            store
                .update_skill_after_install(
                    &existing.id,
                    &existing.name,
                    result.description.as_deref(),
                    existing.source_revision.as_deref(),
                    existing.remote_revision.as_deref(),
                    Some(&result.content_hash),
                    "local_only",
                )
                .map_err(AppError::db)?;
            // Only update source_ref when the match was already by source_ref
            // path (not by hash or name). This avoids permanently rebinding
            // unrelated center skills that merely share a name or content.
            let already_matched_by_ref = source_ref_matches_skill_path(
                &skill.path,
                std::fs::canonicalize(&skill.path).ok().as_ref(),
                existing,
            );
            if existing.source_type == "local" && already_matched_by_ref {
                store
                    .update_skill_source_ref(&existing.id, &skill.path)
                    .map_err(AppError::db)?;
            }
            return Ok(());
        }

        let result =
            installer::install_from_local(&source_path, Some(&skill.name)).map_err(AppError::io)?;

        let now = chrono::Utc::now().timestamp_millis();
        let id = uuid::Uuid::new_v4().to_string();

        let skill_record = SkillRecord {
            id: id.clone(),
            name: result.name.clone(),
            description: result.description.clone(),
            source_type: "local".to_string(),
            source_ref: Some(skill.path.clone()),
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: result.central_path.to_string_lossy().to_string(),
            content_hash: Some(result.content_hash.clone()),
            enabled: true,
            created_at: now,
            updated_at: now,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: Some(now),
            last_check_error: None,
        };

        store.insert_skill(&skill_record).map_err(AppError::db)?;

        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn update_project_skill_to_center(
    store: State<'_, Arc<SkillStore>>,
    project_id: String,
    skill_relative_path: String,
    agent: String,
) -> Result<(), AppError> {
    import_project_skill_to_center(store, project_id, skill_relative_path, agent).await
}

#[tauri::command]
pub fn slugify_skill_names(names: Vec<String>) -> Vec<String> {
    names.iter().map(|n| slugify_skill_dir_name(n)).collect()
}

#[tauri::command]
pub async fn export_skill_to_project(
    store: State<'_, Arc<SkillStore>>,
    skill_id: String,
    project_id: String,
    agents: Option<Vec<String>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let project = store
            .get_project_by_id(&project_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Workspace not found"))?;

        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;

        let source = PathBuf::from(&skill.central_path);
        let dir_name = sync_engine::target_dir_name(&source, &skill.name);
        ensure_safe_skill_relative_path(&dir_name)?;
        let requested_agent_keys = agents.filter(|items| !items.is_empty()).unwrap_or_else(|| {
            if project.workspace_type == "linked" {
                vec![linked_workspace_agent_key(&project)]
            } else {
                vec!["claude_code".to_string()]
            }
        });
        let agent_keys = if project.workspace_type == "linked" {
            requested_agent_keys
        } else {
            let available_targets: std::collections::HashSet<String> =
                project_agent_targets_for_record(&store, &project)
                    .into_iter()
                    .filter(|target| target.installed && target.enabled)
                    .map(|target| target.key)
                    .collect();
            let filtered = requested_agent_keys
                .into_iter()
                .filter(|key| available_targets.contains(key))
                .collect::<Vec<_>>();
            if filtered.is_empty() {
                return Err(AppError::invalid_input(
                    "No enabled installed agents selected for this project",
                ));
            }
            filtered
        };

        for agent_key in &agent_keys {
            let (skills_root, disabled_root) =
                resolve_agent_skills_roots(&store, &project, agent_key)
                    .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent_key)))?;
            let target_dir = skills_root.join(&dir_name);

            if target_dir.strip_prefix(&skills_root).is_err() {
                return Err(AppError::invalid_input("Invalid skill directory path"));
            }

            if target_dir.exists()
                || disabled_root
                    .as_ref()
                    .map(|path| path.join(&dir_name).exists())
                    .unwrap_or(false)
            {
                return Err(AppError::invalid_input(format!(
                    "Skill \"{}\" already exists in this workspace for agent {}",
                    skill.name, agent_key
                )));
            }
        }

        // Two agents can resolve to the same project skills root, in which case
        // the second pass would find the directory the first just wrote and
        // refuse it. The artifact is already correct, so skip instead.
        let mut written: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for agent_key in &agent_keys {
            let (skills_root, _) = resolve_agent_skills_roots(&store, &project, agent_key)
                .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent_key)))?;
            let target_dir = skills_root.join(&dir_name);
            if !written.insert(target_dir.clone()) {
                continue;
            }
            match crate::core::project_skill_service::add_skill_to_project(
                &store,
                &project_id,
                &skill_id,
                agent_key,
            )? {
                crate::core::project_skill_service::AddProjectSkillOutcome::Added(_) => {}
                crate::core::project_skill_service::AddProjectSkillOutcome::AlreadyPresent(_) => {
                    return Err(AppError::invalid_input(format!(
                        "Skill \"{}\" already exists in this workspace for agent {}",
                        skill.name, agent_key
                    )));
                }
            }
        }

        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn update_project_skill_from_center(
    store: State<'_, Arc<SkillStore>>,
    project_id: String,
    skill_relative_path: String,
    agent: String,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_safe_skill_relative_path(&skill_relative_path)?;

        let record = store
            .get_project_by_id(&project_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Workspace not found"))?;

        let configs = agent_skill_configs(&store);
        let skills = read_workspace_skills(&record, &configs);
        let skill = skills
            .iter()
            .find(|s| s.relative_path == skill_relative_path && s.agent == agent)
            .ok_or_else(|| AppError::not_found("Skill not found in workspace"))?;

        let all_managed = store.get_all_skills().unwrap_or_default();
        let managed = find_best_center_match(skill, &all_managed)
            .ok_or_else(|| AppError::not_found("No matching skill in center"))?;

        // Mirror the global-workspace protection (agent_workspace.rs): never
        // overwrite a project copy that has unsynced local edits (#225 review).
        if classify_sync_status(skill, Some(managed)) == "project_newer" {
            return Err(AppError::invalid_input(
                "Project skill is newer than the Skills Center version",
            ));
        }

        let (skills_root, disabled_root) = resolve_agent_skills_roots(&store, &record, &agent)
            .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent)))?;
        let target_path = PathBuf::from(&skill.path);
        if target_path.starts_with(&skills_root) {
            ensure_dir_within_root(&target_path, &skills_root)?;
        } else if disabled_root
            .as_ref()
            .map(|root| target_path.starts_with(root))
            .unwrap_or(false)
        {
            let disabled_root = disabled_root.expect("checked above");
            ensure_dir_within_root(&target_path, &disabled_root)?;
        } else {
            return Err(AppError::invalid_input("Invalid skill directory path"));
        }

        let source = PathBuf::from(&managed.central_path);
        let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
        let mode = sync_engine::sync_mode_for_tool(&agent, configured_mode.as_deref());
        // UserConfirmed: this intentionally replaces an existing project copy
        // the user chose to update, and project deployments never create
        // `skill_targets` rows, so no record could vouch for it. The
        // project_newer check above is the guard that makes this safe.
        sync_engine::sync_skill(
            &source,
            &target_path,
            mode,
            sync_engine::ReplacePolicy::UserConfirmed,
        )
        .map_err(AppError::io)?;
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn toggle_project_skill(
    store: State<'_, Arc<SkillStore>>,
    project_id: String,
    skill_relative_path: String,
    agent: String,
    enabled: bool,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_safe_skill_relative_path(&skill_relative_path)?;

        let record = store
            .get_project_by_id(&project_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Workspace not found"))?;

        let (skills_dir, disabled_dir) = resolve_agent_skills_roots(&store, &record, &agent)
            .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent)))?;
        let disabled_dir = disabled_dir.ok_or_else(|| {
            AppError::invalid_input("This workspace does not support disabling skills")
        })?;

        set_project_skill_enabled_state(&skills_dir, &disabled_dir, &skill_relative_path, enabled)
    })
    .await?
}

#[tauri::command]
pub async fn delete_project_skill(
    store: State<'_, Arc<SkillStore>>,
    project_id: String,
    skill_relative_path: String,
    agent: String,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::core::project_skill_service::remove_skill_from_project(
            &store,
            &project_id,
            &skill_relative_path,
            &agent,
        )
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::{
        classify_sync_status, ensure_distinct_linked_workspace_roots, find_best_center_match,
        project_to_dto, set_project_skill_enabled_state,
    };
    use crate::core::project_skill_service::remove_workspace_skill_target;
    use crate::core::content_hash;
    use crate::core::error::ErrorKind;
    use crate::core::project_scanner::{AgentSkillConfig, ProjectSkillInfo};
    use crate::core::skill_store::{ProjectRecord, SkillRecord};
    use std::fs;
    use tempfile::tempdir;

    fn sample_managed_skill(
        central_path: String,
        content_hash: Option<String>,
        updated_at: i64,
    ) -> SkillRecord {
        SkillRecord {
            id: "skill-1".to_string(),
            name: "Example Skill".to_string(),
            description: None,
            source_type: "local".to_string(),
            source_ref: None,
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path,
            content_hash,
            enabled: true,
            created_at: 0,
            updated_at,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: None,
            last_check_error: None,
        }
    }

    /// Build a library skill with the given identity fields, so a test can
    /// assert the order the layers resolve in.
    fn managed_skill_with_identity(
        id: &str,
        name: &str,
        central_path: String,
        content_hash: Option<String>,
    ) -> SkillRecord {
        SkillRecord {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            source_type: "skillssh".to_string(),
            source_ref: None,
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path,
            content_hash,
            enabled: true,
            created_at: 0,
            updated_at: 0,
            status: "ok".to_string(),
            update_status: "unknown".to_string(),
            last_checked_at: None,
            last_check_error: None,
        }
    }

    fn sample_project_skill(
        path: String,
        content_hash: Option<String>,
        last_modified_at: Option<i64>,
    ) -> ProjectSkillInfo {
        ProjectSkillInfo {
            name: "Example Skill".to_string(),
            dir_name: "example-skill".to_string(),
            relative_path: "example-skill".to_string(),
            description: None,
            path,
            files: vec!["SKILL.md".to_string()],
            enabled: true,
            agent: "claude_code".to_string(),
            agent_display_name: "Claude Code".to_string(),
            tags: Vec::new(),
            in_center: true,
            sync_status: "project_only".to_string(),
            center_skill_id: Some("skill-1".to_string()),
            last_modified_at,
            content_hash,
        }
    }

    /// Build a project skill with the given directory name and agent, to
    /// stand in for one per-agent copy.
    fn project_skill_with_dir(
        dir_name: &str,
        path: String,
        content_hash: Option<String>,
        agent: &str,
    ) -> ProjectSkillInfo {
        ProjectSkillInfo {
            name: dir_name.to_string(),
            dir_name: dir_name.to_string(),
            relative_path: dir_name.to_string(),
            description: None,
            path,
            files: vec!["SKILL.md".to_string()],
            enabled: true,
            agent: agent.to_string(),
            agent_display_name: agent.to_string(),
            tags: Vec::new(),
            in_center: false,
            sync_status: "project_only".to_string(),
            center_skill_id: None,
            last_modified_at: Some(1_000),
            content_hash,
        }
    }

    /// Directory identity must win over a content hash several skills share.
    #[test]
    fn find_best_center_match_prefers_directory_identity_over_shared_hash() {
        let shared_hash = Some("same-content-hash".to_string());
        let project = project_skill_with_dir(
            "adapt",
            "/tmp/project/.claude/skills/adapt".to_string(),
            shared_hash.clone(),
            "claude_code",
        );
        let all_managed = vec![
            managed_skill_with_identity(
                "adapt-id",
                "adapt",
                "/tmp/center/adapt".to_string(),
                shared_hash.clone(),
            ),
            managed_skill_with_identity(
                "polish-id",
                "polish",
                "/tmp/center/polish".to_string(),
                shared_hash,
            ),
        ];

        let matched = find_best_center_match(&project, &all_managed).unwrap();

        assert_eq!(matched.id, "adapt-id");
    }

    /// The central directory name must win over a frontmatter name that repeats.
    #[test]
    fn find_best_center_match_uses_central_directory_before_frontmatter_name() {
        let project = project_skill_with_dir(
            "adapt",
            "/tmp/project/.claude/skills/adapt".to_string(),
            None,
            "claude_code",
        );
        let all_managed = vec![
            managed_skill_with_identity(
                "adapt-id",
                "impeccable",
                "/tmp/center/adapt".to_string(),
                None,
            ),
            managed_skill_with_identity(
                "layout-id",
                "impeccable",
                "/tmp/center/layout".to_string(),
                None,
            ),
        ];

        let matched = find_best_center_match(&project, &all_managed).unwrap();

        assert_eq!(matched.id, "adapt-id");
    }

    /// A unique content hash outranks a directory name owned by a different
    /// skill. A library holding both "Code Review" and "code-review" exports
    /// the first under the slug `code-review`, which is the second one's
    /// library directory — matching on the directory binds the copy to the
    /// wrong row, and that row is also where an import writes back.
    #[test]
    fn find_best_center_match_prefers_a_unique_hash_over_another_skills_directory() {
        let exported_hash = Some("code-review-content".to_string());
        let project = project_skill_with_dir(
            "code-review",
            "/tmp/project/.claude/skills/code-review".to_string(),
            exported_hash.clone(),
            "claude_code",
        );
        let all_managed = vec![
            managed_skill_with_identity(
                "spaced-id",
                "Code Review",
                "/tmp/center/Code Review".to_string(),
                exported_hash,
            ),
            managed_skill_with_identity(
                "slug-id",
                "code-review",
                "/tmp/center/code-review".to_string(),
                Some("unrelated-content".to_string()),
            ),
        ];

        let matched = find_best_center_match(&project, &all_managed).unwrap();

        assert_eq!(matched.id, "spaced-id");
    }

    /// The sidebar project count dedupes by logical skill rather than adding
    /// up per-agent copies.
    #[test]
    fn project_to_dto_counts_logical_skills_not_agent_copies() {
        let tmp = tempdir().unwrap();
        let project_path = tmp.path().join("project");
        let claude_skill = project_path.join(".claude/skills/shared-skill");
        let codex_skill = project_path.join(".codex/skills/shared-skill");
        fs::create_dir_all(&claude_skill).unwrap();
        fs::create_dir_all(&codex_skill).unwrap();
        fs::write(claude_skill.join("SKILL.md"), "# Shared\n").unwrap();
        fs::write(codex_skill.join("SKILL.md"), "# Shared\n").unwrap();

        let record = ProjectRecord {
            id: "project-1".to_string(),
            name: "Project".to_string(),
            path: project_path.to_string_lossy().to_string(),
            workspace_type: "project".to_string(),
            linked_agent_key: None,
            linked_agent_name: None,
            disabled_path: None,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        };
        let configs = vec![
            AgentSkillConfig {
                key: "claude_code".to_string(),
                display_name: "Claude Code".to_string(),
                relative_skills_dir: ".claude/skills".to_string(),
            },
            AgentSkillConfig {
                key: "codex".to_string(),
                display_name: "Codex".to_string(),
                relative_skills_dir: ".codex/skills".to_string(),
            },
        ];

        let dto = project_to_dto(&record, &[], &configs);

        assert_eq!(dto.skill_count, 1);
        assert_eq!(dto.sync_health.project_only, 1);
    }

    #[test]
    fn classify_sync_status_uses_live_center_hash_when_db_hash_is_stale() {
        let center_dir = tempdir().unwrap();
        fs::write(center_dir.path().join("SKILL.md"), "# Example\n").unwrap();
        let live_hash = content_hash::hash_directory(center_dir.path()).unwrap();

        let managed = sample_managed_skill(
            center_dir.path().to_string_lossy().to_string(),
            Some("stale-db-hash".to_string()),
            1_000,
        );
        let project = sample_project_skill(
            center_dir.path().to_string_lossy().to_string(),
            Some(live_hash),
            Some(5_000),
        );

        assert_eq!(classify_sync_status(&project, Some(&managed)), "in_sync");
    }

    /// Newest content mtime of a directory, the same figure the project side
    /// is built from, so both sides of the comparison use one ruler.
    fn center_mtime_ms(dir: &std::path::Path) -> i64 {
        content_hash::latest_modified_ms(&content_hash::list_content_files(dir)).unwrap()
    }

    /// `updated_at` is a database column, not a filesystem mtime. With the
    /// project copy genuinely newer on disk, a much later `updated_at` must not
    /// flip the answer to "center_newer" — that reading invited a pull and
    /// overwrote the edit the user had just made (#328).
    #[test]
    fn classify_sync_status_ignores_the_db_column_when_the_project_is_newer_on_disk() {
        let center_dir = tempdir().unwrap();
        fs::write(center_dir.path().join("SKILL.md"), "# Center\n").unwrap();
        let center_mtime = center_mtime_ms(center_dir.path());

        let project_dir = tempdir().unwrap();
        fs::write(project_dir.path().join("SKILL.md"), "# Project changed\n").unwrap();
        let project_hash = content_hash::hash_directory(project_dir.path()).unwrap();

        let managed = sample_managed_skill(
            center_dir.path().to_string_lossy().to_string(),
            Some("stale-db-hash".to_string()),
            center_mtime + 60_000,
        );
        let project = sample_project_skill(
            project_dir.path().to_string_lossy().to_string(),
            Some(project_hash),
            Some(center_mtime + 5_000),
        );

        assert_eq!(
            classify_sync_status(&project, Some(&managed)),
            "project_newer"
        );
    }

    /// The other direction, and the reason the fix is not simply "always say
    /// project_newer": a center that really is ahead still reports so, with an
    /// `updated_at` old enough that only the real mtime can produce it.
    #[test]
    fn classify_sync_status_reports_a_center_that_is_newer_on_disk() {
        let center_dir = tempdir().unwrap();
        fs::write(center_dir.path().join("SKILL.md"), "# Center\n").unwrap();
        let center_mtime = center_mtime_ms(center_dir.path());

        let project_dir = tempdir().unwrap();
        fs::write(project_dir.path().join("SKILL.md"), "# Project older\n").unwrap();
        let project_hash = content_hash::hash_directory(project_dir.path()).unwrap();

        let managed = sample_managed_skill(
            center_dir.path().to_string_lossy().to_string(),
            Some("stale-db-hash".to_string()),
            0,
        );
        let project = sample_project_skill(
            project_dir.path().to_string_lossy().to_string(),
            Some(project_hash),
            Some(center_mtime - 5_000),
        );

        assert_eq!(
            classify_sync_status(&project, Some(&managed)),
            "center_newer"
        );
    }

    #[test]
    fn linked_workspace_roots_reject_same_directory() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("skills");
        fs::create_dir_all(&root).unwrap();

        let err = ensure_distinct_linked_workspace_roots(&root, &root).unwrap_err();
        assert!(
            err.to_string().contains("must not overlap"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn linked_workspace_roots_reject_nested_directory() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("skills");
        let nested = root.join("disabled");
        fs::create_dir_all(&nested).unwrap();

        let err = ensure_distinct_linked_workspace_roots(&root, &nested).unwrap_err();
        assert!(
            err.to_string().contains("must not overlap"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn linked_workspace_roots_allow_distinct_directories() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("skills");
        let disabled = tmp.path().join("skills-disabled");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&disabled).unwrap();

        ensure_distinct_linked_workspace_roots(&root, &disabled).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn remove_workspace_skill_target_removes_symlink_without_touching_target() {
        let tmp = tempdir().unwrap();
        let real = tmp.path().join("real-skill");
        let link = tmp.path().join("linked-skill");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("SKILL.md"), "# hello").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        remove_workspace_skill_target(&link).unwrap();

        assert!(!link.exists());
        assert!(real.exists());
        assert!(real.join("SKILL.md").exists());
    }

    #[cfg(windows)]
    #[test]
    fn remove_workspace_skill_target_removes_directory_symlink_without_touching_target() {
        let tmp = tempdir().unwrap();
        let real = tmp.path().join("real-skill");
        let link = tmp.path().join("linked-skill");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("SKILL.md"), "# hello").unwrap();
        std::os::windows::fs::symlink_dir(&real, &link).unwrap();

        remove_workspace_skill_target(&link).unwrap();

        assert!(!link.exists());
        assert!(real.exists());
        assert!(real.join("SKILL.md").exists());
    }

    #[cfg(unix)]
    #[test]
    fn set_project_skill_enabled_state_disabling_cleans_duplicate_symlink_without_touching_target()
    {
        use std::os::unix::fs::symlink;

        let tmp = tempdir().unwrap();
        let central_skill = tmp.path().join("central").join("understand-diff");
        let skills_root = tmp.path().join("skills");
        let disabled_root = tmp.path().join("skills-disabled");
        let relative_path = "understand-diff";

        fs::create_dir_all(&central_skill).unwrap();
        fs::write(
            central_skill.join("SKILL.md"),
            "---\nname: understand-diff\n---\n",
        )
        .unwrap();
        fs::create_dir_all(&skills_root).unwrap();
        fs::create_dir_all(&disabled_root).unwrap();

        symlink(&central_skill, skills_root.join(relative_path)).unwrap();
        symlink(&central_skill, disabled_root.join(relative_path)).unwrap();

        set_project_skill_enabled_state(&skills_root, &disabled_root, relative_path, false)
            .unwrap();

        assert!(!skills_root.join(relative_path).exists());
        assert!(disabled_root.join(relative_path).exists());
        assert!(central_skill.exists());
        assert!(central_skill.join("SKILL.md").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn set_project_skill_enabled_state_enabling_cleans_duplicate_symlink_without_touching_target() {
        use std::os::unix::fs::symlink;

        let tmp = tempdir().unwrap();
        let central_skill = tmp.path().join("central").join("understand-diff");
        let skills_root = tmp.path().join("skills");
        let disabled_root = tmp.path().join("skills-disabled");
        let relative_path = "understand-diff";

        fs::create_dir_all(&central_skill).unwrap();
        fs::write(
            central_skill.join("SKILL.md"),
            "---\nname: understand-diff\n---\n",
        )
        .unwrap();
        fs::create_dir_all(&skills_root).unwrap();
        fs::create_dir_all(&disabled_root).unwrap();

        symlink(&central_skill, skills_root.join(relative_path)).unwrap();
        symlink(&central_skill, disabled_root.join(relative_path)).unwrap();

        set_project_skill_enabled_state(&skills_root, &disabled_root, relative_path, true).unwrap();

        assert!(skills_root.join(relative_path).exists());
        assert!(!disabled_root.join(relative_path).exists());
        assert!(central_skill.exists());
        assert!(central_skill.join("SKILL.md").is_file());
    }

    #[test]
    fn set_project_skill_enabled_state_enabling_removes_emptied_disabled_dir() {
        let tmp = tempdir().unwrap();
        let skills_root = tmp.path().join("skills");
        let disabled_root = tmp.path().join("skills-disabled");
        let relative_path = "my-skill";

        let real_disabled = disabled_root.join(relative_path);
        fs::create_dir_all(&skills_root).unwrap();
        fs::create_dir_all(&real_disabled).unwrap();
        fs::write(real_disabled.join("SKILL.md"), "---\nname: my-skill\n---\n").unwrap();

        set_project_skill_enabled_state(&skills_root, &disabled_root, relative_path, true).unwrap();

        assert!(skills_root.join(relative_path).join("SKILL.md").is_file());
        assert!(!disabled_root.exists());
    }

    #[test]
    fn set_project_skill_enabled_state_enabling_keeps_disabled_dir_when_other_skills_remain() {
        let tmp = tempdir().unwrap();
        let skills_root = tmp.path().join("skills");
        let disabled_root = tmp.path().join("skills-disabled");
        let relative_path = "skill-a";

        let real_disabled_a = disabled_root.join(relative_path);
        let real_disabled_b = disabled_root.join("skill-b");
        fs::create_dir_all(&skills_root).unwrap();
        fs::create_dir_all(&real_disabled_a).unwrap();
        fs::create_dir_all(&real_disabled_b).unwrap();
        fs::write(
            real_disabled_a.join("SKILL.md"),
            "---\nname: skill-a\n---\n",
        )
        .unwrap();
        fs::write(
            real_disabled_b.join("SKILL.md"),
            "---\nname: skill-b\n---\n",
        )
        .unwrap();

        set_project_skill_enabled_state(&skills_root, &disabled_root, relative_path, true).unwrap();

        assert!(skills_root.join(relative_path).join("SKILL.md").is_file());
        assert!(disabled_root.is_dir());
        assert!(real_disabled_b.join("SKILL.md").is_file());
    }

    #[test]
    fn set_project_skill_enabled_state_enabling_removes_empty_nested_disabled_dirs() {
        let tmp = tempdir().unwrap();
        let skills_root = tmp.path().join("skills");
        let disabled_root = tmp.path().join("skills-disabled");
        let relative_path = "category/sub/skill-a";

        let real_disabled = disabled_root.join(relative_path);
        fs::create_dir_all(&skills_root).unwrap();
        fs::create_dir_all(&real_disabled).unwrap();
        fs::write(real_disabled.join("SKILL.md"), "---\nname: skill-a\n---\n").unwrap();

        set_project_skill_enabled_state(&skills_root, &disabled_root, relative_path, true).unwrap();

        assert!(skills_root.join(relative_path).join("SKILL.md").is_file());
        assert!(!disabled_root.exists());
    }

    #[test]
    fn set_project_skill_enabled_state_rejects_real_dir_duplicate_on_enable() {
        let tmp = tempdir().unwrap();
        let skills_root = tmp.path().join("skills");
        let disabled_root = tmp.path().join("skills-disabled");
        let relative_path = "my-skill";

        let real_enabled = skills_root.join(relative_path);
        let real_disabled = disabled_root.join(relative_path);
        fs::create_dir_all(&real_enabled).unwrap();
        fs::write(real_enabled.join("SKILL.md"), "---\nname: my-skill\n---\n").unwrap();
        fs::create_dir_all(&real_disabled).unwrap();
        fs::write(real_disabled.join("SKILL.md"), "---\nname: my-skill\n---\n").unwrap();

        let err =
            set_project_skill_enabled_state(&skills_root, &disabled_root, relative_path, true)
                .unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        // Both real dirs must still exist
        assert!(real_enabled.join("SKILL.md").exists());
        assert!(real_disabled.join("SKILL.md").exists());
    }

    #[test]
    fn set_project_skill_enabled_state_rejects_real_dir_duplicate_on_disable() {
        let tmp = tempdir().unwrap();
        let skills_root = tmp.path().join("skills");
        let disabled_root = tmp.path().join("skills-disabled");
        let relative_path = "my-skill";

        let real_enabled = skills_root.join(relative_path);
        let real_disabled = disabled_root.join(relative_path);
        fs::create_dir_all(&real_enabled).unwrap();
        fs::write(real_enabled.join("SKILL.md"), "---\nname: my-skill\n---\n").unwrap();
        fs::create_dir_all(&real_disabled).unwrap();
        fs::write(real_disabled.join("SKILL.md"), "---\nname: my-skill\n---\n").unwrap();

        let err =
            set_project_skill_enabled_state(&skills_root, &disabled_root, relative_path, false)
                .unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert!(real_enabled.join("SKILL.md").exists());
        assert!(real_disabled.join("SKILL.md").exists());
    }
}
