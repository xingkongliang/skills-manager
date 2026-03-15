use crate::core::skill_metadata;
use anyhow::{Context, Result};
use git2::{Direction, Repository};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const CLONE_TIMEOUT_SECS: u64 = 120;

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

#[derive(Debug, Clone)]
pub struct ParsedGitSource {
    pub original_url: String,
    pub clone_url: String,
    pub branch: Option<String>,
    pub subpath: Option<String>,
}

pub fn parse_git_source(url: &str) -> ParsedGitSource {
    let trimmed = url.trim().to_string();
    let (clone_url, branch, subpath) = normalize_url(&trimmed);

    ParsedGitSource {
        original_url: trimmed,
        clone_url,
        branch,
        subpath,
    }
}

pub fn clone_repo_ref(
    url: &str,
    branch: Option<&str>,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<PathBuf> {
    let temp_dir =
        std::env::temp_dir().join(format!("skills-manager-clone-{}", uuid::Uuid::new_v4()));
    let timeout = Duration::from_secs(CLONE_TIMEOUT_SECS);

    // Try system git first (faster, supports SSH)
    let mut command = git_command();
    command.arg("clone").arg("--depth").arg("1");
    if let Some(branch) = branch {
        command.arg("--branch").arg(branch);
    }
    let child = command
        .arg(url)
        .arg(&temp_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    if let Ok(mut child) = child {
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.map_or(false, |c| c.load(Ordering::SeqCst)) {
                let _ = child.kill();
                let _ = std::fs::remove_dir_all(&temp_dir);
                anyhow::bail!("Installation cancelled");
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    if status.success() {
                        return Ok(temp_dir);
                    }
                    break; // fall through to git2
                }
                Ok(None) => {
                    if Instant::now() > deadline {
                        let _ = child.kill();
                        let _ = std::fs::remove_dir_all(&temp_dir);
                        anyhow::bail!(
                            "Git clone timed out after {}s — check your network connection",
                            CLONE_TIMEOUT_SECS
                        );
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(_) => break,
            }
        }
    }

    // Fallback to git2
    let mut builder = git2::build::RepoBuilder::new();
    if let Some(branch) = branch {
        builder.branch(branch);
    }

    // git2: use transfer_progress callback for cancel checking
    let cancel_clone = cancel.cloned();
    let mut callbacks = git2::RemoteCallbacks::new();
    callbacks.transfer_progress(move |_progress| {
        if let Some(ref c) = cancel_clone {
            return !c.load(Ordering::SeqCst);
        }
        true
    });
    let mut fetch_opts = git2::FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);
    builder.fetch_options(fetch_opts);

    builder
        .clone(url, &temp_dir)
        .with_context(|| format!("Failed to clone {}", url))?;

    Ok(temp_dir)
}

pub fn get_head_revision(repo_dir: &Path) -> Result<String> {
    let output = git_command()
        .arg("-C")
        .arg(repo_dir)
        .args(["rev-parse", "HEAD"])
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }
    }

    let repo = Repository::open(repo_dir)?;
    let head = repo.head()?.peel_to_commit()?;
    Ok(head.id().to_string())
}

pub fn resolve_remote_revision(url: &str, branch: Option<&str>) -> Result<String> {
    if let Ok(revision) = resolve_remote_revision_with_git(url, branch) {
        return Ok(revision);
    }

    let repo = Repository::init_bare(std::env::temp_dir().join(format!(
        "skills-manager-remote-{}",
        uuid::Uuid::new_v4()
    )))?;
    let mut remote = repo.remote_anonymous(url)?;
    remote.connect(Direction::Fetch)?;
    let refs = remote.list()?;

    if let Some(branch) = branch {
        let target = format!("refs/heads/{branch}");
        if let Some(head) = refs.iter().find(|head| head.name() == target) {
            return Ok(head.oid().to_string());
        }
    } else {
        if let Some(head) = refs.iter().find(|head| head.name() == "HEAD") {
            return Ok(head.oid().to_string());
        }
    }

    anyhow::bail!("Unable to resolve remote revision for {}", url)
}

pub fn checkout_revision(repo_dir: &Path, revision: &str) -> Result<()> {
    let status = git_command()
        .arg("-C")
        .arg(repo_dir)
        .args(["checkout", "--detach", revision])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if let Ok(status) = status {
        if status.success() {
            return Ok(());
        }
    }

    let repo = Repository::open(repo_dir)?;
    let oid = git2::Oid::from_str(revision)?;
    repo.set_head_detached(oid)?;
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
    Ok(())
}

