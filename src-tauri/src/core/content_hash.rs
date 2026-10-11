use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
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

/// Dependency installs (`npm install`, a Python venv) inside a skill. They stay
/// in the content hash — a directory that holds them is not interchangeable
/// with one that doesn't, and repairs that replace a directory on hash
/// equality must keep seeing the difference. Only the update-check tiebreaker
/// ([`hash_directory_eol_insensitive`]) looks past them (#502). The cost is
/// that an edit to a file a skill keeps under a directory with one of these
/// names goes unnoticed by that tiebreaker — nobody authors skill files there.
const DEPENDENCY_DIRS: &[&str] = &["node_modules", ".venv", "venv"];

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
///
/// Traversal errors are dropped, so an unreadable subdirectory reads as absent.
/// Callers that must not mistake "could not look" for "not there" want
/// [`list_content_files_strict`] instead.
pub fn list_content_files(dir: &Path) -> Vec<ContentEntry> {
    // The lossy walk cannot fail: every error is skipped below.
    walk_content_files(dir, false, false).unwrap_or_default()
}

/// [`list_content_files`] over the same content scope, but a directory that
/// cannot be traversed is an error rather than an empty one.
pub fn list_content_files_strict(dir: &Path) -> Result<Vec<ContentEntry>> {
    walk_content_files(dir, true, false)
}

fn walk_content_files(dir: &Path, strict: bool, skip_deps: bool) -> Result<Vec<ContentEntry>> {
    let mut entries = Vec::new();
    for result in WalkDir::new(dir).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        !is_ignored(&name)
            && !(skip_deps
                && e.depth() > 0
                && e.file_type().is_dir()
                && DEPENDENCY_DIRS.contains(&name.as_ref()))
    }) {
        match result {
            Ok(entry) if entry.file_type().is_file() => entries.push(entry),
            Ok(_) => {}
            Err(err) if strict => {
                return Err(
                    anyhow::Error::new(err).context(format!("failed to walk {}", dir.display()))
                );
            }
            Err(_) => {}
        }
    }

    entries.sort_by(|a, b| a.path().cmp(b.path()));

    let mapped = entries
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
        .collect();
    Ok(mapped)
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

/// True when `content` is text this may safely fold: valid UTF-8 with no NUL.
///
/// Both halves are needed, and both are deliberately stricter than git's
/// heuristic (a NUL in the first 8000 bytes). Git is deciding how to *display*
/// a file; this decides whether two files are the same, so every wrong answer
/// here hides a real change:
/// - a binary whose first NUL falls past byte 8000 is text to git, and two such
///   files differing by one `\r` would fold into each other;
/// - a binary with no NUL at all (`FF 0D 0A` vs `FF 0A`) passes any NUL test,
///   so "no NUL" cannot establish text on its own.
///
/// Requiring valid UTF-8 draws the line at a representation whose `\r\n` really
/// is a line ending. It also means text in a non-UTF-8 encoding is treated as
/// binary and its line endings are not folded — that direction is the safe one:
/// an update is offered that may not be needed, rather than a real one hidden.
fn is_foldable_text(content: &[u8]) -> bool {
    !content.contains(&0) && std::str::from_utf8(content).is_ok()
}

/// Fold CRLF line endings to LF in content [`is_foldable_text`] accepts;
/// everything else is returned untouched.
///
/// The guarantee is bounded, and worth stating exactly: content that fails that
/// test is never folded, so the `FF 0D 0A` / `FF 0A` class of binary cannot
/// normalize together. It is *not* a claim that no two meaningfully different
/// files can — a file that is valid UTF-8 and NUL-free is folded whether or not
/// its `\r\n` carries meaning (a fixture asserting CRLF handling, say). Drawing
/// the line anywhere admits some such file; this one is drawn where `\r\n`
/// almost always is a line ending.
///
/// A lone `\r` (old-Mac line ending, or a carriage return inside a progress
/// bar) is left alone — only `\r\n` is a line ending both platforms mean the
/// same thing by.
pub(crate) fn fold_crlf(content: &[u8]) -> Cow<'_, [u8]> {
    if !is_foldable_text(content) || !content.windows(2).any(|pair| pair == b"\r\n") {
        return Cow::Borrowed(content);
    }
    let mut folded = Vec::with_capacity(content.len());
    let mut index = 0;
    while index < content.len() {
        if content[index] == b'\r' && content.get(index + 1) == Some(&b'\n') {
            index += 1; // drop the CR, keep the LF the next iteration copies
            continue;
        }
        folded.push(content[index]);
        index += 1;
    }
    Cow::Owned(folded)
}

