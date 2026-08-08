use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

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

#[derive(Debug, Clone)]
pub struct RemoteSkillHash {
    pub relative_path: String,
    pub hash: String,
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
    validate_ssh_target(target)?;
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
        "root={root}\nif [ ! -d \"$root\" ]; then exit 0; fi\n{find_skill_files} | while IFS= read -r file; do\n  dir=$(dirname \"$file\")\n  rel=${{dir#\"$root\"/}}\n  if [ \"$dir\" = \"$root\" ]; then\n    rel=.\n  fi\n  name=$(basename \"$dir\")\n  printf '%s\\t%s\\t%s\\n' \"$name\" \"$rel\" \"$dir\"\ndone | sort -u",
        root = remote_path_expr_literal(&root),
        find_skill_files = remote_find_skill_files_script("$root"),
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

pub fn remote_skill_path(agent: &HostAgentRecord, relative_path: &str) -> Result<String> {
    ensure_safe_remote_skill_relative_path(relative_path)?;
    let root = remote_display_path_to_expr(&agent.skill_path);
    if relative_path == "." {
        return Ok(root);
    }
    Ok(format!("{}/{}", root.trim_end_matches('/'), relative_path))
}

pub fn list_remote_skill_hashes(
    host: &HostRecord,
    agent: &HostAgentRecord,
) -> Result<Vec<RemoteSkillHash>> {
    let config = parse_ssh_config(&host.config_json)?;
    let root = remote_display_path_to_expr(&agent.skill_path);
    let script = format!(
        r#"root={root}
if [ ! -d "$root" ]; then exit 0; fi
{find_skill_files} | while IFS= read -r file; do
  dir=$(dirname "$file")
  rel=${{dir#"$root"/}}
  if [ "$dir" = "$root" ]; then
    rel=.
  fi
  hash=$(
    cd "$dir" && find . -type f \
      ! -path './.git/*' \
      ! -name '.DS_Store' \
      ! -name 'Thumbs.db' \
      ! -name '.gitignore' \
      ! -path '*/__pycache__/*' \
      ! -name '*.pyc' \
      -print | LC_ALL=C sort | while IFS= read -r f; do
        relfile=${{f#./}}
        printf '%s' "$relfile"
        cat "$f"
        mode=$(stat -c '%a' "$f" 2>/dev/null || stat -f '%Lp' "$f" 2>/dev/null || true)
        if [ -n "$mode" ]; then
          exec_bits=$(printf '%s\n' "$mode" | awk '{{ n=$1; o=int(n/100)%10; g=int(n/10)%10; w=n%10; b=0; if (o%2==1) b+=64; if (g%2==1) b+=8; if (w%2==1) b+=1; print b }}')
        elif [ -x "$f" ]; then
          exec_bits=73
        else
          exec_bits=0
        fi
        case "$exec_bits" in
          1) printf '\001\000\000\000' ;;
          8) printf '\010\000\000\000' ;;
          9) printf '\011\000\000\000' ;;
          64) printf '\100\000\000\000' ;;
          65) printf '\101\000\000\000' ;;
          72) printf '\110\000\000\000' ;;
          73) printf '\111\000\000\000' ;;
          *) printf '\000\000\000\000' ;;
        esac
      done | if command -v sha256sum >/dev/null 2>&1; then sha256sum; else shasum -a 256; fi | cut -d ' ' -f 1
  )
  printf '%s\t%s\n' "$rel" "$hash"
done | sort -u"#,
        root = remote_path_expr_literal(&root),
        find_skill_files = remote_find_skill_files_script("$root"),
    );
    let stdout = run_ssh(&config.ssh_target, &script)?;
    let mut hashes = Vec::new();
    for line in stdout.lines() {
        let mut parts = line.splitn(2, '\t');
        let Some(relative_path) = parts.next() else {
            continue;
        };
        let Some(hash) = parts.next() else { continue };
        hashes.push(RemoteSkillHash {
            relative_path: relative_path.to_string(),
            hash: hash.to_string(),
        });
    }
    Ok(hashes)
}

