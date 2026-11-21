//! Read-only map of where Claude Code keeps its configuration on this machine:
//! skill folders, plugins, MCP servers, hooks, project `.claude`/`.agents`
//! directories and `CLAUDE.md` / `AGENTS.md` memory files.
//!
//! Nothing here writes, moves or deletes. MCP `env` values are never read into
//! the result, only their keys. Hook commands are shown as written in
//! settings.json, cut to 140 characters.

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::core::error::AppError;

/// Folders never worth descending into when looking for memory files.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".cache",
    ".git",
    ".venv",
    "venv",
    "__pycache__",
    ".npm",
    ".cargo",
    ".rustup",
    "target",
    "dist",
    "build",
    ".next",
    "Android",
    "snap",
    "Library",
];
const MAX_DEPTH: usize = 6;
const MAX_MEMORY_FILES: usize = 5000;

#[derive(Debug, Serialize)]
pub struct MapSkill {
    pub name: String,
    pub description: String,
    /// "user" (~/.claude/skills), "agents" (~/.agents/skills) or "plugin".
    pub source: String,
    /// Display group: the folder, or the plugin id.
    pub group: String,
    pub path: String,
    /// Resolved target when the skill folder is a symlink.
    pub link: Option<String>,
    /// "on", "off" or "user-invocable-only", from settings.json `skillOverrides`.
    pub state: String,
}

