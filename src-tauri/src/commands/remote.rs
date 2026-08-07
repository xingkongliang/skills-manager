use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tauri::State;
use tempfile::tempdir;

use crate::commands::projects::ProjectSkillDocumentDto;
use crate::core::{
    error::AppError, git_fetcher, installer, project_scanner::ProjectSkillInfo,
    skill_store::SkillStore, tool_adapters,
};

const REMOTE_MACHINES_SETTING: &str = "remote_machines";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteMachine {
    pub id: String,
    pub name: String,
    pub ssh_target: String,
    pub skills_dir: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteSkill {
    pub name: String,
    pub path: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteSkillDocument {
    pub skill_id: String,
    pub filename: String,
    pub content: String,
    pub central_path: String,
}

fn load_remote_machines(store: &SkillStore) -> Result<Vec<RemoteMachine>, AppError> {
    store
        .get_setting(REMOTE_MACHINES_SETTING)
        .map_err(AppError::db)?
        .and_then(|raw| serde_json::from_str::<Vec<RemoteMachine>>(&raw).ok())
        .map(Ok)
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn save_remote_machines(store: &SkillStore, machines: &[RemoteMachine]) -> Result<(), AppError> {
    let json = serde_json::to_string(machines)
        .map_err(|e| AppError::internal(format!("Failed to serialize remote machines: {e}")))?;
    store
        .set_setting(REMOTE_MACHINES_SETTING, &json)
        .map_err(AppError::db)
}

fn validate_remote_input(name: &str, ssh_target: &str, skills_dir: &str) -> Result<(), AppError> {
    if name.trim().is_empty() {
        return Err(AppError::invalid_input("Remote machine name is required"));
    }
    if ssh_target.trim().is_empty() {
        return Err(AppError::invalid_input("SSH target is required"));
    }
    if skills_dir.trim().is_empty() {
        return Err(AppError::invalid_input("Remote skills path is required"));
    }
    if ssh_target.chars().any(|c| c.is_control())
        || skills_dir.chars().any(|c| c.is_control())
        || name.chars().any(|c| c.is_control())
        || ssh_target.chars().any(char::is_whitespace)
        || ssh_target.trim().starts_with('-')
    {
        return Err(AppError::invalid_input(
            "Remote machine fields contain invalid characters",
        ));
    }
    let skills_dir = skills_dir.trim();
    if !(skills_dir.starts_with("~/") || skills_dir.starts_with('/')) {
        return Err(AppError::invalid_input(
            "Remote skills path must be absolute or start with ~/",
        ));
    }
    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn remote_path_expr(path: &str) -> String {
    if path == "~" {
        "$HOME".to_string()
    } else if let Some(rest) = path.strip_prefix("~/") {
        format!("$HOME/{}", shell_quote(rest))
    } else {
        shell_quote(path)
    }
}

fn ssh_output(target: &str, script: &str) -> Result<String, AppError> {
    let output = Command::new("ssh")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ConnectTimeout=8")
        .arg(target)
        .arg(script)
        .output()
        .map_err(|e| AppError::io(format!("Failed to run ssh: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let msg = if !stderr.is_empty() { stderr } else { stdout };
        return Err(AppError::io(if msg.is_empty() {
            format!("ssh exited with status {}", output.status)
        } else {
            msg
        }));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_scp_dir_to(
    local_dir: &Path,
    machine: &RemoteMachine,
    remote_dir: &str,
) -> Result<(), AppError> {
    let remote_parent = remote_path_expr(
        remote_dir
            .trim_end_matches('/')
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or(&machine.skills_dir),
    );
    ssh_output(&machine.ssh_target, &format!("mkdir -p {remote_parent}"))?;

    let local_arg = format!("{}/.", local_dir.to_string_lossy());
    let remote_arg = format!("{}:{}", machine.ssh_target, remote_dir);
    let output = Command::new("scp")
        .arg("-r")
        .arg(local_arg)
        .arg(remote_arg)
        .output()
        .map_err(|e| AppError::io(format!("Failed to run scp: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::io(if stderr.is_empty() {
            format!("scp exited with status {}", output.status)
        } else {
            stderr
        }));
    }
    Ok(())
}

fn find_machine(store: &SkillStore, id: &str) -> Result<RemoteMachine, AppError> {
    load_remote_machines(store)?
        .into_iter()
        .find(|machine| machine.id == id)
        .ok_or_else(|| AppError::not_found("Remote machine not found"))
}

fn safe_remote_skill_name(name: &str) -> Result<String, AppError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.contains('/') || trimmed.contains('\\') {
        return Err(AppError::invalid_input("Invalid remote skill name"));
    }
    Ok(trimmed.to_string())
}

fn remote_skill_dir(machine: &RemoteMachine, skill_name: &str) -> String {
    format!(
        "{}/{}",
        machine.skills_dir.trim_end_matches('/'),
        skill_name.trim()
    )
}

fn adapter_for_agent(
    store: &SkillStore,
    agent: &str,
) -> Result<tool_adapters::ToolAdapter, AppError> {
    tool_adapters::all_tool_adapters(store)
        .into_iter()
        .find(|adapter| adapter.key == agent)
        .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent)))
}

fn remote_agent_skills_dir(adapter: &tool_adapters::ToolAdapter) -> String {
    if adapter.is_custom {
        if let Some(path) = adapter.override_skills_dir.as_deref() {
            return path.to_string();
        }
    }
    if adapter.relative_skills_dir.starts_with('/') {
        adapter.relative_skills_dir.clone()
    } else {
        format!("~/{}", adapter.relative_skills_dir.trim_start_matches('/'))
    }
}

fn remote_join(base: &str, child: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        child.trim_start_matches('/')
    )
}

fn safe_remote_relative_path(path: &str) -> Result<String, AppError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid_input("Invalid remote skill path"));
    }
    if trimmed.starts_with('/') || trimmed.contains('\\') {
        return Err(AppError::invalid_input("Invalid remote skill path"));
    }
    for component in trimmed.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(AppError::invalid_input("Invalid remote skill path"));
        }
    }
    Ok(trimmed.to_string())
}