pub fn copy_local_skill_to_remote(
    host: &HostRecord,
    agent: &HostAgentRecord,
    source: &Path,
    relative_path: &str,
) -> Result<()> {
    ensure_safe_remote_skill_relative_path(relative_path)?;
    if relative_path == "." {
        bail!("Refusing to replace the remote agent skill root");
    }
    if !source.join("SKILL.md").exists() && !source.join("skill.md").exists() {
        bail!(
            "Source is not a valid skill directory: {}",
            source.display()
        );
    }
    let config = parse_ssh_config(&host.config_json)?;
    validate_ssh_target(&config.ssh_target)?;
    let root = remote_display_path_to_expr(&agent.skill_path);
    let temp_name = format!(".skills-manager-staged-{}", uuid::Uuid::new_v4().simple());
    let script = remote_install_script(&root, relative_path, &temp_name);
    let mut tar = Command::new("tar")
        .arg("-C")
        .arg(source)
        .args(["-cf", "-", "."])
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to archive {}", source.display()))?;
    let tar_stdout = tar
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Failed to read tar output"))?;
    let ssh = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            &config.ssh_target,
            &format!("sh -lc {}", shell_quote(&script)),
        ])
        .stdin(Stdio::from(tar_stdout))
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to execute ssh for target {}", config.ssh_target))?;
    let ssh_output = ssh.wait_with_output()?;
    let tar_status = tar.wait()?;
    if !tar_status.success() {
        bail!("Failed to archive {}", source.display());
    }
    if !ssh_output.status.success() {
        let message = String::from_utf8_lossy(&ssh_output.stderr)
            .trim()
            .to_string();
        bail!(if message.is_empty() {
            "remote copy failed".to_string()
        } else {
            message
        });
    }
    Ok(())
}

pub fn copy_remote_skill_to_local(
    host: &HostRecord,
    agent: &HostAgentRecord,
    relative_path: &str,
    destination: &Path,
) -> Result<()> {
    ensure_safe_remote_skill_relative_path(relative_path)?;
    if relative_path == "." {
        bail!("Refusing to archive the remote agent skill root");
    }
    let config = parse_ssh_config(&host.config_json)?;
    validate_ssh_target(&config.ssh_target)?;
    let source = remote_skill_path(agent, relative_path)?;
    if destination.exists() {
        std::fs::remove_dir_all(destination)
            .with_context(|| format!("Failed to remove {}", destination.display()))?;
    }
    std::fs::create_dir_all(destination)?;
    let script = format!(
        r#"set -e
source={source}
if [ ! -d "$source" ]; then
  echo "Remote skill not found: $source" >&2
  exit 1
fi
tar -C "$source" -cf - ."#,
        source = remote_path_expr_literal(&source),
    );
    let mut ssh = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            &config.ssh_target,
            &format!("sh -lc {}", shell_quote(&script)),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to execute ssh for target {}", config.ssh_target))?;
    let ssh_stdout = ssh
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Failed to read ssh output"))?;
    let tar = Command::new("tar")
        .arg("-C")
        .arg(destination)
        .args(["-xf", "-"])
        .stdin(Stdio::from(ssh_stdout))
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to extract into {}", destination.display()))?;
    let tar_output = tar.wait_with_output()?;
    let ssh_output = ssh.wait_with_output()?;
    if !ssh_output.status.success() {
        let message = String::from_utf8_lossy(&ssh_output.stderr)
            .trim()
            .to_string();
        bail!(if message.is_empty() {
            "remote archive failed".to_string()
        } else {
            message
        });
    }
    if !tar_output.status.success() {
        let message = String::from_utf8_lossy(&tar_output.stderr)
            .trim()
            .to_string();
        bail!(if message.is_empty() {
            "local extract failed".to_string()
        } else {
            message
        });
    }
    Ok(())
}

pub fn remove_remote_skill(
    host: &HostRecord,
    agent: &HostAgentRecord,
    relative_path: &str,
) -> Result<()> {
    ensure_safe_remote_skill_relative_path(relative_path)?;
    if relative_path == "." {
        bail!("Refusing to remove the remote agent skill root");
    }
    let config = parse_ssh_config(&host.config_json)?;
    let root = remote_display_path_to_expr(&agent.skill_path);
    let script = remote_remove_script(&root, relative_path);
    run_ssh(&config.ssh_target, &script)?;
    Ok(())
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
                    "[ -d {} ]",
                    remote_path_expr_literal(&remote_display_path_to_expr(path))
                )
            })
            .collect::<Vec<_>>()
            .join(" || ");
        let count_script = skill_roots
            .iter()
            .map(|path| {
                let expr = remote_path_expr_literal(&remote_display_path_to_expr(path));
                format!(
                    "if [ -d {path} ]; then\n  root={path}\n  {find_skill_files}\nfi",
                    path = expr,
                    find_skill_files = remote_find_skill_files_script("$root"),
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

fn validate_ssh_target(target: &str) -> Result<()> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        bail!("SSH target is empty");
    }
    if trimmed.starts_with('-') {
        bail!("SSH target cannot start with '-'");
    }
    if trimmed
        .chars()
        .any(|ch| ch.is_control() || matches!(ch, '\0' | '\n' | '\r'))
    {
        bail!("SSH target contains control characters");
    }
    Ok(())
}

fn ensure_safe_remote_skill_relative_path(path: &str) -> Result<()> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        bail!("Remote skill path is empty");
    }
    if trimmed == "." {
        return Ok(());
    }
    if trimmed.starts_with('/')
        || trimmed.starts_with('~')
        || trimmed.contains('\\')
        || trimmed.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || !part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        })
    {
        bail!("Unsafe remote skill path: {path}");
    }
    Ok(())
}

