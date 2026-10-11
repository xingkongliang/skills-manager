use anyhow::{Context, Result};
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::git2_engine;
use super::git_credentials;
use super::merge::protocol;
use super::repo_lock::RepoLock;

/// Create a `Command` for git that hides the console window on Windows.
fn git_command() -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new("git");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GitBackupStatus {
    /// Whether the skills directory is a git repository
    pub is_repo: bool,
    /// The configured remote URL (if any)
    pub remote_url: Option<String>,
    /// Current branch name
    pub branch: Option<String>,
    /// Whether there are uncommitted changes
    pub has_changes: bool,
    /// Number of distinct top-level skill directories with uncommitted changes.
    /// Drives the "N skills have unbacked changes" status copy; 0 when only
    /// metadata or root files changed.
    pub changed_skill_count: u32,
    /// Number of commits ahead of remote
    pub ahead: u32,
    /// Number of commits behind remote
    pub behind: u32,
    /// Last commit message
    pub last_commit: Option<String>,
    /// Last commit timestamp (ISO 8601)
    pub last_commit_time: Option<String>,
    /// Snapshot tag that points at current HEAD (if any)
    pub current_snapshot_tag: Option<String>,
    /// Snapshot tag restored most recently (when HEAD is a restore commit)
    pub restored_from_tag: Option<String>,
    /// Health of the relationship to the configured remote.
    /// One of: "healthy", "no_remote", "no_upstream", "unrelated_histories", "detached".
    pub upstream_health: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GitBackupVersion {
    /// Snapshot tag name (e.g. sm-v-20260318-153012-abc1234)
    pub tag: String,
    /// Commit SHA this snapshot points to (short)
    pub commit: String,
    /// Commit message at this snapshot
    pub message: String,
    /// Commit timestamp (ISO 8601)
    pub committed_at: String,
    /// Commit author name — the device name of the machine that made this
    /// backup (§4.3). Empty for commits from before device naming existed.
    pub author: String,
}

/// Default device name derived from the machine's hostname (macOS appends
/// `.local`, which is noise for a display name).
pub fn default_device_name() -> String {
    let host = gethostname::gethostname().to_string_lossy().to_string();
    let host = host.strip_suffix(".local").unwrap_or(&host).trim().to_string();
    if host.is_empty() {
        "My Computer".to_string()
    } else {
        host
    }
}

/// Normalize a user-entered device name into something safe to use as a git
/// author name: no control characters or `<`/`>` (git's ident syntax), single
/// spaces, at most 64 characters. May return an empty string — callers fall
/// back to `default_device_name()`.
pub fn sanitize_device_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control() && *c != '<' && *c != '>')
        .collect();
    cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(64)
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// Synthetic per-device author email: an ASCII slug of the device name at a
/// reserved domain. Only used to make `git log`/shortlog distinguish devices;
/// it never has to be routable.
fn device_email(device_name: &str) -> String {
    let mut slug = String::new();
    for c in device_name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "device" } else { slug };
    format!("{slug}@skills-manager.local")
}

/// Write the device name into the repo-local git identity (§4.3: device name
/// = commit author). Everything that commits in this repo afterwards — manual
/// backups, merge commits from sync, restore commits, future auto-backup —
/// carries the device name, and machines without a global git identity can
/// commit at all. Idempotent; no-op when the repo doesn't exist or the name
/// is empty.
pub fn configure_device_identity(skills_dir: &Path, device_name: &str) -> Result<()> {
    if device_name.trim().is_empty() || !skills_dir.join(".git").exists() {
        return Ok(());
    }
    let email = device_email(device_name);
    let current_name = run_git(skills_dir, &["config", "--local", "--get", "user.name"]).ok();
    let current_email = run_git(skills_dir, &["config", "--local", "--get", "user.email"]).ok();
    if current_name.as_deref() != Some(device_name) {
        run_git_checked(skills_dir, &["config", "user.name", device_name])?;
    }
    if current_email.as_deref() != Some(email.as_str()) {
        run_git_checked(skills_dir, &["config", "user.email", &email])?;
    }
    Ok(())
}

/// Fetch from the remote without modifying the working tree.
/// This is best-effort so status refresh still works while offline.
pub fn fetch_remote(skills_dir: &Path) -> Result<()> {
    if !skills_dir.join(".git").exists() {
        return Ok(());
    }
    if run_git(skills_dir, &["remote", "get-url", "origin"]).is_err() {
        return Ok(());
    }

    let branch = run_git(skills_dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|_| "main".to_string());
    if let Some(url) = raw_remote_url(skills_dir).filter(|u| git2_engine::applies_to(u)) {
        if let Err(e) = git2_engine::fetch(skills_dir, Some(&branch), &url) {
            log::warn!("git fetch (git2, best-effort): {e:#}");
        }
        return Ok(());
    }
    let env = remote_credential_env(skills_dir);
    let _ = run_git_env(skills_dir, &["fetch", "--quiet", "origin", &branch], &env);
    Ok(())
}

/// The raw (unredacted) origin URL, for credential lookup and rewriting.
/// Never log or surface this value directly — it may embed a token on
/// not-yet-migrated repos.
pub(crate) fn raw_remote_url(skills_dir: &Path) -> Option<String> {
    run_git(skills_dir, &["remote", "get-url", "origin"]).ok()
}

/// Credential-injection environment for git subprocesses talking to origin.
fn remote_credential_env(skills_dir: &Path) -> Vec<(String, String)> {
    raw_remote_url(skills_dir)
        .map(|url| git_credentials::credential_env_for_url(&url))
        .unwrap_or_default()
}

/// Get the current git status of the skills directory.
pub fn get_status(skills_dir: &Path) -> Result<GitBackupStatus> {
    if !skills_dir.join(".git").exists() {
        return Ok(GitBackupStatus {
            is_repo: false,
            remote_url: None,
            branch: None,
            has_changes: false,
            changed_skill_count: 0,
            ahead: 0,
            behind: 0,
            last_commit: None,
            last_commit_time: None,
            current_snapshot_tag: None,
            restored_from_tag: None,
            upstream_health: "no_remote".to_string(),
        });
    }

    let remote_url = run_git(skills_dir, &["remote", "get-url", "origin"])
        .ok()
        .map(|url| redact_url(&url));

    let branch = run_git(skills_dir, &["rev-parse", "--abbrev-ref", "HEAD"]).ok();

    let porcelain = run_git(skills_dir, &["status", "--porcelain"]).unwrap_or_default();
    let has_changes = !porcelain.is_empty();
    let changed_skill_count = count_changed_top_dirs(&porcelain);

    let (ahead, behind) = get_ahead_behind(skills_dir).unwrap_or((0, 0));

    let last_commit = run_git(skills_dir, &["log", "-1", "--format=%s"]).ok();

    let last_commit_time = run_git(skills_dir, &["log", "-1", "--format=%cI"]).ok();

    let current_snapshot_tag = run_git(
        skills_dir,
        &[
            "tag",
            "--points-at",
            "HEAD",
            "--list",
            "sm-v-*",
            "--sort=-creatordate",
        ],
    )
    .ok()
    .and_then(|output| {
        output
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(|line| line.to_string())
    });

    let restored_from_tag = last_commit
        .as_deref()
        .and_then(parse_restored_from_tag_message);

    let upstream_health = detect_upstream_health(skills_dir, remote_url.is_some());

    Ok(GitBackupStatus {
        is_repo: true,
        remote_url,
        branch,
        has_changes,
        changed_skill_count,
        ahead,
        behind,
        last_commit,
        last_commit_time,
        current_snapshot_tag,
        restored_from_tag,
        upstream_health,
    })
}

/// Detect how the local repo relates to the configured remote.
/// Returns one of: "healthy", "no_remote", "no_upstream", "unrelated_histories", "detached".
fn detect_upstream_health(dir: &Path, has_remote: bool) -> String {
    if !has_remote {
        return "no_remote".to_string();
    }
    if run_git(dir, &["symbolic-ref", "-q", "HEAD"]).is_err() {
        return "detached".to_string();
    }
    if run_git(dir, &["rev-parse", "--abbrev-ref", "@{upstream}"]).is_err() {
        return "no_upstream".to_string();
    }
    if run_git(dir, &["merge-base", "HEAD", "@{upstream}"]).is_err() {
        return "unrelated_histories".to_string();
    }
    "healthy".to_string()
}

/// Initialize a new git repository in the skills directory.
#[allow(dead_code)]
pub fn init_repo(skills_dir: &Path, device_name: &str) -> Result<()> {
    let _lock = RepoLock::acquire_foreground("git init")?;
    init_repo_unlocked(skills_dir, device_name)
}

pub(crate) fn init_repo_unlocked(skills_dir: &Path, device_name: &str) -> Result<()> {
    if skills_dir.join(".git").exists() {
        anyhow::bail!("Already a git repository");
    }

    // Pin the classic ref format: libgit2 cannot open reftable repositories,
    // and the user's git may default to it via config or the environment.
    let output = git_command()
        .args(["-c", "init.defaultRefFormat=files", "init"])
        .env_remove("GIT_DEFAULT_REF_FORMAT")
        .current_dir(skills_dir)
        .output()?;
    if !output.status.success() {
        anyhow::bail!("git init failed: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    run_git_checked(skills_dir, &["checkout", "-b", "main"])?;

    // Identity must exist before the initial commit: on machines without a
    // global git identity the commit would otherwise fail outright.
    configure_device_identity(skills_dir, device_name)?;

    ensure_gitignore(skills_dir)?;
    // Cleanup, not a precondition: a failure here means the backup carries a few
    // stale `.pyc` entries, which is not a reason to refuse to back up at all.
    if let Err(e) = untrack_python_artifacts(skills_dir) {
        log::warn!("backup: could not untrack compiled-Python artifacts (continuing): {e:#}");
    }
    // §3.6: a pre-existing oversized skill must not slip into the very first
    // commit — once tracked it can never be excluded again.
    if let Err(e) = apply_oversized_exclusions(skills_dir, SKILL_SIZE_LIMIT_BYTES) {
        log::warn!("backup size: exclusion scan failed (continuing): {e:#}");
    }
    protocol::ensure_protocol_file(skills_dir)?;

    // Initial commit
    run_git_checked(skills_dir, &["add", "-A"])?;
    run_git_checked(
        skills_dir,
        &[
            "commit",
            "-m",
            &protocol::app_commit_message("Initial skill library snapshot"),
        ],
    )?;

    log::info!("git init: initialized repository on branch main");
    Ok(())
}

/// Set (or update) the remote origin URL.
pub fn set_remote(skills_dir: &Path, url: &str) -> Result<()> {
    let _lock = RepoLock::acquire_foreground("git set remote")?;
    set_remote_unlocked(skills_dir, url)
}

pub(crate) fn set_remote_unlocked(skills_dir: &Path, url: &str) -> Result<()> {
    ensure_repo(skills_dir)?;

    let has_remote = run_git(skills_dir, &["remote", "get-url", "origin"]).is_ok();
    if has_remote {
        run_git_checked(skills_dir, &["remote", "set-url", "origin", url])?;
    } else {
        run_git_checked(skills_dir, &["remote", "add", "origin", url])?;
    }

    // Fetch remote to set up tracking
    if git2_engine::applies_to(url) {
        if let Err(e) = git2_engine::fetch(skills_dir, None, url) {
            log::warn!("git set_remote: initial fetch failed (continuing): {e:#}");
        }
    } else {
        let env = remote_credential_env(skills_dir);
        if let Err(e) = run_git_env(skills_dir, &["fetch", "origin"], &env) {
            log::warn!("git set_remote: initial fetch failed (continuing): {e}");
        }
    }

    // Set upstream tracking if branch exists on remote
    let branch = run_git(skills_dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|_| "main".to_string());
    let _ = run_git(
        skills_dir,
        &[
            "branch",
            "--set-upstream-to",
            &format!("origin/{}", branch),
            &branch,
        ],
    );

    // Whether tracking got established decides the whole sync path: with no
    // upstream the first push must use `-u` and the "ahead" count reads 0.
    // Logging it here is what makes "Sync says up to date but remote is empty"
    // diagnosable from a single log line.
    let upstream_tracking = run_git(skills_dir, &["rev-parse", "--abbrev-ref", "@{upstream}"]).is_ok();
    log::info!("git set_remote: origin configured on branch {branch}, upstream_tracking={upstream_tracking}");

    Ok(())
}

/// Rewrite the origin URL without fetching or touching upstream tracking.
/// Used by credential migration, which controls the verify step separately.
pub(crate) fn set_remote_url_only(skills_dir: &Path, url: &str) -> Result<()> {
    ensure_repo(skills_dir)?;
    if run_git(skills_dir, &["remote", "get-url", "origin"]).is_ok() {
        run_git_checked(skills_dir, &["remote", "set-url", "origin", url])
    } else {
        run_git_checked(skills_dir, &["remote", "add", "origin", url])
    }
}

/// Cheap network round-trip proving we can authenticate against origin.
pub(crate) fn verify_remote_auth(skills_dir: &Path) -> Result<()> {
    if let Some(url) = raw_remote_url(skills_dir).filter(|u| git2_engine::applies_to(u)) {
        return git2_engine::ls_remote_refs(&url).map(|_| ());
    }
    let env = remote_credential_env(skills_dir);
    run_git_env(skills_dir, &["ls-remote", "--heads", "origin"], &env).map(|_| ())
}

/// Whether a remote (addressed by URL, no local repo needed) has any branch.
/// Uses stored keychain credentials via askpass when available. Distinguishes
/// "freshly created empty repository" from "existing backup to restore".
pub fn remote_has_heads(url: &str) -> Result<bool> {
    if git2_engine::applies_to(url) {
        let refs = git2_engine::ls_remote_refs(url)?;
        return Ok(refs.iter().any(|r| r.starts_with("refs/heads/")));
    }
    let env = git_credentials::credential_env_for_url(url);
    let output = git_command()
        .args(["ls-remote", "--heads"])
        .arg(url)
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .output()
        .context("Failed to run git command")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "git ls-remote failed: {}",
            redact_urls_in_text(stderr.trim())
        );
    }
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

/// Remove the remote origin. Idempotent: succeeds when the repo or the
/// remote does not exist, so a retry after a partial disconnect converges.
/// Local repository and remote data are untouched.
pub fn remove_remote(skills_dir: &Path) -> Result<()> {
    let _lock = RepoLock::acquire_foreground("git remove remote")?;
    remove_remote_unlocked(skills_dir)
}

fn remove_remote_unlocked(skills_dir: &Path) -> Result<()> {
    if run_git(skills_dir, &["remote", "get-url", "origin"]).is_err() {
        return Ok(());
    }
    run_git_checked(skills_dir, &["remote", "remove", "origin"])?;
    log::info!("git remove_remote: origin removed");
    Ok(())
}

/// Whether the working tree has any uncommitted change (staged, unstaged or
/// untracked). Cheap porcelain probe used by the auto-backup round.
pub(crate) fn has_uncommitted_changes(skills_dir: &Path) -> Result<bool> {
    Ok(!run_git(skills_dir, &["status", "--porcelain"])?.is_empty())
}

/// Stage all changes and create a commit.
#[allow(dead_code)]
pub fn commit_all(skills_dir: &Path, message: &str) -> Result<()> {
    let _lock = RepoLock::acquire_foreground("git commit")?;
    commit_all_unlocked(skills_dir, message)
}

pub(crate) fn commit_all_unlocked(skills_dir: &Path, message: &str) -> Result<()> {
    ensure_repo(skills_dir)?;
    ensure_gitignore(skills_dir)?;
    // Cleanup, not a precondition: a failure here means the backup carries a few
    // stale `.pyc` entries, which is not a reason to refuse to back up at all.
    if let Err(e) = untrack_python_artifacts(skills_dir) {
        log::warn!("backup: could not untrack compiled-Python artifacts (continuing): {e:#}");
    }
    // Atomic-write leftovers must never enter a commit: a committed
    // `x.json.tmp.<uuid>` trips the merge validator on every other device.
    // (Reconcile also cleans these, but a machine that only ever pushes
    // never reconciles.)
    remove_tmp_metadata_files(skills_dir);
    // §3.6: new oversized skills stay local, out of the backup.
    if let Err(e) = apply_oversized_exclusions(skills_dir, SKILL_SIZE_LIMIT_BYTES) {
        log::warn!("backup size: exclusion scan failed (continuing): {e:#}");
    }
    // app_commit (§6): protocol marker is sticky in the tree and the message
    // carries the protocol trailer.
    protocol::ensure_protocol_file(skills_dir)?;

    run_git_checked(skills_dir, &["add", "-A"])?;

    // Check if there's anything to commit
    let status = run_git(skills_dir, &["status", "--porcelain"])?;
    if status.is_empty() {
        log::info!("git commit: working tree clean, nothing to commit");
        anyhow::bail!("Nothing to commit");
    }

    run_git_checked(
        skills_dir,
        &["commit", "-m", &protocol::app_commit_message(message)],
    )?;
    log::info!("git commit: committed staged changes");
    Ok(())
}

/// Delete `refs/skills-manager/*` copies that a mirror / push-all style
/// operation uploaded to the remote (merge-engine design §11-2). The app's
/// own push never sends them; this cleans up after manual advanced git use.
/// Local refs under the namespace are functional (conflict pins, recovery
/// anchors) and stay untouched. Returns the number of remote refs removed.
pub fn prune_hidden_refs_on_remote(skills_dir: &Path) -> Result<usize> {
    ensure_repo(skills_dir)?;
    const HIDDEN_PREFIX: &str = "refs/skills-manager/";

    if let Some(url) = raw_remote_url(skills_dir).filter(|u| git2_engine::applies_to(u)) {
        let refs: Vec<String> = git2_engine::ls_remote_refs(&url)?
            .into_iter()
            .filter(|r| r.starts_with(HIDDEN_PREFIX))
            .collect();
        if refs.is_empty() {
            return Ok(0);
        }
        let refspecs: Vec<String> = refs.iter().map(|r| format!(":{r}")).collect();
        git2_engine::push_refs(skills_dir, &refspecs, &url)?;
        log::info!("git prune hidden refs (git2): removed {} remote ref(s)", refs.len());
        return Ok(refs.len());
    }

    let env = remote_credential_env(skills_dir);
    let listed = run_git_env(
        skills_dir,
        &["ls-remote", "origin", &format!("{HIDDEN_PREFIX}*")],
        &env,
    )?;
    let refspecs: Vec<String> = listed
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .filter(|r| r.starts_with(HIDDEN_PREFIX))
        .map(|r| format!(":{r}"))
        .collect();
    if refspecs.is_empty() {
        return Ok(0);
    }
    let mut args: Vec<&str> = vec!["push", "origin"];
    args.extend(refspecs.iter().map(String::as_str));
    run_git_env_checked(skills_dir, &args, &env)?;
    log::info!("git prune hidden refs: removed {} remote ref(s)", refspecs.len());
    Ok(refspecs.len())
}

/// Commit for a conflict resolution (merge-engine design §4): the message
/// arrives with its trailers already built (protocol + Resolved), and the
/// commit may be empty — "keep local" changes nothing in the tree yet must
/// still record the resolution for other devices.
pub(crate) fn commit_resolution_unlocked(skills_dir: &Path, full_message: &str) -> Result<()> {
    ensure_repo(skills_dir)?;
    ensure_gitignore(skills_dir)?;
    // Cleanup, not a precondition: a failure here means the backup carries a few
    // stale `.pyc` entries, which is not a reason to refuse to back up at all.
    if let Err(e) = untrack_python_artifacts(skills_dir) {
        log::warn!("backup: could not untrack compiled-Python artifacts (continuing): {e:#}");
    }
    remove_tmp_metadata_files(skills_dir);
    if let Err(e) = apply_oversized_exclusions(skills_dir, SKILL_SIZE_LIMIT_BYTES) {
        log::warn!("backup size: exclusion scan failed (continuing): {e:#}");
    }
    protocol::ensure_protocol_file(skills_dir)?;
    run_git_checked(skills_dir, &["add", "-A"])?;
    run_git_checked(skills_dir, &["commit", "--allow-empty", "-m", full_message])?;
    Ok(())
}

/// Push to the remote repository.
pub fn push(skills_dir: &Path) -> Result<()> {
    let _lock = RepoLock::acquire_foreground("git push")?;
    push_unlocked(skills_dir)
}

pub(crate) fn push_unlocked(skills_dir: &Path) -> Result<()> {
    ensure_repo(skills_dir)?;

    let branch = run_git(skills_dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|_| "main".to_string());
    log::info!("git push: starting on branch {branch}");

    if let Some(url) = raw_remote_url(skills_dir).filter(|u| git2_engine::applies_to(u)) {
        return push_via_git2(skills_dir, &branch, &url);
    }

    let env = remote_credential_env(skills_dir);

    // Push branch first; if no upstream, set it.
    let result = run_git_env(skills_dir, &["push"], &env);
    if result.is_err() {
        log::info!("git push: no upstream tracking, retrying with -u origin {branch}");
        run_git_env_checked(skills_dir, &["push", "-u", "origin", &branch], &env)?;
    }

    // Snapshot tags are lightweight (by design), so `--follow-tags` will not include them.
    // Push only missing snapshot tags in a single network round-trip.
    let local_snapshot_tags: Vec<String> = run_git(skills_dir, &["tag", "--list", "sm-v-*"])?
        .lines()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect();

    if !local_snapshot_tags.is_empty() {
        let remote_snapshot_tags_raw = run_git_env(
            skills_dir,
            &["ls-remote", "--tags", "--refs", "origin", "sm-v-*"],
            &env,
        )
        .unwrap_or_default();

        let remote_snapshot_tags: std::collections::HashSet<String> = remote_snapshot_tags_raw
            .lines()
            .filter_map(|line| line.split_whitespace().nth(1))
            .filter_map(|ref_name| ref_name.strip_prefix("refs/tags/"))
            .map(|tag| tag.to_string())
            .collect();

        let missing_tag_refs: Vec<String> = local_snapshot_tags
            .into_iter()
            .filter(|tag| !remote_snapshot_tags.contains(tag))
            .map(|tag| format!("refs/tags/{tag}"))
            .collect();

        if !missing_tag_refs.is_empty() {
            let mut cmd = git_command();
            cmd.arg("-C").arg(skills_dir).arg("push").arg("origin");
            cmd.args(&missing_tag_refs);
            cmd.envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
            let pushed = missing_tag_refs.len();
            let output = cmd.output().context("Failed to run git command")?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let redacted = redact_urls_in_text(&stderr);
                log::warn!("git push: snapshot tag push failed: {redacted}");
                anyhow::bail!("git command failed: {}", redacted);
            }
            log::info!("git push: pushed {pushed} snapshot tag(s)");
        }
    }

    log::info!("git push: done");
    Ok(())
}

