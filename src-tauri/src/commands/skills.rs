use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::State;
use walkdir::WalkDir;

use crate::core::{
    central_repo,
    error::AppError,
    git_fetcher,
    install_cancel::InstallCancelRegistry,
    installer,
    skill_metadata::{self, is_valid_skill_dir},
    skill_store::{SkillRecord, SkillStore, SkillTargetRecord},
    sync_engine,
};

#[derive(Debug, Serialize)]
pub struct ManagedSkillDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub source_type: String,
    pub source_ref: Option<String>,
    pub source_revision: Option<String>,
    pub remote_revision: Option<String>,
    pub update_status: String,
    pub last_checked_at: Option<i64>,
    pub last_check_error: Option<String>,
    pub central_path: String,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub status: String,
    pub targets: Vec<TargetDto>,
    pub scenario_ids: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TargetDto {
    pub id: String,
    pub skill_id: String,
    pub tool: String,
    pub target_path: String,
    pub mode: String,
    pub status: String,
    pub synced_at: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct SkillDocumentDto {
    pub skill_id: String,
    pub filename: String,
    pub content: String,
    pub central_path: String,
}

#[derive(Debug, Clone)]
struct InstallSourceMetadata {
    source_type: String,
    source_ref: Option<String>,
    source_ref_resolved: Option<String>,
    source_subpath: Option<String>,
    source_branch: Option<String>,
    source_revision: Option<String>,
    remote_revision: Option<String>,
    update_status: String,
}

#[derive(Debug, Clone)]
struct GitSkillSource {
    clone_url: String,
    branch: Option<String>,
    subpath: Option<String>,
    locator_skill_id: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct GitSkillPreview {
    pub dir_name: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct GitPreviewResult {
    pub temp_dir: String,
    pub skills: Vec<GitSkillPreview>,
}

#[derive(Debug, serde::Deserialize)]
pub struct SkillInstallItem {
    pub dir_name: String,
    pub name: String,
}

struct CancelRegistrationGuard {
    registry: Arc<InstallCancelRegistry>,
    key: String,
}

impl CancelRegistrationGuard {
    fn new(registry: Arc<InstallCancelRegistry>, key: String) -> Self {
        Self { registry, key }
    }
}

impl Drop for CancelRegistrationGuard {
    fn drop(&mut self) {
        self.registry.remove(&self.key);
    }
}

#[tauri::command]
pub async fn get_managed_skills(store: State<'_, Arc<SkillStore>>) -> Result<Vec<ManagedSkillDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skills = store.get_all_skills().map_err(AppError::db)?;
        let all_targets = store.get_all_targets().map_err(AppError::db)?;
        let tags_map = store.get_tags_map().map_err(AppError::db)?;
        Ok(skills
            .into_iter()
            .map(|skill| managed_skill_to_dto(&store, skill, &all_targets, &tags_map))
            .collect())
    })
    .await?
}

#[tauri::command]
pub async fn get_skills_for_scenario(
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<ManagedSkillDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skills = store
            .get_skills_for_scenario(&scenario_id)
            .map_err(AppError::db)?;
        let all_targets = store.get_all_targets().map_err(AppError::db)?;
        let tags_map = store.get_tags_map().map_err(AppError::db)?;

        Ok(skills
            .into_iter()
            .map(|skill| managed_skill_to_dto(&store, skill, &all_targets, &tags_map))
            .collect())
    })
    .await?
}

#[tauri::command]
pub async fn get_skill_document(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<SkillDocumentDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;

        let central = PathBuf::from(&skill.central_path);
        let candidates = [
            "SKILL.md",
            "skill.md",
            "CLAUDE.md",
            "claude.md",
            "README.md",
            "readme.md",
        ];

        for name in &candidates {
            let path = central.join(name);
            if path.exists() {
                let content = std::fs::read_to_string(&path)?;
                return Ok(SkillDocumentDto {
                    skill_id,
                    filename: name.to_string(),
                    content,
                    central_path: skill.central_path,
                });
            }
        }

        for e in WalkDir::new(&central).max_depth(4).into_iter().flatten() {
            let fname = e.file_name().to_string_lossy();
            if candidates.contains(&fname.as_ref()) {
                let content = std::fs::read_to_string(e.path())?;
                return Ok(SkillDocumentDto {
                    skill_id,
                    filename: fname.to_string(),
                    content,
                    central_path: skill.central_path,
                });
            }
        }

        Err(AppError::not_found("No documentation file found"))
    })
    .await?
}

