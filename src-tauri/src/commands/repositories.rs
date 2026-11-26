//! A recoverable collection of Git repositories, independent of installed skills.
//!
//! The catalog lives beside sync metadata inside the existing backup repository.
//! Scans only read a checkout; importing missing skills is an explicit operation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;
use walkdir::WalkDir;

use crate::core::{
    auto_backup, central_repo,
    error::AppError,
    git_credentials, git_fetcher, installer, path_guard,
    repo_lock::RepoLock,
    skill_metadata,
    skill_store::{SkillRecord, SkillStore},
    sync_metadata,
};

use super::skills::{git_source_from_skill, store_installed_skill_unlocked, InstallSourceMetadata};

const CATALOG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RepositoryMember {
    rel_path: String,
    name: String,
    description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RepositoryRecord {
    id: String,
    identity: String,
    name: String,
    source_ref: String,
    clone_url: String,
    branch: Option<String>,
    documentation_url: Option<String>,
    added_at: i64,
    updated_at: i64,
    last_scanned_at: Option<i64>,
    snapshot_revision: Option<String>,
    scan_error: Option<String>,
    members: Vec<RepositoryMember>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RepositoryCatalog {
    schema_version: u32,
    repositories: Vec<RepositoryRecord>,
    /// Prevent a removed collection being silently recreated from installed skills.
    #[serde(default)]
    removed_ids: BTreeSet<String>,
}

impl Default for RepositoryCatalog {
    fn default() -> Self {
        Self {
            schema_version: CATALOG_VERSION,
            repositories: Vec::new(),
            removed_ids: BTreeSet::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RepositoryMemberDto {
    /// Always relative to the repository root; `.` denotes a root skill.
    pub rel_path: String,
    pub name: String,
    pub description: Option<String>,
    pub installed_skill_ids: Vec<String>,
    /// Unknown until a successful scan. False preserves locally installed members
    /// which disappeared upstream instead of silently removing them from the group.
    pub upstream_present: Option<bool>,
    /// At least one installed member was imported from a different commit.
    /// This is a revision comparison, not a claim its file contents changed.
    pub is_stale: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepositoryDto {
    pub id: String,
    pub identity: String,
    pub name: String,
    pub source_ref: String,
    pub clone_url: String,
    pub branch: Option<String>,
    pub documentation_url: Option<String>,
    pub added_at: i64,
    pub updated_at: i64,
    pub last_scanned_at: Option<i64>,
    pub snapshot_revision: Option<String>,
    pub scan_error: Option<String>,
    pub members: Vec<RepositoryMemberDto>,
    pub skill_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RepositoryImportResult {
    pub repository: RepositoryDto,
    pub imported_skill_ids: Vec<String>,
    pub skipped_count: usize,
}

fn catalog_path() -> PathBuf {
    sync_metadata::metadata_dir().join("repositories.json")
}

fn read_catalog(path: &Path) -> Result<RepositoryCatalog, AppError> {
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RepositoryCatalog::default());
        }
        Err(error) => return Err(AppError::io(error)),
    };
    let catalog: RepositoryCatalog = serde_json::from_slice(&raw).map_err(AppError::io)?;
    if catalog.schema_version != CATALOG_VERSION {
        return Err(AppError::invalid_input(
            "Repository catalog version is unsupported; update the app before changing it",
        ));
    }
    Ok(catalog)
}

fn write_catalog(catalog: &RepositoryCatalog) -> Result<(), AppError> {
    sync_metadata::atomic_write_json(&catalog_path(), catalog).map_err(AppError::io)?;
    auto_backup::notify_central_change();
    Ok(())
}

/// Keep transport choices out of identity, matching the existing repository UI.
/// Refs are deliberately included: main, a release tag, and the default ref
/// remain separate recoverable collections.
fn repository_identity(clone_url: &str, branch: Option<&str>) -> Result<String, AppError> {
    let trimmed = clone_url.trim().trim_end_matches('/');
    let (host, path) = if trimmed.starts_with("git@") && !trimmed.contains("://") {
        let (host, path) = trimmed[4..]
            .split_once(':')
            .ok_or_else(|| AppError::invalid_input("Invalid SSH repository URL"))?;
        (host.to_string(), path.to_string())
    } else {
        let url = reqwest::Url::parse(trimmed)
            .map_err(|_| AppError::invalid_input("Invalid repository URL"))?;
        if !matches!(url.scheme(), "http" | "https" | "ssh")
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(AppError::invalid_input(
                "Use a Git clone URL without a query or fragment",
            ));
        }
        let host = url
            .host_str()
            .ok_or_else(|| AppError::invalid_input("Repository URL is missing a host"))?;
        // A custom SSH/HTTP port may identify a different service.
        let host = match url.port() {
            Some(port) if !(url.scheme() == "ssh" && port == 22) => format!("{host}:{port}"),
            _ => host.to_string(),
        };
        (host, url.path().to_string())
    };
    let path = path.trim_matches('/');
    let path = if path.to_ascii_lowercase().ends_with(".git") {
        &path[..path.len() - 4]
    } else {
        path
    };
    if path.is_empty() || path.split('/').any(|part| matches!(part, "." | ".." | "")) {
        return Err(AppError::invalid_input(
            "Repository URL is missing a valid path",
        ));
    }
    Ok(format!(
        "repo:{}/{}#ref={}",
        host.to_lowercase(),
        path.to_lowercase(),
        urlencoding::encode(branch.unwrap_or_default())
    ))
}

fn identity_id(identity: &str) -> String {
    format!("{:x}", Sha256::digest(identity.as_bytes()))[..24].to_string()
}

fn repository_record(
    source_ref: &str,
    clone_url: &str,
    branch: Option<String>,
) -> Result<RepositoryRecord, AppError> {
    git_fetcher::validate_git_url(clone_url)
        .map_err(|error| AppError::invalid_input(error.to_string()))?;
    if git_credentials::split_credentials_from_url(clone_url).is_some()
        || git_credentials::split_credentials_from_url(source_ref).is_some()
    {
        return Err(AppError::invalid_input(
            "Use a repository URL without embedded credentials",
        ));
    }
    let branch = branch
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if branch.as_deref().is_some_and(|value| {
        value
            .chars()
            .any(|character| matches!(character, '\n' | '\r' | '\0'))
    }) {
        return Err(AppError::invalid_input("Invalid repository ref"));
    }
    let identity = repository_identity(clone_url, branch.as_deref())?;
    let repository_path = identity
        .trim_start_matches("repo:")
        .split("#ref=")
        .next()
        .unwrap_or(&identity);
    let name = repository_path
        .strip_prefix("github.com/")
        .unwrap_or(repository_path)
        .to_string();
    let now = chrono::Utc::now().timestamp_millis();
    Ok(RepositoryRecord {
        id: identity_id(&identity),
        identity,
        name,
        source_ref: source_ref.trim().to_string(),
        clone_url: clone_url.trim().trim_end_matches('/').to_string(),
        branch,
        documentation_url: None,
        added_at: now,
        updated_at: now,
        last_scanned_at: None,
        snapshot_revision: None,
        scan_error: None,
        members: Vec::new(),
    })
}

fn record_from_skill(skill: &SkillRecord) -> Option<RepositoryRecord> {
    if !matches!(skill.source_type.as_str(), "git" | "skillssh") {
        return None;
    }
    let source = git_source_from_skill(skill).ok()?;
    // The catalog is repository-wide even when a skill's original URL names a subpath.
    repository_record(&source.clone_url, &source.clone_url, source.branch).ok()
}

fn member_path(skill: &SkillRecord) -> String {
    git_source_from_skill(skill)
        .ok()
        .and_then(|source| source.subpath)
        .map(|path| {
            path.replace('\\', "/")
                .trim_matches('/')
                .trim_start_matches("./")
                .to_string()
        })
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| ".".to_string())
}

/// Caller holds RepoLock. Read the tracked file every time, so backup restore
/// and sync are immediately reflected without a second SQLite source of truth.
fn load_catalog_unlocked(store: &SkillStore) -> Result<RepositoryCatalog, AppError> {
    let mut catalog = read_catalog(&catalog_path())?;
    let mut changed = false;
    for skill in store.get_all_skills().map_err(AppError::db)? {
        let Some(record) = record_from_skill(&skill) else {
            continue;
        };
        if !catalog.removed_ids.contains(&record.id)
            && !catalog
                .repositories
                .iter()
                .any(|existing| existing.id == record.id)
        {
            catalog.repositories.push(record);
            changed = true;
        }
    }
    if changed {
        catalog
            .repositories
            .sort_by(|a, b| a.identity.cmp(&b.identity));
        write_catalog(&catalog)?;
    }
    Ok(catalog)
}

type InstalledRepositoryIndex<'a> = BTreeMap<String, Vec<(&'a SkillRecord, String)>>;

fn index_installed(skills: &[SkillRecord]) -> InstalledRepositoryIndex<'_> {
    let mut index = BTreeMap::<String, Vec<(&SkillRecord, String)>>::new();
    for skill in skills {
        if let Some(record) = record_from_skill(skill) {
            index
                .entry(record.id)
                .or_default()
                .push((skill, member_path(skill)));
        }
    }
    index
}

