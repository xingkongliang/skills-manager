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
    })
}

/// Initialize a new git repository in the skills directory.
pub fn init_repo(skills_dir: &Path) -> Result<()> {
    let _lock = RepoLock::acquire(skills_dir, "git init")?;
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

    Ok(())
}

/// Set (or update) the remote origin URL.
pub fn set_remote(skills_dir: &Path, url: &str) -> Result<()> {
    let _lock = RepoLock::acquire(skills_dir, "git set remote")?;
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
    let _ = run_git(skills_dir, &["fetch", "origin"]);

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

    Ok(())
}

/// Stage all changes and create a commit.
#[allow(dead_code)]
pub fn commit_all(skills_dir: &Path, message: &str) -> Result<()> {
    let _lock = RepoLock::acquire(skills_dir, "git commit")?;
    commit_all_unlocked(skills_dir, message)
}

pub(crate) fn commit_all_unlocked(skills_dir: &Path, message: &str) -> Result<()> {
    ensure_repo(skills_dir)?;
    ensure_gitignore(skills_dir)?;

    run_git_checked(skills_dir, &["add", "-A"])?;

    // Check if there's anything to commit
    let status = run_git(skills_dir, &["status", "--porcelain"])?;
    if status.is_empty() {
        anyhow::bail!("Nothing to commit");
    }

    run_git_checked(skills_dir, &["commit", "-m", message])?;
    Ok(())
}

/// Push to the remote repository.
pub fn push(skills_dir: &Path) -> Result<()> {
    let _lock = RepoLock::acquire(skills_dir, "git push")?;
    push_unlocked(skills_dir)
}

pub(crate) fn push_unlocked(skills_dir: &Path) -> Result<()> {
    ensure_repo(skills_dir)?;

    let branch = run_git(skills_dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|_| "main".to_string());

    // Push branch first; if no upstream, set it.
    let result = run_git(skills_dir, &["push"]);
    if result.is_err() {
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
            let output = cmd.output().context("Failed to run git command")?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                anyhow::bail!("git command failed: {}", redact_urls_in_text(&stderr));
            }
        }
    }

    Ok(())
}

/// Pull from the remote repository.
#[allow(dead_code)]
pub fn pull(skills_dir: &Path) -> Result<()> {
    let _lock = RepoLock::acquire(skills_dir, "git pull")?;
    pull_unlocked(skills_dir)
}

pub(crate) fn pull_unlocked(skills_dir: &Path) -> Result<()> {
    ensure_repo(skills_dir)?;
    ensure_no_interrupted_git_operation(skills_dir)?;
    run_git_checked(skills_dir, &["pull", "--no-rebase"])?;
    Ok(())
}

/// Create an annotated snapshot tag on current HEAD.
pub fn create_snapshot_tag(skills_dir: &Path) -> Result<String> {
    let _lock = RepoLock::acquire(skills_dir, "git snapshot")?;
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
        return Ok(tag.trim().to_string());
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
    let _lock = RepoLock::acquire(skills_dir, "git restore snapshot")?;
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
    let _lock = RepoLock::acquire(skills_dir, "git clone")?;
    clone_into_unlocked(skills_dir, url)
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

    // Clone
    let status = git_command()
        .arg("clone")
        .arg(url)
        .arg(skills_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status();

    match status {
        Ok(s) if s.success() => {
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
            Ok(())
        }
        _ => {
            // Restore backup on failure
            if let Some(backup) = backup_dir {
                let _ = std::fs::remove_dir_all(skills_dir);
                let _ = std::fs::rename(&backup, skills_dir);
            }
            anyhow::bail!("Failed to clone repository")
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

pub(crate) fn with_repo_lock<T, F>(repo_root: &Path, operation: &str, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    let _lock = RepoLock::acquire(repo_root, operation)?;
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
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("git command failed: {}", redact_urls_in_text(&stderr))
    }
}

fn run_git_checked(dir: &Path, args: &[&str]) -> Result<()> {
    run_git(dir, args)?;
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
    fn parse_restored_tag_with_trailing_whitespace() {
        let msg = "restore: switch skills library to sm-v-20260318-153012-abc1234  ";
        assert_eq!(
            parse_restored_from_tag_message(msg).as_deref(),
            Some("sm-v-20260318-153012-abc1234")
        );
    }
}
