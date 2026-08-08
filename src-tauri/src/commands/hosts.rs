use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::core::hosts::{
    copy_local_skill_to_remote, copy_remote_skill_to_local, inspect_ssh_host_for_add,
    list_importable_ssh_hosts, list_remote_skill_hashes, list_remote_skills, local_host_agents,
    mark_host_offline, now_ts, parse_ssh_config, refresh_ssh_host, remote_skill_path,
    remove_remote_skill, resolve_ssh_target, serialize_ssh_config, test_ssh_connection,
    HostAgentMetadata, SshConfigImportCandidate, SshHostConfig, LOCAL_HOST_ID,
};
use crate::core::repo_lock::RepoLock;
use crate::core::skill_store::{HostAgentRecord, HostRecord, SkillRecord, SkillStore};
use crate::core::{content_hash, error::AppError, installer, sync_metadata};

#[derive(Debug, Serialize, Clone)]
pub struct HostAgentDto {
    pub agent_type: String,
    pub display_name: String,
    pub skill_path: String,
    pub status: String,
    pub skill_count: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct HostDto {
    pub id: String,
    pub name: String,
    pub host_type: String,
    pub status: String,
    pub platform: String,
    pub user: Option<String>,
    pub connection_label: String,
    pub agent_count: usize,
    pub skill_count: usize,
    pub updated_at: i64,
    pub agents: Vec<HostAgentDto>,
}

#[derive(Debug, Serialize)]
pub struct HostSkillDto {
    pub name: String,
    pub relative_path: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct RemoteWorkspaceSkillDto {
    pub key: String,
    pub name: String,
    pub relative_path: String,
    pub remote_path: Option<String>,
    pub library_skill_id: Option<String>,
    pub library_version: Option<String>,
    pub remote_hash: Option<String>,
    pub library_hash: Option<String>,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct RemoteOperationSummary {
    pub changed: usize,
    pub skipped: usize,
}

fn local_host_dto(store: &SkillStore) -> HostDto {
    let agents: Vec<HostAgentDto> = local_host_agents(store)
        .into_iter()
        .map(|(adapter, skill_count)| HostAgentDto {
            agent_type: adapter.key.clone(),
            display_name: adapter.display_name.clone(),
            skill_path: adapter.skills_dir().display().to_string(),
            status: "connected".to_string(),
            skill_count,
        })
        .collect();
    let user = std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("USERNAME").ok());
    let skill_count = agents.iter().map(|agent| agent.skill_count).sum();
    HostDto {
        id: LOCAL_HOST_ID.to_string(),
        name: "Local".to_string(),
        host_type: "local".to_string(),
        status: "connected".to_string(),
        platform: std::env::consts::OS.to_string(),
        user,
        connection_label: "This device".to_string(),
        agent_count: agents.len(),
        skill_count,
        updated_at: now_ts(),
        agents,
    }
}

fn host_agent_dto(record: &HostAgentRecord) -> HostAgentDto {
    let metadata = record
        .metadata_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<HostAgentMetadata>(json).ok());
    HostAgentDto {
        agent_type: record.agent_type.clone(),
        display_name: metadata
            .as_ref()
            .map(|value| value.display_name.clone())
            .unwrap_or_else(|| record.agent_type.clone()),
        skill_path: record.skill_path.clone(),
        status: record.status.clone(),
        skill_count: metadata.map(|value| value.skill_count).unwrap_or(0),
    }
}

fn ssh_connection_label(config: &SshHostConfig) -> String {
    let host = config
        .host_name
        .as_deref()
        .unwrap_or(config.ssh_target.as_str());
    match (&config.user, config.port) {
        (Some(user), Some(port)) => format!("{}@{}:{}", user, host, port),
        (Some(user), None) => format!("{}@{}", user, host),
        (None, Some(port)) => format!("{}:{}", host, port),
        (None, None) => host.to_string(),
    }
}

fn host_to_dto(record: &HostRecord, agents: Vec<HostAgentRecord>) -> Result<HostDto, AppError> {
    let config = parse_ssh_config(&record.config_json).map_err(AppError::internal)?;
    let agents: Vec<HostAgentDto> = agents.iter().map(host_agent_dto).collect();
    let skill_count = agents.iter().map(|agent| agent.skill_count).sum();
    Ok(HostDto {
        id: record.id.clone(),
        name: record.name.clone(),
        host_type: record.host_type.clone(),
        status: record.status.clone(),
        platform: "ssh".to_string(),
        user: config.user.clone(),
        connection_label: ssh_connection_label(&config),
        agent_count: agents.len(),
        skill_count,
        updated_at: record.updated_at,
        agents,
    })
}

fn remote_host_and_agent(
    store: &SkillStore,
    host_id: &str,
    agent_type: &str,
) -> Result<(HostRecord, HostAgentRecord), AppError> {
    if host_id == LOCAL_HOST_ID {
        return Err(AppError::invalid_input(
            "Remote workspace operations require an SSH host",
        ));
    }
    let host = store
        .get_host_by_id(host_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found(format!("Host not found: {host_id}")))?;
    let agent = store
        .get_host_agents(host_id)
        .map_err(AppError::db)?
        .into_iter()
        .find(|record| record.agent_type == agent_type)
        .ok_or_else(|| AppError::not_found(format!("Agent not found on host: {agent_type}")))?;
    Ok((host, agent))
}

fn skill_target_relative_path(skill: &SkillRecord) -> String {
    Path::new(&skill.central_path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| skill.name.clone())
}

fn central_skill_hash(skill: &SkillRecord) -> Option<String> {
    content_hash::hash_directory(Path::new(&skill.central_path)).ok()
}

#[tauri::command]
pub async fn list_hosts(store: State<'_, Arc<SkillStore>>) -> Result<Vec<HostDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut hosts = vec![local_host_dto(&store)];
        for host in store.get_all_hosts().map_err(AppError::db)? {
            let agents = store.get_host_agents(&host.id).map_err(AppError::db)?;
            hosts.push(host_to_dto(&host, agents)?);
        }
        Ok(hosts)
    })
    .await?
}

