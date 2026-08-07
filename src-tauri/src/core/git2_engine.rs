//! git2-based network engine for the backup remote (backup redesign §3.3,
//! Phase 2 pilot).
//!
//! Scope is deliberately narrow: only the four network operations (fetch,
//! push, ls-remote, clone) against http(s) remotes go through libgit2, with
//! credentials injected in-memory from the OS keychain. All local operations
//! (commit, tag, status, merge, read-tree) stay on system git, and SSH /
//! custom remotes always use system git. Opt-in via the `git_backup_engine`
//! setting ("git2"); default is the system git engine.
//!
//! Error normalization matters here: the frontend maps error text produced
//! by system git ("Authentication failed", "Could not resolve host",
//! "non-fast-forward", …) to plain-language copy. libgit2 phrases the same
//! failures differently, so every error leaving this module is prefixed with
//! the equivalent system-git marker.

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use super::git_credentials;

static PILOT_ENABLED: AtomicBool = AtomicBool::new(false);
static PROXY_URL: OnceLock<Mutex<Option<String>>> = OnceLock::new();

/// Sync the engine preference from settings. Called at the entry of backup
/// commands (core code has no store access).
pub fn set_preference(git2_enabled: bool, proxy_url: Option<String>) {
    PILOT_ENABLED.store(git2_enabled, Ordering::Relaxed);
    *PROXY_URL
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = proxy_url.filter(|s| !s.is_empty());
}

fn proxy_url() -> Option<String> {
    PROXY_URL
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Whether the git2 engine should handle operations against `url`.
pub fn applies_to(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    PILOT_ENABLED.load(Ordering::Relaxed)
        && (lower.starts_with("https://") || lower.starts_with("http://"))
}

fn callbacks_for(url: &str) -> git2::RemoteCallbacks<'static> {
    let cred = git_credentials::https_host(url)
        .and_then(|host| git_credentials::load_credential(&host).ok().flatten());
    let mut callbacks = git2::RemoteCallbacks::new();
    // libgit2 re-invokes the credentials callback after a rejection; without
    // a cap that loops forever on a bad token.
    let mut attempts = 0;
    callbacks.credentials(move |_url, username_from_url, _allowed| {
        attempts += 1;
        if attempts > 2 {
            return Err(git2::Error::from_str("authentication attempts exhausted"));
        }
        match &cred {
            Some(c) => git2::Cred::userpass_plaintext(&c.username, &c.password),
            // No stored credential: try the URL's own username (if any) with
            // an empty password rather than hanging on a prompt.
            None => git2::Cred::userpass_plaintext(username_from_url.unwrap_or_default(), ""),
        }
    });
    callbacks
}

fn proxy_options() -> git2::ProxyOptions<'static> {
    let mut opts = git2::ProxyOptions::new();
    match proxy_url() {
        Some(url) => {
            opts.url(&url);
        }
        None => {
            opts.auto();
        }
    }
    opts
}

fn fetch_options(url: &str) -> git2::FetchOptions<'static> {
    let mut opts = git2::FetchOptions::new();
    opts.remote_callbacks(callbacks_for(url));
    opts.proxy_options(proxy_options());
    opts
}

/// Translate a libgit2 error into the marker vocabulary the frontend's git
/// error mapping already understands.
fn normalize_err(e: git2::Error, operation: &str) -> anyhow::Error {
    let msg = e.message().to_string();
    let lower = msg.to_ascii_lowercase();
    let marker = if e.code() == git2::ErrorCode::Auth
        || lower.contains("authentication")
        || lower.contains("401")
        || lower.contains("403")
    {
        "Authentication failed"
    } else if e.code() == git2::ErrorCode::NotFastForward {
        "non-fast-forward"
    } else if e.class() == git2::ErrorClass::Net
        || lower.contains("resolve")
        || lower.contains("connect")
        || lower.contains("timed out")
    {
        "Failed to connect"
    } else if e.class() == git2::ErrorClass::Ssl {
        "TLS/SSL error"
    } else {
        ""
    };
    if marker.is_empty() {
        anyhow::anyhow!("git2 {operation} failed: {msg}")
    } else {
        anyhow::anyhow!("git2 {operation} failed: {marker}: {msg}")
    }
}

