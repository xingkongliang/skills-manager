//! Market updates and explicit conversion of an existing library entry.
use super::{
    client::{source_url, MarketClient, SITE_BASE},
    download,
    models::{validate_identity, Package},
};
use crate::{
    commands::skills::{
        managed_skill_by_id, pending_removals_for, removal_approval_token, resync_copy_targets,
        staged_path_for, swap_skill_directory, StagedPathGuard, UpdateSkillResult,
    },
    core::{
        content_hash,
        error::AppError,
        installer,
        repo_lock::RepoLock,
        skill_store::{SkillRecord, SkillStore},
        sync_metadata,
    },
};
use std::{
    path::Path,
    sync::{atomic::AtomicBool, Arc},
};

pub fn identity(source: &str) -> Result<(String, String), AppError> {
    let url =
        reqwest::Url::parse(source).map_err(|error| AppError::invalid_input(error.to_string()))?;
    let base = reqwest::Url::parse(SITE_BASE).unwrap();
    if url.scheme() != base.scheme()
        || url.host_str() != base.host_str()
        || url.port() != base.port()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(AppError::invalid_input("Invalid Agent Buddy source URL"));
    }
    let path = url
        .path()
        .strip_prefix("/skill/")
        .ok_or_else(|| AppError::invalid_input("Invalid Agent Buddy skill path"))?;
    let identifier = urlencoding::decode(path)
        .map_err(|error| AppError::invalid_input(error.to_string()))?
        .into_owned();
    let repositories: Vec<_> = url
        .query_pairs()
        .filter(|(key, _)| key == "repository")
        .collect();
    if repositories.len() != 1 {
        return Err(AppError::invalid_input(
            "Agent Buddy source must have one repository",
        ));
    }
    let repository = repositories[0].1.to_string();
    validate_identity(&identifier, &repository)?;
    Ok((identifier, repository))
}

pub fn prefetch_version(source: &str, proxy: Option<&str>) -> Result<String, AppError> {
    let (identifier, repository) = identity(source)?;
    let client = MarketClient::shared(proxy)?;
    latest_version(&client, &identifier, &repository)
}

pub fn latest_version(
    client: &MarketClient,
    identifier: &str,
    repository: &str,
) -> Result<String, AppError> {
    tauri::async_runtime::block_on(async {
        let package: Package = client
            .detail(client.detail_url(identifier, repository, None)?)
            .await?;
        if package.identifier != identifier || package.repository != repository {
            return Err(AppError::network(
                "Agent Buddy returned a different package",
            ));
        }
        if package.disabled || package.all_versions_disabled {
            return Err(AppError::invalid_input("Agent Buddy skill is disabled"));
        }
        if package.no_permission || !package.user_permission.as_ref().is_some_and(|p| p.download) {
            return Err(AppError::invalid_input(
                "Agent Buddy download permission is unavailable",
            ));
        }
        package
            .newest_version
            .filter(|v| !v.version.trim().is_empty())
            .map(|v| v.version)
            .ok_or_else(|| AppError::invalid_input("Agent Buddy skill has no installable version"))
    })
}

pub fn same_snapshot(current: &SkillRecord, original: &SkillRecord) -> bool {
    current.central_path == original.central_path
        && current.source_type == original.source_type
        && current.source_ref == original.source_ref
        && current.source_ref_resolved == original.source_ref_resolved
        && current.source_branch == original.source_branch
        && current.source_subpath == original.source_subpath
        && current.source_revision == original.source_revision
        && current.content_hash == original.content_hash
}

