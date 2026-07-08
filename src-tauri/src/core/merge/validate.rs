//! Merged-tree validation (design §7). Pure checking, deliberately
//! independent of (and stricter than) reindex: any violation aborts the
//! whole merge with zero changes. Self-healing corrections (orphan drops)
//! happen earlier, as tree-build inputs — never here.

use anyhow::{Context, Result, bail};
use git2::{ObjectType, Repository, Tree};
use std::collections::{BTreeMap, BTreeSet};

use super::protocol::ProtocolFile;
use super::snapshot::{MAX_SKILL_DEPTH, METADATA_DIR, tree_is_valid_skill_dir};
use crate::core::sync_metadata::{SkillMetaFile, path_key};

/// Skill directories in `tree` that no metadata claims. Used to grandfather
/// legacy dirt: dirs that were already unclaimed in a merge INPUT (committed
/// long ago by old app versions or by hand) must not brick every future
/// merge — the invariant defends against merges *introducing* orphans, not
/// against pre-existing ones. Best-effort: unparsable metadata files simply
/// contribute no claim here (the strict checks still run on the merged tree).
pub fn unclaimed_skill_dirs(repo: &Repository, tree: &Tree) -> Result<BTreeSet<String>> {
    let mut claimed: BTreeSet<String> = BTreeSet::new();
    if let Some(meta_entry) = tree.get_name(METADATA_DIR) {
        if let Ok(meta_tree) = repo.find_tree(meta_entry.id()) {
            if let Ok(skills_tree) = subtree(repo, &meta_tree, "skills") {
                for entry in skills_tree.iter() {
                    if let Ok(blob) = repo.find_blob(entry.id()) {
                        if let Ok(meta) = serde_json::from_slice::<SkillMetaFile>(blob.content())
                        {
                            claimed.insert(meta.path);
                        }
                    }
                }
            }
        }
    }
    let claimed_refs: BTreeSet<&str> = claimed.iter().map(String::as_str).collect();
    let mut out = BTreeSet::new();
    collect_unclaimed(repo, tree, "", &claimed_refs, 0, &mut out)?;
    Ok(out)
}

pub fn validate_merged_tree(
    repo: &Repository,
    tree: &Tree,
    tolerated_unclaimed: &BTreeSet<String>,
) -> Result<()> {
    validate_tree(repo, tree, tolerated_unclaimed, false)
}

/// Validation for a merge INPUT tip (old-client checks): same rules, but
/// metadata-namespace junk (committed `.tmp.` leftovers etc.) is legacy dirt
/// the plan stage drops anyway — a tip carrying it must not hard-block. The
/// merged tree itself stays strict: the planner guarantees junk-free output,
/// so junk there means an engine bug.
pub fn validate_input_tip(
    repo: &Repository,
    tree: &Tree,
    tolerated_unclaimed: &BTreeSet<String>,
) -> Result<()> {
    validate_tree(repo, tree, tolerated_unclaimed, true)
}