#[tauri::command]
pub async fn delete_managed_skill(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;

        let targets = store
            .get_targets_for_skill(&skill_id)
            .map_err(AppError::db)?;
        for target in &targets {
            let target_path = PathBuf::from(&target.target_path);
            sync_engine::remove_target(&target_path).ok();
        }

        let central = PathBuf::from(&skill.central_path);
        if central.exists() {
            std::fs::remove_dir_all(&central).ok();
        }

        store.delete_skill(&skill_id).map_err(AppError::db)?;

        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn install_local(
    source_path: String,
    name: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = PathBuf::from(&source_path);
        let result = installer::install_from_local(&path, name.as_deref()).map_err(AppError::io)?;

        let active = store.get_active_scenario_id().ok().flatten();
        let metadata = InstallSourceMetadata {
            source_type: "local".to_string(),
            source_ref: Some(source_path),
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            update_status: "local_only".to_string(),
        };
        store_installed_skill(&store, &result, &metadata, active.as_deref())?;

        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn install_git(
    repo_url: String,
    name: Option<String>,
    store: State<'_, Arc<SkillStore>>,
    cancel_registry: State<'_, Arc<InstallCancelRegistry>>,
    app_handle: tauri::AppHandle,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let registry = cancel_registry.inner().clone();
    let cancel_key = repo_url.clone();
    let cancel = registry.register(&cancel_key);
    let _cancel_guard = CancelRegistrationGuard::new(registry.clone(), cancel_key);

    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Emitter;
        let emit_progress = |phase: &str| {
            app_handle.emit("install-progress", serde_json::json!({
                "skill_id": repo_url,
                "phase": phase,
            })).ok();
        };

        emit_progress("cloning");
        let parsed = git_fetcher::parse_git_source(&repo_url);
        let temp_dir =
            git_fetcher::clone_repo_ref(&parsed.clone_url, parsed.branch.as_deref(), Some(&cancel)).map_err(AppError::git_or_cancelled)?;

        emit_progress("installing");
        let install_result = (|| -> Result<(installer::InstallResult, InstallSourceMetadata), AppError> {
            let skill_dir = resolve_skill_dir(&temp_dir, parsed.subpath.as_deref(), None)?;
            let revision = git_fetcher::get_head_revision(&temp_dir).map_err(AppError::git)?;
            let result =
                installer::install_from_git_dir(&skill_dir, name.as_deref()).map_err(AppError::io)?;
            let metadata = InstallSourceMetadata {
                source_type: "git".to_string(),
                source_ref: Some(parsed.original_url.clone()),
                source_ref_resolved: Some(parsed.clone_url.clone()),
                source_subpath: git_fetcher::relative_subpath(&temp_dir, &skill_dir),
                source_branch: parsed.branch.clone(),
                source_revision: Some(revision.clone()),
                remote_revision: Some(revision),
                update_status: "up_to_date".to_string(),
            };
            Ok((result, metadata))
        })();

        git_fetcher::cleanup_temp(&temp_dir);

        let (result, metadata) = install_result?;
        let active = store.get_active_scenario_id().ok().flatten();
        store_installed_skill(&store, &result, &metadata, active.as_deref())?;

        emit_progress("done");
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn install_from_skillssh(
    source: String,
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
    cancel_registry: State<'_, Arc<InstallCancelRegistry>>,
    app_handle: tauri::AppHandle,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let registry = cancel_registry.inner().clone();
    let cancel_key_owned = format!("{}/{}", source, skill_id);
    let cancel = registry.register(&cancel_key_owned);
    let _cancel_guard = CancelRegistrationGuard::new(registry.clone(), cancel_key_owned);

    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Emitter;
        let skill_key = format!("{}/{}", source, skill_id);
        let emit_progress = |phase: &str| {
            app_handle.emit("install-progress", serde_json::json!({
                "skill_id": skill_key,
                "phase": phase,
            })).ok();
        };

        emit_progress("cloning");
        let repo_url = format!("https://github.com/{}.git", source);
        let temp_dir = git_fetcher::clone_repo_ref(&repo_url, None, Some(&cancel)).map_err(AppError::git_or_cancelled)?;

        emit_progress("installing");
        let install_result = (|| -> Result<(installer::InstallResult, InstallSourceMetadata), AppError> {
            let skill_dir = resolve_skill_dir(&temp_dir, None, Some(&skill_id))?;
            let revision = git_fetcher::get_head_revision(&temp_dir).map_err(AppError::git)?;
            let source_ref = format!("{}/{}", source, skill_id);
            let (install_name, destination) =
                resolve_skillssh_install_target(&store, &source_ref, &skill_id)?;
            let result =
                installer::install_skill_dir_to_destination(&skill_dir, &install_name, &destination)
                    .map_err(AppError::io)?;
            let metadata = InstallSourceMetadata {
                source_type: "skillssh".to_string(),
                source_ref: Some(source_ref),
                source_ref_resolved: Some(repo_url.clone()),
                source_subpath: git_fetcher::relative_subpath(&temp_dir, &skill_dir),
                source_branch: None,
                source_revision: Some(revision.clone()),
                remote_revision: Some(revision),
                update_status: "up_to_date".to_string(),
            };
            Ok((result, metadata))
        })();

        git_fetcher::cleanup_temp(&temp_dir);

        let (result, metadata) = install_result?;
        let active = store.get_active_scenario_id().ok().flatten();
        store_installed_skill(&store, &result, &metadata, active.as_deref())?;

        emit_progress("done");
        Ok(())
    })
    .await?
}

/// Clone a git repo and return a preview list of skills found, without installing.
/// The caller must follow up with `confirm_git_install` using the returned `temp_dir`.
#[tauri::command]
pub async fn preview_git_install(
    repo_url: String,
    store: State<'_, Arc<SkillStore>>,
    cancel_registry: State<'_, Arc<InstallCancelRegistry>>,
    app_handle: tauri::AppHandle,
) -> Result<GitPreviewResult, AppError> {
    let store = store.inner().clone();
    let proxy_url = store.get_setting("proxy_url").ok().flatten();
    let registry = cancel_registry.inner().clone();
    let cancel_key = repo_url.clone();
    let cancel = registry.register(&cancel_key);
    let _cancel_guard = CancelRegistrationGuard::new(registry.clone(), cancel_key);

    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Emitter;
        app_handle.emit("install-progress", serde_json::json!({
            "skill_id": repo_url,
            "phase": "cloning",
        })).ok();

        let parsed = git_fetcher::parse_git_source(&repo_url);
        let temp_dir = git_fetcher::clone_repo_ref(
            &parsed.clone_url,
            parsed.branch.as_deref(),
            Some(&cancel),
            proxy_url.as_deref(),
        ).map_err(AppError::git_or_cancelled)?;

        let skill_dir = resolve_skill_dir(&temp_dir, parsed.subpath.as_deref(), None)?;
        let dirs = collect_git_skill_dirs(&skill_dir);

        let skills: Vec<GitSkillPreview> = dirs.iter().map(|dir| {
            let meta = skill_metadata::parse_skill_md(dir);
            let dir_name = dir.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let name = meta.name
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| dir_name.clone());
            GitSkillPreview { dir_name, name, description: meta.description }
        }).collect();

        Ok(GitPreviewResult {
            temp_dir: temp_dir.to_string_lossy().to_string(),
            skills,
        })
    })
    .await?
}

