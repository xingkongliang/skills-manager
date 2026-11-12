use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::{
    central_repo, content_hash,
    error::{AppError, TargetConflictDetail},
    skill_store::{ScenarioRecord, SkillStore, SkillTargetRecord},
    sync_engine, sync_metadata, tool_adapters,
    tool_service,
};

#[derive(Debug, Clone)]
pub struct ScenarioSyncTarget {
    pub skill_id: String,
    pub skill_name: String,
    pub tool: String,
    pub source: PathBuf,
    pub target: PathBuf,
    pub mode: sync_engine::SyncMode,
    /// Current content hash of the central skill source, copied from
    /// `SkillRecord.content_hash`. Compared against the previously
    /// synced `SkillTargetRecord.source_hash` to skip redundant
    /// Copy-mode resyncs at startup (issue #153).
    pub source_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncPreviewTarget {
    pub skill_id: String,
    pub skill_name: String,
    pub tool: String,
    pub target_path: String,
    pub mode: String,
}

pub fn ensure_scenario_exists(store: &SkillStore, scenario_id: &str) -> Result<(), AppError> {
    let exists = store
        .get_all_scenarios()
        .map_err(AppError::db)?
        .iter()
        .any(|s| s.id == scenario_id);
    if !exists {
        return Err(AppError::not_found("Scenario not found"));
    }
    Ok(())
}

pub fn enabled_installed_adapters_for_scenario_skill(
    store: &SkillStore,
    scenario_id: &str,
    skill_id: &str,
) -> Result<Vec<tool_adapters::ToolAdapter>, AppError> {
    let adapters = tool_adapters::enabled_installed_adapters(store);
    let adapter_keys: Vec<String> = adapters.iter().map(|a| a.key.clone()).collect();

    store
        .ensure_scenario_skill_tool_defaults(scenario_id, skill_id, &adapter_keys)
        .map_err(AppError::db)?;

    let enabled = store
        .get_enabled_tools_for_scenario_skill(scenario_id, skill_id)
        .map_err(AppError::db)?;
    let enabled_set: HashSet<String> = enabled.into_iter().collect();

    Ok(adapters
        .into_iter()
        .filter(|adapter| enabled_set.contains(&adapter.key))
        .collect())
}

pub fn collect_scenario_sync_targets(
    store: &SkillStore,
    scenario_id: &str,
) -> Result<Vec<ScenarioSyncTarget>, AppError> {
    let skills = store
        .get_skills_for_scenario(scenario_id)
        .map_err(AppError::db)?;
    let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
    let existing_targets = store.get_all_targets().map_err(AppError::db)?;
    let mut targets = Vec::new();

    for skill in &skills {
        let source = PathBuf::from(&skill.central_path);
        let target_name = sync_engine::target_dir_name(&source, &skill.name);
        let adapters = enabled_installed_adapters_for_scenario_skill(store, scenario_id, &skill.id)?;
        for adapter in &adapters {
            let target = deployment_target(adapter, &target_name, &skill.id, &existing_targets);
            let mode = sync_engine::sync_mode_for_tool(&adapter.key, configured_mode.as_deref());
            targets.push(ScenarioSyncTarget {
                skill_id: skill.id.clone(),
                skill_name: skill.name.clone(),
                tool: adapter.key.clone(),
                source: source.clone(),
                target,
                mode,
                source_hash: skill.content_hash.clone(),
            });
        }
    }

    Ok(targets)
}

fn deployment_target(
    adapter: &tool_adapters::ToolAdapter,
    target_name: &str,
    skill_id: &str,
    existing_targets: &[SkillTargetRecord],
) -> PathBuf {
    let root = adapter.skills_dir();
    existing_targets
        .iter()
        .find(|target| target.skill_id == skill_id && target.tool == adapter.key)
        .map(|target| PathBuf::from(&target.target_path))
        .filter(|path| is_skill_path_within_root(path, &root))
        .unwrap_or_else(|| root.join(target_name))
}

fn is_skill_path_within_root(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|relative| {
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_)))
    })
}

pub fn preview_scenario_sync(
    store: &SkillStore,
    scenario_id: &str,
) -> Result<Vec<SyncPreviewTarget>, AppError> {
    collect_scenario_sync_targets(store, scenario_id).map(|targets| {
        targets
            .into_iter()
            .map(|target| SyncPreviewTarget {
                skill_id: target.skill_id,
                skill_name: target.skill_name,
                tool: target.tool,
                target_path: target.target.to_string_lossy().to_string(),
                mode: target.mode.as_str().to_string(),
            })
            .collect()
    })
}

/// Decide which `SyncMode` `is_target_current` should compare against, or
/// `None` if the existing target's mode is incompatible with the desired
/// mode and the skip path must be refused.
///
/// Returns `Some(existing)` when both modes match exactly. Also returns
/// `Some(Copy)` when the existing record is `"copy"` but the desired
/// mode is `Symlink` — this is the Windows fallback case (issue #153):
/// `symlink_dir()` failed on a prior run and we landed in copy mode, so
/// every subsequent startup would re-attempt symlink, fail again, and
/// trigger a full recursive copy. Treating the existing copy as
/// compatible lets the hash gate skip when the source hasn't changed.
///
/// The reverse direction (existing `"symlink"`, desired `Copy`) returns
/// `None` because the user actively changed the `sync_mode` setting and
/// the on-disk symlink doesn't reflect that intent.
fn skip_check_mode(existing_mode: &str, desired: sync_engine::SyncMode) -> Option<sync_engine::SyncMode> {
    match (existing_mode, desired) {
        ("symlink", sync_engine::SyncMode::Symlink) => Some(sync_engine::SyncMode::Symlink),
        ("copy", sync_engine::SyncMode::Copy) => Some(sync_engine::SyncMode::Copy),
        ("copy", sync_engine::SyncMode::Symlink) => Some(sync_engine::SyncMode::Copy),
        _ => None,
    }
}

/// Remove a recorded deployment, keeping (and reporting) anything that no
/// longer matches the record. Every row-driven removal in this module goes
/// through here so none of them can fall back to "delete whatever is there".
fn remove_recorded_or_warn(path: &Path, recorded_mode: &str) -> bool {
    match sync_engine::remove_recorded_target(path, recorded_mode) {
        Ok(removed) => {
            if !removed {
                log::warn!(
                    "Preserving {}: no longer matches its recorded {recorded_mode} deployment; \
                     removing the record only",
                    path.display()
                );
            }
            removed
        }
        Err(e) => {
            log::warn!("Failed to remove sync target {}: {e}", path.display());
            false
        }
    }
}

/// Clean up a deployment at a path its tool no longer reads: the adapter
/// moved, or the tool's skills directory was changed. [`deployment_target`]
/// keeps any recorded path inside the tool's root, so a recorded path that
/// differs from the desired one lies outside it.
///
/// Links are removed as usual. A copy-mode directory may hold edits made
/// there that exist nowhere else (#593), and a matching content hash is no
/// proof it is unedited: the hash skips `.gitignore` and caches. It cannot
/// stay where it is either. Another agent may read that directory too, and
/// moving the tool back would find it in the way. So it is moved into the
/// library's `kept-copies/<tool>/`. Callers drop the record either way.
pub(crate) fn release_abandoned_target(path: &Path, recorded_mode: &str, tool: &str) {
    if recorded_mode != "copy" {
        remove_recorded_or_warn(path, recorded_mode);
        return;
    }
    // Only what still looks like our copy is ours to move.
    match sync_engine::matches_recorded_deployment(path, recorded_mode) {
        Ok(true) => {}
        Ok(false) => return,
        Err(e) => {
            log::warn!("Keeping {}: cannot inspect it ({e})", path.display());
            return;
        }
    }
    match move_into_kept_copies(path, tool) {
        Ok(kept) => log::warn!(
            "Moved {} to {}: {tool} no longer reads that path, and the copy may hold edits",
            path.display(),
            kept.display()
        ),
        Err(e) => log::warn!(
            "Keeping {} in place: could not move it into the library ({e})",
            path.display()
        ),
    }
}

/// A rename only, so nothing is copied half-way; across volumes it fails and
/// the caller keeps the copy where it is.
fn move_into_kept_copies(path: &Path, tool: &str) -> std::io::Result<PathBuf> {
    let root = central_repo::base_dir()
        .join("kept-copies")
        .join(single_dir_name(tool));
    std::fs::create_dir_all(&root)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "skill".to_string());
    let mut kept = root.join(&name);
    let mut n = 2;
    // `symlink_metadata`, not `exists`: a dangling link still takes the name.
    while std::fs::symlink_metadata(&kept).is_ok() {
        kept = root.join(format!("{name}-{n}"));
        n += 1;
    }
    std::fs::rename(path, &kept)?;
    Ok(kept)
}

/// A tool key as one directory name. Custom keys reach here unchecked from
/// the CLI, so a key like `../skills` must not lead outside `kept-copies/`.
fn single_dir_name(key: &str) -> String {
    let name: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() {
        "tool".to_string()
    } else {
        name
    }
}

