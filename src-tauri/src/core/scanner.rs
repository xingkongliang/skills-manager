use anyhow::Result;
use serde::Serialize;
use std::path::Path;

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
    pub locations: Vec<DiscoveredLocation>,
    pub imported: bool,
    pub found_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredLocation {
    pub id: String,
    pub tool: String,
    pub found_path: String,
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

/// Recursively walk `dir` and collect all subdirectories that contain SKILL.md.
/// Skips entries in `RECURSIVE_SCAN_SKIP_DIRS`.
fn collect_skill_dirs_recursive(dir: &Path, results: &mut Vec<std::path::PathBuf>) {
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
        // Skip internal directories
        if RECURSIVE_SCAN_SKIP_DIRS.iter().any(|s| dir_name_str == *s) {
            continue;
        }
        if is_symlink_to_central(&path) {
            continue;
        }
        // If this directory is a valid skill dir, collect it and DON'T descend further
        if skill_metadata::is_valid_skill_dir(&path) {
            results.push(path);
            continue;
        }
        // Otherwise descend into it
        collect_skill_dirs_recursive(&path, results);
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
        if !adapter.is_installed() {
            continue;
        }

        tools_scanned += 1;

        if adapter.recursive_scan {
            // Recursive mode: walk the tree and collect directories that contain SKILL.md
            for scan_dir in adapter.all_scan_dirs() {
                if !scan_dir.exists() {
                    continue;
                }
                let mut skill_dirs = Vec::new();
                collect_skill_dirs_recursive(&scan_dir, &mut skill_dirs);
                for path in skill_dirs {
                    let path_str = path.to_string_lossy().to_string();
                    if managed_paths.contains(&path_str) {
                        continue;
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
                        tool: adapter.key.clone(),
                        found_path: path_str,
                        name_guess: Some(name),
                        fingerprint,
                        found_at,
                        imported_skill_id: None,
                    });
                }
            }
        } else {
            // Flat mode (default): treat immediate children as skills
            for scan_dir in adapter.all_scan_dirs() {
                if !scan_dir.exists() {
                    continue;
                }

                let entries = match std::fs::read_dir(&scan_dir) {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() && !path.is_symlink() {
                        continue;
                    }

                    if is_symlink_to_central(&path) {
                        continue;
                    }

                    let path_str = path.to_string_lossy().to_string();
                    if managed_paths.contains(&path_str) {
                        continue;
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
                        tool: adapter.key.clone(),
                        found_path: path_str,
                        name_guess: Some(name),
                        fingerprint,
                        found_at,
                        imported_skill_id: None,
                    });
                }
            }
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
    use std::collections::HashMap;
    let mut groups: HashMap<String, DiscoveredGroup> = HashMap::new();

    for rec in records {
        let name = rec.name_guess.clone().unwrap_or_else(|| "unknown".into());
        let entry = groups
            .entry(name.clone())
            .or_insert_with(|| DiscoveredGroup {
                name,
                fingerprint: rec.fingerprint.clone(),
                locations: Vec::new(),
                imported: false,
                found_at: rec.found_at,
            });

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
        });
    }

    let mut result: Vec<_> = groups.into_values().collect();
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}
