//! Read-only, session-scoped access to a single skill directory.
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{error::AppError, git_fetcher};

pub const MAX_TEXT_BYTES: usize = 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
const PAGE_SIZE: usize = 200;
const MAX_DIRECTORY_ENTRIES: usize = 10_000;
const TTL: Duration = Duration::from_secs(30 * 60);
const HIDDEN: &[&str] = &[".git", "node_modules", ".venv", "venv", "__pycache__", ".DS_Store"];
const ENTRY_NAMES: &[&str] = &["SKILL.md", "skill.md", "CLAUDE.md", "claude.md", "README.md", "readme.md"];

fn reader_io(error: std::io::Error) -> AppError {
    if error.kind() == std::io::ErrorKind::NotFound { AppError::not_found("reader.missingFile") }
    else { AppError::io("reader.readFailed") }
}

/// A fresh reader checkout must restore tracked links skipped by the install
/// cache copier. Only call this on a temporary clone owned by the reader.
pub fn checkout_snapshot(repo_dir: &Path, revision: &str, cancel: Option<&Arc<AtomicBool>>, proxy: Option<&str>) -> Result<(), AppError> {
    if cancel.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
        return Err(AppError::cancelled("reader.cancelled"));
    }
    match git_fetcher::run_git_watched_at(repo_dir, &["checkout", "--force", "--detach", revision], proxy, cancel, &None) {
        Ok(()) => Ok(()),
        Err(error) if error.chain().any(|cause| cause.downcast_ref::<std::io::Error>().is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)) => {
            // Gitless installations use a full git2 clone. Only a missing
            // executable permits this local fallback; cancellation/timeouts do not.
            let repo = git2::Repository::open(repo_dir).map_err(AppError::git)?;
            let oid = git2::Oid::from_str(revision).map_err(AppError::git)?;
            repo.set_head_detached(oid).map_err(AppError::git)?;
            let mut checkout = git2::build::CheckoutBuilder::new();
            checkout.force().notify_on(git2::CheckoutNotificationType::all())
                .notify(|_, _, _, _, _| !cancel.is_some_and(|flag| flag.load(Ordering::SeqCst)));
            let result = repo.checkout_head(Some(&mut checkout));
            if cancel.is_some_and(|flag| flag.load(Ordering::SeqCst)) { return Err(AppError::cancelled("reader.cancelled")); }
            result.map_err(AppError::git)
        }
        Err(error) => Err(AppError::classify_git_error(error)),
    }
}

#[derive(Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub source_label: String,
    pub revision: Option<String>,
    pub default_path: Option<String>,
    pub read_only: bool,
}

#[derive(Serialize)]
pub struct FileEntry {
    pub relative_path: String,
    pub name: String,
    pub kind: String,
    pub size_bytes: Option<u64>,
    pub preview_kind: Option<String>,
    pub accessible: bool,
}

#[derive(Serialize)]
pub struct DirectoryPage {
    pub entries: Vec<FileEntry>,
    pub next_cursor: Option<usize>,
    pub truncated: bool,
}

#[derive(Serialize)]
pub struct FilePreview {
    pub relative_path: String,
    pub content_hash: Option<String>,
    pub size_bytes: u64,
    pub mime: String,
    pub kind: String,
    pub content: Option<String>,
    pub base64: Option<String>,
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct ResolvedLink {
    pub relative_path: String,
    pub fragment: Option<String>,
    pub directory: bool,
}

pub struct BrowserSession {
    root: PathBuf,
    pub source_label: String,
    pub revision: Option<String>,
    snapshot_root: Option<PathBuf>,
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        if let Some(root) = &self.snapshot_root { git_fetcher::cleanup_temp(root); }
    }
}

pub fn validate_relative(path: &str) -> Result<(), AppError> {
    if path.contains(['\\', '\0', ':']) || path.starts_with('/')
        || Path::new(path).components().any(|c| !matches!(c, Component::Normal(_)))
        || path.split('/').any(|c| HIDDEN.contains(&c)) {
        return Err(AppError::invalid_input("reader.invalidPath"));
    }
    Ok(())
}

