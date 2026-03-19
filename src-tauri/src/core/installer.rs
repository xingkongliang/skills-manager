use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use super::central_repo;
use super::content_hash;
use super::skill_metadata;

pub struct InstallResult {
    pub name: String,
    pub description: Option<String>,
    pub central_path: PathBuf,
    pub content_hash: String,
}

enum PreparedSource {
    Directory(PathBuf),
    Archive {
        _temp_dir: tempfile::TempDir,
        skill_dir: PathBuf,
    },
}

impl PreparedSource {
    fn open(source: &Path) -> Result<Self> {
        if source.is_dir() {
            Ok(PreparedSource::Directory(source.to_path_buf()))
        } else {
            Self::from_archive(source)
        }
    }

    fn from_archive(source: &Path) -> Result<Self> {
        let ext = source
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();
        if ext != "zip" && ext != "skill" {
            bail!("Unsupported archive format: {}", ext);
        }

        let temp_dir = tempfile::tempdir()?;
        let file = std::fs::File::open(source)?;
        let mut archive = zip::ZipArchive::new(file)?;
        archive.extract(temp_dir.path())?;

        // Find SKILL.md
        let mut found = Vec::new();
        for entry in WalkDir::new(temp_dir.path()).max_depth(4) {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy();
            if name == "SKILL.md" || name == "skill.md" || name == "CLAUDE.md" {
                if let Some(parent) = entry.path().parent() {
                    found.push(parent.to_path_buf());
                }
            }
        }

        found.dedup();

        let skill_dir = match found.len() {
            0 => temp_dir.path().to_path_buf(),
            1 => found.into_iter().next().unwrap(),
            _ => bail!("Multiple skill directories found in archive"),
        };

        Ok(PreparedSource::Archive {
            _temp_dir: temp_dir,
            skill_dir,
        })
    }

    fn skill_dir(&self) -> &Path {
        match self {
            PreparedSource::Directory(p) => p,
            PreparedSource::Archive { skill_dir, .. } => skill_dir,
        }
    }
}

pub fn install_from_local(source: &Path, name: Option<&str>) -> Result<InstallResult> {
    let skill_name = resolve_local_skill_name(source, name)?;
    let dest = central_repo::skills_dir().join(&skill_name);
    install_from_local_to_destination(source, Some(&skill_name), &dest)
}

pub fn install_from_local_to_destination(
    source: &Path,
    name: Option<&str>,
    destination: &Path,
) -> Result<InstallResult> {
    let prepared = PreparedSource::open(source)?;
    let skill_dir = prepared.skill_dir();

    let skill_name = match name {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => skill_metadata::infer_skill_name(skill_dir),
    };
    install_skill_dir_to_destination(skill_dir, &skill_name, destination)
}

pub fn resolve_local_skill_name(source: &Path, name: Option<&str>) -> Result<String> {
    let prepared = PreparedSource::open(source)?;
    let skill_dir = prepared.skill_dir();

    Ok(match name {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => skill_metadata::infer_skill_name(skill_dir),
    })
}

pub fn install_from_git_dir(source: &Path, name: Option<&str>) -> Result<InstallResult> {
    install_from_local(source, name)
}

pub fn install_skill_dir_to_destination(
    source: &Path,
    name: &str,
    destination: &Path,
) -> Result<InstallResult> {
    let meta = skill_metadata::parse_skill_md(source);

    if destination.exists() {
        std::fs::remove_dir_all(destination)
            .with_context(|| format!("Failed to remove existing {:?}", destination))?;
    }

    copy_skill_dir(source, destination)?;

    let hash = content_hash::hash_directory(destination)?;

    Ok(InstallResult {
        name: name.to_string(),
        description: meta.description,
        central_path: destination.to_path_buf(),
        content_hash: hash,
    })
}

fn copy_skill_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str == ".git" || name_str == ".DS_Store" {
            continue;
        }

        // Skip symlinks to prevent exfiltration of files outside the skill directory
        if ft.is_symlink() {
            continue;
        }

        let dest_path = dst.join(&name);
        if ft.is_dir() {
            copy_skill_dir(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}
