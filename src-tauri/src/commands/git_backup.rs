use crate::core::{
    central_repo, error::AppError, git2_engine, git_backup, git_credentials, git_fetcher,
    github_api, merge, skill_metadata, sync_metadata,
};
use anyhow::Context;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::State;
use walkdir::WalkDir;

use crate::core::skill_store::SkillStore;

static FETCH_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// Classify a git failure keeping the full anyhow cause chain: the top-level
/// context alone ("object merge aborted…") hides the actual reason (e.g.
/// which validation rule failed), which is what the failure card must show.
fn classify_git_chain(e: anyhow::Error) -> AppError {
    AppError::classify_git_error(format!("{e:#}"))
}

/// Push the persisted engine choice (`git_backup_engine` = "git2" | "system")
/// and proxy setting into the core layer, which has no store access. Called
/// at the entry of every command that can touch the network.
pub(crate) fn sync_engine_pref(store: &SkillStore) {
    let git2_enabled = store
        .get_setting("git_backup_engine")
        .ok()
        .flatten()
        .map(|v| v.trim() == "git2")
        .unwrap_or(false);
    git2_engine::set_preference(git2_enabled, store.proxy_url());
}

/// Resolve the device name (§4.3 设备命名): the persisted setting, or a
/// hostname-derived default that is persisted on first use so it stays stable
/// across sessions.
fn effective_device_name(store: &SkillStore) -> String {
    let saved = store
        .get_setting("backup_device_name")
        .ok()
        .flatten()
        .map(|v| git_backup::sanitize_device_name(&v))
        .filter(|v| !v.is_empty());
    if let Some(name) = saved {
        return name;
    }
    let name = git_backup::default_device_name();
    if let Err(e) = store.set_setting("backup_device_name", &name) {
        log::warn!("device name: failed to persist default: {e:#}");
    }
    name
}

/// Best-effort: bring the repo's commit identity in line with the device name
/// before an operation that can create commits. Identity trouble must never
/// block a backup — commits then just carry the previous (or global) author.
pub(crate) fn apply_device_identity(store: &SkillStore, skills_dir: &Path) {
    let name = effective_device_name(store);
    if let Err(e) = git_backup::configure_device_identity(skills_dir, &name) {
        log::warn!("device name: failed to configure git identity: {e:#}");
    }
}

/// RAII guard that clears `FETCH_IN_FLIGHT` on drop. Survives future
/// cancellation, panic in the blocking task, and early returns — without
/// it, a dropped command future would strand the flag set forever.
struct FetchInFlightGuard;

impl FetchInFlightGuard {
    fn try_acquire() -> Option<Self> {
        FETCH_IN_FLIGHT
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| FetchInFlightGuard)
    }
}

impl Drop for FetchInFlightGuard {
    fn drop(&mut self) {
        FETCH_IN_FLIGHT.store(false, Ordering::Release);
    }
}

#[tauri::command]
pub async fn git_backup_fetch(store: State<'_, Arc<SkillStore>>) -> Result<(), AppError> {
    sync_engine_pref(&store);
    // Coalesce concurrent fetches: a `git fetch` against the central repo
    // already in flight makes any duplicate request redundant, and stacking
    // them up holds open ssh connections to GitHub.
    let Some(_guard) = FetchInFlightGuard::try_acquire() else {
        return Ok(());
    };
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::fetch_remote(&skills_dir).map_err(AppError::git)
    })
    .await?
}

#[tauri::command]
pub async fn git_backup_status(
    store: State<'_, Arc<SkillStore>>,
) -> Result<git_backup::GitBackupStatus, AppError> {
    let _ = store; // ensure DB is available
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || git_backup::get_status(&skills_dir).map_err(AppError::git))
        .await?
}

#[tauri::command]
pub async fn git_backup_init(store: State<'_, Arc<SkillStore>>) -> Result<(), AppError> {
    let store = store.inner().clone();
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::with_repo_lock("git init", || {
            sync_metadata::write_all_from_db_unlocked(&store)?;
            git_backup::init_repo_unlocked(&skills_dir, &effective_device_name(&store))
        })
        .map_err(AppError::git)
    })
    .await?
}