fn kind_and_mime(path: &Path) -> (&'static str, &'static str) {
    match path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "md" | "markdown" | "mdown" => ("markdown", "text/markdown"),
        "png" => ("image", "image/png"),
        "jpg" | "jpeg" => ("image", "image/jpeg"),
        "gif" => ("image", "image/gif"),
        "webp" => ("image", "image/webp"),
        "bmp" => ("image", "image/bmp"),
        "pdf" | "zip" | "gz" | "tar" | "7z" | "woff" | "woff2" | "ttf" | "mp4" | "mp3" | "wav" | "exe" | "dylib" | "so" => ("unsupported", "application/octet-stream"),
        _ => ("text", "text/plain"),
    }
}

impl BrowserSession {
    pub fn local(root: PathBuf, source_label: String) -> Result<Self, AppError> {
        let root = root.canonicalize().map_err(AppError::io)?;
        if !root.is_dir() { return Err(AppError::not_found("reader.missingDirectory")); }
        Ok(Self { root, source_label, revision: None, snapshot_root: None })
    }

    pub fn snapshot(root: PathBuf, label: String, revision: String, owner: PathBuf) -> Result<Self, AppError> {
        let mut session = Self::local(root, label)?;
        session.revision = Some(revision);
        session.snapshot_root = Some(owner);
        Ok(session)
    }

    pub fn resolve(&self, relative: &str) -> Result<PathBuf, AppError> {
        validate_relative(relative)?;
        let root = self.root.canonicalize().map_err(reader_io)?;
        if root != self.root { return Err(AppError::invalid_input("reader.invalidPath")); }
        let mut candidate = root.clone();
        for component in Path::new(relative).components() {
            candidate.push(component);
            let meta = fs::symlink_metadata(&candidate).map_err(reader_io)?;
            // Never walk directory symlinks: they can create cycles or aliases.
            if meta.file_type().is_symlink() && candidate.is_dir() {
                return Err(AppError::invalid_input("reader.invalidPath"));
            }
        }
        let resolved = candidate.canonicalize().map_err(reader_io)?;
        if !resolved.starts_with(&root) { return Err(AppError::invalid_input("reader.invalidPath")); }
        Ok(resolved)
    }

    pub fn default_path(&self) -> Result<Option<String>, AppError> {
        let mut level = vec![String::new()];
        let mut visited = 0;
        for _ in 0..4 {
            let mut next = Vec::new();
            for dir in level {
                for name in ENTRY_NAMES {
                    let candidate = if dir.is_empty() { name.to_string() } else { format!("{dir}/{name}") };
                    if self.resolve(&candidate).map(|p| p.is_file()).unwrap_or(false) { return Ok(Some(candidate)); }
                }
                let mut cursor = 0;
                loop {
                    let page = self.list(&dir, cursor)?;
                    for entry in page.entries {
                        visited += 1;
                        if visited > MAX_DIRECTORY_ENTRIES { return Ok(None); }
                        if entry.kind == "directory" && entry.accessible { next.push(entry.relative_path); }
                    }
                    match page.next_cursor { Some(value) => cursor = value, None => break }
                }
            }
            level = next;
        }
        Ok(None)
    }