/// Install selected skills from a previously cloned temp directory.
#[tauri::command]
pub async fn confirm_git_install(
    repo_url: String,
    temp_dir: String,
    items: Vec<SkillInstallItem>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let temp_path = PathBuf::from(&temp_dir);

        // Security: temp_path must be inside OS temp dir and carry our prefix.
        let expected_prefix = std::env::temp_dir();
        if !temp_path.starts_with(&expected_prefix) {
            return Err(AppError::invalid_input("Invalid temp directory"));
        }
        let dir_name_str = temp_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if !dir_name_str.starts_with("skills-manager-clone-") {
            return Err(AppError::invalid_input("Invalid temp directory"));
        }
        if !temp_path.exists() {
            return Err(AppError::invalid_input("Clone session expired, please try again"));
        }

        let parsed = git_fetcher::parse_git_source(&repo_url);
        let skill_dir = resolve_skill_dir(&temp_path, parsed.subpath.as_deref(), None)?;
        let all_dirs = collect_git_skill_dirs(&skill_dir);
        let revision = git_fetcher::get_head_revision(&temp_path).map_err(AppError::git)?;
        let active = store.get_active_scenario_id().ok().flatten();

        for dir in &all_dirs {
            let dir_name_entry = dir.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let item = match items.iter().find(|i| i.dir_name == dir_name_entry) {
                Some(i) => i,
                None => continue,
            };
            let custom_name = item.name.trim();
            let install_name = if custom_name.is_empty() { None } else { Some(custom_name) };
            let result = installer::install_from_git_dir(dir, install_name).map_err(AppError::io)?;
            let subpath = git_fetcher::relative_subpath(&temp_path, dir);
            let metadata = InstallSourceMetadata {
                source_type: "git".to_string(),
                source_ref: Some(repo_url.clone()),
                source_ref_resolved: Some(parsed.clone_url.clone()),
                source_subpath: subpath,
                source_branch: parsed.branch.clone(),
                source_revision: Some(revision.clone()),
                remote_revision: Some(revision.clone()),
                update_status: "up_to_date".to_string(),
            };
            store_installed_skill(&store, &result, &metadata, active.as_deref())?;
        }

        git_fetcher::cleanup_temp(&temp_path);
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn check_skill_update(
    skill_id: String,
    force: Option<bool>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ManagedSkillDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        check_skill_update_internal(&store, &skill_id, force.unwrap_or(false))
    })
    .await?
}

