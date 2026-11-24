//! Agents that read skills from more than their own directory (#696).
//!
//! Several agents also read a shared root such as `~/.agents/skills`
//! ([`ToolAdapter::additional_scan_dirs`]). This module answers two questions
//! from that data: which agents read a given agent's directory, and whether a
//! deployment leaves some agent finding the same skill in two directories.
//! It only reports; it never changes what a deployment does.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{
    error::AppError,
    skill_store::{SkillStore, SkillTargetRecord},
    tool_adapters::{self, ToolAdapter},
};

/// The directories one agent reads skills from.
#[derive(Debug, Clone)]
pub struct AgentDirs {
    pub key: String,
    pub display_name: String,
    pub installed: bool,
    pub own: PathBuf,
    /// Additional directories, excluding any that are the same as `own`.
    pub also: Vec<PathBuf>,
}

impl AgentDirs {
    /// `extra` is what the user declared for this agent
    /// ([`tool_adapters::tool_extra_scan_dirs`]), merged with the built-in
    /// additional dirs.
    pub fn from_adapter(adapter: &ToolAdapter, extra: &[String]) -> Self {
        let own = adapter.skills_dir();
        let mut keys = vec![dir_key(&own)];
        let mut also = Vec::new();
        for dir in adapter
            .additional_scan_dir_paths()
            .into_iter()
            .chain(extra.iter().map(PathBuf::from))
        {
            let key = dir_key(&dir);
            if !keys.contains(&key) {
                keys.push(key);
                also.push(dir);
            }
        }
        Self {
            key: adapter.key.clone(),
            display_name: adapter.display_name.clone(),
            installed: adapter.is_installed(),
            own,
            also,
        }
    }
}

pub fn all_agent_dirs(store: &SkillStore) -> Vec<AgentDirs> {
    let extra = tool_adapters::tool_extra_scan_dirs(store);
    tool_adapters::all_tool_adapters(store)
        .iter()
        .map(|adapter| {
            AgentDirs::from_adapter(adapter, extra.get(&adapter.key).map_or(&[], Vec::as_slice))
        })
        .collect()
}

/// Comparable form of a directory: the deepest existing ancestor is
/// canonicalized, so a symlinked home or a different spelling of an existing
/// path still matches, and a directory that does not exist yet still compares.
pub fn dir_key(path: &Path) -> PathBuf {
    for ancestor in path.ancestors() {
        if let Ok(real) = dunce::canonicalize(ancestor) {
            return match path.strip_prefix(ancestor) {
                Ok(rest) if !rest.as_os_str().is_empty() => real.join(rest),
                _ => real,
            };
        }
    }
    path.to_path_buf()
}

/// Display names of the installed agents, other than `agent` itself, that also
/// read `agent`'s own directory.
pub fn read_by(agent: &AgentDirs, all: &[AgentDirs]) -> Vec<String> {
    let own = dir_key(&agent.own);
    all.iter()
        .filter(|other| other.key != agent.key && other.installed)
        .filter(|other| other.also.iter().any(|dir| dir_key(dir) == own))
        .map(|other| other.display_name.clone())
        .collect()
}

/// An agent that would find one skill in more than one directory it reads.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DuplicateDeployment {
    pub skill_id: String,
    pub skill_name: String,
    pub agent: String,
    pub agent_name: String,
    /// The directories the agent would find the skill in.
    pub dirs: Vec<String>,
}

/// Agents that see one of `skill_ids` twice, counting a duplicate only when
/// it involves at least one of `tool_keys`, so an unrelated existing
/// duplicate stays quiet.
///
/// With `planned`, `tool_keys` count as deployed already: the CLI asks before
/// deploying, so `--dry-run` gives the same answer. Without it only recorded
/// deployments count, for the app, which asks after deploying and must not
/// warn about a deploy that did not happen.
pub fn find_duplicate_deployments(
    store: &SkillStore,
    skill_ids: &[String],
    tool_keys: &[String],
    planned: bool,
) -> Result<Vec<DuplicateDeployment>, AppError> {
    let agents = all_agent_dirs(store);
    let targets = store.get_all_targets().map_err(AppError::db)?;
    let mut skills = Vec::new();
    for id in skill_ids {
        if let Some(skill) = store.get_skill_by_id(id).map_err(AppError::db)? {
            skills.push((skill.id, skill.name));
        }
    }
    Ok(duplicates(&agents, &targets, &skills, tool_keys, planned))
}