/// Hash the same content scope as [`hash_entries`], with CRLF folded to LF in
/// text files.
///
/// This is a **tiebreaker, not a replacement**: [`hash_entries`] stays the
/// stored, byte-exact identity of a skill, and this is only consulted once two
/// byte hashes already disagree, to answer the narrower question "do these two
/// trees differ by anything other than line endings?".
///
/// It exists because a skill that reaches one machine through a git checkout
/// and another through our own byte copy legitimately holds the same content in
/// two encodings: Git for Windows defaults to `core.autocrlf=true`, so the same
/// file is CRLF there and LF on macOS. Nothing is wrong in that case and the
/// user must not be told there is an update.
///
/// Unlike [`hash_entries`], an unreadable file is an **error, not an empty
/// file**. The two differ in what a wrong answer costs: a byte hash that cannot
/// read a file reports "changed" and the user is offered an update they may not
/// need, while this function's answer *suppresses* an update, so a file it
/// could not read must never be quietly treated as matching the other side.
///
/// Each field is length-framed, where [`hash_entries`] concatenates a path
/// directly onto its content. Unframed, a rename that shifts bytes across the
/// boundary collides: `ab` holding `c\n` and `a` holding `bc\r\n` both feed
/// `abc\n` into the hasher, so a removed file, an added file and a content
/// change all read as "no difference". `hash_entries` shares that flaw and is
/// deliberately left alone — it is the stored identity of every skill, and
/// reframing it would invalidate every hash on disk.
pub fn hash_entries_eol_insensitive(entries: &[ContentEntry]) -> Result<String> {
    let mut hasher = Sha256::new();
    for entry in entries {
        let path = entry.relative_path.as_bytes();
        hasher.update((path.len() as u64).to_le_bytes());
        hasher.update(path);
        let content = std::fs::read(&entry.path)
            .with_context(|| format!("failed to read {}", entry.path.display()))?;
        let folded = fold_crlf(&content);
        hasher.update((folded.len() as u64).to_le_bytes());
        hasher.update(folded.as_ref());
        // Kept in step with `hash_entries`: a permission-only change is still
        // a change, and must not hide behind the line-ending comparison.
        #[cfg(unix)]
        if let Some(bits) = entry.exec_bits {
            hasher.update(bits.to_le_bytes());
        }
    }
    Ok(hex::encode(hasher.finalize()))
}