#[tauri::command]
pub async fn check_all_skill_updates(
    force: Option<bool>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let force_check = force.unwrap_or(false);
        let ids: Vec<String> = store
            .get_all_skills()
            .map_err(AppError::db)?
            .into_iter()
            .map(|skill| skill.id)
            .collect();
        let mut failed = Vec::new();

        for skill_id in ids {
            if let Err(err) = check_skill_update_internal(&store, &skill_id, force_check) {
                failed.push(format!("{skill_id}: {err}"));
            }
        }

        if failed.is_empty() {
            Ok(())
        } else {
            Err(AppError::internal(format!(
                "Failed to check {} skill(s): {}",
                failed.len(),
                failed.join("; ")
            )))
        }
    })
    .await?
}

#[tauri::command]
pub async fn update_skill(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
    cancel_registry: State<'_, Arc<InstallCancelRegistry>>,
) -> Result<ManagedSkillDto, AppError> {
    let store = store.inner().clone();
    let registry = cancel_registry.inner().clone();
    let cancel_key = format!("update:{}", skill_id);
    let cancel = registry.register(&cancel_key);
    let _cancel_guard = CancelRegistrationGuard::new(registry.clone(), cancel_key);

    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;

        if !matches!(skill.source_type.as_str(), "git" | "skillssh") {
            return Err(AppError::invalid_input("Only git-based skills can be updated"));
        }

        let git_source = git_source_from_skill(&skill)?;
        let remote_revision = git_fetcher::resolve_remote_revision(
            &git_source.clone_url,
            git_source.branch.as_deref(),
        )
        .map_err(|e| {
            let message = e.to_string();
            let _ = store.update_skill_check_state(
                &skill_id,
                skill.remote_revision.as_deref(),
                "error",
                Some(&message),
            );
            AppError::git(message)
        })?;

        store
            .update_skill_update_status(&skill_id, "updating")
            .map_err(AppError::db)?;

        let temp_dir =
            git_fetcher::clone_repo_ref(&git_source.clone_url, git_source.branch.as_deref(), Some(&cancel)).map_err(AppError::git_or_cancelled)?;
        let update_result = (|| -> Result<(), AppError> {
            git_fetcher::checkout_revision(&temp_dir, &remote_revision).map_err(AppError::git)?;
            let skill_dir = resolve_skill_dir(
                &temp_dir,
                git_source.subpath.as_deref(),
                git_source.locator_skill_id.as_deref(),
            )?;
            let staged_path = staged_path_for(&skill.central_path);
            let install_result = installer::install_skill_dir_to_destination(&skill_dir, &skill.name, &staged_path)
                .map_err(AppError::io)?;
            swap_skill_directory(&staged_path, Path::new(&skill.central_path))?;

            let source_subpath = git_fetcher::relative_subpath(&temp_dir, &skill_dir);
            store
                .update_skill_source_metadata(
                    &skill.id,
                    Some(&git_source.clone_url),
                    source_subpath.as_deref(),
                    git_source.branch.as_deref(),
                    Some(&remote_revision),
                )
                .map_err(AppError::db)?;
            store
                .update_skill_after_install(
                    &skill.id,
                    &skill.name,
                    install_result.description.as_deref(),
                    Some(&remote_revision),
                    Some(&remote_revision),
                    Some(&install_result.content_hash),
                    "up_to_date",
                )
                .map_err(AppError::db)?;
            resync_copy_targets(&store, &skill.id)?;
            Ok(())
        })();
        git_fetcher::cleanup_temp(&temp_dir);

        match update_result {
            Ok(()) => managed_skill_by_id(&store, &skill_id),
            Err(e) => {
                let _ = store.update_skill_check_state(
                    &skill_id,
                    Some(&remote_revision),
                    "error",
                    Some(&e.message),
                );
                Err(e)
            }
        }
    })
    .await?
}

