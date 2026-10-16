use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use walkdir::WalkDir;

const CONFIG_FILE_NAME: &str = "repo-config.json";

static BASE_DIR_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
/// Test-only redirection of the home directory, so a test can exercise paths
/// that are deliberately *not* relocatable by the user (see `cli_bridge`).
static HOME_DIR_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static SKILLS_DIR_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static STARTUP_WARNINGS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static STARTUP_ERROR_LOG: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn push_startup_warning(code: &str) {
    let mut warnings = STARTUP_WARNINGS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if !warnings.iter().any(|w| w == code) {
        warnings.push(code.to_string());
    }
}

/// Warning codes recorded while resolving the central repository at startup.
/// The frontend maps them to localized banner text (`settings.repoWarning_*`).
pub fn startup_warnings() -> Vec<String> {
    STARTUP_WARNINGS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Record a detailed startup error for later logging. `ensure_central_repo`
/// runs before `tauri_plugin_log` is installed (see `run()` in lib.rs), so a
/// `log::error!` here is swallowed by the default no-op logger. Stash the
/// detail and let `setup` flush it once the real logger exists.
pub(crate) fn record_startup_error(message: String) {
    STARTUP_ERROR_LOG
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(message);
}

/// Drain the startup errors stashed by [`record_startup_error`]. Called from
/// `tauri::Builder::setup` once the logger is up so the detail lands in the log
/// file that a support bundle collects.
pub fn take_startup_errors() -> Vec<String> {
    let mut guard = STARTUP_ERROR_LOG
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::mem::take(&mut guard)
}

/// Global mutex shared by every test that mutates the base-dir override via
/// [`set_test_base_dir_override`]. The override is process-wide static state,
/// so any two tests holding their own per-module locks can still race. Tests
/// must take this guard before calling `set_test_base_dir_override` and keep
/// it alive until they restore the previous value.
#[cfg(test)]
static TEST_BASE_DIR_GUARD: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(test)]
pub(crate) fn test_base_dir_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_BASE_DIR_GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RepoPathConfig {
    repo_path: Option<String>,
    pending_migration_from: Option<String>,
    /// Where the library lived before a move that has completed. Links and DB
    /// paths still pointing there are rewritten once the store is open, then
    /// this is cleared (see [`take_repoint_from`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repoint_from: Option<String>,
}

fn default_base_dir() -> PathBuf {
    home_base_dir()
}

/// `~/.skills-manager`, ignoring any configured relocation.
///
/// The library can be moved anywhere the user likes, but a few things must
/// stay where another program can find them without being told — the CLI
/// bridge an agent runs, above all. Those use this rather than [`base_dir`].
pub fn home_base_dir() -> PathBuf {
    if let Some(path) = HOME_DIR_OVERRIDE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .clone()
    {
        return path.join(".skills-manager");
    }
    dirs::home_dir()
        .expect("Cannot determine home directory")
        .join(".skills-manager")
}

#[cfg(test)]
pub(crate) fn set_test_home_dir_override(path: Option<PathBuf>) {
    *HOME_DIR_OVERRIDE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = path;
}

fn config_file_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(default_base_dir)
        .join("skills-manager")
        .join(CONFIG_FILE_NAME)
}

/// Distinguishes "no config file" (normal fresh install) from "config file
/// exists but cannot be used" (must never be silently treated as a fresh
/// install — that is how a configured library turns into an empty default
/// one and users report "all my skills are gone", issue #228 review).
#[derive(Debug)]
enum ConfigState {
    Missing,
    Valid(RepoPathConfig),
    Invalid(String),
}

fn load_config_state_from(path: &Path) -> ConfigState {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return ConfigState::Missing,
        Err(err) => {
            return ConfigState::Invalid(format!("cannot read {}: {err}", path.display()));
        }
    };
    match serde_json::from_str(&raw) {
        Ok(config) => ConfigState::Valid(config),
        Err(err) => ConfigState::Invalid(format!("corrupt JSON in {}: {err}", path.display())),
    }
}

fn load_config_state() -> ConfigState {
    load_config_state_from(&config_file_path())
}

fn load_config() -> RepoPathConfig {
    match load_config_state() {
        ConfigState::Valid(config) => config,
        ConfigState::Missing | ConfigState::Invalid(_) => RepoPathConfig::default(),
    }
}

fn save_config(config: &RepoPathConfig) -> Result<()> {
    let path = config_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(config)?)?;
    Ok(())
}

fn normalize_path(raw: &str) -> Result<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("Path cannot be empty"));
    }

    let expanded = if trimmed == "~" {
        dirs::home_dir().ok_or_else(|| anyhow!("Cannot determine home directory"))?
    } else if trimmed.starts_with("~/") || trimmed.starts_with("~\\") {
        dirs::home_dir()
            .ok_or_else(|| anyhow!("Cannot determine home directory"))?
            .join(&trimmed[2..])
    } else {
        PathBuf::from(trimmed)
    };

    if !expanded.is_absolute() {
        return Err(anyhow!("Central repository path must be absolute"));
    }

    let mut normalized = PathBuf::new();
    for component in expanded.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

pub fn configured_base_dir() -> Option<PathBuf> {
    load_config()
        .repo_path
        .and_then(|path| normalize_path(&path).ok())
}

