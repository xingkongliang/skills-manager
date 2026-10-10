use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::State;
use walkdir::WalkDir;

use crate::core::{
    audit_log::AuditDraft,
    central_repo,
    error::AppError,
    git_fetcher,
    install_cancel::InstallCancelRegistry,
    installer, path_guard,
    repo_lock::RepoLock,
    scanner,
    skill_metadata::{self, is_valid_skill_dir},
    skill_store::{SkillRecord, SkillStore, SkillTargetRecord},
    sync_engine, sync_metadata,
    timing::should_log_first_or_slow,
};

#[derive(Debug, Serialize)]
pub struct UpdateSkillResult {
    pub skill: ManagedSkillDto,
    /// Whether the skill's file content actually changed.
    /// False when a monorepo commit didn't touch this skill's subdirectory.
    pub content_changed: bool,
    /// What the update would remove, when it declined because of it (#256).
    /// Non-empty means **nothing was changed**: show these and call again with
    /// `approved_removals` set to `removal_approval` if the user accepts.
    ///
    /// Empty on every ordinary update, including approved ones.
    pub pending_removals: Vec<PendingRemoval>,
    /// Identifies exactly what `pending_removals` describes. Passing it back
    /// approves *that* list against *that* revision and nothing else — if the
    /// remote moves on, or the skill writes another file while the dialog is
    /// open, the approval no longer matches and the user is asked again.
    pub removal_approval: Option<String>,
}

/// Stands in for a revision when binding a re-import's approval: there is no
/// remote to move on, but the removal set still has to be bound.
const REIMPORT_APPROVAL_DOMAIN: &str = "reimport";

/// Result of re-importing a local skill from its source path.
#[derive(Debug, Serialize)]
pub struct ReimportSkillResult {
    pub skill: ManagedSkillDto,
    /// Non-empty means **nothing was changed** — see [`UpdateSkillResult`].
    pub pending_removals: Vec<PendingRemoval>,
    /// Approves exactly `pending_removals` — see [`UpdateSkillResult`].
    pub removal_approval: Option<String>,
}

/// Where a path about to be removed lives.
#[derive(Debug, Clone, Serialize)]
pub struct PendingRemoval {
    /// [`LIBRARY_LOCATION`], or the key of the agent whose deployed copy holds
    /// it. The user needs to know which directory to go and rescue.
    pub location: String,
    pub path: String,
}

/// `PendingRemoval::location` for the central library, as opposed to an agent's
/// deployed copy.
pub const LIBRARY_LOCATION: &str = "library";

enum UpdateOutcome {
    Applied {
        content_changed: bool,
    },
    /// Declined, having changed nothing.
    Held {
        pending: Vec<PendingRemoval>,
        approval: String,
    },
}

/// Everything a replacement would take away — from the library and from every
/// copy-mode deployment of this skill.
///
/// `staged` is the tree about to be installed, or `None` when the library keeps
/// what it already has. Even then the deployments are torn down and rebuilt from
/// it, which loses files just as effectively, so they are always checked.
///
/// Compared against the *staged* tree rather than the source it came from: the
/// installer drops `.git` and every symlink, so anything else would report a
/// path as surviving that the swap goes on to remove.
pub(crate) fn pending_removals_for(
    store: &SkillStore,
    skill: &SkillRecord,
    staged: Option<&Path>,
) -> Result<Vec<PendingRemoval>, AppError> {
    let library = Path::new(&skill.central_path);
    let mut pending = Vec::new();

    if let Some(staged) = staged {
        for path in crate::core::removals::removed_paths(library, staged).map_err(AppError::io)? {
            pending.push(PendingRemoval {
                location: LIBRARY_LOCATION.to_string(),
                path,
            });
        }
    }

    let effective_new = staged.unwrap_or(library);
    for target in store
        .get_targets_for_skill(&skill.id)
        .map_err(AppError::db)?
    {
        if target.mode != "copy" {
            continue;
        }
        for path in
            crate::core::removals::removed_paths(Path::new(&target.target_path), effective_new)
                .map_err(AppError::io)?
        {
            pending.push(PendingRemoval {
                location: target.tool.clone(),
                path,
            });
        }
    }
    Ok(pending)
}

/// A stable name for one exact set of removals at one exact revision.
fn removal_approval_token(revision: &str, pending: &[PendingRemoval]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(revision.as_bytes());
    let mut rows: Vec<String> = pending
        .iter()
        .map(|p| format!("{}\u{0}{}", p.location, p.path))
        .collect();
    rows.sort();
    for row in rows {
        hasher.update(row.as_bytes());
        hasher.update([0]);
    }
    hex::encode(hasher.finalize())
}

/// Removes a staged directory unless the swap claimed it.
struct StagedPathGuard<'a> {
    path: &'a Path,
    armed: std::cell::Cell<bool>,
}

impl<'a> StagedPathGuard<'a> {
    fn new(path: &'a Path, armed: bool) -> Self {
        Self {
            path,
            armed: std::cell::Cell::new(armed),
        }
    }

    /// The swap has taken ownership of it; there is nothing left to clean.
    fn release(&self) {
        self.armed.set(false);
    }
}

impl Drop for StagedPathGuard<'_> {
    fn drop(&mut self) {
        if self.armed.get() {
            // Declining an update must leave nothing behind — a stray
            // `.name.staged-<uuid>` inside the library is picked up by the
            // metadata rebuild scan as a skill of its own.
            let _ = remove_path_if_exists(self.path);
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BatchUpdateSkillsResult {
    pub refreshed: usize,
    pub unchanged: usize,
    pub failed: Vec<String>,
    /// Skills left alone because updating would have removed files the new
    /// version does not have. Named so the user can go and look, rather than
    /// wondering why the badge did not clear.
    pub held_back: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchDeleteSkillsResult {
    pub deleted: usize,
    pub failed: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ManagedSkillDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub source_type: String,
    pub source_ref: Option<String>,
    pub source_ref_resolved: Option<String>,
    pub source_subpath: Option<String>,
    pub source_branch: Option<String>,
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
    pub preset_ids: Vec<String>,
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

#[derive(Debug, Serialize)]
pub struct SourceSkillDocumentDto {
    pub skill_id: String,
    pub filename: String,
    pub content: String,
    pub source_label: String,
    pub revision: String,
}

/// Whole-directory diff between the central copy (`original`) and the source
/// (`updated`), covering the same file scope that drives the update badge so
/// the diff can never come back empty while the badge says "update available".
#[derive(Debug, Serialize)]
pub struct SkillSourceDiffDto {
    pub skill_id: String,
    pub source_label: String,
    pub revision: String,
    pub entries: Vec<SkillSourceDiffEntryDto>,
}

#[derive(Debug, Serialize, Clone)]
pub struct SkillSourceDiffEntryDto {
    pub relative_path: String,
    /// "added" | "removed" | "modified"
    pub status: String,
    /// "text" | "binary" | "too_large" | "permission_only"
    pub content_kind: String,
    /// Present only when `content_kind == "text"`.
    pub original_text: Option<String>,
    pub updated_text: Option<String>,
    pub executable_before: bool,
    pub executable_after: bool,
}

#[derive(Debug, Clone)]
pub struct InstallSourceMetadata {
    pub source_type: String,
    pub source_ref: Option<String>,
    pub source_ref_resolved: Option<String>,
    pub source_subpath: Option<String>,
    pub source_branch: Option<String>,
    pub source_revision: Option<String>,
    pub remote_revision: Option<String>,
    pub update_status: String,
}

#[derive(Debug, Clone)]
pub struct GitSkillSource {
    pub clone_url: String,
    pub branch: Option<String>,
    pub subpath: Option<String>,
    pub locator_skill_id: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct GitSkillPreview {
    /// Path relative to the resolved scan root, using `/` separators. Stable key.
    pub rel_path: String,
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
    pub rel_path: String,
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

static GET_MANAGED_SKILLS_FIRST_CALL: AtomicBool = AtomicBool::new(true);

#[tauri::command]
pub async fn get_managed_skills(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<ManagedSkillDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let start = Instant::now();
        let skills = store.get_all_skills().map_err(AppError::db)?;
        let all_targets = store.get_all_targets().map_err(AppError::db)?;
        let tags_map = store.get_tags_map().map_err(AppError::db)?;
        let count = skills.len();
        let dtos: Vec<ManagedSkillDto> = skills
            .into_iter()
            .map(|skill| managed_skill_to_dto(&store, skill, &all_targets, &tags_map))
            .collect();
        let elapsed_ms = start.elapsed().as_millis();
        if should_log_first_or_slow(&GET_MANAGED_SKILLS_FIRST_CALL, elapsed_ms, 100) {
            log::info!("get_managed_skills: {count} skills in {elapsed_ms} ms");
        }
        Ok(dtos)
    })
    .await?
}

#[tauri::command]
pub async fn get_skills_for_preset(
    preset_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<ManagedSkillDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skills = store
            .get_skills_for_scenario(&preset_id)
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

        let (filename, content) = read_skill_document_from_dir(Path::new(&skill.central_path))?;

        Ok(SkillDocumentDto {
            skill_id,
            filename,
            content,
            central_path: skill.central_path,
        })
    })
    .await?
}

#[tauri::command]
pub async fn get_source_skill_document(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<SourceSkillDocumentDto, AppError> {
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;

        if matches!(skill.source_type.as_str(), "local" | "import") {
            let source_path = skill.source_ref.as_ref().ok_or_else(|| {
                AppError::not_found("Local skill is missing its original source path")
            })?;
            let source_dir = PathBuf::from(source_path);
            if !source_dir.exists() {
                return Err(AppError::not_found("Original source path no longer exists"));
            }
            let (filename, content) = read_skill_document_from_dir(&source_dir)?;
            return Ok(SourceSkillDocumentDto {
                skill_id,
                filename,
                content,
                source_label: source_label_for_skill(&skill),
                revision: "workspace".to_string(),
            });
        }

        if !matches!(skill.source_type.as_str(), "git" | "skillssh") {
            return Err(AppError::invalid_input(
                "Skill does not support source diff preview",
            ));
        }

        let git_source = git_source_from_skill(&skill)?;
        git_fetcher::validate_git_url(&git_source.clone_url).map_err(AppError::git)?;
        let remote_revision = git_fetcher::resolve_remote_revision(
            &git_source.clone_url,
            git_source.branch.as_deref(),
            proxy_url.as_deref(),
        )
        .map_err(AppError::git)?;

        let temp_dir = git_fetcher::clone_repo_ref_scoped(
            &git_source.clone_url,
            git_source.branch.as_deref(),
            git_source.subpath.as_deref(),
            None,
            proxy_url.as_deref(),
            None,
        )
        .map_err(AppError::classify_git_error)?;

        let result = (|| -> Result<SourceSkillDocumentDto, AppError> {
            git_fetcher::checkout_revision(&temp_dir, &remote_revision).map_err(AppError::git)?;
            let skill_dir = resolve_skill_dir(
                &temp_dir,
                git_source.subpath.as_deref(),
                git_source.locator_skill_id.as_deref(),
            )?;
            let (filename, content) = read_skill_document_from_dir(&skill_dir)?;

            Ok(SourceSkillDocumentDto {
                skill_id,
                filename,
                content,
                source_label: source_label_for_skill(&skill),
                revision: remote_revision,
            })
        })();

        git_fetcher::cleanup_temp(&temp_dir);
        result
    })
    .await?
}

/// Files larger than this are flagged but not sent to the frontend — the
/// line diff is O(n²), so previewing a huge file would hang the UI.
const MAX_DIFF_FILE_BYTES: usize = 256 * 1024;

/// Classify a file's bytes for diffing: oversized and binary files get a
/// summary row instead of a text body.
fn classify_diff_bytes(bytes: Option<Vec<u8>>) -> (&'static str, Option<String>) {
    match bytes {
        Some(b) if b.len() > MAX_DIFF_FILE_BYTES => ("too_large", None),
        Some(b) if b.contains(&0) => ("binary", None),
        Some(b) => match String::from_utf8(b) {
            Ok(text) => ("text", Some(text)),
            Err(_) => ("binary", None),
        },
        None => ("binary", None),
    }
}

/// Diff the whole content scope of two skill directories. `original_dir` is
/// the central copy (old), `updated_dir` is the source (new). Uses the same
/// file enumeration as the hash so it reports exactly what flips the badge.
fn build_source_diff_entries(
    original_dir: &Path,
    updated_dir: &Path,
) -> Vec<SkillSourceDiffEntryDto> {
    use crate::core::content_hash::{self, ContentEntry};
    use std::collections::BTreeMap;

    let index = |dir: &Path| -> BTreeMap<String, ContentEntry> {
        content_hash::list_content_files(dir)
            .into_iter()
            .map(|e| (e.relative_path.clone(), e))
            .collect()
    };
    let original = index(original_dir);
    let updated = index(updated_dir);

    let mut keys: Vec<&String> = original.keys().chain(updated.keys()).collect();
    keys.sort();
    keys.dedup();

    let mut entries = Vec::new();
    for key in keys {
        match (original.get(key), updated.get(key)) {
            (None, Some(u)) => {
                let (kind, text) = classify_diff_bytes(std::fs::read(&u.path).ok());
                entries.push(SkillSourceDiffEntryDto {
                    relative_path: key.clone(),
                    status: "added".into(),
                    content_kind: kind.into(),
                    original_text: None,
                    updated_text: text,
                    executable_before: false,
                    executable_after: u.is_executable(),
                });
            }
            (Some(o), None) => {
                let (kind, text) = classify_diff_bytes(std::fs::read(&o.path).ok());
                entries.push(SkillSourceDiffEntryDto {
                    relative_path: key.clone(),
                    status: "removed".into(),
                    content_kind: kind.into(),
                    original_text: text,
                    updated_text: None,
                    executable_before: o.is_executable(),
                    executable_after: false,
                });
            }
            (Some(o), Some(u)) => {
                let o_bytes = std::fs::read(&o.path).ok();
                let u_bytes = std::fs::read(&u.path).ok();
                let exec_before = o.is_executable();
                let exec_after = u.is_executable();
                let bytes_equal = o_bytes.is_some() && o_bytes == u_bytes;

                if bytes_equal {
                    if exec_before == exec_after {
                        continue; // unchanged — must match the hash's verdict
                    }
                    entries.push(SkillSourceDiffEntryDto {
                        relative_path: key.clone(),
                        status: "modified".into(),
                        content_kind: "permission_only".into(),
                        original_text: None,
                        updated_text: None,
                        executable_before: exec_before,
                        executable_after: exec_after,
                    });
                    continue;
                }

                let (o_kind, o_text) = classify_diff_bytes(o_bytes);
                let (u_kind, u_text) = classify_diff_bytes(u_bytes);
                let (kind, original_text, updated_text) = if o_kind == "text" && u_kind == "text" {
                    ("text", o_text, u_text)
                } else if o_kind == "too_large" || u_kind == "too_large" {
                    ("too_large", None, None)
                } else {
                    ("binary", None, None)
                };
                entries.push(SkillSourceDiffEntryDto {
                    relative_path: key.clone(),
                    status: "modified".into(),
                    content_kind: kind.into(),
                    original_text,
                    updated_text,
                    executable_before: exec_before,
                    executable_after: exec_after,
                });
            }
            (None, None) => {}
        }
    }

    entries
}

#[tauri::command]
pub async fn get_skill_source_diff(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<SkillSourceDiffDto, AppError> {
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;

        let central_dir = PathBuf::from(&skill.central_path);
        let source_label = source_label_for_skill(&skill);

        if matches!(skill.source_type.as_str(), "local" | "import") {
            let source_path = skill.source_ref.as_ref().ok_or_else(|| {
                AppError::not_found("Local skill is missing its original source path")
            })?;
            let source_dir = PathBuf::from(source_path);
            if !source_dir.exists() {
                return Err(AppError::not_found("Original source path no longer exists"));
            }
            let entries = build_source_diff_entries(&central_dir, &source_dir);
            return Ok(SkillSourceDiffDto {
                skill_id,
                source_label,
                revision: "workspace".to_string(),
                entries,
            });
        }

        if !matches!(skill.source_type.as_str(), "git" | "skillssh") {
            return Err(AppError::invalid_input(
                "Skill does not support source diff preview",
            ));
        }

        let git_source = git_source_from_skill(&skill)?;
        git_fetcher::validate_git_url(&git_source.clone_url).map_err(AppError::git)?;
        let remote_revision = git_fetcher::resolve_remote_revision(
            &git_source.clone_url,
            git_source.branch.as_deref(),
            proxy_url.as_deref(),
        )
        .map_err(AppError::git)?;

        let temp_dir = git_fetcher::clone_repo_ref_scoped(
            &git_source.clone_url,
            git_source.branch.as_deref(),
            git_source.subpath.as_deref(),
            None,
            proxy_url.as_deref(),
            None,
        )
        .map_err(AppError::classify_git_error)?;

        let result = (|| -> Result<SkillSourceDiffDto, AppError> {
            git_fetcher::checkout_revision(&temp_dir, &remote_revision).map_err(AppError::git)?;
            let skill_dir = resolve_skill_dir(
                &temp_dir,
                git_source.subpath.as_deref(),
                git_source.locator_skill_id.as_deref(),
            )?;
            let entries = build_source_diff_entries(&central_dir, &skill_dir);
            Ok(SkillSourceDiffDto {
                skill_id,
                source_label,
                revision: remote_revision,
                entries,
            })
        })();

        git_fetcher::cleanup_temp(&temp_dir);
        result
    })
    .await?
}

fn read_skill_document_from_dir(dir: &Path) -> Result<(String, String), AppError> {
    let candidates = [
        "SKILL.md",
        "skill.md",
        "CLAUDE.md",
        "claude.md",
        "README.md",
        "readme.md",
    ];

    for name in &candidates {
        let path = dir.join(name);
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            return Ok((name.to_string(), content));
        }
    }

    for e in WalkDir::new(dir).max_depth(4).into_iter().flatten() {
        let fname = e.file_name().to_string_lossy();
        if candidates.contains(&fname.as_ref()) {
            let content = std::fs::read_to_string(e.path())?;
            return Ok((fname.to_string(), content));
        }
    }

    Err(AppError::not_found("No documentation file found"))
}

fn source_label_for_skill(skill: &SkillRecord) -> String {
    match skill.source_type.as_str() {
        "skillssh" => "skills.sh".to_string(),
        "git" => "Git".to_string(),
        "local" => "Local".to_string(),
        "import" => "Imported".to_string(),
        other => other.to_string(),
    }
}

#[tauri::command]
pub async fn delete_managed_skill(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = delete_managed_skills_by_ids(&store, &[skill_id.clone()])?;
        if result.deleted == 0 {
            return Err(AppError::not_found("Skill not found"));
        }
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn delete_managed_skills(
    skill_ids: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<BatchDeleteSkillsResult, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || delete_managed_skills_by_ids(&store, &skill_ids))
        .await?
}

pub fn delete_managed_skills_by_ids(
    store: &SkillStore,
    skill_ids: &[String],
) -> Result<BatchDeleteSkillsResult, AppError> {
    sync_metadata::with_repo_lock("delete skills", || {
        let mut deleted = 0;
        let mut failed = Vec::new();

        for skill_id in skill_ids {
            let Some(skill) = store.get_skill_by_id(skill_id)? else {
                store.log_audit(
                    AuditDraft::new("remove")
                        .skill(skill_id.clone(), "")
                        .fail("not found"),
                );
                failed.push(skill_id.clone());
                continue;
            };

            let targets = store.get_targets_for_skill(skill_id)?;
            for target in &targets {
                sync_engine::remove_recorded_target_or_warn(
                    &PathBuf::from(&target.target_path),
                    &target.mode,
                );
            }

            let central = PathBuf::from(&skill.central_path);
            if central.exists() {
                std::fs::remove_dir_all(&central).ok();
            }

            store.delete_skill(skill_id)?;
            store.log_audit(
                AuditDraft::new("remove")
                    .skill(skill_id.clone(), skill.name.clone())
                    .ok(),
            );
            deleted += 1;
        }

        if deleted > 0 {
            sync_metadata::write_all_from_db_unlocked(store)?;
        }

        Ok(BatchDeleteSkillsResult { deleted, failed })
    })
    .map_err(AppError::db)
}

/// Append an audit log entry summarising an install attempt.
/// `source_label` is short text identifying the source (e.g. "local", "git", "skillssh").
fn log_install_outcome(
    store: &SkillStore,
    source_label: &str,
    outcome: Result<&(String, String), &AppError>,
) {
    let draft = AuditDraft::new("install").detail(source_label);
    let draft = match outcome {
        Ok((id, name)) => draft.skill(id.clone(), name.clone()).ok(),
        Err(e) => draft.fail(e.to_string()),
    };
    store.log_audit(draft);
}