fn to_dto(record: &RepositoryRecord, index: &InstalledRepositoryIndex<'_>) -> RepositoryDto {
    let mut members: BTreeMap<String, RepositoryMemberDto> = record
        .members
        .iter()
        .map(|member| {
            (
                member.rel_path.clone(),
                RepositoryMemberDto {
                    rel_path: member.rel_path.clone(),
                    name: member.name.clone(),
                    description: member.description.clone(),
                    installed_skill_ids: Vec::new(),
                    upstream_present: Some(true),
                    is_stale: false,
                },
            )
        })
        .collect();
    let mut skill_ids = Vec::new();
    for (skill, rel_path) in index.get(&record.id).into_iter().flatten() {
        skill_ids.push(skill.id.clone());
        let member = members
            .entry(rel_path.clone())
            .or_insert_with(|| RepositoryMemberDto {
                rel_path: rel_path.clone(),
                name: skill.name.clone(),
                description: skill.description.clone(),
                installed_skill_ids: Vec::new(),
                upstream_present: record.last_scanned_at.map(|_| false),
                is_stale: false,
            });
        member.installed_skill_ids.push(skill.id.clone());
        member.is_stale |= record.snapshot_revision.as_ref().is_some_and(|revision| {
            skill
                .source_revision
                .as_ref()
                .is_some_and(|installed| installed != revision)
        });
    }
    skill_ids.sort();
    for member in members.values_mut() {
        member.installed_skill_ids.sort();
    }
    RepositoryDto {
        id: record.id.clone(),
        identity: record.identity.clone(),
        name: record.name.clone(),
        source_ref: record.source_ref.clone(),
        clone_url: record.clone_url.clone(),
        branch: record.branch.clone(),
        documentation_url: record.documentation_url.clone(),
        added_at: record.added_at,
        updated_at: record.updated_at,
        last_scanned_at: record.last_scanned_at,
        snapshot_revision: record.snapshot_revision.clone(),
        scan_error: record.scan_error.clone(),
        members: members.into_values().collect(),
        skill_ids,
    }
}

