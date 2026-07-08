//! Pending-conflict machinery (design §4, §11-4/5): the source of truth for
//! "needs attention" is the commit history's `Skills-Manager-Conflicts:` /
//! `Skills-Manager-Resolved:` trailers, replayed in topological order. The
//! hidden refs under `refs/skills-manager/` only pin theirs-side objects
//! against GC and record where the theirs version lives; the SQLite table is
//! a rebuildable UI projection.

use anyhow::{Context, Result};
use git2::{Oid, Repository, Sort};
use std::collections::BTreeMap;

use super::decision::Side;
use super::protocol::{TRAILER_CONFLICTS, TRAILER_RESOLVED, parse_trailer_ids};
use super::snapshot::{Snapshot, skill_identical};

pub const REF_PRE_MERGE: &str = "refs/skills-manager/pre-merge";
pub const REF_APPLYING: &str = "refs/skills-manager/applying";
pub const CONFLICT_REF_PREFIX: &str = "refs/skills-manager/conflict/";
pub const STAGING_REF_PREFIX: &str = "refs/skills-manager/conflict-staging/";

/// Days after which a stale pre-merge safety ref is dropped at startup.
pub const PRE_MERGE_RETENTION_DAYS: i64 = 30;

pub fn conflict_ref(skill_id: &str) -> String {
    format!("{CONFLICT_REF_PREFIX}{skill_id}")
}

pub fn staging_ref(attempt_id: &str, skill_id: &str) -> String {
    format!("{STAGING_REF_PREFIX}{attempt_id}/{skill_id}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrailerState {
    Active,
    Closed,
}

/// Replay conflict trailers over `hide..tip` (or the whole history up to
/// `tip` when `hide` is `None`) in topological oldest→newest order (§11-5).
pub fn replay_trailers(
    repo: &Repository,
    tip: Oid,
    hide: Option<Oid>,
) -> Result<BTreeMap<String, TrailerState>> {
    let mut walk = repo.revwalk()?;
    walk.push(tip)?;
    if let Some(hide) = hide {
        walk.hide(hide)?;
    }
    walk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)?;
    let mut state = BTreeMap::new();
    for oid in walk {
        let commit = repo.find_commit(oid?)?;
        let message = commit.message().unwrap_or_default();
        for id in parse_trailer_ids(message, TRAILER_CONFLICTS) {
            state.insert(id, TrailerState::Active);
        }
        for id in parse_trailer_ids(message, TRAILER_RESOLVED) {
            state.insert(id, TrailerState::Closed);
        }
    }
    Ok(state)
}

#[derive(Debug)]
pub enum PendingOutcome {
    /// Declared-pending skills and the side whose version is pinned.
    Pinned(BTreeMap<String, Side>),
    /// Both sides independently declared the same skill pending with
    /// different versions — the merge must stop and go manual (§4).
    Blocked(Vec<String>),
}

/// Combine the trailer state of both sides into the declared-pending pin set
/// (§11-4). Implements:
/// - declared on one side only → pin that side;
/// - shared (pre-base) declaration, untouched → pin ours (the local kept
///   version), the theirs pointer just advances;
/// - `Closed` + inherited `Active` → the resolution wins, not pending; the
///   resolving side's tree change merges as a normal edit;
/// - `Closed` + re-declared `Active` → the newer declaration stays pending;
/// - independently declared on both sides: identical versions pin ours,
///   different versions block.
pub fn effective_pending(
    repo: &Repository,
    base: Oid,
    ours_tip: Oid,
    theirs_tip: Oid,
    ours_snap: &Snapshot,
    theirs_snap: &Snapshot,
) -> Result<PendingOutcome> {
    let full_ours = replay_trailers(repo, ours_tip, None)?;
    let full_theirs = replay_trailers(repo, theirs_tip, None)?;
    let range_ours = replay_trailers(repo, ours_tip, Some(base))?;
    let range_theirs = replay_trailers(repo, theirs_tip, Some(base))?;

    let mut pinned: BTreeMap<String, Side> = BTreeMap::new();
    let mut blocked: Vec<String> = Vec::new();

    let ids: std::collections::BTreeSet<&String> =
        full_ours.keys().chain(full_theirs.keys()).collect();
    for id in ids {
        let a = full_ours.get(id).copied();
        let b = full_theirs.get(id).copied();
        let declared_a = range_ours.get(id) == Some(&TrailerState::Active);
        let declared_b = range_theirs.get(id) == Some(&TrailerState::Active);
        match (a, b) {
            (Some(TrailerState::Active), Some(TrailerState::Active)) => {
                if declared_a && declared_b {
                    let same = match (ours_snap.skills.get(id), theirs_snap.skills.get(id)) {
                        (Some(o), Some(t)) => skill_identical(o, t),
                        (None, None) => true,
                        _ => false,
                    };
                    if same {
                        pinned.insert(id.clone(), Side::Ours);
                    } else {
                        blocked.push(id.clone());
                    }
                } else if declared_b {
                    pinned.insert(id.clone(), Side::Theirs);
                } else {
                    // declared in ours range, or shared pre-base declaration
                    pinned.insert(id.clone(), Side::Ours);
                }
            }
            (Some(TrailerState::Active), None) => {
                pinned.insert(id.clone(), Side::Ours);
            }
            (None, Some(TrailerState::Active)) => {
                pinned.insert(id.clone(), Side::Theirs);
            }
            (Some(TrailerState::Active), Some(TrailerState::Closed)) => {
                if declared_a {
                    pinned.insert(id.clone(), Side::Ours);
                }
            }
            (Some(TrailerState::Closed), Some(TrailerState::Active)) => {
                if declared_b {
                    pinned.insert(id.clone(), Side::Theirs);
                }
            }
            _ => {} // closed or never declared on both sides
        }
    }

    if blocked.is_empty() {
        Ok(PendingOutcome::Pinned(pinned))
    } else {
        blocked.sort();
        Ok(PendingOutcome::Blocked(blocked))
    }
}

