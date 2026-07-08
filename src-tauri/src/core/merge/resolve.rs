//! Conflict resolution actions (design §4): keep local / use remote / keep
//! both. Each runs inside the repo lock, takes a user-visible safety
//! snapshot first, records the resolution with a `Skills-Manager-Resolved:`
//! trailer (the cross-device close signal), then drops the pinned ref and
//! projection row.

use anyhow::{Context, Result, bail};
use git2::{ObjectType, Oid, Repository};
use std::collections::BTreeSet;
use std::path::Path;

use super::apply::rebuild_pending_projection;
use super::pending::{self, conflict_ref};
use super::protocol::{self, TRAILER_RESOLVED};
use super::snapshot;
use crate::core::skill_store::SkillStore;
use crate::core::sync_metadata::{self, SkillMetaFile, path_key};
use crate::core::{git_backup, skill_metadata};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveAction {
    KeepLocal,
    UseRemote,
    KeepBoth,
}

impl ResolveAction {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "keep_local" => Some(Self::KeepLocal),
            "use_remote" => Some(Self::UseRemote),
            "keep_both" => Some(Self::KeepBoth),
            _ => None,
        }
    }
}

/// Resolve one pending conflict. Returns the safety snapshot tag taken
/// before any change. Caller holds the repo lock and reconciles the DB
/// afterwards.
pub fn resolve_conflict_unlocked(
    store: &SkillStore,
    skills_dir: &Path,
    skill_id: &str,
    action: ResolveAction,
) -> Result<String> {
    git_backup::ensure_no_interrupted_git_operation(skills_dir)?;

    // Protect the current state: flush the DB projection, commit anything
    // dirty, then take the user-visible safety snapshot (§4 先打快照 tag).
    sync_metadata::write_all_from_db_unlocked(store)?;
    if git_backup::has_uncommitted_changes(skills_dir)? {
        git_backup::commit_all_unlocked(skills_dir, "backup")?;
    }
    let safety_tag = git_backup::create_snapshot_tag_unlocked(skills_dir)?;

    let repo = Repository::open(skills_dir).context("failed to open skills repository")?;
    let theirs_commit = pending::ref_target(&repo, &conflict_ref(skill_id))
        .or_else(|| {
            store
                .list_pending_conflicts()
                .ok()?
                .into_iter()
                .find(|r| r.skill_id == skill_id)
                .and_then(|r| Oid::from_str(&r.theirs_commit).ok())
        });

    match action {
        ResolveAction::KeepLocal => {}
        ResolveAction::UseRemote => {
            let theirs_commit = theirs_commit
                .context("remote version of this conflict is no longer available")?;
            apply_use_remote(&repo, skills_dir, skill_id, theirs_commit)?;
        }
        ResolveAction::KeepBoth => {
            let theirs_commit = theirs_commit
                .context("remote version of this conflict is no longer available")?;
            apply_keep_both(&repo, skills_dir, skill_id, theirs_commit)?;
        }
    }

    let action_label = match action {
        ResolveAction::KeepLocal => "keep local",
        ResolveAction::UseRemote => "use remote",
        ResolveAction::KeepBoth => "keep both",
    };
    let message = format!(
        "{}\n{}: {}",
        protocol::app_commit_message(&format!("resolve conflict: {action_label}")),
        TRAILER_RESOLVED,
        skill_id
    );
    git_backup::commit_resolution_unlocked(skills_dir, &message)?;

    pending::delete_ref(&repo, &conflict_ref(skill_id));
    rebuild_pending_projection(&repo, store)?;
    log::info!("conflict resolution: {skill_id} → {action_label} (safety point {safety_tag})");
    Ok(safety_tag)
}

