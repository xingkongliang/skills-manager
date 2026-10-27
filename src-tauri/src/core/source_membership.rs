//! Is a stored Git Skill still in its Source's tree — and if so, where?
//!
//! `source_subpath` records where a skill was installed from. Upstream is free
//! to rename the directory, move it, or delete it, and the record keeps the old
//! path; a plain "does this path exist" check then reports `Path ... does not
//! exist in the repository`, which is the same answer for two very different
//! situations: the skill moved, or the skill is gone.
//!
//! [`locate`] answers the question once, with three outcomes that stay distinct
//! because they ask for three different actions from the user:
//!
//! - [`Placement::At`] — the path still holds the skill. Nothing to decide.
//! - [`Placement::Moved`] — the skill is elsewhere in the same checkout, with a
//!   confidence. The user confirms a one-click repoint; the module never
//!   rewrites anything itself.
//! - [`Placement::Gone`] — no candidate in the checkout. Retrying the update can
//!   never succeed, so the user is offered "detach source" / "delete" instead.
//!
//! The scan is confined to the checkout it is handed: a skill that moved to a
//! different repository is `Gone` here, not searched for across Sources.
//!
//! One checkout, one skill, one answer. Callers that locate many skills against
//! the same checkout pay for the walk per skill; caching is deliberately not
//! part of this module's state.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use walkdir::{DirEntry, WalkDir};

use crate::core::content_hash;
use crate::core::git_command::git_command;
use crate::core::path_guard;
use crate::core::skill_metadata;
use crate::core::skill_store::SkillRecord;

/// Depth cap on the candidate scan. Matches the repo-wide locator search in
/// `git_fetcher::find_skill_dir`: deep enough for `.agents/skills/<name>`, and
/// shallow enough that a repository shipping many trees stays a cheap walk.
const MAX_SCAN_DEPTH: usize = 6;

/// Directories never worth descending into while looking for skills: version
/// control metadata is always present and never holds a skill.
const PRUNED_DIRS: &[&str] = &[".git"];

/// How sure we are that a candidate is the same skill as the stored record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// The directory's content hash equals the stored `content_hash`: same
    /// bytes, so nothing but the path changed.
    High,
    /// Only the name matched. The path may be right, or it may be a different
    /// skill that happens to share a name — the user decides.
    Low,
}

/// Why a directory was proposed as this skill's new home.
///
/// The variants are exclusive and ordered strongest-first: a directory whose
/// content hash matches is reported as [`MoveReason::ContentHash`] even if its
/// name matches too, so the reason always names the most concrete evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveReason {
    /// Directory content hash equals the stored `content_hash`.
    ContentHash,
    /// Directory basename equals the stored skill name.
    DirectoryName,
    /// `SKILL.md` frontmatter `name` equals the stored skill name.
    FrontmatterName,
}

/// One place the skill might have moved to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveCandidate {
    /// Path relative to the checkout root, `/`-separated, with no `./` or
    /// trailing slash — the form that goes straight back into
    /// `source_subpath`. Empty means the checkout root itself.
    pub subpath: String,
    pub confidence: Confidence,
    pub reason: MoveReason,
}

/// The three outcomes, kept separate because each leads to a different action.
///
/// Deliberately not `Serialize`: the wire format the UI reads is
/// `UpdateOutcome`'s (a status plus the candidate list), and the candidates are
/// persisted in `skill_move_candidates`. Nothing ever sends a `Placement`, so it
/// stays a plain value rather than growing a format nobody reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// Still where the record says. Normal update path.
    At {
        /// Normalized stored subpath; empty when the checkout root is the skill.
        subpath: String,
    },
    /// Somewhere else in this checkout. Never auto-applied: changing the source
    /// pointer is the user's decision.
    ///
    /// A list rather than a single candidate because low confidence does not
    /// rank: when two directories both merely share the name, the module hands
    /// over both and lets the UI ask, instead of guessing.
    Moved { candidates: Vec<MoveCandidate> },
    /// Nowhere in this checkout. Not an error — a real answer, and one that
    /// stops the "retry the update forever" loop.
    Gone,
}