#[tauri::command]
pub async fn reimport_local_skill(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ManagedSkillDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;

        if !matches!(skill.source_type.as_str(), "local" | "import") {
            return Err(AppError::invalid_input("Only local skills can be reimported"));
        }

        let source_path = skill
            .source_ref
            .clone()
            .ok_or_else(|| AppError::not_found("Local skill is missing its original source path"))?;
        let path = PathBuf::from(&source_path);
        if !path.exists() {
            store
                .update_skill_check_state(&skill.id, None, "source_missing", Some("Original source path no longer exists"))
                .map_err(AppError::db)?;
            return Err(AppError::not_found("Original source path no longer exists"));
        }

        store
            .update_skill_update_status(&skill_id, "updating")
            .map_err(AppError::db)?;

        let result = (|| -> Result<(), AppError> {
            let staged_path = staged_path_for(&skill.central_path);
            let install_result = installer::install_from_local_to_destination(&path, Some(&skill.name), &staged_path)
                .map_err(AppError::io)?;
            swap_skill_directory(&staged_path, Path::new(&skill.central_path))?;
            store
                .update_skill_after_install(
                    &skill.id,
                    &skill.name,
                    install_result.description.as_deref(),
                    None,
                    None,
                    Some(&install_result.content_hash),
                    "local_only",
                )
                .map_err(AppError::db)?;
            resync_copy_targets(&store, &skill.id)?;
            Ok(())
        })();

        match result {
            Ok(()) => managed_skill_by_id(&store, &skill_id),
            Err(e) => {
                let _ = store.update_skill_check_state(&skill_id, None, "error", Some(&e.message));
                Err(e)
            }
        }
    })
    .await?
}

fn managed_skill_to_dto(
    store: &SkillStore,
    skill: SkillRecord,
    all_targets: &[SkillTargetRecord],
    tags_map: &std::collections::HashMap<String, Vec<String>>,
) -> ManagedSkillDto {
    let targets = all_targets
        .iter()
        .filter(|target| target.skill_id == skill.id)
        .map(|target| TargetDto {
            id: target.id.clone(),
            skill_id: target.skill_id.clone(),
            tool: target.tool.clone(),
            target_path: target.target_path.clone(),
            mode: target.mode.clone(),
            status: target.status.clone(),
            synced_at: target.synced_at,
        })
        .collect();

    let scenario_ids = store.get_scenarios_for_skill(&skill.id).unwrap_or_default();
    let tags = tags_map.get(&skill.id).cloned().unwrap_or_default();

    ManagedSkillDto {
        id: skill.id,
        name: skill.name,
        description: skill.description,
        source_type: skill.source_type,
        source_ref: skill.source_ref,
        source_revision: skill.source_revision,
        remote_revision: skill.remote_revision,
        update_status: skill.update_status,
        last_checked_at: skill.last_checked_at,
        last_check_error: skill.last_check_error,
        central_path: skill.central_path,
        enabled: skill.enabled,
        created_at: skill.created_at,
        updated_at: skill.updated_at,
        status: skill.status,
        targets,
        scenario_ids,
        tags,
    }
}

fn managed_skill_by_id(store: &SkillStore, skill_id: &str) -> Result<ManagedSkillDto, AppError> {
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;
    let all_targets = store.get_all_targets().map_err(AppError::db)?;
    let tags_map = store.get_tags_map().map_err(AppError::db)?;
    Ok(managed_skill_to_dto(store, skill, &all_targets, &tags_map))
}

