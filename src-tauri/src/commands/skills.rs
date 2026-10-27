use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;
use tauri::{Emitter, State};
use walkdir::WalkDir;

use crate::core::{
    audit_log::AuditDraft,
    central_repo,
    error::AppError,
    git_fetcher,
    install_cancel::InstallCancelRegistry,
    installer, path_guard,
    remote_batch,
    remote_ref::{is_git_channel, RemoteIdentity, RemoteRef},
    repo_lock::RepoLock,
    scanner, scenario_service,
    skill_metadata::{self, is_valid_skill_dir},
    skill_store::{PendingRemovalRecord, SkillRecord, SkillStore, SkillTargetRecord},
    source_membership::{self, Confidence, MoveCandidate, Placement},
    sync_engine, sync_metadata,
    timing::should_log_first_or_slow,
    well_known,
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
    /// Where the Source says the skill went, strongest candidate first. Empty
    /// unless the recorded path no longer holds it: the batch's `moved` bucket,
    /// for one skill. Nothing was written — re-pointing is the user's call.
    pub moved: Vec<MoveCandidate>,
    /// True when the Source no longer holds this skill anywhere. Not a failure:
    /// retrying cannot help, so the row offers detach or delete instead.
    pub gone: bool,
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

/// One skill's removals in one place: the library, or a single agent copy.
///
/// A skill can appear in more than one group, because approving is per skill
/// while the paths it would delete may live in several directories. The panel
/// shows that split; the token still covers the whole skill.
#[derive(Debug, Clone, Serialize)]
pub struct PendingRemovalGroup {
    pub skill_id: String,
    pub skill_name: String,
    /// The Source the skill came from, in the form `source_ref_resolved` stores.
    pub source: String,
    /// The revision the stored list describes, and the first half of its token.
    pub revision: String,
    /// [`LIBRARY_LOCATION`], or the key of the agent whose copy holds it.
    pub location: String,
    pub paths: Vec<String>,
}

/// One skill whose approval did not go through. Other skills in the same batch
/// are unaffected: removals are per-skill, not a transaction across skills.
#[derive(Debug, Serialize)]
pub struct FailedRemoval {
    pub name: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ApproveRemovalsResult {
    pub approved: usize,
    pub failed: Vec<FailedRemoval>,
}

/// What one skill's approval attempt means for the batch.
enum ApprovalOutcome {
    Applied,
    /// The list or revision differs from the stored question. The update has
    /// written the new list, so the panel must show it before another approval.
    ListChanged,
    /// A move or deletion upstream: approving removals is not the right action.
    CannotApply(String),
}

/// Stored rather than translated: the panel refreshes itself after this, and the
/// message is only a last-resort description for logs and API callers.
const PENDING_REMOVALS_CHANGED_MESSAGE: &str =
    "The removals changed since they were listed; review the updated list before approving";

#[derive(Debug)]
enum UpdateOutcome {
    Applied {
        content_changed: bool,
    },
    /// Declined, having changed nothing.
    Held {
        pending: Vec<PendingRemoval>,
        approval: String,
    },
    /// The recorded path no longer holds this skill, but the Source still does:
    /// upstream renamed or moved it. Nothing was written — re-pointing the
    /// source is the user's decision, so this carries the evidence it needs.
    Moved { candidates: Vec<MoveCandidate> },
    /// The Source no longer holds this skill anywhere. Not a failure: there is
    /// nothing to retry, and the user is offered detach/delete instead.
    Gone,
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
    /// True when the user stopped the round: the counters describe what finished.
    pub cancelled: bool,
    /// How many paths an update would have removed across every held-back
    /// skill. The names alone do not tell the user whether it is one file or a
    /// hundred.
    pub held_back_paths: usize,
    /// Skills whose recorded path no longer holds them, but which their Source
    /// still contains somewhere else — a rename or a directory move. Carried
    /// with the candidate paths and the confidence in each, because the fix is
    /// a re-point the user has to confirm.
    pub moved_sources: Vec<MovedSource>,
    /// Skills their Source no longer contains anywhere. Kept apart from `failed`
    /// because retrying cannot help: the advice is to detach the source or
    /// delete the skill.
    pub gone_sources: Vec<String>,
    /// Skills the Sources gained that the library does not hold. Offered by the
    /// UI; never installed by the round.
    pub new_skills: Vec<NewSkill>,
}

/// One skill that moved upstream, with every place the checkout proposes for it.
#[derive(Debug, Serialize)]
pub struct MovedSource {
    pub name: String,
    /// Strongest first (see [`source_membership::locate`]). More than one when
    /// only the name matched, since then the module refuses to pick for the
    /// user.
    pub candidates: Vec<MoveCandidate>,
}

#[derive(Debug, Serialize)]
pub struct BatchDeleteSkillsResult {
    pub deleted: usize,
    pub failed: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct InstallLocalResult {
    pub skill_id: String,
    pub name: String,
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
    /// Where the last `moved` check found this skill in its Source. Empty for
    /// every other status. Persisted so the row can still offer the one-click
    /// re-point after a restart, when the checkout is long gone.
    pub move_candidates: Vec<MoveCandidate>,
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

#[derive(Debug, Serialize)]
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

        if !is_git_channel(&skill.source_type) {
            return Err(AppError::invalid_input(
                "Skill does not support source diff preview",
            ));
        }

        let git_source = RemoteRef::from_skill(&skill)?;
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
                git_source.locator.as_deref(),
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

        if !is_git_channel(&skill.source_type) {
            return Err(AppError::invalid_input(
                "Skill does not support source diff preview",
            ));
        }

        let git_source = RemoteRef::from_skill(&skill)?;
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
                git_source.locator.as_deref(),
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

/// One place that records "this update did not finish", so a row cannot be left
/// showing `updating` forever by a path that forgot to write the error.
fn record_update_failure(
    store: &SkillStore,
    skill_id: &str,
    revision: Option<&str>,
    message: &str,
) {
    let _ = store.update_skill_check_state(skill_id, revision, "error", Some(message));
}

/// Drop a held removal list for a skill that just ended anywhere else.
///
/// A list is a description of one held state; once the skill is applied, moved,
/// gone or failed, that description is stale and must not outlive it. The
/// round and the single-skill error path both call this so the rule has one
/// definition. A failed cleanup is logged, not promoted to a skill failure:
/// the update itself already has an answer.
fn clear_stale_pending_removals(store: &SkillStore, skill_id: &str) {
    if let Err(err) = store.clear_skill_pending_removals(skill_id) {
        log::warn!(
            "could not clear pending removals for {skill_id}: {}",
            err
        );
    }
}

/// What an update did, as the audit trail records it. Both the single-skill and
/// the batch path end here, so the two cannot describe the same outcome
/// differently.
impl<'a> From<&'a UpdateOutcome> for UpdateAudit<'a> {
    fn from(outcome: &'a UpdateOutcome) -> Self {
        match outcome {
            UpdateOutcome::Held { pending, .. } => UpdateAudit::Held {
                removed: pending.len(),
            },
            UpdateOutcome::Applied { content_changed } => UpdateAudit::Applied {
                content_changed: *content_changed,
            },
            UpdateOutcome::Moved { candidates } => UpdateAudit::Moved { candidates },
            UpdateOutcome::Gone => UpdateAudit::Gone,
        }
    }
}

enum UpdateAudit<'a> {
    Applied { content_changed: bool },
    Held { removed: usize },
    /// Moved upstream; nothing was written, and the first candidate is where
    /// the Source says it went.
    Moved { candidates: &'a [MoveCandidate] },
    Gone,
    Failed(&'a str),
}

/// `Confidence` in the lowercase form the audit detail uses; serde spells it the
/// same way on the wire, so the two cannot drift.
fn confidence_label(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::High => "high",
        Confidence::Low => "low",
    }
}

fn log_update_entry(
    store: &SkillStore,
    skill_id: &str,
    name: &str,
    source_label: &str,
    entry: UpdateAudit<'_>,
) {
    let mut draft = AuditDraft::new("update").detail(source_label);
    match entry {
        UpdateAudit::Held { removed } => {
            draft = draft
                .skill(skill_id.to_string(), name.to_string())
                .detail(format!("{source_label}; held back — would remove {removed} path(s)"))
                .ok();
        }
        UpdateAudit::Applied { content_changed } => {
            draft = draft
                .skill(skill_id.to_string(), name.to_string())
                .detail(if content_changed {
                    format!("{source_label}; content changed")
                } else {
                    format!("{source_label}; unchanged")
                })
                .ok();
        }
        UpdateAudit::Failed(message) => {
            draft = draft
                .skill(skill_id.to_string(), name.to_string())
                .fail(message.to_string());
        }
        UpdateAudit::Moved { candidates } => {
            // Naming the first candidate is the whole point of the entry: a
            // reader can go and look at that path instead of re-deriving it.
            let where_to = match candidates.first() {
                Some(first) => format!(
                    " — first candidate '{}' ({} confidence)",
                    first.subpath,
                    confidence_label(first.confidence)
                ),
                None => String::new(),
            };
            draft = draft
                .skill(skill_id.to_string(), name.to_string())
                .detail(format!("{source_label}; moved upstream{where_to}"))
                .ok();
        }
        UpdateAudit::Gone => {
            draft = draft
                .skill(skill_id.to_string(), name.to_string())
                .detail(format!("{source_label}; gone upstream — no candidate in the checkout"))
                .ok();
        }
    }
    store.log_audit(draft);
}

fn log_update_outcome(
    store: &SkillStore,
    skill_id: &str,
    source_label: &str,
    outcome: Result<&UpdateSkillResult, &AppError>,
) {
    let (name, entry) = match outcome {
        Ok(result) if result.gone => (result.skill.name.clone(), UpdateAudit::Gone),
        Ok(result) if !result.moved.is_empty() => (
            result.skill.name.clone(),
            UpdateAudit::Moved {
                candidates: &result.moved,
            },
        ),
        Ok(result) if !result.pending_removals.is_empty() => (
            result.skill.name.clone(),
            UpdateAudit::Held {
                removed: result.pending_removals.len(),
            },
        ),
        Ok(result) => (
            result.skill.name.clone(),
            UpdateAudit::Applied {
                content_changed: result.content_changed,
            },
        ),
        Err(e) => (
            store
                .get_skill_by_id(skill_id)
                .ok()
                .flatten()
                .map(|s| s.name)
                .unwrap_or_default(),
            UpdateAudit::Failed(&e.message),
        ),
    };
    log_update_entry(store, skill_id, &name, source_label, entry);
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
) -> Result<InstallLocalResult, AppError> {
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
        outcome.map(|(skill_id, name)| InstallLocalResult { skill_id, name })
    })
    .await?
}

/// Install a skill a website publishes itself (skills.sh `/site/<domain>/<skill>`).
/// It is a snapshot: there is no repository to check for updates against.
fn install_site_skill(
    store: &SkillStore,
    site_url: &str,
    name: Option<&str>,
    proxy_url: Option<&str>,
    emit_progress: &dyn Fn(&str),
    cancel: &AtomicBool,
) -> Result<(String, String), AppError> {
    emit_progress("cloning");
    let downloaded =
        well_known::download_site_skill(site_url, proxy_url).map_err(AppError::network)?;
    let result = (|| -> Result<(String, String), AppError> {
        emit_progress("installing");
        let _lock = RepoLock::acquire_foreground("install website skill").map_err(AppError::db)?;
        // Checked once the lock is held, right before anything is written: the
        // download is not interruptible and the lock can take a while, and a
        // cancel during either must still leave the library untouched.
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(AppError::cancelled("Installation cancelled by user"));
        }
        let (skill_id, result) = install_site_skill_dir(
            store,
            site_url,
            &downloaded.skill_dir,
            name,
            &downloaded.resolved_url,
            downloaded.revision.clone(),
            None,
        )?;
        Ok((skill_id, result.name))
    })();
    well_known::cleanup_temp(&downloaded.temp_dir);
    result
}

