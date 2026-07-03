use crate::core::central_repo;
use crate::core::skill_metadata;
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use git2::{Direction, Repository};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{ChildStderr, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const CLONE_TIMEOUT_SECS: u64 = 300;

/// Filename prefix shared by isolated install checkouts under `std::env::temp_dir()`.
/// Used by both `materialize_cached_repo` (writer) and `validate_clone_temp_path` (reader).
pub const CLONE_TEMP_PREFIX: &str = "skills-manager-clone-";

/// Callback type for reporting clone progress messages to the UI.
pub type ProgressCallback = Box<dyn Fn(&str) + Send>;

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

/// Validate that a URL uses an allowed scheme for git operations.
/// Only permits `https://`, `http://`, `ssh://`, and SCP-style `git@` URLs,
/// plus shorthand like `user/repo` (no scheme). Rejects everything else
/// including `file://`, `ext::`, bare local paths, and UNC paths.
pub fn validate_git_url(url: &str) -> Result<()> {
    let trimmed = url.trim();
    let lower = trimmed.to_lowercase();

    // Explicitly allowed schemes
    if lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("ssh://")
        || lower.starts_with("git@")
    {
        return Ok(());
    }

    // Allow GitHub/GitLab shorthand like "user/repo" or "user/repo.git"
    if !trimmed.contains("://")
        && !trimmed.contains('\\')
        && !trimmed.starts_with('/')
        && !trimmed.starts_with('.')
        && !trimmed.starts_with('~')
        && trimmed.contains('/')
    {
        let bytes = trimmed.as_bytes();
        let is_windows_path =
            bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
        if !is_windows_path {
            return Ok(());
        }
    }

    anyhow::bail!("URL scheme not allowed: only https, http, ssh, and git@ are permitted");
}

/// Strip clone-irrelevant differences from a URL so equivalent forms share a
/// cache slot and pass remote-equality checks. Specifically:
/// - Trims whitespace and a trailing `/`.
/// - Drops a trailing `.git` suffix (servers accept both forms).
///
/// This is intentionally conservative — no case folding, no scheme rewriting,
/// no path normalization beyond the suffix — so non-GitHub hosts that treat
/// paths case-sensitively or distinguish schemes are unaffected.
fn canonicalize_clone_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    trimmed
        .strip_suffix(".git")
        .unwrap_or(trimmed)
        .to_string()
}

/// Compute a stable cache directory name for a given clone URL. Hashes the
/// canonical form so e.g. `https://github.com/x/y` and `https://github.com/x/y.git`
/// share the same cache slot.
fn repo_cache_dir(url: &str) -> PathBuf {
    let canonical = canonicalize_clone_url(url);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let short = &hash[..16];
    central_repo::cache_dir().join("repos").join(short)
}

struct RepoCacheLock {
    _file: File,
}

fn lock_repo_cache(
    cached_dir: &Path,
    on_progress: &Option<ProgressCallback>,
) -> Result<RepoCacheLock> {
    if let Some(parent) = cached_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let lock_path = cached_dir.with_extension("lock");
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("Failed to open repo cache lock {}", lock_path.display()))?;

    // Try non-blocking first; if contended, surface a progress message before blocking.
    if file.try_lock_exclusive().is_err() {
        if let Some(cb) = on_progress {
            cb("Waiting for another install of this repository to finish…");
        }
        file.lock_exclusive()
            .with_context(|| format!("Failed to lock repo cache {}", lock_path.display()))?;
    }
    Ok(RepoCacheLock { _file: file })
}