#[tauri::command]
pub async fn list_importable_ssh_hosts_cmd() -> Result<Vec<SshConfigImportCandidate>, AppError> {
    tauri::async_runtime::spawn_blocking(move || list_importable_ssh_hosts().map_err(AppError::io))
        .await?
}

#[tauri::command]
pub async fn test_ssh_host_connection(ssh_target: String) -> Result<HostDto, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = resolve_ssh_target(ssh_target.trim()).map_err(AppError::io)?;
        test_ssh_connection(&config).map_err(AppError::network)?;
        Ok(HostDto {
            id: "preview".to_string(),
            name: config.ssh_target.clone(),
            host_type: "ssh".to_string(),
            status: "connected".to_string(),
            platform: "ssh".to_string(),
            user: config.user.clone(),
            connection_label: ssh_connection_label(&config),
            agent_count: 0,
            skill_count: 0,
            updated_at: now_ts(),
            agents: Vec::new(),
        })
    })
    .await?
}

#[tauri::command]
pub async fn add_ssh_host(
    name: String,
    ssh_target: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<HostDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let name = name.trim().to_string();
        let ssh_target = ssh_target.trim().to_string();
        if name.is_empty() || ssh_target.is_empty() {
            return Err(AppError::invalid_input("Name and SSH target are required"));
        }
        let existing = store.get_all_hosts().map_err(AppError::db)?;
        if existing
            .iter()
            .any(|host| host.name.eq_ignore_ascii_case(&name))
        {
            return Err(AppError::invalid_input(format!(
                "Host name already exists: {name}"
            )));
        }
        if existing.iter().any(|host| {
            parse_ssh_config(&host.config_json)
                .map(|config| config.ssh_target == ssh_target)
                .unwrap_or(false)
        }) {
            return Err(AppError::invalid_input(format!(
                "SSH target already exists: {ssh_target}"
            )));
        }

        let now = now_ts();
        let host = HostRecord {
            id: Uuid::new_v4().to_string(),
            name,
            host_type: "ssh".to_string(),
            config_json: serialize_ssh_config(&SshHostConfig {
                ssh_target,
                ..SshHostConfig::default()
            })
            .map_err(AppError::internal)?,
            status: "connected".to_string(),
            created_at: now,
            updated_at: now,
        };
        let (refreshed, agents) =
            inspect_ssh_host_for_add(&store, &host).map_err(AppError::network)?;
        store.insert_host(&refreshed).map_err(AppError::db)?;
        store
            .replace_host_agents(&refreshed.id, &agents)
            .map_err(AppError::db)?;
        host_to_dto(&refreshed, agents)
    })
    .await?
}