pub fn update(
    store: &SkillStore,
    skill_id: &str,
    proxy: Option<&str>,
    target: Option<(&str, &str)>,
    cancel: Option<&Arc<AtomicBool>>,
    approval: Option<&str>,
) -> Result<UpdateSkillResult, AppError> {
    let original = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;
    let (identifier, repository) = match target {
        Some((identifier, repository)) => {
            validate_identity(identifier, repository)?;
            (identifier.to_owned(), repository.to_owned())
        }
        None => {
            if original.source_type != "bytedance" {
                return Err(AppError::invalid_input("Skill is not from Agent Buddy"));
            }
            identity(
                original
                    .source_ref
                    .as_deref()
                    .ok_or_else(|| AppError::invalid_input("Agent Buddy source is missing"))?,
            )?
        }
    };
    let client = MarketClient::shared(proxy)?;
    let cancel = cancel
        .cloned()
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let outcome = (|| {
        // Download and extract off the library lock. Never use a persisted asset URL.
        let downloaded = tauri::async_runtime::block_on(download::prepare(
            &client,
            &identifier,
            &repository,
            &cancel,
        ))?;
        apply_downloaded(
            store,
            &original,
            downloaded,
            &identifier,
            &repository,
            target.is_some(),
            &cancel,
            approval,
        )
    })();
    if let Err(error) = &outcome {
        if target.is_none() && !cancel.load(std::sync::atomic::Ordering::SeqCst) {
            // Do not write an error onto a concurrently converted/updated entry.
            if let Ok(_lock) = RepoLock::acquire_foreground("record Agent Buddy update failure") {
                if store
                    .get_skill_by_id(skill_id)
                    .ok()
                    .flatten()
                    .is_some_and(|current| same_snapshot(&current, &original))
                {
                    let _ = store.update_skill_check_state(
                        skill_id,
                        original.remote_revision.as_deref(),
                        "error",
                        Some(&error.message),
                    );
                }
            }
        }
    }
    outcome
}

/// Commit a verified package, keeping download work outside the repository lock.
pub(super) fn apply_downloaded(
    store: &SkillStore,
    original: &SkillRecord,
    downloaded: download::DownloadedSkill,
    identifier: &str,
    repository: &str,
    converting: bool,
    cancel: &AtomicBool,
    approval: Option<&str>,
) -> Result<UpdateSkillResult, AppError> {
    let skill_id = original.id.as_str();
    let _lock = RepoLock::acquire_foreground("update Agent Buddy skill").map_err(AppError::io)?;
    download::check_cancel(cancel)?;
    let current = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill was removed while downloading"))?;
    if !same_snapshot(&current, original) {
        return Err(AppError::invalid_input(
            "Skill source changed while downloading; retry the update",
        ));
    }
    let staged = staged_path_for(&current.central_path);
    let guard = StagedPathGuard::new(&staged, true);
    let installed =
        installer::install_skill_dir_to_destination(&downloaded.skill_dir, &current.name, &staged)
            .map_err(AppError::io)?;
    // Compare the actual library too, so user edits cannot be silently classified as unchanged.
    let changed = content_hash::hash_directory(Path::new(&current.central_path))
        .map_err(AppError::io)?
        != installed.content_hash;
    let pending = pending_removals_for(store, &current, changed.then_some(staged.as_path()))?;
    let source = source_url(identifier, repository);
    let token = removal_approval_token(&format!("{source}\0{}", downloaded.version), &pending);
    if !pending.is_empty() && approval != Some(token.as_str()) {
        // A declined conversion must retain the old source's revision and check state.
        if !converting {
            store
                .update_skill_check_state(
                    skill_id,
                    Some(&downloaded.version),
                    "update_available",
                    None,
                )
                .map_err(AppError::db)?;
        }
        return Ok(UpdateSkillResult {
            skill: managed_skill_by_id(store, skill_id)?,
            content_changed: false,
            pending_removals: pending,
            removal_approval: Some(token),
        });
    }
    download::check_cancel(cancel)?;
    if changed {
        swap_skill_directory(&staged, Path::new(&current.central_path))?;
        guard.release();
    }
    store
        .update_skill_after_reinstall(
            skill_id,
            &current.name,
            installed.description.as_deref(),
            "bytedance",
            Some(&source),
            Some(&downloaded.download_url),
            None,
            None,
            Some(&downloaded.version),
            Some(&downloaded.version),
            Some(&installed.content_hash),
            "up_to_date",
        )
        .map_err(AppError::db)?;
    resync_copy_targets(store, skill_id)?;
    sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;
    Ok(UpdateSkillResult {
        skill: managed_skill_by_id(store, skill_id)?,
        content_changed: changed,
        pending_removals: Vec::new(),
        removal_approval: None,
    })
}
