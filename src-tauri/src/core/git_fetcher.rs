use crate::core::central_repo;
use crate::core::git_credentials;
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
    repo_cache_dir_for(url, false)
}

/// Cache slot for a URL. A subpath-scoped checkout gets its own `-sparse` slot
/// rather than sharing the full one, because the two are not interchangeable:
/// the flows that need a whole tree (repo preview with no subpath, the skills.sh
/// locator search, `resolve_skill_dir`'s repo-wide fallback) must never be handed
/// a checkout holding one directory. A repo used both ways simply keeps two
/// caches, and the full path keeps behaving exactly as it did before.
fn repo_cache_dir_for(url: &str, sparse: bool) -> PathBuf {
    let canonical = canonicalize_clone_url(url);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let short = &hash[..16];
    let name = if sparse {
        format!("{short}-sparse")
    } else {
        short.to_string()
    };
    central_repo::cache_dir().join("repos").join(name)
}

/// Upper bound on the whole repo cache.
///
/// A backstop, not a quota. Nothing ever deleted a cache slot before, so a
/// library built from many repositories accumulated one checkout per repository
/// and kept it forever — measured at 569 MB across 48 repositories on an
/// ordinary machine, the oldest untouched for four months.
const REPO_CACHE_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;

struct RepoCacheLock {
    _file: File,
}

/// Delete least-recently-used cache slots until the cache is back under the limit.
///
/// Called only where a slot is about to be cloned fresh. Refreshing an existing
/// slot adds a delta; a new repository adds a whole checkout, so that is where the
/// growth comes from — and it keeps a batch update from paying for the walk once
/// per repository (the walk is ~0.8s over a 569 MB cache).
///
/// `keep` is the slot the caller is about to write. A slot another install holds
/// is skipped rather than waited for: deleting a checkout from under a running
/// install would break it. The lock *file* is deliberately left behind — unlinking
/// it while someone waits on it would let two installs each hold a lock on a
/// different inode for the same slot.
fn prune_repo_cache(keep: &Path) {
    prune_cache_root(
        &central_repo::cache_dir().join("repos"),
        keep,
        REPO_CACHE_LIMIT_BYTES,
    )
}

/// Takes its root and limit rather than reading them from the central config, so
/// it is a plain function of the directory in front of it — testable without
/// touching process-global state that parallel tests would fight over.
fn prune_cache_root(root: &Path, keep: &Path, limit: u64) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };

    let mut slots: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
    let mut total: u64 = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let (size, used) = slot_stats(&path);
        total = total.saturating_add(size);
        slots.push((path, size, used));
    }

    slots.sort_by_key(|(_, _, used)| *used);
    for (path, size, _) in slots {
        if total <= limit {
            break;
        }
        if path == keep {
            continue;
        }
        let lock_path = path.with_extension("lock");
        let Ok(file) = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
        else {
            continue;
        };
        if file.try_lock_exclusive().is_err() {
            continue;
        }
        if std::fs::remove_dir_all(&path).is_ok() {
            log::info!("pruned repo cache slot {}", path.display());
            total = total.saturating_sub(size);
        }
    }
}

/// Bytes in a cache slot, and when anything inside it was last written.
///
/// The recency half cannot come from the slot directory's own mtime: a fetch
/// writes objects deep inside `.git`, never in the slot root, so that mtime stays
/// at creation time and "least recently used" would silently mean "oldest",
/// evicting a repository that is updated weekly before one nobody has touched
/// since it was cloned. The size walk visits every file anyway, so the real
/// answer is free.
fn slot_stats(dir: &Path) -> (u64, std::time::SystemTime) {
    let mut size = 0u64;
    let mut newest = std::time::UNIX_EPOCH;
    for entry in walkdir::WalkDir::new(dir).into_iter().flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        size = size.saturating_add(meta.len());
        if let Ok(modified) = meta.modified() {
            newest = newest.max(modified);
        }
    }
    (size, newest)
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

    // `-c` is a *global* git option: `git fetch -c key=value` is rejected with
    // "unknown switch `c`", so it has to precede the subcommand. It used to sit
    // after `fetch`, which made every cache refresh fail outright whenever a
    // proxy was configured — those users re-cloned the whole repository on every
    // install and update instead of ever reusing the cache.
    let mut fetch_cmd = git_command();
    fetch_cmd.arg("-C").arg(cached);
    if let Some(proxy) = proxy_url.filter(|s| !s.is_empty()) {
        fetch_cmd.arg("-c").arg(format!("http.proxy={proxy}"));
        fetch_cmd.arg("-c").arg(format!("https.proxy={proxy}"));
    }
    fetch_cmd.arg("fetch").arg("--depth").arg("1");
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

    // Reset to the fetched HEAD. A branch has a remote-tracking ref, but a tag
    // does not — `origin/<tag>` is not a revision — so fall back to FETCH_HEAD,
    // which the fetch above just wrote. Without the fallback a tag source never
    // reuses its cache and silently re-clones on every check.
    let targets: Vec<String> = match branch {
        Some(b) => vec![format!("origin/{b}"), "FETCH_HEAD".to_string()],
        None => vec!["origin/HEAD".to_string()],
    };
    // Against a full cache this reset is local and returns immediately. Against a
    // sparse cache it is a partial clone, so writing the worktree lazily fetches
    // the blobs it needs — which is why it runs under the same timeout and cancel
    // flag as the fetch above instead of blocking forever on a dead network.
    let mut cancelled = false;
    let reset_ok = targets.iter().any(|target| {
        match run_git_watched_at(
            cached,
            &["reset", "--hard", target],
            proxy_url,
            cancel,
            &None,
        ) {
            Ok(()) => true,
            Err(e) => {
                cancelled |= is_cancellation(&e);
                false
            }
        }
    });
    // A cancellation is not a broken cache. Collapsed into `reset_ok == false` it
    // reads as "every target failed", and the cache below gets deleted — so
    // cancelling an install would silently cost the user the next full download.
    // The fetch above already treats cancellation this way; so must this.
    if cancelled {
        bail!("Installation cancelled");
    }
    match reset_ok {
        true => Ok(true),
        false => {
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
    clone_repo_ref_scoped(url, branch, None, cancel, proxy_url, on_progress)
}

/// Clone `url`, narrowed to `subpath` when one is known.
///
/// With a subpath this fetches only that directory (`--filter=blob:none` plus a
/// sparse-checkout), which is the difference between downloading a repository and
/// downloading one skill — measured at 15 MB versus 472 KB for a single skill out
/// of `anthropics/skills`. Every way that can fail — a server with
/// `uploadpack.allowFilter` off, a git too old to read the sparse arguments the
/// way we mean them, a subpath that upstream has since moved — falls back to the
/// full checkout, so the narrow path can only ever be faster, never the reason an
/// install stops working.
///
/// Pass `None` for `subpath` whenever the caller needs to search the repository
/// rather than read one known directory.
pub fn clone_repo_ref_scoped(
    url: &str,
    branch: Option<&str>,
    subpath: Option<&str>,
    cancel: Option<&Arc<AtomicBool>>,
    proxy_url: Option<&str>,
    on_progress: Option<ProgressCallback>,
) -> Result<PathBuf> {
    if let Some(subpath) = sparse_pattern(subpath) {
        match clone_repo_sparse(url, branch, &subpath, cancel, proxy_url, &on_progress) {
            Ok(dir) => return Ok(dir),
            Err(e) if is_cancellation(&e) => return Err(e),
            Err(e) => {
                log::info!(
                    "narrow clone of '{subpath}' from {url} unavailable, using a full checkout: {e}"
                );
            }
        }
    }

    clone_repo_full(url, branch, cancel, proxy_url, on_progress)
}

/// Whether an error is a user cancellation rather than a fault worth retrying
/// differently.
fn is_cancellation(err: &anyhow::Error) -> bool {
    let message = err.to_string();
    message.contains("cancelled") || message.contains("canceled")
}

/// Turn a stored subpath into a sparse-checkout pattern, or `None` when it is not
/// one we are willing to hand to git.
///
/// The pattern given to git must select the same directory the caller will later
/// `join` onto the checkout. Tidying the string here — trimming, swapping
/// separators — would break that: on unix a trailing space and a backslash are
/// both legal parts of a directory name, so `skills/foo ` would narrow to
/// `skills/foo`, pass the guard, and then leave the caller reading a directory the
/// checkout does not contain, with no failure to fall back on. So nothing is
/// rewritten; a path that is not already clean simply takes the full checkout.
///
/// Windows is the one exception, and only because `\` cannot be part of a name
/// there — `content_hash` normalizes separators under the same `cfg` for the same
/// reason.
fn sparse_pattern(subpath: Option<&str>) -> Option<String> {
    let raw = subpath?;
    #[cfg(windows)]
    let raw = &raw.replace('\\', "/");

    if raw.is_empty()
        || raw != raw.trim()
        || raw.starts_with('/')
        || raw.ends_with('/')
        || raw.contains('\\')
    {
        return None;
    }
    if raw
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return None;
    }
    Some(raw.to_string())
}

