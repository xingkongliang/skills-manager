use anyhow::Result;
use serde::Serialize;
use std::path::Path;

use crate::content_hash;
use crate::skill_metadata;
use crate::skill_store::DiscoveredSkillRecord;
use crate::tool_adapters;

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

fn is_symlink_to_central(path: &Path) -> bool {
    if let Ok(target) = std::fs::read_link(path) {
        let central = super::central_repo::skills_dir();
        return target.starts_with(&central);
    }
    false
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
                    is_native: false,
                });
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