/// Put a downloaded website skill into the library; the caller holds the repo
/// lock. Every entry point records the canonical skills.sh URL, so reinstalling
/// from the market, a pasted link or the CLI lands on the same record and
/// directory instead of adding a `-2` copy.
pub fn install_site_skill_dir(
    store: &SkillStore,
    site_url: &str,
    skill_dir: &Path,
    name: Option<&str>,
    resolved_url: &str,
    revision: Option<String>,
    active_scenario_id: Option<&str>,
) -> Result<(String, installer::InstallResult), AppError> {
    let site = well_known::parse_site_ref(site_url)
        .map_err(|error| AppError::invalid_input(error.to_string()))?;
    let source_ref = site.canonical_url();
    let base_name = match name.map(str::trim).filter(|name| !name.is_empty()) {
        Some(custom) => skill_metadata::sanitize_skill_name(custom)
            .ok_or_else(|| AppError::invalid_input(format!("Invalid skill name: '{custom}'")))?,
        None => site.skill_name,
    };
    let (install_name, destination) =
        resolve_install_target(store, "well-known", &source_ref, &base_name)?;
    let result = installer::install_skill_dir_to_destination(skill_dir, &install_name, &destination)
        .map_err(AppError::io)?;
    let metadata = InstallSourceMetadata {
        source_type: "well-known".to_string(),
        source_ref: Some(source_ref),
        source_ref_resolved: Some(resolved_url.to_string()),
        source_subpath: None,
        source_branch: None,
        source_revision: revision.clone(),
        remote_revision: revision,
        update_status: "local_only".to_string(),
    };
    let skill_id = store_installed_skill_unlocked(store, &result, &metadata, active_scenario_id)?;
    Ok((skill_id, result))
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

        if well_known::is_site_ref(&repo_url) {
            let outcome = install_site_skill(
                &store,
                &repo_url,
                name.as_deref(),
                proxy_url.as_deref(),
                &emit_progress,
                &cancel,
            );
            log_install_outcome(&store, "well-known", outcome.as_ref());
            outcome?;
            emit_progress("done");
            return Ok(());
        }

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

/// Build the reference for a skill the round discovered, without a tree URL.
///
/// A tree URL has to guess which segment is the branch and is only understood
/// for GitHub-shaped URLs. The round already knows the Source, the branch and
/// the subpath as separate values, so no parsing is needed — and a Source with
/// no branch stays `None` instead of being handed the first path segment as a
/// branch name.
fn new_skill_source(
    source_ref_resolved: &str,
    branch: Option<&str>,
    subpath: &str,
) -> Result<RemoteRef, AppError> {
    git_fetcher::validate_git_url(source_ref_resolved).map_err(AppError::git)?;
    Ok(RemoteRef {
        original_url: source_ref_resolved.to_string(),
        clone_url: source_ref_resolved.to_string(),
        branch: branch.filter(|branch| !branch.is_empty()).map(str::to_string),
        subpath: source_membership::normalize_subpath(Some(subpath))
            .filter(|subpath| !subpath.is_empty()),
        locator: None,
    })
}

/// Install one skill a round discovered, from the Source it came from.
///
/// Deliberately not routed through `install_git` with a tree URL: the round
/// already holds the three values separately, and a branch-less Source or a
/// GitLab/ssh URL does not survive that string form.
///
/// The round has just fetched this Source, so the cache is warm. `Reuse` takes
/// that copy and only clones when the cache has gone; the fallback is the
/// ordinary narrow clone.
#[tauri::command]
pub async fn install_new_skill(
    source_ref_resolved: String,
    branch: Option<String>,
    subpath: String,
    store: State<'_, Arc<SkillStore>>,
    cancel_registry: State<'_, Arc<InstallCancelRegistry>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    let registry = cancel_registry.inner().clone();
    let cancel_key = format!(
        "install-new:{source_ref_resolved}#{}#{subpath}",
        branch.as_deref().unwrap_or("")
    );
    let cancel = registry.register(&cancel_key);
    let _cancel_guard = CancelRegistrationGuard::new(registry, cancel_key);

    tauri::async_runtime::spawn_blocking(move || {
        let source = new_skill_source(&source_ref_resolved, branch.as_deref(), &subpath)?;
        let temp_dir = git_fetcher::clone_repo_ref_scoped_with(
            &source.clone_url,
            source.branch.as_deref(),
            source.subpath.as_deref(),
            Some(&cancel),
            proxy_url.as_deref(),
            None,
            git_fetcher::CacheRefresh::Reuse,
        )
        .map_err(AppError::classify_git_error)?;

        let install_result = (|| -> Result<(), AppError> {
            let _lock =
                RepoLock::acquire_foreground("install discovered skill").map_err(AppError::db)?;
            let skill_dir = resolve_skill_dir(&temp_dir, source.subpath.as_deref(), None)?;
            let revision = git_fetcher::get_head_revision(&temp_dir).map_err(AppError::git)?;
            let result = installer::install_from_git_dir(&skill_dir, None).map_err(AppError::io)?;
            let metadata = InstallSourceMetadata {
                source_type: "git".to_string(),
                source_ref: Some(source.original_url.clone()),
                source_ref_resolved: Some(source.clone_url.clone()),
                source_subpath: git_fetcher::relative_subpath(&temp_dir, &skill_dir),
                source_branch: source.branch.clone(),
                source_revision: Some(revision.clone()),
                remote_revision: Some(revision),
                update_status: "up_to_date".to_string(),
            };
            store_installed_skill_unlocked(&store, &result, &metadata, None)?;
            Ok(())
        })();

        git_fetcher::cleanup_temp(&temp_dir);
        install_result
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

        // skills.sh also lists skills that websites publish themselves; their
        // source is the site's domain, not a GitHub `owner/repo`.
        if !source.contains('/') {
            let site_url = format!("https://skills.sh/site/{source}/{skill_id}");
            let outcome = install_site_skill(
                &store,
                &site_url,
                None,
                proxy_url.as_deref(),
                &emit_progress,
                &cancel,
            );
            log_install_outcome(&store, "well-known", outcome.as_ref());
            outcome?;
            emit_progress("done");
            return Ok(());
        }

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
        if well_known::is_site_ref(&repo_url) {
            app_handle
                .emit(
                    "install-progress",
                    serde_json::json!({
                        "skill_id": repo_url,
                        "phase": "cloning",
                    }),
                )
                .ok();
            let downloaded = well_known::download_site_skill(&repo_url, proxy_url.as_deref())
                .map_err(AppError::network)?;
            let result = (|| -> Result<GitPreviewResult, AppError> {
                if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err(AppError::cancelled("Installation cancelled by user"));
                }
                let dirs = collect_git_skill_dirs(&downloaded.skill_dir);
                if dirs.is_empty() {
                    return Err(AppError::not_found("Downloaded source contains no skills"));
                }
                let skills = dirs
                    .iter()
                    .map(|dir| {
                        let meta = skill_metadata::parse_skill_md(dir);
                        let rel_path = skill_rel_key(&downloaded.skill_dir, dir);
                        let basename = dir
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| rel_path.clone());
                        let name = meta
                            .name
                            .filter(|s| !s.trim().is_empty())
                            .unwrap_or(basename);
                        GitSkillPreview {
                            rel_path,
                            name,
                            description: meta.description,
                        }
                    })
                    .collect();
                Ok(GitPreviewResult {
                    temp_dir: downloaded.temp_dir.to_string_lossy().to_string(),
                    skills,
                })
            })();
            if result.is_err() {
                well_known::cleanup_temp(&downloaded.temp_dir);
            }
            return result;
        }
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
        if well_known::is_site_ref(&repo_url) {
            let temp_path = validate_clone_temp_path(&temp_dir)?;
            let result: Result<(), AppError> = (|| {
                if items.is_empty() {
                    return Ok(());
                }
                let skill_dir = well_known::skill_dir_from_temp(&temp_path)
                    .map_err(|error| AppError::invalid_input(error.to_string()))?;
                let all_dirs = collect_git_skill_dirs(&skill_dir);
                let _lock = RepoLock::acquire_foreground("confirm website skill install")
                    .map_err(AppError::db)?;
                for dir in &all_dirs {
                    let rel_key = skill_rel_key(&skill_dir, dir);
                    let item = match items.iter().find(|item| item.rel_path == rel_key) {
                        Some(item) => item,
                        None => continue,
                    };
                    install_site_skill_dir(
                        &store,
                        &repo_url,
                        dir,
                        Some(item.name.as_str()),
                        &repo_url,
                        None,
                        None,
                    )?;
                }
                Ok(())
            })();
            well_known::cleanup_temp(&temp_path);
            return result;
        }
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
        // popped failures. Each remote also carries its subpath skills' installed
        // directories, so the monorepo siblings share one subtree fetch (#561).
        let mut remotes: HashMap<RemoteIdentity, HashSet<(String, String)>> = HashMap::new();
        for skill in &skills {
            if !is_git_channel(&skill.source_type) {
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
            if let Ok(source) = RemoteRef::from_skill(skill) {
                let installed = source.installed_subtree(skill);
                remotes
                    .entry(source.identity())
                    .or_default()
                    .extend(installed);
            }
        }
        let remote_revisions = if remotes.is_empty() {
            HashMap::new()
        } else {
            resolve_remotes_concurrent(remotes, proxy_url.clone())
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
            let prefetched = if is_git_channel(&skill.source_type) {
                RemoteRef::from_skill(skill)
                    .ok()
                    .and_then(|source| remote_revisions.get(&source.identity()).cloned())
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
/// A remote revision resolved off the central-repo lock, tagged with the remote
/// it was resolved for. The tag is what makes it safe to apply later: a
/// reinstall keeps a skill's row and repoints its source
/// (`update_skill_after_reinstall`), so the applying side re-derives the key
/// from the freshly read record and drops a prefetch that no longer matches.
#[derive(Clone)]
pub struct PrefetchedRemote {
    key: RemoteIdentity,
    result: Result<String, String>,
    /// The `(source_revision, subpath)` pairs whose directory is the same tree
    /// at the resolved revision — see [`git_fetcher::unchanged_subtrees`].
    unchanged: HashSet<(String, String)>,
}

/// Resolve `key`'s head, then which of `installed` it leaves unchanged.
fn resolve_remote(
    key: RemoteIdentity,
    installed: &HashSet<(String, String)>,
    proxy_url: Option<&str>,
) -> PrefetchedRemote {
    let result =
        git_fetcher::resolve_remote_revision(&key.clone_url, key.branch.as_deref(), proxy_url)
            .map_err(|err| err.to_string());
    let unchanged = match &result {
        Ok(head) => git_fetcher::unchanged_subtrees(&key.clone_url, head, installed, proxy_url),
        Err(_) => HashSet::new(),
    };
    PrefetchedRemote {
        key,
        result,
        unchanged,
    }
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
    if !is_git_channel(&skill.source_type) {
        return None;
    }
    if should_skip_update_check(store, &skill, force).unwrap_or(false) {
        return None;
    }
    let source = RemoteRef::from_skill(&skill).ok()?;
    let installed = source.installed_subtree(&skill).into_iter().collect();
    Some(resolve_remote(
        source.identity(),
        &installed,
        proxy_url,
    ))
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
    remotes: HashMap<RemoteIdentity, HashSet<(String, String)>>,
    proxy_url: Option<String>,
) -> HashMap<RemoteIdentity, PrefetchedRemote> {
    resolve_concurrent(remotes.keys().cloned().collect(), |key| {
        resolve_remote(key.clone(), &remotes[key], proxy_url.as_deref())
    })
}

/// Run `resolve` over every remote concurrently (bounded by
/// `MAX_CHECK_CONCURRENCY`) with work-stealing, and collect each result. Factored
/// out of [`resolve_remotes_concurrent`] so the concurrency contract is testable
/// with an injected resolver instead of live network: every remote is resolved
/// exactly once, a per-remote failure is stored as `Err` rather than aborting the
/// batch, and — since this function never touches `RepoLock` — resolution always
/// runs off the central-repo lock.
fn resolve_concurrent<T, F>(remotes: Vec<RemoteIdentity>, resolve: F) -> HashMap<RemoteIdentity, T>
where
    T: Send,
    F: Fn(&RemoteIdentity) -> T + Sync,
{
    // One runner for both phases; only the cap differs (a check reads a ref
    // advertisement, an update copies a tree).
    remote_batch::run_concurrent(remotes, MAX_CHECK_CONCURRENCY, resolve)
        .into_iter()
        .collect()
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

/// Settings key holding the new skills the user has said no to, oldest first.
/// A prompt that returns every round is a prompt the user learns to dismiss
/// without reading.
const DISMISSED_NEW_SKILLS: &str = "dismissed_new_skills";

/// Upper bound on the refusal list.
///
/// Pruning to "keys still in this round's trees" would forget every Source the
/// round did not visit, and a skill upstream deleted today can reappear. A
/// bounded oldest-first list keeps the record from growing forever while
/// forgetting only what the user has not been asked about in a long time.
const MAX_DISMISSED_NEW_SKILLS: usize = 500;

fn dismissed_new_skills(store: &SkillStore) -> Vec<String> {
    store
        .get_setting(DISMISSED_NEW_SKILLS)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .unwrap_or_default()
}

/// Remember the refusals so the next round does not ask again.
fn remember_dismissed_new_skills(
    store: &SkillStore,
    keys: Vec<String>,
) -> Result<(), AppError> {
    let mut current = dismissed_new_skills(store);
    let before = current.len();
    for key in keys {
        if !current.contains(&key) {
            current.push(key);
        }
    }
    if current.len() == before {
        return Ok(());
    }
    if current.len() > MAX_DISMISSED_NEW_SKILLS {
        let overflow = current.len() - MAX_DISMISSED_NEW_SKILLS;
        current.drain(..overflow);
    }
    store
        .set_setting(
            DISMISSED_NEW_SKILLS,
            &serde_json::to_string(&current).unwrap_or_default(),
        )
        .map_err(AppError::db)
}

/// Stable key for a discovered skill: the Source's identity (which folds `.git`
/// and a trailing slash, and carries the branch) plus the name in lower case.
/// Name rather than subpath, because one repository shipping the same skill in
/// two trees is one prompt; the frontend copies this key and never builds one.
fn new_skill_key(source: &RemoteIdentity, name: &str) -> String {
    format!("{}#{}", source.key(), name.trim().to_lowercase())
}

/// Every subpath the library already holds, per Source. Read once per round
/// rather than once per group: the scan needs the whole Source's inventory to
/// tell a new skill from an installed one.
///
/// Keyed by [`RemoteIdentity`], not by the URL string: `.git`, a trailing slash
/// and the branch are part of the question, and comparing URLs by spelling is
/// how an installed skill came back as new and overwrote itself on install.
fn installed_subpaths_by_source(
    store: &SkillStore,
) -> Result<HashMap<RemoteIdentity, Vec<String>>, AppError> {
    let mut out: HashMap<RemoteIdentity, Vec<String>> = HashMap::new();
    for skill in store.get_all_skills().map_err(AppError::db)? {
        let Some(subpath) = skill.source_subpath.as_deref() else {
            continue;
        };
        let Ok(source) = RemoteRef::from_skill(&skill) else {
            continue;
        };
        let Some(subpath) = source_membership::normalize_subpath(Some(subpath)) else {
            continue;
        };
        out.entry(source.identity()).or_default().push(subpath);
    }
    Ok(out)
}

/// The skills this checkout holds that the library does not, minus the ones
/// this round already answered a different way.
///
/// A member `locate` reports as `Moved` contributes its candidate subpaths to
/// the exclusion set. Without that, a rename arrives twice in one round — as a
/// move and as a new skill — and installing the second would rewrite the
/// existing record's `source_subpath`, which only the user may do.
///
/// `seen` deduplicates across groups: one name from one Source is one prompt,
/// even if two checkouts of it are scanned.
fn collect_new_skills(
    source: &RemoteRef,
    checkout: &Path,
    members: &[(String, SkillRecord)],
    known_subpaths: &[String],
    dismissed: &HashSet<String>,
    seen: &mut HashSet<(RemoteIdentity, String)>,
) -> Vec<NewSkill> {
    let mut moved_to: HashSet<String> = HashSet::new();
    for (_, skill) in members {
        if let Placement::Moved { candidates } = source_membership::locate(skill, checkout) {
            moved_to.extend(candidates.into_iter().map(|candidate| candidate.subpath));
        }
    }

    let identity = source.identity();
    let mut found = Vec::new();
    for skill in source_membership::discover(checkout, known_subpaths) {
        if moved_to.contains(&skill.subpath)
            || skill.also_at.iter().any(|path| moved_to.contains(path))
        {
            continue;
        }
        let key = new_skill_key(&identity, &skill.name);
        if dismissed.contains(&key) {
            continue;
        }
        if !seen.insert((identity.clone(), skill.name.to_lowercase())) {
            continue;
        }
        found.push(NewSkill {
            name: skill.name,
            subpath: skill.subpath,
            source_ref_resolved: source.clone_url.clone(),
            branch: source.branch.clone(),
            key,
        });
    }
    found
}

/// Stop offering these skills. The round filters them out from then on.
#[tauri::command]
pub fn dismiss_new_skills(
    keys: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    remember_dismissed_new_skills(store.inner(), keys)
}

/// Split a batch into one group per Source, keeping the caller's order.
///
/// Skills that cannot be tied to a remote (local, imported, or a broken source
/// reference) come back separately: they still update, but one at a time, and
/// nothing about them is shared.
fn group_by_source(
    loaded: Vec<(String, SkillRecord)>,
) -> (
    Vec<(RemoteIdentity, Vec<(String, SkillRecord)>)>,
    Vec<(String, SkillRecord)>,
) {
    let mut groups: Vec<(RemoteIdentity, Vec<(String, SkillRecord)>)> = Vec::new();
    let mut index: HashMap<RemoteIdentity, usize> = HashMap::new();
    let mut ungrouped = Vec::new();

    for (skill_id, skill) in loaded {
        if !is_git_channel(&skill.source_type) {
            ungrouped.push((skill_id, skill));
            continue;
        }
        let identity = match RemoteRef::from_skill(&skill) {
            Ok(source) => source.identity(),
            Err(_) => {
                ungrouped.push((skill_id, skill));
                continue;
            }
        };
        match index.get(&identity) {
            Some(&at) => groups[at].1.push((skill_id, skill)),
            None => {
                index.insert(identity.clone(), groups.len());
                groups.push((identity, vec![(skill_id, skill)]));
            }
        }
    }
    (groups, ungrouped)
}

/// How a round of updates should behave.
///
/// The foreground batch wants speed; the background scheduler wants speed too,
/// but must leave a gap between skills so a user-initiated operation waiting on
/// the central-repo lock wins it. Both go through the same orchestration so the
/// two cannot drift: one resolves per Source, one applies per skill, one writes
/// the library metadata at the end.
pub struct UpdatePolicy<'a> {
    pub yield_between: Option<std::time::Duration>,
    /// Set by the user's cancel button; checked between skills.
    pub cancel: Option<Arc<AtomicBool>>,
    /// Where to report "skill n of m". The scheduler passes none.
    pub progress: Option<&'a (dyn Fn(RoundProgress<'_>) + Sync)>,
}

impl Default for UpdatePolicy<'_> {
    fn default() -> Self {
        Self {
            yield_between: None,
            cancel: None,
            progress: None,
        }
    }
}

impl<'a> UpdatePolicy<'a> {
    /// The background round's policy: yield the lock between skills.
    pub fn yield_to_foreground(gap: std::time::Duration) -> Self {
        Self {
            yield_between: Some(gap),
            ..Default::default()
        }
    }
}

/// How far a round has got, for a UI that would otherwise show a spinner and
/// nothing else for minutes.
pub struct RoundProgress<'a> {
    pub done: usize,
    pub total: usize,
    pub skill: &'a str,
    /// True when the skill is about to start, false when it has finished. A
    /// finished-skill counter alone sits still through the first, slowest
    /// checkout, which reads as a frozen UI.
    pub started: bool,
}

/// Cancel key for the foreground batch, next to the per-skill `update:<id>`.
pub const BATCH_UPDATE_CANCEL_KEY: &str = "update-batch";

/// The round's counters, one per end state.
///
/// A finished skill is filed by one `match` on its outcome, so the buckets are
/// alternatives and never additions: nothing is counted twice, and nothing
/// recoverable by re-pointing or detaching is also counted as a failure. Kept
/// as a value rather than a handful of loose counters so that rule is
/// assertable without a git remote in the test.
#[derive(Default)]
struct RoundTally {
    refreshed: usize,
    unchanged: usize,
    held_back: Vec<String>,
    held_back_paths: usize,
    moved_sources: Vec<MovedSource>,
    gone_sources: Vec<String>,
    new_skills: Vec<NewSkill>,
    failed: Vec<String>,
}

impl RoundTally {
    /// File one finished skill under exactly one heading.
    fn record(&mut self, name: &str, outcome: &Result<UpdateOutcome, AppError>) {
        match outcome {
            Ok(UpdateOutcome::Applied { content_changed: true }) => self.refreshed += 1,
            Ok(UpdateOutcome::Applied { content_changed: false }) => self.unchanged += 1,
            Ok(UpdateOutcome::Held { pending, .. }) => {
                self.held_back_paths += pending.len();
                self.held_back.push(name.to_string());
            }
            Ok(UpdateOutcome::Moved { candidates }) => self.moved_sources.push(MovedSource {
                name: name.to_string(),
                candidates: candidates.clone(),
            }),
            Ok(UpdateOutcome::Gone) => self.gone_sources.push(name.to_string()),
            Err(err) => self.failed.push(format!("{name}: {}", err.message)),
        }
    }

    fn into_result(self, cancelled: bool) -> BatchUpdateSkillsResult {
        BatchUpdateSkillsResult {
            refreshed: self.refreshed,
            unchanged: self.unchanged,
            failed: self.failed,
            held_back: self.held_back,
            cancelled,
            held_back_paths: self.held_back_paths,
            moved_sources: self.moved_sources,
            gone_sources: self.gone_sources,
            new_skills: self.new_skills,
        }
    }
}

/// Update a set of skills: resoleach Source once, then fetch and apply.
pub fn run_update_round(
    store: &SkillStore,
    skill_ids: Vec<String>,
    policy: UpdatePolicy<'_>,
) -> Result<BatchUpdateSkillsResult, AppError> {
    let total = skill_ids.len();
    let mut done = 0usize;
    let mut cancelled = false;
    let mut tally = RoundTally::default();
    let proxy_url = store.proxy_url();

    // Group by Source before doing anything: a library built from a dozen
    // repositories holds many skills per repository, and every skill used to
    // resolve and fetch its own copy. One resolution per Source, and the
    // first checkout warms the cache the siblings then reuse.
    let mut loaded: Vec<(String, SkillRecord)> = Vec::new();
    for skill_id in skill_ids {
        match store.get_skill_by_id(&skill_id).map_err(AppError::db)? {
            Some(skill) => loaded.push((skill_id, skill)),
            None => tally.failed.push(format!("{skill_id}: Skill not found")),
        }
    }
    let report = |done: usize, skill: &str, started: bool| {
        if let Some(progress) = policy.progress {
            progress(RoundProgress {
                done,
                total,
                skill,
                started,
            });
        }
    };
    let is_cancelled =
        || policy.cancel.as_ref().is_some_and(|flag| flag.load(std::sync::atomic::Ordering::SeqCst));

    if is_cancelled() {
        // Stopping must not cost a round of network calls first.
        return Ok(tally.into_result(true));
    }

    // A library read that fails means every stored subpath is unknown, which
    // would present the whole library as new. Skip discovery for the round
    // instead: the update itself does not depend on it.
    let installed = match installed_subpaths_by_source(store) {
        Ok(installed) => Some(installed),
        Err(error) => {
            log::warn!(
                "skipping new-skill discovery: cannot read the library: {}",
                error.message
            );
            None
        }
    };
    let dismissed: HashSet<String> = dismissed_new_skills(store).into_iter().collect();
    let mut seen_new_skills: HashSet<(RemoteIdentity, String)> = HashSet::new();

    let (groups, ungrouped) = group_by_source(loaded);
    // Local and imported sources share nothing and have no remote to
    // resolve, so they keep re-importing one at a time, as they always did.
    for (skill_id, skill) in ungrouped {
        if is_cancelled() {
            cancelled = true;
            break;
        }
        report(done, &skill.name, true);
        match skill.source_type.as_str() {
            "local" | "import" => {
                let outcome = reimport_local_skill_internal(store, &skill_id, None);
                log_reimport_outcome(store, &skill_id, outcome.as_ref());
                match outcome {
                    Ok(ref result) if !result.pending_removals.is_empty() => {
                        tally.held_back.push(skill.name.clone());
                    }
                    Ok(_) => tally.refreshed += 1,
                    Err(ref err) => tally
                        .failed
                        .push(format!("{}: {}", skill.name, &err.message)),
                }
                // Re-import approvals are not stored in the table, so any rows
                // this skill had from an older Git attempt are stale here too.
                if !matches!(&outcome, Ok(result) if !result.pending_removals.is_empty()) {
                    clear_stale_pending_removals(store, &skill_id);
                }
                done += 1;
                report(done, &skill.name, false);
            }
            _ => {
                let message = RemoteRef::from_skill(&skill)
                    .err()
                    .map(|err| err.message)
                    .unwrap_or_else(|| "Skill has no usable source".to_string());
                tally.failed.push(format!("{}: {}", skill.name, message));
            }
        }
    }

    let transport = remote_batch::GitTransport::new(proxy_url.clone());

    // Fetch and apply, Source by Source.
    for (_identity, members) in groups {
        if is_cancelled() {
            cancelled = true;
            break;
        }

        // Every member of this group parsed into a Source when the group was
        // built, so the plan lines up with the members by construction.
        let mut prepared: Vec<Option<Result<PreparedUpdate, AppError>>> =
            (0..members.len()).map(|_| None).collect();
        let mut group_sources: Vec<Option<RemoteRef>> = Vec::with_capacity(members.len());
        for (_, skill) in &members {
            group_sources.push(RemoteRef::from_skill(skill).ok());
        }
        let plan = remote_batch::plan_fetches(
            &group_sources
                .iter()
                .map(|source| source.clone().expect("grouped skills parse"))
                .collect::<Vec<_>>(),
        );

        // The slot that has to refresh goes first and alone: its siblings
        // copy out of what it fetched. Everything else can copy in parallel.
        let (refreshing, reusing): (Vec<usize>, Vec<usize>) = (0..members.len())
            .partition(|at| plan[*at] == git_fetcher::CacheRefresh::Fetch);

        // The first checkout of a Source is where the revision comes from;
        // every later one reuses it rather than asking the remote again.
        let mut head: Option<String> = None;
        let mut scanned: Option<std::path::PathBuf> = None;
        for at in refreshing {
            let (skill_id, skill) = &members[at];
            let outcome = prepare_update(
                store,
                skill_id,
                skill,
                &transport,
                git_fetcher::CacheRefresh::Fetch,
                None,
            );
            if let Ok(prepared_update) = &outcome {
                head = Some(prepared_update.revision.clone());
                if scanned.is_none() {
                    scanned = Some(prepared_update.checkout.clone());
                }
            }
            prepared[at] = Some(outcome);
        }

        if !reusing.is_empty() {
            let fetched = remote_batch::run_concurrent(
                reusing,
                remote_batch::MAX_UPDATE_CONCURRENCY,
                |at| {
                    let (skill_id, skill) = &members[*at];
                    prepare_update(
                        store,
                        skill_id,
                        skill,
                        &transport,
                        git_fetcher::CacheRefresh::Reuse,
                        head.as_deref(),
                    )
                },
            );
            for (at, outcome) in fetched {
                prepared[at] = Some(outcome);
            }
        }

        // The checkout is on disk for the length of this group: the cheapest
        // moment there will ever be to see which skills the Source gained.
        if let (Some(checkout), Some(installed)) = (&scanned, &installed) {
            if let Some((_, any_member)) = members.first() {
                if let Ok(source) = RemoteRef::from_skill(any_member) {
                    let known_subpaths = installed
                        .get(&source.identity())
                        .cloned()
                        .unwrap_or_default();
                    tally.new_skills.extend(collect_new_skills(
                        &source,
                        checkout,
                        &members,
                        &known_subpaths,
                        &dismissed,
                        &mut seen_new_skills,
                    ));
                }
            }
        }

        for (at, (skill_id, skill)) in members.iter().enumerate() {
            // A background round must not starve a user-initiated update: the
            // gap is longer than the foreground lock poll, so a waiter wins.
            if let Some(gap) = policy.yield_between {
                std::thread::sleep(gap);
            }
            let outcome = match prepared[at].take() {
                Some(Ok(prepared)) => {
                    apply_prepared_update(store, skill, &prepared, None, false)
                }
                Some(Err(err)) => Err(err),
                None => Err(AppError::git("Update was not prepared")),
            };
            log_update_entry(
                store,
                skill_id,
                &skill.name,
                "git",
                match &outcome {
                    Ok(applied) => UpdateAudit::from(applied),
                    Err(err) => UpdateAudit::Failed(&err.message),
                },
            );
            if let Err(err) = &outcome {
                record_update_failure(
                    store,
                    skill_id,
                    skill.remote_revision.as_deref(),
                    &err.message,
                );
            }
            tally.record(&skill.name, &outcome);
            if !matches!(&outcome, Ok(UpdateOutcome::Held { .. })) {
                clear_stale_pending_removals(store, skill_id);
            }
        }
    }

    // One description of the library for the whole batch, not one per skill
    // — and under the lock, which every writer of that description takes.
    let _lock = RepoLock::acquire_foreground("batch update metadata").map_err(AppError::db)?;
    sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;

    log::info!(
        "update round: {} refreshed, {} unchanged, {} held back ({} path(s)), {} moved upstream, {} gone upstream, {} failed, cancelled={cancelled}",
        tally.refreshed,
        tally.unchanged,
        tally.held_back.len(),
        tally.held_back_paths,
        tally.moved_sources.len(),
        tally.gone_sources.len(),
        tally.failed.len()
    );

    Ok(tally.into_result(cancelled))
}

#[derive(Clone, serde::Serialize)]
struct UpdateProgressPayload {
    done: usize,
    total: usize,
    skill: String,
}

#[tauri::command]
pub async fn batch_update_skills(
    skill_ids: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
    cancel_registry: State<'_, Arc<InstallCancelRegistry>>,
    app: tauri::AppHandle,
) -> Result<BatchUpdateSkillsResult, AppError> {
    let store = store.inner().clone();
    let registry = cancel_registry.inner().clone();
    let cancel = registry.register(BATCH_UPDATE_CANCEL_KEY);
    let _cancel_guard = CancelRegistrationGuard::new(registry, BATCH_UPDATE_CANCEL_KEY.to_string());

    tauri::async_runtime::spawn_blocking(move || {
        // The UI has nothing else to show for a long round; one event per skill
        // is cheap and lets it say "42 of 137" instead of spinning.
        let emit = |progress: RoundProgress<'_>| {
            let _ = app.emit(
                "skill-update-progress",
                UpdateProgressPayload {
                    done: progress.done,
                    total: progress.total,
                    skill: progress.skill.to_string(),
                },
            );
        };
        run_update_round(
            &store,
            skill_ids,
            UpdatePolicy {
                cancel: Some(cancel),
                progress: Some(&emit),
                ..Default::default()
            },
        )
    })
    .await?
}

