//! What a replacement would take away (#256).
//!
//! Updating a skill replaces its directory wholesale, so anything the user — or
//! the skill itself — wrote inside it disappears without warning. The reporter
//! of #256 lost the PowerPoint templates `ppt-master` had written into its own
//! `templates/`, and only found out afterwards.
//!
//! The updater cannot tell whose files are whose; that needs per-file provenance
//! and is a much larger change. But it does not need to. At the moment of the
//! swap both trees are on disk, so it can answer a narrower question that costs
//! nothing to compute and nothing to store:
//!
//! > which paths exist now, and are simply not in the new version?
//!
//! Those are the ones about to vanish. Some are the user's; some are files the
//! author deleted upstream. Saying "these will be removed" is true of both,
//! makes no claim about ownership, and is enough for a person to recognise their
//! own work and stop.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Names that reappear the next time the skill runs, so listing them would be
/// noise rather than information.
///
/// Deliberately narrower than `content_hash::is_ignored`, which also drops
/// `.gitignore`: that one is answering "what counts as skill content", while
/// this one is answering "would the user miss it". A `.gitignore` they wrote is
/// worth a mention; a `.pyc` never is.
fn is_regenerable(name: &str) -> bool {
    matches!(name, "__pycache__" | ".DS_Store" | "Thumbs.db") || name.ends_with(".pyc")
}

/// Paths that exist under `current` but not under `replacement`.
///
/// Reported by path alone: a file whose *contents* change still exists
/// afterwards, and warning about it would bury the ones that do not.
///
/// A directory missing from the replacement entirely is reported as one entry
/// with a trailing `/`, rather than every file beneath it. Without that, a
/// nested `.git` the user created would bury the dialog under thousands of
/// object files, and a whole removed `templates/` would read as unrelated
/// losses instead of one.
///
/// Sorted, so the same update always reads the same way.
pub fn removed_paths(current: &Path, replacement: &Path) -> Result<Vec<String>> {
    // `is_dir()` follows symlinks and folds every error into "no". Only a
    // genuine absence may answer "nothing will be lost"; anything else — a
    // dangling link, a plain file, an unreadable path — has to say so.
    match std::fs::symlink_metadata(current) {
        Ok(md) if md.is_dir() => {}
        Ok(_) => return Ok(vec![display_path(Path::new(""), false)]),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(anyhow::Error::from(err)
                .context(format!("Cannot inspect {:?} before replacing it", current)));
        }
    }

    let mut out = Vec::new();
    collect(current, replacement, Path::new(""), &mut out)?;
    out.sort();
    Ok(out)
}

/// Coarse kind, so a path that changes shape counts as removed rather than
/// surviving: replacing a file with a directory of the same name still takes
/// the file's contents away.
fn kind_of(md: &std::fs::Metadata) -> u8 {
    if md.file_type().is_symlink() {
        0
    } else if md.is_dir() {
        1
    } else {
        2
    }
}

fn collect(
    current_root: &Path,
    replacement_root: &Path,
    prefix: &Path,
    out: &mut Vec<String>,
) -> Result<()> {
    let dir = current_root.join(prefix);
    let entries = std::fs::read_dir(&dir).with_context(|| format!("Failed to read {:?}", dir))?;

    for entry in entries {
        let entry = entry.with_context(|| format!("Failed to read an entry in {:?}", dir))?;
        let name = entry.file_name();
        if is_regenerable(&name.to_string_lossy()) {
            continue;
        }

        let relative = prefix.join(&name);
        let is_dir = entry
            .file_type()
            .with_context(|| format!("Failed to inspect {:?}", entry.path()))?
            .is_dir();

        let current_md = std::fs::symlink_metadata(entry.path())
            .with_context(|| format!("Failed to inspect {:?}", entry.path()))?;

        match std::fs::symlink_metadata(replacement_root.join(&relative)) {
            // Same path, same shape. Its contents may differ, which is what an
            // update is for; only look deeper if it is a directory.
            Ok(md) if kind_of(&md) == kind_of(&current_md) => {
                if is_dir {
                    collect(current_root, replacement_root, &relative, out)?;
                }
            }
            // Same path, different shape: whatever is here now does not survive.
            Ok(_) => out.push(display_path(&relative, is_dir)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                out.push(display_path(&relative, is_dir));
            }
            // Neither present nor absent as far as we can tell. Refuse rather
            // than guess: a wrong "nothing will be lost" is the failure this
            // exists to prevent, and a wrong warning trains people to click
            // through the real ones.
            Err(err) => {
                return Err(anyhow::Error::from(err).context(format!(
                    "Cannot tell whether {:?} survives the update",
                    relative
                )));
            }
        }
    }
    Ok(())
}

fn display_path(relative: &Path, is_dir: bool) -> String {
    // `\` is a legitimate filename character on unix, so only Windows'
    // separators are normalised for display.
    #[cfg(windows)]
    let mut shown = relative.to_string_lossy().replace('\\', "/");
    #[cfg(not(windows))]
    let mut shown = relative.to_string_lossy().into_owned();

    if shown.is_empty() {
        shown.push('.');
    }
    if is_dir {
        shown.push('/');
    }
    shown
}