    pub fn list(&self, relative: &str, cursor: usize) -> Result<DirectoryPage, AppError> {
        let path = self.resolve(relative)?;
        if !path.is_dir() { return Err(AppError::invalid_input("reader.missingDirectory")); }
        let mut entries = Vec::new();
        let mut truncated = false;
        for entry in fs::read_dir(path).map_err(AppError::io)? {
            let entry = entry.map_err(AppError::io)?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue; };
            if HIDDEN.contains(&name.as_str()) { continue; }
            if entries.len() == MAX_DIRECTORY_ENTRIES { truncated = true; break; }
            let rel = if relative.is_empty() { name.clone() } else { format!("{relative}/{name}") };
            let meta = fs::symlink_metadata(entry.path()).map_err(AppError::io)?;
            let kind = if meta.file_type().is_symlink() { "symlink" } else if meta.is_dir() { "directory" } else { "file" };
            let accessible = self.resolve(&rel).map(|p| p.is_dir() || p.is_file()).unwrap_or(false);
            entries.push(FileEntry { relative_path: rel, name, kind: kind.into(), size_bytes: meta.is_file().then_some(meta.len()), preview_kind: if kind == "directory" { None } else { Some(kind_and_mime(&entry.path()).0.into()) }, accessible });
        }
        entries.sort_by(|a, b| {
            let rank = |e: &FileEntry| if e.kind == "directory" { 0 } else if ENTRY_NAMES.contains(&e.name.as_str()) { 1 } else { 2 };
            rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name))
        });
        let next_cursor = (cursor.saturating_add(PAGE_SIZE) < entries.len()).then_some(cursor.saturating_add(PAGE_SIZE));
        Ok(DirectoryPage { entries: entries.into_iter().skip(cursor).take(PAGE_SIZE).collect(), next_cursor, truncated })
    }

    pub fn read(&self, relative: &str) -> Result<FilePreview, AppError> {
        let path = self.resolve(relative)?;
        if !fs::metadata(&path).map_err(AppError::io)?.is_file() { return Err(AppError::invalid_input("reader.notRegularFile")); }
        let file = self.open_regular_file(&path)?;
        let meta = file.metadata().map_err(AppError::io)?;
        if !meta.is_file() || self.resolve(relative)? != path { return Err(AppError::invalid_input("reader.invalidPath")); }
        let (kind, mime) = kind_and_mime(Path::new(relative));
        let mut preview = FilePreview { relative_path: relative.into(), content_hash: None, size_bytes: meta.len(), mime: mime.into(), kind: "unsupported".into(), content: None, base64: None, reason: None };
        if kind == "unsupported" { preview.reason = Some("format".into()); return Ok(preview); }
        let limit = if kind == "image" { MAX_IMAGE_BYTES } else { MAX_TEXT_BYTES };
        let mut bytes = Vec::new();
        file.take((limit + 1) as u64).read_to_end(&mut bytes).map_err(AppError::io)?;
        if bytes.len() > limit { preview.reason = Some("too_large".into()); return Ok(preview); }
        preview.content_hash = Some(hex::encode(Sha256::digest(&bytes)));
        if kind == "image" {
            preview.kind = kind.into();
            preview.base64 = Some(base64::engine::general_purpose::STANDARD.encode(bytes));
        } else if bytes.contains(&0) { preview.reason = Some("binary".into()); }
        else {
            match String::from_utf8(bytes) {
                Ok(text) => { preview.kind = kind.into(); preview.content = Some(text.trim_start_matches('\u{feff}').to_owned()); }
                Err(_) => preview.reason = Some("encoding".into()),
            }
        }
        Ok(preview)
    }

    #[cfg(unix)]
    fn open_regular_file(&self, resolved: &Path) -> Result<fs::File, AppError> {
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let relative = resolved.strip_prefix(&self.root).map_err(|_| AppError::invalid_input("reader.invalidPath"))?;
        let mut directory = fs::OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK).open(&self.root).map_err(AppError::io)?;
        let root_meta = fs::metadata(&self.root).map_err(AppError::io)?;
        let opened_meta = directory.metadata().map_err(AppError::io)?;
        if root_meta.dev() != opened_meta.dev() || root_meta.ino() != opened_meta.ino() || self.root.canonicalize().map_err(AppError::io)? != self.root {
            return Err(AppError::invalid_input("reader.invalidPath"));
        }
        let components: Vec<_> = relative.components().collect();
        if components.is_empty() { return Err(AppError::invalid_input("reader.notRegularFile")); }
        for (index, component) in components.iter().enumerate() {
            if !matches!(component, Component::Normal(_)) { return Err(AppError::invalid_input("reader.invalidPath")); }
            let name = CString::new(component.as_os_str().as_bytes()).map_err(|_| AppError::invalid_input("reader.invalidPath"))?;
            let last = index + 1 == components.len();
            let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK | if last { 0 } else { libc::O_DIRECTORY };
            // SAFETY: directory owns a live fd; name is NUL terminated. A successful
            // openat returns a new owned descriptor, transferred exactly once to File.
            let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
            if fd < 0 { return Err(AppError::io(std::io::Error::last_os_error())); }
            let file = unsafe { fs::File::from_raw_fd(fd) };
            if last {
                if !file.metadata().map_err(AppError::io)?.is_file() { return Err(AppError::invalid_input("reader.notRegularFile")); }
                return Ok(file);
            }
            directory = file;
        }
        Err(AppError::invalid_input("reader.notRegularFile"))
    }

    #[cfg(not(unix))]
    fn open_regular_file(&self, resolved: &Path) -> Result<fs::File, AppError> {
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)] {
            use std::os::windows::fs::OpenOptionsExt;
            const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
            options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let file = options.open(resolved).map_err(AppError::io)?;
        let meta = file.metadata().map_err(AppError::io)?;
        #[cfg(windows)] {
            use std::os::windows::fs::MetadataExt;
            const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
            if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 { return Err(AppError::invalid_input("reader.invalidPath")); }
        }
        if !meta.is_file() { return Err(AppError::invalid_input("reader.notRegularFile")); }
        Ok(file)
    }

    pub fn resolve_link(&self, from: &str, href: &str) -> Result<ResolvedLink, AppError> {
        if href.contains(':') || href.starts_with('/') || href.starts_with("//") { return Err(AppError::invalid_input("reader.invalidPath")); }
        let (raw_path, fragment) = href.split_once('#').map(|(a,b)| (a, Some(b))).unwrap_or((href, None));
        let decoded = urlencoding::decode(raw_path).map_err(AppError::io)?;
        // Disallow ambiguous double encoding and URL queries in filesystem links.
        if decoded.contains('%') || decoded.contains('?') || decoded.contains(['\\', ':', '\0']) { return Err(AppError::invalid_input("reader.invalidPath")); }
        validate_relative(from)?;
        let mut parts: Vec<&str> = from.split('/').collect();
        if !decoded.is_empty() {
            parts.pop();
            for part in decoded.split('/') {
                match part {
                    "" | "." => {},
                    ".." => { if parts.pop().is_none() { return Err(AppError::invalid_input("reader.invalidPath")); } },
                    other => parts.push(other),
                }
            }
        }
        let relative_path = parts.join("/");
        let resolved = self.resolve(&relative_path)?;
        let fragment = fragment.map(|s| urlencoding::decode(s).map(|s| s.into_owned())).transpose().map_err(AppError::io)?;
        Ok(ResolvedLink { relative_path, fragment, directory: resolved.is_dir() })
    }
}