/// Move credentials embedded in `url` into the OS keychain and return the
/// sanitized URL. Falls back to the original URL when the keychain is
/// unavailable (e.g. Linux without a secret service) so backup keeps working
/// with the legacy embedded-credential behavior.
fn sanitize_url_to_keychain(url: &str) -> String {
    let Some((cred, sanitized)) = git_credentials::split_credentials_from_url(url) else {
        return url.to_string();
    };
    let Some(host) = git_credentials::https_host(&sanitized) else {
        return url.to_string();
    };
    match git_credentials::store_credential(&host, &cred) {
        Ok(()) => sanitized,
        Err(e) => {
            log::warn!(
                "git credentials: keychain unavailable, keeping embedded credentials: {e:#}"
            );
            url.to_string()
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GithubBackupConnectResult {
    /// Credential-free HTTPS URL of the backup repository.
    pub url: String,
    pub login: String,
    pub repo_created: bool,
    /// False when a pre-existing PUBLIC repository was connected — the UI
    /// warns; app-created repositories are always private.
    pub repo_private: bool,
    /// True when the remote already has commits — the frontend restores
    /// (clones) instead of initializing a fresh backup.
    pub remote_has_content: bool,
}

/// GitHub guided connect (backup redesign Phase 2, PAT mode): validate the
/// token, find or create the private backup repository, store the token in
/// the OS keychain, and save the credential-free URL. The keychain is
/// mandatory here — guided mode never falls back to token-in-URL.
#[tauri::command]
pub async fn github_backup_connect(
    store: State<'_, Arc<SkillStore>>,
    token: String,
    repo_name: String,
) -> Result<GithubBackupConnectResult, AppError> {
    let store = store.inner().clone();
    sync_engine_pref(&store);
    tokio::task::spawn_blocking(move || {
        connect_with_token(&store, token.trim(), repo_name.trim(), "pat")
    })
    .await?
}

/// Shared tail of both connect paths (PAT and Device Flow): validate the
/// token, find/create the repo, keychain the token, save the URL, and probe
/// whether the remote already has content. `method` ("oauth" | "pat") is
/// persisted so the disconnect matrix (§3.1) can point revocation at the
/// right GitHub page.
fn connect_with_token(
    store: &SkillStore,
    token: &str,
    repo_name: &str,
    method: &str,
) -> Result<GithubBackupConnectResult, AppError> {
    if token.is_empty() {
        return Err(AppError::invalid_input("Token is empty"));
    }
    if !github_api::is_valid_repo_name(repo_name) {
        return Err(AppError::invalid_input("Invalid repository name"));
    }

    let proxy_url = store.proxy_url();
    let info = github_api::connect_backup_repo(token, repo_name, proxy_url.as_deref())
        .map_err(AppError::network)?;

    git_credentials::store_credential(
        "github.com",
        &git_credentials::RemoteCredential {
            username: info.login.clone(),
            password: token.to_string(),
        },
    )
    .map_err(|e| AppError::internal(format!("KEYCHAIN_UNAVAILABLE: {e:#}")))?;

    store
        .set_setting("git_backup_remote_url", &info.url)
        .map_err(AppError::db)?;
    store
        .set_setting("github_auth_method", method)
        .map_err(AppError::db)?;

    let remote_has_content =
        git_backup::remote_has_heads(&info.url).map_err(classify_git_chain)?;

    Ok(GithubBackupConnectResult {
        url: info.url,
        login: info.login,
        repo_created: info.repo_created,
        repo_private: info.repo_private,
        remote_has_content,
    })
}

#[tauri::command]
pub async fn github_device_flow_start(
    store: State<'_, Arc<SkillStore>>,
) -> Result<github_api::DeviceFlowStart, AppError> {
    let store = store.inner().clone();
    tokio::task::spawn_blocking(move || {
        let proxy_url = store.proxy_url();
        github_api::device_flow_start(proxy_url.as_deref()).map_err(AppError::network)
    })
    .await?
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GithubDevicePollResult {
    /// "pending" | "slow_down" | "connected"
    pub status: String,
    pub result: Option<GithubBackupConnectResult>,
}

/// One device-flow poll. On authorization the OAuth token stays in the
/// backend: it goes straight through `connect_with_token` into the OS
/// keychain and is never returned to the webview.
#[tauri::command]
pub async fn github_device_flow_poll(
    store: State<'_, Arc<SkillStore>>,
    device_code: String,
    repo_name: String,
) -> Result<GithubDevicePollResult, AppError> {
    let store = store.inner().clone();
    sync_engine_pref(&store);
    tokio::task::spawn_blocking(move || {
        let proxy_url = store.proxy_url();
        match github_api::device_flow_poll(&device_code, proxy_url.as_deref())
            .map_err(AppError::network)?
        {
            github_api::DevicePollOutcome::Pending => Ok(GithubDevicePollResult {
                status: "pending".to_string(),
                result: None,
            }),
            github_api::DevicePollOutcome::SlowDown => Ok(GithubDevicePollResult {
                status: "slow_down".to_string(),
                result: None,
            }),
            github_api::DevicePollOutcome::Authorized { token } => {
                let result = connect_with_token(&store, &token, repo_name.trim(), "oauth")?;
                Ok(GithubDevicePollResult {
                    status: "connected".to_string(),
                    result: Some(result),
                })
            }
        }
    })
    .await?
}

/// Sanitize a remote URL before it is persisted anywhere: embedded
/// credentials go to the OS keychain, the returned URL is what the frontend
/// must save and display.
#[tauri::command]
pub async fn git_backup_sanitize_remote_url(url: String) -> Result<String, AppError> {
    git_fetcher::validate_git_url(&url).map_err(AppError::git)?;
    tokio::task::spawn_blocking(move || Ok(sanitize_url_to_keychain(url.trim())))
        .await?
}

#[tauri::command]
pub async fn git_backup_set_remote(
    store: State<'_, Arc<SkillStore>>,
    url: String,
) -> Result<String, AppError> {
    sync_engine_pref(&store);
    git_fetcher::validate_git_url(&url).map_err(AppError::git)?;
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        let effective = sanitize_url_to_keychain(url.trim());
        git_backup::set_remote(&skills_dir, &effective).map_err(classify_git_chain)?;
        Ok(effective)
    })
    .await?
}

/// Disconnect the local machine from the backup remote (#260, §3.1 断开本机):
/// remove the git origin, clear the saved remote URL setting, and delete the
/// machine's stored access credential. Remote repository data and the local
/// repo are kept; other devices are unaffected.
#[tauri::command]
pub async fn git_backup_remove_remote(store: State<'_, Arc<SkillStore>>) -> Result<(), AppError> {
    let store = store.inner().clone();
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || disconnect_local(&store, &skills_dir)).await?
}

fn disconnect_local(store: &SkillStore, skills_dir: &Path) -> Result<(), AppError> {
    // Collect credential hosts before the URLs are gone.
    let mut hosts = std::collections::HashSet::new();
    if let Some(url) = git_backup::raw_remote_url(skills_dir) {
        hosts.extend(git_credentials::https_host(&url));
    }
    if let Ok(Some(url)) = store.get_setting("git_backup_remote_url") {
        hosts.extend(git_credentials::https_host(&url));
    }

    git_backup::remove_remote(skills_dir).map_err(AppError::git)?;
    store
        .set_setting("git_backup_remote_url", "")
        .map_err(AppError::db)?;
    let _ = store.set_setting("github_auth_method", "");

    for host in hosts {
        if let Err(e) = git_credentials::delete_credential(&host) {
            log::warn!("git disconnect: failed to delete keychain credential: {e:#}");
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn git_backup_commit(
    store: State<'_, Arc<SkillStore>>,
    message: String,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::with_repo_lock("git commit", || {
            apply_device_identity(&store, &skills_dir);
            sync_metadata::write_all_from_db_unlocked(&store)?;
            git_backup::commit_all_unlocked(&skills_dir, &message)
        })
        .map_err(AppError::git)
    })
    .await?
}

#[tauri::command]
pub async fn git_backup_push(store: State<'_, Arc<SkillStore>>) -> Result<(), AppError> {
    sync_engine_pref(&store);
    let store = store.inner().clone();
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::push(&skills_dir).map_err(classify_git_chain)?;
        // A successful manual push also resolves any lingering auto-backup
        // failure — the persistent failure card must not outlive the problem.
        let _ = store.set_setting(crate::core::auto_backup::SETTING_LAST_ERROR, "");
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn git_backup_pull(
    store: State<'_, Arc<SkillStore>>,
) -> Result<merge::MergeSummary, AppError> {
    let store = store.inner().clone();
    sync_engine_pref(&store);
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::with_repo_lock("git pull", || {
            // Merge commits must carry this device's identity too.
            apply_device_identity(&store, &skills_dir);
            // Object merge by default since 3d-β; merge_engine=system is the
            // escape hatch back to the line-level git merge.
            let summary = merge::gated_pull_unlocked(&store, &skills_dir)?;
            reconcile_skills_index_unlocked(&store)?;
            store.log_audit(
                crate::core::audit_log::AuditDraft::new("sync_merge")
                    .detail(format!(
                        "engine={} updated={} kept_local={} conflicts={} pending={}",
                        summary.engine,
                        summary.updated.len(),
                        summary.kept_local.len(),
                        summary.new_conflicts.len(),
                        summary.pending_total
                    ))
                    .ok(),
            );
            Ok(summary)
        })
        .map_err(classify_git_chain)
    })
    .await?
}

/// Outcome of a full sync transaction for the frontend.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncOutcome {
    /// Local changes were committed as part of this sync.
    pub committed: bool,
    /// Merge result when a merge ran (None when nothing to pull).
    pub merge: Option<merge::MergeSummary>,
    pub pushed: bool,
    /// Snapshot tag on the pushed state (None when nothing was pushed).
    pub snapshot_tag: Option<String>,
}