/// Where the user asked the library to live (takes effect at the next launch).
fn requested_base_from(config: &RepoPathConfig) -> PathBuf {
    config
        .repo_path
        .as_deref()
        .and_then(|path| normalize_path(path).ok())
        .unwrap_or_else(default_base_dir)
}

/// Where the library actually is: the source of a move that hasn't happened
/// yet, otherwise the requested location. Saving a new path therefore never
/// switches a running session — the move happens at the next launch.
fn live_base_from(config: &RepoPathConfig) -> PathBuf {
    if let Some(source) = config
        .pending_migration_from
        .as_deref()
        .and_then(|path| normalize_path(path).ok())
    {
        if source.is_dir() {
            return source;
        }
    }
    requested_base_from(config)
}

pub fn base_dir() -> PathBuf {
    if let Some(path) = BASE_DIR_OVERRIDE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .clone()
    {
        return path;
    }

    live_base_from(&load_config())
}

/// The location a pending move will go to at the next launch, if any.
pub fn pending_base_dir() -> Option<PathBuf> {
    if base_dir_override_active() {
        return None;
    }
    let config = load_config();
    let requested = requested_base_from(&config);
    (live_base_from(&config) != requested).then_some(requested)
}

/// Whether an explicit runtime base-dir override is active (CLI `--skills-root`
/// / `--path`). Startup migration is skipped when it is — the caller chose a
/// specific library and the app's shared pending-migration marker doesn't apply.
fn base_dir_override_active() -> bool {
    BASE_DIR_OVERRIDE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some()
}

pub fn set_runtime_base_dir_override(path: Option<PathBuf>) {
    *BASE_DIR_OVERRIDE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = path;
}

pub fn set_runtime_skills_dir_override(path: Option<PathBuf>) {
    *SKILLS_DIR_OVERRIDE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = path;
}

#[cfg(test)]
pub(crate) fn set_test_base_dir_override(path: Option<PathBuf>) {
    set_runtime_base_dir_override(path);
    set_runtime_skills_dir_override(None);
}

pub fn skills_dir() -> PathBuf {
    if let Some(path) = SKILLS_DIR_OVERRIDE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .clone()
    {
        return path;
    }
    base_dir().join("skills")
}

/// Derive a stable per-skills-root state directory under the user's default base.
///
/// CLI's `--skills-root` lets agents operate on an external skills checkout
/// (e.g. a freshly cloned `my-skills`) without touching the app's default repo.
/// The manager still needs a home for its DB, scenarios, cache, and logs — but
/// putting that state inside the external checkout would pollute the user's
/// repo, and putting it in the parent directory would silently litter wherever
/// the user happened to clone. Instead, namespace the state under
/// `<default-base>/external/<sanitized-name>-<short-hash>/`, keyed by the
/// canonical path of the skills root so repeat invocations reuse the same DB.
pub fn external_base_dir(skills_root: &Path) -> PathBuf {
    // canonicalize() requires the path to exist. For not-yet-cloned targets we
    // still want a stable namespace, so fall back to absolutizing + lexically
    // normalizing the path. Without this, `./my-skills`, `my-skills`, and
    // `a/../my-skills` would hash to different namespaces despite resolving
    // to the same location.
    let canonical = match skills_root.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            let absolute = if skills_root.is_absolute() {
                skills_root.to_path_buf()
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(skills_root))
                    .unwrap_or_else(|_| skills_root.to_path_buf())
            };
            lexically_normalize(&absolute)
        }
    };
    let name = canonical
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("external");
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    let short_hash: String = digest.iter().take(5).map(|b| format!("{:02x}", b)).collect();
    default_base_dir()
        .join("external")
        .join(format!("{}-{}", sanitize_dir_name(name), short_hash))
}

/// Lexically normalize `.` and `..` segments without touching the filesystem.
/// `..` over a normal segment cancels it; `..` over a root or another `..`
/// is preserved (so we don't pretend to escape the filesystem root).
fn lexically_normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out: Vec<Component> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir) | Some(Component::Prefix(_)) => {
                    // can't go above root — drop the `..`
                }
                _ => out.push(comp),
            },
            other => out.push(other),
        }
    }
    out.iter().collect()
}

fn sanitize_dir_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "external".to_string()
    } else {
        cleaned
    }
}

pub fn scenarios_dir() -> PathBuf {
    base_dir().join("scenarios")
}

pub fn packages_dir() -> PathBuf {
    base_dir().join("packages")
}

pub fn cache_dir() -> PathBuf {
    base_dir().join("cache")
}

pub fn logs_dir() -> PathBuf {
    base_dir().join("logs")
}

pub fn db_path() -> PathBuf {
    base_dir().join("skills-manager.db")
}

pub fn set_base_dir_override(path: Option<String>) -> Result<PathBuf> {
    let mut config = load_config();

    // Resolve from the persisted config, never `base_dir()`: a runtime override
    // (CLI `--skills-root`) is not where the app's library lives. Changing the
    // path twice before a restart still migrates from where the data really is.
    let data_location = live_base_from(&config);

    let (next, persist_repo_path) = match path {
        Some(raw) => (normalize_path(&raw)?, true),
        None => (default_base_dir(), false),
    };

    config.repo_path = if persist_repo_path {
        Some(next.to_string_lossy().to_string())
    } else {
        None
    };
    config.pending_migration_from = if next != data_location {
        Some(data_location.to_string_lossy().to_string())
    } else {
        None
    };
    save_config(&config)?;
    Ok(next)
}