fn selected_record(catalog: &RepositoryCatalog, id: &str) -> Result<RepositoryRecord, AppError> {
    catalog
        .repositories
        .iter()
        .find(|record| record.id == id)
        .cloned()
        .ok_or_else(|| AppError::not_found("Repository is no longer in the collection"))
}

#[tauri::command]
pub async fn list_repositories(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<RepositoryDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _lock =
            RepoLock::acquire_foreground("list repository collection").map_err(AppError::db)?;
        let catalog = load_catalog_unlocked(&store)?;
        let skills = store.get_all_skills().map_err(AppError::db)?;
        let index = index_installed(&skills);
        Ok(catalog
            .repositories
            .iter()
            .map(|record| to_dto(record, &index))
            .collect())
    })
    .await?
}

#[tauri::command]
pub async fn save_repository(
    repo_url: String,
    documentation_url: Option<String>,
    branch: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<RepositoryDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let parsed = git_fetcher::parse_git_source(&repo_url);
        let mut record = repository_record(&repo_url, &parsed.clone_url, branch.or(parsed.branch))?;
        let update_documentation = documentation_url.is_some();
        let documentation_url = documentation_url
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if let Some(raw) = &documentation_url {
            let url = reqwest::Url::parse(raw)
                .map_err(|_| AppError::invalid_input("Invalid documentation URL"))?;
            if !matches!(url.scheme(), "http" | "https")
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(AppError::invalid_input(
                    "Documentation URL must be an HTTP(S) URL without credentials",
                ));
            }
        }
        let _lock =
            RepoLock::acquire_foreground("save repository collection").map_err(AppError::db)?;
        let mut catalog = load_catalog_unlocked(&store)?;
        if let Some(existing) = catalog
            .repositories
            .iter_mut()
            .find(|existing| existing.id == record.id)
        {
            if update_documentation {
                existing.documentation_url = documentation_url;
            }
            existing.updated_at = chrono::Utc::now().timestamp_millis();
            record = existing.clone();
        } else {
            record.documentation_url = documentation_url;
            catalog.repositories.push(record.clone());
        }
        catalog.removed_ids.remove(&record.id);
        catalog
            .repositories
            .sort_by(|a, b| a.identity.cmp(&b.identity));
        write_catalog(&catalog)?;
        Ok(to_dto(
            &record,
            &index_installed(&store.get_all_skills().map_err(AppError::db)?),
        ))
    })
    .await?
}