fn parse_remote_agent_skill_line(
    line: &str,
) -> Option<(String, String, Option<String>, Vec<String>)> {
    let mut parts = line.splitn(4, '\t');
    let relative_path = parts.next()?.to_string();
    let name = parts.next()?.to_string();
    let description = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let files = parts
        .next()
        .unwrap_or_default()
        .split('|')
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .collect();
    Some((relative_path, name, description, files))
}

fn read_remote_center_skill_keys(machine: &RemoteMachine) -> HashSet<String> {
    let root = remote_path_expr(&machine.skills_dir);
    let output = ssh_output(
        &machine.ssh_target,
        &format!(
            r#"root={root}; mkdir -p "$root"; find -L "$root" -mindepth 1 -maxdepth 2 \( -path "$root/.git" -o -path "*/.git" \) -prune -o -type f \( -name SKILL.md -o -name skill.md \) -print | while IFS= read -r marker; do
dir=$(dirname "$marker")
rel=${{dir#"$root"/}}
base=$(basename "$dir")
case "$rel" in "") continue;; esac
printf '%s\n%s\n' "$rel" "$base"
done"#
        ),
    )
    .unwrap_or_default();
    output
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn read_remote_agent_skills_internal(
    machine: &RemoteMachine,
    adapter: &tool_adapters::ToolAdapter,
) -> Result<Vec<ProjectSkillInfo>, AppError> {
    let root = remote_path_expr(&remote_agent_skills_dir(adapter));
    let max_depth = if adapter.recursive_scan { 8 } else { 2 };
    let output = ssh_output(
        &machine.ssh_target,
        &format!(
            r#"root={root}; mkdir -p "$root"; find -L "$root" -mindepth 1 -maxdepth {max_depth} \( -path "$root/.git" -o -path "*/.git" \) -prune -o -type f \( -name SKILL.md -o -name skill.md \) -print | while IFS= read -r marker; do
dir=$(dirname "$marker")
rel=${{dir#"$root"/}}
case "$rel" in "") continue;; esac
name=$(basename "$dir")
desc=$(awk 'NR==1 && $0=="---" {{ in_fm=1; next }} in_fm && $0=="---" {{ exit }} in_fm && $0 ~ /^[[:space:]]*description:/ {{ sub(/^[[:space:]]*description:[[:space:]]*/, ""); gsub(/^\"|\"$/, ""); gsub(/^'\''|'\''$/, ""); print; exit }}' "$marker" 2>/dev/null || true)
files=$(find -L "$dir" -maxdepth 2 -type f -printf '%P|' 2>/dev/null || true)
printf '%s\t%s\t%s\t%s\n' "$rel" "$name" "$desc" "$files"
done"#
        ),
    )?;

    let center_skill_keys = read_remote_center_skill_keys(machine);
    let mut skills = Vec::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let Some((relative_path, name, description, files)) = parse_remote_agent_skill_line(line)
        else {
            continue;
        };
        let base = relative_path
            .rsplit('/')
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or(&name);
        let center = center_skill_keys.contains(&relative_path) || center_skill_keys.contains(base);
        let path = remote_join(&remote_agent_skills_dir(adapter), &relative_path);
        skills.push(ProjectSkillInfo {
            name,
            dir_name: relative_path
                .rsplit('/')
                .next()
                .unwrap_or(&relative_path)
                .to_string(),
            relative_path,
            description,
            path,
            files,
            enabled: true,
            agent: adapter.key.clone(),
            agent_display_name: adapter.display_name.clone(),
            tags: Vec::new(),
            in_center: center,
            sync_status: if center { "in_sync" } else { "project_only" }.to_string(),
            center_skill_id: None,
            last_modified_at: None,
            content_hash: None,
        });
    }
    skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(skills)
}

