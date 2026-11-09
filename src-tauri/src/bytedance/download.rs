use super::{
    client::MarketClient,
    models::{validate_identity, Component, Package},
};
use crate::core::error::AppError;
use std::{
    collections::HashSet,
    future::Future,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

const MAX_DOWNLOAD: usize = 50 * 1024 * 1024;
const MAX_EXTRACTED: u64 = 100 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;

pub struct DownloadedSkill {
    pub _temp: tempfile::TempDir,
    pub skill_dir: PathBuf,
    pub name: String,
    pub version: String,
    pub download_url: String,
}

pub fn check_cancel(cancel: &AtomicBool) -> Result<(), AppError> {
    if cancel.load(Ordering::SeqCst) {
        Err(AppError::cancelled(
            "ByteDance skill installation cancelled",
        ))
    } else {
        Ok(())
    }
}

async fn cancellable<T>(
    cancel: &AtomicBool,
    work: impl Future<Output = Result<T, AppError>>,
) -> Result<T, AppError> {
    check_cancel(cancel)?;
    tokio::pin!(work);
    let mut interval = tokio::time::interval(Duration::from_millis(50));
    loop {
        tokio::select! {
            result = &mut work => return result,
            _ = interval.tick() => check_cancel(cancel)?,
        }
    }
}

pub fn download_url(raw: &str) -> Result<reqwest::Url, AppError> {
    let mut url =
        reqwest::Url::parse(raw).map_err(|error| AppError::invalid_input(error.to_string()))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(AppError::invalid_input(
            "ByteDance asset must have an HTTPS download URL",
        ));
    }
    if url.host_str() == Some("skills.byted.org") {
        url.set_path(
            &url.path()
                .replacen("/api/v1/download/", "/api/v4/skills/download/", 1),
        );
    }
    Ok(url)
}

pub async fn prepare(
    client: &MarketClient,
    identifier: &str,
    repository: &str,
    cancel: &Arc<AtomicBool>,
) -> Result<DownloadedSkill, AppError> {
    validate_identity(identifier, repository)?;
    let package: Package = cancellable(
        cancel,
        client.detail(client.detail_url(identifier, repository, None)?),
    )
    .await?;
    if package.identifier != identifier || package.repository != repository {
        return Err(AppError::network("ByteDance returned a different package"));
    }
    if package.disabled
        || package.all_versions_disabled
        || package.no_permission
        || !package.user_permission.as_ref().is_some_and(|p| p.download)
    {
        return Err(AppError::invalid_input(
            "ByteDance skill is disabled or download permission is unavailable",
        ));
    }
    let version = package
        .newest_version
        .filter(|v| !v.version.trim().is_empty())
        .ok_or_else(|| AppError::invalid_input("ByteDance skill has no installable version"))?
        .version;
    let component: Component = cancellable(
        cancel,
        client.detail(client.detail_url(identifier, repository, Some(&version))?),
    )
    .await?;
    if component.disabled || component.version != version {
        return Err(AppError::invalid_input(
            "ByteDance skill version is disabled or mismatched",
        ));
    }
    let assets: Vec<_> = component
        .assets
        .into_iter()
        .filter(|asset| {
            asset.content_type == "application/zip"
                || asset.blob_name.to_ascii_lowercase().ends_with(".zip")
        })
        .collect();
    if assets.len() != 1 {
        return Err(AppError::invalid_input(
            "ByteDance skill must provide one ZIP asset",
        ));
    }
    let url = download_url(&assets[0].download_link)?;
    let bytes = read_archive(client, url.clone(), cancel).await?;
    check_cancel(cancel)?;
    let cancel = cancel.clone();
    let (temp, skill_dir) = tauri::async_runtime::spawn_blocking(move || {
        let temp = tempfile::tempdir().map_err(AppError::io)?;
        let skill_dir = extract(&bytes, temp.path(), &cancel)?;
        Ok::<_, AppError>((temp, skill_dir))
    })
    .await
    .map_err(AppError::internal)??;
    Ok(DownloadedSkill {
        _temp: temp,
        skill_dir,
        name: package.name,
        version,
        download_url: url.to_string(),
    })
}

