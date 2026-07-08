//! Atomic application of the object merge (design §5) plus the surrounding
//! pipeline: preconditions, old-client detection (§6), fast-forward guard,
//! crash-safe ref choreography and the startup recovery protocol.

use anyhow::{Context, Result, bail};
use git2::{Oid, Repository, Sort};
use std::collections::BTreeMap;
use std::path::Path;

use super::decision::{self, DecisionInput, MergePlan, Side, Touch};
use super::pending::{
    self, PendingOutcome, REF_APPLYING, REF_PRE_MERGE, TrailerState, conflict_ref,
};
use super::protocol::{self, ProtocolFile};
use super::snapshot::{self, FileEntry, METADATA_DIR, Snapshot, skill_identical};
use super::treebuild::{TreeEdit, apply_tree_edits};
use super::validate::{self, validate_merged_tree};
use crate::core::skill_store::{PendingConflictRow, SkillStore};
use crate::core::sync_metadata;
use crate::core::{git_backup, repo_lock::RepoLock};

/// Human-readable outcome of a sync merge (§4.5/§8). Returned to the
/// frontend by the pull command; commit messages only carry the count line.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct MergeSummary {
    /// "object" or "system" (legacy fallback keeps the old line-merge path).
    pub engine: String,
    pub up_to_date: bool,
    pub fast_forward: bool,
    pub updated: Vec<UpdatedSkill>,
    /// Skill ids whose local version was kept over a conflicting remote one.
    pub kept_local: Vec<String>,
    /// Skill ids newly declared "needs attention" by this merge.
    pub new_conflicts: Vec<String>,
    /// Total pendings (old + new) after the merge.
    pub pending_total: usize,
    /// Set when an old-client write was tolerated with a warning (§6).
    pub old_client_warning: Option<String>,
    pub legacy_fallback: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdatedSkill {
    pub skill_id: String,
    pub path: String,
    /// Device (commit author) that last touched this skill on the remote.
    pub from_device: String,
}

struct TouchInfo {
    time: i64,
    commit: String,
    author: String,
}