/// Replace the local version with the pinned theirs version: remove the
/// local content dir + metadata, extract theirs content at theirs path
/// (collision-adjusted against the other local skills), rebuild metadata.
fn apply_use_remote(
    repo: &Repository,
    skills_dir: &Path,
    skill_id: &str,
    theirs_commit: Oid,
) -> Result<()> {
    let theirs_tree = repo.find_commit(theirs_commit)?.tree()?;
    let theirs_snap = snapshot::read_snapshot(repo, &theirs_tree)
        .context("failed to read the pinned remote version")?;

    let local_meta = read_worktree_meta(skills_dir, skill_id)?;
    let meta_file = worktree_meta_path(skills_dir, skill_id);

    let Some(theirs_skill) = theirs_snap.skills.get(skill_id) else {
        // The remote side of this conflict was a deletion — adopting it means
        // deleting locally.
        if let Some(meta) = &local_meta {
            let dir = skills_dir.join(&meta.path);
            if dir.exists() {
                std::fs::remove_dir_all(&dir)?;
            }
        }
        if meta_file.exists() {
            std::fs::remove_file(&meta_file)?;
        }
        return Ok(());
    };
    let content = theirs_skill
        .content
        .context("pinned remote version has no content directory")?;

    // Stage the remote content OUTSIDE the library first: a mid-extraction
    // failure (disk full, permissions) must leave the local version intact
    // instead of a half-replaced skill that the next auto backup commits.
    let target_path = free_path_for(skills_dir, &theirs_skill.meta.path, skill_id)?;
    let staged = stage_skill_extract(repo, content, skills_dir)?;
    // Overlapping paths (equal, or one nested inside the other — e.g. local
    // `alpha` vs remote `alpha/beta`) must remove the local dir FIRST, or the
    // removal would take the freshly placed remote content with it. The
    // staged copy outside the library keeps the remote version safe either
    // way; only for fully disjoint paths is the local dir kept until the new
    // content has landed.
    let local_path = local_meta.as_ref().map(|meta| meta.path.as_str());
    let overlaps = local_path.map(|local| {
        local == target_path
            || target_path.starts_with(&format!("{local}/"))
            || local.starts_with(&format!("{target_path}/"))
    });
    if overlaps == Some(true) {
        if let Some(meta) = &local_meta {
            let dir = skills_dir.join(&meta.path);
            if dir.exists() {
                std::fs::remove_dir_all(&dir)?;
            }
        }
        place_staged(&staged, skills_dir, &target_path)?;
    } else {
        place_staged(&staged, skills_dir, &target_path)?;
        if let Some(meta) = &local_meta {
            let dir = skills_dir.join(&meta.path);
            if dir.exists() {
                if let Err(e) = std::fs::remove_dir_all(&dir) {
                    let _ = std::fs::remove_dir_all(skills_dir.join(&target_path));
                    return Err(e).with_context(|| {
                        format!("failed to remove local skill {}", dir.display())
                    });
                }
            }
        }
    }

    let meta = SkillMetaFile {
        schema_version: theirs_skill.meta.schema_version,
        skill_id: skill_id.to_string(),
        path_key: path_key(&target_path),
        path: target_path,
        enabled: theirs_skill.meta.enabled,
        tags: theirs_skill.meta.tags.clone(),
        source: theirs_skill.meta.source.clone(),
    };
    write_worktree_meta(skills_dir, &meta)
}

/// Keep the local version and add the theirs version as a new skill with a
/// fresh id, its directory suffixed with the origin device name (§4; the
/// design writes `name (来自 <设备名>)` — a language-neutral `name (device)`
/// is used since backend strings are not localized).
fn apply_keep_both(
    repo: &Repository,
    skills_dir: &Path,
    skill_id: &str,
    theirs_commit: Oid,
) -> Result<()> {
    let theirs_c = repo.find_commit(theirs_commit)?;
    let device = theirs_c.author().name().unwrap_or("").trim().to_string();
    let theirs_tree = theirs_c.tree()?;
    let theirs_snap = snapshot::read_snapshot(repo, &theirs_tree)
        .context("failed to read the pinned remote version")?;

    let Some(theirs_skill) = theirs_snap.skills.get(skill_id) else {
        // Remote side was a deletion — nothing to duplicate; keeping local is
        // the whole resolution.
        return Ok(());
    };
    let content = theirs_skill
        .content
        .context("pinned remote version has no content directory")?;

    let suffix = skill_metadata::sanitize_skill_name(&device)
        .unwrap_or_else(|| "remote".to_string());
    let base_name = format!("{} ({})", theirs_skill.meta.path, suffix);
    let new_id = uuid::Uuid::new_v4().to_string();
    let target_path = free_path_for(skills_dir, &base_name, &new_id)?;
    let staged = stage_skill_extract(repo, content, skills_dir)?;
    place_staged(&staged, skills_dir, &target_path)?;

    let meta = SkillMetaFile {
        schema_version: theirs_skill.meta.schema_version,
        skill_id: new_id,
        path_key: path_key(&target_path),
        path: target_path,
        enabled: theirs_skill.meta.enabled,
        tags: theirs_skill.meta.tags.clone(),
        source: theirs_skill.meta.source.clone(),
    };
    write_worktree_meta(skills_dir, &meta)
}

