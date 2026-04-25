use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

pub struct RepoLock {
    file: File,
}

impl RepoLock {
    pub fn acquire(repo_root: &Path, operation: &str) -> Result<Self> {
        std::fs::create_dir_all(repo_root)?;
        let lock_path = repo_root.join(".skills-manager.lock");
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&lock_path)
            .with_context(|| format!("failed to open repo lock {}", lock_path.display()))?;

        file.try_lock_exclusive()
            .with_context(|| format!("skills repository is busy: {operation}"))?;

        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        writeln!(
            file,
            "pid={}\nhostname={}\noperation={}\nstart_time={}",
            std::process::id(),
            std::env::var("HOSTNAME")
                .or_else(|_| std::env::var("COMPUTERNAME"))
                .unwrap_or_else(|_| "unknown".to_string()),
            operation,
            chrono::Utc::now().to_rfc3339()
        )?;
        file.sync_all()?;

        Ok(Self { file })
    }
}

impl Drop for RepoLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