#[derive(Debug, Serialize)]
pub struct MapPlugin {
    pub id: String,
    pub version: String,
    pub enabled: bool,
    pub skills: Vec<String>,
    pub commands: Vec<String>,
    pub agents: Vec<String>,
    pub has_hooks: bool,
    pub has_mcp: bool,
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct MapMcpServer {
    pub name: String,
    /// "user", "project" or "settings".
    pub scope: String,
    /// File (and for project scope, the project) that declares it.
    pub defined_in: String,
    pub transport: String,
    pub env_keys: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct MapHook {
    pub event: String,
    pub matcher: String,
    pub command: String,
}

#[derive(Debug, Serialize)]
pub struct MapMemoryFile {
    pub path: String,
    pub dir: String,
    pub file: String,
    pub size: u64,
    pub symlink: bool,
}

#[derive(Debug, Serialize)]
pub struct MapDotDir {
    pub path: String,
    pub kind: String,
    pub skills: usize,
    pub commands: usize,
    pub agents: usize,
    pub files: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ConfigMap {
    pub home: String,
    pub skills: Vec<MapSkill>,
    pub plugins: Vec<MapPlugin>,
    pub mcp_servers: Vec<MapMcpServer>,
    pub hooks: Vec<MapHook>,
    pub memory_files: Vec<MapMemoryFile>,
    pub dot_dirs: Vec<MapDotDir>,
    pub override_count: usize,
    /// True when the memory-file walk stopped at MAX_MEMORY_FILES.
    pub truncated: bool,
    pub usage: Usage,
}

#[derive(Debug, Default, Serialize)]
pub struct UsageEntry {
    pub count: u32,
    /// ISO-8601 timestamp of the latest use, as written in the transcript.
    pub last_used: String,
}

/// How often skills and MCP servers were actually used, counted from the
/// Claude Code session transcripts in `~/.claude/projects`. Only tool names and
/// timestamps are read; message text is never kept.
#[derive(Debug, Default, Serialize)]
pub struct Usage {
    pub sessions: usize,
    /// Earliest timestamp seen: usage before it is unknown, because Claude Code
    /// deletes transcripts older than `cleanupPeriodDays`.
    pub since: Option<String>,
    pub retention_days: u64,
    /// Keyed by the name the Skill tool or slash command used.
    pub skills: BTreeMap<String, UsageEntry>,
    /// Keyed by the server segment of `mcp__<server>__<tool>`.
    pub mcp: BTreeMap<String, UsageEntry>,
}

impl Usage {
    fn bump(map: &mut BTreeMap<String, UsageEntry>, key: &str, ts: &str) {
        let e = map.entry(key.to_string()).or_default();
        e.count += 1;
        if ts > e.last_used.as_str() {
            e.last_used = ts.to_string();
        }
    }

    /// Feeds one transcript line. Cheap substring checks skip the lines that
    /// cannot hold a tool call or a slash command before any JSON parsing.
    fn read_line(&mut self, line: &str) {
        let has_tool = line.contains("\"tool_use\"");
        let has_command = line.contains("<command-name>/");
        if !has_tool && !has_command {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let ts = v["timestamp"].as_str().unwrap_or("");
        // `Option::is_none_or` needs Rust 1.82; the crate supports 1.77.
        #[allow(clippy::unnecessary_map_or)]
        let earlier = self.since.as_deref().map_or(true, |s| ts < s);
        if !ts.is_empty() && earlier {
            self.since = Some(ts.to_string());
        }
        let content = &v["message"]["content"];
        if has_tool {
            for item in content.as_array().into_iter().flatten() {
                if item["type"] != "tool_use" {
                    continue;
                }
                let name = item["name"].as_str().unwrap_or("");
                if name == "Skill" {
                    if let Some(skill) = item["input"]["skill"].as_str() {
                        Self::bump(&mut self.skills, skill, ts);
                    }
                } else if let Some(rest) = name.strip_prefix("mcp__") {
                    if let Some((server, _)) = rest.split_once("__") {
                        Self::bump(&mut self.mcp, server, ts);
                    }
                }
            }
        }
        // Slash commands are only counted in what the user typed, never in assistant text.
        if has_command && v["type"] == "user" {
            let text = match content {
                Value::String(s) => s.clone(),
                Value::Array(a) => a
                    .iter()
                    .filter_map(|i| i["text"].as_str())
                    .collect::<Vec<_>>()
                    .join(""),
                _ => String::new(),
            };
            for part in text.split("<command-name>/").skip(1) {
                let Some((cmd, _)) = part.split_once("</command-name>") else {
                    continue;
                };
                let valid = !cmd.is_empty()
                    && cmd
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-' | '.'));
                if valid {
                    Self::bump(&mut self.skills, cmd, ts);
                }
            }
        }
    }
}

fn collect_usage(home: &Path, retention_days: u64) -> Usage {
    use std::io::BufRead;
    let mut usage = Usage {
        retention_days,
        ..Default::default()
    };
    let root = home.join(".claude/projects");
    for entry in walkdir::WalkDir::new(&root)
        .max_depth(4)
        .into_iter()
        .flatten()
    {
        if entry.path().extension().and_then(|x| x.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(file) = std::fs::File::open(entry.path()) else {
            continue;
        };
        usage.sessions += 1;
        for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
            usage.read_line(&line);
        }
    }
    usage
}

fn tilde(home: &Path, p: &Path) -> String {
    match p.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => p.to_string_lossy().to_string(),
    }
}

fn read_json(p: &Path) -> Value {
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null)
}

/// The `description` from a SKILL.md front matter, folded onto one line.
fn skill_description(skill_md: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(skill_md) else {
        return String::new();
    };
    let Some(rest) = text.strip_prefix("---") else {
        return String::new();
    };
    let Some(end) = rest.find("\n---") else {
        return String::new();
    };
    let mut desc = String::new();
    let mut in_desc = false;
    for line in rest[..end].lines() {
        if let Some(v) = line.strip_prefix("description:") {
            let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
            if !matches!(v, ">" | "|" | ">-" | "|-") {
                desc.push_str(v);
            }
            in_desc = true;
        } else if in_desc && line.starts_with(' ') {
            if !desc.is_empty() {
                desc.push(' ');
            }
            desc.push_str(line.trim());
        } else if in_desc {
            break;
        }
    }
    desc.chars().take(300).collect()
}

fn names_in(dir: &Path, want_md: bool) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if want_md {
                (p.extension().and_then(|x| x.to_str()) == Some("md"))
                    .then(|| p.file_stem().map(|s| s.to_string_lossy().to_string()))
                    .flatten()
            } else {
                p.join("SKILL.md")
                    .is_file()
                    .then(|| e.file_name().to_string_lossy().to_string())
            }
        })
        .collect();
    out.sort();
    out
}

fn count_entries(dir: &Path) -> usize {
    std::fs::read_dir(dir).map(|rd| rd.count()).unwrap_or(0)
}

fn state_for(overrides: &serde_json::Map<String, Value>, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|k| overrides.get(*k).and_then(|v| v.as_str()))
        .unwrap_or("on")
        .to_string()
}

