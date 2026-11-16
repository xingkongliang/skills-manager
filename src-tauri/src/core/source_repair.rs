//! Where is a stored skill, inside a checkout of its Source?
//!
//! `source_subpath` records where a skill was installed from, and the Source is
//! free to move or delete it afterwards. The check only ever sees a blobless
//! tree, so it can answer "gone from the recorded path" — not "gone", and not
//! "moved". This module answers the fuller question against a real checkout,
//! which is what a repair page has when the user asks it to look (#655).
//!
//! Three answers, because they lead to three different actions:
//!
//! - [`Placement::At`] — update in place.
//! - [`Placement::Moved`] — re-point at a candidate, keeping the skill's
//!   identity (id, presets, deployments, tags).
//! - [`Placement::Gone`] — detach the Source, or delete.
//!
//! The scan is confined to the checkout it is handed: a skill that moved to a
//! different repository is `Gone` here.

use std::path::{Path, PathBuf};

use walkdir::{DirEntry, WalkDir};

use crate::core::content_hash;
use crate::core::path_guard;
use crate::core::skill_metadata;
use crate::core::subpath;
use crate::core::skill_store::SkillRecord;

/// Depth cap on the candidate scan. Matches the repo-wide locator search in
/// `git_fetcher::find_skill_dir`: deep enough for `.agents/skills/<name>`, and
/// shallow enough that a repository holding many trees stays a cheap walk.
const MAX_SCAN_DEPTH: usize = 6;

/// Why a directory was proposed as the skill's new home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveEvidence {
    /// The directory's content hash equals the recorded one: same bytes, so
    /// only the path changed.
    ContentHash,
    /// Only the directory name matches. It may be the same skill, or a
    /// different one that happens to share a name — the user decides.
    DirectoryName,
}

/// One place the skill might have moved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveCandidate {
    /// Relative to the checkout root, `/`-separated, no leading `./` and no
    /// trailing slash — the form that goes straight back into `source_subpath`.
    pub subpath: String,
    pub evidence: MoveEvidence,
}

/// The three answers, kept apart because each asks for a different action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// Still where the record says. `subpath` is empty when the skill sits at
    /// the checkout root.
    At { subpath: String },
    /// Somewhere else in this checkout. Never applied automatically: changing
    /// the source pointer is the user's decision.
    Moved { candidates: Vec<MoveCandidate> },
    /// Nowhere in this checkout.
    Gone,
}

/// The skill directories a checkout holds, walked once.
///
/// A scan asks about every skill of a Source, and each question would otherwise
/// re-walk and re-hash the same tree. Building this once is what makes "one
/// clone per Source" also mean one walk per Source (#655).
pub struct CheckoutIndex {
    checkout: PathBuf,
    skills: Vec<PathBuf>,
}

impl CheckoutIndex {
    pub fn build(checkout: &Path) -> Self {
        Self {
            checkout: checkout.to_path_buf(),
            skills: skill_dirs(checkout).map(|entry| entry.into_path()).collect(),
        }
    }
}

/// Where does `skill` sit in `checkout`?
///
/// Never fails: "not found" is [`Placement::Gone`], not an error, so a caller
/// cannot mistake a scan for a failed one.
pub fn locate(skill: &SkillRecord, checkout: &Path) -> Placement {
    locate_in(&CheckoutIndex::build(checkout), skill)
}

/// [`locate`], against a tree already walked.
pub fn locate_in(index: &CheckoutIndex, skill: &SkillRecord) -> Placement {
    let checkout = index.checkout.as_path();
    if let Some(subpath) = stored_location(skill, checkout) {
        return Placement::At { subpath };
    }

    let (by_hash, by_name) = scan(skill, checkout, &index.skills);
    // Content is evidence; a shared name is a guess that has to be shown, not
    // acted on. When both exist, only the evidence is worth the user's time.
    let candidates = if by_hash.is_empty() { by_name } else { by_hash };
    if candidates.is_empty() {
        Placement::Gone
    } else {
        Placement::Moved { candidates }
    }
}

/// The recorded path, when it still resolves to a skill directory.
///
/// "Resolves to a skill" is stricter than "exists": a Source that reorganized
/// itself can leave the recorded path occupied by a container that merely holds
/// skills, and treating that as `At` would hand the updater a directory that is
/// not the skill (#367).
fn stored_location(skill: &SkillRecord, checkout: &Path) -> Option<String> {
    let subpath = normalized_subpath(skill.source_subpath.as_deref())?;
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

/// Every skill directory in the checkout, split by the evidence that picked it.
fn scan(
    skill: &SkillRecord,
    checkout: &Path,
    skill_dirs: &[PathBuf],
) -> (Vec<MoveCandidate>, Vec<MoveCandidate>) {
    let mut by_hash = Vec::new();
    let mut by_name = Vec::new();
    for path in skill_dirs {
        let path = path.as_path();
        if let Some(hash) = skill.content_hash.as_deref() {
            if content_hash::hash_directory(path).is_ok_and(|live| live == hash) {
                by_hash.push(candidate(checkout, path, MoveEvidence::ContentHash));
                continue;
            }
        }
        let file_name = path.file_name().map(|name| name.to_string_lossy().to_string());
        if file_name.as_deref() == Some(skill.name.as_str()) {
            by_name.push(candidate(checkout, path, MoveEvidence::DirectoryName));
        }
    }
    by_hash.sort_by(|a, b| a.subpath.cmp(&b.subpath));
    by_name.sort_by(|a, b| a.subpath.cmp(&b.subpath));
    (by_hash, by_name)
}

fn skill_dirs(checkout: &Path) -> impl Iterator<Item = DirEntry> {
    WalkDir::new(checkout)
        .max_depth(MAX_SCAN_DEPTH)
        .into_iter()
        .flatten()
        .filter(|entry| entry.file_type().is_dir())
        // Version control metadata is always present and never holds a skill.
        .filter(|entry| entry.file_name() != ".git")
        .filter(|entry| skill_metadata::is_valid_skill_dir(entry.path()))
}

fn candidate(checkout: &Path, path: &Path, evidence: MoveEvidence) -> MoveCandidate {
    MoveCandidate {
        subpath: subpath::relative_to(checkout, path).unwrap_or_default(),
        evidence,
    }
}

/// A stored subpath in the form this module compares and returns.
///
/// `None` only for a path that can never be a location inside the checkout:
/// absolute, or climbing out of it.
fn normalized_subpath(subpath: Option<&str>) -> Option<String> {
    let raw = subpath.unwrap_or("").trim();
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return None;
    }
    if raw.is_empty() {
        return Some(String::new());
    }
    Some(raw.trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests;