/// Stop a running batch update. Skills already finished stay finished.
#[tauri::command]
pub fn cancel_batch_update(cancel_registry: State<'_, Arc<InstallCancelRegistry>>) -> bool {
    cancel_registry.cancel(BATCH_UPDATE_CANCEL_KEY)
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

/// Turn a Git-backed skill into an ordinary local one.
///
/// The counterpart of [`repoint_skill_source_internal`] for the other answer a
/// Source can give: it no longer holds this skill anywhere, so there is nothing
/// to re-point at and the row would otherwise wait forever. Every file stays
/// exactly as it is — the library copy becomes the source of truth, which is
/// what keeps hand edits made while the skill was gone.
///
/// Only the pointer columns change: `source_type` becomes `local`,
/// `source_ref` the library directory itself, and the git-only fields
/// (`source_ref_resolved`, `source_subpath`, `source_branch`,
/// `source_revision`, `remote_revision`) are cleared so the next check hashes
/// the source path instead of looking for a remote. The status becomes
/// `up_to_date`: against itself, which is true and stops the badge.
pub fn detach_skill_source_internal(
    store: &SkillStore,
    skill_id: &str,
) -> Result<ManagedSkillDto, AppError> {
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;

    if !is_git_channel(&skill.source_type) {
        return Err(AppError::invalid_input(
            "Only git-based skills can detach their source",
        ));
    }

    {
        let _lock = RepoLock::acquire_foreground("detach skill source").map_err(AppError::db)?;
        store
            .update_skill_after_reinstall(
                &skill.id,
                &skill.name,
                skill.description.as_deref(),
                "local",
                Some(&skill.central_path),
                None,
                None,
                None,
                None,
                None,
                skill.content_hash.as_deref(),
                "up_to_date",
            )
            .map_err(AppError::db)?;
        // A move answer and a held removal list are about a Source this skill
        // no longer has.
        store
            .clear_skill_move_candidates(&skill.id)
            .map_err(AppError::db)?;
        store
            .clear_skill_pending_removals(&skill.id)
            .map_err(AppError::db)?;
        // Same mirror as every other pointer change: startup reindexes from it,
        // and without this write the git pointer would come back on next launch.
        sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;
    }

    store.log_audit(
        AuditDraft::new("detach")
            .skill(skill.id.clone(), skill.name.clone())
            .detail(format!(
                "{} -> local ({})",
                skill.source_ref.as_deref().unwrap_or("git"),
                skill.central_path
            ))
            .ok(),
    );

    managed_skill_by_id(store, skill_id)
}

#[tauri::command]
pub async fn detach_skill_source(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ManagedSkillDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || detach_skill_source_internal(&store, &skill_id))
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
    let move_candidates = store.get_skill_move_candidates(&skill.id).unwrap_or_default();

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
        move_candidates,
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

    if !is_git_channel(&skill.source_type) {
        return Err(AppError::invalid_input(
            "Only git-based skills can be updated",
        ));
    }

    let transport = remote_batch::GitTransport::with_cancel(proxy_url.map(str::to_string), cancel);
    let prepared = match prepare_update(
        store,
        skill.id.as_str(),
        &skill,
        &transport,
        git_fetcher::CacheRefresh::Fetch,
        None,
    ) {
        Ok(prepared) => prepared,
        Err(err) => {
            // No held answer is true any more once the check itself failed.
            clear_stale_pending_removals(store, &skill.id);
            return Err(err);
        }
    };
    // Applying is the only part that takes the central-repo lock, and it never
    // reaches the network: the checkout is already on disk.
    let update_result = apply_prepared_update(store, &skill, &prepared, approved_removals, true);
    match update_result {
        Ok(outcome) => {
            // A move and a deletion are answers, not errors: both left the
            // library untouched and both ask the user for an action, so they
            // come back as fields the caller can describe in the same words the
            // batch uses. Only an actual failure is an `Err`.
            let (content_changed, pending_removals, removal_approval, moved, gone) = match outcome {
                UpdateOutcome::Applied { content_changed } => {
                    (content_changed, Vec::new(), None, Vec::new(), false)
                }
                UpdateOutcome::Held { pending, approval } => {
                    (false, pending, Some(approval), Vec::new(), false)
                }
                UpdateOutcome::Moved { candidates } => {
                    (false, Vec::new(), None, candidates, false)
                }
                UpdateOutcome::Gone => (false, Vec::new(), None, Vec::new(), true),
            };
            let skill = managed_skill_by_id(store, skill_id)?;
            Ok(UpdateSkillResult {
                skill,
                content_changed,
                pending_removals,
                removal_approval,
                moved,
                gone,
            })
        }
        Err(e) => {
            record_update_failure(store, skill_id, Some(&prepared.revision), &e.message);
            clear_stale_pending_removals(store, skill_id);
            Err(e)
        }
    }
}

/// The stored removals, grouped per skill and location, ready for the panel.
///
/// The store returns one row per path because that is what the token covers;
/// the interface wants contiguous groups, and the library must come first
/// within a skill so the most likely place to rescue a file reads first.
fn pending_removal_groups(store: &SkillStore) -> Result<Vec<PendingRemovalGroup>, AppError> {
    let rows = store.list_pending_removals().map_err(AppError::db)?;
    let mut groups: Vec<PendingRemovalGroup> = Vec::new();
    for row in rows {
        let same_group = groups.last().is_some_and(|group| {
            group.skill_id == row.skill_id && group.location == row.location
        });
        if same_group {
            groups
                .last_mut()
                .expect("checked above")
                .paths
                .push(row.path);
            continue;
        }
        groups.push(PendingRemovalGroup {
            skill_id: row.skill_id,
            skill_name: row.skill_name,
            source: row.source,
            revision: row.revision,
            location: row.location,
            paths: vec![row.path],
        });
    }
    Ok(groups)
}