fn collect_skill_dir(
    home: &Path,
    dir: &Path,
    source: &str,
    overrides: &serde_json::Map<String, Value>,
    out: &mut Vec<MapSkill>,
) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if !p.is_dir() {
            continue;
        }
        let name = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let link = std::fs::symlink_metadata(&p)
            .ok()
            .filter(|m| m.file_type().is_symlink())
            .and_then(|_| std::fs::canonicalize(&p).ok())
            .map(|t| tilde(home, &t));
        out.push(MapSkill {
            description: skill_description(&p.join("SKILL.md")),
            state: state_for(overrides, &[&name]),
            source: source.to_string(),
            group: tilde(home, dir),
            path: tilde(home, &p),
            link,
            name,
        });
    }
}

fn transport(cfg: &Value) -> String {
    cfg.get("type")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| {
            if cfg.get("command").is_some() {
                "stdio".into()
            } else if cfg.get("url").is_some() {
                "http".into()
            } else {
                "unknown".into()
            }
        })
}

fn push_mcp(out: &mut Vec<MapMcpServer>, servers: &Value, scope: &str, defined_in: String) {
    let Some(map) = servers.as_object() else {
        return;
    };
    for (name, cfg) in map {
        let mut env_keys: Vec<String> = cfg
            .get("env")
            .and_then(|e| e.as_object())
            .map(|e| e.keys().cloned().collect())
            .unwrap_or_default();
        env_keys.sort();
        out.push(MapMcpServer {
            name: name.clone(),
            scope: scope.to_string(),
            defined_in: defined_in.clone(),
            transport: transport(cfg),
            env_keys,
        });
    }
}

fn walk_home(home: &Path, memory: &mut Vec<MapMemoryFile>, dots: &mut Vec<MapDotDir>) -> bool {
    let claude_root = home.join(".claude");
    let mut truncated = false;
    let walker = walkdir::WalkDir::new(home)
        .max_depth(MAX_DEPTH)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            if e.file_type().is_dir() {
                if SKIP_DIRS.contains(&name.as_ref()) {
                    return false;
                }
                // Hidden folders are skipped, except the ones that hold agent config.
                if name.starts_with('.') && name != ".claude" && name != ".agents" {
                    return false;
                }
                // Inside ~/.claude, plugin caches and session logs are not config.
                if e.path().starts_with(&claude_root)
                    && (name == "plugins" || name == "projects" || name == "file-history")
                {
                    return false;
                }
            }
            true
        });
    for entry in walker.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        if entry.file_type().is_dir() && (name == ".claude" || name == ".agents") {
            let files = [
                "settings.json",
                "settings.local.json",
                "CLAUDE.md",
                ".mcp.json",
            ]
            .iter()
            .filter(|f| path.join(f).exists())
            .map(|f| f.to_string())
            .collect();
            dots.push(MapDotDir {
                path: tilde(home, path),
                kind: name.clone(),
                skills: count_entries(&path.join("skills")),
                commands: count_entries(&path.join("commands")),
                agents: count_entries(&path.join("agents")),
                files,
            });
            continue;
        }
        if matches!(name.as_str(), "CLAUDE.md" | "AGENTS.md" | "CLAUDE.local.md") {
            if memory.len() >= MAX_MEMORY_FILES {
                truncated = true;
                break;
            }
            let symlink = entry.path_is_symlink();
            let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            memory.push(MapMemoryFile {
                path: tilde(home, path),
                dir: tilde(home, path.parent().unwrap_or(home)),
                file: name,
                size,
                symlink,
            });
        }
    }
    memory.sort_by(|a, b| a.path.cmp(&b.path));
    truncated
}

