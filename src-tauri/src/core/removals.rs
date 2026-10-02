//! What a replacement would take away (#256).
//!
//! Updating a skill replaces its directory wholesale, so anything the user — or
//! the skill itself — wrote inside it disappears without warning. The reporter
//! of #256 lost the PowerPoint templates `ppt-master` had written into its own
//! `templates/`, and only found out afterwards.
//!
//! Two questions, answered separately.
//!
//! [`removed_paths`]: which paths exist now, and are simply not in the new
//! version? At the moment of the swap both trees are on disk, so this costs
//! nothing to compute and nothing to store. Some of those paths are the user's;
//! some are files the author deleted upstream. Saying "these will be removed" is
//! true of both, makes no claim about ownership, and is enough for a person to
//! recognise their own work and stop.
//!
//! [`overwritten_paths`]: which files did the user change that the new version
//! writes over? The path survives, so the two trees alone cannot say — it takes
//! the installed revision as a third, fixed reference, which the caller fetches.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::core::content_hash::same_content_eol_insensitive;

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

/// Files a replacement would write over after the user changed them.
///
/// [`removed_paths`] sees what a replacement takes away by absence. It cannot
/// see a file the new version also ships: the path survives, the edit does not.
/// Telling an edit from an upstream change needs a third tree, so this compares
/// three:
///
/// - `current`: what is on disk now — the library, or a copy-mode deployment;
/// - `baseline`: the files of the installed revision, prepared the way the
///   installer prepares a library copy;
/// - `replacement`: what is about to be written.
///
/// A file is reported when the replacement will write different content over it
/// *and* it no longer matches the baseline: edited since install, or never part
/// of it — created by the user or the skill at a path the new version now ships.
/// A file that still matches the baseline is upstream's to change, and is not
/// news.
///
/// Content is compared with CRLF folded to LF in text, so a checkout's line
/// endings are never mistaken for an edit, nor a re-saved line ending for one.
///
/// Paths the replacement does not have as a file are left to [`removed_paths`],
/// which already reports them; listing them here too would show one loss twice.
///
/// Sorted, so the same update always reads the same way.
pub fn overwritten_paths(current: &Path, baseline: &Path, replacement: &Path) -> Result<Vec<String>> {
    match std::fs::symlink_metadata(current) {
        Ok(md) if md.is_dir() => {}
        // Not a directory, or not there: whatever is lost is a removal.
        Ok(_) => return Ok(Vec::new()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(anyhow::Error::from(err)
                .context(format!("Cannot inspect {:?} before replacing it", current)));
        }
    }

    let mut out = Vec::new();
    collect_overwritten(current, baseline, replacement, Path::new(""), &mut out)?;
    out.sort();
    Ok(out)
}

/// The regular file at `path`, `None` when there is no regular file there.
///
/// Anything but a clean answer is an error: reading "could not look" as "not
/// there" would turn an unreadable baseline into "edited" or an unreadable
/// replacement into "nothing to overwrite".
fn read_regular_file(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::symlink_metadata(path) {
        Ok(md) if md.is_file() => std::fs::read(path)
            .map(Some)
            .with_context(|| format!("Failed to read {:?}", path)),
        Ok(_) => Ok(None),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(anyhow::Error::from(err).context(format!("Cannot inspect {:?}", path))),
    }
}