fn remote_find_skill_files_script(root_expr: &str) -> String {
    format!(
        "find -L \"{root}\" -mindepth 1 -maxdepth 8 \\( -path \"{root}/.git\" -o -path '*/.git' \\) -prune -o -type f \\( -name SKILL.md -o -name skill.md \\) -print 2>/dev/null",
        root = root_expr
    )
}

fn remote_path_expr_literal(path: &str) -> String {
    if path == "$HOME" {
        return "$HOME".to_string();
    }
    if let Some(rest) = path.strip_prefix("$HOME/") {
        if rest.is_empty() {
            "$HOME".to_string()
        } else {
            format!("$HOME/{}", shell_quote(rest))
        }
    } else {
        shell_quote(path)
    }
}

fn remote_safe_parent_script(action: &str, create_missing_parent: bool) -> String {
    let missing_parent_action = if create_missing_parent {
        r#"mkdir -- "$parent""#
    } else {
        "exit 0"
    };
    format!(
        r#"root_real=$(cd "$root" && pwd -P)
parent="$root"
parent_rel=${{rel%/*}}
if [ "$parent_rel" = "$rel" ]; then
  parent_rel=
fi
if [ -n "$parent_rel" ]; then
  old_ifs=$IFS
  IFS=/
  set -- $parent_rel
  IFS=$old_ifs
  for part do
    parent="$parent/$part"
    if [ -L "$parent" ]; then
      echo "Refusing to {action} through symlink parent: $parent" >&2
      exit 1
    fi
    if [ -e "$parent" ]; then
      if [ ! -d "$parent" ]; then
        echo "Remote parent is not a directory: $parent" >&2
        exit 1
      fi
      parent_real=$(cd "$parent" && pwd -P)
      case "$parent_real" in
        "$root_real"|"$root_real"/*) ;;
        *) echo "Remote parent escapes skill root: $parent" >&2; exit 1 ;;
      esac
    else
      {missing_parent_action}
    fi
  done
fi
base=${{rel##*/}}
target="$parent/$base"
if [ -z "$target" ] || [ "$target" = "/" ] || [ "$target" = "$HOME" ]; then
  echo "Refusing to {action} unsafe target: $target" >&2
  exit 1
fi"#,
        action = action,
        missing_parent_action = missing_parent_action,
    )
}

