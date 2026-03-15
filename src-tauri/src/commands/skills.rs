use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::State;
use walkdir::WalkDir;

use crate::core::{
    central_repo,
    git_fetcher,
    install_cancel::InstallCancelRegistry,
    installer,
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
pub async fn get_managed_skills(store: State<'_, Arc<SkillStore>>) -> Result<Vec<ManagedSkillDto>, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skills = store.get_all_skills().map_err(|e| e.to_string())?;
        let all_targets = store.get_all_targets().map_err(|e| e.to_string())?;
        let tags_map = store.get_tags_map().map_err(|e| e.to_string())?;
        Ok(skills
            .into_iter()
            .map(|skill| managed_skill_to_dto(&store, skill, &all_targets, &tags_map))
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_skills_for_scenario(
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<ManagedSkillDto>, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skills = store
            .get_skills_for_scenario(&scenario_id)
            .map_err(|e| e.to_string())?;
        let all_targets = store.get_all_targets().map_err(|e| e.to_string())?;
        let tags_map = store.get_tags_map().map_err(|e| e.to_string())?;

        Ok(skills
            .into_iter()
            .map(|skill| managed_skill_to_dto(&store, skill, &all_targets, &tags_map))
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_skill_document(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<SkillDocumentDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Skill not found".to_string())?;

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
                let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                return Ok(SkillDocumentDto {
                    skill_id,
                    filename: name.to_string(),
                    content,
                    central_path: skill.central_path,
                });
            }
        }

        for entry in WalkDir::new(&central).max_depth(4) {
            if let Ok(e) = entry {
                let fname = e.file_name().to_string_lossy();
                if candidates.contains(&fname.as_ref()) {
                    let content = std::fs::read_to_string(e.path()).map_err(|e| e.to_string())?;
                    return Ok(SkillDocumentDto {
                        skill_id,
                        filename: fname.to_string(),
                        content,
                        central_path: skill.central_path,
                    });
                }
            }
        }

        Err("No documentation file found".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_managed_skill(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Skill not found".to_string())?;

        let targets = store
            .get_targets_for_skill(&skill_id)
            .map_err(|e| e.to_string())?;
        for target in &targets {
            let target_path = PathBuf::from(&target.target_path);
            sync_engine::remove_target(&target_path).ok();
        }

        let central = PathBuf::from(&skill.central_path);
        if central.exists() {
            std::fs::remove_dir_all(&central).ok();
        }

        store.delete_skill(&skill_id).map_err(|e| e.to_string())?;

        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn install_local(
    source_path: String,
    name: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = PathBuf::from(&source_path);
        let result = installer::install_from_local(&path, name.as_deref()).map_err(|e| e.to_string())?;

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
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn install_git(
    repo_url: String,
    name: Option<String>,
    store: State<'_, Arc<SkillStore>>,
    cancel_registry: State<'_, Arc<InstallCancelRegistry>>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
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
            git_fetcher::clone_repo_ref(&parsed.clone_url, parsed.branch.as_deref(), Some(&cancel)).map_err(|e| e.to_string())?;

        emit_progress("installing");
        let install_result = (|| -> Result<(installer::InstallResult, InstallSourceMetadata), String> {
            let skill_dir = resolve_skill_dir(&temp_dir, parsed.subpath.as_deref(), None)?;
            let revision = git_fetcher::get_head_revision(&temp_dir).map_err(|e| e.to_string())?;
            let result =
                installer::install_from_git_dir(&skill_dir, name.as_deref()).map_err(|e| e.to_string())?;
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
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn install_from_skillssh(
    source: String,
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
    cancel_registry: State<'_, Arc<InstallCancelRegistry>>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
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
        let temp_dir = git_fetcher::clone_repo_ref(&repo_url, None, Some(&cancel)).map_err(|e| e.to_string())?;

        emit_progress("installing");
        let install_result = (|| -> Result<(installer::InstallResult, InstallSourceMetadata), String> {
            let skill_dir = resolve_skill_dir(&temp_dir, None, Some(&skill_id))?;
            let revision = git_fetcher::get_head_revision(&temp_dir).map_err(|e| e.to_string())?;
            let source_ref = format!("{}/{}", source, skill_id);
            let (install_name, destination) =
                resolve_skillssh_install_target(&store, &source_ref, &skill_id)?;
            let result =
                installer::install_skill_dir_to_destination(&skill_dir, &install_name, &destination)
                    .map_err(|e| e.to_string())?;
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
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn check_skill_update(
    skill_id: String,
    force: Option<bool>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ManagedSkillDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        check_skill_update_internal(&store, &skill_id, force.unwrap_or(false))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn check_all_skill_updates(
    force: Option<bool>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let force_check = force.unwrap_or(false);
        let ids: Vec<String> = store
            .get_all_skills()
            .map_err(|e| e.to_string())?
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
            Err(format!(
                "Failed to check {} skill(s): {}",
                failed.len(),
                failed.join("; ")
            ))
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn update_skill(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
    cancel_registry: State<'_, Arc<InstallCancelRegistry>>,
) -> Result<ManagedSkillDto, String> {
    let store = store.inner().clone();
    let registry = cancel_registry.inner().clone();
    let cancel_key = format!("update:{}", skill_id);
    let cancel = registry.register(&cancel_key);
    let _cancel_guard = CancelRegistrationGuard::new(registry.clone(), cancel_key);

    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Skill not found".to_string())?;

        if !matches!(skill.source_type.as_str(), "git" | "skillssh") {
            return Err("Only git-based skills can be updated".to_string());
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
            message
        })?;

        store
            .update_skill_update_status(&skill_id, "updating")
            .map_err(|e| e.to_string())?;

        let temp_dir =
            git_fetcher::clone_repo_ref(&git_source.clone_url, git_source.branch.as_deref(), Some(&cancel)).map_err(|e| e.to_string())?;
        let update_result = (|| -> Result<(), String> {
            git_fetcher::checkout_revision(&temp_dir, &remote_revision).map_err(|e| e.to_string())?;
            let skill_dir = resolve_skill_dir(
                &temp_dir,
                git_source.subpath.as_deref(),
                git_source.locator_skill_id.as_deref(),
            )?;
            let staged_path = staged_path_for(&skill.central_path);
            let install_result = installer::install_skill_dir_to_destination(&skill_dir, &skill.name, &staged_path)
                .map_err(|e| e.to_string())?;
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
                .map_err(|e| e.to_string())?;
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
                .map_err(|e| e.to_string())?;
            resync_copy_targets(&store, &skill.id)?;
            Ok(())
        })();
        git_fetcher::cleanup_temp(&temp_dir);

        match update_result {
            Ok(()) => managed_skill_by_id(&store, &skill_id),
            Err(message) => {
                let _ = store.update_skill_check_state(
                    &skill_id,
                    Some(&remote_revision),
                    "error",
                    Some(&message),
                );
                Err(message)
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn reimport_local_skill(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ManagedSkillDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Skill not found".to_string())?;

        if !matches!(skill.source_type.as_str(), "local" | "import") {
            return Err("Only local skills can be reimported".to_string());
        }

        let source_path = skill
            .source_ref
            .clone()
            .ok_or_else(|| "Local skill is missing its original source path".to_string())?;
        let path = PathBuf::from(&source_path);
        if !path.exists() {
            store
                .update_skill_check_state(&skill.id, None, "source_missing", Some("Original source path no longer exists"))
                .map_err(|e| e.to_string())?;
            return Err("Original source path no longer exists".to_string());
        }

        store
            .update_skill_update_status(&skill_id, "updating")
            .map_err(|e| e.to_string())?;

        let result = (|| -> Result<(), String> {
            let staged_path = staged_path_for(&skill.central_path);
            let install_result = installer::install_from_local_to_destination(&path, Some(&skill.name), &staged_path)
                .map_err(|e| e.to_string())?;
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
                .map_err(|e| e.to_string())?;
            resync_copy_targets(&store, &skill.id)?;
            Ok(())
        })();

        match result {
            Ok(()) => managed_skill_by_id(&store, &skill_id),
            Err(message) => {
                let _ = store.update_skill_check_state(&skill_id, None, "error", Some(&message));
                Err(message)
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
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

fn managed_skill_by_id(store: &SkillStore, skill_id: &str) -> Result<ManagedSkillDto, String> {
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Skill not found".to_string())?;
    let all_targets = store.get_all_targets().map_err(|e| e.to_string())?;
    let tags_map = store.get_tags_map().map_err(|e| e.to_string())?;
    Ok(managed_skill_to_dto(store, skill, &all_targets, &tags_map))
}

fn store_installed_skill(
    store: &SkillStore,
    result: &installer::InstallResult,
    metadata: &InstallSourceMetadata,
    active_scenario_id: Option<&str>,
) -> Result<String, String> {
    let now = chrono::Utc::now().timestamp_millis();
    let central_path = result.central_path.to_string_lossy().to_string();

    if let Some(existing) = store
        .get_skill_by_central_path(&central_path)
        .map_err(|e| e.to_string())?
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
            .map_err(|e| e.to_string())?;

        if let Some(scenario_id) = active_scenario_id {
            store
                .add_skill_to_scenario(scenario_id, &existing.id)
                .map_err(|e| e.to_string())?;
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

    store.insert_skill(&record).map_err(|e| e.to_string())?;

    if let Some(scenario_id) = active_scenario_id {
        store
            .add_skill_to_scenario(scenario_id, &id)
            .map_err(|e| e.to_string())?;
    }

    Ok(id)
}

fn check_skill_update_internal(
    store: &SkillStore,
    skill_id: &str,
    force: bool,
) -> Result<ManagedSkillDto, String> {
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Skill not found".to_string())?;

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
                    .map_err(|e| e.to_string())?;
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
                        .map_err(|e| e.to_string())?;
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
                        .map_err(|e| e.to_string())?;
                    return Err(message);
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
                .map_err(|e| e.to_string())?;
        }
        _ => {
            store
                .update_skill_check_state(&skill.id, None, "unknown", None)
                .map_err(|e| e.to_string())?;
        }
    }

    managed_skill_by_id(store, skill_id)
}

fn should_skip_update_check(
    store: &SkillStore,
    skill: &SkillRecord,
    force: bool,
) -> Result<bool, String> {
    if force {
        return Ok(false);
    }

    let ttl_minutes = store
        .get_setting("update_check_ttl_minutes")
        .map_err(|e| e.to_string())?
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

fn git_source_from_skill(skill: &SkillRecord) -> Result<GitSkillSource, String> {
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
                .ok_or_else(|| "Git skill is missing its source URL".to_string())?;
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
                .ok_or_else(|| "skills.sh skill is missing its source reference".to_string())?;
            let (repo_source, fallback_skill_id) = source_ref
                .rsplit_once('/')
                .ok_or_else(|| "Invalid skills.sh source reference".to_string())?;
            Ok(GitSkillSource {
                clone_url: format!("https://github.com/{}.git", repo_source),
                branch: skill.source_branch.clone(),
                subpath: skill.source_subpath.clone(),
                locator_skill_id: Some(fallback_skill_id.to_string()),
            })
        }
        _ => Err("Skill does not support git-based updates".to_string()),
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

fn resolve_skill_dir(
    repo_dir: &Path,
    subpath: Option<&str>,
    skill_id: Option<&str>,
) -> Result<PathBuf, String> {
    if let Some(subpath) = subpath {
        let path = repo_dir.join(subpath);
        if path.exists() && path.is_dir() {
            return Ok(path);
        }
    }

    git_fetcher::find_skill_dir(repo_dir, skill_id).map_err(|e| e.to_string())
}

fn resolve_skillssh_install_target(
    store: &SkillStore,
    source_ref: &str,
    skill_id: &str,
) -> Result<(String, PathBuf), String> {
    if let Some(existing) = store
        .get_skill_by_source_ref("skillssh", source_ref)
        .map_err(|e| e.to_string())?
    {
        return Ok((existing.name, PathBuf::from(existing.central_path)));
    }

    let base_name = skill_id.trim();
    if base_name.is_empty() {
        return Err("Skill id is empty".to_string());
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
            .map_err(|e| e.to_string())?
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

fn swap_skill_directory(staged_path: &Path, current_path: &Path) -> Result<(), String> {
    let backup_path = current_path.with_file_name(format!(
        ".{}.backup-{}",
        current_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "skill".to_string()),
        uuid::Uuid::new_v4()
    ));

    if current_path.exists() {
        std::fs::rename(current_path, &backup_path).map_err(|e| e.to_string())?;
    }

    if let Err(err) = std::fs::rename(staged_path, current_path) {
        if backup_path.exists() {
            let _ = std::fs::rename(&backup_path, current_path);
        }
        let _ = remove_path_if_exists(staged_path);
        return Err(err.to_string());
    }

    remove_path_if_exists(&backup_path)?;
    Ok(())
}

fn resync_copy_targets(store: &SkillStore, skill_id: &str) -> Result<(), String> {
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Skill not found".to_string())?;
    let source = PathBuf::from(&skill.central_path);
    let targets = store
        .get_targets_for_skill(skill_id)
        .map_err(|e| e.to_string())?;

    for target in targets {
        if target.mode != "copy" {
            continue;
        }

        sync_engine::sync_skill(&source, Path::new(&target.target_path), sync_engine::SyncMode::Copy)
            .map_err(|e| e.to_string())?;

        let updated_target = SkillTargetRecord {
            synced_at: Some(chrono::Utc::now().timestamp_millis()),
            status: "ok".to_string(),
            last_error: None,
            ..target
        };
        store
            .insert_target(&updated_target)
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_all_tags(store: State<'_, Arc<SkillStore>>) -> Result<Vec<String>, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.get_all_tags().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn set_skill_tags(
    skill_id: String,
    tags: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.set_tags_for_skill(&skill_id, &tags).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn cancel_install(
    key: String,
    cancel_registry: State<'_, Arc<InstallCancelRegistry>>,
) -> Result<bool, String> {
    Ok(cancel_registry.cancel(&key))
}

fn remove_path_if_exists(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        std::fs::remove_dir_all(path).map_err(|e| e.to_string())?;
    } else if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}
