use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use walkdir::WalkDir;

const CONFIG_FILE_NAME: &str = "repo-config.json";

static BASE_DIR_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static SKILLS_DIR_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

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

fn load_config() -> RepoPathConfig {
    let path = config_file_path();
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return RepoPathConfig::default(),
    };

    serde_json::from_str(&raw).unwrap_or_default()
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

    match path {
        Some(raw) => {
            let next = normalize_path(&raw)?;
            config.repo_path = Some(next.to_string_lossy().to_string());
            config.pending_migration_from = if next != current {
                Some(current.to_string_lossy().to_string())
            } else {
                None
            };
            save_config(&config)?;
            Ok(next)
        }
        None => {
            let next = default_base_dir();
            config.repo_path = None;
            config.pending_migration_from = if next != current {
                Some(current.to_string_lossy().to_string())
            } else {
                None
            };
            save_config(&config)?;
            Ok(next)
        }
    }
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
    let mut config = load_config();
    let current_base = base_dir();
    migrate_repo_if_needed(&mut config, &current_base)?;

    let dirs = [skills_dir(), scenarios_dir(), cache_dir(), logs_dir()];
    for d in &dirs {
        fs::create_dir_all(d)?;
    }

    // Migrate from old path if it exists
    let old_path = dirs::home_dir().unwrap().join(".agent-skills");
    if old_path.exists() && !current_base.join("skills").exists() {
        log::info!("Migrating from old path {:?}", old_path);
        if let Ok(entries) = fs::read_dir(&old_path) {
            for entry in entries.flatten() {
                let dest = current_base.join(entry.file_name());
                if !dest.exists() {
                    let _ = fs::rename(entry.path(), &dest);
                }
            }
        }
    }

    Ok(())
}
