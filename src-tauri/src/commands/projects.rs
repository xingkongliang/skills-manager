use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use serde::Serialize;
use tauri::State;

use crate::core::error::TargetConflictDetail;
use crate::core::repo_lock::RepoLock;
use crate::core::audit_log::AuditDraft;
use crate::core::skill_store::{ProjectRecord, SkillRecord, SkillStore};
use crate::core::timing::should_log_first_or_slow;
use crate::core::{
    error::AppError, file_manager, installer, project_scanner, sync_engine, sync_metadata,
    tool_adapters,
};

#[derive(Serialize, Default, Debug)]
pub struct SyncHealthDto {
    pub in_sync: usize,
    pub project_newer: usize,
    pub center_newer: usize,
    pub diverged: usize,
    pub project_only: usize,
}

#[derive(Serialize, Debug)]
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
    let mut grouped: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for adapter in tool_adapters::all_tool_adapters(store) {
        let project_dir = adapter.project_relative_skills_dir().to_string();
        if project_dir.is_empty() {
            continue;
        }
        if let Some((_, agents)) = grouped.iter_mut().find(|(dir, _)| *dir == project_dir) {
            agents.push((adapter.key, adapter.display_name));
        } else {
            grouped.push((project_dir, vec![(adapter.key, adapter.display_name)]));
        }
    }

    grouped
        .into_iter()
        .filter_map(|(relative_skills_dir, agents)| {
            let (key, first_display_name) = agents.first()?.clone();
            let display_name = if agents.len() == 1 {
                first_display_name
            } else {
                agents
                    .into_iter()
                    .map(|(_, display_name)| display_name)
                    .collect::<Vec<_>>()
                    .join(" / ")
            };
            Some(project_scanner::AgentSkillConfig {
                key,
                display_name,
                relative_skills_dir,
            })
        })
        .collect()
}

fn linked_workspace_agent_key(rec: &ProjectRecord) -> String {
    rec.linked_agent_key
        .clone()
        .unwrap_or_else(|| slugify_skill_dir_name(&rec.name))
}

fn linked_workspace_agent_name(rec: &ProjectRecord) -> String {
    rec.linked_agent_name
        .clone()
        .unwrap_or_else(|| rec.name.clone())
}

fn read_workspace_skills(
    rec: &ProjectRecord,
    configs: &[project_scanner::AgentSkillConfig],
) -> Vec<project_scanner::ProjectSkillInfo> {
    if rec.workspace_type == "linked" {
        return project_scanner::read_linked_workspace_skills(
            Path::new(&rec.path),
            rec.disabled_path.as_deref().map(Path::new),
            &linked_workspace_agent_key(rec),
            &linked_workspace_agent_name(rec),
            true,
        );
    }
    project_scanner::read_project_skills(Path::new(&rec.path), configs)
}

/// Resolve the enabled and disabled skills root directories for a given agent in a workspace.
fn resolve_agent_skills_roots(
    store: &SkillStore,
    rec: &ProjectRecord,
    agent: &str,
) -> Option<(PathBuf, Option<PathBuf>)> {
    if rec.workspace_type == "linked" {
        if linked_workspace_agent_key(rec) != agent {
            return None;
        }
        return Some((
            PathBuf::from(&rec.path),
            rec.disabled_path.as_ref().map(PathBuf::from),
        ));
    }

    let adapter = tool_adapters::all_tool_adapters(store)
        .into_iter()
        .find(|adapter| adapter.key == agent)?;
    let project_dir = adapter.project_relative_skills_dir();
    let skills_root = Path::new(&rec.path).join(project_dir);
    let disabled_root = Path::new(&rec.path).join(format!("{}-disabled", project_dir));
    Some((skills_root, Some(disabled_root)))
}

fn project_agent_targets_for_record(
    store: &SkillStore,
    rec: &ProjectRecord,
) -> Vec<ProjectAgentTargetDto> {
    if rec.workspace_type == "linked" {
        return vec![ProjectAgentTargetDto {
            key: linked_workspace_agent_key(rec),
            display_name: linked_workspace_agent_name(rec),
            enabled: true,
            installed: true,
            is_custom: false,
        }];
    }

    let disabled_tools: std::collections::HashSet<String> = store
        .get_setting("disabled_tools")
        .ok()
        .flatten()
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default()
        .into_iter()
        .collect();

    let adapters = tool_adapters::all_tool_adapters(store);
    agent_skill_configs(store)
        .into_iter()
        .map(|config| {
            // One row stands for every agent that reads this project directory
            // (Cline, Warp and GitLab Duo all read `.agents/skills`), under the
            // key of whichever comes first. It is usable while any of them is
            // installed and enabled, not only the one whose key it carries.
            let sharing: Vec<_> = adapters
                .iter()
                .filter(|adapter| {
                    adapter.project_relative_skills_dir() == config.relative_skills_dir
                })
                .collect();
            let installed: Vec<bool> = sharing
                .iter()
                .map(|adapter| adapter.is_installed())
                .collect();
            let any_installed = installed.contains(&true);
            // Count only the agents that are installed; with none installed the
            // row is unavailable anyway and reports that first.
            let enabled = sharing
                .iter()
                .zip(&installed)
                .filter(|(_, installed)| **installed || !any_installed)
                .any(|(adapter, _)| !disabled_tools.contains(&adapter.key));
            ProjectAgentTargetDto {
                enabled,
                installed: any_installed,
                is_custom: sharing.first().is_some_and(|adapter| adapter.is_custom),
                key: config.key,
                display_name: config.display_name,
            }
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
    if skill_relative_path.trim().is_empty() {
        return Err(AppError::invalid_input("Invalid skill directory path"));
    }
    let mut saw_component = false;
    for component in Path::new(skill_relative_path).components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(AppError::invalid_input("Invalid skill directory path"));
        }
        saw_component = true;
    }
    if !saw_component {
        return Err(AppError::invalid_input("Invalid skill directory path"));
    }
    Ok(())
}

pub(crate) fn ensure_dir_within_root(path: &Path, root: &Path) -> Result<(), AppError> {
    // First check that the lexical path (before symlink resolution) is under root.
    // This ensures the link itself lives where expected.
    let abs_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let abs_root = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()?.join(root)
    };
    if !abs_path.starts_with(&abs_root) {
        return Err(AppError::invalid_input("Invalid skill directory path"));
    }
    Ok(())
}

fn remove_workspace_skill_target(path: &Path) -> Result<(), AppError> {
    sync_engine::remove_target(path).map_err(AppError::io)
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
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in name.chars().flat_map(|c| c.to_lowercase()) {
        let valid = ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.';
        if valid {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches(|c| c == '-' || c == '_' || c == '.');
    if trimmed.is_empty() {
        "skill".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn source_ref_matches_skill_path(
    skill_path: &str,
    skill_canonical: Option<&PathBuf>,
    managed: &SkillRecord,
) -> bool {
    let Some(source_ref) = managed.source_ref.as_deref() else {
        return false;
    };
    if source_ref == skill_path {
        return true;
    }
    let Some(skill_canonical) = skill_canonical else {
        return false;
    };
    let Ok(source_canonical) = std::fs::canonicalize(source_ref) else {
        return false;
    };
    source_canonical == *skill_canonical
}

pub(crate) fn find_best_center_match<'a>(
    skill: &project_scanner::ProjectSkillInfo,
    all_managed: &'a [SkillRecord],
) -> Option<&'a SkillRecord> {
    let skill_hash = skill.content_hash.as_deref();
    let canonical_skill_path = std::fs::canonicalize(&skill.path).ok();

    // source_ref is the strongest direct link there is.
    if let Some(managed) = all_managed.iter().find(|managed| {
        source_ref_matches_skill_path(&skill.path, canonical_skill_path.as_ref(), managed)
    }) {
        return Some(managed);
    }

    // A content hash that names exactly one library skill outranks any
    // directory or name evidence: it says the two directories hold the same
    // bytes, while a directory name only says they were once called the same
    // thing. An export written under a slugified name can land on a directory
    // that now reads as a *different* skill (a library holding both
    // "Code Review" and "code-review" exports the first as `code-review`),
    // and this match also decides where an import writes back — binding the
    // wrong row there overwrites the other skill.
    //
    // Only a unique hash qualifies. Several skills sharing one hash is the
    // arbitrary-pick this fix exists to remove, and those fall through to the
    // directory and name layers below.
    if let Some(hash) = skill_hash {
        let mut by_hash = all_managed
            .iter()
            .filter(|managed| managed.content_hash.as_deref() == Some(hash));
        if let Some(first) = by_hash.next() {
            if by_hash.next().is_none() {
                return Some(first);
            }
        }
    }

    // The central directory name is steadier than the frontmatter name:
    // several skills shipped from one repo can share the latter.
    let by_central_dir: Vec<&SkillRecord> = all_managed
        .iter()
        .filter(|managed| {
            Path::new(&managed.central_path)
                .file_name()
                .map(|name| name.to_string_lossy().eq_ignore_ascii_case(&skill.dir_name))
                .unwrap_or(false)
        })
        .collect();
    if let Some(managed) = unique_center_match(&by_central_dir, skill_hash) {
        return Some(managed);
    }

    // Covers the ordinary skill whose name and directory agree.
    let by_name: Vec<&SkillRecord> = all_managed
        .iter()
        .filter(|managed| {
            slugify_skill_dir_name(&managed.name).eq_ignore_ascii_case(&skill.dir_name)
        })
        .collect();
    if let Some(managed) = unique_center_match(&by_name, skill_hash) {
        return Some(managed);
    }

    None
}

/// Pick the one skill among candidates sharing an identity signal,
/// disambiguating by content hash when the signal alone leaves several.
fn unique_center_match<'a>(
    candidates: &[&'a SkillRecord],
    skill_hash: Option<&str>,
) -> Option<&'a SkillRecord> {
    match candidates.len() {
        0 => None,
        1 => Some(candidates[0]),
        _ => {
            let hash = skill_hash?;
            let mut filtered = candidates
                .iter()
                .copied()
                .filter(|managed| managed.content_hash.as_deref() == Some(hash));
            let first = filtered.next()?;
            filtered.next().is_none().then_some(first)
        }
    }
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
    tauri::async_runtime::spawn_blocking(move || get_projects_internal(&store)).await?
}

pub fn get_projects_internal(store: &SkillStore) -> Result<Vec<ProjectDto>, AppError> {
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
}

#[tauri::command]
pub async fn add_project(
    store: State<'_, Arc<SkillStore>>,
    path: String,
) -> Result<ProjectDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || add_project_internal(&store, path)).await?
}

