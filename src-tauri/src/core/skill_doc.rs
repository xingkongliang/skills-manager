//! Shared handling of a skill's documentation file — the `SKILL.md` (and its
//! accepted variants) that every workspace shows and that inline editing
//! writes back.
//!
//! Reading and writing share one filename list on purpose: whatever a read
//! hands the UI must be a name the matching save is willing to write, or a
//! user could open a document the app then refuses to store.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::core::error::AppError;

/// Documentation filenames a skill directory may carry, in the order they are
/// preferred when several are present.
pub const DOCUMENT_CANDIDATES: &[&str] = &[
    "SKILL.md",
    "skill.md",
    "CLAUDE.md",
    "claude.md",
    "README.md",
    "readme.md",
];

/// Largest document an editor will load or store. Well past any real skill
/// document, and low enough that a stray binary cannot be pushed through the
/// save command.
pub const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;

/// Resolve a client-supplied filename to the exact [`DOCUMENT_CANDIDATES`]
/// entry it names.
///
/// The returned name is one of our own constants rather than the caller's
/// string, so a save can only ever land on a file the app already recognises:
/// no separators, no `..`, no rename-by-save, and no writing a `.sh` into a
/// skill directory through the document endpoint.
pub fn canonical_document_name(filename: &str) -> Result<&'static str, AppError> {
    DOCUMENT_CANDIDATES
        .iter()
        .copied()
        .find(|candidate| *candidate == filename)
        .ok_or_else(|| {
            AppError::invalid_input(format!(
                "\"{filename}\" is not an editable skill document"
            ))
        })
}

/// Reject a document body too large to be a skill document.
pub fn validate_document_content(content: &str) -> Result<(), AppError> {
    if content.len() > MAX_DOCUMENT_BYTES {
        return Err(AppError::invalid_input(
            "Document is too large to save from the editor",
        ));
    }
    Ok(())
}

/// Locate a skill's document inside `dir`, returning its filename and content.
///
/// `allowed_roots` bounds where a symlinked document may resolve to; a link
/// that escapes every root is skipped rather than read. Pass an empty slice to
/// accept any resolution (the central library, where the directory itself is
/// already the trust boundary).
pub fn read_document(
    dir: &Path,
    allowed_roots: &[PathBuf],
) -> Result<(String, String), AppError> {
    let path = find_document_path(dir, allowed_roots)
        .ok_or_else(|| AppError::not_found("No documentation file found"))?;
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let content = std::fs::read_to_string(&path)?;
    Ok((filename, content))
}

/// Path of the document `read_document` would return, if there is one.
pub fn find_document_path(dir: &Path, allowed_roots: &[PathBuf]) -> Option<PathBuf> {
    DOCUMENT_CANDIDATES
        .iter()
        .map(|candidate| dir.join(candidate))
        .find(|path| path.is_file() && symlink_stays_within(path, allowed_roots))
}

/// True when `path` is not a symlink, or resolves inside one of `allowed_roots`.
/// An empty root list imposes no restriction; a broken link never passes.
fn symlink_stays_within(path: &Path, allowed_roots: &[PathBuf]) -> bool {
    if allowed_roots.is_empty() {
        return true;
    }
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.file_type().is_symlink() {
        return true;
    }
    let Ok(resolved) = std::fs::canonicalize(path) else {
        return false;
    };
    allowed_roots.iter().any(|root| {
        std::fs::canonicalize(root)
            .map(|canonical| resolved.starts_with(&canonical))
            .unwrap_or(false)
    })
}

/// Fingerprint of a document's text, line endings folded away.
///
/// The editor sends back the fingerprint of what it loaded, so a save can tell
/// "nobody touched this" from "the watcher, a sync, or another device rewrote
/// it while the panel was open". Folding line endings keeps a CRLF checkout
/// from reading as a conflict against the LF text the editor holds.
pub fn document_fingerprint(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n");
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Refuse a save whose base no longer matches what is on disk.
///
/// `expected` is the fingerprint the editor loaded with; `None` means the
/// caller is knowingly overwriting (the user chose to after being told).
pub fn ensure_unchanged(path: &Path, expected: Option<&str>) -> Result<(), AppError> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let current = std::fs::read_to_string(path).unwrap_or_default();
    if document_fingerprint(&current) == expected {
        return Ok(());
    }
    Err(AppError::invalid_input(DOCUMENT_CHANGED_ON_DISK))
}

/// Marker the frontend matches on to offer "reload" or "overwrite" instead of
/// showing a save error the user can do nothing with.
pub const DOCUMENT_CHANGED_ON_DISK: &str = "document_changed_on_disk";

/// Rewrite `content` with the line ending `existing` already uses.
///
/// A `<textarea>` hands back `\n` whatever the file held, so without this a
/// one-word edit to a CRLF document would rewrite every line of it — churn in
/// the backup history, and a whole-file diff against the upstream source.
pub fn match_line_endings(existing: &str, content: &str) -> String {
    if !uses_crlf(existing) {
        return content.to_string();
    }
    // Normalise first: content that already carries CRLF must not become CRCRLF.
    content.replace("\r\n", "\n").replace('\n', "\r\n")
}

/// Whether a document is written with Windows line endings. Decided by the
/// first line break, which is what a file mixing both would be saved as by any
/// editor that picks one.
fn uses_crlf(content: &str) -> bool {
    match content.find('\n') {
        Some(0) => false,
        Some(index) => content.as_bytes()[index - 1] == b'\r',
        None => false,
    }
}