/// Clone only `subpath` out of `url`, reusing (and widening) the sparse cache slot.
fn clone_repo_sparse(
    url: &str,
    branch: Option<&str>,
    subpath: &str,
    cancel: Option<&Arc<AtomicBool>>,
    proxy_url: Option<&str>,
    on_progress: &Option<ProgressCallback>,
) -> Result<PathBuf> {
    let cached_dir = repo_cache_dir_for(url, true);
    let _cache_lock = lock_repo_cache(&cached_dir, on_progress)?;

    // One slot serves every skill from this repo: the objects are what is
    // expensive, and they are shared. The lock holds until this call's copy is
    // taken, so a later install re-scoping the worktree cannot disturb it.
    let reusable = cached_dir.exists()
        && try_update_cached_repo(&cached_dir, url, branch, proxy_url, cancel, on_progress)?;

    if !reusable {
        let _ = std::fs::remove_dir_all(&cached_dir);
        prune_repo_cache(&cached_dir);
        sparse_clone_into(&cached_dir, url, branch, cancel, proxy_url, on_progress).inspect_err(
            |_| {
                let _ = std::fs::remove_dir_all(&cached_dir);
            },
        )?;
    }

    set_sparse_scope(&cached_dir, subpath, cancel, proxy_url, on_progress)?;

    // `sparse-checkout set` succeeds on a path the repository does not have — it
    // just leaves that path absent, keeping the root files cone mode always
    // includes. So the result has to be inspected rather than trusted.
    if !sparse_checkout_holds_a_skill(&cached_dir, subpath) {
        bail!("'{subpath}' is not a skill directory in the narrow checkout of {url}");
    }

    copy_cached_repo(&cached_dir, cancel)
}

fn sparse_clone_into(
    dest: &Path,
    url: &str,
    branch: Option<&str>,
    cancel: Option<&Arc<AtomicBool>>,
    proxy_url: Option<&str>,
    on_progress: &Option<ProgressCallback>,
) -> Result<()> {
    if let Some(cb) = on_progress {
        cb("Fetching only the requested skill directory…");
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut command = git_command();
    command
        .arg("clone")
        .arg("--filter=blob:none")
        .arg("--no-checkout")
        .arg("--sparse")
        .arg("--depth")
        .arg("1");
    // `git clone -c` applies the value before the initial fetch, so unlike
    // `git fetch` this really does route the clone through the proxy.
    if let Some(proxy) = proxy_url.filter(|s| !s.is_empty()) {
        command.arg("-c").arg(format!("http.proxy={proxy}"));
        command.arg("-c").arg(format!("https.proxy={proxy}"));
    }
    if let Some(branch) = branch {
        command.arg("--branch").arg(branch);
    }
    command.arg("--progress").arg(url).arg(dest);
    run_git_watched(command, cancel, on_progress)
}

/// Point the cache's sparse checkout at `subpath` and materialize it.
///
/// `set` rather than `add`: fetched objects stay in the cache either way, so
/// carrying every previously requested skill in the worktree buys nothing.
fn set_sparse_scope(
    cached: &Path,
    subpath: &str,
    cancel: Option<&Arc<AtomicBool>>,
    proxy_url: Option<&str>,
    on_progress: &Option<ProgressCallback>,
) -> Result<()> {
    // `--cone` is explicit because cone mode is only the default on newer git,
    // and the non-cone reading of the same argument is a gitignore-style pattern
    // that would not bring the directory's contents with it.
    run_git_watched_at(
        cached,
        &["sparse-checkout", "set", "--cone", subpath],
        proxy_url,
        cancel,
        on_progress,
    )?;
    run_git_watched_at(cached, &["checkout"], proxy_url, cancel, on_progress)
}

/// Whether the narrow checkout produced a skill at exactly `subpath`.
///
/// Deliberately strict: a directory that merely *contains* skills is refused, so
/// a container subpath takes the full checkout. That is not conservatism for its
/// own sake — `resolve_skill_dir` accepts a stored path only when the path itself
/// is a skill, and otherwise searches the whole repository for the locator id. Let
/// a container through here and that search runs against a tree holding one
/// directory, which does not fail cleanly: it can resolve a *different* skill that
/// happens to be inside the narrow scope. Refusing containers keeps the repo-wide
/// search on a repo-wide checkout, which is the only tree it is correct on.
fn sparse_checkout_holds_a_skill(cached: &Path, subpath: &str) -> bool {
    skill_metadata::is_valid_skill_dir(&cached.join(subpath))
}

/// Materialize an install checkout from a sparse cache by copying it.
///
/// `git clone --local` cannot be used here. Cloning from a partial clone asks the
/// source to serve objects it does not have, and git aborts with "could not fetch
/// <oid> from promisor remote", taking the whole install with it. A plain copy
/// keeps the promisor and sparse configuration intact, and is cheap precisely
/// because a sparse cache holds one skill instead of a repository.
fn copy_cached_repo(cached: &Path, cancel: Option<&Arc<AtomicBool>>) -> Result<PathBuf> {
    let temp_dir =
        std::env::temp_dir().join(format!("{CLONE_TEMP_PREFIX}{}", uuid::Uuid::new_v4()));
    if let Err(e) = copy_dir_contents(cached, &temp_dir, cancel) {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(e);
    }
    detach_from_promisor(&temp_dir);
    Ok(temp_dir)
}

/// Cut an install checkout loose from the promisor remote it was copied from.
///
/// This is the boundary that keeps the narrow clone from becoming a permanent tax
/// on everyone who touches this code. A partial clone carries IOUs: a git command
/// that reaches an object we never fetched silently becomes a network round trip,
/// which can hang, needs a proxy, and needs credentials. Callers run git against
/// the checkout we hand back — `checkout_revision`, `get_head_revision`, whatever
/// gets added next — and nothing in the type system would tell them that.
///
/// Dropping the config makes a missing object an immediate error instead, exactly
/// as it has always been in a shallow full checkout.
///
/// The precise claim, because a broader one would be wrong: no git command against
/// the returned checkout fetches an object *behind the caller's back*. An explicit
/// `fetch`/`pull`/`ls-remote` still reaches origin, whose URL the copy keeps — no
/// caller does that today, and one that started would be doing something visibly
/// network-shaped. What this rules out is the invisible case, which is the one
/// nobody would think to instrument.
fn detach_from_promisor(repo_dir: &Path) {
    // Three keys, because git registers a promisor remote from *any* of them.
    // `git clone --filter` writes the two `remote.origin.*` ones and, since 2.25,
    // not `extensions.partialClone` — but leaving that one out would mean the set
    // is complete only for the git that happens to write our caches, and getting
    // it wrong restores lazy fetching silently.
    for key in [
        "remote.origin.promisor",
        "remote.origin.partialclonefilter",
        "extensions.partialClone",
    ] {
        let _ = git_command()
            .arg("-C")
            .arg(repo_dir)
            .args(["config", "--unset", key])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Recursive copy that skips symlinks, so it can never follow a link out of the
/// cache. Nothing downstream misses them: the installer drops symlinks, and
/// `content_hash` counts only regular files.
fn copy_dir_contents(src: &Path, dst: &Path, cancel: Option<&Arc<AtomicBool>>) -> Result<()> {
    if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
        bail!("Installation cancelled");
    }
    std::fs::create_dir_all(dst).with_context(|| format!("Failed to create {}", dst.display()))?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        let target = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_contents(&entry.path(), &target, cancel)?;
        } else {
            std::fs::copy(entry.path(), &target).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    entry.path().display(),
                    target.display()
                )
            })?;
        }
    }
    Ok(())
}