fn directory_has_entries(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    Ok(fs::read_dir(path)?.next().is_some())
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    for entry in WalkDir::new(source) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source)?;
        let destination = target.join(relative);
        if entry.file_type().is_symlink() {
            copy_symlink(entry.path(), &destination)?;
        } else if entry.file_type().is_dir() {
            fs::create_dir_all(&destination)?;
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &destination).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    entry.path().display(),
                    destination.display()
                )
            })?;
        }
    }
    Ok(())
}

/// Recreate a link as a link. Following it would turn a file link into a copy
/// and fail outright on a directory link, aborting a cross-volume move.
fn copy_symlink(source: &Path, destination: &Path) -> Result<()> {
    let link = fs::read_link(source)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&link, destination)?;
    #[cfg(windows)]
    {
        if fs::metadata(source).map(|m| m.is_dir()).unwrap_or(false) {
            std::os::windows::fs::symlink_dir(&link, destination)?;
        } else {
            std::os::windows::fs::symlink_file(&link, destination)?;
        }
    }
    Ok(())
}

/// Files the app recreates on its own. A target holding nothing else is not a
/// library: it is what an earlier session or the CLI bridge left behind.
const REGENERABLE_ROOT_FILES: &[&str] = &[".skills-manager.lock", "git-askpass.sh"];
/// OS metadata, debris wherever it appears.
const OS_METADATA_FILES: &[&str] = &[".DS_Store", "Thumbs.db", "desktop.ini"];