/// Write an edited document back over `path`, preserving the line endings the
/// file already used and leaving the original in place if the write fails.
///
/// The watcher is muted around the write: the caller refreshed the UI for the
/// save it just performed, so echoing our own change back is redundant work.
/// Auto-backup is notified before the mute is consulted, so a saved edit still
/// reaches the backup queue.
pub fn write_document(path: &Path, content: &str) -> Result<(), AppError> {
    validate_document_content(content)?;

    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let body = match_line_endings(&existing, content);

    let parent = path
        .parent()
        .ok_or_else(|| AppError::invalid_input("Invalid document path"))?;
    crate::core::file_watcher::mute_self_writes(parent);

    // Write beside the target and rename over it, so an interrupted save
    // cannot leave a skill holding half a document.
    let temp_path = parent.join(format!(".{}.tmp-{}", document_stem(path), uuid::Uuid::new_v4()));
    std::fs::write(&temp_path, body.as_bytes())?;

    // `rename` replaces the destination on both platforms we ship, but it
    // fails across a symlinked document (the link would be replaced by the
    // file). Writing through the link in that case is deliberate: a
    // symlink-synced skill has exactly one copy, and editing it is the point.
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if let Err(err) = std::fs::rename(&temp_path, &target) {
        let direct = std::fs::write(&target, body.as_bytes());
        let _ = std::fs::remove_file(&temp_path);
        direct.map_err(|_| AppError::io(err))?;
    }

    Ok(())
}

fn document_stem(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn canonical_name_accepts_every_candidate() {
        for candidate in DOCUMENT_CANDIDATES {
            assert_eq!(canonical_document_name(candidate).unwrap(), *candidate);
        }
    }

    #[test]
    fn canonical_name_rejects_paths_and_unknown_files() {
        for name in [
            "../SKILL.md",
            "nested/SKILL.md",
            "..\\SKILL.md",
            "install.sh",
            "Skill.md",
            "",
        ] {
            assert!(
                canonical_document_name(name).is_err(),
                "{name} should not be editable"
            );
        }
    }

    #[test]
    fn read_document_prefers_skill_md() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("README.md"), "readme").unwrap();
        fs::write(tmp.path().join("SKILL.md"), "skill").unwrap();

        let (filename, content) = read_document(tmp.path(), &[]).unwrap();
        assert_eq!(filename, "SKILL.md");
        assert_eq!(content, "skill");
    }

    #[test]
    fn read_document_without_any_candidate_is_not_found() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("notes.txt"), "nope").unwrap();
        assert!(read_document(tmp.path(), &[]).is_err());
    }

    #[test]
    fn crlf_documents_keep_their_line_endings() {
        let existing = "---\r\nname: demo\r\n---\r\n";
        let edited = "---\nname: demo\nmodel: opus\n---\n";
        assert_eq!(
            match_line_endings(existing, edited),
            "---\r\nname: demo\r\nmodel: opus\r\n---\r\n"
        );
    }

    #[test]
    fn lf_documents_are_left_alone() {
        let existing = "# Title\nbody\n";
        let edited = "# Title\nnew body\n";
        assert_eq!(match_line_endings(existing, edited), edited);
    }

    #[test]
    fn crlf_conversion_is_idempotent() {
        let existing = "a\r\nb\r\n";
        let once = match_line_endings(existing, "a\nb\n");
        assert_eq!(match_line_endings(existing, &once), once);
    }

    #[test]
    fn a_leading_newline_does_not_read_as_crlf() {
        // `content[index - 1]` would panic (or read the wrong byte) on a
        // document whose very first character is the line break.
        assert!(!uses_crlf("\nbody"));
    }

    #[test]
    fn write_document_replaces_content_and_leaves_no_temp_files() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("SKILL.md");
        fs::write(&path, "---\nname: demo\n---\nold\n").unwrap();

        write_document(&path, "---\nname: demo\n---\nnew\n").unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "---\nname: demo\n---\nnew\n"
        );
        let leftovers: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name != "SKILL.md")
            .collect();
        assert!(leftovers.is_empty(), "unexpected leftovers: {leftovers:?}");
    }

    #[test]
    fn write_document_preserves_crlf_on_disk() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("SKILL.md");
        fs::write(&path, "line one\r\nline two\r\n").unwrap();

        write_document(&path, "line one\nline three\n").unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "line one\r\nline three\r\n"
        );
    }

    #[test]
    fn fingerprint_ignores_line_endings() {
        assert_eq!(
            document_fingerprint("a\r\nb\r\n"),
            document_fingerprint("a\nb\n")
        );
        assert_ne!(document_fingerprint("a\n"), document_fingerprint("b\n"));
    }

    #[test]
    fn ensure_unchanged_passes_for_a_matching_base() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("SKILL.md");
        fs::write(&path, "body\r\n").unwrap();

        assert!(ensure_unchanged(&path, Some(&document_fingerprint("body\n"))).is_ok());
        assert!(ensure_unchanged(&path, None).is_ok());
    }

    #[test]
    fn ensure_unchanged_rejects_an_outdated_base() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("SKILL.md");
        fs::write(&path, "changed by someone else\n").unwrap();

        let err = ensure_unchanged(&path, Some(&document_fingerprint("what the editor loaded\n")))
            .unwrap_err();
        assert!(err.message.contains(DOCUMENT_CHANGED_ON_DISK));
    }

    #[test]
    fn write_document_rejects_an_oversized_body() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("SKILL.md");
        fs::write(&path, "small").unwrap();

        let huge = "x".repeat(MAX_DOCUMENT_BYTES + 1);
        assert!(write_document(&path, &huge).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "small");
    }
}