#[tauri::command]
pub async fn refresh_host(
    host_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<HostDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if host_id == LOCAL_HOST_ID {
            return Ok(local_host_dto(&store));
        }
        let host = store
            .get_host_by_id(&host_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found(format!("Host not found: {host_id}")))?;
        let refreshed = match refresh_ssh_host(&store, &host) {
            Ok(updated) => updated,
            Err(error) => {
                mark_host_offline(&store, &host, &error.to_string()).map_err(AppError::db)?
            }
        };
        let agents = store.get_host_agents(&refreshed.id).map_err(AppError::db)?;
        host_to_dto(&refreshed, agents)
    })
    .await?
}

#[tauri::command]
pub async fn delete_host(
    host_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if host_id == LOCAL_HOST_ID {
            return Err(AppError::invalid_input("Local host cannot be deleted"));
        }
        store.delete_host(&host_id).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn list_host_skills(
    host_id: String,
    agent_type: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<HostSkillDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if host_id == LOCAL_HOST_ID {
            return Err(AppError::invalid_input(
                "Local host skill listing is not supported here",
            ));
        }
        let host = store
            .get_host_by_id(&host_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found(format!("Host not found: {host_id}")))?;
        let agent = store
            .get_host_agents(&host_id)
            .map_err(AppError::db)?
            .into_iter()
            .find(|record| record.agent_type == agent_type)
            .ok_or_else(|| AppError::not_found(format!("Agent not found on host: {agent_type}")))?;
        let entries = list_remote_skills(&host, &agent).map_err(AppError::network)?;
        Ok(entries
            .into_iter()
            .map(|entry| HostSkillDto {
                name: entry.name,
                relative_path: entry.relative_path,
                path: entry.path,
            })
            .collect())
    })
    .await?
}

#[tauri::command]
pub async fn list_remote_workspace_skills(
    host_id: String,
    agent_type: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<RemoteWorkspaceSkillDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (host, agent) = remote_host_and_agent(&store, &host_id, &agent_type)?;
        let remote_entries = list_remote_skills(&host, &agent).map_err(AppError::network)?;
        let remote_hashes = list_remote_skill_hashes(&host, &agent)
            .map_err(AppError::network)?
            .into_iter()
            .map(|entry| (entry.relative_path, entry.hash))
            .collect::<HashMap<_, _>>();
        let mut remote_by_rel = remote_entries
            .into_iter()
            .map(|entry| (entry.relative_path.clone(), entry))
            .collect::<HashMap<_, _>>();

        let mut out = Vec::new();
        for skill in store.get_all_skills().map_err(AppError::db)? {
            if !skill.enabled {
                continue;
            }
            let relative_path = skill_target_relative_path(&skill);
            let remote = remote_by_rel.remove(&relative_path);
            let library_hash = central_skill_hash(&skill);
            let remote_hash = remote_hashes.get(&relative_path).cloned();
            let status = match (&remote, &library_hash, &remote_hash) {
                (None, _, _) => "missing",
                (Some(_), Some(library), Some(remote)) if library == remote => "synced",
                (Some(_), _, _) => "conflict",
            };
            out.push(RemoteWorkspaceSkillDto {
                key: format!("library:{}", skill.id),
                name: skill.name.clone(),
                relative_path,
                remote_path: remote.as_ref().map(|entry| entry.path.clone()),
                library_skill_id: Some(skill.id.clone()),
                library_version: skill
                    .source_revision
                    .clone()
                    .or_else(|| skill.remote_revision.clone()),
                remote_hash,
                library_hash,
                status: status.to_string(),
            });
        }

        for entry in remote_by_rel.into_values() {
            out.push(RemoteWorkspaceSkillDto {
                key: format!("remote:{}", entry.relative_path),
                name: entry.name,
                relative_path: entry.relative_path.clone(),
                remote_path: Some(entry.path),
                library_skill_id: None,
                library_version: None,
                remote_hash: remote_hashes.get(&entry.relative_path).cloned(),
                library_hash: None,
                status: "remote_only".to_string(),
            });
        }

        out.sort_by(|a, b| {
            a.status
                .cmp(&b.status)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(out)
    })
    .await?
}

