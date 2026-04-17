use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::core::skill_store::{ProjectRecord, SkillRecord, SkillStore};
use crate::core::{error::AppError, installer, project_scanner, sync_engine, tool_adapters};

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
    let mut grouped: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for adapter in tool_adapters::all_tool_adapters(store) {
        if adapter.relative_skills_dir.is_empty() {
            continue;
        }
        if let Some((_, agents)) = grouped
            .iter_mut()
            .find(|(dir, _)| *dir == adapter.relative_skills_dir)
        {
            agents.push((adapter.key, adapter.display_name));
        } else {
            grouped.push((
                adapter.relative_skills_dir,
                vec![(adapter.key, adapter.display_name)],
            ));
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
    let skills_root = Path::new(&rec.path).join(&adapter.relative_skills_dir);
    let disabled_root = Path::new(&rec.path).join(format!("{}-disabled", &adapter.relative_skills_dir));
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

    agent_skill_configs(store)
        .into_iter()
        .map(|config| {
            let adapter = tool_adapters::find_adapter_with_store(store, &config.key);
            ProjectAgentTargetDto {
                enabled: !disabled_tools.contains(&config.key),
                installed: adapter
                    .as_ref()
                    .map(|a| a.is_installed())
                    .unwrap_or(false),
                is_custom: adapter.as_ref().map(|a| a.is_custom).unwrap_or(false),
                key: config.key,
                display_name: config.display_name,
            }
        })
        .collect()
}

fn project_to_dto(
    rec: &ProjectRecord,
    all_managed: &[SkillRecord],
    configs: &[project_scanner::AgentSkillConfig],
) -> ProjectDto {
    let skills = read_workspace_skills(rec, configs);
    let skill_count = skills.len();

    let mut health = SyncHealthDto::default();
    for skill in &skills {
        let matched = find_best_center_match(skill, all_managed);
        let status = classify_sync_status(skill, matched);
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

fn ensure_safe_skill_relative_path(skill_relative_path: &str) -> Result<(), AppError> {
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

fn ensure_dir_within_root(path: &Path, root: &Path) -> Result<(), AppError> {
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

fn remove_project_skill_entry(path: &Path) -> Result<(), AppError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(AppError::io(err)),
    };

    if metadata.file_type().is_symlink() || metadata.is_file() {
        std::fs::remove_file(path)?;
    } else if metadata.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }

    Ok(())
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
                remove_project_skill_entry(&disabled_path)?;
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
        return Ok(());
    }

    if disabled_path.is_dir() {
        ensure_dir_within_root(&disabled_path, disabled_dir)?;
        if enabled_path.exists() {
            ensure_dir_within_root(&enabled_path, skills_dir)?;
            remove_project_skill_entry(&enabled_path)?;
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

fn slugify_skill_dir_name(name: &str) -> String {
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

fn source_ref_matches_skill_path(
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

fn find_best_center_match<'a>(
    skill: &project_scanner::ProjectSkillInfo,
    all_managed: &'a [SkillRecord],
) -> Option<&'a SkillRecord> {
    let skill_hash = skill.content_hash.as_deref();
    let canonical_skill_path = std::fs::canonicalize(&skill.path).ok();

    all_managed
        .iter()
        .filter_map(|managed| {
            if source_ref_matches_skill_path(&skill.path, canonical_skill_path.as_ref(), managed) {
                return Some((managed, 3));
            }
            if skill_hash.is_some() && managed.content_hash.as_deref() == skill_hash {
                return Some((managed, 2));
            }
            let managed_dir_name = slugify_skill_dir_name(&managed.name);
            if managed_dir_name.eq_ignore_ascii_case(&skill.dir_name) {
                return Some((managed, 1));
            }
            None
        })
        .max_by_key(|(_, score)| *score)
        .map(|(managed, _)| managed)
}

fn classify_sync_status(
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

    // DB hash may be stale — recompute center hash from disk as fallback
    if let Some(project_hash) = skill.content_hash.as_deref() {
        if let Ok(live_center_hash) =
            crate::core::content_hash::hash_directory(Path::new(&managed.central_path))
        {
            if project_hash == live_center_hash {
                return "in_sync".to_string();
            }
        }
    }

    let Some(project_modified_at) = skill.last_modified_at else {
        return "diverged".to_string();
    };

    let center_updated_at = managed.updated_at;
    let threshold_ms = 1_000;
    if project_modified_at > center_updated_at + threshold_ms {
        "project_newer".to_string()
    } else if center_updated_at > project_modified_at + threshold_ms {
        "center_newer".to_string()
    } else {
        "diverged".to_string()
    }
}

#[tauri::command]
pub async fn get_projects(store: State<'_, Arc<SkillStore>>) -> Result<Vec<ProjectDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let records = store.get_all_projects().map_err(AppError::db)?;
        let all_managed = store.get_all_skills().map_err(AppError::db)?;
        let configs = agent_skill_configs(&store);
        Ok(records
            .iter()
            .map(|r| project_to_dto(r, &all_managed, &configs))
            .collect())
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
        let project_path = Path::new(&path);
        if !project_path.is_dir() {
            return Err(AppError::invalid_input("Directory does not exist"));
        }
        let claude_dir = project_path.join(".claude");
        let skills_dir = claude_dir.join("skills");
        let disabled_dir = claude_dir.join("skills-disabled");

        // Support initializing an empty project directory as a managed project.
        std::fs::create_dir_all(&skills_dir)?;
        std::fs::create_dir_all(&disabled_dir)?;

        let name = project_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let now = chrono::Utc::now().timestamp_millis();
        let record = ProjectRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            path: path.clone(),
            workspace_type: "project".to_string(),
            linked_agent_key: None,
            linked_agent_name: None,
            disabled_path: None,
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
        for skill in &mut skills {
            let matched = find_best_center_match(skill, &all_managed);
            skill.in_center = matched.is_some();
            skill.center_skill_id = matched.map(|m| m.id.clone());
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

        let (skills_root, disabled_root) =
            resolve_agent_skills_roots(&store, &record, &agent)
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

        let active = store.get_active_scenario_id().ok().flatten();
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

        if let Some(scenario_id) = active.as_deref() {
            store
                .add_skill_to_scenario(scenario_id, &id)
                .map_err(AppError::db)?;
        }

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

        let dir_name = slugify_skill_dir_name(&skill.name);
        ensure_safe_skill_relative_path(&dir_name)?;

        let source = PathBuf::from(&skill.central_path);
        let agent_keys = agents
            .filter(|items| !items.is_empty())
            .unwrap_or_else(|| {
                if project.workspace_type == "linked" {
                    vec![linked_workspace_agent_key(&project)]
                } else {
                    vec!["claude_code".to_string()]
                }
            });

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

        for agent_key in &agent_keys {
            let (skills_root, _) = resolve_agent_skills_roots(&store, &project, agent_key)
                .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent_key)))?;
            let target_dir = skills_root.join(&dir_name);
            std::fs::create_dir_all(&skills_root)?;
            sync_engine::sync_skill(&source, &target_dir, sync_engine::SyncMode::Copy)
                .map_err(AppError::io)?;
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

        let (skills_root, disabled_root) =
            resolve_agent_skills_roots(&store, &record, &agent)
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
        sync_engine::sync_skill(&source, &target_path, sync_engine::SyncMode::Copy)
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

        let (skills_dir, disabled_dir) =
            resolve_agent_skills_roots(&store, &record, &agent)
                .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent)))?;
        let disabled_dir = disabled_dir
            .ok_or_else(|| AppError::invalid_input("This workspace does not support disabling skills"))?;
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
        ensure_safe_skill_relative_path(&skill_relative_path)?;

        let record = store
            .get_project_by_id(&project_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Workspace not found"))?;

        let (skills_root, disabled_root) =
            resolve_agent_skills_roots(&store, &record, &agent)
                .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent)))?;
        let skills_dir = skills_root.join(&skill_relative_path);
        let disabled_dir = disabled_root.as_ref().map(|root| root.join(&skill_relative_path));

        let (target, target_root) = if skills_dir.is_dir() {
            (skills_dir, skills_root)
        } else if let Some(disabled_dir) = disabled_dir.filter(|path| path.is_dir()) {
            (disabled_dir, disabled_root.expect("present when disabled_dir exists"))
        } else {
            return Err(AppError::not_found("Skill directory not found"));
        };

        ensure_dir_within_root(&target, &target_root)?;
        std::fs::remove_dir_all(&target)?;
        Ok(())
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::{
        classify_sync_status, ensure_distinct_linked_workspace_roots,
        set_project_skill_enabled_state,
    };
    use crate::core::content_hash;
    use crate::core::project_scanner::ProjectSkillInfo;
    use crate::core::skill_store::SkillRecord;
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
            in_center: true,
            sync_status: "project_only".to_string(),
            center_skill_id: Some("skill-1".to_string()),
            last_modified_at,
            content_hash,
        }
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

    #[test]
    fn classify_sync_status_falls_back_to_timestamps_when_live_hash_differs() {
        let center_dir = tempdir().unwrap();
        fs::write(center_dir.path().join("SKILL.md"), "# Center\n").unwrap();

        let project_dir = tempdir().unwrap();
        fs::write(project_dir.path().join("SKILL.md"), "# Project changed\n").unwrap();
        let project_hash = content_hash::hash_directory(project_dir.path()).unwrap();

        let managed = sample_managed_skill(
            center_dir.path().to_string_lossy().to_string(),
            Some("stale-db-hash".to_string()),
            1_000,
        );
        let project = sample_project_skill(
            project_dir.path().to_string_lossy().to_string(),
            Some(project_hash),
            Some(5_000),
        );

        assert_eq!(
            classify_sync_status(&project, Some(&managed)),
            "project_newer"
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
    fn set_project_skill_enabled_state_disabling_cleans_duplicate_symlink_without_touching_target() {
        use std::os::unix::fs::symlink;

        let tmp = tempdir().unwrap();
        let central_skill = tmp.path().join("central").join("understand-diff");
        let skills_root = tmp.path().join("skills");
        let disabled_root = tmp.path().join("skills-disabled");
        let relative_path = "understand-diff";

        fs::create_dir_all(&central_skill).unwrap();
        fs::write(central_skill.join("SKILL.md"), "---\nname: understand-diff\n---\n").unwrap();
        fs::create_dir_all(&skills_root).unwrap();
        fs::create_dir_all(&disabled_root).unwrap();

        symlink(&central_skill, skills_root.join(relative_path)).unwrap();
        symlink(&central_skill, disabled_root.join(relative_path)).unwrap();

        set_project_skill_enabled_state(&skills_root, &disabled_root, relative_path, false).unwrap();

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
        fs::write(central_skill.join("SKILL.md"), "---\nname: understand-diff\n---\n").unwrap();
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
}