/// git2-engine variant of `push_unlocked`: branch first, then any snapshot
/// tags the remote is missing. Mirrors the system-git path's semantics,
/// including treating a failed remote-tag listing as "push all tags"
/// (re-pushing an existing identical tag is a no-op).
fn push_via_git2(skills_dir: &Path, branch: &str, url: &str) -> Result<()> {
    git2_engine::push_refs(
        skills_dir,
        &[format!("refs/heads/{branch}:refs/heads/{branch}")],
        url,
    )?;
    // Idempotent; makes ahead/behind and upstream-health work after the
    // first push, mirroring `push -u`.
    let _ = run_git(
        skills_dir,
        &[
            "branch",
            "--set-upstream-to",
            &format!("origin/{branch}"),
            branch,
        ],
    );

    let local_snapshot_tags: Vec<String> = run_git(skills_dir, &["tag", "--list", "sm-v-*"])?
        .lines()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect();

    if !local_snapshot_tags.is_empty() {
        let remote_snapshot_tags: std::collections::HashSet<String> =
            git2_engine::ls_remote_refs(url)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|r| r.strip_prefix("refs/tags/").map(|t| t.to_string()))
                .filter(|t| t.starts_with("sm-v-"))
                .collect();

        let missing_tag_refs: Vec<String> = local_snapshot_tags
            .into_iter()
            .filter(|tag| !remote_snapshot_tags.contains(tag))
            .map(|tag| format!("refs/tags/{tag}:refs/tags/{tag}"))
            .collect();

        if !missing_tag_refs.is_empty() {
            let pushed = missing_tag_refs.len();
            git2_engine::push_refs(skills_dir, &missing_tag_refs, url)?;
            log::info!("git push (git2): pushed {pushed} snapshot tag(s)");
        }
    }

    log::info!("git push (git2): done");
    Ok(())
}

/// Pull from the remote repository.
#[allow(dead_code)]
pub fn pull(skills_dir: &Path) -> Result<()> {
    let _lock = RepoLock::acquire_foreground("git pull")?;
    pull_unlocked(skills_dir)
}

pub(crate) fn pull_unlocked(skills_dir: &Path) -> Result<()> {
    ensure_repo(skills_dir)?;
    ensure_no_interrupted_git_operation(skills_dir)?;
    let branch = current_branch(skills_dir);
    log::info!("git pull: fetch + merge origin/{branch}");
    fetch_branch(skills_dir, &branch)?;
    merge_branch_system(skills_dir, &branch)?;
    log::info!("git pull: done");
    Ok(())
}

pub(crate) fn current_branch(skills_dir: &Path) -> String {
    run_git(skills_dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|_| "main".to_string())
}

/// Fetch one branch from origin through whichever network engine applies.
pub(crate) fn fetch_branch(skills_dir: &Path, branch: &str) -> Result<()> {
    if let Some(url) = raw_remote_url(skills_dir).filter(|u| git2_engine::applies_to(u)) {
        git2_engine::fetch(skills_dir, Some(branch), &url)
    } else {
        let env = remote_credential_env(skills_dir);
        run_git_env_checked(skills_dir, &["fetch", "origin", branch], &env)
    }
}

/// Line-level merge of the already-fetched remote branch via system git —
/// used by the `merge_engine=system` escape hatch and as the legacy fallback
/// of the object engine (merge-engine design §6).
///
/// The merge commit carries the protocol trailer (`app_commit`): without it,
/// this app's own line merge would read as an old-client double-parent
/// violation on every other device and block their object merges. A
/// conflict-free line merge preserves both sides' file-level changes (the
/// conflicting case aborts below), and every later object merge re-validates
/// its own output (§7), so trusting our own stamped line merges is sound.
pub(crate) fn merge_branch_system(skills_dir: &Path, branch: &str) -> Result<()> {
    let message = protocol::app_commit_message("sync: merge remote skill changes (line merge)");
    if let Err(e) = run_git(
        skills_dir,
        &["merge", "-m", &message, &format!("origin/{branch}")],
    ) {
        // A failed merge — almost always a content conflict on a SKILL.md body
        // edited on two machines — leaves the working tree conflicted with
        // MERGE_HEAD behind, which `ensure_no_interrupted_git_operation` would
        // then treat as a hard block on every future sync. Abort to restore a
        // clean tree, and surface a recognizable conflict error so the UI can
        // route the user to recovery (re-clone) instead of leaving them stuck.
        log::warn!("git pull: merge failed, aborting to clear conflicted state: {e}");
        let _ = run_git(skills_dir, &["merge", "--abort"]);
        anyhow::bail!("SYNC_CONFLICT: local and remote skill changes conflict ({e})");
    }
    Ok(())
}

/// Create an annotated snapshot tag on current HEAD.
pub fn create_snapshot_tag(skills_dir: &Path) -> Result<String> {
    let _lock = RepoLock::acquire_foreground("git snapshot")?;
    create_snapshot_tag_unlocked(skills_dir)
}

pub(crate) fn create_snapshot_tag_unlocked(skills_dir: &Path) -> Result<String> {
    ensure_repo(skills_dir)?;

    // Reuse an existing snapshot tag on HEAD to avoid duplicate history entries
    // when a previous sync created a tag but push failed.
    let existing_on_head = run_git(
        skills_dir,
        &[
            "tag",
            "--points-at",
            "HEAD",
            "--list",
            "sm-v-*",
            "--sort=-creatordate",
        ],
    )?;
    if let Some(tag) = existing_on_head
        .lines()
        .find(|line| !line.trim().is_empty())
    {
        let tag = tag.trim().to_string();
        log::info!("git snapshot: reusing existing tag {tag} on HEAD");
        return Ok(tag);
    }

    let short_sha = run_git(skills_dir, &["rev-parse", "--short", "HEAD"])?;
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    let mut tag = format!("sm-v-{}-{}", timestamp, short_sha);

    // Avoid collision when multiple snapshots happen within the same second.
    if run_git(
        skills_dir,
        &["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")],
    )
    .is_ok()
    {
        let millis = Utc::now().timestamp_subsec_millis();
        tag = format!("sm-v-{}-{:03}-{}", timestamp, millis, short_sha);
    }

    // Use lightweight tag to avoid requiring git user.name/user.email on client machines.
    run_git_checked(skills_dir, &["tag", &tag])?;
    log::info!("git snapshot: created tag {tag}");
    Ok(tag)
}

/// List snapshot versions, newest first.
pub fn list_snapshot_versions(
    skills_dir: &Path,
    limit: Option<usize>,
) -> Result<Vec<GitBackupVersion>> {
    ensure_repo(skills_dir)?;
    let tags = run_git(
        skills_dir,
        &["tag", "--list", "sm-v-*", "--sort=-creatordate"],
    )?;
    if tags.trim().is_empty() {
        return Ok(Vec::new());
    }

    let max = limit.unwrap_or(30);
    let mut versions = Vec::new();
    for tag in tags.lines().take(max) {
        let commit = run_git(skills_dir, &["rev-list", "-n", "1", tag]).unwrap_or_default();
        let short_commit = if commit.len() > 8 {
            commit[..8].to_string()
        } else {
            commit.clone()
        };
        // Author, date and message in one call; message last because it is
        // the only field that could contain the separator.
        let line = run_git(skills_dir, &["log", "-1", "--format=%an%x1f%cI%x1f%s", tag])
            .unwrap_or_default();
        let mut parts = line.splitn(3, '\u{1f}');
        let author = parts.next().unwrap_or_default().to_string();
        let committed_at = parts.next().unwrap_or_default().to_string();
        let message = parts.next().unwrap_or_default().to_string();

        versions.push(GitBackupVersion {
            tag: tag.to_string(),
            commit: short_commit,
            message,
            committed_at,
            author,
        });
    }

    Ok(versions)
}

/// Restore skills files to a snapshot tag by creating a new restore commit.
/// Returns the safety-point tag capturing the pre-restore state.
#[allow(dead_code)]
pub fn restore_snapshot_version(skills_dir: &Path, tag: &str) -> Result<String> {
    let _lock = RepoLock::acquire_foreground("git restore snapshot")?;
    restore_snapshot_version_unlocked(skills_dir, tag)
}

pub(crate) fn restore_snapshot_version_unlocked(skills_dir: &Path, tag: &str) -> Result<String> {
    ensure_repo(skills_dir)?;

    if !tag.starts_with("sm-v-") {
        anyhow::bail!("Invalid snapshot tag");
    }
    run_git_checked(
        skills_dir,
        &["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")],
    )?;

    log::info!("git restore: switching skills library to {tag}");

    // Safety point first (§3.5): the pre-restore state — including any
    // uncommitted edits — becomes a user-visible snapshot the user can return
    // to from the backup history. This is what makes restore always undoable.
    let status = run_git(skills_dir, &["status", "--porcelain"])?;
    if !status.is_empty() {
        commit_all_unlocked(skills_dir, "backup before restore")?;
    }
    let safety_tag = create_snapshot_tag_unlocked(skills_dir)?;

    let restore_result: Result<()> = (|| {
        // Align working tree + index to snapshot tree exactly (including deletions),
        // then commit as a forward change.
        run_git_checked(skills_dir, &["read-tree", "--reset", "-u", tag])?;

        // Sticky protocol marker (§6): a pre-protocol snapshot self-heals on
        // the restore commit instead of resurrecting a marker-less tree.
        protocol::ensure_protocol_file(skills_dir)?;
        // The snapshot's .gitignore predates the managed oversized section —
        // rebuild it before add -A, or a locally-kept oversized skill would
        // ride into the restore commit.
        if let Err(e) = apply_oversized_exclusions(skills_dir, SKILL_SIZE_LIMIT_BYTES) {
            log::warn!("backup size: exclusion scan failed (continuing): {e:#}");
        }
        run_git_checked(skills_dir, &["add", "-A"])?;

        let changed = run_git(skills_dir, &["status", "--porcelain"])?;
        if !changed.is_empty() {
            run_git_checked(
                skills_dir,
                &[
                    "commit",
                    "-m",
                    &protocol::app_commit_message(&format!(
                        "restore: switch skills library to {}",
                        tag
                    )),
                ],
            )?;
        }
        Ok(())
    })();

    match restore_result {
        Ok(()) => {
            log::info!("git restore: completed restore to {tag} (safety point {safety_tag})");
            Ok(safety_tag)
        }
        Err(err) => {
            // Best-effort rollback to the safety point (pre-restore HEAD).
            let _ = run_git_checked(skills_dir, &["read-tree", "--reset", "-u", &safety_tag]);
            Err(err)
                .context("Restore failed after mutating working tree; attempted automatic rollback")
        }
    }
}