/// Validate a directory for registration and build its record without
/// touching the disk or the database, so a dry run sees the same refusals as
/// the real add.
///
/// The path is stored canonicalized and compared canonicalized: the `path`
/// column is UNIQUE on the raw string, so `/a/b`, `/a/b/` and a symlink to it
/// used to register as three projects, and a byte-identical repeat surfaced
/// only as a database error.
pub fn prepare_project_record(store: &SkillStore, path: &Path) -> Result<ProjectRecord, AppError> {
    if !path.is_dir() {
        return Err(AppError::invalid_input("Directory does not exist"));
    }
    let canonical = dunce::canonicalize(path)?;
    let stored = stored_project_path(&canonical, &std::path::absolute(path)?);
    let canonical_string = stored.to_string_lossy().to_string();
    let existing = store.get_all_projects().map_err(AppError::db)?;
    if let Some(project) = existing.iter().find(|project| {
        project.path == canonical_string
            || dunce::canonicalize(&project.path).is_ok_and(|p| p == canonical)
    }) {
        return Err(AppError::invalid_input(format!(
            "Project '{}' is already registered at {} (id: {})",
            project.name, project.path, project.id
        )));
    }

    let name = canonical
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let now = chrono::Utc::now().timestamp_millis();
    Ok(ProjectRecord {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        path: canonical_string,
        workspace_type: "project".to_string(),
        linked_agent_key: None,
        linked_agent_name: None,
        disabled_path: None,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    })
}

/// The path to store for a project. On Windows `canonicalize` returns a
/// verbatim path (`\\?\C:\repo`), in which `/` is not a separator, so joining
/// a project-relative directory such as `.agents/skills` onto it names
/// nothing. `dunce::canonicalize` drops the prefix only where that keeps the
/// same meaning (plain drive paths); it keeps `\\?\UNC\…` and names Windows
/// would reinterpret (`repo.`, `CON`). For those, store the absolute form of
/// what the caller gave, which is what was stored before canonicalization.
fn stored_project_path(canonical: &Path, absolute_input: &Path) -> PathBuf {
    if canonical.to_string_lossy().starts_with(r"\\?\") {
        absolute_input.to_path_buf()
    } else {
        canonical.to_path_buf()
    }
}

pub fn add_project_internal(store: &SkillStore, path: String) -> Result<ProjectDto, AppError> {
    let record = prepare_project_record(store, Path::new(&path))?;
    let claude_dir = Path::new(&record.path).join(".claude");

    // Support initializing an empty project directory as a managed project.
    std::fs::create_dir_all(claude_dir.join("skills"))?;
    std::fs::create_dir_all(claude_dir.join("skills-disabled"))?;

    store.insert_project(&record).map_err(AppError::db)?;
    let all_managed = store.get_all_skills().map_err(AppError::db)?;
    let configs = agent_skill_configs(&store);
    Ok(project_to_dto(&record, &all_managed, &configs))
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
    tauri::async_runtime::spawn_blocking(move || remove_project_internal(&store, id)).await?
}

pub fn remove_project_internal(store: &SkillStore, id: String) -> Result<(), AppError> {
    store.delete_project(&id).map_err(AppError::db)
}

/// Show a workspace's folder in the OS file manager, selected where the
/// platform can (Linux `xdg-open` opens the containing directory).
///
/// The path is resolved from the id here instead of being accepted from the
/// frontend, so a caller cannot ask the OS to open an arbitrary path. The
/// folder is checked first because a file manager given a missing path either
/// fails silently or opens somewhere the user did not ask for.
#[tauri::command]
pub async fn reveal_project_folder(
    store: State<'_, Arc<SkillStore>>,
    project_id: String,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let record = store
            .get_project_by_id(&project_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Workspace not found"))?;

        let path = PathBuf::from(&record.path);
        if !path.is_dir() {
            return Err(AppError::invalid_input(format!(
                "Folder does not exist: {}",
                record.path
            )));
        }

        // explorer.exe exits non-zero even when it succeeds, so the platform
        // branch is judged by `exit_is_failure` rather than checked here.
        let status = file_manager::reveal_item(&path).map_err(AppError::io)?;
        if file_manager::exit_is_failure(status.success()) {
            return Err(AppError::io(format!(
                "File manager exited with status: {status}"
            )));
        }
        Ok(())
    })
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
        get_project_agent_targets_internal(&store, project_id)
    })
    .await?
}

pub fn get_project_agent_targets_internal(
    store: &SkillStore,
    project_id: String,
) -> Result<Vec<ProjectAgentTargetDto>, AppError> {
    let record = store
        .get_project_by_id(&project_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Workspace not found"))?;
    Ok(project_agent_targets_for_record(&store, &record))
}

#[tauri::command]
pub async fn get_project_skills(
    store: State<'_, Arc<SkillStore>>,
    project_id: String,
) -> Result<Vec<project_scanner::ProjectSkillInfo>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        get_project_skills_internal(&store, project_id)
    })
    .await?
}

pub fn get_project_skills_internal(
    store: &SkillStore,
    project_id: String,
) -> Result<Vec<project_scanner::ProjectSkillInfo>, AppError> {
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
        // "Update to center" is the user's explicit overwrite; the frontend
        // already refuses when several copies differ (pushSkillToCenterAndAlign).
        import_project_skill_to_center_internal(
            &store,
            project_id,
            skill_relative_path,
            agent,
            true,
        )
        .map(|_| ())
    })
    .await?
}

/// Push a project skill into the library: overwrite the library skill it
/// matches, or create a new local skill. Without `force`, refuses to overwrite
/// a library copy whose content differs. A copy already in sync is left alone.
pub fn import_project_skill_to_center_internal(
    store: &SkillStore,
    project_id: String,
    skill_relative_path: String,
    agent: String,
    force: bool,
) -> Result<ProjectImportResult, AppError> {
    // The CLI and the app can write the library at the same time, so the
    // destination is resolved and authorized under the lock: a plan made
    // before it could miss a same-named skill another writer just installed
    // and overwrite it without --force.
    let _lock = RepoLock::acquire_foreground("import project skill").map_err(AppError::io)?;
    let (mut outcome, skill) =
        resolve_project_import(store, &project_id, &skill_relative_path, &agent)?;
    if let Some(refusal) = project_import_refusal(&outcome, force) {
        return Err(refusal);
    }
    if outcome.action == ProjectImportAction::Unchanged {
        return Ok(outcome);
    }

    let source_path = PathBuf::from(&skill.path);
    let all_managed = store.get_all_skills().map_err(AppError::db)?;
    // Write to the library skill the authorized plan matched (by the same
    // find_best_center_match the UI uses), nothing re-derived after it.
    // After updating, bind source_ref so future imports match by exact path.
    let matched = outcome
        .skill_id
        .as_ref()
        .and_then(|id| all_managed.iter().find(|managed| &managed.id == id));
    if let Some(existing) = matched {
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
        sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::io)?;
        store.log_audit(
            AuditDraft::new("import")
                .skill(existing.id.clone(), existing.name.clone())
                .detail(format!("from project copy {}", skill.path))
                .ok(),
        );
        outcome.action = ProjectImportAction::Overwrite;
        outcome.skill_id = Some(existing.id.clone());
        outcome.name = existing.name.clone();
        outcome.central_path = Some(existing.central_path.clone());
        return Ok(outcome);
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
    sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::io)?;
    store.log_audit(
        AuditDraft::new("import")
            .skill(id.clone(), result.name.clone())
            .detail(format!("from project copy {}", skill.path))
            .ok(),
    );

    outcome.action = ProjectImportAction::Create;
    outcome.skill_id = Some(id);
    outcome.name = result.name;
    outcome.central_path = Some(skill_record.central_path);
    Ok(outcome)
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
        export_skill_to_project_internal(&store, skill_id, project_id, agents)
    })
    .await?
}

pub fn export_skill_to_project_internal(
    store: &SkillStore,
    skill_id: String,
    project_id: String,
    agents: Option<Vec<String>>,
) -> Result<(), AppError> {
    let project = get_project_record(store, &project_id)?;

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
            project_agent_targets_for_record(store, &project)
                .into_iter()
                .filter(|target| target.installed && target.enabled)
                .map(|target| target.key)
                .collect();
        // A shared directory's row carries one member's key; map the others
        // onto it so naming any agent that reads the directory works.
        let mut filtered: Vec<String> = Vec::new();
        for key in requested_agent_keys {
            let key = map_to_group_key(store, &project, key);
            if available_targets.contains(&key) && !filtered.contains(&key) {
                filtered.push(key);
            }
        }
        if filtered.is_empty() {
            return Err(AppError::invalid_input(
                "No enabled installed agents selected for this project",
            ));
        }
        filtered
    };

    let plan = plan_project_deploy(store, &project_id, &[skill_id], &agent_keys)?;
    // The GUI only offers "add" where nothing is there yet, so anything in the
    // way, even this same skill, is refused as before.
    if let Some(occupied) = plan
        .iter()
        .find(|target| target.state != ProjectDeployState::Create)
    {
        return Err(AppError::invalid_input(format!(
            "Skill \"{}\" already exists in this workspace for agent {}",
            skill.name, occupied.agent
        )));
    }
    apply_project_deploy(store, &plan)
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
        // The frontend has already asked before removing a copy that is not
        // in sync (confirmUnsyncedRemoval), so the guard below is the CLI's.
        delete_project_skill_internal(&store, project_id, skill_relative_path, agent, true)
    })
    .await?
}