fn materialize_cached_repo(
    cached: &Path,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<PathBuf> {
    let temp_dir =
        std::env::temp_dir().join(format!("{CLONE_TEMP_PREFIX}{}", uuid::Uuid::new_v4()));

    // `git clone --local` with default hardlinks: cache objects are content-addressed
    // and immutable, the flock above prevents the cache from being mutated mid-clone,
    // and any later cache deletion leaves linked objects intact in the temp checkout.
    let child = git_command()
        .arg("clone")
        .arg("--local")
        .arg(cached)
        .arg(&temp_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();

    let mut system_git_stderr: Option<String> = None;
    if let Ok(mut child) = child {
        let deadline = Instant::now() + Duration::from_secs(CLONE_TIMEOUT_SECS);
        loop {
            if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_dir_all(&temp_dir);
                anyhow::bail!("Installation cancelled");
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    if status.success() {
                        return Ok(temp_dir);
                    }
                    let mut stderr_buf = String::new();
                    if let Some(mut stderr) = child.stderr.take() {
                        let _ = stderr.read_to_string(&mut stderr_buf);
                    }
                    let _ = std::fs::remove_dir_all(&temp_dir);
                    system_git_stderr = Some(stderr_buf);
                    break;
                }
                Ok(None) => {
                    if Instant::now() > deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        let _ = std::fs::remove_dir_all(&temp_dir);
                        anyhow::bail!(
                            "Local clone from cache timed out after {}s",
                            CLONE_TIMEOUT_SECS
                        );
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(_) => {
                    let _ = std::fs::remove_dir_all(&temp_dir);
                    break;
                }
            }
        }
    }

    let source = cached
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Cached repo path is not valid UTF-8"))?;
    match git2::build::RepoBuilder::new().clone(source, &temp_dir) {
        Ok(_) => Ok(temp_dir),
        Err(err) => {
            let _ = std::fs::remove_dir_all(&temp_dir);
            let detail = system_git_stderr
                .filter(|s| !s.trim().is_empty())
                .map(|s| format!(" (system git: {})", s.trim()))
                .unwrap_or_default();
            anyhow::bail!(
                "Failed to create install checkout from cache: {}{}",
                err,
                detail
            )
        }
    }
}

/// Try to update an existing cached repo via fetch + reset.
/// Returns Ok(true) if the cache was reused, Ok(false) if it should be re-cloned.
fn try_update_cached_repo(
    cached: &Path,
    url: &str,
    branch: Option<&str>,
    proxy_url: Option<&str>,
    cancel: Option<&Arc<AtomicBool>>,
    on_progress: &Option<ProgressCallback>,
) -> Result<bool> {
    if !cached.join(".git").exists() {
        return Ok(false);
    }

    // Verify the remote URL still matches.
    let current_remote = {
        let mut cmd = git_command();
        cmd.arg("-C")
            .arg(cached)
            .args(["remote", "get-url", "origin"]);
        let output = cmd.output().ok();
        output
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let remote_matches = current_remote
        .as_deref()
        .is_some_and(|r| canonicalize_clone_url(r) == canonicalize_clone_url(url));
    if !remote_matches {
        // URL changed — discard cache.
        let _ = std::fs::remove_dir_all(cached);
        return Ok(false);
    }

    if let Some(cb) = on_progress {
        cb("Updating cached repository…");
    }

    let mut fetch_cmd = git_command();
    fetch_cmd
        .arg("-C")
        .arg(cached)
        .arg("fetch")
        .arg("--depth")
        .arg("1");
    if let Some(proxy) = proxy_url.filter(|s| !s.is_empty()) {
        fetch_cmd.arg("-c").arg(format!("http.proxy={proxy}"));
        fetch_cmd.arg("-c").arg(format!("https.proxy={proxy}"));
    }
    fetch_cmd.arg("origin");
    if let Some(branch) = branch {
        fetch_cmd.arg(branch);
    }
    fetch_cmd.stdout(Stdio::null()).stderr(Stdio::null());

    let child = fetch_cmd.spawn();
    if let Ok(mut child) = child {
        let deadline = Instant::now() + Duration::from_secs(CLONE_TIMEOUT_SECS);
        loop {
            if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("Installation cancelled");
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        // Fetch failed — discard cache and re-clone.
                        let _ = std::fs::remove_dir_all(cached);
                        return Ok(false);
                    }
                    break;
                }
                Ok(None) => {
                    if Instant::now() > deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        let _ = std::fs::remove_dir_all(cached);
                        return Ok(false);
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(_) => {
                    let _ = std::fs::remove_dir_all(cached);
                    return Ok(false);
                }
            }
        }
    } else {
        let _ = std::fs::remove_dir_all(cached);
        return Ok(false);
    }

    // Reset to the fetched HEAD.
    let target = branch
        .map(|b| format!("origin/{b}"))
        .unwrap_or_else(|| "origin/HEAD".to_string());
    let reset_status = git_command()
        .arg("-C")
        .arg(cached)
        .args(["reset", "--hard", &target])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match reset_status {
        Ok(s) if s.success() => Ok(true),
        _ => {
            let _ = std::fs::remove_dir_all(cached);
            Ok(false)
        }
    }
}

/// Filter out SSH informational warnings from stderr lines.
fn is_ssh_warning(line: &str) -> bool {
    let trimmed = line.trim().trim_start_matches("** ");
    trimmed.starts_with("WARNING:")
        || trimmed.starts_with("This session may")
        || trimmed.starts_with("The server may")
        || trimmed.starts_with("See https://openssh.com")
}