fn log_update_outcome(
    store: &SkillStore,
    skill_id: &str,
    source_label: &str,
    outcome: Result<&UpdateSkillResult, &AppError>,
) {
    let mut draft = AuditDraft::new("update").detail(source_label);
    match outcome {
        Ok(result) if !result.pending_removals.is_empty() => {
            // Held back, not applied. Recording it as a successful "unchanged"
            // would make the audit trail disagree with what actually happened.
            draft = draft
                .skill(result.skill.id.clone(), result.skill.name.clone())
                .detail(format!(
                    "{source_label}; held back — would remove {} path(s)",
                    result.pending_removals.len()
                ))
                .ok();
        }
        Ok(result) => {
            draft = draft
                .skill(result.skill.id.clone(), result.skill.name.clone())
                .detail(if result.content_changed {
                    format!("{source_label}; content changed")
                } else {
                    format!("{source_label}; unchanged")
                })
                .ok();
        }
        Err(e) => {
            let name = store
                .get_skill_by_id(skill_id)
                .ok()
                .flatten()
                .map(|s| s.name)
                .unwrap_or_default();
            draft = draft.skill(skill_id.to_string(), name).fail(e.to_string());
        }
    }
    store.log_audit(draft);
}

fn log_reimport_outcome(
    store: &SkillStore,
    skill_id: &str,
    outcome: Result<&ReimportSkillResult, &AppError>,
) {
    let mut draft = AuditDraft::new("update").detail("local");
    match outcome {
        Ok(result) if !result.pending_removals.is_empty() => {
            draft = draft
                .skill(result.skill.id.clone(), result.skill.name.clone())
                .detail(format!(
                    "local; held back — would remove {} path(s)",
                    result.pending_removals.len()
                ))
                .ok();
        }
        Ok(result) => {
            draft = draft
                .skill(result.skill.id.clone(), result.skill.name.clone())
                .ok();
        }
        Err(e) => {
            let name = store
                .get_skill_by_id(skill_id)
                .ok()
                .flatten()
                .map(|s| s.name)
                .unwrap_or_default();
            draft = draft.skill(skill_id.to_string(), name).fail(e.to_string());
        }
    }
    store.log_audit(draft);
}

#[tauri::command]
pub async fn install_local(
    source_path: String,
    name: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let outcome = (|| -> Result<(String, String), AppError> {
            let path = PathBuf::from(&source_path);
            let metadata = InstallSourceMetadata {
                source_type: "local".to_string(),
                source_ref: Some(source_path.clone()),
                source_ref_resolved: None,
                source_subpath: None,
                source_branch: None,
                source_revision: None,
                remote_revision: None,
                update_status: "local_only".to_string(),
            };
            let _lock =
                RepoLock::acquire_foreground("install local skill").map_err(AppError::db)?;
            let result =
                installer::install_from_local(&path, name.as_deref()).map_err(AppError::io)?;
            let skill_name = result.name.clone();
            // Install only adds the skill to the central library; preset
            // membership is an explicit action (see issue #213).
            let skill_id = store_installed_skill_unlocked(&store, &result, &metadata, None)?;
            Ok((skill_id, skill_name))
        })();
        log_install_outcome(&store, "local", outcome.as_ref());
        outcome.map(|_| ())
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
    let proxy_url = store.proxy_url();
    let registry = cancel_registry.inner().clone();
    let cancel_key = repo_url.clone();
    let cancel = registry.register(&cancel_key);
    let _cancel_guard = CancelRegistrationGuard::new(registry.clone(), cancel_key);

    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Emitter;
        let emit_progress = |phase: &str| {
            app_handle
                .emit(
                    "install-progress",
                    serde_json::json!({
                        "skill_id": repo_url,
                        "phase": phase,
                    }),
                )
                .ok();
        };

        let outcome = (|| -> Result<(String, String), AppError> {
            git_fetcher::validate_git_url(&repo_url).map_err(AppError::git)?;
            emit_progress("cloning");
            let parsed = git_fetcher::parse_git_source_resolved(&repo_url, proxy_url.as_deref());
            let app_for_progress = app_handle.clone();
            let url_for_progress = repo_url.clone();
            let progress_cb: git_fetcher::ProgressCallback = Box::new(move |msg: &str| {
                app_for_progress
                    .emit(
                        "install-progress",
                        serde_json::json!({
                            "skill_id": url_for_progress,
                            "phase": "cloning",
                            "detail": msg,
                        }),
                    )
                    .ok();
            });
            let temp_dir = git_fetcher::clone_repo_ref_scoped(
                &parsed.clone_url,
                parsed.branch.as_deref(),
                parsed.subpath.as_deref(),
                Some(&cancel),
                proxy_url.as_deref(),
                Some(progress_cb),
            )
            .map_err(AppError::classify_git_error)?;

            emit_progress("installing");
            let install_result = (|| -> Result<(String, String), AppError> {
                let _lock =
                    RepoLock::acquire_foreground("install git skill").map_err(AppError::db)?;
                let skill_dir = resolve_skill_dir(&temp_dir, parsed.subpath.as_deref(), None)?;
                let revision = git_fetcher::get_head_revision(&temp_dir).map_err(AppError::git)?;
                let result = installer::install_from_git_dir(&skill_dir, name.as_deref())
                    .map_err(AppError::io)?;
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
                let skill_name = result.name.clone();
                let skill_id = store_installed_skill_unlocked(&store, &result, &metadata, None)?;
                Ok((skill_id, skill_name))
            })();

            git_fetcher::cleanup_temp(&temp_dir);
            install_result
        })();

        log_install_outcome(&store, "git", outcome.as_ref());
        outcome?;

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
    let proxy_url = store.proxy_url();
    let registry = cancel_registry.inner().clone();
    let cancel_key_owned = format!("{}/{}", source, skill_id);
    let cancel = registry.register(&cancel_key_owned);
    let _cancel_guard = CancelRegistrationGuard::new(registry.clone(), cancel_key_owned);

    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Emitter;
        let skill_key = format!("{}/{}", source, skill_id);
        let emit_progress = |phase: &str| {
            app_handle
                .emit(
                    "install-progress",
                    serde_json::json!({
                        "skill_id": skill_key,
                        "phase": phase,
                    }),
                )
                .ok();
        };

        let outcome = (|| -> Result<(String, String), AppError> {
            emit_progress("cloning");
            let repo_url = format!("https://github.com/{}.git", source);
            let app_for_progress = app_handle.clone();
            let skill_key_for_progress = skill_key.clone();
            let progress_cb: git_fetcher::ProgressCallback = Box::new(move |msg: &str| {
                app_for_progress
                    .emit(
                        "install-progress",
                        serde_json::json!({
                            "skill_id": skill_key_for_progress,
                            "phase": "cloning",
                            "detail": msg,
                        }),
                    )
                    .ok();
            });
            let temp_dir = git_fetcher::clone_repo_ref_with_progress(
                &repo_url,
                None,
                Some(&cancel),
                proxy_url.as_deref(),
                Some(progress_cb),
            )
            .map_err(AppError::classify_git_error)?;

            emit_progress("installing");
            let install_result = (|| -> Result<(String, String), AppError> {
                let _lock =
                    RepoLock::acquire_foreground("install skillssh skill").map_err(AppError::db)?;
                let skill_dir = resolve_skill_dir(&temp_dir, None, Some(&skill_id))?;
                let revision = git_fetcher::get_head_revision(&temp_dir).map_err(AppError::git)?;
                let source_ref = format!("{}/{}", source, skill_id);
                let (install_name, destination) =
                    resolve_skillssh_install_target(&store, &source_ref, &skill_id)?;
                let result = installer::install_skill_dir_to_destination(
                    &skill_dir,
                    &install_name,
                    &destination,
                )
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
                let skill_name = result.name.clone();
                let new_id = store_installed_skill_unlocked(&store, &result, &metadata, None)?;
                Ok((new_id, skill_name))
            })();

            git_fetcher::cleanup_temp(&temp_dir);
            install_result
        })();

        log_install_outcome(&store, "skillssh", outcome.as_ref());
        outcome?;

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
        app_handle
            .emit(
                "install-progress",
                serde_json::json!({
                    "skill_id": repo_url,
                    "phase": "cloning",
                }),
            )
            .ok();

        let parsed = git_fetcher::parse_git_source_resolved(&repo_url, proxy_url.as_deref());
        let app_for_progress = app_handle.clone();
        let url_for_progress = repo_url.clone();
        let progress_cb: git_fetcher::ProgressCallback = Box::new(move |msg: &str| {
            app_for_progress
                .emit(
                    "install-progress",
                    serde_json::json!({
                        "skill_id": url_for_progress,
                        "phase": "cloning",
                        "detail": msg,
                    }),
                )
                .ok();
        });
        let temp_dir = git_fetcher::clone_repo_ref_scoped(
            &parsed.clone_url,
            parsed.branch.as_deref(),
            parsed.subpath.as_deref(),
            Some(&cancel),
            proxy_url.as_deref(),
            Some(progress_cb),
        )
        .map_err(AppError::classify_git_error)?;

        let build_preview = || -> Result<GitPreviewResult, AppError> {
            let skill_dir = resolve_skill_dir(&temp_dir, parsed.subpath.as_deref(), None)?;
            let dirs = collect_git_skill_dirs(&skill_dir);

            let skills: Vec<GitSkillPreview> = dirs
                .iter()
                .map(|dir| {
                    let meta = skill_metadata::parse_skill_md(dir);
                    let rel_path = skill_rel_key(&skill_dir, dir);
                    let basename = dir
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| rel_path.clone());
                    let name = meta
                        .name
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| basename.clone());
                    GitSkillPreview {
                        rel_path,
                        name,
                        description: meta.description,
                    }
                })
                .collect();

            Ok(GitPreviewResult {
                temp_dir: temp_dir.to_string_lossy().to_string(),
                skills,
            })
        };

        build_preview().inspect_err(|_e| {
            git_fetcher::cleanup_temp(&temp_dir);
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
    let proxy_url = store.proxy_url();
    tauri::async_runtime::spawn_blocking(move || {
        let temp_path = validate_clone_temp_path(&temp_dir)?;

        let result: Result<(), AppError> = (|| {
            if items.is_empty() {
                return Ok(());
            }

            let parsed = git_fetcher::parse_git_source_resolved(&repo_url, proxy_url.as_deref());
            let skill_dir = resolve_skill_dir(&temp_path, parsed.subpath.as_deref(), None)?;
            let all_dirs = collect_git_skill_dirs(&skill_dir);
            let revision = git_fetcher::get_head_revision(&temp_path).map_err(AppError::git)?;
            let _lock =
                RepoLock::acquire_foreground("confirm git install").map_err(AppError::db)?;

            for dir in &all_dirs {
                let rel_key = skill_rel_key(&skill_dir, dir);
                let item = match items.iter().find(|i| i.rel_path == rel_key) {
                    Some(i) => i,
                    None => continue,
                };
                let custom_name = item.name.trim();
                let install_name = if custom_name.is_empty() {
                    None
                } else {
                    Some(custom_name)
                };
                let result =
                    installer::install_from_git_dir(dir, install_name).map_err(AppError::io)?;
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
                store_installed_skill_unlocked(&store, &result, &metadata, None)?;
            }
            Ok(())
        })();

        // Always clean up temp directory, regardless of success or failure.
        git_fetcher::cleanup_temp(&temp_path);
        result
    })
    .await?
}

/// Clean up temp directory from a cancelled preview session.
#[tauri::command]
pub async fn cancel_git_preview(temp_dir: String) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        if let Ok(temp_path) = validate_clone_temp_path(&temp_dir) {
            git_fetcher::cleanup_temp(&temp_path);
        }
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
    let proxy_url = store.proxy_url();
    tauri::async_runtime::spawn_blocking(move || {
        let force = force.unwrap_or(false);
        // Resolve first, take the lock second. Holding it across `ls-remote`
        // meant one check of a slow remote could occupy the repository for the
        // whole round-trip and fail every concurrent operation (#315).
        let prefetched = prefetch_skill_remote(&store, &skill_id, force, proxy_url.as_deref());
        let _lock = RepoLock::acquire_foreground("check skill update").map_err(AppError::db)?;
        check_skill_update_internal_with_remote(&store, &skill_id, force, prefetched)
    })
    .await?
}

#[tauri::command]
pub async fn check_all_skill_updates(
    force: Option<bool>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    tauri::async_runtime::spawn_blocking(move || {
        let force_check = force.unwrap_or(false);
        let skills = store.get_all_skills().map_err(AppError::db)?;

        // ── Phase A: resolve every distinct remote once, concurrently ──
        // Collect the git-backed skills that still need a network check keyed by
        // (clone_url, branch). Skills installed from subdirectories of the same
        // monorepo collapse to a single `ls-remote`, and each remote is queried
        // off the central-repo lock so a slow remote (e.g. vercel/ai's ref
        // advertisement runs ~30s) never starves a concurrent check into a 20s
        // lock-timeout "busy" failure — the reason "检查全部" both crawled and
        // popped failures.
        let mut remotes: HashSet<RemoteKey> = HashSet::new();
        for skill in &skills {
            if !matches!(skill.source_type.as_str(), "git" | "skillssh") {
                continue;
            }
            match should_skip_update_check(&store, skill, force_check) {
                Ok(true) => continue,
                Ok(false) => {}
                // A transient skip-decision error (e.g. a settings read) must not
                // abort the whole batch: fall through so Phase B still checks this
                // skill and collects any real failure per-skill, as before.
                Err(err) => log::warn!(
                    "check all: skip-decision for {} failed, checking anyway: {}",
                    skill.id,
                    err.message
                ),
            }
            if let Ok(source) = git_source_from_skill(skill) {
                remotes.insert(RemoteKey::from(source));
            }
        }
        let remote_revisions = if remotes.is_empty() {
            HashMap::new()
        } else {
            resolve_remotes_concurrent(remotes.into_iter().collect(), proxy_url.clone())
        };

        // ── Phase B: apply the resolved revisions + local-source checks ──
        // Phase A already did every network read, so this loop only computes and
        // writes each skill's status columns. Re-take the central-repo lock per
        // skill around that write — the same guard the pre-concurrent code used so
        // a concurrent manual install/update can't race the `update_status` write
        // — but now the lock is never held across a slow `ls-remote`, because the
        // network happened off the lock in Phase A, and the apply step itself
        // can't reach the network. A skill whose source moved (or whose TTL
        // expired) between the two phases has no usable prefetch and is simply
        // left for the next round. Lock contention is still reported per skill
        // so the caller knows the check didn't complete for it.
        let mut failed = Vec::new();
        for skill in &skills {
            let prefetched = if matches!(skill.source_type.as_str(), "git" | "skillssh") {
                git_source_from_skill(skill).ok().and_then(|source| {
                    let key = RemoteKey::from(source);
                    remote_revisions
                        .get(&key)
                        .cloned()
                        .map(|result| PrefetchedRemote { key, result })
                })
            } else {
                None
            };
            let _lock = match RepoLock::acquire("check skill update") {
                Ok(lock) => lock,
                Err(err) => {
                    failed.push(format!("{}: {}", skill.id, err));
                    continue;
                }
            };
            if let Err(err) =
                check_skill_update_internal_with_remote(&store, &skill.id, force_check, prefetched)
            {
                // Surface the real per-skill reason so a batch that "just fails"
                // is diagnosable from the logs, not only the aggregated toast.
                log::warn!("check all: {} failed: {}", skill.id, err.message);
                failed.push(format!("{}: {}", skill.id, err));
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

/// A distinct remote to resolve once during a batch check. Several skills can
/// share one — e.g. many skills installed from subdirectories of a single
/// monorepo — so keying by (clone_url, branch) collapses the redundant network
/// queries the per-skill loop used to make.
#[derive(Clone, PartialEq, Eq, Hash)]
struct RemoteKey {
    clone_url: String,
    branch: Option<String>,
}

impl From<GitSkillSource> for RemoteKey {
    fn from(source: GitSkillSource) -> Self {
        RemoteKey {
            clone_url: source.clone_url,
            branch: source.branch,
        }
    }
}

impl RemoteKey {
    /// Whether `source` still points at this remote. Subpath is deliberately
    /// ignored: two subdirectories of one repo share a head revision.
    fn matches(&self, source: &GitSkillSource) -> bool {
        self.clone_url == source.clone_url && self.branch == source.branch
    }
}

/// A remote revision resolved off the central-repo lock, tagged with the remote
/// it was resolved for. The tag is what makes it safe to apply later: a
/// reinstall keeps a skill's row and repoints its source
/// (`update_skill_after_reinstall`), so the applying side re-derives the key
/// from the freshly read record and drops a prefetch that no longer matches.
#[derive(Clone)]
pub struct PrefetchedRemote {
    key: RemoteKey,
    result: Result<String, String>,
}

/// Resolve one skill's remote revision *before* the caller takes the
/// central-repo lock. Every lock-holding update-check path goes through this:
/// holding the lock across a slow `ls-remote` is what made an unrelated
/// foreground operation fail with a 20s "repository is busy" (#315).
///
/// Returns `None` when there is nothing to resolve — a local skill, one still
/// inside its check TTL, or an unparseable source — in which case the check
/// itself does no network either.
pub fn prefetch_skill_remote(
    store: &SkillStore,
    skill_id: &str,
    force: bool,
    proxy_url: Option<&str>,
) -> Option<PrefetchedRemote> {
    let skill = store.get_skill_by_id(skill_id).ok().flatten()?;
    if !matches!(skill.source_type.as_str(), "git" | "skillssh") {
        return None;
    }
    if should_skip_update_check(store, &skill, force).unwrap_or(false) {
        return None;
    }
    let key = RemoteKey::from(git_source_from_skill(&skill).ok()?);
    let result =
        git_fetcher::resolve_remote_revision(&key.clone_url, key.branch.as_deref(), proxy_url)
            .map_err(|err| err.to_string());
    Some(PrefetchedRemote { key, result })
}

/// Upper bound on concurrent `ls-remote` queries during a batch check. Collapses
/// the wall-clock cost of a large library from "sum of every remote" to "slowest
/// single remote" without opening an unbounded number of git subprocesses.
const MAX_CHECK_CONCURRENCY: usize = 8;

/// Resolve each remote's head revision concurrently, without the central-repo
/// lock — these are read-only remote reads. A failed resolution is stored as
/// `Err(message)` so Phase B can mark just that remote's skills as errored
/// without aborting the batch.
fn resolve_remotes_concurrent(
    remotes: Vec<RemoteKey>,
    proxy_url: Option<String>,
) -> HashMap<RemoteKey, Result<String, String>> {
    resolve_concurrent(remotes, |key| {
        git_fetcher::resolve_remote_revision(
            &key.clone_url,
            key.branch.as_deref(),
            proxy_url.as_deref(),
        )
        .map_err(|err| err.to_string())
    })
}

/// Run `resolve` over every remote concurrently (bounded by
/// `MAX_CHECK_CONCURRENCY`) with work-stealing, and collect each result. Factored
/// out of [`resolve_remotes_concurrent`] so the concurrency contract is testable
/// with an injected resolver instead of live network: every remote is resolved
/// exactly once, a per-remote failure is stored as `Err` rather than aborting the
/// batch, and — since this function never touches `RepoLock` — resolution always
/// runs off the central-repo lock.
fn resolve_concurrent<F>(
    remotes: Vec<RemoteKey>,
    resolve: F,
) -> HashMap<RemoteKey, Result<String, String>>
where
    F: Fn(&RemoteKey) -> Result<String, String> + Sync,
{
    use std::sync::atomic::{AtomicUsize, Ordering};

    let next = AtomicUsize::new(0);
    let results: Mutex<HashMap<RemoteKey, Result<String, String>>> =
        Mutex::new(HashMap::with_capacity(remotes.len()));
    let worker_count = MAX_CHECK_CONCURRENCY.min(remotes.len().max(1));

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| loop {
                let idx = next.fetch_add(1, Ordering::Relaxed);
                let Some(key) = remotes.get(idx) else { break };

                let resolved = resolve(key);
                if let Ok(mut map) = results.lock() {
                    map.insert(key.clone(), resolved);
                }
            });
        }
    });

    results.into_inner().unwrap_or_default()
}

/// Update one skill.
///
/// `approved_removals` carries back `removal_approval` from a call that
/// declined. The first call from the UI passes `None`; if it comes back with
/// `pending_removals`, the user is shown exactly what would disappear and only
/// then is it called again with that token.
#[tauri::command]
pub async fn update_skill(
    skill_id: String,
    approved_removals: Option<String>,
    store: State<'_, Arc<SkillStore>>,
    cancel_registry: State<'_, Arc<InstallCancelRegistry>>,
) -> Result<UpdateSkillResult, AppError> {
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    let registry = cancel_registry.inner().clone();
    let cancel_key = format!("update:{}", skill_id);
    let cancel = registry.register(&cancel_key);
    let _cancel_guard = CancelRegistrationGuard::new(registry.clone(), cancel_key);

    tauri::async_runtime::spawn_blocking(move || {
        let outcome =
            update_git_skill_internal(
                &store,
                &skill_id,
                proxy_url.as_deref(),
                Some(&cancel),
                approved_removals.as_deref(),
            );
        log_update_outcome(&store, &skill_id, "git", outcome.as_ref());
        outcome
    })
    .await?
}

#[tauri::command]
pub async fn reimport_local_skill(
    skill_id: String,
    approved_removals: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ReimportSkillResult, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let outcome =
            reimport_local_skill_internal(&store, &skill_id, approved_removals.as_deref());
        log_reimport_outcome(&store, &skill_id, outcome.as_ref());
        outcome
    })
    .await?
}