/// Remove one skill directory from a project workspace: the enabled copy if
/// there is one, else the disabled one (the slot the GUI has always picked).
/// Without `force`, refuses unless that very directory is a recognizable
/// skill that is safe to remove.
pub fn delete_project_skill_internal(
    store: &SkillStore,
    project_id: String,
    skill_relative_path: String,
    agent: String,
    force: bool,
) -> Result<(), AppError> {
    ensure_safe_skill_relative_path(&skill_relative_path)?;

    let record = get_project_record(store, &project_id)?;
    ensure_workspace_path_exists(&record)?;

    let (skills_root, disabled_root) = resolve_agent_skills_roots(store, &record, &agent)
        .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent)))?;
    let skills_dir = skills_root.join(&skill_relative_path);
    let disabled_dir = disabled_root
        .as_ref()
        .map(|root| root.join(&skill_relative_path));

    let (target, target_root) = if skills_dir.is_dir() {
        (skills_dir, skills_root)
    } else if let Some(disabled_dir) = disabled_dir.filter(|path| path.is_dir()) {
        (
            disabled_dir,
            disabled_root.expect("present when disabled_dir exists"),
        )
    } else {
        return Err(AppError::not_found("Skill directory not found"));
    };

    ensure_dir_within_root(&target, &target_root)?;
    // Check the directory that is about to go, not one found by name: the
    // enabled and disabled slots can hold different things.
    if !force {
        ensure_removable(store, &record, &target)?;
    }
    remove_workspace_skill_target(&target)?;
    Ok(())
}

/// Remove exactly the copy a removal plan named. Without `force`, the copy is
/// checked again first.
pub fn remove_planned_project_copy(
    store: &SkillStore,
    project_id: &str,
    target: &ProjectRemovalTarget,
    force: bool,
) -> Result<(), AppError> {
    let record = get_project_record(store, project_id)?;
    ensure_workspace_path_exists(&record)?;
    let group = project_agent_group(store, &record, &target.agent)
        .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", target.agent)))?;
    let path = PathBuf::from(&target.path);
    let root = std::iter::once(PathBuf::from(&group.skills_root))
        .chain(group.disabled_root.clone())
        .find(|root| path.starts_with(root))
        .ok_or_else(|| AppError::invalid_input("Invalid skill directory path"))?;
    ensure_dir_within_root(&path, &root)?;
    if !force {
        ensure_removable(store, &record, &path)?;
    }
    remove_workspace_skill_target(&path)
}

/// Refuse unless what sits at `target` is a scanned project skill whose
/// removal cannot lose content the library lacks.
fn ensure_removable(store: &SkillStore, record: &ProjectRecord, target: &Path) -> Result<(), AppError> {
    let configs = agent_skill_configs(store);
    let variants = read_workspace_skills(record, &configs);
    let all_managed = store.get_all_skills().map_err(AppError::db)?;
    let reason = match variants.iter().find(|v| Path::new(&v.path) == target) {
        None => Some("is not a recognizable skill directory".to_string()),
        Some(variant) => {
            let matched = find_best_center_match(variant, &all_managed);
            let status = classify_sync_status(variant, matched);
            (!removal_is_safe(variant, matched, &status))
                .then(|| unsafe_removal_reason(&status))
        }
    };
    match reason {
        None => Ok(()),
        Some(reason) => Err(AppError::target_conflict(
            "Refusing to remove a project skill copy that may hold content not in the library",
            vec![TargetConflictDetail {
                path: target.to_string_lossy().to_string(),
                reason,
            }],
        )),
    }
}

// ── Planning shared by the project commands and the projects CLI ──────────
//
// The GUI decides a lot in the frontend before it calls a project command: it
// only offers "add" where nothing sits in the way, and it asks before removing
// a copy that is not in sync. The CLI has no such frontend, so the checks it
// needs live here, where a dry run and the real run read the same answer.

fn get_project_record(store: &SkillStore, project_id: &str) -> Result<ProjectRecord, AppError> {
    store
        .get_project_by_id(project_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Workspace not found"))
}

/// A workspace whose folder is gone scans as "no skills", and a deploy into it
/// would `create_dir_all` the project back into existence. Refuse instead.
fn ensure_workspace_path_exists(rec: &ProjectRecord) -> Result<(), AppError> {
    if Path::new(&rec.path).is_dir() {
        Ok(())
    } else {
        Err(AppError::invalid_input(format!(
            "Project path does not exist: {}",
            rec.path
        )))
    }
}

/// One project skills directory and the agents that read it. `key` is the
/// agent the directory's row is filed under (the first registered adapter
/// that reads it), which is the key project scans report as `agent`.
#[derive(Serialize, Clone, Debug)]
pub struct ProjectAgentGroup {
    pub key: String,
    pub shared_by: Vec<String>,
    pub skills_root: String,
    #[serde(skip)]
    pub disabled_root: Option<PathBuf>,
}

/// Resolve any agent key to the project directory it writes into. Warp, Cline
/// and GitLab Duo all read `.agents/skills`, and the row is filed under one of
/// them, so a caller naming another member must land on the same directory
/// instead of being told the agent is not available.
pub fn project_agent_group(
    store: &SkillStore,
    rec: &ProjectRecord,
    agent: &str,
) -> Option<ProjectAgentGroup> {
    if rec.workspace_type == "linked" {
        let key = linked_workspace_agent_key(rec);
        if key != agent {
            return None;
        }
        return Some(ProjectAgentGroup {
            shared_by: vec![key.clone()],
            key,
            skills_root: rec.path.clone(),
            disabled_root: rec.disabled_path.as_ref().map(PathBuf::from),
        });
    }

    let adapters = tool_adapters::all_tool_adapters(store);
    let dir = adapters
        .iter()
        .find(|adapter| adapter.key == agent)?
        .project_relative_skills_dir()
        .to_string();
    if dir.is_empty() {
        return None;
    }
    let key = agent_skill_configs(store)
        .into_iter()
        .find(|config| config.relative_skills_dir == dir)?
        .key;
    let shared_by = adapters
        .iter()
        .filter(|adapter| adapter.project_relative_skills_dir() == dir)
        .map(|adapter| adapter.key.clone())
        .collect();
    let (skills_root, disabled_root) = resolve_agent_skills_roots(store, rec, &key)?;
    Some(ProjectAgentGroup {
        key,
        shared_by,
        skills_root: skills_root.to_string_lossy().to_string(),
        disabled_root,
    })
}

/// Every directory group of a workspace, in the order its rows are listed.
pub fn project_agent_groups(store: &SkillStore, rec: &ProjectRecord) -> Vec<ProjectAgentGroup> {
    if rec.workspace_type == "linked" {
        return project_agent_group(store, rec, &linked_workspace_agent_key(rec))
            .into_iter()
            .collect();
    }
    agent_skill_configs(store)
        .into_iter()
        .filter_map(|config| project_agent_group(store, rec, &config.key))
        .collect()
}

fn map_to_group_key(store: &SkillStore, rec: &ProjectRecord, agent: String) -> String {
    project_agent_group(store, rec, &agent)
        .map(|group| group.key)
        .unwrap_or(agent)
}

/// The scanned copy of a skill directory for one directory group. Exact match
/// first; on a case-insensitive filesystem the directory may differ in case.
fn find_variant<'a>(
    variants: &'a [project_scanner::ProjectSkillInfo],
    agent: &str,
    relative_path: &str,
) -> Option<&'a project_scanner::ProjectSkillInfo> {
    variants
        .iter()
        .find(|v| v.agent == agent && v.relative_path == relative_path)
        .or_else(|| {
            variants.iter().find(|v| {
                v.agent == agent && v.relative_path.eq_ignore_ascii_case(relative_path)
            })
        })
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectDeployState {
    Create,
    AlreadyDeployed,
    Conflict,
}

#[derive(Serialize, Clone, Debug)]
pub struct ProjectDeployTarget {
    pub skill_id: String,
    pub skill: String,
    pub agent: String,
    pub shared_by: Vec<String>,
    pub path: String,
    pub state: ProjectDeployState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_status: Option<String>,
    #[serde(skip)]
    source: PathBuf,
}

