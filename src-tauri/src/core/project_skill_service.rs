use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::error::AppError;
use super::project_scanner::{self, AgentSkillConfig, ProjectSkillInfo};
use super::skill_store::{ProjectRecord, SkillRecord, SkillStore};
use super::sync_engine::{self, ReplacePolicy};
use super::tool_adapters;

#[derive(Debug, Clone)]
pub struct ProjectAgentTarget {
    pub key: String,
    pub display_name: String,
    pub enabled: bool,
    pub installed: bool,
    pub is_custom: bool,
    pub skills_root: PathBuf,
    pub disabled_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ProjectSkillVariant {
    pub skill_id: Option<String>,
    pub skill_name: String,
    pub agent: String,
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub enum AddProjectSkillOutcome {
    Added(ProjectSkillVariant),
    AlreadyPresent(ProjectSkillVariant),
}

/// Return project skill targets using the same root grouping as the Projects UI.
pub fn list_project_agent_targets(
    store: &SkillStore,
    project_id: &str,
) -> Result<Vec<ProjectAgentTarget>, AppError> {
    let project = get_project(store, project_id)?;
    if project.workspace_type == "linked" {
        let (agent, display_name) = linked_workspace_agent(&project);
        return Ok(vec![ProjectAgentTarget {
            key: agent,
            display_name,
            enabled: true,
            installed: true,
            is_custom: false,
            skills_root: PathBuf::from(&project.path),
            disabled_root: project.disabled_path.as_ref().map(PathBuf::from),
        }]);
    }

    let disabled_tools = disabled_tools(store);
    Ok(project_agent_configs(store)
        .into_iter()
        .filter_map(|config| {
            let adapter = tool_adapters::find_adapter_with_store(store, &config.key);
            Some(ProjectAgentTarget {
                enabled: !disabled_tools.contains(&config.key),
                installed: adapter.as_ref().is_some_and(|adapter| adapter.is_installed()),
                is_custom: adapter.as_ref().is_some_and(|adapter| adapter.is_custom),
                skills_root: Path::new(&project.path).join(&config.relative_skills_dir),
                disabled_root: Some(Path::new(&project.path).join(format!(
                    "{}-disabled",
                    config.relative_skills_dir
                ))),
                key: config.key,
                display_name: config.display_name,
            })
        })
        .collect())
}

/// Scan the project and attach a central skill ID when the UI's matcher finds one.
pub fn scan_project_skill_variants(
    store: &SkillStore,
    project_id: &str,
) -> Result<Vec<ProjectSkillVariant>, AppError> {
    let project = get_project(store, project_id)?;
    let configs = project_agent_configs(store);
    let mut skills = read_workspace_skills(&project, &configs);
    let managed = store.get_all_skills().map_err(AppError::db)?;

    Ok(skills
        .drain(..)
        .map(|skill| {
            let skill_id = find_best_center_match(&skill, &managed).map(|record| record.id.clone());
            ProjectSkillVariant {
                skill_id,
                skill_name: skill.name,
                agent: skill.agent,
                relative_path: skill.relative_path,
                absolute_path: PathBuf::from(skill.path),
                enabled: skill.enabled,
            }
        })
        .collect())
}

/// Copy one central skill to one installed, enabled project agent without overwriting.
pub fn add_skill_to_project(
    store: &SkillStore,
    project_id: &str,
    skill_id: &str,
    agent: &str,
) -> Result<AddProjectSkillOutcome, AppError> {
    let target = resolve_project_agent_target(store, project_id, agent)?;
    if !target.enabled {
        return Err(AppError::invalid_input(format!(
            "Agent '{}' is disabled for this project",
            target.key
        )));
    }
    if !target.installed {
        return Err(AppError::invalid_input(format!(
            "Agent '{}' is not installed for this project",
            target.key
        )));
    }

    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found(format!("Skill not found: {skill_id}")))?;
    let source = PathBuf::from(&skill.central_path);
    let relative_path = sync_engine::target_dir_name(&source, &skill.name);
    ensure_safe_skill_relative_path(&relative_path)?;

    if let Some(variant) = find_existing_variant(&target, &relative_path)? {
        return Ok(AddProjectSkillOutcome::AlreadyPresent(ProjectSkillVariant {
            skill_id: Some(skill.id),
            skill_name: skill.name,
            agent: target.key,
            relative_path,
            absolute_path: variant.0,
            enabled: variant.1,
        }));
    }

    let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
    std::fs::create_dir_all(&target.skills_root)?;
    let destination = target.skills_root.join(&relative_path);
    let mode = sync_engine::sync_mode_for_tool(&target.key, configured_mode.as_deref());
    sync_engine::sync_skill(&source, &destination, mode, ReplacePolicy::NoClobber)
        .map_err(AppError::io)?;

    Ok(AddProjectSkillOutcome::Added(ProjectSkillVariant {
        skill_id: Some(skill.id),
        skill_name: skill.name,
        agent: target.key,
        relative_path,
        absolute_path: destination,
        enabled: true,
    }))
}

/// Remove one project copy, including from a known inactive project agent.
pub fn remove_skill_from_project(
    store: &SkillStore,
    project_id: &str,
    skill_relative_path: &str,
    agent: &str,
) -> Result<(), AppError> {
    let target = preview_remove_skill_from_project(store, project_id, skill_relative_path, agent)?;
    remove_workspace_skill_target(&target.absolute_path)
}

/// Resolve the exact project target that a removal would affect without writing.
pub fn preview_remove_skill_from_project(
    store: &SkillStore,
    project_id: &str,
    skill_relative_path: &str,
    agent: &str,
) -> Result<ProjectSkillVariant, AppError> {
    ensure_safe_skill_relative_path(skill_relative_path)?;
    let target = resolve_project_agent_target(store, project_id, agent)?;
    let enabled_path = target.skills_root.join(skill_relative_path);
    let disabled_path = target
        .disabled_root
        .as_ref()
        .map(|root| root.join(skill_relative_path));

    let (path, root, enabled) = if enabled_path.is_dir() {
        (enabled_path, target.skills_root, true)
    } else if let (Some(disabled_path), Some(disabled_root)) =
        (disabled_path, target.disabled_root)
    {
        if !disabled_path.is_dir() {
            return Err(AppError::not_found("Skill directory not found"));
        }
        (disabled_path, disabled_root, false)
    } else {
        return Err(AppError::not_found("Skill directory not found"));
    };

    ensure_dir_within_root(&path, &root)?;
    ensure_parent_within_root(&path, &root)?;
    let scanned = scan_project_skill_variants(store, project_id)?
        .into_iter()
        .find(|variant| {
            variant.agent == agent && variant.relative_path == skill_relative_path
        });
    Ok(scanned.unwrap_or_else(|| ProjectSkillVariant {
        skill_id: None,
        skill_name: Path::new(skill_relative_path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| skill_relative_path.to_string()),
        agent: agent.to_string(),
        relative_path: skill_relative_path.to_string(),
        absolute_path: path,
        enabled,
    }))
}

fn resolve_project_agent_target(
    store: &SkillStore,
    project_id: &str,
    agent: &str,
) -> Result<ProjectAgentTarget, AppError> {
    list_project_agent_targets(store, project_id)?
        .into_iter()
        .find(|target| target.key == agent)
        .ok_or_else(|| AppError::not_found(format!("Unknown project agent: {agent}")))
}

fn find_existing_variant(
    target: &ProjectAgentTarget,
    relative_path: &str,
) -> Result<Option<(PathBuf, bool)>, AppError> {
    let enabled = target.skills_root.join(relative_path);
    if path_entry_exists(&enabled)? {
        return Ok(Some((enabled, true)));
    }
    if let Some(disabled_root) = &target.disabled_root {
        let disabled = disabled_root.join(relative_path);
        if path_entry_exists(&disabled)? {
            return Ok(Some((disabled, false)));
        }
    }
    Ok(None)
}

fn path_entry_exists(path: &Path) -> Result<bool, AppError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(AppError::io(error)),
    }
}

