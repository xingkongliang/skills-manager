use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::Path;
use walkdir::WalkDir;

const IGNORED: &[&str] = &[".git", ".DS_Store", "Thumbs.db", ".gitignore"];

pub fn hash_directory(dir: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut entries: Vec<_> = WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !IGNORED.contains(&name.as_ref())
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .collect();

    entries.sort_by(|a, b| a.path().cmp(b.path()));

    for entry in entries {
        let rel = entry
            .path()
            .strip_prefix(dir)
            .unwrap_or(entry.path())
            .to_string_lossy();
        hasher.update(rel.as_bytes());
        if let Ok(content) = std::fs::read(entry.path()) {
            hasher.update(&content);
        }
        // Include executable bit so permission-only changes are detected.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = entry.path().metadata() {
                let mode = meta.permissions().mode();
                hasher.update(&(mode & 0o111).to_le_bytes());
            }
        }
    }

    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

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
}