pub async fn read_archive(
    client: &MarketClient,
    url: reqwest::Url,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, AppError> {
    let mut response = cancellable(cancel, async {
        client
            .http
            .get(url)
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .map_err(AppError::network)?
            .error_for_status()
            .map_err(AppError::network)
    })
    .await?;
    if response
        .content_length()
        .is_some_and(|n| n > MAX_DOWNLOAD as u64)
    {
        return Err(AppError::invalid_input("ByteDance ZIP exceeds 50 MiB"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = cancellable(cancel, async {
        response.chunk().await.map_err(AppError::network)
    })
    .await?
    {
        if bytes.len() + chunk.len() > MAX_DOWNLOAD {
            return Err(AppError::invalid_input("ByteDance ZIP exceeds 50 MiB"));
        }
        bytes.extend_from_slice(&chunk);
    }
    check_cancel(cancel)?;
    Ok(bytes)
}

fn safe_zip_path(name: &str) -> Result<PathBuf, AppError> {
    let parts: Vec<_> = name.trim_end_matches('/').split('/').collect();
    if name.len() > 512
        || parts.is_empty()
        || parts.len() > 16
        || name.contains('\\')
        || name.contains(':')
        || name.chars().any(char::is_control)
        || parts.iter().any(|part| {
            let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            part.is_empty()
                || matches!(*part, "." | "..")
                || part.ends_with(['.', ' '])
                || matches!(
                    stem.as_str(),
                    "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
                )
                || (stem
                    .get(..3)
                    .is_some_and(|prefix| prefix == "COM" || prefix == "LPT")
                    && matches!(
                        stem.get(3..),
                        Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
                    ))
        })
    {
        return Err(AppError::invalid_input(format!("Unsafe ZIP path: {name}")));
    }
    Ok(parts.iter().collect())
}

pub fn extract(bytes: &[u8], root: &Path, cancel: &AtomicBool) -> Result<PathBuf, AppError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| AppError::invalid_input(error.to_string()))?;
    if archive.len() > MAX_ENTRIES {
        return Err(AppError::invalid_input(
            "ByteDance ZIP has too many entries",
        ));
    }
    let mut paths = Vec::with_capacity(archive.len());
    let mut seen = HashSet::new();
    let mut total = 0u64;
    let mut path_bytes = 0usize;
    let mut components = 0usize;
    // Validate the entire central directory before writing anything.
    for index in 0..archive.len() {
        check_cancel(cancel)?;
        let entry = archive
            .by_index(index)
            .map_err(|error| AppError::invalid_input(error.to_string()))?;
        let path = safe_zip_path(entry.name())?;
        let mode = entry.unix_mode().unwrap_or(0) & 0o170000;
        if !matches!(mode, 0 | 0o100000 | 0o040000) {
            return Err(AppError::invalid_input(
                "ByteDance ZIP contains a link or special file",
            ));
        }
        if !seen.insert(path.to_string_lossy().to_lowercase()) {
            return Err(AppError::invalid_input(
                "ByteDance ZIP contains duplicate paths",
            ));
        }
        path_bytes += entry.name().len();
        components += path.components().count();
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| AppError::invalid_input("Invalid ZIP size"))?;
        if total > MAX_EXTRACTED || path_bytes > 64 * 1024 || components > 16 * 1024 {
            return Err(AppError::invalid_input(
                "ByteDance ZIP exceeds extraction limits",
            ));
        }
        paths.push(path);
    }
    let mut written = 0u64;
    for (index, path) in paths.into_iter().enumerate() {
        check_cancel(cancel)?;
        let mut entry = archive
            .by_index(index)
            .map_err(|error| AppError::invalid_input(error.to_string()))?;
        let destination = root.join(path);
        if entry.is_dir() {
            std::fs::create_dir_all(&destination).map_err(AppError::io)?;
            continue;
        }
        std::fs::create_dir_all(destination.parent().unwrap()).map_err(AppError::io)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(AppError::io)?;
        let mut buffer = [0u8; 32 * 1024];
        let mut entry_written = 0u64;
        loop {
            check_cancel(cancel)?;
            let size = entry.read(&mut buffer).map_err(AppError::io)?;
            if size == 0 {
                break;
            }
            written += size as u64;
            entry_written += size as u64;
            if written > MAX_EXTRACTED || entry_written > entry.size() {
                return Err(AppError::invalid_input(
                    "ByteDance ZIP exceeds extraction limits",
                ));
            }
            file.write_all(&buffer[..size]).map_err(AppError::io)?;
        }
        #[cfg(unix)]
        if entry.unix_mode().is_some_and(|mode| mode & 0o111 != 0) {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o755))
                .map_err(AppError::io)?;
        }
    }
    let mut skill_dirs = HashSet::new();
    for entry in walkdir::WalkDir::new(root).max_depth(16) {
        let entry = entry.map_err(AppError::io)?;
        if entry.file_type().is_file()
            && matches!(entry.file_name().to_str(), Some("SKILL.md" | "skill.md"))
        {
            skill_dirs.insert(entry.path().parent().unwrap().to_path_buf());
        }
    }
    if skill_dirs.len() != 1 {
        return Err(AppError::invalid_input(
            "ByteDance ZIP must contain exactly one skill directory with SKILL.md",
        ));
    }
    Ok(skill_dirs.into_iter().next().unwrap())
}