/// Full backup sync as one transaction under a single repo lock (merge-engine
/// design §9 并发收敛): commit → fetch/merge → snapshot → push, retrying the
/// fetch/merge/push tail when another device pushes concurrently. Replaces
/// the frontend-orchestrated commit/pull/push sequence, whose lock gaps let a
/// benign race surface as a non-fast-forward "needs recovery" error.
#[tauri::command]
pub async fn git_backup_sync(
    store: State<'_, Arc<SkillStore>>,
    message: String,
) -> Result<SyncOutcome, AppError> {
    let store = store.inner().clone();
    sync_engine_pref(&store);
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::with_repo_lock("git sync", || run_sync_blocking(&store, &skills_dir, &message))
            .map_err(classify_git_chain)
    })
    .await?
}

const SYNC_PUSH_ATTEMPTS: usize = 3;

fn run_sync_blocking(
    store: &SkillStore,
    skills_dir: &Path,
    message: &str,
) -> anyhow::Result<SyncOutcome> {
    apply_device_identity(store, skills_dir);

    // Local changes first — they must be safe before any network step.
    sync_metadata::write_all_from_db_unlocked(store)?;
    // Rebuild the oversized exclusions BEFORE the dirty check: a previously
    // excluded skill that shrank below the limit re-enters the backup by
    // making .gitignore (and the skill itself) show up as changes.
    if let Err(e) =
        git_backup::apply_oversized_exclusions(skills_dir, git_backup::SKILL_SIZE_LIMIT_BYTES)
    {
        log::warn!("backup size: exclusion scan failed (continuing): {e:#}");
    }
    let mut committed = false;
    if git_backup::has_uncommitted_changes(skills_dir)? {
        git_backup::commit_all_unlocked(skills_dir, message)?;
        committed = true;
    }

    // Best-effort initial fetch: a missing remote branch (fresh remote) or a
    // network failure must not block the local commit; push surfaces real
    // connectivity errors below when there is something to push.
    let branch = git_backup::current_branch(skills_dir);
    if let Err(e) = git_backup::fetch_branch(skills_dir, &branch) {
        log::info!("git sync: initial fetch failed (continuing): {e:#}");
    }

    let mut merge_summary: Option<merge::MergeSummary> = None;
    let mut pushed = false;
    let mut snapshot_tag: Option<String> = None;
    for attempt in 0..SYNC_PUSH_ATTEMPTS {
        let status = git_backup::get_status(skills_dir)?;
        if status.behind > 0 {
            let summary = merge::gated_pull_unlocked(store, skills_dir)?;
            reconcile_skills_index_unlocked(store)?;
            merge_summary = Some(summary);
        }

        let status = git_backup::get_status(skills_dir)?;
        let needs_push =
            committed || status.ahead > 0 || status.upstream_health == "no_upstream";
        if !needs_push {
            break;
        }
        // Reuses an existing tag on HEAD, so retries don't mint duplicates.
        snapshot_tag = Some(git_backup::create_snapshot_tag_unlocked(skills_dir)?);
        match git_backup::push_unlocked(skills_dir) {
            Ok(()) => {
                pushed = true;
                break;
            }
            Err(e) => {
                let msg = format!("{e:#}");
                let rejected = msg.contains("non-fast-forward")
                    || msg.contains("fetch first")
                    || msg.contains("[rejected]")
                    || msg.contains("failed to push some refs");
                if !rejected || attempt + 1 == SYNC_PUSH_ATTEMPTS {
                    return Err(e);
                }
                log::info!("git sync: push rejected (attempt {}), refetching", attempt + 1);
                git_backup::fetch_branch(skills_dir, &branch)?;
            }
        }
    }

    if pushed {
        // A successful sync also clears a lingering auto-backup failure card.
        let _ = store.set_setting(crate::core::auto_backup::SETTING_LAST_ERROR, "");
    }
    store.log_audit(
        crate::core::audit_log::AuditDraft::new("sync")
            .detail(format!(
                "committed={} merged={} pushed={}",
                committed,
                merge_summary.is_some(),
                pushed
            ))
            .ok(),
    );
    Ok(SyncOutcome { committed, merge: merge_summary, pushed, snapshot_tag })
}