/// Approve the stored question for each skill, one at a time.
///
/// `update_one` is the existing single-skill update path: this function never
/// removes anything itself, so an approval cannot grow into a bypass. It is a
/// seam only so tests can drive the same loop without a git remote.
///
/// No cross-skill transaction: eight successes and two failures are reported as
/// exactly that. Removal is a per-skill act, and rolling back files after they
/// are gone is not something a transaction can do.
fn approve_pending_removals_with<F>(
    store: &SkillStore,
    skill_ids: Vec<String>,
    mut update_one: F,
) -> Result<ApproveRemovalsResult, AppError>
where
    F: FnMut(&SkillStore, &str, &str) -> Result<ApprovalOutcome, AppError>,
{
    let mut approved = 0usize;
    let mut failed = Vec::new();

    for skill_id in skill_ids {
        let skill = store.get_skill_by_id(&skill_id).map_err(AppError::db)?;
        let name = skill
            .as_ref()
            .map(|skill| skill.name.clone())
            .unwrap_or_else(|| skill_id.clone());

        let Some(stored) = store
            .get_skill_pending_removals(&skill_id)
            .map_err(AppError::db)?
        else {
            failed.push(FailedRemoval {
                name,
                message: "No pending removals to approve".to_string(),
            });
            continue;
        };
        if skill.is_none() {
            failed.push(FailedRemoval {
                name,
                message: "Skill not found".to_string(),
            });
            continue;
        }

        match update_one(store, &skill_id, &stored.approval) {
            Ok(ApprovalOutcome::Applied) => approved += 1,
            Ok(ApprovalOutcome::ListChanged) => failed.push(FailedRemoval {
                name,
                message: PENDING_REMOVALS_CHANGED_MESSAGE.to_string(),
            }),
            Ok(ApprovalOutcome::CannotApply(message)) => {
                failed.push(FailedRemoval { name, message })
            }
            Err(err) => failed.push(FailedRemoval {
                name,
                message: err.message,
            }),
        }
    }

    Ok(ApproveRemovalsResult { approved, failed })
}

fn approve_removals_internal(
    store: &SkillStore,
    skill_ids: Vec<String>,
) -> Result<ApproveRemovalsResult, AppError> {
    let proxy_url = store.proxy_url();
    approve_pending_removals_with(store, skill_ids, |store, skill_id, approval| {
        let outcome = update_git_skill_internal(
            store,
            skill_id,
            proxy_url.as_deref(),
            None,
            Some(approval),
        );
        log_update_outcome(store, skill_id, "git", outcome.as_ref());
        match outcome {
            // The token no longer matched the recomputed list, and the update
            // has already persisted the new one. Saying so is the whole job:
            // applying it anyway is what the token exists to prevent.
            Ok(result) if !result.pending_removals.is_empty() => {
                Ok(ApprovalOutcome::ListChanged)
            }
            Ok(result) if result.gone => Ok(ApprovalOutcome::CannotApply(
                "The Source no longer contains this skill".to_string(),
            )),
            Ok(result) if !result.moved.is_empty() => Ok(ApprovalOutcome::CannotApply(
                "The Source moved this skill; re-point it before approving removals".to_string(),
            )),
            Ok(_) => Ok(ApprovalOutcome::Applied),
            Err(err) => Ok(ApprovalOutcome::CannotApply(err.message)),
        }
    })
}

/// List every skill the last update held back, with the exact paths and the
/// revision it was held at. Empty when nothing is waiting.
#[tauri::command]
pub async fn list_pending_removals(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<PendingRemovalGroup>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || pending_removal_groups(&store)).await?
}

/// Approve the stored removal list for each selected skill.
///
/// Each skill goes through the normal single-skill update with the token it was
/// stored with; there is no bulk-delete path. Serial on purpose: a failure is
/// easier to attribute to one skill than to a concurrent batch.
#[tauri::command]
pub async fn approve_removals(
    skill_ids: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ApproveRemovalsResult, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || approve_removals_internal(&store, skill_ids)).await?
}

/// Move a skill's source pointer to where the Source says it went.
///
/// This is deliberately the whole action: `source_subpath` is the only column
/// written, and no skill content is copied, deleted or replaced. A rename
/// upstream changes where the next update should fetch from, not what the user
/// has on disk, so `central_path`, `content_hash` and every installed file stay
/// exactly as they were; only the metadata mirror is refreshed so the pointer
/// survives the next startup. The row returns to `update_available`: the verdict
/// belonged to the old path, and the next check decides with the new one.
///
/// Keeping `source_ref_resolved` and the branch/revision is what makes this a
/// pointer move rather than a quiet reinstall: the Source is unchanged, only the
/// directory inside it is.
pub fn repoint_skill_source_internal(
    store: &SkillStore,
    skill_id: &str,
    subpath: &str,
) -> Result<ManagedSkillDto, AppError> {
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;

    if !is_git_channel(&skill.source_type) {
        return Err(AppError::invalid_input(
            "Only git-based skills can re-point their source",
        ));
    }

    let subpath = source_membership::normalize_subpath(Some(subpath)).ok_or_else(|| {
        AppError::invalid_input("The new source path must be relative to the repository root")
    })?;
    let stored_subpath = (!subpath.is_empty()).then_some(subpath.as_str());

    sync_metadata::with_repo_lock("re-point skill source", || {
        store.update_skill_source_metadata(
            &skill.id,
            skill.source_ref_resolved.as_deref(),
            stored_subpath,
            skill.source_branch.as_deref(),
            skill.source_revision.as_deref(),
        )?;
        // `update_available` belongs to the new path, and clears the move
        // candidates: their answer was about the old one. The check time is
        // set here too, so a TTL round does not immediately re-ask.
        store.update_skill_check_state(
            &skill.id,
            skill.remote_revision.as_deref(),
            "update_available",
            None,
        )?;
        // The removals were computed against the old path; the next check has
        // to describe the new one before anything is approved.
        store
            .clear_skill_pending_removals(&skill.id)
            .map_err(AppError::db)?;
        // The metadata snapshot is the database's mirror and is what startup
        // reindexes from; without this line the new pointer would be undone by
        // the old file on the next launch. It writes no skill content — the
        // library copy and its hash stay exactly as they were.
        sync_metadata::ensure_skill_metadata_unlocked(store, skill_id)
    })
    .map_err(AppError::db)?;

    store.log_audit(
        AuditDraft::new("repoint")
            .skill(skill.id.clone(), skill.name.clone())
            .detail(format!(
                "{} -> {}",
                skill.source_subpath.as_deref().unwrap_or("."),
                if subpath.is_empty() { "." } else { subpath.as_str() }
            ))
            .ok(),
    );

    managed_skill_by_id(store, skill_id)
}

#[tauri::command]
pub async fn repoint_skill_source(
    skill_id: String,
    subpath: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ManagedSkillDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        repoint_skill_source_internal(&store, &skill_id, &subpath)
    })
    .await?
}

/// A Source's code, resolved and on disk, fetched before any lock is taken.
///
/// Holding the central-repo lock across a network round-trip is what made
/// unrelated foreground operations fail with a 20s "repository is busy" (#315),
/// so the fetch happens here and [`apply_prepared_update`] only writes.
/// A skill a Source now holds that the library does not.
#[derive(Debug, Clone, Serialize)]
pub struct NewSkill {
    pub name: String,
    /// Where it sits in the repository, the form `source_subpath` takes.
    pub subpath: String,
    pub source_ref_resolved: String,
    pub branch: Option<String>,
    /// Stable selection and dismissal key, built here so the UI never has to
    /// decide which URL spellings are the same Source.
    pub key: String,
}

pub struct PreparedUpdate {
    pub source: RemoteRef,
    pub revision: String,
    pub checkout: PathBuf,
}

impl Drop for PreparedUpdate {
    fn drop(&mut self) {
        git_fetcher::cleanup_temp(&self.checkout);
    }
}

/// Resolve one skill's head and put its code on disk. No central-repo lock.
fn prepare_update(
    store: &SkillStore,
    skill_id: &str,
    skill: &SkillRecord,
    transport: &dyn remote_batch::RemoteTransport,
    refresh: git_fetcher::CacheRefresh,
    resolved_head: Option<&str>,
) -> Result<PreparedUpdate, AppError> {
    if !is_git_channel(&skill.source_type) {
        return Err(AppError::invalid_input(
            "Only git-based skills can be updated",
        ));
    }

    let source = RemoteRef::from_skill(skill)?;
    git_fetcher::validate_git_url(&source.clone_url).map_err(AppError::git)?;
    // A batch resolves each Source once and hands the answer to every skill it
    // holds; a single update resolves its own.
    store
        .update_skill_update_status(skill_id, "updating")
        .map_err(AppError::db)?;

    let checkout = transport.fetch_checkout(&source, refresh).map_err(|e| {
        let error = AppError::classify_git_error(e);
        record_update_failure(store, skill_id, skill.remote_revision.as_deref(), &error.message);
        error
    })?;
    // The checkout knows which revision it is at, so the remote is never asked
    // twice: what used to be `ls-remote` then `fetch` is now one fetch.
    let revision = match resolved_head {
        Some(head) => head.to_string(),
        None => git_fetcher::get_head_revision(&checkout)
            .map_err(|e| AppError::git(e.to_string()))?,
    };
    // Pin it here, off the lock: apply must stay filesystem-only so it can be
    // exercised against a plain directory.
    git_fetcher::checkout_revision(&checkout, &revision).map_err(|e| {
        let error = AppError::git(e.to_string());
        record_update_failure(store, skill_id, Some(&revision), &error.message);
        error
    })?;

    Ok(PreparedUpdate {
        source,
        revision,
        checkout,
    })
}

/// Where the checkout says this skill is, once the recorded path is no longer
/// taken on trust.
#[derive(Debug)]
enum UpdateSite {
    /// The recorded path still holds the skill. Nothing changed.
    At(PathBuf),
    /// Somewhere else in the same checkout, strongest candidate first.
    Moved(Vec<MoveCandidate>),
    /// Nowhere in this checkout.
    Gone,
}

/// Why a Source that no longer holds a skill says so, in the row's own words.
/// Deliberately not a failure the user can retry away.
const SOURCE_GONE_MESSAGE: &str =
    "The Source no longer contains this skill in any directory; retrying the update will not help";

/// Ask the checkout where the skill is, instead of assuming the recorded path.
///
/// This replaces the bare "does `<subpath>` exist" question, whose single error
/// answer (`Path '…' does not exist in the repository`) described a rename, a
/// move and a deletion identically — so all three became an update that failed
/// forever and a retry button that could never work.
///
/// The answer is `locate`'s and nothing else. A skills.sh Source names its
/// skill by id, so its reference carries a locator that could still find a
/// directory — but that is a name match, and `locate` already reports name
/// matches as low-confidence candidates. Resolving it here would apply the
/// update and rewrite `source_subpath` from a guess, while the only thing
/// allowed to move a source pointer is the user accepting a candidate
/// (`repoint_skill_source`). A `Moved` answer stays a question; a `Gone` one
/// stays gone rather than being "rescued" into an update from a guess.
fn locate_update_site(skill: &SkillRecord, checkout: &Path) -> UpdateSite {
    match source_membership::locate(skill, checkout) {
        Placement::At { subpath } => UpdateSite::At(if subpath.is_empty() {
            checkout.to_path_buf()
        } else {
            checkout.join(subpath)
        }),
        Placement::Moved { candidates } => UpdateSite::Moved(candidates),
        Placement::Gone => UpdateSite::Gone,
    }
}