fn spawn_stderr_collector(
    stderr: Option<ChildStderr>,
    forward_progress: bool,
) -> (
    std::sync::mpsc::Receiver<String>,
    std::thread::JoinHandle<String>,
) {
    let (stderr_tx, stderr_rx) = std::sync::mpsc::channel::<String>();
    let stderr_thread = std::thread::spawn(move || {
        let mut collected = String::new();
        let Some(stderr) = stderr else {
            return collected;
        };

        let mut reader = BufReader::new(stderr);
        let mut line = Vec::new();
        let mut byte = [0u8; 1];

        loop {
            match reader.read(&mut byte) {
                Ok(0) => {
                    if !line.is_empty() {
                        emit_stderr_line(&line, forward_progress, &stderr_tx, &mut collected);
                    }
                    break;
                }
                Ok(_) if byte[0] == b'\n' || byte[0] == b'\r' => {
                    if !line.is_empty() {
                        emit_stderr_line(&line, forward_progress, &stderr_tx, &mut collected);
                        line.clear();
                    }
                }
                Ok(_) => line.push(byte[0]),
                Err(_) => break,
            }
        }

        collected
    });

    (stderr_rx, stderr_thread)
}

fn emit_stderr_line(
    line: &[u8],
    forward_progress: bool,
    stderr_tx: &std::sync::mpsc::Sender<String>,
    collected: &mut String,
) {
    let line = String::from_utf8_lossy(line).to_string();
    if !is_ssh_warning(&line) {
        collected.push_str(&line);
        collected.push('\n');
    }
    if forward_progress {
        let _ = stderr_tx.send(line);
    }
}

pub fn clone_repo_ref(
    url: &str,
    branch: Option<&str>,
    cancel: Option<&Arc<AtomicBool>>,
    proxy_url: Option<&str>,
) -> Result<PathBuf> {
    clone_repo_ref_with_progress(url, branch, cancel, proxy_url, None)
}

