use serde::Serialize;
use std::path::{Path, PathBuf};

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
    #[serde(default)]
    pub relative_path: String,
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
    pub tags: Vec<String>,
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

pub fn read_linked_workspace_skills(
    skills_root: &Path,
    disabled_root: Option<&Path>,
    agent_key: &str,
    agent_display_name: &str,
) -> Vec<ProjectSkillInfo> {
    let mut skills = Vec::new();
    read_skills_from_dir(
        skills_root,
        true,
        agent_key,
        agent_display_name,
        &mut skills,
    );
    if let Some(disabled_root) = disabled_root {
        read_skills_from_dir(
            disabled_root,
            false,
            agent_key,
            agent_display_name,
            &mut skills,
        );
    }
    skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    skills
}

fn should_skip_dir(root: &Path, dir: &Path) -> bool {
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.starts_with('.') {
        return true;
    }

    // Ignore embedded plugin/cache bundle layouts such as:
    // <bundle>/<version>/skills/<skill>/SKILL.md
    // The workspace root itself may be named "skills", so only skip nested
    // container directories that introduce another "skills" subtree.
    dir != root && dir.join("skills").is_dir()
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
    let mut visited = std::collections::HashSet::new();
    if let Ok(canon) = std::fs::canonicalize(dir) {
        visited.insert(canon);
    }
    read_skills_from_dir_recursive(
        dir,
        dir,
        enabled,
        agent,
        agent_display_name,
        skills,
        &mut visited,
    );
}

fn read_skills_from_dir_recursive(
    root: &Path,
    current: &Path,
    enabled: bool,
    agent: &str,
    agent_display_name: &str,
    skills: &mut Vec<ProjectSkillInfo>,
    visited: &mut std::collections::HashSet<PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(current) else {
        return;
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        if skill_metadata::is_valid_skill_dir(&path) {
            let dir_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let relative_path = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");

            let meta = skill_metadata::parse_skill_md(&path);
            let name = meta
                .name
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| dir_name.clone());

            let files = list_files(&path);

            skills.push(ProjectSkillInfo {
                name,
                dir_name: dir_name.clone(),
                relative_path,
                description: meta.description,
                path: path.to_string_lossy().to_string(),
                files,
                enabled,
                agent: agent.to_string(),
                agent_display_name: agent_display_name.to_string(),
                tags: Vec::new(),
                in_center: false,
                sync_status: "project_only".to_string(),
                center_skill_id: None,
                last_modified_at: latest_modified_millis(&path),
                content_hash: content_hash::hash_directory(&path).ok(),
            });
            continue;
        }

        // Only check visited set before recursing into namespace dirs
        // to prevent symlink cycles. Skill dirs (above) are leaf nodes and
        // are allowed to alias the same canonical target.

        if should_skip_dir(root, &path) {
            continue;
        }

        let canon = match std::fs::canonicalize(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !visited.insert(canon) {
            continue;
        }
        read_skills_from_dir_recursive(
            root,
            &path,
            enabled,
            agent,
            agent_display_name,
            skills,
            visited,
        );
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
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn walk(path: &Path, current: &mut Option<i64>, visited: &mut HashSet<PathBuf>) {
        // Canonicalize to detect symlink cycles
        let canon = match std::fs::canonicalize(path) {
            Ok(c) => c,
            Err(_) => return,
        };
        if !visited.insert(canon) {
            return;
        }

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
            walk(&entry.path(), current, visited);
        }
    }

    let mut latest = None;
    let mut visited = HashSet::new();
    walk(dir, &mut latest, &mut visited);
    latest
}

#[cfg(test)]
mod tests {
    use super::{read_linked_workspace_skills, read_project_skills, AgentSkillConfig};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn reads_nested_project_skills_recursively() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join(".hermes").join("skills");
        let nested_skill = root.join("research").join("web-search");
        fs::create_dir_all(&nested_skill).unwrap();
        fs::write(
            nested_skill.join("SKILL.md"),
            "---\nname: Web Search\ndescription: Nested skill\n---\n",
        )
        .unwrap();

        let configs = vec![AgentSkillConfig {
            key: "hermes".to_string(),
            display_name: "Hermes".to_string(),
            relative_skills_dir: ".hermes/skills".to_string(),
        }];

        let skills = read_project_skills(tmp.path(), &configs);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].dir_name, "web-search");
        assert_eq!(skills[0].relative_path, "research/web-search");
        assert_eq!(skills[0].name, "Web Search");
    }

    #[test]
    fn prefers_skill_dir_over_namespace_parent_dir() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join(".hermes").join("skills");
        let namespace = root.join("research");
        let nested_skill = namespace.join("web-search");
        fs::create_dir_all(&nested_skill).unwrap();
        fs::write(namespace.join("notes.txt"), "namespace").unwrap();
        fs::write(nested_skill.join("SKILL.md"), "# Nested").unwrap();

        let configs = vec![AgentSkillConfig {
            key: "hermes".to_string(),
            display_name: "Hermes".to_string(),
            relative_skills_dir: ".hermes/skills".to_string(),
        }];

        let skills = read_project_skills(tmp.path(), &configs);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].relative_path, "research/web-search");
    }

    #[test]
    fn linked_workspace_skips_hidden_dirs_and_embedded_bundle_skills() {
        let tmp = tempdir().unwrap();
        let skills_root = tmp.path().join("skills");
        let disabled_root = tmp.path().join("skills-disabled");

        let top_level_skill = skills_root.join("understand");
        fs::create_dir_all(&top_level_skill).unwrap();
        fs::write(top_level_skill.join("SKILL.md"), "---\nname: understand\n---\n").unwrap();

        let hidden_skill = skills_root.join(".claude").join("skills").join("hidden-skill");
        fs::create_dir_all(&hidden_skill).unwrap();
        fs::write(hidden_skill.join("SKILL.md"), "---\nname: hidden-skill\n---\n").unwrap();

        let embedded_enabled = skills_root
            .join("understand-anything")
            .join("understand-anything")
            .join("311f2ad1aca5")
            .join("skills")
            .join("understand");
        fs::create_dir_all(&embedded_enabled).unwrap();
        fs::write(embedded_enabled.join("SKILL.md"), "---\nname: understand\n---\n").unwrap();

        let disabled_skill = disabled_root.join("understand-diff");
        fs::create_dir_all(&disabled_skill).unwrap();
        fs::write(
            disabled_skill.join("SKILL.md"),
            "---\nname: understand-diff\n---\n",
        )
        .unwrap();

        let embedded_disabled = disabled_root
            .join("claude-plugins-official")
            .join("superpowers")
            .join("5.0.7")
            .join("skills")
            .join("brainstorming");
        fs::create_dir_all(&embedded_disabled).unwrap();
        fs::write(
            embedded_disabled.join("SKILL.md"),
            "---\nname: brainstorming\n---\n",
        )
        .unwrap();

        let skills = read_linked_workspace_skills(
            &skills_root,
            Some(&disabled_root),
            "linked",
            "Linked",
        );

        let names: Vec<&str> = skills.iter().map(|skill| skill.name.as_str()).collect();
        assert_eq!(names, vec!["understand", "understand-diff"]);
        assert_eq!(
            skills.iter().filter(|skill| skill.name == "understand").count(),
            1
        );
        assert!(skills
            .iter()
            .any(|skill| skill.name == "understand" && skill.enabled));
        assert!(skills
            .iter()
            .any(|skill| skill.name == "understand-diff" && !skill.enabled));
    }
}