/// A skill the Source holds that this library does not.
///
/// The other half of "where is my skill": `locate` answers for one skill the
/// library already has, `discover` answers for the ones it does not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredSkill {
    /// Path relative to the checkout root, in the form `source_subpath` takes.
    pub subpath: String,
    /// The name to show and to install under: frontmatter first, directory last.
    pub name: String,
    /// Every path the same skill was found at, when a repository ships one skill
    /// under several trees (`.claude/skills/x` and `.agents/skills/x`). The first
    /// entry is `subpath`; the rest exist so a caller can tell a duplicate from a
    /// genuinely new skill.
    pub also_at: Vec<String>,
}

/// Skills in the commit `checkout` points at that `installed_subpaths` does not
/// cover.
///
/// Deliberately reads the tree, not the working copy. The checkout a round scans
/// was fetched for one skill, so its sparse cone holds that one directory and a
/// filesystem walk can only ever find skills the library already has. The
/// commit's tree is complete even in a blobless clone, so `git ls-tree` answers
/// offline and without fetching a single skill's content.
///
/// Deliberately keyed by directory name after the first sighting: a repository
/// that keeps one skill in two trees (a Claude layout and an agents layout) is
/// one new skill, not two prompts. Names are compared case-insensitively,
/// because two directories differing only by case are the same skill in practice.
///
/// `installed_subpaths` is what the library already has from this Source; a
/// directory whose subpath is in there is not new, whatever its name says — a
/// rename is `locate`'s question, not this one.
pub fn discover(checkout: &Path, installed_subpaths: &[String]) -> Vec<DiscoveredSkill> {
    let installed: HashSet<String> = installed_subpaths
        .iter()
        .filter_map(|subpath| normalize_subpath(Some(subpath)))
        .collect();
    let mut found: Vec<DiscoveredSkill> = Vec::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();

    for subpath in skill_subpaths_in_tree(checkout) {
        if installed.contains(&subpath) {
            continue;
        }
        let name = directory_name(checkout, &subpath);

        match by_name.get(&name.to_lowercase()) {
            Some(&at) => {
                if !found[at].also_at.contains(&subpath) {
                    found[at].also_at.push(subpath);
                }
            }
            None => {
                by_name.insert(name.to_lowercase(), found.len());
                found.push(DiscoveredSkill {
                    subpath,
                    name,
                    also_at: Vec::new(),
                });
            }
        }
    }

    found.sort_by(|a, b| a.subpath.cmp(&b.subpath));
    found
}