#[tauri::command]
pub async fn remove_repository(
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _lock =
            RepoLock::acquire_foreground("remove repository collection").map_err(AppError::db)?;
        let mut catalog = load_catalog_unlocked(&store)?;
        catalog.repositories.retain(|record| record.id != id);
        catalog.removed_ids.insert(id);
        write_catalog(&catalog)
    })
    .await?
}

/// RAII cleanup also covers errors while parsing or importing a checkout.
struct Checkout(PathBuf);
impl Drop for Checkout {
    fn drop(&mut self) {
        git_fetcher::cleanup_temp(&self.0);
    }
}

fn fetch_repository(record: &RepositoryRecord, store: &SkillStore) -> Result<Checkout, AppError> {
    // Revalidate restored metadata before it can reach Git.
    let validated =
        repository_record(&record.source_ref, &record.clone_url, record.branch.clone())?;
    if validated.id != record.id || validated.identity != record.identity {
        return Err(AppError::invalid_input(
            "Repository catalog identity does not match its source",
        ));
    }
    git_fetcher::clone_repo_ref_scoped(
        &record.clone_url,
        record.branch.as_deref(),
        None,
        None,
        store.proxy_url().as_deref(),
        None,
    )
    .map(Checkout)
    .map_err(AppError::classify_git_error)
}

/// Ponytail's INSTALL.md documents `skills/` as its portable payload. Its
/// `.claude`/other native plugin copies are alternate host payloads, not more
/// skills to bulk-import. Keep this exception tied to the exact repository.
fn portable_scope(record: &RepositoryRecord) -> Option<&'static str> {
    (record.identity.split("#ref=").next() == Some("repo:github.com/dietrichgebert/ponytail"))
        .then_some("skills")
}

fn scan_checkout(
    root: &Path,
    scope: Option<&str>,
) -> Result<(String, Vec<RepositoryMember>), AppError> {
    let revision = git_fetcher::get_head_revision(root).map_err(AppError::git)?;
    let mut members = Vec::new();
    let scan_root = scope
        .map(|scope| root.join(scope))
        .unwrap_or_else(|| root.to_path_buf());
    if !scan_root.exists() {
        return Ok((revision, members));
    }
    if !path_guard::is_path_safe(root, &scan_root) {
        return Err(AppError::invalid_input(
            "Repository scan scope resolves outside the checkout",
        ));
    }
    let mut walker = WalkDir::new(scan_root).follow_links(false).into_iter();
    while let Some(entry) = walker.next() {
        let entry = entry.map_err(AppError::io)?;
        if !entry.file_type().is_dir() {
            continue;
        }
        if entry.depth() > 0
            && matches!(
                entry.file_name().to_str(),
                Some(".git" | ".skills-manager" | "node_modules" | ".hub")
            )
        {
            walker.skip_current_dir();
            continue;
        }
        let dir = entry.path();
        // Never parse a skill marker that redirects outside the checkout.
        let markers = ["SKILL.md", "skill.md"].map(|name| dir.join(name));
        let has_marker = markers.iter().any(|marker| {
            marker
                .symlink_metadata()
                .is_ok_and(|meta| meta.file_type().is_file())
                && path_guard::is_path_safe(root, marker)
        });
        let unsafe_marker = markers.iter().any(|marker| {
            marker
                .symlink_metadata()
                .is_ok_and(|meta| !meta.file_type().is_file())
                || (marker.exists() && !path_guard::is_path_safe(root, marker))
        });
        if !has_marker || unsafe_marker {
            continue;
        }
        let meta = skill_metadata::parse_skill_md(dir);
        let rel_path = git_fetcher::relative_subpath(root, dir).unwrap_or_else(|| ".".to_string());
        let name = meta
            .name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| {
                if rel_path == "." {
                    "root-skill".to_string()
                } else {
                    dir.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()
                }
            });
        members.push(RepositoryMember {
            rel_path,
            name,
            description: meta.description,
        });
        walker.skip_current_dir();
    }
    members.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok((revision, members))
}