fn validate_tree(
    repo: &Repository,
    tree: &Tree,
    tolerated_unclaimed: &BTreeSet<String>,
    tolerate_metadata_junk: bool,
) -> Result<()> {
    let meta_tree = subtree(repo, tree, METADATA_DIR)
        .context("merged tree validation: .skills-manager missing")?;

    // 1. schema.json / protocol.json exist and parse.
    let schema_blob = blob_of(repo, &meta_tree, "schema.json")
        .context("merged tree validation: schema.json missing")?;
    serde_json::from_slice::<serde_json::Value>(&schema_blob)
        .context("merged tree validation: schema.json unparsable")?;
    let protocol_blob = blob_of(repo, &meta_tree, "protocol.json")
        .context("merged tree validation: protocol.json missing")?;
    serde_json::from_slice::<ProtocolFile>(&protocol_blob)
        .context("merged tree validation: protocol.json unparsable")?;

    // 2. every skills/{id}.json parses, path_key matches and is unique.
    let mut metas: BTreeMap<String, SkillMetaFile> = BTreeMap::new();
    if let Ok(skills_tree) = subtree(repo, &meta_tree, "skills") {
        let mut seen_keys: BTreeMap<String, String> = BTreeMap::new();
        for entry in skills_tree.iter() {
            let name = entry.name().unwrap_or_default().to_string();
            let Some(stem) = name.strip_suffix(".json") else {
                if tolerate_metadata_junk
                    && super::decision::is_metadata_namespace_junk(&format!(
                        "{METADATA_DIR}/skills/{name}"
                    ))
                {
                    continue;
                }
                bail!("merged tree validation: unexpected file skills/{name}");
            };
            let raw = repo
                .find_blob(entry.id())
                .with_context(|| format!("skills/{name} is not a blob"))?
                .content()
                .to_vec();
            let meta: SkillMetaFile = serde_json::from_slice(&raw)
                .with_context(|| format!("merged tree validation: skills/{name} unparsable"))?;
            if meta.skill_id != stem {
                bail!("merged tree validation: skills/{name} skill_id mismatch");
            }
            if meta.path_key != path_key(&meta.path) {
                bail!(
                    "merged tree validation: {} path_key does not match path '{}'",
                    meta.skill_id,
                    meta.path
                );
            }
            if let Some(previous) = seen_keys.insert(meta.path_key.clone(), meta.skill_id.clone())
            {
                bail!(
                    "merged tree validation: folded path collision between {} and {}",
                    previous,
                    meta.skill_id
                );
            }
            metas.insert(stem.to_string(), meta);
        }
    }

    // 3. every metadata path exists, is a valid skill dir, depth ≤ 6; no
    //    claimed path may nest inside another claimed path.
    let claimed: BTreeSet<&str> = metas.values().map(|m| m.path.as_str()).collect();
    for meta in metas.values() {
        let depth = meta.path.split('/').count();
        if depth > MAX_SKILL_DEPTH || meta.path.is_empty() || meta.path.starts_with('.') {
            bail!(
                "merged tree validation: {} path '{}' invalid or too deep",
                meta.skill_id,
                meta.path
            );
        }
        let entry = tree
            .get_path(std::path::Path::new(&meta.path))
            .with_context(|| {
                format!(
                    "merged tree validation: {} path '{}' missing from tree",
                    meta.skill_id, meta.path
                )
            })?;
        if entry.kind() != Some(ObjectType::Tree) {
            bail!(
                "merged tree validation: {} path '{}' is not a directory",
                meta.skill_id,
                meta.path
            );
        }
        let dir = repo.find_tree(entry.id())?;
        if !tree_is_valid_skill_dir(&dir) {
            bail!(
                "merged tree validation: {} path '{}' is not a valid skill dir",
                meta.skill_id,
                meta.path
            );
        }
        for other in &claimed {
            if other.len() > meta.path.len()
                && other.starts_with(meta.path.as_str())
                && other.as_bytes()[meta.path.len()] == b'/'
            {
                bail!(
                    "merged tree validation: claimed path '{}' nests inside '{}'",
                    other,
                    meta.path
                );
            }
        }
    }

    // 4. every valid skill dir at any depth (dot-dirs excluded) is claimed
    //    by exactly one metadata entry — except dirs grandfathered by the
    //    caller (already unclaimed in a merge input; the merge must not
    //    INTRODUCE orphans, but pre-existing legacy dirt syncs through).
    //    Claimed dirs are not descended: a SKILL.md in a skill's own
    //    subfolder belongs to that skill.
    let mut unclaimed = BTreeSet::new();
    collect_unclaimed(repo, tree, "", &claimed, 0, &mut unclaimed)?;
    for dir in unclaimed {
        if !tolerated_unclaimed.contains(&dir) {
            bail!(
                "merged tree validation: unclaimed skill directory '{}' (no metadata owns it)",
                dir
            );
        }
    }

    // 5. membership / scenario reference completeness.
    let mut scenario_ids: BTreeSet<String> = BTreeSet::new();
    if let Ok(scenarios_tree) = subtree(repo, &meta_tree, "scenarios") {
        for entry in scenarios_tree.iter() {
            let name = entry.name().unwrap_or_default();
            if let Some(stem) = name.strip_suffix(".json") {
                scenario_ids.insert(stem.to_string());
            }
        }
    }
    if let Ok(members_tree) = subtree(repo, &meta_tree, "scenario-skills") {
        for dir in members_tree.iter() {
            let sid = dir.name().unwrap_or_default().to_string();
            if !scenario_ids.contains(&sid) {
                bail!("merged tree validation: membership references unknown scenario {sid}");
            }
            if dir.kind() != Some(ObjectType::Tree) {
                bail!("merged tree validation: scenario-skills/{sid} is not a directory");
            }
            let dt = repo.find_tree(dir.id())?;
            for entry in dt.iter() {
                let name = entry.name().unwrap_or_default();
                let Some(skid) = name.strip_suffix(".json") else {
                    continue;
                };
                if !metas.contains_key(skid) {
                    bail!(
                        "merged tree validation: membership {sid}/{skid} references unknown skill"
                    );
                }
            }
        }
    }

    Ok(())
}

fn subtree<'r>(repo: &'r Repository, tree: &Tree, name: &str) -> Result<Tree<'r>> {
    let entry = tree
        .get_name(name)
        .with_context(|| format!("missing {name}"))?;
    repo.find_tree(entry.id())
        .with_context(|| format!("{name} is not a directory"))
}

fn blob_of(repo: &Repository, tree: &Tree, name: &str) -> Result<Vec<u8>> {
    let entry = tree
        .get_name(name)
        .with_context(|| format!("missing {name}"))?;
    Ok(repo
        .find_blob(entry.id())
        .with_context(|| format!("{name} is not a file"))?
        .content()
        .to_vec())
}

fn collect_unclaimed(
    repo: &Repository,
    tree: &Tree,
    prefix: &str,
    claimed: &BTreeSet<&str>,
    depth: usize,
    out: &mut BTreeSet<String>,
) -> Result<()> {
    if depth >= MAX_SKILL_DEPTH {
        // Deliberately mirrors reconcile's WalkDir max_depth(6) horizon: a
        // SKILL.md deeper than 6 components is not a skill anywhere in the
        // system — reconcile never adopts it, merges treat its files as
        // residual — so it is not an unclaimed-dir violation either.
        return Ok(());
    }
    for entry in tree.iter() {
        if entry.kind() != Some(ObjectType::Tree) {
            continue;
        }
        let name = entry.name().unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        if claimed.contains(path.as_str()) {
            continue; // this dir belongs to a skill; do not descend
        }
        let dir = repo.find_tree(entry.id())?;
        if tree_is_valid_skill_dir(&dir) {
            out.insert(path);
            continue;
        }
        collect_unclaimed(repo, &dir, &path, claimed, depth + 1, out)?;
    }
    Ok(())
}
