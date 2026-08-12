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
}

#[derive(Debug, Serialize)]
pub struct BatchUpdateSkillsResult {
    pub refreshed: usize,
    pub unchanged: usize,
    pub failed: Vec<String>,
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

        let temp_dir = git_fetcher::clone_repo_ref(
            &git_source.clone_url,
            git_source.branch.as_deref(),
            None,
            proxy_url.as_deref(),
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

        let temp_dir = git_fetcher::clone_repo_ref(
            &git_source.clone_url,
            git_source.branch.as_deref(),
            None,
            proxy_url.as_deref(),
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
                let target_path = PathBuf::from(&target.target_path);
                sync_engine::remove_target(&target_path).ok();
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
    outcome: Result<&ManagedSkillDto, &AppError>,
) {
    let mut draft = AuditDraft::new("update").detail("local");
    match outcome {
        Ok(dto) => {
            draft = draft.skill(dto.id.clone(), dto.name.clone()).ok();
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
            let temp_dir = git_fetcher::clone_repo_ref_with_progress(
                &parsed.clone_url,
                parsed.branch.as_deref(),
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
        let temp_dir = git_fetcher::clone_repo_ref_with_progress(
            &parsed.clone_url,
            parsed.branch.as_deref(),
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

#[tauri::command]
pub async fn update_skill(
    skill_id: String,
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
            update_git_skill_internal(&store, &skill_id, proxy_url.as_deref(), Some(&cancel));
        log_update_outcome(&store, &skill_id, "git", outcome.as_ref());
        outcome
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
        let outcome = reimport_local_skill_internal(&store, &skill_id);
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
                        update_git_skill_internal(&store, &skill_id, proxy_url.as_deref(), None);
                    log_update_outcome(&store, &skill_id, "git", outcome.as_ref());
                    match outcome {
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
                    let outcome = reimport_local_skill_internal(&store, &skill_id);
                    log_reimport_outcome(&store, &skill_id, outcome.as_ref());
                    match outcome {
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
        })
    })
    .await?
}

#[tauri::command]
pub async fn relink_local_skill_source(
    skill_id: String,
    source_path: String,
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

        let result = (|| -> Result<(), AppError> {
            let _lock = RepoLock::acquire_foreground("relink local skill").map_err(AppError::db)?;
            let staged_path = staged_path_for(&skill.central_path);
            let install_result = installer::install_from_local_to_destination(
                &path,
                Some(&skill.name),
                &staged_path,
            )
            .map_err(AppError::io)?;
            swap_skill_directory(&staged_path, Path::new(&skill.central_path))?;
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

pub fn update_git_skill_internal(
    store: &SkillStore,
    skill_id: &str,
    proxy_url: Option<&str>,
    cancel: Option<&Arc<AtomicBool>>,
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

    let temp_dir = git_fetcher::clone_repo_ref(
        &git_source.clone_url,
        git_source.branch.as_deref(),
        cancel,
        proxy_url,
    )
    .map_err(AppError::classify_git_error)?;
    let update_result = (|| -> Result<bool, AppError> {
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

        if content_changed {
            let staged_path = staged_path_for(&skill.central_path);
            let install_result =
                installer::install_skill_dir_to_destination(&skill_dir, &skill.name, &staged_path)
                    .map_err(AppError::io)?;
            swap_skill_directory(&staged_path, Path::new(&skill.central_path))?;

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
        Ok(content_changed)
    })();
    git_fetcher::cleanup_temp(&temp_dir);

    match update_result {
        Ok(content_changed) => {
            let skill = managed_skill_by_id(store, skill_id)?;
            Ok(UpdateSkillResult {
                skill,
                content_changed,
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

    let temp_dir =
        git_fetcher::clone_repo_ref(&parsed.clone_url, branch.as_deref(), None, proxy_url)
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

pub fn reimport_local_skill_internal(
    store: &SkillStore,
    skill_id: &str,
) -> Result<ManagedSkillDto, AppError> {
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

    let result = (|| -> Result<(), AppError> {
        let _lock = RepoLock::acquire_foreground("reimport local skill").map_err(AppError::db)?;
        let staged_path = staged_path_for(&skill.central_path);
        let install_result =
            installer::install_from_local_to_destination(&path, Some(&skill.name), &staged_path)
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
        resync_copy_targets(store, &skill.id)?;
        sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;
        Ok(())
    })();

    match result {
        Ok(()) => managed_skill_by_id(store, skill_id),
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
                            Ok(live_hash) => match skill.content_hash.as_deref() {
                                Some(stored) if stored == live_hash.as_str() => {
                                    ("up_to_date", None)
                                }
                                Some(_) => ("update_available", None),
                                None => ("local_only", None),
                            },
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
        repo.store
            .insert_target(&SkillTargetRecord {
                id: "target-1".to_string(),
                skill_id: "skill-1".to_string(),
                tool: "cursor".to_string(),
                target_path: target_dir.to_string_lossy().to_string(),
                mode: "symlink".to_string(),
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
}