fn apply_snapshot(record: &mut RepositoryRecord, revision: String, members: Vec<RepositoryMember>) {
    let now = chrono::Utc::now().timestamp_millis();
    record.members = members;
    record.snapshot_revision = Some(revision);
    record.last_scanned_at = Some(now);
    record.updated_at = now;
    record.scan_error = None;
}

fn get_record(store: &SkillStore, id: &str) -> Result<RepositoryRecord, AppError> {
    let _lock = RepoLock::acquire_foreground("read repository collection").map_err(AppError::db)?;
    selected_record(&load_catalog_unlocked(store)?, id)
}

fn record_scan_error(store: &SkillStore, id: &str, error: &AppError) {
    let result = (|| {
        let _lock =
            RepoLock::acquire_foreground("record repository scan error").map_err(AppError::db)?;
        let mut catalog = load_catalog_unlocked(store)?;
        if let Some(record) = catalog
            .repositories
            .iter_mut()
            .find(|record| record.id == id)
        {
            record.scan_error = Some(error.message.clone());
            write_catalog(&catalog)?;
        }
        Ok::<_, AppError>(())
    })();
    if let Err(error) = result {
        log::warn!("Could not record repository scan failure: {error}");
    }
}

#[tauri::command]
pub async fn scan_repository(
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<RepositoryDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let record = get_record(&store, &id)?;
        let scan = fetch_repository(&record, &store)
            .and_then(|checkout| scan_checkout(&checkout.0, portable_scope(&record)));
        let (revision, members) = match scan {
            Ok(scan) => scan,
            Err(error) => {
                record_scan_error(&store, &id, &error);
                return Err(error);
            }
        };
        let _lock =
            RepoLock::acquire_foreground("save repository snapshot").map_err(AppError::db)?;
        let mut catalog = load_catalog_unlocked(&store)?;
        let current = catalog
            .repositories
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or_else(|| AppError::not_found("Repository was removed while scanning"))?;
        apply_snapshot(current, revision, members);
        let dto = to_dto(
            current,
            &index_installed(&store.get_all_skills().map_err(AppError::db)?),
        );
        write_catalog(&catalog)?;
        Ok(dto)
    })
    .await?
}

/// Must be called while holding RepoLock. A different source with identical
/// contents is still a separate skill: never reuse its existing destination.
fn unused_destination(store: &SkillStore, name: &str) -> Result<(String, PathBuf), AppError> {
    let base =
        skill_metadata::sanitize_skill_name(name).unwrap_or_else(|| "repository-skill".to_string());
    for index in 1u32.. {
        let name = if index == 1 {
            base.clone()
        } else {
            format!("{base}-{index}")
        };
        let path = central_repo::skills_dir().join(&name);
        match path.symlink_metadata() {
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if store
                    .get_skill_by_central_path(&path.to_string_lossy())
                    .map_err(AppError::db)?
                    .is_none()
                {
                    return Ok((name, path));
                }
            }
            Err(error) => return Err(AppError::io(error)),
        }
    }
    Err(AppError::invalid_input(
        "Could not choose an unused skill name",
    ))
}