/// Whether the deployment recorded at exactly this path is still current, so a
/// resync can leave it alone: a link to the source, or a real directory copied
/// from the library's current content hash (#153).
///
/// [`sync_engine::sync_skill`] rewrites a recorded copy wholesale. Only the
/// startup and preset sync asked this first, so every other resync (a preset
/// applied from the PresetBar, a batch deploy, the sync after an install, an
/// update that changed no content) deleted whatever had been written into the
/// agent's copy although the library had not changed. Skipping never deletes
/// anything, so every resync can ask.
///
/// Only the calling tool's own row counts. Rows of other tools sharing the
/// directory are history of their own deployments, and the removal paths rely
/// on their disagreeing to keep a directory whose owner is unclear. A copy
/// replaced by a file or a foreign link is not current, so `sync_skill` still
/// gets to refuse whatever replaced it.
pub(crate) fn recorded_target_is_current(
    record: Option<&SkillTargetRecord>,
    source: &Path,
    target: &Path,
    desired: sync_engine::SyncMode,
    library_hash: Option<&str>,
) -> bool {
    let Some(record) = record.filter(|record| record.status == "ok") else {
        return false;
    };
    let Some(check_mode) = skip_check_mode(&record.mode, desired) else {
        return false;
    };
    // A link to the source serves the library's content too; anything else
    // in place of the copy goes to `sync_skill`, which refuses it.
    if matches!(check_mode, sync_engine::SyncMode::Copy)
        && !matches!(
            sync_engine::classify_target(target, Some(source)),
            Ok(sync_engine::TargetState::RealDir | sync_engine::TargetState::LinkToSource)
        )
    {
        return false;
    }
    sync_engine::is_target_current(
        source,
        target,
        check_mode,
        record.source_hash.as_deref(),
        library_hash,
    )
}

/// A key identifying the *directory entry* a target path names, used to spot
/// two rows that claim one object under different spellings.
///
/// The parent is canonicalized (resolving `..` and symlinked ancestors) and the
/// final component re-attached verbatim — deliberately NOT `canonicalize()` on
/// the whole path, which follows the final symlink and would collapse every
/// agent's link into the one library directory they all point at, making
/// distinct deployments look like the same object.
///
/// Known gap: on a case-insensitive filesystem two spellings of the final
/// component still key differently. That is the same lexical assumption the
/// rest of this module makes about `target_path`. It is not harmless
/// everywhere: in the batch deploy, a contradicting row spelled with a
/// different case is not pooled, so the remaining rows can authorize a
/// replacement the contradiction should have blocked (pre-existing, H review
/// item 5). Outside the batch, rows of other agents only ever adopt, so there
/// the gap costs a refusal or an extra row, never a deletion.
pub(crate) fn ownership_key(path: &Path, memo: &mut HashMap<PathBuf, PathBuf>) -> PathBuf {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return path.to_path_buf();
    };
    // Rows share a handful of skills directories, so memoizing the parent keeps
    // this to a few syscalls no matter how large the library is.
    let canonical_parent = match memo.get(parent) {
        Some(cached) => cached.clone(),
        None => {
            let resolved = parent.canonicalize().unwrap_or_else(|_| parent.to_path_buf());
            memo.insert(parent.to_path_buf(), resolved.clone());
            resolved
        }
    };
    canonical_parent.join(name)
}

/// What our records say we put at each target path, pooled across every row
/// that names it — the one ownership judgment every deploy path asks.
///
/// Ownership belongs to the path, not to the `(skill, tool)` pair. When several
/// agents share a skills directory, a row for any of them is evidence about the
/// object there; otherwise the agent without its own row refuses a directory we
/// demonstrably wrote for its neighbour (#363 review, cluster 37 A5). The
/// evidence is narrow on purpose:
///
/// - Every row naming the object must agree on the mode. Contradictory rows
///   mean we do not know what we put there, so they vouch for nothing —
///   taking whichever came first would let a stale `copy` row authorize
///   deleting a real directory that replaced our link.
/// - Every row naming the object must belong to the skill being deployed. A
///   row of another skill on the same path is that skill's deployment, not
///   evidence for this one.
///
/// Rows are pooled from the whole table, not just the pairs being deployed:
/// `skill_targets` is unique on `(skill_id, tool)`, not on `target_path`, so a
/// row nobody selected can still be a claim on the same object. They are keyed
/// by directory entry rather than by path string, so a row spelled differently
/// (symlinked ancestor, `..`) still counts as evidence about the same object.
struct PathEvidence {
    memo: HashMap<PathBuf, PathBuf>,
    /// `(skill_id, tool, mode)` of every row, by the directory entry it names.
    rows: HashMap<PathBuf, Vec<(String, String, String)>>,
    /// What this run itself wrote, as `(skill_id, mode)`. It outranks the
    /// rows: they describe the previous deployment, this describes what is on
    /// disk right now, and preferring them would make the second agent on a
    /// shared path refuse the directory the first one just created after a
    /// mode change.
    written: HashMap<PathBuf, (String, String)>,
}

/// What [`PathEvidence`] says about one target, for one `(skill, tool)` pair.
#[derive(Debug, PartialEq, Eq)]
enum Evidence {
    /// This agent's own row for this skill, or this run's own write: may
    /// replace what it recorded.
    Own(String),
    /// Only other agents' rows for this skill. Outside a user-initiated batch
    /// these may adopt an identical copy, never replace one: the other agent's
    /// own skip check is what preserves edits made in its copy, and borrowing
    /// its row to rewrite the directory would undo that (H review, item 1).
    Borrowed(String),
    /// Nothing vouches for the object.
    Unvouched,
}

impl Evidence {
    /// Both tiers, for the batch deploy: the user selected these pairs and
    /// confirmed the batch, so any agreeing row authorizes replacement.
    fn any_mode(&self) -> Option<&str> {
        match self {
            Evidence::Own(mode) | Evidence::Borrowed(mode) => Some(mode),
            Evidence::Unvouched => None,
        }
    }
}

impl PathEvidence {
    fn new(rows: &[SkillTargetRecord]) -> Self {
        let mut memo = HashMap::new();
        let mut by_key: HashMap<PathBuf, Vec<(String, String, String)>> = HashMap::new();
        for row in rows {
            let key = ownership_key(Path::new(&row.target_path), &mut memo);
            by_key.entry(key).or_default().push((
                row.skill_id.clone(),
                row.tool.clone(),
                row.mode.clone(),
            ));
        }
        Self {
            memo,
            rows: by_key,
            written: HashMap::new(),
        }
    }

    fn judge(&mut self, skill_id: &str, tool: &str, target: &Path) -> Evidence {
        let key = ownership_key(target, &mut self.memo);
        if let Some((writer, mode)) = self.written.get(&key) {
            return if writer == skill_id {
                Evidence::Own(mode.clone())
            } else {
                Evidence::Unvouched
            };
        }
        let Some(rows) = self.rows.get(&key) else {
            return Evidence::Unvouched;
        };
        let Some((_, _, first_mode)) = rows.first() else {
            return Evidence::Unvouched;
        };
        if !rows
            .iter()
            .all(|(owner, _, mode)| owner == skill_id && mode == first_mode)
        {
            return Evidence::Unvouched;
        }
        if rows.iter().any(|(_, row_tool, _)| row_tool == tool) {
            Evidence::Own(first_mode.clone())
        } else {
            Evidence::Borrowed(first_mode.clone())
        }
    }

    fn wrote(&mut self, skill_id: &str, target: &Path, mode: sync_engine::SyncMode) {
        let key = ownership_key(target, &mut self.memo);
        self.written
            .insert(key, (skill_id.to_string(), mode.as_str().to_string()));
    }

    fn has_written(&mut self, target: &Path) -> bool {
        let key = ownership_key(target, &mut self.memo);
        self.written.contains_key(&key)
    }

    /// Another skill was written there earlier in this run. A dry run has to
    /// treat that path as occupied, because the real run will find it so.
    fn written_by_other_skill(&mut self, skill_id: &str, target: &Path) -> bool {
        let key = ownership_key(target, &mut self.memo);
        self.written
            .get(&key)
            .is_some_and(|(writer, _)| writer != skill_id)
    }
}

/// What a deploy outside the batch does with [`PathEvidence::judge`]'s answer.
enum DeployStep {
    /// Write, replacing only what this recorded mode authorizes (`None`:
    /// nothing of the user's may be destroyed).
    Write(Option<String>),
    /// The directory already holds exactly the library's content and another
    /// agent sharing it recorded it: add this agent's row, touch nothing.
    Adopt(String),
}

fn deploy_step(evidence: Evidence, source: &Path, target: &Path) -> DeployStep {
    match evidence {
        Evidence::Own(mode) => DeployStep::Write(Some(mode)),
        Evidence::Borrowed(mode) if copy_matches_library(&mode, source, target) => {
            DeployStep::Adopt(mode)
        }
        // A link to the source, or nothing at all, is still written: neither
        // destroys anything. Anything else is refused, as it always was.
        Evidence::Borrowed(_) | Evidence::Unvouched => DeployStep::Write(None),
    }
}

/// Whether a borrowed copy-mode directory holds exactly the library's content,
/// compared the way the stranded-skill backfill does (`hash_directory` on both
/// sides, failing closed).
///
/// The hash skips `.gitignore`, caches and the like, so equal hashes do not
/// prove the copy is unedited (#593). That is acceptable here only because
/// adopting writes nothing: a wrong answer adds a row, it never deletes.
fn copy_matches_library(mode: &str, source: &Path, target: &Path) -> bool {
    mode == "copy"
        && matches!(
            sync_engine::classify_target(target, Some(source)),
            Ok(sync_engine::TargetState::RealDir)
        )
        && source.is_dir()
        && matches!(
            (
                content_hash::hash_directory(target),
                content_hash::hash_directory(source),
            ),
            (Ok(deployed), Ok(library)) if deployed == library
        )
}

/// The row recording an adopted directory: nothing was written, so its mode
/// is the borrowed evidence's and its hash the library's it was compared to.
fn adopted_target_record(
    skill_id: &str,
    tool: &str,
    target: &Path,
    mode: String,
    source_hash: Option<String>,
) -> SkillTargetRecord {
    SkillTargetRecord {
        id: uuid::Uuid::new_v4().to_string(),
        skill_id: skill_id.to_string(),
        tool: tool.to_string(),
        target_path: target.to_string_lossy().to_string(),
        mode,
        status: "ok".to_string(),
        synced_at: Some(chrono::Utc::now().timestamp_millis()),
        last_error: None,
        source_hash,
    }
}

