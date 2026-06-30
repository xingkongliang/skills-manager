use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::content_hash;
use super::skill_metadata;
use super::skill_store::DiscoveredSkillRecord;
use super::tool_adapters;

pub struct ScanPlan {
    pub tools_scanned: usize,
    pub skills_found: usize,
    pub discovered: Vec<DiscoveredSkillRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredGroup {
    pub name: String,
    pub fingerprint: Option<String>,
    pub collection: Option<String>,
    pub collection_url: Option<String>,
    pub locations: Vec<DiscoveredLocation>,
    pub imported: bool,
    pub found_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredLocation {
    pub id: String,
    pub tool: String,
    pub found_path: String,
    pub is_symlink: bool,
    pub link_target: Option<String>,
    pub collection: Option<String>,
    pub collection_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillCollection {
    pub(crate) source: String,
    pub(crate) source_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SkillLockFile {
    skills: std::collections::HashMap<String, SkillLockEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillLockEntry {
    source: Option<String>,
    source_url: Option<String>,
}

/// Directories to skip during recursive scans (internal/tool-specific metadata).
const RECURSIVE_SCAN_SKIP_DIRS: &[&str] = &[".hub", ".git", "node_modules"];

fn is_symlink_to_central(path: &Path) -> bool {
    if let Ok(target) = std::fs::read_link(path) {
        let central = super::central_repo::skills_dir();
        return target.starts_with(&central);
    }
    false
}

/// Recursively walk `dir` and return all subdirectories that contain SKILL.md.
/// Stops descending when a skill dir is found (skills don't nest). Skips
/// `.git` / `node_modules` / `.hub` and guards against symlink cycles.
pub fn collect_skill_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    let mut visited = HashSet::new();
    collect_skill_dirs_recursive(dir, &mut visited, &mut results);
    results
}

fn collect_skill_dirs_recursive(
    dir: &Path,
    visited: &mut HashSet<PathBuf>,
    results: &mut Vec<PathBuf>,
) {
    let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if !visited.insert(canonical) {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() && !path.is_symlink() {
            continue;
        }
        let dir_name = entry.file_name();
        let dir_name_str = dir_name.to_string_lossy();
        if RECURSIVE_SCAN_SKIP_DIRS.iter().any(|s| dir_name_str == *s) {
            continue;
        }
        if is_symlink_to_central(&path) {
            continue;
        }
        if skill_metadata::is_valid_skill_dir(&path) {
            results.push(path);
            continue;
        }
        collect_skill_dirs_recursive(&path, visited, results);
    }
}

/// Build a `DiscoveredSkillRecord` for `path` and push it onto `discovered`,
/// unless `path` is already tracked in `managed_paths`.
fn push_discovered(
    adapter_key: &str,
    path: PathBuf,
    managed_paths: &[String],
    discovered: &mut Vec<DiscoveredSkillRecord>,
) {
    let path_str = path.to_string_lossy().to_string();
    if managed_paths.contains(&path_str) {
        return;
    }
    let name = skill_metadata::infer_skill_name(&path);
    let fingerprint = content_hash::hash_directory(&path).ok();
    let found_at = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    discovered.push(DiscoveredSkillRecord {
        id: uuid::Uuid::new_v4().to_string(),
        tool: adapter_key.to_string(),
        found_path: path_str,
        name_guess: Some(name),
        fingerprint,
        found_at,
        imported_skill_id: None,
    });
}

fn scan_flat_dir(
    adapter_key: &str,
    scan_dir: &Path,
    managed_paths: &[String],
    discovered: &mut Vec<DiscoveredSkillRecord>,
) {
    let entries = match std::fs::read_dir(scan_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() && !path.is_symlink() {
            continue;
        }
        if is_symlink_to_central(&path) || !skill_metadata::is_valid_skill_dir(&path) {
            continue;
        }
        push_discovered(adapter_key, path, managed_paths, discovered);
    }
}

fn scan_recursive_dir(
    adapter_key: &str,
    scan_dir: &Path,
    managed_paths: &[String],
    discovered: &mut Vec<DiscoveredSkillRecord>,
) {
    let mut skill_dirs = Vec::new();
    let mut visited = HashSet::new();
    collect_skill_dirs_recursive(scan_dir, &mut visited, &mut skill_dirs);
    for path in skill_dirs {
        push_discovered(adapter_key, path, managed_paths, discovered);
    }
}

#[allow(dead_code)]
pub fn scan_local_skills(managed_paths: &[String]) -> Result<ScanPlan> {
    scan_local_skills_with_adapters(managed_paths, &tool_adapters::default_tool_adapters())
}

pub fn scan_local_skills_with_adapters(
    managed_paths: &[String],
    adapters: &[tool_adapters::ToolAdapter],
) -> Result<ScanPlan> {
    let mut discovered = Vec::new();
    let mut tools_scanned = 0;

    for adapter in adapters {
        let installed = adapter.is_installed();
        let additional_dirs = adapter.additional_existing_scan_dirs();

        // Discover via additional_scan_dirs even when the legacy detect dir is
        // missing — handles tools whose skills land in a shared location
        // (e.g. copilot → ~/.agents/skills) on machines without the legacy
        // vendor dir.
        if !installed && additional_dirs.is_empty() {
            continue;
        }

        tools_scanned += 1;

        if installed {
            let primary_scan_dir = adapter.skills_dir();
            if primary_scan_dir.exists() {
                if adapter.recursive_scan {
                    scan_recursive_dir(
                        &adapter.key,
                        &primary_scan_dir,
                        managed_paths,
                        &mut discovered,
                    );
                } else {
                    scan_flat_dir(
                        &adapter.key,
                        &primary_scan_dir,
                        managed_paths,
                        &mut discovered,
                    );
                }
            }
        }

        // Additional scan dirs are already resolved to concrete skills roots.
        for scan_dir in additional_dirs {
            scan_flat_dir(&adapter.key, &scan_dir, managed_paths, &mut discovered);
        }
    }

    let skills_found = discovered.len();
    Ok(ScanPlan {
        tools_scanned,
        skills_found,
        discovered,
    })
}

pub fn group_discovered(records: &[DiscoveredSkillRecord]) -> Vec<DiscoveredGroup> {
    group_discovered_with_registry(records, &HashMap::new())
}

pub(crate) fn group_discovered_with_registry(
    records: &[DiscoveredSkillRecord],
    registry: &HashMap<String, SkillCollection>,
) -> Vec<DiscoveredGroup> {
    let mut groups: HashMap<String, DiscoveredGroup> = HashMap::new();
    let prefix_counts = repeated_collection_prefix_counts(records);

    for rec in records {
        let name = rec.name_guess.clone().unwrap_or_else(|| "unknown".into());
        let collection = collection_for_record(&name, &rec.found_path, registry, &prefix_counts);
        let link_info = link_info_for_path(&rec.found_path);
        let group_key = if let Some(fingerprint) = rec.fingerprint.as_deref() {
            format!("fp:{name}:{fingerprint}")
        } else {
            format!("path:{name}:{}", rec.found_path)
        };
        let entry = groups.entry(group_key).or_insert_with(|| DiscoveredGroup {
            name,
            fingerprint: rec.fingerprint.clone(),
            collection: collection.as_ref().map(|c| c.source.clone()),
            collection_url: collection.as_ref().and_then(|c| c.source_url.clone()),
            locations: Vec::new(),
            imported: false,
            found_at: rec.found_at,
        });
        if entry.collection.is_none() {
            entry.collection = collection.as_ref().map(|c| c.source.clone());
            entry.collection_url = collection.as_ref().and_then(|c| c.source_url.clone());
        }

        if rec.imported_skill_id.is_some() {
            entry.imported = true;
        }

        // Use the earliest found_at
        if rec.found_at < entry.found_at {
            entry.found_at = rec.found_at;
        }

        entry.locations.push(DiscoveredLocation {
            id: rec.id.clone(),
            tool: rec.tool.clone(),
            found_path: rec.found_path.clone(),
            is_symlink: link_info.is_symlink,
            link_target: link_info.target,
            collection: collection.as_ref().map(|c| c.source.clone()),
            collection_url: collection.as_ref().and_then(|c| c.source_url.clone()),
        });
    }

    let mut result: Vec<_> = groups.into_values().collect();
    for group in &mut result {
        group.locations.sort_by(|a, b| {
            a.is_symlink
                .cmp(&b.is_symlink)
                .then_with(|| a.tool.cmp(&b.tool))
                .then_with(|| a.found_path.cmp(&b.found_path))
        });
    }
    result.sort_by(|a, b| match (&a.collection, &b.collection) {
        (Some(a_collection), Some(b_collection)) => a_collection
            .cmp(b_collection)
            .then_with(|| a.name.cmp(&b.name)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.name.cmp(&b.name),
    });
    result
}

fn collection_for_record(
    name: &str,
    path: &str,
    registry: &HashMap<String, SkillCollection>,
    prefix_counts: &HashMap<String, usize>,
) -> Option<SkillCollection> {
    skill_collection_for_path(path)
        .or_else(|| registry.get(name).cloned())
        .or_else(|| prefix_collection_for_name(name, prefix_counts))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LinkInfo {
    is_symlink: bool,
    target: Option<String>,
}

fn link_info_for_path(path: &str) -> LinkInfo {
    let path = Path::new(path);
    let is_symlink = std::fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false);
    if !is_symlink {
        return LinkInfo {
            is_symlink: false,
            target: None,
        };
    }

    LinkInfo {
        is_symlink: true,
        target: resolve_link_target(path).map(|target| target.to_string_lossy().into_owned()),
    }
}

fn resolve_link_target(path: &Path) -> Option<PathBuf> {
    let target = std::fs::read_link(path).ok()?;
    if target.is_absolute() {
        Some(target)
    } else {
        Some(path.parent().unwrap_or_else(|| Path::new("")).join(target))
    }
}

fn skill_collection_for_path(path: &str) -> Option<SkillCollection> {
    let path = Path::new(path);
    skill_collection_for_physical_path(path).or_else(|| {
        resolve_link_target(path)
            .as_deref()
            .and_then(skill_collection_for_physical_path)
    })
}

fn skill_collection_for_physical_path(path: &Path) -> Option<SkillCollection> {
    let skill_name = path.file_name()?.to_str()?;
    let mut dir = path.parent();
    while let Some(current) = dir {
        let lock_path = current.join(".skill-lock.json");
        if let Some(collection) = read_skill_collection_from_lock(&lock_path, skill_name) {
            return Some(collection);
        }
        dir = current.parent();
    }
    None
}

fn read_skill_collection_from_lock(lock_path: &Path, skill_name: &str) -> Option<SkillCollection> {
    let content = std::fs::read_to_string(lock_path).ok()?;
    let lock: SkillLockFile = serde_json::from_str(&content).ok()?;
    let entry = lock.skills.get(skill_name)?;
    let source = entry.source.as_deref()?.trim();
    if source.is_empty() {
        return None;
    }
    Some(SkillCollection {
        source: source.to_string(),
        source_url: entry
            .source_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    })
}

fn repeated_collection_prefix_counts(records: &[DiscoveredSkillRecord]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for name in records.iter().filter_map(|rec| rec.name_guess.as_deref()) {
        if let Some(prefix) = collection_prefix_for_name(name) {
            *counts.entry(prefix).or_insert(0) += 1;
        }
    }
    counts
}

pub(crate) fn repeated_collection_prefixes(records: &[DiscoveredSkillRecord]) -> Vec<String> {
    let mut prefixes: Vec<_> = repeated_collection_prefix_counts(records)
        .into_iter()
        .filter_map(|(prefix, count)| (count >= 2).then_some(prefix))
        .collect();
    prefixes.sort();
    prefixes
}

fn prefix_collection_for_name(
    name: &str,
    prefix_counts: &HashMap<String, usize>,
) -> Option<SkillCollection> {
    let prefix = collection_prefix_for_name(name)?;
    if prefix_counts.get(&prefix).copied().unwrap_or(0) < 2 {
        return None;
    }
    Some(SkillCollection {
        source: prefix,
        source_url: None,
    })
}

fn collection_prefix_for_name(name: &str) -> Option<String> {
    let (prefix, _) = name.split_once('-')?;
    let prefix = prefix.trim();
    if prefix.len() < 3 || !prefix.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(prefix.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_skill(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("SKILL.md"), "---\nname: x\n---\n# x").unwrap();
    }

    fn run(root: &Path) -> Vec<PathBuf> {
        let mut results = Vec::new();
        let mut visited = HashSet::new();
        collect_skill_dirs_recursive(root, &mut visited, &mut results);
        results.sort();
        results
    }

    #[test]
    fn recursive_finds_nested_skills() {
        let tmp = tempdir().unwrap();
        write_skill(&tmp.path().join("devops/deploy-k8s"));
        write_skill(&tmp.path().join("software-development/super-dev"));

        let results = run(tmp.path());
        assert_eq!(results.len(), 2);
        let names: Vec<_> = results
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
            .collect();
        assert!(names.contains(&"deploy-k8s"));
        assert!(names.contains(&"super-dev"));
    }

    #[test]
    fn recursive_stops_descending_into_skill_dir() {
        // A skill dir's own subdirectories must not be reported as separate skills,
        // even if they happen to contain their own SKILL.md.
        let tmp = tempdir().unwrap();
        write_skill(&tmp.path().join("my-skill"));
        write_skill(&tmp.path().join("my-skill/nested"));

        let results = run(tmp.path());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_name().unwrap(), "my-skill");
    }

    #[test]
    fn recursive_skips_internal_dirs() {
        let tmp = tempdir().unwrap();
        write_skill(&tmp.path().join(".git/bogus"));
        write_skill(&tmp.path().join("node_modules/pkg"));
        write_skill(&tmp.path().join(".hub/hidden"));
        write_skill(&tmp.path().join("real-category/real-skill"));

        let results = run(tmp.path());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_name().unwrap(), "real-skill");
    }

    #[test]
    fn recursive_finds_deeply_nested_skill() {
        let tmp = tempdir().unwrap();
        let mut deep = tmp.path().to_path_buf();
        for _ in 0..16 {
            deep = deep.join("lvl");
        }
        write_skill(&deep);

        let results = run(tmp.path());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_name().unwrap(), "lvl");
    }

    #[cfg(unix)]
    #[test]
    fn recursive_survives_symlink_cycle() {
        use std::os::unix::fs::symlink;

        let tmp = tempdir().unwrap();
        write_skill(&tmp.path().join("category/real-skill"));
        // Self-referential loop: `category/loop -> category`
        symlink(
            tmp.path().join("category"),
            tmp.path().join("category/loop"),
        )
        .unwrap();

        let results = run(tmp.path());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_name().unwrap(), "real-skill");
    }

    #[test]
    fn flat_scan_requires_skill_marker() {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("not-a-skill")).unwrap();
        write_skill(&tmp.path().join("real-skill"));

        let adapter = tool_adapters::ToolAdapter {
            key: "test".into(),
            display_name: "Test".into(),
            relative_skills_dir: String::new(),
            relative_detect_dir: String::new(),
            additional_scan_dirs: vec![],
            override_skills_dir: Some(tmp.path().to_string_lossy().to_string()),
            is_custom: true,
            recursive_scan: false,
            project_relative_skills_dir: None,
            category: Default::default(),
        };

        let plan = scan_local_skills_with_adapters(&[], &[adapter]).unwrap();
        assert_eq!(plan.skills_found, 1);
        assert_eq!(
            plan.discovered[0].found_path,
            tmp.path().join("real-skill").to_string_lossy()
        );
    }