#[tauri::command]
pub async fn install_skill_to_remote_host(
    host_id: String,
    agent_type: String,
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<RemoteWorkspaceSkillDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (host, agent) = remote_host_and_agent(&store, &host_id, &agent_type)?;
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;
        let relative_path = skill_target_relative_path(&skill);
        copy_local_skill_to_remote(
            &host,
            &agent,
            Path::new(&skill.central_path),
            &relative_path,
        )
        .map_err(AppError::network)?;
        let library_hash = central_skill_hash(&skill);
        let remote_path = remote_skill_path(&agent, &relative_path).map_err(AppError::internal)?;
        Ok(RemoteWorkspaceSkillDto {
            key: format!("library:{}", skill.id),
            name: skill.name,
            relative_path,
            remote_path: Some(remote_path),
            library_skill_id: Some(skill.id),
            library_version: skill.source_revision.or(skill.remote_revision),
            remote_hash: library_hash.clone(),
            library_hash,
            status: "synced".to_string(),
        })
    })
    .await?
}

#[tauri::command]
pub async fn remove_skill_from_remote_host(
    host_id: String,
    agent_type: String,
    relative_path: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (host, agent) = remote_host_and_agent(&store, &host_id, &agent_type)?;
        remove_remote_skill(&host, &agent, &relative_path).map_err(AppError::network)
    })
    .await?
}

#[tauri::command]
pub async fn adopt_remote_skill_to_library(
    host_id: String,
    agent_type: String,
    relative_path: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<String, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (host, agent) = remote_host_and_agent(&store, &host_id, &agent_type)?;
        let _lock = RepoLock::acquire_foreground("adopt remote skill").map_err(AppError::db)?;
        let temp_dir =
            std::env::temp_dir().join(format!("skills-manager-remote-adopt-{}", Uuid::new_v4()));
        copy_remote_skill_to_local(&host, &agent, &relative_path, &temp_dir)
            .map_err(AppError::network)?;
        let base_name = relative_path
            .rsplit('/')
            .next()
            .filter(|value| !value.is_empty() && *value != ".")
            .unwrap_or("remote-skill");
        let result =
            installer::install_from_local(&temp_dir, Some(base_name)).map_err(AppError::io)?;
        let _ = std::fs::remove_dir_all(&temp_dir);
        let now = chrono::Utc::now().timestamp_millis();
        let id = Uuid::new_v4().to_string();
        let source_ref = format!("{}:{}:{}", host.name, agent.agent_type, relative_path);
        let record = SkillRecord {
            id: id.clone(),
            name: result.name.clone(),
            description: result.description.clone(),
            source_type: "remote".to_string(),
            source_ref: Some(source_ref),
            source_ref_resolved: None,
            source_subpath: Some(relative_path),
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: result.central_path.to_string_lossy().to_string(),
            content_hash: Some(result.content_hash),
            enabled: true,
            created_at: now,
            updated_at: now,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: Some(now),
            last_check_error: None,
        };
        store.insert_skill(&record).map_err(AppError::db)?;
        sync_metadata::write_all_from_db_unlocked(&store).map_err(AppError::db)?;
        Ok(id)
    })
    .await?
}

#[tauri::command]
pub async fn apply_preset_to_remote_host(
    host_id: String,
    agent_type: String,
    preset_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<RemoteOperationSummary, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (host, agent) = remote_host_and_agent(&store, &host_id, &agent_type)?;
        let preset_skills = store
            .get_skills_for_scenario(&preset_id)
            .map_err(AppError::db)?;
        let mut skipped = 0;
        let remote_hashes = list_remote_skill_hashes(&host, &agent)
            .map_err(AppError::network)?
            .into_iter()
            .map(|entry| (entry.relative_path, entry.hash))
            .collect::<HashMap<_, _>>();
        let mut changed = 0;
        for skill in preset_skills {
            let relative_path = skill_target_relative_path(&skill);
            let library_hash = central_skill_hash(&skill);
            let remote_hash = remote_hashes.get(&relative_path).cloned();
            if library_hash.is_some() && library_hash == remote_hash {
                skipped += 1;
                continue;
            }
            copy_local_skill_to_remote(
                &host,
                &agent,
                Path::new(&skill.central_path),
                &relative_path,
            )
            .map_err(AppError::network)?;
            changed += 1;
        }
        Ok(RemoteOperationSummary { changed, skipped })
    })
    .await?
}
