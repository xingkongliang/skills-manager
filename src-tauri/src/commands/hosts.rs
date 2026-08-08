use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::core::error::AppError;
use crate::core::hosts::{
    inspect_ssh_host_for_add, list_importable_ssh_hosts, list_remote_skills, local_host_agents,
    mark_host_offline, now_ts, parse_ssh_config, refresh_ssh_host, resolve_ssh_target,
    serialize_ssh_config, test_ssh_connection, HostAgentMetadata, SshConfigImportCandidate,
    SshHostConfig, LOCAL_HOST_ID,
};
use crate::core::skill_store::{HostAgentRecord, HostRecord, SkillStore};

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