/// Whether `path` can be dropped safely: a regenerable file, the CLI bridge's
/// files in the default home's `bin/` (republished at every launch, so they
/// must not block moving the library back home), or a directory holding only
/// such things. Links are never followed or counted as debris, and anything
/// that cannot be inspected is kept.
fn is_regenerable(path: &Path, target: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if meta.is_file() {
        let parent = path.parent();
        let in_bridge_dir = parent == Some(super::cli_bridge::bridge_dir().as_path());
        return OS_METADATA_FILES.contains(&name)
            || (parent == Some(target) && REGENERABLE_ROOT_FILES.contains(&name))
            || (in_bridge_dir && super::cli_bridge::is_bridge_file(name));
    }
    if !meta.is_dir() {
        return false;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    entries
        .into_iter()
        .all(|entry| entry.is_ok_and(|entry| is_regenerable(&entry.path(), target)))
}

/// Remove debris from a migration target so the move can proceed. All-or-
/// nothing: if anything in it is not known debris, nothing is touched.
fn clear_regenerable_target(target: &Path) -> Result<()> {
    let Ok(entries) = fs::read_dir(target) else {
        return Ok(());
    };
    let Ok(entries) = entries
        .map(|entry| entry.map(|e| e.path()))
        .collect::<std::io::Result<Vec<PathBuf>>>()
    else {
        return Ok(());
    };
    if !entries.iter().all(|path| is_regenerable(path, target)) {
        return Ok(());
    }
    for path in entries {
        if fs::symlink_metadata(&path)?.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Whether two paths resolve to the same directory. Falls back to a lexical
/// comparison when either side can't be canonicalized (e.g. the target does not
/// exist yet), so a purely cosmetic difference (case, `8.3` names, a symlink)
/// isn't mistaken for a real relocation.
fn paths_are_same_dir(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

/// Move by copying into the (empty) target, for when a rename can't (another
/// volume). On failure the partial copy is removed — everything in the target
/// is ours, and left behind it would block every retry as "not empty". On
/// success the old copy is kept but set aside: left in place it looks like the
/// live library and blocks ever moving back to that path.
fn move_by_copy(source: &Path, target: &Path) -> Result<()> {
    if let Err(err) = copy_dir_recursive(source, target) {
        if let Ok(entries) = fs::read_dir(target) {
            for entry in entries.flatten() {
                let path = entry.path();
                let _ = if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_file(&path)
                };
            }
        }
        return Err(err);
    }
    let aside = source.with_file_name(format!(
        "{}.moved-{}",
        source.file_name().and_then(|n| n.to_str()).unwrap_or("library"),
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    ));
    if let Err(err) = fs::rename(source, &aside) {
        record_startup_error(format!(
            "central repo: copied the library to {}, but cannot set the old copy at {} aside ({err})",
            target.display(),
            source.display()
        ));
    }
    Ok(())
}

/// What the caller should do after attempting a pending central-repo move.
enum MigrationOutcome {
    /// No move was pending, or it completed. Run against the configured base.
    Proceed,
    /// The move could not complete safely. The marker stays, so `base_dir()`
    /// keeps resolving to the intact source; the next launch retries.
    UseSource,
}

/// Try to satisfy a pending central-repository relocation.
///
/// This runs before the logger, the panic hook, and the window exist (see
/// `run()` in lib.rs), so it must never return an error that would panic the
/// process into a windowless death (#252). Every failure instead records a
/// startup warning + a deferred log line and falls back to the source, where
/// the user's data is known to be intact. It mutates `config` in place but
/// does NOT persist it — the caller saves once, which also keeps this unit
/// testable without touching the real config file.
fn migrate_repo_if_needed(config: &mut RepoPathConfig, current_base: &Path) -> MigrationOutcome {
    let Some(source_raw) = config.pending_migration_from.clone() else {
        return MigrationOutcome::Proceed;
    };
    let source = match normalize_path(&source_raw) {
        Ok(path) => path,
        Err(err) => {
            // The stored path is unusable, so the move can never proceed. Drop
            // the marker to stop retrying every launch and run against target.
            record_startup_error(format!(
                "central repo: pending migration source {source_raw:?} is invalid ({err}); dropping it"
            ));
            config.pending_migration_from = None;
            return MigrationOutcome::Proceed;
        }
    };

    // Nothing left to move: the source is gone (moved already, or the old
    // location was removed), or source and target are the same directory.
    // Compare canonically, not just lexically — on a case-insensitive volume
    // `D:\Skills` and `d:\skills` are one directory (likewise 8.3 vs long, or a
    // symlink), and a lexical mismatch would otherwise loop forever on
    // `migration_incomplete`, telling the user to empty their own library.
    if !source.exists() || paths_are_same_dir(&source, current_base) {
        // A source that is gone while the target exists is a move that
        // finished but was never recorded (crash or failed config save right
        // after the rename): its links and DB paths still need repointing.
        if !source.exists() && current_base.is_dir() {
            config.repoint_from = Some(source.to_string_lossy().to_string());
        }
        config.pending_migration_from = None;
        return MigrationOutcome::Proceed;
    }

    // A target nested inside the source can never be a valid destination.
    if current_base.starts_with(&source) {
        record_startup_error(format!(
            "central repo: migration target {} is inside source {}; keeping data at the source",
            current_base.display(),
            source.display()
        ));
        push_startup_warning("migration_incomplete");
        return MigrationOutcome::UseSource;
    }

    // Only ever move into an absent/empty target — never blind-merge. A
    // non-empty target is either a real library we must not overwrite or debris
    // from a failed attempt we cannot tell apart; keeping the user on their
    // intact source is lossless, overwriting is not. A fresh target also means
    // the recursive copy only ever creates new files, so it can never hit the
    // read-only git pack files that overwriting bricked startup on (#252).
    if let Err(err) = clear_regenerable_target(current_base) {
        record_startup_error(format!(
            "central repo: cannot clear leftovers in migration target {} ({err})",
            current_base.display()
        ));
    }
    let target_empty = match directory_has_entries(current_base) {
        Ok(has_entries) => !has_entries,
        Err(err) => {
            record_startup_error(format!(
                "central repo: cannot inspect migration target {} ({err}); keeping data at source {}",
                current_base.display(),
                source.display()
            ));
            push_startup_warning("migration_incomplete");
            return MigrationOutcome::UseSource;
        }
    };
    if !target_empty {
        record_startup_error(format!(
            "central repo: migration target {} is not empty; keeping data at source {}",
            current_base.display(),
            source.display()
        ));
        push_startup_warning("migration_incomplete");
        return MigrationOutcome::UseSource;
    }

    if let Some(parent) = current_base.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            record_startup_error(format!(
                "central repo: cannot create migration target parent {} ({err}); keeping data at source {}",
                parent.display(),
                source.display()
            ));
            push_startup_warning("migration_incomplete");
            return MigrationOutcome::UseSource;
        }
    }

    // Same volume: an atomic rename moves the whole tree cheaply. Cross volume
    // (or a rename the OS refuses): copy into the empty target. Because the
    // target is empty, no existing file is ever overwritten.
    if fs::rename(&source, current_base).is_err() {
        if let Err(err) = move_by_copy(&source, current_base) {
            record_startup_error(format!(
                "central repo: migration copy from {} to {} failed ({err:#}); keeping data at source",
                source.display(),
                current_base.display()
            ));
            push_startup_warning("migration_incomplete");
            return MigrationOutcome::UseSource;
        }
    }

    config.pending_migration_from = None;
    config.repoint_from = Some(source.to_string_lossy().to_string());
    MigrationOutcome::Proceed
}

/// Every process using the app's library holds this lease shared for its
/// whole life; moving the library takes it exclusively. So a move happens only
/// while nothing else has the library open (a running app, an agent's CLI call,
/// a second launch), and whoever starts mid-move waits for it to finish. The
/// file lives next to the config, which never moves with the library.
static LIBRARY_LEASE: OnceLock<Mutex<Option<fs::File>>> = OnceLock::new();

fn lease_slot() -> std::sync::MutexGuard<'static, Option<fs::File>> {
    LIBRARY_LEASE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Take the lease for the rest of this process. Returns whether it is held
/// exclusively — only then may this process move the library. Without a lock
/// file (unwritable config dir) nobody moves anything, which is safe.
fn take_library_lease(try_exclusive: bool) -> bool {
    use fs2::FileExt;
    let mut slot = lease_slot();
    if slot.is_some() {
        return false;
    }
    let path = config_file_path().with_file_name("library.lock");
    let file = path
        .parent()
        .and_then(|parent| fs::create_dir_all(parent).ok())
        .and_then(|_| {
            fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path)
                .ok()
        });
    let Some(file) = file else {
        return false;
    };
    // Fully qualified: newer std `File` has inherent lock methods of the same
    // names; keep every call on one implementation.
    let exclusive = try_exclusive && FileExt::try_lock_exclusive(&file).is_ok();
    if !exclusive {
        // Blocks only while another process is moving the library.
        let _ = FileExt::lock_shared(&file);
    }
    *slot = Some(file);
    exclusive
}