/// Every directory in the commit's tree that holds a `SKILL.md`, as the
/// `/`-separated subpath `source_subpath` uses. The checkout root is `""`.
fn skill_subpaths_in_tree(checkout: &Path) -> Vec<String> {
    let output = match git_command()
        .arg("-C")
        .arg(checkout)
        .args(["ls-tree", "-r", "-z", "--name-only", "HEAD"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            log::warn!(
                "discover: cannot read the tree at {}: {}",
                checkout.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
            return Vec::new();
        }
        Err(error) => {
            log::warn!(
                "discover: cannot run git in {}: {error}",
                checkout.display()
            );
            return Vec::new();
        }
    };

    output
        .stdout
        .split(|byte| *byte == 0)
        .filter_map(|raw| std::str::from_utf8(raw).ok())
        .filter_map(skill_dir_from_tree_path)
        .collect()
}

/// The directory part of a tracked file path, when that file is a `SKILL.md`.
///
/// Names come from the directory, never from the file: reading frontmatter would
/// fetch blobs the narrow clone deliberately never downloaded.
fn skill_dir_from_tree_path(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = path.split('/').collect();
    let file = parts.pop()?;
    if file != "SKILL.md" {
        return None;
    }
    if parts.len() > MAX_SCAN_DEPTH {
        return None;
    }
    if parts.iter().any(|part| {
        part.is_empty()
            || PRUNED_DIRS
                .iter()
                .any(|pruned| part.eq_ignore_ascii_case(pruned))
    }) {
        return None;
    }
    Some(parts.join("/"))
}

/// The name to show and to install under, taken from the directory.
fn directory_name(checkout: &Path, subpath: &str) -> String {
    let basename = subpath
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    basename
        .or_else(|| {
            checkout
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "skill".to_string())
}

/// Where does `skill` sit in `checkout`?
///
/// Never fails: "not found" is [`Placement::Gone`], not an `Err`, so callers
/// cannot mistake a scan for a failure of the check.
pub fn locate(skill: &SkillRecord, checkout: &Path) -> Placement {
    if let Some(subpath) = stored_location(skill, checkout) {
        return Placement::At { subpath };
    }

    let candidates = scan(skill, checkout);
    if candidates.is_empty() {
        Placement::Gone
    } else {
        Placement::Moved { candidates }
    }
}

/// The stored subpath, if it still resolves to a skill directory.
///
/// "Resolves to a skill" is stricter than "exists": an upstream reorganization
/// can leave the recorded path occupied by a category folder that merely
/// contains skills, and calling that `At` would hand the updater a directory
/// that is not the skill.
fn stored_location(skill: &SkillRecord, checkout: &Path) -> Option<String> {
    let subpath = normalize_subpath(skill.source_subpath.as_deref())?;
    let dir = if subpath.is_empty() {
        checkout.to_path_buf()
    } else {
        checkout.join(&subpath)
    };
    if !path_guard::is_path_safe(checkout, &dir) {
        return None;
    }
    skill_metadata::is_valid_skill_dir(&dir).then_some(subpath)
}

/// Every directory in the checkout that could be this skill, strongest first.
///
/// Content-hash matches, when there are any, are the answer: the bytes are
/// identical, so a name coincidence elsewhere adds nothing but noise. With no
/// hash match, every name match comes back — ranking them is the UI's job.
fn scan(skill: &SkillRecord, checkout: &Path) -> Vec<MoveCandidate> {
    let wanted_name = skill.name.trim();
    let stored_hash = skill
        .content_hash
        .as_deref()
        .map(str::trim)
        .filter(|hash| !hash.is_empty());

    let mut candidates = Vec::new();
    for entry in WalkDir::new(checkout)
        .max_depth(MAX_SCAN_DEPTH)
        .follow_links(false)
        .into_iter()
        .filter_entry(keep_walking)
        .flatten()
    {
        if !entry.file_type().is_dir() {
            continue;
        }
        let dir = entry.path();
        if !skill_metadata::is_valid_skill_dir(dir) {
            continue;
        }
        let Some(subpath) = relative_subpath(checkout, dir) else {
            continue;
        };

        if let Some(stored) = stored_hash {
            let same_content = content_hash::hash_directory(dir)
                .map(|hash| hash == stored)
                .unwrap_or(false);
            if same_content {
                candidates.push(MoveCandidate {
                    subpath,
                    confidence: Confidence::High,
                    reason: MoveReason::ContentHash,
                });
                continue;
            }
        }

        if wanted_name.is_empty() {
            continue;
        }

        let basename_matches = dir
            .file_name()
            .map(|name| name.to_string_lossy())
            .is_some_and(|name| name.trim().eq_ignore_ascii_case(wanted_name));
        if basename_matches {
            candidates.push(MoveCandidate {
                subpath,
                confidence: Confidence::Low,
                reason: MoveReason::DirectoryName,
            });
            continue;
        }

        let frontmatter_matches = skill_metadata::parse_skill_md(dir)
            .name
            .is_some_and(|name| name.trim().eq_ignore_ascii_case(wanted_name));
        if frontmatter_matches {
            candidates.push(MoveCandidate {
                subpath,
                confidence: Confidence::Low,
                reason: MoveReason::FrontmatterName,
            });
        }
    }

    if candidates
        .iter()
        .any(|candidate| candidate.confidence == Confidence::High)
    {
        candidates.retain(|candidate| candidate.confidence == Confidence::High);
    }
    // Deterministic order, so the first candidate a caller shows is stable:
    // strongest reason first, then path.
    candidates.sort_by(|a, b| {
        a.reason
            .cmp(&b.reason)
            .then_with(|| a.subpath.cmp(&b.subpath))
    });
    candidates
}

/// Descend into everything except version control metadata. The depth-0 guard
/// keeps a checkout that itself lives under a pruned name scannable.
fn keep_walking(entry: &DirEntry) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_dir() {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    !PRUNED_DIRS
        .iter()
        .any(|pruned| name.eq_ignore_ascii_case(pruned))
}

/// `dir` relative to `checkout` in the form `source_subpath` is stored in.
fn relative_subpath(checkout: &Path, dir: &Path) -> Option<String> {
    let relative = dir.strip_prefix(checkout).ok()?;
    let parts: Vec<String> = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    Some(parts.join("/"))
}

/// Canonicalize a stored subpath for joining and for writing back.
///
/// `None` means the stored path cannot name a directory inside the checkout
/// (absolute, or climbing out with `..`); `Some("")` means the checkout root.
pub(crate) fn normalize_subpath(raw: Option<&str>) -> Option<String> {
    let raw = raw.unwrap_or("").trim().trim_matches('/');
    if raw.is_empty() {
        return Some(String::new());
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return None;
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// A stored record pointing at `subpath`, with the hash of what the library
    /// held when it was installed.
    fn stored(name: &str, subpath: Option<&str>, content_hash: Option<&str>) -> SkillRecord {
        SkillRecord {
            id: name.to_string(),
            name: name.to_string(),
            description: None,
            source_type: "git".to_string(),
            source_ref: Some("https://github.com/acme/skills".to_string()),
            source_ref_resolved: Some("https://github.com/acme/skills".to_string()),
            source_subpath: subpath.map(str::to_string),
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: format!("/library/{name}"),
            content_hash: content_hash.map(str::to_string),
            enabled: true,
            created_at: 0,
            updated_at: 0,
            status: "ok".to_string(),
            update_status: "unknown".to_string(),
            last_checked_at: None,
            last_check_error: None,
        }
    }

    /// Lay a skill down at `subpath` (`""` for the checkout root) with the given
    /// frontmatter name and body, and return its directory.
    fn put_skill(root: &Path, subpath: &str, frontmatter_name: &str, body: &str) -> PathBuf {
        let dir = if subpath.is_empty() {
            root.to_path_buf()
        } else {
            root.join(subpath)
        };
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {frontmatter_name}\ndescription: fixture\n---\n\n{body}\n"),
        )
        .unwrap();
        dir
    }

    /// The hash a record would carry for content laid down by [`put_skill`],
    /// taken from a copy the scan never sees.
    fn install_hash(frontmatter_name: &str, body: &str) -> String {
        let library = TempDir::new().unwrap();
        let dir = put_skill(library.path(), "", frontmatter_name, body);
        content_hash::hash_directory(&dir).unwrap()
    }

    fn candidate(subpath: &str, confidence: Confidence, reason: MoveReason) -> MoveCandidate {
        MoveCandidate {
            subpath: subpath.to_string(),
            confidence,
            reason,
        }
    }

    /// The identity cases: no content hash involved, so the answer rests purely
    /// on paths and names.
    #[test]
    fn table_of_path_and_name_outcomes() {
        struct Case {
            label: &'static str,
            stored_subpath: Option<&'static str>,
            stored_name: &'static str,
            /// `(subpath, frontmatter name)` — the checkout's whole content.
            layout: &'static [(&'static str, &'static str)],
            expected: Placement,
        }

        let cases = [
            Case {
                label: "still at the stored path",
                stored_subpath: Some("skills/alpha"),
                stored_name: "alpha",
                layout: &[("skills/alpha", "alpha")],
                expected: Placement::At {
                    subpath: "skills/alpha".to_string(),
                },
            },
            Case {
                label: "stored path written with a trailing slash and ./ prefix",
                stored_subpath: Some("./skills/alpha/"),
                stored_name: "alpha",
                layout: &[("skills/alpha", "alpha")],
                expected: Placement::At {
                    subpath: "skills/alpha".to_string(),
                },
            },
            Case {
                label: "single-skill repo with no stored subpath",
                stored_subpath: None,
                stored_name: "alpha",
                layout: &[("", "alpha")],
                expected: Placement::At {
                    subpath: String::new(),
                },
            },
            Case {
                label: "moved, same directory name",
                stored_subpath: Some("skills/alpha"),
                stored_name: "alpha",
                layout: &[("agents/skills/alpha", "alpha")],
                expected: Placement::Moved {
                    candidates: vec![candidate(
                        "agents/skills/alpha",
                        Confidence::Low,
                        MoveReason::DirectoryName,
                    )],
                },
            },
            Case {
                label: "renamed directory, frontmatter keeps the name",
                stored_subpath: Some("skills/alpha"),
                stored_name: "alpha",
                layout: &[("skills/alpha-renamed", "alpha")],
                expected: Placement::Moved {
                    candidates: vec![candidate(
                        "skills/alpha-renamed",
                        Confidence::Low,
                        MoveReason::FrontmatterName,
                    )],
                },
            },
            Case {
                label: "renamed directory whose frontmatter name was renamed too",
                stored_subpath: Some("skills/alpha"),
                stored_name: "alpha",
                layout: &[("skills/beta", "beta")],
                expected: Placement::Gone,
            },
            Case {
                label: "name match is case-insensitive",
                stored_subpath: Some("skills/Alpha"),
                stored_name: "alpha",
                layout: &[("skills/other", "ALPHA")],
                expected: Placement::Moved {
                    candidates: vec![candidate(
                        "skills/other",
                        Confidence::Low,
                        MoveReason::FrontmatterName,
                    )],
                },
            },
            Case {
                label: "stored path now a container holding no SKILL.md",
                stored_subpath: Some("skills/alpha"),
                stored_name: "alpha",
                layout: &[("skills/alpha/nested", "alpha")],
                expected: Placement::Moved {
                    candidates: vec![candidate(
                        "skills/alpha/nested",
                        Confidence::Low,
                        MoveReason::FrontmatterName,
                    )],
                },
            },
            Case {
                label: "directory with no SKILL.md is never a candidate",
                stored_subpath: Some("skills/alpha"),
                stored_name: "alpha",
                layout: &[],
                expected: Placement::Gone,
            },
            Case {
                label: "stored subpath climbing out of the checkout is ignored",
                stored_subpath: Some("../alpha"),
                stored_name: "alpha",
                layout: &[("skills/beta", "beta")],
                expected: Placement::Gone,
            },
        ];

        for case in cases {
            let checkout = TempDir::new().unwrap();
            for (subpath, frontmatter_name) in case.layout {
                put_skill(checkout.path(), subpath, frontmatter_name, "body");
            }
            let record = stored(case.stored_name, case.stored_subpath, None);
            assert_eq!(
                locate(&record, checkout.path()),
                case.expected,
                "case: {}",
                case.label
            );
        }
    }

    #[test]
    fn a_container_at_the_stored_path_is_not_treated_as_at() {
        let checkout = TempDir::new().unwrap();
        // Upstream turned `skills/alpha` into a category folder and put the skill
        // one level down; reporting `At` here would hand the updater a directory
        // that is not the skill.
        put_skill(checkout.path(), "skills/alpha/nested", "alpha", "body");
        assert!(!skill_metadata::is_valid_skill_dir(
            &checkout.path().join("skills/alpha")
        ));

        let record = stored("alpha", Some("skills/alpha"), None);

        assert_eq!(
            locate(&record, checkout.path()),
            Placement::Moved {
                candidates: vec![candidate(
                    "skills/alpha/nested",
                    Confidence::Low,
                    MoveReason::FrontmatterName,
                )],
            }
        );
    }

    #[test]
    fn identical_content_anywhere_is_a_high_confidence_move() {
        let checkout = TempDir::new().unwrap();
        // The upstream rename from the report: `skills/embedded-captions` became
        // `.agents/skills/captions-overlay`.
        put_skill(
            checkout.path(),
            ".agents/skills/captions-overlay",
            "captions-overlay",
            "same bytes",
        );
        let record = stored(
            "embedded-captions",
            Some("skills/embedded-captions"),
            Some(&install_hash("captions-overlay", "same bytes")),
        );

        assert_eq!(
            locate(&record, checkout.path()),
            Placement::Moved {
                candidates: vec![candidate(
                    ".agents/skills/captions-overlay",
                    Confidence::High,
                    MoveReason::ContentHash,
                )],
            }
        );
    }

    #[test]
    fn same_name_with_changed_content_is_only_low_confidence() {
        let checkout = TempDir::new().unwrap();
        put_skill(checkout.path(), ".agents/skills/alpha", "alpha", "edited upstream");
        let record = stored(
            "alpha",
            Some("skills/alpha"),
            Some(&install_hash("alpha", "as installed")),
        );

        assert_eq!(
            locate(&record, checkout.path()),
            Placement::Moved {
                candidates: vec![candidate(
                    ".agents/skills/alpha",
                    Confidence::Low,
                    MoveReason::DirectoryName,
                )],
            }
        );
    }

    #[test]
    fn identical_content_is_low_confidence_when_the_record_has_no_hash() {
        let checkout = TempDir::new().unwrap();
        put_skill(checkout.path(), ".agents/skills/alpha", "alpha", "body");
        let record = stored("alpha", Some("skills/alpha"), None);

        assert_eq!(
            locate(&record, checkout.path()),
            Placement::Moved {
                candidates: vec![candidate(
                    ".agents/skills/alpha",
                    Confidence::Low,
                    MoveReason::DirectoryName,
                )],
            }
        );
    }

    #[test]
    fn several_name_matches_all_come_back_in_a_stable_order() {
        let checkout = TempDir::new().unwrap();
        put_skill(checkout.path(), "providers/zeta/alpha", "alpha", "one");
        put_skill(checkout.path(), "skills/gamma", "alpha", "two");
        let record = stored("alpha", Some("skills/alpha"), None);

        let Placement::Moved { candidates } = locate(&record, checkout.path()) else {
            panic!("expected a move with every name match");
        };
        // Both dirname match (`providers/zeta/alpha`) and frontmatter match
        // (`skills/gamma`) are kept; nothing is dropped for the caller's benefit.
        assert_eq!(
            candidates,
            vec![
                candidate(
                    "providers/zeta/alpha",
                    Confidence::Low,
                    MoveReason::DirectoryName
                ),
                candidate(
                    "skills/gamma",
                    Confidence::Low,
                    MoveReason::FrontmatterName
                ),
            ]
        );
    }

    #[test]
    fn a_content_match_wins_over_name_matches() {
        let checkout = TempDir::new().unwrap();
        put_skill(
            checkout.path(),
            ".agents/skills/captions-overlay",
            "captions-overlay",
            "same bytes",
        );
        put_skill(
            checkout.path(),
            "elsewhere/embedded-captions",
            "embedded-captions",
            "other bytes",
        );
        let record = stored(
            "embedded-captions",
            Some("skills/embedded-captions"),
            Some(&install_hash("captions-overlay", "same bytes")),
        );

        assert_eq!(
            locate(&record, checkout.path()),
            Placement::Moved {
                candidates: vec![candidate(
                    ".agents/skills/captions-overlay",
                    Confidence::High,
                    MoveReason::ContentHash,
                )],
            }
        );
    }

    #[test]
    fn an_empty_checkout_is_gone_not_an_error() {
        let checkout = TempDir::new().unwrap();
        let record = stored("alpha", Some("skills/alpha"), Some("deadbeef"));

        assert_eq!(locate(&record, checkout.path()), Placement::Gone);
    }

    fn write_skill(dir: &Path, frontmatter_name: Option<&str>) {
        std::fs::create_dir_all(dir).unwrap();
        let body = match frontmatter_name {
            Some(name) => format!("---\nname: {name}\n---\nbody\n"),
            None => "body\n".to_string(),
        };
        std::fs::write(dir.join("SKILL.md"), body).unwrap();
    }

    fn git(args: &[&str], cwd: Option<&Path>) {
        let mut command = crate::core::git_command::git_command();
        if let Some(cwd) = cwd {
            command.arg("-C").arg(cwd);
        }
        let status = command.args(args).status().unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    /// A checkout of `repo` narrowed to `cone`, exactly as the round receives a
    /// Source: only the installed skill is on disk, the rest lives in the tree.
    fn sparse_checkout(repo: &Path, cone: &str) -> TempDir {
        let tmp = TempDir::new().unwrap();
        git(
            &[
                "clone",
                "-q",
                "--no-checkout",
                repo.to_str().unwrap(),
                tmp.path().to_str().unwrap(),
            ],
            None,
        );
        git(&["sparse-checkout", "init", "--cone"], Some(tmp.path()));
        git(&["sparse-checkout", "set", cone], Some(tmp.path()));
        git(&["checkout", "-q", "HEAD"], Some(tmp.path()));
        tmp
    }

    fn commit_all(root: &Path) {
        git(&["init", "-q"], Some(root));
        git(&["add", "-A"], Some(root));
        git(
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "fixture",
            ],
            Some(root),
        );
    }

    /// A real repository whose committed tree holds `skills`, so discovery can
    /// ask git rather than the working copy.
    fn git_repo(skills: &[&str]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        for skill in skills {
            let name = skill.rsplit('/').next().unwrap();
            write_skill(&tmp.path().join(skill), Some(name));
        }
        commit_all(tmp.path());
        tmp
    }

    /// The round's checkout is sparse: it holds only the skill the fetch was
    /// narrowed to, so a filesystem walk can never see a skill added upstream.
    /// The commit's tree still holds every skill, and a blobless clone has all
    /// the trees, which is why discovery reads `ls-tree` and stays offline.
    #[test]
    fn a_skill_only_the_tree_holds_is_still_found() {
        let repo = git_repo(&["skills/alpha", "skills/beta"]);
        let checkout = sparse_checkout(repo.path(), "skills/alpha");
        assert!(checkout.path().join("skills/alpha").is_dir());
        assert!(
            !checkout.path().join("skills/beta").exists(),
            "the fixture must be sparse, or the test proves nothing"
        );

        let found = discover(checkout.path(), &["skills/alpha".to_string()]);

        assert_eq!(
            found.iter().map(|skill| skill.name.as_str()).collect::<Vec<_>>(),
            vec!["beta"]
        );
    }

    #[test]
    fn two_trees_holding_one_skill_are_one_new_skill() {
        let repo = git_repo(&[".claude/skills/captions", ".agents/skills/captions"]);

        let found = discover(repo.path(), &[]);

        assert_eq!(found.len(), 1, "one skill, two layouts");
        assert_eq!(found[0].name, "captions");
        assert_eq!(found[0].also_at.len(), 1, "the second tree is recorded, not listed");
    }

    #[test]
    fn a_subpath_the_library_already_has_is_not_new() {
        let repo = git_repo(&["skills/known", "skills/fresh"]);

        let found = discover(repo.path(), &["skills/known".to_string()]);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].subpath, "skills/fresh");
    }

    #[test]
    fn the_name_comes_from_the_directory_not_the_frontmatter() {
        let repo = TempDir::new().unwrap();
        write_skill(&repo.path().join("skills/nameless"), Some("frontmatter-name"));
        commit_all(repo.path());

        let found = discover(repo.path(), &[]);

        assert_eq!(found[0].name, "nameless");
    }

    #[test]
    fn names_that_differ_only_by_case_are_one_skill() {
        let repo = git_repo(&["a/Captions", "b/captions"]);

        assert_eq!(discover(repo.path(), &[]).len(), 1);
    }

    #[test]
    fn git_metadata_holds_no_skills() {
        let repo = git_repo(&[]);
        write_skill(&repo.path().join(".git/fake"), Some("fake"));

        assert!(discover(repo.path(), &[]).is_empty());
    }

    #[test]
    fn a_missing_checkout_is_gone_not_an_error() {
        let checkout = TempDir::new().unwrap();
        let absent = checkout.path().join("never-cloned");
        let record = stored("alpha", Some("skills/alpha"), None);

        assert_eq!(locate(&record, &absent), Placement::Gone);
    }

    #[test]
    fn skills_inside_git_metadata_are_not_candidates() {
        let checkout = TempDir::new().unwrap();
        put_skill(checkout.path(), ".git/skills/alpha", "alpha", "body");
        let record = stored("alpha", Some("skills/alpha"), None);

        assert_eq!(locate(&record, checkout.path()), Placement::Gone);
    }
}