/// Decide, without writing, what deploying `skill_ids` to `agents` in a
/// project would do for each (skill, directory) pair: create it, find this
/// very skill already there, or find something else in the way.
///
/// Agents must be installed and enabled (any member of a shared directory
/// counts, as in the project's agent rows); linked workspaces accept only
/// their own key.
pub fn plan_project_deploy(
    store: &SkillStore,
    project_id: &str,
    skill_ids: &[String],
    agents: &[String],
) -> Result<Vec<ProjectDeployTarget>, AppError> {
    let project = get_project_record(store, project_id)?;
    ensure_workspace_path_exists(&project)?;
    if agents.is_empty() {
        return Err(AppError::invalid_input("No agent selected"));
    }

    let rows = project_agent_targets_for_record(store, &project);
    let mut groups: Vec<ProjectAgentGroup> = Vec::new();
    for agent in agents {
        let group = project_agent_group(store, &project, agent)
            .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent)))?;
        if project.workspace_type != "linked" {
            let row = rows
                .iter()
                .find(|row| row.key == group.key)
                .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent)))?;
            if !row.installed {
                return Err(AppError::invalid_input(format!(
                    "Agent is not installed: {}",
                    agent
                )));
            }
            if !row.enabled {
                return Err(AppError::invalid_input(format!("Agent is disabled: {}", agent)));
            }
        }
        if !groups.iter().any(|existing| existing.key == group.key) {
            groups.push(group);
        }
    }

    let configs = agent_skill_configs(store);
    let variants = read_workspace_skills(&project, &configs);
    let all_managed = store.get_all_skills().map_err(AppError::db)?;

    let mut plan = Vec::new();
    for skill_id in skill_ids {
        let skill = store
            .get_skill_by_id(skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;
        let source = PathBuf::from(&skill.central_path);
        // The write copies or links this directory; without it the run fails,
        // so the plan (and with it a dry run) fails first.
        if !source.is_dir() {
            return Err(AppError::not_found(format!(
                "Library copy of \"{}\" is missing: {}",
                skill.name,
                source.display()
            )));
        }
        let dir_name = sync_engine::target_dir_name(&source, &skill.name);
        ensure_safe_skill_relative_path(&dir_name)?;

        for group in &groups {
            let enabled_path = Path::new(&group.skills_root).join(&dir_name);
            let disabled_path = group.disabled_root.as_ref().map(|root| root.join(&dir_name));
            // symlink_metadata, not exists(): a dangling link is in the way too.
            let occupied = std::iter::once(enabled_path.clone())
                .chain(disabled_path)
                .find(|path| std::fs::symlink_metadata(path).is_ok());

            let (state, sync_status, path) = match occupied {
                None => (ProjectDeployState::Create, None, enabled_path),
                Some(path) => {
                    let variant = find_variant(&variants, &group.key, &dir_name);
                    let matched = variant.and_then(|v| find_best_center_match(v, &all_managed));
                    match (variant, matched) {
                        (Some(variant), Some(matched)) if matched.id == skill.id => (
                            ProjectDeployState::AlreadyDeployed,
                            Some(classify_sync_status(variant, Some(matched))),
                            path,
                        ),
                        _ => (ProjectDeployState::Conflict, None, path),
                    }
                }
            };
            plan.push(ProjectDeployTarget {
                skill_id: skill.id.clone(),
                skill: skill.name.clone(),
                agent: group.key.clone(),
                shared_by: group.shared_by.clone(),
                path: path.to_string_lossy().to_string(),
                state,
                sync_status,
                source: source.clone(),
            });
        }
    }

    // Two library skills whose directories share a name (`a/demo`, `b/demo`)
    // would both be written to one project directory; only the first would
    // land. Refuse the batch instead of reporting both as created.
    let mut collisions: Vec<TargetConflictDetail> = Vec::new();
    for (index, target) in plan.iter().enumerate() {
        if target.state != ProjectDeployState::Create {
            continue;
        }
        if let Some(earlier) = plan[..index].iter().find(|earlier| {
            earlier.state == ProjectDeployState::Create
                && earlier.path == target.path
                && earlier.skill_id != target.skill_id
        }) {
            collisions.push(TargetConflictDetail {
                path: target.path.clone(),
                // Names alone can match (both frontmatter names may be
                // "demo"), so say which library directories collide.
                reason: format!(
                    "skills \"{}\" ({}) and \"{}\" ({}) would both be written here",
                    earlier.skill,
                    earlier.source.display(),
                    target.skill,
                    target.source.display()
                ),
            });
        }
    }
    if !collisions.is_empty() {
        return Err(AppError::target_conflict(
            format!(
                "Refusing to deploy: {} project target(s) would receive two different skills",
                collisions.len()
            ),
            collisions,
        ));
    }
    Ok(plan)
}

/// The refusal for a deploy plan that has something else in the way, in the
/// shape the CLI envelope and the frontend already read. `None` when clear.
pub fn project_deploy_conflicts(plan: &[ProjectDeployTarget]) -> Option<AppError> {
    let conflicts: Vec<TargetConflictDetail> = plan
        .iter()
        .filter(|target| target.state == ProjectDeployState::Conflict)
        .map(|target| TargetConflictDetail {
            path: target.path.clone(),
            reason: "exists and is not this library skill".to_string(),
        })
        .collect();
    if conflicts.is_empty() {
        return None;
    }
    Some(AppError::target_conflict(
        format!(
            "Refusing to deploy: {} of {} project target(s) already hold something else",
            conflicts.len(),
            plan.len()
        ),
        conflicts,
    ))
}

/// Write the `Create` targets of a plan. Callers decide first what to do about
/// conflicts and existing copies; NoClobber still refuses anything that
/// appeared since the plan was made.
pub fn apply_project_deploy(
    store: &SkillStore,
    plan: &[ProjectDeployTarget],
) -> Result<(), AppError> {
    let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
    // Two agents can resolve to the same project skills root; write it once.
    let mut written: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for target in plan
        .iter()
        .filter(|target| target.state == ProjectDeployState::Create)
    {
        let target_dir = PathBuf::from(&target.path);
        if !written.insert(target_dir.clone()) {
            continue;
        }
        if let Some(parent) = target_dir.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mode = sync_engine::sync_mode_for_tool(&target.agent, configured_mode.as_deref());
        sync_engine::sync_skill(
            &target.source,
            &target_dir,
            mode,
            sync_engine::ReplacePolicy::NoClobber,
        )
        .map_err(AppError::io)?;
    }
    Ok(())
}

#[derive(Serialize, Clone, Debug)]
pub struct ProjectRemovalTarget {
    pub skill_id: String,
    pub skill: String,
    pub agent: String,
    pub shared_by: Vec<String>,
    pub relative_path: String,
    pub path: String,
    pub sync_status: String,
    /// False when the copy may hold content the library does not have.
    pub safe: bool,
}

/// Only a hash match proves a copy holds nothing of its own; `center_newer`
/// is reached after the hashes already differed (see pushSkillToCenterAndAlign
/// in ProjectDetail.tsx). A symlink holds no content of its own either.
///
/// A link is safe only when it resolves to the matched library skill's own
/// directory, i.e. it is our deployment. A link the user made to somewhere
/// else is their configuration, whatever its target holds.
fn removal_is_safe(
    variant: &project_scanner::ProjectSkillInfo,
    matched: Option<&SkillRecord>,
    sync_status: &str,
) -> bool {
    let is_link = std::fs::symlink_metadata(&variant.path)
        .is_ok_and(|meta| meta.file_type().is_symlink());
    if is_link {
        return matched.is_some_and(|managed| {
            match (
                dunce::canonicalize(&variant.path),
                dunce::canonicalize(&managed.central_path),
            ) {
                (Ok(link_target), Ok(library_copy)) => link_target == library_copy,
                _ => false,
            }
        });
    }
    // A real copy needs equal hashes *and* nothing the library copy lacks:
    // the hash skips `.gitignore`, `.git` and links, so equal hashes alone
    // would let those go with it (#593). An unreadable entry is not proof of
    // absence, so an error here is unsafe too.
    sync_status == "in_sync"
        && matched.is_some_and(|managed| {
            crate::core::removals::removed_paths(
                Path::new(&variant.path),
                Path::new(&managed.central_path),
            )
            .is_ok_and(|extra| extra.is_empty())
        })
}

/// Why a copy is not safe to remove. An in-sync copy can only be unsafe
/// because it holds entries the hash does not see.
fn unsafe_removal_reason(sync_status: &str) -> String {
    if sync_status == "in_sync" {
        "has content not in the library (files the library copy lacks, such as .gitignore, .git or links)".to_string()
    } else {
        format!("has content not in the library (sync status: {sync_status})")
    }
}

/// The project copies of library skills `skill_ids` that an undeploy would
/// remove. Empty `agents` means every directory of the project. Agents that
/// are disabled or uninstalled are accepted, so stale copies can be cleaned.
/// Copies that match no library skill (`project_only`) are never listed.
pub fn plan_project_undeploy(
    store: &SkillStore,
    project_id: &str,
    skill_ids: &[String],
    agents: &[String],
) -> Result<Vec<ProjectRemovalTarget>, AppError> {
    let project = get_project_record(store, project_id)?;
    ensure_workspace_path_exists(&project)?;

    let groups = if agents.is_empty() {
        project_agent_groups(store, &project)
    } else {
        let mut groups: Vec<ProjectAgentGroup> = Vec::new();
        for agent in agents {
            let group = project_agent_group(store, &project, agent)
                .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent)))?;
            if !groups.iter().any(|existing| existing.key == group.key) {
                groups.push(group);
            }
        }
        groups
    };
    for skill_id in skill_ids {
        if store.get_skill_by_id(skill_id).map_err(AppError::db)?.is_none() {
            return Err(AppError::not_found("Skill not found"));
        }
    }

    let configs = agent_skill_configs(store);
    let variants = read_workspace_skills(&project, &configs);
    let all_managed = store.get_all_skills().map_err(AppError::db)?;

    let mut plan = Vec::new();
    for variant in &variants {
        let Some(group) = groups.iter().find(|group| group.key == variant.agent) else {
            continue;
        };
        let Some(matched) = find_best_center_match(variant, &all_managed) else {
            continue;
        };
        if !skill_ids.contains(&matched.id) {
            continue;
        }
        let sync_status = classify_sync_status(variant, Some(matched));
        plan.push(ProjectRemovalTarget {
            skill_id: matched.id.clone(),
            skill: matched.name.clone(),
            agent: group.key.clone(),
            shared_by: group.shared_by.clone(),
            relative_path: variant.relative_path.clone(),
            path: variant.path.clone(),
            safe: removal_is_safe(variant, Some(matched), &sync_status),
            sync_status,
        });
    }
    Ok(plan)
}