/// `git -C <dir> <args…>` under the shared cancel/timeout policy.
///
/// These are not local-only commands. In a partial clone both `sparse-checkout`
/// and `checkout` fetch the blobs they are about to write, so they block on the
/// network exactly like a clone does and have to be just as cancellable — the
/// reason this exists rather than a plain `Command::output()`.
///
/// The proxy is passed per invocation rather than read from the cache's config:
/// `git clone -c` persisted whatever proxy was in effect when the cache was
/// created, and a command line `-c` is what lets a since-changed proxy win over
/// that stale value.
fn run_git_watched_at(
    dir: &Path,
    args: &[&str],
    proxy_url: Option<&str>,
    cancel: Option<&Arc<AtomicBool>>,
    on_progress: &Option<ProgressCallback>,
) -> Result<()> {
    let mut command = git_command();
    command.arg("-C").arg(dir);
    if let Some(proxy) = proxy_url.filter(|s| !s.is_empty()) {
        command.arg("-c").arg(format!("http.proxy={proxy}"));
        command.arg("-c").arg(format!("https.proxy={proxy}"));
    }
    command.args(args);
    run_git_watched(command, cancel, on_progress)
}

/// Run a git subprocess under the shared cancel/timeout policy, forwarding its
/// stderr to `on_progress`.
///
/// The full-clone loop below is deliberately left inline rather than reshaped to
/// call this: it has no end-to-end test coverage, so the safe move is to add a
/// path beside it, not to rewrite it.
fn run_git_watched(
    mut command: Command,
    cancel: Option<&Arc<AtomicBool>>,
    on_progress: &Option<ProgressCallback>,
) -> Result<()> {
    let mut child = command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to start git")?;

    let (stderr_rx, stderr_thread) =
        spawn_stderr_collector(child.stderr.take(), on_progress.is_some());
    let deadline = Instant::now() + Duration::from_secs(CLONE_TIMEOUT_SECS);

    loop {
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Installation cancelled");
        }

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
                    return Ok(());
                }
                bail!("git exited with {}: {}", status, collected.trim());
            }
            Ok(None) => {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!("git timed out after {}s", CLONE_TIMEOUT_SECS);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                bail!("Failed to wait for git: {e}")
            }
        }
    }
}

