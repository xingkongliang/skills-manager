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

/// Resolve the enabled and disabled skills root directories for a given agent in a project.
fn resolve_agent_skills_roots(
    store: &SkillStore,
    project_path: &Path,
    agent: &str,
) -> Option<(PathBuf, PathBuf)> {
    let adapter = tool_adapters::all_tool_adapters(store)
        .into_iter()
        .find(|adapter| adapter.key == agent)?;
    let skills_root = project_path.join(&adapter.relative_skills_dir);
    let disabled_root = project_path.join(format!("{}-disabled", &adapter.relative_skills_dir));
    Some((skills_root, disabled_root))
}

fn project_agent_targets(store: &SkillStore) -> Vec<ProjectAgentTargetDto> {
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
        .map(|config| ProjectAgentTargetDto {
            enabled: !disabled_tools.contains(&config.key),
            installed: tool_adapters::find_adapter_with_store(store, &config.key)
                .map(|adapter| adapter.is_installed())
                .unwrap_or(false),
            is_custom: tool_adapters::find_adapter_with_store(store, &config.key)
                .map(|adapter| adapter.is_custom)
                .unwrap_or(false),
            key: config.key,
            display_name: config.display_name,
        })
        .collect()
}

fn project_to_dto(
    rec: &ProjectRecord,
    all_managed: &[SkillRecord],
    configs: &[project_scanner::AgentSkillConfig],
) -> ProjectDto {
    let skills = project_scanner::read_project_skills(Path::new(&rec.path), configs);
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
        sort_order: rec.sort_order,
        skill_count,
        sync_health: health,
        created_at: rec.created_at,
        updated_at: rec.updated_at,
    }
}

fn ensure_safe_skill_dir_name(skill_dir_name: &str) -> Result<(), AppError> {
    if skill_dir_name.trim().is_empty() {
        return Err(AppError::invalid_input("Invalid skill directory name"));
    }
    let mut components = Path::new(skill_dir_name).components();
    let only = components
        .next()
        .ok_or_else(|| AppError::invalid_input("Invalid skill directory name"))?;
    if components.next().is_some() {
        return Err(AppError::invalid_input("Invalid skill directory name"));
    }
    if !matches!(only, Component::Normal(_)) {
        return Err(AppError::invalid_input("Invalid skill directory name"));
    }
    Ok(())
}