fn store_installed_skill(
    store: &SkillStore,
    result: &installer::InstallResult,
    metadata: &InstallSourceMetadata,
    active_scenario_id: Option<&str>,
) -> Result<String, AppError> {
    let now = chrono::Utc::now().timestamp_millis();
    let central_path = result.central_path.to_string_lossy().to_string();

    if let Some(existing) = store
        .get_skill_by_central_path(&central_path)
        .map_err(AppError::db)?
    {
        store
            .update_skill_after_reinstall(
                &existing.id,
                &result.name,
                result.description.as_deref(),
                &metadata.source_type,
                metadata.source_ref.as_deref(),
                metadata.source_ref_resolved.as_deref(),
                metadata.source_subpath.as_deref(),
                metadata.source_branch.as_deref(),
                metadata.source_revision.as_deref(),
                metadata.remote_revision.as_deref(),
                Some(&result.content_hash),
                &metadata.update_status,
            )
            .map_err(AppError::db)?;

        if let Some(scenario_id) = active_scenario_id {
            store
                .add_skill_to_scenario(scenario_id, &existing.id)
                .map_err(AppError::db)?;
        }

        return Ok(existing.id);
    }

    let id = uuid::Uuid::new_v4().to_string();

    let record = SkillRecord {
        id: id.clone(),
        name: result.name.clone(),
        description: result.description.clone(),
        source_type: metadata.source_type.clone(),
        source_ref: metadata.source_ref.clone(),
        source_ref_resolved: metadata.source_ref_resolved.clone(),
        source_subpath: metadata.source_subpath.clone(),
        source_branch: metadata.source_branch.clone(),
        source_revision: metadata.source_revision.clone(),
        remote_revision: metadata.remote_revision.clone(),
        central_path,
        content_hash: Some(result.content_hash.clone()),
        enabled: true,
        created_at: now,
        updated_at: now,
        status: "ok".to_string(),
        update_status: metadata.update_status.clone(),
        last_checked_at: Some(now),
        last_check_error: None,
    };

    store.insert_skill(&record).map_err(AppError::db)?;

    if let Some(scenario_id) = active_scenario_id {
        store
            .add_skill_to_scenario(scenario_id, &id)
            .map_err(AppError::db)?;
    }

    Ok(id)
}

fn check_skill_update_internal(
    store: &SkillStore,
    skill_id: &str,
    force: bool,
) -> Result<ManagedSkillDto, AppError> {
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;

    if should_skip_update_check(store, &skill, force)? {
        return managed_skill_by_id(store, skill_id);
    }

    match skill.source_type.as_str() {
        "git" | "skillssh" => {
            let git_source = git_source_from_skill(&skill)?;
            let metadata_updated =
                skill.source_ref_resolved.as_deref() != Some(git_source.clone_url.as_str())
                    || skill.source_subpath.as_deref() != git_source.subpath.as_deref()
                    || skill.source_branch.as_deref() != git_source.branch.as_deref();
            if metadata_updated {
                store
                    .update_skill_source_metadata(
                        &skill.id,
                        Some(&git_source.clone_url),
                        git_source.subpath.as_deref(),
                        git_source.branch.as_deref(),
                        skill.source_revision.as_deref(),
                    )
                    .map_err(AppError::db)?;
            }

            match git_fetcher::resolve_remote_revision(&git_source.clone_url, git_source.branch.as_deref()) {
                Ok(remote_revision) => {
                    let update_status = match skill.source_revision.as_deref() {
                        Some(current) if current == remote_revision => "up_to_date",
                        Some(_) => "update_available",
                        None => "unknown",
                    };
                    store
                        .update_skill_check_state(
                            &skill.id,
                            Some(&remote_revision),
                            update_status,
                            None,
                        )
                        .map_err(AppError::db)?;
                }
                Err(err) => {
                    let message = err.to_string();
                    store
                        .update_skill_check_state(
                            &skill.id,
                            skill.remote_revision.as_deref(),
                            "error",
                            Some(&message),
                        )
                        .map_err(AppError::db)?;
                    return Err(AppError::git(message));
                }
            }
        }
        "local" | "import" => {
            let source_exists = skill
                .source_ref
                .as_ref()
                .map(|path| Path::new(path).exists())
                .unwrap_or(false);
            let (status, error) = if source_exists {
                ("local_only", None)
            } else {
                ("source_missing", Some("Original source path no longer exists"))
            };
            store
                .update_skill_check_state(&skill.id, None, status, error)
                .map_err(AppError::db)?;
        }
        _ => {
            store
                .update_skill_check_state(&skill.id, None, "unknown", None)
                .map_err(AppError::db)?;
        }
    }

    managed_skill_by_id(store, skill_id)
}

fn should_skip_update_check(
    store: &SkillStore,
    skill: &SkillRecord,
    force: bool,
) -> Result<bool, AppError> {
    if force {
        return Ok(false);
    }

    let ttl_minutes = store
        .get_setting("update_check_ttl_minutes")
        .map_err(AppError::db)?
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(60);
    let ttl_ms = ttl_minutes * 60 * 1000;
    let stable_status = !matches!(
        skill.update_status.as_str(),
        "unknown" | "checking" | "updating" | "error"
    );

    Ok(stable_status
        && skill
            .last_checked_at
            .map(|checked| chrono::Utc::now().timestamp_millis() - checked < ttl_ms)
            .unwrap_or(false))
}