#[derive(Default)]
pub struct BrowserSessionStore { inner: Mutex<Registry> }
#[derive(Default)]
struct Registry {
    sessions: HashMap<String, (Arc<BrowserSession>, Instant)>,
    requests: HashMap<String, (Arc<AtomicBool>, Option<String>, Instant)>,
}

impl BrowserSessionStore {
    pub fn begin(&self, request_id: &str) -> Result<Arc<AtomicBool>, AppError> {
        if request_id.len() > 128 || request_id.is_empty() { return Err(AppError::invalid_input("reader.invalidPath")); }
        let mut inner = self.inner.lock().unwrap();
        inner.sessions.retain(|_, (_, at)| at.elapsed() < TTL);
        inner.requests.retain(|_, (_, _, at)| at.elapsed() < TTL);
        let token = inner.requests.entry(request_id.to_owned()).or_insert_with(|| (Arc::new(AtomicBool::new(false)), None, Instant::now()));
        Ok(token.0.clone())
    }

    pub fn publish(&self, request_id: &str, session: BrowserSession) -> Result<SessionInfo, AppError> {
        let default_path = session.default_path()?;
        let mut inner = self.inner.lock().unwrap();
        let request = inner.requests.get_mut(request_id).ok_or_else(|| AppError::cancelled("reader.cancelled"))?;
        if request.0.load(Ordering::SeqCst) { return Err(AppError::cancelled("reader.cancelled")); }
        let id = uuid::Uuid::new_v4().to_string();
        request.1 = Some(id.clone());
        let info = SessionInfo { session_id: id.clone(), source_label: session.source_label.clone(), revision: session.revision.clone(), default_path, read_only: true };
        inner.sessions.insert(id, (Arc::new(session), Instant::now()));
        Ok(info)
    }

    pub fn get(&self, id: &str) -> Result<Arc<BrowserSession>, AppError> {
        let mut inner = self.inner.lock().unwrap();
        let (session, at) = inner.sessions.get_mut(id).ok_or_else(|| AppError::not_found("reader.sessionExpired"))?;
        if at.elapsed() > TTL { inner.sessions.remove(id); return Err(AppError::not_found("reader.sessionExpired")); }
        *at = Instant::now();
        Ok(session.clone())
    }

    pub fn close(&self, id: &str) { self.inner.lock().unwrap().sessions.remove(id); }
    pub fn cancel(&self, request_id: &str) {
        let mut inner = self.inner.lock().unwrap();
        let request = inner.requests.entry(request_id.to_owned()).or_insert_with(|| (Arc::new(AtomicBool::new(true)), None, Instant::now()));
        request.0.store(true, Ordering::SeqCst);
        if let Some(id) = request.1.take() { inner.sessions.remove(&id); }
    }
}

#[cfg(test)]
#[path = "skill_browser_tests.rs"]
mod tests;