// ── hidden refs ──

pub fn write_ref(repo: &Repository, name: &str, target: Oid, log: &str) -> Result<()> {
    repo.reference(name, target, true, log)
        .with_context(|| format!("failed to write {name}"))?;
    Ok(())
}

pub fn delete_ref(repo: &Repository, name: &str) {
    if let Ok(mut r) = repo.find_reference(name) {
        let _ = r.delete();
    }
}

pub fn ref_target(repo: &Repository, name: &str) -> Option<Oid> {
    repo.find_reference(name).ok().and_then(|r| r.target())
}

/// All staging refs, as (attempt_id, skill_id, target).
pub fn list_staging_refs(repo: &Repository) -> Vec<(String, String, Oid)> {
    let mut out = Vec::new();
    if let Ok(refs) = repo.references_glob(&format!("{STAGING_REF_PREFIX}*")) {
        for r in refs.flatten() {
            let Some(name) = r.name() else { continue };
            let Some(rest) = name.strip_prefix(STAGING_REF_PREFIX) else { continue };
            let Some((attempt, skill_id)) = rest.split_once('/') else { continue };
            if let Some(target) = r.target() {
                out.push((attempt.to_string(), skill_id.to_string(), target));
            }
        }
    }
    out
}

/// All promoted conflict refs, as (skill_id, target).
pub fn list_conflict_refs(repo: &Repository) -> Vec<(String, Oid)> {
    let mut out = Vec::new();
    if let Ok(refs) = repo.references_glob(&format!("{CONFLICT_REF_PREFIX}*")) {
        for r in refs.flatten() {
            let Some(name) = r.name() else { continue };
            let Some(skill_id) = name.strip_prefix(CONFLICT_REF_PREFIX) else { continue };
            if let Some(target) = r.target() {
                out.push((skill_id.to_string(), target));
            }
        }
    }
    out
}

/// Step 11 (§4/§5): promote this attempt's staging refs to durable conflict
/// refs and delete every remaining staging ref (ghosts from crashed
/// attempts included).
pub fn promote_staging(repo: &Repository, attempt_id: &str) -> Result<()> {
    for (attempt, skill_id, target) in list_staging_refs(repo) {
        if attempt == attempt_id {
            write_ref(repo, &conflict_ref(&skill_id), target, "promote conflict ref")?;
        }
        delete_ref(repo, &staging_ref(&attempt, &skill_id));
    }
    Ok(())
}

/// Startup recovery for pending refs (§4 启动恢复协议): make sure every id
/// declared active by HEAD's history has a durable conflict ref pointing at
/// (or past) its **current** declaration — promoting from staging when
/// possible, else rebuilding from the declaring merge commit's second parent
/// (which also makes freshly-cloned devices able to resolve with the remote
/// version). An existing ref that merely advanced beyond the declaration
/// (仅推进对端指针) is kept; one left over from a superseded declaration
/// (resolved, then re-declared while this device was offline) is rewritten —
/// otherwise "use remote" would apply the pre-resolution version. Ghost
/// staging refs are then removed.
pub fn heal_conflict_refs(repo: &Repository, head: Oid) -> Result<Vec<(String, Oid)>> {
    let active: Vec<String> = replay_trailers(repo, head, None)?
        .into_iter()
        .filter(|(_, s)| *s == TrailerState::Active)
        .map(|(id, _)| id)
        .collect();

    let staging = list_staging_refs(repo);
    let mut healed = Vec::new();
    for id in &active {
        let desired = staging
            .iter()
            .find(|(_, skill_id, _)| skill_id == id)
            .map(|(_, _, t)| *t)
            .or_else(|| find_declaring_theirs(repo, head, id));
        let Some(desired) = desired else { continue };
        let up_to_date = match ref_target(repo, &conflict_ref(id)) {
            None => false,
            Some(existing) if existing == desired => true,
            // A pointer that descends from the current declaration's theirs
            // side was legitimately advanced; anything else is stale.
            Some(existing) => repo.graph_descendant_of(existing, desired).unwrap_or(false),
        };
        if !up_to_date {
            write_ref(repo, &conflict_ref(id), desired, "heal conflict ref")?;
            healed.push((id.clone(), desired));
        }
    }
    for (attempt, skill_id, _) in staging {
        delete_ref(repo, &staging_ref(&attempt, &skill_id));
    }
    // Conflict refs for ids that are no longer active are stale — drop them
    // so the projection cannot resurrect resolved conflicts.
    let active_set: std::collections::BTreeSet<&String> = active.iter().collect();
    for (skill_id, _) in list_conflict_refs(repo) {
        if !active_set.contains(&skill_id) {
            delete_ref(repo, &conflict_ref(&skill_id));
        }
    }
    Ok(healed)
}

/// Newest commit in HEAD's history whose Conflicts trailer declares `id`;
/// its second parent is the theirs side at declaration time.
fn find_declaring_theirs(repo: &Repository, head: Oid, id: &str) -> Option<Oid> {
    let mut walk = repo.revwalk().ok()?;
    walk.push(head).ok()?;
    walk.set_sorting(Sort::TOPOLOGICAL).ok()?;
    for oid in walk.flatten() {
        let commit = repo.find_commit(oid).ok()?;
        let message = commit.message().unwrap_or_default();
        if parse_trailer_ids(message, TRAILER_CONFLICTS)
            .iter()
            .any(|declared| declared == id)
        {
            return commit.parent_id(1).ok();
        }
    }
    None
}