fn git_source_from_skill(skill: &SkillRecord) -> Result<GitSkillSource, AppError> {
    if let Some(resolved) = &skill.source_ref_resolved {
        return Ok(GitSkillSource {
            clone_url: resolved.clone(),
            branch: skill.source_branch.clone(),
            subpath: skill.source_subpath.clone(),
            locator_skill_id: skill_ssh_id(skill),
        });
    }

    match skill.source_type.as_str() {
        "git" => {
            let source_ref = skill
                .source_ref
                .as_ref()
                .ok_or_else(|| AppError::invalid_input("Git skill is missing its source URL"))?;
            let parsed = git_fetcher::parse_git_source(source_ref);
            Ok(GitSkillSource {
                clone_url: parsed.clone_url,
                branch: parsed.branch,
                subpath: skill.source_subpath.clone().or(parsed.subpath),
                locator_skill_id: None,
            })
        }
        "skillssh" => {
            let source_ref = skill
                .source_ref
                .as_ref()
                .ok_or_else(|| AppError::invalid_input("skills.sh skill is missing its source reference"))?;
            let (repo_source, fallback_skill_id) = source_ref
                .rsplit_once('/')
                .ok_or_else(|| AppError::invalid_input("Invalid skills.sh source reference"))?;
            Ok(GitSkillSource {
                clone_url: format!("https://github.com/{}.git", repo_source),
                branch: skill.source_branch.clone(),
                subpath: skill.source_subpath.clone(),
                locator_skill_id: Some(fallback_skill_id.to_string()),
            })
        }
        _ => Err(AppError::invalid_input("Skill does not support git-based updates")),
    }
}

fn skill_ssh_id(skill: &SkillRecord) -> Option<String> {
    if skill.source_type != "skillssh" {
        return None;
    }

    skill.source_ref
        .as_deref()
        .and_then(|source_ref| source_ref.rsplit_once('/').map(|(_, skill_id)| skill_id.to_string()))
}

/// Return the list of individual skill directories to install from a resolved repo dir.
/// If `skill_dir` is itself a valid skill, returns `[skill_dir]`.
/// Otherwise enumerates immediate subdirs that are valid skills; falls back to `[skill_dir]`.
fn collect_git_skill_dirs(skill_dir: &Path) -> Vec<PathBuf> {
    if is_valid_skill_dir(skill_dir) {
        return vec![skill_dir.to_path_buf()];
    }
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(skill_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && is_valid_skill_dir(p))
        .collect();
    dirs.sort();
    if dirs.is_empty() {
        vec![skill_dir.to_path_buf()]
    } else {
        dirs
    }
}

fn resolve_skill_dir(
    repo_dir: &Path,
    subpath: Option<&str>,
    skill_id: Option<&str>,
) -> Result<PathBuf, AppError> {
    if let Some(subpath) = subpath {
        let path = repo_dir.join(subpath);
        if path.exists() && path.is_dir() {
            return Ok(path);
        }
    }

    git_fetcher::find_skill_dir(repo_dir, skill_id).map_err(AppError::git)
}

fn resolve_skillssh_install_target(
    store: &SkillStore,
    source_ref: &str,
    skill_id: &str,
) -> Result<(String, PathBuf), AppError> {
    if let Some(existing) = store
        .get_skill_by_source_ref("skillssh", source_ref)
        .map_err(AppError::db)?
    {
        return Ok((existing.name, PathBuf::from(existing.central_path)));
    }

    let base_name = skill_id.trim();
    if base_name.is_empty() {
        return Err(AppError::invalid_input("Skill id is empty"));
    }

    let mut attempt = 1;
    loop {
        let candidate_name = if attempt == 1 {
            base_name.to_string()
        } else {
            format!("{base_name}-{attempt}")
        };
        let candidate_path = central_repo::skills_dir().join(&candidate_name);
        let candidate_path_str = candidate_path.to_string_lossy().to_string();
        let occupied = store
            .get_skill_by_central_path(&candidate_path_str)
            .map_err(AppError::db)?
            .is_some();

        if !occupied {
            return Ok((candidate_name, candidate_path));
        }

        attempt += 1;
    }
}

fn staged_path_for(central_path: &str) -> PathBuf {
    let path = PathBuf::from(central_path);
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "skill".to_string());
    path.with_file_name(format!(".{file_name}.staged-{}", uuid::Uuid::new_v4()))
}

