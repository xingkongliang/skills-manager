use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use super::central_repo;
use super::content_hash;
use super::skill_metadata::{self, sanitize_skill_name};
use super::sync_engine;

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

    let skills_dir = central_repo::skills_dir();
    let dest = unique_skill_dest(&skills_dir, &sanitized_name, skill_dir)?;
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

pub fn hash_local_source(source: &Path) -> Result<String> {
    let prepared = PreparedSource::open(source)?;
    content_hash::hash_directory(prepared.skill_dir())
}

/// [`hash_local_source`] with CRLF folded to LF, for the "do these differ by
/// anything other than line endings?" tiebreaker. Goes through the same
/// [`PreparedSource`] so an archive source is answered the same way a
/// directory source is.
pub fn hash_local_source_eol_insensitive(source: &Path) -> Result<String> {
    let prepared = PreparedSource::open(source)?;
    content_hash::hash_directory_eol_insensitive(prepared.skill_dir())
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

    // A local skill can be a symlink back to this exact central directory
    // (for example, after restoring the database). Treat that as an already
    // installed directory: removing `destination` here would delete the data
    // that `source` resolves to before the copy starts.
    if source != destination {
        let source_canonical = source.canonicalize().ok();
        let destination_canonical = destination.canonicalize().ok();
        if source_canonical.is_some() && source_canonical == destination_canonical {
            let hash = content_hash::hash_directory(destination)?;
            return Ok(InstallResult {
                name: name.to_string(),
                description: meta.description,
                central_path: destination.to_path_buf(),
                content_hash: hash,
            });
        }
    }

    sync_engine::ensure_dst_not_inside_src(source, destination)?;

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
fn unique_skill_dest(parent: &Path, sanitized_name: &str, source: &Path) -> Result<PathBuf> {
    let source_hash = content_hash::hash_directory(source)?;

    for i in 1u32.. {
        let candidate = if i == 1 {
            parent.join(sanitized_name)
        } else {
            parent.join(format!("{}-{}", sanitized_name, i))
        };

        if !candidate.exists() {
            return Ok(candidate);
        }

        if content_hash::hash_directory(&candidate).ok().as_deref() == Some(source_hash.as_str()) {
            return Ok(candidate);
        }
    }

    Ok(parent.join(sanitized_name))
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
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

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
        let source = make_skill_dir(tmp.path(), "source", Some("a-b"));
        let dest = unique_skill_dest(tmp.path(), "a-b", &source).unwrap();
        assert_eq!(dest, tmp.path().join("a-b"));
    }

    #[test]
    fn unique_dest_reuses_base_for_same_content() {
        let tmp = tempdir().unwrap();
        let existing = make_skill_dir(tmp.path(), "a-b", Some("A B"));
        let source = make_skill_dir(tmp.path(), "source", Some("A B"));
        std::fs::write(existing.join("body.md"), "same").unwrap();
        std::fs::write(source.join("body.md"), "same").unwrap();

        let dest = unique_skill_dest(tmp.path(), "a-b", &source).unwrap();
        assert_eq!(dest, tmp.path().join("a-b"));
    }

    #[test]
    fn unique_dest_uses_suffix_for_different_content_even_if_name_matches() {
        let tmp = tempdir().unwrap();
        let existing = make_skill_dir(tmp.path(), "a-b", Some("A-B"));
        let source = make_skill_dir(tmp.path(), "source", Some("A-B"));
        std::fs::write(existing.join("body.md"), "old").unwrap();
        std::fs::write(source.join("body.md"), "new").unwrap();

        let dest = unique_skill_dest(tmp.path(), "a-b", &source).unwrap();
        assert_eq!(dest, tmp.path().join("a-b-2"));
    }

    #[test]
    fn unique_dest_reuses_existing_suffix_for_same_content() {
        let tmp = tempdir().unwrap();
        let first = make_skill_dir(tmp.path(), "a-b", Some("A-B"));
        let second = make_skill_dir(tmp.path(), "a-b-2", Some("A-B"));
        let source = make_skill_dir(tmp.path(), "source", Some("A-B"));
        std::fs::write(first.join("body.md"), "first").unwrap();
        std::fs::write(second.join("body.md"), "second").unwrap();
        std::fs::write(source.join("body.md"), "second").unwrap();

        let dest = unique_skill_dest(tmp.path(), "a-b", &source).unwrap();
        assert_eq!(dest, tmp.path().join("a-b-2"));
    }

    #[test]
    fn install_skill_dir_refuses_destination_inside_source() {
        let tmp = tempdir().unwrap();
        let source = make_skill_dir(tmp.path(), "skills", Some("skills"));
        std::fs::write(source.join("body.md"), "data").unwrap();
        let destination = source.join("skills");

        let err = install_skill_dir_to_destination(&source, "skills", &destination)
            .err()
            .expect("expected refusal");
        assert!(
            err.to_string().contains("infinite recursion"),
            "unexpected error: {err}"
        );
        // The source must not be touched, and no nested copy must exist.
        assert!(source.join("body.md").exists());
        assert!(!destination.exists());
    }

    #[test]
    fn unique_dest_legacy_no_metadata_base_can_reinstall_if_content_matches() {
        let tmp = tempdir().unwrap();
        let existing = make_skill_dir(tmp.path(), "legacy", None);
        let source = make_skill_dir(tmp.path(), "source", None);
        std::fs::write(existing.join("body.md"), "same").unwrap();
        std::fs::write(source.join("body.md"), "same").unwrap();

        let dest = unique_skill_dest(tmp.path(), "legacy", &source).unwrap();
        assert_eq!(dest, tmp.path().join("legacy"));
    }

    /// An archive source is prepared by the same extraction, so the tiebreaker
    /// has to answer for it exactly as it does for a directory: a CRLF archive
    /// of the same skill is not an update. Covers the only source shape where
    /// the comparison re-extracts rather than re-walking.
    #[test]
    fn eol_insensitive_hash_sees_through_an_archive_with_crlf_content() {
        let tmp = tempdir().unwrap();
        let archive = tmp.path().join("demo.skill");
        write_skill_archive(&archive, "line one\r\nline two\r\n");

        let extracted = tmp.path().join("extracted");
        std::fs::create_dir_all(&extracted).unwrap();
        std::fs::write(extracted.join("SKILL.md"), "---\nname: Demo Skill\n---\n").unwrap();
        std::fs::write(extracted.join("body.md"), "line one\nline two\n").unwrap();

        assert_ne!(
            hash_local_source(&archive).unwrap(),
            content_hash::hash_directory(&extracted).unwrap(),
            "the byte hashes disagree, which is what sends the check to the tiebreaker"
        );
        assert_eq!(
            hash_local_source_eol_insensitive(&archive).unwrap(),
            content_hash::hash_directory_eol_insensitive(&extracted).unwrap()
        );
    }

    fn write_skill_archive(path: &Path, body: &str) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        zip.start_file("demo-skill/SKILL.md", options).unwrap();
        zip.write_all(b"---\nname: Demo Skill\n---\n").unwrap();
        zip.start_file("demo-skill/body.md", options).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn hash_local_source_matches_extracted_archive_representation() {
        let tmp = tempdir().unwrap();
        let archive = tmp.path().join("demo.skill");
        write_skill_archive(&archive, "same content");

        let extracted = tmp.path().join("extracted");
        std::fs::create_dir_all(&extracted).unwrap();
        std::fs::write(extracted.join("SKILL.md"), "---\nname: Demo Skill\n---\n").unwrap();
        std::fs::write(extracted.join("body.md"), "same content").unwrap();

        let archive_hash = hash_local_source(&archive).unwrap();
        let dir_hash = content_hash::hash_directory(&extracted).unwrap();

        assert_eq!(archive_hash, dir_hash);
    }

    #[test]
    fn hash_local_source_detects_archive_content_changes() {
        let tmp = tempdir().unwrap();
        let archive = tmp.path().join("demo.zip");
        write_skill_archive(&archive, "v1");
        let first_hash = hash_local_source(&archive).unwrap();

        write_skill_archive(&archive, "v2");
        let second_hash = hash_local_source(&archive).unwrap();

        assert_ne!(first_hash, second_hash);
    }
}