/// Fetch `branch` (or the remote's configured refspecs when `None`) from
/// origin, updating the usual remote-tracking refs.
pub fn fetch(repo_dir: &Path, branch: Option<&str>, url: &str) -> Result<()> {
    let repo = git2::Repository::open(repo_dir).context("Failed to open repository")?;
    let mut remote = repo.find_remote("origin").context("No origin remote")?;
    let refspecs: Vec<String> = match branch {
        Some(b) => vec![format!("+refs/heads/{b}:refs/remotes/origin/{b}")],
        None => Vec::new(),
    };
    let refs: Vec<&str> = refspecs.iter().map(String::as_str).collect();
    remote
        .fetch(&refs, Some(&mut fetch_options(url)), None)
        .map_err(|e| normalize_err(e, "fetch"))?;
    log::info!(
        "git2 fetch: done ({})",
        branch.unwrap_or("configured refspecs")
    );
    Ok(())
}

/// Push the given refspecs to origin. Per-reference rejections (the
/// non-fast-forward case) are surfaced as errors even though the transport
/// call itself succeeds.
pub fn push_refs(repo_dir: &Path, refspecs: &[String], url: &str) -> Result<()> {
    let repo = git2::Repository::open(repo_dir).context("Failed to open repository")?;
    let mut remote = repo.find_remote("origin").context("No origin remote")?;

    let rejected: std::sync::Arc<Mutex<Vec<String>>> = Default::default();
    let rejected_in_cb = rejected.clone();
    let mut callbacks = callbacks_for(url);
    callbacks.push_update_reference(move |refname, status| {
        if let Some(reason) = status {
            rejected_in_cb
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(format!("{refname}: {reason}"));
        }
        Ok(())
    });

    let mut opts = git2::PushOptions::new();
    opts.remote_callbacks(callbacks);
    opts.proxy_options(proxy_options());

    remote
        .push(refspecs, Some(&mut opts))
        .map_err(|e| normalize_err(e, "push"))?;

    let rejected = rejected.lock().unwrap_or_else(|e| e.into_inner());
    if !rejected.is_empty() {
        // Same vocabulary as system git so the UI routes to recovery.
        anyhow::bail!(
            "git2 push failed: non-fast-forward, failed to push some refs ({})",
            rejected.join("; ")
        );
    }
    log::info!("git2 push: pushed {} refspec(s)", refspecs.len());
    Ok(())
}

/// List remote ref names (heads and tags) for `url` without a local repo.
pub fn ls_remote_refs(url: &str) -> Result<Vec<String>> {
    let mut remote =
        git2::Remote::create_detached(url).context("Failed to create detached remote")?;
    let connection = remote
        .connect_auth(
            git2::Direction::Fetch,
            Some(callbacks_for(url)),
            Some(proxy_options()),
        )
        .map_err(|e| normalize_err(e, "ls-remote"))?;
    let names = connection
        .list()
        .map_err(|e| normalize_err(e, "ls-remote"))?
        .iter()
        .map(|head| head.name().to_string())
        .collect();
    Ok(names)
}