/// Convenience for callers holding paths as strings.
pub fn removed_paths_between(current: &str, replacement: &Path) -> Result<Vec<String>> {
    removed_paths(&PathBuf::from(current), replacement)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    /// The reported case: a skill wrote a template into its own folder, and the
    /// next update took it away.
    #[test]
    fn reports_a_file_the_new_version_does_not_have() {
        let tmp = TempDir::new().unwrap();
        let current = tmp.path().join("current");
        let replacement = tmp.path().join("new");
        write(&current.join("SKILL.md"), "v1");
        write(&current.join("templates/default.pptx"), "upstream");
        write(&current.join("templates/mine.pptx"), "user work");
        write(&replacement.join("SKILL.md"), "v2");
        write(&replacement.join("templates/default.pptx"), "upstream v2");

        assert_eq!(
            removed_paths(&current, &replacement).unwrap(),
            vec!["templates/mine.pptx"]
        );
    }

    /// An update changes files; that is the point. Only absence is news.
    #[test]
    fn says_nothing_when_every_path_survives() {
        let tmp = TempDir::new().unwrap();
        let current = tmp.path().join("current");
        let replacement = tmp.path().join("new");
        write(&current.join("SKILL.md"), "v1");
        write(&current.join("scripts/run.py"), "old");
        write(&replacement.join("SKILL.md"), "v2 — rewritten");
        write(&replacement.join("scripts/run.py"), "new");
        write(&replacement.join("scripts/extra.py"), "added upstream");

        assert!(removed_paths(&current, &replacement).unwrap().is_empty());
    }

    /// One line the user can act on, not a wall of object files.
    #[test]
    fn rolls_up_a_directory_the_new_version_drops_entirely() {
        let tmp = TempDir::new().unwrap();
        let current = tmp.path().join("current");
        let replacement = tmp.path().join("new");
        write(&current.join("SKILL.md"), "v1");
        write(&current.join(".git/objects/aa/bb"), "x");
        write(&current.join(".git/HEAD"), "ref");
        write(&current.join("templates/a.pptx"), "x");
        write(&current.join("templates/b.pptx"), "x");
        write(&replacement.join("SKILL.md"), "v2");

        assert_eq!(
            removed_paths(&current, &replacement).unwrap(),
            vec![".git/", "templates/"]
        );
    }

    /// Compiled bytecode comes back on its own; a `.gitignore` does not.
    #[test]
    fn skips_regenerable_artifacts_but_not_dotfiles_in_general() {
        let tmp = TempDir::new().unwrap();
        let current = tmp.path().join("current");
        let replacement = tmp.path().join("new");
        write(&current.join("SKILL.md"), "v1");
        write(&current.join("scripts/__pycache__/m.cpython-311.pyc"), "x");
        write(&current.join("stray.pyc"), "x");
        write(&current.join(".DS_Store"), "x");
        write(&current.join(".gitignore"), "mine");
        write(&replacement.join("SKILL.md"), "v2");

        assert_eq!(
            removed_paths(&current, &replacement).unwrap(),
            vec![".gitignore", "scripts/"]
        );
    }

    /// Replacing a file with a directory of the same name still takes the
    /// file's contents away, and vice versa.
    #[test]
    fn a_path_that_changes_shape_counts_as_removed() {
        let tmp = TempDir::new().unwrap();
        let current = tmp.path().join("current");
        let replacement = tmp.path().join("new");
        write(&current.join("thing"), "a file the user edited");
        write(&current.join("other/inner.txt"), "x");
        write(&replacement.join("thing/inner.txt"), "now a directory");
        write(&replacement.join("other"), "now a file");

        assert_eq!(
            removed_paths(&current, &replacement).unwrap(),
            vec!["other/", "thing"]
        );
    }

    #[test]
    fn an_absent_current_directory_reports_nothing() {
        let tmp = TempDir::new().unwrap();
        assert!(removed_paths(&tmp.path().join("gone"), tmp.path())
            .unwrap()
            .is_empty());
    }

    /// A path that can be neither confirmed present nor confirmed absent must
    /// never come back as "nothing will be lost" — that is the exact failure
    /// this exists to prevent. Driven by pointing at a *file* where a directory
    /// belongs.
    ///
    /// The platforms refuse differently, and both are safe. Unix reports
    /// `ENOTDIR`, which is neither presence nor absence, so the walk gives up.
    /// Windows maps the same lookup to `ERROR_PATH_NOT_FOUND`, which arrives as
    /// `NotFound`, so the path reads as absent and is listed as about to go.
    /// The update is held back either way; what neither may do is answer empty.
    #[test]
    fn never_answers_nothing_will_be_lost_when_a_path_cannot_be_classified() {
        let tmp = TempDir::new().unwrap();
        let current = tmp.path().join("current");
        write(&current.join("SKILL.md"), "v1");
        let replacement = tmp.path().join("not-a-directory");
        write(&replacement, "this is a file");

        match removed_paths(&current, &replacement) {
            Err(err) => assert!(
                format!("{err:#}").contains("survives the update"),
                "refused, but not for the reason expected: {err:#}"
            ),
            Ok(reported) => assert!(
                reported.iter().any(|p| p == "SKILL.md"),
                "an unclassifiable path was reported as safe: {reported:?}"
            ),
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_the_new_version_lacks_is_reported() {
        let tmp = TempDir::new().unwrap();
        let current = tmp.path().join("current");
        let replacement = tmp.path().join("new");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::create_dir_all(&replacement).unwrap();
        std::os::unix::fs::symlink("/somewhere", current.join("link")).unwrap();

        assert_eq!(removed_paths(&current, &replacement).unwrap(), vec!["link"]);
    }
}