/// The refusal for removals that could lose content. `None` when all are safe.
pub fn project_removal_conflicts(plan: &[ProjectRemovalTarget]) -> Option<AppError> {
    let conflicts: Vec<TargetConflictDetail> = plan
        .iter()
        .filter(|target| !target.safe)
        .map(|target| TargetConflictDetail {
            path: target.path.clone(),
            reason: unsafe_removal_reason(&target.sync_status),
        })
        .collect();
    if conflicts.is_empty() {
        return None;
    }
    Some(AppError::target_conflict(
        format!(
            "Refusing to remove {} project skill copy(ies) that may hold content not in the library",
            conflicts.len()
        ),
        conflicts,
    ))
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectImportAction {
    Create,
    Unchanged,
    Overwrite,
}

#[derive(Serialize, Clone, Debug)]
pub struct ProjectImportResult {
    pub action: ProjectImportAction,
    pub skill_id: Option<String>,
    pub name: String,
    pub central_path: Option<String>,
    pub agent: String,
    pub relative_path: String,
    pub path: String,
}

/// Find the project copy an import reads and decide what it would do to the
/// library. Refuses when more than one copy of the skill in the project holds
/// content the library lacks: there is then no copy that is safe to push, the
/// rule pushSkillToCenterAndAlign (ProjectDetail.tsx) enforces in the GUI.
fn resolve_project_import(
    store: &SkillStore,
    project_id: &str,
    skill_relative_path: &str,
    agent: &str,
) -> Result<(ProjectImportResult, project_scanner::ProjectSkillInfo), AppError> {
    ensure_safe_skill_relative_path(skill_relative_path)?;
    let project = get_project_record(store, project_id)?;
    ensure_workspace_path_exists(&project)?;
    let group_key = map_to_group_key(store, &project, agent.to_string());

    let configs = agent_skill_configs(store);
    let variants = read_workspace_skills(&project, &configs);
    let skill = variants
        .iter()
        .find(|s| s.relative_path == skill_relative_path && s.agent == group_key)
        .cloned()
        .ok_or_else(|| AppError::not_found("Skill not found in workspace"))?;
    let all_managed = store.get_all_skills().map_err(AppError::db)?;

    let key = skill.relative_path.to_lowercase();
    let unsynced: Vec<&project_scanner::ProjectSkillInfo> = variants
        .iter()
        .filter(|v| v.relative_path.to_lowercase() == key)
        .filter(|v| classify_sync_status(v, find_best_center_match(v, &all_managed)) != "in_sync")
        .collect();
    if unsynced.len() > 1 {
        return Err(AppError::target_conflict(
            format!(
                "Refusing to import: {} copies of this skill in the project hold content not in the library, so no single copy is safe to push",
                unsynced.len()
            ),
            unsynced
                .iter()
                .map(|v| TargetConflictDetail {
                    path: v.path.clone(),
                    reason: "another copy of this skill also differs from the library".to_string(),
                })
                .collect(),
        ));
    }

    let matched = find_best_center_match(&skill, &all_managed);
    if matched.is_none() {
        // The same name check install_from_local applies to a new skill, so
        // a dry run refuses a name the import itself would refuse.
        installer::resolve_local_skill_name(Path::new(&skill.path), Some(&skill.name))
            .map_err(|e| AppError::invalid_input(e.to_string()))?;
    }
    let action = match matched {
        None => ProjectImportAction::Create,
        Some(managed) if classify_sync_status(&skill, Some(managed)) == "in_sync" => {
            ProjectImportAction::Unchanged
        }
        Some(_) => ProjectImportAction::Overwrite,
    };
    Ok((
        ProjectImportResult {
            action,
            skill_id: matched.map(|m| m.id.clone()),
            name: matched.map(|m| m.name.clone()).unwrap_or_else(|| skill.name.clone()),
            central_path: matched.map(|m| m.central_path.clone()),
            agent: group_key,
            relative_path: skill.relative_path.clone(),
            path: skill.path.clone(),
        },
        skill,
    ))
}

/// The refusal an import plan meets without `force`: overwriting a library
/// copy whose content differs. Shared so a dry run refuses what the run would.
pub fn project_import_refusal(plan: &ProjectImportResult, force: bool) -> Option<AppError> {
    if plan.action != ProjectImportAction::Overwrite || force {
        return None;
    }
    Some(AppError::target_conflict(
        format!(
            "Refusing to overwrite the library copy of \"{}\": its content differs from the project copy",
            plan.name
        ),
        vec![TargetConflictDetail {
            path: plan.central_path.clone().unwrap_or_default(),
            reason: "library copy differs from the project copy".to_string(),
        }],
    ))
}

/// What importing a project skill would do, without writing anything.
pub fn plan_project_import(
    store: &SkillStore,
    project_id: &str,
    skill_relative_path: &str,
    agent: &str,
) -> Result<ProjectImportResult, AppError> {
    resolve_project_import(store, project_id, skill_relative_path, agent).map(|(plan, _)| plan)
}

#[cfg(test)]
mod tests {
    use super::{
        classify_sync_status, ensure_distinct_linked_workspace_roots, find_best_center_match,
        project_to_dto, remove_workspace_skill_target, set_project_skill_enabled_state,
    };
    use crate::core::content_hash;
    use crate::core::error::ErrorKind;
    use crate::core::project_scanner::{AgentSkillConfig, ProjectSkillInfo};
    use crate::core::skill_store::{ProjectRecord, SkillRecord, SkillStore};
    use std::fs;
    use tempfile::tempdir;

    /// Several agents share `.agents/skills` in a project, and their row carries
    /// the first one's key. That agent being off must not take the directory
    /// away from Warp; only all of them being off does. Which agent comes first
    /// changes as adapters are added (#605 put Antigravity ahead of Cline), so
    /// the test reads it instead of naming it.
    #[test]
    fn a_shared_project_dir_stays_usable_while_another_agent_reads_it() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        // A path override counts as installed, whatever is on this machine.
        let warp_path = tmp.path().join("warp-skills").to_string_lossy().to_string();
        store
            .set_setting(
                "custom_tool_paths",
                &serde_json::json!({ "warp": warp_path }).to_string(),
            )
            .unwrap();
        let record = ProjectRecord {
            id: "p".to_string(),
            name: "p".to_string(),
            path: tmp.path().to_string_lossy().to_string(),
            workspace_type: "project".to_string(),
            linked_agent_key: None,
            linked_agent_name: None,
            disabled_path: None,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        };
        let shared_row = |store: &SkillStore| {
            super::project_agent_targets_for_record(store, &record)
                .into_iter()
                .find(|target| target.display_name.contains("Warp"))
                .expect("a row for the shared .agents/skills directory")
        };

        let disable = |store: &SkillStore, keys: &[String]| {
            store
                .set_setting("disabled_tools", &serde_json::to_string(keys).unwrap())
                .unwrap();
        };

        let key = shared_row(&store).key;
        assert_ne!(key, "warp", "the row must be keyed under another agent");
        disable(&store, &[key.clone()]);
        let row = shared_row(&store);
        assert_eq!(row.key, key);
        assert!(row.installed, "Warp is installed");
        assert!(row.enabled, "Warp still reads the directory");

        let sharing: Vec<String> = crate::core::tool_adapters::all_tool_adapters(&store)
            .into_iter()
            .filter(|adapter| adapter.project_relative_skills_dir() == ".agents/skills")
            .map(|adapter| adapter.key)
            .collect();
        disable(&store, &sharing);
        assert!(!shared_row(&store).enabled, "nobody left to read it");
    }

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

#[cfg(test)]
mod add_project_tests {
    use super::add_project_internal;
    use crate::core::skill_store::SkillStore;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn add_project_creates_skill_roots_and_stores_the_canonical_path() {
        let tmp = tempdir().unwrap();
        let project_dir = tmp.path().join("repo");
        fs::create_dir(&project_dir).unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();

        let dto = add_project_internal(&store, project_dir.to_string_lossy().to_string()).unwrap();

        let canonical = dunce::canonicalize(&project_dir).unwrap();
        assert_eq!(dto.path, canonical.to_string_lossy());
        assert_eq!(dto.name, "repo");
        assert!(project_dir.join(".claude/skills").is_dir());
        assert!(project_dir.join(".claude/skills-disabled").is_dir());
        assert_eq!(store.get_all_projects().unwrap().len(), 1);
    }

    #[test]
    fn add_project_rejects_a_missing_directory_without_side_effects() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let missing = tmp.path().join("missing");

        let err = add_project_internal(&store, missing.to_string_lossy().to_string()).unwrap_err();

        assert!(err.to_string().contains("Directory does not exist"));
        assert!(!missing.exists());
        assert!(store.get_all_projects().unwrap().is_empty());
    }

    /// The stored path must not keep Windows' verbatim prefix: `/` in a
    /// project-relative dir like `.agents/skills` is not a separator there.
    /// dunce strips it from drive paths; it keeps UNC shares verbatim, so
    /// those fall back to the caller's own absolute spelling.
    #[test]
    fn stored_project_paths_never_keep_a_verbatim_prefix_dunce_left() {
        use super::stored_project_path as stored;
        use std::path::Path;
        assert_eq!(
            stored(Path::new(r"\\?\UNC\server\share\repo"), Path::new(r"\\server\share\repo")),
            Path::new(r"\\server\share\repo")
        );
        assert_eq!(
            stored(Path::new("/private/tmp/repo"), Path::new("/tmp/repo")),
            Path::new("/private/tmp/repo"),
            "an ordinary canonical path is stored canonical"
        );
    }

    /// dunce's own rules, which only exist on Windows: a drive path loses the
    /// prefix; a name Windows would reinterpret keeps it.
    #[cfg(windows)]
    #[test]
    fn dunce_simplifies_only_what_keeps_its_meaning() {
        use std::path::Path;
        assert_eq!(dunce::simplified(Path::new(r"\\?\C:\Users\me\repo")), Path::new(r"C:\Users\me\repo"));
        assert_eq!(dunce::simplified(Path::new(r"\\?\C:\repo.")), Path::new(r"\\?\C:\repo."));
        assert_eq!(dunce::simplified(Path::new(r"\\?\C:\CON\x")), Path::new(r"\\?\C:\CON\x"));
        assert_eq!(dunce::simplified(Path::new(r"\\?\UNC\server\share\repo")), Path::new(r"\\?\UNC\server\share\repo"));
    }

    /// `/a/b`, `/a/b/` and `/a/x/../b` are one project. The raw-string UNIQUE
    /// column let the spellings through as separate rows.
    #[test]
    fn add_project_rejects_another_spelling_of_a_registered_path() {
        let tmp = tempdir().unwrap();
        let project_dir = tmp.path().join("repo");
        fs::create_dir(&project_dir).unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let first = add_project_internal(&store, project_dir.to_string_lossy().to_string()).unwrap();

        for spelling in [
            format!("{}/", project_dir.to_string_lossy()),
            project_dir.join("..").join("repo").to_string_lossy().to_string(),
        ] {
            let err = add_project_internal(&store, spelling.clone()).unwrap_err();
            assert!(err.to_string().contains("already registered"), "{spelling}: {err}");
            assert!(err.to_string().contains(&first.id), "{spelling}: {err}");
        }
        assert_eq!(store.get_all_projects().unwrap().len(), 1);
    }
}