/// Pending "needs attention" conflicts (merge-engine design §4) for the
/// Backup page. Reads the rebuildable SQLite projection.
#[tauri::command]
pub async fn git_backup_pending_conflicts(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<crate::core::skill_store::PendingConflictRow>, AppError> {
    let store = store.inner().clone();
    tokio::task::spawn_blocking(move || store.list_pending_conflicts().map_err(AppError::db))
        .await?
}

/// Resolve one pending conflict (§4 解决动作): action is one of
/// "keep_local" | "use_remote" | "keep_both". Returns the safety snapshot
/// tag taken before the resolution.
#[tauri::command]
pub async fn git_backup_resolve_conflict(
    store: State<'_, Arc<SkillStore>>,
    skill_id: String,
    action: String,
) -> Result<String, AppError> {
    let Some(action) = merge::resolve::ResolveAction::parse(&action) else {
        return Err(AppError::invalid_input("Unknown resolve action"));
    };
    let store = store.inner().clone();
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::with_repo_lock("resolve conflict", || {
            apply_device_identity(&store, &skills_dir);
            let safety_tag = merge::resolve::resolve_conflict_unlocked(
                &store, &skills_dir, &skill_id, action,
            )?;
            reconcile_skills_index_unlocked(&store)?;
            store.log_audit(
                crate::core::audit_log::AuditDraft::new("resolve_conflict")
                    .skill(skill_id.clone(), skill_id.clone())
                    .detail(format!("action={action:?}"))
                    .ok(),
            );
            Ok(safety_tag)
        })
        .map_err(classify_git_chain)
    })
    .await?
}