// ── worktree helpers ──

fn worktree_meta_path(skills_dir: &Path, skill_id: &str) -> std::path::PathBuf {
    skills_dir
        .join(snapshot::METADATA_DIR)
        .join("skills")
        .join(format!("{skill_id}.json"))
}

fn read_worktree_meta(skills_dir: &Path, skill_id: &str) -> Result<Option<SkillMetaFile>> {
    let path = worktree_meta_path(skills_dir, skill_id);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    Ok(Some(serde_json::from_str(&raw).with_context(|| {
        format!("invalid skill metadata {}", path.display())
    })?))
}

fn write_worktree_meta(skills_dir: &Path, meta: &SkillMetaFile) -> Result<()> {
    let path = worktree_meta_path(skills_dir, &meta.skill_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, sync_metadata::canonical_json_bytes(meta)?)?;
    Ok(())
}

/// First free variant of `wanted` (folded-key comparison against all other
/// local skills' metadata): `wanted`, then `wanted (2)`, `(3)`, …
fn free_path_for(skills_dir: &Path, wanted: &str, own_id: &str) -> Result<String> {
    let mut occupied: BTreeSet<String> = BTreeSet::new();
    let mut own_path: Option<String> = None;
    let meta_dir = skills_dir.join(snapshot::METADATA_DIR).join("skills");
    if meta_dir.is_dir() {
        for entry in std::fs::read_dir(&meta_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(raw) = std::fs::read_to_string(&path) {
                    if let Ok(meta) = serde_json::from_str::<SkillMetaFile>(&raw) {
                        if path.file_stem().map(|s| s.to_string_lossy() == own_id).unwrap_or(false) {
                            own_path = Some(meta.path);
                            continue;
                        }
                        occupied.insert(meta.path_key);
                    }
                }
            }
        }
    }
    let is_free = |candidate: &str, occupied: &BTreeSet<String>, own_path: Option<&str>| {
        !occupied.contains(&path_key(candidate))
            && (own_path == Some(candidate) || !skills_dir.join(candidate).exists())
    };
    if is_free(wanted, &occupied, own_path.as_deref()) {
        return Ok(wanted.to_string());
    }
    for n in 2..1000 {
        let candidate = match wanted.rsplit_once('/') {
            Some((dir, name)) => format!("{dir}/{name} ({n})"),
            None => format!("{wanted} ({n})"),
        };
        if is_free(&candidate, &occupied, own_path.as_deref()) {
            return Ok(candidate);
        }
    }
    bail!("no free path found for {wanted}");
}

/// Extract a content tree into a staging directory *next to* (never inside)
/// the library, on the same volume so the final move is a plain rename.
/// Cleans up after itself on failure.
fn stage_skill_extract(
    repo: &Repository,
    content: git2::Oid,
    skills_dir: &Path,
) -> Result<std::path::PathBuf> {
    let staging_parent = skills_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    let staged = staging_parent.join(format!(".sm-resolve-{}", uuid::Uuid::now_v7()));
    match extract_tree_to_dir(repo, &repo.find_tree(content)?, &staged) {
        Ok(()) => Ok(staged),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staged);
            Err(e)
        }
    }
}

/// Move a staged extraction into its final place inside the library.
fn place_staged(staged: &Path, skills_dir: &Path, target_rel: &str) -> Result<()> {
    let target = skills_dir.join(target_rel);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Err(e) = std::fs::rename(staged, &target) {
        let _ = std::fs::remove_dir_all(staged);
        return Err(anyhow::Error::from(e)
            .context(format!("failed to move resolved skill into {}", target.display())));
    }
    Ok(())
}

fn extract_tree_to_dir(repo: &Repository, tree: &git2::Tree, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in tree.iter() {
        let name = entry.name().context("tree entry with non-utf8 name")?;
        let target = dest.join(name);
        match entry.kind() {
            Some(ObjectType::Tree) => {
                extract_tree_to_dir(repo, &repo.find_tree(entry.id())?, &target)?;
            }
            Some(ObjectType::Blob) => {
                let blob = repo.find_blob(entry.id())?;
                std::fs::write(&target, blob.content())?;
                #[cfg(unix)]
                if entry.filemode() == 0o100755 {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
                }
            }
            _ => log::warn!(
                "conflict resolution: skipping unsupported entry {} in extracted skill",
                target.display()
            ),
        }
    }
    Ok(())
}
