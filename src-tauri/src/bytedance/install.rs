use super::{
    client::source_url,
    download::{check_cancel, DownloadedSkill},
};
use crate::{
    commands::skills::{store_installed_skill_unlocked, InstallSourceMetadata},
    core::{
        central_repo, error::AppError, installer, repo_lock::RepoLock,
        skill_metadata::sanitize_skill_name, skill_store::SkillStore,
    },
};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

pub fn unique_target(
    store: &SkillStore,
    root: &Path,
    name: &str,
) -> Result<(String, PathBuf), AppError> {
    let base = sanitize_skill_name(name)
        .ok_or_else(|| AppError::invalid_input("Invalid ByteDance skill name"))?;
    for suffix in 0..10_000 {
        let name = if suffix == 0 {
            base.clone()
        } else {
            format!("{base}-{suffix}")
        };
        let path = root.join(&name);
        let exists = match path.symlink_metadata() {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(AppError::io(error)),
        };
        if !exists
            && store
                .get_skill_by_central_path(&path.to_string_lossy())
                .map_err(AppError::db)?
                .is_none()
        {
            return Ok((name, path));
        }
    }
    Err(AppError::invalid_input(
        "No available directory for ByteDance skill",
    ))
}

pub fn commit(
    store: &SkillStore,
    downloaded: DownloadedSkill,
    identifier: &str,
    repository: &str,
    cancel: &AtomicBool,
) -> Result<String, AppError> {
    check_cancel(cancel)?;
    let _lock = RepoLock::acquire_foreground("install_from_bytedance").map_err(AppError::io)?;
    check_cancel(cancel)?;
    let source_ref = source_url(identifier, repository);
    if let Some(existing) = store
        .get_skill_by_source_ref("bytedance", &source_ref)
        .map_err(AppError::db)?
    {
        return Ok(existing.id);
    }
    let root = central_repo::skills_dir();
    std::fs::create_dir_all(&root).map_err(AppError::io)?;
    let (name, destination) = unique_target(store, &root, &downloaded.name)?;
    let staging = tempfile::tempdir_in(&root).map_err(AppError::io)?;
    let mut installed = installer::install_skill_dir_to_destination(
        &downloaded.skill_dir,
        &name,
        &staging.path().join("skill"),
    )
    .map_err(AppError::io)?;
    check_cancel(cancel)?;
    std::fs::rename(&installed.central_path, &destination).map_err(AppError::io)?;
    installed.central_path = destination.clone();
    let metadata = InstallSourceMetadata {
        source_type: "bytedance".into(),
        source_ref: Some(source_ref),
        source_ref_resolved: Some(downloaded.download_url),
        source_revision: Some(downloaded.version),
        source_subpath: None,
        source_branch: None,
        remote_revision: None,
        update_status: "local_only".into(),
    };
    let result = store_installed_skill_unlocked(store, &installed, &metadata, None);
    // A metadata-file write can fail after the DB insert; preserve that installation.
    if result.is_err()
        && store
            .get_skill_by_central_path(&destination.to_string_lossy())
            .map_err(AppError::db)?
            .is_none()
    {
        let _ = std::fs::remove_dir_all(&destination);
    }
    result
}