fn get_project(store: &SkillStore, project_id: &str) -> Result<ProjectRecord, AppError> {
    store
        .get_project_by_id(project_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Workspace not found"))
}

fn disabled_tools(store: &SkillStore) -> HashSet<String> {
    store
        .get_setting("disabled_tools")
        .ok()
        .flatten()
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default()
        .into_iter()
        .collect()
}

pub(crate) fn project_agent_configs(store: &SkillStore) -> Vec<AgentSkillConfig> {
    let mut grouped: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for adapter in tool_adapters::all_tool_adapters(store) {
        let relative_skills_dir = adapter.project_relative_skills_dir().to_string();
        if relative_skills_dir.is_empty() {
            continue;
        }
        if let Some((_, agents)) = grouped
            .iter_mut()
            .find(|(dir, _)| *dir == relative_skills_dir)
        {
            agents.push((adapter.key, adapter.display_name));
        } else {
            grouped.push((
                relative_skills_dir,
                vec![(adapter.key, adapter.display_name)],
            ));
        }
    }

    grouped
        .into_iter()
        .filter_map(|(relative_skills_dir, agents)| {
            let (key, first_display_name) = agents.first()?.clone();
            let display_name = if agents.len() == 1 {
                first_display_name
            } else {
                agents
                    .into_iter()
                    .map(|(_, display_name)| display_name)
                    .collect::<Vec<_>>()
                    .join(" / ")
            };
            Some(AgentSkillConfig {
                key,
                display_name,
                relative_skills_dir,
            })
        })
        .collect()
}

pub(crate) fn linked_workspace_agent(project: &ProjectRecord) -> (String, String) {
    (
        project
            .linked_agent_key
            .clone()
            .unwrap_or_else(|| slugify_skill_dir_name(&project.name)),
        project
            .linked_agent_name
            .clone()
            .unwrap_or_else(|| project.name.clone()),
    )
}

pub(crate) fn read_workspace_skills(
    project: &ProjectRecord,
    configs: &[AgentSkillConfig],
) -> Vec<ProjectSkillInfo> {
    if project.workspace_type == "linked" {
        let (agent, display_name) = linked_workspace_agent(project);
        return project_scanner::read_linked_workspace_skills(
            Path::new(&project.path),
            project.disabled_path.as_deref().map(Path::new),
            &agent,
            &display_name,
            true,
        );
    }
    project_scanner::read_project_skills(Path::new(&project.path), configs)
}

pub(crate) fn resolve_agent_skills_roots(
    store: &SkillStore,
    project: &ProjectRecord,
    agent: &str,
) -> Option<(PathBuf, Option<PathBuf>)> {
    if project.workspace_type == "linked" {
        if linked_workspace_agent(project).0 != agent {
            return None;
        }
        return Some((
            PathBuf::from(&project.path),
            project.disabled_path.as_ref().map(PathBuf::from),
        ));
    }

    let adapter = tool_adapters::find_adapter_with_store(store, agent)?;
    let project_dir = adapter.project_relative_skills_dir();
    let skills_root = Path::new(&project.path).join(project_dir);
    let disabled_root = Path::new(&project.path).join(format!("{}-disabled", project_dir));
    Some((skills_root, Some(disabled_root)))
}

pub(crate) fn ensure_safe_skill_relative_path(
    skill_relative_path: &str,
) -> Result<(), AppError> {
    if skill_relative_path.trim().is_empty()
        || Path::new(skill_relative_path)
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(AppError::invalid_input("Invalid skill directory path"));
    }
    Ok(())
}