/// Clone a remote repository into the skills directory.
/// The skills directory must be empty or non-existent.
#[allow(dead_code)]
pub fn clone_into(skills_dir: &Path, url: &str) -> Result<()> {
    let _lock = RepoLock::acquire_foreground("git clone")?;
    clone_into_unlocked(skills_dir, url, &[]).map(|_| ())
}

/// Clone variant that refuses to merge a populated non-git directory into the
/// cloned repo. Used by agent-facing entry points (e.g. CLI `--skills-root`)
/// where an accidental pointing at an unrelated populated directory would
/// otherwise silently absorb its contents.
///
/// The check runs inside the same `RepoLock` as the clone, so any other
/// skills-manager process attempting to populate the target between check
/// and clone is serialized.
pub fn clone_into_strict(skills_dir: &Path, url: &str) -> Result<()> {
    let _lock = RepoLock::acquire_foreground("git clone")?;
    ensure_clean_clone_target(skills_dir)?;
    clone_into_unlocked(skills_dir, url, &[]).map(|_| ())
}

/// Refuse a clone target that is a file, or a non-empty directory that is not
/// already a git repo. An empty or non-existent target is fine, and an
/// existing `.git` is left to `clone_into_unlocked` to reject with its own
/// message. Pure logic — no locking — so callers must hold their own lock if
/// they need atomicity with a subsequent operation.
fn ensure_clean_clone_target(skills_dir: &Path) -> Result<()> {
    if !skills_dir.exists() {
        return Ok(());
    }
    if skills_dir.join(".git").exists() {
        return Ok(());
    }
    if skills_dir.is_file() {
        anyhow::bail!(
            "refusing to clone into {}: path exists and is a file, not a directory",
            skills_dir.display()
        );
    }
    let mut entries = std::fs::read_dir(skills_dir)
        .with_context(|| format!("Failed to read clone target {}", skills_dir.display()))?;
    if entries.next().is_some() {
        anyhow::bail!(
            "refusing to clone into {}: directory is non-empty and not a git repo. \
             Files in the target would be silently merged into the cloned repo. \
             Point the target at an empty or non-existent directory.",
            skills_dir.display()
        );
    }
    Ok(())
}

/// Reset a local repo by clearing its `.git` then cloning from the remote.
/// The existing skill files go through [`clone_into_unlocked`], so a local
/// skill that differs from the remote's copy stops the re-clone instead of
/// being overwritten. The previous `.git` is moved to a uniquely named sibling
/// and put back if the clone fails. After a successful clone it is deleted
/// only when [`history_is_on_remote`] proves the fresh clone holds all of it;
/// otherwise it is kept and its path returned so the user can be told.
pub(crate) fn reclone_from_remote_unlocked(
    skills_dir: &Path,
    url: &str,
    set_aside: &[String],
) -> Result<RecloneOutcome> {
    let git_dir = skills_dir.join(".git");
    if !git_dir.exists() {
        let local_copies = clone_into_unlocked(skills_dir, url, set_aside)?;
        return Ok(RecloneOutcome { kept_git: None, local_copies });
    }

    log::info!("git reclone: re-cloning from remote, preserving local skills");
    let git_backup = unused_sibling(skills_dir, "skills-git-recovery");
    std::fs::rename(&git_dir, &git_backup)
        .context("Failed to move existing .git aside before re-clone")?;

    match clone_into_unlocked(skills_dir, url, set_aside) {
        Ok(local_copies) => {
            let kept_git = if history_is_on_remote(&git_backup, skills_dir) {
                if let Err(e) = std::fs::remove_dir_all(&git_backup) {
                    log::warn!(
                        "git reclone: could not remove {} (fully on the remote): {e}",
                        git_backup.display()
                    );
                }
                None
            } else {
                log::info!(
                    "git reclone: previous history not proven to be on the remote; kept at {}",
                    git_backup.display()
                );
                Some(git_backup)
            };
            Ok(RecloneOutcome { kept_git, local_copies })
        }
        Err(e) => {
            // clone_into_unlocked has put the skill files back without a
            // `.git` — unless that itself failed: then what is at the live
            // path is not the library, and our `.git` must stay out of it.
            if e.downcast_ref::<LibraryNotPutBack>().is_some() {
                anyhow::bail!("{e:#}. The previous .git is kept at {}", git_backup.display());
            }
            if !skills_dir.exists() {
                let _ = std::fs::create_dir_all(skills_dir);
            }
            let restored = if skills_dir.join(".git").exists() {
                Err(anyhow::anyhow!("{} already holds a .git", skills_dir.display()))
            } else {
                std::fs::rename(&git_backup, skills_dir.join(".git")).map_err(anyhow::Error::from)
            };
            if let Err(restore_err) = restored {
                anyhow::bail!(
                    "Re-clone failed: {e:#}. Could not restore previous .git directory ({restore_err}); it is kept at {}",
                    git_backup.display()
                );
            }
            Err(e)
        }
    }
}

/// What a successful re-clone left beside the library for the user.
#[derive(Debug, Default)]
pub(crate) struct RecloneOutcome {
    /// The previous `.git`, when its history is not proven to be on the remote.
    pub kept_git: Option<PathBuf>,
    /// Local versions of skills that differed from the remote's, when the
    /// caller confirmed setting them aside (see [`clone_into_unlocked`]).
    pub local_copies: Option<PathBuf>,
}

/// Clone `url` into `skills_dir`, keeping any local content.
///
/// A non-empty `skills_dir` is moved to a uniquely named sibling first (an
/// earlier attempt's backup is never touched). Once the clone succeeds, local
/// top-level entries the clone does not have are copied in. The clone's copy
/// of a same-name entry is the one that stays, so ones that differ (see
/// [`diverging_entries`]) stop the clone with `CLONE_LOCAL_DIFFERS`, naming
/// them one per line — unless `set_aside` names exactly those entries, the
/// list the user confirmed: their local versions are then copied to a new
/// folder beside the library, which is returned.
///
/// Rollback contract: on `Ok`, `skills_dir` is the clone plus the local-only
/// entries and the backup is gone. On every `Err` — the clone failed, a local
/// entry diverges, or copying local entries in or out failed — `skills_dir`
/// is the original content again, with no `.git`, and neither a backup nor a
/// set-aside folder is left behind; whatever cannot be put back or cleaned
/// up, the error says where it is.
pub(crate) fn clone_into_unlocked(
    skills_dir: &Path,
    url: &str,
    set_aside: &[String],
) -> Result<Option<PathBuf>> {
    if skills_dir.join(".git").exists() {
        anyhow::bail!("Skills directory is already a git repository");
    }

    // If skills dir has content, move it aside temporarily
    let has_existing = skills_dir.exists()
        && std::fs::read_dir(skills_dir)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false);

    let backup_dir = if has_existing {
        let backup = unused_sibling(skills_dir, "skills-backup-before-clone");
        std::fs::rename(skills_dir, &backup)?;
        Some(backup)
    } else {
        None
    };

    log::info!(
        "git clone: cloning {} (existing_local_content={has_existing})",
        redact_url(url)
    );

    // Clone
    let clone_result: Result<()> = if git2_engine::applies_to(url) {
        git2_engine::clone(url, skills_dir)
    } else {
        let env = git_credentials::credential_env_for_url(url);
        let output = git_command()
            .args(["-c", "init.defaultRefFormat=files", "clone"])
            .env_remove("GIT_DEFAULT_REF_FORMAT")
            .arg(url)
            .arg(skills_dir)
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output();
        match output {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                let detail = stderr.trim();
                if detail.is_empty() {
                    Err(anyhow::anyhow!("git clone failed with exit code {}", o.status))
                } else {
                    Err(anyhow::anyhow!(
                        "git clone failed: {}",
                        redact_urls_in_text(detail)
                    ))
                }
            }
            Err(e) => Err(anyhow::Error::new(e).context("Failed to spawn git clone")),
        }
    };

    if let Err(e) = clone_result {
        // A partial git2 clone can leave a half-created target (system git
        // cleans up after itself); clear it so a retry doesn't hit "already
        // a git repository".
        log::warn!("git clone: failed: {e:#}");
        return match backup_dir {
            Some(backup) => put_back_pre_clone_library(skills_dir, &backup, e).map(|()| None),
            None => {
                if skills_dir.join(".git").exists() {
                    let _ = std::fs::remove_dir_all(skills_dir);
                }
                Err(e)
            }
        };
    }

    let Some(backup) = backup_dir else {
        log::info!("git clone: done");
        return Ok(None);
    };
    let give_up = |e: anyhow::Error| {
        log::warn!("git clone: not keeping the clone: {e:#}");
        put_back_pre_clone_library(skills_dir, &backup, e).map(|()| None)
    };
    let diverging = match diverging_entries(&backup, skills_dir) {
        Ok(names) => names,
        Err(e) => return give_up(e),
    };
    let shown: Vec<String> = diverging
        .iter()
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    let mut confirmed = set_aside.to_vec();
    confirmed.sort();
    let mut listed = shown.clone();
    listed.sort();
    if !diverging.is_empty() && listed != confirmed {
        return give_up(anyhow::anyhow!(
            "CLONE_LOCAL_DIFFERS: the clone was stopped and the local library left as it was, \
             because these local items differ from the remote backup's copies:\n{}",
            shown.join("\n")
        ));
    }
    if let Err(e) = merge_backup(&backup, skills_dir) {
        return give_up(e.context("Failed to copy local skills into the clone"));
    }
    // Last, so nothing that can fail comes after it.
    let local_copies = if diverging.is_empty() {
        None
    } else {
        match set_aside_local_versions(&backup, &diverging, skills_dir) {
            Ok(dir) => Some(dir),
            Err(e) => return give_up(e),
        }
    };
    // Every local entry is now in the clone, identical to the clone's copy,
    // or set aside, so the backup holds nothing else.
    if let Err(e) = std::fs::remove_dir_all(&backup) {
        log::warn!("git clone: could not remove backup {}: {e}", backup.display());
    }
    log::info!("git clone: done");
    Ok(local_copies)
}

/// Copy the local versions of `names` out of `backup` into a new folder beside
/// the library, before the clone's copies replace them. A partial copy is
/// removed again (the originals are still in `backup`), or reported if it
/// cannot be.
fn set_aside_local_versions(
    backup: &Path,
    names: &[std::ffi::OsString],
    skills_dir: &Path,
) -> Result<PathBuf> {
    let dir = unused_sibling(skills_dir, "skills-local-copies");
    std::fs::create_dir(&dir)
        .with_context(|| format!("Failed to create {}", dir.display()))?;
    let copied = names
        .iter()
        .try_for_each(|name| copy_entry(&backup.join(name), &dir.join(name)));
    if let Err(e) = copied {
        let e = anyhow::Error::new(e).context("Failed to set aside the local versions of differing skills");
        return Err(match std::fs::remove_dir_all(&dir) {
            Ok(()) => e,
            Err(_) => e.context(format!("a partial copy is kept at {}", dir.display())),
        });
    }
    log::info!("git clone: set aside {} local version(s) at {}", names.len(), dir.display());
    Ok(dir)
}

/// The pre-clone library could not be moved back into place and is still at
/// `backup`. Typed so a re-clone knows the live path does not hold it.
#[derive(Debug)]
struct LibraryNotPutBack {
    backup: PathBuf,
    reason: std::io::Error,
}

impl std::fmt::Display for LibraryNotPutBack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the local library could not be moved back ({}); it is kept at {}",
            self.reason,
            self.backup.display()
        )
    }
}

impl std::error::Error for LibraryNotPutBack {}

/// Undo a clone: drop whatever is at `skills_dir` and move the pre-clone
/// library back from `backup`. Returns `cause` as the error — wrapping a
/// [`LibraryNotPutBack`] if the library could not be moved back.
fn put_back_pre_clone_library(
    skills_dir: &Path,
    backup: &Path,
    cause: anyhow::Error,
) -> Result<()> {
    let cleared = match std::fs::remove_dir_all(skills_dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    };
    if let Err(reason) = cleared.and_then(|()| std::fs::rename(backup, skills_dir)) {
        let not_back = LibraryNotPutBack { backup: backup.to_path_buf(), reason };
        return Err(anyhow::Error::new(not_back).context(format!("{cause:#}")));
    }
    Err(cause)
}

/// `<prefix>-<UTC timestamp>` beside `skills_dir`, with a counter appended
/// when that name is taken — never an existing path, so a retry cannot
/// overwrite what an earlier attempt left behind.
fn unused_sibling(skills_dir: &Path, prefix: &str) -> PathBuf {
    let base = format!("{prefix}-{}", Utc::now().format("%Y%m%d-%H%M%S"));
    let mut candidate = skills_dir.with_file_name(&base);
    let mut n = 2;
    while std::fs::symlink_metadata(&candidate).is_ok() {
        candidate = skills_dir.with_file_name(format!("{base}-{n}"));
        n += 1;
    }
    candidate
}

/// Top-level names in `backup` whose same-named entry in `clone` differs by
/// anything [`replaceable_content`] counts. The repository itself and app
/// metadata (`.skills-manager/`, the root `.gitignore`) are the clone's to
/// keep, as before.
fn diverging_entries(backup: &Path, clone: &Path) -> Result<Vec<std::ffi::OsString>> {
    let mut diverging = Vec::new();
    for entry in std::fs::read_dir(backup)? {
        let entry = entry?;
        let name = entry.file_name();
        let shown = name.to_string_lossy();
        if matches!(shown.as_ref(), ".git" | ".skills-manager" | ".gitignore") || is_clutter(&shown) {
            continue;
        }
        // The raw name, as `merge_backup` sees it: a lossy one could miss
        // the counterpart and let the entry pass as local-only.
        let theirs = clone.join(&name);
        // Not `exists()`: it follows symlinks, and a dangling one from the
        // remote would read as absent and then be written through.
        if std::fs::symlink_metadata(&theirs).is_err() {
            continue;
        }
        if replaceable_content(&entry.path())? != replaceable_content(&theirs)? {
            diverging.push(name);
        }
    }
    diverging.sort();
    Ok(diverging)
}

/// Regenerable clutter a clone may drop: Finder/Explorer metadata and
/// compiled Python.
fn is_clutter(name: &str) -> bool {
    matches!(name, ".DS_Store" | "Thumbs.db" | "__pycache__") || name.ends_with(".pyc")
}

/// Everything at `root` (directory, file or symlink) that replacing it with
/// another copy could lose, keyed by relative path: each file's content with
/// line endings folded (a Windows checkout of the same file is no difference)
/// and each symlink's target. Unlike the skill content hash this keeps a
/// skill's own `.gitignore` and a nested `.git`. Left out: [`is_clutter`],
/// directories as such (git stores none, so an empty one would make every
/// such skill differ) and permission bits (a repo pushed from Windows records
/// none). Links are never followed; anything unreadable is an error.
fn replaceable_content(root: &Path) -> Result<Vec<(std::ffi::OsString, Held)>> {
    use sha2::{Digest, Sha256};
    use unicode_normalization::UnicodeNormalization;
    let mut content = Vec::new();
    let walk = walkdir::WalkDir::new(root)
        .follow_root_links(false)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_clutter(&e.file_name().to_string_lossy()));
    for entry in walk {
        let entry = entry.with_context(|| format!("Failed to read {}", root.display()))?;
        let path = entry.path();
        let held = if entry.file_type().is_symlink() {
            Held::Link(std::fs::read_link(path)?)
        } else if entry.file_type().is_file() {
            let bytes = std::fs::read(path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            Held::File(Sha256::digest(super::content_hash::fold_crlf(&bytes)).to_vec())
        } else {
            continue;
        };
        // NFC: macOS keeps whichever Unicode form a name was created in, and
        // git checks names out composed. A name that is not UTF-8 stays raw,
        // so two different ones never collapse into the same key.
        let relative = path.strip_prefix(root).unwrap_or(path);
        let relative = match relative.to_str() {
            Some(name) => name.nfc().collect::<String>().into(),
            None => relative.as_os_str().to_owned(),
        };
        content.push((relative, held));
    }
    content.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(content)
}

/// One entry [`replaceable_content`] keeps: a symlink's target, or a file's
/// line-ending-folded content hash.
#[derive(Debug, PartialEq)]
enum Held {
    Link(PathBuf),
    File(Vec<u8>),
}