/// Authorization for a deployment write: a `skill_targets` row claiming this
/// exact path lets us replace what it recorded, otherwise we may only write
/// where nothing of the user's would be destroyed (#363).
fn replace_policy(recorded_mode: Option<&str>) -> sync_engine::ReplacePolicy<'_> {
    match recorded_mode {
        Some(mode) => sync_engine::ReplacePolicy::Recorded { mode },
        None => sync_engine::ReplacePolicy::NoClobber,
    }
}

/// One target a sync refused to write because it is not ours to replace.
///
/// Kept structured all the way to the caller: an agent driving the CLI has to
/// tell the user *which* path is in the way and what to do about it, and a
/// pre-rendered English sentence cannot be branched on (#363).
#[derive(Debug, Clone, Serialize)]
pub struct TargetConflict {
    pub target: PathBuf,
    pub reason: String,
}

impl fmt::Display for TargetConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&sync_engine::refusal_message(&self.target, &self.reason))
    }
}

/// Sync every desired target, returning the ownership refusals rather than
/// failing on them.
///
/// Refusals are *reported*, not thrown, because whether one should abort the
/// operation depends entirely on the caller: a user typing `skills sync` needs
/// an error, while app startup must not be prevented from launching by a
/// name collision in an agent directory. Returning the list lets each caller
/// state that policy; an early `Err` here made startup panic (#363).
pub fn sync_desired_targets(
    store: &SkillStore,
    desired_targets: &[ScenarioSyncTarget],
) -> Result<Vec<TargetConflict>, AppError> {
    let batch_start = Instant::now();
    let all_targets = store.get_all_targets().map_err(AppError::db)?;
    let mut evidence = PathEvidence::new(&all_targets);
    let existing_targets: HashMap<(String, String), SkillTargetRecord> = all_targets
        .into_iter()
        .map(|target| ((target.skill_id.clone(), target.tool.clone()), target))
        .collect();

    let mut synced_count = 0usize;
    let mut skipped_count = 0usize;
    let mut failed_count = 0usize;
    let mut refusals: Vec<TargetConflict> = Vec::new();

    for desired in desired_targets {
        let target_start = Instant::now();
        let key = (desired.skill_id.clone(), desired.tool.clone());
        if let Some(existing) = existing_targets.get(&key) {
            let target_path = PathBuf::from(&existing.target_path);
            if target_path != desired.target {
                // Adapters can share a skills directory: amp and replit both
                // deploy to ~/.config/agents/skills, and kimi did too until it
                // moved to ~/.kimi-code/skills (#270). When one of them is
                // retargeted, the old path is not this tool's leftover — it is
                // still another tool's live deployment. Drop the stale record,
                // but leave the directory to whoever is still deployed there.
                let claimed_by_another_tool = desired_targets.iter().any(|other| {
                    other.tool != desired.tool
                        && other.skill_id == desired.skill_id
                        && other.target == target_path
                });
                if claimed_by_another_tool {
                    log::info!(
                        "Keeping {}: still the deployment target of another tool; \
                         dropping {}'s stale record only",
                        target_path.display(),
                        desired.tool
                    );
                } else {
                    release_abandoned_target(&target_path, &existing.mode, &desired.tool);
                }
                if let Err(e) = store.delete_target(&desired.skill_id, &desired.tool) {
                    log::warn!(
                        "Failed to delete stale target record for skill {}, tool {}: {e}",
                        desired.skill_id,
                        desired.tool
                    );
                }
            } else if recorded_target_is_current(
                Some(existing),
                &desired.source,
                &desired.target,
                desired.mode,
                desired.source_hash.as_deref(),
            ) {
                // Surface the Windows fallback case in logs so operators
                // can tell when a target is permanently on Copy because
                // an earlier symlink_dir() failed (issue #153). Helpful
                // when a user later enables Developer Mode and wonders
                // why Symlink isn't being re-attempted.
                if existing.mode == "copy" && matches!(desired.mode, sync_engine::SyncMode::Symlink)
                {
                    log::debug!(
                        "sync_desired_targets: skill {} ({}) staying on copy fallback for {} (content unchanged); undeploy and deploy the skill again to retry symlink",
                        desired.skill_id,
                        desired.skill_name,
                        desired.tool
                    );
                }
                skipped_count += 1;
                continue;
            }
        }

        // This agent's own row is what lets us replace a copy-mode directory;
        // another agent's row on a shared directory only lets us adopt an
        // identical copy. Anything else must be left alone (#363, `PathEvidence`).
        let recorded_mode = match deploy_step(
            evidence.judge(&desired.skill_id, &desired.tool, &desired.target),
            &desired.source,
            &desired.target,
        ) {
            DeployStep::Write(mode) => mode,
            DeployStep::Adopt(mode) => {
                let record = adopted_target_record(
                    &desired.skill_id,
                    &desired.tool,
                    &desired.target,
                    mode,
                    desired.source_hash.clone(),
                );
                if let Err(e) = store.insert_target(&record) {
                    log::warn!(
                        "Failed to insert adopted target for skill {}: {e}",
                        desired.skill_id
                    );
                }
                synced_count += 1;
                continue;
            }
        };
        match sync_engine::sync_skill(
            &desired.source,
            &desired.target,
            desired.mode,
            replace_policy(recorded_mode.as_deref()),
        ) {
            Ok(actual_mode) => {
                evidence.wrote(&desired.skill_id, &desired.target, actual_mode);
                let now = chrono::Utc::now().timestamp_millis();
                let target_record = SkillTargetRecord {
                    id: uuid::Uuid::new_v4().to_string(),
                    skill_id: desired.skill_id.clone(),
                    tool: desired.tool.clone(),
                    target_path: desired.target.to_string_lossy().to_string(),
                    mode: actual_mode.as_str().to_string(),
                    status: "ok".to_string(),
                    synced_at: Some(now),
                    last_error: None,
                    // Record the hash that was just synced so the next
                    // run of this loop can short-circuit when the central
                    // skill content has not changed (issue #153).
                    source_hash: desired.source_hash.clone(),
                };
                if let Err(e) = store.insert_target(&target_record) {
                    log::warn!(
                        "Failed to insert sync target for skill {}: {e}",
                        desired.skill_id
                    );
                }
                synced_count += 1;
                let elapsed = target_start.elapsed().as_millis();
                if elapsed >= 200 {
                    log::warn!(
                        "sync_desired_targets: slow sync ({elapsed} ms, mode={}) for skill {} ({}) -> {}",
                        actual_mode.as_str(),
                        desired.skill_id,
                        desired.skill_name,
                        desired.target.display()
                    );
                }
            }
            Err(e) => {
                failed_count += 1;
                // An ownership refusal is not a transient failure to log and
                // move past: the user's content is intact but the skill they
                // asked for is not deployed, and saying "ok" to that is the
                // half of #363 that made the data loss invisible.
                if let Some(refused) = e.downcast_ref::<sync_engine::ReplaceRefused>() {
                    refusals.push(TargetConflict {
                        target: refused.target.clone(),
                        reason: refused.reason.to_string(),
                    });
                }
                log::warn!(
                    "Failed to sync skill {} ({}) to {} after {} ms: {e}",
                    desired.skill_id,
                    desired.skill_name,
                    desired.target.display(),
                    target_start.elapsed().as_millis()
                );
            }
        }
    }

    log::info!(
        "sync_desired_targets: {} targets in {} ms (synced={synced_count}, skipped={skipped_count}, failed={failed_count})",
        desired_targets.len(),
        batch_start.elapsed().as_millis()
    );

    // Everything that could be synced has been; hand back what was not.
    // Ordinary IO failures stay logged-and-tolerated, as before.
    Ok(refusals)
}

/// Preview the ownership decisions made by `sync_desired_targets` without changing targets.
pub fn preflight_scenario_sync_targets(
    store: &SkillStore,
    desired_targets: &[ScenarioSyncTarget],
) -> Result<(), AppError> {
    let mut evidence = PathEvidence::new(&store.get_all_targets().map_err(AppError::db)?);
    let mut conflicts = Vec::new();
    for desired in desired_targets {
        // The real run will find another skill's deployment here even though
        // the disk does not show it yet.
        if evidence.written_by_other_skill(&desired.skill_id, &desired.target) {
            conflicts.push(TargetConflictDetail {
                path: desired.target.display().to_string(),
                reason: sync_engine::ReplacePolicy::NoClobber.describe().to_string(),
            });
            continue;
        }
        let recorded_mode = match deploy_step(
            evidence.judge(&desired.skill_id, &desired.tool, &desired.target),
            &desired.source,
            &desired.target,
        ) {
            DeployStep::Write(mode) => mode,
            DeployStep::Adopt(_) => continue,
        };
        match sync_engine::preflight_replace(
            &desired.source,
            &desired.target,
            desired.mode,
            replace_policy(recorded_mode.as_deref()),
        ) {
            Ok(()) => evidence.wrote(&desired.skill_id, &desired.target, desired.mode),
            Err(error) => match error.downcast_ref::<sync_engine::ReplaceRefused>() {
                Some(refused) => conflicts.push(TargetConflictDetail {
                    path: refused.target.display().to_string(),
                    reason: refused.reason.to_string(),
                }),
                None => return Err(AppError::io(error)),
            },
        }
    }
    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(AppError::target_conflict(
            format!(
                "Refusing to sync: {} target(s) would overwrite content that is not ours. Nothing was changed.",
                conflicts.len()
            ),
            conflicts,
        ))
    }
}

/// Turn reported refusals into the error a user-initiated command should show.
/// Deliberately says only that these targets were skipped — everything else in
/// the operation did apply, so claiming "nothing happened" would be false.
pub fn refusals_to_error(refusals: Vec<TargetConflict>) -> Result<(), AppError> {
    if refusals.is_empty() {
        return Ok(());
    }
    let summary = format!(
        "{} skill(s) were skipped because their target is not ours to replace \
         (nothing at those paths was deleted; everything else was applied). {}",
        refusals.len(),
        refusals
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join("; ")
    );
    Err(AppError::target_conflict(
        summary,
        refusals
            .into_iter()
            .map(|c| TargetConflictDetail {
                path: c.target.display().to_string(),
                reason: c.reason,
            })
            .collect(),
    ))
}