#[tauri::command]
pub async fn batch_update_skills(
    skill_ids: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<BatchUpdateSkillsResult, AppError> {
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    tauri::async_runtime::spawn_blocking(move || {
        let mut refreshed = 0usize;
        let mut unchanged = 0usize;
        let mut failed = Vec::new();
        let mut held_back = Vec::new();

        for skill_id in skill_ids {
            let skill = match store.get_skill_by_id(&skill_id).map_err(AppError::db)? {
                Some(skill) => skill,
                None => {
                    failed.push(format!("{skill_id}: Skill not found"));
                    continue;
                }
            };

            match skill.source_type.as_str() {
                "git" | "skillssh" => {
                    let outcome =
                        update_git_skill_internal(&store, &skill_id, proxy_url.as_deref(), None, None);
                    log_update_outcome(&store, &skill_id, "git", outcome.as_ref());
                    match outcome {
                        Ok(result) if !result.pending_removals.is_empty() => {
                            // Held back rather than applied: it would have taken
                            // away files the new version does not have, and a
                            // batch has nobody to ask.
                            held_back.push(skill.name.clone());
                        }
                        Ok(result) => {
                            if result.content_changed {
                                refreshed += 1;
                            } else {
                                unchanged += 1;
                            }
                        }
                        Err(err) => failed.push(format!("{}: {}", skill.name, err.message)),
                    }
                }
                "local" | "import" => {
                    let outcome = reimport_local_skill_internal(&store, &skill_id, None);
                    log_reimport_outcome(&store, &skill_id, outcome.as_ref());
                    match outcome {
                        Ok(result) if !result.pending_removals.is_empty() => {
                            held_back.push(skill.name.clone());
                        }
                        Ok(_) => refreshed += 1,
                        Err(err) => failed.push(format!("{}: {}", skill.name, err.message)),
                    }
                }
                _ => failed.push(format!("{}: Source type cannot be refreshed", skill.name)),
            }
        }

        Ok(BatchUpdateSkillsResult {
            refreshed,
            unchanged,
            failed,
            held_back,
        })
    })
    .await?
}

#[tauri::command]
/// Re-point a local skill at a different source directory.
///
/// `approved_removals` behaves as on the update paths: choosing a new source is
/// not a statement about discarding what the library has accumulated, so a
/// replacement that would take files away stops and reports them first.
pub async fn relink_local_skill_source(
    skill_id: String,
    source_path: String,
    approved_removals: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ReimportSkillResult, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;

        if !matches!(skill.source_type.as_str(), "local" | "import") {
            return Err(AppError::invalid_input(
                "Only local skills can relink source paths",
            ));
        }

        let path = PathBuf::from(&source_path);
        if !path.exists() {
            return Err(AppError::not_found("Selected source path does not exist"));
        }
        if !is_valid_skill_dir(&path) {
            return Err(AppError::invalid_input(
                "Selected source path is not a valid skill directory",
            ));
        }

        store
            .update_skill_update_status(&skill_id, "updating")
            .map_err(AppError::db)?;

        let result = (|| -> Result<(Vec<PendingRemoval>, Option<String>), AppError> {
            let _lock = RepoLock::acquire_foreground("relink local skill").map_err(AppError::db)?;
            let staged_path = staged_path_for(&skill.central_path);
            let install_result = installer::install_from_local_to_destination(
                &path,
                Some(&skill.name),
                &staged_path,
            )
            .inspect_err(|_| {
                let _ = remove_path_if_exists(&staged_path);
            })
            .map_err(AppError::io)?;
            let staged_guard = StagedPathGuard::new(&staged_path, true);

            // Picking a new source says which source to follow. It does not say
            // to discard whatever has accumulated in the library since — same
            // replacement, same guard.
            let pending = pending_removals_for(&store, &skill, Some(&staged_path))?;
            let approval = removal_approval_token(&source_path, &pending);
            if !pending.is_empty() && approved_removals.as_deref() != Some(approval.as_str()) {
                // Put back exactly what was there. Hardcoding a status loses
                // `source_missing` — the only state relink is reachable from —
                // so declining would hide the Relink and Detach buttons on the
                // next refresh, and `check_state` would also clear the recorded
                // error and check time that nothing here has re-established.
                store
                    .update_skill_update_status(&skill.id, &skill.update_status)
                    .map_err(AppError::db)?;
                return Ok((pending, Some(approval)));
            }

            swap_skill_directory(&staged_path, Path::new(&skill.central_path))?;
            staged_guard.release();
            store
                .update_skill_after_reinstall(
                    &skill.id,
                    &skill.name,
                    install_result.description.as_deref(),
                    &skill.source_type,
                    Some(&source_path),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(&install_result.content_hash),
                    "local_only",
                )
                .map_err(AppError::db)?;
            resync_copy_targets(&store, &skill.id)?;
            sync_metadata::write_all_from_db_unlocked(&store).map_err(AppError::db)?;
            Ok((Vec::new(), None))
        })();

        match result {
            Ok((pending_removals, removal_approval)) => Ok(ReimportSkillResult {
                skill: managed_skill_by_id(&store, &skill_id)?,
                pending_removals,
                removal_approval,
            }),
            Err(e) => {
                let _ = store.update_skill_check_state(&skill_id, None, "error", Some(&e.message));
                Err(e)
            }
        }
    })
    .await?
}

#[tauri::command]
pub async fn detach_local_skill_source(
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
            return Err(AppError::invalid_input(
                "Only local skills can detach source paths",
            ));
        }

        {
            let _lock = RepoLock::acquire_foreground("detach local skill").map_err(AppError::db)?;
            store
                .update_skill_after_reinstall(
                    &skill.id,
                    &skill.name,
                    skill.description.as_deref(),
                    &skill.source_type,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    skill.content_hash.as_deref(),
                    "local_only",
                )
                .map_err(AppError::db)?;
            sync_metadata::write_all_from_db_unlocked(&store).map_err(AppError::db)?;
        }

        managed_skill_by_id(&store, &skill_id)
    })
    .await?
}

#[tauri::command]
/// Re-install a skill whose recorded local path is gone, from a repository.
///
/// The first call reports instead of committing: it returns the difference and
/// the paths a replacement would take away, with an approval bound to that exact
/// revision and list. The second call carries that approval and commits.
/// Anything that changes in between — a push, a file written into the library —
/// invalidates the approval and the change is reported again rather than
/// applied.
pub async fn recover_skill_source(
    skill_id: String,
    repo_url: String,
    locator_source: Option<String>,
    locator_skill_id: Option<String>,
    subpath: Option<String>,
    branch: Option<String>,
    approved_removals: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<SetSourceResult, AppError> {
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    tauri::async_runtime::spawn_blocking(move || {
        recover_skill_source_internal(
            &store,
            &skill_id,
            &repo_url,
            locator_source.as_deref(),
            locator_skill_id.as_deref(),
            subpath.as_deref(),
            branch.as_deref(),
            proxy_url.as_deref(),
            approved_removals.as_deref(),
        )
    })
    .await?
}

/// Upper bound on one batch recovery. Every entry is a clone, so an unbounded
/// list is a way to occupy the process for an unbounded time. The largest
/// library this ships against is a few hundred skills, so the cap sits well
/// above any real batch and only rejects a runaway list.
const MAX_BATCH_RECOVER: usize = 200;

/// One skill's entry in a batch recovery request.
///
/// The source travels with the skill because a `source_missing` row carries no
/// usable remote — the dead local path is the only thing recorded, which is the
/// whole reason it needs recovering. Approval is per skill for the same reason:
/// [`crate::core::removals`] binds a token to one revision and one removal list,
/// and no single revision is true for several repositories at once. A batch
/// therefore carries N independent approvals, never one shared one.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct BatchRecoverRequest {
    pub skill_id: String,
    pub repo_url: String,
    #[serde(default)]
    pub locator_source: Option<String>,
    #[serde(default)]
    pub locator_skill_id: Option<String>,
    #[serde(default)]
    pub subpath: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    /// Token from a previous report for *this* skill. `None` on the first call,
    /// which reports without writing.
    #[serde(default)]
    pub approved_removals: Option<String>,
}

/// One skill's outcome inside a batch.
///
/// Every failure mode is data here rather than an error that ends the batch: a
/// skill whose upstream 404s must not cost the others their commit. The same
/// reason `pending_removals` is not a failure — declining to approve is an
/// answer, not an error.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BatchRecoverItem {
    pub skill_id: String,
    pub name: String,
    pub applied: bool,
    pub content_changed: bool,
    pub clone_url: String,
    pub revision: String,
    pub subpath: Option<String>,
    pub branch: Option<String>,
    pub pending_removals: Vec<PendingRemoval>,
    pub removal_approval: Option<String>,
    pub diff_entries: Vec<SkillSourceDiffEntryDto>,
    pub central_copy_exists: bool,
    pub duplicate_skill_name: Option<String>,
    /// Why this skill did not make it. `None` on success — including a skill
    /// that is held back awaiting approval, which is reported through
    /// `removal_approval` instead.
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BatchRecoverResult {
    pub requested: usize,
    pub applied: usize,
    pub held: usize,
    pub failed: usize,
    pub items: Vec<BatchRecoverItem>,
}

/// Recover several skills whose original paths are gone, in one call.
///
/// Two calls make the batch: the first reports every skill without writing
/// anything, the second carries each skill's own approval back. Reporting
/// resolves N upstreams, so a batch costs N clones — that is inherent to the
/// two-phase contract, not something this loop can avoid.
///
/// Serial on purpose. Each commit takes and releases the central repo lock
/// inside itself, so this loop never holds it across a clone — which is the
/// whole reason the lock is not taken out here. Running two commits
/// concurrently would serialise on that lock anyway while adding the cost of
/// interleaved staging directories for no gain.
///
/// `on_progress` is called as `(index, total, name, done)` around each skill.
/// Reporting N sources means N clones, and a batch of a dozen is minutes of
/// work; a caller showing one spinner for that is indistinguishable from a
/// hang. It is an argument rather than an app handle so the CLI can print the
/// same steps and the tests can ignore them.
pub fn batch_recover_skill_sources_internal(
    store: &SkillStore,
    requests: &[BatchRecoverRequest],
    proxy_url: Option<&str>,
    mut on_progress: Option<&mut dyn FnMut(usize, usize, &str, bool)>,
) -> Result<BatchRecoverResult, AppError> {
    if requests.is_empty() {
        return Err(AppError::invalid_input("No skills to recover"));
    }
    if requests.len() > MAX_BATCH_RECOVER {
        return Err(AppError::invalid_input(format!(
            "A batch recovers at most {MAX_BATCH_RECOVER} skills at a time; got {}",
            requests.len()
        )));
    }

    // The same row twice would recover once and then be refused on the second
    // pass by the local/import guard, which reads as a batch bug rather than a
    // caller mistake. Say so up front instead.
    let mut seen = std::collections::HashSet::new();
    for request in requests {
        if !seen.insert(request.skill_id.as_str()) {
            return Err(AppError::invalid_input(format!(
                "Skill '{}' appears more than once in the batch",
                request.skill_id
            )));
        }
    }

    let mut applied = 0usize;
    let mut held = 0usize;
    let mut failed = 0usize;
    let mut items = Vec::with_capacity(requests.len());

    for (position, request) in requests.iter().enumerate() {
        let name = store
            .get_skill_by_id(&request.skill_id)
            .ok()
            .flatten()
            .map(|skill| skill.name)
            .unwrap_or_else(|| request.skill_id.clone());

        if let Some(report) = on_progress.as_mut() {
            report(position + 1, requests.len(), &name, false);
        }

        let item = match recover_skill_source_internal(
            store,
            &request.skill_id,
            &request.repo_url,
            request.locator_source.as_deref(),
            request.locator_skill_id.as_deref(),
            request.subpath.as_deref(),
            request.branch.as_deref(),
            proxy_url,
            request.approved_removals.as_deref(),
        ) {
            Ok(result) => {
                if result.applied {
                    applied += 1;
                } else {
                    held += 1;
                }
                BatchRecoverItem {
                    skill_id: result.skill_id,
                    name: name.clone(),
                    applied: result.applied,
                    content_changed: result.content_changed,
                    clone_url: result.clone_url,
                    revision: result.revision,
                    subpath: result.subpath,
                    branch: result.branch,
                    pending_removals: result.pending_removals,
                    removal_approval: result.removal_approval,
                    diff_entries: result.diff_entries,
                    central_copy_exists: result.central_copy_exists,
                    duplicate_skill_name: result.duplicate_skill_name,
                    error: None,
                }
            }
            // The row keeps its pre-attempt status, so a skill that fails here
            // is still recoverable on the next run — the batch reports the
            // reason instead of ending the call.
            Err(err) => {
                failed += 1;
                BatchRecoverItem {
                    skill_id: request.skill_id.clone(),
                    name: name.clone(),
                    applied: false,
                    content_changed: false,
                    clone_url: request.repo_url.clone(),
                    revision: String::new(),
                    subpath: request.subpath.clone(),
                    branch: request.branch.clone(),
                    pending_removals: Vec::new(),
                    removal_approval: None,
                    diff_entries: Vec::new(),
                    central_copy_exists: false,
                    duplicate_skill_name: None,
                    error: Some(err.message.clone()),
                }
            }
        };
        if let Some(report) = on_progress.as_mut() {
            report(position + 1, requests.len(), &name, true);
        }
        items.push(item);
    }

    Ok(BatchRecoverResult {
        requested: requests.len(),
        applied,
        held,
        failed,
        items,
    })
}