fn collect_overwritten(
    current_root: &Path,
    baseline_root: &Path,
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
        let file_type = entry
            .file_type()
            .with_context(|| format!("Failed to inspect {:?}", entry.path()))?;
        if file_type.is_dir() {
            collect_overwritten(current_root, baseline_root, replacement_root, &relative, out)?;
            continue;
        }
        if !file_type.is_file() {
            // A link the installer would never have written; if the new version
            // puts a file there, `removed_paths` reports the change of shape.
            continue;
        }

        let Some(incoming) = read_regular_file(&replacement_root.join(&relative))? else {
            continue;
        };
        let on_disk =
            std::fs::read(entry.path()).with_context(|| format!("Failed to read {:?}", entry.path()))?;
        if same_content_eol_insensitive(&on_disk, &incoming) {
            continue;
        }
        let installed = read_regular_file(&baseline_root.join(&relative))?;
        if installed.is_some_and(|installed| same_content_eol_insensitive(&on_disk, &installed)) {
            continue;
        }
        out.push(display_path(&relative, false));
    }
    Ok(())
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

    // ── overwritten_paths ──

    struct Trees {
        _tmp: TempDir,
        current: PathBuf,
        baseline: PathBuf,
        replacement: PathBuf,
    }

    fn trees() -> Trees {
        let tmp = TempDir::new().unwrap();
        let trees = Trees {
            current: tmp.path().join("current"),
            baseline: tmp.path().join("baseline"),
            replacement: tmp.path().join("new"),
            _tmp: tmp,
        };
        for dir in [&trees.current, &trees.baseline, &trees.replacement] {
            std::fs::create_dir_all(dir).unwrap();
        }
        trees
    }

    fn overwritten(t: &Trees) -> Vec<String> {
        overwritten_paths(&t.current, &t.baseline, &t.replacement).unwrap()
    }

    /// The remaining gap in #256: the new version ships the file too, so the
    /// path survives and only the edit is lost.
    #[test]
    fn reports_an_edited_file_the_new_version_also_ships() {
        let t = trees();
        write(&t.baseline.join("templates/default.md"), "upstream v1");
        write(&t.current.join("templates/default.md"), "upstream v1, tuned by the user");
        write(&t.replacement.join("templates/default.md"), "upstream v2");

        assert_eq!(overwritten(&t), vec!["templates/default.md"]);
    }

    /// An untouched file changing upstream is what an update is for.
    #[test]
    fn says_nothing_about_a_file_only_upstream_changed() {
        let t = trees();
        write(&t.baseline.join("SKILL.md"), "v1");
        write(&t.current.join("SKILL.md"), "v1");
        write(&t.replacement.join("SKILL.md"), "v2");

        assert!(overwritten(&t).is_empty());
    }

    /// An edit the new version happens to agree with loses nothing.
    #[test]
    fn says_nothing_when_the_new_version_carries_the_same_content() {
        let t = trees();
        write(&t.baseline.join("SKILL.md"), "v1");
        write(&t.current.join("SKILL.md"), "the fix the user made by hand");
        write(&t.replacement.join("SKILL.md"), "the fix the user made by hand");

        assert!(overwritten(&t).is_empty());
    }

    /// A file that was never installed, at a path the new version now ships:
    /// whatever is there is the user's, and it is about to be replaced.
    #[test]
    fn reports_a_file_the_install_never_had_when_the_new_version_adds_it() {
        let t = trees();
        write(&t.baseline.join("SKILL.md"), "v1");
        write(&t.current.join("SKILL.md"), "v1");
        write(&t.current.join("config.json"), "{\"mine\": true}");
        write(&t.replacement.join("SKILL.md"), "v1");
        write(&t.replacement.join("config.json"), "{\"defaults\": true}");

        assert_eq!(overwritten(&t), vec!["config.json"]);
    }

    /// The same text with different line endings is not an edit: a Windows
    /// checkout and a re-save in another editor both produce it.
    #[test]
    fn line_endings_alone_are_not_an_edit() {
        let t = trees();
        write(&t.baseline.join("SKILL.md"), "line one\nline two\n");
        write(&t.current.join("SKILL.md"), "line one\r\nline two\r\n");
        write(&t.replacement.join("SKILL.md"), "line one\nline two changed\n");

        assert!(overwritten(&t).is_empty());
    }

    /// A path the new version lacks is a removal; one report per loss.
    #[test]
    fn leaves_paths_the_new_version_lacks_to_removed_paths() {
        let t = trees();
        write(&t.baseline.join("notes.md"), "v1");
        write(&t.current.join("notes.md"), "edited");

        assert!(overwritten(&t).is_empty());
        assert_eq!(
            removed_paths(&t.current, &t.replacement).unwrap(),
            vec!["notes.md"]
        );
    }

    /// Bytecode is rebuilt on the next run and is not the user's work.
    #[test]
    fn skips_regenerable_artifacts() {
        let t = trees();
        write(&t.current.join("scripts/__pycache__/m.cpython-311.pyc"), "local");
        write(&t.replacement.join("scripts/__pycache__/m.cpython-311.pyc"), "shipped");

        assert!(overwritten(&t).is_empty());
    }

    #[test]
    fn an_absent_current_directory_reports_nothing_overwritten() {
        let t = trees();
        let gone = t.current.join("gone");
        assert!(overwritten_paths(&gone, &t.baseline, &t.replacement)
            .unwrap()
            .is_empty());
    }
}