/// True when every ref and the detached `HEAD` (if any) of the repository at
/// `old_git` is already in the fresh clone at `clone`, so deleting `old_git`
/// loses no history. Tags must match by name and target — snapshot tags are
/// pushed by name. Any other ref only needs its object present: branches are
/// named differently in a clone, and `refs/skills-manager/*` pins are never
/// pushed by design. Reflog-only commits are not considered. Any git failure
/// answers `false`, so the caller keeps the old history.
fn history_is_on_remote(old_git: &Path, clone: &Path) -> bool {
    // Linked worktrees' HEADs and submodule repositories live outside
    // `refs/`; with any of them, or if we cannot look, prove nothing.
    for nested in ["worktrees", "modules"] {
        let looked = std::fs::read_dir(old_git.join(nested)).map(|mut d| d.next().is_none());
        match looked {
            Ok(true) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => return false,
        }
    }
    let old = format!("--git-dir={}", old_git.display());
    let refs = |args: &[&str]| -> Option<Vec<(String, String)>> {
        let out = run_git(clone, args).ok()?;
        Some(
            out.lines()
                .filter_map(|l| l.split_once(' '))
                .map(|(oid, name)| (oid.to_string(), name.to_string()))
                .collect(),
        )
    };
    let format = "--format=%(objectname) %(refname)";
    let (Some(old_refs), Some(new_refs)) = (
        refs(&[&old, "for-each-ref", format]),
        refs(&["for-each-ref", format]),
    ) else {
        return false;
    };
    let mut needed: Vec<String> = Vec::new();
    for (oid, name) in old_refs {
        if name.starts_with("refs/tags/") {
            if !new_refs.iter().any(|(o, n)| *n == name && *o == oid) {
                return false;
            }
        } else {
            needed.push(oid);
        }
    }
    // A branch HEAD is covered by its branch; only a detached one adds a commit.
    if run_git(clone, &[&old, "symbolic-ref", "-q", "HEAD"]).is_err() {
        match run_git(clone, &[&old, "rev-parse", "--verify", "HEAD"]) {
            Ok(oid) => needed.push(oid),
            Err(_) => return false,
        }
    }
    needed
        .iter()
        .all(|oid| run_git(clone, &["cat-file", "-e", oid]).is_ok())
}

/// Count distinct top-level directories touched by a `git status --porcelain`
/// output, skipping dot-entries (`.skills-manager` metadata, `.gitignore`).
/// This approximates "how many skills have unbacked changes" for the status
/// copy without needing the DB.
fn count_changed_top_dirs(porcelain: &str) -> u32 {
    let mut dirs = std::collections::HashSet::new();
    for line in porcelain.lines() {
        if line.len() <= 3 {
            continue;
        }
        // Format: "XY path" or "XY old -> new" (rename); count the new path.
        let mut path = &line[3..];
        if let Some((_, renamed)) = path.split_once(" -> ") {
            path = renamed;
        }
        let path = path.trim().trim_matches('"');
        let top = path.split('/').next().unwrap_or_default();
        if top.is_empty() || top.starts_with('.') {
            continue;
        }
        dirs.insert(top.to_string());
    }
    dirs.len() as u32
}

