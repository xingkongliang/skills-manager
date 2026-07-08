//! Automatic backup (§3.4): after central-repo changes settle for a couple of
//! minutes, commit and push in the background — no snapshot tag (tags are
//! reserved for user-visible backup points).
//!
//! With the object merge engine (merge-engine design §9 3d-γ) a round that
//! finds the remote ahead also merges it and pushes, so two devices converge
//! hands-free. The one deliberate exception (§4 收窄阻尼): while a remote
//! change touches a skill that is pending a local conflict decision, the
//! round backs off and waits for a manual sync — unrelated updates keep
//! flowing. With the `merge_engine=system` escape hatch the old behavior
//! (wait for manual sync whenever the remote is ahead) is kept.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use super::central_repo;
use super::git_backup;
use super::merge;
use super::repo_lock::RepoLock;
use super::skill_store::SkillStore;
use super::sync_metadata;

/// Off-switch setting. Anything except an explicit "off"-ish value means on.
const SETTING_ENABLED: &str = "backup_auto_enabled";
/// Last auto-backup failure, kept until a successful push (auto or manual)
/// clears it, so the Backup page failure card survives navigation/restart.
pub const SETTING_LAST_ERROR: &str = "backup_last_auto_error";
const EVENT_COMPLETED: &str = "backup-auto-completed";
const AUTO_COMMIT_MESSAGE: &str = "auto backup";

/// Trailing debounce after the last central-repo change (§3.4 分钟级防抖).
const DEBOUNCE: Duration = Duration::from_secs(120);
/// Scheduler wake cadence; also the retry latency after a busy repo lock.
const POLL_INTERVAL: Duration = Duration::from_secs(15);
/// Delay before the first round, which uploads anything committed at the
/// previous exit. Matches the auto-updater's startup courtesy delay.
const INITIAL_DELAY: Duration = Duration::from_secs(90);
/// Cap for the exponential retry backoff after failed rounds: 2^5 × 2min ≈ 1h.
const MAX_BACKOFF_SHIFT: u32 = 5;

static DIRTY: AtomicBool = AtomicBool::new(false);
static LAST_CHANGE_MS: AtomicI64 = AtomicI64::new(0);
static CONSECUTIVE_FAILURES: AtomicU32 = AtomicU32::new(0);

/// Called by the file watcher whenever something outside `.git` changes in the
/// central repository. Re-arms the debounce window.
pub fn notify_central_change() {
    LAST_CHANGE_MS.store(chrono::Utc::now().timestamp_millis(), Ordering::Release);
    DIRTY.store(true, Ordering::Release);
}

/// Pure debounce/backoff decision, kept free of the statics for testability.
/// A round is due when a change is pending and the quiet period — stretched
/// exponentially by consecutive failures — has elapsed since the last change.
fn is_due(dirty: bool, last_change_ms: i64, now_ms: i64, failures: u32) -> bool {
    if !dirty {
        return false;
    }
    let quiet = DEBOUNCE.as_millis() as i64;
    let backoff = quiet << failures.min(MAX_BACKOFF_SHIFT);
    now_ms - last_change_ms >= backoff
}

pub fn is_enabled(store: &SkillStore) -> bool {
    let value = store
        .get_setting(SETTING_ENABLED)
        .ok()
        .flatten()
        .map(|v| v.trim().to_ascii_lowercase());
    !matches!(value.as_deref(), Some("off" | "false" | "0" | "no"))
}