fn duplicates(
    agents: &[AgentDirs],
    targets: &[SkillTargetRecord],
    skills: &[(String, String)],
    tool_keys: &[String],
    planned: bool,
) -> Vec<DuplicateDeployment> {
    let mut out = Vec::new();
    for (skill_id, skill_name) in skills {
        // Every directory the skill is (or is about to be) in, and whether it
        // got there through this deployment.
        let mut present: Vec<(PathBuf, PathBuf, bool)> = Vec::new();
        let mut add = |dir: PathBuf, from_deploy: bool| {
            let key = dir_key(&dir);
            match present.iter_mut().find(|(k, _, _)| *k == key) {
                Some(entry) => entry.2 |= from_deploy,
                None => present.push((key, dir, from_deploy)),
            }
        };
        for target in targets
            .iter()
            .filter(|t| &t.skill_id == skill_id && t.status == "ok")
        {
            if let Some(dir) = Path::new(&target.target_path).parent() {
                add(dir.to_path_buf(), tool_keys.contains(&target.tool));
            }
        }
        if planned {
            for agent in agents.iter().filter(|a| tool_keys.contains(&a.key)) {
                add(agent.own.clone(), true);
            }
        }

        for reader in agents.iter().filter(|a| a.installed && !a.also.is_empty()) {
            let reads: HashSet<PathBuf> = std::iter::once(&reader.own)
                .chain(&reader.also)
                .map(|dir| dir_key(dir))
                .collect();
            let hits: Vec<&(PathBuf, PathBuf, bool)> = present
                .iter()
                .filter(|(k, _, _)| reads.contains(k))
                .collect();
            if hits.len() >= 2 && hits.iter().any(|(_, _, from_deploy)| *from_deploy) {
                out.push(DuplicateDeployment {
                    skill_id: skill_id.clone(),
                    skill_name: skill_name.clone(),
                    agent: reader.key.clone(),
                    agent_name: reader.display_name.clone(),
                    dirs: hits
                        .iter()
                        .map(|(_, dir, _)| dir.to_string_lossy().to_string())
                        .collect(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::skill_store::SkillRecord;
    use crate::core::tool_adapters::CustomToolDef;
    use crate::core::tool_service;
    use tempfile::tempdir;

    fn agent(key: &str, own: &Path, also: &[&Path]) -> AgentDirs {
        AgentDirs {
            key: key.into(),
            display_name: key.to_uppercase(),
            installed: true,
            own: own.to_path_buf(),
            also: also.iter().map(|p| p.to_path_buf()).collect(),
        }
    }

    fn target(skill: &str, tool: &str, dir: &Path) -> SkillTargetRecord {
        SkillTargetRecord {
            id: format!("{skill}-{tool}"),
            skill_id: skill.into(),
            tool: tool.into(),
            target_path: dir.join(skill).to_string_lossy().to_string(),
            mode: "symlink".into(),
            status: "ok".into(),
            synced_at: None,
            last_error: None,
            source_hash: None,
        }
    }

    fn skills() -> Vec<(String, String)> {
        vec![("s1".into(), "my-skill".into())]
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        shared: PathBuf,
        codex_dir: PathBuf,
        agents: Vec<AgentDirs>,
    }

    /// Codex reads its own dir plus `shared`; a custom agent owns `shared`;
    /// Claude reads only its own dir.
    fn fixture() -> Fixture {
        let tmp = tempdir().unwrap();
        let shared = tmp.path().join(".agents/skills");
        let codex_dir = tmp.path().join(".codex/skills");
        let claude_dir = tmp.path().join(".claude/skills");
        std::fs::create_dir_all(&shared).unwrap();
        let agents = vec![
            agent("codex", &codex_dir, &[&shared]),
            agent("agents_shared", &shared, &[]),
            agent("claude_code", &claude_dir, &[]),
        ];
        Fixture {
            _tmp: tmp,
            shared,
            codex_dir,
            agents,
        }
    }

    #[test]
    fn deploying_to_agent_already_covered_by_shared_dir_warns() {
        let f = fixture();
        let targets = vec![target("s1", "agents_shared", &f.shared)];
        let found = duplicates(&f.agents, &targets, &skills(), &["codex".into()], true);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].agent, "codex");
        assert_eq!(found[0].skill_name, "my-skill");
        assert_eq!(found[0].dirs.len(), 2);
    }

    #[test]
    fn deploying_to_shared_dir_warns_about_its_readers() {
        let f = fixture();
        let targets = vec![target("s1", "codex", &f.codex_dir)];
        let found = duplicates(
            &f.agents,
            &targets,
            &skills(),
            &["agents_shared".into()],
            true,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].agent, "codex");
    }

    #[test]
    fn deploying_elsewhere_does_not_report_existing_duplicate() {
        let f = fixture();
        let targets = vec![
            target("s1", "codex", &f.codex_dir),
            target("s1", "agents_shared", &f.shared),
        ];
        let found = duplicates(
            &f.agents,
            &targets,
            &skills(),
            &["claude_code".into()],
            true,
        );
        assert!(found.is_empty());
    }

    #[test]
    fn single_directory_is_not_a_duplicate() {
        let f = fixture();
        let found = duplicates(&f.agents, &[], &skills(), &["codex".into()], true);
        assert!(found.is_empty());
        let found = duplicates(&f.agents, &[], &skills(), &["agents_shared".into()], true);
        assert!(found.is_empty());
    }

    #[test]
    fn failed_target_does_not_count() {
        let f = fixture();
        let mut failed = target("s1", "agents_shared", &f.shared);
        failed.status = "error".into();
        let found = duplicates(&f.agents, &[failed], &skills(), &["codex".into()], true);
        assert!(found.is_empty());
    }

    #[test]
    fn uninstalled_reader_is_ignored() {
        let mut f = fixture();
        f.agents[0].installed = false;
        let targets = vec![target("s1", "agents_shared", &f.shared)];
        let found = duplicates(&f.agents, &targets, &skills(), &["codex".into()], true);
        assert!(found.is_empty());
    }

    #[test]
    fn unplanned_check_only_counts_recorded_deployments() {
        let f = fixture();
        let shared_only = vec![target("s1", "agents_shared", &f.shared)];
        // The app asks after deploying: no Codex row means the deploy did not happen.
        let found = duplicates(&f.agents, &shared_only, &skills(), &["codex".into()], false);
        assert!(found.is_empty());
        let both = vec![
            target("s1", "agents_shared", &f.shared),
            target("s1", "codex", &f.codex_dir),
        ];
        let found = duplicates(&f.agents, &both, &skills(), &["codex".into()], false);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn dir_key_matches_spellings_of_one_directory() {
        let f = fixture();
        let dotted = f.shared.join("..").join("skills");
        assert_eq!(dir_key(&dotted), dir_key(&f.shared));
        // A directory that does not exist yet still compares by its path.
        assert_eq!(dir_key(&f.codex_dir), dir_key(&f.codex_dir.join(".")));
    }

    #[test]
    fn read_by_lists_installed_agents_reading_the_dir() {
        let f = fixture();
        assert_eq!(read_by(&f.agents[1], &f.agents), vec!["CODEX".to_string()]);
        assert!(read_by(&f.agents[0], &f.agents).is_empty());
    }

    /// A store with a custom agent that owns `shared`, the built-in Hermes
    /// pointed at its own temp dir (a path override also marks it installed),
    /// and skill `s1` deployed to the shared agent.
    fn hermes_store(tmp: &Path) -> (SkillStore, PathBuf, PathBuf) {
        let store = SkillStore::new(&tmp.join("test.db")).unwrap();
        let shared = tmp.join(".agents").join("skills");
        let hermes = tmp.join(".hermes").join("skills");
        std::fs::create_dir_all(&shared).unwrap();
        let customs = vec![CustomToolDef {
            key: "agents_shared".into(),
            display_name: "Shared".into(),
            skills_dir: shared.to_string_lossy().to_string(),
            project_relative_skills_dir: None,
            category: Default::default(),
            icon: None,
        }];
        tool_service::set_custom_tools(&store, &customs).unwrap();
        let paths = [("hermes".to_string(), hermes.to_string_lossy().to_string())].into();
        tool_service::set_custom_tool_paths(&store, &paths).unwrap();
        let central = tmp.join("library").join("my-skill");
        store
            .insert_skill(&SkillRecord {
                id: "s1".into(),
                name: "my-skill".into(),
                description: None,
                source_type: "import".into(),
                source_ref: None,
                source_ref_resolved: None,
                source_subpath: None,
                source_branch: None,
                source_revision: None,
                remote_revision: None,
                central_path: central.to_string_lossy().to_string(),
                content_hash: None,
                enabled: true,
                created_at: 1,
                updated_at: 1,
                status: "ok".into(),
                update_status: "local_only".into(),
                last_checked_at: None,
                last_check_error: None,
            })
            .unwrap();
        store
            .insert_target(&target("s1", "agents_shared", &shared))
            .unwrap();
        (store, shared, hermes)
    }

    fn info(store: &SkillStore, key: &str) -> tool_service::ToolInfo {
        tool_service::list_tool_info(store)
            .into_iter()
            .find(|i| i.key == key)
            .unwrap()
    }

    #[test]
    fn user_declared_dir_makes_hermes_see_a_shared_skill_twice() {
        let tmp = tempdir().unwrap();
        let (store, shared, _) = hermes_store(tmp.path());
        let ids = vec!["s1".to_string()];
        let hermes = vec!["hermes".to_string()];

        // Hermes reads `~/.agents/skills` only when its config says so, so
        // nothing is known until the user declares it.
        assert!(find_duplicate_deployments(&store, &ids, &hermes, true)
            .unwrap()
            .is_empty());

        tool_service::set_tool_extra_scan_dirs(
            &store,
            "hermes",
            &[shared.to_string_lossy().to_string()],
        )
        .unwrap();
        let found = find_duplicate_deployments(&store, &ids, &hermes, true).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].agent, "hermes");
        assert_eq!(found[0].skill_name, "my-skill");
        // The shared agent's card names Hermes as a reader.
        assert_eq!(
            info(&store, "agents_shared").read_by,
            vec!["Hermes Agent".to_string()]
        );
    }

    #[test]
    fn extra_scan_dirs_round_trip_through_settings() {
        let tmp = tempdir().unwrap();
        let (store, shared, hermes) = hermes_store(tmp.path());

        // Without the setting every agent loads with no declared dirs.
        assert!(store.get_setting("tool_extra_scan_dirs").unwrap().is_none());
        assert!(info(&store, "hermes").extra_scan_dirs.is_empty());

        let shared_str = shared.to_string_lossy().to_string();
        let saved = tool_service::set_tool_extra_scan_dirs(
            &store,
            "hermes",
            &[
                shared_str.clone(),
                // Another spelling of the same dir, and Hermes' own dir: both dropped.
                shared.join(".").to_string_lossy().to_string(),
                hermes.to_string_lossy().to_string(),
            ],
        )
        .unwrap();
        assert_eq!(saved, vec![shared_str.clone()]);
        let hermes_info = info(&store, "hermes");
        assert_eq!(hermes_info.extra_scan_dirs, vec![shared_str.clone()]);
        assert!(hermes_info.also_reads.contains(&shared_str));

        // Custom agents take the same setting.
        tool_service::set_tool_extra_scan_dirs(
            &store,
            "agents_shared",
            &[hermes.to_string_lossy().to_string()],
        )
        .unwrap();
        assert_eq!(info(&store, "agents_shared").also_reads.len(), 1);

        // Clearing removes the entry; a relative path or unknown agent is refused.
        tool_service::set_tool_extra_scan_dirs(&store, "hermes", &[]).unwrap();
        assert!(info(&store, "hermes").extra_scan_dirs.is_empty());
        let raw = store.get_setting("tool_extra_scan_dirs").unwrap().unwrap();
        let saved: std::collections::HashMap<String, Vec<String>> =
            serde_json::from_str(&raw).unwrap();
        assert!(!saved.contains_key("hermes"));
        assert!(saved.contains_key("agents_shared"));
        assert!(
            tool_service::set_tool_extra_scan_dirs(&store, "hermes", &["relative/dir".into()])
                .is_err()
        );
        assert!(tool_service::set_tool_extra_scan_dirs(&store, "no_such_agent", &[]).is_err());

        // A corrupt value loads as empty rather than failing.
        store
            .set_setting("tool_extra_scan_dirs", "not json")
            .unwrap();
        assert!(info(&store, "hermes").extra_scan_dirs.is_empty());
    }
}