#[tauri::command]
/// Recover several skills whose original paths are gone, in one call.
///
/// Mirrors [`recover_skill_source`] per skill rather than replacing it: every
/// entry goes through the same gate and the same commit path, so a batch cannot
/// acquire a capability the single-skill command refuses. See
/// [`batch_recover_skill_sources_internal`] for why the loop is serial.
///
/// Emits `batch-recover-progress` around each skill. Reporting N sources means
/// N clones, so a batch can run for minutes; without this the only honest
/// thing the UI could show was a spinner that never ends.
pub async fn batch_recover_skill_sources(
    app: tauri::AppHandle,
    requests: Vec<BatchRecoverRequest>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<BatchRecoverResult, AppError> {
    use tauri::Emitter;
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    tauri::async_runtime::spawn_blocking(move || {
        let mut report = |index: usize, total: usize, name: &str, done: bool| {
            let _ = app.emit(
                "batch-recover-progress",
                serde_json::json!({
                    "index": index,
                    "total": total,
                    "name": name,
                    "done": done,
                }),
            );
        };
        batch_recover_skill_sources_internal(
            &store,
            &requests,
            proxy_url.as_deref(),
            Some(&mut report),
        )
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

    let preset_ids = store.get_scenarios_for_skill(&skill.id).unwrap_or_default();
    let tags = tags_map.get(&skill.id).cloned().unwrap_or_default();

    // Prefer description from SKILL.md so the list view reflects edits made
    // directly on disk (file watcher emits a change event; this read serves
    // the fresh value). Keep `name` on the DB value to avoid drift with
    // sync target directory names.
    let description = skill_metadata::parse_skill_md(Path::new(&skill.central_path))
        .description
        .filter(|s| !s.trim().is_empty())
        .or(skill.description);

    ManagedSkillDto {
        id: skill.id,
        name: skill.name,
        description,
        source_type: skill.source_type,
        source_ref: skill.source_ref,
        source_ref_resolved: skill.source_ref_resolved,
        source_subpath: skill.source_subpath,
        source_branch: skill.source_branch,
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
        preset_ids,
        tags,
    }
}

pub fn managed_skill_by_id(
    store: &SkillStore,
    skill_id: &str,
) -> Result<ManagedSkillDto, AppError> {
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;
    let all_targets = store.get_all_targets().map_err(AppError::db)?;
    let tags_map = store.get_tags_map().map_err(AppError::db)?;
    Ok(managed_skill_to_dto(store, skill, &all_targets, &tags_map))
}

/// Update an installed git-sourced skill.
///
/// `approved_removals` carries back the token from a previous call that
/// declined, approving exactly the list it reported at exactly that revision.
/// Without it — or with a stale one — an update that would take away files the
/// new version does not have stops and reports them instead, having changed
/// nothing. See [`crate::core::removals`].
///
/// Unattended callers pass `None` and simply do not update: nobody is there to
/// be asked, and applying anyway is what #256 was.
pub fn update_git_skill_internal(
    store: &SkillStore,
    skill_id: &str,
    proxy_url: Option<&str>,
    cancel: Option<&Arc<AtomicBool>>,
    approved_removals: Option<&str>,
) -> Result<UpdateSkillResult, AppError> {
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;

    if !matches!(skill.source_type.as_str(), "git" | "skillssh") {
        return Err(AppError::invalid_input(
            "Only git-based skills can be updated",
        ));
    }

    let git_source = git_source_from_skill(&skill)?;
    git_fetcher::validate_git_url(&git_source.clone_url).map_err(AppError::git)?;
    let remote_revision = git_fetcher::resolve_remote_revision(
        &git_source.clone_url,
        git_source.branch.as_deref(),
        proxy_url,
    )
    .map_err(|e| {
        let message = e.to_string();
        let _ = store.update_skill_check_state(
            skill_id,
            skill.remote_revision.as_deref(),
            "error",
            Some(&message),
        );
        AppError::git(message)
    })?;

    store
        .update_skill_update_status(skill_id, "updating")
        .map_err(AppError::db)?;

    let temp_dir = git_fetcher::clone_repo_ref_scoped(
        &git_source.clone_url,
        git_source.branch.as_deref(),
        git_source.subpath.as_deref(),
        cancel,
        proxy_url,
        None,
    )
    .map_err(AppError::classify_git_error)?;
    let update_result = (|| -> Result<UpdateOutcome, AppError> {
        git_fetcher::checkout_revision(&temp_dir, &remote_revision).map_err(AppError::git)?;
        let skill_dir = resolve_skill_dir(
            &temp_dir,
            git_source.subpath.as_deref(),
            git_source.locator_skill_id.as_deref(),
        )?;

        let new_hash =
            crate::core::content_hash::hash_directory(&skill_dir).map_err(AppError::io)?;
        let content_changed = skill.content_hash.as_deref() != Some(new_hash.as_str());
        let source_subpath = git_fetcher::relative_subpath(&temp_dir, &skill_dir);
        let _lock = RepoLock::acquire_foreground("update installed skill").map_err(AppError::db)?;

        // Stage first, then compare. The tree that lands in the library is the
        // installer's output, not the raw checkout — it drops `.git` and every
        // symlink — so comparing against the checkout would report a path as
        // surviving that the swap then removes.
        let staged_path = staged_path_for(&skill.central_path);
        let install_result = if content_changed {
            Some(
                installer::install_skill_dir_to_destination(&skill_dir, &skill.name, &staged_path)
                    .inspect_err(|_| {
                        let _ = remove_path_if_exists(&staged_path);
                    })
                    .map_err(AppError::io)?,
            )
        } else {
            None
        };
        let staged_guard = StagedPathGuard::new(&staged_path, install_result.is_some());

        let pending = pending_removals_for(store, &skill, install_result.is_some().then_some(staged_path.as_path()))?;

        // A confirmation answers one exact question: this revision, this list
        // as shown. It closes the window while the dialog is open — a push, or
        // a file that changes the list, re-asks. Note a directory the new
        // version drops is one entry, so a file created *inside* it afterwards
        // does not change the list; approving `outputs/` approves the subtree. It cannot close the window
        // between this scan and the removal itself: the repo lock holds off
        // Skills Manager, not the agent processes writing into these very
        // directories. Narrowing that further needs the directories frozen
        // before the scan, not another scan.
        let approval = removal_approval_token(&remote_revision, &pending);
        if !pending.is_empty() && approved_removals != Some(approval.as_str()) {
            // Declining is not a failure: nothing was touched and the update is
            // still waiting. Clear the `updating` marker here, inside the lock,
            // rather than after releasing it — doing it later lets a concurrent
            // update overwrite the state, and swallowing the error would leave
            // the skill showing "updating" forever.
            store
                .update_skill_check_state(
                    &skill.id,
                    Some(&remote_revision),
                    "update_available",
                    None,
                )
                .map_err(AppError::db)?;
            return Ok(UpdateOutcome::Held { pending, approval });
        }

        if let Some(install_result) = install_result {
            swap_skill_directory(&staged_path, Path::new(&skill.central_path))?;
            // Only now is it the library's. Releasing before the swap left the
            // staged directory behind whenever its first rename failed.
            staged_guard.release();

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
            resync_copy_targets(store, &skill.id)?;
            sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;
        } else {
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
                .update_skill_check_state(&skill.id, Some(&remote_revision), "up_to_date", None)
                .map_err(AppError::db)?;
            resync_copy_targets(store, &skill.id)?;
            sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;
        }
        Ok(UpdateOutcome::Applied { content_changed })
    })();
    git_fetcher::cleanup_temp(&temp_dir);

    match update_result {
        Ok(outcome) => {
            let (content_changed, pending_removals, removal_approval) = match outcome {
                UpdateOutcome::Applied { content_changed } => (content_changed, Vec::new(), None),
                UpdateOutcome::Held { pending, approval } => (false, pending, Some(approval)),
            };
            let skill = managed_skill_by_id(store, skill_id)?;
            Ok(UpdateSkillResult {
                skill,
                content_changed,
                pending_removals,
                removal_approval,
            })
        }
        Err(e) => {
            let _ = store.update_skill_check_state(
                skill_id,
                Some(&remote_revision),
                "error",
                Some(&e.message),
            );
            Err(e)
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SetSourceResult {
    pub skill_id: String,
    pub name: String,
    /// Source type before the change (`local`, `import`, `git`, `skillssh`).
    pub previous_source_type: String,
    pub previous_source_ref: Option<String>,
    pub clone_url: String,
    pub subpath: Option<String>,
    pub branch: Option<String>,
    pub revision: String,
    /// Whether the new source's content differs from the hash recorded for the
    /// library copy. False means the re-point is metadata-only — no file is
    /// rewritten. Compared against the recorded hash, not a fresh hash of the
    /// central directory, so hand-edits made after install do not count as a
    /// difference (and are left in place, since no file work runs).
    pub content_changed: bool,
    pub dry_run: bool,
    /// Whether the re-point was committed. `false` means the change is waiting
    /// on the user's answer: nothing was written and `pending_removals` says
    /// what approving it would take away. Same contract as
    /// [`UpdateSkillResult::pending_removals`] — see the comment on
    /// [`BatchUpdateSkillsResult::held_back`] for why declining is not a
    /// failure and must not be reported as one.
    ///
    /// A `dry_run` report sets this to `true`: producing the report *is* its
    /// whole job, and there is nothing pending to approve.
    pub applied: bool,
    /// Non-empty together with `applied == false` — the paths approving this
    /// re-point would remove, and the approval that releases them.
    pub pending_removals: Vec<PendingRemoval>,
    pub removal_approval: Option<String>,
    /// What the new source has that the library copy does not, and the reverse.
    /// Empty when `applied` is true (nothing was replaced) or when the two trees
    /// are identical.
    pub diff_entries: Vec<SkillSourceDiffEntryDto>,
    /// Whether the library copy still exists. `false` means there is nothing to
    /// diff against and nothing to roll back to: the re-install is the only
    /// remaining copy of this skill's content, and `pending_removals` is empty
    /// only because there is nothing left to remove.
    pub central_copy_exists: bool,
    /// Name of another installed skill already tracked at this exact source, if
    /// any. Recovering onto a source that is already in the library gives two
    /// rows the same upstream, which then update and deploy independently.
    pub duplicate_skill_name: Option<String>,
}

/// Resolve the skill directory inside a fresh checkout, strictly.
///
/// Unlike [`resolve_skill_dir`], an explicit subpath that does not land on a
/// valid skill directory inside `repo_dir` is an error rather than a silent
/// fallback to repo-wide discovery. Re-pointing establishes a *new* source of
/// truth for an already-installed skill, so guessing is worse than failing: a
/// typo would otherwise install some unrelated directory — or the whole repo —
/// over the existing central copy.
fn resolve_repoint_skill_dir(repo_dir: &Path, subpath: Option<&str>) -> Result<PathBuf, AppError> {
    let Some(subpath) = subpath else {
        return if is_valid_skill_dir(repo_dir) {
            Ok(repo_dir.to_path_buf())
        } else {
            Err(AppError::subpath_required(
                "Repository root is not a skill directory (no SKILL.md); pass --subpath",
            ))
        };
    };

    // `Path::join` returns the argument verbatim when it is absolute, and `..`
    // segments climb out, so the candidate must be checked before it is used.
    // `is_path_safe` canonicalizes both sides, which also catches symlinks that
    // point outside the checkout.
    let candidate = repo_dir.join(subpath);
    if !path_guard::is_path_safe(repo_dir, &candidate) {
        return Err(AppError::invalid_input(format!(
            "Subpath '{subpath}' resolves outside the repository"
        )));
    }
    if !candidate.is_dir() {
        return Err(AppError::not_found(format!(
            "Subpath '{subpath}' does not exist in the repository"
        )));
    }
    if !is_valid_skill_dir(&candidate) {
        return Err(AppError::invalid_input(format!(
            "Subpath '{subpath}' is not a skill directory (no SKILL.md)"
        )));
    }
    Ok(candidate)
}

/// What a re-point needs to know about the source it is pointing at.
///
/// The pair `locator_source` + `locator_skill_id` is what keeps a skills.sh
/// skill locatable after upstream moves the directory: `source_ref` is written
/// as `"{locator_source}/{locator_skill_id}"` (the same shape
/// [`install_from_skillssh`] writes), and [`git_source_from_skill`] recovers the
/// locator from it. Write a bare clone URL there instead and
/// [`skill_ssh_id`] would cut it into `("https://github.com/owner", "repo.git")`
/// — a locator that names nothing.
#[derive(Debug, Clone, Copy)]
struct RepointRequest<'a> {
    clone_url: &'a str,
    subpath: Option<&'a str>,
    branch: Option<&'a str>,
    locator_source: Option<&'a str>,
    locator_skill_id: Option<&'a str>,
}

/// The source metadata a committed re-point writes to the row.
struct RepointSource {
    source_type: String,
    source_ref: String,
    source_ref_resolved: String,
    branch: Option<String>,
    remote_revision: String,
}

/// Tracks what this call actually did to the row, so a failure reports the
/// truth about it rather than a fixed "error".
///
/// Same pair of markers [`set_git_source_internal`] has always carried: a
/// refusal that never set `updating` must not be dressed up as an error, and the
/// revision written on failure has to be the one the row still points at.
#[derive(Default)]
struct RepointProgress {
    marked_updating: std::cell::Cell<bool>,
    source_committed: std::cell::Cell<bool>,
}

#[derive(Debug)]
enum RepointOutcome {
    Applied {
        content_changed: bool,
        resolved_subpath: Option<String>,
    },
    /// Waiting on the user's answer; nothing was written.
    Held {
        pending: Vec<PendingRemoval>,
        approval: String,
        diff_entries: Vec<SkillSourceDiffEntryDto>,
        central_copy_exists: bool,
        duplicate_skill_name: Option<String>,
        resolved_subpath: Option<String>,
    },
}

/// The row metadata a re-point commits.
///
/// A skills.sh candidate has to be written as `{owner}/{repo}/{skill_id}`, not
/// as its clone URL: [`skill_ssh_id`] recovers the locator by cutting the ref on
/// `/`, and a URL cuts into `("https://github.com/owner", "repo.git")` — a
/// locator that names nothing, and the whole reason not to downgrade such a
/// skill to `git` (see [`git_source_from_skill`]). Either half of the locator
/// pair alone is not enough to make one, so both are required.
fn repoint_source_for(
    request: &RepointRequest<'_>,
    original_url: &str,
    clone_url: &str,
    branch: Option<String>,
    remote_revision: &str,
) -> RepointSource {
    match (request.locator_source, request.locator_skill_id) {
        (Some(owner_repo), Some(locator)) => RepointSource {
            source_type: "skillssh".to_string(),
            source_ref: format!("{}/{}", owner_repo, locator),
            source_ref_resolved: clone_url.to_string(),
            branch,
            remote_revision: remote_revision.to_string(),
        },
        _ => RepointSource {
            source_type: "git".to_string(),
            source_ref: original_url.to_string(),
            source_ref_resolved: clone_url.to_string(),
            branch,
            remote_revision: remote_revision.to_string(),
        },
    }
}

/// Which status a failed re-point leaves on the row.
///
/// The error itself always goes to `last_check_error`; this only decides what
/// the badge reads, and that is not free — the relink / find-source / detach
/// actions are offered for `source_missing` and nothing else, so overwriting it
/// strands the row with no way out.
///
/// Three cases, because each is a different claim about the row:
/// - committed: it points at the new source now, and the old status described a
///   source it no longer tracks.
/// - still `source_missing`: the path really is gone, which has not stopped being
///   true just because this attempt failed. The caller's own status never wins
///   here — "error" is not a licence to hide the exit.
/// - anything else: report under what the caller asked for.
fn failure_status_for<'a>(
    source_committed: bool,
    snapshot_status: &str,
    caller_status: &'a str,
) -> &'a str {
    if source_committed {
        "error"
    } else if snapshot_status == "source_missing" {
        "source_missing"
    } else {
        caller_status
    }
}

/// Re-point an installed skill at a git source **in place**.
///
/// The skill row is updated by id, so the skill id, tags, preset membership and
/// deployment targets all survive. This is the only safe way to convert a
/// `local` skill to a `git` one: `install` reuses a central directory only when
/// the content hash matches exactly (see `installer::unique_skill_dest`) and
/// otherwise silently allocates `<name>-2`, while `remove` + `install` drops the
/// id and everything keyed to it.
///
/// `force` is the CLI's "I know, overwrite" switch. Without it a re-point that
/// would take files away stops and reports them, exactly as the update paths do.
pub fn set_git_source_internal(
    store: &SkillStore,
    skill_id: &str,
    git_url: &str,
    subpath: Option<&str>,
    branch: Option<&str>,
    proxy_url: Option<&str>,
    force: bool,
    dry_run: bool,
) -> Result<SetSourceResult, AppError> {
    repoint_skill(
        store,
        skill_id,
        &RepointRequest {
            clone_url: git_url,
            subpath,
            branch,
            locator_source: None,
            locator_skill_id: None,
        },
        proxy_url,
        force,
        dry_run,
        None,
        // A failed `set-source` has always reported itself as an error on the
        // row; the CLI prints the message either way.
        "error",
    )
}

/// Re-install a skill whose recorded local path is gone, from a repository found
/// online.
///
/// Same in-place re-point as [`set_git_source_internal`], which is what keeps the
/// id, tags, preset membership and deployments — but it never takes `force`:
/// overwriting is authorised through the approval token instead, so the user is
/// shown exactly which files the replacement would take away. A failure restores
/// the status the row had rather than stamping `error` on it, because the
/// relink / find-source / detach buttons are only reachable *from* `source_missing`
/// and an error status would hide all three.
pub fn recover_skill_source_internal(
    store: &SkillStore,
    skill_id: &str,
    repo_url: &str,
    locator_source: Option<&str>,
    locator_skill_id: Option<&str>,
    subpath: Option<&str>,
    branch: Option<&str>,
    proxy_url: Option<&str>,
    approved_removals: Option<&str>,
) -> Result<SetSourceResult, AppError> {
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;

    // Same guard relink and re-import carry. Without it this command would be a
    // general "re-point anything at anything" entry point, reachable by anything
    // that can construct an invoke.
    if !matches!(skill.source_type.as_str(), "local" | "import") {
        return Err(AppError::invalid_input(
            "Only local skills can recover their source",
        ));
    }

    let held_status = skill.update_status.clone();
    repoint_skill(
        store,
        skill_id,
        &RepointRequest {
            clone_url: repo_url,
            subpath,
            branch,
            locator_source,
            locator_skill_id,
        },
        proxy_url,
        false,
        false,
        approved_removals,
        &held_status,
    )
}

/// The one path both re-points go through: resolve and fetch the source off the
/// lock, then hand a fresh checkout to [`commit_repoint_locked`], which owns
/// everything that writes.
///
/// `failure_status` is what a failure reports on the row — see
/// [`RepointProgress`].
fn repoint_skill(
    store: &SkillStore,
    skill_id: &str,
    request: &RepointRequest<'_>,
    proxy_url: Option<&str>,
    force: bool,
    dry_run: bool,
    approved_removals: Option<&str>,
    failure_status: &str,
) -> Result<SetSourceResult, AppError> {
    // Read before anything can change underneath us: this snapshot is what the
    // locked re-read is checked against.
    let snapshot = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;

    // Validate before parsing: resolving a GitHub tree URL runs `ls-remote`, so
    // an unvalidated URL would reach the network first. `install` validates its
    // raw input the same way.
    git_fetcher::validate_git_url(request.clone_url).map_err(AppError::git)?;
    let parsed = git_fetcher::parse_git_source_resolved(request.clone_url, proxy_url);

    // An explicit flag wins over whatever the URL encodes. An empty subpath is
    // the caller saying "the skill is at the repo root", which is distinct from
    // omitting it and letting the URL decide.
    let branch = request
        .branch
        .map(str::to_string)
        .or_else(|| parsed.branch.clone());
    let subpath = match request.subpath {
        Some("") => None,
        Some(value) => Some(value.to_string()),
        None => parsed.subpath.clone(),
    };

    let remote_revision =
        git_fetcher::resolve_remote_revision(&parsed.clone_url, branch.as_deref(), proxy_url)
            // Classified, not `git`: this is the first call to reach the network,
            // and for a private repository it is the one that fails — waiting
            // for the clone would mean a second, identical round trip. Without
            // the classifier the auth case falls through as a plain git error
            // and the caller cannot tell "needs credentials" from "no such
            // repository".
            .map_err(AppError::classify_git_error)?;

    let temp_dir = git_fetcher::clone_repo_ref_scoped(
        &parsed.clone_url,
        branch.as_deref(),
        subpath.as_deref(),
        None,
        proxy_url,
        None,
    )
    .map_err(AppError::classify_git_error)?;

    // Nothing before this point has written to the store, so a failure during
    // the network phase leaves no state to unwind — in particular the skill is
    // never left stuck in `updating`.
    let progress = RepointProgress::default();
    let outcome = (|| -> Result<RepointOutcome, AppError> {
        git_fetcher::checkout_revision(&temp_dir, &remote_revision).map_err(AppError::git)?;

        // A locator means upstream may have moved the skill since it was
        // recorded, so the forgiving resolver is right — it falls back to
        // searching the checkout (issue #278). Without one it must be the
        // strict resolver: an unrelated root, or the whole `skills/` container,
        // would otherwise be installed over the existing copy.
        let skill_dir = match request.locator_skill_id {
            Some(locator) => resolve_skill_dir(&temp_dir, subpath.as_deref(), Some(locator))?,
            None => resolve_repoint_skill_dir(&temp_dir, subpath.as_deref())?,
        };
        let resolved_subpath = git_fetcher::relative_subpath(&temp_dir, &skill_dir);

        let new_hash =
            crate::core::content_hash::hash_directory(&skill_dir).map_err(AppError::io)?;

        let source = repoint_source_for(
            request,
            &parsed.original_url,
            &parsed.clone_url,
            branch.clone(),
            &remote_revision,
        );

        // Report before refusing: inspecting a skill whose content differs is
        // exactly what a dry run is for, so it must not need `force` to run.
        // Producing the report *is* its job — there is nothing pending to
        // approve — so it counts as applied.
        if dry_run {
            return Ok(RepointOutcome::Applied {
                content_changed: snapshot.content_hash.as_deref() != Some(new_hash.as_str()),
                resolved_subpath,
            });
        }

        commit_repoint_locked(
            store,
            &snapshot,
            &skill_dir,
            &new_hash,
            resolved_subpath.as_deref(),
            &source,
            force,
            approved_removals,
            &progress,
        )
    })();

    git_fetcher::cleanup_temp(&temp_dir);

    match outcome {
        Ok(RepointOutcome::Applied {
            content_changed,
            resolved_subpath,
        }) => Ok(SetSourceResult {
            skill_id: snapshot.id,
            name: snapshot.name,
            previous_source_type: snapshot.source_type,
            previous_source_ref: snapshot.source_ref,
            clone_url: parsed.clone_url,
            subpath: resolved_subpath,
            branch,
            revision: remote_revision,
            content_changed,
            dry_run,
            applied: true,
            pending_removals: Vec::new(),
            removal_approval: None,
            diff_entries: Vec::new(),
            // A dry run committed nothing, so this still answers about the
            // library copy as it was; after a real commit the swap has just
            // put it there, which reads the same.
            central_copy_exists: Path::new(&snapshot.central_path).is_dir(),
            duplicate_skill_name: None,
        }),
        Ok(RepointOutcome::Held {
            pending,
            approval,
            diff_entries,
            central_copy_exists,
            duplicate_skill_name,
            resolved_subpath,
        }) => Ok(SetSourceResult {
            skill_id: snapshot.id,
            name: snapshot.name,
            previous_source_type: snapshot.source_type,
            previous_source_ref: snapshot.source_ref,
            clone_url: parsed.clone_url,
            subpath: resolved_subpath,
            branch,
            revision: remote_revision,
            content_changed: false,
            dry_run,
            applied: false,
            pending_removals: pending,
            removal_approval: Some(approval),
            diff_entries,
            central_copy_exists,
            duplicate_skill_name,
        }),
        Err(e) => {
            if progress.marked_updating.get() {
                // Only clear `updating` if this call actually set it. A refusal
                // (bad subpath, nothing approved) touched nothing, so marking
                // the skill as errored would be a lie.
                //
                // `update_skill_check_state` always writes the revision column,
                // so it has to be given the one that matches whichever source
                // the row now describes: the new source's revision once the
                // re-point committed, otherwise the revision the old source
                // already had. Passing the newly resolved revision
                // unconditionally would file a commit from the new repo under a
                // skill still pointing at the old one; passing None would blank
                // a revision that is still valid.
                let revision = if progress.source_committed.get() {
                    Some(remote_revision.as_str())
                } else {
                    snapshot.remote_revision.as_deref()
                };
                // `failure_status` is what the caller wants the row to read
                // afterwards: "error" for the CLI, and for recovery the status it
                // already had — the source is still the dead local path, so the
                // buttons that can rescue it must stay reachable. The message
                // goes to `last_check_error` either way, so nothing is lost.
                let status = failure_status_for(
                    progress.source_committed.get(),
                    &snapshot.update_status,
                    failure_status,
                );
                let _ = store.update_skill_check_state(
                    skill_id,
                    revision,
                    status,
                    Some(&e.message),
                );
            }
            Err(e)
        }
    }
}

/// Apply a re-point to `snapshot`, or hold it for the user's answer.
///
/// Owns every write to the store, which is why it takes the lock itself: the
/// clone happened outside it and the row may have moved on, and staging,
/// comparing and swapping all have to see one consistent library. `snapshot` is
/// the row as it was read before the network phase — the re-read is refused
/// unless it still matches.
///
/// `skill_dir` must already be a resolved skill directory inside a fresh
/// checkout. `force` is the CLI's blunt "overwrite anyway" switch and is
/// deliberately not offered to the recovery path.
fn commit_repoint_locked(
    store: &SkillStore,
    snapshot: &SkillRecord,
    skill_dir: &Path,
    new_hash: &str,
    resolved_subpath: Option<&str>,
    source: &RepointSource,
    force: bool,
    approved_removals: Option<&str>,
    progress: &RepointProgress,
) -> Result<RepointOutcome, AppError> {
    let _lock = RepoLock::acquire_foreground("repoint skill source").map_err(AppError::db)?;

    // The clone happened outside the lock, so the skill may have been removed or
    // re-pointed meanwhile. Re-read and refuse to apply a decision made against
    // a stale snapshot.
    let current = store
        .get_skill_by_id(&snapshot.id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill was removed while fetching the source"))?;
    if current.central_path != snapshot.central_path
        || current.content_hash != snapshot.content_hash
        || current.source_type != snapshot.source_type
        || current.source_ref != snapshot.source_ref
    {
        return Err(AppError::invalid_input(
            "Skill changed while fetching the source; try again",
        ));
    }

    let central_path = Path::new(&current.central_path);
    let central_copy_exists = central_path.is_dir();
    // A recorded hash can still match while the files it described are gone —
    // a cleaner emptied the directory, or the library was moved. Writing nothing
    // and reporting success would leave a skill that is listed but cannot be
    // read, so a missing copy counts as a difference even when the hash agrees.
    let content_changed = !central_copy_exists
        || current.content_hash.as_deref() != Some(new_hash);

    store
        .update_skill_update_status(&current.id, "updating")
        .map_err(AppError::db)?;
    progress.marked_updating.set(true);

    // Stage first, then compare. The tree that lands in the library is the
    // installer's output, not the raw checkout — it drops `.git` and every
    // symlink — so comparing against the checkout would report a path as
    // surviving that the swap then removes.
    let staged_path = staged_path_for(&current.central_path);
    let install_result = if content_changed {
        Some(
            installer::install_skill_dir_to_destination(skill_dir, &current.name, &staged_path)
                .inspect_err(|_| {
                    let _ = remove_path_if_exists(&staged_path);
                })
                .map_err(AppError::io)?,
        )
    } else {
        None
    };
    let staged_guard = StagedPathGuard::new(&staged_path, install_result.is_some());

    // Always checked, even when the library copy keeps its content: every
    // copy-mode deployment is torn down and rebuilt from that tree, which loses
    // files just as effectively.
    let pending = pending_removals_for(
        store,
        &current,
        install_result.is_some().then_some(staged_path.as_path()),
    )?;

    // The library copy is the "before"; the staged tree is the "after". With
    // identical content there is no staged tree and this compares the library
    // against itself, which is empty — correct, there is nothing to show.
    let diff_entries = build_source_diff_entries(
        central_path,
        install_result
            .is_some()
            .then_some(staged_path.as_path())
            .unwrap_or(central_path),
    );

    // A confirmation answers one exact question: this revision, this list as
    // shown. It closes the window while the dialog is open — a push, or a file
    // that changes the list, re-asks.
    let approval = removal_approval_token(&source.remote_revision, &pending);
    // A differing tree is itself the thing to confirm, not just a source of
    // removals: two skills can share a name and a file list while sharing
    // nothing else, and at this point the library copy may be the only one left.
    if !force
        && (content_changed || !pending.is_empty())
        && approved_removals != Some(approval.as_str())
    {
        let duplicate_skill_name = duplicate_source_name(store, &current, source)?;
        // Put back exactly what was there. Hardcoding a status loses
        // `source_missing` — the only state recovery is reachable from — so
        // declining would hide the relink / find-source / detach buttons on the
        // next refresh, and `check_state` would also clear the recorded error
        // and check time that nothing here has re-established. Restoring only
        // the status leaves those alone.
        store
            .update_skill_update_status(&current.id, &current.update_status)
            .map_err(AppError::db)?;
        return Ok(RepointOutcome::Held {
            pending,
            approval,
            diff_entries,
            central_copy_exists,
            duplicate_skill_name,
            resolved_subpath: resolved_subpath.map(str::to_string),
        });
    }

    // Identical content needs no file work — swapping would rewrite the central
    // copy for a metadata-only change, and `installer` does not copy exactly the
    // set of files `content_hash` covers, so the rewrite could alter files while
    // still reporting `content_changed: false`.
    let description = match install_result.as_ref() {
        Some(result) => result.description.clone(),
        None => current.description.clone(),
    };

    if install_result.is_some() {
        swap_skill_directory(&staged_path, central_path)?;
        // Only now is it the library's; before this the guard still owns it.
        staged_guard.release();
    }

    store
        .update_skill_after_reinstall(
            &current.id,
            &current.name,
            description.as_deref(),
            &source.source_type,
            Some(&source.source_ref),
            Some(&source.source_ref_resolved),
            resolved_subpath,
            source.branch.as_deref(),
            Some(&source.remote_revision),
            Some(&source.remote_revision),
            Some(new_hash),
            "up_to_date",
        )
        .map_err(AppError::db)?;
    progress.source_committed.set(true);
    resync_copy_targets(store, &current.id)?;
    sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;

    Ok(RepointOutcome::Applied {
        content_changed,
        resolved_subpath: resolved_subpath.map(str::to_string),
    })
}

/// Another installed skill already tracking this exact source, by name.
///
/// Two rows on one upstream would update, deploy and back up independently, and
/// the auto-updater would visit both — so recovery says so before committing.
fn duplicate_source_name(
    store: &SkillStore,
    current: &SkillRecord,
    source: &RepointSource,
) -> Result<Option<String>, AppError> {
    Ok(store
        .get_skill_by_source_ref(&source.source_type, &source.source_ref)
        .map_err(AppError::db)?
        .filter(|other| other.id != current.id)
        .map(|other| other.name))
}

/// Re-import a local skill from its recorded source path.
///
/// `approved_removals` mirrors the git path: without it — or with one that no
/// longer matches the recomputed list — a re-import that would take away files
/// the source does not have stops and reports them.
pub fn reimport_local_skill_internal(
    store: &SkillStore,
    skill_id: &str,
    approved_removals: Option<&str>,
) -> Result<ReimportSkillResult, AppError> {
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;

    if !matches!(skill.source_type.as_str(), "local" | "import") {
        return Err(AppError::invalid_input(
            "Only local skills can be reimported",
        ));
    }

    let source_path = skill
        .source_ref
        .clone()
        .ok_or_else(|| AppError::not_found("Local skill is missing its original source path"))?;
    let path = PathBuf::from(&source_path);
    if !path.exists() {
        store
            .update_skill_check_state(
                &skill.id,
                None,
                "source_missing",
                Some("Original source path no longer exists"),
            )
            .map_err(AppError::db)?;
        return Err(AppError::not_found("Original source path no longer exists"));
    }

    store
        .update_skill_update_status(skill_id, "updating")
        .map_err(AppError::db)?;

    let result = (|| -> Result<(Vec<PendingRemoval>, Option<String>), AppError> {
        let _lock = RepoLock::acquire_foreground("reimport local skill").map_err(AppError::db)?;
        let staged_path = staged_path_for(&skill.central_path);
        let install_result =
            installer::install_from_local_to_destination(&path, Some(&skill.name), &staged_path)
                .inspect_err(|_| {
                    let _ = remove_path_if_exists(&staged_path);
                })
                .map_err(AppError::io)?;
        let staged_guard = StagedPathGuard::new(&staged_path, true);

        // Same replacement, same guard. Re-importing is explicit about the
        // *source*, not about discarding whatever has accumulated in the
        // library since — and for a local skill the "update" button runs this,
        // so leaving it uncovered would guard one path and not its twin.
        let pending = pending_removals_for(store, &skill, Some(&staged_path))?;
        // Bound to the set itself, not to a constant. A constant would match on
        // the approving call no matter what the recomputed list said, so a file
        // written while the dialog was open would be deleted having never been
        // shown — which is the whole failure this is here to prevent.
        let approval = removal_approval_token(REIMPORT_APPROVAL_DOMAIN, &pending);
        if !pending.is_empty() && approved_removals != Some(approval.as_str()) {
            // Restore the status this started from rather than asserting one:
            // declining changed nothing, so nothing about the skill's state
            // should read differently afterwards.
            store
                .update_skill_update_status(&skill.id, &skill.update_status)
                .map_err(AppError::db)?;
            return Ok((pending, Some(approval)));
        }

        swap_skill_directory(&staged_path, Path::new(&skill.central_path))?;
        // Only now is it the library's; before this the guard still owns it.
        staged_guard.release();
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
        resync_copy_targets(store, &skill.id)?;
        sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;
        Ok((Vec::new(), None))
    })();

    match result {
        Ok((pending_removals, removal_approval)) => Ok(ReimportSkillResult {
            skill: managed_skill_by_id(store, skill_id)?,
            pending_removals,
            removal_approval,
        }),
        Err(e) => {
            let _ = store.update_skill_check_state(skill_id, None, "error", Some(&e.message));
            Err(e)
        }
    }
}

pub fn store_installed_skill_unlocked(
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
        sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;

        if let Some(scenario_id) = active_scenario_id {
            if let Err(e) =
                super::presets::sync_skill_to_active_preset(store, scenario_id, &existing.id)
            {
                log::warn!("Failed to sync reinstalled skill to preset: {e}");
            }
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
    sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;

    if let Some(scenario_id) = active_scenario_id {
        if let Err(e) = super::presets::sync_skill_to_active_preset(store, scenario_id, &id) {
            log::warn!("Failed to sync newly installed skill to preset: {e}");
        }
    }

    Ok(id)
}

/// Check one skill end to end: resolve its remote, then write the status.
///
/// The caller must **not** hold the central-repo lock — the resolution here is
/// a network call. Paths that need the lock take it around
/// [`check_skill_update_internal_with_remote`] only, after prefetching.
pub fn check_skill_update_internal(
    store: &SkillStore,
    skill_id: &str,
    force: bool,
    proxy_url: Option<&str>,
) -> Result<ManagedSkillDto, AppError> {
    let prefetched = prefetch_skill_remote(store, skill_id, force, proxy_url);
    check_skill_update_internal_with_remote(store, skill_id, force, prefetched)
}

/// Write one skill's update status from an already-resolved remote revision.
///
/// This never touches the network — [`prefetch_skill_remote`] does that off the
/// central-repo lock, and callers hold the lock only for this write. A git
/// skill whose `prefetched` is missing or points at a remote the skill no
/// longer uses is left untouched for the next round.
pub fn check_skill_update_internal_with_remote(
    store: &SkillStore,
    skill_id: &str,
    force: bool,
    prefetched: Option<PrefetchedRemote>,
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
            let metadata_updated = skill.source_ref_resolved.as_deref()
                != Some(git_source.clone_url.as_str())
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

            // Apply the revision resolved off the lock — but only if the skill
            // still points at the remote it was resolved for. A reinstall keeps
            // the row and repoints its source, so a stale prefetch would record
            // a status computed against the wrong remote.
            //
            // When nothing usable was prefetched, skip the skill instead of
            // resolving here: every caller of this function holds the
            // central-repo lock, and a network call under that lock is the
            // 20s "busy" failure the off-lock split exists to remove (#315).
            // The next round picks the skill up.
            let Some(remote_result) = prefetched
                .filter(|prefetched| prefetched.key.matches(&git_source))
                .map(|prefetched| prefetched.result)
            else {
                log::debug!(
                    "check update: no usable prefetched remote for {}, skipping this round",
                    skill.id
                );
                return managed_skill_by_id(store, skill_id);
            };
            match remote_result {
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
                Err(message) => {
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
            let (status, error): (&str, Option<String>) = match skill.source_ref.as_deref() {
                Some(path) => {
                    let source_path = Path::new(path);
                    if !source_path.exists() {
                        (
                            "source_missing",
                            Some("Original source path no longer exists".to_string()),
                        )
                    } else {
                        match installer::hash_local_source(source_path) {
                            Ok(live_hash) => local_source_status(&skill, source_path, &live_hash),
                            Err(err) => ("error", Some(err.to_string())),
                        }
                    }
                }
                None => ("local_only", None),
            };
            store
                .update_skill_check_state(&skill.id, None, status, error.as_deref())
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

/// Classify a `local`/`import` skill against its freshly hashed source.
fn local_source_status(
    skill: &SkillRecord,
    source: &Path,
    live_hash: &str,
) -> (&'static str, Option<String>) {
    match skill.content_hash.as_deref() {
        None => ("local_only", None),
        Some(stored) if stored == live_hash => ("up_to_date", None),
        // The byte hashes disagree. Before offering an update, rule out the one
        // difference that is not one — see [`differs_only_by_line_endings`].
        Some(_) if differs_only_by_line_endings(source, Path::new(&skill.central_path)) => {
            ("up_to_date", None)
        }
        Some(_) => ("update_available", None),
    }
}

/// True when the original source and the library copy hold the same content in
/// two line-ending encodings, and nothing else.
///
/// A `local`/`import` skill is checked by hashing the user's own source path,
/// which is theirs to keep however they like — commonly a git working tree.
/// Git for Windows defaults to `core.autocrlf=true`, so on a Windows + macOS
/// pair the same checkout is CRLF on one machine and LF on the other while the
/// library copy (our own byte copy, or a copy synced from the other machine)
/// keeps the other encoding. Byte hashes then disagree forever and the skill
/// sits at "update available"; re-importing rewrites the library in the local
/// encoding, the other machine sees *its* copy drift, and the two devices push
/// the same skill back and forth. Nothing changed, so nothing should be offered.
///
/// Deliberately compares the two live trees rather than the stored hash: the
/// stored hash answers "what did we install?", and the question here is "do
/// these two directories differ right now?". Any failure to read either side
/// answers `false`, leaving the byte-hash verdict standing — this may only ever
/// suppress a false update, never assert sameness it could not establish.
fn differs_only_by_line_endings(source: &Path, central: &Path) -> bool {
    let (Ok(source_hash), Ok(central_hash)) = (
        installer::hash_local_source_eol_insensitive(source),
        crate::core::content_hash::hash_directory_eol_insensitive(central),
    ) else {
        return false;
    };
    source_hash == central_hash
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

pub fn git_source_from_skill(skill: &SkillRecord) -> Result<GitSkillSource, AppError> {
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
                // Prefer the branch resolved at install time — it survives
                // slash-branch tree URLs that the sync parse can't disambiguate.
                branch: skill.source_branch.clone().or(parsed.branch),
                subpath: skill.source_subpath.clone().or(parsed.subpath),
                locator_skill_id: None,
            })
        }
        "skillssh" => {
            let source_ref = skill.source_ref.as_ref().ok_or_else(|| {
                AppError::invalid_input("skills.sh skill is missing its source reference")
            })?;
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
        _ => Err(AppError::invalid_input(
            "Skill does not support git-based updates",
        )),
    }
}

fn skill_ssh_id(skill: &SkillRecord) -> Option<String> {
    if skill.source_type != "skillssh" {
        return None;
    }

    skill.source_ref.as_deref().and_then(|source_ref| {
        source_ref
            .rsplit_once('/')
            .map(|(_, skill_id)| skill_id.to_string())
    })
}

/// Return the list of individual skill directories to install from a resolved repo dir.
/// If `skill_dir` is itself a valid skill, returns `[skill_dir]`.
/// Otherwise recursively walks for skill dirs (e.g. `category/<skill>` layouts).
/// Returns an empty Vec when nothing is found — callers must handle that.
pub fn collect_git_skill_dirs(skill_dir: &Path) -> Vec<PathBuf> {
    if is_valid_skill_dir(skill_dir) {
        return vec![skill_dir.to_path_buf()];
    }
    let mut dirs = scanner::collect_skill_dirs(skill_dir);
    dirs.sort();
    dirs
}

/// Stable identifier for a discovered skill within a preview/confirm cycle.
/// Uses forward slashes regardless of platform so the frontend sees consistent keys.
pub fn skill_rel_key(skill_dir: &Path, dir: &Path) -> String {
    let rel = dir.strip_prefix(skill_dir).unwrap_or(dir);
    if rel.as_os_str().is_empty() {
        dir.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    } else {
        rel.to_string_lossy().replace('\\', "/")
    }
}

/// Validate and canonicalize a temp directory path used by the git preview/install flow.
/// Returns the canonicalized path if it passes security checks.
pub fn validate_clone_temp_path(temp_dir: &str) -> Result<PathBuf, AppError> {
    let raw_path = PathBuf::from(temp_dir);
    if !raw_path.exists() {
        return Err(AppError::invalid_input(
            "Clone session expired, please try again",
        ));
    }
    // Canonicalize to resolve symlinks and `..` segments before checking prefix.
    let temp_path = raw_path
        .canonicalize()
        .map_err(|_| AppError::invalid_input("Invalid temp directory"))?;

    // Preview confirmation must operate on an isolated checkout, never the repo cache.
    let expected_prefix = std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir());
    if temp_path.starts_with(&expected_prefix) {
        let dir_name_str = temp_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if dir_name_str.starts_with(git_fetcher::CLONE_TEMP_PREFIX) {
            return Ok(temp_path);
        }
    }

    Err(AppError::invalid_input("Invalid temp directory"))
}

/// Resolve which directory of a fresh checkout holds the skill.
///
/// Both inputs are attacker-reachable. `subpath` comes from the path segment of
/// a `…/tree/<branch>/<path>` URL the user pasted, and `skill_id` from the part
/// after `@` in a skills.sh shorthand, which `parse_skillssh_shorthand` does not
/// constrain to a single path segment. `Path::join` returns an absolute argument
/// verbatim and `..` segments climb out of the checkout, so both are checked for
/// containment: without that, `install` copies the resolved directory into the
/// library — which for a git-backed library can then be pushed to the user's
/// backup remote.
///
/// A subpath that stays inside the checkout but does not exist is a different
/// case: it is only recoverable when a skills.sh locator can find the skill by
/// id, which is how a skill that moved upstream is picked up again (#278).
/// Without a locator, falling through to repo-wide discovery would install or
/// update whatever that discovery happens to return — in a repository that
/// groups its skills, the entire `skills/` container.
pub fn resolve_skill_dir(
    repo_dir: &Path,
    subpath: Option<&str>,
    skill_id: Option<&str>,
) -> Result<PathBuf, AppError> {
    if let Some(subpath) = subpath {
        let candidate = repo_dir.join(subpath);
        if !path_guard::is_path_safe(repo_dir, &candidate) {
            return Err(AppError::invalid_input(format!(
                "Path '{subpath}' resolves outside the repository"
            )));
        }
        // With a locator to fall back on, the stored path is only taken when it
        // still holds a skill. An upstream reorganization can leave the path
        // occupied by a container or an unrelated directory, and copying that
        // over the installed skill is the same mistake as guessing — let the
        // locator look the skill up at its new home instead.
        let usable = if skill_id.is_some() {
            is_valid_skill_dir(&candidate)
        } else {
            candidate.is_dir()
        };
        if usable {
            return Ok(candidate);
        }
        if skill_id.is_none() {
            return Err(AppError::not_found(format!(
                "Path '{subpath}' does not exist in the repository"
            )));
        }
    }

    // `find_skill_dir` joins the locator id onto the checkout in several places
    // before falling back to a recursive search, so its answer is checked too.
    let resolved = git_fetcher::find_skill_dir(repo_dir, skill_id).map_err(AppError::git)?;
    if !path_guard::is_path_safe(repo_dir, &resolved) {
        return Err(AppError::invalid_input(
            "Resolved skill directory is outside the repository",
        ));
    }
    Ok(resolved)
}

pub fn resolve_skillssh_install_target(
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

pub fn staged_path_for(central_path: &str) -> PathBuf {
    let path = PathBuf::from(central_path);
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "skill".to_string());
    path.with_file_name(format!(".{file_name}.staged-{}", uuid::Uuid::new_v4()))
}

pub fn swap_skill_directory(staged_path: &Path, current_path: &Path) -> Result<(), AppError> {
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

pub fn resync_copy_targets(store: &SkillStore, skill_id: &str) -> Result<(), AppError> {
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

        // Recorded: this walks existing rows, so each path is one we wrote.
        // The row's mode is filtered to "copy" above, and sync_engine still
        // refuses if what is on disk no longer matches that record.
        sync_engine::sync_skill(
            &source,
            Path::new(&target.target_path),
            sync_engine::SyncMode::Copy,
            sync_engine::ReplacePolicy::Recorded {
                mode: target.mode.as_str(),
            },
        )
        .map_err(AppError::io)?;

        let updated_target = SkillTargetRecord {
            synced_at: Some(chrono::Utc::now().timestamp_millis()),
            status: "ok".to_string(),
            last_error: None,
            // Refresh the hash so the startup freshness check (#153)
            // sees this resync as up-to-date instead of stale.
            source_hash: skill.content_hash.clone(),
            ..target
        };
        store.insert_target(&updated_target).map_err(AppError::db)?;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_all_tags(store: State<'_, Arc<SkillStore>>) -> Result<Vec<String>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.get_all_tags().map_err(AppError::db)).await?
}

#[tauri::command]
pub async fn set_skill_tags(
    skill_id: String,
    tags: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || set_skill_tags_internal(&store, &skill_id, &tags))
        .await?
}

/// Shared implementation for GUI and CLI tag writes. Keeping the DB row and
/// its backup metadata under one repo lock prevents another process from
/// reindexing the half-written state between those two operations.
pub fn set_skill_tags_internal(
    store: &SkillStore,
    skill_id: &str,
    tags: &[String],
) -> Result<(), AppError> {
    let mut normalized = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if !tag.is_empty() && !normalized.iter().any(|existing| existing == tag) {
            normalized.push(tag.to_string());
        }
    }

    sync_metadata::with_repo_lock("set skill tags", || {
        store.set_tags_for_skill(skill_id, &normalized)?;
        sync_metadata::ensure_skill_metadata_unlocked(store, skill_id)
    })
    .map_err(AppError::db)
}

/// Globally rename a tag across all skills (used by the tag filter bar). If the
/// new name already exists, the tags are merged.
#[tauri::command]
pub async fn rename_tag(
    old_name: String,
    new_name: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        rename_tag_internal(&store, &old_name, &new_name).map(|_| ())
    })
    .await?
}

pub fn rename_tag_internal(
    store: &SkillStore,
    old_name: &str,
    new_name: &str,
) -> Result<Vec<String>, AppError> {
    let old_name = old_name.trim();
    let new_name = new_name.trim();
    if old_name.is_empty() || new_name.is_empty() {
        return Err(AppError::invalid_input("Tag name cannot be empty"));
    }
    if new_name == old_name {
        return Ok(Vec::new());
    }
    sync_metadata::with_repo_lock("rename tag", || {
        let affected = store.rename_tag(old_name, new_name)?;
        for skill_id in &affected {
            sync_metadata::ensure_skill_metadata_unlocked(store, skill_id)?;
        }
        Ok(affected)
    })
    .map_err(AppError::db)
}

/// Globally delete a tag from all skills (used by the tag filter bar).
#[tauri::command]
pub async fn delete_tag(name: String, store: State<'_, Arc<SkillStore>>) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || delete_tag_internal(&store, &name).map(|_| ()))
        .await?
}

pub fn delete_tag_internal(store: &SkillStore, name: &str) -> Result<Vec<String>, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::invalid_input("Tag name cannot be empty"));
    }
    sync_metadata::with_repo_lock("delete tag", || {
        let affected = store.delete_tag(name)?;
        for skill_id in &affected {
            sync_metadata::ensure_skill_metadata_unlocked(store, skill_id)?;
        }
        Ok(affected)
    })
    .map_err(AppError::db)
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
            if let Ok(Some(_)) = store.get_skill_by_central_path(&central_str) {
                skipped += 1;
                continue;
            }

            let install_result = (|| -> Result<String, AppError> {
                let _lock =
                    RepoLock::acquire_foreground("batch import skill").map_err(AppError::db)?;
                let result =
                    installer::install_from_local(dir, Some(&name)).map_err(AppError::io)?;
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
                store_installed_skill_unlocked(&store, &result, &metadata, None)
            })();

            match install_result {
                Ok(_) => imported += 1,
                Err(e) => errors.push(format!("{}: {}", name, e)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::ErrorKind;
    use crate::core::skill_store::ScenarioRecord;
    use std::fs;
    use tempfile::{tempdir, TempDir};

    struct TestRepo {
        _lock: std::sync::MutexGuard<'static, ()>,
        _tmp: TempDir,
        store: SkillStore,
    }

    impl Drop for TestRepo {
        fn drop(&mut self) {
            central_repo::set_test_base_dir_override(None);
        }
    }

    fn test_repo() -> TestRepo {
        let lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();
        TestRepo {
            _lock: lock,
            _tmp: tmp,
            store,
        }
    }

    fn write_skill_dir(name: &str) -> PathBuf {
        let dir = central_repo::skills_dir().join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), format!("---\nname: {name}\n---\n")).unwrap();
        dir
    }

    fn sample_skill(id: &str, name: &str, central_path: &Path) -> SkillRecord {
        SkillRecord {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            source_type: "import".to_string(),
            source_ref: Some(central_path.to_string_lossy().to_string()),
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: central_path.to_string_lossy().to_string(),
            content_hash: None,
            enabled: true,
            created_at: 1,
            updated_at: 1,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: None,
            last_check_error: None,
        }
    }

    #[test]
    fn batch_delete_removes_skills_targets_and_stale_metadata_once() {
        let repo = test_repo();
        let skill_one_dir = write_skill_dir("skill-one");
        let skill_two_dir = write_skill_dir("skill-two");
        repo.store
            .insert_skill(&sample_skill("skill-1", "skill-one", &skill_one_dir))
            .unwrap();
        repo.store
            .insert_skill(&sample_skill("skill-2", "skill-two", &skill_two_dir))
            .unwrap();

        let target_dir = repo._tmp.path().join("target-skill-one");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("SKILL.md"), "# target").unwrap();
        // A real directory pairs with a copy record; a symlink record over a
        // real directory is the #435 data-loss shape and now preserves it
        // (see deleting_a_skill_preserves_user_content_...).
        repo.store
            .insert_target(&SkillTargetRecord {
                id: "target-1".to_string(),
                skill_id: "skill-1".to_string(),
                tool: "cursor".to_string(),
                target_path: target_dir.to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: None,
            })
            .unwrap();

        sync_metadata::write_all_from_db_unlocked(&repo.store).unwrap();
        assert!(sync_metadata::metadata_dir()
            .join("skills/skill-1.json")
            .exists());
        assert!(sync_metadata::metadata_dir()
            .join("skills/skill-2.json")
            .exists());

        let result = delete_managed_skills_by_ids(
            &repo.store,
            &["skill-1".to_string(), "missing-skill".to_string()],
        )
        .unwrap();

        assert_eq!(result.deleted, 1);
        assert_eq!(result.failed, vec!["missing-skill".to_string()]);
        assert!(repo.store.get_skill_by_id("skill-1").unwrap().is_none());
        assert!(repo.store.get_skill_by_id("skill-2").unwrap().is_some());
        assert!(!skill_one_dir.exists());
        assert!(skill_two_dir.exists());
        assert!(!target_dir.exists());
        assert!(!sync_metadata::metadata_dir()
            .join("skills/skill-1.json")
            .exists());
        assert!(sync_metadata::metadata_dir()
            .join("skills/skill-2.json")
            .exists());
    }

    /// The whole point of the preflight: it must see the user's file in the
    /// library *and* the one in an agent's deployed copy, and say which is
    /// which — a bare filename does not tell anyone where to go and rescue it.
    #[test]
    fn the_preflight_covers_the_library_and_every_deployed_copy() {
        let repo = test_repo();
        let central = write_skill_dir("ppt-master");
        fs::create_dir_all(central.join("templates")).unwrap();
        fs::write(central.join("templates/mine.pptx"), "user work").unwrap();
        repo.store
            .insert_skill(&sample_skill("skill-1", "ppt-master", &central))
            .unwrap();

        // A copy-mode deployment the user has also written into.
        let target_dir = repo._tmp.path().join("agent/ppt-master");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("SKILL.md"), "x").unwrap();
        fs::write(target_dir.join("notes.md"), "notes in the agent copy").unwrap();
        repo.store
            .insert_target(&SkillTargetRecord {
                id: "t1".to_string(),
                skill_id: "skill-1".to_string(),
                tool: "claude_code".to_string(),
                target_path: target_dir.to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: None,
            })
            .unwrap();

        // The new version carries only SKILL.md.
        let staged = repo._tmp.path().join("staged");
        fs::create_dir_all(&staged).unwrap();
        fs::write(staged.join("SKILL.md"), "v2").unwrap();

        let skill = repo.store.get_skill_by_id("skill-1").unwrap().unwrap();
        let pending = pending_removals_for(&repo.store, &skill, Some(&staged)).unwrap();

        let found: Vec<(String, String)> = pending
            .iter()
            .map(|p| (p.location.clone(), p.path.replace('\\', "/")))
            .collect();
        assert!(
            found.contains(&(LIBRARY_LOCATION.to_string(), "templates/".to_string())),
            "the library's own directory must be reported: {found:?}"
        );
        assert!(
            found.contains(&("claude_code".to_string(), "notes.md".to_string())),
            "the agent copy is torn down and rebuilt too: {found:?}"
        );
    }

    /// With no content change nothing is swapped, so the library keeps what it
    /// has — but the deployments are still rebuilt from it, which is its own way
    /// to lose a file.
    #[test]
    fn a_metadata_only_update_still_checks_the_deployed_copies() {
        let repo = test_repo();
        let central = write_skill_dir("stable");
        repo.store
            .insert_skill(&sample_skill("skill-1", "stable", &central))
            .unwrap();

        let target_dir = repo._tmp.path().join("agent/stable");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("SKILL.md"), "x").unwrap();
        fs::write(target_dir.join("mine.txt"), "only in the agent copy").unwrap();
        repo.store
            .insert_target(&SkillTargetRecord {
                id: "t1".to_string(),
                skill_id: "skill-1".to_string(),
                tool: "cursor".to_string(),
                target_path: target_dir.to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: None,
            })
            .unwrap();

        let skill = repo.store.get_skill_by_id("skill-1").unwrap().unwrap();
        // `None` staged: the library is unchanged, and is itself the baseline.
        let pending = pending_removals_for(&repo.store, &skill, None).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].location, "cursor");
        assert_eq!(pending[0].path, "mine.txt");
    }

    /// Symlink-mode deployments are not copied over, so they are not at risk and
    /// must not generate noise.
    #[test]
    fn symlink_deployments_are_not_reported() {
        let repo = test_repo();
        let central = write_skill_dir("linked");
        repo.store
            .insert_skill(&sample_skill("skill-1", "linked", &central))
            .unwrap();

        let target_dir = repo._tmp.path().join("agent/linked");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("whatever.md"), "x").unwrap();
        repo.store
            .insert_target(&SkillTargetRecord {
                id: "t1".to_string(),
                skill_id: "skill-1".to_string(),
                tool: "grok".to_string(),
                target_path: target_dir.to_string_lossy().to_string(),
                mode: "symlink".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: None,
            })
            .unwrap();

        let skill = repo.store.get_skill_by_id("skill-1").unwrap().unwrap();
        assert!(pending_removals_for(&repo.store, &skill, None)
            .unwrap()
            .is_empty());
    }

    /// An approval answers one exact question: this revision, this list.
    #[test]
    fn an_approval_does_not_carry_to_a_different_revision_or_list() {
        let a = vec![PendingRemoval {
            location: LIBRARY_LOCATION.to_string(),
            path: "templates/mine.pptx".to_string(),
        }];
        let mut b = a.clone();
        b.push(PendingRemoval {
            location: LIBRARY_LOCATION.to_string(),
            path: "templates/another.pptx".to_string(),
        });

        assert_eq!(
            removal_approval_token("rev1", &a),
            removal_approval_token("rev1", &a),
            "the same question must produce the same token"
        );
        assert_ne!(
            removal_approval_token("rev1", &a),
            removal_approval_token("rev2", &a),
            "upstream moved on"
        );
        assert_ne!(
            removal_approval_token("rev1", &a),
            removal_approval_token("rev1", &b),
            "the skill wrote another file while the dialog was open"
        );
    }

    /// Drives the real `reimport_local_skill_internal`, because the bug this
    /// guards against was in the wiring, not the hash: the approval was compared
    /// against a constant, so the recomputed list was never consulted. A test
    /// that only calls the token function twice passes either way.
    #[test]
    fn a_stale_reimport_approval_does_not_authorize_a_grown_list() {
        let repo = test_repo();
        let source = repo._tmp.path().join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "---\nname: gen\n---\n").unwrap();

        let central = write_skill_dir("gen");
        fs::write(central.join("mine.txt"), "user work").unwrap();
        let mut record = sample_skill("skill-1", "gen", &central);
        record.source_ref = Some(source.to_string_lossy().to_string());
        repo.store.insert_skill(&record).unwrap();

        // First attempt: held, with a token for the list the user is shown.
        let first = reimport_local_skill_internal(&repo.store, "skill-1", None).unwrap();
        assert_eq!(first.pending_removals.len(), 1);
        let shown = first.removal_approval.clone().unwrap();
        assert!(central.join("mine.txt").is_file(), "nothing may be touched");

        // The skill writes another file while the dialog is open.
        fs::write(central.join("appeared-later.txt"), "also mine").unwrap();

        // The old approval must not cover it.
        let second =
            reimport_local_skill_internal(&repo.store, "skill-1", Some(&shown)).unwrap();
        assert_eq!(
            second.pending_removals.len(),
            2,
            "the grown list must be shown again, not silently applied"
        );
        assert!(central.join("appeared-later.txt").is_file());
        assert!(central.join("mine.txt").is_file());

        // Approving the list actually shown does go through.
        let approved = second.removal_approval.clone().unwrap();
        let third =
            reimport_local_skill_internal(&repo.store, "skill-1", Some(&approved)).unwrap();
        assert!(third.pending_removals.is_empty());
        assert!(!central.join("mine.txt").exists(), "the approved removal applies");
    }

    fn write_skill_at(root: &Path, rel: &str) -> PathBuf {
        let dir = root.join(rel);
        fs::create_dir_all(&dir).unwrap();
        let basename = dir.file_name().unwrap().to_string_lossy().to_string();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {basename}\n---\n"),
        )
        .unwrap();
        dir
    }

    #[test]
    fn collect_git_skill_dirs_finds_nested_categories() {
        // Mirrors mattpocock/skills layout: skills/<category>/<skill>/SKILL.md.
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        write_skill_at(root, "in-progress/foo");
        write_skill_at(root, "in-progress/bar");
        write_skill_at(root, "stable/baz");

        let dirs = collect_git_skill_dirs(root);
        let keys: Vec<String> = dirs.iter().map(|d| skill_rel_key(root, d)).collect();
        assert_eq!(dirs.len(), 3, "should find skills two levels deep");
        assert!(keys.contains(&"in-progress/foo".to_string()));
        assert!(keys.contains(&"in-progress/bar".to_string()));
        assert!(keys.contains(&"stable/baz".to_string()));
    }

    #[test]
    fn collect_git_skill_dirs_returns_self_when_root_is_skill() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("SKILL.md"), "---\nname: x\n---").unwrap();
        let dirs = collect_git_skill_dirs(root);
        assert_eq!(dirs, vec![root.to_path_buf()]);
    }

    #[test]
    fn collect_git_skill_dirs_returns_empty_when_no_skills() {
        // Previously this case returned [skill_dir] as a bogus fallback,
        // which then surfaced a non-skill category dir as installable.
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("empty-category")).unwrap();
        let dirs = collect_git_skill_dirs(root);
        assert!(dirs.is_empty(), "no fallback to scan root when empty");
    }

    #[test]
    fn skill_rel_key_uses_forward_slashes() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("repo");
        let nested = root.join("a").join("b");
        let key = skill_rel_key(&root, &nested);
        assert_eq!(key, "a/b");
    }

    #[test]
    fn skill_rel_key_disambiguates_same_basename_across_categories() {
        // Two skills with the same dir basename in different categories must
        // produce distinct rel keys — that's the point of using rel paths.
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let a_foo = write_skill_at(root, "category-a/foo");
        let b_foo = write_skill_at(root, "category-b/foo");

        let dirs = collect_git_skill_dirs(root);
        assert_eq!(dirs.len(), 2);

        let k_a = skill_rel_key(root, &a_foo);
        let k_b = skill_rel_key(root, &b_foo);
        assert_ne!(k_a, k_b);
        assert_eq!(k_a, "category-a/foo");
        assert_eq!(k_b, "category-b/foo");
    }

    // ── RemoteKey dedup (batch check_all fan-out) ──

    fn source(clone_url: &str, branch: Option<&str>, subpath: Option<&str>) -> GitSkillSource {
        GitSkillSource {
            clone_url: clone_url.to_string(),
            branch: branch.map(str::to_string),
            subpath: subpath.map(str::to_string),
            locator_skill_id: None,
        }
    }

    /// The whole point of keying Phase A by `RemoteKey`: skills installed from
    /// different subdirectories of the same monorepo (same clone_url + branch)
    /// must collapse to one network query, while a different branch stays
    /// distinct. This is what turns 4 `mattpocock/skills` skills into 1
    /// `ls-remote` instead of 4.
    #[test]
    fn remote_key_dedups_by_url_and_branch_ignoring_subpath() {
        let mut per_remote: HashMap<RemoteKey, usize> = HashMap::new();
        let skills = [
            source("https://github.com/mattpocock/skills.git", None, Some("a")),
            source("https://github.com/mattpocock/skills.git", None, Some("b")),
            source("https://github.com/mattpocock/skills.git", None, None),
            source("https://github.com/vercel/ai.git", None, None),
            // Same repo, different branch → must NOT collapse with the None-branch group.
            source(
                "https://github.com/mattpocock/skills.git",
                Some("next"),
                None,
            ),
        ];
        for s in skills {
            *per_remote.entry(RemoteKey::from(s)).or_insert(0) += 1;
        }

        assert_eq!(per_remote.len(), 3, "distinct remotes to query");
        assert_eq!(
            per_remote[&RemoteKey {
                clone_url: "https://github.com/mattpocock/skills.git".to_string(),
                branch: None,
            }],
            3,
            "three subpaths of one repo/branch share a single query"
        );
        assert_eq!(
            per_remote[&RemoteKey {
                clone_url: "https://github.com/mattpocock/skills.git".to_string(),
                branch: Some("next".to_string()),
            }],
            1,
            "a different branch is a separate remote"
        );
    }

    fn remote(url: &str, branch: Option<&str>) -> RemoteKey {
        RemoteKey {
            clone_url: url.to_string(),
            branch: branch.map(|b| b.to_string()),
        }
    }

    /// Work-stealing must cover every remote exactly once and collect each
    /// resolver result under its own key — this exercises the real concurrent
    /// loop, not just `RemoteKey`'s hashing.
    #[test]
    fn resolve_concurrent_resolves_every_remote_exactly_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let remotes: Vec<RemoteKey> = (0..20)
            .map(|i| remote(&format!("https://example.test/r{i}"), None))
            .collect();
        let calls = AtomicUsize::new(0);

        let out = resolve_concurrent(remotes.clone(), |key| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(format!("rev:{}", key.clone_url))
        });

        assert_eq!(
            calls.load(Ordering::Relaxed),
            remotes.len(),
            "each remote resolved exactly once"
        );
        assert_eq!(out.len(), remotes.len());
        for key in &remotes {
            assert!(matches!(out.get(key), Some(Ok(v)) if *v == format!("rev:{}", key.clone_url)));
        }
    }

    /// A single remote failing must be stored as `Err` for that key alone and
    /// never abort the batch (the "检查全部 both crawled and popped failures" fix
    /// depends on this isolation).
    #[test]
    fn resolve_concurrent_isolates_per_remote_failures() {
        let ok = remote("https://example.test/ok", None);
        let bad = remote("https://example.test/bad", Some("main"));

        let out = resolve_concurrent(vec![ok.clone(), bad.clone()], |key| {
            if key.clone_url.ends_with("/bad") {
                Err("boom".to_string())
            } else {
                Ok("rev".to_string())
            }
        });

        assert!(matches!(out.get(&ok), Some(Ok(v)) if v == "rev"));
        assert!(matches!(out.get(&bad), Some(Err(e)) if e == "boom"));
    }

    /// The resolutions must genuinely overlap: with several remotes and a
    /// resolver that lingers, more than one worker is inside `resolve` at once.
    /// Because `resolve_concurrent` holds no `RepoLock`, this is also the proof
    /// that the network step runs off the central-repo lock.
    #[test]
    fn resolve_concurrent_runs_remotes_in_parallel() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let remotes: Vec<RemoteKey> = (0..8).map(|i| remote(&format!("r{i}"), None)).collect();
        let in_flight = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);

        let out = resolve_concurrent(remotes, |_key| {
            let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(20));
            in_flight.fetch_sub(1, Ordering::SeqCst);
            Ok("rev".to_string())
        });

        assert_eq!(out.len(), 8);
        assert!(
            peak.load(Ordering::SeqCst) >= 2,
            "expected concurrent resolution, peak in-flight was {}",
            peak.load(Ordering::SeqCst)
        );
    }

    // ── Applying a prefetched remote under the lock ──

    /// A git-backed skill pinned at `old-rev` on `remote_url`.
    fn insert_git_skill(store: &SkillStore, id: &str, remote_url: &str) {
        let dir = write_skill_dir(id);
        let mut skill = sample_skill(id, id, &dir);
        skill.source_type = "git".to_string();
        skill.source_ref = Some(remote_url.to_string());
        skill.source_ref_resolved = Some(remote_url.to_string());
        skill.source_revision = Some("old-rev".to_string());
        skill.update_status = "unknown".to_string();
        store.insert_skill(&skill).unwrap();
    }

    fn prefetch(url: &str, revision: &str) -> Option<PrefetchedRemote> {
        Some(PrefetchedRemote {
            key: remote(url, None),
            result: Ok(revision.to_string()),
        })
    }

    /// The happy path: a prefetch resolved for the skill's own remote is applied.
    #[test]
    fn matching_prefetched_remote_is_applied() {
        let repo = test_repo();
        insert_git_skill(&repo.store, "skill-1", "https://example.test/a.git");

        let dto = check_skill_update_internal_with_remote(
            &repo.store,
            "skill-1",
            false,
            prefetch("https://example.test/a.git", "new-rev"),
        )
        .unwrap();

        assert_eq!(dto.update_status, "update_available");
        let stored = repo.store.get_skill_by_id("skill-1").unwrap().unwrap();
        assert_eq!(stored.remote_revision.as_deref(), Some("new-rev"));
    }

    /// A reinstall between the off-lock resolve and this write keeps the skill's
    /// row but repoints its source. The revision resolved for the *old* remote
    /// must not be recorded against the new one — it would show a fabricated
    /// "up to date"/"update available" for a source it was never read from.
    #[test]
    fn prefetched_remote_for_a_different_source_is_discarded() {
        let repo = test_repo();
        insert_git_skill(&repo.store, "skill-1", "https://example.test/new.git");

        let dto = check_skill_update_internal_with_remote(
            &repo.store,
            "skill-1",
            false,
            prefetch("https://example.test/old.git", "rev-of-old-remote"),
        )
        .unwrap();

        assert_eq!(
            dto.update_status, "unknown",
            "status left for the next round"
        );
        let stored = repo.store.get_skill_by_id("skill-1").unwrap().unwrap();
        assert_eq!(
            stored.remote_revision, None,
            "no revision from a stale remote"
        );
        assert_eq!(stored.last_checked_at, None, "the check did not complete");
    }

    /// Insert a `local` skill whose library copy is `central_body` and whose
    /// original source path holds `source_body`, with the stored hash recorded
    /// from the library copy exactly as an install would leave it.
    fn insert_local_skill(repo: &TestRepo, id: &str, central_body: &str, source_body: &str) {
        let central = central_repo::skills_dir().join(id);
        fs::create_dir_all(&central).unwrap();
        fs::write(central.join("SKILL.md"), central_body).unwrap();

        let source = repo._tmp.path().join(format!("{id}-source"));
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), source_body).unwrap();

        let mut skill = sample_skill(id, id, &central);
        skill.source_type = "local".to_string();
        skill.source_ref = Some(source.to_string_lossy().to_string());
        skill.content_hash = Some(crate::core::content_hash::hash_directory(&central).unwrap());
        skill.update_status = "unknown".to_string();
        repo.store.insert_skill(&skill).unwrap();
    }

    /// Drives the real check, because the wiring is where this can go wrong:
    /// the tiebreaker can be correct and still never be consulted. A Windows
    /// checkout of the same skill is CRLF while the library copy synced from a
    /// Mac is LF — byte hashes disagree, but there is no update to offer, and
    /// offering one starts a re-import ping-pong between the two machines.
    #[test]
    fn a_local_source_that_differs_only_in_line_endings_is_up_to_date() {
        let repo = test_repo();
        insert_local_skill(
            &repo,
            "skill-1",
            "---\nname: skill-1\n---\nbody\n",
            "---\r\nname: skill-1\r\n---\r\nbody\r\n",
        );

        let dto =
            check_skill_update_internal_with_remote(&repo.store, "skill-1", true, None).unwrap();

        assert_eq!(dto.update_status, "up_to_date");
    }

    /// #502: `npm install` in a local skill's source is not an update.
    #[test]
    fn a_local_source_with_only_installed_dependencies_is_up_to_date() {
        let repo = test_repo();
        let body = "---\nname: skill-1\n---\nbody\n";
        insert_local_skill(&repo, "skill-1", body, body);
        let deps = repo._tmp.path().join("skill-1-source/node_modules/pkg");
        fs::create_dir_all(&deps).unwrap();
        fs::write(deps.join("index.js"), "module.exports = 1;\n").unwrap();

        let dto =
            check_skill_update_internal_with_remote(&repo.store, "skill-1", true, None).unwrap();

        assert_eq!(dto.update_status, "up_to_date");
    }

    /// A vanished library copy still has an update to offer. This is a
    /// regression guard on the end-to-end path, not proof of the empty-tree
    /// guard itself — a non-empty source cannot collide with an empty library,
    /// so what pins that collision is
    /// `content_hash::tests::an_empty_or_missing_directory_has_no_tiebreaker_hash`.
    #[test]
    fn a_missing_library_copy_is_not_up_to_date() {
        let repo = test_repo();
        insert_local_skill(
            &repo,
            "skill-1",
            "---\nname: skill-1\n---\nbody\n",
            "---\r\nname: skill-1\r\n---\r\nbody\r\n",
        );
        fs::remove_dir_all(central_repo::skills_dir().join("skill-1")).unwrap();

        let dto =
            check_skill_update_internal_with_remote(&repo.store, "skill-1", true, None).unwrap();

        assert_eq!(dto.update_status, "update_available");
    }

    /// The other half of the same wiring: the tiebreaker must not swallow a
    /// real edit. Without this, "always up to date" would pass the test above.
    #[test]
    fn a_local_source_with_a_real_edit_still_reports_an_update() {
        let repo = test_repo();
        insert_local_skill(
            &repo,
            "skill-1",
            "---\nname: skill-1\n---\nbody\n",
            "---\r\nname: skill-1\r\n---\r\nbody, rewritten\r\n",
        );

        let dto =
            check_skill_update_internal_with_remote(&repo.store, "skill-1", true, None).unwrap();

        assert_eq!(dto.update_status, "update_available");
    }

    /// A remote that failed to resolve off the lock still has to land as an
    /// `error` status here, not be swallowed as "nothing to apply" — the batch
    /// check counts that error and the card shows the reason.
    #[test]
    fn failed_prefetch_for_the_current_source_records_the_error() {
        let repo = test_repo();
        insert_git_skill(&repo.store, "skill-1", "https://example.test/a.git");

        let err = check_skill_update_internal_with_remote(
            &repo.store,
            "skill-1",
            false,
            Some(PrefetchedRemote {
                key: remote("https://example.test/a.git", None),
                result: Err("could not read from remote".to_string()),
            }),
        )
        .unwrap_err();

        assert!(err.message.contains("could not read from remote"));
        let stored = repo.store.get_skill_by_id("skill-1").unwrap().unwrap();
        assert_eq!(stored.update_status, "error");
        assert_eq!(
            stored.last_check_error.as_deref(),
            Some("could not read from remote")
        );
    }

    /// Callers hold the central-repo lock across this write, so a git skill with
    /// nothing prefetched must be skipped rather than resolved inline — that
    /// inline call is the lock-held network round-trip behind the 20s "busy"
    /// failures (#315).
    #[test]
    fn missing_prefetch_never_resolves_under_the_lock() {
        let repo = test_repo();
        insert_git_skill(&repo.store, "skill-1", "https://example.test/a.git");

        let dto =
            check_skill_update_internal_with_remote(&repo.store, "skill-1", false, None).unwrap();

        assert_eq!(dto.update_status, "unknown");
        let stored = repo.store.get_skill_by_id("skill-1").unwrap().unwrap();
        assert_eq!(stored.last_checked_at, None, "no network, no write");
    }

    fn write_skill(dir: &Path, name: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: d\n---\nbody\n"),
        )
        .unwrap();
    }

    #[test]
    fn repoint_accepts_a_valid_subpath() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path();
        write_skill(&repo.join("note-manager"), "note-manager");

        let resolved = resolve_repoint_skill_dir(repo, Some("note-manager")).unwrap();
        assert_eq!(resolved, repo.join("note-manager"));
    }

    #[test]
    fn repoint_accepts_repo_root_when_it_is_a_skill() {
        let tmp = tempdir().unwrap();
        write_skill(tmp.path(), "root-skill");

        let resolved = resolve_repoint_skill_dir(tmp.path(), None).unwrap();
        assert_eq!(resolved, tmp.path());
    }

    #[test]
    fn repoint_rejects_repo_root_without_skill_md() {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("some-dir")).unwrap();

        let err = resolve_repoint_skill_dir(tmp.path(), None).unwrap_err();
        assert!(
            err.message.contains("not a skill directory"),
            "{}",
            err.message
        );
    }

    #[test]
    fn repoint_rejects_missing_subpath_instead_of_falling_back() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path();
        // A real skill exists elsewhere: the lenient resolver would discover it.
        write_skill(&repo.join("other"), "other");

        let err = resolve_repoint_skill_dir(repo, Some("typo")).unwrap_err();
        assert!(err.message.contains("does not exist"), "{}", err.message);
    }

    #[test]
    fn repoint_rejects_subpath_that_is_not_a_skill_dir() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path();
        fs::create_dir_all(repo.join("docs")).unwrap();

        let err = resolve_repoint_skill_dir(repo, Some("docs")).unwrap_err();
        assert!(
            err.message.contains("not a skill directory"),
            "{}",
            err.message
        );
    }

    #[test]
    fn repoint_rejects_absolute_subpath() {
        let tmp = tempdir().unwrap();
        let outside = tmp.path().join("outside");
        write_skill(&outside, "outside");
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();

        // `Path::join` returns an absolute argument verbatim, so without the
        // guard this would install a directory from outside the checkout.
        let err = resolve_repoint_skill_dir(&repo, Some(outside.to_str().unwrap())).unwrap_err();
        assert!(
            err.message.contains("outside the repository"),
            "{}",
            err.message
        );
    }

    #[test]
    fn repoint_rejects_parent_traversal_subpath() {
        let tmp = tempdir().unwrap();
        write_skill(&tmp.path().join("outside"), "outside");
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();

        let err = resolve_repoint_skill_dir(&repo, Some("../outside")).unwrap_err();
        assert!(
            err.message.contains("outside the repository"),
            "{}",
            err.message
        );
    }

    #[cfg(unix)]
    #[test]
    fn repoint_rejects_symlink_escaping_the_checkout() {
        let tmp = tempdir().unwrap();
        let outside = tmp.path().join("outside");
        write_skill(&outside, "outside");
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        std::os::unix::fs::symlink(&outside, repo.join("link")).unwrap();

        let err = resolve_repoint_skill_dir(&repo, Some("link")).unwrap_err();
        assert!(
            err.message.contains("outside the repository"),
            "{}",
            err.message
        );
    }

    // ── resolve_skill_dir: install / update / preview resolution ───────────
    //
    // Both inputs reach this from a URL the user pasted. The cases below are
    // the ones that let a crafted or merely wrong URL resolve to something the
    // caller did not ask for.

    #[test]
    fn resolve_accepts_a_subpath_inside_the_checkout() {
        let tmp = tempdir().unwrap();
        write_skill(&tmp.path().join("skills").join("pdf"), "pdf");

        let resolved = resolve_skill_dir(tmp.path(), Some("skills/pdf"), None).unwrap();
        assert_eq!(resolved, tmp.path().join("skills").join("pdf"));
    }

    #[test]
    fn resolve_still_returns_a_container_for_enumeration() {
        // preview/confirm install walk a container to list the skills inside
        // it, so an existing non-skill directory must keep resolving.
        let tmp = tempdir().unwrap();
        write_skill(&tmp.path().join("skills").join("pdf"), "pdf");
        write_skill(&tmp.path().join("skills").join("docx"), "docx");

        let resolved = resolve_skill_dir(tmp.path(), Some("skills"), None).unwrap();
        assert_eq!(resolved, tmp.path().join("skills"));
        // What preview/confirm actually do with that container.
        assert_eq!(collect_git_skill_dirs(&resolved).len(), 2);
    }

    #[test]
    fn resolve_rejects_parent_traversal_with_and_without_a_locator() {
        let tmp = tempdir().unwrap();
        write_skill(&tmp.path().join("outside"), "outside");
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();

        // A locator must not soften the traversal check: the escaping path is
        // refused either way, never quietly ignored in favour of discovery.
        for locator in [None, Some("outside")] {
            let err = resolve_skill_dir(&repo, Some("../outside"), locator).unwrap_err();
            assert!(
                err.message.contains("outside the repository"),
                "locator {locator:?}: {}",
                err.message
            );
        }
    }

    #[test]
    fn resolve_rejects_absolute_subpath() {
        let tmp = tempdir().unwrap();
        let outside = tmp.path().join("outside");
        write_skill(&outside, "outside");
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();

        let err =
            resolve_skill_dir(&repo, Some(outside.to_str().unwrap()), None).unwrap_err();
        assert!(
            err.message.contains("outside the repository"),
            "{}",
            err.message
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_subpath_symlinked_out_of_the_checkout() {
        let tmp = tempdir().unwrap();
        let outside = tmp.path().join("outside");
        write_skill(&outside, "outside");
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        std::os::unix::fs::symlink(&outside, repo.join("link")).unwrap();

        let err = resolve_skill_dir(&repo, Some("link"), None).unwrap_err();
        assert!(
            err.message.contains("outside the repository"),
            "{}",
            err.message
        );
    }

    #[test]
    fn resolve_rejects_a_locator_that_escapes_the_checkout() {
        // `owner/repo@../../x` survives parse_skillssh_shorthand, which only
        // checks the owner/repo half, so the locator itself can climb out.
        let tmp = tempdir().unwrap();
        write_skill(&tmp.path().join("outside"), "outside");
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();

        let err = resolve_skill_dir(&repo, None, Some("../outside")).unwrap_err();
        assert!(
            err.message.contains("outside the repository"),
            "{}",
            err.message
        );
    }

    #[test]
    fn resolve_refuses_a_missing_subpath_instead_of_discovering_the_container() {
        // The measured bug: a tree URL naming a directory that does not exist
        // installed the whole `skills/` container as one skill.
        let tmp = tempdir().unwrap();
        write_skill(&tmp.path().join("skills").join("pdf"), "pdf");

        let err = resolve_skill_dir(tmp.path(), Some("artifacts-builder"), None).unwrap_err();
        assert!(err.message.contains("does not exist"), "{}", err.message);
    }

    #[test]
    fn resolve_lets_a_locator_recover_a_skill_that_moved_upstream() {
        // #278's recovery path: the stored subpath is stale because upstream
        // reorganized, and the locator finds the skill at its new home.
        let tmp = tempdir().unwrap();
        write_skill(&tmp.path().join("skills").join("db"), "db");

        let resolved = resolve_skill_dir(tmp.path(), Some("db"), Some("db")).unwrap();
        assert_eq!(resolved, tmp.path().join("skills").join("db"));
    }

    #[test]
    fn resolve_lets_a_locator_override_a_path_that_is_no_longer_the_skill() {
        // The harder half of a reorganization: the stored path still exists,
        // but upstream turned it into a container and moved the skill. Taking
        // the path would copy the container over the installed skill.
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("db")).unwrap();
        write_skill(&tmp.path().join("db").join("nested"), "nested");
        write_skill(&tmp.path().join("skills").join("db"), "db");

        let resolved = resolve_skill_dir(tmp.path(), Some("db"), Some("db")).unwrap();
        assert_eq!(resolved, tmp.path().join("skills").join("db"));
    }

    #[test]
    fn resolve_errors_when_the_locator_finds_nothing() {
        // Still #278: no match must not fall through to a container or root.
        let tmp = tempdir().unwrap();
        write_skill(&tmp.path().join("skills").join("db"), "db");

        let err = resolve_skill_dir(tmp.path(), Some("gone"), Some("nope-not-here")).unwrap_err();
        assert!(err.message.contains("not found"), "{}", err.message);
    }

    /// #435: removing a library skill used to delete whatever sat at each
    /// recorded target. A real directory that replaced our symlink is the
    /// user's local fork — it must survive the removal.
    #[test]
    fn deleting_a_skill_preserves_user_content_that_replaced_a_recorded_link() {
        let repo = test_repo();
        let dir = write_skill_dir("my-skill");
        repo.store
            .insert_skill(&sample_skill("s1", "my-skill", &dir))
            .unwrap();

        let agent_tmp = tempdir().unwrap();
        let target = agent_tmp.path().join("my-skill");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("mine.txt"), "DO_NOT_OVERWRITE").unwrap();
        repo.store
            .insert_target(&SkillTargetRecord {
                id: "t1".to_string(),
                skill_id: "s1".to_string(),
                tool: "test_agent".to_string(),
                target_path: target.to_string_lossy().to_string(),
                mode: "symlink".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: None,
            })
            .unwrap();

        let result = delete_managed_skills_by_ids(&repo.store, &["s1".to_string()]).unwrap();

        assert_eq!(result.deleted, 1);
        assert!(result.failed.is_empty());
        assert_eq!(
            fs::read_to_string(target.join("mine.txt")).unwrap(),
            "DO_NOT_OVERWRITE",
            "the user's local fork must survive the skill removal"
        );
        assert!(repo.store.get_targets_for_skill("s1").unwrap().is_empty());
    }

    //── Recovery: re-pointing a skill whose local path is gone ──

    /// A `local` row as the app leaves one after an install: a real content
    /// hash, reported missing. `sample_skill` leaves the hash empty, which
    /// would make every re-point look like a content change.
    fn lost_local_skill(
        repo: &TestRepo,
        id: &str,
        name: &str,
        body: &str,
    ) -> (SkillRecord, PathBuf) {
        let central = write_skill_dir(name);
        fs::write(central.join("SKILL.md"), body).unwrap();
        let mut skill = sample_skill(id, name, &central);
        skill.content_hash = Some(crate::core::content_hash::hash_directory(&central).unwrap());
        skill.update_status = "source_missing".to_string();
        skill.last_check_error = Some("Original source path no longer exists".to_string());
        repo.store.insert_skill(&skill).unwrap();
        (skill, central)
    }

    /// A fresh checkout, standing in for what `clone` would have left behind
    /// once the skill directory has been located inside it.
    fn checkout(repo: &TestRepo, files: &[(&str, &str)]) -> PathBuf {
        let dir = repo._tmp.path().join("checkout");
        fs::create_dir_all(&dir).unwrap();
        for (name, body) in files {
            let path = dir.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }
        dir
    }

    fn git_source(revision: &str) -> RepointSource {
        RepointSource {
            source_type: "git".to_string(),
            source_ref: "https://github.com/acme/skills".to_string(),
            source_ref_resolved: "https://github.com/acme/skills.git".to_string(),
            branch: None,
            remote_revision: revision.to_string(),
        }
    }

    /// Drive the gate itself — no network, no clone. Testing the gate rather
    /// than the token function is deliberate: the bugs this guards against
    /// have all been in the wiring, and a test that only calls the hasher twice
    /// passes either way.
    fn run_gate(
        repo: &TestRepo,
        snapshot: &SkillRecord,
        fresh: &Path,
        source: &RepointSource,
        approved: Option<&str>,
    ) -> Result<RepointOutcome, AppError> {
        let new_hash = crate::core::content_hash::hash_directory(fresh).unwrap();
        commit_repoint_locked(
            &repo.store,
            snapshot,
            fresh,
            &new_hash,
            None,
            source,
            false,
            approved,
            &RepointProgress::default(),
        )
    }

    /// The five things a held re-point has to report back.
    #[allow(clippy::type_complexity)]
    fn held(
        outcome: RepointOutcome,
    ) -> (
        Vec<PendingRemoval>,
        String,
        Vec<SkillSourceDiffEntryDto>,
        bool,
        Option<String>,
    ) {
        match outcome {
            RepointOutcome::Held {
                pending,
                approval,
                diff_entries,
                central_copy_exists,
                duplicate_skill_name,
                ..
            } => (
                pending,
                approval,
                diff_entries,
                central_copy_exists,
                duplicate_skill_name,
            ),
            RepointOutcome::Applied { .. } => {
                panic!("the re-point was applied when it should have waited for approval")
            }
        }
    }

    fn applied(outcome: RepointOutcome) -> bool {
        match outcome {
            RepointOutcome::Applied { .. } => true,
            RepointOutcome::Held { .. } => false,
        }
    }

    /// Staging directories left in the library. A stray one is picked up by the
    /// metadata rebuild scan as a skill of its own, so every path that stops
    /// early has to leave none.
    fn leftover_staging_dirs() -> Vec<String> {
        central_repo::skills_dir()
            .read_dir()
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .filter(|name| name.contains(".staged-"))
            .collect()
    }

    /// The clone runs outside the lock, so the row can move on while it is in
    /// flight. A decision made against the snapshot must not be applied to
    /// whatever the row has become.
    #[test]
    fn a_repoint_refuses_a_row_that_moved_on_since_the_snapshot() {
        let repo = test_repo();
        let (skill, central) = lost_local_skill(&repo, "s1", "pdf", "old body");

        // Something else got here first — the user relinked, or another update
        // landed. The snapshot the caller holds no longer describes the row.
        let mut moved = skill.clone();
        moved.source_ref = Some("D:/somewhere/else".to_string());
        repo.store.upsert_skill(&moved).unwrap();

        let fresh = checkout(&repo, &[("SKILL.md", "new body")]);
        let err = run_gate(&repo, &skill, &fresh, &git_source("rev1"), None).unwrap_err();

        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert!(err.message.contains("changed while fetching"));
        assert_eq!(
            fs::read_to_string(central.join("SKILL.md")).unwrap(),
            "old body",
            "a refused re-point must not have written anything"
        );
        assert!(leftover_staging_dirs().is_empty());
    }

    /// The shape decision, which is the only branch in the network phase that
    /// is new: a candidate is either a locator or a URL, and half a locator
    /// must never be stored as one.
    #[test]
    fn a_recovered_source_is_either_a_locator_or_the_url_they_gave_and_nothing_between() {
        let plain = RepointRequest {
            clone_url: "https://github.com/acme/skills.git",
            subpath: None,
            branch: None,
            locator_source: None,
            locator_skill_id: None,
        };
        let resolve = |request: &RepointRequest<'_>| {
            repoint_source_for(
                request,
                "https://github.com/acme/skills",
                "https://github.com/acme/skills.git",
                None,
                "rev1",
            )
        };

        let url = resolve(&plain);
        assert_eq!(url.source_type, "git");
        assert_eq!(url.source_ref, "https://github.com/acme/skills");

        let located = resolve(&RepointRequest {
            locator_source: Some("acme/skills"),
            locator_skill_id: Some("pdf"),
            ..plain
        });
        assert_eq!(located.source_type, "skillssh");
        assert_eq!(
            located.source_ref, "acme/skills/pdf",
            "the ref is what skill_ssh_id cuts the locator out of"
        );

        for half in [
            RepointRequest {
                locator_source: Some("acme/skills"),
                ..plain
            },
            RepointRequest {
                locator_skill_id: Some("pdf"),
                ..plain
            },
        ] {
            assert_eq!(
                resolve(&half).source_type,
                "git",
                "one half of a locator names no skill, so it must not be stored as one"
            );
        }
    }

    /// The badge a failed re-point leaves behind.
