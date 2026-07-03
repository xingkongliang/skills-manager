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

fn migrate_repo_if_needed(config: &mut RepoPathConfig, current_base: &Path) -> Result<()> {
    let Some(source_raw) = config.pending_migration_from.clone() else {
        return Ok(());
    };
    let source = normalize_path(&source_raw)?;
    if source == current_base || !source.exists() {
        config.pending_migration_from = None;
        save_config(config)?;
        return Ok(());
    }
    if current_base.starts_with(&source) {
        return Err(anyhow!(
            "Central repository path cannot be inside the current repository"
        ));
    }

    let target_has_entries = directory_has_entries(current_base)?;
    if let Some(parent) = current_base.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(&source, current_base) {
        Ok(_) => {}
        Err(_) => {
            if target_has_entries {
                log::info!(
                    "Central repository target {} already exists; merging data from {}",
                    current_base.display(),
                    source.display()
                );
            }
            fs::create_dir_all(current_base)?;
            copy_dir_recursive(&source, current_base)?;
        }
    }

    config.pending_migration_from = None;
    save_config(config)?;
    Ok(())
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

    let current_base = base_dir();
    migrate_repo_if_needed(&mut config, &current_base)?;

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