pub(crate) fn ensure_dir_within_root(path: &Path, root: &Path) -> Result<(), AppError> {
    let abs_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let abs_root = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()?.join(root)
    };
    if !abs_path.starts_with(&abs_root) {
        return Err(AppError::invalid_input("Invalid skill directory path"));
    }
    Ok(())
}

fn ensure_parent_within_root(path: &Path, root: &Path) -> Result<(), AppError> {
    let canonical_root = std::fs::canonicalize(root)?;
    let parent = path
        .parent()
        .ok_or_else(|| AppError::invalid_input("Invalid skill directory path"))?;
    let canonical_parent = std::fs::canonicalize(parent)?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err(AppError::invalid_input("Invalid skill directory path"));
    }
    Ok(())
}

pub(crate) fn remove_workspace_skill_target(path: &Path) -> Result<(), AppError> {
    sync_engine::remove_target(path).map_err(AppError::io)
}

pub(crate) fn slugify_skill_dir_name(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in name.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches(|ch| matches!(ch, '-' | '_' | '.'));
    if trimmed.is_empty() {
        "skill".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn source_ref_matches_skill_path(
    skill_path: &str,
    skill_canonical: Option<&Path>,
    managed: &SkillRecord,
) -> bool {
    let Some(source_ref) = managed.source_ref.as_deref() else {
        return false;
    };
    if source_ref == skill_path {
        return true;
    }
    let Some(skill_canonical) = skill_canonical else {
        return false;
    };
    std::fs::canonicalize(source_ref)
        .map(|source_canonical| source_canonical == skill_canonical)
        .unwrap_or(false)
}

pub(crate) fn find_best_center_match<'a>(
    skill: &ProjectSkillInfo,
    all_managed: &'a [SkillRecord],
) -> Option<&'a SkillRecord> {
    let skill_hash = skill.content_hash.as_deref();
    let canonical_skill_path = std::fs::canonicalize(&skill.path).ok();

    if let Some(managed) = all_managed.iter().find(|managed| {
        source_ref_matches_skill_path(&skill.path, canonical_skill_path.as_deref(), managed)
    }) {
        return Some(managed);
    }

    if let Some(hash) = skill_hash {
        let mut by_hash = all_managed
            .iter()
            .filter(|managed| managed.content_hash.as_deref() == Some(hash));
        if let Some(first) = by_hash.next() {
            if by_hash.next().is_none() {
                return Some(first);
            }
        }
    }

    let by_central_dir: Vec<&SkillRecord> = all_managed
        .iter()
        .filter(|managed| {
            Path::new(&managed.central_path)
                .file_name()
                .map(|name| name.to_string_lossy().eq_ignore_ascii_case(&skill.dir_name))
                .unwrap_or(false)
        })
        .collect();
    if let Some(managed) = unique_center_match(&by_central_dir, skill_hash) {
        return Some(managed);
    }

    let by_name: Vec<&SkillRecord> = all_managed
        .iter()
        .filter(|managed| slugify_skill_dir_name(&managed.name).eq_ignore_ascii_case(&skill.dir_name))
        .collect();
    unique_center_match(&by_name, skill_hash)
}