fn import_missing_unlocked(
    store: &SkillStore,
    record: &RepositoryRecord,
    root: &Path,
    revision: &str,
    members: &[RepositoryMember],
) -> Result<(Vec<String>, usize), AppError> {
    let existing = store.get_all_skills().map_err(AppError::db)?;
    let mut installed_paths: BTreeSet<String> = existing
        .iter()
        .filter(|skill| record_from_skill(skill).is_some_and(|source| source.id == record.id))
        .map(member_path)
        .collect();
    let mut imported = Vec::new();
    let mut skipped = 0;
    for member in members {
        if installed_paths.contains(&member.rel_path) {
            skipped += 1;
            continue;
        }
        let dir = if member.rel_path == "." {
            root.to_path_buf()
        } else {
            root.join(&member.rel_path)
        };
        if !path_guard::is_path_safe(root, &dir) {
            return Err(AppError::invalid_input(
                "Skill path resolves outside the repository",
            ));
        }
        let (name, destination) = unused_destination(store, &member.name)?;
        let result = installer::install_skill_dir_to_destination(&dir, &name, &destination)
            .map_err(AppError::io)?;
        let metadata = InstallSourceMetadata {
            source_type: "git".to_string(),
            source_ref: Some(record.clone_url.clone()),
            source_ref_resolved: Some(record.clone_url.clone()),
            source_subpath: (member.rel_path != ".").then(|| member.rel_path.clone()),
            source_branch: record.branch.clone(),
            source_revision: Some(revision.to_string()),
            remote_revision: Some(revision.to_string()),
            update_status: "up_to_date".to_string(),
        };
        imported.push(store_installed_skill_unlocked(
            store, &result, &metadata, None,
        )?);
        installed_paths.insert(member.rel_path.clone());
    }
    Ok((imported, skipped))
}

fn select_members(
    members: &[RepositoryMember],
    paths: Option<&[String]>,
) -> Result<Vec<RepositoryMember>, AppError> {
    let Some(paths) = paths else {
        return Ok(members.to_vec());
    };
    let paths: BTreeSet<&str> = paths.iter().map(String::as_str).collect();
    if paths
        .iter()
        .any(|path| !members.iter().any(|member| member.rel_path == *path))
    {
        return Err(AppError::invalid_input(
            "A selected skill path was not found in the repository snapshot; scan it again",
        ));
    }
    Ok(members
        .iter()
        .filter(|member| paths.contains(member.rel_path.as_str()))
        .cloned()
        .collect())
}