fn read_remote_skill_description(machine: &RemoteMachine, skill_path: &str) -> Option<String> {
    let quoted = shell_quote(skill_path);
    let output = ssh_output(
        &machine.ssh_target,
        &format!(
            "for f in {quoted}/SKILL.md {quoted}/skill.md; do test -f \"$f\" || continue; awk 'NR==1 && $0==\"---\" {{ in_fm=1; next }} in_fm && $0==\"---\" {{ exit }} in_fm && $0 ~ /^[[:space:]]*description:/ {{ sub(/^[[:space:]]*description:[[:space:]]*/, \"\"); gsub(/^\\\"|\\\"$/, \"\"); gsub(/^'\\''|'\\''$/, \"\"); print; exit }}' \"$f\"; break; done"
        ),
    )
    .ok()?;
    let trimmed = output.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[tauri::command]
pub async fn get_remote_machines(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<RemoteMachine>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || load_remote_machines(&store)).await?
}

#[tauri::command]
pub async fn upsert_remote_machine(
    machine: RemoteMachine,
    store: State<'_, Arc<SkillStore>>,
) -> Result<RemoteMachine, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        validate_remote_input(&machine.name, &machine.ssh_target, &machine.skills_dir)?;
        let mut machines = load_remote_machines(&store)?;
        let mut next = RemoteMachine {
            id: machine.id.trim().to_string(),
            name: machine.name.trim().to_string(),
            ssh_target: machine.ssh_target.trim().to_string(),
            skills_dir: machine.skills_dir.trim().to_string(),
        };
        if next.id.is_empty() {
            next.id = uuid::Uuid::new_v4().to_string();
        }
        if let Some(existing) = machines.iter_mut().find(|item| item.id == next.id) {
            *existing = next.clone();
        } else {
            machines.push(next.clone());
        }
        save_remote_machines(&store, &machines)?;
        Ok(next)
    })
    .await?
}

#[tauri::command]
pub async fn delete_remote_machine(
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut machines = load_remote_machines(&store)?;
        machines.retain(|machine| machine.id != id);
        save_remote_machines(&store, &machines)
    })
    .await?
}

#[tauri::command]
pub async fn test_remote_machine(machine: RemoteMachine) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        validate_remote_input(&machine.name, &machine.ssh_target, &machine.skills_dir)?;
        let dir = remote_path_expr(&machine.skills_dir);
        ssh_output(
            &machine.ssh_target,
            &format!("mkdir -p {dir} && test -d {dir} && test -w {dir}"),
        )?;
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn list_remote_skills(
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<RemoteSkill>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let machine = find_machine(&store, &id)?;
        let dir = remote_path_expr(&machine.skills_dir);
        let output = ssh_output(
            &machine.ssh_target,
            &format!(
                "mkdir -p {dir}; find -L {dir} -mindepth 1 -maxdepth 1 \\( -name .git -prune -o -type d -print \\)"
            ),
        )?;
        let mut skills = Vec::new();
        for line in output.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let quoted = shell_quote(line);
            let marker = ssh_output(
                &machine.ssh_target,
                &format!("test -f {quoted}/SKILL.md -o -f {quoted}/skill.md && printf yes || true"),
            )?;
            if marker.trim() == "yes" {
                let name = line
                    .rsplit('/')
                    .next()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(line)
                    .to_string();
                skills.push(RemoteSkill {
                    name,
                    path: line.to_string(),
                    description: read_remote_skill_description(&machine, line),
                });
            }
        }
        skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(skills)
    })
    .await?
}

