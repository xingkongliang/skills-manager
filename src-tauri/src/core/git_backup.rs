use anyhow::{Context, Result};
use chrono::Utc;
use std::path::Path;
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

    run_git_checked(skills_dir, &["init"])?;
    run_git_checked(skills_dir, &["checkout", "-b", "main"])?;

    // Identity must exist before the initial commit: on machines without a
    // global git identity the commit would otherwise fail outright.
    configure_device_identity(skills_dir, device_name)?;

    ensure_gitignore(skills_dir)?;
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
    clone_into_unlocked(skills_dir, url)
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
    clone_into_unlocked(skills_dir, url)
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
/// The existing skill files are preserved through the same backup-then-merge flow
/// used by `clone_into_unlocked`. The previous `.git` is moved to a sibling
/// directory and only deleted after a successful clone, so a failed re-clone
/// (e.g., network/auth error) restores the original repository state instead
/// of permanently losing history, snapshots, and remotes.
pub(crate) fn reclone_from_remote_unlocked(skills_dir: &Path, url: &str) -> Result<()> {
    let git_dir = skills_dir.join(".git");
    if !git_dir.exists() {
        return clone_into_unlocked(skills_dir, url);
    }

    log::info!("git reclone: re-cloning from remote, preserving local skills");
    let ts = Utc::now().format("%Y%m%d-%H%M%S");
    let git_backup = skills_dir.with_file_name(format!("skills-git-recovery-{ts}"));
    if git_backup.exists() {
        std::fs::remove_dir_all(&git_backup)?;
    }
    std::fs::rename(&git_dir, &git_backup)
        .context("Failed to move existing .git aside before re-clone")?;

    match clone_into_unlocked(skills_dir, url) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&git_backup);
            Ok(())
        }
        Err(e) => {
            // Two failure shapes are possible inside clone_into_unlocked:
            //   1. `git clone` itself failed: clone_into_unlocked already
            //      restored skill files from skills-backup-before-clone, and
            //      no `.git` exists in skills_dir.
            //   2. `git clone` succeeded but the subsequent merge_backup
            //      step failed: skills_dir now contains a fresh `.git` plus
            //      partially-merged files, and the user's original files are
            //      still parked at skills-backup-before-clone.
            // In case 2 we must tear down the partial clone and restore the
            // pre-clone user files before renaming our saved .git back, or
            // the rename collides with the new .git and silently leaves the
            // user inside the wrong repository.
            let new_git_dir = skills_dir.join(".git");
            if new_git_dir.exists() {
                let pre_clone_backup = skills_dir.with_file_name("skills-backup-before-clone");
                let _ = std::fs::remove_dir_all(skills_dir);
                if pre_clone_backup.exists() {
                    let _ = std::fs::rename(&pre_clone_backup, skills_dir);
                }
            }
            if !skills_dir.exists() {
                let _ = std::fs::create_dir_all(skills_dir);
            }
            if let Err(restore_err) = std::fs::rename(&git_backup, skills_dir.join(".git")) {
                anyhow::bail!(
                    "Re-clone failed: {e}. Could not restore previous .git directory ({restore_err}); a backup is kept at {}",
                    git_backup.display()
                );
            }
            Err(e)
        }
    }
}

pub(crate) fn clone_into_unlocked(skills_dir: &Path, url: &str) -> Result<()> {
    if skills_dir.join(".git").exists() {
        anyhow::bail!("Skills directory is already a git repository");
    }

    // If skills dir has content, move it aside temporarily
    let has_existing = skills_dir.exists()
        && std::fs::read_dir(skills_dir)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false);

    let backup_dir = if has_existing {
        let backup = skills_dir.with_file_name("skills-backup-before-clone");
        if backup.exists() {
            std::fs::remove_dir_all(&backup)?;
        }
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
            .arg("clone")
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

    match clone_result {
        Ok(()) => {
            // Merge back any existing skills that don't conflict
            if let Some(backup) = backup_dir {
                merge_backup(&backup, skills_dir).with_context(|| {
                    format!(
                        "Failed to merge local backup into cloned repository. Backup kept at {}",
                        backup.display()
                    )
                })?;
                std::fs::remove_dir_all(&backup)?;
            }
            log::info!("git clone: done");
            Ok(())
        }
        Err(e) => {
            // Restore backup on failure. A partial git2 clone can leave a
            // half-created target (system git cleans up after itself);
            // clear it so a retry doesn't hit "already a git repository".
            if let Some(backup) = backup_dir {
                let _ = std::fs::remove_dir_all(skills_dir);
                let _ = std::fs::rename(&backup, skills_dir);
            } else if skills_dir.join(".git").exists() {
                let _ = std::fs::remove_dir_all(skills_dir);
            }
            log::warn!("git clone: failed: {e:#}");
            Err(e)
        }
    }
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

fn ensure_gitignore(skills_dir: &Path) -> Result<()> {
    let gitignore = skills_dir.join(".gitignore");
    let required = [".DS_Store", "Thumbs.db", "*.tmp", ".skills-manager.lock"];
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

/// Merge backup directory contents into the cloned repo (non-conflicting files only).
fn merge_backup(backup: &Path, target: &Path) -> Result<()> {
    crate::core::sync_engine::ensure_dst_not_inside_src(backup, target)?;
    let entries = std::fs::read_dir(backup)?;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let dest = target.join(&name);
        if !dest.exists() && name != ".git" {
            if entry.file_type()?.is_dir() {
                copy_dir_all(&entry.path(), &dest)?;
            } else {
                std::fs::copy(entry.path(), &dest)?;
            }
        }
    }
    Ok(())
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        // Skip symlinks to prevent following links outside the source directory
        if ty.is_symlink() {
            continue;
        }
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
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

        let err = clone_into_unlocked(&target, &url).unwrap_err();
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

        let err = clone_into_unlocked(&target, url).unwrap_err();
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
}