pub fn relative_subpath(repo_dir: &Path, skill_dir: &Path) -> Option<String> {
    let relative = skill_dir.strip_prefix(repo_dir).ok()?;
    if relative.as_os_str().is_empty() {
        None
    } else {
        Some(relative.to_string_lossy().to_string())
    }
}

fn normalize_url(url: &str) -> (String, Option<String>, Option<String>) {
    let trimmed = url.trim();

    // Already a full URL
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("git@")
    {
        if let Some((clone_url, branch, subpath)) = parse_github_tree_url(trimmed) {
            return (clone_url, Some(branch), subpath);
        }
        return (trimmed.to_string(), None, None);
    }

    // Shorthand: user/repo
    if trimmed.contains('/') && !trimmed.contains(' ') {
        return (format!("https://github.com/{}.git", trimmed), None, None);
    }

    (trimmed.to_string(), None, None)
}

pub fn find_skill_dir(repo_dir: &Path, skill_id: Option<&str>) -> Result<PathBuf> {
    // If skill_id provided, look for it specifically
    if let Some(id) = skill_id {
        let direct = repo_dir.join(id);
        if direct.exists() && direct.is_dir() {
            return Ok(direct);
        }

        let in_skills = repo_dir.join("skills").join(id);
        if in_skills.exists() && in_skills.is_dir() {
            return Ok(in_skills);
        }

        // Recursive search: match by directory name or SKILL.md name field
        let mut name_match: Option<PathBuf> = None;
        for entry in walkdir::WalkDir::new(repo_dir).max_depth(3) {
            if let Ok(e) = entry {
                if e.file_type().is_dir() {
                    if e.file_name().to_string_lossy() == id {
                        return Ok(e.path().to_path_buf());
                    }
                    if name_match.is_none() {
                        let meta = skill_metadata::parse_skill_md(e.path());
                        if meta.name.as_deref() == Some(id) {
                            name_match = Some(e.path().to_path_buf());
                        }
                    }
                }
            }
        }
        if let Some(path) = name_match {
            return Ok(path);
        }
    }

    // Check if root is a skill
    let has_skill_md = ["SKILL.md", "skill.md", "CLAUDE.md"]
        .iter()
        .any(|f| repo_dir.join(f).exists());
    if has_skill_md {
        return Ok(repo_dir.to_path_buf());
    }

    // Check skills/ subdirectory
    let skills_subdir = repo_dir.join("skills");
    if skills_subdir.is_dir() {
        return Ok(skills_subdir);
    }

    let skill_subdir = repo_dir.join("skill");
    if skill_subdir.is_dir() {
        return Ok(skill_subdir);
    }

    // Default to root
    Ok(repo_dir.to_path_buf())
}

pub fn cleanup_temp(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}

fn parse_github_tree_url(url: &str) -> Option<(String, String, Option<String>)> {
    let re = regex::Regex::new(
        r"^(https://github\.com/[^/]+/[^/]+?)(?:\.git)?/tree/([^/]+)(?:/(.+))?$",
    )
    .ok()?;
    let caps = re.captures(url)?;
    let clone_url = format!("{}.git", caps.get(1)?.as_str());
    let branch = caps.get(2)?.as_str().to_string();
    let subpath = caps.get(3).map(|m| m.as_str().to_string());
    Some((clone_url, branch, subpath))
}

fn resolve_remote_revision_with_git(url: &str, branch: Option<&str>) -> Result<String> {
    let target = branch
        .map(|branch| format!("refs/heads/{branch}"))
        .unwrap_or_else(|| "HEAD".to_string());
    let output = git_command()
        .args(["ls-remote", url, &target])
        .output()
        .with_context(|| format!("Failed to query remote {}", url))?;

    if !output.status.success() {
        anyhow::bail!("git ls-remote exited with {}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let revision = stdout
        .lines()
        .find_map(|line| line.split_whitespace().next())
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("No remote revision found"))?;

    Ok(revision)
}

#[cfg(test)]
mod tests {
    use super::parse_git_source;

    #[test]
    fn parses_github_tree_urls() {
        let parsed = parse_git_source("https://github.com/acme/skills/tree/main/tools/my-skill");
        assert_eq!(parsed.clone_url, "https://github.com/acme/skills.git");
        assert_eq!(parsed.branch.as_deref(), Some("main"));
        assert_eq!(parsed.subpath.as_deref(), Some("tools/my-skill"));
    }

    #[test]
    fn parses_shorthand_urls() {
        let parsed = parse_git_source("acme/skills");
        assert_eq!(parsed.clone_url, "https://github.com/acme/skills.git");
        assert_eq!(parsed.branch, None);
        assert_eq!(parsed.subpath, None);
    }
}