fn unique_center_match<'a>(
    candidates: &[&'a SkillRecord],
    skill_hash: Option<&str>,
) -> Option<&'a SkillRecord> {
    match candidates.len() {
        0 => None,
        1 => Some(candidates[0]),
        _ => {
            let hash = skill_hash?;
            let mut filtered = candidates
                .iter()
                .copied()
                .filter(|managed| managed.content_hash.as_deref() == Some(hash));
            let first = filtered.next()?;
            filtered.next().is_none().then_some(first)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::skill_store::{ProjectRecord, SkillRecord, SkillStore};
    use crate::core::tool_adapters::{CustomToolDef, ToolCategory};
    use crate::core::tool_service;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn managed_skill(id: &str, central_path: &Path, source_ref: Option<String>) -> SkillRecord {
        SkillRecord {
            id: id.to_string(),
            name: "demo".to_string(),
            description: Some("A demo skill".to_string()),
            source_type: "local".to_string(),
            source_ref,
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: central_path.to_string_lossy().into_owned(),
            content_hash: None,
            enabled: true,
            created_at: 1,
            updated_at: 1,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: None,
            last_check_error: None,
        }
    }

    fn setup_custom_project_skill(
        tmp: &tempfile::TempDir,
        store: &SkillStore,
    ) -> (ProjectRecord, PathBuf, PathBuf) {
        let global_skills = tmp.path().join("global-skills");
        fs::create_dir_all(&global_skills).unwrap();
        tool_service::set_custom_tools(
            store,
            &[CustomToolDef {
                key: "test_agent".to_string(),
                display_name: "Test Agent".to_string(),
                skills_dir: global_skills.to_string_lossy().into_owned(),
                project_relative_skills_dir: Some(".test-agent/skills".to_string()),
                category: ToolCategory::Coding,
            }],
        )
        .unwrap();
        store.set_setting("sync_mode", "copy").unwrap();

        let project_root = tmp.path().join("repo");
        fs::create_dir(&project_root).unwrap();
        let project = crate::core::project_service::add_project(store, &project_root).unwrap();
        let central_path = tmp.path().join("central/demo");
        fs::create_dir_all(&central_path).unwrap();
        fs::write(
            central_path.join("SKILL.md"),
            "---\nname: demo\ndescription: A demo skill\n---\n",
        )
        .unwrap();
        store
            .insert_skill(&managed_skill("skill-demo", &central_path, None))
            .unwrap();
        let target_root = project_root.join(".test-agent/skills");
        (project, central_path, target_root)
    }

    #[test]
    fn project_agent_targets_resolve_standard_roots_and_disabled_state() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let project_root = tmp.path().join("repo");
        fs::create_dir(&project_root).unwrap();
        let project = crate::core::project_service::add_project(&store, &project_root).unwrap();
        store
            .set_setting("disabled_tools", r#"["claude_code"]"#)
            .unwrap();

        let targets = list_project_agent_targets(&store, &project.id).unwrap();
        let claude = targets
            .iter()
            .find(|target| target.key == "claude_code")
            .unwrap();
        let expected_disabled = Path::new(&project.path).join(".claude/skills-disabled");

        assert_eq!(claude.skills_root, Path::new(&project.path).join(".claude/skills"));
        assert_eq!(claude.disabled_root.as_deref(), Some(expected_disabled.as_path()));
        assert!(!claude.enabled);
    }

    #[test]
    fn linked_workspace_exposes_only_its_registered_agent_target() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let linked_root = tmp.path().join("linked-skills");
        fs::create_dir(&linked_root).unwrap();
        let project = ProjectRecord {
            id: "linked-id".to_string(),
            name: "Linked Skills".to_string(),
            path: linked_root.to_string_lossy().into_owned(),
            workspace_type: "linked".to_string(),
            linked_agent_key: Some("linked-agent".to_string()),
            linked_agent_name: Some("Linked Agent".to_string()),
            disabled_path: None,
            sort_order: 0,
            created_at: 1,
            updated_at: 1,
        };
        store.insert_project(&project).unwrap();

        let targets = list_project_agent_targets(&store, &project.id).unwrap();

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].key, "linked-agent");
        assert_eq!(targets[0].skills_root, linked_root);
        assert!(targets[0].enabled);
        assert!(targets[0].installed);
    }

    #[test]
    fn scanning_project_skills_attaches_matching_central_skill_id() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let project_root = tmp.path().join("repo");
        fs::create_dir_all(project_root.join(".claude/skills/demo")).unwrap();
        fs::write(
            project_root.join(".claude/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: A demo skill\n---\n",
        )
        .unwrap();
        let project = crate::core::project_service::add_project(&store, &project_root).unwrap();
        let central_path = tmp.path().join("central/demo");
        fs::create_dir_all(&central_path).unwrap();
        fs::write(central_path.join("SKILL.md"), "# demo\n").unwrap();
        let project_skill_path = project_root.join(".claude/skills/demo");
        store
            .insert_skill(&managed_skill(
                "skill-demo",
                &central_path,
                Some(project_skill_path.to_string_lossy().into_owned()),
            ))
            .unwrap();

        let variants = scan_project_skill_variants(&store, &project.id).unwrap();

        let variant = variants
            .iter()
            .find(|variant| variant.agent == "claude_code")
            .unwrap();
        assert_eq!(variant.skill_id.as_deref(), Some("skill-demo"));
        assert_eq!(variant.relative_path, "demo");
        assert!(variant.enabled);
    }

    #[test]
    fn adding_skill_copies_without_overwriting_and_reports_existing_target() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let (project, central_path, target_root) = setup_custom_project_skill(&tmp, &store);

        let first = add_skill_to_project(&store, &project.id, "skill-demo", "test_agent").unwrap();

        assert!(matches!(first, AddProjectSkillOutcome::Added(_)));
        assert_eq!(
            fs::read_to_string(target_root.join("demo/SKILL.md")).unwrap(),
            fs::read_to_string(central_path.join("SKILL.md")).unwrap()
        );
        fs::write(target_root.join("demo/SKILL.md"), "local edit").unwrap();

        let second = add_skill_to_project(&store, &project.id, "skill-demo", "test_agent").unwrap();

        assert!(matches!(second, AddProjectSkillOutcome::AlreadyPresent(_)));
        assert_eq!(
            fs::read_to_string(target_root.join("demo/SKILL.md")).unwrap(),
            "local edit"
        );
    }

    #[test]
    fn adding_skill_rejects_disabled_agent_before_creating_files() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let (project, _, target_root) = setup_custom_project_skill(&tmp, &store);
        store
            .set_setting("disabled_tools", r#"["test_agent"]"#)
            .unwrap();

        let error = add_skill_to_project(&store, &project.id, "skill-demo", "test_agent")
            .unwrap_err();

        assert!(error.to_string().contains("disabled"));
        assert!(!target_root.join("demo").exists());
    }

    #[test]
    fn removing_skill_works_for_known_disabled_agent_and_preserves_library_copy() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let (project, central_path, target_root) = setup_custom_project_skill(&tmp, &store);
        add_skill_to_project(&store, &project.id, "skill-demo", "test_agent").unwrap();
        store
            .set_setting("disabled_tools", r#"["test_agent"]"#)
            .unwrap();

        remove_skill_from_project(&store, &project.id, "demo", "test_agent").unwrap();

        assert!(!target_root.join("demo").exists());
        assert!(central_path.join("SKILL.md").is_file());
    }

    #[test]
    fn removing_skill_from_disabled_root_removes_only_the_project_copy() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let (project, central_path, target_root) = setup_custom_project_skill(&tmp, &store);
        let disabled_root = target_root.with_file_name("skills-disabled");
        fs::create_dir_all(disabled_root.join("demo")).unwrap();
        fs::write(disabled_root.join("demo/SKILL.md"), "disabled project copy").unwrap();

        remove_skill_from_project(&store, &project.id, "demo", "test_agent").unwrap();

        assert!(!disabled_root.join("demo").exists());
        assert!(central_path.join("SKILL.md").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn removing_project_symlink_does_not_delete_its_external_target() {
        use std::os::unix::fs::symlink;

        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let (project, _, target_root) = setup_custom_project_skill(&tmp, &store);
        let external_skill = tmp.path().join("external/demo");
        fs::create_dir_all(&external_skill).unwrap();
        fs::write(external_skill.join("SKILL.md"), "external source").unwrap();
        fs::create_dir_all(&target_root).unwrap();
        symlink(&external_skill, target_root.join("demo")).unwrap();

        remove_skill_from_project(&store, &project.id, "demo", "test_agent").unwrap();

        assert!(!target_root.join("demo").exists());
        assert!(external_skill.join("SKILL.md").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn removing_skill_through_symlinked_parent_refuses_external_target() {
        use std::os::unix::fs::symlink;

        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let (project, _, target_root) = setup_custom_project_skill(&tmp, &store);
        let external_root = tmp.path().join("external-skills");
        let external_skill = external_root.join("demo");
        fs::create_dir_all(&external_skill).unwrap();
        fs::write(external_skill.join("SKILL.md"), "external skill").unwrap();
        fs::create_dir_all(&target_root).unwrap();
        symlink(&external_root, target_root.join("outside")).unwrap();

        let result = remove_skill_from_project(&store, &project.id, "outside/demo", "test_agent");

        assert!(result.is_err(), "symlink escape should be rejected");
        assert!(external_skill.join("SKILL.md").is_file());
    }

    #[test]
    fn remove_preview_returns_exact_path_without_mutating_it() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let (project, _, target_root) = setup_custom_project_skill(&tmp, &store);
        add_skill_to_project(&store, &project.id, "skill-demo", "test_agent").unwrap();

        let target = preview_remove_skill_from_project(&store, &project.id, "demo", "test_agent")
            .unwrap();

        assert_eq!(target.relative_path, "demo");
        assert_eq!(
            target.absolute_path,
            fs::canonicalize(target_root.join("demo")).unwrap()
        );
        assert!(target.enabled);
        assert!(target.absolute_path.is_dir());
    }

    #[test]
    fn removing_skill_rejects_parent_traversal() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let (project, _, _) = setup_custom_project_skill(&tmp, &store);

        let error = remove_skill_from_project(&store, &project.id, "../outside", "test_agent")
            .unwrap_err();

        assert!(error.to_string().contains("Invalid skill directory path"));
    }
}