#[tauri::command]
pub async fn git_backup_clone(
    store: State<'_, Arc<SkillStore>>,
    url: String,
) -> Result<(), AppError> {
    git_fetcher::validate_git_url(&url).map_err(AppError::git)?;
    let store = store.inner().clone();
    sync_engine_pref(&store);
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        let effective = sanitize_url_to_keychain(url.trim());
        git_backup::with_repo_lock("git clone", || {
            git_backup::clone_into_unlocked(&skills_dir, &effective)?;
            apply_device_identity(&store, &skills_dir);
            reconcile_skills_index_unlocked(&store)
        })
        .map_err(classify_git_chain)
    })
    .await?
}

/// Recovery: discard the local `.git` and re-clone from the configured remote.
/// Existing skill files are preserved via the same backup-then-merge flow
/// used by the regular clone path.
#[tauri::command]
pub async fn git_backup_reclone(
    store: State<'_, Arc<SkillStore>>,
    url: String,
) -> Result<(), AppError> {
    git_fetcher::validate_git_url(&url).map_err(AppError::git)?;
    let store = store.inner().clone();
    sync_engine_pref(&store);
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        let effective = sanitize_url_to_keychain(url.trim());
        git_backup::with_repo_lock("git reclone", || {
            git_backup::reclone_from_remote_unlocked(&skills_dir, &effective)?;
            apply_device_identity(&store, &skills_dir);
            reconcile_skills_index_unlocked(&store)
        })
        .map_err(classify_git_chain)
    })
    .await?
}

#[tauri::command]
pub async fn git_backup_create_snapshot(
    store: State<'_, Arc<SkillStore>>,
) -> Result<String, AppError> {
    let _ = store;
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::create_snapshot_tag(&skills_dir).map_err(AppError::git)
    })
    .await?
}

#[tauri::command]
pub async fn git_backup_list_versions(
    store: State<'_, Arc<SkillStore>>,
    limit: Option<u32>,
) -> Result<Vec<git_backup::GitBackupVersion>, AppError> {
    let _ = store;
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::list_snapshot_versions(&skills_dir, limit.map(|v| v as usize))
            .map_err(AppError::git)
    })
    .await?
}

/// Restore a snapshot. Returns the safety-point tag that captured the
/// pre-restore state, so the UI can tell the user the restore is undoable.
#[tauri::command]
pub async fn git_backup_restore_version(
    store: State<'_, Arc<SkillStore>>,
    tag: String,
) -> Result<String, AppError> {
    let store = store.inner().clone();
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::with_repo_lock("git restore snapshot", || {
            // The safety-point and restore commits are made by this device.
            apply_device_identity(&store, &skills_dir);
            let safety_tag = git_backup::restore_snapshot_version_unlocked(&skills_dir, &tag)?;
            reconcile_skills_index_unlocked(&store)?;
            Ok(safety_tag)
        })
        .map_err(AppError::git)
    })
    .await?
}

/// Effective device name (§4.3 设备命名): the saved setting, or a persisted
/// hostname-derived default.
#[tauri::command]
pub async fn backup_device_name(store: State<'_, Arc<SkillStore>>) -> Result<String, AppError> {
    let store = store.inner().clone();
    tokio::task::spawn_blocking(move || Ok(effective_device_name(&store))).await?
}