/// Install a skill from a checkout that is already on disk.
///
/// The only step that takes the central-repo lock, and it cannot reach the
/// network: everything expensive about the network happened in
/// [`prepare_update`].
fn apply_prepared_update(
    store: &SkillStore,
    skill: &SkillRecord,
    prepared: &PreparedUpdate,
    approved_removals: Option<&str>,
    flush_metadata: bool,
) -> Result<UpdateOutcome, AppError> {
    let source = &prepared.source;
    let revision = &prepared.revision;
    let checkout = &prepared.checkout;
    let update_result = (|| -> Result<UpdateOutcome, AppError> {
        let skill_dir = match locate_update_site(skill, checkout) {
            UpdateSite::At(dir) => {
                // Any earlier move answer is spent: this path holds the skill.
                store
                    .clear_skill_move_candidates(&skill.id)
                    .map_err(AppError::db)?;
                dir
            }
            UpdateSite::Moved(candidates) => {
                // Nothing to apply and nothing to fail: the update is waiting on
                // a decision that is not this round's to make. Recording the
                // head keeps the next check from re-diffing what this fetch
                // just read, and clears the `updating` marker. The candidates
                // are cached for the UI, which has to offer the button after a
                // restart too.
                store
                    .update_skill_check_state(&skill.id, Some(&revision), "moved", None)
                    .map_err(AppError::db)?;
                store
                    .replace_skill_move_candidates(&skill.id, &candidates)
                    .map_err(AppError::db)?;
                // The old list described a path this skill no longer occupies.
                store
                    .clear_skill_pending_removals(&skill.id)
                    .map_err(AppError::db)?;
                return Ok(UpdateOutcome::Moved { candidates });
            }
            UpdateSite::Gone => {
                store
                    .update_skill_check_state(
                        &skill.id,
                        skill.remote_revision.as_deref(),
                        "source_gone",
                        Some(SOURCE_GONE_MESSAGE),
                    )
                    .map_err(AppError::db)?;
                store
                    .clear_skill_pending_removals(&skill.id)
                    .map_err(AppError::db)?;
                return Ok(UpdateOutcome::Gone);
            }
        };

        let new_hash =
            crate::core::content_hash::hash_directory(&skill_dir).map_err(AppError::io)?;
        let content_changed = skill.content_hash.as_deref() != Some(new_hash.as_str());
        let source_subpath = git_fetcher::relative_subpath(&checkout, &skill_dir);
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
        let approval = removal_approval_token(&revision, &pending);
        if !pending.is_empty() && approved_removals != Some(approval.as_str()) {
            // Declining is not a failure: nothing was touched and the update is
            // still waiting. Clear the `updating` marker here, inside the lock,
            // rather than after releasing it — doing it later lets a concurrent
            // update overwrite the state, and swallowing the error would leave
            // the skill showing "updating" forever.
            store
                .update_skill_check_state(
                    &skill.id,
                    Some(&revision),
                    "update_available",
                    None,
                )
                .map_err(AppError::db)?;
            // Persist the question exactly as it was asked: the panel has to
            // survive a restart, and approval must pass back the same token
            // instead of re-deriving a list the user never saw.
            let stored: Vec<PendingRemovalRecord> = pending
                .iter()
                .map(|removal| PendingRemovalRecord {
                    location: removal.location.clone(),
                    path: removal.path.clone(),
                })
                .collect();
            store
                .replace_skill_pending_removals(
                    &skill.id,
                    revision.as_str(),
                    &approval,
                    &stored,
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
                    Some(&source.clone_url),
                    source_subpath.as_deref(),
                    source.branch.as_deref(),
                    Some(&revision),
                )
                .map_err(AppError::db)?;
            store
                .update_skill_after_install(
                    &skill.id,
                    &skill.name,
                    install_result.description.as_deref(),
                    Some(&revision),
                    Some(&revision),
                    Some(&install_result.content_hash),
                    "up_to_date",
                )
                .map_err(AppError::db)?;
            resync_copy_targets(store, &skill.id)?;
            if flush_metadata {
                sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;
            }
        } else {
            store
                .update_skill_source_metadata(
                    &skill.id,
                    Some(&source.clone_url),
                    source_subpath.as_deref(),
                    source.branch.as_deref(),
                    Some(&revision),
                )
                .map_err(AppError::db)?;
            store
                .update_skill_check_state(&skill.id, Some(&revision), "up_to_date", None)
                .map_err(AppError::db)?;
            resync_copy_targets(store, &skill.id)?;
            if flush_metadata {
                sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;
            }
        }
        // Nothing is waiting for an approval any more: whatever list an older
        // held state described, it no longer describes this skill.
        store
            .clear_skill_pending_removals(&skill.id)
            .map_err(AppError::db)?;
        Ok(UpdateOutcome::Applied { content_changed })
    })();
    update_result
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
            Err(AppError::invalid_input(
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

/// Re-point an installed skill at a git source **in place**.
///
/// The skill row is updated by id, so the skill id, tags, preset membership and
/// deployment targets all survive. This is the only safe way to convert a
/// `local` skill to a `git` one: `install` reuses a central directory only when
/// the content hash matches exactly (see `installer::unique_skill_dest`) and
/// otherwise silently allocates `<name>-2`, while `remove` + `install` drops the
/// id and everything keyed to it.
///
/// When the new source's content differs from the current central copy the
/// command refuses unless `force` is set. Re-pointing is not an update: the
/// remote is not yet known to be the authoritative copy, so overwriting local
/// content that may exist nowhere else has to be a deliberate choice.
#[allow(clippy::too_many_arguments)]
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
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;

    // Validate before parsing: resolving a GitHub tree URL runs `ls-remote`, so
    // an unvalidated URL would reach the network first. `install` validates its
    // raw input the same way.
    git_fetcher::validate_git_url(git_url).map_err(AppError::git)?;
    let parsed = git_fetcher::parse_git_source_resolved(git_url, proxy_url);

    // An explicit flag wins over whatever the URL encodes. `--subpath ""` is the
    // caller saying "the skill is at the repo root", which is distinct from
    // omitting the flag and letting the URL decide.
    let branch = branch.map(str::to_string).or_else(|| parsed.branch.clone());
    let subpath = match subpath {
        Some("") => None,
        Some(value) => Some(value.to_string()),
        None => parsed.subpath.clone(),
    };

    let remote_revision =
        git_fetcher::resolve_remote_revision(&parsed.clone_url, branch.as_deref(), proxy_url)
            .map_err(|e| AppError::git(e.to_string()))?;

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
    let marked_updating = std::cell::Cell::new(false);
    let source_committed = std::cell::Cell::new(false);
    let outcome = (|| -> Result<(String, bool), AppError> {
        git_fetcher::checkout_revision(&temp_dir, &remote_revision).map_err(AppError::git)?;
        let skill_dir = resolve_repoint_skill_dir(&temp_dir, subpath.as_deref())?;
        let resolved_subpath = git_fetcher::relative_subpath(&temp_dir, &skill_dir);

        let new_hash =
            crate::core::content_hash::hash_directory(&skill_dir).map_err(AppError::io)?;
        let content_changed = skill.content_hash.as_deref() != Some(new_hash.as_str());

        // Report before refusing: inspecting a skill whose content differs is
        // exactly what --dry-run is for, so it must not need --force to run.
        if dry_run {
            return Ok((resolved_subpath.unwrap_or_default(), content_changed));
        }
        if content_changed && !force {
            return Err(AppError::invalid_input(
                "New source content differs from the current library copy; \
                 re-run with --dry-run to inspect, or --force to overwrite",
            ));
        }

        let _lock = RepoLock::acquire_foreground("set skill source").map_err(AppError::db)?;

        // The clone happened outside the lock, so the skill may have been
        // removed or re-pointed meanwhile. Re-read and refuse to apply a
        // decision made against a stale snapshot.
        let current = store
            .get_skill_by_id(skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill was removed while fetching the source"))?;
        if current.central_path != skill.central_path
            || current.content_hash != skill.content_hash
            || current.source_type != skill.source_type
            || current.source_ref != skill.source_ref
        {
            return Err(AppError::invalid_input(
                "Skill changed while fetching the source; re-run the command",
            ));
        }

        store
            .update_skill_update_status(skill_id, "updating")
            .map_err(AppError::db)?;
        marked_updating.set(true);

        // Identical content needs no file work — swapping would rewrite the
        // central copy for a metadata-only change, and `installer` does not
        // copy exactly the set of files `content_hash` covers, so the rewrite
        // could alter files while still reporting `content_changed: false`.
        let description = if content_changed {
            let staged_path = staged_path_for(&skill.central_path);
            let install_result =
                installer::install_skill_dir_to_destination(&skill_dir, &skill.name, &staged_path)
                    .inspect_err(|_| {
                        let _ = std::fs::remove_dir_all(&staged_path);
                    })
                    .map_err(AppError::io)?;
            swap_skill_directory(&staged_path, Path::new(&skill.central_path))?;
            install_result.description
        } else {
            skill.description.clone()
        };

        store
            .update_skill_after_reinstall(
                &skill.id,
                &skill.name,
                description.as_deref(),
                "git",
                Some(&parsed.original_url),
                Some(&parsed.clone_url),
                resolved_subpath.as_deref(),
                branch.as_deref(),
                Some(&remote_revision),
                Some(&remote_revision),
                Some(&new_hash),
                "up_to_date",
            )
            .map_err(AppError::db)?;
        source_committed.set(true);
        resync_copy_targets(store, &skill.id)?;
        sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;
        Ok((resolved_subpath.unwrap_or_default(), content_changed))
    })();

    git_fetcher::cleanup_temp(&temp_dir);

    match outcome {
        Ok((resolved_subpath, content_changed)) => Ok(SetSourceResult {
            skill_id: skill.id,
            name: skill.name,
            previous_source_type: skill.source_type,
            previous_source_ref: skill.source_ref,
            clone_url: parsed.clone_url,
            subpath: (!resolved_subpath.is_empty()).then_some(resolved_subpath),
            branch,
            revision: remote_revision,
            content_changed,
            dry_run,
        }),
        Err(e) => {
            // Only clear `updating` if this call actually set it. A refusal
            // (bad subpath, content differs without --force) touched nothing,
            // so marking the skill as errored would be a lie.
            //
            // `update_skill_check_state` always writes the revision column, so
            // it has to be given the one that matches whichever source the row
            // now describes: the new source's revision once the re-point
            // committed, otherwise the revision the old source already had.
            // Passing the newly resolved revision unconditionally would file a
            // commit from the new repo under a skill still pointing at the old
            // one; passing None would blank a revision that is still valid.
            if marked_updating.get() {
                let revision = if source_committed.get() {
                    Some(remote_revision.as_str())
                } else {
                    skill.remote_revision.as_deref()
                };
                let _ = store.update_skill_check_state(
                    skill_id,
                    revision,
                    "error",
                    Some(&e.message),
                );
            }
            Err(e)
        }
    }
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
            let git_source = RemoteRef::from_skill(&skill)?;
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
            let Some(PrefetchedRemote {
                result: remote_result,
                unchanged,
                ..
            }) = prefetched.filter(|prefetched| prefetched.key == git_source.identity())
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
                        // The repo moved but this skill's directory did not
                        // (#561). Record the new head as its source, as an
                        // update that changed nothing would, so the next
                        // check compares from there without fetching again.
                        Some(_)
                            if git_source.installed_subtree(&skill)
                                .is_some_and(|installed| unchanged.contains(&installed)) =>
                        {
                            store
                                .update_skill_source_revision(&skill.id, &remote_revision)
                                .map_err(AppError::db)?;
                            "up_to_date"
                        }
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
        // A website snapshot has no revision to compare against; it stays
        // `local_only` rather than turning `unknown` on every check.
        "well-known" => {}
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
    // `moved` and `source_gone` are answers, not revisions waiting to be
    // compared: a check can only ask the Source about a path that no longer
    // holds the skill, so re-diffing one would replace the answer with a
    // verdict about the wrong directory and drop the candidates the row's
    // button is made of. Neither TTL nor `force` has anything to add — the
    // exits are re-pointing the source, detaching it, or deleting the skill.
    if matches!(skill.update_status.as_str(), "moved" | "source_gone") {
        return Ok(true);
    }

    if force {
        return Ok(false);
    }

    let ttl_minutes = store
        .get_setting("update_check_ttl_minutes")
        .map_err(AppError::db)?
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(60);
    let ttl_ms = ttl_minutes * 60 * 1000;
    // Statuses a TTL round must not re-check: the answer is already known and
    // asking again cannot change it within the TTL.
    let stable_status = matches!(
        skill.update_status.as_str(),
        "up_to_date" | "update_available" | "source_missing" | "local_only"
    );

    Ok(stable_status
        && skill
            .last_checked_at
            .map(|checked| chrono::Utc::now().timestamp_millis() - checked < ttl_ms)
            .unwrap_or(false))
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
        if dir_name_str.starts_with(git_fetcher::CLONE_TEMP_PREFIX)
            || dir_name_str.starts_with(well_known::TEMP_DIR_PREFIX)
        {
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
    resolve_install_target(store, "skillssh", source_ref, skill_id)
}

/// A reinstall keeps the directory its record already owns; a new skill takes
/// the first name that is neither another record's (even with identical
/// content) nor an existing folder, which installing would replace.
fn resolve_install_target(
    store: &SkillStore,
    source_type: &str,
    source_ref: &str,
    skill_id: &str,
) -> Result<(String, PathBuf), AppError> {
    if let Some(existing) = store
        .get_skill_by_source_ref(source_type, source_ref)
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
        let occupied = candidate_path.exists()
            || store
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
        // An update that changed no content leaves the copies alone.
        if scenario_service::recorded_target_is_current(
            Some(&target),
            &source,
            Path::new(&target.target_path),
            sync_engine::SyncMode::Copy,
            skill.content_hash.as_deref(),
        ) {
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

    // ── batch_update_skills grouping ──

    fn stored(id: &str, source: &str, branch: Option<&str>, subpath: Option<&str>) -> (String, SkillRecord) {
        let mut skill = SkillRecord {
            id: id.to_string(),
            name: id.to_string(),
            description: None,
            source_type: "git".to_string(),
            source_ref: Some(source.to_string()),
            source_ref_resolved: Some(source.to_string()),
            source_subpath: subpath.map(str::to_string),
            source_branch: branch.map(str::to_string),
            source_revision: None,
            remote_revision: None,
            central_path: format!("/fixture/{id}"),
            content_hash: None,
            enabled: true,
            created_at: 0,
            updated_at: 0,
            status: "ok".to_string(),
            update_status: "update_available".to_string(),
            last_checked_at: None,
            last_check_error: None,
        };
        skill.name = id.to_string();
        (id.to_string(), skill)
    }

    fn git(args: &[&str], cwd: &Path) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    /// A checkout with a committed tree, which is what discovery reads.
    fn git_checkout(skills: &[&str]) -> TempDir {
        let tmp = tempdir().unwrap();
        for skill in skills {
            let name = skill.rsplit('/').next().unwrap();
            let dir = tmp.path().join(skill);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: fixture\n---\n"),
            )
            .unwrap();
        }
        git(&["init", "-q"], tmp.path());
        git(&["add", "-A"], tmp.path());
        git(
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "fixture",
            ],
            tmp.path(),
        );
        tmp
    }

    /// The discovered skill's Source is built, not parsed out of a tree URL: a
    /// missing branch must stay missing and the subpath must survive verbatim.
    #[test]
    fn an_installed_new_skill_keeps_its_branch_and_subpath_separate() {
        let source = new_skill_source(
            "https://gitlab.com/acme/skills.git",
            None,
            "skills/beta",
        )
        .unwrap();
        assert_eq!(source.clone_url, "https://gitlab.com/acme/skills.git");
        assert_eq!(source.branch, None, "no branch is not a branch named 'skills'");
        assert_eq!(source.subpath.as_deref(), Some("skills/beta"));

        let branched =
            new_skill_source("git@gitlab.com:acme/skills.git", Some("next"), "skills/beta")
                .unwrap();
        assert_eq!(branched.branch.as_deref(), Some("next"));

        let root = new_skill_source("https://example.com/acme/skills", None, "").unwrap();
        assert_eq!(root.subpath, None, "the repository root is the whole checkout");
    }

    /// The key is the only comparison the frontend gets, so spellings that name
    /// one Source must fold to one key, and the branch must not disappear.
    #[test]
    fn a_new_skill_key_folds_spellings_and_keeps_the_branch() {
        let plain = RemoteRef::from_url("https://example.com/acme/skills");
        let dotted = RemoteRef::from_url("https://example.com/acme/skills.git");
        let slashed = RemoteRef::from_url("https://example.com/acme/skills/");
        let key = new_skill_key(&plain.identity(), "Fresh");

        assert_eq!(key, new_skill_key(&dotted.identity(), "fresh"));
        assert_eq!(key, new_skill_key(&slashed.identity(), "fresh"));

        let mut next = plain.clone();
        next.branch = Some("next".to_string());
        assert_ne!(key, new_skill_key(&next.identity(), "fresh"));
    }

    /// A refusal is remembered, and the list has a ceiling so a library used for
    /// years cannot grow it without bound.
    #[test]
    fn a_refused_new_skill_is_not_offered_again() {
        let repo = test_repo();
        let identity = RemoteRef::from_url("https://example.com/acme/skills.git").identity();
        let key = new_skill_key(&identity, "fresh");

        remember_dismissed_new_skills(&repo.store, vec![key.clone()]).unwrap();
        assert!(dismissed_new_skills(&repo.store).contains(&key));

        // Refusing twice is not two entries.
        remember_dismissed_new_skills(&repo.store, vec![key.clone()]).unwrap();
        assert_eq!(dismissed_new_skills(&repo.store).len(), 1);

        let overflow = (0..=MAX_DISMISSED_NEW_SKILLS)
            .map(|index| format!("{key}-{index}"))
            .collect();
        remember_dismissed_new_skills(&repo.store, overflow).unwrap();
        let all = dismissed_new_skills(&repo.store);
        assert_eq!(all.len(), MAX_DISMISSED_NEW_SKILLS);
        assert!(!all.contains(&key), "the oldest entry is the one dropped");
        assert!(all.contains(&format!("{key}-{MAX_DISMISSED_NEW_SKILLS}")));
    }

    /// The bug the ticket's first acceptance test names: a renamed skill is
    /// locate's answer, and must not also be offered as a new install. That
    /// install would rewrite the existing record's source path.
    #[test]
    fn a_rename_is_not_also_offered_as_a_new_skill() {
        let checkout = git_checkout(&["skills/alpha", "skills/beta"]);
        let record = stored(
            "alpha",
            "https://example.com/acme/skills",
            None,
            Some("skills/old-alpha"),
        )
        .1;
        let source = RemoteRef::from_skill(&record).unwrap();
        let members = vec![("alpha".to_string(), record)];
        let mut seen = HashSet::new();

        let found = collect_new_skills(
            &source,
            checkout.path(),
            &members,
            &[],
            &HashSet::new(),
            &mut seen,
        );

        assert_eq!(
            found.iter().map(|skill| skill.name.as_str()).collect::<Vec<_>>(),
            vec!["beta"]
        );
    }

    /// `.git` and a trailing slash describe one repository. Comparing the
    /// stored URL as a string missed that, so an installed skill came back as
    /// new and installing it would overwrite the live directory.
    #[test]
    fn url_spellings_of_one_source_do_not_produce_a_second_prompt() {
        let repo = test_repo();
        let checkout = git_checkout(&["skills/alpha", "skills/beta"]);
        let installed = stored(
            "alpha",
            "https://example.com/acme/skills.git",
            None,
            Some("skills/alpha"),
        )
        .1;
        repo.store.insert_skill(&installed).unwrap();

        let by_source = installed_subpaths_by_source(&repo.store).unwrap();
        let source = RemoteRef::from_url("https://example.com/acme/skills/");
        let known = by_source
            .get(&source.identity())
            .cloned()
            .expect("the same remote, however it is spelled");

        let mut seen = HashSet::new();
        let found = collect_new_skills(
            &source,
            checkout.path(),
            &[],
            &known,
            &HashSet::new(),
            &mut seen,
        );

        assert_eq!(
            found.iter().map(|skill| skill.name.as_str()).collect::<Vec<_>>(),
            vec!["beta"]
        );
    }

    #[test]
    fn a_batch_asks_each_source_once() {
        let loaded = vec![
            stored("a", "https://github.com/mattpocock/skills.git", None, Some("a")),
            stored("b", "https://github.com/mattpocock/skills.git", None, Some("b")),
            stored("c", "https://github.com/mattpocock/skills.git", None, None),
            stored("d", "https://github.com/vercel/ai.git", None, None),
            stored("e", "https://github.com/mattpocock/skills.git", Some("next"), None),
        ];
        let (groups, ungrouped) = group_by_source(loaded);

        assert!(ungrouped.is_empty());
        assert_eq!(groups.len(), 3, "three distinct remotes to resolve");
        let sizes: Vec<usize> = groups.iter().map(|(_, members)| members.len()).collect();
        assert_eq!(sizes, vec![3, 1, 1], "subpath siblings collapse; branch does not");
    }

    #[test]
    fn local_and_broken_sources_are_not_grouped() {
        let mut local = stored("l1", "https://example.com/x.git", None, None).1;
        local.source_type = "local".to_string();
        let mut broken = stored("b1", "https://example.com/y.git", None, None).1;
        broken.source_ref = None;
        broken.source_ref_resolved = None;

        let (groups, ungrouped) =
            group_by_source(vec![("l1".to_string(), local), ("b1".to_string(), broken)]);

        assert!(groups.is_empty());
        assert_eq!(ungrouped.len(), 2);
    }

    // ── apply_prepared_update: the three end states ──

    fn write_tree(root: &std::path::Path, files: &[(&str, &str)]) {
        for (name, contents) in files {
            let path = root.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, contents).unwrap();
        }
    }

    /// A git skill already in the library, plus a prepared checkout shaped like
    /// one the transport would hand over.
    ///
    /// Both sides carry a `SKILL.md`: the checkout is scanned as a tree of real
    /// skills, and a directory without one is not a skill directory at all — a
    /// fixture missing it would describe something the updater never sees.
    fn prepared_fixture(
        repo: &TestRepo,
        central_files: &[(&str, &str)],
        checkout_files: &[(&str, &str)],
    ) -> (SkillRecord, PreparedUpdate, TempDir) {
        prepared_fixture_named(
            repo,
            "skill-under-test",
            "skill-under-test",
            "skill",
            central_files,
            checkout_files,
        )
    }

    fn prepared_fixture_named(
        repo: &TestRepo,
        id: &str,
        name: &str,
        subpath: &str,
        central_files: &[(&str, &str)],
        checkout_files: &[(&str, &str)],
    ) -> (SkillRecord, PreparedUpdate, TempDir) {
        const SKILL_MD: (&str, &str) = ("SKILL.md", "---\nname: skill-under-test\ndescription: fixture\n---\n\nbody\n");
        let central_path = central_repo::skills_dir().join(id);
        write_tree(&central_path, &[&[SKILL_MD], central_files].concat());
        let hash = crate::core::content_hash::hash_directory(&central_path).unwrap();

        let record = SkillRecord {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            source_type: "git".to_string(),
            source_ref: Some("https://example.com/acme/skills".to_string()),
            source_ref_resolved: Some("https://example.com/acme/skills".to_string()),
            source_subpath: Some(subpath.to_string()),
            source_branch: None,
            source_revision: Some("rev1".to_string()),
            remote_revision: Some("rev1".to_string()),
            central_path: central_path.to_string_lossy().to_string(),
            content_hash: Some(hash),
            enabled: true,
            created_at: 0,
            updated_at: 0,
            status: "ok".to_string(),
            update_status: "update_available".to_string(),
            last_checked_at: None,
            last_check_error: None,
        };
        repo.store.upsert_skill(&record).unwrap();

        let checkout = tempdir().unwrap();
        write_tree(
            &checkout.path().join(subpath),
            &[&[SKILL_MD], checkout_files].concat(),
        );
        let prepared = PreparedUpdate {
            source: RemoteRef::from_skill(&record).unwrap(),
            revision: "rev2".to_string(),
            checkout: checkout.path().to_path_buf(),
        };
        (record, prepared, checkout)
    }

    #[test]
    fn unchanged_content_installs_nothing_and_records_the_new_revision() {
        let repo = test_repo();
        let (record, prepared, _checkout) = prepared_fixture(
            &repo,
            &[("a.txt", "same")],
            &[("a.txt", "same")],
        );

        let outcome = apply_prepared_update(&repo.store, &record, &prepared, None, false).unwrap();

        assert!(matches!(outcome, UpdateOutcome::Applied { content_changed: false }));
        let after = repo.store.get_skill_by_id(&record.id).unwrap().unwrap();
        assert_eq!(after.source_revision.as_deref(), Some("rev2"));
        assert_eq!(after.update_status, "up_to_date");
    }

    #[test]
    fn changed_content_replaces_the_library_copy() {
        let repo = test_repo();
        let (record, prepared, _checkout) = prepared_fixture(
            &repo,
            &[("a.txt", "old")],
            &[("a.txt", "new"), ("b.txt", "added")],
        );

        let outcome = apply_prepared_update(&repo.store, &record, &prepared, None, false).unwrap();

        assert!(matches!(outcome, UpdateOutcome::Applied { content_changed: true }));
        let central = std::path::Path::new(&record.central_path);
        assert_eq!(fs::read_to_string(central.join("a.txt")).unwrap(), "new");
        assert_eq!(fs::read_to_string(central.join("b.txt")).unwrap(), "added");
        let after = repo.store.get_skill_by_id(&record.id).unwrap().unwrap();
        assert_eq!(after.update_status, "up_to_date");
        assert_ne!(after.content_hash, record.content_hash);
    }

    #[test]
    fn a_dropped_file_holds_the_update_back_for_a_decision() {
        let repo = test_repo();
        // The library has a file the new revision does not: applying it would
        // remove something the user may still want.
        let (record, prepared, _checkout) = prepared_fixture(
            &repo,
            &[("a.txt", "old"), ("keep-me.txt", "mine")],
            &[("a.txt", "new")],
        );

        let outcome = apply_prepared_update(&repo.store, &record, &prepared, None, false).unwrap();

        match outcome {
            UpdateOutcome::Held { pending, approval } => {
                assert!(pending
                    .iter()
                    .any(|removal| removal.path.contains("keep-me.txt")));
                assert!(!approval.is_empty());
            }
            other => panic!("expected the update to be held, got {other:?}"),
        }
        // Nothing was touched, and the row is still waiting rather than stuck.
        let central = std::path::Path::new(&record.central_path);
        assert_eq!(fs::read_to_string(central.join("a.txt")).unwrap(), "old");
        assert_eq!(fs::read_to_string(central.join("keep-me.txt")).unwrap(), "mine");
        let after = repo.store.get_skill_by_id(&record.id).unwrap().unwrap();
        assert_eq!(after.update_status, "update_available");
    }

    #[test]
    fn a_held_update_persists_every_path_and_its_revision() {
        let repo = test_repo();
        let (record, prepared, _checkout) = prepared_fixture(
            &repo,
            &[("a.txt", "old"), ("keep-me.txt", "mine")],
            &[("a.txt", "new")],
        );
        // A copy-mode deployment holds a file the new version also drops: the
        // stored list must name that directory too, not just the library.
        let target_dir = repo._tmp.path().join("agent/skill-under-test");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("SKILL.md"), "---\nname: skill-under-test\n---\n").unwrap();
        fs::write(target_dir.join("agent-only.md"), "agent copy").unwrap();
        repo.store
            .insert_target(&SkillTargetRecord {
                id: "target-1".to_string(),
                skill_id: record.id.clone(),
                tool: "claude_code".to_string(),
                target_path: target_dir.to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: None,
            })
            .unwrap();

        let held = apply_prepared_update(&repo.store, &record, &prepared, None, false).unwrap();
        let approval = match held {
            UpdateOutcome::Held { pending, approval } => {
                assert_eq!(pending.len(), 2);
                approval
            }
            other => panic!("expected the update to be held, got {other:?}"),
        };

        let groups = pending_removal_groups(&repo.store).unwrap();
        assert_eq!(groups.len(), 2, "the library and one agent copy");
        let group = &groups[0];
        assert_eq!(group.skill_id, record.id);
        assert_eq!(group.skill_name, record.name);
        assert_eq!(group.source, "https://example.com/acme/skills");
        assert_eq!(group.revision, "rev2");
        assert_eq!(group.location, LIBRARY_LOCATION, "the library reads first");
        assert_eq!(group.paths, vec!["keep-me.txt".to_string()]);
        assert_eq!(groups[1].location, "claude_code");
        assert_eq!(groups[1].paths, vec!["agent-only.md".to_string()]);

        let stored = repo
            .store
            .get_skill_pending_removals(&record.id)
            .unwrap()
            .expect("the held list must survive the call");
        assert_eq!(stored.revision, "rev2");
        assert_eq!(stored.approval, approval, "the token shown is the token stored");
        assert_eq!(
            stored.removals,
            vec![
                PendingRemovalRecord {
                    location: LIBRARY_LOCATION.to_string(),
                    path: "keep-me.txt".to_string(),
                },
                PendingRemovalRecord {
                    location: "claude_code".to_string(),
                    path: "agent-only.md".to_string(),
                },
            ]
        );
    }

    #[test]
    fn applying_or_approving_a_held_update_clears_its_rows() {
        let repo = test_repo();
        let (held, prepared, _checkout) = prepared_fixture_named(
            &repo,
            "held",
            "held",
            "skill",
            &[("a.txt", "old"), ("keep-me.txt", "mine")],
            &[("a.txt", "new")],
        );
        apply_prepared_update(&repo.store, &held, &prepared, None, false).unwrap();
        let stored = repo
            .store
            .get_skill_pending_removals(&held.id)
            .unwrap()
            .expect("held first");
        assert!(!stored.removals.is_empty());

        let approved = apply_prepared_update(
            &repo.store,
            &held,
            &prepared,
            Some(&stored.approval),
            false,
        )
        .unwrap();
        assert!(matches!(
            approved,
            UpdateOutcome::Applied {
                content_changed: true
            }
        ));
        assert!(!Path::new(&held.central_path).join("keep-me.txt").exists());
        assert!(
            repo.store
                .get_skill_pending_removals(&held.id)
                .unwrap()
                .is_none(),
            "an approved list must not outlive the approval"
        );

        // A skill that ends `up_to_date` is just as finished: a row from an
        // older held state must not survive the verdict.
        let (clean, clean_prepared, _clean_checkout) = prepared_fixture_named(
            &repo,
            "clean",
            "clean",
            "skill",
            &[("a.txt", "same")],
            &[("a.txt", "same")],
        );
        repo.store
            .replace_skill_pending_removals(
                &clean.id,
                "rev1",
                "stale-token",
                &[PendingRemovalRecord {
                    location: LIBRARY_LOCATION.to_string(),
                    path: "gone.txt".to_string(),
                }],
            )
            .unwrap();
        let outcome =
            apply_prepared_update(&repo.store, &clean, &clean_prepared, None, false).unwrap();
        assert!(matches!(
            outcome,
            UpdateOutcome::Applied {
                content_changed: false
            }
        ));
        assert!(repo
            .store
            .get_skill_pending_removals(&clean.id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_stale_stored_token_never_deletes_files() {
        let repo = test_repo();
        let (record, prepared, _checkout) = prepared_fixture(
            &repo,
            &[("a.txt", "old"), ("keep-me.txt", "mine")],
            &[("a.txt", "new")],
        );
        apply_prepared_update(&repo.store, &record, &prepared, None, false).unwrap();

        // The stored question now describes a revision the remote has left.
        let stored = repo
            .store
            .get_skill_pending_removals(&record.id)
            .unwrap()
            .unwrap();
        repo.store
            .replace_skill_pending_removals(
                &record.id,
                "rev-old",
                "a-token-for-a-different-question",
                &stored.removals,
            )
            .unwrap();

        let result = approve_pending_removals_with(
            &repo.store,
            vec![record.id.clone()],
            |store, _id, token| match apply_prepared_update(
                store, &record, &prepared, Some(token), false,
            )? {
                UpdateOutcome::Applied { .. } => Ok(ApprovalOutcome::Applied),
                UpdateOutcome::Held { .. } => Ok(ApprovalOutcome::ListChanged),
                UpdateOutcome::Moved { .. } => {
                    Ok(ApprovalOutcome::CannotApply("moved".to_string()))
                }
                UpdateOutcome::Gone => Ok(ApprovalOutcome::CannotApply("gone".to_string())),
            },
        )
        .unwrap();

        assert_eq!(result.approved, 0);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].name, record.name);
        assert_eq!(result.failed[0].message, PENDING_REMOVALS_CHANGED_MESSAGE);
        let central = Path::new(&record.central_path);
        assert!(central.join("keep-me.txt").is_file(), "nothing may be deleted");
        let after = repo.store.get_skill_by_id(&record.id).unwrap().unwrap();
        assert_eq!(after.update_status, "update_available");
        let regrown = repo
            .store
            .get_skill_pending_removals(&record.id)
            .unwrap()
            .expect("the new question must replace the stale one");
        assert_eq!(regrown.revision, "rev2");
        assert_ne!(regrown.approval, "a-token-for-a-different-question");
    }

    #[test]
    fn approving_a_batch_reports_each_skill_and_leaves_the_stale_one_alone() {
        let repo = test_repo();
        let mut fixtures: HashMap<String, (SkillRecord, PreparedUpdate)> = HashMap::new();
        let mut checkouts = Vec::new();
        for id in ["a", "b", "c"] {
            let (record, prepared, checkout) = prepared_fixture_named(
                &repo,
                id,
                id,
                "skill",
                &[("a.txt", "old"), ("mine.txt", "mine")],
                &[("a.txt", "new")],
            );
            fixtures.insert(id.to_string(), (record, prepared));
            checkouts.push(checkout);
        }

        for (record, prepared) in fixtures.values() {
            let outcome =
                apply_prepared_update(&repo.store, record, prepared, None, false).unwrap();
            assert!(matches!(outcome, UpdateOutcome::Held { .. }));
        }

        // One stored question no longer matches the revision a fresh update
        // computes, so its approval cannot cover the list.
        let (stale_record, _) = fixtures.get("c").unwrap();
        let stale = repo
            .store
            .get_skill_pending_removals(&stale_record.id)
            .unwrap()
            .unwrap();
        repo.store
            .replace_skill_pending_removals(
                &stale_record.id,
                "rev-old",
                "stale-token",
                &stale.removals,
            )
            .unwrap();

        let result = approve_pending_removals_with(
            &repo.store,
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
            |store, id, token| {
                let (record, prepared) = fixtures.get(id).expect("known fixture");
                match apply_prepared_update(store, record, prepared, Some(token), false)? {
                    UpdateOutcome::Applied { .. } => Ok(ApprovalOutcome::Applied),
                    UpdateOutcome::Held { .. } => Ok(ApprovalOutcome::ListChanged),
                    UpdateOutcome::Moved { .. } => {
                        Ok(ApprovalOutcome::CannotApply("moved".to_string()))
                    }
                    UpdateOutcome::Gone => Ok(ApprovalOutcome::CannotApply("gone".to_string())),
                }
            },
        )
        .unwrap();

        assert_eq!(result.approved, 2);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].name, "c");
        let central = |id: &str| {
            Path::new(&fixtures.get(id).unwrap().0.central_path).to_path_buf()
        };
        assert!(!central("a").join("mine.txt").exists());
        assert!(!central("b").join("mine.txt").exists());
        assert!(
            central("c").join("mine.txt").is_file(),
            "the stale approval must not delete anything"
        );
        for id in ["a", "b"] {
            assert!(
                repo.store
                    .get_skill_pending_removals(&fixtures[id].0.id)
                    .unwrap()
                    .is_none()
            );
        }
        let still_held = repo
            .store
            .get_skill_pending_removals(&fixtures["c"].0.id)
            .unwrap()
            .expect("the refreshed question stays for the panel");
        assert_eq!(still_held.revision, "rev2");
    }

    /// The batch pays for one whole-library rewrite, at the end. Anything that
    /// forgets to pass `flush_metadata: false` turns an O(N) batch into O(N²).
    #[test]
    fn the_library_is_rewritten_once_per_batch_not_once_per_skill() {
        use std::sync::atomic::Ordering;

        let repo = test_repo();
        let (record, prepared, _checkout) = prepared_fixture(
            &repo,
            &[("a.txt", "old")],
            &[("a.txt", "new")],
        );

        let before = crate::core::sync_metadata::WRITE_ALL_CALLS.load(Ordering::SeqCst);
        apply_prepared_update(&repo.store, &record, &prepared, None, false).unwrap();
        let after_skill = crate::core::sync_metadata::WRITE_ALL_CALLS.load(Ordering::SeqCst);
        assert_eq!(
            after_skill, before,
            "a skill inside a batch must not rewrite the whole library"
        );

        let (record2, prepared2, _checkout2) = prepared_fixture(
            &repo,
            &[("a.txt", "old")],
            &[("a.txt", "newer")],
        );
        apply_prepared_update(&repo.store, &record2, &prepared2, None, true).unwrap();
        let after_batch = crate::core::sync_metadata::WRITE_ALL_CALLS.load(Ordering::SeqCst);
        assert_eq!(
            after_batch,
            before + 1,
            "the batch boundary writes it exactly once"
        );
    }

    #[test]
    fn a_cancelled_round_stops_before_touching_the_network() {
        let repo = test_repo();
        let (record, _prepared, _checkout) = prepared_fixture(
            &repo,
            &[("a.txt", "old")],
            &[("a.txt", "new")],
        );

        let summary = run_update_round(
            &repo.store,
            vec![record.id.clone()],
            UpdatePolicy {
                cancel: Some(Arc::new(AtomicBool::new(true))),
                ..Default::default()
            },
        )
        .unwrap();

        assert!(summary.cancelled);
        assert_eq!(summary.refreshed, 0);
        // The library copy is untouched: the round never got as far as a skill.
        let central = std::path::Path::new(&record.central_path);
        assert_eq!(fs::read_to_string(central.join("a.txt")).unwrap(), "old");
    }

    /// Medium 5: the round's four end states are alternatives, so each one
    /// lands in its own bucket and in no other. In particular a move is not
    /// also a refresh, and a deletion is not also a failure — the bug that
    /// started this was one upstream rename reported as 15 failures.
    #[test]
    fn the_rounds_four_end_states_never_share_a_bucket() {
        let mut tally = RoundTally::default();
        tally.record("at", &Ok(UpdateOutcome::Applied { content_changed: true }));
        tally.record(
            "moved",
            &Ok(UpdateOutcome::Moved {
                candidates: vec![MoveCandidate {
                    subpath: "moved-here".to_string(),
                    confidence: Confidence::High,
                    reason: source_membership::MoveReason::ContentHash,
                }],
            }),
        );
        tally.record("gone", &Ok(UpdateOutcome::Gone));
        tally.record("failed", &Err(AppError::git("boom")));

        let summary = tally.into_result(false);

        assert_eq!(summary.refreshed, 1, "At lands in refreshed");
        assert_eq!(summary.unchanged, 0);
        assert!(summary.held_back.is_empty());
        assert_eq!(summary.held_back_paths, 0);
        assert_eq!(summary.moved_sources.len(), 1);
        assert_eq!(summary.moved_sources[0].name, "moved");
        assert_eq!(summary.gone_sources, vec!["gone".to_string()]);
        assert_eq!(summary.failed, vec!["failed: boom".to_string()]);
        // Nothing is in two places: the four names are accounted for once each.
        assert!(!summary.failed.iter().any(|entry| entry.starts_with("moved")));
        assert!(!summary.failed.iter().any(|entry| entry.starts_with("gone")));
        assert!(!summary.gone_sources.iter().any(|name| name == "moved"));
    }

    // ── apply_prepared_update: a Source that no longer holds the recorded path ──

    /// Replace the checkout's fixture layout with another one, as an upstream
    /// push would.
    fn rewrite_checkout(checkout: &TempDir, files: &[(&str, &str)]) {
        fs::remove_dir_all(checkout.path().join("skill")).unwrap();
        write_tree(checkout.path(), files);
    }

    /// Upstream renamed or moved the directory: the skill is still there, and
    /// applying the fetched tree from the old path would have failed forever.
    #[test]
    fn a_renamed_directory_is_reported_as_moved_with_its_candidates() {
        let repo = test_repo();
        let (record, prepared, checkout) = prepared_fixture(
            &repo,
            &[("a.txt", "old")],
            &[("a.txt", "old")],
        );
        // The report's shape: `skills/<name>` became `.agents/skills/<name>`.
        rewrite_checkout(
            &checkout,
            &[(
                ".agents/skills/skill-under-test/SKILL.md",
                "---\nname: skill-under-test\n---\n\nbody\n",
            )],
        );

        let outcome = apply_prepared_update(&repo.store, &record, &prepared, None, false).unwrap();

        match outcome {
            UpdateOutcome::Moved { candidates } => {
                // Only the name matched — the content hash cannot, because the
                // new directory holds a `SKILL.md` the library copy never had.
                assert_eq!(
                    candidates,
                    vec![MoveCandidate {
                        subpath: ".agents/skills/skill-under-test".to_string(),
                        confidence: Confidence::Low,
                        reason: source_membership::MoveReason::DirectoryName,
                    }]
                );
            }
            other => panic!("expected a move, got {other:?}"),
        }

        // Nothing was written: re-pointing the source is the user's decision.
        let central = std::path::Path::new(&record.central_path);
        assert_eq!(fs::read_to_string(central.join("a.txt")).unwrap(), "old");
        let after = repo.store.get_skill_by_id(&record.id).unwrap().unwrap();
        assert_eq!(after.source_subpath.as_deref(), Some("skill"));
        assert_eq!(after.source_revision.as_deref(), Some("rev1"));
        // Waiting on that decision, not stuck at `updating` forever.
        assert_eq!(after.update_status, "moved");
        assert!(after.last_check_error.is_none());
    }

    /// Identical bytes elsewhere is the strong answer: the path moved, and
    /// nothing else about the skill did.
    #[test]
    fn identical_content_elsewhere_is_a_high_confidence_move() {
        let repo = test_repo();
        let (record, prepared, checkout) = prepared_fixture(
            &repo,
            &[("a.txt", "same")],
            &[("a.txt", "same")],
        );
        // The same tree the library holds, at a new path — `SKILL.md` included,
        // since the content hash covers the whole directory.
        let installed = fs::read_to_string(checkout.path().join("skill/SKILL.md")).unwrap();
        rewrite_checkout(
            &checkout,
            &[
                ("moved-here/SKILL.md", installed.as_str()),
                ("moved-here/a.txt", "same"),
            ],
        );

        let outcome = apply_prepared_update(&repo.store, &record, &prepared, None, false).unwrap();

        match outcome {
            UpdateOutcome::Moved { candidates } => assert_eq!(
                candidates,
                vec![MoveCandidate {
                    subpath: "moved-here".to_string(),
                    confidence: Confidence::High,
                    reason: source_membership::MoveReason::ContentHash,
                }]
            ),
            other => panic!("expected a high-confidence move, got {other:?}"),
        }
        let after = repo.store.get_skill_by_id(&record.id).unwrap().unwrap();
        assert_eq!(after.source_subpath.as_deref(), Some("skill"));
        assert_eq!(after.update_status, "moved");
    }

    /// Re-pointing is a pointer move and nothing else: the library copy — even
    /// a file the user put there after the move was found — stays byte for
    /// byte, and the row returns to `update_available` so the next check can
    /// judge the new path.
    #[test]
    fn repointing_moves_only_the_source_pointer() {
        let repo = test_repo();
        let (record, prepared, checkout) = prepared_fixture(
            &repo,
            &[("a.txt", "same")],
            &[("a.txt", "same")],
        );
        // Identical bytes at a new path, `SKILL.md` included: the high-confidence
        // answer, and the one the UI offers as a single click.
        let installed = fs::read_to_string(checkout.path().join("skill/SKILL.md")).unwrap();
        rewrite_checkout(
            &checkout,
            &[
                ("moved-here/SKILL.md", installed.as_str()),
                ("moved-here/a.txt", "same"),
            ],
        );
        let before = repo.store.get_skill_by_id(&record.id).unwrap().unwrap();
        let outcome = apply_prepared_update(&repo.store, &record, &prepared, None, false).unwrap();
        assert!(matches!(outcome, UpdateOutcome::Moved { .. }));

        // The candidate must still be there when the UI comes back later; the
        // checkout is gone by then.
        let stored_candidates = repo.store.get_skill_move_candidates(&record.id).unwrap();
        assert_eq!(stored_candidates.len(), 1);
        assert_eq!(stored_candidates[0].subpath, "moved-here");
        assert_eq!(stored_candidates[0].confidence, Confidence::High);

        // Work the user did while the row sat at `moved`. Re-pointing must not
        // consult the library at all, let alone replace it.
        let library = Path::new(&record.central_path);
        fs::write(library.join("mine.txt"), "user work").unwrap();

        let dto = repoint_skill_source_internal(&repo.store, &record.id, "moved-here").unwrap();

        assert_eq!(dto.source_subpath.as_deref(), Some("moved-here"));
        assert_eq!(dto.update_status, "update_available");
        assert_eq!(dto.last_check_error, None);
        assert_eq!(dto.central_path, before.central_path);
        assert_eq!(dto.source_ref_resolved, before.source_ref_resolved);
        assert_eq!(dto.source_branch, before.source_branch);
        assert_eq!(dto.source_revision, before.source_revision);

        let after = repo.store.get_skill_by_id(&record.id).unwrap().unwrap();
        assert_eq!(after.source_subpath.as_deref(), Some("moved-here"));
        assert_eq!(after.central_path, before.central_path);
        assert_eq!(after.content_hash, before.content_hash);
        assert_eq!(after.source_ref_resolved, before.source_ref_resolved);
        assert_eq!(after.source_revision, before.source_revision);
        assert_eq!(
            fs::read_to_string(library.join("mine.txt")).unwrap(),
            "user work"
        );
        assert_eq!(fs::read_to_string(library.join("a.txt")).unwrap(), "same");
        assert!(
            repo.store
                .get_skill_move_candidates(&record.id)
                .unwrap()
                .is_empty(),
            "the move answer is spent once the pointer follows it"
        );

        // Startup reindexes the database from the metadata mirror. The pointer
        // has to be there too, or the next launch quietly undoes the click.
        crate::core::sync_metadata::reindex_from_metadata_unlocked(&repo.store).unwrap();
        let reindexed = repo.store.get_skill_by_id(&record.id).unwrap().unwrap();
        assert_eq!(reindexed.source_subpath.as_deref(), Some("moved-here"));
    }

    /// A skills.sh reference names its skill by id, so the record carries a
    /// locator independent of its `source_subpath`. It is a name guess: the
    /// locator may well find a directory, but that directory is exactly what
    /// `locate` already reports as a candidate. Using it here would write the
    /// guessed path into `source_subpath` without the user's decision.
    fn skills_sh_fixture(
        repo: &TestRepo,
        name: &str,
        locator: &str,
        checkout_files: &[(&str, &str)],
    ) -> (SkillRecord, PreparedUpdate, TempDir) {
        const SKILL_MD: (&str, &str) = (
            "SKILL.md",
            "---\nname: skill-under-test\ndescription: fixture\n---\n\nbody\n",
        );
        let id = "skill-under-test";
        let central_path = central_repo::skills_dir().join(id);
        write_tree(&central_path, &[SKILL_MD, ("a.txt", "keep")]);
        let hash = crate::core::content_hash::hash_directory(&central_path).unwrap();

        let record = SkillRecord {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            source_type: "skillssh".to_string(),
            source_ref: Some(format!("acme/skills/{locator}")),
            source_ref_resolved: Some("https://github.com/acme/skills.git".to_string()),
            source_subpath: Some("skills/gone".to_string()),
            source_branch: None,
            source_revision: Some("rev1".to_string()),
            remote_revision: Some("rev1".to_string()),
            central_path: central_path.to_string_lossy().to_string(),
            content_hash: Some(hash),
            enabled: true,
            created_at: 0,
            updated_at: 0,
            status: "ok".to_string(),
            update_status: "update_available".to_string(),
            last_checked_at: None,
            last_check_error: None,
        };
        repo.store.upsert_skill(&record).unwrap();

        // The recorded path is gone, and the skill is somewhere else now.
        let checkout = tempdir().unwrap();
        write_tree(checkout.path(), checkout_files);

        let prepared = PreparedUpdate {
            source: RemoteRef::from_skill(&record).unwrap(),
            revision: "rev2".to_string(),
            checkout: checkout.path().to_path_buf(),
        };
        (record, prepared, checkout)
    }

    /// Medium 4: when `locate` says the skill moved, the skills.sh locator must
    /// not answer the question instead. Its match is the same directory, so the
    /// only difference is that the locator would apply the update and rewrite
    /// `source_subpath` — a re-point nobody asked for.
    #[test]
    fn a_move_is_not_overridden_by_the_skills_sh_locator() {
        let repo = test_repo();
        let (record, prepared, _checkout) = skills_sh_fixture(
            &repo,
            "captions-overlay",
            "captions-overlay",
            &[(
                ".agents/skills/captions-overlay/SKILL.md",
                "---
name: captions-overlay
---

body
",
            )],
        );
        // The locator really would have found it: the directory is named after
        // the id, which is what `find_skill_dir` matches on.
        assert_eq!(prepared.source.locator.as_deref(), Some("captions-overlay"));

        let outcome = apply_prepared_update(&repo.store, &record, &prepared, None, false).unwrap();

        match outcome {
            UpdateOutcome::Moved { candidates } => assert_eq!(
                candidates[0].subpath, ".agents/skills/captions-overlay",
                "the candidate `locate` found is the answer"
            ),
            other => panic!("a move must not become an update: {other:?}"),
        }
        let after = repo.store.get_skill_by_id(&record.id).unwrap().unwrap();
        assert_eq!(
            after.source_subpath.as_deref(),
            Some("skills/gone"),
            "only the user's re-point may write the path"
        );
    }

    /// Same rule for the other answer: a locator that happens to name a
    /// directory does not turn "gone" into a silent update from a guessed path.
    #[test]
    fn a_gone_skill_is_not_rescued_by_the_skills_sh_locator() {
        let repo = test_repo();
        let (record, prepared, _checkout) = skills_sh_fixture(
            &repo,
            "display-name",
            "captions-overlay",
            &[(
                ".agents/skills/captions-overlay/SKILL.md",
                "---
name: embedded-captions
---

body
",
            )],
        );

        let outcome = apply_prepared_update(&repo.store, &record, &prepared, None, false).unwrap();

        assert!(matches!(outcome, UpdateOutcome::Gone), "got {other:?}", other = "an update");
        let after = repo.store.get_skill_by_id(&record.id).unwrap().unwrap();
        assert_eq!(after.update_status, "source_gone");
        assert_eq!(after.source_subpath.as_deref(), Some("skills/gone"));
    }

    /// The Source really did delete it. Counting this as a failure is what made
    /// the badge impossible to clear; the round has an answer of its own.
    #[test]
    fn a_skill_the_source_no_longer_holds_anywhere_is_gone() {
        let repo = test_repo();
        let (record, prepared, checkout) = prepared_fixture(
            &repo,
            &[("a.txt", "keep")],
            &[("a.txt", "keep")],
        );
        // An unrelated skill took the repository's only slot.
        rewrite_checkout(
            &checkout,
            &[("skills/other/SKILL.md", "---\nname: other\n---\n\nbody\n")],
        );

        let outcome = apply_prepared_update(&repo.store, &record, &prepared, None, false).unwrap();

        assert!(matches!(outcome, UpdateOutcome::Gone), "got {outcome:?}");
        // The library keeps what it has; only the check state records the answer.
        let central = std::path::Path::new(&record.central_path);
        assert_eq!(fs::read_to_string(central.join("a.txt")).unwrap(), "keep");
        let after = repo.store.get_skill_by_id(&record.id).unwrap().unwrap();
        assert_eq!(after.source_subpath.as_deref(), Some("skill"));
        assert_eq!(after.update_status, "source_gone");
        assert_eq!(after.last_check_error.as_deref(), Some(SOURCE_GONE_MESSAGE));
    }

    /// High 2: the other way out of `source_gone`. The user keeps the copy on
    /// disk — including whatever they edited while it was gone — and the row
    /// stops claiming there is a Source to update from.
    #[test]
    fn detaching_a_gone_skills_source_keeps_every_file() {
        let repo = test_repo();
        let (record, _prepared, _checkout) = prepared_fixture(
            &repo,
            &[("a.txt", "old")],
            &[("a.txt", "old")],
        );
        let library = Path::new(&record.central_path);
        fs::write(library.join("mine.txt"), "user work").unwrap();
        repo.store
            .update_skill_check_state(&record.id, Some("rev1"), "source_gone", Some(SOURCE_GONE_MESSAGE))
            .unwrap();
        repo.store
            .replace_skill_move_candidates(
                &record.id,
                &[MoveCandidate {
                    subpath: "somewhere".to_string(),
                    confidence: Confidence::Low,
                    reason: source_membership::MoveReason::DirectoryName,
                }],
            )
            .unwrap();

        let dto = detach_skill_source_internal(&repo.store, &record.id).unwrap();

        assert_eq!(dto.source_type, "local");
        assert_eq!(dto.source_ref.as_deref(), Some(record.central_path.as_str()));
        assert_eq!(dto.source_ref_resolved, None);
        assert_eq!(dto.source_subpath, None);
        assert_eq!(dto.source_branch, None);
        assert_eq!(dto.source_revision, None);
        assert_eq!(dto.remote_revision, None);
        assert_eq!(dto.update_status, "up_to_date");
        assert_eq!(dto.last_check_error, None);
        assert!(dto.move_candidates.is_empty(), "the move answer belonged to the old source");

        // Nothing on disk was touched: detaching is a pointer change.
        assert_eq!(fs::read_to_string(library.join("a.txt")).unwrap(), "old");
        assert_eq!(
            fs::read_to_string(library.join("mine.txt")).unwrap(),
            "user work"
        );
        let after = repo.store.get_skill_by_id(&record.id).unwrap().unwrap();
        assert_eq!(after.content_hash, record.content_hash);
        assert_eq!(after.central_path, record.central_path);

        // The mirror is what a restart reindexes from; a stale git pointer
        // there would undo the change on the next launch.
        crate::core::sync_metadata::reindex_from_metadata_unlocked(&repo.store).unwrap();
        let reindexed = repo.store.get_skill_by_id(&record.id).unwrap().unwrap();
        assert_eq!(reindexed.source_type, "local");
        assert_eq!(
            reindexed.source_ref.as_deref(),
            Some(record.central_path.as_str())
        );
    }

    /// Detaching a source that is already local has nothing to remove, and
    /// saying so is clearer than silently rewriting the row.
    #[test]
    fn detaching_is_rejected_for_skills_without_a_git_source() {
        let repo = test_repo();
        let dir = write_skill_dir("skill-1");
        repo.store
            .insert_skill(&sample_skill("skill-1", "skill-1", &dir))
            .unwrap();

        let err = detach_skill_source_internal(&repo.store, "skill-1").unwrap_err();

        assert!(err.message.contains("git-based"), "got: {}", err.message);
        let after = repo.store.get_skill_by_id("skill-1").unwrap().unwrap();
        assert_eq!(after.source_type, "import");
    }

    /// The two answers are settled, not transient: no check — forced or not —
    /// may re-diff them against the old path, because the only way out is the
    /// user re-pointing the source or detaching it. The TTL is not the reason;
    /// the answer is.
    #[test]
    fn moved_and_source_gone_are_never_rechecked() {
        let repo = test_repo();
        let dir = write_skill_dir("skill-1");
        repo.store
            .insert_skill(&sample_skill("skill-1", "skill-1", &dir))
            .unwrap();

        for status in ["moved", "source_gone"] {
            repo.store
                .update_skill_check_state("skill-1", Some("rev"), status, None)
                .unwrap();
            let stored = repo.store.get_skill_by_id("skill-1").unwrap().unwrap();
            assert!(
                should_skip_update_check(&repo.store, &stored, false).unwrap(),
                "{status} must be skipped by a TTL round"
            );
            assert!(
                should_skip_update_check(&repo.store, &stored, true).unwrap(),
                "even an explicit check must leave {status} alone"
            );
        }
    }

    /// High 1: a check used to overwrite a settled answer — `moved` came back
    /// as `update_available` and its candidates were deleted with it, so the
    /// one button that fixes the row vanished on the next round. Status and
    /// candidates both survive a check now.
    #[test]
    fn a_check_leaves_moved_and_source_gone_rows_alone() {
        let repo = test_repo();
        insert_git_skill(&repo.store, "skill-1", "https://example.test/a.git");
        let candidates = vec![MoveCandidate {
            subpath: "moved-here".to_string(),
            confidence: Confidence::Low,
            reason: source_membership::MoveReason::DirectoryName,
        }];

        // `force` is the case that used to run: it bypassed every skip, so it
        // is the one that used to erase the answer.
        for (status, force) in [
            ("moved", false),
            ("moved", true),
            ("source_gone", false),
            ("source_gone", true),
        ] {
            repo.store
                .update_skill_check_state("skill-1", Some("old-rev"), status, None)
                .unwrap();
            if status == "moved" {
                repo.store
                    .replace_skill_move_candidates("skill-1", &candidates)
                    .unwrap();
            }

            check_skill_update_internal_with_remote(
                &repo.store,
                "skill-1",
                force,
                prefetch("https://example.test/a.git", "new-rev"),
            )
            .unwrap();

            let stored = repo.store.get_skill_by_id("skill-1").unwrap().unwrap();
            assert_eq!(
                stored.update_status, status,
                "{status} (force={force}) must survive a check"
            );
            assert_eq!(
                repo.store.get_skill_move_candidates("skill-1").unwrap(),
                if status == "moved" {
                    candidates.clone()
                } else {
                    Vec::new()
                },
                "{status} (force={force}) must keep exactly its own candidates"
            );
        }
    }

    /// Medium 3: one skill at a time reaches the same audit vocabulary as a
    /// batch. A move and a deletion are answers — a `Failed` entry would show
    /// the skill as "check failed" in the history and invite a retry that
    /// cannot work.
    #[test]
    fn a_single_skills_move_and_deletion_are_audited_as_answers() {
        let repo = test_repo();
        let dir = write_skill_dir("skill-1");
        repo.store
            .insert_skill(&sample_skill("skill-1", "skill-1", &dir))
            .unwrap();
        let candidates = vec![MoveCandidate {
            subpath: "moved-here".to_string(),
            confidence: Confidence::High,
            reason: source_membership::MoveReason::ContentHash,
        }];

        log_update_outcome(
            &repo.store,
            "skill-1",
            "git",
            Ok(&UpdateSkillResult {
                skill: managed_skill_by_id(&repo.store, "skill-1").unwrap(),
                content_changed: false,
                pending_removals: Vec::new(),
                removal_approval: None,
                moved: candidates,
                gone: false,
            }),
        );
        log_update_outcome(
            &repo.store,
            "skill-1",
            "git",
            Ok(&UpdateSkillResult {
                skill: managed_skill_by_id(&repo.store, "skill-1").unwrap(),
                content_changed: false,
                pending_removals: Vec::new(),
                removal_approval: None,
                moved: Vec::new(),
                gone: true,
            }),
        );

        let entries = repo.store.list_audit(None).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(
            entries.iter().all(|entry| entry.success),
            "neither answer is a failure"
        );
        assert!(
            entries[0].detail.as_deref().unwrap_or("").contains("gone upstream"),
            "got: {:?}",
            entries[0].detail
        );
        assert!(
            entries[1]
                .detail
                .as_deref()
                .unwrap_or("")
                .contains("moved upstream — first candidate 'moved-here'"),
            "got: {:?}",
            entries[1].detail
        );
    }

    // ── RemoteIdentity dedup (batch check_all fan-out) ──

    fn source(clone_url: &str, branch: Option<&str>, subpath: Option<&str>) -> RemoteRef {
        RemoteRef {
            original_url: clone_url.to_string(),
            clone_url: clone_url.to_string(),
            branch: branch.map(str::to_string),
            subpath: subpath.map(str::to_string),
            locator: None,
        }
    }

    /// The whole point of keying Phase A by `RemoteIdentity`: skills installed from
    /// different subdirectories of the same monorepo (same clone_url + branch)
    /// must collapse to one network query, while a different branch stays
    /// distinct. This is what turns 4 `mattpocock/skills` skills into 1
    /// `ls-remote` instead of 4.
    #[test]
    fn remote_key_dedups_by_url_and_branch_ignoring_subpath() {
        let mut per_remote: HashMap<RemoteIdentity, usize> = HashMap::new();
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
            *per_remote.entry(s.identity()).or_insert(0) += 1;
        }

        assert_eq!(per_remote.len(), 3, "distinct remotes to query");
        assert_eq!(
            per_remote[&RemoteIdentity {
                clone_url: "https://github.com/mattpocock/skills.git".to_string(),
                branch: None,
            }],
            3,
            "three subpaths of one repo/branch share a single query"
        );
        assert_eq!(
            per_remote[&RemoteIdentity {
                clone_url: "https://github.com/mattpocock/skills.git".to_string(),
                branch: Some("next".to_string()),
            }],
            1,
            "a different branch is a separate remote"
        );
    }

    fn remote(url: &str, branch: Option<&str>) -> RemoteIdentity {
        RemoteIdentity {
            clone_url: url.to_string(),
            branch: branch.map(|b| b.to_string()),
        }
    }

    /// Work-stealing must cover every remote exactly once and collect each
    /// resolver result under its own key — this exercises the real concurrent
    /// loop, not just `RemoteIdentity`'s hashing.
    #[test]
    fn resolve_concurrent_resolves_every_remote_exactly_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let remotes: Vec<RemoteIdentity> = (0..20)
            .map(|i| remote(&format!("https://example.test/r{i}"), None))
            .collect();
        let calls = AtomicUsize::new(0);

        let out = resolve_concurrent(remotes.clone(), |key| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok::<_, String>(format!("rev:{}", key.clone_url))
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
        let remotes: Vec<RemoteIdentity> = (0..8).map(|i| remote(&format!("r{i}"), None)).collect();
        let in_flight = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);

        let out = resolve_concurrent(remotes, |_key| {
            let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(20));
            in_flight.fetch_sub(1, Ordering::SeqCst);
            Ok::<_, String>("rev".to_string())
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
            unchanged: HashSet::new(),
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
                unchanged: HashSet::new(),
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

    // ── Subpath skills of one monorepo (#561) ──

    fn git_in(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.email=t@t", "-c", "user.name=t"])
            .args(args)
            .output()
            .expect("git must be runnable");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// `file://` wants forward slashes and, on Windows, a slash before the drive.
    fn file_url(path: &Path) -> String {
        let raw = path.display().to_string().replace('\\', "/");
        if raw.starts_with('/') {
            format!("file://{raw}")
        } else {
            format!("file:///{raw}")
        }
    }

    /// A repository holding a root skill and two sibling skills, `skills/a` and
    /// `skills/b`, served over `file://`. Returns its path, URL and first commit.
    fn monorepo(repo: &TestRepo) -> (PathBuf, String, String) {
        let dir = repo._tmp.path().join("monorepo");
        write_skill(&dir, "root");
        write_skill(&dir.join("skills/a"), "a");
        write_skill(&dir.join("skills/b"), "b");
        git_in(&dir, &["init", "-q"]);
        git_in(&dir, &["add", "-A"]);
        git_in(&dir, &["commit", "-qm", "init"]);
        // The blobless fetch is served by this repo, so the switch belongs here.
        git_in(&dir, &["config", "uploadpack.allowFilter", "true"]);
        let first = git_in(&dir, &["rev-parse", "HEAD"]);
        let url = file_url(&dir);
        (dir, url, first)
    }

    fn commit_file(dir: &Path, rel: &str, body: &str) -> String {
        fs::write(dir.join(rel), body).unwrap();
        git_in(dir, &["add", rel]);
        git_in(dir, &["commit", "-qm", rel]);
        git_in(dir, &["rev-parse", "HEAD"])
    }

    fn insert_monorepo_skill(
        store: &SkillStore,
        id: &str,
        url: &str,
        subpath: Option<&str>,
        revision: &str,
    ) {
        insert_git_skill(store, id, url);
        store
            .update_skill_source_metadata(id, Some(url), subpath, None, Some(revision))
            .unwrap();
    }

    /// The reported bug, end to end through the real check: a commit that only
    /// touches `skills/a` is an update for `a` and nothing else in that repo —
    /// except a skill installed from the repository root, which still sees
    /// every commit, exactly as before.
    #[test]
    fn a_monorepo_commit_flags_only_the_skill_it_touches() {
        let repo = test_repo();
        let (dir, url, first) = monorepo(&repo);
        insert_monorepo_skill(&repo.store, "a", &url, Some("skills/a"), &first);
        insert_monorepo_skill(&repo.store, "b", &url, Some("skills/b"), &first);
        insert_monorepo_skill(&repo.store, "root", &url, None, &first);

        let head = commit_file(&dir, "skills/a/SKILL.md", "---\nname: a\n---\nchanged\n");

        let check = |id: &str| check_skill_update_internal(&repo.store, id, true, None).unwrap();
        assert_eq!(check("a").update_status, "update_available");
        assert_eq!(check("b").update_status, "up_to_date");
        assert_eq!(check("root").update_status, "update_available");

        // `b`'s directory is the same tree at the new head, so that head is now
        // what it was installed from: the next check needs no fetch.
        let b = repo.store.get_skill_by_id("b").unwrap().unwrap();
        assert_eq!(b.source_revision.as_deref(), Some(head.as_str()));
        assert_eq!(b.remote_revision.as_deref(), Some(head.as_str()));
        let a = repo.store.get_skill_by_id("a").unwrap().unwrap();
        assert_eq!(a.source_revision.as_deref(), Some(first.as_str()));

        // A `.gitattributes` above the directory changes the bytes an update
        // writes into it — here every line ending — while its tree stays put.
        commit_file(&dir, ".gitattributes", "skills/b/** text eol=crlf\n");
        assert_eq!(check("b").update_status, "update_available");
        let b = repo.store.get_skill_by_id("b").unwrap().unwrap();
        assert_eq!(b.source_revision.as_deref(), Some(head.as_str()));
    }

    /// An installed revision the server no longer has (a force-push, say) cannot
    /// be compared, so the check reports an update exactly as it always did.
    #[test]
    fn an_unfetchable_installed_revision_still_reports_an_update() {
        let repo = test_repo();
        let (dir, url, _) = monorepo(&repo);
        let gone = "0123456789abcdef0123456789abcdef01234567";
        insert_monorepo_skill(&repo.store, "b", &url, Some("skills/b"), gone);
        commit_file(&dir, "skills/a/SKILL.md", "---\nname: a\n---\nchanged\n");

        let dto = check_skill_update_internal(&repo.store, "b", true, None).unwrap();

        assert_eq!(dto.update_status, "update_available");
        let b = repo.store.get_skill_by_id("b").unwrap().unwrap();
        assert_eq!(b.source_revision.as_deref(), Some(gone));
    }

    /// "Check all" resolves each remote once. Its siblings — here installed at
    /// different revisions — are answered from that one resolution.
    #[test]
    fn one_resolution_answers_every_sibling_of_a_remote() {
        let repo = test_repo();
        let (dir, url, first) = monorepo(&repo);
        let second = commit_file(&dir, "SKILL.md", "---\nname: root\n---\nroot only\n");
        commit_file(&dir, "skills/a/SKILL.md", "---\nname: a\n---\nchanged\n");

        let key = remote(&url, None);
        let installed: HashSet<(String, String)> = [
            (first.clone(), "skills/a".to_string()),
            (second.clone(), "skills/b".to_string()),
        ]
        .into_iter()
        .collect();
        let out = resolve_remotes_concurrent(HashMap::from([(key.clone(), installed)]), None);

        assert_eq!(
            out[&key].unchanged,
            HashSet::from([(second, "skills/b".to_string())])
        );
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

    fn site_skill_source(body: &str) -> TempDir {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("SKILL.md"), body).unwrap();
        dir
    }

    /// Reinstalling a website skill, however its URL is spelled, updates the
    /// record and directory it already has. A same-named skill from another
    /// site gets its own, even with identical content.
    #[test]
    fn a_site_skill_reinstall_keeps_its_record_and_another_site_gets_its_own() {
        let repo = test_repo();
        let feishu = "https://skills.sh/site/open.feishu.cn/lark-im";
        let v1 = site_skill_source("---\nname: lark-im\n---\nv1\n");
        let (id, installed) =
            install_site_skill_dir(&repo.store, feishu, v1.path(), None, "r", None, None).unwrap();

        let v2 = site_skill_source("---\nname: lark-im\n---\nv2\n");
        let respelled = "https://www.skills.sh/site/open.feishu.cn/lark-im";
        let (again, _) =
            install_site_skill_dir(&repo.store, respelled, v2.path(), None, "r", None, None).unwrap();
        assert_eq!(again, id);
        assert!(fs::read_to_string(installed.central_path.join("SKILL.md")).unwrap().ends_with("v2\n"));
        assert!(!central_repo::skills_dir().join("lark-im-2").exists());

        let larksuite = "https://skills.sh/site/open.larksuite.com/lark-im";
        let (other, other_result) =
            install_site_skill_dir(&repo.store, larksuite, v2.path(), None, "r", None, None).unwrap();
        assert_ne!(other, id);
        assert_eq!(other_result.name, "lark-im-2");
        let original = repo.store.get_skill_by_id(&id).unwrap().unwrap();
        assert_eq!(original.source_ref.as_deref(), Some(feishu));
    }

    /// A folder already in the library that no record owns is not ours to
    /// replace: a new website skill with the same name goes next to it.
    #[test]
    fn a_new_site_skill_never_replaces_an_unrecorded_folder() {
        let repo = test_repo();
        let stray = central_repo::skills_dir().join("lark-doc");
        fs::create_dir_all(&stray).unwrap();
        fs::write(stray.join("notes.md"), "mine").unwrap();

        let source = site_skill_source("---\nname: lark-doc\n---\n");
        let (_, installed) = install_site_skill_dir(
            &repo.store,
            "https://skills.sh/site/open.feishu.cn/lark-doc",
            source.path(),
            None,
            "r",
            None,
            None,
        )
        .unwrap();
        assert_eq!(installed.name, "lark-doc-2");
        assert_eq!(fs::read_to_string(stray.join("notes.md")).unwrap(), "mine");
    }
}