#[tauri::command]
pub async fn get_remote_skill_document(
    id: String,
    skill_name: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<RemoteSkillDocument, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill_name = safe_remote_skill_name(&skill_name)?;
        let machine = find_machine(&store, &id)?;
        let skill_dir = remote_skill_dir(&machine, &skill_name);
        let quoted = remote_path_expr(&skill_dir);
        let filename = ssh_output(
            &machine.ssh_target,
            &format!(
                "if test -f {quoted}/SKILL.md; then printf SKILL.md; elif test -f {quoted}/skill.md; then printf skill.md; else exit 2; fi"
            ),
        )?;
        let filename = filename.trim().to_string();
        let content = ssh_output(
            &machine.ssh_target,
            &format!("cat {quoted}/{}", shell_quote(&filename)),
        )?;
        Ok(RemoteSkillDocument {
            skill_id: skill_name,
            filename,
            content,
            central_path: skill_dir,
        })
    })
    .await?
}

#[tauri::command]
pub async fn install_local_to_remote(
    id: String,
    source_path: String,
    name: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let machine = find_machine(&store, &id)?;
        let temp = tempdir().map_err(AppError::io)?;
        let skill_name =
            installer::resolve_local_skill_name(Path::new(&source_path), name.as_deref())
                .map_err(AppError::io)?;
        let staging = temp.path().join(&skill_name);
        installer::install_from_local_to_destination(
            Path::new(&source_path),
            Some(&skill_name),
            &staging,
        )
        .map_err(AppError::io)?;
        let remote_target = remote_skill_dir(&machine, &skill_name);
        let quoted = remote_path_expr(&remote_target);
        ssh_output(
            &machine.ssh_target,
            &format!("rm -rf -- {quoted} && mkdir -p {quoted}"),
        )?;
        run_scp_dir_to(&staging, &machine, &remote_target)?;
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn install_git_to_remote(
    id: String,
    repo_url: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if repo_url.trim().is_empty() || repo_url.chars().any(|c| c.is_control()) {
            return Err(AppError::invalid_input("Git repository URL is required"));
        }
        let machine = find_machine(&store, &id)?;
        let skills_root = remote_path_expr(&machine.skills_dir);
        let repo = shell_quote(repo_url.trim());
        ssh_output(
            &machine.ssh_target,
            &format!(
                "set -e; root={skills_root}; mkdir -p \"$root\"; tmp=$(mktemp -d); trap 'rm -rf \"$tmp\"' EXIT; git clone --depth 1 {repo} \"$tmp/repo\" >/dev/null; found=\"$tmp/found\"; find \"$tmp/repo\" -maxdepth 5 \\( -name SKILL.md -o -name skill.md \\) -type f | while IFS= read -r marker; do dir=$(dirname \"$marker\"); name=$(basename \"$dir\"); rm -rf -- \"$root/$name\"; mkdir -p \"$root/$name\"; cp -R \"$dir\"/. \"$root/$name\"/; printf . >> \"$found\"; done; test -s \"$found\""
            ),
        )?;
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn delete_remote_skill(
    id: String,
    skill_name: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill_name = safe_remote_skill_name(&skill_name)?;
        let machine = find_machine(&store, &id)?;
        let target = remote_skill_dir(&machine, &skill_name);
        let quoted = remote_path_expr(&target);
        ssh_output(&machine.ssh_target, &format!("rm -rf -- {quoted}"))?;
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn get_remote_agent_skills(
    id: String,
    agent: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<ProjectSkillInfo>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let machine = find_machine(&store, &id)?;
        let adapter = adapter_for_agent(&store, &agent)?;
        read_remote_agent_skills_internal(&machine, &adapter)
    })
    .await?
}

#[tauri::command]
pub async fn get_remote_agent_skill_document(
    id: String,
    agent: String,
    skill_relative_path: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ProjectSkillDocumentDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let machine = find_machine(&store, &id)?;
        let adapter = adapter_for_agent(&store, &agent)?;
        let relative_path = safe_remote_relative_path(&skill_relative_path)?;
        let root = remote_path_expr(&remote_agent_skills_dir(&adapter));
        let rel = shell_quote(&relative_path);
        let filename = ssh_output(
            &machine.ssh_target,
            &format!(
                "root={root}; if test -f \"$root\"/{rel}/SKILL.md; then printf SKILL.md; elif test -f \"$root\"/{rel}/skill.md; then printf skill.md; elif test -f \"$root\"/{rel}/README.md; then printf README.md; else exit 2; fi"
            ),
        )?;
        let filename = filename.trim().to_string();
        let file = shell_quote(&filename);
        let content = ssh_output(
            &machine.ssh_target,
            &format!("root={root}; cat \"$root\"/{rel}/{file}"),
        )?;
        Ok(ProjectSkillDocumentDto {
            skill_name: relative_path,
            filename,
            content,
        })
    })
    .await?
}