/// Rename this device. Only affects backups made from now on — history keeps
/// the author it was written with. Returns the sanitized name actually saved.
#[tauri::command]
pub async fn backup_set_device_name(
    store: State<'_, Arc<SkillStore>>,
    name: String,
) -> Result<String, AppError> {
    let store = store.inner().clone();
    tokio::task::spawn_blocking(move || {
        let sanitized = git_backup::sanitize_device_name(&name);
        if sanitized.is_empty() {
            return Err(AppError::invalid_input("Device name is empty"));
        }
        store
            .set_setting("backup_device_name", &sanitized)
            .map_err(AppError::db)?;
        let skills_dir = central_repo::skills_dir();
        if let Err(e) = git_backup::configure_device_identity(&skills_dir, &sanitized) {
            log::warn!("device name: failed to configure git identity: {e:#}");
        }
        Ok(sanitized)
    })
    .await?
}

#[tauri::command]
pub async fn git_backup_size_report() -> Result<git_backup::BackupSizeReport, AppError> {
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || git_backup::size_report(&skills_dir).map_err(AppError::io))
        .await?
}

/// Migrate credentials embedded in the remote URL (`user:token@host`) into
/// the OS keychain (§3.7). Rewrites `.git/config` and the saved setting to
/// the credential-free URL, verifies authentication still works, and rolls
/// everything back on any failure — no half-migrated state. Returns the
/// sanitized URL when a migration happened, `None` when there was nothing to
/// migrate.
#[tauri::command]
pub async fn git_backup_migrate_credentials(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Option<String>, AppError> {
    let store = store.inner().clone();
    tokio::task::spawn_blocking(move || migrate_embedded_credentials(&store).map_err(AppError::git))
        .await?
}

pub fn migrate_embedded_credentials(store: &SkillStore) -> anyhow::Result<Option<String>> {
    sync_engine_pref(store);
    let skills_dir = central_repo::skills_dir();
    git_backup::with_repo_lock("git credential migration", || {
        migrate_embedded_credentials_unlocked(store, &skills_dir)
    })
}

fn migrate_embedded_credentials_unlocked(
    store: &SkillStore,
    skills_dir: &Path,
) -> anyhow::Result<Option<String>> {
    let has_embedded = |url: &String| git_credentials::split_credentials_from_url(url).is_some();

    let config_url = if skills_dir.join(".git").exists() {
        git_backup::raw_remote_url(skills_dir)
    } else {
        None
    };
    let setting_url = store
        .get_setting("git_backup_remote_url")
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty());

    let config_had_creds = config_url.as_ref().map(has_embedded).unwrap_or(false);
    let source_url = if config_had_creds {
        config_url.clone()
    } else {
        setting_url.clone().filter(has_embedded)
    };
    let Some(source_url) = source_url else {
        return Ok(None);
    };

    let (cred, sanitized) = git_credentials::split_credentials_from_url(&source_url)
        .expect("source_url was checked to carry credentials");
    let host = git_credentials::https_host(&sanitized)
        .context("Cannot determine host for credential migration")?;

    // Step 1: token into the keychain. Nothing on disk has changed yet, so a
    // failure here leaves everything as it was.
    git_credentials::store_credential(&host, &cred)?;

    // Step 2: rewrite `.git/config` to the credential-free URL, then verify
    // that authentication through the keychain still works. Any failure rolls
    // back to the exact previous state.
    if config_had_creds {
        if let Err(e) = git_backup::set_remote_url_only(skills_dir, &sanitized) {
            let _ = git_credentials::delete_credential(&host);
            return Err(e);
        }
        if let Err(e) = git_backup::verify_remote_auth(skills_dir) {
            let _ = git_backup::set_remote_url_only(skills_dir, &source_url);
            let _ = git_credentials::delete_credential(&host);
            return Err(e.context(
                "Credential migration verification failed; previous configuration restored",
            ));
        }
    }

    // Step 3: rewrite the saved setting.
    if setting_url.as_deref() != Some(sanitized.as_str()) {
        if let Err(e) = store.set_setting("git_backup_remote_url", &sanitized) {
            if config_had_creds {
                let _ = git_backup::set_remote_url_only(skills_dir, &source_url);
            }
            let _ = git_credentials::delete_credential(&host);
            return Err(e);
        }
    }

    log::info!("git credentials: migrated embedded token to OS keychain for {host}");
    Ok(Some(sanitized))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestEnv {
        _lock: std::sync::MutexGuard<'static, ()>,
        _tmp: tempfile::TempDir,
        store: SkillStore,
        skills_dir: std::path::PathBuf,
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            central_repo::set_test_base_dir_override(None);
        }
    }

    /// Isolated base dir (askpass script, skills repo, DB) + mock keyring.
    fn test_env() -> TestEnv {
        git_credentials::use_mock_keyring();
        let lock = central_repo::test_base_dir_lock();
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        let skills_dir = central_repo::skills_dir();
        std::fs::create_dir_all(&skills_dir).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();
        TestEnv {
            _lock: lock,
            _tmp: tmp,
            store,
            skills_dir,
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn origin_url(dir: &Path) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["remote", "get-url", "origin"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn device_name_default_persists_and_rename_updates_repo_config() {
        let env = test_env();
        // First resolve derives a hostname default and persists it, so the
        // name stays stable even if the hostname later changes.
        let name = effective_device_name(&env.store);
        assert!(!name.is_empty());
        assert_eq!(
            env.store.get_setting("backup_device_name").unwrap().as_deref(),
            Some(name.as_str())
        );

        // With a repo present, a rename rewrites the repo-local identity used
        // for all future commits (§4.3).
        git(&env.skills_dir, &["init", "-b", "main"]);
        env.store
            .set_setting("backup_device_name", "Work Laptop")
            .unwrap();
        apply_device_identity(&env.store, &env.skills_dir);
        let user_name = std::process::Command::new("git")
            .arg("-C")
            .arg(&env.skills_dir)
            .args(["config", "--local", "--get", "user.name"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&user_name.stdout).trim(),
            "Work Laptop"
        );
    }

    #[test]
    fn sync_transaction_commits_merges_and_pushes_in_one_call() {
        let env = test_env();
        // Local repo wired to a bare remote.
        let remote = env._tmp.path().join("remote.git");
        assert!(std::process::Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(&remote)
            .output()
            .unwrap()
            .status
            .success());
        crate::core::git_backup::init_repo_unlocked(&env.skills_dir, "Device A").unwrap();
        crate::core::git_backup::set_remote_unlocked(&env.skills_dir, remote.to_str().unwrap())
            .unwrap();
        crate::core::git_backup::push_unlocked(&env.skills_dir).unwrap();

        // Another device clones and pushes a change (with protocol markers).
        let other = env._tmp.path().join("other");
        assert!(std::process::Command::new("git")
            .arg("clone")
            .arg(&remote)
            .arg(&other)
            .output()
            .unwrap()
            .status
            .success());
        crate::core::git_backup::configure_device_identity(&other, "Device B").unwrap();
        std::fs::write(other.join("from-b.md"), "b").unwrap();
        crate::core::git_backup::commit_all_unlocked(&other, "from B").unwrap();
        git(&other, &["push", "origin", "main"]);

        // Local uncommitted edit + being behind: one sync call must commit,
        // merge, snapshot and push — no separate commit/pull/push round trips.
        std::fs::write(env.skills_dir.join("local.md"), "a").unwrap();
        let outcome = run_sync_blocking(&env.store, &env.skills_dir, "backup").unwrap();
        assert!(outcome.committed);
        assert!(outcome.merge.is_some(), "a merge must have run");
        assert!(outcome.pushed);
        assert!(outcome.snapshot_tag.is_some());

        // Remote now holds the merged state with both files.
        let remote_head = {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&remote)
                .args(["rev-parse", "main"])
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let local_head = {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&env.skills_dir)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        assert_eq!(remote_head, local_head);
        assert!(env.skills_dir.join("from-b.md").exists());
        assert!(env.skills_dir.join("local.md").exists());

        // A second sync with nothing new is a full no-op.
        let quiet = run_sync_blocking(&env.store, &env.skills_dir, "backup").unwrap();
        assert!(!quiet.committed);
        assert!(quiet.merge.is_none());
        assert!(!quiet.pushed);
        assert!(quiet.snapshot_tag.is_none());
    }

    #[test]
    fn migrate_is_noop_without_embedded_credentials() {
        let env = test_env();
        git(&env.skills_dir, &["init", "-b", "main"]);
        git(
            &env.skills_dir,
            &["remote", "add", "origin", "https://github.com/acme/repo.git"],
        );
        env.store
            .set_setting("git_backup_remote_url", "https://github.com/acme/repo.git")
            .unwrap();

        let result =
            migrate_embedded_credentials_unlocked(&env.store, &env.skills_dir).unwrap();
        assert_eq!(result, None);
        assert_eq!(origin_url(&env.skills_dir), "https://github.com/acme/repo.git");
    }

    #[test]
    fn disconnect_clears_origin_and_setting_and_is_idempotent() {
        // #260 acceptance: after "disconnect this machine" neither the git
        // origin nor the saved setting may survive, so nothing can backfill
        // the URL when the settings page reopens.
        let env = test_env();
        git(&env.skills_dir, &["init", "-b", "main"]);
        git(
            &env.skills_dir,
            &["remote", "add", "origin", "https://github.com/acme/repo.git"],
        );
        env.store
            .set_setting("git_backup_remote_url", "https://github.com/acme/repo.git")
            .unwrap();

        disconnect_local(&env.store, &env.skills_dir).unwrap();
        assert_eq!(origin_url(&env.skills_dir), "");
        assert_eq!(
            env.store.get_setting("git_backup_remote_url").unwrap().as_deref(),
            Some("")
        );

        // A second disconnect (nothing left to remove) must still succeed.
        disconnect_local(&env.store, &env.skills_dir).unwrap();
    }

    #[test]
    fn migrate_sanitizes_setting_when_no_repo_exists() {
        let env = test_env();
        // Only the saved setting carries a token (repo not initialized yet):
        // the token moves to the keychain and the setting is rewritten, with
        // no network verification possible or needed.
        env.store
            .set_setting(
                "git_backup_remote_url",
                "https://user:tok@github.com/acme/repo.git",
            )
            .unwrap();

        let result =
            migrate_embedded_credentials_unlocked(&env.store, &env.skills_dir).unwrap();
        assert_eq!(result.as_deref(), Some("https://github.com/acme/repo.git"));
        assert_eq!(
            env.store.get_setting("git_backup_remote_url").unwrap().as_deref(),
            Some("https://github.com/acme/repo.git")
        );
    }

    #[test]
    fn migrate_rolls_back_config_and_setting_on_verify_failure() {
        let env = test_env();
        // Unreachable host: verification must fail after the config rewrite,
        // and the rollback must restore the exact original state (§3.7 —
        // no half-migrated state).
        let token_url = "https://user:sometok@127.0.0.1:1/acme/repo.git";
        git(&env.skills_dir, &["init", "-b", "main"]);
        git(&env.skills_dir, &["remote", "add", "origin", token_url]);
        env.store
            .set_setting("git_backup_remote_url", token_url)
            .unwrap();

        let err = migrate_embedded_credentials_unlocked(&env.store, &env.skills_dir).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("verification failed"),
            "unexpected error: {msg}"
        );
        assert!(
            !msg.contains("sometok"),
            "token must not leak into the error: {msg}"
        );
        assert_eq!(origin_url(&env.skills_dir), token_url);
        assert_eq!(
            env.store.get_setting("git_backup_remote_url").unwrap().as_deref(),
            Some(token_url)
        );
    }
}

pub(crate) fn reconcile_skills_index_unlocked(store: &SkillStore) -> anyhow::Result<()> {
    sync_metadata::cleanup_temporary_files()?;
    if sync_metadata::has_complete_skill_snapshot() {
        sync_metadata::reindex_from_metadata_unlocked(store)?;
        return Ok(());
    }

    let skills_dir = central_repo::skills_dir();
    std::fs::create_dir_all(&skills_dir)?;

    // Remove stale DB records whose central directories no longer exist.
    let existing = store.get_all_skills()?;
    for skill in existing {
        if !std::path::Path::new(&skill.central_path).exists() {
            store.delete_skill(&skill.id)?;
        }
    }

    // Add missing DB records for directories present in central repo.
    for entry in WalkDir::new(&skills_dir)
        .min_depth(1)
        .max_depth(6)
        .into_iter()
        .filter_entry(|e| e.file_name().to_string_lossy() != ".git")
        .flatten()
    {
        let path = entry.path().to_path_buf();
        if !entry.file_type().is_dir() || !skill_metadata::is_valid_skill_dir(&path) {
            continue;
        }

        let central_path = path.to_string_lossy().to_string();
        if store.get_skill_by_central_path(&central_path)?.is_some() {
            continue;
        }

        let meta = crate::core::skill_metadata::parse_skill_md(&path);
        let inferred_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown-skill".to_string());
        let name = meta
            .name
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(inferred_name);
        let now = chrono::Utc::now().timestamp_millis();

        let record = crate::core::skill_store::SkillRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            description: meta.description,
            source_type: "import".to_string(),
            source_ref: Some(central_path.clone()),
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path,
            content_hash: crate::core::content_hash::hash_directory(&path).ok(),
            enabled: true,
            created_at: now,
            updated_at: now,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: Some(now),
            last_check_error: None,
        };

        store.insert_skill(&record)?;
    }

    sync_metadata::write_all_from_db_unlocked(store)
}