///
/// The `source_missing` case is the one that matters: that status is the *only*
/// condition under which the relink / find-source / detach actions render, so
/// an error written over it takes away the row's last exit.
#[test]
fn a_failed_repoint_never_hides_the_recovery_actions() {
    // Committed: the row points at the new source now, so the old badge is
    // describing something it no longer tracks.
    assert_eq!(failure_status_for(true, "source_missing", "error"), "error");
    // Not committed and still missing: "error" must not win, even though the
    // caller asked for it.
    assert_eq!(
        failure_status_for(false, "source_missing", "error"),
        "source_missing",
        "the CLI asks for 'error'; overwriting this would strand the row"
    );
    assert_eq!(failure_status_for(false, "up_to_date", "error"), "error");
    assert_eq!(
        failure_status_for(false, "up_to_date", "source_missing"),
        "source_missing",
        "recovery passes its own snapshot status for an ordinary row"
    );
}

/// A recorded hash can still match while the files it described are gone. That
/// has to read as a difference, or the re-point reports success having written
/// nothing — and the skill stays listed but unreadable.
#[test]
fn a_gone_library_copy_is_reinstalled_even_when_the_hash_agrees() {
    let repo = test_repo();
    let central = write_skill_dir("vanished");
    let fresh = checkout(&repo, &[("SKILL.md", "---\nname: vanished\n---\n")]);

    let mut skill = sample_skill("s1", "vanished", &central);
    // The hash is a lie: it describes content that is no longer on disk.
    skill.content_hash = Some(crate::core::content_hash::hash_directory(&fresh).unwrap());
    skill.update_status = "source_missing".to_string();
    repo.store.insert_skill(&skill).unwrap();
    fs::remove_dir_all(&central).unwrap();

    let (pending, approval, _, exists, _) =
        held(run_gate(&repo, &skill, &fresh, &git_source("rev1"), None).unwrap());
    assert!(!exists);
    assert!(
        pending.is_empty(),
        "nothing is being taken away — the files are already gone"
    );
    // The token matching is what lets this through, and only because the gate
    // still counts the missing copy as a difference worth asking about.
    assert!(
        applied(run_gate(&repo, &skill, &fresh, &git_source("rev1"), Some(&approval)).unwrap()),
        "a matching hash is not a reason to write nothing"
    );
    assert_eq!(
        fs::read_to_string(central.join("SKILL.md")).unwrap(),
        "---\nname: vanished\n---\n"
    );
    assert_eq!(
        repo.store.get_skill_by_id("s1").unwrap().unwrap().update_status,
        "up_to_date"
    );
}

