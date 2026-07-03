use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const IGNORED: &[&str] = &[".git", ".DS_Store", "Thumbs.db", ".gitignore", "__pycache__"];

/// True for names excluded from a skill's content scope: the exact-match
/// [`IGNORED`] entries plus compiled-Python artifacts (`*.pyc`). These are
/// regenerated whenever a skill's Python scripts run, so without excluding
/// them a copy-mode deployment would read as permanently "changed" against
/// the library the first time the skill is used.
fn is_ignored(name: &str) -> bool {
    IGNORED.contains(&name) || name.ends_with(".pyc")
}

/// One file in a skill's canonical "content scope" — the set of files that
/// both [`hash_directory`] and the source-diff command operate on. Sharing
/// this enumeration keeps the update badge and the diff from ever
/// disagreeing about which files count.
pub struct ContentEntry {
    /// Path relative to the scanned directory, in the same lossy form the
    /// hash consumes (keeps the hashed byte stream stable).
    pub relative_path: String,
    pub path: PathBuf,
    /// `mode & 0o111` on unix when metadata is readable, else `None`.
    /// Always `None` on non-unix. `None` means "not folded into the hash".
    pub exec_bits: Option<u32>,
    /// Modification time in ms since the Unix epoch, captured during the walk
    /// so callers don't need a second `metadata()` syscall (or a separate
    /// recursive walk) just to learn when the content last changed.
    pub modified_ms: Option<i64>,
}

impl ContentEntry {
    pub fn is_executable(&self) -> bool {
        self.exec_bits.map_or(false, |bits| bits != 0)
    }
}

#[cfg(unix)]
fn exec_bits_of(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    path.metadata().ok().map(|m| m.permissions().mode() & 0o111)
}

#[cfg(not(unix))]
fn exec_bits_of(_path: &Path) -> Option<u32> {
    None
}

/// Enumerate the files that make up a skill's content, sorted by path and
/// filtered by the shared ignore-list. Single source of truth for "what is
/// skill content"; hashing and diffing both build on it.
pub fn list_content_files(dir: &Path) -> Vec<ContentEntry> {
    let mut entries: Vec<_> = WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !is_ignored(&name)
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .collect();

    entries.sort_by(|a, b| a.path().cmp(b.path()));

    entries
        .into_iter()
        .map(|entry| {
            let relative_path = entry
                .path()
                .strip_prefix(dir)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .into_owned();
            // Normalize Windows separators to `/` so the hashed byte
            // stream is identical across platforms — otherwise Windows
            // feeds `sub\c.md` into the hash and disagrees with every
            // other OS about the same content. Windows-only because `\`
            // is a legal filename character on unix.
            #[cfg(windows)]
            let relative_path = relative_path.replace('\\', "/");
            let exec_bits = exec_bits_of(entry.path());
            // Reuse WalkDir's already-fetched metadata for the mtime: no extra
            // stat, and no separate recursive walk to answer "last modified?".
            let modified_ms = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64);
            ContentEntry {
                relative_path,
                path: entry.into_path(),
                exec_bits,
                modified_ms,
            }
        })
        .collect()
}

/// Hash a prepared content-file list. Split out from [`hash_directory`] so a
/// caller that already walked the tree (via [`list_content_files`]) can hash
/// and inspect the same entries without walking again (#248).
pub fn hash_entries(entries: &[ContentEntry]) -> String {
    let mut hasher = Sha256::new();
    for entry in entries {
        hasher.update(entry.relative_path.as_bytes());
        if let Ok(content) = std::fs::read(&entry.path) {
            hasher.update(&content);
        }
        // Include executable bit so permission-only changes are detected.
        #[cfg(unix)]
        if let Some(bits) = entry.exec_bits {
            hasher.update(&bits.to_le_bytes());
        }
    }
    hex::encode(hasher.finalize())
}

/// Latest content-file modification time (ms since epoch) from a prepared
/// entry list. Scoped to the same files the hash covers: dirs, `.git`,
/// `.DS_Store`, and `*.pyc` are excluded, so it reflects real content change.
pub fn latest_modified_ms(entries: &[ContentEntry]) -> Option<i64> {
    entries.iter().filter_map(|e| e.modified_ms).max()
}