pub fn clone_repo_ref_with_progress(
    url: &str,
    branch: Option<&str>,
    cancel: Option<&Arc<AtomicBool>>,
    proxy_url: Option<&str>,
    on_progress: Option<ProgressCallback>,
) -> Result<PathBuf> {
    let cached_dir = repo_cache_dir(url);
    let _cache_lock = lock_repo_cache(&cached_dir, &on_progress)?;

    // Try cached repo first.
    if cached_dir.exists() {
        match try_update_cached_repo(&cached_dir, url, branch, proxy_url, cancel, &on_progress) {
            Ok(true) => return materialize_cached_repo(&cached_dir, cancel),
            Ok(false) => { /* cache invalid, fall through to clone */ }
            Err(e) => {
                // Propagate cancellation.
                if e.to_string().contains("cancelled") || e.to_string().contains("canceled") {
                    return Err(e);
                }
                // Otherwise fall through to clone.
            }
        }
    }

    // Remove any leftover partial clone.
    let _ = std::fs::remove_dir_all(&cached_dir);

    let timeout = Duration::from_secs(CLONE_TIMEOUT_SECS);
    let mut system_git_stderr: Option<String> = None;

    // Try system git first (faster, supports SSH).
    let mut command = git_command();
    command.arg("clone").arg("--depth").arg("1");
    if let Some(proxy) = proxy_url.filter(|s| !s.is_empty()) {
        command.arg("-c").arg(format!("http.proxy={proxy}"));
        command.arg("-c").arg(format!("https.proxy={proxy}"));
    }
    if let Some(branch) = branch {
        command.arg("--branch").arg(branch);
    }
    command.arg("--progress"); // Force progress output to stderr.
    let child = command
        .arg(url)
        .arg(&cached_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();

    if let Ok(mut child) = child {
        let (stderr_rx, stderr_thread) =
            spawn_stderr_collector(child.stderr.take(), on_progress.is_some());

        let deadline = Instant::now() + timeout;
        loop {
            if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_dir_all(&cached_dir);
                anyhow::bail!("Installation cancelled");
            }

            // Forward progress lines from stderr thread.
            if let Some(ref cb) = on_progress {
                while let Ok(line) = stderr_rx.try_recv() {
                    if !is_ssh_warning(&line) && !line.trim().is_empty() {
                        cb(&line);
                    }
                }
            }

            match child.try_wait() {
                Ok(Some(status)) => {
                    let collected = stderr_thread.join().unwrap_or_default();
                    if status.success() {
                        return materialize_cached_repo(&cached_dir, cancel);
                    }
                    system_git_stderr = Some(collected);
                    // Clean up failed clone.
                    let _ = std::fs::remove_dir_all(&cached_dir);
                    break; // fall through to git2
                }
                Ok(None) => {
                    if Instant::now() > deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        let _ = std::fs::remove_dir_all(&cached_dir);
                        anyhow::bail!(
                            "Git clone timed out after {}s — check your network connection",
                            CLONE_TIMEOUT_SECS
                        );
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(_) => {
                    let _ = std::fs::remove_dir_all(&cached_dir);
                    break;
                }
            }
        }
    }

    // Fallback to git2 with timeout and shallow clone.
    if let Some(ref cb) = on_progress {
        cb("Trying alternative clone method…");
    }

    let mut builder = git2::build::RepoBuilder::new();
    if let Some(branch) = branch {
        builder.branch(branch);
    }

    let cancel_clone = cancel.cloned();
    let clone_deadline = Instant::now() + Duration::from_secs(CLONE_TIMEOUT_SECS);
    let mut callbacks = git2::RemoteCallbacks::new();

    let progress_for_cb: Option<Arc<std::sync::Mutex<ProgressCallback>>> =
        on_progress.map(|cb| Arc::new(std::sync::Mutex::new(cb)));
    let progress_for_transfer = progress_for_cb.clone();

    callbacks.transfer_progress(move |stats| {
        if let Some(ref c) = cancel_clone {
            if c.load(Ordering::SeqCst) {
                return false;
            }
        }
        if Instant::now() > clone_deadline {
            return false;
        }
        if let Some(ref cb) = progress_for_transfer {
            if let Ok(cb) = cb.lock() {
                let msg = format!(
                    "Receiving objects: {}/{} ({:.1} KB)",
                    stats.received_objects(),
                    stats.total_objects(),
                    stats.received_bytes() as f64 / 1024.0
                );
                cb(&msg);
            }
        }
        true
    });

    let mut fetch_opts = git2::FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);
    fetch_opts.depth(1);
    if let Some(proxy) = proxy_url.filter(|s| !s.is_empty()) {
        let mut proxy_opts = git2::ProxyOptions::new();
        proxy_opts.url(proxy);
        fetch_opts.proxy_options(proxy_opts);
    }
    builder.fetch_options(fetch_opts);

    match builder.clone(url, &cached_dir) {
        Ok(_) => materialize_cached_repo(&cached_dir, cancel),
        Err(git2_err) => {
            let _ = std::fs::remove_dir_all(&cached_dir);
            // Include system git stderr in the error if available.
            let detail = system_git_stderr
                .filter(|s| !s.trim().is_empty())
                .map(|s| format!(" (system git: {})", s.trim()))
                .unwrap_or_default();
            anyhow::bail!("Failed to clone {}: {}{}", url, git2_err, detail)
        }
    }
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

pub fn resolve_remote_revision(
    url: &str,
    branch: Option<&str>,
    proxy_url: Option<&str>,
) -> Result<String> {
    if let Ok(revision) = resolve_remote_revision_with_git(url, branch, proxy_url) {
        return Ok(revision);
    }

    let repo = Repository::init_bare(
        std::env::temp_dir().join(format!("skills-manager-remote-{}", uuid::Uuid::new_v4())),
    )?;
    let mut remote = repo.remote_anonymous(url)?;
    let mut proxy_opts = git2::ProxyOptions::new();
    if let Some(proxy) = proxy_url.filter(|s| !s.is_empty()) {
        proxy_opts.url(proxy);
    }
    remote.connect_auth(Direction::Fetch, None, Some(proxy_opts))?;
    let refs = remote.list()?;

    if let Some(branch) = branch {
        let target = format!("refs/heads/{branch}");
        if let Some(head) = refs.iter().find(|head| head.name() == target) {
            return Ok(head.oid().to_string());
        }
    } else if let Some(head) = refs.iter().find(|head| head.name() == "HEAD") {
        return Ok(head.oid().to_string());
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
    // If skill_id provided, look for it specifically. Each branch must validate
    // that the resolved dir actually contains a SKILL.md — otherwise it returns
    // a container/category dir that the installer will reject later with a
    // worse error.
    if let Some(id) = skill_id {
        let direct = repo_dir.join(id);
        if skill_metadata::is_valid_skill_dir(&direct) {
            return Ok(direct);
        }

        let in_skills = repo_dir.join("skills").join(id);
        if skill_metadata::is_valid_skill_dir(&in_skills) {
            return Ok(in_skills);
        }

        // Prefer the unified agent skills location over provider-specific
        // variants (e.g. `.cursor/skills`, `.claude/skills`). Repos that
        // publish per-provider variants typically also ship a generic copy
        // at `.agents/skills`; the recursive fallback below is non-deterministic
        // across filesystems, so without this check we may pick an arbitrary
        // provider's transformed variant.
        let in_agents = repo_dir.join(".agents").join("skills").join(id);
        if skill_metadata::is_valid_skill_dir(&in_agents) {
            return Ok(in_agents);
        }

        // Recursive search: match by directory name or SKILL.md name field.
        // The basename branch must check validity; the SKILL.md-name branch is
        // implicitly validated by parsing the frontmatter.
        let mut name_match: Option<PathBuf> = None;
        for e in walkdir::WalkDir::new(repo_dir)
            .max_depth(6)
            .into_iter()
            .flatten()
        {
            if e.file_type().is_dir() {
                if e.file_name().to_string_lossy() == id
                    && skill_metadata::is_valid_skill_dir(e.path())
                {
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
        if let Some(path) = name_match {
            return Ok(path);
        }

        // A specific skill id was requested but nothing matched. Error instead
        // of falling through to a container/root — otherwise the installer would
        // copy the entire `skills/` container (or an unrelated root skill) under
        // the requested name, duplicating every skill in the repo. See issue #278.
        bail!("Skill '{}' not found in {}", id, repo_dir.display());
    }

    // No skill id requested — resolve a repo-wide skill location. The fallbacks
    // below are intended for enumeration flows (collect_git_skill_dirs walks the
    // resolved container to list individual skills), not for a single install.
    // Check if root is a skill
    let has_skill_md = ["SKILL.md", "skill.md"]
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
    let (clone_url, path) = parse_github_tree_url_path(url)?;
    let (branch, subpath) = split_tree_branch_path(&path, &[]);
    Some((clone_url, branch, subpath))
}

/// Match a GitHub `…/tree/<path>` URL and return `(clone_url, path_after_tree)`.
/// The path is left unsplit so callers can disambiguate branch-vs-subpath with
/// knowledge of the actual remote branches.
fn parse_github_tree_url_path(url: &str) -> Option<(String, String)> {
    let re = regex::Regex::new(r"^(https://github\.com/[^/]+/[^/]+?)(?:\.git)?/tree/(.+)$").ok()?;
    let caps = re.captures(url)?;
    let clone_url = format!("{}.git", caps.get(1)?.as_str());
    let path = caps.get(2)?.as_str().to_string();
    Some((clone_url, path))
}

/// Split a `tree/<path>` tail into `(branch, optional subpath)`.
///
/// GitHub tree URLs are ambiguous when the branch name contains a `/`
/// (e.g. `feature/x`). With an empty `known_branches`, fall back to the
/// optimistic interpretation: first segment is the branch, rest is subpath
/// (correct for the vast majority of repos that use single-segment branch names).
/// With a populated `known_branches`, pick the longest branch that matches a
/// `/`-bounded prefix of `path`.
fn split_tree_branch_path(path: &str, known_branches: &[String]) -> (String, Option<String>) {
    if !known_branches.is_empty() {
        let mut best: Option<&str> = None;
        for branch in known_branches {
            if branch.is_empty() {
                continue;
            }
            let matches = path == branch.as_str()
                || path
                    .strip_prefix(branch.as_str())
                    .is_some_and(|rest| rest.starts_with('/'));
            if matches && best.is_none_or(|b: &str| branch.len() > b.len()) {
                best = Some(branch);
            }
        }
        if let Some(branch) = best {
            let subpath = path
                .strip_prefix(branch)
                .and_then(|rest| rest.strip_prefix('/'))
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            return (branch.to_string(), subpath);
        }
    }

    let mut parts = path.splitn(2, '/');
    let branch = parts.next().unwrap_or("").to_string();
    let subpath = parts.next().filter(|s| !s.is_empty()).map(|s| s.to_string());
    (branch, subpath)
}

/// Network-aware variant of `parse_git_source`. For GitHub `…/tree/<path>` URLs
/// with multi-segment paths, queries `git ls-remote --heads` to resolve which
/// segments belong to the branch and which to the subpath. Falls back to the
/// optimistic (single-segment-branch) parse on network failure.
pub fn parse_git_source_resolved(url: &str, proxy_url: Option<&str>) -> ParsedGitSource {
    let mut parsed = parse_git_source(url);
    let original_url = parsed.original_url.clone();

    let Some((clone_url, path)) = parse_github_tree_url_path(&original_url) else {
        return parsed;
    };
    // No ambiguity if the tree path is a single segment.
    if !path.contains('/') {
        return parsed;
    }

    let branches = match list_remote_branches(&clone_url, proxy_url) {
        Ok(b) if !b.is_empty() => b,
        Ok(_) => {
            log::warn!(
                "ls-remote returned no branches for {}; using optimistic tree-URL parse",
                clone_url
            );
            return parsed;
        }
        Err(e) => {
            log::warn!(
                "ls-remote failed for {}: {} — using optimistic tree-URL parse (slash-branch URLs may parse incorrectly)",
                clone_url,
                e
            );
            return parsed;
        }
    };
    let (branch, subpath) = split_tree_branch_path(&path, &branches);
    parsed.branch = Some(branch);
    parsed.subpath = subpath;
    parsed
}

fn list_remote_branches(url: &str, proxy_url: Option<&str>) -> Result<Vec<String>> {
    let mut cmd = git_command();
    if let Some(proxy) = proxy_url.filter(|s| !s.is_empty()) {
        cmd.arg("-c").arg(format!("http.proxy={proxy}"));
        cmd.arg("-c").arg(format!("https.proxy={proxy}"));
    }
    let output = cmd
        .args(["ls-remote", "--heads", url])
        .output()
        .with_context(|| format!("Failed to list remote branches for {}", url))?;

    if !output.status.success() {
        anyhow::bail!("git ls-remote --heads exited with {}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let branches: Vec<String> = stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let _sha = parts.next()?;
            let refname = parts.next()?;
            refname
                .strip_prefix("refs/heads/")
                .map(|name| name.to_string())
        })
        .collect();
    Ok(branches)
}

fn resolve_remote_revision_with_git(
    url: &str,
    branch: Option<&str>,
    proxy_url: Option<&str>,
) -> Result<String> {
    let target = branch
        .map(|branch| format!("refs/heads/{branch}"))
        .unwrap_or_else(|| "HEAD".to_string());
    let mut cmd = git_command();
    if let Some(proxy) = proxy_url.filter(|s| !s.is_empty()) {
        cmd.arg("-c").arg(format!("http.proxy={proxy}"));
        cmd.arg("-c").arg(format!("https.proxy={proxy}"));
    }
    let output = cmd
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
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // ── parse_git_source ──

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

    #[test]
    fn parses_github_tree_url_branch_only() {
        let parsed = parse_git_source("https://github.com/acme/skills/tree/develop");
        assert_eq!(parsed.clone_url, "https://github.com/acme/skills.git");
        assert_eq!(parsed.branch.as_deref(), Some("develop"));
        assert_eq!(parsed.subpath, None);
    }

    #[test]
    fn parses_full_https_url() {
        let parsed = parse_git_source("https://github.com/acme/skills.git");
        assert_eq!(parsed.clone_url, "https://github.com/acme/skills.git");
        assert_eq!(parsed.branch, None);
        assert_eq!(parsed.subpath, None);
    }

    #[test]
    fn parses_git_ssh_url() {
        let parsed = parse_git_source("git@github.com:acme/skills.git");
        assert_eq!(parsed.clone_url, "git@github.com:acme/skills.git");
        assert_eq!(parsed.branch, None);
        assert_eq!(parsed.subpath, None);
    }

    #[test]
    fn preserves_original_url() {
        let input = "  acme/skills  ";
        let parsed = parse_git_source(input);
        assert_eq!(parsed.original_url, "acme/skills");
    }

    #[test]
    fn handles_plain_string_no_slash() {
        let parsed = parse_git_source("something");
        assert_eq!(parsed.clone_url, "something");
    }

    #[test]
    fn normalize_http_url_passthrough() {
        let parsed = parse_git_source("http://gitlab.example.com/repo.git");
        assert_eq!(parsed.clone_url, "http://gitlab.example.com/repo.git");
        assert_eq!(parsed.branch, None);
    }

    // ── find_skill_dir ──

    #[test]
    fn find_skill_dir_root_with_skill_md() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("SKILL.md"), "---\nname: foo\n---").unwrap();
        let found = find_skill_dir(tmp.path(), None).unwrap();
        assert_eq!(found, tmp.path());
    }

    #[test]
    fn find_skill_dir_root_with_claude_md() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("CLAUDE.md"), "# instructions").unwrap();
        let found = find_skill_dir(tmp.path(), None).unwrap();
        assert_eq!(found, tmp.path());
    }

    #[test]
    fn find_skill_dir_skills_subdirectory() {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let found = find_skill_dir(tmp.path(), None).unwrap();
        assert_eq!(found, tmp.path().join("skills"));
    }

    #[test]
    fn find_skill_dir_skill_subdirectory() {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("skill")).unwrap();
        let found = find_skill_dir(tmp.path(), None).unwrap();
        assert_eq!(found, tmp.path().join("skill"));
    }

    #[test]
    fn find_skill_dir_by_id_direct() {
        let tmp = tempdir().unwrap();
        let skill = tmp.path().join("my-skill");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "content").unwrap();
        let found = find_skill_dir(tmp.path(), Some("my-skill")).unwrap();
        assert_eq!(found, skill);
    }

    #[test]
    fn find_skill_dir_by_id_in_skills_subdir() {
        let tmp = tempdir().unwrap();
        let skill = tmp.path().join("skills").join("my-skill");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "content").unwrap();
        let found = find_skill_dir(tmp.path(), Some("my-skill")).unwrap();
        assert_eq!(found, skill);
    }

    #[test]
    fn find_skill_dir_prefers_agents_skills_over_provider_variants() {
        // When a repo publishes both the unified `.agents/skills/<id>` variant
        // and provider-specific variants (e.g. `.cursor/skills/<id>`), we must
        // pick the unified one — the recursive fallback is non-deterministic
        // across filesystems.
        let tmp = tempdir().unwrap();
        let agents = tmp.path().join(".agents").join("skills").join("my-skill");
        let cursor = tmp.path().join(".cursor").join("skills").join("my-skill");
        let claude = tmp.path().join(".claude").join("skills").join("my-skill");
        for dir in [&agents, &cursor, &claude] {
            fs::create_dir_all(dir).unwrap();
            fs::write(dir.join("SKILL.md"), "content").unwrap();
        }
        let found = find_skill_dir(tmp.path(), Some("my-skill")).unwrap();
        assert_eq!(found, agents);
    }

    #[test]
    fn find_skill_dir_skips_invalid_basename_match() {
        // A directory with the right basename but no SKILL.md must NOT be
        // returned; the search should continue and find the real skill deeper.
        let tmp = tempdir().unwrap();
        let bogus = tmp.path().join("my-skill"); // bogus: dir name matches but empty
        fs::create_dir_all(&bogus).unwrap();
        let real = tmp.path().join("category").join("my-skill");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("SKILL.md"), "content").unwrap();

        let found = find_skill_dir(tmp.path(), Some("my-skill")).unwrap();
        assert_eq!(found, real);
    }

    #[test]
    fn find_skill_dir_errors_when_skill_id_missing_even_if_root_is_skill() {
        // When a skill id is requested but doesn't match, we must error even if
        // the repo root happens to be a skill — previously this fell back to the
        // root and installed an unrelated skill under the requested name (#278).
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("SKILL.md"), "---\nname: root\n---").unwrap();
        let bogus_dir = tmp.path().join("my-skill");
        fs::create_dir_all(&bogus_dir).unwrap();

        let found = find_skill_dir(tmp.path(), Some("my-skill"));
        assert!(
            found.is_err(),
            "expected error for missing skill id, got {:?}",
            found
        );
    }

    #[test]
    fn find_skill_dir_errors_when_requested_skill_id_missing() {
        // Repro for "install a skill whose name doesn't exist upstream" (issue #278).
        // Repo layout mirrors mattpocock/skills: skills/<category>/<skill>/SKILL.md.
        // Asking for a skill id that doesn't exist MUST error, not silently return
        // the skills/ container (which would install the entire repo as one skill).
        let tmp = tempdir().unwrap();
        let ask_matt = tmp.path().join("skills").join("engineering").join("ask-matt");
        let tdd = tmp.path().join("skills").join("engineering").join("tdd");
        fs::create_dir_all(&ask_matt).unwrap();
        fs::write(ask_matt.join("SKILL.md"), "---\nname: ask-matt\n---").unwrap();
        fs::create_dir_all(&tdd).unwrap();
        fs::write(tdd.join("SKILL.md"), "---\nname: tdd\n---").unwrap();

        // Existing skill resolves fine.
        let found = find_skill_dir(tmp.path(), Some("ask-matt")).unwrap();
        assert_eq!(found, ask_matt);

        // Missing skill id must error instead of returning the skills/ container.
        let missing = find_skill_dir(tmp.path(), Some("caveman"));
        assert!(
            missing.is_err(),
            "expected an error for missing skill id, got {:?}",
            missing
        );
    }

    #[test]
    fn find_skill_dir_fallback_to_root() {
        let tmp = tempdir().unwrap();
        let found = find_skill_dir(tmp.path(), None).unwrap();
        assert_eq!(found, tmp.path());
    }

    // ── relative_subpath ──

    #[test]
    fn relative_subpath_nested() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let skill = repo.join("tools").join("my-skill");
        assert_eq!(
            relative_subpath(&repo, &skill).map(|s| s.replace('\\', "/")),
            Some("tools/my-skill".to_string())
        );
    }

    #[test]
    fn relative_subpath_root_returns_none() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path().join("repo");
        assert_eq!(relative_subpath(&repo, &repo), None);
    }

    #[test]
    fn relative_subpath_unrelated_returns_none() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let other = tmp.path().join("other").join("skill");
        assert_eq!(relative_subpath(&repo, &other), None);
    }

    #[test]
    fn parse_github_tree_url_with_dot_git_suffix() {
        let parsed = parse_git_source("https://github.com/acme/skills.git/tree/main/sub");
        assert_eq!(parsed.clone_url, "https://github.com/acme/skills.git");
        assert_eq!(parsed.branch.as_deref(), Some("main"));
        assert_eq!(parsed.subpath.as_deref(), Some("sub"));
    }

    #[test]
    fn parse_non_github_url_no_tree_extraction() {
        let parsed = parse_git_source("https://gitlab.com/acme/skills/tree/main/sub");
        assert_eq!(
            parsed.clone_url,
            "https://gitlab.com/acme/skills/tree/main/sub"
        );
        assert_eq!(parsed.branch, None);
    }

    // ── split_tree_branch_path ──

    #[test]
    fn split_tree_branch_path_defaults_to_first_segment_when_no_branches_known() {
        // Optimistic (offline) parse: first segment is the branch.
        let (b, s) = split_tree_branch_path("main/sub/dir", &[]);
        assert_eq!(b, "main");
        assert_eq!(s.as_deref(), Some("sub/dir"));
    }

    #[test]
    fn split_tree_branch_path_picks_longest_known_branch_with_slash() {
        // When the branch name contains `/`, only the known-branches list lets
        // us pick the right split. Issue #121 follow-up: branch `feature/x`.
        let branches = vec!["main".to_string(), "feature/x".to_string()];
        let (b, s) = split_tree_branch_path("feature/x/skills/foo", &branches);
        assert_eq!(b, "feature/x");
        assert_eq!(s.as_deref(), Some("skills/foo"));
    }

    #[test]
    fn split_tree_branch_path_prefers_longer_when_multiple_match() {
        // Both `feature` and `feature/x` are valid prefixes; pick the longer.
        let branches = vec!["feature".to_string(), "feature/x".to_string()];
        let (b, s) = split_tree_branch_path("feature/x/sub", &branches);
        assert_eq!(b, "feature/x");
        assert_eq!(s.as_deref(), Some("sub"));
    }

    #[test]
    fn split_tree_branch_path_branch_only_no_subpath() {
        let branches = vec!["release/2.0".to_string()];
        let (b, s) = split_tree_branch_path("release/2.0", &branches);
        assert_eq!(b, "release/2.0");
        assert_eq!(s, None);
    }

    #[test]
    fn split_tree_branch_path_falls_back_when_no_known_branch_matches() {
        // Defensive: if `known_branches` is non-empty but nothing matches
        // (e.g. ls-remote returned stale data), fall back to optimistic parse.
        let branches = vec!["develop".to_string()];
        let (b, s) = split_tree_branch_path("main/sub", &branches);
        assert_eq!(b, "main");
        assert_eq!(s.as_deref(), Some("sub"));
    }

    #[test]
    fn split_tree_branch_path_does_not_match_partial_segment() {
        // `feat` must not match path `feature/x` even though it's a string prefix.
        let branches = vec!["feat".to_string()];
        let (b, s) = split_tree_branch_path("feature/x/sub", &branches);
        assert_eq!(b, "feature"); // falls through to optimistic parse
        assert_eq!(s.as_deref(), Some("x/sub"));
    }

    #[test]
    fn split_tree_branch_path_trailing_slash() {
        // `main/` (trailing slash, no subpath) — branch `main`, subpath None.
        let (b, s) = split_tree_branch_path("main/", &[]);
        assert_eq!(b, "main");
        assert_eq!(s, None);
    }

    // ── repo_cache_dir ──

    #[test]
    fn repo_cache_dir_is_deterministic() {
        let a = repo_cache_dir("https://github.com/acme/skills.git");
        let b = repo_cache_dir("https://github.com/acme/skills.git");
        assert_eq!(a, b);
    }

    #[test]
    fn repo_cache_dir_differs_for_different_urls() {
        let a = repo_cache_dir("https://github.com/acme/skills.git");
        let b = repo_cache_dir("https://github.com/acme/other.git");
        assert_ne!(a, b);
    }

    #[test]
    fn repo_cache_dir_canonicalizes_dot_git_suffix() {
        // `…/y` and `…/y.git` clone the same repo; they must share a cache slot.
        let a = repo_cache_dir("https://github.com/acme/skills");
        let b = repo_cache_dir("https://github.com/acme/skills.git");
        assert_eq!(a, b);
    }

    #[test]
    fn repo_cache_dir_canonicalizes_trailing_slash() {
        let a = repo_cache_dir("https://github.com/acme/skills/");
        let b = repo_cache_dir("https://github.com/acme/skills");
        assert_eq!(a, b);
    }

    #[test]
    fn repo_cache_dir_canonicalizes_dot_git_with_trailing_slash() {
        // `.git` after a trailing slash strip — must collapse to the same key.
        let a = repo_cache_dir("https://github.com/acme/skills.git/");
        let b = repo_cache_dir("https://github.com/acme/skills");
        assert_eq!(a, b);
    }

    #[test]
    fn canonicalize_clone_url_preserves_distinct_repos() {
        // Sanity: canonicalization must not collapse genuinely different URLs.
        assert_ne!(
            canonicalize_clone_url("https://github.com/acme/skills"),
            canonicalize_clone_url("https://github.com/acme/other")
        );
        assert_ne!(
            canonicalize_clone_url("https://github.com/acme/skills"),
            canonicalize_clone_url("git@github.com:acme/skills")
        );
    }

    // ── cleanup_temp ──

    #[test]
    fn cleanup_temp_removes_non_cache_dir() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join("some-temp");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("file.txt"), "data").unwrap();
        cleanup_temp(&dir);
        assert!(!dir.exists());
    }
}