#[tauri::command]
pub async fn import_repository_skills(
    id: String,
    member_paths: Option<Vec<String>>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<RepositoryImportResult, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let record = get_record(&store, &id)?;
        let checkout = match fetch_repository(&record, &store) {
            Ok(checkout) => checkout,
            Err(error) => {
                record_scan_error(&store, &id, &error);
                return Err(error);
            }
        };
        let (revision, members) = scan_checkout(&checkout.0, portable_scope(&record))?;
        let selected_members = select_members(&members, member_paths.as_deref())?;
        let _lock = RepoLock::acquire_foreground("import missing repository skills")
            .map_err(AppError::db)?;
        let mut catalog = load_catalog_unlocked(&store)?;
        let mut current = selected_record(&catalog, &id)?;
        // Discovery and every imported member come from one checkout revision.
        apply_snapshot(&mut current, revision.clone(), members);
        let (imported_skill_ids, skipped_count) =
            import_missing_unlocked(&store, &current, &checkout.0, &revision, &selected_members)?;
        let repository = to_dto(
            &current,
            &index_installed(&store.get_all_skills().map_err(AppError::db)?),
        );
        if let Some(record) = catalog
            .repositories
            .iter_mut()
            .find(|record| record.id == id)
        {
            *record = current;
        }
        write_catalog(&catalog)?;
        Ok(RepositoryImportResult {
            repository,
            imported_skill_ids,
            skipped_count,
        })
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ResetBaseDir;
    impl Drop for ResetBaseDir {
        fn drop(&mut self) {
            central_repo::set_test_base_dir_override(None);
        }
    }

    fn committed_checkout(root: &Path, files: &[(&str, &str)]) -> String {
        let repo = git2::Repository::init(root).unwrap();
        for (path, contents) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("Catalog test", "catalog@example.com").unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .unwrap()
            .to_string()
    }

    #[test]
    fn repository_identity_normalizes_transport_suffix_case_and_ref() {
        let https =
            repository_identity("https://github.com/Owner/Repo.git/", Some("feature/x")).unwrap();
        assert_eq!(https, "repo:github.com/owner/repo#ref=feature%2Fx");
        assert_eq!(
            https,
            repository_identity("git@github.com:owner/repo", Some("feature/x")).unwrap()
        );
        assert_eq!(
            https,
            repository_identity("ssh://git@GITHUB.com/owner/repo.git", Some("feature/x")).unwrap()
        );
        assert_ne!(
            https,
            repository_identity("https://github.com/owner/repo", Some("main")).unwrap()
        );
        assert_ne!(
            repository_identity("ssh://git@host:2222/o/r", None).unwrap(),
            repository_identity("ssh://git@host/o/r", None).unwrap()
        );
    }

    #[test]
    fn invalid_or_secret_sources_are_rejected() {
        assert!(repository_record(
            "https://user:token@example.com/a/b",
            "https://user:token@example.com/a/b",
            None
        )
        .is_err());
        assert!(repository_record("file:///tmp/repo", "file:///tmp/repo", None).is_err());
        assert!(repository_identity("https://example.com/o/r?ref=main", None).is_err());
    }

    #[test]
    fn catalog_file_preserves_empty_repositories_and_removals() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("catalog.json");
        let mut catalog = RepositoryCatalog::default();
        let record =
            repository_record("owner/repo", "https://github.com/owner/repo.git", None).unwrap();
        catalog.repositories.push(record);
        catalog.removed_ids.insert("removed".to_string());
        sync_metadata::atomic_write_json(&path, &catalog).unwrap();
        let loaded = read_catalog(&path).unwrap();
        assert_eq!(loaded.repositories.len(), 1);
        assert!(loaded.repositories[0].members.is_empty());
        assert!(loaded.removed_ids.contains("removed"));
        std::fs::write(&path, "{invalid").unwrap();
        assert!(
            read_catalog(&path).is_err(),
            "corrupt catalog must not be silently overwritten"
        );
    }

    #[test]
    fn scan_accepts_repository_without_skills_and_preserves_repository_paths() {
        let empty = tempfile::tempdir().unwrap();
        let revision = committed_checkout(
            empty.path(),
            &[("README.md", "A repository without SKILL.md\n")],
        );
        let (observed_revision, members) = scan_checkout(empty.path(), None).unwrap();
        assert_eq!(observed_revision, revision);
        assert!(members.is_empty());

        let root_skill = tempfile::tempdir().unwrap();
        committed_checkout(
            root_skill.path(),
            &[("SKILL.md", "---\nname: root\n---\nRoot skill\n")],
        );
        let (_, members) = scan_checkout(root_skill.path(), None).unwrap();
        assert_eq!(members[0].rel_path, ".");

        let checkout = tempfile::tempdir().unwrap();
        committed_checkout(
            checkout.path(),
            &[
                (
                    "skills/example/SKILL.md",
                    "---\nname: example\n---\nPortable\n",
                ),
                (
                    ".claude/skills/example/SKILL.md",
                    "---\nname: example\n---\nNative alternative\n",
                ),
            ],
        );
        let record = repository_record(
            "DietrichGebert/ponytail",
            "https://github.com/DietrichGebert/ponytail.git",
            None,
        )
        .unwrap();
        let (_, portable) = scan_checkout(checkout.path(), portable_scope(&record)).unwrap();
        assert_eq!(portable.len(), 1);
        assert_eq!(portable[0].rel_path, "skills/example");
        let fork = repository_record(
            "someone/ponytail",
            "https://github.com/someone/ponytail.git",
            None,
        )
        .unwrap();
        let (_, all) = scan_checkout(checkout.path(), portable_scope(&fork)).unwrap();
        assert_eq!(
            all.len(),
            2,
            "The portable exception must not match another repository"
        );
        assert_eq!(
            select_members(&all, Some(&["skills/example".into()]))
                .unwrap()
                .len(),
            1
        );
        assert!(select_members(&all, Some(&["../outside".into()])).is_err());
        assert!(select_members(&all, Some(&[])).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn scan_does_not_follow_external_skill_links_or_marker_links() {
        let checkout = tempfile::tempdir().unwrap();
        committed_checkout(checkout.path(), &[("README.md", "No skills\n")]);
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(
            outside.path().join("SKILL.md"),
            "---\nname: outside\n---\nPrivate\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), checkout.path().join("linked-dir")).unwrap();
        let marker_dir = checkout.path().join("linked-marker");
        std::fs::create_dir(&marker_dir).unwrap();
        std::os::unix::fs::symlink(outside.path().join("SKILL.md"), marker_dir.join("SKILL.md"))
            .unwrap();
        std::fs::write(
            marker_dir.join("skill.md"),
            "---\nname: alternate\n---\nSafe but shadowed\n",
        )
        .unwrap();
        let (_, members) = scan_checkout(checkout.path(), None).unwrap();
        assert!(members.is_empty());
        assert!(scan_checkout(checkout.path(), Some("linked-dir")).is_err());
    }

    #[test]
    fn missing_import_is_idempotent_and_preserves_unrelated_skills() {
        let _guard = central_repo::test_base_dir_lock();
        let temp = tempfile::tempdir().unwrap();
        central_repo::set_test_base_dir_override(Some(temp.path().to_path_buf()));
        let _reset = ResetBaseDir;
        let result = (|| {
            let store = SkillStore::new(&temp.path().join("skills.db")).unwrap();
            let checkout = temp.path().join("checkout");
            let dir = checkout.join("skills/one");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("SKILL.md"),
                "---\nname: shared-name\ndescription: Example\n---\nOriginal\n",
            )
            .unwrap();
            // An unrelated skill with byte-identical content must retain its source.
            let first = installer::install_from_local(&dir, Some("shared-name")).unwrap();
            let meta = InstallSourceMetadata {
                source_type: "local".into(),
                source_ref: Some(dir.to_string_lossy().into()),
                source_ref_resolved: None,
                source_subpath: None,
                source_branch: None,
                source_revision: None,
                remote_revision: None,
                update_status: "unknown".into(),
            };
            let original_id = store_installed_skill_unlocked(&store, &first, &meta, None).unwrap();
            let mut record = repository_record(
                "owner/repo",
                "https://github.com/owner/repo.git",
                Some("main".into()),
            )
            .unwrap();
            let members = vec![RepositoryMember {
                rel_path: "skills/one".into(),
                name: "shared-name".into(),
                description: None,
            }];
            let (imported, skipped) =
                import_missing_unlocked(&store, &record, &checkout, "commit-one", &members)
                    .unwrap();
            assert_eq!((imported.len(), skipped), (1, 0));
            assert_eq!(
                store
                    .get_skill_by_id(&original_id)
                    .unwrap()
                    .unwrap()
                    .source_type,
                "local"
            );
            assert_eq!(
                store
                    .get_skill_by_id(&imported[0])
                    .unwrap()
                    .unwrap()
                    .source_subpath
                    .as_deref(),
                Some("skills/one")
            );
            assert_eq!(
                store.get_skill_by_id(&imported[0]).unwrap().unwrap().name,
                "shared-name-2"
            );
            let (again, skipped) =
                import_missing_unlocked(&store, &record, &checkout, "commit-two", &members)
                    .unwrap();
            assert!(again.is_empty());
            assert_eq!(skipped, 1);
            assert_eq!(
                store
                    .get_skill_by_id(&imported[0])
                    .unwrap()
                    .unwrap()
                    .source_revision
                    .as_deref(),
                Some("commit-one")
            );
            // A removed upstream path stays visible and installed, with revision drift.
            apply_snapshot(&mut record, "commit-two".into(), Vec::new());
            let dto = to_dto(&record, &index_installed(&store.get_all_skills().unwrap()));
            assert_eq!(dto.members[0].upstream_present, Some(false));
            assert!(dto.members[0].is_stale);
            assert_eq!(dto.skill_ids, imported);
            let mut catalog = load_catalog_unlocked(&store).unwrap();
            assert_eq!(
                catalog.repositories.len(),
                1,
                "Installed Git sources seed the catalog"
            );
            catalog.repositories.clear();
            catalog.removed_ids.insert(record.id);
            write_catalog(&catalog).unwrap();
            sync_metadata::write_all_from_db_unlocked(&store).unwrap();
            assert!(
                load_catalog_unlocked(&store)
                    .unwrap()
                    .repositories
                    .is_empty(),
                "Removing a collection persists while installed skills remain"
            );
        })();
        central_repo::set_test_base_dir_override(None);
        result
    }
}