#[tauri::command]
pub async fn add_remote_skill_to_agent(
    id: String,
    skill_name: String,
    agent: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill_name = safe_remote_skill_name(&skill_name)?;
        let machine = find_machine(&store, &id)?;
        let adapter = adapter_for_agent(&store, &agent)?;
        let source_root = remote_path_expr(&machine.skills_dir);
        let target_root = remote_path_expr(&remote_agent_skills_dir(&adapter));
        let skill = shell_quote(&skill_name);
        ssh_output(
            &machine.ssh_target,
            &format!(
                "set -e; source_root={source_root}; target_root={target_root}; src=\"$source_root\"/{skill}; dst=\"$target_root\"/{skill}; test -d \"$src\"; mkdir -p \"$target_root\"; src_real=$(readlink -f \"$src\" 2>/dev/null || printf '%s' \"$src\"); dst_real=$(readlink -f \"$dst\" 2>/dev/null || printf '%s' \"$dst\"); if test \"$src\" = \"$dst\" -o \"$src_real\" = \"$dst_real\"; then exit 0; fi; rm -rf -- \"$dst\"; mkdir -p \"$dst\"; cp -R \"$src\"/. \"$dst\"/"
            ),
        )?;
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn remove_remote_agent_skill(
    id: String,
    agent: String,
    skill_relative_path: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let machine = find_machine(&store, &id)?;
        let adapter = adapter_for_agent(&store, &agent)?;
        let relative_path = safe_remote_relative_path(&skill_relative_path)?;
        let root = remote_path_expr(&remote_agent_skills_dir(&adapter));
        let center_root = remote_path_expr(&machine.skills_dir);
        let rel = shell_quote(&relative_path);
        ssh_output(
            &machine.ssh_target,
            &format!(
                "set -e; root={root}; center_root={center_root}; target=\"$root\"/{rel}; root_real=$(readlink -f \"$root\" 2>/dev/null || printf '%s' \"$root\"); center_real=$(readlink -f \"$center_root\" 2>/dev/null || printf '%s' \"$center_root\"); if test \"$root_real\" = \"$center_real\"; then exit 0; fi; if test -L \"$target\"; then rm -- \"$target\"; exit 0; fi; target_real=$(readlink -f \"$target\" 2>/dev/null || printf '%s' \"$target\"); case \"$target_real\" in \"$root_real\"/*) rm -rf -- \"$target\" ;; *) exit 3 ;; esac"
            ),
        )?;
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn install_skillssh_to_remote(
    id: String,
    source: String,
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if source.trim().is_empty() || skill_id.trim().is_empty() {
            return Err(AppError::invalid_input(
                "Marketplace skill source is required",
            ));
        }
        let machine = find_machine(&store, &id)?;
        let repo_url = format!("https://github.com/{}.git", source.trim());
        let temp_dir =
            git_fetcher::clone_repo_ref(&repo_url, None, None, store.proxy_url().as_deref())
                .map_err(AppError::classify_git_error)?;
        let result = (|| -> Result<(), AppError> {
            let skill_dir =
                crate::commands::skills::resolve_skill_dir(&temp_dir, None, Some(skill_id.trim()))?;
            let remote_name = safe_remote_skill_name(skill_id.trim())?;
            let staging = tempdir().map_err(AppError::io)?;
            let staged_skill = staging.path().join(&remote_name);
            installer::install_skill_dir_to_destination(&skill_dir, &remote_name, &staged_skill)
                .map_err(AppError::io)?;
            let remote_target = remote_skill_dir(&machine, &remote_name);
            let quoted = remote_path_expr(&remote_target);
            ssh_output(
                &machine.ssh_target,
                &format!("rm -rf -- {quoted} && mkdir -p {quoted}"),
            )?;
            run_scp_dir_to(&staged_skill, &machine, &remote_target)
        })();
        git_fetcher::cleanup_temp(&temp_dir);
        result
    })
    .await?
}