fn remote_install_script(root: &str, relative_path: &str, temp_name: &str) -> String {
    format!(
        r#"set -e
root={root}
rel={rel}
if [ ! -d "$root" ]; then
  mkdir -p -- "$root"
fi
{safe_parent}
stage="$root/{temp_name}"
if [ -L "$stage" ]; then
  rm -f -- "$stage"
else
  rm -rf -- "$stage"
fi
mkdir -p -- "$stage"
tar -x -C "$stage"
if [ -L "$target" ]; then
  rm -f -- "$target"
elif [ -e "$target" ]; then
  if [ -d "$target" ]; then
    target_real=$(cd "$target" && pwd -P)
    case "$target_real" in
      "$root_real"|"$root_real"/*) ;;
      *) echo "Remote target escapes skill root: $target" >&2; exit 1 ;;
    esac
  fi
  rm -rf -- "$target"
fi
mv -- "$stage" "$target""#,
        root = remote_path_expr_literal(root),
        rel = shell_quote(relative_path),
        temp_name = temp_name,
        safe_parent = remote_safe_parent_script("write", true),
    )
}

fn remote_remove_script(root: &str, relative_path: &str) -> String {
    format!(
        r#"set -e
root={root}
rel={rel}
if [ ! -d "$root" ]; then
  exit 0
fi
{safe_parent}
if [ -L "$target" ]; then
  rm -f -- "$target"
elif [ -e "$target" ]; then
  if [ -d "$target" ]; then
    target_real=$(cd "$target" && pwd -P)
    case "$target_real" in
      "$root_real"|"$root_real"/*) ;;
      *) echo "Remote target escapes skill root: $target" >&2; exit 1 ;;
    esac
  fi
  rm -rf -- "$target"
fi"#,
        root = remote_path_expr_literal(root),
        rel = shell_quote(relative_path),
        safe_parent = remote_safe_parent_script("remove", false),
    )
}

fn run_ssh(target: &str, remote_script: &str) -> Result<String> {
    validate_ssh_target(target)?;
    let remote_command = format!("sh -lc {}", shell_quote(remote_script));
    let output = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            target,
            &remote_command,
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
        ensure_safe_remote_skill_relative_path, normalize_remote_path, parse_ssh_config_candidates,
        remote_adapter_skill_roots, remote_find_skill_files_script, remote_install_script,
        remote_path_expr_literal, remote_remove_script, shell_quote, validate_ssh_target,
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
    fn shell_quote_keeps_script_as_single_sh_c_argument() {
        let command = format!("sh -lc {}", shell_quote("printf connected"));
        assert_eq!(command, "sh -lc 'printf connected'");
    }

    #[test]
    fn ssh_target_rejects_client_option_injection() {
        assert!(validate_ssh_target("dev-box").is_ok());
        assert!(validate_ssh_target("alice@example.com").is_ok());
        assert!(validate_ssh_target("-oProxyCommand=touch /tmp/pwn").is_err());
        assert!(validate_ssh_target("dev\nbox").is_err());
    }

    #[test]
    fn remote_find_skill_files_is_bounded_and_prunes_git() {
        let script = remote_find_skill_files_script("$root");

        assert!(script.contains("find -L \"$root\""));
        assert!(script.contains("-mindepth 1"));
        assert!(script.contains("-maxdepth 8"));
        assert!(script.contains("-path \"$root/.git\""));
        assert!(script.contains("-path '*/.git'"));
        assert!(script.contains("-prune"));
        assert!(script.contains("-name SKILL.md -o -name skill.md"));
    }

    #[test]
    fn remote_skill_relative_path_rejects_escape_paths() {
        assert!(ensure_safe_remote_skill_relative_path("codex-review").is_ok());
        assert!(ensure_safe_remote_skill_relative_path("group/codex-review").is_ok());
        assert!(ensure_safe_remote_skill_relative_path(".").is_ok());
        assert!(ensure_safe_remote_skill_relative_path("../secret").is_err());
        assert!(ensure_safe_remote_skill_relative_path("/tmp/secret").is_err());
        assert!(ensure_safe_remote_skill_relative_path("group//skill").is_err());
        assert!(ensure_safe_remote_skill_relative_path("~/.ssh").is_err());
        assert!(ensure_safe_remote_skill_relative_path("group/$(touch-pwn)").is_err());
        assert!(ensure_safe_remote_skill_relative_path("group/a b").is_err());
    }

    #[test]
    fn remote_path_expr_literals_do_not_expand_untrusted_shell_content() {
        assert_eq!(
            remote_path_expr_literal("$HOME/.codex/skills"),
            "$HOME/'.codex/skills'"
        );
        assert_eq!(
            remote_path_expr_literal("/tmp/a$(touch pwn)"),
            "'/tmp/a$(touch pwn)'"
        );
    }

    #[test]
    fn remote_install_script_guards_symlinks_and_containment() {
        let script = remote_install_script("$HOME/.codex/skills", "group/demo", ".stage");

        assert!(script.contains("root=$HOME/'.codex/skills'"));
        assert!(script.contains("rel='group/demo'"));
        assert!(!script.contains("root=\"$HOME"));
        assert!(script.contains("if [ -L \"$parent\" ]"));
        assert!(script.contains("Refusing to write through symlink parent"));
        assert!(script.contains("if [ -L \"$target\" ]; then"));
        assert!(script.contains("rm -f -- \"$target\""));
        assert!(script.contains("target_real=$(cd \"$target\" && pwd -P)"));
        assert!(script.contains("\"$root_real\"|\"$root_real\"/*"));
        assert!(script.contains("rm -rf -- \"$target\""));
        assert!(script.contains("mv -- \"$stage\" \"$target\""));
    }

    #[test]
    fn remote_remove_script_deletes_symlink_itself_and_does_not_create_missing_parent() {
        let script = remote_remove_script("$HOME/.codex/skills", "group/demo");

        assert!(script.contains("root=$HOME/'.codex/skills'"));
        assert!(script.contains("rel='group/demo'"));
        assert!(!script.contains("root=\"$HOME"));
        assert!(script.contains("if [ -L \"$parent\" ]"));
        assert!(script.contains("Refusing to remove through symlink parent"));
        assert!(script.contains("if [ -L \"$target\" ]; then"));
        assert!(script.contains("rm -f -- \"$target\""));
        assert!(script.contains("target_real=$(cd \"$target\" && pwd -P)"));
        assert!(script.contains("\"$root_real\"|\"$root_real\"/*"));
        assert!(script.contains("rm -rf -- \"$target\""));
        assert!(script.contains("else\n      exit 0"));
        assert!(!script.contains("mkdir -- \"$parent\""));
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