    #[test]
    fn additional_scan_dirs_scan_concrete_skills_roots() {
        let tmp = tempdir().unwrap();
        let primary = tmp.path().join("skills");
        let plugin_skills = tmp.path().join("plugins").join("vendor").join("skills");
        fs::create_dir_all(&primary).unwrap();
        write_skill(&plugin_skills.join("packaged-skill"));

        let adapter = tool_adapters::ToolAdapter {
            key: "test".into(),
            display_name: "Test".into(),
            relative_skills_dir: String::new(),
            relative_detect_dir: String::new(),
            additional_scan_dirs: vec![],
            override_skills_dir: Some(primary.to_string_lossy().to_string()),
            is_custom: true,
            recursive_scan: false,
            project_relative_skills_dir: None,
            category: Default::default(),
        };

        let adapter_with_extra = tool_adapters::ToolAdapter {
            additional_scan_dirs: vec![plugin_skills.to_string_lossy().to_string()],
            ..adapter
        };

        let plan = scan_local_skills_with_adapters(&[], &[adapter_with_extra]).unwrap();
        assert_eq!(plan.skills_found, 1);
        assert_eq!(
            plan.discovered[0].found_path,
            plugin_skills.join("packaged-skill").to_string_lossy()
        );
    }

    #[test]
    fn grouping_keeps_same_name_different_fingerprint_separate() {
        let records = vec![
            DiscoveredSkillRecord {
                id: "1".into(),
                tool: "a".into(),
                found_path: "/tmp/one".into(),
                name_guess: Some("shared".into()),
                fingerprint: Some("hash-a".into()),
                found_at: 10,
                imported_skill_id: None,
            },
            DiscoveredSkillRecord {
                id: "2".into(),
                tool: "b".into(),
                found_path: "/tmp/two".into(),
                name_guess: Some("shared".into()),
                fingerprint: Some("hash-b".into()),
                found_at: 20,
                imported_skill_id: None,
            },
        ];

        let groups = group_discovered(&records);
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn grouping_merges_same_name_same_fingerprint() {
        let records = vec![
            DiscoveredSkillRecord {
                id: "1".into(),
                tool: "a".into(),
                found_path: "/tmp/one".into(),
                name_guess: Some("shared".into()),
                fingerprint: Some("hash-a".into()),
                found_at: 10,
                imported_skill_id: None,
            },
            DiscoveredSkillRecord {
                id: "2".into(),
                tool: "b".into(),
                found_path: "/tmp/two".into(),
                name_guess: Some("shared".into()),
                fingerprint: Some("hash-a".into()),
                found_at: 20,
                imported_skill_id: None,
            },
        ];

        let groups = group_discovered(&records);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].locations.len(), 2);
    }

    #[test]
    fn grouping_keeps_skill_lock_collection_members_adjacent() {
        let tmp = tempdir().unwrap();
        let skills_root = tmp.path().join("skills");
        write_skill(&skills_root.join("z-superpower"));
        write_skill(&skills_root.join("a-other"));
        write_skill(&skills_root.join("m-superpower"));
        fs::write(
            tmp.path().join(".skill-lock.json"),
            r#"{
              "version": 3,
              "skills": {
                "z-superpower": {
                  "source": "obra/superpowers",
                  "sourceType": "github",
                  "sourceUrl": "https://github.com/obra/superpowers.git"
                },
                "m-superpower": {
                  "source": "obra/superpowers",
                  "sourceType": "github",
                  "sourceUrl": "https://github.com/obra/superpowers.git"
                },
                "a-other": {
                  "source": "vercel-labs/skills",
                  "sourceType": "github",
                  "sourceUrl": "https://github.com/vercel-labs/skills.git"
                }
              }
            }"#,
        )
        .unwrap();

        let records = vec![
            DiscoveredSkillRecord {
                id: "1".into(),
                tool: "shared".into(),
                found_path: skills_root
                    .join("z-superpower")
                    .to_string_lossy()
                    .into_owned(),
                name_guess: Some("z-superpower".into()),
                fingerprint: Some("hash-z".into()),
                found_at: 10,
                imported_skill_id: None,
            },
            DiscoveredSkillRecord {
                id: "2".into(),
                tool: "shared".into(),
                found_path: skills_root.join("a-other").to_string_lossy().into_owned(),
                name_guess: Some("a-other".into()),
                fingerprint: Some("hash-a".into()),
                found_at: 20,
                imported_skill_id: None,
            },
            DiscoveredSkillRecord {
                id: "3".into(),
                tool: "shared".into(),
                found_path: skills_root
                    .join("m-superpower")
                    .to_string_lossy()
                    .into_owned(),
                name_guess: Some("m-superpower".into()),
                fingerprint: Some("hash-m".into()),
                found_at: 30,
                imported_skill_id: None,
            },
        ];

        let names: Vec<_> = group_discovered(&records)
            .into_iter()
            .map(|group| group.name)
            .collect();

        assert_eq!(names, vec!["m-superpower", "z-superpower", "a-other"]);
    }

    #[test]
    fn grouping_reads_collection_from_symlink_target_lock() {
        let tmp = tempdir().unwrap();
        let agents_root = tmp.path().join("agents");
        let agents_skills = agents_root.join("skills");
        let visible_skills = tmp.path().join("claude").join("skills");
        let target = agents_skills.join("using-superpowers");
        let link = visible_skills.join("using-superpowers");
        write_skill(&target);
        fs::create_dir_all(&visible_skills).unwrap();
        fs::write(
            agents_root.join(".skill-lock.json"),
            r#"{
              "version": 3,
              "skills": {
                "using-superpowers": {
                  "source": "obra/superpowers",
                  "sourceUrl": "https://github.com/obra/superpowers.git"
                }
              }
            }"#,
        )
        .unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&target, &link).unwrap();

        let groups = group_discovered(&[DiscoveredSkillRecord {
            id: "1".into(),
            tool: "claude_code".into(),
            found_path: link.to_string_lossy().into_owned(),
            name_guess: Some("using-superpowers".into()),
            fingerprint: Some("hash-superpowers".into()),
            found_at: 10,
            imported_skill_id: None,
        }]);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].collection.as_deref(), Some("obra/superpowers"));
    }

    #[test]
    fn grouping_reports_symlink_location_target() {
        let tmp = tempdir().unwrap();
        let target = tmp
            .path()
            .join("agents")
            .join("skills")
            .join("linked-skill");
        let link = tmp.path().join("codex").join("skills").join("linked-skill");
        write_skill(&target);
        fs::create_dir_all(link.parent().unwrap()).unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&target, &link).unwrap();

        let groups = group_discovered(&[DiscoveredSkillRecord {
            id: "1".into(),
            tool: "codex".into(),
            found_path: link.to_string_lossy().into_owned(),
            name_guess: Some("linked-skill".into()),
            fingerprint: Some("hash-linked".into()),
            found_at: 10,
            imported_skill_id: None,
        }]);

        assert_eq!(groups.len(), 1);
        assert!(groups[0].locations[0].is_symlink);
        assert_eq!(
            groups[0].locations[0].link_target.as_deref(),
            Some(target.to_string_lossy().as_ref())
        );
    }

    #[test]
    fn grouping_orders_physical_location_before_symlink_locations() {
        let tmp = tempdir().unwrap();
        let target = tmp
            .path()
            .join("agents")
            .join("skills")
            .join("review-skill");
        let claude_link = tmp
            .path()
            .join("claude")
            .join("skills")
            .join("review-skill");
        let codex_link = tmp.path().join("codex").join("skills").join("review-skill");
        write_skill(&target);
        fs::create_dir_all(claude_link.parent().unwrap()).unwrap();
        fs::create_dir_all(codex_link.parent().unwrap()).unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, &claude_link).unwrap();
            std::os::unix::fs::symlink(&target, &codex_link).unwrap();
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_dir(&target, &claude_link).unwrap();
            std::os::windows::fs::symlink_dir(&target, &codex_link).unwrap();
        }

        let groups = group_discovered(&[
            DiscoveredSkillRecord {
                id: "1".into(),
                tool: "codex".into(),
                found_path: codex_link.to_string_lossy().into_owned(),
                name_guess: Some("review-skill".into()),
                fingerprint: Some("hash-review".into()),
                found_at: 10,
                imported_skill_id: None,
            },
            DiscoveredSkillRecord {
                id: "2".into(),
                tool: "claude_code".into(),
                found_path: claude_link.to_string_lossy().into_owned(),
                name_guess: Some("review-skill".into()),
                fingerprint: Some("hash-review".into()),
                found_at: 20,
                imported_skill_id: None,
            },
            DiscoveredSkillRecord {
                id: "3".into(),
                tool: "vercel".into(),
                found_path: target.to_string_lossy().into_owned(),
                name_guess: Some("review-skill".into()),
                fingerprint: Some("hash-review".into()),
                found_at: 30,
                imported_skill_id: None,
            },
        ]);

        assert_eq!(groups.len(), 1);
        let tools: Vec<_> = groups[0]
            .locations
            .iter()
            .map(|location| location.tool.as_str())
            .collect();

        assert_eq!(tools, vec!["vercel", "claude_code", "codex"]);
    }

    #[test]
    fn grouping_uses_external_registry_source_before_prefix_fallback() {
        use std::collections::HashMap;

        let records = vec![
            DiscoveredSkillRecord {
                id: "1".into(),
                tool: "claude_code".into(),
                found_path: "/tmp/gitnexus-cli".into(),
                name_guess: Some("gitnexus-cli".into()),
                fingerprint: Some("hash-cli".into()),
                found_at: 10,
                imported_skill_id: None,
            },
            DiscoveredSkillRecord {
                id: "2".into(),
                tool: "claude_code".into(),
                found_path: "/tmp/gitnexus-exploring".into(),
                name_guess: Some("gitnexus-exploring".into()),
                fingerprint: Some("hash-exploring".into()),
                found_at: 20,
                imported_skill_id: None,
            },
        ];
        let mut registry = HashMap::new();
        registry.insert(
            "gitnexus-cli".to_string(),
            SkillCollection {
                source: "abhigyanpatwari/gitnexus".to_string(),
                source_url: Some("https://github.com/abhigyanpatwari/gitnexus.git".to_string()),
            },
        );
        registry.insert(
            "gitnexus-exploring".to_string(),
            SkillCollection {
                source: "abhigyanpatwari/gitnexus".to_string(),
                source_url: Some("https://github.com/abhigyanpatwari/gitnexus.git".to_string()),
            },
        );

        let groups = group_discovered_with_registry(&records, &registry);

        assert!(groups
            .iter()
            .all(|group| group.collection.as_deref() == Some("abhigyanpatwari/gitnexus")));
    }

    #[test]
    fn grouping_uses_repeated_name_prefix_as_collection_fallback() {
        let records = vec![
            DiscoveredSkillRecord {
                id: "1".into(),
                tool: "claude_code".into(),
                found_path: "/tmp/gitnexus-cli".into(),
                name_guess: Some("gitnexus-cli".into()),
                fingerprint: Some("hash-cli".into()),
                found_at: 10,
                imported_skill_id: None,
            },
            DiscoveredSkillRecord {
                id: "2".into(),
                tool: "claude_code".into(),
                found_path: "/tmp/gitnexus-exploring".into(),
                name_guess: Some("gitnexus-exploring".into()),
                fingerprint: Some("hash-exploring".into()),
                found_at: 20,
                imported_skill_id: None,
            },
            DiscoveredSkillRecord {
                id: "3".into(),
                tool: "claude_code".into(),
                found_path: "/tmp/frontend-design".into(),
                name_guess: Some("frontend-design".into()),
                fingerprint: Some("hash-design".into()),
                found_at: 30,
                imported_skill_id: None,
            },
        ];

        let groups = group_discovered(&records);
        let gitnexus: Vec<_> = groups
            .iter()
            .filter(|group| group.collection.as_deref() == Some("gitnexus"))
            .map(|group| group.name.as_str())
            .collect();

        assert_eq!(gitnexus, vec!["gitnexus-cli", "gitnexus-exploring"]);
        assert!(groups
            .iter()
            .find(|group| group.name == "frontend-design")
            .and_then(|group| group.collection.as_deref())
            .is_none());
    }
}
