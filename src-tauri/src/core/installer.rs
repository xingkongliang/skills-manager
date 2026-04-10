use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use super::central_repo;
use super::content_hash;
use super::skill_metadata::{self, sanitize_skill_name};

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
        safe_extract(&mut archive, temp_dir.path())?;

        // Find supported skill markers for local/archive import flows.
        let mut found = Vec::new();
        for entry in WalkDir::new(temp_dir.path()).max_depth(4) {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy();
            if name == "SKILL.md" || name == "skill.md" {
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
    let prepared = PreparedSource::open(source)?;
    let skill_dir = prepared.skill_dir();

    let sanitized_name = match name {
        Some(n) if !n.is_empty() => {
            sanitize_skill_name(n).ok_or_else(|| anyhow::anyhow!("Invalid skill name: '{}'", n))?
        }
        _ => skill_metadata::infer_skill_name(skill_dir),
    };

    let source_meta_name = skill_metadata::parse_skill_md(skill_dir)
        .name
        .unwrap_or_else(|| sanitized_name.clone());

    let skills_dir = central_repo::skills_dir();
    let dest = unique_skill_dest(&skills_dir, &sanitized_name, &source_meta_name);
    let final_name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| sanitized_name.clone());

    install_skill_dir_to_destination(skill_dir, &final_name, &dest)
}

pub fn install_from_local_to_destination(
    source: &Path,
    name: Option<&str>,
    destination: &Path,
) -> Result<InstallResult> {
    let prepared = PreparedSource::open(source)?;
    let skill_dir = prepared.skill_dir();

    let skill_name = match name {
        Some(n) if !n.is_empty() => {
            sanitize_skill_name(n).ok_or_else(|| anyhow::anyhow!("Invalid skill name: '{}'", n))?
        }
        _ => skill_metadata::infer_skill_name(skill_dir),
    };
    install_skill_dir_to_destination(skill_dir, &skill_name, destination)
}

pub fn resolve_local_skill_name(source: &Path, name: Option<&str>) -> Result<String> {
    let prepared = PreparedSource::open(source)?;
    let skill_dir = prepared.skill_dir();

    Ok(match name {
        Some(n) if !n.is_empty() => {
            sanitize_skill_name(n).ok_or_else(|| anyhow::anyhow!("Invalid skill name: '{}'", n))?
        }
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

/// Extract a ZIP archive into `dest`, skipping any entry whose path would
/// escape the destination directory (Zip Slip defence).
fn safe_extract(archive: &mut zip::ZipArchive<std::fs::File>, dest: &Path) -> Result<()> {
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;

        // enclosed_name() returns None for absolute paths and entries that
        // contain `..` components, so those are silently skipped.
        let entry_path = match entry.enclosed_name() {
            Some(name) => dest.join(name),
            None => continue,
        };

        // Belt-and-suspenders: verify the resolved path stays inside dest.
        if !entry_path.starts_with(dest) {
            continue;
        }

        if entry.is_dir() {
            std::fs::create_dir_all(&entry_path)?;
        } else {
            if let Some(parent) = entry_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut outfile = std::fs::File::create(&entry_path)?;
            std::io::copy(&mut entry, &mut outfile)?;

            // Restore Unix file permissions (especially executable bits)
            // from the ZIP entry metadata.
            #[cfg(unix)]
            {
                if let Some(mode) = entry.unix_mode() {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        &entry_path,
                        std::fs::Permissions::from_mode(mode),
                    );
                }
            }
        }
    }
    Ok(())
}

/// Return a collision-safe destination directory for an install.
///
/// Rules:
/// - Prefer `<name>` if missing.
/// - Reuse an existing directory when it clearly belongs to the same skill
///   (same metadata `name`, or legacy no-metadata `<name>` directory).
/// - Otherwise allocate `<name>-2`, `<name>-3`, ...
fn unique_skill_dest(parent: &Path, sanitized_name: &str, source_meta_name: &str) -> PathBuf {
    for i in 1u32.. {
        let candidate = if i == 1 {
            parent.join(sanitized_name)
        } else {
            parent.join(format!("{}-{}", sanitized_name, i))
        };

        if !candidate.exists() {
            return candidate;
        }

        let existing_meta_name = skill_metadata::parse_skill_md(&candidate).name.or_else(|| {
            if i == 1 {
                Some(sanitized_name.to_string())
            } else {
                None
            }
        });

        if existing_meta_name.as_deref() == Some(source_meta_name) {
            return candidate;
        }
    }

    parent.join(sanitized_name)
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_skill_dir(parent: &Path, dir_name: &str, meta_name: Option<&str>) -> PathBuf {
        let dir = parent.join(dir_name);
        std::fs::create_dir_all(&dir).unwrap();
        if let Some(name) = meta_name {
            std::fs::write(dir.join("SKILL.md"), format!("---\nname: {}\n---\n", name)).unwrap();
        }
        dir
    }

    #[test]
    fn unique_dest_returns_base_when_free() {
        let tmp = tempdir().unwrap();
        let dest = unique_skill_dest(tmp.path(), "a-b", "a-b");
        assert_eq!(dest, tmp.path().join("a-b"));
    }

    #[test]
    fn unique_dest_reuses_base_for_same_meta_name() {
        let tmp = tempdir().unwrap();
        make_skill_dir(tmp.path(), "a-b", Some("A B"));

        let dest = unique_skill_dest(tmp.path(), "a-b", "A B");
        assert_eq!(dest, tmp.path().join("a-b"));
    }

    #[test]
    fn unique_dest_uses_suffix_for_different_meta_name() {
        let tmp = tempdir().unwrap();
        make_skill_dir(tmp.path(), "a-b", Some("A-B"));

        let dest = unique_skill_dest(tmp.path(), "a-b", "A:B");
        assert_eq!(dest, tmp.path().join("a-b-2"));
    }

    #[test]
    fn unique_dest_reuses_existing_suffix_for_reinstall() {
        let tmp = tempdir().unwrap();
        make_skill_dir(tmp.path(), "a-b", Some("A-B"));
        make_skill_dir(tmp.path(), "a-b-2", Some("A:B"));

        let dest = unique_skill_dest(tmp.path(), "a-b", "A:B");
        assert_eq!(dest, tmp.path().join("a-b-2"));
    }

    #[test]
    fn unique_dest_legacy_no_metadata_base_can_reinstall() {
        let tmp = tempdir().unwrap();
        make_skill_dir(tmp.path(), "legacy", None);

        let dest = unique_skill_dest(tmp.path(), "legacy", "legacy");
        assert_eq!(dest, tmp.path().join("legacy"));
    }
}