pub fn unsync_obsolete_scenario_targets(
    store: &SkillStore,
    old_scenario_id: &str,
    desired_targets: &[ScenarioSyncTarget],
) -> Result<(), AppError> {
    let desired_paths: HashMap<(String, String), PathBuf> = desired_targets
        .iter()
        .map(|target| {
            (
                (target.skill_id.clone(), target.tool.clone()),
                target.target.clone(),
            )
        })
        .collect();

    let old_skill_ids = store
        .get_skill_ids_for_scenario(old_scenario_id)
        .map_err(AppError::db)?;
    for skill_id in &old_skill_ids {
        let targets = store.get_targets_for_skill(skill_id).unwrap_or_default();
        for target in &targets {
            let path = PathBuf::from(&target.target_path);
            let key = (skill_id.clone(), target.tool.clone());
            match desired_paths.get(&key) {
                Some(desired) if desired == &path => continue,
                // Still deployed to this tool under the new preset, only
                // somewhere else: the old path is abandoned, not removed by
                // the user.
                Some(_) => release_abandoned_target(&path, &target.mode, &target.tool),
                None => {
                    remove_recorded_or_warn(&path, &target.mode);
                }
            }
            if let Err(e) = store.delete_target(skill_id, &target.tool) {
                log::warn!(
                    "Failed to delete target record for skill {skill_id}, tool {}: {e}",
                    target.tool
                );
            }
        }
    }

    Ok(())
}

pub fn unsync_scenario_skills(store: &SkillStore, scenario_id: &str) -> Result<(), AppError> {
    let skill_ids = store
        .get_skill_ids_for_scenario(scenario_id)
        .map_err(AppError::db)?;

    for skill_id in &skill_ids {
        let targets = store.get_targets_for_skill(skill_id).unwrap_or_default();
        for target in &targets {
            let path = PathBuf::from(&target.target_path);
            remove_recorded_or_warn(&path, &target.mode);
            if let Err(e) = store.delete_target(skill_id, &target.tool) {
                log::warn!(
                    "Failed to delete target record for skill {skill_id}, tool {}: {e}",
                    target.tool
                );
            }
        }
    }

    Ok(())
}

pub fn sync_scenario_skills(
    store: &SkillStore,
    scenario_id: &str,
) -> Result<Vec<TargetConflict>, AppError> {
    let desired_targets = collect_scenario_sync_targets(store, scenario_id)?;
    sync_desired_targets(store, &desired_targets)
}

pub fn apply_scenario_to_default(
    store: &SkillStore,
    scenario_id: &str,
) -> Result<Vec<TargetConflict>, AppError> {
    ensure_scenario_exists(store, scenario_id)?;
    let desired_targets = collect_scenario_sync_targets(store, scenario_id)?;

    if let Ok(Some(old_id)) = store.get_active_scenario_id() {
        if old_id != scenario_id {
            unsync_obsolete_scenario_targets(store, &old_id, &desired_targets)?;
        }
    }

    store.set_active_scenario(scenario_id).map_err(AppError::db)?;
    sync_desired_targets(store, &desired_targets)
}