/// Full clone (backup needs complete history — no shallow).
pub fn clone(url: &str, dest: &Path) -> Result<()> {
    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fetch_options(url));
    builder
        .clone(url, dest)
        .map_err(|e| normalize_err(e, "clone"))?;
    log::info!("git2 clone: done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Platform-correct file URL: Windows paths need forward slashes and a
    /// third slash before the drive letter (`file:///C:/...`).
    fn file_url(path: &Path) -> String {
        let s = path.display().to_string().replace('\\', "/");
        if s.starts_with('/') {
            format!("file://{s}")
        } else {
            format!("file:///{s}")
        }
    }

    fn git(dir: &Path, args: &[&str]) {
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
    }

    #[test]
    fn applies_to_requires_flag_and_https() {
        // Default off: nothing routes to git2.
        assert!(!applies_to("https://github.com/a/b.git"));
        PILOT_ENABLED.store(true, Ordering::Relaxed);
        assert!(applies_to("https://github.com/a/b.git"));
        assert!(applies_to("HTTP://example.com/a/b.git"));
        assert!(!applies_to("git@github.com:a/b.git"));
        assert!(!applies_to("ssh://git@github.com/a/b.git"));
        assert!(!applies_to("/local/path"));
        PILOT_ENABLED.store(false, Ordering::Relaxed);
    }

    #[test]
    fn push_fetch_ls_remote_roundtrip_against_local_remote() {
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
        let url = file_url(&remote);

        git(&work, &["init", "-b", "main"]);
        git(&work, &["remote", "add", "origin", &url]);
        std::fs::write(work.join("a.txt"), "v1").unwrap();
        git(&work, &["add", "-A"]);
        git(&work, &["commit", "-m", "v1"]);
        git(&work, &["tag", "sm-v-20260101-000000-abc"]);

        // Push branch + tag through git2.
        push_refs(
            &work,
            &[
                "refs/heads/main:refs/heads/main".to_string(),
                "refs/tags/sm-v-20260101-000000-abc:refs/tags/sm-v-20260101-000000-abc".to_string(),
            ],
            &url,
        )
        .unwrap();

        // Remote now lists both refs.
        let refs = ls_remote_refs(&url).unwrap();
        assert!(refs.iter().any(|r| r == "refs/heads/main"), "{refs:?}");
        assert!(
            refs.iter()
                .any(|r| r == "refs/tags/sm-v-20260101-000000-abc"),
            "{refs:?}"
        );

        // git2 push updated the local remote-tracking ref (parity with
        // system git — ahead/behind and upstream health depend on it).
        let out = Command::new("git")
            .arg("-C")
            .arg(&work)
            .args(["rev-parse", "refs/remotes/origin/main"])
            .output()
            .unwrap();
        assert!(out.status.success(), "tracking ref missing after git2 push");

        // Fetch through git2 from a second clone after a new remote commit.
        let other = tmp.path().join("other");
        clone(&url, &other).unwrap();
        std::fs::write(other.join("b.txt"), "from other").unwrap();
        git(&other, &["add", "-A"]);
        git(&other, &["commit", "-m", "v2"]);
        push_refs(
            &other,
            &["refs/heads/main:refs/heads/main".to_string()],
            &url,
        )
        .unwrap();

        fetch(&work, Some("main"), &url).unwrap();
        let out = Command::new("git")
            .arg("-C")
            .arg(&work)
            .args(["rev-list", "--count", "main..origin/main"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "1",
            "fetch should see the new remote commit"
        );
    }

    #[test]
    fn push_rejection_reports_non_fast_forward_vocabulary() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        std::fs::create_dir_all(&a).unwrap();
        assert!(Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(&remote)
            .output()
            .unwrap()
            .status
            .success());
        let url = file_url(&remote);

        git(&a, &["init", "-b", "main"]);
        git(&a, &["remote", "add", "origin", &url]);
        std::fs::write(a.join("f.txt"), "base").unwrap();
        git(&a, &["add", "-A"]);
        git(&a, &["commit", "-m", "base"]);
        git(&a, &["push", "origin", "main"]);

        clone(&url, &b).unwrap();

        // Diverge: A pushes a new commit, B commits without pulling.
        std::fs::write(a.join("f.txt"), "from a").unwrap();
        git(&a, &["commit", "-am", "a2"]);
        git(&a, &["push", "origin", "main"]);
        std::fs::write(b.join("f.txt"), "from b").unwrap();
        git(&b, &["commit", "-am", "b2"]);

        let err =
            push_refs(&b, &["refs/heads/main:refs/heads/main".to_string()], &url).unwrap_err();
        let msg = format!("{err:#}").to_ascii_lowercase();
        assert!(
            msg.contains("non-fast-forward") || msg.contains("fast-forward"),
            "frontend error mapping relies on this vocabulary, got: {msg}"
        );
    }
}
