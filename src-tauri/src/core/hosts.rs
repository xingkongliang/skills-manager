use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;

use crate::core::skill_store::{HostAgentRecord, HostRecord, SkillStore};
use crate::core::tool_adapters::{self, ToolAdapter};

pub const LOCAL_HOST_ID: &str = "local";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SshHostConfig {
    pub ssh_target: String,
    pub user: Option<String>,
    pub host_name: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostAgentMetadata {
    pub display_name: String,
    pub skill_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteSkillEntry {
    pub name: String,
    pub relative_path: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SshConfigImportCandidate {
    pub alias: String,
    pub host_name: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
}

pub fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn serialize_ssh_config(config: &SshHostConfig) -> Result<String> {
    Ok(serde_json::to_string(config)?)
}

pub fn parse_ssh_config(config_json: &str) -> Result<SshHostConfig> {
    Ok(serde_json::from_str(config_json)?)
}

pub fn list_importable_ssh_hosts() -> Result<Vec<SshConfigImportCandidate>> {
    let path = dirs::home_dir()
        .ok_or_else(|| anyhow!("Cannot determine home directory"))?
        .join(".ssh/config");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read SSH config at {}", path.display()))?;
    Ok(parse_ssh_config_candidates(&content))
}

pub fn parse_ssh_config_candidates(content: &str) -> Vec<SshConfigImportCandidate> {
    #[derive(Default, Clone)]
    struct Partial {
        aliases: Vec<String>,
        host_name: Option<String>,
        user: Option<String>,
        port: Option<u16>,
        identity_file: Option<String>,
    }

    fn flush(current: &Partial, out: &mut Vec<SshConfigImportCandidate>) {
        if current.aliases.is_empty() {
            return;
        }
        for alias in &current.aliases {
            out.push(SshConfigImportCandidate {
                alias: alias.clone(),
                host_name: current.host_name.clone(),
                user: current.user.clone(),
                port: current.port,
                identity_file: current.identity_file.clone(),
            });
        }
    }

    let mut out = Vec::new();
    let mut current = Partial::default();

    for raw in content.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        let rest = parts.collect::<Vec<_>>();
        if rest.is_empty() {
            continue;
        }
        match key.to_ascii_lowercase().as_str() {
            "host" => {
                flush(&current, &mut out);
                current = Partial::default();
                current.aliases = rest
                    .into_iter()
                    .filter(|value| {
                        !value.contains('*') && !value.contains('?') && !value.contains('!')
                    })
                    .map(ToString::to_string)
                    .collect();
            }
            "hostname" => current.host_name = Some(rest.join(" ")),
            "user" => current.user = Some(rest.join(" ")),
            "port" => current.port = rest[0].parse::<u16>().ok(),
            "identityfile" => current.identity_file = Some(rest.join(" ")),
            _ => {}
        }
    }

    flush(&current, &mut out);
    out.sort_by(|a, b| a.alias.cmp(&b.alias));
    out.dedup_by(|a, b| a.alias == b.alias);
    out
}

pub fn resolve_ssh_target(target: &str) -> Result<SshHostConfig> {
    let output = Command::new("ssh")
        .args(["-G", target])
        .output()
        .with_context(|| "Failed to execute ssh -G")?;
    if !output.status.success() {
        bail!(
            "Failed to resolve SSH target {}: {}",
            target,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8(output.stdout)?;
    let mut fields = HashMap::<String, String>::new();
    for line in stdout.lines() {
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        let value = parts.collect::<Vec<_>>().join(" ");
        if !value.is_empty() {
            fields.insert(key.to_ascii_lowercase(), value);
        }
    }
    Ok(SshHostConfig {
        ssh_target: target.to_string(),
        user: fields.get("user").cloned(),
        host_name: fields.get("hostname").cloned(),
        port: fields
            .get("port")
            .and_then(|value| value.parse::<u16>().ok()),
        identity_file: fields.get("identityfile").cloned(),
    })
}

pub fn test_ssh_connection(config: &SshHostConfig) -> Result<()> {
    let output = run_ssh(&config.ssh_target, "printf connected")?;
    if output.trim() != "connected" {
        bail!("Unexpected SSH handshake result for {}", config.ssh_target);
    }
    Ok(())
}

fn inspect_ssh_host(
    store: &SkillStore,
    host: &HostRecord,
) -> Result<(HostRecord, Vec<HostAgentRecord>)> {
    let mut config = parse_ssh_config(&host.config_json)?;
    let resolved = resolve_ssh_target(&config.ssh_target)?;
    config.user = resolved.user;
    config.host_name = resolved.host_name;
    config.port = resolved.port;
    config.identity_file = resolved.identity_file;
    test_ssh_connection(&config)?;
    let agents = discover_remote_agents(store, host, &config)?;

    let updated = HostRecord {
        config_json: serialize_ssh_config(&config)?,
        status: "connected".to_string(),
        updated_at: now_ts(),
        ..host.clone()
    };
    Ok((updated, agents))
}

pub fn refresh_ssh_host(store: &SkillStore, host: &HostRecord) -> Result<HostRecord> {
    let (updated, agents) = inspect_ssh_host(store, host)?;
    store.upsert_host(&updated)?;
    store.replace_host_agents(&host.id, &agents)?;
    Ok(updated)
}

pub fn inspect_ssh_host_for_add(
    store: &SkillStore,
    host: &HostRecord,
) -> Result<(HostRecord, Vec<HostAgentRecord>)> {
    inspect_ssh_host(store, host)
}

pub fn mark_host_offline(
    store: &SkillStore,
    host: &HostRecord,
    message: &str,
) -> Result<HostRecord> {
    let updated = HostRecord {
        status: format!("offline: {message}"),
        updated_at: now_ts(),
        ..host.clone()
    };
    store.upsert_host(&updated)?;
    store.replace_host_agents(&host.id, &[])?;
    Ok(updated)
}

pub fn list_remote_skills(
    host: &HostRecord,
    agent: &HostAgentRecord,
) -> Result<Vec<RemoteSkillEntry>> {
    let config = parse_ssh_config(&host.config_json)?;
    let root = remote_display_path_to_expr(&agent.skill_path);
    let script = format!(
        "root=\"{root}\"\nif [ ! -d \"$root\" ]; then exit 0; fi\nfind \"$root\" -type f -name SKILL.md 2>/dev/null | while IFS= read -r file; do\n  dir=$(dirname \"$file\")\n  rel=${{dir#\"$root\"/}}\n  if [ \"$dir\" = \"$root\" ]; then\n    rel=.\n  fi\n  name=$(basename \"$dir\")\n  printf '%s\\t%s\\t%s\\n' \"$name\" \"$rel\" \"$dir\"\ndone | sort -u"
    );
    let stdout = run_ssh(&config.ssh_target, &script)?;
    let mut entries = Vec::new();
    for line in stdout.lines() {
        let mut parts = line.splitn(3, '\t');
        let Some(name) = parts.next() else { continue };
        let Some(relative_path) = parts.next() else {
            continue;
        };
        let Some(path) = parts.next() else { continue };
        entries.push(RemoteSkillEntry {
            name: name.to_string(),
            relative_path: relative_path.to_string(),
            path: path.to_string(),
        });
    }
    Ok(entries)
}

pub fn local_host_agents(store: &SkillStore) -> Vec<(ToolAdapter, usize)> {
    let targets = store.get_all_targets().unwrap_or_default();
    tool_adapters::all_tool_adapters(store)
        .into_iter()
        .filter(|adapter| adapter.is_installed())
        .map(|adapter| {
            let count = targets
                .iter()
                .filter(|target| target.tool == adapter.key)
                .count();
            (adapter, count)
        })
        .collect()
}

fn discover_remote_agents(
    store: &SkillStore,
    host: &HostRecord,
    config: &SshHostConfig,
) -> Result<Vec<HostAgentRecord>> {
    let adapters = tool_adapters::all_tool_adapters(store);
    let mut agents = Vec::new();

    for adapter in adapters {
        let skill_roots = remote_adapter_skill_roots(&adapter);
        if skill_roots.is_empty() {
            continue;
        }
        let skill_path = skill_roots[0].clone();
        let detect_roots = remote_adapter_detect_roots(&adapter, &skill_roots);
        let detect_clause = detect_roots
            .iter()
            .map(|path| {
                format!(
                    "[ -d \"{}\" ]",
                    shell_double_quote_escape(&remote_display_path_to_expr(path))
                )
            })
            .collect::<Vec<_>>()
            .join(" || ");
        let count_script = skill_roots
            .iter()
            .map(|path| {
                let expr = shell_double_quote_escape(&remote_display_path_to_expr(path));
                format!(
                    "if [ -d \"{path}\" ]; then\n  find \"{path}\" -type f -name SKILL.md 2>/dev/null\nfi",
                    path = expr,
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let script = format!(
            "if {detect}; then\n  count=$(\n    ({count_script}) | sort -u | wc -l | tr -d ' '\n  )\n  printf '%s\\n' \"$count\"\nfi",
            detect = detect_clause,
            count_script = count_script,
        );
        let stdout = run_ssh(&config.ssh_target, &script)?;
        let trimmed = stdout.trim();
        if trimmed.is_empty() {
            continue;
        }
        let skill_count = trimmed.parse::<usize>().unwrap_or(0);
        let metadata = HostAgentMetadata {
            display_name: adapter.display_name.clone(),
            skill_count,
        };
        agents.push(HostAgentRecord {
            id: format!("{}:{}", host.id, adapter.key),
            host_id: host.id.clone(),
            agent_type: adapter.key,
            skill_path,
            status: "available".to_string(),
            metadata_json: Some(serde_json::to_string(&metadata)?),
        });
    }

    Ok(agents)
}

fn remote_home_path(relative: &str) -> String {
    format!("~/{}", relative.trim_start_matches('/'))
}

fn remote_adapter_skill_roots(adapter: &ToolAdapter) -> Vec<String> {
    let mut roots = Vec::new();
    if let Some(path) = adapter
        .override_skills_dir
        .as_deref()
        .and_then(normalize_remote_path)
    {
        roots.push(path);
    } else if let Some(path) = normalize_remote_path(&adapter.relative_skills_dir) {
        roots.push(path);
    }

    for path in &adapter.additional_scan_dirs {
        if let Some(path) = normalize_remote_path(path) {
            if !roots.contains(&path) {
                roots.push(path);
            }
        }
    }

    roots
}

fn remote_adapter_detect_roots(adapter: &ToolAdapter, skill_roots: &[String]) -> Vec<String> {
    let mut roots = Vec::new();
    if let Some(path) = normalize_remote_path(&adapter.relative_detect_dir) {
        roots.push(path);
    }
    for path in skill_roots {
        if !roots.contains(path) {
            roots.push(path.clone());
        }
    }
    roots
}

fn normalize_remote_path(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('~') || trimmed.starts_with('/') || trimmed.starts_with("$HOME/") {
        return Some(trimmed.to_string());
    }
    Some(remote_home_path(trimmed))
}

fn remote_display_path_to_expr(path: &str) -> String {
    if let Some(stripped) = path.strip_prefix("~/") {
        format!("$HOME/{stripped}")
    } else {
        path.to_string()
    }
}

fn shell_double_quote_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
}

fn run_ssh(target: &str, remote_script: &str) -> Result<String> {
    let output = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            target,
            "sh",
            "-lc",
            remote_script,
        ])
        .output()
        .with_context(|| format!("Failed to execute ssh for target {target}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.trim();
        if message.is_empty() {
            bail!("ssh command failed for target {target}");
        }
        bail!(message.to_string());
    }

    Ok(String::from_utf8(output.stdout)?)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_remote_path, parse_ssh_config_candidates, remote_adapter_skill_roots, shell_quote,
    };
    use crate::core::tool_adapters::{ToolAdapter, ToolCategory};

    #[test]
    fn parses_aliases_from_ssh_config() {
        let content = r#"
Host dev01 dev01-alt
  HostName 10.0.0.1
  User alice
  Port 2222
  IdentityFile ~/.ssh/id_ed25519

Host *.corp
  User ignored

Host dev02
  HostName 10.0.0.2
"#;

        let parsed = parse_ssh_config_candidates(content);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].alias, "dev01");
        assert_eq!(parsed[0].host_name.as_deref(), Some("10.0.0.1"));
        assert_eq!(parsed[0].user.as_deref(), Some("alice"));
        assert_eq!(parsed[0].port, Some(2222));
        assert_eq!(parsed[1].alias, "dev01-alt");
        assert_eq!(parsed[2].alias, "dev02");
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("ab'cd"), "'ab'\\''cd'");
    }

    #[test]
    fn normalize_remote_path_keeps_absolute_and_expands_relative() {
        assert_eq!(
            normalize_remote_path(".claude/skills").as_deref(),
            Some("~/.claude/skills")
        );
        assert_eq!(
            normalize_remote_path("/srv/skills").as_deref(),
            Some("/srv/skills")
        );
        assert_eq!(
            normalize_remote_path("~/custom/skills").as_deref(),
            Some("~/custom/skills")
        );
        assert!(normalize_remote_path("   ").is_none());
    }

    #[test]
    fn remote_adapter_skill_roots_prefer_override_and_include_additional_dirs() {
        let adapter = ToolAdapter {
            key: "custom".to_string(),
            display_name: "Custom".to_string(),
            relative_skills_dir: ".ignored/skills".to_string(),
            relative_detect_dir: String::new(),
            additional_scan_dirs: vec![
                ".extra/skills".to_string(),
                "/opt/shared-skills".to_string(),
            ],
            override_skills_dir: Some("~/preferred-skills".to_string()),
            is_custom: true,
            recursive_scan: false,
            project_relative_skills_dir: None,
            category: ToolCategory::Coding,
        };

        assert_eq!(
            remote_adapter_skill_roots(&adapter),
            vec![
                "~/preferred-skills".to_string(),
                "~/.extra/skills".to_string(),
                "/opt/shared-skills".to_string(),
            ]
        );
    }
}