pub fn sync_skill_to_active_scenario(
    store: &SkillStore,
    scenario_id: &str,
    skill_id: &str,
) -> Result<(), AppError> {
    if let Ok(Some(active_id)) = store.get_active_scenario_id() {
        if active_id == scenario_id {
            let adapters = enabled_installed_adapters_for_scenario_skill(store, scenario_id, skill_id)?;
            let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
            let Ok(Some(skill)) = store.get_skill_by_id(skill_id) else {
                return Ok(());
            };
            let source = PathBuf::from(&skill.central_path);
            let target_name = sync_engine::target_dir_name(&source, &skill.name);
            let old_targets = store.get_targets_for_skill(skill_id).unwrap_or_default();
            let mut evidence = PathEvidence::new(&store.get_all_targets().map_err(AppError::db)?);
            for adapter in &adapters {
                let target = deployment_target(adapter, &target_name, skill_id, &old_targets);
                if let Some(old) = old_targets.iter().find(|t| t.tool == adapter.key) {
                    let old_path = PathBuf::from(&old.target_path);
                    if old_path != target {
                        // As in `sync_desired_targets`: a path another tool
                        // still deploys to is its live deployment.
                        let claimed_by_another_tool = old_targets.iter().any(|other| {
                            other.tool != adapter.key
                                && PathBuf::from(&other.target_path) == old_path
                        });
                        if claimed_by_another_tool {
                            log::info!(
                                "Keeping {}: still the deployment target of another tool; \
                                 dropping {}'s stale record only",
                                old_path.display(),
                                adapter.key
                            );
                        } else {
                            release_abandoned_target(&old_path, &old.mode, &adapter.key);
                        }
                        let _ = store.delete_target(skill_id, &adapter.key);
                    }
                }

                let mode = sync_engine::sync_mode_for_tool(&adapter.key, configured_mode.as_deref());
                let same_path_record = old_targets.iter().find(|old| {
                    old.tool == adapter.key && PathBuf::from(&old.target_path) == target
                });
                if recorded_target_is_current(
                    same_path_record,
                    &source,
                    &target,
                    mode,
                    skill.content_hash.as_deref(),
                ) {
                    continue;
                }
                // As in `sync_desired_targets`: another agent sharing this
                // skills directory may hold a row that lets us adopt it.
                let recorded_mode = match deploy_step(
                    evidence.judge(skill_id, &adapter.key, &target),
                    &source,
                    &target,
                ) {
                    DeployStep::Write(mode) => mode,
                    DeployStep::Adopt(mode) => {
                        let record = adopted_target_record(
                            skill_id,
                            &adapter.key,
                            &target,
                            mode,
                            skill.content_hash.clone(),
                        );
                        if let Err(e) = store.insert_target(&record) {
                            log::warn!("Failed to insert adopted target for skill {skill_id}: {e}");
                        }
                        continue;
                    }
                };
                match sync_engine::sync_skill(
                    &source,
                    &target,
                    mode,
                    replace_policy(recorded_mode.as_deref()),
                ) {
                    Ok(actual_mode) => {
                        evidence.wrote(skill_id, &target, actual_mode);
                        let now = chrono::Utc::now().timestamp_millis();
                        let target_record = super::skill_store::SkillTargetRecord {
                            id: uuid::Uuid::new_v4().to_string(),
                            skill_id: skill_id.to_string(),
                            tool: adapter.key.clone(),
                            target_path: target.to_string_lossy().to_string(),
                            mode: actual_mode.as_str().to_string(),
                            status: "ok".to_string(),
                            synced_at: Some(now),
                            last_error: None,
                            source_hash: skill.content_hash.clone(),
                        };
                        if let Err(e) = store.insert_target(&target_record) {
                            log::warn!("Failed to insert sync target for skill {skill_id}: {e}");
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to sync skill {skill_id} to {}: {e}",
                            target.display()
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn ensure_default_startup_scenario(store: &SkillStore) -> Result<(), AppError> {
    let mut scenarios = store.get_all_scenarios().map_err(AppError::db)?;
    if scenarios.is_empty() {
        let now = chrono::Utc::now().timestamp_millis();
        let default_scenario = ScenarioRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Default".to_string(),
            description: Some("Default startup scenario".to_string()),
            icon: None,
            sort_order: 0,
            created_at: now,
            updated_at: now,
        };
        store.insert_scenario(&default_scenario).map_err(AppError::db)?;
        scenarios.push(default_scenario);
    }

    // Startup restores whatever preset was last active; there is no separate
    // "default startup preset" setting to override it.
    let current_active = store.get_active_scenario_id().map_err(AppError::db)?;

    let desired_active = current_active
        .clone()
        .filter(|id| scenarios.iter().any(|scenario| scenario.id == *id))
        .unwrap_or_else(|| scenarios[0].id.clone());

    if current_active.as_deref() != Some(desired_active.as_str()) {
        if let Some(old_active) = current_active.as_deref() {
            unsync_scenario_skills(store, old_active)?;
        }
        store
            .set_active_scenario(&desired_active)
            .map_err(AppError::db)?;
    }

    // Startup policy: a collision must never stop the app from launching. The
    // colliding skill simply is not deployed, its content is untouched, and the
    // workspace view shows it as not synced.
    let refusals = sync_scenario_skills(store, &desired_active)?;
    for refusal in &refusals {
        log::warn!("startup sync skipped a target: {refusal}");
    }
    Ok(())
}

pub fn ensure_cli_scenario_state(store: &SkillStore) -> Result<(), AppError> {
    let mut scenarios = store.get_all_scenarios().map_err(AppError::db)?;
    if scenarios.is_empty() {
        let now = chrono::Utc::now().timestamp_millis();
        let default_scenario = ScenarioRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Default".to_string(),
            description: Some("Default startup scenario".to_string()),
            icon: None,
            sort_order: 0,
            created_at: now,
            updated_at: now,
        };
        store.insert_scenario(&default_scenario).map_err(AppError::db)?;
        scenarios.push(default_scenario);
    }

    let current_active = store.get_active_scenario_id().map_err(AppError::db)?;
    if current_active
        .as_deref()
        .is_some_and(|id| scenarios.iter().any(|scenario| scenario.id == id))
    {
        return Ok(());
    }

    store
        .set_active_scenario(&scenarios[0].id)
        .map_err(AppError::db)
}

pub fn restore_all_skills_sync_included(store: &SkillStore) -> Result<bool, AppError> {
    let mut changed = false;
    for skill in store.get_all_skills().map_err(AppError::db)? {
        if !skill.enabled {
            store
                .update_skill_enabled(&skill.id, true)
                .map_err(AppError::db)?;
            changed = true;
        }
    }
    Ok(changed)
}

pub fn sync_active_scenario_to_tool(store: &SkillStore, tool_key: &str) {
    if let Ok(Some(active_id)) = store.get_active_scenario_id() {
        let Ok(skill_ids) = store.get_skill_ids_for_scenario(&active_id) else {
            return;
        };
        for skill_id in skill_ids {
            if let Ok(adapters) = enabled_installed_adapters_for_scenario_skill(store, &active_id, &skill_id)
            {
                if adapters.iter().any(|adapter| adapter.key == tool_key) {
                    let _ = sync_skill_to_active_scenario(store, &active_id, &skill_id);
                }
            }
        }
    }
}

/// Why a caller is writing to an agent's skills directory.
///
/// Adoption and stranded-target repair legitimately write over an existing
/// real directory that has no `skill_targets` row — that directory is the very
/// thing the user asked us to take over. Ordinary deployment must never do
/// that, so the two intents cannot share a code path (#363).
#[derive(Debug, Clone, Copy)]
pub enum DeployIntent<'a> {
    /// Ordinary deployment: replace only what our own records vouch for.
    Managed,
    /// The user explicitly asked us to take over this scanned skill path.
    AdoptExisting(&'a Path),
}

/// Re-point every `source_ref` that names `target` at the referring skill's own
/// central copy, ahead of an adoption that replaces `target` with a managed
/// deployment (#425).
///
/// An adopted directory stops being an independent copy the moment the
/// adoption runs: it becomes a link to (or copy of) central that can be
/// removed at any time — by the agent's own skill management cleaning up what
/// it does not recognize, or by a later undeploy. A skill whose `source_ref`
/// still names that directory then fails its update check with
/// `source_missing` forever, even though the central copy is intact.
///
/// The central copy always exists while the skill row does, so re-pointing
/// keeps the source resolvable; the content-hash comparison against it keeps
/// reporting `up_to_date`. Matching is by exact string or canonicalized path,
/// because import records can spell the same directory with mixed separators
/// than the deployment target computed from the adapter.
pub(crate) fn detach_source_refs_from_adoption_target(
    store: &SkillStore,
    target: &Path,
) -> Result<(), AppError> {
    let target_str = target.to_string_lossy().into_owned();
    // `canonicalize` resolves through symlinks, but a symlinked target is only
    // unlinked by the replacement below (`remove_classified_target` treats
    // LinkToSource/ForeignLink as `remove_link`) -- whatever it pointed at
    // survives untouched. Resolving through the link would re-point the
    // source_ref of a directory that is still on disk and still the user's
    // source of truth, and the update check would then compare central to
    // itself forever. An exact string match still applies: if the source_ref
    // *is* the link path, that link really is going away.
    let target_canonical = if std::fs::symlink_metadata(target)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false)
    {
        None
    } else {
        std::fs::canonicalize(target).ok()
    };
    for skill in store.get_all_skills().map_err(AppError::db)? {
        let Some(source_ref) = skill.source_ref.as_deref() else {
            continue;
        };
        // Already re-pointed by a previous adoption — nothing to do.
        if source_ref == skill.central_path {
            continue;
        }
        let points_at_target = source_ref == target_str
            || target_canonical.as_ref().is_some_and(|canonical| {
                std::fs::canonicalize(source_ref).is_ok_and(|source| &source == canonical)
            });
        if points_at_target {
            store
                .update_skill_source_ref(&skill.id, &skill.central_path)
                .map_err(AppError::db)?;
            log::info!(
                "adoption: re-pointed source_ref of skill '{}' at its central copy; \
                 the adopted agent directory is now a deployment, not a source (#425)",
                skill.name
            );
        }
    }
    Ok(())
}

pub fn sync_single_skill_to_tool(
    store: &SkillStore,
    skill_id: &str,
    tool: &str,
    intent: DeployIntent<'_>,
) -> Result<(), AppError> {
    let adapter = tool_adapters::find_adapter_with_store(store, tool)
        .ok_or_else(|| AppError::not_found(format!("Unknown tool: {}", tool)))?;

    if !adapter.is_installed() {
        return Err(AppError::not_found(format!(
            "{} is not installed",
            adapter.display_name
        )));
    }

    if tool_service::get_disabled_tools(store).contains(&tool.to_string()) {
        return Err(AppError::invalid_input(format!(
            "{} is disabled",
            adapter.display_name
        )));
    }

    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;

    let source = PathBuf::from(&skill.central_path);
    let existing_targets = store.get_targets_for_skill(skill_id).unwrap_or_default();
    let target = match intent {
        DeployIntent::AdoptExisting(path) => {
            let root = adapter.skills_dir();
            if !is_skill_path_within_root(path, &root) {
                return Err(AppError::invalid_input(
                    "Skill path is outside the agent skills directory",
                ));
            }
            path.to_path_buf()
        }
        DeployIntent::Managed => deployment_target(
            &adapter,
            &sync_engine::target_dir_name(&source, &skill.name),
            skill_id,
            &existing_targets,
        ),
    };
    let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
    let mode = sync_engine::sync_mode_for_tool(tool, configured_mode.as_deref());
    // Another agent sharing this skills directory may hold the only row for
    // the path (`PathEvidence`). That row lets this agent adopt an identical
    // copy, not replace one.
    let step = match intent {
        DeployIntent::AdoptExisting(_) => DeployStep::Write(None),
        DeployIntent::Managed => {
            let all_targets = store.get_all_targets().map_err(AppError::db)?;
            let evidence = PathEvidence::new(&all_targets).judge(skill_id, tool, &target);
            deploy_step(evidence, &source, &target)
        }
    };
    let recorded_mode = match step {
        DeployStep::Write(mode) => mode,
        DeployStep::Adopt(mode) => {
            let record =
                adopted_target_record(skill_id, tool, &target, mode, skill.content_hash.clone());
            return store.insert_target(&record).map_err(AppError::db);
        }
    };
    let policy = match intent {
        DeployIntent::AdoptExisting(_) => sync_engine::ReplacePolicy::UserConfirmed,
        DeployIntent::Managed => replace_policy(recorded_mode.as_deref()),
    };
    if matches!(intent, DeployIntent::AdoptExisting(_)) {
        // The directory at `target` may still be the import source of the very
        // skill being deployed (or of a sibling record): re-point those at
        // central BEFORE the replacement, so a failure partway through the
        // adoption can never leave a dangling source behind (#425).
        detach_source_refs_from_adoption_target(store, &target)?;
    }
    if matches!(intent, DeployIntent::Managed) {
        let same_path_record = existing_targets.iter().find(|existing| {
            existing.tool == tool && PathBuf::from(&existing.target_path) == target
        });
        if recorded_target_is_current(
            same_path_record,
            &source,
            &target,
            mode,
            skill.content_hash.as_deref(),
        ) {
            return Ok(());
        }
    }
    let actual_mode = sync_engine::sync_skill(&source, &target, mode, policy).map_err(AppError::io)?;

    let now = chrono::Utc::now().timestamp_millis();
    let target_record = SkillTargetRecord {
        id: uuid::Uuid::new_v4().to_string(),
        skill_id: skill_id.to_string(),
        tool: tool.to_string(),
        target_path: target.to_string_lossy().to_string(),
        mode: actual_mode.as_str().to_string(),
        status: "ok".to_string(),
        synced_at: Some(now),
        last_error: None,
        source_hash: skill.content_hash.clone(),
    };

    store.insert_target(&target_record).map_err(AppError::db)?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub enum BatchApplyMode {
    Add,
    Remove,
}

/// Record a deploy or removal of `skill_ids × tool_keys` in the active
/// preset's per-agent toggles, which the startup sync deploys from. Skills
/// outside the active preset are skipped; the active preset never changes.
/// Every launch reindexes the toggles from the sync metadata, so each changed
/// member's file is rewritten as well (#549).
pub fn set_active_preset_tool_toggles(
    store: &SkillStore,
    skill_ids: &[String],
    tool_keys: &[String],
    enabled: bool,
) -> Result<(), AppError> {
    let Ok(Some(active_id)) = store.get_active_scenario_id() else {
        return Ok(());
    };
    let members = store
        .get_skill_ids_for_scenario(&active_id)
        .map_err(AppError::db)?;
    let skill_ids: Vec<&String> = skill_ids.iter().filter(|id| members.contains(id)).collect();
    if skill_ids.is_empty() || tool_keys.is_empty() {
        return Ok(());
    }
    let adapter_keys: Vec<String> = tool_adapters::enabled_installed_adapters(store)
        .iter()
        .map(|a| a.key.clone())
        .collect();
    sync_metadata::with_repo_lock("set skill tool toggle", || {
        for skill_id in skill_ids {
            store.ensure_scenario_skill_tool_defaults(&active_id, skill_id, &adapter_keys)?;
            for tool in tool_keys {
                store.set_scenario_skill_tool_enabled(&active_id, skill_id, tool, enabled)?;
            }
            sync_metadata::write_scenario_skill_unlocked(store, &active_id, skill_id)?;
        }
        Ok(())
    })
    .map_err(AppError::db)
}

/// Apply a batch of `(skill_id × tool_key)` pairs in either Add or Remove mode
/// without changing `active_scenario_id`.
///
/// This is the tray-side preset apply primitive, also behind the CLI's
/// `presets deploy/undeploy` and `skills deploy/undeploy`. It also turns the
/// pairs on (Add) or off (Remove) in the active preset's toggles, for skills
/// in that preset. The startup sync deploys the active preset from those
/// toggles: one left on brought a removed skill back at the next launch, and
/// one left off kept a deployed skill out of the preview and out of the
/// startup repair (#549). The toggles go first, so a busy repo lock fails the
/// batch before any file changes, and a pair that then fails to deploy is
/// retried by the startup sync.
///
/// Remove mode handles shared physical paths: a `target_path` may be referenced
/// by multiple `(skill_id, tool)` records when several tools resolve to the same
/// skills directory. The filesystem path is only removed when no remaining
/// `skill_targets` row references it after the batch deletions, so removing one
/// preset's tools never wipes another tool's still-active files.
pub fn apply_skills_to_tools(
    store: &SkillStore,
    skill_ids: &[String],
    tool_keys: &[String],
    mode: BatchApplyMode,
) -> Result<(), AppError> {
    if skill_ids.is_empty() || tool_keys.is_empty() {
        return Ok(());
    }

    let add = matches!(mode, BatchApplyMode::Add);
    set_active_preset_tool_toggles(store, skill_ids, tool_keys, add)?;
    match mode {
        BatchApplyMode::Add => apply_add(store, skill_ids, tool_keys, false),
        BatchApplyMode::Remove => apply_remove(store, skill_ids, tool_keys),
    }
}

/// Check the same targets and ownership rules as batch deployment without writing.
pub fn preflight_add_skills_to_tools(
    store: &SkillStore,
    skill_ids: &[String],
    tool_keys: &[String],
) -> Result<(), AppError> {
    if skill_ids.is_empty() || tool_keys.is_empty() {
        return Ok(());
    }
    apply_add(store, skill_ids, tool_keys, true)
}

fn apply_add(
    store: &SkillStore,
    skill_ids: &[String],
    tool_keys: &[String],
    preflight_only: bool,
) -> Result<(), AppError> {
    let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
    let disabled = tool_service::get_disabled_tools(store);

    let mut adapters: HashMap<String, tool_adapters::ToolAdapter> = HashMap::new();
    for key in tool_keys {
        if disabled.contains(key) {
            log::debug!("apply_skills_to_tools: skipping disabled tool {key}");
            continue;
        }
        let Some(adapter) = tool_adapters::find_adapter_with_store(store, key) else {
            log::warn!("apply_skills_to_tools: unknown tool {key}");
            continue;
        };
        if !adapter.is_installed() {
            log::debug!(
                "apply_skills_to_tools: skipping uninstalled tool {} ({key})",
                adapter.display_name
            );
            continue;
        }
        adapters.insert(key.clone(), adapter);
    }

    // Plan every pair up front, then refuse the whole batch if any one of them
    // would destroy something (#363, expectation 4). Not a rollback — a
    // finished copy cannot be undone — but nothing is written until every pair
    // has been judged.
    struct PlannedPair {
        skill: crate::core::skill_store::SkillRecord,
        tool_key: String,
        source: PathBuf,
        target: PathBuf,
        mode: sync_engine::SyncMode,
    }

    let existing_targets = store.get_all_targets().map_err(AppError::db)?;
    let mut plan: Vec<PlannedPair> = Vec::new();
    for skill_id in skill_ids {
        let Ok(Some(skill)) = store.get_skill_by_id(skill_id) else {
            log::warn!("apply_skills_to_tools: skill {skill_id} not found");
            continue;
        };
        let source = PathBuf::from(&skill.central_path);
        let target_name = sync_engine::target_dir_name(&source, &skill.name);
        for (tool_key, adapter) in &adapters {
            plan.push(PlannedPair {
                skill: skill.clone(),
                tool_key: tool_key.clone(),
                mode: sync_engine::sync_mode_for_tool(tool_key, configured_mode.as_deref()),
                source: source.clone(),
                target: deployment_target(adapter, &target_name, skill_id, &existing_targets),
            });
        }
    }

    // Two skills whose central directories share a basename resolve to the
    // same deployment path (`target_dir_name` uses the basename only), so the
    // second would silently overwrite the first. Neither is the user's data,
    // but the result is a target whose contents don't match its record.
    // Two library skills planning onto one path is a naming problem, not a
    // user's directory being in the way — kept apart so the refusal that does
    // mean the latter is the only thing tagged as a target conflict.
    let mut duplicate_targets: Vec<String> = Vec::new();
    let mut conflicts: Vec<TargetConflictDetail> = Vec::new();
    let mut probe_errors: Vec<String> = Vec::new();
    // Keyed on skill id, not name: names are not unique, and two distinct
    // skills that happen to share one would otherwise slip through as "the
    // same skill" and silently overwrite each other.
    let mut planned_paths: HashMap<&Path, (&str, &str)> = HashMap::new();
    for pair in &plan {
        let entry = (pair.skill.id.as_str(), pair.skill.name.as_str());
        if let Some((first_id, first_name)) = planned_paths.insert(pair.target.as_path(), entry) {
            if first_id != pair.skill.id {
                duplicate_targets.push(format!(
                    "{} — skills \"{}\" and \"{}\" both deploy here",
                    pair.target.display(),
                    first_name,
                    pair.skill.name
                ));
            }
        }
    }
    if !duplicate_targets.is_empty() {
        return Err(AppError::invalid_input(format!(
            "Refusing to deploy: {} target path(s) are claimed by two different skills. \
             Nothing was changed. {}",
            duplicate_targets.len(),
            duplicate_targets.join("; ")
        )));
    }

    // Shared skills directories: a row of another agent on the same path
    // vouches for it, see `PathEvidence`.
    let mut evidence = PathEvidence::new(&existing_targets);
    let mut preflighted: HashSet<&Path> = HashSet::new();
    for pair in &plan {
        if !preflighted.insert(pair.target.as_path()) {
            continue;
        }
        if let Err(e) = sync_engine::preflight_replace(
            &pair.source,
            &pair.target,
            pair.mode,
            replace_policy(
                evidence
                    .judge(&pair.skill.id, &pair.tool_key, &pair.target)
                    .any_mode(),
            ),
        ) {
            // Keep the refusal's own path and reason: the caller may be an
            // agent that has to name the directory in the way and offer the
            // way out, which a flattened sentence cannot support.
            match e.downcast_ref::<sync_engine::ReplaceRefused>() {
                Some(refused) => conflicts.push(TargetConflictDetail {
                    path: refused.target.display().to_string(),
                    reason: refused.reason.to_string(),
                }),
                // Failing to even inspect the target (permissions, a broken
                // mount) is an IO problem. Reporting it as a conflict would
                // tell the caller to adopt or move content that may not exist.
                None => probe_errors.push(format!("{}: {e}", pair.target.display())),
            }
        }
    }
    if !probe_errors.is_empty() {
        return Err(AppError::io(format!(
            "Refusing to deploy: {} target(s) could not be inspected. Nothing was changed. {}",
            probe_errors.len(),
            probe_errors.join("; ")
        )));
    }
    if !conflicts.is_empty() {
        let summary = format!(
            "Refusing to deploy: {} of {} target(s) would overwrite content that is not ours. \
             Nothing was changed. {}",
            conflicts.len(),
            plan.len(),
            conflicts
                .iter()
                .map(|c| sync_engine::refusal_message(Path::new(&c.path), &c.reason))
                .collect::<Vec<_>>()
                .join("; ")
        );
        return Err(AppError::target_conflict(summary, conflicts));
    }
    if preflight_only {
        return Ok(());
    }

    let mut synced = 0usize;
    let mut failed = 0usize;
    let mut left = 0usize;
    for pair in &plan {
        let PlannedPair {
            skill,
            tool_key,
            source,
            target,
            mode,
        } = pair;
        let skill_id = &skill.id;
        // Several tools can resolve to one skills directory, so two pairs of
        // this batch may write the same path. What this batch just wrote there
        // is not this pair's record, so it must not be skipped as current.
        if !evidence.has_written(target) {
            let own_record = existing_targets.iter().find(|row| {
                row.skill_id == *skill_id
                    && row.tool == *tool_key
                    && Path::new(&row.target_path) == target
            });
            if recorded_target_is_current(
                own_record,
                source,
                target,
                *mode,
                skill.content_hash.as_deref(),
            ) {
                left += 1;
                continue;
            }
        }
        let recorded = evidence.judge(skill_id, tool_key, target);
        match sync_engine::sync_skill(source, target, *mode, replace_policy(recorded.any_mode())) {
            Ok(actual_mode) => {
                evidence.wrote(skill_id, target, actual_mode);
                let now = chrono::Utc::now().timestamp_millis();
                let target_record = SkillTargetRecord {
                    id: uuid::Uuid::new_v4().to_string(),
                    skill_id: skill_id.clone(),
                    tool: tool_key.clone(),
                    target_path: target.to_string_lossy().to_string(),
                    mode: actual_mode.as_str().to_string(),
                    status: "ok".to_string(),
                    synced_at: Some(now),
                    last_error: None,
                    source_hash: skill.content_hash.clone(),
                };
                if let Err(e) = store.insert_target(&target_record) {
                    log::warn!(
                        "apply_skills_to_tools: failed to insert target for skill {skill_id} / {tool_key}: {e}"
                    );
                    failed += 1;
                } else {
                    synced += 1;
                }
            }
            Err(e) => {
                failed += 1;
                log::warn!(
                    "apply_skills_to_tools: failed to sync skill {skill_id} ({}) to {}: {e}",
                    skill.name,
                    target.display()
                );
            }
        }
    }

    log::info!(
        "apply_skills_to_tools(Add): skills={} tools={} synced={synced} left={left} failed={failed}",
        skill_ids.len(),
        adapters.len(),
    );
    Ok(())
}

fn apply_remove(
    store: &SkillStore,
    skill_ids: &[String],
    tool_keys: &[String],
) -> Result<(), AppError> {
    let tool_set: HashSet<&String> = tool_keys.iter().collect();

    // Keep each row's recorded mode: it is the only evidence of what we put at
    // that path, and it has to survive until after the filesystem is inspected
    // (#363, expectation 5).
    let mut to_delete: Vec<(String, String, PathBuf, String)> = Vec::new();
    for skill_id in skill_ids {
        let targets = store.get_targets_for_skill(skill_id).unwrap_or_default();
        for target in targets {
            if tool_set.contains(&target.tool) {
                to_delete.push((
                    skill_id.clone(),
                    target.tool.clone(),
                    PathBuf::from(&target.target_path),
                    target.mode.clone(),
                ));
            }
        }
    }

    if to_delete.is_empty() {
        return Ok(());
    }

    // Rows go first, exactly as before: the recorded modes this batch needs are
    // already captured above, so deleting now costs no evidence, and re-reading
    // afterwards keeps the survivor set honest if any delete failed. Reading
    // must not fail open — treating a DB error as "nothing survives" would
    // authorize removing a path another tool is still using.
    for (skill_id, tool, _, _) in &to_delete {
        if let Err(e) = store.delete_target(skill_id, tool) {
            log::warn!(
                "apply_skills_to_tools(Remove): failed to delete target record for skill {skill_id} / {tool}: {e}"
            );
        }
    }

    // Must not fail open: treating a read error as "nothing survives" would
    // authorize deleting a path another tool still uses. It also must not fail
    // with an error, because the rows above are already gone — returning Err
    // here would leave callers thinking nothing happened. So on a read error,
    // keep every path and stop.
    // Keyed by directory entry, as deployment pools them: a surviving row that
    // reaches the same directory through a symlinked parent still uses it.
    let mut key_memo: HashMap<PathBuf, PathBuf> = HashMap::new();
    let still_referenced: HashSet<PathBuf> = match store.get_all_targets() {
        Ok(rows) => rows
            .into_iter()
            .map(|t| ownership_key(Path::new(&t.target_path), &mut key_memo))
            .collect(),
        Err(e) => {
            log::warn!(
                "apply_skills_to_tools(Remove): cannot verify remaining references ({e}); \
                 leaving every affected path on disk"
            );
            return Ok(());
        }
    };

    // One path may be claimed by several doomed rows. Their modes must agree
    // before anything is deleted: disagreement means we do not actually know
    // what we put there, and for a fix whose whole purpose is preservation,
    // ambiguous evidence has to preserve. (Taking whichever mode happens to
    // match would let a stale `copy` row authorize deleting a user directory
    // that replaced our symlink.)
    let mut candidates: Vec<(&PathBuf, Vec<&String>)> = Vec::new();
    let mut seen: HashMap<&PathBuf, usize> = HashMap::new();
    for (_, _, path, mode) in &to_delete {
        match seen.get(path) {
            Some(index) => candidates[*index].1.push(mode),
            None => {
                seen.insert(path, candidates.len());
                candidates.push((path, vec![mode]));
            }
        }
    }

    let mut removed = 0usize;
    let mut preserved = 0usize;
    for (path, recorded_modes) in candidates {
        if still_referenced.contains(&ownership_key(path, &mut key_memo)) {
            log::debug!(
                "apply_skills_to_tools(Remove): keeping {} (still referenced by another target)",
                path.display()
            );
            continue;
        }
        let mut modes: Vec<&str> = recorded_modes.iter().map(|m| m.as_str()).collect();
        modes.sort_unstable();
        modes.dedup();
        let outcome = match modes.as_slice() {
            [single] => sync_engine::remove_recorded_target(path, single),
            // Contradictory records: keep the object, drop the rows.
            _ => {
                log::warn!(
                    "apply_skills_to_tools(Remove): {} has conflicting records ({}); \
                     preserving it and removing the records only",
                    path.display(),
                    modes.join("/")
                );
                Ok(false)
            }
        };
        match outcome {
            Ok(true) => removed += 1,
            // Someone replaced our deployment with content of their own. The
            // record goes; the content stays.
            Ok(false) => {
                preserved += 1;
                log::warn!(
                    "apply_skills_to_tools(Remove): preserving {} — no longer matches its \
                     recorded deployment ({}); removing the record only",
                    path.display(),
                    recorded_modes
                        .iter()
                        .map(|m| m.as_str())
                        .collect::<Vec<_>>()
                        .join("/")
                );
            }
            Err(e) => {
                log::warn!(
                    "apply_skills_to_tools(Remove): failed to remove {}: {e}",
                    path.display()
                );
            }
        }
    }

    log::info!(
        "apply_skills_to_tools(Remove): pairs={} fs_removed={removed} preserved={preserved}",
        to_delete.len(),
    );
    Ok(())
}

#[cfg(test)]
mod sync_desired_targets_tests {
    use super::*;
    use crate::core::skill_store::{SkillRecord, SkillStore, SkillTargetRecord};
    use std::fs;
    use tempfile::tempdir;

    /// Two adapters can point at the same skills directory — `amp` and
    /// `replit` both deploy to `~/.config/agents/skills`, and `kimi` did too
    /// until it moved to `~/.kimi-code/skills` (#270). When one of them is
    /// retargeted, its stale record must be dropped, but the directory the
    /// others are still deployed to must survive: it is their live
    /// deployment, not this tool's leftover.
    #[test]
    fn retargeting_one_tool_keeps_a_directory_another_tool_still_claims() {
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();

        let source = central_repo::skills_dir().join("skill-a");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "real source").unwrap();

        // The shared deployment both tools were synced to.
        let shared = tmp.path().join("shared-agents").join("skill-a");
        fs::create_dir_all(&shared).unwrap();
        fs::write(shared.join("SKILL.md"), "real source").unwrap();

        // Where the retargeted tool is moving to.
        let moved = tmp.path().join("kimi-code").join("skill-a");

        let skill = SkillRecord {
            id: "skill-a".to_string(),
            name: "skill-a".to_string(),
            description: None,
            source_type: "import".to_string(),
            source_ref: Some(source.to_string_lossy().to_string()),
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: source.to_string_lossy().to_string(),
            content_hash: Some("h1".to_string()),
            enabled: true,
            created_at: 1,
            updated_at: 1,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: None,
            last_check_error: None,
        };
        store.insert_skill(&skill).unwrap();

        for (id, tool) in [("target-amp", "amp"), ("target-kimi", "kimi")] {
            store
                .insert_target(&SkillTargetRecord {
                    id: id.to_string(),
                    skill_id: "skill-a".to_string(),
                    tool: tool.to_string(),
                    target_path: shared.to_string_lossy().to_string(),
                    mode: "copy".to_string(),
                    status: "ok".to_string(),
                    synced_at: Some(1),
                    last_error: None,
                    source_hash: Some("h1".to_string()),
                })
                .unwrap();
        }

        // Adapter order puts amp before kimi, so amp is skipped as current
        // before kimi reaches its retarget branch.
        let desired = vec![
            ScenarioSyncTarget {
                skill_id: "skill-a".to_string(),
                skill_name: "skill-a".to_string(),
                tool: "amp".to_string(),
                source: source.clone(),
                target: shared.clone(),
                mode: sync_engine::SyncMode::Copy,
                source_hash: Some("h1".to_string()),
            },
            ScenarioSyncTarget {
                skill_id: "skill-a".to_string(),
                skill_name: "skill-a".to_string(),
                tool: "kimi".to_string(),
                source: source.clone(),
                target: moved.clone(),
                mode: sync_engine::SyncMode::Copy,
                source_hash: Some("h1".to_string()),
            },
        ];

        sync_desired_targets(&store, &desired).unwrap();

        assert!(
            shared.join("SKILL.md").exists(),
            "amp's live deployment was deleted while retargeting kimi"
        );
        assert!(
            moved.join("SKILL.md").exists(),
            "kimi was not deployed to its new path"
        );

        central_repo::set_test_base_dir_override(None);
    }


    /// Kept copies land under `kept-copies/`, whatever the tool key looks
    /// like, and never on top of an earlier one, dangling links included.
    #[test]
    fn kept_copies_stay_inside_kept_copies_and_never_collide() {
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let library = tmp.path().join("library");
        central_repo::set_test_base_dir_override(Some(library.clone()));
        let make_copy = |dir: &str| {
            let path = tmp.path().join(dir).join("skill-a");
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("notes.md"), dir).unwrap();
            path
        };

        // A CLI-created key with path syntax in it.
        let first = make_copy("agent-1");
        release_abandoned_target(&first, "copy", "../skills");
        let kept_root = library.join("kept-copies").join("___skills");
        assert_eq!(
            fs::read_to_string(kept_root.join("skill-a/notes.md")).unwrap(),
            "agent-1"
        );
        assert!(!library.join("skills").exists(), "escaped kept-copies/");

        #[cfg(unix)]
        std::os::unix::fs::symlink(tmp.path().join("gone"), kept_root.join("skill-a-2")).unwrap();
        let second = make_copy("agent-2");
        release_abandoned_target(&second, "copy", "../skills");
        assert!(!second.exists(), "the second copy was not moved");
        let landed = if cfg!(unix) { "skill-a-3" } else { "skill-a-2" };
        assert_eq!(
            fs::read_to_string(kept_root.join(landed).join("notes.md")).unwrap(),
            "agent-2"
        );

        central_repo::set_test_base_dir_override(None);
    }

    /// Startup must survive a collision. `ensure_default_startup_scenario`
    /// reaches this function through `sync_scenario_skills`, and its caller
    /// chain ends at `initialize_store().expect(...)` in lib.rs — so returning
    /// `Err` for an ownership refusal panicked the desktop app on launch under
    /// exactly the condition #363 exists to handle. The refusal must come back
    /// as data, with the user's directory untouched.
    #[test]
    fn ownership_refusal_is_reported_not_returned_as_error() {
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();

        let source = central_repo::skills_dir().join("skill-a");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "real source").unwrap();

        // An unmanaged directory sitting exactly where we want to deploy.
        let target = tmp.path().join("agent-skills").join("skill-a");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("unmanaged.txt"), "DO_NOT_OVERWRITE").unwrap();

        let desired = vec![ScenarioSyncTarget {
            skill_id: "skill-a".to_string(),
            skill_name: "skill-a".to_string(),
            tool: "claude-code".to_string(),
            source,
            target: target.clone(),
            mode: sync_engine::SyncMode::Symlink,
            source_hash: Some("h1".to_string()),
        }];

        let refusals = sync_desired_targets(&store, &desired)
            .expect("a refusal must not surface as Err: that panics app startup");
        assert_eq!(refusals.len(), 1, "{refusals:?}");
        assert!(
            refusals[0].to_string().contains("Refusing to replace"),
            "{refusals:?}"
        );
        // The path must survive as data, not only inside the sentence: an
        // agent driving the CLI has to name the directory that is in the way.
        assert_eq!(refusals[0].target, target);

        // ...and it must still be a path when it reaches the caller's error.
        let err = refusals_to_error(refusals).expect_err("a refusal must become an error here");
        assert_eq!(err.kind, crate::core::error::ErrorKind::TargetConflict);
        match err.details {
            Some(ref details) => {
                assert_eq!(details.conflicts.len(), 1);
                assert_eq!(details.conflicts[0].path, target.display().to_string());
            }
            None => panic!("expected structured target-conflict details"),
        }
        assert_eq!(
            fs::read_to_string(target.join("unmanaged.txt")).unwrap(),
            "DO_NOT_OVERWRITE"
        );

        central_repo::set_test_base_dir_override(None);
    }

    /// Issue #153 regression: when the existing target was written in
    /// Copy mode (Windows symlink fallback) but the configured mode is
    /// Symlink, and the source content hash hasn't changed, the sync
    /// must be skipped. Prior to the fix the mode-equality guard would
    /// reject the skip branch and re-attempt the full recursive copy
    /// every startup.
    #[test]
    fn copy_fallback_target_with_matching_hash_is_skipped() {
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();

        // Real source dir with one file (the central skill).
        let source = central_repo::skills_dir().join("skill-a");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "real source").unwrap();

        // Pre-existing target dir with a marker file that would be wiped
        // by copy_dir_recursive's pre-clean step if a re-sync ran.
        let target = tmp.path().join("agent-skills").join("skill-a");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("MARKER.txt"), "do not wipe me").unwrap();

        // DB rows: skill content_hash = "h1"; existing target also at "h1",
        // mode "copy" (i.e. previously fell back from Symlink).
        let skill = SkillRecord {
            id: "skill-a".to_string(),
            name: "skill-a".to_string(),
            description: None,
            source_type: "import".to_string(),
            source_ref: Some(source.to_string_lossy().to_string()),
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: source.to_string_lossy().to_string(),
            content_hash: Some("h1".to_string()),
            enabled: true,
            created_at: 1,
            updated_at: 1,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: None,
            last_check_error: None,
        };
        store.insert_skill(&skill).unwrap();

        store
            .insert_target(&SkillTargetRecord {
                id: "target-1".to_string(),
                skill_id: "skill-a".to_string(),
                tool: "claude-code".to_string(),
                target_path: target.to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: Some("h1".to_string()),
            })
            .unwrap();

        // Desired target: same source/target/hash but Symlink mode
        // (the configured default that originally fell back to Copy).
        let desired = vec![ScenarioSyncTarget {
            skill_id: "skill-a".to_string(),
            skill_name: "skill-a".to_string(),
            tool: "claude-code".to_string(),
            source: source.clone(),
            target: target.clone(),
            mode: sync_engine::SyncMode::Symlink,
            source_hash: Some("h1".to_string()),
        }];

        sync_desired_targets(&store, &desired).unwrap();

        // The marker file proves no re-sync ran (a real re-sync would
        // have called copy_dir_recursive after wiping the target).
        assert!(
            target.join("MARKER.txt").exists(),
            "target dir was wiped — skip did not fire"
        );
        // The skill's actual SKILL.md should NOT have been copied in,
        // because we skipped the sync entirely.
        assert!(
            !target.join("SKILL.md").exists(),
            "SKILL.md appeared — sync ran instead of skipping"
        );

        central_repo::set_test_base_dir_override(None);
    }

    /// Companion: if the target has been manually deleted, even with a
    /// matching hash, we must NOT skip — the user's agent dir is
    /// otherwise left broken.
    #[test]
    fn deleted_target_with_matching_hash_forces_resync() {
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();

        let source = central_repo::skills_dir().join("skill-b");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "real source").unwrap();

        // Target path that does NOT exist on disk.
        let target = tmp.path().join("agent-skills").join("skill-b");

        let skill = SkillRecord {
            id: "skill-b".to_string(),
            name: "skill-b".to_string(),
            description: None,
            source_type: "import".to_string(),
            source_ref: Some(source.to_string_lossy().to_string()),
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: source.to_string_lossy().to_string(),
            content_hash: Some("h1".to_string()),
            enabled: true,
            created_at: 1,
            updated_at: 1,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: None,
            last_check_error: None,
        };
        store.insert_skill(&skill).unwrap();

        store
            .insert_target(&SkillTargetRecord {
                id: "target-2".to_string(),
                skill_id: "skill-b".to_string(),
                tool: "claude-code".to_string(),
                target_path: target.to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: Some("h1".to_string()),
            })
            .unwrap();

        let desired = vec![ScenarioSyncTarget {
            skill_id: "skill-b".to_string(),
            skill_name: "skill-b".to_string(),
            tool: "claude-code".to_string(),
            source: source.clone(),
            target: target.clone(),
            mode: sync_engine::SyncMode::Copy,
            source_hash: Some("h1".to_string()),
        }];

        sync_desired_targets(&store, &desired).unwrap();

        // Sync must have run — target should now exist with the source content.
        assert!(target.join("SKILL.md").exists(), "missing target was not re-synced");

        central_repo::set_test_base_dir_override(None);
    }

    /// `canonicalize` resolves through symlinks, but the replacement that
    /// follows an adoption does not: a symlinked target is only unlinked, so
    /// whatever it pointed at survives. Re-pointing on the resolved path would
    /// silently orphan a directory that is still on disk and still the user's
    /// source of truth -- later edits in it would never be seen again (#425).
    #[cfg(unix)]
    #[test]
    fn a_symlinked_adoption_target_does_not_detach_the_directory_it_points_at() {
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();

        let central = central_repo::skills_dir().join("jira");
        fs::create_dir_all(&central).unwrap();
        fs::write(central.join("SKILL.md"), "central copy").unwrap();

        // The folder the skill was imported from: still the user's source.
        let import_src = tmp.path().join("claude").join("skills").join("jira");
        fs::create_dir_all(&import_src).unwrap();
        fs::write(import_src.join("SKILL.md"), "user copy").unwrap();

        // Another agent's skills dir holds a symlink to that same folder.
        let link = tmp.path().join("duo").join("skills").join("jira");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&import_src, &link).unwrap();

        store
            .insert_skill(&SkillRecord {
                id: "jira".to_string(),
                name: "jira".to_string(),
                description: None,
                source_type: "import".to_string(),
                source_ref: Some(import_src.to_string_lossy().to_string()),
                source_ref_resolved: None,
                source_subpath: None,
                source_branch: None,
                source_revision: None,
                remote_revision: None,
                central_path: central.to_string_lossy().to_string(),
                content_hash: Some("h1".to_string()),
                enabled: true,
                created_at: 1,
                updated_at: 1,
                status: "ok".to_string(),
                update_status: "local_only".to_string(),
                last_checked_at: None,
                last_check_error: None,
            })
            .unwrap();

        // Adopting the link must leave the pointee's source_ref alone.
        detach_source_refs_from_adoption_target(&store, &link).unwrap();
        let after = store.get_skill_by_id("jira").unwrap().unwrap();
        assert_eq!(
            after.source_ref.as_deref(),
            Some(import_src.to_string_lossy().as_ref()),
            "a symlinked target detached the real directory it points at"
        );

        // But adopting the real directory still detaches it: that one is
        // about to be replaced by a deployment.
        detach_source_refs_from_adoption_target(&store, &import_src).unwrap();
        let after = store.get_skill_by_id("jira").unwrap().unwrap();
        assert_eq!(
            after.source_ref.as_deref(),
            Some(central.to_string_lossy().as_ref()),
            "adopting the real source dir no longer re-points source_ref"
        );

        central_repo::set_test_base_dir_override(None);
    }
}