fn clone_repo_full(
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
    prune_repo_cache(&cached_dir);

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

    git_credentials::install_git2_credentials(&mut callbacks, url);
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
            // The pinned ref may be a tag, which the clone above cannot check
            // out. Retry in the shape that works before giving up.
            if let Some(pinned) = branch {
                if clone_tag_with_git2(url, pinned, &cached_dir, cancel, proxy_url).is_ok() {
                    return materialize_cached_repo(&cached_dir, cancel);
                }
            }
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

/// Fetch a repository pinned to a **tag** with libgit2.
///
/// `RepoBuilder::branch` resolves its argument under `refs/remotes/origin/`, so
/// a tag fails there with "reference 'refs/remotes/origin/<tag>' not found".
/// Init an empty repo and shallow-fetch the tag ref directly instead, then
/// detach onto the commit it peels to — cloning the default branch first would
/// download a second snapshot nobody wants.
///
/// Verified against a real https remote; `file://` cannot do shallow at all, so
/// a local-transport test would prove nothing here.
///
/// Only reachable when system git is missing or failed: system git's
/// `clone --branch` already accepts tags.
fn clone_tag_with_git2(
    url: &str,
    tag: &str,
    cached_dir: &Path,
    cancel: Option<&Arc<AtomicBool>>,
    proxy_url: Option<&str>,
) -> Result<()> {
    let _ = std::fs::remove_dir_all(cached_dir);
    let deadline = Instant::now() + Duration::from_secs(CLONE_TIMEOUT_SECS);
    let fetch_opts = || {
        let cancel = cancel.cloned();
        let mut callbacks = git2::RemoteCallbacks::new();
        callbacks.transfer_progress(move |_| {
            if cancel.as_ref().is_some_and(|c| c.load(Ordering::SeqCst)) {
                return false;
            }
            Instant::now() <= deadline
        });
        git_credentials::install_git2_credentials(&mut callbacks, url);
        let mut opts = git2::FetchOptions::new();
        opts.remote_callbacks(callbacks);
        opts.depth(1);
        if let Some(proxy) = proxy_url.filter(|s| !s.is_empty()) {
            let mut proxy_opts = git2::ProxyOptions::new();
            proxy_opts.url(proxy);
            opts.proxy_options(proxy_opts);
        }
        opts
    };

    let repo = git2::Repository::init(cached_dir)?;
    {
        // A named remote, not an anonymous one: the cache-reuse path identifies
        // a cached repo by `git remote get-url origin`.
        let mut remote = repo.remote("origin", url)?;
        let refspec = format!("+refs/tags/{tag}:refs/tags/{tag}");
        remote.fetch(&[refspec.as_str()], Some(&mut fetch_opts()), None)?;
    }
    let commit = repo.revparse_single(&format!("refs/tags/{tag}^{{commit}}"))?;
    repo.set_head_detached(commit.id())?;
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
    Ok(())
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
    let mut callbacks = git2::RemoteCallbacks::new();
    git_credentials::install_git2_credentials(&mut callbacks, url);
    remote.connect_auth(Direction::Fetch, Some(callbacks), Some(proxy_opts))?;
    let refs = remote.list()?;

    for wanted in candidate_ref_names(branch) {
        if let Some(head) = refs.iter().find(|head| head.name() == wanted) {
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

pub fn fetch_and_checkout_revision(
    repo_dir: &Path,
    revision: &str,
    proxy_url: Option<&str>,
) -> Result<()> {
    if checkout_revision(repo_dir, revision).is_ok() {
        return Ok(());
    }

    let mut command = git_command();
    command.arg("-C").arg(repo_dir);
    if let Some(proxy) = proxy_url.filter(|value| !value.is_empty()) {
        command.arg("-c").arg(format!("http.proxy={proxy}"));
        command.arg("-c").arg(format!("https.proxy={proxy}"));
    }
    let output = command
        .args(["fetch", "--depth", "1", "origin", revision])
        .output()
        .context("Failed to fetch pinned package revision")?;
    if !output.status.success() {
        let detail =
            crate::core::log_sanitize::sanitize(&String::from_utf8_lossy(&output.stderr));
        bail!("Pinned package revision is unavailable: {}", detail.trim());
    }
    checkout_revision(repo_dir, revision)
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

/// Split a `tree/<path>` tail against the remote's actual refs.
///
/// Branches are matched first: a tag may only claim a URL that no branch
/// explains. Matching the longest ref across heads and tags at once would let
/// a tag named `main/v1` swallow a URL that means branch `main` plus subpath
/// `v1/...`.
fn split_tree_path_with_known_refs(
    path: &str,
    known: &RemoteRefNames,
) -> (String, Option<String>) {
    match_known_ref(path, &known.heads)
        .or_else(|| match_known_ref(path, &known.tags))
        .unwrap_or_else(|| split_tree_branch_path(path, &[]))
}

/// Longest ref in `known` that matches a `/`-bounded prefix of `path`, split
/// into `(ref, optional subpath)`. `None` when nothing matches.
///
/// Callers must try branches before tags: taking the longest match across both
/// at once lets a tag named `main/v1` steal a URL that means branch `main`
/// plus subpath `v1/...`.
fn match_known_ref(path: &str, known: &[String]) -> Option<(String, Option<String>)> {
    let mut best: Option<&str> = None;
    for name in known {
        if name.is_empty() {
            continue;
        }
        let matches = path == name.as_str()
            || path
                .strip_prefix(name.as_str())
                .is_some_and(|rest| rest.starts_with('/'));
        if matches && best.is_none_or(|b: &str| name.len() > b.len()) {
            best = Some(name);
        }
    }
    best.map(|name| {
        let subpath = path
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix('/'))
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        (name.to_string(), subpath)
    })
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
    if let Some(hit) = match_known_ref(path, known_branches) {
        return hit;
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

    let known = match list_remote_ref_names(&clone_url, proxy_url) {
        Ok(refs) if !refs.is_empty() => refs,
        Ok(_) => {
            log::warn!(
                "ls-remote returned no refs for {}; using optimistic tree-URL parse",
                clone_url
            );
            return parsed;
        }
        Err(e) => {
            log::warn!(
                "ls-remote failed for {}: {} — using optimistic tree-URL parse (slash-ref URLs may parse incorrectly)",
                clone_url,
                e
            );
            return parsed;
        }
    };
    let (branch, subpath) = split_tree_path_with_known_refs(&path, &known);
    parsed.branch = Some(branch);
    parsed.subpath = subpath;
    parsed
}

/// Branch and tag names on the remote, used to split a `tree/<path>` URL.
///
/// Tags count: `tree/release/v1.0/skills/foo` is as valid a tag URL as it is a
/// branch URL, and listing only heads would silently split it as branch
/// `release` plus subpath `v1.0/skills/foo`. They stay in separate lists
/// because branches must be matched first.
#[derive(Debug, Default, PartialEq)]
struct RemoteRefNames {
    heads: Vec<String>,
    tags: Vec<String>,
}

impl RemoteRefNames {
    fn is_empty(&self) -> bool {
        self.heads.is_empty() && self.tags.is_empty()
    }
}

fn list_remote_ref_names(url: &str, proxy_url: Option<&str>) -> Result<RemoteRefNames> {
    let mut cmd = git_command();
    if let Some(proxy) = proxy_url.filter(|s| !s.is_empty()) {
        cmd.arg("-c").arg(format!("http.proxy={proxy}"));
        cmd.arg("-c").arg(format!("https.proxy={proxy}"));
    }
    let output = cmd
        .args(["ls-remote", "--heads", "--tags", url])
        .output()
        .with_context(|| format!("Failed to list remote refs for {}", url))?;

    if !output.status.success() {
        anyhow::bail!("git ls-remote exited with {}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_remote_ref_names(&stdout))
}

fn parse_remote_ref_names(stdout: &str) -> RemoteRefNames {
    let mut refs = RemoteRefNames::default();
    for line in stdout.lines() {
        let mut parts = line.split_whitespace();
        let Some(_sha) = parts.next() else { continue };
        let Some(refname) = parts.next() else { continue };
        // Peeled entries name the same tag; keep one entry per ref.
        if refname.ends_with("^{}") {
            continue;
        }
        if let Some(name) = refname.strip_prefix("refs/heads/") {
            refs.heads.push(name.to_string());
        } else if let Some(name) = refname.strip_prefix("refs/tags/") {
            refs.tags.push(name.to_string());
        }
    }
    refs
}

/// Ref names that a stored source ref may resolve to, most preferred first.
///
/// A source ref is stored as a bare name (`main`, `v0.8.0`) with no record of
/// whether it is a branch or a tag — GitHub `tree/` URLs do not distinguish
/// them either. Branches win over tags so an ambiguous name keeps its historic
/// meaning, and the peeled tag (`^{}`) wins over the tag object because only
/// the peeled line carries the commit an annotated tag points at.
fn candidate_ref_names(source_ref: Option<&str>) -> Vec<String> {
    match source_ref {
        Some(name) => vec![
            format!("refs/heads/{name}"),
            format!("refs/tags/{name}^{{}}"),
            format!("refs/tags/{name}"),
        ],
        None => vec!["HEAD".to_string()],
    }
}

/// Pick a revision out of `git ls-remote` output by exact ref name.
///
/// Never take "the first line": querying a tag returns both the tag object and
/// its peeled commit, and their order is the remote's business, not ours.
fn select_remote_revision(stdout: &str, candidates: &[String]) -> Option<String> {
    let refs: Vec<(&str, &str)> = stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let sha = parts.next()?;
            let name = parts.next()?;
            (!sha.is_empty()).then_some((name, sha))
        })
        .collect();

    candidates.iter().find_map(|wanted| {
        refs.iter()
            .find(|(name, _)| *name == wanted.as_str())
            .map(|(_, sha)| sha.to_string())
    })
}

fn resolve_remote_revision_with_git(
    url: &str,
    branch: Option<&str>,
    proxy_url: Option<&str>,
) -> Result<String> {
    let candidates = candidate_ref_names(branch);
    let mut cmd = git_command();
    if let Some(proxy) = proxy_url.filter(|s| !s.is_empty()) {
        cmd.arg("-c").arg(format!("http.proxy={proxy}"));
        cmd.arg("-c").arg(format!("https.proxy={proxy}"));
    }
    cmd.args(["ls-remote", url]);
    cmd.args(&candidates);
    let output = cmd
        .output()
        .with_context(|| format!("Failed to query remote {}", url))?;

    if !output.status.success() {
        anyhow::bail!("git ls-remote exited with {}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    select_remote_revision(&stdout, &candidates)
        .ok_or_else(|| anyhow::anyhow!("No remote revision found"))
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
    fn find_skill_dir_by_id_matches_root_skill_md_name() {
        // A single-skill repo whose root SKILL.md carries `name: root-name`.
        // Requesting that id must still resolve to the root via the frontmatter
        // name match — the #278 bail! only fires when *nothing* matches, and must
        // not regress this legitimate root case.
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("SKILL.md"), "---\nname: root-name\n---").unwrap();

        // Matching id resolves to the root via the frontmatter name match.
        let found = find_skill_dir(tmp.path(), Some("root-name")).unwrap();
        assert_eq!(found, tmp.path());

        // Non-matching id on the SAME root-skill repo must error, not fall back
        // to the root. This proves the positive case above returns the root via
        // name_match rather than the skill_id==None root fallback — so the test
        // would fail if the old fall-through were reintroduced.
        let missing = find_skill_dir(tmp.path(), Some("nope-not-here"));
        assert!(
            missing.is_err(),
            "expected error for non-matching id on a root-skill repo, got {:?}",
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

    // ── libgit2 tag clone (network; run with `-- --ignored`) ──

    const TAG_REPO: &str = "https://github.com/xingkongliang/skills-manager.git";
    const TAG_NAME: &str = "v1.36.0";
    const TAG_COMMIT: &str = "824c51e1c09e64a0ace8cff893d7ec8b3e079959";

    #[test]
    #[ignore = "hits the network"]
    fn libgit2_clones_a_tag_and_lands_on_its_commit() {
        let tmp = tempdir().unwrap();
        let dest = tmp.path().join("repo");
        clone_tag_with_git2(TAG_REPO, TAG_NAME, &dest, None, None)
            .expect("libgit2 must be able to clone a tag");
        let repo = git2::Repository::open(&dest).unwrap();
        assert_eq!(
            repo.head().unwrap().peel_to_commit().unwrap().id().to_string(),
            TAG_COMMIT
        );
        assert!(dest.join("package.json").exists(), "working tree checked out");
    }

    #[test]
    #[ignore = "hits the network and mutates PATH; run alone"]
    fn a_tag_installs_with_no_system_git_on_path() {
        // The whole point of the git2 path: a machine without git. Emptying PATH
        // makes every `git_command()` fail, so this exercises the real fallback
        // wiring, not just the helper.
        let original = std::env::var_os("PATH");
        std::env::set_var("PATH", "");
        let result = clone_repo_ref(TAG_REPO, Some(TAG_NAME), None, None);
        match original {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }

        let dir = result.expect("a tag source must install without system git");
        assert!(dir.join("package.json").exists());
        cleanup_temp(&dir);
    }

    // ── remote ref resolution ──

    #[test]
    fn candidate_refs_prefer_branch_then_peeled_tag_then_tag() {
        assert_eq!(
            candidate_ref_names(Some("v0.8.0")),
            vec![
                "refs/heads/v0.8.0",
                "refs/tags/v0.8.0^{}",
                "refs/tags/v0.8.0",
            ]
        );
        assert_eq!(candidate_ref_names(None), vec!["HEAD"]);
    }

    #[test]
    fn resolves_annotated_tag_to_its_peeled_commit() {
        // Both lines come back for an annotated tag. The first one is the tag
        // object; only the peeled line is the commit the tag points at.
        let stdout = "857196de\trefs/tags/v0.8.0\n346411fa\trefs/tags/v0.8.0^{}\n";
        assert_eq!(
            select_remote_revision(stdout, &candidate_ref_names(Some("v0.8.0"))),
            Some("346411fa".to_string())
        );
    }

    #[test]
    fn resolves_lightweight_tag_without_peeled_line() {
        let stdout = "c2ad91cc\trefs/tags/preview-1\n";
        assert_eq!(
            select_remote_revision(stdout, &candidate_ref_names(Some("preview-1"))),
            Some("c2ad91cc".to_string())
        );
    }

    #[test]
    fn branch_wins_over_a_tag_of_the_same_name() {
        let stdout = "aaaa\trefs/tags/release\nbbbb\trefs/heads/release\n";
        assert_eq!(
            select_remote_revision(stdout, &candidate_ref_names(Some("release"))),
            Some("bbbb".to_string())
        );
    }

    #[test]
    fn unrelated_refs_do_not_resolve() {
        // Guards the old "just take the first line" behaviour: a ref the remote
        // does not have must fail rather than borrow another ref's revision.
        let stdout = "aaaa\trefs/heads/main\n";
        assert_eq!(
            select_remote_revision(stdout, &candidate_ref_names(Some("v9.9.9"))),
            None
        );
    }

    #[test]
    fn head_resolves_when_no_ref_is_pinned() {
        let stdout = "94f6d9c0\tHEAD\naaaa\trefs/heads/master\n";
        assert_eq!(
            select_remote_revision(stdout, &candidate_ref_names(None)),
            Some("94f6d9c0".to_string())
        );
    }

    #[test]
    fn remote_ref_names_split_heads_from_tags_and_drop_peeled_duplicates() {
        let stdout = "aaaa\trefs/heads/main\n\
                      bbbb\trefs/tags/v1.0\n\
                      cccc\trefs/tags/v1.0^{}\n\
                      dddd\trefs/pull/7/head\n";
        let refs = parse_remote_ref_names(stdout);
        assert_eq!(refs.heads, vec!["main"]);
        assert_eq!(refs.tags, vec!["v1.0"]);
    }

    #[test]
    fn a_tag_cannot_steal_a_url_that_a_branch_explains() {
        // Longest-match across heads and tags at once would read this as tag
        // `main/v1` + subpath `skills`, silently re-pointing the skill.
        let refs = parse_remote_ref_names(
            "aaaa\trefs/heads/main\nbbbb\trefs/tags/main/v1\n",
        );
        let (branch, subpath) = split_tree_path_with_known_refs("main/v1/skills", &refs);
        assert_eq!(branch, "main");
        assert_eq!(subpath.as_deref(), Some("v1/skills"));
    }

    #[test]
    fn a_slash_tag_still_resolves_when_no_branch_matches() {
        let refs = parse_remote_ref_names(
            "aaaa\trefs/heads/master\nbbbb\trefs/tags/release/v0.8.0\n",
        );
        let (branch, subpath) =
            split_tree_path_with_known_refs("release/v0.8.0/skills/herdr", &refs);
        assert_eq!(branch, "release/v0.8.0");
        assert_eq!(subpath.as_deref(), Some("skills/herdr"));
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
    fn split_tree_branch_path_handles_a_tag_with_a_slash() {
        let refs = vec!["master".to_string(), "release/v0.8.0".to_string()];
        let (b, s) = split_tree_branch_path("release/v0.8.0/skills/herdr", &refs);
        assert_eq!(b, "release/v0.8.0");
        assert_eq!(s.as_deref(), Some("skills/herdr"));
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

    #[test]
    fn fetch_and_checkout_revision_recovers_commit_missing_from_shallow_clone() {
        let tmp = tempdir().unwrap();
        let origin = tmp.path().join("origin");
        let checkout = tmp.path().join("checkout");
        assert!(git_command()
            .args(["init", "--initial-branch", "main"])
            .arg(&origin)
            .status()
            .unwrap()
            .success());
        for (key, value) in [("user.name", "Test"), ("user.email", "test@example.com")] {
            assert!(git_command()
                .arg("-C")
                .arg(&origin)
                .args(["config", key, value])
                .status()
                .unwrap()
                .success());
        }
        fs::write(origin.join("value.txt"), "old").unwrap();
        assert!(git_command()
            .arg("-C")
            .arg(&origin)
            .args(["add", "."])
            .status()
            .unwrap()
            .success());
        assert!(git_command()
            .arg("-C")
            .arg(&origin)
            .args(["commit", "-m", "old"])
            .status()
            .unwrap()
            .success());
        let old_revision = get_head_revision(&origin).unwrap();
        fs::write(origin.join("value.txt"), "new").unwrap();
        assert!(git_command()
            .arg("-C")
            .arg(&origin)
            .args(["commit", "-am", "new"])
            .status()
            .unwrap()
            .success());
        assert!(git_command()
            .args(["clone", "--depth", "1", "--no-local"])
            .arg(&origin)
            .arg(&checkout)
            .status()
            .unwrap()
            .success());

        fetch_and_checkout_revision(&checkout, &old_revision, None).unwrap();
        assert_eq!(get_head_revision(&checkout).unwrap(), old_revision);
        assert_eq!(fs::read_to_string(checkout.join("value.txt")).unwrap(), "old");
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
    // ── narrow (partial + sparse) clone ──

    #[test]
    fn sparse_slot_is_separate_from_the_full_slot() {
        let url = "https://github.com/acme/skills";
        assert_ne!(
            repo_cache_dir_for(url, true),
            repo_cache_dir_for(url, false)
        );
        assert_eq!(repo_cache_dir_for(url, false), repo_cache_dir(url));
        // Both slots still canonicalize the URL the same way.
        assert_eq!(
            repo_cache_dir_for(url, true),
            repo_cache_dir_for("https://github.com/acme/skills.git/", true)
        );
    }

    #[test]
    fn sparse_pattern_accepts_plain_subpaths_and_rejects_the_rest() {
        assert_eq!(
            sparse_pattern(Some("skills/foo")).as_deref(),
            Some("skills/foo")
        );
        assert_eq!(sparse_pattern(Some("a")).as_deref(), Some("a"));

        // Nothing to narrow to, or something we refuse to hand to git. Every one
        // of these means "use the full checkout", never an error.
        assert_eq!(sparse_pattern(None), None);
        assert_eq!(sparse_pattern(Some("")), None);
        assert_eq!(sparse_pattern(Some("   ")), None);
        assert_eq!(sparse_pattern(Some("/")), None);
        assert_eq!(sparse_pattern(Some("../escape")), None);
        assert_eq!(sparse_pattern(Some("skills/../../etc")), None);
        assert_eq!(sparse_pattern(Some("skills/./foo")), None);
        assert_eq!(sparse_pattern(Some("skills//foo")), None);
        assert_eq!(sparse_pattern(Some("/skills/foo")), None);
        assert_eq!(sparse_pattern(Some("skills/foo/")), None);
    }

    /// The pattern handed to git has to name the same directory the caller will
    /// join onto the checkout. Tidying these up instead of refusing them would
    /// narrow to a neighbouring directory, pass the guard, and leave the caller
    /// reading a path the checkout does not have — with no failure to fall back
    /// on, because from git's point of view everything succeeded.
    #[cfg(unix)]
    #[test]
    fn sparse_pattern_refuses_names_it_would_have_to_rewrite() {
        // A trailing space and a backslash are both legal in a unix directory name.
        assert_eq!(sparse_pattern(Some("skills/foo ")), None);
        assert_eq!(sparse_pattern(Some(" skills/foo")), None);
        assert_eq!(sparse_pattern(Some("skills\\foo")), None);
    }

    #[test]
    fn sparse_checkout_holds_a_skill_distinguishes_a_skill_from_leftovers() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        // Missing entirely — what `sparse-checkout set` leaves behind when the
        // path does not exist upstream, silently and with exit code 0.
        assert!(!sparse_checkout_holds_a_skill(root, "skills/gone"));

        // Present but empty.
        fs::create_dir_all(root.join("skills/empty")).unwrap();
        assert!(!sparse_checkout_holds_a_skill(root, "skills/empty"));

        // Non-empty but not a skill and holding none: upstream reorganized and
        // left something else at the recorded path. This must fail so the caller
        // falls back to a full checkout, where the locator search can find the
        // skill at its new home.
        fs::create_dir_all(root.join("skills/moved")).unwrap();
        fs::write(root.join("skills/moved/README.md"), "moved elsewhere").unwrap();
        assert!(!sparse_checkout_holds_a_skill(root, "skills/moved"));

        // A skill.
        fs::create_dir_all(root.join("skills/real")).unwrap();
        fs::write(root.join("skills/real/SKILL.md"), "---\nname: real\n---").unwrap();
        assert!(sparse_checkout_holds_a_skill(root, "skills/real"));

        // A directory that only *contains* skills is refused, even though the
        // narrow checkout of it looks perfectly healthy. `resolve_skill_dir` takes
        // a stored path only when the path itself is a skill and otherwise
        // searches the whole repository for its locator id; against a tree holding
        // one directory that search does not fail cleanly, it can land on whatever
        // skill happens to be inside the narrow scope. Refusing containers here is
        // what keeps that search on a full checkout.
        fs::create_dir_all(root.join("group/inner")).unwrap();
        fs::write(root.join("group/inner/SKILL.md"), "---\nname: inner\n---").unwrap();
        assert!(!sparse_checkout_holds_a_skill(root, "group"));
    }

    /// The cancelled-reset guard in `try_update_cached_repo` recognizes a
    /// cancellation by its message, so the two have to keep agreeing: if they
    /// drift, cancelling an install goes back to deleting a healthy cache.
    #[test]
    fn a_cancelled_git_run_reports_itself_as_a_cancellation() {
        let tmp = tempdir().unwrap();
        let cancel = Arc::new(AtomicBool::new(true));
        let err = run_git_watched_at(tmp.path(), &["status"], None, Some(&cancel), &None)
            .expect_err("a set cancel flag must abort the run");
        assert!(is_cancellation(&err), "got {err}");
    }

    #[test]
    fn copy_dir_contents_copies_nested_files() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(src.join("a/b")).unwrap();
        fs::write(src.join("top.txt"), "top").unwrap();
        fs::write(src.join("a/b/deep.txt"), "deep").unwrap();

        let dst = tmp.path().join("dst");
        copy_dir_contents(&src, &dst, None).unwrap();

        assert_eq!(fs::read_to_string(dst.join("top.txt")).unwrap(), "top");
        assert_eq!(
            fs::read_to_string(dst.join("a/b/deep.txt")).unwrap(),
            "deep"
        );
    }

    #[cfg(unix)]
    #[test]
    fn copy_dir_contents_skips_symlinks_instead_of_following_them_out() {
        let tmp = tempdir().unwrap();
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "not ours").unwrap();

        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("kept.txt"), "kept").unwrap();
        std::os::unix::fs::symlink(&outside, src.join("escape")).unwrap();

        let dst = tmp.path().join("dst");
        copy_dir_contents(&src, &dst, None).unwrap();

        assert!(dst.join("kept.txt").exists());
        assert!(
            !dst.join("escape").exists(),
            "a symlink out of the cache must not be followed into the checkout"
        );
    }

    #[test]
    fn copy_dir_contents_honours_cancellation() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a.txt"), "a").unwrap();

        let cancel = Arc::new(AtomicBool::new(true));
        let err = copy_dir_contents(&src, &tmp.path().join("dst"), Some(&cancel)).unwrap_err();
        assert!(is_cancellation(&err), "got {err}");
    }

    // ── narrow clone, end to end (network; run with `-- --ignored`) ──

    const SPARSE_REPO: &str = "https://github.com/anthropics/skills";
    const SPARSE_SUBPATH: &str = "skills/mcp-builder";
    const SPARSE_SIBLING: &str = "skills/canvas-design";

    #[test]
    #[ignore = "hits the network"]
    fn narrow_clone_fetches_one_skill_and_leaves_its_siblings_behind() {
        let tmp = tempdir().unwrap();
        central_repo::set_test_base_dir_override(Some(tmp.path().to_path_buf()));

        let checkout =
            clone_repo_ref_scoped(SPARSE_REPO, None, Some(SPARSE_SUBPATH), None, None, None)
                .expect("narrow clone must succeed against a filter-capable remote");

        assert!(
            checkout.join(SPARSE_SUBPATH).join("SKILL.md").is_file(),
            "the requested skill must be checked out"
        );
        assert!(
            !checkout.join(SPARSE_SIBLING).exists(),
            "a sibling skill must not be downloaded — that is the whole point"
        );
        assert!(
            repo_cache_dir_for(SPARSE_REPO, true).exists(),
            "the narrow checkout must come from the sparse cache slot"
        );
        assert!(
            !repo_cache_dir_for(SPARSE_REPO, false).exists(),
            "the full cache slot must be left untouched"
        );
        // End to end, over a real partial clone: what the caller receives is not
        // one. Callers run plain git against this directory, so it has to be
        // incapable of wandering onto the network behind their back.
        for key in ["remote.origin.promisor", "remote.origin.partialclonefilter"] {
            let out = Command::new("git")
                .arg("-C")
                .arg(&checkout)
                .args(["config", "--get", key])
                .output()
                .unwrap();
            assert!(
                !out.status.success(),
                "the install checkout must be detached from the promisor remote ({key})"
            );
        }

        // A second skill from the same repo reuses the cache — its objects are
        // what cost something — and re-scopes the worktree to itself.
        let second =
            clone_repo_ref_scoped(SPARSE_REPO, None, Some(SPARSE_SIBLING), None, None, None)
                .expect("a second skill must reuse the cache");
        assert!(second.join(SPARSE_SIBLING).join("SKILL.md").is_file());
        assert!(
            !second.join(SPARSE_SUBPATH).exists(),
            "each checkout carries the skill it asked for, not every earlier one"
        );

        cleanup_temp(&checkout);
        cleanup_temp(&second);
        central_repo::set_test_base_dir_override(None);
    }

    #[test]
    #[ignore = "hits the network"]
    fn a_container_subpath_falls_back_to_the_full_checkout() {
        let tmp = tempdir().unwrap();
        central_repo::set_test_base_dir_override(Some(tmp.path().to_path_buf()));

        // `skills` holds skills but is not one. Narrowing to it would leave the
        // locator search looking at a fraction of the repository.
        let checkout = clone_repo_ref_scoped(SPARSE_REPO, None, Some("skills"), None, None, None)
            .expect("a container subpath must degrade to a full checkout");

        assert!(checkout.join(SPARSE_SUBPATH).join("SKILL.md").is_file());
        assert!(
            checkout.join(SPARSE_SIBLING).join("SKILL.md").is_file(),
            "the fallback must be a full checkout, siblings included"
        );

        cleanup_temp(&checkout);
        central_repo::set_test_base_dir_override(None);
    }

    #[test]
    #[ignore = "hits the network"]
    fn a_subpath_that_no_longer_exists_falls_back_to_the_full_checkout() {
        let tmp = tempdir().unwrap();
        central_repo::set_test_base_dir_override(Some(tmp.path().to_path_buf()));

        // `sparse-checkout set` succeeds on a path that is not in the tree — it
        // simply leaves it absent — so without the post-checkout inspection this
        // would hand back a checkout missing the very directory it was asked for,
        // instead of a repository the locator search can walk.
        let checkout = clone_repo_ref_scoped(
            SPARSE_REPO,
            None,
            Some("skills/this-skill-does-not-exist"),
            None,
            None,
            None,
        )
        .expect("a stale subpath must degrade to a full checkout, not an error");

        assert!(
            checkout.join(SPARSE_SUBPATH).join("SKILL.md").is_file(),
            "the fallback must be a full checkout the caller can search"
        );

        cleanup_temp(&checkout);
        central_repo::set_test_base_dir_override(None);
    }
    // ── the promisor boundary ──

    /// The rule this locks in: nothing handed outside this module is a partial
    /// clone. If someone removes the detach, callers' plain `git` calls silently
    /// become network calls again — which is the maintenance cost the narrow
    /// clone would otherwise impose on every future change.
    #[test]
    fn an_install_checkout_is_never_left_attached_to_a_promisor_remote() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        assert!(Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("init")
            .status()
            .is_ok_and(|s| s.success()));
        for (key, value) in [
            ("remote.origin.url", "https://example.invalid/x.git"),
            ("remote.origin.promisor", "true"),
            ("remote.origin.partialclonefilter", "blob:none"),
        ] {
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["config", key, value])
                .status()
                .unwrap();
        }

        detach_from_promisor(&repo);

        for key in ["remote.origin.promisor", "remote.origin.partialclonefilter"] {
            let out = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["config", "--get", key])
                .output()
                .unwrap();
            assert!(
                !out.status.success(),
                "{key} must be gone, still reads {}",
                String::from_utf8_lossy(&out.stdout).trim()
            );
        }
        // The remote itself stays: it is what `source_ref_resolved` reports and
        // what a later fetch of this checkout would use.
        let url = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["config", "--get", "remote.origin.url"])
            .output()
            .unwrap();
        assert!(url.status.success());
    }

    /// Pins the *outcome* rather than the config keys: a checkout that has been
    /// detached must fail on a missing object without reaching for the remote.
    ///
    /// Naming the keys is not enough on its own. Git registers a promisor remote
    /// from any of three settings, so a key set that is right for today's git and
    /// wrong for another would leave lazy fetching switched back on with every
    /// key-checking test still green. This one asks the question the boundary is
    /// actually about, and needs no network to do it: the partial clone is served
    /// over `file://` from a repo in the same temp directory.
    #[test]
    fn a_detached_checkout_fails_on_a_missing_object_without_reaching_the_remote() {
        fn git(dir: &Path, args: &[&str]) -> std::process::Output {
            Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .expect("git must be runnable")
        }

        let tmp = tempdir().unwrap();
        let source = tmp.path().join("source");
        fs::create_dir_all(source.join("kept")).unwrap();
        fs::create_dir_all(source.join("absent")).unwrap();
        fs::write(source.join("kept/f.txt"), "kept").unwrap();
        fs::write(source.join("absent/f.txt"), "absent").unwrap();
        git(&source, &["init"]);
        git(&source, &["add", "-A"]);
        git(
            &source,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-m",
                "init",
            ],
        );
        // The filter is served by the *source* repo, so the switch belongs there.
        git(&source, &["config", "uploadpack.allowFilter", "true"]);

        // `file://` needs forward slashes and, on Windows, a slash before the
        // drive letter: `file:///C:/…` where unix wants `file:///tmp/…`.
        fn file_url(path: &Path) -> String {
            let raw = path.display().to_string().replace('\\', "/");
            if raw.starts_with('/') {
                format!("file://{raw}")
            } else {
                format!("file:///{raw}")
            }
        }

        let checkout = tmp.path().join("checkout");
        let source_url = file_url(&source);
        let cloned = Command::new("git")
            .args(["clone", "--filter=blob:none", "--no-local", "--no-checkout"])
            .arg("--sparse")
            .arg(&source_url)
            .arg(&checkout)
            .output()
            .unwrap();
        assert!(
            cloned.status.success(),
            "clone failed: {}",
            String::from_utf8_lossy(&cloned.stderr)
        );
        git(&checkout, &["sparse-checkout", "set", "--cone", "kept"]);
        git(&checkout, &["checkout"]);
        // Stand in for a git that records the promisor under `extensions` instead
        // of, or as well as, the two `remote.origin.*` keys. Today's git writes
        // only the latter pair, so without this line the third key in
        // `detach_from_promisor` would be defensive code no test ever exercises —
        // and dropping it from the list would go unnoticed.
        git(&checkout, &["config", "extensions.partialClone", "origin"]);

        let oid = git(&checkout, &["rev-parse", "HEAD:absent/f.txt"]);
        let oid = String::from_utf8_lossy(&oid.stdout).trim().to_string();

        // Point origin somewhere that does not exist, so any attempt to reach it
        // is unmistakable in the error rather than quietly succeeding.
        let gone = file_url(&tmp.path().join("gone"));
        git(&checkout, &["config", "remote.origin.url", &gone]);

        // Precondition, asserted rather than assumed: this really is a partial
        // clone with that object missing. Without it the test could pass on a git
        // that ignored the filter and fetched everything.
        let before = git(&checkout, &["cat-file", "-s", &oid]);
        let before_err = String::from_utf8_lossy(&before.stderr).to_string();
        assert!(
            !before.status.success() && before_err.contains("promisor"),
            "setup must produce a genuine partial clone, got: {before_err}"
        );

        detach_from_promisor(&checkout);

        let after = git(&checkout, &["cat-file", "-s", &oid]);
        let after_err = String::from_utf8_lossy(&after.stderr).to_string();
        assert!(!after.status.success(), "the object is still missing");
        assert!(
            !after_err.contains("promisor") && !after_err.contains(&gone),
            "a detached checkout must not go to the remote for a missing object, got: {after_err}"
        );

        // And the objects it does have are still readable.
        assert_eq!(
            fs::read_to_string(checkout.join("kept/f.txt")).unwrap(),
            "kept"
        );
    }

    // ── cache prune ──

    /// Writes the marker deep inside the slot, the way git writes fetched objects,
    /// so the recency signal has to come from a walk rather than the slot's own
    /// directory mtime.
    fn seed_cache_slot(root: &Path, name: &str, bytes: usize, used: std::time::SystemTime) {
        let deep = root.join(name).join(".git").join("objects");
        fs::create_dir_all(&deep).unwrap();
        let blob = deep.join("pack");
        fs::write(&blob, vec![b'x'; bytes]).unwrap();
        File::options()
            .write(true)
            .open(&blob)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(used))
            .unwrap();
    }

    #[test]
    fn prune_is_a_no_op_while_the_cache_fits() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        seed_cache_slot(root, "a", 1024, std::time::UNIX_EPOCH);

        prune_cache_root(root, &root.join("nothing"), 8192);

        assert!(
            root.join("a").exists(),
            "a cache under the limit is left alone"
        );
    }

    #[test]
    fn prune_ranks_by_the_newest_file_inside_a_slot_not_the_slot_itself() {
        // Both slots were created now; only their contents differ in age. Ranking
        // on the directory's own mtime cannot tell them apart, which is exactly
        // the mistake this guards against.
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        let now = std::time::SystemTime::now();
        seed_cache_slot(root, "stale", 4096, now - Duration::from_secs(86_400 * 30));
        seed_cache_slot(root, "fresh", 4096, now);

        // Make the two signals disagree on purpose, or this test proves nothing:
        // adding an entry to `stale` bumps that directory's own mtime to now, so
        // by directory mtime `stale` looks like the *most* recently used slot,
        // while the only file in it is a month old. Ranking on the directory
        // therefore evicts `fresh` and keeps `stale` — the exact inversion this
        // guards against. An empty directory adds no bytes and no file mtime, so
        // it cannot move the correct signal.
        fs::create_dir_all(root.join("stale").join("refs")).unwrap();

        prune_cache_root(root, &root.join("nothing"), 6000);

        assert!(!root.join("stale").exists(), "the stale slot goes first");
        assert!(root.join("fresh").exists(), "the recently used slot stays");
    }

    #[test]
    fn prune_evicts_the_least_recently_used_and_spares_the_slot_being_written() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        let now = std::time::SystemTime::now();
        let day = Duration::from_secs(86_400);
        // `being-written` is deliberately the *least* recently used slot. If it
        // were the newest, plain LRU would spare it anyway and the guard could be
        // deleted with every assertion still green — which is exactly how the
        // first version of this test proved nothing.
        seed_cache_slot(root, "being-written", 4096, now - day * 30);
        seed_cache_slot(root, "middle", 4096, now - day);
        seed_cache_slot(root, "newest", 4096, now);

        // Fits two of the three.
        prune_cache_root(root, &root.join("being-written"), 10_000);

        assert!(
            root.join("being-written").exists(),
            "the slot the caller is about to clone into must survive, even as the \
             least recently used one"
        );
        assert!(
            !root.join("middle").exists(),
            "eviction moves past the protected slot to the next oldest"
        );
        assert!(root.join("newest").exists(), "eviction stops once it fits");
    }

    #[test]
    fn prune_skips_a_slot_another_install_is_holding() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        let now = std::time::SystemTime::now();
        seed_cache_slot(root, "held", 4096, now - Duration::from_secs(86_400 * 30));
        seed_cache_slot(root, "free", 4096, now - Duration::from_secs(86_400));

        // Stand in for another install that already holds the oldest slot.
        let held = lock_repo_cache(&root.join("held"), &None).unwrap();

        prune_cache_root(root, &root.join("nothing"), 6000);

        assert!(
            root.join("held").exists(),
            "deleting a checkout out from under a running install would break it"
        );
        assert!(
            !root.join("free").exists(),
            "eviction moves on to the next candidate instead of giving up"
        );
        drop(held);
    }
}