/// [`hash_entries_eol_insensitive`] over a directory, enumerated strictly.
///
/// Two refusals rather than an answer, both because this function's answer can
/// only ever *suppress* an update:
/// - a directory that cannot be traversed is an error, not an absent one —
///   otherwise two trees whose unreadable subtrees hold different files would
///   match on what remains;
/// - an empty or missing directory is an error, not "the hash of nothing",
///   which every empty tree would share.
///
/// Dependency installs ([`DEPENDENCY_DIRS`]) are skipped as well: `npm install`
/// in a local skill's source is not an update to offer (#502).
pub fn hash_directory_eol_insensitive(dir: &Path) -> Result<String> {
    let entries = walk_content_files(dir, true, true)?;
    if entries.is_empty() {
        anyhow::bail!("no content files under {}", dir.display());
    }
    hash_entries_eol_insensitive(&entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// #502: the update tiebreaker looks past dependency installs, while the
    /// stored-identity hash keeps seeing them.
    #[test]
    fn dependency_installs_only_bypass_the_update_tiebreaker() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("SKILL.md"), "# hello").unwrap();
        let loose = hash_directory_eol_insensitive(tmp.path()).unwrap();
        let strict = hash_directory(tmp.path()).unwrap();

        for dir in [
            "node_modules/pkg",
            "scripts/node_modules/pkg",
            ".venv/lib",
            "venv/lib",
        ] {
            fs::create_dir_all(tmp.path().join(dir)).unwrap();
            fs::write(tmp.path().join(dir).join("index.js"), "x").unwrap();
        }

        assert_eq!(hash_directory_eol_insensitive(tmp.path()).unwrap(), loose);
        assert_ne!(hash_directory(tmp.path()).unwrap(), strict);
    }

    /// The skip applies below the root: a skill whose own directory happens to
    /// be named `venv` still hashes its content.
    #[test]
    fn a_skill_directory_named_like_a_dependency_dir_is_still_hashed() {
        let tmp = tempdir().unwrap();
        let skill = tmp.path().join("venv");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "# hello").unwrap();

        assert!(hash_directory_eol_insensitive(&skill).is_ok());
    }

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

    /// The case this exists for: the same skill reaching two machines through
    /// different pipelines (git checkout on Windows vs our byte copy) holds the
    /// same text in two encodings. The byte hash must still see that difference
    /// — it is the stored identity — while the tiebreaker must not.
    #[test]
    fn line_endings_alone_split_the_byte_hash_but_not_the_tiebreaker() {
        let lf = tempdir().unwrap();
        fs::write(lf.path().join("SKILL.md"), "# Skill\nline two\n").unwrap();
        let crlf = tempdir().unwrap();
        fs::write(crlf.path().join("SKILL.md"), "# Skill\r\nline two\r\n").unwrap();

        assert_ne!(
            hash_directory(lf.path()).unwrap(),
            hash_directory(crlf.path()).unwrap(),
            "the byte hash is the stored identity and must stay byte-exact"
        );
        assert_eq!(
            hash_directory_eol_insensitive(lf.path()).unwrap(),
            hash_directory_eol_insensitive(crlf.path()).unwrap()
        );
    }

    /// The tiebreaker must not become a blanket "everything is the same":
    /// a real edit has to survive it, or it would silently suppress updates.
    #[test]
    fn the_tiebreaker_still_separates_real_edits() {
        let one = tempdir().unwrap();
        fs::write(one.path().join("SKILL.md"), "# Skill\r\nline two\r\n").unwrap();
        let two = tempdir().unwrap();
        fs::write(two.path().join("SKILL.md"), "# Skill\r\nline THREE\r\n").unwrap();

        assert_ne!(
            hash_directory_eol_insensitive(one.path()).unwrap(),
            hash_directory_eol_insensitive(two.path()).unwrap()
        );
    }

    /// The sniff must cover the whole file, not git's first 8000 bytes. These
    /// two binaries put their first NUL past that window and differ only by a
    /// `\r`, so a prefix-only sniff folds them together and reports a real
    /// difference as "up to date".
    #[test]
    fn a_binary_whose_first_nul_is_past_8000_bytes_is_still_binary() {
        let mut early = vec![b'A'; 9000];
        early.extend_from_slice(b"\x00\r\ntail");
        let mut late = vec![b'A'; 9000];
        late.extend_from_slice(b"\x00\ntail");

        let one = tempdir().unwrap();
        fs::write(one.path().join("blob.bin"), &early).unwrap();
        let two = tempdir().unwrap();
        fs::write(two.path().join("blob.bin"), &late).unwrap();

        assert_ne!(
            hash_directory_eol_insensitive(one.path()).unwrap(),
            hash_directory_eol_insensitive(two.path()).unwrap()
        );
    }

    fn entry(relative_path: &str, path: PathBuf) -> ContentEntry {
        ContentEntry {
            relative_path: relative_path.to_string(),
            path,
            exec_bits: None,
            modified_ms: None,
        }
    }

    /// The tiebreaker only ever *suppresses* an update, so a file it could not
    /// read must be an error rather than an empty file — otherwise two
    /// unreadable files would hash alike and hide a real change. The byte hash
    /// makes the opposite trade deliberately; see the doc comments.
    #[test]
    fn an_unreadable_file_is_an_error_not_an_empty_one() {
        let tmp = tempdir().unwrap();
        let missing = vec![entry("gone.md", tmp.path().join("gone.md"))];
        let other_missing = vec![entry("gone.md", tmp.path().join("also-gone.md"))];

        assert!(hash_entries_eol_insensitive(&missing).is_err());
        assert_eq!(
            hash_entries(&missing),
            hash_entries(&other_missing),
            "the byte hash folds an unreadable file to nothing — two different \
             unreadable files are indistinguishable to it. That is its existing \
             contract, and the reason the tiebreaker may not share it."
        );
    }

    /// An unreadable *directory* is the same hazard one level up, and
    /// [`list_content_files`] drops it silently. Both trees would then be judged
    /// on the readable remainder, and a subtree difference would vanish.
    #[cfg(unix)]
    #[test]
    fn an_untraversable_subdirectory_is_an_error_for_the_tiebreaker() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("SKILL.md"), "# skill\n").unwrap();
        let locked = tmp.path().join("templates");
        fs::create_dir(&locked).unwrap();
        fs::write(locked.join("inner.md"), "content").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

        // Root ignores the mode, which would make the assertion below vacuous —
        // so establish that the identity running this test is actually blocked.
        let mode_is_enforced = fs::read_dir(&locked).is_err();
        let strict = hash_directory_eol_insensitive(tmp.path());
        let lossy = list_content_files(tmp.path());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

        if mode_is_enforced {
            assert!(
                strict.is_err(),
                "an unreadable subtree must not be reported as absent"
            );
            assert_eq!(
                lossy.len(),
                1,
                "the lossy walk keeps its existing contract: it sees only SKILL.md"
            );
        } else {
            // Say so rather than pass quietly: a green run under an identity
            // that ignores the mode has asserted nothing here.
            eprintln!(
                "WARNING: an_untraversable_subdirectory_is_an_error_for_the_tiebreaker \
                 asserted nothing — this identity can read a 0o000 directory"
            );
        }
    }

    /// Unframed, a path concatenated straight onto its content lets a rename
    /// shift bytes across the boundary: `ab` holding `c\n` and `a` holding
    /// `bc\r\n` both produce `abc\n`, so a removed file, an added file and an
    /// edit would all read as no difference.
    #[test]
    fn a_path_cannot_bleed_into_its_content() {
        let tmp = tempdir().unwrap();
        let first = tmp.path().join("first");
        fs::write(&first, "c\n").unwrap();
        let second = tmp.path().join("second");
        fs::write(&second, "bc\r\n").unwrap();

        assert_ne!(
            hash_entries_eol_insensitive(&[entry("ab", first)]).unwrap(),
            hash_entries_eol_insensitive(&[entry("a", second)]).unwrap()
        );
    }

    /// "No NUL" does not establish text: this payload has none, and folding it
    /// would hide a real one-byte difference between two binaries.
    #[test]
    fn nul_free_binary_content_is_still_not_folded() {
        let one = tempdir().unwrap();
        fs::write(one.path().join("payload.bin"), b"\xff\r\n").unwrap();
        let two = tempdir().unwrap();
        fs::write(two.path().join("payload.bin"), b"\xff\n").unwrap();

        assert_ne!(
            hash_directory_eol_insensitive(one.path()).unwrap(),
            hash_directory_eol_insensitive(two.path()).unwrap()
        );
    }

    /// A permission-only change must not hide behind the line-ending
    /// comparison, the same way it does not hide from the byte hash.
    #[cfg(unix)]
    #[test]
    fn an_executable_bit_change_alone_still_differs() {
        use std::os::unix::fs::PermissionsExt;

        let plain = tempdir().unwrap();
        fs::write(plain.path().join("run.sh"), "echo hi\n").unwrap();
        let executable = tempdir().unwrap();
        let script = executable.path().join("run.sh");
        fs::write(&script, "echo hi\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

        assert_ne!(
            hash_directory_eol_insensitive(plain.path()).unwrap(),
            hash_directory_eol_insensitive(executable.path()).unwrap()
        );
    }

    /// "The hash of nothing" is the same for every empty tree, so a missing
    /// library copy would compare equal to an empty source and report a
    /// vanished skill as up to date.
    #[test]
    fn an_empty_or_missing_directory_has_no_tiebreaker_hash() {
        let tmp = tempdir().unwrap();
        assert!(hash_directory_eol_insensitive(&tmp.path().join("nope")).is_err());
        assert!(hash_directory_eol_insensitive(tmp.path()).is_err());
    }

    /// Two different binaries whose bytes happen to contain `\r\n` must not
    /// fold into each other. Without the binary sniff these two files differ
    /// only by a `\r` and would hash identically — a real skill ships `.png`
    /// and `.pptx` templates, so this is not a theoretical collision.
    #[test]
    fn binary_content_is_never_folded() {
        let one = tempdir().unwrap();
        fs::write(one.path().join("logo.png"), b"\x89PNG\x00\r\n\x1a\n").unwrap();
        let two = tempdir().unwrap();
        fs::write(two.path().join("logo.png"), b"\x89PNG\x00\n\x1a\n").unwrap();

        assert_ne!(
            hash_directory_eol_insensitive(one.path()).unwrap(),
            hash_directory_eol_insensitive(two.path()).unwrap(),
            "a NUL byte marks these binary; folding them would lose a real difference"
        );
    }

    /// A lone CR is not a CRLF line ending and must not be dropped, or content
    /// that genuinely differs by one would read as identical.
    #[test]
    fn a_lone_carriage_return_is_left_alone() {
        let with_cr = tempdir().unwrap();
        fs::write(with_cr.path().join("a.txt"), "progress\rdone\r\n").unwrap();
        let without = tempdir().unwrap();
        fs::write(without.path().join("a.txt"), "progressdone\r\n").unwrap();

        assert_ne!(
            hash_directory_eol_insensitive(with_cr.path()).unwrap(),
            hash_directory_eol_insensitive(without.path()).unwrap()
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
