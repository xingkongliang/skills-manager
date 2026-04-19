use anyhow::Result;
use std::path::PathBuf;

pub fn base_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Cannot determine home directory")
        .join(".skills-manager")
}

pub fn skills_dir() -> PathBuf {
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

pub fn ensure_central_repo() -> Result<()> {
    let dirs = [skills_dir(), scenarios_dir(), cache_dir(), logs_dir()];
    for d in &dirs {
        std::fs::create_dir_all(d)?;
    }

    // Migrate from old path if it exists
    let old_path = dirs::home_dir().unwrap().join(".agent-skills");
    if old_path.exists() && !base_dir().join("skills").exists() {
        log::info!("Migrating from old path {:?}", old_path);
        if let Ok(entries) = std::fs::read_dir(&old_path) {
            for entry in entries.flatten() {
                let dest = base_dir().join(entry.file_name());
                if !dest.exists() {
                    let _ = std::fs::rename(entry.path(), &dest);
                }
            }
        }
    }

    // Install/refresh built-in skills (e.g. pack-router-gen) into the vault.
    // Idempotent: safe to run on every startup.
    if let Err(e) = crate::builtin_skills::install_builtin_skills(&skills_dir()) {
        log::warn!("Failed to install builtin skills: {}", e);
    }

    Ok(())
}