fn swap_skill_directory(staged_path: &Path, current_path: &Path) -> Result<(), AppError> {
    let backup_path = current_path.with_file_name(format!(
        ".{}.backup-{}",
        current_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "skill".to_string()),
        uuid::Uuid::new_v4()
    ));

    if current_path.exists() {
        std::fs::rename(current_path, &backup_path)?;
    }

    if let Err(err) = std::fs::rename(staged_path, current_path) {
        if backup_path.exists() {
            let _ = std::fs::rename(&backup_path, current_path);
        }
        let _ = remove_path_if_exists(staged_path);
        return Err(err.into());
    }

    remove_path_if_exists(&backup_path)?;
    Ok(())
}

fn resync_copy_targets(store: &SkillStore, skill_id: &str) -> Result<(), AppError> {
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;
    let source = PathBuf::from(&skill.central_path);
    let targets = store
        .get_targets_for_skill(skill_id)
        .map_err(AppError::db)?;

    for target in targets {
        if target.mode != "copy" {
            continue;
        }

        sync_engine::sync_skill(&source, Path::new(&target.target_path), sync_engine::SyncMode::Copy)
            .map_err(AppError::io)?;

        let updated_target = SkillTargetRecord {
            synced_at: Some(chrono::Utc::now().timestamp_millis()),
            status: "ok".to_string(),
            last_error: None,
            ..target
        };
        store
            .insert_target(&updated_target)
            .map_err(AppError::db)?;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_all_tags(store: State<'_, Arc<SkillStore>>) -> Result<Vec<String>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.get_all_tags().map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn set_skill_tags(
    skill_id: String,
    tags: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.set_tags_for_skill(&skill_id, &tags).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn cancel_install(
    key: String,
    cancel_registry: State<'_, Arc<InstallCancelRegistry>>,
) -> Result<bool, AppError> {
    Ok(cancel_registry.cancel(&key))
}

#[derive(Debug, Serialize)]
pub struct BatchImportResult {
    pub imported: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn batch_import_folder(
    folder_path: String,
    store: State<'_, Arc<SkillStore>>,
    app_handle: tauri::AppHandle,
) -> Result<BatchImportResult, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Emitter;

        let root = PathBuf::from(&folder_path);
        if !root.is_dir() {
            return Err(AppError::invalid_input("Selected path is not a directory"));
        }

        // Collect valid skill subdirectories (depth=1)
        let mut skill_dirs: Vec<PathBuf> = Vec::new();
        let entries = std::fs::read_dir(&root)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if is_valid_skill_dir(&path) {
                skill_dirs.push(path);
            }
        }

        if skill_dirs.is_empty() {
            return Ok(BatchImportResult {
                imported: 0,
                skipped: 0,
                errors: vec![],
            });
        }

        let total = skill_dirs.len();
        let mut imported = 0usize;
        let mut skipped = 0usize;
        let mut errors = Vec::new();
        let active = store.get_active_scenario_id().ok().flatten();

        for (i, dir) in skill_dirs.iter().enumerate() {
            let name = skill_metadata::infer_skill_name(dir);

            app_handle
                .emit(
                    "batch-import-progress",
                    serde_json::json!({
                        "current": i + 1,
                        "total": total,
                        "name": &name,
                    }),
                )
                .ok();

            // Check if already imported by prospective central path
            let prospective_central = central_repo::skills_dir().join(&name);
            let central_str = prospective_central.to_string_lossy().to_string();
            if let Ok(Some(existing)) = store.get_skill_by_central_path(&central_str) {
                if let Some(ref scenario_id) = active {
                    if let Err(e) = store.add_skill_to_scenario(scenario_id, &existing.id) {
                        errors.push(format!("{}: {}", name, e));
                        continue;
                    }
                }
                skipped += 1;
                continue;
            }

            match installer::install_from_local(dir, Some(&name)) {
                Ok(result) => {
                    let metadata = InstallSourceMetadata {
                        source_type: "local".to_string(),
                        source_ref: Some(dir.to_string_lossy().to_string()),
                        source_ref_resolved: None,
                        source_subpath: None,
                        source_branch: None,
                        source_revision: None,
                        remote_revision: None,
                        update_status: "local_only".to_string(),
                    };
                    match store_installed_skill(&store, &result, &metadata, active.as_deref()) {
                        Ok(_) => imported += 1,
                        Err(e) => errors.push(format!("{}: {}", name, e)),
                    }
                }
                Err(e) => {
                    errors.push(format!("{}: {}", name, e));
                }
            }
        }

        Ok(BatchImportResult {
            imported,
            skipped,
            errors,
        })
    })
    .await?
}

fn remove_path_if_exists(path: &Path) -> Result<(), AppError> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}