/// Object-merge pull (§5 流程). Caller holds the repo lock and has applied
/// the device identity; this function performs P1/P2, fetch, merge and the
/// atomic apply. Returns the merge summary.
pub fn object_merge_pull_unlocked(store: &SkillStore, skills_dir: &Path) -> Result<MergeSummary> {
    git_backup::ensure_no_interrupted_git_operation(skills_dir)?;

    // P1: project the DB to metadata files, then commit anything dirty.
    sync_metadata::write_all_from_db_unlocked(store)?;
    if git_backup::has_uncommitted_changes(skills_dir)? {
        git_backup::commit_all_unlocked(skills_dir, "backup")?;
    }
    // P2: the tree must now be clean (gitignored files aside — porcelain
    // does not list them).
    if git_backup::has_uncommitted_changes(skills_dir)? {
        bail!("working tree still dirty after pre-merge commit; aborting sync");
    }

    let branch = git_backup::current_branch(skills_dir);
    git_backup::fetch_branch(skills_dir, &branch)?;

    let repo = Repository::open(skills_dir).context("failed to open skills repository")?;
    let ours = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .context("repository has no HEAD commit")?;
    let theirs = repo
        .refname_to_id(&format!("refs/remotes/origin/{branch}"))
        .with_context(|| format!("origin/{branch} not found after fetch"))?;

    if theirs == ours {
        return Ok(up_to_date_summary(&repo, store));
    }
    let base = repo
        .merge_base(ours, theirs)
        .context("no common history with the remote (unrelated histories)")?;
    if base == theirs {
        return Ok(up_to_date_summary(&repo, store));
    }

    // §6 legacy: a remote written only by pre-protocol clients keeps the
    // current line-merge behavior.
    let theirs_tree = repo.find_commit(theirs)?.tree()?;
    if theirs_tree
        .get_path(Path::new(protocol::PROTOCOL_FILE_REL))
        .is_err()
    {
        log::info!("object merge: remote is pre-protocol (legacy), falling back to git merge");
        git_backup::merge_branch_system(skills_dir, &branch)?;
        let pending_total = rebuild_pending_projection(&repo, store).unwrap_or(0);
        return Ok(MergeSummary {
            engine: "system".to_string(),
            legacy_fallback: true,
            pending_total,
            ..Default::default()
        });
    }

    // §6 mixed-write detection on both sides.
    let old_client_warning = check_old_client_writes(&repo, base, ours, theirs)?;

    let base_tree = repo.find_commit(base)?.tree()?;
    let ours_tree = repo.find_commit(ours)?.tree()?;
    let base_snap = snapshot::read_snapshot(&repo, &base_tree)
        .context("failed to read base snapshot")?;
    let ours_snap = snapshot::read_snapshot(&repo, &ours_tree)
        .context("failed to read local snapshot")?;
    let theirs_snap = snapshot::read_snapshot(&repo, &theirs_tree)
        .context("failed to read remote snapshot")?;

    // §4/§11-4: declared-pending set from trailers.
    let pinned = match pending::effective_pending(
        &repo, base, ours, theirs, &ours_snap, &theirs_snap,
    )? {
        PendingOutcome::Pinned(p) => p,
        PendingOutcome::Blocked(ids) => bail!(
            "SYNC_CONFLICT: skill(s) {} are pending on both devices with different versions; resolve the conflict on one device first",
            ids.join(", ")
        ),
    };

    // ff guard (§4): fast-forward unless theirs touches a locally-declared
    // pending skill.
    if base == ours {
        let ff_blocked = pinned.iter().any(|(id, side)| {
            *side == Side::Ours
                && match (ours_snap.skills.get(id), theirs_snap.skills.get(id)) {
                    (Some(o), Some(t)) => !skill_identical(o, t),
                    (None, None) => false,
                    _ => true,
                }
        });
        if !ff_blocked {
            return apply_fast_forward(
                store,
                &repo,
                &branch,
                ours,
                theirs,
                &ours_snap,
                &theirs_snap,
                old_client_warning,
            );
        }
        log::info!("object merge: ff blocked (remote touches pending skills), full merge");
    }

    // §3 decision.
    let ours_touch = touch_info_map(&repo, ours, base)?;
    let theirs_touch = touch_info_map(&repo, theirs, base)?;
    let ours_touch_simple: BTreeMap<String, Touch> = ours_touch
        .iter()
        .map(|(p, t)| (p.clone(), (t.time, t.commit.clone())))
        .collect();
    let theirs_touch_simple: BTreeMap<String, Touch> = theirs_touch
        .iter()
        .map(|(p, t)| (p.clone(), (t.time, t.commit.clone())))
        .collect();
    let plan = decision::decide(&DecisionInput {
        base: &base_snap,
        ours: &ours_snap,
        theirs: &theirs_snap,
        pinned: &pinned,
        ours_touch: &ours_touch_simple,
        theirs_touch: &theirs_touch_simple,
    })?;

    // §5 steps 4–5: build + validate the merged tree. Skill dirs that were
    // already unclaimed in either input tip (legacy dirt from old versions)
    // are grandfathered — the validator only rejects orphans the merge
    // itself would introduce.
    let edits = plan_to_edits(&repo, &ours_snap, &theirs_snap, &plan)?;
    let merged_tree_oid = apply_tree_edits(&repo, Some(&ours_tree), &edits)?;
    let merged_tree = repo.find_tree(merged_tree_oid)?;
    let mut tolerated = validate::unclaimed_skill_dirs(&repo, &ours_tree)?;
    tolerated.extend(validate::unclaimed_skill_dirs(&repo, &theirs_tree)?);
    validate_merged_tree(&repo, &merged_tree, &tolerated)
        .context("object merge aborted (zero changes)")?;

    // §5 忽略文件注: paths the checkout would create must not be shadowed by
    // untracked/ignored files on disk — explicit error, never a silent FORCE.
    let blockers = blocking_workdir_paths(&repo, skills_dir, &ours_tree, &merged_tree)?;
    if !blockers.is_empty() {
        bail!(
            "sync blocked: untracked or ignored files are in the way of incoming skills: {} — move or delete them and sync again",
            blockers.join(", ")
        );
    }

    // Trailer cap (§4): overflow blocks the automatic path.
    let conflict_ids: Vec<String> =
        plan.new_conflicts.iter().map(|c| c.skill_id.clone()).collect();
    let trailer = protocol::format_conflicts_trailer(&conflict_ids);
    if let Some((_, overflowed)) = &trailer {
        if *overflowed {
            bail!(
                "SYNC_CONFLICT: {} conflicting skills in one merge exceeds the automatic limit; resolve some conflicts manually first",
                conflict_ids.len()
            );
        }
    }

    // §5 steps 6–8: safety refs, staging refs, merge commit.
    pending::write_ref(&repo, REF_PRE_MERGE, ours, "pre-merge safety point")?;
    let attempt_id = uuid::Uuid::now_v7().to_string();
    for conflict in &plan.new_conflicts {
        pending::write_ref(
            &repo,
            &pending::staging_ref(&attempt_id, &conflict.skill_id),
            theirs,
            "conflict staging",
        )?;
    }

    let mut message = format!(
        "sync: merge remote skill changes ({} updated, {} kept local, {} conflicts)",
        plan.updated_from_theirs.len(),
        plan.kept_local.len(),
        plan.new_conflicts.len()
    );
    message = protocol::app_commit_message(&message);
    if let Some((line, _)) = &trailer {
        message.push('\n');
        message.push_str(line);
    }
    let sig = repo
        .signature()
        .or_else(|_| git2::Signature::now("Skills Manager", "skills-manager@local"))?;
    let ours_commit = repo.find_commit(ours)?;
    let theirs_commit = repo.find_commit(theirs)?;
    let merge_commit = repo.commit(
        None,
        &sig,
        &sig,
        &message,
        &merged_tree,
        &[&ours_commit, &theirs_commit],
    )?;

    // §5 steps 7'–11: applying marker, branch CAS, checkout, promote.
    finish_apply(&repo, &branch, ours, merge_commit, &merged_tree, &attempt_id)?;

    // §4 钉住: the local machine only advances the theirs pointer of
    // still-active pendings; freshly staged conflicts were just promoted to
    // the same theirs commit.
    for id in &plan.still_pending {
        pending::write_ref(&repo, &conflict_ref(id), theirs, "advance theirs pointer")?;
    }

    let pending_total = rebuild_pending_projection(&repo, store)?;
    let summary = build_summary(&plan, &theirs_snap, &theirs_touch, pending_total, old_client_warning);
    log::info!(
        "object merge: done ({} updated, {} kept local, {} new conflicts, {} pending)",
        summary.updated.len(),
        summary.kept_local.len(),
        summary.new_conflicts.len(),
        summary.pending_total
    );
    Ok(summary)
}