#[cfg(test)]
mod project_plan_tests {
    use super::*;
    use crate::core::{central_repo, content_hash};
    use std::fs;
    use tempfile::{tempdir, TempDir};

    struct Fixture {
        _guard: std::sync::MutexGuard<'static, ()>,
        _tmp: TempDir,
        store: SkillStore,
        project_id: String,
        repo: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            central_repo::set_test_base_dir_override(None);
        }
    }

    /// A store with one registered project. Path overrides make the named
    /// agents count as installed on any machine.
    fn fixture(installed_agents: &[&str]) -> Fixture {
        let guard = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        central_repo::set_test_base_dir_override(Some(tmp.path().join("center")));
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let overrides: serde_json::Map<String, serde_json::Value> = installed_agents
            .iter()
            .map(|key| {
                let dir = tmp.path().join("global").join(key);
                (key.to_string(), serde_json::Value::String(dir.to_string_lossy().to_string()))
            })
            .collect();
        store
            .set_setting("custom_tool_paths", &serde_json::Value::Object(overrides).to_string())
            .unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        let project_id = add_project_internal(&store, repo.to_string_lossy().to_string())
            .unwrap()
            .id;
        let repo = dunce::canonicalize(&repo).unwrap();
        Fixture { _guard: guard, _tmp: tmp, store, project_id, repo }
    }

    fn write_skill(dir: &Path, name: &str, body: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: test\n---\n{body}\n"),
        )
        .unwrap();
    }

    /// A library skill in the (test) central skills directory.
    fn library_skill(fx: &Fixture, id: &str, dir: &str, body: &str) -> SkillRecord {
        let central = central_repo::skills_dir().join(dir);
        write_skill(&central, dir, body);
        let hash = content_hash::hash_entries(&content_hash::list_content_files(&central));
        let record = SkillRecord {
            id: id.to_string(),
            name: dir.to_string(),
            description: None,
            source_type: "local".to_string(),
            source_ref: None,
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: central.to_string_lossy().to_string(),
            content_hash: Some(hash),
            enabled: true,
            created_at: 0,
            updated_at: 0,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: None,
            last_check_error: None,
        };
        fx.store.insert_skill(&record).unwrap();
        record
    }

    fn ids(skills: &[&SkillRecord]) -> Vec<String> {
        skills.iter().map(|s| s.id.clone()).collect()
    }

    fn conflict_paths(err: &AppError) -> Vec<String> {
        assert_eq!(err.kind, crate::core::error::ErrorKind::TargetConflict, "{err}");
        err.details
            .as_ref()
            .unwrap()
            .conflicts
            .iter()
            .map(|c| c.path.clone())
            .collect()
    }

    #[test]
    fn deploy_plan_creates_then_recognizes_the_same_skill_without_writing_on_plan() {
        let fx = fixture(&["claude_code"]);
        let demo = library_skill(&fx, "s-demo", "demo", "# Demo");
        let target = fx.repo.join(".claude/skills/demo");

        let plan = plan_project_deploy(&fx.store, &fx.project_id, &ids(&[&demo]), &["claude_code".into()]).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].state, ProjectDeployState::Create);
        assert!(fs::symlink_metadata(&target).is_err(), "planning must not write");

        apply_project_deploy(&fx.store, &plan).unwrap();
        assert!(target.join("SKILL.md").is_file());

        let again = plan_project_deploy(&fx.store, &fx.project_id, &ids(&[&demo]), &["claude_code".into()]).unwrap();
        assert_eq!(again[0].state, ProjectDeployState::AlreadyDeployed);
        assert_eq!(again[0].sync_status.as_deref(), Some("in_sync"));

        // The GUI path keeps refusing anything already there, as before.
        let gui = export_skill_to_project_internal(&fx.store, demo.id.clone(), fx.project_id.clone(), None);
        assert!(gui.unwrap_err().to_string().contains("already exists"));
    }

    /// The directory name is taken, but by another library skill's content:
    /// deploying must not report it as done, and must not touch it.
    #[test]
    fn deploy_refuses_a_same_named_directory_that_belongs_to_another_skill() {
        let fx = fixture(&["claude_code"]);
        let demo = library_skill(&fx, "s-demo", "demo", "# Demo");
        let other = library_skill(&fx, "s-other", "other", "# Other");
        let squatter = fx.repo.join(".claude/skills/demo");
        fs::create_dir_all(squatter.parent().unwrap()).unwrap();
        // Byte-identical to `other`, so its unique hash names `other`.
        fs::create_dir_all(&squatter).unwrap();
        fs::copy(Path::new(&other.central_path).join("SKILL.md"), squatter.join("SKILL.md")).unwrap();
        let before = fs::read_to_string(squatter.join("SKILL.md")).unwrap();

        let plan = plan_project_deploy(&fx.store, &fx.project_id, &ids(&[&demo]), &["claude_code".into()]).unwrap();
        assert_eq!(plan[0].state, ProjectDeployState::Conflict);
        let err = project_deploy_conflicts(&plan).expect("a conflict");
        assert_eq!(conflict_paths(&err), vec![squatter.to_string_lossy().to_string()]);

        // The GUI path refuses as well, and the directory is untouched.
        let gui = export_skill_to_project_internal(&fx.store, demo.id.clone(), fx.project_id.clone(), Some(vec!["claude_code".into()]));
        assert!(gui.unwrap_err().to_string().contains("already exists"));
        assert_eq!(fs::read_to_string(squatter.join("SKILL.md")).unwrap(), before);
    }

    /// Review finding 5: two library skills whose directories share a name
    /// (`a/demo`, `b/demo`) would both land in `<project>/.claude/skills/demo`.
    /// The batch is refused before anything is written.
    #[test]
    fn deploy_refuses_two_skills_that_land_on_the_same_directory() {
        let fx = fixture(&["claude_code"]);
        let mut skills = Vec::new();
        for (id, group, body) in [("s-a", "a", "# A's demo"), ("s-b", "b", "# B's demo")] {
            let central = central_repo::skills_dir().join(group).join("demo");
            write_skill(&central, "demo", body);
            let mut record = library_skill(&fx, &format!("{id}-tmp"), &format!("{group}-tmp"), body);
            fs::remove_dir_all(&record.central_path).unwrap();
            fx.store.delete_skill(&record.id).unwrap();
            record.id = id.to_string();
            record.name = format!("{group}-demo");
            record.central_path = central.to_string_lossy().to_string();
            fx.store.insert_skill(&record).unwrap();
            skills.push(record);
        }

        let err = plan_project_deploy(&fx.store, &fx.project_id, &ids(&[&skills[0], &skills[1]]), &["claude_code".into()])
            .expect_err("two skills on one directory");
        let target = fx.repo.join(".claude/skills/demo");
        assert_eq!(conflict_paths(&err), vec![target.to_string_lossy().to_string()]);
        let reason = &err.details.as_ref().unwrap().conflicts[0].reason;
        assert!(reason.contains("a-demo") && reason.contains("b-demo"), "{reason}");
        assert!(
            reason.contains(&skills[0].central_path) && reason.contains(&skills[1].central_path),
            "names can coincide, so the library directories are named: {reason}"
        );
        assert!(fs::symlink_metadata(&target).is_err());
    }

    #[test]
    fn deploy_refuses_a_directory_that_is_not_a_skill() {
        let fx = fixture(&["claude_code"]);
        let demo = library_skill(&fx, "s-demo", "demo", "# Demo");
        let squatter = fx.repo.join(".claude/skills/demo");
        fs::create_dir_all(&squatter).unwrap();
        fs::write(squatter.join("notes.txt"), "mine").unwrap();

        let plan = plan_project_deploy(&fx.store, &fx.project_id, &ids(&[&demo]), &["claude_code".into()]).unwrap();
        assert_eq!(plan[0].state, ProjectDeployState::Conflict);
        assert!(project_deploy_conflicts(&plan).is_some());
    }

    /// Warp reads `.agents/skills`, whose row is filed under another agent.
    /// Naming Warp must reach that directory, from the CLI and the GUI path.
    #[test]
    fn any_agent_reading_a_shared_directory_can_deploy_into_it() {
        let fx = fixture(&["warp"]);
        let demo = library_skill(&fx, "s-demo", "demo", "# Demo");
        let rep = project_agent_group(&fx.store, &get_project_record(&fx.store, &fx.project_id).unwrap(), "warp").unwrap();
        assert_ne!(rep.key, "warp", "the row must be filed under another agent for this test to mean anything");
        assert!(rep.shared_by.contains(&"warp".to_string()));

        let plan = plan_project_deploy(&fx.store, &fx.project_id, &ids(&[&demo]), &["warp".into()]).unwrap();
        assert_eq!(plan[0].agent, rep.key);
        assert_eq!(PathBuf::from(&plan[0].path), fx.repo.join(".agents/skills/demo"));

        export_skill_to_project_internal(&fx.store, demo.id.clone(), fx.project_id.clone(), Some(vec!["warp".into()])).unwrap();
        assert!(fx.repo.join(".agents/skills/demo/SKILL.md").is_file());
    }

    #[test]
    fn a_missing_project_folder_is_refused_and_not_recreated() {
        let fx = fixture(&["claude_code"]);
        let demo = library_skill(&fx, "s-demo", "demo", "# Demo");
        fs::remove_dir_all(&fx.repo).unwrap();

        let err = plan_project_deploy(&fx.store, &fx.project_id, &ids(&[&demo]), &["claude_code".into()]).unwrap_err();
        assert!(err.to_string().contains("Project path does not exist"), "{err}");
        let gui = export_skill_to_project_internal(&fx.store, demo.id.clone(), fx.project_id.clone(), None);
        assert!(gui.unwrap_err().to_string().contains("Project path does not exist"));
        assert!(!fx.repo.exists(), "the project folder must not be recreated");
        assert!(plan_project_undeploy(&fx.store, &fx.project_id, &ids(&[&demo]), &[]).is_err());
        assert!(plan_project_import(&fx.store, &fx.project_id, "demo", "claude_code").is_err());
    }

    /// A copied deployment the user edited holds content the library lacks.
    #[test]
    fn undeploy_refuses_a_copy_that_is_not_in_sync_unless_forced() {
        let fx = fixture(&["claude_code"]);
        fx.store.set_setting("sync_mode", "copy").unwrap();
        let demo = library_skill(&fx, "s-demo", "demo", "# Demo");
        let plan = plan_project_deploy(&fx.store, &fx.project_id, &ids(&[&demo]), &["claude_code".into()]).unwrap();
        apply_project_deploy(&fx.store, &plan).unwrap();
        let copy = fx.repo.join(".claude/skills/demo");
        fs::write(copy.join("SKILL.md"), "---\nname: demo\n---\n# Edited in the project\n").unwrap();

        let removal = plan_project_undeploy(&fx.store, &fx.project_id, &ids(&[&demo]), &[]).unwrap();
        assert_eq!(removal.len(), 1);
        assert!(!removal[0].safe, "status {}", removal[0].sync_status);
        assert_eq!(conflict_paths(&project_removal_conflicts(&removal).unwrap()), vec![copy.to_string_lossy().to_string()]);

        let err = delete_project_skill_internal(&fx.store, fx.project_id.clone(), "demo".into(), "claude_code".into(), false).unwrap_err();
        assert_eq!(conflict_paths(&err).len(), 1);
        assert!(copy.join("SKILL.md").is_file(), "a refused removal deletes nothing");

        delete_project_skill_internal(&fx.store, fx.project_id.clone(), "demo".into(), "claude_code".into(), true).unwrap();
        assert!(!copy.exists());
    }

    #[test]
    fn undeploy_removes_an_in_sync_copy_and_only_this_skills_copies() {
        let fx = fixture(&["claude_code"]);
        let demo = library_skill(&fx, "s-demo", "demo", "# Demo");
        let other = library_skill(&fx, "s-other", "other", "# Other");
        let plan = plan_project_deploy(&fx.store, &fx.project_id, &ids(&[&demo, &other]), &["claude_code".into()]).unwrap();
        apply_project_deploy(&fx.store, &plan).unwrap();
        // A skill written in the project, matching nothing in the library.
        write_skill(&fx.repo.join(".claude/skills/mine"), "mine", "# Mine");

        let removal = plan_project_undeploy(&fx.store, &fx.project_id, &ids(&[&demo]), &[]).unwrap();
        assert_eq!(removal.len(), 1);
        assert_eq!(removal[0].relative_path, "demo");
        assert!(removal[0].safe);
        assert!(fx.repo.join(".claude/skills/demo").exists(), "planning must not delete");

        delete_project_skill_internal(&fx.store, fx.project_id.clone(), "demo".into(), "claude_code".into(), false).unwrap();
        assert!(!fx.repo.join(".claude/skills/demo").exists());
        assert!(fx.repo.join(".claude/skills/other").exists());
        assert!(fx.repo.join(".claude/skills/mine").exists());
    }

    /// Without force, a project-only skill (nothing in the library holds its
    /// content) cannot be deleted through the shared path.
    #[test]
    fn delete_without_force_refuses_a_project_only_skill() {
        let fx = fixture(&["claude_code"]);
        write_skill(&fx.repo.join(".claude/skills/mine"), "mine", "# Mine");
        let err = delete_project_skill_internal(&fx.store, fx.project_id.clone(), "mine".into(), "claude_code".into(), false).unwrap_err();
        assert_eq!(conflict_paths(&err).len(), 1);
        assert!(fx.repo.join(".claude/skills/mine/SKILL.md").is_file());
    }

    /// A directory the scanner does not recognize as a skill (no SKILL.md)
    /// cannot be judged, so without force it is not deleted either.
    #[test]
    fn delete_without_force_refuses_a_directory_that_is_not_a_skill() {
        let fx = fixture(&["claude_code"]);
        let junk = fx.repo.join(".claude/skills/junk");
        fs::create_dir_all(&junk).unwrap();
        fs::write(junk.join("notes.txt"), "mine").unwrap();
        let err = delete_project_skill_internal(&fx.store, fx.project_id.clone(), "junk".into(), "claude_code".into(), false).unwrap_err();
        assert_eq!(conflict_paths(&err).len(), 1);
        assert!(junk.join("notes.txt").is_file());
    }

    /// The enabled slot holds the user's unrelated files; the disabled slot
    /// holds an in-sync copy. Whatever passes the check must be what is
    /// removed: the unrelated files are never touched without force.
    #[test]
    fn a_removal_deletes_exactly_the_copy_it_checked() {
        let fx = fixture(&["claude_code"]);
        let demo = library_skill(&fx, "s-demo", "demo", "# Demo");
        let disabled = fx.repo.join(".claude/skills-disabled/demo");
        fs::create_dir_all(&disabled).unwrap();
        fs::copy(Path::new(&demo.central_path).join("SKILL.md"), disabled.join("SKILL.md")).unwrap();
        let junk = fx.repo.join(".claude/skills/demo");
        fs::create_dir_all(&junk).unwrap();
        fs::write(junk.join("notes.txt"), "mine").unwrap();

        // By relative path: the slot that would be deleted is the junk one,
        // which is not a recognizable skill, so it is refused.
        let err = delete_project_skill_internal(&fx.store, fx.project_id.clone(), "demo".into(), "claude_code".into(), false).unwrap_err();
        assert_eq!(conflict_paths(&err), vec![junk.to_string_lossy().to_string()]);
        assert!(junk.join("notes.txt").is_file());
    }

    /// Same layout, removing through the plan: the planned copy is the
    /// disabled one, and only it goes.
    #[test]
    fn a_planned_removal_deletes_exactly_the_planned_copy() {
        let fx = fixture(&["claude_code"]);
        let demo = library_skill(&fx, "s-demo", "demo", "# Demo");
        let disabled = fx.repo.join(".claude/skills-disabled/demo");
        fs::create_dir_all(&disabled).unwrap();
        fs::copy(Path::new(&demo.central_path).join("SKILL.md"), disabled.join("SKILL.md")).unwrap();
        let junk = fx.repo.join(".claude/skills/demo");
        fs::create_dir_all(&junk).unwrap();
        fs::write(junk.join("notes.txt"), "mine").unwrap();

        let plan = plan_project_undeploy(&fx.store, &fx.project_id, &ids(&[&demo]), &[]).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(PathBuf::from(&plan[0].path), disabled);
        remove_planned_project_copy(&fx.store, &fx.project_id, &plan[0], false).unwrap();
        assert!(!disabled.exists());
        assert!(junk.join("notes.txt").is_file());
    }

    /// A link the user made to somewhere else is their configuration, even
    /// when its target's content happens to equal the library's. Only a link
    /// that resolves to the matched library skill's own directory is ours.
    #[cfg(unix)]
    #[test]
    fn undeploy_refuses_a_symlink_that_does_not_point_at_the_library_copy() {
        let fx = fixture(&["claude_code"]);
        let demo = library_skill(&fx, "s-demo", "demo", "# Demo");
        let skills = fx.repo.join(".claude/skills");
        let link = skills.join("demo");

        for (label, body) in [("different", "# Someone else's demo"), ("identical", "# Demo")] {
            let elsewhere = fx._tmp.path().join(format!("elsewhere-{label}/demo"));
            write_skill(&elsewhere, "demo", body);
            if label == "identical" {
                fs::copy(Path::new(&demo.central_path).join("SKILL.md"), elsewhere.join("SKILL.md")).unwrap();
            }
            let _ = fs::remove_file(&link);
            std::os::unix::fs::symlink(&elsewhere, &link).unwrap();

            let plan = plan_project_undeploy(&fx.store, &fx.project_id, &ids(&[&demo]), &[]).unwrap();
            assert_eq!(plan.len(), 1, "{label}");
            assert!(!plan[0].safe, "{label}: status {}", plan[0].sync_status);
            assert!(delete_project_skill_internal(&fx.store, fx.project_id.clone(), "demo".into(), "claude_code".into(), false).is_err(), "{label}");
            assert!(fs::symlink_metadata(&link).is_ok(), "{label}: the link stays");
        }

        // Our own deployment (a link to the library copy) is still removable.
        fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&demo.central_path, &link).unwrap();
        let plan = plan_project_undeploy(&fx.store, &fx.project_id, &ids(&[&demo]), &[]).unwrap();
        assert!(plan[0].safe);
        delete_project_skill_internal(&fx.store, fx.project_id.clone(), "demo".into(), "claude_code".into(), false).unwrap();
        assert!(fs::symlink_metadata(&link).is_err());
        assert!(Path::new(&demo.central_path).join("SKILL.md").is_file(), "the library copy survives");
    }

    /// Equal hashes do not prove a copy holds nothing of its own: the hash
    /// skips `.gitignore`, `.git` and links (#593). Anything in the copy that
    /// the library copy lacks makes the removal unsafe.
    #[cfg(unix)]
    #[test]
    fn undeploy_refuses_an_in_sync_copy_holding_entries_the_library_lacks() {
        let fx = fixture(&["claude_code"]);
        fx.store.set_setting("sync_mode", "copy").unwrap();
        let demo = library_skill(&fx, "s-demo", "demo", "# Demo");
        let copy = fx.repo.join(".claude/skills/demo");
        let extras: [(&str, fn(&Path)); 3] = [
            (".gitignore", |copy| fs::write(copy.join(".gitignore"), "secrets/\n").unwrap()),
            (".git", |copy| {
                fs::create_dir_all(copy.join(".git")).unwrap();
                fs::write(copy.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
            }),
            ("link", |copy| std::os::unix::fs::symlink("/tmp", copy.join("link")).unwrap()),
        ];

        for (label, add_extra) in extras {
            let _ = fs::remove_dir_all(&copy);
            let plan = plan_project_deploy(&fx.store, &fx.project_id, &ids(&[&demo]), &["claude_code".into()]).unwrap();
            apply_project_deploy(&fx.store, &plan).unwrap();
            add_extra(&copy);

            let removal = plan_project_undeploy(&fx.store, &fx.project_id, &ids(&[&demo]), &[]).unwrap();
            assert_eq!(removal[0].sync_status, "in_sync", "{label}: the hash cannot see it");
            assert!(!removal[0].safe, "{label}: must not be safe");
            assert!(delete_project_skill_internal(&fx.store, fx.project_id.clone(), "demo".into(), "claude_code".into(), false).is_err(), "{label}");
            assert!(copy.exists(), "{label}: kept");
        }

        // A plain copy with nothing extra is still removable.
        fs::remove_dir_all(&copy).unwrap();
        let plan = plan_project_deploy(&fx.store, &fx.project_id, &ids(&[&demo]), &["claude_code".into()]).unwrap();
        apply_project_deploy(&fx.store, &plan).unwrap();
        assert!(plan_project_undeploy(&fx.store, &fx.project_id, &ids(&[&demo]), &[]).unwrap()[0].safe);
    }

    /// Review finding 4: whether an import may overwrite is decided under the
    /// repo lock. Here another writer installs a differing "mine" while the
    /// import waits for the lock; without --force it must be refused, not
    /// overwritten on the strength of a plan made before the lock.
    #[test]
    fn import_decides_whether_it_overwrites_under_the_lock() {
        let fx = fixture(&["claude_code"]);
        write_skill(&fx.repo.join(".claude/skills/mine"), "mine", "# Project version");
        let held = RepoLock::acquire_foreground("test holds the lock").unwrap();

        let result = std::thread::scope(|scope| {
            let import = scope.spawn(|| {
                import_project_skill_to_center_internal(&fx.store, fx.project_id.clone(), "mine".into(), "claude_code".into(), false)
            });
            // Long enough for the import to plan and block on the lock.
            std::thread::sleep(std::time::Duration::from_millis(500));
            library_skill(&fx, "s-mine", "mine", "# Someone else's version");
            drop(held);
            import.join().unwrap()
        });

        let err = result.expect_err("an overwrite without --force must be refused");
        assert_eq!(conflict_paths(&err).len(), 1);
        let library = central_repo::skills_dir().join("mine/SKILL.md");
        assert!(fs::read_to_string(library).unwrap().contains("Someone else's version"));
    }

    /// Review finding 7: a dry run must fail where the run certainly will.
    /// A library skill whose own directory is gone cannot be deployed.
    #[test]
    fn deploy_plan_refuses_a_library_skill_whose_directory_is_missing() {
        let fx = fixture(&["claude_code"]);
        let demo = library_skill(&fx, "s-demo", "demo", "# Demo");
        fs::remove_dir_all(&demo.central_path).unwrap();

        let err = plan_project_deploy(&fx.store, &fx.project_id, &ids(&[&demo]), &["claude_code".into()])
            .expect_err("the run would fail, so the plan must");
        assert!(err.to_string().contains(&demo.central_path), "{err}");
        assert!(fs::symlink_metadata(fx.repo.join(".claude/skills/demo")).is_err());
    }

    /// A skill name the installer rejects (`...` is empty once trailing dots
    /// go) fails the import plan as it fails the import.
    #[test]
    fn import_plan_refuses_a_name_the_installer_rejects() {
        let fx = fixture(&["claude_code"]);
        write_skill(&fx.repo.join(".claude/skills/dots"), "...", "# Dots");

        let real = import_project_skill_to_center_internal(&fx.store, fx.project_id.clone(), "dots".into(), "claude_code".into(), false);
        assert!(real.is_err(), "the installer rejects the name");
        let err = plan_project_import(&fx.store, &fx.project_id, "dots", "claude_code")
            .expect_err("the plan must refuse what the run refuses");
        assert!(err.to_string().contains("Invalid skill name"), "{err}");
        assert!(fx.store.get_all_skills().unwrap().is_empty());
    }

    #[test]
    fn import_creates_a_library_skill_with_metadata_and_audit() {
        let fx = fixture(&["claude_code"]);
        write_skill(&fx.repo.join(".claude/skills/mine"), "mine", "# Mine");
        assert!(!sync_metadata::has_complete_skill_snapshot());

        let plan = plan_project_import(&fx.store, &fx.project_id, "mine", "claude_code").unwrap();
        assert_eq!(plan.action, ProjectImportAction::Create);
        assert!(fx.store.get_all_skills().unwrap().is_empty(), "planning must not write the library");

        let done = import_project_skill_to_center_internal(&fx.store, fx.project_id.clone(), "mine".into(), "claude_code".into(), false).unwrap();
        assert_eq!(done.action, ProjectImportAction::Create);
        let skills = fx.store.get_all_skills().unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(Some(skills[0].id.clone()), done.skill_id);
        assert!(Path::new(&skills[0].central_path).join("SKILL.md").is_file());
        assert!(fx.store.list_audit(None).unwrap().iter().any(|e| e.action == "import"));
        assert!(sync_metadata::has_complete_skill_snapshot(), "sync metadata must be written");

        let again = plan_project_import(&fx.store, &fx.project_id, "mine", "claude_code").unwrap();
        assert_eq!(again.action, ProjectImportAction::Unchanged);
    }

    #[test]
    fn import_needs_force_to_overwrite_a_differing_library_copy() {
        let fx = fixture(&["claude_code"]);
        fx.store.set_setting("sync_mode", "copy").unwrap();
        let demo = library_skill(&fx, "s-demo", "demo", "# Demo");
        let plan = plan_project_deploy(&fx.store, &fx.project_id, &ids(&[&demo]), &["claude_code".into()]).unwrap();
        apply_project_deploy(&fx.store, &plan).unwrap();
        fs::write(fx.repo.join(".claude/skills/demo/SKILL.md"), "---\nname: demo\n---\n# Better\n").unwrap();
        let library_file = Path::new(&demo.central_path).join("SKILL.md");
        let before = fs::read_to_string(&library_file).unwrap();

        assert_eq!(plan_project_import(&fx.store, &fx.project_id, "demo", "claude_code").unwrap().action, ProjectImportAction::Overwrite);
        let err = import_project_skill_to_center_internal(&fx.store, fx.project_id.clone(), "demo".into(), "claude_code".into(), false).unwrap_err();
        assert_eq!(conflict_paths(&err), vec![demo.central_path.clone()]);
        assert_eq!(fs::read_to_string(&library_file).unwrap(), before, "refused import must not write");

        let done = import_project_skill_to_center_internal(&fx.store, fx.project_id.clone(), "demo".into(), "claude_code".into(), true).unwrap();
        assert_eq!(done.action, ProjectImportAction::Overwrite);
        assert!(fs::read_to_string(&library_file).unwrap().contains("# Better"));
    }

    /// Two agents' copies both differ from the library: pushing either would
    /// make the other look stale and invite overwriting it.
    #[test]
    fn import_refuses_when_several_copies_differ_even_with_force() {
        let fx = fixture(&["claude_code", "cursor"]);
        write_skill(&fx.repo.join(".claude/skills/mine"), "mine", "# Claude edit");
        write_skill(&fx.repo.join(".cursor/skills/mine"), "mine", "# Cursor edit");

        for force in [false, true] {
            let err = import_project_skill_to_center_internal(&fx.store, fx.project_id.clone(), "mine".into(), "claude_code".into(), force).unwrap_err();
            assert_eq!(conflict_paths(&err).len(), 2, "force={force}");
        }
        assert!(plan_project_import(&fx.store, &fx.project_id, "mine", "cursor").is_err());
        assert!(fx.store.get_all_skills().unwrap().is_empty());
    }
}