fn ensure_dir_within_root(path: &Path, root: &Path) -> Result<(), AppError> {
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

fn find_source_ref_match<'a>(
    skill: &project_scanner::ProjectSkillInfo,
    all_managed: &'a [SkillRecord],
) -> Option<&'a SkillRecord> {
    find_best_center_match(skill, all_managed)
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
) -> Result<Vec<ProjectAgentTargetDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || Ok(project_agent_targets(&store))).await?
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
            .ok_or_else(|| AppError::not_found("Project not found"))?;

        let configs = agent_skill_configs(&store);
        let mut skills = project_scanner::read_project_skills(Path::new(&record.path), &configs);

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
    project_path: String,
    skill_dir_name: String,
    agent: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ProjectSkillDocumentDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_safe_skill_dir_name(&skill_dir_name)?;

        let (skills_root, disabled_root) =
            resolve_agent_skills_roots(&store, Path::new(&project_path), &agent)
                .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent)))?;
        let skill_dir = skills_root.join(&skill_dir_name);
        let skill_dir = if skill_dir.is_dir() {
            ensure_dir_within_root(&skill_dir, &skills_root)?;
            skill_dir
        } else {
            let disabled = disabled_root.join(&skill_dir_name);
            if disabled.is_dir() {
                ensure_dir_within_root(&disabled, &disabled_root)?;
                disabled
            } else {
                return Err(AppError::not_found("Skill directory not found"));
            }
        };

        let candidates = ["SKILL.md", "skill.md", "CLAUDE.md", "README.md"];
        for candidate in &candidates {
            let file_path = skill_dir.join(candidate);
            if file_path.is_file() {
                let content = std::fs::read_to_string(&file_path)?;
                return Ok(ProjectSkillDocumentDto {
                    skill_name: skill_dir_name,
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
    skill_dir_name: String,
    agent: String,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_safe_skill_dir_name(&skill_dir_name)?;

        let record = store
            .get_project_by_id(&project_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Project not found"))?;

        let configs = agent_skill_configs(&store);
        let skills = project_scanner::read_project_skills(Path::new(&record.path), &configs);
        let skill = skills
            .iter()
            .find(|s| s.dir_name == skill_dir_name && s.agent == agent)
            .ok_or_else(|| AppError::not_found("Skill not found in project"))?;

        let source_path = PathBuf::from(&skill.path);
        let all_managed = store.get_all_skills().unwrap_or_default();
        if let Some(existing) = find_source_ref_match(skill, &all_managed) {
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
    skill_dir_name: String,
    agent: String,
) -> Result<(), AppError> {
    import_project_skill_to_center(store, project_id, skill_dir_name, agent).await
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
            .ok_or_else(|| AppError::not_found("Project not found"))?;

        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;

        let dir_name = slugify_skill_dir_name(&skill.name);
        ensure_safe_skill_dir_name(&dir_name)?;

        let source = PathBuf::from(&skill.central_path);
        let agent_keys = agents
            .filter(|items| !items.is_empty())
            .unwrap_or_else(|| vec!["claude_code".to_string()]);

        for agent_key in &agent_keys {
            let (skills_root, disabled_root) =
                resolve_agent_skills_roots(&store, Path::new(&project.path), agent_key)
                    .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent_key)))?;
            let target_dir = skills_root.join(&dir_name);

            if target_dir.strip_prefix(&skills_root).is_err() {
                return Err(AppError::invalid_input("Invalid skill directory path"));
            }

            if target_dir.exists() || disabled_root.join(&dir_name).exists() {
                return Err(AppError::invalid_input(format!(
                    "Skill \"{}\" already exists in this project for agent {}",
                    skill.name, agent_key
                )));
            }
        }

        for agent_key in &agent_keys {
            let (skills_root, _) = resolve_agent_skills_roots(&store, Path::new(&project.path), agent_key)
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
    skill_dir_name: String,
    agent: String,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_safe_skill_dir_name(&skill_dir_name)?;

        let record = store
            .get_project_by_id(&project_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Project not found"))?;

        let configs = agent_skill_configs(&store);
        let skills = project_scanner::read_project_skills(Path::new(&record.path), &configs);
        let skill = skills
            .iter()
            .find(|s| s.dir_name == skill_dir_name && s.agent == agent)
            .ok_or_else(|| AppError::not_found("Skill not found in project"))?;

        let all_managed = store.get_all_skills().unwrap_or_default();
        let managed = find_best_center_match(skill, &all_managed)
            .ok_or_else(|| AppError::not_found("No matching skill in center"))?;

        let (skills_root, disabled_root) =
            resolve_agent_skills_roots(&store, Path::new(&record.path), &agent)
                .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent)))?;
        let target_path = PathBuf::from(&skill.path);
        if target_path.starts_with(&skills_root) {
            ensure_dir_within_root(&target_path, &skills_root)?;
        } else if target_path.starts_with(&disabled_root) {
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
    skill_dir_name: String,
    agent: String,
    enabled: bool,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_safe_skill_dir_name(&skill_dir_name)?;

        let record = store
            .get_project_by_id(&project_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Project not found"))?;

        let (skills_dir, disabled_dir) =
            resolve_agent_skills_roots(&store, Path::new(&record.path), &agent)
                .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent)))?;

        if enabled {
            let from = disabled_dir.join(&skill_dir_name);
            let to = skills_dir.join(&skill_dir_name);

            if !from.is_dir() {
                return Err(AppError::not_found(
                    "Skill directory not found in skills-disabled",
                ));
            }
            ensure_dir_within_root(&from, &disabled_dir)?;
            if to.exists() {
                return Err(AppError::invalid_input(
                    "Skill already exists in skills directory",
                ));
            }
            std::fs::rename(&from, &to)?;
        } else {
            let from = skills_dir.join(&skill_dir_name);
            let to = disabled_dir.join(&skill_dir_name);

            if !from.is_dir() {
                return Err(AppError::not_found("Skill directory not found"));
            }
            ensure_dir_within_root(&from, &skills_dir)?;
            std::fs::create_dir_all(&disabled_dir)?;
            if to.exists() {
                return Err(AppError::invalid_input(
                    "Skill already exists in skills-disabled directory",
                ));
            }
            std::fs::rename(&from, &to)?;
        }

        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn delete_project_skill(
    store: State<'_, Arc<SkillStore>>,
    project_id: String,
    skill_dir_name: String,
    agent: String,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_safe_skill_dir_name(&skill_dir_name)?;

        let record = store
            .get_project_by_id(&project_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Project not found"))?;

        let (skills_root, disabled_root) =
            resolve_agent_skills_roots(&store, Path::new(&record.path), &agent)
                .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent)))?;
        let skills_dir = skills_root.join(&skill_dir_name);
        let disabled_dir = disabled_root.join(&skill_dir_name);

        let (target, target_root) = if skills_dir.is_dir() {
            (skills_dir, skills_root)
        } else if disabled_dir.is_dir() {
            (disabled_dir, disabled_root)
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
    use super::classify_sync_status;
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
}