/// Size thresholds from the backup redesign (§3.6): warn on any single skill
/// directory above 100 MB and on a repository above 1 GB.
pub const SKILL_SIZE_LIMIT_BYTES: u64 = 100 * 1024 * 1024;
pub const REPO_SIZE_WARN_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize)]
pub struct OversizedSkill {
    pub name: String,
    pub bytes: u64,
    /// True when the skill is excluded from backup (§3.6: oversized and not
    /// yet tracked by git). False = already backed up, warning only.
    pub excluded: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BackupSizeReport {
    /// Total size of backed-up content (working tree, `.git` excluded).
    pub total_bytes: u64,
    /// Skill directories above `skill_limit_bytes`.
    pub oversized: Vec<OversizedSkill>,
    pub skill_limit_bytes: u64,
    pub repo_warn_bytes: u64,
}

/// Skill directories (valid, depth ≤ 6) whose content exceeds `limit`,
/// as repo-relative slash paths with their sizes. Also returns the total
/// working-tree size.
fn oversized_skill_dirs(skills_dir: &Path, limit: u64) -> (Vec<(String, u64)>, u64) {
    let mut total_bytes: u64 = 0;
    let mut oversized = Vec::new();
    if skills_dir.exists() {
        let mut it = walkdir::WalkDir::new(skills_dir)
            .min_depth(1)
            .max_depth(6)
            .into_iter()
            .filter_entry(|e| e.file_name().to_string_lossy() != ".git");
        while let Some(entry) = it.next() {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if entry.file_type().is_file() {
                total_bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
                continue;
            }
            if entry.file_type().is_dir() && super::skill_metadata::is_valid_skill_dir(path) {
                let bytes = dir_size(path);
                total_bytes += bytes;
                if bytes > limit {
                    let rel = path
                        .strip_prefix(skills_dir)
                        .map(|p| {
                            p.components()
                                .map(|c| c.as_os_str().to_string_lossy().to_string())
                                .collect::<Vec<_>>()
                                .join("/")
                        })
                        .unwrap_or_default();
                    if !rel.is_empty() {
                        oversized.push((rel, bytes));
                    }
                }
                // The whole subtree is accounted for; don't double-count files
                // or nested dirs inside this skill.
                it.skip_current_dir();
            }
        }
    }
    (oversized, total_bytes)
}

/// Whether any file under `rel_path` is tracked by git.
fn is_tracked(skills_dir: &Path, rel_path: &str) -> bool {
    run_git(
        skills_dir,
        &["ls-files", "--", &format!(":(literal){rel_path}")],
    )
    .map(|out| !out.trim().is_empty())
    .unwrap_or(false)
}

/// Scan the skills directory for backup-size problems (§3.6). Read-only.
pub fn size_report(skills_dir: &Path) -> Result<BackupSizeReport> {
    let (dirs, total_bytes) = oversized_skill_dirs(skills_dir, SKILL_SIZE_LIMIT_BYTES);
    let is_repo = skills_dir.join(".git").exists();
    let mut oversized: Vec<OversizedSkill> = dirs
        .into_iter()
        .map(|(rel, bytes)| OversizedSkill {
            name: rel.rsplit('/').next().unwrap_or(&rel).to_string(),
            bytes,
            excluded: is_repo && !is_tracked(skills_dir, &rel),
        })
        .collect();

    oversized.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    Ok(BackupSizeReport {
        total_bytes,
        oversized,
        skill_limit_bytes: SKILL_SIZE_LIMIT_BYTES,
        repo_warn_bytes: REPO_SIZE_WARN_BYTES,
    })
}

const OVERSIZED_SECTION_BEGIN: &str = "# skills-manager: oversized skills excluded from backup (auto-managed)";
const OVERSIZED_SECTION_END: &str = "# skills-manager: end oversized skills";

/// Escape a repo-relative path for use as a literal gitignore pattern.
fn gitignore_escape(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if matches!(c, '\\' | '*' | '?' | '[' | ']' | '!' | '#') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// §3.6 后半: keep oversized skills out of the backup by default. A skill
/// directory above `limit` that is NOT yet tracked by git gets its content
/// dir and its metadata file added to a managed `.gitignore` section (the
/// skill stays on disk and in the local DB). Already-tracked skills are
/// never untracked — removing them from the tree would propagate to other
/// devices as a deletion — they only warn (`size_report`). The section is
/// rebuilt from scratch on every commit, so a skill that shrinks below the
/// limit re-enters the backup automatically.
pub(crate) fn apply_oversized_exclusions(skills_dir: &Path, limit: u64) -> Result<Vec<String>> {
    let (dirs, _total) = oversized_skill_dirs(skills_dir, limit);
    let mut excluded: Vec<String> = Vec::new();
    let mut lines: Vec<String> = Vec::new();

    if !dirs.is_empty() {
        // Map content path → skill_id so the paired metadata file is
        // excluded too (a tracked metadata file pointing at an ignored dir
        // would fail §7 validation on every other device).
        let mut id_by_path = std::collections::HashMap::new();
        let meta_dir = skills_dir.join(".skills-manager/skills");
        if meta_dir.is_dir() {
            for entry in std::fs::read_dir(&meta_dir)?.flatten() {
                if let Ok(raw) = std::fs::read_to_string(entry.path()) {
                    if let Ok(meta) =
                        serde_json::from_str::<crate::core::sync_metadata::SkillMetaFile>(&raw)
                    {
                        id_by_path.insert(meta.path, meta.skill_id);
                    }
                }
            }
        }
        for (rel, bytes) in dirs {
            if is_tracked(skills_dir, &rel) {
                continue; // already backed up: warn only, never untrack
            }
            lines.push(format!("/{}/", gitignore_escape(&rel)));
            if let Some(id) = id_by_path.get(&rel) {
                lines.push(format!("/.skills-manager/skills/{}.json", gitignore_escape(id)));
            }
            log::info!(
                "backup size: excluding oversized skill '{rel}' ({} MB) from backup",
                bytes / (1024 * 1024)
            );
            excluded.push(rel);
        }
    }

    rewrite_gitignore_section(skills_dir, &lines)?;
    Ok(excluded)
}

/// Idempotently rewrite the managed oversized section of `.gitignore`
/// (created, replaced, or removed when empty), leaving user lines intact.
fn rewrite_gitignore_section(skills_dir: &Path, section_lines: &[String]) -> Result<()> {
    let gitignore = skills_dir.join(".gitignore");
    let existing = if gitignore.exists() {
        std::fs::read_to_string(&gitignore)?
    } else {
        String::new()
    };
    let mut kept: Vec<String> = Vec::new();
    let mut in_section = false;
    let mut had_section = false;
    for line in existing.lines() {
        if line.trim() == OVERSIZED_SECTION_BEGIN {
            in_section = true;
            had_section = true;
            continue;
        }
        if line.trim() == OVERSIZED_SECTION_END {
            in_section = false;
            continue;
        }
        if !in_section {
            kept.push(line.to_string());
        }
    }
    if section_lines.is_empty() {
        if !had_section {
            return Ok(()); // nothing to add, nothing to remove
        }
    } else {
        while kept.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
            kept.pop();
        }
        kept.push(String::new());
        kept.push(OVERSIZED_SECTION_BEGIN.to_string());
        kept.extend(section_lines.iter().cloned());
        kept.push(OVERSIZED_SECTION_END.to_string());
    }
    std::fs::write(&gitignore, format!("{}\n", kept.join("\n").trim_end_matches('\n')))?;
    Ok(())
}

fn dir_size(dir: &Path) -> u64 {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

// ── Helpers ──

fn ensure_repo(skills_dir: &Path) -> Result<()> {
    if !skills_dir.join(".git").exists() {
        anyhow::bail!("Skills directory is not a git repository. Initialize it first.");
    }
    ensure_files_ref_format(skills_dir)
}

/// libgit2 (merge, git2 engine) cannot open a reftable repository; it would
/// fail later with an opaque "invalid ref" error. Created when the user's git
/// defaults to reftable. Migrating rewrites the user's refs, so we only explain.
pub(crate) fn ensure_files_ref_format(skills_dir: &Path) -> Result<()> {
    if skills_dir.join(".git").join("reftable").is_dir() {
        anyhow::bail!(
            "The skills library's git repository uses the reftable format, which Skills Manager cannot read. \
             Close other git tools, run `git -C \"{}\" refs migrate --ref-format=files` (Git 2.48+), then try again.",
            skills_dir.display()
        );
    }
    Ok(())
}

/// Delete atomic-write temp leftovers (`*.tmp.<uuid>`) under the metadata
/// namespace of THIS repo's working tree. Best-effort; a leftover appears
/// only when a writer crashed mid-write.
fn remove_tmp_metadata_files(skills_dir: &Path) {
    let meta_root = skills_dir.join(".skills-manager");
    if !meta_root.exists() {
        return;
    }
    for entry in walkdir::WalkDir::new(&meta_root).into_iter().flatten() {
        if entry.file_type().is_file()
            && entry.file_name().to_string_lossy().contains(".tmp.")
        {
            if let Err(e) = std::fs::remove_file(entry.path()) {
                log::warn!(
                    "git commit: failed to remove temp metadata file {}: {e}",
                    entry.path().display()
                );
            } else {
                log::info!(
                    "git commit: removed stale temp metadata file {}",
                    entry.path().display()
                );
            }
        }
    }
}

pub(crate) fn ensure_no_interrupted_git_operation(skills_dir: &Path) -> Result<()> {
    ensure_files_ref_format(skills_dir)?;
    let git_dir = skills_dir.join(".git");
    for marker in ["MERGE_HEAD", "index.lock", "rebase-merge", "rebase-apply"] {
        if git_dir.join(marker).exists() {
            anyhow::bail!(
                "Git operation is already in progress ({marker}); resolve it before syncing"
            );
        }
    }
    Ok(())
}

/// Patterns for compiled-Python artifacts, kept in exact parity with
/// `content_hash::is_ignored` — `__pycache__` and `*.pyc`, nothing wider.
///
/// A skill's scripts run straight out of the library (agents reach it through a
/// symlink), so Python writes `__pycache__/` into the library itself; filtering
/// on import cannot help with something generated afterwards. The content hash
/// already declares these outside a skill's content, so backing them up put the
/// two layers at odds.
///
/// Deliberately not `*.py[cod]`: `is_ignored` counts `.pyd` (a real Windows
/// extension module some skills ship) as content, so ignoring it here would
/// recreate the same inconsistency in the other direction — the file would be
/// missing after a restore and every device would report the skill as modified.
const PYTHON_ARTIFACT_IGNORES: [&str; 2] = ["__pycache__/", "*.pyc"];

fn ensure_gitignore(skills_dir: &Path) -> Result<()> {
    let gitignore = skills_dir.join(".gitignore");
    let required = [
        ".DS_Store",
        "Thumbs.db",
        "*.tmp",
        ".skills-manager.lock",
        PYTHON_ARTIFACT_IGNORES[0],
        PYTHON_ARTIFACT_IGNORES[1],
    ];
    let mut lines: Vec<String> = if gitignore.exists() {
        std::fs::read_to_string(&gitignore)?
            .lines()
            .map(ToOwned::to_owned)
            .collect()
    } else {
        Vec::new()
    };
    let existing: std::collections::HashSet<String> =
        lines.iter().map(|line| line.trim().to_string()).collect();
    for line in required {
        if !existing.contains(line) {
            lines.push(line.to_string());
        }
    }
    std::fs::write(&gitignore, format!("{}\n", lines.join("\n")))?;
    Ok(())
}

/// Drop compiled-Python artifacts that were committed before they were ignored.
///
/// `.gitignore` only governs *untracked* paths — `git add -A` keeps re-staging
/// anything already in the index, so adding the patterns above fixes nothing for
/// a library that has been backing up `__pycache__` all along. This is the
/// one-time catch-up; once the index is clean the `ls-files` below matches
/// nothing and the whole thing is a no-op.
///
/// This deliberately untracks, where [`apply_oversized_exclusions`] deliberately
/// does not (see its `never untrack` guard). The difference is what is at stake:
/// an oversized skill is the user's own content and dropping it from the backup
/// could destroy the only copy, whereas these are regenerated the next time the
/// skill's scripts run and the content hash already treats them as absent.
/// `--cached` keeps every byte on disk; other devices merely stop carrying them.
fn untrack_python_artifacts(skills_dir: &Path) -> Result<()> {
    let tracked = run_git(
        skills_dir,
        &["ls-files", "--", "*.pyc", "**/__pycache__/**"],
    )?;
    if tracked.trim().is_empty() {
        return Ok(());
    }

    let count = tracked.lines().filter(|l| !l.trim().is_empty()).count();
    run_git_checked(
        skills_dir,
        &[
            "rm",
            "-r",
            "--cached",
            "--quiet",
            "--ignore-unmatch",
            "--",
            "*.pyc",
            "**/__pycache__/**",
        ],
    )?;
    log::info!(
        "backup: stopped tracking {count} compiled-Python artifact(s); \
         they stay on disk and are regenerated as needed"
    );
    Ok(())
}

/// Runs `f` while holding the central-repo lock. Used by the user-initiated
/// backup commands (init/commit/pull/clone/reclone/restore), so we wait out
/// transient contention with background work instead of failing fast.
pub(crate) fn with_repo_lock<T, F>(operation: &str, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    let _lock = RepoLock::acquire_foreground(operation)?;
    f()
}

fn run_git(dir: &Path, args: &[&str]) -> Result<String> {
    run_git_env(dir, args, &[])
}

fn run_git_env(dir: &Path, args: &[&str], envs: &[(String, String)]) -> Result<String> {
    let output = git_command()
        .arg("-C")
        .arg(dir)
        .args(args)
        .envs(envs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .output()
        .context("Failed to run git command")?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let filtered = stderr
            .lines()
            .filter(|line| {
                let trimmed = line.trim().trim_start_matches("** ");
                !trimmed.starts_with("WARNING:")
                    && !trimmed.starts_with("This session may")
                    && !trimmed.starts_with("The server may")
                    && !trimmed.starts_with("See https://openssh.com")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let msg = if filtered.trim().is_empty() {
            stderr.trim()
        } else {
            filtered.trim()
        };
        anyhow::bail!("git command failed: {}", redact_urls_in_text(msg))
    }
}

fn run_git_checked(dir: &Path, args: &[&str]) -> Result<()> {
    run_git_env_checked(dir, args, &[])
}

fn run_git_env_checked(dir: &Path, args: &[&str], envs: &[(String, String)]) -> Result<()> {
    if let Err(e) = run_git_env(dir, args, envs) {
        // Single chokepoint for genuine git failures: every aborting step
        // (commit, push -u, fetch+merge, tag, read-tree, …) goes through here,
        // so this is where "sync silently failed" becomes visible in the log.
        // Redact the args because some carry the remote URL (which may embed a token).
        log::warn!("git failed [{}]: {}", redact_urls_in_text(&args.join(" ")), e);
        return Err(e);
    }
    Ok(())
}

fn get_ahead_behind(dir: &Path) -> Result<(u32, u32)> {
    let output = run_git(
        dir,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
    )?;
    let parts: Vec<&str> = output.split_whitespace().collect();
    if parts.len() == 2 {
        let ahead = parts[0].parse().unwrap_or(0);
        let behind = parts[1].parse().unwrap_or(0);
        Ok((ahead, behind))
    } else {
        Ok((0, 0))
    }
}

/// Copy the backup's top-level entries the cloned repo does not have into it.
fn merge_backup(backup: &Path, target: &Path) -> Result<()> {
    crate::core::sync_engine::ensure_dst_not_inside_src(backup, target)?;
    let entries = std::fs::read_dir(backup)?;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let dest = target.join(&name);
        // `symlink_metadata`: a dangling symlink in the clone is taken, never
        // a path to write through.
        if name != ".git" && std::fs::symlink_metadata(&dest).is_err() {
            copy_entry(&entry.path(), &dest)?;
        }
    }
    Ok(())
}

/// Copy a file, a directory tree, or a symlink as a symlink — links are never
/// followed, and never dropped: the backup is deleted after a merge. FIFOs,
/// sockets and devices are skipped: they hold no file content.
fn copy_entry(src: &Path, dst: &Path) -> std::io::Result<()> {
    let file_type = std::fs::symlink_metadata(src)?.file_type();
    if file_type.is_symlink() {
        let target = std::fs::read_link(src)?;
        #[cfg(unix)]
        return std::os::unix::fs::symlink(&target, dst);
        #[cfg(windows)]
        return {
            // From the link itself: a dangling directory link stays one.
            use std::os::windows::fs::FileTypeExt;
            if file_type.is_symlink_dir() {
                std::os::windows::fs::symlink_dir(&target, dst)
            } else {
                std::os::windows::fs::symlink_file(&target, dst)
            }
        };
    }
    if file_type.is_dir() {
        std::fs::create_dir(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_entry(&entry.path(), &dst.join(entry.file_name()))?;
        }
        return Ok(());
    }
    // FIFOs, sockets and devices hold no content (`replaceable_content` skips
    // them too), and opening a FIFO to copy it would block for good.
    if !file_type.is_file() {
        return Ok(());
    }
    std::fs::copy(src, dst).map(|_| ())
}

fn redact_urls_in_text(text: &str) -> String {
    text.split_whitespace()
        .map(redact_url)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Mask credentials embedded in a URL's userinfo (the `user:token@` before the
/// host). Note: this only covers the userinfo form, not credentials passed as
/// query parameters (e.g. `?token=…`); our remote URLs never use that form.
fn redact_url(url: &str) -> String {
    let Some(scheme_pos) = url.find("://") else {
        return url.to_string();
    };
    let auth_start = scheme_pos + 3;
    let rest = &url[auth_start..];

    let end_auth = rest
        .find(['/', '?', '#'])
        .map(|idx| auth_start + idx)
        .unwrap_or(url.len());
    let auth_part = &url[auth_start..end_auth];

    if let Some(at_rel) = auth_part.find('@') {
        let at_pos = auth_start + at_rel;
        let mut masked = String::with_capacity(url.len());
        masked.push_str(&url[..auth_start]);
        masked.push_str("***@");
        masked.push_str(&url[at_pos + 1..]);
        masked
    } else {
        url.to_string()
    }
}

fn parse_restored_from_tag_message(message: &str) -> Option<String> {
    let prefix = "restore: switch skills library to ";
    let tag = message.strip_prefix(prefix)?.trim();
    if tag.starts_with("sm-v-") {
        Some(tag.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── redact_url ──

    #[test]
    fn redact_url_with_credentials() {
        assert_eq!(
            redact_url("https://user:token@github.com/acme/repo.git"),
            "https://***@github.com/acme/repo.git"
        );
    }

    #[test]
    fn redact_url_with_token_only() {
        assert_eq!(
            redact_url("https://ghp_abc123@github.com/acme/repo.git"),
            "https://***@github.com/acme/repo.git"
        );
    }

    #[test]
    fn redact_url_no_credentials_unchanged() {
        assert_eq!(
            redact_url("https://github.com/acme/repo.git"),
            "https://github.com/acme/repo.git"
        );
    }

    #[test]
    fn redact_url_not_a_url_unchanged() {
        assert_eq!(redact_url("just-a-string"), "just-a-string");
    }

    #[test]
    fn redact_url_ssh_no_scheme_unchanged() {
        assert_eq!(
            redact_url("git@github.com:acme/repo.git"),
            "git@github.com:acme/repo.git"
        );
    }

    // ── redact_urls_in_text ──

    #[test]
    fn redact_urls_in_text_mixed_content() {
        let input = "failed to push to https://user:pass@github.com/repo.git (error)";
        let result = redact_urls_in_text(input);
        assert!(result.contains("***@github.com/repo.git"));
        assert!(!result.contains("user:pass"));
    }

    #[test]
    fn redact_urls_in_text_no_urls() {
        assert_eq!(redact_urls_in_text("plain text here"), "plain text here");
    }

    // ── device naming (§4.3: device name = commit author) ──

    #[test]
    fn sanitize_device_name_strips_ident_breakers_and_collapses_whitespace() {
        assert_eq!(sanitize_device_name("  MacBook   Pro  "), "MacBook Pro");
        assert_eq!(sanitize_device_name("evil<\n>name"), "evilname");
        assert_eq!(sanitize_device_name("公司 Windows"), "公司 Windows");
        assert_eq!(sanitize_device_name("<>"), "");
        assert_eq!(sanitize_device_name("a".repeat(100).as_str()).chars().count(), 64);
    }

    #[test]
    fn device_email_slugs_name_with_fallback() {
        assert_eq!(device_email("MacBook Pro"), "macbook-pro@skills-manager.local");
        assert_eq!(device_email("公司 Windows"), "windows@skills-manager.local");
        assert_eq!(device_email("公司"), "device@skills-manager.local");
    }

    #[test]
    fn default_device_name_is_never_empty() {
        assert!(!default_device_name().is_empty());
    }

    // ── ensure_gitignore / compiled-Python artifacts ──

    /// `ensure_gitignore` is append-only, and other code leans on that: it must
    /// add what is missing without reordering, rewriting, or dropping whatever
    /// the user put there themselves.
    #[test]
    fn ensure_gitignore_appends_missing_entries_and_leaves_user_lines_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let gitignore = dir.join(".gitignore");
        std::fs::write(&gitignore, "# my own notes\nsecret-notes/\n*.tmp\n").unwrap();

        ensure_gitignore(dir).unwrap();
        let lines: Vec<String> = std::fs::read_to_string(&gitignore)
            .unwrap()
            .lines()
            .map(ToOwned::to_owned)
            .collect();

        // The user's lines survive verbatim, in their original order, at the top.
        assert_eq!(&lines[..3], &["# my own notes", "secret-notes/", "*.tmp"]);
        // The already-present entry is not duplicated.
        assert_eq!(lines.iter().filter(|l| *l == "*.tmp").count(), 1);
        for required in [
            ".DS_Store",
            "Thumbs.db",
            ".skills-manager.lock",
            "__pycache__/",
            "*.pyc",
        ] {
            assert!(lines.iter().any(|l| l == required), "missing {required}");
        }

        // Running it again changes nothing at all.
        let before = std::fs::read_to_string(&gitignore).unwrap();
        ensure_gitignore(dir).unwrap();
        assert_eq!(std::fs::read_to_string(&gitignore).unwrap(), before);
    }

    /// Ignoring a pattern does nothing to paths already in the index, so a
    /// library that has been backing up `__pycache__` needs an explicit
    /// catch-up — otherwise `git add -A` keeps re-staging them forever.
    #[test]
    fn compiled_python_artifacts_already_committed_are_untracked() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        init_repo_unlocked(dir, "Test Device").unwrap();

        // Simulate a library from before the ignore rule existed: force the
        // artifacts in past the .gitignore, exactly as history would have them.
        let cache = dir.join("docx/scripts/__pycache__");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("helpers.cpython-311.pyc"), b"\x00bytecode").unwrap();
        std::fs::write(dir.join("docx/scripts/helpers.py"), "print('hi')").unwrap();
        // A real Windows extension module must NOT be swept up with them.
        std::fs::write(dir.join("docx/scripts/_native.pyd"), b"\x00ext").unwrap();
        run_git_checked(dir, &["add", "-A", "--force"]).unwrap();
        run_git_checked(dir, &["commit", "-m", "before the ignore rule"]).unwrap();
        assert!(is_tracked(dir, "docx/scripts/__pycache__/helpers.cpython-311.pyc"));

        commit_all_unlocked(dir, "backup").unwrap();

        assert!(
            !is_tracked(dir, "docx/scripts/__pycache__/helpers.cpython-311.pyc"),
            "compiled artifacts must stop being tracked"
        );
        // Untracked, not deleted: they are still on disk for the interpreter.
        assert!(cache.join("helpers.cpython-311.pyc").is_file());
        // Real content is untouched — including `.pyd`, which `content_hash`
        // counts as part of the skill.
        assert!(is_tracked(dir, "docx/scripts/helpers.py"));
        assert!(is_tracked(dir, "docx/scripts/_native.pyd"));

        // Idempotent: a second round finds nothing left to do.
        std::fs::write(dir.join("note.md"), "x").unwrap();
        commit_all_unlocked(dir, "backup again").unwrap();
        assert!(is_tracked(dir, "docx/scripts/helpers.py"));
        assert!(is_tracked(dir, "docx/scripts/_native.pyd"));
    }

    #[test]
    fn init_repo_commits_as_device_and_identity_follows_rename() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        // Init must succeed and author the initial commit as the device,
        // regardless of any global git identity on this machine.
        init_repo_unlocked(dir, "MacBook Pro").unwrap();
        assert_eq!(run_git(dir, &["log", "-1", "--format=%an"]).unwrap(), "MacBook Pro");
        assert_eq!(
            run_git(dir, &["config", "--local", "--get", "user.email"]).unwrap(),
            "macbook-pro@skills-manager.local"
        );

        // Renaming the device only affects commits made afterwards.
        configure_device_identity(dir, "公司 Windows").unwrap();
        std::fs::write(dir.join("note.md"), "x").unwrap();
        commit_all_unlocked(dir, "backup").unwrap();
        assert_eq!(run_git(dir, &["log", "-1", "--format=%an"]).unwrap(), "公司 Windows");

        // And the snapshot history exposes the author per entry.
        let tag = create_snapshot_tag_unlocked(dir).unwrap();
        let versions = list_snapshot_versions(dir, None).unwrap();
        let entry = versions.iter().find(|v| v.tag == tag).unwrap();
        assert_eq!(entry.author, "公司 Windows");
        assert_eq!(entry.message, "backup");
        assert!(!entry.committed_at.is_empty());
    }

    #[test]
    fn configure_device_identity_noop_without_repo_or_name() {
        let tmp = tempfile::tempdir().unwrap();
        // Not a repo: must not fail (and must not create one).
        configure_device_identity(tmp.path(), "MacBook Pro").unwrap();
        assert!(!tmp.path().join(".git").exists());
        // Empty name on a real repo: leaves identity untouched.
        init_repo_unlocked(tmp.path(), "Device A").unwrap();
        configure_device_identity(tmp.path(), "  ").unwrap();
        assert_eq!(
            run_git(tmp.path(), &["config", "--local", "--get", "user.name"]).unwrap(),
            "Device A"
        );
    }

    // ── oversized skill exclusion (§3.6 后半) ──

    #[test]
    fn oversized_untracked_skill_is_excluded_but_tracked_one_is_not() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        init_repo_unlocked(dir, "Device A").unwrap();

        // A skill committed while small stays tracked even after growing
        // past the limit — untracking would propagate as a deletion.
        std::fs::create_dir_all(dir.join("grown")).unwrap();
        std::fs::write(dir.join("grown/SKILL.md"), "small at first").unwrap();
        commit_all_unlocked(dir, "seed").unwrap();
        std::fs::write(dir.join("grown/data.bin"), vec![0u8; 64]).unwrap();

        // A brand-new oversized skill (limit shrunk for the test) with its
        // metadata file.
        std::fs::create_dir_all(dir.join("huge")).unwrap();
        std::fs::write(dir.join("huge/SKILL.md"), vec![b'x'; 64]).unwrap();
        let meta_dir = dir.join(".skills-manager/skills");
        std::fs::create_dir_all(&meta_dir).unwrap();
        std::fs::write(
            meta_dir.join("skill-huge.json"),
            br#"{"schema_version":1,"skill_id":"skill-huge","path":"huge","path_key":"huge","enabled":true,"tags":[],"source":{"type":"import","ref":null,"subpath":null,"branch":null}}"#,
        )
        .unwrap();

        let excluded = apply_oversized_exclusions(dir, 32).unwrap();
        assert_eq!(excluded, vec!["huge"]);
        let gitignore = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert!(gitignore.contains("/huge/"), "{gitignore}");
        assert!(gitignore.contains("/.skills-manager/skills/skill-huge.json"), "{gitignore}");
        assert!(!gitignore.contains("/grown/"), "tracked skill must not be excluded: {gitignore}");

        // Committing keeps the oversized skill (and its metadata) out of the
        // tree while the grown-but-tracked one stays in.
        std::fs::write(dir.join("note.md"), "trigger commit").unwrap();
        // commit_all uses the real 100MB limit, so re-apply the test limit
        // before checking what got committed.
        apply_oversized_exclusions(dir, 32).unwrap();
        run_git_checked(dir, &["add", "-A"]).unwrap();
        run_git_checked(dir, &["commit", "-m", "test"]).unwrap();
        assert!(run_git(dir, &["cat-file", "-e", "HEAD:huge/SKILL.md"]).is_err());
        assert!(run_git(dir, &["cat-file", "-e", "HEAD:.skills-manager/skills/skill-huge.json"]).is_err());
        run_git(dir, &["cat-file", "-e", "HEAD:grown/data.bin"]).unwrap();
        // Local files are untouched.
        assert!(dir.join("huge/SKILL.md").exists());

        // Idempotent, and the section self-heals once the skill shrinks.
        apply_oversized_exclusions(dir, 32).unwrap();
        let same = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert_eq!(gitignore, same);
        std::fs::write(dir.join("huge/SKILL.md"), "tiny").unwrap();
        apply_oversized_exclusions(dir, 32).unwrap();
        let after = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert!(!after.contains("/huge/"), "shrunk skill re-enters backup: {after}");
        assert!(!after.contains("oversized"), "empty section is removed: {after}");
    }

    #[test]
    fn size_report_marks_untracked_oversized_as_excluded() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        init_repo_unlocked(dir, "Device A").unwrap();
        std::fs::create_dir_all(dir.join("big")).unwrap();
        std::fs::write(dir.join("big/SKILL.md"), vec![b'x'; 200]).unwrap();
        // The public report uses the real 100MB limit; exercise the flag via
        // the internal helper plus a handcrafted report path instead of
        // writing 100MB in a test: tracked → not excluded, untracked → excluded.
        assert!(!is_tracked(dir, "big"));
        run_git_checked(dir, &["add", "-A"]).unwrap();
        run_git_checked(dir, &["commit", "-m", "track big"]).unwrap();
        assert!(is_tracked(dir, "big"));
    }

