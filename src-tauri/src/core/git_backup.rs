use anyhow::{Context, Result};
use chrono::Utc;
use std::path::Path;
use std::process::Command;

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
    let _ = run_git(skills_dir, &["fetch", "--quiet", "origin", &branch]);
    Ok(())
}

/// Get the current git status of the skills directory.
pub fn get_status(skills_dir: &Path) -> Result<GitBackupStatus> {
    if !skills_dir.join(".git").exists() {
        return Ok(GitBackupStatus {
            is_repo: false,
            remote_url: None,
            branch: None,
            has_changes: false,
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

    let has_changes = run_git(skills_dir, &["status", "--porcelain"])
        .map(|s| !s.is_empty())
        .unwrap_or(false);

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
pub fn init_repo(skills_dir: &Path) -> Result<()> {
    let _lock = RepoLock::acquire_foreground("git init")?;
    init_repo_unlocked(skills_dir)
}

pub(crate) fn init_repo_unlocked(skills_dir: &Path) -> Result<()> {
    if skills_dir.join(".git").exists() {
        anyhow::bail!("Already a git repository");
    }

    run_git_checked(skills_dir, &["init"])?;
    run_git_checked(skills_dir, &["checkout", "-b", "main"])?;

    ensure_gitignore(skills_dir)?;

    // Initial commit
    run_git_checked(skills_dir, &["add", "-A"])?;
    run_git_checked(
        skills_dir,
        &["commit", "-m", "Initial skill library snapshot"],
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
    if let Err(e) = run_git(skills_dir, &["fetch", "origin"]) {
        log::warn!("git set_remote: initial fetch failed (continuing): {e}");
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

/// Stage all changes and create a commit.
#[allow(dead_code)]
pub fn commit_all(skills_dir: &Path, message: &str) -> Result<()> {
    let _lock = RepoLock::acquire_foreground("git commit")?;
    commit_all_unlocked(skills_dir, message)
}

pub(crate) fn commit_all_unlocked(skills_dir: &Path, message: &str) -> Result<()> {
    ensure_repo(skills_dir)?;
    ensure_gitignore(skills_dir)?;

    run_git_checked(skills_dir, &["add", "-A"])?;

    // Check if there's anything to commit
    let status = run_git(skills_dir, &["status", "--porcelain"])?;
    if status.is_empty() {
        log::info!("git commit: working tree clean, nothing to commit");
        anyhow::bail!("Nothing to commit");
    }

    run_git_checked(skills_dir, &["commit", "-m", message])?;
    log::info!("git commit: committed staged changes");
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

    // Push branch first; if no upstream, set it.
    let result = run_git(skills_dir, &["push"]);
    if result.is_err() {
        log::info!("git push: no upstream tracking, retrying with -u origin {branch}");
        run_git_checked(skills_dir, &["push", "-u", "origin", &branch])?;
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
        let remote_snapshot_tags_raw = run_git(
            skills_dir,
            &["ls-remote", "--tags", "--refs", "origin", "sm-v-*"],
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

/// Pull from the remote repository.
#[allow(dead_code)]
pub fn pull(skills_dir: &Path) -> Result<()> {
    let _lock = RepoLock::acquire_foreground("git pull")?;
    pull_unlocked(skills_dir)
}

pub(crate) fn pull_unlocked(skills_dir: &Path) -> Result<()> {
    ensure_repo(skills_dir)?;
    ensure_no_interrupted_git_operation(skills_dir)?;
    let branch = run_git(skills_dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|_| "main".to_string());
    log::info!("git pull: fetch + merge origin/{branch}");

    run_git_checked(skills_dir, &["fetch", "origin", &branch])?;
    if let Err(e) = run_git(skills_dir, &["merge", &format!("origin/{branch}")]) {
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
    log::info!("git pull: done");
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
        let message = run_git(skills_dir, &["log", "-1", "--format=%s", tag]).unwrap_or_default();
        let committed_at =
            run_git(skills_dir, &["log", "-1", "--format=%cI", tag]).unwrap_or_default();

        versions.push(GitBackupVersion {
            tag: tag.to_string(),
            commit: short_commit,
            message,
            committed_at,
        });
    }

    Ok(versions)
}

/// Restore skills files to a snapshot tag by creating a new restore commit.
#[allow(dead_code)]
pub fn restore_snapshot_version(skills_dir: &Path, tag: &str) -> Result<()> {
    let _lock = RepoLock::acquire_foreground("git restore snapshot")?;
    restore_snapshot_version_unlocked(skills_dir, tag)
}

pub(crate) fn restore_snapshot_version_unlocked(skills_dir: &Path, tag: &str) -> Result<()> {
    ensure_repo(skills_dir)?;

    if !tag.starts_with("sm-v-") {
        anyhow::bail!("Invalid snapshot tag");
    }
    run_git_checked(
        skills_dir,
        &["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")],
    )?;

    let status = run_git(skills_dir, &["status", "--porcelain"])?;
    if !status.is_empty() {
        anyhow::bail!("Working tree has uncommitted changes. Sync or commit before restore.");
    }

    log::info!("git restore: switching skills library to {tag}");

    // Keep a restore point before we mutate the working tree.
    let head_short = run_git(skills_dir, &["rev-parse", "--short", "HEAD"])?;
    let restore_point = format!(
        "sm-restore-point-{}-{}",
        Utc::now().format("%Y%m%d-%H%M%S"),
        head_short
    );
    run_git_checked(skills_dir, &["tag", &restore_point])?;

    let restore_result: Result<()> = (|| {
        // Align working tree + index to snapshot tree exactly (including deletions),
        // then commit as a forward change.
        run_git_checked(skills_dir, &["read-tree", "--reset", "-u", tag])?;

        let changed = run_git(skills_dir, &["status", "--porcelain"])?;
        if !changed.is_empty() {
            run_git_checked(
                skills_dir,
                &[
                    "commit",
                    "-m",
                    &format!("restore: switch skills library to {}", tag),
                ],
            )?;
        }
        Ok(())
    })();

    // Always clean up the restore-point tag, regardless of outcome.
    let cleanup = || {
        let _ = run_git(skills_dir, &["tag", "-d", &restore_point]);
    };

    match restore_result {
        Ok(()) => {
            cleanup();
            log::info!("git restore: completed restore to {tag}");
            Ok(())
        }
        Err(err) => {
            // Best-effort rollback to pre-restore HEAD.
            let _ = run_git_checked(skills_dir, &["read-tree", "--reset", "-u", &restore_point]);
            cleanup();
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
    let output = git_command()
        .arg("clone")
        .arg(url)
        .arg(skills_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();

    match output {
        Ok(o) if o.status.success() => {
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
        result => {
            // Restore backup on failure
            if let Some(backup) = backup_dir {
                let _ = std::fs::remove_dir_all(skills_dir);
                let _ = std::fs::rename(&backup, skills_dir);
            }
            match result {
                Ok(o) => {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    let detail = stderr.trim();
                    if detail.is_empty() {
                        log::warn!("git clone: failed with exit code {}", o.status);
                        anyhow::bail!("git clone failed with exit code {}", o.status)
                    } else {
                        let redacted = redact_urls_in_text(detail);
                        log::warn!("git clone: failed: {redacted}");
                        anyhow::bail!("git clone failed: {}", redacted)
                    }
                }
                Err(e) => Err(anyhow::Error::new(e).context("Failed to spawn git clone")),
            }
        }
    }
}

// ── Helpers ──

fn ensure_repo(skills_dir: &Path) -> Result<()> {
    if !skills_dir.join(".git").exists() {
        anyhow::bail!("Skills directory is not a git repository. Initialize it first.");
    }
    Ok(())
}

fn ensure_no_interrupted_git_operation(skills_dir: &Path) -> Result<()> {
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
    let output = git_command()
        .arg("-C")
        .arg(dir)
        .args(args)
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
    if let Err(e) = run_git(dir, args) {
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
