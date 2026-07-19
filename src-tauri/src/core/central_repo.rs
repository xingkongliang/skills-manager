use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use walkdir::WalkDir;

const CONFIG_FILE_NAME: &str = "repo-config.json";

static BASE_DIR_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
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
fn record_startup_error(message: String) {
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
}

fn default_base_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Cannot determine home directory")
        .join(".skills-manager")
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

pub fn base_dir() -> PathBuf {
    if let Some(path) = BASE_DIR_OVERRIDE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .clone()
    {
        return path;
    }

    configured_base_dir().unwrap_or_else(default_base_dir)
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
    let current = base_dir();
    let mut config = load_config();

    // The actual on-disk data location can differ from `current` when the user
    // already changed the path once but hasn't restarted yet — `current` then
    // reflects the unsatisfied future target stored in `repo_path`, while the
    // data still sits at `pending_migration_from`. Track the true location so
    // multiple changes before restart still migrate from the right source.
    let data_location = match &config.pending_migration_from {
        Some(src) => match normalize_path(src) {
            Ok(path) if path.is_dir() => path,
            _ => current.clone(),
        },
        None => current.clone(),
    };

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
        if entry.file_type().is_dir() {
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

/// How a directory should land in the backup area before it is overwritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupKind {
    /// The caller is about to overwrite `src` in place, so the old tree is
    /// *moved* out of the way (source disappears, backup holds the only copy).
    Move,
    /// The caller keeps `src` as-is and only wants a snapshot, so the tree is
    /// *copied* (source stays put, backup is a duplicate).
    Copy,
}

/// Back a directory up under `<central_root>/backups/<skill_dir_name>/<reason>/`.
///
/// One snapshot per `(skill, reason)`: the destination name is the reason alone,
/// so a later backup for the same reason *replaces* the previous one instead of
/// piling up a fresh timestamped directory every time. This bounds `backups/` to
/// at most one entry per skill per overwrite path (reimport / explicit-pull /
/// spoke-ahead), which is why repeated overwrites can no longer grow it without
/// bound. Returns the directory the backup landed in.
///
/// The replacement is staged: the new snapshot is written to a temporary sibling
/// on the same volume first, then swapped into place, so a mid-write failure
/// never destroys the previous, still-valid backup. If the final swap fails the
/// staged copy is left intact rather than deleted — for a `Move` the source is
/// already gone, so the staged copy is the only surviving copy of the data and
/// must not be discarded.
///
/// `Move` is cross-volume safe: `src` may live on a different drive than the
/// central repo (project on `D:`, library on `C:`), where `std::fs::rename`
/// fails with `EXDEV`. On any rename failure we fall back to a recursive
/// copy-then-delete so the observable semantics stay "move".
pub fn backup_directory(
    central_root: &Path,
    src: &Path,
    skill_dir_name: &str,
    reason: &str,
    kind: BackupKind,
) -> Result<PathBuf> {
    let parent = central_root
        .join("backups")
        .join(sanitize_backup_component(skill_dir_name));
    fs::create_dir_all(&parent).with_context(|| {
        format!("Failed to create backup directory {}", parent.display())
    })?;

    let reason_name = sanitize_backup_component(reason);
    let dest = parent.join(&reason_name);

    // Stage the new snapshot under a temporary sibling, then swap it into place.
    // Until the swap completes the previous backup at `dest` stays untouched.
    let staging = unique_backup_dir(&parent, &format!("{reason_name}.incoming"));
    match kind {
        BackupKind::Move => move_directory(src, &staging)?,
        BackupKind::Copy => copy_dir_with_rollback(src, &staging)?,
    }

    // Drop the previous snapshot only now that the replacement is fully written.
    if dest.exists() {
        fs::remove_dir_all(&dest).with_context(|| {
            format!("Failed to remove previous backup {}", dest.display())
        })?;
    }
    fs::rename(&staging, &dest).with_context(|| {
        format!(
            "Failed to finalize backup {} at {}",
            staging.display(),
            dest.display()
        )
    })?;
    Ok(dest)
}

/// Pick a not-yet-existing directory under `parent`, adding `-1`, `-2`, … to
/// `base_name` on collision. Used to name the transient `.incoming` staging dir
/// so a leftover from an earlier interrupted backup is never reused mid-write.
//
// TOCTOU note: there is a gap between the `exists()` check and the caller's
// later `create_dir_all`/rename into the returned path. That race is acceptable
// here because every central-repo write is serialized by `RepoLock` (see
// core::repo_lock) — two backups never run concurrently, so no other writer can
// claim the name in between. Do not reach for an atomic create-if-absent here.
fn unique_backup_dir(parent: &Path, base_name: &str) -> PathBuf {
    let candidate = parent.join(base_name);
    if !candidate.exists() {
        return candidate;
    }
    let mut n = 1u32;
    loop {
        let candidate = parent.join(format!("{base_name}-{n}"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Move `src` to `dest`, falling back to copy-then-delete when a plain rename
/// can't cross volumes (or the OS otherwise refuses it).
fn move_directory(src: &Path, dest: &Path) -> Result<()> {
    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(_) => move_via_copy_delete(src, dest),
    }
}

/// The cross-volume fallback, factored out so it is unit-testable on a single
/// volume (a genuine cross-drive tempdir is hard to arrange in tests): copy the
/// whole tree, then remove the source, preserving "move" semantics.
fn move_via_copy_delete(src: &Path, dest: &Path) -> Result<()> {
    copy_dir_with_rollback(src, dest)?;
    fs::remove_dir_all(src).with_context(|| {
        format!(
            "Failed to remove source {} after backup copy to {}",
            src.display(),
            dest.display()
        )
    })?;
    Ok(())
}

/// Recursively copy `src` into `dest`, rolling back a half-written `dest` if the
/// copy fails partway. Without this, a consumer could later mistake a truncated
/// half-tree for a valid backup. The rollback is best-effort — if it too fails,
/// the original copy error still propagates. For `Move` the source is only
/// removed *after* a successful copy, so a failed backup never loses data.
fn copy_dir_with_rollback(src: &Path, dest: &Path) -> Result<()> {
    if let Err(err) = copy_dir_recursive(src, dest) {
        // Discard whatever partial tree was written so it can't be consumed.
        let _ = fs::remove_dir_all(dest);
        return Err(err);
    }
    Ok(())
}

/// Clean one path component destined for a backup directory name. Everything
/// outside `[A-Za-z0-9-_]` becomes `-`, which also neutralises `/`, `\`, and
/// `.` — so no `..` component and no separator can smuggle the backup outside
/// `backups/<skill>/`. Empty input degrades to a stable placeholder.
fn sanitize_backup_component(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "unknown".to_string()
    } else {
        cleaned
    }
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

/// What the caller should do after attempting a pending central-repo move.
enum MigrationOutcome {
    /// No move was pending, or it completed. Run against the configured base.
    Proceed,
    /// The move could not complete safely; the intact library still lives at
    /// this path. Run this session against it and retry on the next launch.
    UseSource(PathBuf),
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
        return MigrationOutcome::UseSource(source);
    }

    // Only ever move into an absent/empty target — never blind-merge. A
    // non-empty target is either a real library we must not overwrite or debris
    // from a failed attempt we cannot tell apart; keeping the user on their
    // intact source is lossless, overwriting is not. A fresh target also means
    // the recursive copy only ever creates new files, so it can never hit the
    // read-only git pack files that overwriting bricked startup on (#252).
    let target_empty = match directory_has_entries(current_base) {
        Ok(has_entries) => !has_entries,
        Err(err) => {
            record_startup_error(format!(
                "central repo: cannot inspect migration target {} ({err}); keeping data at source {}",
                current_base.display(),
                source.display()
            ));
            push_startup_warning("migration_incomplete");
            return MigrationOutcome::UseSource(source);
        }
    };
    if !target_empty {
        record_startup_error(format!(
            "central repo: migration target {} is not empty; keeping data at source {}",
            current_base.display(),
            source.display()
        ));
        push_startup_warning("migration_incomplete");
        return MigrationOutcome::UseSource(source);
    }

    if let Some(parent) = current_base.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            record_startup_error(format!(
                "central repo: cannot create migration target parent {} ({err}); keeping data at source {}",
                parent.display(),
                source.display()
            ));
            push_startup_warning("migration_incomplete");
            return MigrationOutcome::UseSource(source);
        }
    }

    // Same volume: an atomic rename moves the whole tree cheaply. Cross volume
    // (or a rename the OS refuses): copy into the empty target. Because the
    // target is empty, no existing file is ever overwritten.
    if fs::rename(&source, current_base).is_err() {
        if let Err(err) = copy_dir_recursive(&source, current_base) {
            record_startup_error(format!(
                "central repo: migration copy from {} to {} failed ({err:#}); keeping data at source",
                source.display(),
                current_base.display()
            ));
            push_startup_warning("migration_incomplete");
            return MigrationOutcome::UseSource(source);
        }
    }

    config.pending_migration_from = None;
    MigrationOutcome::Proceed
}

pub fn ensure_central_repo() -> Result<()> {
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
    if !base_dir_override_active() {
        let pending_before = config.pending_migration_from.clone();
        let current_base = base_dir();
        let outcome = migrate_repo_if_needed(&mut config, &current_base);
        if config.pending_migration_from != pending_before {
            if let Err(err) = save_config(&config) {
                record_startup_error(format!(
                    "central repo: failed to persist migration state ({err}); it may retry next launch"
                ));
            }
        }
        if let MigrationOutcome::UseSource(source) = outcome {
            // Run this whole session against the intact source library.
            // `base_dir()` (and every dir derived from it) now resolves there,
            // so the code below and the rest of startup stay consistent.
            set_runtime_base_dir_override(Some(source));
        }
    }
    // Re-resolve: a fallback override above may have changed the base.
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

    let dirs = [skills_dir(), scenarios_dir(), cache_dir(), logs_dir()];
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

        match outcome {
            MigrationOutcome::UseSource(p) => {
                assert_eq!(p, normalize_path(&src.path().to_string_lossy()).unwrap());
            }
            _ => panic!("expected UseSource for a non-empty target"),
        }
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

    // ── backup_directory (F2) ──

    /// Build a small source tree with a nested file and return its root.
    fn make_src_tree(root: &Path) {
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(root.join("top.txt"), b"top").unwrap();
        fs::write(root.join("nested/inner.txt"), b"inner").unwrap();
    }

    #[test]
    fn backup_move_creates_backup_and_removes_source() {
        let central = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let src = work.path().join("my-skill");
        make_src_tree(&src);

        let dest = backup_directory(
            central.path(),
            &src,
            "my-skill",
            "overwrite",
            BackupKind::Move,
        )
        .unwrap();

        // Source is gone (moved), backup holds the full tree.
        assert!(!src.exists(), "move must remove the source");
        assert_eq!(fs::read(dest.join("top.txt")).unwrap(), b"top");
        assert_eq!(fs::read(dest.join("nested/inner.txt")).unwrap(), b"inner");
        // Lands under backups/<skill>/<reason>/ — the reason alone names it.
        assert!(dest.starts_with(central.path().join("backups").join("my-skill")));
        assert_eq!(dest.file_name().and_then(|n| n.to_str()), Some("overwrite"));
        // No staging debris left behind on the happy path.
        assert!(!dest.with_file_name("overwrite.incoming").exists());
    }

    #[test]
    fn backup_copy_creates_backup_and_keeps_source() {
        let central = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let src = work.path().join("my-skill");
        make_src_tree(&src);

        let dest = backup_directory(
            central.path(),
            &src,
            "my-skill",
            "snapshot",
            BackupKind::Copy,
        )
        .unwrap();

        // Source still intact, backup is a duplicate.
        assert_eq!(fs::read(src.join("top.txt")).unwrap(), b"top");
        assert_eq!(fs::read(dest.join("top.txt")).unwrap(), b"top");
        assert_eq!(fs::read(dest.join("nested/inner.txt")).unwrap(), b"inner");
    }

    #[test]
    fn backup_same_reason_overwrites_previous_and_does_not_accumulate() {
        // One snapshot per (skill, reason): a second backup for the same reason
        // replaces the first at the *same* path rather than piling up a new
        // directory, so repeated overwrites can't grow backups/ without bound.
        let central = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let skill_backups = central.path().join("backups").join("skill");

        let src1 = work.path().join("skill-a");
        fs::create_dir_all(&src1).unwrap();
        fs::write(src1.join("v1.txt"), b"first").unwrap();
        let first = backup_directory(
            central.path(),
            &src1,
            "skill",
            "overwrite",
            BackupKind::Move,
        )
        .unwrap();

        let src2 = work.path().join("skill-b");
        fs::create_dir_all(&src2).unwrap();
        fs::write(src2.join("v2.txt"), b"second").unwrap();
        let second = backup_directory(
            central.path(),
            &src2,
            "skill",
            "overwrite",
            BackupKind::Move,
        )
        .unwrap();

        // Same destination both times.
        assert_eq!(first, second, "same reason must reuse the same path");
        // Only the latest snapshot survives; the previous file is gone.
        assert!(!second.join("v1.txt").exists(), "old snapshot must be replaced");
        assert_eq!(fs::read(second.join("v2.txt")).unwrap(), b"second");
        // backups/skill/ holds exactly one entry — no accumulation, no staging debris.
        let entries: Vec<_> = fs::read_dir(&skill_backups)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(entries, vec!["overwrite".to_string()], "got {entries:?}");
    }

    #[test]
    fn backup_sanitizes_illegal_reason_and_cannot_escape() {
        let central = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let src = work.path().join("s");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("f.txt"), b"x").unwrap();

        let dest = backup_directory(
            central.path(),
            &src,
            "skill",
            "../../etc/passwd",
            BackupKind::Copy,
        )
        .unwrap();

        // Stays inside the skill's backup area — no traversal escaped.
        let skill_backups = central.path().join("backups").join("skill");
        assert!(dest.starts_with(&skill_backups), "{}", dest.display());
        // Strong guard: walk EVERY component of the full path and assert none is
        // a `..` parent-dir segment. A lexical `starts_with` alone can be fooled
        // by an embedded `..`; component inspection cannot.
        assert!(
            !has_parent_dir_component(&dest),
            "no path component may be `..`: {}",
            dest.display()
        );
        // The reason component itself carries no separators or `..`.
        let name = dest.file_name().unwrap().to_str().unwrap();
        assert!(!name.contains('/') && !name.contains('\\') && !name.contains(".."));
        // "../../etc/passwd" → each of the 6 leading `.`/`/` chars becomes a dash.
        assert_eq!(name, "------etc-passwd");
    }

    /// True if any component of `path` is a `..` parent-dir segment.
    fn has_parent_dir_component(path: &Path) -> bool {
        path.components()
            .any(|c| matches!(c, Component::ParentDir))
    }

    #[test]
    fn move_via_copy_delete_moves_tree_and_removes_source() {
        // Directly exercises the cross-volume fallback path (rename → EXDEV):
        // copy the tree, then delete the source, preserving move semantics.
        let work = tempfile::tempdir().unwrap();
        let src = work.path().join("src");
        make_src_tree(&src);
        let dest = work.path().join("dest");

        move_via_copy_delete(&src, &dest).unwrap();

        assert!(!src.exists(), "source removed after copy");
        assert_eq!(fs::read(dest.join("top.txt")).unwrap(), b"top");
        assert_eq!(fs::read(dest.join("nested/inner.txt")).unwrap(), b"inner");
    }

    #[test]
    fn copy_failure_rolls_back_partial_dest_and_keeps_source() {
        // Deterministically force a mid-copy failure: pre-create `dest/nested`
        // as a FILE so copying src's `nested/` DIRECTORY into it fails. The
        // rollback must then wipe the whole half-written `dest`, and — because
        // this is the Move fallback — the source stays fully intact (no data
        // lost on a failed backup). This is the same code path the untestable
        // cross-volume Copy rollback takes.
        let work = tempfile::tempdir().unwrap();
        let src = work.path().join("src");
        make_src_tree(&src); // src/top.txt + src/nested/inner.txt
        let dest = work.path().join("dest");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("nested"), b"i am a file, not a dir").unwrap();

        let err = move_via_copy_delete(&src, &dest);
        assert!(err.is_err(), "copy into a file-blocked subdir must fail");

        // Half-written backup fully rolled back — nothing to mis-consume.
        assert!(!dest.exists(), "partial dest must be removed on failure");
        // Source untouched: move never deletes before a successful copy.
        assert_eq!(fs::read(src.join("top.txt")).unwrap(), b"top");
        assert_eq!(fs::read(src.join("nested/inner.txt")).unwrap(), b"inner");
    }

    #[test]
    fn backup_sanitizes_illegal_skill_name_and_cannot_escape() {
        // Traversal defense is independent of `reason`: a hostile
        // `skill_dir_name` must also stay under backups/ with no `..` component.
        let central = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let src = work.path().join("s");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("f.txt"), b"x").unwrap();

        let dest = backup_directory(
            central.path(),
            &src,
            "../../evil",
            "overwrite",
            BackupKind::Copy,
        )
        .unwrap();

        assert!(dest.starts_with(central.path().join("backups")));
        assert!(
            !has_parent_dir_component(&dest),
            "no `..` component: {}",
            dest.display()
        );
    }

    #[test]
    fn backup_empty_reason_uses_unknown_placeholder() {
        let central = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let src = work.path().join("s");
        fs::create_dir_all(&src).unwrap();

        let dest = backup_directory(
            central.path(),
            &src,
            "skill",
            "",
            BackupKind::Copy,
        )
        .unwrap();

        assert_eq!(dest.file_name().and_then(|n| n.to_str()), Some("unknown"));
    }

    #[test]
    fn backup_missing_source_returns_err_not_panic() {
        let central = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let missing = work.path().join("does-not-exist");

        let result = backup_directory(
            central.path(),
            &missing,
            "skill",
            "overwrite",
            BackupKind::Copy,
        );
        assert!(result.is_err(), "missing source must be a graceful Err");
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