/// After a move, go back to sharing the library with other processes.
fn downgrade_library_lease() {
    use fs2::FileExt;
    if let Some(file) = lease_slot().as_ref() {
        let _ = FileExt::unlock(file);
        let _ = FileExt::lock_shared(file);
    }
}

/// Take the pre-move location recorded by a completed move, if links and DB
/// paths pointing there still need rewriting. Call [`clear_repoint_from`] once
/// that is done, so a crash in between retries at the next launch.
pub fn take_repoint_from() -> Option<(PathBuf, PathBuf)> {
    if base_dir_override_active() {
        return None;
    }
    let config = load_config();
    let from = normalize_path(config.repoint_from.as_deref()?).ok()?;
    Some((from, base_dir()))
}

pub fn clear_repoint_from() -> Result<()> {
    let mut config = load_config();
    if config.repoint_from.take().is_some() {
        save_config(&config)?;
    }
    Ok(())
}

/// `allow_migration`: whether this process may carry out a pending move. Only
/// the app at startup and an explicit CLI `repo set-path` do; everything else
/// keeps running against the library where it is.
pub fn ensure_central_repo(allow_migration: bool) -> Result<()> {
    // A config file that exists but cannot be used means the app is about to
    // run against the default location even though the user configured (and
    // populated) another one. Never let that pass silently — it presents as
    // "the library was rebuilt empty, all skills lost" (#228 review).
    let mut config = match load_config_state() {
        ConfigState::Valid(config) => {
            if let Some(raw) = config.repo_path.as_deref() {
                if let Err(err) = normalize_path(raw) {
                    log::error!(
                        "central repo: configured repo_path {raw:?} is invalid ({err}); \
                         falling back to the default location"
                    );
                    push_startup_warning("repo_path_invalid");
                }
            }
            config
        }
        ConfigState::Missing => RepoPathConfig::default(),
        ConfigState::Invalid(detail) => {
            log::error!(
                "central repo: config is unreadable ({detail}); \
                 falling back to the default location"
            );
            push_startup_warning("config_unreadable");
            RepoPathConfig::default()
        }
    };

    // Only auto-migrate the app's own config-driven base. When a runtime base
    // override is active (CLI `--skills-root` / `--path`), the pending marker in
    // the shared config belongs to a different library and must not be applied
    // to — or override — the explicitly chosen root. The app's own startup never
    // sets an override before this point, so the #252 path is unaffected.
    // A `--skills-root` run keeps its state under the default home too, so it
    // shares the lease; it just never moves the app's library.
    let override_active = base_dir_override_active();
    let may_move = take_library_lease(
        allow_migration && !override_active && config.pending_migration_from.is_some(),
    );
    if may_move {
        // Re-read: another process may have finished a move while we waited.
        config = load_config();
        let pending_before = config.pending_migration_from.clone();
        let target = requested_base_from(&config);
        let _ = migrate_repo_if_needed(&mut config, &target);
        if config.pending_migration_from != pending_before {
            if let Err(err) = save_config(&config) {
                record_startup_error(format!(
                    "central repo: failed to persist migration state ({err}); it may retry next launch"
                ));
            }
        }
        downgrade_library_lease();
    }
    // Re-resolve: a completed move changed the base.
    let current_base = base_dir();

    // Legacy `.agent-skills` migration must run before create_dir_all below:
    // it renames entries into `current_base` and skips ones that already
    // exist, so pre-created empty dirs would silently swallow it (the old
    // ordering made this branch dead code).
    let legacy_path = dirs::home_dir().map(|home| home.join(".agent-skills"));
    if let Some(old_path) = legacy_path {
        if old_path.exists() && !current_base.join("skills").exists() {
            log::info!("Migrating from old path {:?}", old_path);
            fs::create_dir_all(&current_base)?;
            if let Ok(entries) = fs::read_dir(&old_path) {
                for entry in entries.flatten() {
                    let dest = current_base.join(entry.file_name());
                    if !dest.exists() {
                        let _ = fs::rename(entry.path(), &dest);
                    }
                }
            }
        }
    }

    let dirs = [skills_dir(), scenarios_dir(), packages_dir(), cache_dir(), logs_dir()];
    for d in &dirs {
        fs::create_dir_all(d)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── migrate_repo_if_needed (#252) ──

    fn config_migrating(source: &Path, target: &Path) -> RepoPathConfig {
        RepoPathConfig {
            repo_path: Some(target.to_string_lossy().to_string()),
            pending_migration_from: Some(source.to_string_lossy().to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn migration_into_empty_target_moves_and_clears_marker() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap(); // exists but empty
        fs::create_dir_all(src.path().join("skills")).unwrap();
        fs::write(src.path().join("skills/s.md"), b"skill").unwrap();

        let mut config = config_migrating(src.path(), dst.path());
        let outcome = migrate_repo_if_needed(&mut config, dst.path());

        assert!(matches!(outcome, MigrationOutcome::Proceed));
        assert_eq!(config.pending_migration_from, None);
        assert_eq!(fs::read(dst.path().join("skills/s.md")).unwrap(), b"skill");
    }

    #[test]
    fn live_base_is_the_move_source_until_the_move_happens() {
        // Saving a new path must not switch the running session: everything
        // it wrote would land in the target and block the move at the next
        // launch (#449 #469 #393).
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let config = config_migrating(src.path(), &dst.path().join("lib"));
        assert_eq!(
            live_base_from(&config),
            normalize_path(&src.path().to_string_lossy()).unwrap()
        );
        assert_eq!(
            requested_base_from(&config),
            normalize_path(&dst.path().join("lib").to_string_lossy()).unwrap()
        );

        // Once the source is gone (moved), the requested location is live.
        let moved = config_migrating(&src.path().join("gone"), &dst.path().join("lib"));
        assert_eq!(live_base_from(&moved), requested_base_from(&moved));
    }

    #[test]
    fn migration_clears_regenerable_leftovers_and_records_repoint() {
        // What an earlier session and the CLI bridge leave in a target: a
        // lock file, empty skeleton dirs, OS metadata, the bridge's `bin/`.
        let _guard = test_base_dir_lock();
        let src = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        set_test_home_dir_override(Some(home.path().to_path_buf()));
        let dst = home_base_dir(); // moving back to the default home
        fs::write(src.path().join("a.txt"), b"src").unwrap();
        fs::create_dir_all(&dst).unwrap();
        fs::write(dst.join(".skills-manager.lock"), b"pid=1").unwrap();
        fs::write(dst.join(".DS_Store"), b"").unwrap();
        fs::create_dir_all(dst.join("skills")).unwrap();
        fs::create_dir_all(dst.join("cache/repos")).unwrap();
        let bridge = super::super::cli_bridge::bridge_path();
        fs::create_dir_all(bridge.parent().unwrap()).unwrap();
        fs::write(&bridge, b"bin").unwrap();
        fs::write(bridge.with_file_name(".version"), b"1.0").unwrap();

        let mut config = config_migrating(src.path(), &dst);
        let outcome = migrate_repo_if_needed(&mut config, &dst);
        set_test_home_dir_override(None);

        assert!(matches!(outcome, MigrationOutcome::Proceed));
        assert_eq!(config.pending_migration_from, None);
        assert!(config.repoint_from.is_some());
        assert_eq!(fs::read(dst.join("a.txt")).unwrap(), b"src");
    }

    #[test]
    fn app_file_names_count_as_debris_only_at_the_target_root() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        fs::write(src.path().join("a.txt"), b"src").unwrap();
        fs::create_dir_all(dst.path().join("tools")).unwrap();
        fs::write(dst.path().join("tools/git-askpass.sh"), b"mine").unwrap();

        let mut config = config_migrating(src.path(), dst.path());
        let outcome = migrate_repo_if_needed(&mut config, dst.path());

        assert!(matches!(outcome, MigrationOutcome::UseSource));
        assert!(dst.path().join("tools/git-askpass.sh").exists());
    }

    #[test]
    fn move_by_copy_sets_the_old_copy_aside() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("lib");
        let dst = tmp.path().join("new");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a.txt"), b"src").unwrap();

        move_by_copy(&src, &dst).unwrap();

        assert_eq!(fs::read(dst.join("a.txt")).unwrap(), b"src");
        assert!(!src.exists(), "the path is free to move back to");
        let aside: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("lib.moved-"))
            .collect();
        assert_eq!(aside.len(), 1, "the old copy is kept");
    }

    #[test]
    #[cfg(unix)]
    fn move_by_copy_failure_leaves_the_target_empty() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("lib");
        let dst = tmp.path().join("new");
        fs::create_dir_all(src.join("a")).unwrap();
        fs::write(src.join("a/ok.txt"), b"x").unwrap();
        fs::write(src.join("z-unreadable"), b"x").unwrap();
        fs::set_permissions(src.join("z-unreadable"), fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(src.join("z-unreadable")).is_ok() {
            return; // running as root: can't provoke the failure
        }

        assert!(move_by_copy(&src, &dst).is_err());

        assert!(!directory_has_entries(&dst).unwrap(), "retry must see an empty target");
        assert!(src.join("a/ok.txt").exists(), "source untouched");
        fs::set_permissions(src.join("z-unreadable"), fs::Permissions::from_mode(0o644)).unwrap();
    }

    #[test]
    fn a_bin_dir_outside_the_default_home_is_not_debris() {
        // Only the default home's `bin/` holds the bridge; elsewhere a file
        // with the bridge's name is the user's.
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        fs::write(src.path().join("a.txt"), b"src").unwrap();
        fs::create_dir_all(dst.path().join("bin")).unwrap();
        fs::write(dst.path().join("bin/skills-manager-cli"), b"mine").unwrap();

        let mut config = config_migrating(src.path(), dst.path());
        let outcome = migrate_repo_if_needed(&mut config, dst.path());

        assert!(matches!(outcome, MigrationOutcome::UseSource));
        assert!(dst.path().join("bin/skills-manager-cli").exists());
    }

    #[test]
    fn a_move_that_finished_unrecorded_still_repoints() {
        // Crash (or failed config save) right after the rename: the source is
        // gone, the target holds the library, the marker is still pending.
        let dst = tempfile::tempdir().unwrap();
        let gone = dst.path().join("moved-away");
        let mut config = config_migrating(&gone, dst.path());

        let outcome = migrate_repo_if_needed(&mut config, dst.path());

        assert!(matches!(outcome, MigrationOutcome::Proceed));
        assert_eq!(config.pending_migration_from, None);
        assert_eq!(config.repoint_from.as_deref(), Some(gone.to_string_lossy().as_ref()));
    }

    #[test]
    fn migration_leaves_a_target_with_real_content_untouched() {
        // One real file among the leftovers: nothing is removed.
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        fs::write(src.path().join("a.txt"), b"src").unwrap();
        fs::write(dst.path().join(".skills-manager.lock"), b"").unwrap();
        fs::create_dir_all(dst.path().join("skills/mine")).unwrap();
        fs::write(dst.path().join("skills/mine/SKILL.md"), b"x").unwrap();

        let mut config = config_migrating(src.path(), dst.path());
        let outcome = migrate_repo_if_needed(&mut config, dst.path());

        assert!(matches!(outcome, MigrationOutcome::UseSource));
        assert!(dst.path().join(".skills-manager.lock").exists());
        assert!(dst.path().join("skills/mine/SKILL.md").exists());
        assert_eq!(config.repoint_from, None);
    }

    #[test]
    #[cfg(unix)]
    fn migration_does_not_follow_a_link_disguised_as_a_skeleton_dir() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(src.path().join("a.txt"), b"src").unwrap();
        std::os::unix::fs::symlink(outside.path(), dst.path().join("skills")).unwrap();

        let mut config = config_migrating(src.path(), dst.path());
        let outcome = migrate_repo_if_needed(&mut config, dst.path());

        assert!(matches!(outcome, MigrationOutcome::UseSource));
        assert!(outside.path().exists());
    }

    #[test]
    #[cfg(unix)]
    fn copy_dir_recursive_keeps_links_as_links() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        fs::create_dir_all(src.path().join("real")).unwrap();
        fs::write(src.path().join("real/f"), b"x").unwrap();
        std::os::unix::fs::symlink("real", src.path().join("dir-link")).unwrap();
        std::os::unix::fs::symlink("real/f", src.path().join("file-link")).unwrap();

        copy_dir_recursive(src.path(), &dst.path().join("out")).unwrap();

        let out = dst.path().join("out");
        assert_eq!(fs::read_link(out.join("dir-link")).unwrap(), Path::new("real"));
        assert_eq!(fs::read_link(out.join("file-link")).unwrap(), Path::new("real/f"));
    }

    #[test]
    fn migration_into_nonempty_target_keeps_source_and_marker() {
        // The whole point of #252's safety: never blind-merge over a
        // non-empty target (real data or failed-attempt debris we can't tell
        // apart). Fall back to the intact source and keep retrying.
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        fs::write(src.path().join("a.txt"), b"src").unwrap();
        fs::write(dst.path().join("existing.txt"), b"dst-data").unwrap();

        let mut config = config_migrating(src.path(), dst.path());
        let outcome = migrate_repo_if_needed(&mut config, dst.path());

        assert!(matches!(outcome, MigrationOutcome::UseSource));
        assert_eq!(
            live_base_from(&config),
            normalize_path(&src.path().to_string_lossy()).unwrap()
        );
        assert!(config.pending_migration_from.is_some(), "marker kept for retry");
        assert_eq!(fs::read(dst.path().join("existing.txt")).unwrap(), b"dst-data");
        assert_eq!(fs::read(src.path().join("a.txt")).unwrap(), b"src");
    }

    #[test]
    #[cfg(unix)]
    fn migration_same_dir_via_symlink_clears_marker() {
        // A cosmetic path difference that resolves to the same directory (here
        // a symlink; on Windows, case / 8.3 names) must not be mistaken for a
        // real relocation — otherwise it loops forever on `migration_incomplete`
        // telling the user to empty their own library.
        let real = tempfile::tempdir().unwrap();
        fs::create_dir_all(real.path().join("skills")).unwrap();
        let link_parent = tempfile::tempdir().unwrap();
        let link = link_parent.path().join("aliased");
        std::os::unix::fs::symlink(real.path(), &link).unwrap();

        let mut config = config_migrating(real.path(), &link);
        let outcome = migrate_repo_if_needed(&mut config, &link);

        assert!(matches!(outcome, MigrationOutcome::Proceed));
        assert_eq!(config.pending_migration_from, None, "same-dir move clears marker");
        // The real library is untouched.
        assert!(real.path().join("skills").exists());
    }

    #[test]
    fn migration_with_missing_source_clears_marker() {
        let dst = tempfile::tempdir().unwrap();
        let missing = dst.path().join("does-not-exist");
        let mut config = config_migrating(&missing, dst.path());

        let outcome = migrate_repo_if_needed(&mut config, dst.path());
        assert!(matches!(outcome, MigrationOutcome::Proceed));
        assert_eq!(config.pending_migration_from, None);
    }

    #[test]
    fn no_pending_migration_is_a_noop() {
        let dst = tempfile::tempdir().unwrap();
        let mut config = RepoPathConfig {
            repo_path: Some(dst.path().to_string_lossy().to_string()),
            pending_migration_from: None,
            ..Default::default()
        };
        let outcome = migrate_repo_if_needed(&mut config, dst.path());
        assert!(matches!(outcome, MigrationOutcome::Proceed));
        assert_eq!(config.pending_migration_from, None);
    }

    #[test]
    fn copy_dir_recursive_copies_read_only_source_files() {
        // git pack files (.idx/.pack/.rev) are read-only. Copying them into a
        // fresh target must succeed — the #252 brick only happened when
        // OVERWRITING an existing read-only file, which migration now avoids by
        // only ever moving into an empty target.
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let pack = src.path().join("pack.idx");
        fs::write(&pack, b"packdata").unwrap();
        let mut perms = fs::metadata(&pack).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&pack, perms).unwrap();

        let target = dst.path().join("out");
        copy_dir_recursive(src.path(), &target).unwrap();
        assert_eq!(fs::read(target.join("pack.idx")).unwrap(), b"packdata");
    }

    // ── load_config_state_from ──

    #[test]
    fn config_state_missing_file_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let state = load_config_state_from(&tmp.path().join("repo-config.json"));
        assert!(matches!(state, ConfigState::Missing));
    }

    #[test]
    fn config_state_valid_json_is_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("repo-config.json");
        fs::write(&path, r#"{ "repo_path": "/tmp/lib", "pending_migration_from": null }"#)
            .unwrap();
        match load_config_state_from(&path) {
            ConfigState::Valid(config) => {
                assert_eq!(config.repo_path.as_deref(), Some("/tmp/lib"));
            }
            other => panic!("expected Valid, got {other:?}"),
        }
    }

    #[test]
    fn config_state_corrupt_json_is_invalid_not_fresh_install() {
        // A corrupt config must never be treated like a missing one — that is
        // the "library rebuilt empty, all skills lost" failure mode (#228).
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("repo-config.json");
        fs::write(&path, "{ not json").unwrap();
        let state = load_config_state_from(&path);
        assert!(matches!(state, ConfigState::Invalid(_)), "{state:?}");
    }

    #[test]
    fn external_base_dir_lives_under_default_base_external() {
        let dir = external_base_dir(Path::new("/tmp/some/my-skills"));
        let prefix = default_base_dir().join("external");
        assert!(
            dir.starts_with(&prefix),
            "expected {} to start with {}",
            dir.display(),
            prefix.display()
        );
    }

    #[test]
    fn external_base_dir_is_stable_for_same_path() {
        let a = external_base_dir(Path::new("/tmp/some/my-skills"));
        let b = external_base_dir(Path::new("/tmp/some/my-skills"));
        assert_eq!(a, b);
    }

    #[test]
    fn external_base_dir_differs_for_different_paths() {
        let a = external_base_dir(Path::new("/tmp/one/my-skills"));
        let b = external_base_dir(Path::new("/tmp/two/my-skills"));
        assert_ne!(a, b);
    }

    #[test]
    fn external_base_dir_does_not_pollute_skills_root_or_its_parent() {
        let skills_root = Path::new("/tmp/external-test/my-skills");
        let dir = external_base_dir(skills_root);
        assert!(!dir.starts_with(skills_root));
        assert!(!dir.starts_with(skills_root.parent().unwrap()));
    }

    #[test]
    fn sanitize_dir_name_replaces_unsafe_characters() {
        assert_eq!(sanitize_dir_name("my skills"), "my-skills");
        assert_eq!(sanitize_dir_name("a/b\\c:d"), "a-b-c-d");
        assert_eq!(sanitize_dir_name(""), "external");
    }

    #[test]
    fn external_base_dir_relative_path_is_stable_against_absolute_form() {
        // For a not-yet-existing target, a relative path should namespace the
        // same as its cwd-absolutized form. We simulate by passing both forms
        // and asserting they match.
        let cwd = std::env::current_dir().unwrap();
        let rel = Path::new("nonexistent-skills-target-xyz");
        let abs = cwd.join(rel);
        assert_eq!(external_base_dir(rel), external_base_dir(&abs));
    }

    #[test]
    fn external_base_dir_normalizes_redundant_segments() {
        // `./x`, `x`, and `a/../x` should all hash to the same namespace when
        // none of them exist on disk.
        let plain = external_base_dir(Path::new("nonexistent-norm-target"));
        let dot = external_base_dir(Path::new("./nonexistent-norm-target"));
        let parent = external_base_dir(Path::new("a/../nonexistent-norm-target"));
        assert_eq!(plain, dot);
        assert_eq!(plain, parent);
    }

    #[test]
    fn lexically_normalize_handles_basic_cases() {
        assert_eq!(
            lexically_normalize(Path::new("/a/./b/../c")),
            PathBuf::from("/a/c")
        );
        assert_eq!(
            lexically_normalize(Path::new("./a/b")),
            PathBuf::from("a/b")
        );
        assert_eq!(lexically_normalize(Path::new("/..")), PathBuf::from("/"));
    }
}