pub fn build_config_map(home: &Path) -> ConfigMap {
    let settings = read_json(&home.join(".claude/settings.json"));
    let empty = serde_json::Map::new();
    let overrides = settings
        .get("skillOverrides")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let enabled = settings.get("enabledPlugins").and_then(|v| v.as_object());

    let mut skills = Vec::new();
    collect_skill_dir(
        home,
        &home.join(".claude/skills"),
        "user",
        overrides,
        &mut skills,
    );
    collect_skill_dir(
        home,
        &home.join(".agents/skills"),
        "agents",
        overrides,
        &mut skills,
    );

    let mut plugins = Vec::new();
    let installed = read_json(&home.join(".claude/plugins/installed_plugins.json"));
    if let Some(map) = installed.get("plugins").and_then(|v| v.as_object()) {
        let sorted: BTreeMap<_, _> = map.iter().collect();
        for (id, versions) in sorted {
            let first = versions.as_array().and_then(|a| a.first());
            let install_path = first
                .and_then(|v| v.get("installPath"))
                .and_then(|v| v.as_str())
                .map(PathBuf::from)
                .unwrap_or_default();
            let plugin_name = id.split('@').next().unwrap_or(id);
            let plugin_enabled = enabled
                .and_then(|e| e.get(id))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let skill_names = names_in(&install_path.join("skills"), false);
            for s in &skill_names {
                let dir = install_path.join("skills").join(s);
                skills.push(MapSkill {
                    name: format!("{plugin_name}:{s}"),
                    description: skill_description(&dir.join("SKILL.md")),
                    source: "plugin".into(),
                    group: id.clone(),
                    path: tilde(home, &dir),
                    link: None,
                    // `skillOverrides` does not apply to plugin skills: they follow the plugin.
                    state: if plugin_enabled { "on" } else { "off" }.into(),
                });
            }
            plugins.push(MapPlugin {
                id: id.clone(),
                version: first
                    .and_then(|v| v.get("version"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                enabled: plugin_enabled,
                skills: skill_names,
                commands: names_in(&install_path.join("commands"), true),
                agents: names_in(&install_path.join("agents"), true),
                has_hooks: install_path.join("hooks").exists(),
                has_mcp: install_path.join(".mcp.json").exists(),
                path: tilde(home, &install_path),
            });
        }
    }

    let mut mcp_servers = Vec::new();
    let claude_json = read_json(&home.join(".claude.json"));
    push_mcp(
        &mut mcp_servers,
        &claude_json["mcpServers"],
        "user",
        "~/.claude.json".into(),
    );
    if let Some(projects) = claude_json.get("projects").and_then(|v| v.as_object()) {
        for (proj, v) in projects {
            push_mcp(
                &mut mcp_servers,
                &v["mcpServers"],
                "project",
                tilde(home, Path::new(proj)),
            );
        }
    }
    push_mcp(
        &mut mcp_servers,
        &settings["mcpServers"],
        "settings",
        "~/.claude/settings.json".into(),
    );

    let mut hooks = Vec::new();
    if let Some(events) = settings.get("hooks").and_then(|v| v.as_object()) {
        for (event, entries) in events {
            for entry in entries.as_array().into_iter().flatten() {
                let matcher = entry["matcher"].as_str().unwrap_or("").to_string();
                for h in entry["hooks"].as_array().into_iter().flatten() {
                    let command = h["command"]
                        .as_str()
                        .or_else(|| h["type"].as_str())
                        .unwrap_or("")
                        .chars()
                        .take(140)
                        .collect();
                    hooks.push(MapHook {
                        event: event.clone(),
                        matcher: matcher.clone(),
                        command,
                    });
                }
            }
        }
    }

    let mut memory_files = Vec::new();
    let mut dot_dirs = Vec::new();
    let truncated = walk_home(home, &mut memory_files, &mut dot_dirs);
    // Claude Code's documented default for `cleanupPeriodDays` is 30.
    let retention_days = settings["cleanupPeriodDays"].as_u64().unwrap_or(30);
    let usage = collect_usage(home, retention_days);

    ConfigMap {
        home: "~".into(),
        override_count: overrides.len(),
        skills,
        plugins,
        mcp_servers,
        hooks,
        memory_files,
        dot_dirs,
        truncated,
        usage,
    }
}

#[tauri::command]
pub async fn get_config_map() -> Result<ConfigMap, AppError> {
    let home = dirs::home_dir().ok_or_else(|| AppError::io("home directory not found"))?;
    Ok(tauri::async_runtime::spawn_blocking(move || build_config_map(&home)).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn maps_skills_plugins_mcp_and_memory_without_env_values() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        fs::create_dir_all(home.join(".claude/skills/alpha")).unwrap();
        fs::write(
            home.join(".claude/skills/alpha/SKILL.md"),
            "---\nname: alpha\ndescription: First skill\n---\nbody",
        )
        .unwrap();
        fs::write(
            home.join(".claude/settings.json"),
            r#"{"skillOverrides":{"alpha":"off","beta":"off"},"enabledPlugins":{"kit@m":true},"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"echo hi"}]}]}}"#,
        )
        .unwrap();
        fs::write(
            home.join(".claude.json"),
            r#"{"mcpServers":{"srv":{"command":"x","env":{"API_TOKEN":"secret-value"}}}}"#,
        )
        .unwrap();
        let plugin = home.join(".claude/plugins/cache/kit");
        fs::create_dir_all(plugin.join("skills/beta")).unwrap();
        fs::write(
            plugin.join("skills/beta/SKILL.md"),
            "---\nname: beta\n---\n",
        )
        .unwrap();
        fs::write(
            home.join(".claude/plugins/installed_plugins.json"),
            format!(
                r#"{{"plugins":{{"kit@m":[{{"installPath":{},"version":"1"}}]}}}}"#,
                serde_json::to_string(&plugin.to_string_lossy()).unwrap()
            ),
        )
        .unwrap();
        fs::create_dir_all(home.join("proj/.claude")).unwrap();
        fs::write(home.join("proj/CLAUDE.md"), "# p").unwrap();
        fs::create_dir_all(home.join("node_modules/x")).unwrap();
        fs::write(home.join("node_modules/x/CLAUDE.md"), "ignored").unwrap();

        let map = build_config_map(home);
        assert_eq!(map.skills.len(), 2);
        assert_eq!(map.skills[0].state, "off");
        // Overrides do not reach plugin skills: they follow the enabled plugin.
        assert_eq!(map.skills[1].name, "kit:beta");
        assert_eq!(map.skills[1].state, "on");
        assert_eq!(map.skills[0].description, "First skill");
        assert_eq!(map.mcp_servers[0].env_keys, vec!["API_TOKEN".to_string()]);
        assert_eq!(map.hooks[0].command, "echo hi");
        assert!(map
            .memory_files
            .iter()
            .any(|m| m.path == "~/proj/CLAUDE.md"));
        assert!(!map
            .memory_files
            .iter()
            .any(|m| m.path.contains("node_modules")));
        assert!(map.dot_dirs.iter().any(|d| d.path == "~/proj/.claude"));
        let json = serde_json::to_string(&map).unwrap();
        assert_eq!(map.usage.sessions, 0);
        assert!(!json.contains("secret-value"));
    }

    #[test]
    fn counts_skill_mcp_and_slash_usage_from_transcripts() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let dir = home.join(".claude/projects/-proj");
        fs::create_dir_all(dir.join("sub/subagents")).unwrap();
        let lines = [
            r#"{"timestamp":"2026-01-02T00:00:00Z","message":{"content":[{"type":"tool_use","name":"Skill","input":{"skill":"alpha"}}]}}"#,
            r#"{"timestamp":"2026-01-05T00:00:00Z","message":{"content":[{"type":"tool_use","name":"mcp__notes__search","input":{}},{"type":"tool_use","name":"Skill","input":{"skill":"alpha"}}]}}"#,
            r#"{"type":"user","timestamp":"2026-01-01T00:00:00Z","message":{"content":"<command-name>/beta</command-name> secret text"}}"#,
            r#"{"type":"assistant","timestamp":"2026-01-06T00:00:00Z","message":{"content":[{"type":"text","text":"<command-name>/gamma</command-name>"}]}}"#,
            r#"{"type":"user","timestamp":"2026-01-06T00:00:00Z","message":{"content":"<command-name>/not a skill</command-name>"}}"#,
            r#"{"timestamp":"2026-01-03T00:00:00Z","message":{"content":"plain text, nothing to count"}}"#,
            "not json",
        ];
        fs::write(dir.join("a.jsonl"), lines.join("\n")).unwrap();
        fs::write(
            dir.join("sub/subagents/b.jsonl"),
            r#"{"timestamp":"2026-01-04T00:00:00Z","message":{"content":[{"type":"tool_use","name":"mcp__notes__read","input":{}}]}}"#,
        )
        .unwrap();

        let usage = collect_usage(home, 30);
        assert_eq!(usage.sessions, 2);
        assert_eq!(usage.skills["alpha"].count, 2);
        assert_eq!(usage.skills["alpha"].last_used, "2026-01-05T00:00:00Z");
        assert_eq!(usage.skills["beta"].count, 1);
        assert_eq!(usage.mcp["notes"].count, 2);
        assert!(!usage.skills.contains_key("gamma"));
        assert_eq!(usage.skills.len(), 2);
        assert_eq!(usage.since.as_deref(), Some("2026-01-01T00:00:00Z"));
        let json = serde_json::to_string(&usage).unwrap();
        assert!(!json.contains("secret text"));
    }
}