/// A differing tree is itself something to confirm, even with nothing to
    /// delete: two skills can share a name and a file list while sharing
    /// nothing else, and at this point the library copy may be the only one
    /// left in existence.
    #[test]
    fn recovery_holds_when_the_remote_differs_even_with_nothing_to_delete() {
        let repo = test_repo();
        let (skill, central) = lost_local_skill(&repo, "s1", "pdf", "old body");
        let fresh = checkout(&repo, &[("SKILL.md", "new body")]);

        let (pending, approval, diff, exists, duplicate) =
            held(run_gate(&repo, &skill, &fresh, &git_source("rev1"), None).unwrap());

        assert!(
            pending.is_empty(),
            "no path is being taken away — that is the whole point of this case"
        );
        assert!(exists, "the library copy is still there, and still at risk");
        assert_eq!(duplicate, None);
        assert!(
            !diff.is_empty(),
            "the user has to be shown what would change before it does"
        );
        assert_eq!(fs::read_to_string(central.join("SKILL.md")).unwrap(), "old body");
        assert_eq!(
            fs::read_to_string(&fresh.join("SKILL.md")).unwrap(),
            "new body"
        );
        assert!(!approval.is_empty());
        assert!(
            leftover_staging_dirs().is_empty(),
            "a held re-point staged a copy and must have cleaned it up"
        );
    }

    /// The half `update_git_skill_internal` deliberately kept: even with the
    /// library copy unchanged, every copy-mode deployment is torn down and
    /// rebuilt, which takes a private file with it.
    #[test]
    fn recovery_holds_when_only_a_deployed_copy_would_lose_a_file() {
        let repo = test_repo();
        let (skill, _central) = lost_local_skill(&repo, "s1", "pdf", "same body");
        let fresh = checkout(&repo, &[("SKILL.md", "same body")]);

        let deployed = repo._tmp.path().join("agent/pdf");
        fs::create_dir_all(&deployed).unwrap();
        fs::write(deployed.join("SKILL.md"), "same body").unwrap();
        fs::write(deployed.join("mine.txt"), "private").unwrap();
        repo.store
            .insert_target(&SkillTargetRecord {
                id: "t1".to_string(),
                skill_id: "s1".to_string(),
                tool: "cursor".to_string(),
                target_path: deployed.to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: None,
            })
            .unwrap();

        let (pending, _, _, _, _) =
            held(run_gate(&repo, &skill, &fresh, &git_source("rev1"), None).unwrap());

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].location, "cursor");
        assert_eq!(pending[0].path, "mine.txt");
        assert!(
            deployed.join("mine.txt").exists(),
            "a held re-point must not have taken it"
        );
    }

    /// The whole two-leg exchange, and what a committed re-point has to
    /// preserve: the id, and everything keyed to it.
    #[test]
    fn an_approved_recovery_repoints_in_place_and_keeps_what_is_keyed_to_the_id() {
        let repo = test_repo();
        let (skill, central) = lost_local_skill(&repo, "s1", "pdf", "old body");
        let deployed = repo._tmp.path().join("agent/pdf");
        fs::create_dir_all(&deployed).unwrap();
        fs::write(deployed.join("SKILL.md"), "old body").unwrap();
        repo.store
            .insert_target(&SkillTargetRecord {
                id: "t1".to_string(),
                skill_id: "s1".to_string(),
                tool: "cursor".to_string(),
                target_path: deployed.to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: None,
            })
            .unwrap();
        repo.store
            .set_tags_for_skill("s1", &["pdf".to_string()])
            .unwrap();
        repo.store
            .insert_scenario(&ScenarioRecord {
                id: "sc1".to_string(),
                name: "work".to_string(),
                description: None,
                icon: None,
                sort_order: 0,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
        repo.store.add_skill_to_scenario("sc1", "s1").unwrap();

        let fresh = checkout(&repo, &[("SKILL.md", "new body")]);
        let (_, approval, _, _, _) =
            held(run_gate(&repo, &skill, &fresh, &git_source("rev1"), None).unwrap());

        assert!(applied(
            run_gate(&repo, &skill, &fresh, &git_source("rev1"), Some(&approval)).unwrap()
        ));

        let row = repo.store.get_skill_by_id("s1").unwrap().unwrap();
        assert_eq!(row.id, "s1", "the id is the whole point of a re-point");
        assert_eq!(row.central_path, central.to_string_lossy());
        assert_eq!(fs::read_to_string(central.join("SKILL.md")).unwrap(), "new body");
        assert_eq!(row.source_type, "git");
        assert_eq!(row.source_ref.as_deref(), Some("https://github.com/acme/skills"));
        assert_eq!(row.remote_revision.as_deref(), Some("rev1"));
        assert_eq!(row.update_status, "up_to_date");
        assert_eq!(row.last_check_error, None);
        assert_eq!(
            repo.store.get_tags_map().unwrap().get("s1"),
            Some(&vec!["pdf".to_string()])
        );
        assert_eq!(repo.store.get_scenarios_for_skill("s1").unwrap(), vec!["sc1"]);

        let target = repo.store.get_targets_for_skill("s1").unwrap();
        assert_eq!(target.len(), 1, "the deployment target survives");
        assert_eq!(
            target[0].source_hash, row.content_hash,
            "the deployment was resynced to the new content"
        );
    }

    /// The window the approval exists to close. Approving a list is not
    /// approving whatever the list has since become.
    #[test]
    fn a_stale_approval_does_not_authorize_a_grown_list() {
        let repo = test_repo();
        let (skill, central) = lost_local_skill(&repo, "s1", "pdf", "old body");
        let fresh = checkout(&repo, &[("SKILL.md", "new body")]);

        let (_, approval, _, _, _) =
            held(run_gate(&repo, &skill, &fresh, &git_source("rev1"), None).unwrap());

        // The user took the dialog to go and made something, and so does
        // something else that only the second call can see.
        fs::write(central.join("appeared-later.md"), "mine").unwrap();

        let (pending, _, _, _, _) = held(
            run_gate(&repo, &skill, &fresh, &git_source("rev1"), Some(&approval)).unwrap(),
        );
        assert_eq!(pending.len(), 1, "the new file has to be asked about");
        assert_eq!(pending[0].path, "appeared-later.md");
        assert!(central.join("appeared-later.md").exists());
        assert_eq!(fs::read_to_string(central.join("SKILL.md")).unwrap(), "old body");
    }

    /// The `skillssh` locator is what keeps an upstream that moves the skill
    /// locatable, and it only survives if `source_ref` is the
    /// `{owner}/{repo}/{skill_id}` shape rather than a bare clone URL — a URL
    /// cut on `/` yields a locator that names nothing.
    #[test]
    fn a_skillssh_recovery_writes_a_locator_source_ref() {
        let repo = test_repo();
        let (skill, _central) = lost_local_skill(&repo, "s1", "pdf", "old body");
        let fresh = checkout(&repo, &[("SKILL.md", "new body")]);

        let source = RepointSource {
            source_type: "skillssh".to_string(),
            source_ref: "acme/skills/pdf".to_string(),
            source_ref_resolved: "https://github.com/acme/skills.git".to_string(),
            branch: None,
            remote_revision: "rev1".to_string(),
        };

        let (_, approval, _, _, _) =
            held(run_gate(&repo, &skill, &fresh, &source, None).unwrap());
        assert!(applied(
            run_gate(&repo, &skill, &fresh, &source, Some(&approval)).unwrap()
        ));

        let row = repo.store.get_skill_by_id("s1").unwrap().unwrap();
        assert_eq!(row.source_type, "skillssh");
        assert_eq!(row.source_ref.as_deref(), Some("acme/skills/pdf"));

        let resolved = git_source_from_skill(&row).unwrap();
        assert_eq!(resolved.locator_skill_id.as_deref(), Some("pdf"));
        assert_eq!(resolved.clone_url, "https://github.com/acme/skills.git");
    }

    /// Recovery exists for rows that lost their path. Without this guard the
    /// command is a general "re-point anything at anything" entry point.
    #[test]
    fn recovery_refuses_a_row_that_is_not_local_or_import() {
        let repo = test_repo();
        let central = write_skill_dir("tracked");
        let mut skill = sample_skill("g1", "tracked", &central);
        skill.source_type = "git".to_string();
        skill.source_ref = Some("https://github.com/acme/skills.git".to_string());
        repo.store.insert_skill(&skill).unwrap();

        let err = recover_skill_source_internal(
            &repo.store,
            "g1",
            "https://github.com/acme/skills.git",
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert!(err.message.contains("local"));
        assert_eq!(
            repo.store.get_skill_by_id("g1").unwrap().unwrap().source_type,
            "git",
            "a refused recovery must not have touched the row"
        );
    }

    // ── Batch recovery ──

    /// A minimal batch entry: every field past the id and the URL has a
    /// default in the real flow, and these cases turn on neither.
    fn request_for(skill_id: &str, repo_url: &str) -> BatchRecoverRequest {
        BatchRecoverRequest {
            skill_id: skill_id.to_string(),
            repo_url: repo_url.to_string(),
            locator_source: None,
            locator_skill_id: None,
            subpath: None,
            branch: None,
            approved_removals: None,
        }
    }

    /// Reported success for a batch that did nothing is the one answer the UI
    /// cannot recover from, so an empty batch is refused.
    #[test]
    fn an_empty_batch_recovers_nothing_rather_than_reporting_success() {
        let repo = test_repo();
        let err = batch_recover_skill_sources_internal(&repo.store, &[], None, None).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert!(err.message.contains("No skills"));
    }

    /// Every entry is a clone, so an unbounded list is an unbounded wait.
    #[test]
    fn a_batch_is_capped_so_a_runaway_list_cannot_queue_unbounded_clones() {
        let repo = test_repo();
        let requests: Vec<BatchRecoverRequest> = (0..=MAX_BATCH_RECOVER)
            .map(|i| request_for(&format!("s{i}"), "https://github.com/acme/skills.git"))
            .collect();
        let err =
            batch_recover_skill_sources_internal(&repo.store, &requests, None, None).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert!(
            err.message.contains(&MAX_BATCH_RECOVER.to_string()),
            "the message has to say where the limit is: {}",
            err.message
        );
    }

    /// The same row twice would recover once and then be refused by the
    /// local/import guard, which reads as a batch bug rather than a caller
    /// mistake. Say so before anything runs.
    #[test]
    fn a_repeated_skill_is_refused_before_anything_runs() {
        let repo = test_repo();
        let entry = request_for("s1", "https://github.com/acme/skills.git");
        let err = batch_recover_skill_sources_internal(&repo.store, &[entry.clone(), entry], None, None)
            .unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert!(err.message.contains("more than once"));
    }

    /// The point of a batch is that one bad skill does not cost the others
    /// their turn. Each refusal here lands before the network, so this needs no
    /// remote — and it asserts the rows are untouched, because a refusal that
    /// left `source_missing` behind as `error` would hide the rescue buttons.
    #[test]
    fn one_skill_failing_does_not_cost_the_batch_its_place() {
        let repo = test_repo();

        // Never lost its source, so the guard refuses it.
        let central = write_skill_dir("tracked");
        let mut tracked = sample_skill("g1", "tracked", &central);
        tracked.source_type = "git".to_string();
        tracked.source_ref = Some("https://github.com/acme/skills.git".to_string());
        repo.store.insert_skill(&tracked).unwrap();

        // The only kind that can be recovered at all.
        let (lost, _) = lost_local_skill(&repo, "s1", "pdf", "old body");

        let result = batch_recover_skill_sources_internal(
            &repo.store,
            &[
                request_for("does-not-exist", "https://github.com/acme/skills.git"),
                request_for("g1", "https://github.com/acme/skills.git"),
                request_for(&lost.id, "file:///etc/passwd"),
            ],
            None,
            None,
        )
        .unwrap();

        assert_eq!(result.requested, 3);
        assert_eq!(result.failed, 3);
        assert_eq!(result.applied, 0);
        assert_eq!(
            result.held, 0,
            "a refusal is a failure, not something awaiting approval"
        );
        assert_eq!(result.items.len(), 3, "the loop ran to the end of the list");
        assert!(
            result.items.iter().all(|item| item.error.is_some()),
            "every entry carries its own reason: {:?}",
            result
                .items
                .iter()
                .map(|item| item.error.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(result.items[0].skill_id, "does-not-exist");
        assert_eq!(result.items[1].name, "tracked");
        assert_eq!(
            repo.store
                .get_skill_by_id(&lost.id)
                .unwrap()
                .unwrap()
                .update_status,
            "source_missing",
            "a refused batch entry must leave the row recoverable"
        );
    }

    /// The whole diff, before any write — the one command that could produce
    /// it (`get_skill_source_diff`) reads the path that no longer exists, so
    /// this is the only place the user can see it.
    #[test]
    fn recovery_reports_the_full_diff_before_any_write() {
        let repo = test_repo();
        let (skill, central) = lost_local_skill(&repo, "s1", "pdf", "old body");
        fs::write(central.join("gone-upstream.md"), "old").unwrap();
        let fresh = checkout(&repo, &[("SKILL.md", "new body"), ("new.md", "new")]);

        let (_, _, diff, _, _) =
            held(run_gate(&repo, &skill, &fresh, &git_source("rev1"), None).unwrap());

        let status = |name: &str| {
            diff.iter()
                .find(|entry| entry.relative_path == name)
                .map(|entry| entry.status.clone())
        };
        assert_eq!(status("new.md").as_deref(), Some("added"));
        assert_eq!(status("gone-upstream.md").as_deref(), Some("removed"));
        assert_eq!(status("SKILL.md").as_deref(), Some("modified"));
        assert!(
            central.join("gone-upstream.md").exists(),
            "the library copy must be exactly as it was"
        );
    }

    /// The most dangerous cell in the grid: nothing left to diff against and
    /// nothing to roll back to. The list is empty only because there is nothing
    /// left to remove, which is not the same as there being nothing to lose.
    #[test]
    fn a_gone_library_copy_is_reported_rather_than_looking_safe() {
        let repo = test_repo();
        let absent = central_repo::skills_dir().join("never-existed");
        let mut skill = sample_skill("s1", "pdf", &absent);
        skill.content_hash = Some("stale".to_string());
        skill.update_status = "source_missing".to_string();
        repo.store.insert_skill(&skill).unwrap();

        let fresh = checkout(&repo, &[("SKILL.md", "new body")]);
        let (pending, _, diff, exists, _) =
            held(run_gate(&repo, &skill, &fresh, &git_source("rev1"), None).unwrap());

        assert!(
            !exists,
            "the frontend keys its loudest warning on this"
        );
        assert!(pending.is_empty());
        assert!(
            diff.iter().all(|entry| entry.status == "added"),
            "with no baseline every file is new, which is not the same as 'no change'"
        );
    }

    /// Declining is not a failure. Restoring the status is what keeps the
    /// relink / find-source / detach buttons reachable — they are only shown
    /// for `source_missing` — and leaving the recorded check alone is what
    /// keeps the explanation of why the path went missing.
    #[test]
    fn a_held_recovery_restores_the_status_it_started_from() {
        let repo = test_repo();
        let (mut skill, _central) = lost_local_skill(&repo, "s1", "pdf", "old body");
        skill.remote_revision = Some("the old revision".to_string());
        repo.store.upsert_skill(&skill).unwrap();
        let snapshot = repo.store.get_skill_by_id("s1").unwrap().unwrap();

        let fresh = checkout(&repo, &[("SKILL.md", "new body")]);
        held(run_gate(&repo, &snapshot, &fresh, &git_source("rev1"), None).unwrap());

        let row = repo.store.get_skill_by_id("s1").unwrap().unwrap();
        assert_eq!(row.update_status, "source_missing");
        assert_ne!(row.update_status, "update_available");
        assert_eq!(row.remote_revision.as_deref(), Some("the old revision"));
        assert_eq!(
            row.last_check_error.as_deref(),
            Some("Original source path no longer exists"),
            "a decline is not a failed check, and must not read as one"
        );
    }

    /// Nothing to replace, nothing to remove, nothing to ask: a source that
    /// already holds the exact same bytes is a metadata change and should not
    /// cost a dialog or a rewrite of the library copy.
    #[test]
    fn an_identical_source_repoints_without_a_dialog() {
        let repo = test_repo();
        let (skill, central) = lost_local_skill(&repo, "s1", "pdf", "same body");
        let fresh = checkout(&repo, &[("SKILL.md", "same body")]);

        assert!(applied(
            run_gate(&repo, &skill, &fresh, &git_source("rev1"), None).unwrap()
        ));

        let row = repo.store.get_skill_by_id("s1").unwrap().unwrap();
        assert_eq!(row.source_type, "git");
        assert_eq!(row.update_status, "up_to_date");
        assert_eq!(fs::read_to_string(central.join("SKILL.md")).unwrap(), "same body");
        assert!(
            leftover_staging_dirs().is_empty(),
            "a no-op re-point must leave no staging directory behind"
        );
    }

    /// Recovery onto a source that is already in the library gives two rows the
    /// same upstream; both would then update, deploy and back up on their own.
    #[test]
    fn a_recovery_onto_an_already_tracked_source_says_so() {
        let repo = test_repo();
        let other = write_skill_dir("pdf-2");
        let mut tracked = sample_skill("g1", "pdf (already installed)", &other);
        tracked.source_type = "git".to_string();
        tracked.source_ref = Some("https://github.com/acme/skills".to_string());
        repo.store.insert_skill(&tracked).unwrap();

        let (skill, _central) = lost_local_skill(&repo, "s1", "pdf", "old body");
        let fresh = checkout(&repo, &[("SKILL.md", "new body")]);

        let (_, _, _, _, duplicate) =
            held(run_gate(&repo, &skill, &fresh, &git_source("rev1"), None).unwrap());
        assert_eq!(duplicate.as_deref(), Some("pdf (already installed)"));
    }
}
