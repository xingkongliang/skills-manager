//! The canonical spelling of a Skill's Subpath.
//!
//! A Subpath is the directory a Skill lives in inside its Source, written
//! `/`-separated and without a trailing slash. It is the form git stores, the
//! form libgit2's `Tree::get_path` accepts, and the form a Source scan reports.
//!
//! [`relative_to`] produces a Subpath from a checkout; [`canonical`] restores
//! one from a value stored earlier, for the readers that compare it against a
//! git tree.
//!
//! Windows is the one platform that needs a repair: `Path::to_string_lossy`
//! separates with `\` there, and libgit2 reads that as part of one entry name
//! rather than a separator — which marked live Skills as Source Gone once their
//! check compared the two (#671). `\` is a legal filename character on unix, so
//! the substitution is `cfg`-gated, as it is in `content_hash` and
//! `git_fetcher::sparse_pattern`. The cost of that gate: a database written on
//! Windows and then read on unix keeps its backslashes and still names nothing
//! there.

use std::path::Path;

/// A Subpath in the form the rest of the code compares and returns.
///
/// On Windows, separators become `/`; on unix nothing is rewritten, because a
/// name may contain `\`. A trailing slash is dropped either way: it names the
/// same directory and only makes two spellings of one Subpath compare unequal.
pub fn canonical(raw: &str) -> String {
    canonical_with(raw, cfg!(windows))
}

/// The `cfg` is a parameter so both branches can be tested on any host: the
/// Windows branch is the one this module exists for, and it is the one a
/// unix-only test run would otherwise never execute.
fn canonical_with(raw: &str, separators_are_windows: bool) -> String {
    let unified = if separators_are_windows {
        raw.replace('\\', "/")
    } else {
        raw.to_string()
    };
    unified.trim_end_matches('/').to_string()
}

/// Where `dir` sits inside `root`, as a Subpath.
///
/// `None` when `dir` is the root itself (a Skill at the top of its Source has
/// no Subpath) or when it is not inside `root` at all.
pub fn relative_to(root: &Path, dir: &Path) -> Option<String> {
    let relative = dir.strip_prefix(root).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    Some(
        relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spelling a Windows build writes. It has to mean the same directory
    /// as the slash form, because the comparison it feeds speaks only `/`.
    #[test]
    fn a_windows_separator_is_the_same_subpath() {
        assert_eq!(canonical_with(r"skills\b", true), "skills/b");
        assert_eq!(
            canonical_with(r"tools\my-skill\nested", true),
            "tools/my-skill/nested"
        );
    }

    /// `\` names a real directory on unix, so rewriting it there would invent a
    /// different Subpath — the one case the Windows repair must not touch.
    #[test]
    fn a_backslash_is_part_of_the_name_where_it_is_legal() {
        assert_eq!(canonical_with(r"skills\b", false), r"skills\b");
    }

    #[test]
    fn a_trailing_slash_is_dropped() {
        assert_eq!(canonical_with("skills/b/", false), "skills/b");
        assert_eq!(canonical_with(r"skills\b\\", true), "skills/b");
    }

    #[test]
    fn a_slash_subpath_is_already_canonical() {
        assert_eq!(canonical_with("skills/b", true), "skills/b");
        assert_eq!(canonical_with("", true), "");
    }

    /// The value that goes into `source_subpath`, whatever the host's
    /// separator is. Asserting the literal (not a recomputed form) is the
    /// point: the two spellings must come out identical.
    #[test]
    fn a_nested_directory_reads_as_a_slash_subpath() {
        let root = Path::new("/tmp/checkout");
        let dir = root.join("tools").join("my-skill");
        assert_eq!(relative_to(root, &dir).as_deref(), Some("tools/my-skill"));
    }

    #[test]
    fn the_root_itself_has_no_subpath() {
        let root = Path::new("/tmp/checkout");
        assert_eq!(relative_to(root, root), None);
    }

    #[test]
    fn a_directory_outside_the_root_has_no_subpath() {
        let root = Path::new("/tmp/checkout");
        assert_eq!(relative_to(root, Path::new("/tmp/other/skill")), None);
    }
}
