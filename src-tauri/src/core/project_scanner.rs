use serde::Serialize;
use std::path::Path;

use super::{content_hash, skill_metadata};

/// Lightweight config describing where an agent keeps project-level skills.
#[derive(Debug, Clone)]
pub struct AgentSkillConfig {
    pub key: String,
    pub display_name: String,
    /// Relative path from project root to the skills directory (e.g. ".claude/skills").
    pub relative_skills_dir: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectSkillInfo {
    pub name: String,
    pub dir_name: String,
    pub description: Option<String>,
    pub path: String,
    pub files: Vec<String>,
    pub enabled: bool,
    /// Agent key that owns this skill (e.g. "claude_code", "cursor").
    #[serde(default)]
    pub agent: String,
    /// Human-readable agent name (e.g. "Claude Code", "Cursor").
    #[serde(default)]
    pub agent_display_name: String,
    #[serde(default)]
    pub in_center: bool,
    #[serde(default)]
    pub sync_status: String,
    #[serde(default)]
    pub center_skill_id: Option<String>,
    #[serde(skip_serializing)]
    pub last_modified_at: Option<i64>,
    #[serde(skip_serializing)]
    pub content_hash: Option<String>,
}

/// Read skills from all configured agents' project-level skill directories.
pub fn read_project_skills(
    project_path: &Path,
    agent_configs: &[AgentSkillConfig],
) -> Vec<ProjectSkillInfo> {
    let mut skills = Vec::new();

    for config in agent_configs {
        let skills_dir = project_path.join(&config.relative_skills_dir);
        let disabled_dir = project_path.join(format!("{}-disabled", &config.relative_skills_dir));

        read_skills_from_dir(
            &skills_dir,
            true,
            &config.key,
            &config.display_name,
            &mut skills,
        );
        read_skills_from_dir(
            &disabled_dir,
            false,
            &config.key,
            &config.display_name,
            &mut skills,
        );
    }

    skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    skills
}

fn read_skills_from_dir(
    dir: &Path,
    enabled: bool,
    agent: &str,
    agent_display_name: &str,
    skills: &mut Vec<ProjectSkillInfo>,
) {
    if !dir.is_dir() {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let dir_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let meta = skill_metadata::parse_skill_md(&path);
            let name = meta
                .name
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| dir_name.clone());

            let files = list_files(&path);

            skills.push(ProjectSkillInfo {
                name,
                dir_name: dir_name.clone(),
                description: meta.description,
                path: path.to_string_lossy().to_string(),
                files,
                enabled,
                agent: agent.to_string(),
                agent_display_name: agent_display_name.to_string(),
                in_center: false,
                sync_status: "project_only".to_string(),
                center_skill_id: None,
                last_modified_at: latest_modified_millis(&path),
                content_hash: content_hash::hash_directory(&path).ok(),
            });
        }
    }
}

/// Scan a root directory for projects containing any agent's skills directory.
pub fn scan_projects_in_dir(
    root: &Path,
    max_depth: usize,
    agent_configs: &[AgentSkillConfig],
) -> Vec<String> {
    let mut results = Vec::new();
    scan_recursive(root, 0, max_depth, agent_configs, &mut results);
    results.sort();
    results
}

fn has_any_agent_skills(dir: &Path, agent_configs: &[AgentSkillConfig]) -> bool {
    agent_configs
        .iter()
        .any(|config| dir.join(&config.relative_skills_dir).is_dir())
}

fn scan_recursive(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    agent_configs: &[AgentSkillConfig],
    results: &mut Vec<String>,
) {
    if depth > max_depth {
        return;
    }

    if has_any_agent_skills(dir, agent_configs) {
        results.push(dir.to_string_lossy().to_string());
        return; // don't recurse into subdirectories of a matched project
    }

    if depth == max_depth {
        return;
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            // Skip hidden directories and common non-project dirs
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || name == "__pycache__"
            {
                continue;
            }
            scan_recursive(&path, depth + 1, max_depth, agent_configs, results);
        }
    }
}

fn list_files(dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name() {
                    files.push(name.to_string_lossy().to_string());
                }
            }
        }
    }
    files.sort();
    files
}

fn latest_modified_millis(dir: &Path) -> Option<i64> {
    fn walk(path: &Path, current: &mut Option<i64>) {
        let Ok(meta) = std::fs::metadata(path) else {
            return;
        };
        if let Ok(modified) = meta.modified() {
            if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                let ts = duration.as_millis() as i64;
                if current.map_or(true, |value| ts > value) {
                    *current = Some(ts);
                }
            }
        }

        if !meta.is_dir() {
            return;
        }

        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            walk(&entry.path(), current);
        }
    }

    let mut latest = None;
    walk(dir, &mut latest);
    latest
}