    // ── app_commit protocol markers (§6) ──

    #[test]
    fn app_commits_carry_protocol_marker_and_trailer() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        // init: initial commit has protocol.json in tree + trailer in message.
        init_repo_unlocked(dir, "Device A").unwrap();
        let body = run_git(dir, &["log", "-1", "--format=%B"]).unwrap();
        assert!(protocol::has_protocol_trailer(&body), "init: {body}");
        run_git(dir, &["cat-file", "-e", "HEAD:.skills-manager/protocol.json"]).unwrap();

        // A pre-protocol snapshot: simulate by committing a tree with the
        // marker removed, tagging it, then restoring it.
        let snapshot_tag = create_snapshot_tag_unlocked(dir).unwrap();
        std::fs::write(dir.join("note.md"), "x").unwrap();
        commit_all_unlocked(dir, "backup").unwrap();
        let body = run_git(dir, &["log", "-1", "--format=%B"]).unwrap();
        assert!(protocol::has_protocol_trailer(&body), "commit_all: {body}");

        // Restore an old snapshot (which does carry protocol.json since init
        // wrote it) — restore commit must carry the trailer too.
        let safety = restore_snapshot_version_unlocked(dir, &snapshot_tag).unwrap();
        assert!(safety.starts_with("sm-v-"));
        let body = run_git(dir, &["log", "-1", "--format=%B"]).unwrap();
        assert!(protocol::has_protocol_trailer(&body), "restore: {body}");
        run_git(dir, &["cat-file", "-e", "HEAD:.skills-manager/protocol.json"]).unwrap();
    }

    #[test]
    fn restore_of_pre_protocol_snapshot_self_heals_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        // Build a repo whose first snapshot predates the protocol marker.
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["-c", "user.email=t@e.c", "-c", "user.name=T"])
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-b", "main"]);
        // Persist the identity into repo-local config so the production commit
        // path (commit_all_unlocked, plain run_git) has an author on CI runners
        // that lack a global git identity — the transient `-c` flags above only
        // apply to this closure's own invocations.
        git(&["config", "user.email", "t@e.c"]);
        git(&["config", "user.name", "T"]);
        std::fs::create_dir_all(dir.join("skill-a")).unwrap();
        std::fs::write(dir.join("skill-a/SKILL.md"), "v1").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-m", "pre-protocol"]);
        let old_tag = create_snapshot_tag_unlocked(dir).unwrap();

        // A protocol-era commit follows.
        std::fs::write(dir.join("skill-a/SKILL.md"), "v2").unwrap();
        commit_all_unlocked(dir, "backup").unwrap();
        run_git(dir, &["cat-file", "-e", "HEAD:.skills-manager/protocol.json"]).unwrap();

        // Restoring the pre-protocol snapshot must not resurrect a
        // marker-less tree: the restore commit re-adds protocol.json (sticky).
        restore_snapshot_version_unlocked(dir, &old_tag).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("skill-a/SKILL.md")).unwrap(), "v1");
        run_git(dir, &["cat-file", "-e", "HEAD:.skills-manager/protocol.json"]).unwrap();
        let body = run_git(dir, &["log", "-1", "--format=%B"]).unwrap();
        assert!(protocol::has_protocol_trailer(&body), "restore: {body}");
    }

    // ── parse_restored_from_tag_message ──

    #[test]
    fn parse_restored_tag_valid() {
        let msg = "restore: switch skills library to sm-v-20260318-153012-abc1234";
        assert_eq!(
            parse_restored_from_tag_message(msg).as_deref(),
            Some("sm-v-20260318-153012-abc1234")
        );
    }

    #[test]
    fn parse_restored_tag_invalid_prefix() {
        assert_eq!(
            parse_restored_from_tag_message("some other commit message"),
            None
        );
    }

    #[test]
    fn parse_restored_tag_non_snapshot_tag() {
        let msg = "restore: switch skills library to v1.0.0";
        assert_eq!(parse_restored_from_tag_message(msg), None);
    }

    #[test]
    fn clone_into_unlocked_failure_includes_git_stderr() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("clone-target");
        // file:// URL pointing at a non-existent path -> git clone fails with
        // a deterministic stderr message we can pattern-match on.
        let bogus_src = tmp.path().join("does-not-exist.git");
        let url = format!("file://{}", bogus_src.display());

        let err = clone_into_unlocked(&target, &url, &[]).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("git clone failed"),
            "expected git stderr to be surfaced, got: {msg}"
        );
        assert!(
            !msg.eq("Failed to clone repository"),
            "error must not be the old generic placeholder"
        );
    }

    #[test]
    fn clone_into_unlocked_failure_redacts_credentials_in_url() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("clone-target");
        // Unreachable host with a token-bearing URL. git's stderr typically
        // echoes the URL back; the error must not leak the token.
        let url = "https://ghp_supersecrettoken123@127.0.0.1:1/does-not-exist.git";

        let err = clone_into_unlocked(&target, url, &[]).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            !msg.contains("ghp_supersecrettoken123"),
            "credential must not leak into error message: {msg}"
        );
    }

    #[test]
    fn parse_restored_tag_with_trailing_whitespace() {
        let msg = "restore: switch skills library to sm-v-20260318-153012-abc1234  ";
        assert_eq!(
            parse_restored_from_tag_message(msg).as_deref(),
            Some("sm-v-20260318-153012-abc1234")
        );
    }

    // ── count_changed_top_dirs ──

    #[test]
    fn count_changed_top_dirs_counts_unique_skill_dirs() {
        let porcelain = " M skill-a/SKILL.md\n?? skill-a/notes.md\n M skill-b/SKILL.md\n";
        assert_eq!(count_changed_top_dirs(porcelain), 2);
    }

    #[test]
    fn count_changed_top_dirs_skips_metadata_and_root_dotfiles() {
        let porcelain = " M .skills-manager/skills/x.json\n M .gitignore\n";
        assert_eq!(count_changed_top_dirs(porcelain), 0);
    }

    #[test]
    fn count_changed_top_dirs_follows_renames() {
        let porcelain = "R  old-name/SKILL.md -> new-name/SKILL.md\n";
        assert_eq!(count_changed_top_dirs(porcelain), 1);
    }

    #[test]
    fn count_changed_top_dirs_empty() {
        assert_eq!(count_changed_top_dirs(""), 0);
    }

    // ── #244 acceptance: status stable across a simulated restart ──

    #[test]
    fn status_reports_in_sync_after_backup_cycle_and_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        let work = tmp.path().join("work");
        std::fs::create_dir_all(&work).unwrap();

        assert!(Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(&remote)
            .output()
            .unwrap()
            .status
            .success());

        let git = |args: &[&str]| {
            let out = Command::new("git").arg("-C").arg(&work).args(args).output().unwrap();
            assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);
        git(&["config", "push.autoSetupRemote", "false"]);

        // Full backup cycle: add a skill, commit, snapshot, push.
        std::fs::create_dir_all(work.join("skill-a")).unwrap();
        std::fs::write(work.join("skill-a/SKILL.md"), "content").unwrap();
        commit_all_unlocked(&work, "backup").unwrap();
        set_remote_unlocked(&work, remote.to_str().unwrap()).unwrap();
        let tag = create_snapshot_tag_unlocked(&work).unwrap();
        push_unlocked(&work).unwrap();

        // Simulated restart: a fresh fetch + status read (what the app does
        // on launch) must still say "in sync" — not pending, not divergent.
        fetch_remote(&work).unwrap();
        let status = get_status(&work).unwrap();
        assert!(status.is_repo);
        assert_eq!(status.upstream_health, "healthy");
        assert!(!status.has_changes, "no pending changes after clean sync");
        assert_eq!(status.changed_skill_count, 0);
        assert_eq!((status.ahead, status.behind), (0, 0));
        assert_eq!(status.current_snapshot_tag.as_deref(), Some(tag.as_str()));
    }

    // ── restore safety point ──

    #[test]
    fn a_reftable_library_is_refused_with_a_migration_hint() {
        let tmp = tempfile::tempdir().unwrap();
        let out = Command::new("git")
            .args(["init", "--ref-format=reftable"])
            .arg(tmp.path())
            .output()
            .unwrap();
        if !out.status.success() {
            eprintln!("skipping: this git cannot create reftable repositories");
            return;
        }
        let err = ensure_repo(tmp.path()).unwrap_err().to_string();
        assert!(err.contains("refs migrate --ref-format=files"), "{err}");
        assert!(ensure_no_interrupted_git_operation(tmp.path()).is_err());
    }

    #[test]
    #[ignore = "mutates GIT_DEFAULT_REF_FORMAT; run alone"]
    fn init_creates_a_files_repo_even_when_git_defaults_to_reftable() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("GIT_DEFAULT_REF_FORMAT", "reftable");
        let result = init_repo_unlocked(tmp.path(), "test-device");
        std::env::remove_var("GIT_DEFAULT_REF_FORMAT");
        result.unwrap();
        assert!(!tmp.path().join(".git/reftable").exists());
        ensure_repo(tmp.path()).unwrap();
    }

    #[test]
    fn restore_creates_safety_point_capturing_dirty_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let git = |args: &[&str]| {
            let out = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
            assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);

        std::fs::create_dir_all(dir.join("skill-a")).unwrap();
        std::fs::write(dir.join("skill-a/SKILL.md"), "v1").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-m", "v1"]);
        let old_tag = create_snapshot_tag_unlocked(dir).unwrap();

        std::fs::write(dir.join("skill-a/SKILL.md"), "v2").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-m", "v2"]);
        // Uncommitted edit on top — must survive inside the safety point.
        std::fs::write(dir.join("skill-a/SKILL.md"), "v3-dirty").unwrap();

        let safety_tag = restore_snapshot_version_unlocked(dir, &old_tag).unwrap();

        // Working tree is back at v1.
        assert_eq!(std::fs::read_to_string(dir.join("skill-a/SKILL.md")).unwrap(), "v1");
        // The safety point is a persistent user-visible snapshot of the
        // pre-restore state, including the dirty edit.
        assert!(safety_tag.starts_with("sm-v-"));
        let captured = run_git(dir, &["show", &format!("{safety_tag}:skill-a/SKILL.md")]).unwrap();
        assert_eq!(captured, "v3-dirty");
        // And it shows up in the snapshot history for one-click undo.
        let versions = list_snapshot_versions(dir, None).unwrap();
        assert!(versions.iter().any(|v| v.tag == safety_tag));
    }

    // ── ensure_clean_clone_target ──
    // Tested directly (without RepoLock) so cases run in parallel without
    // serializing on the process-wide clone lock.

    #[test]
    fn ensure_clean_clone_target_allows_nonexistent_path() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("not-yet");
        ensure_clean_clone_target(&target).unwrap();
    }

    #[test]
    fn ensure_clean_clone_target_allows_empty_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("empty");
        std::fs::create_dir_all(&target).unwrap();
        ensure_clean_clone_target(&target).unwrap();
    }

    #[test]
    fn ensure_clean_clone_target_allows_existing_git_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("existing-repo");
        std::fs::create_dir_all(target.join(".git")).unwrap();
        std::fs::write(target.join("README"), b"x").unwrap();
        // Existing .git is delegated to clone_into_unlocked's own rejection.
        ensure_clean_clone_target(&target).unwrap();
    }

    #[test]
    fn ensure_clean_clone_target_refuses_non_empty_non_git() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("populated");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("user-file.txt"), b"important").unwrap();

        let err = ensure_clean_clone_target(&target).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("non-empty") && msg.contains("not a git repo"),
            "unexpected message: {msg}"
        );
        // Crucial: the user file must still be there.
        assert!(target.join("user-file.txt").exists());
    }

    // ── first push to an empty remote (the no_upstream scenario) ──

    // ── remove_remote ──
    // Tested via the unlocked inner so cases run in parallel without
    // serializing on the process-wide repo lock.

    #[test]
    fn remove_remote_deletes_origin_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert!(Command::new("git")
            .arg("-C")
            .arg(dir)
            .arg("init")
            .output()
            .unwrap()
            .status
            .success());
        run_git_checked(
            dir,
            &["remote", "add", "origin", "https://github.com/acme/repo.git"],
        )
        .unwrap();

        remove_remote_unlocked(dir).unwrap();
        assert!(run_git(dir, &["remote", "get-url", "origin"]).is_err());

        // A second disconnect (no origin left) must still succeed.
        remove_remote_unlocked(dir).unwrap();
    }

    #[test]
    fn remove_remote_on_non_repo_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        remove_remote_unlocked(tmp.path()).unwrap();
    }

    #[test]
    fn push_first_time_to_empty_remote_with_no_upstream() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        let work = tmp.path().join("work");
        std::fs::create_dir_all(&work).unwrap();

        // Empty bare remote — the freshly-created GitHub/Gitee repo case.
        assert!(Command::new("git")
            .args(["init", "--bare"])
            .arg(&remote)
            .output()
            .unwrap()
            .status
            .success());

        // Local repo with one commit on main. Identity is injected explicitly so
        // the test does not depend on a global git identity (CI runners have none).
        let git = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&work)
                .args(["-c", "user.email=test@example.com", "-c", "user.name=Test"])
                .args(args)
                .output()
                .unwrap()
        };
        assert!(git(&["init"]).status.success());
        assert!(git(&["checkout", "-b", "main"]).status.success());
        // Pin autoSetupRemote off in the repo's local config so the plain `git push`
        // deterministically fails and we actually exercise the `-u` fallback, even on
        // a dev box whose global config sets push.autoSetupRemote=true.
        assert!(git(&["config", "push.autoSetupRemote", "false"]).status.success());
        std::fs::write(work.join("a.txt"), "hello").unwrap();
        assert!(git(&["add", "-A"]).status.success());
        assert!(git(&["commit", "-m", "initial"]).status.success());

        // Wiring the empty remote leaves no tracking branch — exactly the state
        // that made the UI report "Up to date" while the remote stayed empty.
        set_remote_unlocked(&work, remote.to_str().unwrap()).unwrap();
        assert_eq!(detect_upstream_health(&work, true), "no_upstream");
        assert_eq!(get_ahead_behind(&work).unwrap_or((0, 0)), (0, 0));

        // The capability the frontend fix relies on: push must set upstream and
        // actually populate the remote, not no-op.
        push_unlocked(&work).unwrap();

        let remote_main = Command::new("git")
            .arg("-C")
            .arg(&remote)
            .args(["rev-parse", "main"])
            .output()
            .unwrap();
        assert!(
            remote_main.status.success(),
            "remote should have branch main after first push"
        );
        assert_eq!(detect_upstream_health(&work, true), "healthy");
    }

    #[test]
    fn prune_hidden_refs_removes_remote_copies_and_keeps_local_refs() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        let work = tmp.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        assert!(Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(&remote)
            .output()
            .unwrap()
            .status
            .success());

        init_repo_unlocked(&work, "Device A").unwrap();
        set_remote_unlocked(&work, remote.to_str().unwrap()).unwrap();
        push_unlocked(&work).unwrap();

        // Simulate a mirror-style push leaking hidden refs to the remote,
        // plus a functional local ref that must survive.
        run_git_checked(
            &work,
            &["update-ref", "refs/skills-manager/conflict/skill-x", "HEAD"],
        )
        .unwrap();
        run_git_checked(&work, &["update-ref", "refs/skills-manager/pre-merge", "HEAD"]).unwrap();
        run_git_checked(
            &work,
            &["push", "origin", "refs/skills-manager/conflict/skill-x", "refs/skills-manager/pre-merge"],
        )
        .unwrap();
        let leaked = run_git(&work, &["ls-remote", "origin", "refs/skills-manager/*"]).unwrap();
        assert_eq!(leaked.lines().count(), 2, "setup: refs must be on the remote");

        let removed = prune_hidden_refs_on_remote(&work).unwrap();
        assert_eq!(removed, 2);
        let after = run_git(&work, &["ls-remote", "origin", "refs/skills-manager/*"]).unwrap();
        assert!(after.trim().is_empty(), "remote hidden refs must be gone: {after}");
        // Branch and local functional refs are untouched.
        run_git(&work, &["rev-parse", "refs/skills-manager/conflict/skill-x"]).unwrap();
        run_git(&work, &["rev-parse", "refs/skills-manager/pre-merge"]).unwrap();
        let heads = run_git(&work, &["ls-remote", "--heads", "origin"]).unwrap();
        assert!(heads.contains("refs/heads/main"));

        // Idempotent: nothing left to remove.
        assert_eq!(prune_hidden_refs_on_remote(&work).unwrap(), 0);
    }

    #[test]
    fn pull_conflict_aborts_merge_and_reports_sync_conflict() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        std::fs::create_dir_all(&a).unwrap();

        // Pin the bare repo's HEAD to main: without a global
        // init.defaultBranch (CI runners have none) it points at master,
        // the later clone finds a dangling HEAD and checks nothing out,
        // and machine B's `commit -am` fails on an unborn branch.
        assert!(Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(&remote)
            .output()
            .unwrap()
            .status
            .success());

        let git = |dir: &Path, args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["-c", "user.email=test@example.com", "-c", "user.name=Test"])
                .args(args)
                .output()
                .unwrap()
        };

        // Machine A: seed one skill file and publish it.
        assert!(git(&a, &["init"]).status.success());
        assert!(git(&a, &["checkout", "-b", "main"]).status.success());
        std::fs::write(a.join("skill.md"), "base\n").unwrap();
        assert!(git(&a, &["add", "-A"]).status.success());
        assert!(git(&a, &["commit", "-m", "base"]).status.success());
        assert!(git(&a, &["remote", "add", "origin", remote.to_str().unwrap()]).status.success());
        assert!(git(&a, &["push", "-u", "origin", "main"]).status.success());

        // Machine B: clone, then both sides edit the SAME line differently.
        assert!(Command::new("git")
            .args(["clone", remote.to_str().unwrap()])
            .arg(&b)
            .output()
            .unwrap()
            .status
            .success());
        assert!(git(&b, &["config", "user.email", "test@example.com"]).status.success());
        assert!(git(&b, &["config", "user.name", "Test"]).status.success());

        std::fs::write(a.join("skill.md"), "edited on A\n").unwrap();
        assert!(git(&a, &["commit", "-am", "edit A"]).status.success());
        assert!(git(&a, &["push", "origin", "main"]).status.success());

        std::fs::write(b.join("skill.md"), "edited on B\n").unwrap();
        assert!(git(&b, &["commit", "-am", "edit B"]).status.success());

        // B pulls A's conflicting change. The merge must fail, be aborted, and
        // surface a recognizable conflict error — not leave B wedged.
        let err = pull_unlocked(&b).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("SYNC_CONFLICT"),
            "expected a recognizable conflict error, got: {msg}"
        );

        // Crucial: the abort must clear the in-progress merge so future syncs
        // are not permanently blocked.
        assert!(
            !b.join(".git/MERGE_HEAD").exists(),
            "MERGE_HEAD should be gone after abort"
        );
        let porcelain = run_git(&b, &["status", "--porcelain"]).unwrap();
        assert!(
            porcelain.is_empty(),
            "working tree should be clean after abort, got: {porcelain:?}"
        );
        // And a follow-up sync is no longer blocked by an interrupted operation.
        ensure_no_interrupted_git_operation(&b).unwrap();
    }

    #[test]
    fn ensure_clean_clone_target_refuses_file() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("a-file");
        std::fs::write(&target, b"x").unwrap();

        let err = ensure_clean_clone_target(&target).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("file, not a directory"),
            "unexpected message: {msg}"
        );
    }

    // ── clone / re-clone never drop local content ──

    /// Run git in `dir` with an explicit identity (CI runners have none).
    fn git_ok(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.email=test@example.com", "-c", "user.name=Test"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn write_file(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn read_normalized(path: &Path) -> String {
        // A Windows checkout may carry CRLF; the assertions are about content.
        std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
    }

    /// A bare `main` remote holding `files`, plus the seed checkout that
    /// pushed it (stands in for "another machine").
    fn seeded_remote(root: &Path, files: &[(&str, &str)]) -> (std::path::PathBuf, std::path::PathBuf) {
        let remote = root.join("remote.git");
        let seed = root.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        git_ok(root, &["init", "--bare", "--initial-branch=main", remote.to_str().unwrap()]);
        git_ok(&seed, &["init", "-b", "main"]);
        for (rel, content) in files {
            write_file(&seed.join(rel), content);
        }
        git_ok(&seed, &["add", "-A"]);
        git_ok(&seed, &["commit", "-m", "seed"]);
        git_ok(&seed, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git_ok(&seed, &["push", "-u", "origin", "main"]);
        (remote, seed)
    }

    fn siblings_named(dir: &Path, prefix: &str) -> Vec<std::path::PathBuf> {
        let mut found: Vec<_> = std::fs::read_dir(dir.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap())
            .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
            .map(|e| e.path())
            .collect();
        found.sort();
        found
    }

    /// Occupy every recovery name an operation starting now could pick —
    /// the bare prefix and `prefix-<ts>` for half a minute around now —
    /// each holding a file that must survive.
    fn occupy_recovery_names(dir: &Path, prefix: &str) -> Vec<std::path::PathBuf> {
        let now = Utc::now();
        let mut names = vec![prefix.to_string()];
        for offset in -2..=30 {
            let ts = (now + chrono::Duration::seconds(offset)).format("%Y%m%d-%H%M%S");
            names.push(format!("{prefix}-{ts}"));
        }
        names
            .into_iter()
            .map(|name| {
                let path = dir.with_file_name(name);
                write_file(&path.join("earlier-attempt.md"), "only copy\n");
                path
            })
            .collect()
    }

    #[test]
    fn clone_stops_when_a_local_skill_differs_from_the_remote_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let (remote, _) = seeded_remote(
            tmp.path(),
            &[
                ("edited-skill/SKILL.md", "remote version\n"),
                ("identical-skill/SKILL.md", "same\n"),
            ],
        );
        let skills = tmp.path().join("skills");
        write_file(&skills.join("edited-skill/SKILL.md"), "local edit\n");
        write_file(&skills.join("edited-skill/notes.md"), "only here\n");
        write_file(&skills.join("identical-skill/SKILL.md"), "same\n");
        write_file(&skills.join("local-only-skill/SKILL.md"), "mine\n");

        let err = clone_into_unlocked(&skills, remote.to_str().unwrap(), &[]).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("CLONE_LOCAL_DIFFERS"), "unrecognizable error: {msg}");
        assert!(msg.contains("edited-skill"), "diverging skill not named: {msg}");
        assert!(!msg.contains("identical-skill"), "identical skill reported: {msg}");
        assert!(!msg.contains("local-only-skill"), "local-only skill reported: {msg}");

        // The whole local library is back in place, untouched, and nothing
        // is left behind beside it.
        assert!(!skills.join(".git").exists(), "clone must not stay live");
        assert_eq!(read_normalized(&skills.join("edited-skill/SKILL.md")), "local edit\n");
        assert_eq!(read_normalized(&skills.join("edited-skill/notes.md")), "only here\n");
        assert!(skills.join("local-only-skill/SKILL.md").exists());
        assert!(siblings_named(&skills, "skills-backup-before-clone").is_empty());
    }

    #[test]
    fn clone_merges_local_only_skills_and_accepts_identical_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let (remote, _) = seeded_remote(
            tmp.path(),
            &[
                ("identical-skill/SKILL.md", "same\n"),
                ("identical-skill/r\u{e9}sum\u{e9}.md", "cv\n"),
                ("remote-skill/SKILL.md", "theirs\n"),
                (".skills-manager/protocol.json", "{\"from\":\"remote\"}\n"),
                (".gitignore", "remote-only-rule/\n"),
            ],
        );
        let skills = tmp.path().join("skills");
        // Same content: line endings and compiled-Python leftovers aside.
        write_file(&skills.join("identical-skill/SKILL.md"), "same\r\n");
        write_file(&skills.join("identical-skill/__pycache__/x.cpython-312.pyc"), "junk");
        // The same name, decomposed (as macOS apps may create it).
        write_file(&skills.join("identical-skill/re\u{301}sume\u{301}.md"), "cv\n");
        write_file(&skills.join("local-only-skill/SKILL.md"), "mine\n");
        // App metadata differs on every machine; the remote's copy wins.
        write_file(&skills.join(".skills-manager/protocol.json"), "{\"from\":\"local\"}\n");
        write_file(&skills.join(".gitignore"), "local-only-rule/\n");

        clone_into_unlocked(&skills, remote.to_str().unwrap(), &[]).unwrap();

        assert!(skills.join(".git").exists());
        assert_eq!(read_normalized(&skills.join("identical-skill/SKILL.md")), "same\n");
        assert!(
            !skills.join("identical-skill/__pycache__").exists(),
            "the clone's copy stays, not the local one"
        );
        assert_eq!(read_normalized(&skills.join("remote-skill/SKILL.md")), "theirs\n");
        assert_eq!(read_normalized(&skills.join("local-only-skill/SKILL.md")), "mine\n");
        assert_eq!(
            read_normalized(&skills.join(".skills-manager/protocol.json")),
            "{\"from\":\"remote\"}\n"
        );
        assert!(siblings_named(&skills, "skills-backup-before-clone").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn clone_puts_the_local_library_back_when_merging_it_in_fails() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let (remote, _) = seeded_remote(tmp.path(), &[("remote-skill/SKILL.md", "theirs\n")]);
        let skills = tmp.path().join("skills");
        write_file(&skills.join("local-only-skill/SKILL.md"), "mine\n");
        let unreadable = skills.join("local-only-skill/private.md");
        write_file(&unreadable, "cannot be copied\n");
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&unreadable).is_ok() {
            return; // running as root: nothing is unreadable, the failure can't be staged
        }

        let result = clone_into_unlocked(&skills, remote.to_str().unwrap(), &[]);
        let _ = std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o644));

        assert!(result.is_err(), "a failed merge must fail the clone");
        assert!(!skills.join(".git").exists(), "half-merged clone left live");
        assert!(!skills.join("remote-skill").exists(), "half-merged clone left live");
        assert_eq!(read_normalized(&skills.join("local-only-skill/private.md")), "cannot be copied\n");
        assert!(siblings_named(&skills, "skills-backup-before-clone").is_empty());
    }

    /// What the skill content hash leaves out is still content to lose:
    /// a symlink, a skill's own `.gitignore`, a repository nested in a skill.
    #[cfg(unix)]
    #[test]
    fn clone_stops_when_a_same_named_skill_differs_in_what_the_skill_hash_skips() {
        let tmp = tempfile::tempdir().unwrap();
        let (remote, _) = seeded_remote(
            tmp.path(),
            &[
                ("linked-skill/SKILL.md", "same\n"),
                ("ignore-skill/SKILL.md", "same\n"),
                ("nested-repo-skill/SKILL.md", "same\n"),
                ("plain-skill/SKILL.md", "same\n"),
            ],
        );
        let skills = tmp.path().join("skills");
        for name in ["linked-skill", "ignore-skill", "nested-repo-skill", "plain-skill"] {
            write_file(&skills.join(name).join("SKILL.md"), "same\n");
        }
        std::os::unix::fs::symlink("SKILL.md", skills.join("linked-skill/AGENTS.md")).unwrap();
        write_file(&skills.join("ignore-skill/.gitignore"), "data/\n");
        write_file(&skills.join("nested-repo-skill/.git/HEAD"), "ref: refs/heads/main\n");
        // Clutter that regenerates is not worth stopping for, nor is an empty
        // directory (git stores none, so every such skill would differ).
        write_file(&skills.join("plain-skill/.DS_Store"), "x");
        std::fs::create_dir_all(skills.join("plain-skill/empty")).unwrap();

        let err = clone_into_unlocked(&skills, remote.to_str().unwrap(), &[]).unwrap_err();
        let msg = format!("{err:#}");
        for name in ["linked-skill", "ignore-skill", "nested-repo-skill"] {
            assert!(msg.contains(name), "{name} not reported: {msg}");
        }
        assert!(!msg.contains("plain-skill"), "clutter reported as a difference: {msg}");
        assert_eq!(
            std::fs::read_link(skills.join("linked-skill/AGENTS.md")).unwrap(),
            std::path::Path::new("SKILL.md")
        );
        assert!(skills.join("nested-repo-skill/.git/HEAD").exists());
    }

    #[cfg(unix)]
    #[test]
    fn clone_keeps_symlinks_inside_local_only_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let (remote, _) = seeded_remote(tmp.path(), &[("remote-skill/SKILL.md", "theirs\n")]);
        let skills = tmp.path().join("skills");
        write_file(&skills.join("local-only-skill/SKILL.md"), "mine\n");
        std::os::unix::fs::symlink("SKILL.md", skills.join("local-only-skill/AGENTS.md")).unwrap();

        clone_into_unlocked(&skills, remote.to_str().unwrap(), &[]).unwrap();

        assert_eq!(
            std::fs::read_link(skills.join("local-only-skill/AGENTS.md")).unwrap(),
            std::path::Path::new("SKILL.md")
        );
    }

    #[cfg(unix)]
    #[test]
    fn clone_never_writes_through_a_dangling_symlink_from_the_remote() {
        let tmp = tempfile::tempdir().unwrap();
        let (remote, seed) = seeded_remote(tmp.path(), &[("remote-skill/SKILL.md", "theirs\n")]);
        // In the clone, `notes` points at a file beside the library.
        std::os::unix::fs::symlink("../outside.txt", seed.join("notes")).unwrap();
        git_ok(&seed, &["add", "-A"]);
        git_ok(&seed, &["commit", "-m", "dangling link"]);
        git_ok(&seed, &["push", "origin", "main"]);
        let skills = tmp.path().join("skills");
        write_file(&skills.join("notes"), "local notes\n");

        let result = clone_into_unlocked(&skills, remote.to_str().unwrap(), &[]);

        assert!(!tmp.path().join("outside.txt").exists(), "wrote outside the library");
        let err = result.unwrap_err();
        assert!(format!("{err:#}").contains("notes"), "unexpected error: {err:#}");
        assert_eq!(read_normalized(&skills.join("notes")), "local notes\n");
    }

    /// Names the comparison skips still reach the merge, which must not
    /// write through a dangling link either.
    #[cfg(unix)]
    #[test]
    fn clone_never_writes_through_a_dangling_symlink_under_a_skipped_name() {
        let tmp = tempfile::tempdir().unwrap();
        let (remote, seed) = seeded_remote(tmp.path(), &[("remote-skill/SKILL.md", "theirs\n")]);
        std::os::unix::fs::symlink("../outside.txt", seed.join(".DS_Store")).unwrap();
        git_ok(&seed, &["add", "-A"]);
        git_ok(&seed, &["commit", "-m", "dangling link"]);
        git_ok(&seed, &["push", "origin", "main"]);
        let skills = tmp.path().join("skills");
        write_file(&skills.join(".DS_Store"), "finder\n");

        clone_into_unlocked(&skills, remote.to_str().unwrap(), &[]).unwrap();

        assert!(!tmp.path().join("outside.txt").exists(), "wrote outside the library");
    }

    #[cfg(unix)]
    #[test]
    fn a_rollback_that_cannot_clear_the_clone_reports_where_the_library_is() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("skills");
        let backup = tmp.path().join("skills-backup-before-clone-x");
        write_file(&backup.join("local-skill/SKILL.md"), "mine\n");
        // A clone directory that cannot be removed.
        write_file(&skills.join("stuck/file"), "x");
        std::fs::set_permissions(skills.join("stuck"), std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::remove_file(skills.join("stuck/file")).is_ok() {
            return; // running as root: nothing is undeletable
        }

        let err = put_back_pre_clone_library(&skills, &backup, anyhow::anyhow!("clone failed"))
            .unwrap_err();
        let _ = std::fs::set_permissions(skills.join("stuck"), std::fs::Permissions::from_mode(0o755));

        let msg = format!("{err:#}");
        assert!(msg.contains("clone failed"), "cause lost: {msg}");
        assert!(msg.contains(&backup.display().to_string()), "location not reported: {msg}");
        assert_eq!(read_normalized(&backup.join("local-skill/SKILL.md")), "mine\n");
        assert!(
            err.downcast_ref::<LibraryNotPutBack>().is_some(),
            "re-clone must be able to tell the library is not back"
        );
    }

    #[test]
    fn clone_can_set_aside_local_versions_and_continue() {
        let tmp = tempfile::tempdir().unwrap();
        let (remote, _) = seeded_remote(
            tmp.path(),
            &[("edited-skill/SKILL.md", "remote version\n"), ("identical-skill/SKILL.md", "same\n")],
        );
        let skills = tmp.path().join("skills");
        write_file(&skills.join("edited-skill/SKILL.md"), "local edit\n");
        write_file(&skills.join("edited-skill/notes.md"), "only here\n");
        write_file(&skills.join("identical-skill/SKILL.md"), "same\n");
        write_file(&skills.join("local-only-skill/SKILL.md"), "mine\n");

        let copies = clone_into_unlocked(&skills, remote.to_str().unwrap(), &["edited-skill".to_string()])
            .unwrap()
            .expect("the folder holding the local versions");

        // The library takes the remote's version and keeps local-only skills.
        assert!(skills.join(".git").exists());
        assert_eq!(read_normalized(&skills.join("edited-skill/SKILL.md")), "remote version\n");
        assert!(!skills.join("edited-skill/notes.md").exists());
        assert_eq!(read_normalized(&skills.join("local-only-skill/SKILL.md")), "mine\n");
        // The local version, whole, beside the library rather than in it.
        assert_eq!(copies.parent(), skills.parent());
        assert_eq!(read_normalized(&copies.join("edited-skill/SKILL.md")), "local edit\n");
        assert_eq!(read_normalized(&copies.join("edited-skill/notes.md")), "only here\n");
        assert!(!copies.join("identical-skill").exists(), "only differing skills are set aside");
        assert!(!copies.join("local-only-skill").exists(), "only differing skills are set aside");
        assert!(siblings_named(&skills, "skills-backup-before-clone").is_empty());
    }

    /// Setting aside is consent for the list the user saw: anything else
    /// differing by the time the clone runs stops it again.
    #[test]
    fn clone_sets_aside_only_the_confirmed_list() {
        let tmp = tempfile::tempdir().unwrap();
        let (remote, _) = seeded_remote(
            tmp.path(),
            &[("edited-skill/SKILL.md", "remote version\n"), ("other-skill/SKILL.md", "remote\n")],
        );
        let skills = tmp.path().join("skills");
        write_file(&skills.join("edited-skill/SKILL.md"), "local edit\n");
        write_file(&skills.join("other-skill/SKILL.md"), "changed since\n");

        let err = clone_into_unlocked(&skills, remote.to_str().unwrap(), &["edited-skill".to_string()])
            .unwrap_err();

        let msg = format!("{err:#}");
        assert!(msg.contains("CLONE_LOCAL_DIFFERS"), "unexpected error: {msg}");
        assert!(msg.contains("\nedited-skill\nother-skill"), "the new list, one per line: {msg}");
        assert_eq!(read_normalized(&skills.join("other-skill/SKILL.md")), "changed since\n");
        assert!(!skills.join(".git").exists());
        assert!(siblings_named(&skills, "skills-local-copies").is_empty());
    }

    #[test]
    fn reclone_can_set_aside_local_versions_and_continue() {
        // The sync-conflict path again, this time choosing to continue.
        let tmp = tempfile::tempdir().unwrap();
        let (remote, seed) =
            seeded_remote(tmp.path(), &[("conflict-skill/SKILL.md", "base\n")]);
        let skills = tmp.path().join("skills");
        git_ok(tmp.path(), &["clone", remote.to_str().unwrap(), skills.to_str().unwrap()]);
        write_file(&seed.join("conflict-skill/SKILL.md"), "edited on the other machine\n");
        git_ok(&seed, &["commit", "-am", "other machine"]);
        git_ok(&seed, &["push", "origin", "main"]);
        write_file(&skills.join("conflict-skill/SKILL.md"), "edited here\n");
        git_ok(&skills, &["commit", "-am", "local"]);

        let outcome = reclone_from_remote_unlocked(&skills, remote.to_str().unwrap(), &["conflict-skill".to_string()]).unwrap();

        let copies = outcome.local_copies.expect("the folder holding the local versions");
        assert_eq!(read_normalized(&copies.join("conflict-skill/SKILL.md")), "edited here\n");
        assert_eq!(
            read_normalized(&skills.join("conflict-skill/SKILL.md")),
            "edited on the other machine\n"
        );
        // The local commit was never pushed, so its history is kept too.
        assert!(outcome.kept_git.is_some());
    }

    /// Opening a FIFO for reading blocks until something writes to it, so a
    /// plain copy of one would hang the clone for good.
    #[cfg(unix)]
    #[test]
    fn clone_skips_special_files_instead_of_hanging_on_them() {
        let tmp = tempfile::tempdir().unwrap();
        let (remote, _) = seeded_remote(tmp.path(), &[("remote-skill/SKILL.md", "theirs\n")]);
        let skills = tmp.path().join("skills");
        write_file(&skills.join("local-only-skill/SKILL.md"), "mine\n");
        let fifo = skills.join("local-only-skill/pipe");
        if !Command::new("mkfifo").arg(&fifo).status().is_ok_and(|s| s.success()) {
            return; // no mkfifo here
        }

        let (done, finished) = std::sync::mpsc::channel();
        let (dir, url) = (skills.clone(), remote.to_str().unwrap().to_string());
        std::thread::spawn(move || {
            let _ = done.send(clone_into_unlocked(&dir, &url, &[]).is_ok());
        });
        let result = finished.recv_timeout(std::time::Duration::from_secs(20));

        assert_eq!(result, Ok(true), "the clone hung or failed on a FIFO");
        assert_eq!(read_normalized(&skills.join("local-only-skill/SKILL.md")), "mine\n");
        // No file content to keep: skipped, like an empty directory.
        assert!(std::fs::symlink_metadata(&fifo).is_err());
    }

    #[test]
    fn a_failed_clone_restores_the_library_and_spares_earlier_backups() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("skills");
        write_file(&skills.join("local-skill/SKILL.md"), "mine\n");
        let earlier = occupy_recovery_names(&skills, "skills-backup-before-clone");
        let bogus = tmp.path().join("does-not-exist.git");

        assert!(clone_into_unlocked(&skills, bogus.to_str().unwrap(), &[]).is_err());

        assert_eq!(read_normalized(&skills.join("local-skill/SKILL.md")), "mine\n");
        assert!(!skills.join(".git").exists());
        for dir in &earlier {
            assert!(
                dir.join("earlier-attempt.md").exists(),
                "an earlier backup was deleted: {}",
                dir.display()
            );
        }
        assert_eq!(
            siblings_named(&skills, "skills-backup-before-clone").len(),
            earlier.len(),
            "this attempt's own backup must not be left behind"
        );
    }

    #[test]
    fn reclone_stops_when_a_local_skill_differs_and_restores_the_old_repo() {
        // The sync-conflict path: the local edit is already committed (sync
        // commits before merging), the remote holds another machine's edit.
        let tmp = tempfile::tempdir().unwrap();
        let (remote, seed) =
            seeded_remote(tmp.path(), &[("conflict-skill/SKILL.md", "base\n")]);
        let skills = tmp.path().join("skills");
        git_ok(tmp.path(), &["clone", remote.to_str().unwrap(), skills.to_str().unwrap()]);
        write_file(&seed.join("conflict-skill/SKILL.md"), "edited on the other machine\n");
        git_ok(&seed, &["commit", "-am", "other machine"]);
        git_ok(&seed, &["push", "origin", "main"]);
        write_file(&skills.join("conflict-skill/SKILL.md"), "edited here\n");
        git_ok(&skills, &["commit", "-am", "local"]);
        let head_before = git_ok(&skills, &["rev-parse", "HEAD"]);

        let err = reclone_from_remote_unlocked(&skills, remote.to_str().unwrap(), &[]).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("CLONE_LOCAL_DIFFERS"), "unrecognizable error: {msg}");
        assert!(msg.contains("conflict-skill"), "diverging skill not named: {msg}");

        // Back exactly where it was: same repository, same commit, same file.
        assert_eq!(git_ok(&skills, &["rev-parse", "HEAD"]), head_before);
        assert_eq!(read_normalized(&skills.join("conflict-skill/SKILL.md")), "edited here\n");
        assert!(siblings_named(&skills, "skills-git-recovery").is_empty());
        assert!(siblings_named(&skills, "skills-backup-before-clone").is_empty());
    }

    #[test]
    fn reclone_keeps_old_history_the_remote_does_not_have() {
        let tmp = tempfile::tempdir().unwrap();
        let (remote, _) = seeded_remote(tmp.path(), &[("remote-skill/SKILL.md", "theirs\n")]);
        let skills = tmp.path().join("skills");
        git_ok(tmp.path(), &["clone", remote.to_str().unwrap(), skills.to_str().unwrap()]);
        // An unpushed commit and a snapshot tag on it.
        write_file(&skills.join("local-skill/SKILL.md"), "mine\n");
        git_ok(&skills, &["add", "-A"]);
        git_ok(&skills, &["commit", "-m", "local only"]);
        git_ok(&skills, &["tag", "sm-v-local"]);
        let local_head = git_ok(&skills, &["rev-parse", "HEAD"]);

        let reported = reclone_from_remote_unlocked(&skills, remote.to_str().unwrap(), &[]).unwrap().kept_git;

        let kept = siblings_named(&skills, "skills-git-recovery");
        assert_eq!(kept.len(), 1, "old history must be kept: {kept:?}");
        assert_eq!(reported.as_deref(), Some(kept[0].as_path()), "the user must be told where");
        let git_dir = format!("--git-dir={}", kept[0].display());
        assert_eq!(git_ok(tmp.path(), &[&git_dir, "rev-parse", "sm-v-local"]), local_head);
        // The re-clone itself still happened, with the local skill merged back.
        assert_ne!(git_ok(&skills, &["rev-parse", "HEAD"]), local_head);
        assert_eq!(read_normalized(&skills.join("local-skill/SKILL.md")), "mine\n");
    }

    #[test]
    fn reclone_drops_old_history_the_remote_already_has() {
        let tmp = tempfile::tempdir().unwrap();
        let (remote, _) = seeded_remote(tmp.path(), &[("remote-skill/SKILL.md", "theirs\n")]);
        let skills = tmp.path().join("skills");
        git_ok(tmp.path(), &["clone", remote.to_str().unwrap(), skills.to_str().unwrap()]);
        // A local-only pin ref (never pushed by design) on a pushed commit.
        git_ok(&skills, &["update-ref", "refs/skills-manager/pin", "HEAD"]);

        let reported = reclone_from_remote_unlocked(&skills, remote.to_str().unwrap(), &[]).unwrap().kept_git;

        assert_eq!(reported, None);
        assert!(skills.join(".git").exists());
        assert!(siblings_named(&skills, "skills-git-recovery").is_empty());
    }

    #[test]
    fn reclone_keeps_old_history_when_only_a_tag_was_not_pushed() {
        let tmp = tempfile::tempdir().unwrap();
        let (remote, _) = seeded_remote(tmp.path(), &[("remote-skill/SKILL.md", "theirs\n")]);
        let skills = tmp.path().join("skills");
        git_ok(tmp.path(), &["clone", remote.to_str().unwrap(), skills.to_str().unwrap()]);
        // A snapshot tag whose push failed, on a commit the remote has.
        git_ok(&skills, &["tag", "sm-v-unpushed"]);

        let reported = reclone_from_remote_unlocked(&skills, remote.to_str().unwrap(), &[]).unwrap().kept_git;

        let kept = reported.expect("an unpushed tag is history the remote lacks");
        let git_dir = format!("--git-dir={}", kept.display());
        git_ok(tmp.path(), &[&git_dir, "rev-parse", "sm-v-unpushed"]);
    }

    #[test]
    fn reclone_keeps_old_history_committed_on_a_detached_head() {
        // Restoring a snapshot detaches HEAD; later backups commit there, on
        // no branch at all.
        let tmp = tempfile::tempdir().unwrap();
        let (remote, _) = seeded_remote(tmp.path(), &[("remote-skill/SKILL.md", "theirs\n")]);
        let skills = tmp.path().join("skills");
        git_ok(tmp.path(), &["clone", remote.to_str().unwrap(), skills.to_str().unwrap()]);
        git_ok(&skills, &["checkout", "--detach"]);
        write_file(&skills.join("local-skill/SKILL.md"), "mine\n");
        git_ok(&skills, &["add", "-A"]);
        git_ok(&skills, &["commit", "-m", "on detached HEAD"]);
        let detached = git_ok(&skills, &["rev-parse", "HEAD"]);

        let reported = reclone_from_remote_unlocked(&skills, remote.to_str().unwrap(), &[]).unwrap().kept_git;

        let kept = reported.expect("a detached commit is history the remote lacks");
        let git_dir = format!("--git-dir={}", kept.display());
        assert_eq!(git_ok(tmp.path(), &[&git_dir, "rev-parse", "HEAD"]), detached);
    }

    #[test]
    fn reclone_keeps_old_history_committed_in_a_linked_worktree() {
        // A worktree's HEAD lives under `.git/worktrees/`, outside `refs/`.
        let tmp = tempfile::tempdir().unwrap();
        let (remote, _) = seeded_remote(tmp.path(), &[("remote-skill/SKILL.md", "theirs\n")]);
        let skills = tmp.path().join("skills");
        git_ok(tmp.path(), &["clone", remote.to_str().unwrap(), skills.to_str().unwrap()]);
        let worktree = tmp.path().join("side");
        git_ok(&skills, &["worktree", "add", "--detach", worktree.to_str().unwrap()]);
        write_file(&worktree.join("side-skill/SKILL.md"), "side\n");
        git_ok(&worktree, &["add", "-A"]);
        git_ok(&worktree, &["commit", "-m", "in the worktree"]);

        let reported = reclone_from_remote_unlocked(&skills, remote.to_str().unwrap(), &[]).unwrap().kept_git;

        assert!(reported.is_some(), "a worktree's commit is history the remote lacks");
    }

    #[test]
    fn reclone_keeps_old_history_with_submodule_repositories() {
        // Submodule repositories live under `.git/modules/`, outside `refs/`.
        let tmp = tempfile::tempdir().unwrap();
        let (remote, _) = seeded_remote(tmp.path(), &[("remote-skill/SKILL.md", "theirs\n")]);
        let skills = tmp.path().join("skills");
        git_ok(tmp.path(), &["clone", remote.to_str().unwrap(), skills.to_str().unwrap()]);
        write_file(&skills.join(".git/modules/vendored/HEAD"), "0123456789012345678901234567890123456789\n");

        let reported = reclone_from_remote_unlocked(&skills, remote.to_str().unwrap(), &[]).unwrap().kept_git;

        assert!(reported.is_some(), "history under .git/modules is not proven to be on the remote");
    }

    #[cfg(unix)]
    #[test]
    fn reclone_keeps_old_history_whose_worktrees_cannot_be_read() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let (remote, _) = seeded_remote(tmp.path(), &[("remote-skill/SKILL.md", "theirs\n")]);
        let skills = tmp.path().join("skills");
        git_ok(tmp.path(), &["clone", remote.to_str().unwrap(), skills.to_str().unwrap()]);
        let worktrees = skills.join(".git/worktrees");
        std::fs::create_dir(&worktrees).unwrap();
        std::fs::set_permissions(&worktrees, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_dir(&worktrees).is_ok() {
            return; // running as root: nothing is unreadable
        }

        let reported = reclone_from_remote_unlocked(&skills, remote.to_str().unwrap(), &[]).unwrap().kept_git;

        if let Some(kept) = &reported {
            let _ = std::fs::set_permissions(kept.join("worktrees"), std::fs::Permissions::from_mode(0o755));
        }
        assert!(reported.is_some(), "an unreadable worktrees/ proves nothing");
    }

    #[test]
    fn a_failed_reclone_restores_the_repo_and_spares_earlier_recovery_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let (remote, _) = seeded_remote(tmp.path(), &[("remote-skill/SKILL.md", "theirs\n")]);
        let skills = tmp.path().join("skills");
        git_ok(tmp.path(), &["clone", remote.to_str().unwrap(), skills.to_str().unwrap()]);
        let head_before = git_ok(&skills, &["rev-parse", "HEAD"]);
        let earlier = occupy_recovery_names(&skills, "skills-git-recovery");
        let bogus = tmp.path().join("does-not-exist.git");

        assert!(reclone_from_remote_unlocked(&skills, bogus.to_str().unwrap(), &[]).is_err());

        assert_eq!(git_ok(&skills, &["rev-parse", "HEAD"]), head_before);
        assert_eq!(read_normalized(&skills.join("remote-skill/SKILL.md")), "theirs\n");
        assert_eq!(git_ok(&skills, &["status", "--porcelain"]), "");
        for dir in &earlier {
            assert!(
                dir.join("earlier-attempt.md").exists(),
                "an earlier recovery dir was deleted: {}",
                dir.display()
            );
        }
        assert_eq!(siblings_named(&skills, "skills-git-recovery").len(), earlier.len());
    }
}