pub fn hash_directory(dir: &Path) -> Result<String> {
    Ok(hash_entries(&list_content_files(dir)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Project-workspace skills may now be symlinks to the central library
    /// (#225). Sync-status classification hashes the project path directly,
    /// so hashing through a symlinked root must see the real content.
    #[cfg(unix)]
    #[test]
    fn hash_through_symlinked_root_matches_real_directory() {
        let tmp = tempdir().unwrap();
        let real = tmp.path().join("skill");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("SKILL.md"), "# hello").unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        assert_eq!(
            hash_directory(&link).unwrap(),
            hash_directory(&real).unwrap()
        );
    }

    #[test]
    fn hash_deterministic_same_content() {
        let tmp1 = tempdir().unwrap();
        fs::write(tmp1.path().join("a.txt"), "hello").unwrap();
        fs::write(tmp1.path().join("b.txt"), "world").unwrap();

        let tmp2 = tempdir().unwrap();
        fs::write(tmp2.path().join("a.txt"), "hello").unwrap();
        fs::write(tmp2.path().join("b.txt"), "world").unwrap();

        let h1 = hash_directory(tmp1.path()).unwrap();
        let h2 = hash_directory(tmp2.path()).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_differs_with_different_content() {
        let tmp1 = tempdir().unwrap();
        fs::write(tmp1.path().join("a.txt"), "hello").unwrap();

        let tmp2 = tempdir().unwrap();
        fs::write(tmp2.path().join("a.txt"), "world").unwrap();

        let h1 = hash_directory(tmp1.path()).unwrap();
        let h2 = hash_directory(tmp2.path()).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_ignores_dot_git() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), "content").unwrap();
        let h1 = hash_directory(tmp.path()).unwrap();

        // Add .git directory — hash should not change
        fs::create_dir_all(tmp.path().join(".git")).unwrap();
        fs::write(tmp.path().join(".git/config"), "git stuff").unwrap();
        let h2 = hash_directory(tmp.path()).unwrap();

        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_ignores_ds_store() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), "content").unwrap();
        let h1 = hash_directory(tmp.path()).unwrap();

        fs::write(tmp.path().join(".DS_Store"), "binary stuff").unwrap();
        let h2 = hash_directory(tmp.path()).unwrap();

        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_ignores_pycache() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("run.py"), "print('hi')").unwrap();
        let h1 = hash_directory(tmp.path()).unwrap();

        // Running the script generates a __pycache__ dir — hash must not change.
        fs::create_dir_all(tmp.path().join("__pycache__")).unwrap();
        fs::write(
            tmp.path().join("__pycache__/run.cpython-311.pyc"),
            "bytecode",
        )
        .unwrap();
        let h2 = hash_directory(tmp.path()).unwrap();

        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_ignores_loose_pyc() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("a.py"), "x = 1").unwrap();
        let h1 = hash_directory(tmp.path()).unwrap();

        // A .pyc sitting next to its source (not under __pycache__) is excluded too.
        fs::write(tmp.path().join("a.pyc"), "bytecode").unwrap();
        let h2 = hash_directory(tmp.path()).unwrap();

        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_empty_directory() {
        let tmp = tempdir().unwrap();
        let h = hash_directory(tmp.path()).unwrap();
        // SHA256 of empty input
        assert_eq!(
            h,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn hash_includes_subdirectories() {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("sub")).unwrap();
        fs::write(tmp.path().join("sub/file.md"), "nested").unwrap();

        let h1 = hash_directory(tmp.path()).unwrap();

        // Different subdir name → different hash
        let tmp2 = tempdir().unwrap();
        fs::create_dir_all(tmp2.path().join("other")).unwrap();
        fs::write(tmp2.path().join("other/file.md"), "nested").unwrap();

        let h2 = hash_directory(tmp2.path()).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn list_content_files_sorted_with_relative_paths_and_ignores() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("b.txt"), "b").unwrap();
        fs::write(tmp.path().join("a.txt"), "a").unwrap();
        fs::create_dir_all(tmp.path().join("sub")).unwrap();
        fs::write(tmp.path().join("sub/c.md"), "c").unwrap();
        fs::write(tmp.path().join(".DS_Store"), "junk").unwrap();

        let entries = list_content_files(tmp.path());
        let rels: Vec<_> = entries.iter().map(|e| e.relative_path.clone()).collect();
        // Sorted by path, ignore-listed files excluded, subdirs included.
        assert_eq!(rels, vec!["a.txt", "b.txt", "sub/c.md"]);
    }

    #[test]
    fn latest_modified_ms_reflects_content_files_and_ignores_empty() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), "a").unwrap();
        fs::create_dir_all(tmp.path().join("sub")).unwrap();
        fs::write(tmp.path().join("sub/b.md"), "b").unwrap();
        fs::write(tmp.path().join(".DS_Store"), "junk").unwrap();

        let entries = list_content_files(tmp.path());
        // Matches the max mtime over exactly the enumerated (non-ignored)
        // content files, computed from the same single walk with no extra stat.
        assert_eq!(
            latest_modified_ms(&entries),
            entries.iter().filter_map(|e| e.modified_ms).max()
        );
        assert!(latest_modified_ms(&entries).is_some());

        // No content files → no timestamp.
        let empty = tempdir().unwrap();
        assert_eq!(latest_modified_ms(&list_content_files(empty.path())), None);
    }

    #[test]
    fn hash_entries_matches_hash_directory() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), "hello").unwrap();
        fs::create_dir_all(tmp.path().join("sub")).unwrap();
        fs::write(tmp.path().join("sub/c.md"), "nested").unwrap();

        // The split-out entry hasher must agree with the whole-directory hash.
        assert_eq!(
            hash_entries(&list_content_files(tmp.path())),
            hash_directory(tmp.path()).unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn list_content_files_reports_executable_bit() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempdir().unwrap();
        let script = tmp.path().join("run.sh");
        fs::write(&script, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(tmp.path().join("plain.txt"), "x").unwrap();

        let entries = list_content_files(tmp.path());
        let by_name = |name: &str| entries.iter().find(|e| e.relative_path == name).unwrap();
        assert!(by_name("run.sh").is_executable());
        assert!(!by_name("plain.txt").is_executable());
    }
}