#[derive(Debug, PartialEq)]
pub(crate) enum Outcome {
    /// Nothing attempted: disabled, no repo/remote, busy lock, needs repair.
    Skipped(&'static str),
    /// Tree clean and remote current — nothing to do.
    UpToDate,
    /// Committed and/or pushed successfully.
    BackedUp,
    /// The remote moved ahead (another device pushed). With the system
    /// engine this waits for a manual sync; with the object engine it is a
    /// transient state — the re-armed next round merges and pushes.
    RemoteAhead,
    /// §4 收窄阻尼: the remote touches a skill with an unresolved local
    /// conflict — deliberate backpressure until a manual decision.
    PausedOnConflict,
    Failed(String),
}

pub fn start<R: Runtime>(app: AppHandle<R>, store: Arc<SkillStore>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(INITIAL_DELAY).await;
        // Startup round: push whatever the exit-time commit captured. Backdate
        // the change stamp so the round is due on the first tick.
        LAST_CHANGE_MS.store(
            chrono::Utc::now().timestamp_millis() - DEBOUNCE.as_millis() as i64,
            Ordering::Release,
        );
        DIRTY.store(true, Ordering::Release);

        loop {
            let now_ms = chrono::Utc::now().timestamp_millis();
            if is_due(
                DIRTY.load(Ordering::Acquire),
                LAST_CHANGE_MS.load(Ordering::Acquire),
                now_ms,
                CONSECUTIVE_FAILURES.load(Ordering::Acquire),
            ) {
                DIRTY.store(false, Ordering::Release);
                let store_for_round = store.clone();
                let outcome = tauri::async_runtime::spawn_blocking(move || {
                    run_round_blocking(&store_for_round)
                })
                .await
                .unwrap_or_else(|e| Outcome::Failed(format!("join error: {e}")));
                handle_outcome(&app, &store, outcome);
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}

#[derive(Serialize, Clone)]
struct AutoBackupPayload {
    ok: bool,
    /// True when the remote moved ahead and a manual sync is needed.
    pending: bool,
    error: Option<String>,
}

fn handle_outcome<R: Runtime>(app: &AppHandle<R>, store: &SkillStore, outcome: Outcome) {
    let emit = |payload: AutoBackupPayload| {
        if let Err(e) = app.emit(EVENT_COMPLETED, payload) {
            log::debug!("auto backup: emit failed: {e}");
        }
    };
    let clear_error = |store: &SkillStore| {
        let had_error = matches!(
            store.get_setting(SETTING_LAST_ERROR).ok().flatten().as_deref(),
            Some(v) if !v.trim().is_empty()
        );
        if had_error {
            let _ = store.set_setting(SETTING_LAST_ERROR, "");
        }
        had_error
    };

    match outcome {
        Outcome::Skipped(reason) => {
            if reason == "repo busy" {
                // A foreground operation held the lock; retry on the next tick
                // (the change stamp is untouched, so the round is due again).
                DIRTY.store(true, Ordering::Release);
            }
            log::debug!("auto backup: skipped ({reason})");
        }
        Outcome::UpToDate => {
            CONSECUTIVE_FAILURES.store(0, Ordering::Release);
            // Only worth telling the UI if a stale failure card should clear.
            if clear_error(store) {
                emit(AutoBackupPayload { ok: true, pending: false, error: None });
            }
        }
        Outcome::BackedUp => {
            CONSECUTIVE_FAILURES.store(0, Ordering::Release);
            clear_error(store);
            log::info!("auto backup: committed/pushed");
            emit(AutoBackupPayload { ok: true, pending: false, error: None });
        }
        Outcome::RemoteAhead => {
            CONSECUTIVE_FAILURES.store(0, Ordering::Release);
            log::info!("auto backup: remote is ahead, waiting for the next round or manual sync");
            emit(AutoBackupPayload { ok: false, pending: true, error: None });
        }
        Outcome::PausedOnConflict => {
            CONSECUTIVE_FAILURES.store(0, Ordering::Release);
            log::info!(
                "auto backup: remote touches a pending conflict — paused until it is resolved (§4)"
            );
            emit(AutoBackupPayload { ok: false, pending: true, error: None });
        }
        Outcome::Failed(msg) => {
            CONSECUTIVE_FAILURES.fetch_add(1, Ordering::AcqRel);
            log::warn!("auto backup: failed: {msg}");
            if let Err(e) = store.set_setting(SETTING_LAST_ERROR, &msg) {
                log::warn!("auto backup: failed to persist error: {e:#}");
            }
            // Re-arm so the round retries after the (backed-off) quiet period.
            notify_central_change();
            emit(AutoBackupPayload { ok: false, pending: false, error: Some(msg) });
        }
    }
}

pub(crate) fn run_round_blocking(store: &SkillStore) -> Outcome {
    if !is_enabled(store) {
        return Outcome::Skipped("disabled");
    }
    let skills_dir = central_repo::skills_dir();
    if !skills_dir.join(".git").exists() {
        return Outcome::Skipped("no repo");
    }
    if git_backup::raw_remote_url(&skills_dir).is_none() {
        return Outcome::Skipped("no remote");
    }
    crate::commands::git_backup::sync_engine_pref(store);

    // Fail fast instead of queueing behind a user-initiated operation.
    let Ok(_lock) = RepoLock::acquire("auto backup") else {
        return Outcome::Skipped("repo busy");
    };
    if git_backup::ensure_no_interrupted_git_operation(&skills_dir).is_err() {
        return Outcome::Skipped("interrupted git operation");
    }

    // See the true remote state (3d-γ): rounds are debounced, so one quiet
    // fetch per round is cheap; offline it just leaves the refs stale.
    if let Err(e) = git_backup::fetch_remote(&skills_dir) {
        log::debug!("auto backup: fetch failed (continuing offline): {e:#}");
    }

    let status = match git_backup::get_status(&skills_dir) {
        Ok(status) => status,
        Err(e) => return Outcome::Failed(format!("{e:#}")),
    };
    if status.upstream_health == "unrelated_histories" || status.upstream_health == "detached" {
        // The Backup page routes these to the recovery flow; retrying in the
        // background would only thrash.
        return Outcome::Skipped("needs manual repair");
    }

    crate::commands::git_backup::apply_device_identity(store, &skills_dir);
    if let Err(e) = sync_metadata::write_all_from_db_unlocked(store) {
        return Outcome::Failed(format!("{e:#}"));
    }
    // Before the dirty check, so a shrunk previously-excluded skill re-enters
    // the backup via the resulting .gitignore change (§3.6).
    if let Err(e) =
        git_backup::apply_oversized_exclusions(&skills_dir, git_backup::SKILL_SIZE_LIMIT_BYTES)
    {
        log::debug!("auto backup: exclusion scan failed (continuing): {e:#}");
    }

    let mut committed = false;
    match git_backup::has_uncommitted_changes(&skills_dir) {
        Ok(true) => {
            if let Err(e) = git_backup::commit_all_unlocked(&skills_dir, AUTO_COMMIT_MESSAGE) {
                return Outcome::Failed(format!("{e:#}"));
            }
            committed = true;
        }
        Ok(false) => {}
        Err(e) => return Outcome::Failed(format!("{e:#}")),
    }

    if status.behind > 0 {
        // The system engine never line-merges in the background — the local
        // commit is safe, the merge waits for a user-driven sync.
        if !merge::object_merge_enabled(store) {
            return Outcome::RemoteAhead;
        }
        // §4 收窄阻尼: unrelated remote updates flow automatically; a remote
        // change to a skill awaiting a local conflict decision pauses the
        // round (deliberate backpressure, cleared by resolving + syncing).
        match merge::remote_touches_pending(store, &skills_dir) {
            Ok(true) => return Outcome::PausedOnConflict,
            Ok(false) => {}
            Err(e) => return Outcome::Failed(format!("{e:#}")),
        }
        match merge::gated_pull_unlocked(store, &skills_dir) {
            Ok(_summary) => {
                if let Err(e) = crate::commands::git_backup::reconcile_skills_index_unlocked(store)
                {
                    return Outcome::Failed(format!("{e:#}"));
                }
            }
            Err(e) => return Outcome::Failed(format!("{e:#}")),
        }
    }

    // Re-read after a possible merge: a merge commit shows up as ahead > 0,
    // a pure fast-forward leaves nothing to push.
    let status = match git_backup::get_status(&skills_dir) {
        Ok(status) => status,
        Err(e) => return Outcome::Failed(format!("{e:#}")),
    };
    let needs_push = committed || status.ahead > 0 || status.upstream_health == "no_upstream";
    if !needs_push {
        return Outcome::UpToDate;
    }
    match git_backup::push_unlocked(&skills_dir) {
        Ok(()) => Outcome::BackedUp,
        Err(e) => {
            let msg = format!("{e:#}");
            if msg.contains("non-fast-forward")
                || msg.contains("fetch first")
                || msg.contains("[rejected]")
                || msg.contains("failed to push some refs")
            {
                // Another device pushed inside our window. Re-arm so the next
                // round (post-debounce) fetches, merges and retries.
                notify_central_change();
                Outcome::RemoteAhead
            } else {
                Outcome::Failed(msg)
            }
        }
    }
}

/// Best-effort "退出前" save (§3.4): commit outstanding changes locally so
/// nothing is lost between sessions. Never touches the network — the next
/// startup round pushes — and never blocks quitting (fail-fast lock).
pub fn commit_on_exit(store: &SkillStore) {
    if !is_enabled(store) {
        return;
    }
    let skills_dir = central_repo::skills_dir();
    if !skills_dir.join(".git").exists() {
        return;
    }
    let Ok(_lock) = RepoLock::acquire("auto backup on exit") else {
        return;
    };
    if git_backup::ensure_no_interrupted_git_operation(&skills_dir).is_err() {
        return;
    }
    crate::commands::git_backup::apply_device_identity(store, &skills_dir);
    if let Err(e) = sync_metadata::write_all_from_db_unlocked(store) {
        log::warn!("auto backup on exit: metadata write failed: {e:#}");
    }
    match git_backup::has_uncommitted_changes(&skills_dir) {
        Ok(true) => {
            if let Err(e) = git_backup::commit_all_unlocked(&skills_dir, AUTO_COMMIT_MESSAGE) {
                log::warn!("auto backup on exit: commit failed: {e:#}");
            } else {
                log::info!("auto backup on exit: committed pending changes");
            }
        }
        Ok(false) => {}
        Err(e) => log::warn!("auto backup on exit: status check failed: {e:#}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── is_due (pure debounce/backoff decision) ──

    const MIN2: i64 = 120_000;

    #[test]
    fn not_due_without_pending_change() {
        assert!(!is_due(false, 0, MIN2 * 10, 0));
    }

    #[test]
    fn due_after_quiet_period() {
        assert!(!is_due(true, 1_000_000, 1_000_000 + MIN2 - 1, 0));
        assert!(is_due(true, 1_000_000, 1_000_000 + MIN2, 0));
    }

    #[test]
    fn failures_stretch_the_quiet_period_with_a_cap() {
        let base = 1_000_000;
        assert!(!is_due(true, base, base + MIN2, 1));
        assert!(is_due(true, base, base + 2 * MIN2, 1));
        // Backoff is capped at 2^5 — an hour-ish, never unbounded.
        assert!(is_due(true, base, base + 32 * MIN2, 40));
        assert!(!is_due(true, base, base + 31 * MIN2, 40));
    }

    // ── round behaviour against a local bare remote ──

    struct TestEnv {
        _lock: std::sync::MutexGuard<'static, ()>,
        _tmp: tempfile::TempDir,
        store: SkillStore,
        skills_dir: std::path::PathBuf,
        remote: std::path::PathBuf,
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            central_repo::set_test_base_dir_override(None);
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

    fn git_out(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Skills repo (one commit, upstream tracking) + local bare remote.
    fn test_env() -> TestEnv {
        let lock = central_repo::test_base_dir_lock();
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        let skills_dir = central_repo::skills_dir();
        std::fs::create_dir_all(&skills_dir).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();

        let remote = tmp.path().join("remote.git");
        let out = std::process::Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(&remote)
            .output()
            .unwrap();
        assert!(out.status.success());

        git_backup::init_repo_unlocked(&skills_dir, "Test Device").unwrap();
        git_backup::set_remote_unlocked(&skills_dir, remote.to_str().unwrap()).unwrap();
        git_backup::push_unlocked(&skills_dir).unwrap();

        TestEnv {
            _lock: lock,
            _tmp: tmp,
            store,
            skills_dir,
            remote,
        }
    }

    #[test]
    fn round_commits_and_pushes_without_snapshot_tag() {
        let env = test_env();
        std::fs::create_dir_all(env.skills_dir.join("skill-a")).unwrap();
        std::fs::write(env.skills_dir.join("skill-a/SKILL.md"), "content").unwrap();

        let outcome = run_round_blocking(&env.store);
        assert_eq!(outcome, Outcome::BackedUp);

        // The change reached the remote…
        let remote_head = git_out(&env.remote, &["rev-parse", "main"]);
        let local_head = git_out(&env.skills_dir, &["rev-parse", "HEAD"]);
        assert_eq!(remote_head, local_head);
        assert_eq!(
            git_out(&env.skills_dir, &["log", "-1", "--format=%s"]),
            AUTO_COMMIT_MESSAGE
        );
        // …and §3.4 holds: automatic backups never mint a snapshot tag.
        assert_eq!(git_out(&env.skills_dir, &["tag", "--list", "sm-v-*"]), "");

        // A second round with nothing new is a no-op.
        assert_eq!(run_round_blocking(&env.store), Outcome::UpToDate);
    }

    /// Clone the test remote as a second "device" with its own identity.
    fn clone_other_device(env: &TestEnv) -> std::path::PathBuf {
        let other = env._tmp.path().join("other");
        let out = std::process::Command::new("git")
            .arg("clone")
            .arg(&env.remote)
            .arg(&other)
            .output()
            .unwrap();
        assert!(out.status.success());
        git_backup::configure_device_identity(&other, "Device B").unwrap();
        other
    }

    #[test]
    fn round_with_system_engine_waits_for_manual_sync_when_remote_ahead() {
        let env = test_env();
        // Escape hatch: the legacy engine never merges in the background.
        env.store.set_setting("merge_engine", "system").unwrap();
        let other = clone_other_device(&env);
        std::fs::write(other.join("from-b.md"), "b").unwrap();
        git(&other, &["add", "-A"]);
        git(&other, &["commit", "-m", "from B"]);
        git(&other, &["push", "origin", "main"]);

        std::fs::write(env.skills_dir.join("local.md"), "a").unwrap();

        let outcome = run_round_blocking(&env.store);
        assert_eq!(outcome, Outcome::RemoteAhead);
        // The local change was still committed (protected locally)…
        assert!(!git_backup::has_uncommitted_changes(&env.skills_dir).unwrap());
        // …but nothing reached the remote and no failure is recorded.
        assert_eq!(
            git_out(&env.remote, &["log", "-1", "--format=%s", "main"]),
            "from B"
        );
        assert_eq!(
            env.store.get_setting(SETTING_LAST_ERROR).unwrap(),
            None
        );
    }

    #[test]
    fn round_merges_remote_changes_and_pushes_back() {
        // 3d-γ: with the (default) object engine, a round that finds the
        // remote ahead merges it and pushes the result — hands-free two-way
        // sync, including the round's own fetch.
        let env = test_env();
        let other = clone_other_device(&env);
        std::fs::write(other.join("from-b.md"), "b").unwrap();
        git_backup::commit_all_unlocked(&other, "from B").unwrap();
        git(&other, &["push", "origin", "main"]);

        // No manual fetch here — the round discovers the remote change itself.
        std::fs::write(env.skills_dir.join("local.md"), "a").unwrap();
        let outcome = run_round_blocking(&env.store);
        assert_eq!(outcome, Outcome::BackedUp);

        // Both sides' changes are present locally and on the remote.
        assert!(env.skills_dir.join("from-b.md").exists());
        assert!(env.skills_dir.join("local.md").exists());
        let remote_head = git_out(&env.remote, &["rev-parse", "main"]);
        let local_head = git_out(&env.skills_dir, &["rev-parse", "HEAD"]);
        assert_eq!(remote_head, local_head);
        assert_eq!(env.store.get_setting(SETTING_LAST_ERROR).unwrap(), None);
    }

    #[test]
    fn round_pauses_when_remote_touches_a_pending_conflict() {
        // §4 收窄阻尼: a remote change to a skill that awaits a local
        // conflict decision pauses the automatic round; an unrelated remote
        // change keeps flowing.
        let env = test_env();
        let other = clone_other_device(&env);

        // A managed skill exists on both sides (seeded through the remote).
        let seed_skill = |dir: &Path| {
            let skill_dir = dir.join("alpha");
            std::fs::create_dir_all(&skill_dir).unwrap();
            std::fs::write(skill_dir.join("SKILL.md"), "base").unwrap();
            let meta_dir = dir.join(".skills-manager/skills");
            std::fs::create_dir_all(&meta_dir).unwrap();
            let meta = crate::core::sync_metadata::SkillMetaFile {
                schema_version: 1,
                skill_id: "skill-1".to_string(),
                path: "alpha".to_string(),
                path_key: "alpha".to_string(),
                enabled: true,
                tags: vec![],
                source: crate::core::sync_metadata::SourceMeta {
                    source_type: "import".to_string(),
                    ref_: None,
                    subpath: None,
                    branch: None,
                },
            };
            std::fs::write(
                meta_dir.join("skill-1.json"),
                crate::core::sync_metadata::canonical_json_bytes(&meta).unwrap(),
            )
            .unwrap();
            std::fs::write(
                dir.join(".skills-manager/schema.json"),
                b"{\n  \"schema_version\": 1,\n  \"app_min_version\": \"2.0.0\",\n  \"created_by\": \"skills-manager\"\n}\n",
            )
            .unwrap();
        };
        seed_skill(&env.skills_dir);
        crate::core::sync_metadata::reindex_from_metadata_unlocked(&env.store).unwrap();
        git_backup::commit_all_unlocked(&env.skills_dir, "seed skill").unwrap();
        git_backup::push_unlocked(&env.skills_dir).unwrap();
        git(&other, &["pull", "origin", "main"]);

        // The skill is pending a local decision (projection row is the gate
        // the damping check reads).
        env.store
            .replace_pending_conflicts(&[crate::core::skill_store::PendingConflictRow {
                skill_id: "skill-1".to_string(),
                theirs_commit: String::new(),
                theirs_path: Some("alpha".to_string()),
                detected_at: 1,
            }])
            .unwrap();

        // Remote touches that very skill → the round pauses.
        std::fs::write(other.join("alpha/SKILL.md"), "changed on B").unwrap();
        git_backup::commit_all_unlocked(&other, "touch pending skill").unwrap();
        git(&other, &["push", "origin", "main"]);
        std::fs::write(env.skills_dir.join("local.md"), "a").unwrap();
        let outcome = run_round_blocking(&env.store);
        assert_eq!(outcome, Outcome::PausedOnConflict);
        // Local version untouched, remote not merged.
        assert_eq!(
            std::fs::read_to_string(env.skills_dir.join("alpha/SKILL.md")).unwrap(),
            "base"
        );

        // Once the conflict is resolved (projection cleared), the same
        // remote state merges automatically again.
        env.store.replace_pending_conflicts(&[]).unwrap();
        let outcome = run_round_blocking(&env.store);
        assert_eq!(outcome, Outcome::BackedUp);
        assert_eq!(
            std::fs::read_to_string(env.skills_dir.join("alpha/SKILL.md")).unwrap(),
            "changed on B"
        );
    }

    #[test]
    fn round_skips_when_disabled() {
        let env = test_env();
        env.store.set_setting(SETTING_ENABLED, "off").unwrap();
        std::fs::write(env.skills_dir.join("x.md"), "x").unwrap();
        assert_eq!(run_round_blocking(&env.store), Outcome::Skipped("disabled"));
        assert!(git_backup::has_uncommitted_changes(&env.skills_dir).unwrap());
    }

    #[test]
    fn commit_on_exit_commits_locally_without_pushing() {
        let env = test_env();
        let remote_before = git_out(&env.remote, &["rev-parse", "main"]);
        std::fs::write(env.skills_dir.join("late-edit.md"), "bye").unwrap();

        commit_on_exit(&env.store);

        assert!(!git_backup::has_uncommitted_changes(&env.skills_dir).unwrap());
        assert_eq!(
            git_out(&env.skills_dir, &["log", "-1", "--format=%s"]),
            AUTO_COMMIT_MESSAGE
        );
        // Exit-time save is local only; the push belongs to the next launch.
        assert_eq!(git_out(&env.remote, &["rev-parse", "main"]), remote_before);
    }
}