#[cfg(test)]
mod skip_check_mode_tests {
    use super::skip_check_mode;
    use super::sync_engine::SyncMode;

    #[test]
    fn matching_modes_are_compatible() {
        assert!(matches!(
            skip_check_mode("symlink", SyncMode::Symlink),
            Some(SyncMode::Symlink)
        ));
        assert!(matches!(
            skip_check_mode("copy", SyncMode::Copy),
            Some(SyncMode::Copy)
        ));
    }

    #[test]
    fn copy_existing_with_symlink_desired_treated_as_copy() {
        // Windows fallback case (issue #153): record says copy because
        // symlink_dir failed previously. We accept that and let the hash
        // gate decide freshness, instead of re-attempting symlink and
        // triggering a full recopy on every startup.
        assert!(matches!(
            skip_check_mode("copy", SyncMode::Symlink),
            Some(SyncMode::Copy)
        ));
    }

    #[test]
    fn symlink_existing_with_copy_desired_is_incompatible() {
        // User flipped sync_mode setting from symlink to copy — the
        // on-disk symlink no longer reflects intent, must resync.
        assert!(skip_check_mode("symlink", SyncMode::Copy).is_none());
    }

    #[test]
    fn unknown_existing_mode_is_incompatible() {
        assert!(skip_check_mode("garbage", SyncMode::Symlink).is_none());
        assert!(skip_check_mode("", SyncMode::Copy).is_none());
    }
}