/// Shared tail of merge and fast-forward (§5 steps 7'–11): write the
/// applying marker, move the branch ref (CAS against the expected old
/// head), force the working tree onto the new commit (pre-checked — clean
/// tree, no blockers), then promote staging refs and clear the marker.
fn finish_apply(
    repo: &Repository,
    branch: &str,
    old_head: Oid,
    new_head: Oid,
    new_tree: &git2::Tree,
    attempt_id: &str,
) -> Result<()> {
    pending::write_ref(repo, REF_APPLYING, new_head, "object merge applying")?;

    let branch_ref = format!("refs/heads/{branch}");
    repo.reference_matching(
        &branch_ref,
        new_head,
        true,
        old_head,
        "skills-manager: object merge",
    )
    .context("branch moved while merging; aborting with no changes")?;

    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.force();
    if let Err(e) = repo.checkout_tree(new_tree.as_object(), Some(&mut checkout)) {
        // Roll the ref back and restore the (possibly partially checked-out)
        // working tree. The applying marker is only cleared once the
        // rollback checkout succeeded — otherwise it stays so the startup
        // recovery settles the working tree (§5 恢复协议).
        let _ = repo.reference(&branch_ref, old_head, true, "object merge rollback");
        let rolled_back = repo
            .find_commit(old_head)
            .and_then(|c| c.tree())
            .and_then(|old_tree| {
                let mut co = git2::build::CheckoutBuilder::new();
                co.force();
                repo.checkout_tree(old_tree.as_object(), Some(&mut co))
            });
        match rolled_back {
            Ok(()) => pending::delete_ref(repo, REF_APPLYING),
            Err(rollback_err) => log::error!(
                "object merge: rollback checkout also failed, leaving applying marker for startup recovery: {rollback_err}"
            ),
        }
        return Err(anyhow::Error::from(e).context("checkout of merged tree failed; rolled back"));
    }

    pending::promote_staging(repo, attempt_id)?;
    pending::delete_ref(repo, REF_APPLYING);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_fast_forward(
    store: &SkillStore,
    repo: &Repository,
    branch: &str,
    ours: Oid,
    theirs: Oid,
    ours_snap: &Snapshot,
    theirs_snap: &Snapshot,
    old_client_warning: Option<String>,
) -> Result<MergeSummary> {
    let skills_dir = repo
        .workdir()
        .context("skills repository has no working directory")?;
    let ours_tree = repo.find_commit(ours)?.tree()?;
    let theirs_tree = repo.find_commit(theirs)?.tree()?;

    let blockers = blocking_workdir_paths(repo, skills_dir, &ours_tree, &theirs_tree)?;
    if !blockers.is_empty() {
        bail!(
            "sync blocked: untracked or ignored files are in the way of incoming skills: {} — move or delete them and sync again",
            blockers.join(", ")
        );
    }

    pending::write_ref(repo, REF_PRE_MERGE, ours, "pre-merge safety point")?;
    let attempt_id = uuid::Uuid::now_v7().to_string(); // no staging refs; promote is a no-op
    finish_apply(repo, branch, ours, theirs, &theirs_tree, &attempt_id)?;

    // The remote history may itself declare or resolve conflicts — make the
    // durable refs match HEAD's trailer state.
    pending::heal_conflict_refs(repo, theirs)?;
    let pending_total = rebuild_pending_projection(repo, store)?;

    let theirs_touch = touch_info_map(repo, theirs, ours)?;
    let mut updated = Vec::new();
    for (id, t) in &theirs_snap.skills {
        let changed = match ours_snap.skills.get(id) {
            Some(o) => !skill_identical(o, t),
            None => true,
        };
        if changed {
            updated.push(UpdatedSkill {
                skill_id: id.clone(),
                path: t.meta.path.clone(),
                from_device: device_for_skill(&theirs_touch, id, Some(&t.meta.path)),
            });
        }
    }
    for id in ours_snap.skills.keys() {
        if !theirs_snap.skills.contains_key(id) {
            updated.push(UpdatedSkill {
                skill_id: id.clone(),
                path: ours_snap.skills[id].meta.path.clone(),
                from_device: device_for_skill(&theirs_touch, id, None),
            });
        }
    }

    Ok(MergeSummary {
        engine: "object".to_string(),
        fast_forward: true,
        updated,
        pending_total,
        old_client_warning,
        ..Default::default()
    })
}

fn up_to_date_summary(repo: &Repository, store: &SkillStore) -> MergeSummary {
    let pending_total = rebuild_pending_projection(repo, store).unwrap_or(0);
    MergeSummary {
        engine: "object".to_string(),
        up_to_date: true,
        pending_total,
        ..Default::default()
    }
}

// ── §6 old-client detection ──

struct Violation {
    sha: String,
    author: String,
    time: String,
    double_parent: bool,
}

fn check_old_client_writes(
    repo: &Repository,
    base: Oid,
    ours: Oid,
    theirs: Oid,
) -> Result<Option<String>> {
    let ours_violations = scan_old_client(repo, base, ours)?;
    let theirs_violations = scan_old_client(repo, base, theirs)?;
    if ours_violations.is_empty() && theirs_violations.is_empty() {
        return Ok(None);
    }

    let describe = |v: &Violation| format!("{} ({}, {})", v.sha, v.author, v.time);
    let doubles: Vec<String> = ours_violations
        .iter()
        .chain(&theirs_violations)
        .filter(|v| v.double_parent)
        .map(describe)
        .collect();
    if !doubles.is_empty() {
        bail!(
            "sync blocked: an old skills-manager version performed a line-level merge on another device — commit(s) {}. Upgrade that device and retry, or use the recovery flow (a safety snapshot is taken first)",
            doubles.join("; ")
        );
    }

    // Single-parent old-client writes: allow if the affected side's tip still
    // passes structural validation and no SKILL.md carries conflict markers.
    for (label, tip, violated) in [
        ("local", ours, !ours_violations.is_empty()),
        ("remote", theirs, !theirs_violations.is_empty()),
    ] {
        if !violated {
            continue;
        }
        let tree = repo.find_commit(tip)?.tree()?;
        // Legacy unclaimed dirs and committed metadata junk in the tip are
        // tolerated here for the same reason as in the merge itself; the
        // structural checks still apply.
        let tolerated = validate::unclaimed_skill_dirs(repo, &tree)?;
        validate::validate_input_tip(repo, &tree, &tolerated).with_context(|| {
            format!(
                "sync blocked: an old skills-manager version wrote to the {label} library and left it inconsistent — upgrade that device and repair via the recovery flow"
            )
        })?;
        let snap = snapshot::read_snapshot(repo, &tree)?;
        let markers = scan_conflict_markers(repo, &snap)?;
        if !markers.is_empty() {
            bail!(
                "sync blocked: conflict markers found in {} (written by an old version): {}",
                label,
                markers.join(", ")
            );
        }
    }

    let listed: Vec<String> = ours_violations
        .iter()
        .chain(&theirs_violations)
        .take(3)
        .map(describe)
        .collect();
    Ok(Some(format!(
        "old skills-manager version wrote commit(s) {} — please upgrade that device",
        listed.join("; ")
    )))
}

fn scan_old_client(repo: &Repository, base: Oid, tip: Oid) -> Result<Vec<Violation>> {
    let mut walk = repo.revwalk()?;
    walk.push(tip)?;
    walk.hide(base)?;
    let mut out = Vec::new();
    for oid in walk {
        let commit = repo.find_commit(oid?)?;
        let tree = commit.tree()?;
        let has_marker = tree.get_path(Path::new(protocol::PROTOCOL_FILE_REL)).is_ok();
        let has_trailer = protocol::has_protocol_trailer(commit.message().unwrap_or_default());
        if has_marker && !has_trailer {
            out.push(Violation {
                sha: commit.id().to_string()[..7].to_string(),
                author: commit.author().name().unwrap_or("unknown").to_string(),
                time: chrono::DateTime::from_timestamp(commit.time().seconds(), 0)
                    .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default(),
                double_parent: commit.parent_count() > 1,
            });
        }
    }
    Ok(out)
}

fn scan_conflict_markers(repo: &Repository, snap: &Snapshot) -> Result<Vec<String>> {
    let mut hits = Vec::new();
    for skill in snap.skills.values() {
        let Some(content) = skill.content else { continue };
        let tree = repo.find_tree(content)?;
        for marker in snapshot::SKILL_DIR_MARKERS {
            if let Some(entry) = tree.get_name(marker) {
                if let Ok(blob) = repo.find_blob(entry.id()) {
                    let content = blob.content();
                    if content.starts_with(b"<<<<<<< ")
                        || content
                            .windows(9)
                            .any(|w| w == b"\n<<<<<<< ")
                    {
                        hits.push(format!("{}/{}", skill.meta.path, marker));
                    }
                }
            }
        }
    }
    Ok(hits)
}

// ── last-touch attribution ──

/// Newest-first per-path info over `hide..tip`: which commit last touched a
/// path, when, and by whom. Merge commits are diffed against their first
/// parent (attribution approximation).
fn touch_info_map(repo: &Repository, tip: Oid, hide: Oid) -> Result<BTreeMap<String, TouchInfo>> {
    let mut walk = repo.revwalk()?;
    walk.push(tip)?;
    walk.hide(hide)?;
    walk.set_sorting(Sort::TOPOLOGICAL)?;
    let mut out: BTreeMap<String, TouchInfo> = BTreeMap::new();
    for oid in walk {
        let commit = repo.find_commit(oid?)?;
        let tree = commit.tree()?;
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
        let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
        for delta in diff.deltas() {
            for file in [delta.new_file(), delta.old_file()] {
                let Some(path) = file.path().and_then(|p| p.to_str()) else { continue };
                out.entry(path.to_string()).or_insert_with(|| TouchInfo {
                    time: commit.time().seconds(),
                    commit: commit.id().to_string(),
                    author: commit.author().name().unwrap_or("").to_string(),
                });
            }
        }
    }
    Ok(out)
}

fn device_for_skill(
    touch: &BTreeMap<String, TouchInfo>,
    skill_id: &str,
    theirs_path: Option<&str>,
) -> String {
    let meta_path = format!("{METADATA_DIR}/skills/{skill_id}.json");
    if let Some(info) = touch.get(&meta_path) {
        return info.author.clone();
    }
    if let Some(prefix) = theirs_path {
        let dir_prefix = format!("{prefix}/");
        if let Some(info) = touch
            .iter()
            .find(|(p, _)| p.as_str() == prefix || p.starts_with(&dir_prefix))
            .map(|(_, i)| i)
        {
            return info.author.clone();
        }
    }
    String::new()
}

// ── plan → tree edits ──

fn plan_to_edits(
    repo: &Repository,
    ours: &Snapshot,
    theirs: &Snapshot,
    plan: &MergePlan,
) -> Result<BTreeMap<String, TreeEdit>> {
    let mut edits: BTreeMap<String, TreeEdit> = BTreeMap::new();

    // Removals first; puts inserted afterwards overwrite same-path removes.
    for (id, ours_skill) in &ours.skills {
        let keep = plan.skills.get(id);
        let path_kept = keep.map(|p| p.meta.path == ours_skill.meta.path).unwrap_or(false);
        if !path_kept {
            edits.insert(ours_skill.meta.path.clone(), TreeEdit::Remove);
        }
        if keep.is_none() {
            edits.insert(skill_meta_path(id), TreeEdit::Remove);
        }
    }
    for (id, planned) in &plan.skills {
        let content = planned
            .content
            .with_context(|| format!("skill {id} has no content directory to merge"))?;
        let ours_skill = ours.skills.get(id);
        let same_content_in_place = ours_skill
            .map(|o| o.meta.path == planned.meta.path && o.content == Some(content))
            .unwrap_or(false);
        if !same_content_in_place {
            edits.insert(planned.meta.path.clone(), TreeEdit::PutTree { oid: content });
        }
        let bytes = sync_metadata::canonical_json_bytes(&planned.meta)?;
        let blob = repo.blob(&bytes)?;
        let unchanged = ours_skill.map(|o| o.meta_entry.oid == blob).unwrap_or(false);
        if !unchanged {
            edits.insert(
                skill_meta_path(id),
                TreeEdit::PutBlob { oid: blob, mode: 0o100644 },
            );
        }
    }

    diff_file_maps(
        &mut edits,
        &ours.scenarios,
        &plan.scenarios,
        |id| format!("{METADATA_DIR}/scenarios/{id}.json"),
    );
    diff_file_maps(
        &mut edits,
        &ours.memberships,
        &plan.memberships,
        |(sid, skid)| format!("{METADATA_DIR}/scenario-skills/{sid}/{skid}.json"),
    );
    diff_file_maps(&mut edits, &ours.residual, &plan.residual, |p| p.clone());

    // protocol.json: larger merge_protocol wins; ties resolve to the smaller
    // blob OID so both devices pick the same bytes (§3).
    let protocol_pick = pick_versioned(
        ours.protocol.as_ref().map(|(e, p)| (*e, u64::from(p.merge_protocol))),
        theirs.protocol.as_ref().map(|(e, p)| (*e, u64::from(p.merge_protocol))),
    );
    match protocol_pick {
        Some(entry) => {
            if ours.protocol.as_ref().map(|(e, _)| *e) != Some(entry) {
                edits.insert(
                    protocol::PROTOCOL_FILE_REL.to_string(),
                    TreeEdit::PutBlob { oid: entry.oid, mode: entry.mode },
                );
            }
        }
        None => {
            let bytes = protocol::protocol_file_bytes(&ProtocolFile::default())?;
            let blob = repo.blob(&bytes)?;
            edits.insert(
                protocol::PROTOCOL_FILE_REL.to_string(),
                TreeEdit::PutBlob { oid: blob, mode: 0o100644 },
            );
        }
    }
    // schema.json: larger schema_version wins, same tie-break.
    if let Some(entry) = pick_versioned(ours.schema, theirs.schema) {
        if ours.schema.map(|(e, _)| e) != Some(entry) {
            edits.insert(
                format!("{METADATA_DIR}/schema.json"),
                TreeEdit::PutBlob { oid: entry.oid, mode: entry.mode },
            );
        }
    }

    Ok(edits)
}

fn skill_meta_path(id: &str) -> String {
    format!("{METADATA_DIR}/skills/{id}.json")
}

fn diff_file_maps<K: Ord>(
    edits: &mut BTreeMap<String, TreeEdit>,
    ours: &BTreeMap<K, FileEntry>,
    planned: &BTreeMap<K, FileEntry>,
    to_path: impl Fn(&K) -> String,
) {
    for key in ours.keys() {
        if !planned.contains_key(key) {
            edits.insert(to_path(key), TreeEdit::Remove);
        }
    }
    for (key, entry) in planned {
        if ours.get(key) != Some(entry) {
            edits.insert(to_path(key), TreeEdit::PutBlob { oid: entry.oid, mode: entry.mode });
        }
    }
}

fn pick_versioned(
    ours: Option<(FileEntry, u64)>,
    theirs: Option<(FileEntry, u64)>,
) -> Option<FileEntry> {
    match (ours, theirs) {
        (None, None) => None,
        (Some((e, _)), None) => Some(e),
        (None, Some((e, _))) => Some(e),
        (Some((oe, ov)), Some((te, tv))) => {
            if ov > tv {
                Some(oe)
            } else if tv > ov {
                Some(te)
            } else if oe == te || oe.oid.as_bytes() <= te.oid.as_bytes() {
                Some(oe)
            } else {
                Some(te)
            }
        }
    }
}

// ── checkout pre-check (§5 忽略文件注 / §11-6) ──

/// Paths the target tree adds relative to `ours` that already exist on disk
/// (necessarily untracked or ignored — tracked paths live in `ours`). A
/// FORCE checkout would silently overwrite them, so the merge refuses.
fn blocking_workdir_paths(
    repo: &Repository,
    skills_dir: &Path,
    ours_tree: &git2::Tree,
    target_tree: &git2::Tree,
) -> Result<Vec<String>> {
    let diff = repo.diff_tree_to_tree(Some(ours_tree), Some(target_tree), None)?;
    let mut blockers = Vec::new();
    for delta in diff.deltas() {
        if delta.status() != git2::Delta::Added {
            continue;
        }
        let Some(path) = delta.new_file().path() else { continue };
        if skills_dir.join(path).exists() {
            blockers.push(path.to_string_lossy().to_string());
        }
    }
    Ok(blockers)
}

// ── pending projection (§4 SQLite 投影) ──

/// Rebuild the pending_conflicts table from HEAD's trailer state plus the
/// conflict refs. Returns the number of active pendings.
pub fn rebuild_pending_projection(repo: &Repository, store: &SkillStore) -> Result<usize> {
    let head = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .context("repository has no HEAD commit")?;
    let active: Vec<String> = pending::replay_trailers(repo, head, None)?
        .into_iter()
        .filter(|(_, s)| *s == TrailerState::Active)
        .map(|(id, _)| id)
        .collect();

    let existing: BTreeMap<String, PendingConflictRow> = store
        .list_pending_conflicts()?
        .into_iter()
        .map(|row| (row.skill_id.clone(), row))
        .collect();

    let mut rows = Vec::new();
    for id in active {
        let Some(theirs_commit) = pending::ref_target(repo, &conflict_ref(&id)) else {
            // No pinned object (e.g. ref lost and unreconstructable): the
            // trailer still marks it pending; surface without a commit.
            rows.push(PendingConflictRow {
                skill_id: id.clone(),
                theirs_commit: String::new(),
                theirs_path: existing.get(&id).and_then(|r| r.theirs_path.clone()),
                detected_at: existing
                    .get(&id)
                    .map(|r| r.detected_at)
                    .unwrap_or_else(|| chrono::Utc::now().timestamp_millis()),
            });
            continue;
        };
        let theirs_path = theirs_skill_path(repo, theirs_commit, &id);
        rows.push(PendingConflictRow {
            skill_id: id.clone(),
            theirs_commit: theirs_commit.to_string(),
            theirs_path,
            detected_at: existing
                .get(&id)
                .map(|r| r.detected_at)
                .unwrap_or_else(|| chrono::Utc::now().timestamp_millis()),
        });
    }
    let total = rows.len();
    store.replace_pending_conflicts(&rows)?;
    Ok(total)
}

/// Whether origin/&lt;branch&gt; changes any skill that is pending locally —
/// the narrowed damping gate of the automatic sync round (§4 收窄阻尼):
/// unrelated remote updates flow automatically, but while a touched skill
/// awaits a decision the round applies deliberate backpressure. Errors
/// reading the remote tree count as "touched" (pause; the manual sync
/// surfaces the real problem).
pub fn remote_touches_pending(store: &SkillStore, skills_dir: &Path) -> Result<bool> {
    let pending = store.list_pending_conflicts()?;
    if pending.is_empty() {
        return Ok(false);
    }
    let repo = Repository::open(skills_dir)?;
    let branch = git_backup::current_branch(skills_dir);
    let Ok(theirs) = repo.refname_to_id(&format!("refs/remotes/origin/{branch}")) else {
        return Ok(false); // no remote branch — nothing to touch anything
    };
    let head = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .context("repository has no HEAD commit")?;
    if theirs == head {
        return Ok(false);
    }
    let head_snap = snapshot::read_snapshot(&repo, &repo.find_commit(head)?.tree()?)?;
    let theirs_snap = match snapshot::read_snapshot(&repo, &repo.find_commit(theirs)?.tree()?) {
        Ok(snap) => snap,
        Err(e) => {
            log::warn!("auto sync damping: cannot read remote snapshot, pausing: {e:#}");
            return Ok(true);
        }
    };
    Ok(pending.iter().any(|row| {
        match (head_snap.skills.get(&row.skill_id), theirs_snap.skills.get(&row.skill_id)) {
            (Some(o), Some(t)) => !skill_identical(o, t),
            (None, None) => false,
            _ => true,
        }
    }))
}

/// The skill's path inside a pinned theirs commit, for display.
pub(crate) fn theirs_skill_path(repo: &Repository, commit: Oid, skill_id: &str) -> Option<String> {
    let tree = repo.find_commit(commit).ok()?.tree().ok()?;
    let entry = tree
        .get_path(Path::new(&skill_meta_path(skill_id)))
        .ok()?;
    let blob = repo.find_blob(entry.id()).ok()?;
    let meta: crate::core::sync_metadata::SkillMetaFile =
        serde_json::from_slice(blob.content()).ok()?;
    Some(meta.path)
}

fn build_summary(
    plan: &MergePlan,
    theirs_snap: &Snapshot,
    theirs_touch: &BTreeMap<String, TouchInfo>,
    pending_total: usize,
    old_client_warning: Option<String>,
) -> MergeSummary {
    let updated = plan
        .updated_from_theirs
        .iter()
        .map(|id| {
            let path = plan
                .skills
                .get(id)
                .map(|p| p.meta.path.clone())
                .or_else(|| theirs_snap.skills.get(id).map(|s| s.meta.path.clone()))
                .unwrap_or_default();
            let theirs_path = theirs_snap.skills.get(id).map(|s| s.meta.path.clone());
            UpdatedSkill {
                skill_id: id.clone(),
                path,
                from_device: device_for_skill(theirs_touch, id, theirs_path.as_deref()),
            }
        })
        .collect();
    MergeSummary {
        engine: "object".to_string(),
        updated,
        kept_local: plan.kept_local.clone(),
        new_conflicts: plan.new_conflicts.iter().map(|c| c.skill_id.clone()).collect(),
        pending_total,
        old_client_warning,
        ..Default::default()
    }
}

// ── startup recovery (§5 启动恢复协议) ──

/// Best-effort crash recovery, run once at startup before background work.
/// Never blocks startup: a busy lock or an error only logs.
pub fn recover_on_startup(store: &SkillStore, skills_dir: &Path) {
    if !skills_dir.join(".git").exists() {
        return;
    }
    let Ok(_lock) = RepoLock::acquire("merge recovery") else {
        return;
    };
    match recover_locked(store, skills_dir) {
        Ok(()) => {}
        Err(e) => log::warn!("merge recovery: {e:#}"),
    }
}

fn recover_locked(store: &SkillStore, skills_dir: &Path) -> Result<()> {
    let repo = Repository::open(skills_dir)?;
    let head = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .context("repository has no HEAD commit")?;

    if let Some(applying) = pending::ref_target(&repo, REF_APPLYING) {
        let old_head = pending::ref_target(&repo, REF_PRE_MERGE);
        match old_head {
            Some(old_head) if head == old_head => {
                // Crash between marker and branch move, or a crashed
                // rollback: the merge never took effect. The working tree
                // may still carry a partial checkout from the failed
                // attempt — settle it back onto HEAD (rescuing anything
                // that differs; debris and user edits made since the crash
                // cannot be told apart, so both are kept safe).
                log::info!("merge recovery: apply never landed, cleaning up");
                settle_worktree(&repo, old_head)?;
                pending::delete_ref(&repo, REF_APPLYING);
                pending::delete_ref(&repo, REF_PRE_MERGE);
                for (attempt, skill_id, _) in pending::list_staging_refs(&repo) {
                    pending::delete_ref(&repo, &pending::staging_ref(&attempt, &skill_id));
                }
            }
            Some(old_head) if head == applying => {
                // Crash between branch move and checkout/cleanup: replay.
                log::info!("merge recovery: replaying interrupted apply");
                replay_interrupted_apply(&repo, old_head, head)?;
            }
            _ => {
                let msg = "merge recovery: HEAD does not match either side of an interrupted sync; not touching anything — use the backup page recovery flow";
                log::error!("{msg}");
                let _ = store.set_setting(crate::core::auto_backup::SETTING_LAST_ERROR, msg);
                return Ok(());
            }
        }
    } else {
        // No interrupted apply: heal refs against HEAD trailers and clean
        // ghost staging refs; drop a stale pre-merge safety ref.
        pending::heal_conflict_refs(&repo, head)?;
        if let Some(pre) = pending::ref_target(&repo, REF_PRE_MERGE) {
            if let Ok(commit) = repo.find_commit(pre) {
                let age_days =
                    (chrono::Utc::now().timestamp() - commit.time().seconds()) / 86_400;
                if age_days > pending::PRE_MERGE_RETENTION_DAYS {
                    pending::delete_ref(&repo, REF_PRE_MERGE);
                }
            }
        }
    }

    rebuild_pending_projection(&repo, store)?;
    Ok(())
}

/// HEAD already points at the merge commit but the working tree may still be
/// the old one (§5 v3-R3 finding 2/3): if the tree was touched since the
/// crash, snapshot it to a rescue commit + user-visible tag first; then
/// finish checkout and step-11 cleanup.
fn replay_interrupted_apply(repo: &Repository, old_head: Oid, head: Oid) -> Result<()> {
    settle_worktree_from(repo, old_head, head)?;
    pending::heal_conflict_refs(repo, head)?;
    pending::delete_ref(repo, REF_APPLYING);
    Ok(())
}

/// Settle the working tree onto `target` after a crash: an untouched tree
/// (still exactly `expected_clean`, ignoring unrelated untracked files) is
/// force-checked-out directly; anything else — user edits or debris from a
/// partial checkout — is preserved first in a rescue commit + user-visible
/// snapshot tag. Never silently overwrites (§5 恢复不吞用户数据).
fn settle_worktree_from(repo: &Repository, expected_clean: Oid, target: Oid) -> Result<()> {
    let expected_tree = repo.find_commit(expected_clean)?.tree()?;
    let target_commit = repo.find_commit(target)?;
    let target_tree = target_commit.tree()?;

    if !worktree_matches_tree(repo, &expected_tree, &target_tree)? {
        let sig = repo
            .signature()
            .or_else(|_| git2::Signature::now("Skills Manager", "skills-manager@local"))?;
        let mut index = repo.index()?;
        index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
        let rescue_tree = repo.find_tree(index.write_tree()?)?;
        let rescue = repo.commit(
            None,
            &sig,
            &sig,
            &protocol::app_commit_message("rescue: working tree changed during interrupted sync"),
            &rescue_tree,
            &[&target_commit],
        )?;
        let tag = format!(
            "sm-v-{}-{}",
            chrono::Utc::now().format("%Y%m%d-%H%M%S"),
            &rescue.to_string()[..7]
        );
        repo.tag_lightweight(&tag, &repo.find_object(rescue, None)?, true)?;
        log::warn!("merge recovery: working tree preserved in rescue snapshot {tag}");
    }

    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.force();
    repo.checkout_tree(target_tree.as_object(), Some(&mut checkout))?;
    Ok(())
}

fn settle_worktree(repo: &Repository, target: Oid) -> Result<()> {
    settle_worktree_from(repo, target, target)
}

/// Whether the working tree exactly matches `expected`, considering only
/// paths that the replayed checkout would touch (union of both trees, §11-6)
/// so unrelated untracked/ignored files don't trigger a rescue snapshot.
fn worktree_matches_tree(
    repo: &Repository,
    expected: &git2::Tree,
    target: &git2::Tree,
) -> Result<bool> {
    let mut opts = git2::DiffOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(true)
        .recurse_ignored_dirs(true);
    let diff = repo.diff_tree_to_workdir(Some(expected), Some(&mut opts))?;
    for delta in diff.deltas() {
        for file in [delta.new_file(), delta.old_file()] {
            let Some(path) = file.path() else { continue };
            if expected.get_path(path).is_ok() || target.get_path(path).is_ok() {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
