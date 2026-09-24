use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context};
use app_lib::commands::{presets as preset_cmd, skills as cmd, tools as tool_cmd};
use app_lib::core::{
    app_state,
    audit_log::AuditDraft,
    central_repo,
    error::{AppError, ErrorKind},
    git_backup, git_fetcher, installer, merge,
    repo_lock::RepoLock,
    scenario_service, skill_metadata,
    skill_store::SkillStore,
    skillssh_api, sync_engine, sync_metadata, tool_adapters, tool_service,
};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;

#[derive(Parser, Debug)]
#[command(name = "skills-manager-cli")]
#[command(about = "Shared-core CLI for skills-manager", version)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    skills_root: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Repo(RepoArgs),
    #[command(name = "agents", visible_alias = "tools")]
    Tools(ToolsArgs),
    Projects(ProjectsArgs),
    Skills(SkillsArgs),
    #[command(alias = "scenarios")]
    Presets(PresetArgs),
    Git(GitArgs),
}

#[derive(Args, Debug)]
struct ProjectsArgs {
    #[command(subcommand)]
    command: ProjectsCommand,
}

#[derive(Subcommand, Debug)]
enum ProjectsCommand {
    /// Link an existing project directory to the desktop app.
    Add { path: PathBuf },
    /// List linked projects in the app database.
    List,
    /// Remove a project link only; project files are kept.
    Remove {
        /// Project ID, exact name, or path.
        project_ref: String,
    },
    /// Copy one or more central skills into the project for selected agents.
    AddSkill {
        project_ref: String,
        skill_ref: String,
        #[arg(long = "agent", value_name = "AGENT", required = true)]
        agents: Vec<String>,
    },
    /// Copy every skill in a preset into the project for selected agents.
    AddPreset {
        project_ref: String,
        preset_ref: String,
        #[arg(long = "agent", value_name = "AGENT", required = true)]
        agents: Vec<String>,
    },
    /// Remove one project-local skill from selected agents.
    RemoveSkill(RemoveProjectSkillArgs),
    /// Remove project-local copies of every skill in a preset.
    RemovePreset(RemoveProjectPresetArgs),
}

#[derive(Args, Debug)]
#[group(required = true, multiple = false)]
struct ProjectRemoveSafetyArgs {
    /// Preview exact project paths without deleting files.
    #[arg(long)]
    dry_run: bool,
    /// Confirm deleting the selected project copies.
    #[arg(long)]
    yes: bool,
}

#[derive(Args, Debug)]
struct RemoveProjectSkillArgs {
    project_ref: String,
    /// Path relative to the selected agent's skills root.
    skill_relative_path: String,
    #[arg(long = "agent", value_name = "AGENT", required = true)]
    agents: Vec<String>,
    #[command(flatten)]
    safety: ProjectRemoveSafetyArgs,
}

#[derive(Args, Debug)]
struct RemoveProjectPresetArgs {
    project_ref: String,
    preset_ref: String,
    #[arg(long = "agent", value_name = "AGENT", required = true)]
    agents: Vec<String>,
    #[command(flatten)]
    safety: ProjectRemoveSafetyArgs,
}

#[derive(Args, Debug)]
struct RepoArgs {
    #[command(subcommand)]
    command: RepoCommand,
}

#[derive(Subcommand, Debug)]
enum RepoCommand {
    Status,
    SetPath { path: String },
    ResetPath,
}

#[derive(Args, Debug)]
struct ToolsArgs {
    #[command(subcommand)]
    command: ToolsCommand,
}

#[derive(Subcommand, Debug)]
enum ToolsCommand {
    List,
    Enable {
        #[arg(required = true)]
        agents: Vec<String>,
    },
    Disable {
        #[arg(required = true)]
        agents: Vec<String>,
    },
}

#[derive(Args, Debug)]
struct SkillsArgs {
    #[command(subcommand)]
    command: SkillsCommand,
}

#[derive(Subcommand, Debug)]
enum SkillsCommand {
    List {
        #[arg(long)]
        query: Option<String>,
        #[arg(long = "tag", conflicts_with = "untagged")]
        tags: Vec<String>,
        #[arg(long)]
        preset: Option<String>,
        #[arg(long, value_name = "AGENT")]
        deployed_to: Option<String>,
        #[arg(long)]
        untagged: bool,
        #[arg(long)]
        no_preset: bool,
        #[arg(long)]
        source: Option<String>,
    },
    Show {
        reference: String,
    },
    Export {
        reference: String,
        #[arg(long)]
        dest: PathBuf,
        /// Overwrite the destination if it already exists. Without this, an
        /// existing destination is left untouched and the command fails.
        #[arg(long)]
        force: bool,
    },
    Install {
        /// Ref: local path, git URL, or owner/repo[@skill] / owner/repo/skill
        reference: String,
        #[arg(long, conflicts_with_all = ["git", "skillssh"])]
        local: bool,
        #[arg(long, conflicts_with_all = ["local", "skillssh"])]
        git: bool,
        #[arg(long, conflicts_with_all = ["local", "git"])]
        skillssh: bool,
        #[arg(long)]
        name: Option<String>,
        /// Add to current active preset and sync agents
        #[arg(long, conflicts_with = "sync_preset")]
        sync: bool,
        /// Add to given preset (by id or name) and sync agents
        #[arg(long, alias = "sync-scenario", value_name = "REF")]
        sync_preset: Option<String>,
    },
    Update {
        /// Skill ref (id / name / dir basename / central path). Omit for --all.
        reference: Option<String>,
        #[arg(long)]
        all: bool,
    },
    Check {
        reference: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        force: bool,
    },
    Remove {
        references: Vec<String>,
        #[arg(long, short)]
        yes: bool,
        #[arg(long)]
        dry_run: bool,
    },
    /// Deprecated compatibility command: use skills deploy.
    Enable {
        references: Vec<String>,
    },
    /// Deprecated compatibility command: use skills undeploy.
    Disable {
        references: Vec<String>,
    },
    /// Deploy library skills to one or more agents' global skill directories.
    Deploy {
        #[arg(required = true)]
        references: Vec<String>,
        #[arg(long = "agent", alias = "to", value_name = "AGENT", required = true)]
        agents: Vec<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove managed deployments from one or more agents.
    Undeploy {
        #[arg(required = true)]
        references: Vec<String>,
        #[arg(long = "agent", alias = "from", value_name = "AGENT", required = true)]
        agents: Vec<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Show preset membership and actual per-agent deployment state.
    Status {
        reference: String,
    },
    Sync {
        /// Preset id or name (default = current active preset)
        #[arg(long, alias = "scenario")]
        preset: Option<String>,
        /// Tool key (default = all enabled tools)
        #[arg(long)]
        tool: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    Search {
        query: String,
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Re-point an installed skill at a git source in place, keeping its id,
    /// tags, preset membership and deployments.
    SetSource {
        /// Skill ref (id / name / dir basename / central path)
        reference: String,
        /// Git URL or owner/repo, optionally a GitHub tree URL encoding branch and subpath
        #[arg(long = "git-url")]
        git_url: String,
        /// Subpath inside the repo. Pass "" if the skill is at the repo root.
        /// Overrides a subpath encoded in the URL.
        #[arg(long)]
        subpath: Option<String>,
        /// Branch to track. Overrides a branch encoded in the URL.
        #[arg(long)]
        branch: Option<String>,
        /// Overwrite the central copy when the new source's content differs.
        /// Without this, a content difference is refused.
        #[arg(long)]
        force: bool,
        /// Resolve and compare without writing anything.
        #[arg(long)]
        dry_run: bool,
    },
    Adopt {
        /// Agent skill dirs to scan (e.g. ~/.claude/skills), or a single skill dir
        paths: Vec<PathBuf>,
        /// If set, adopt as git source (only with single adoptable skill)
        #[arg(long)]
        git_url: Option<String>,
        /// Subpath inside the git repo where the adopted skill lives. Required
        /// with --git-url when the URL itself does not encode a subpath. Pass
        /// "" if the skill is at the repo root.
        #[arg(long)]
        git_subpath: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    Tag(TagArgs),
}

#[derive(Args, Debug)]
struct TagArgs {
    #[command(subcommand)]
    command: TagCommand,
}

#[derive(Subcommand, Debug)]
enum TagCommand {
    Add {
        reference: String,
        tags: Vec<String>,
    },
    Remove {
        reference: String,
        tags: Vec<String>,
    },
    Set {
        reference: String,
        tags: Vec<String>,
    },
    Rename {
        old_name: String,
        new_name: String,
    },
    Delete {
        name: String,
        #[arg(long, short)]
        yes: bool,
        #[arg(long)]
        dry_run: bool,
    },
    List {
        reference: Option<String>,
    },
}

#[derive(Args, Debug)]
struct PresetArgs {
    #[command(subcommand)]
    command: PresetCommand,
}

#[derive(Subcommand, Debug)]
enum PresetCommand {
    List,
    Current,
    Show {
        reference: String,
    },
    Create {
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        icon: Option<String>,
    },
    Update {
        reference: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        icon: Option<String>,
    },
    Delete {
        reference: String,
        #[arg(long, short)]
        yes: bool,
        #[arg(long)]
        dry_run: bool,
    },
    Preview {
        reference: String,
    },
    /// Legacy exclusive switch: replaces the current active preset.
    Apply {
        reference: String,
    },
    /// Legacy exclusive close operation. Prefer undeploy for additive presets.
    Deactivate {
        reference: String,
    },
    /// Additively deploy this preset without removing other deployed presets.
    #[command(alias = "activate", alias = "enable", alias = "start", alias = "open")]
    Deploy {
        reference: String,
        #[arg(long = "agent", value_name = "AGENT")]
        agents: Vec<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove this preset's deployed pairs without changing its membership.
    #[command(alias = "disable", alias = "stop", alias = "close", alias = "off")]
    Undeploy {
        reference: String,
        #[arg(long = "agent", value_name = "AGENT")]
        agents: Vec<String>,
        #[arg(long)]
        dry_run: bool,
    },
    Status {
        reference: String,
        #[arg(long = "agent", value_name = "AGENT")]
        agents: Vec<String>,
    },
    AddSkill {
        preset: String,
        #[arg(required = true)]
        skills: Vec<String>,
    },
    RemoveSkill {
        preset: String,
        #[arg(required = true)]
        skills: Vec<String>,
    },
}

#[derive(Args, Debug)]
struct GitArgs {
    #[command(subcommand)]
    command: GitCommand,
}

#[derive(Subcommand, Debug)]
enum GitCommand {
    Status,
    Init,
    Clone {
        url: String,
    },
    SetRemote {
        url: String,
    },
    Pull,
    Push,
    Commit {
        #[arg(short, long)]
        message: String,
    },
    Versions {
        #[arg(long)]
        limit: Option<usize>,
    },
    Restore {
        tag: String,
    },
    /// Remove refs/skills-manager/* that a `git push --mirror`/--all style
    /// operation uploaded to the backup remote. Local sync refs are kept.
    PruneSyncRefs,
}

#[derive(Debug, Serialize)]
struct RepoStatus {
    base_dir: String,
    skills_dir: String,
    db_path: String,
    metadata_dir: String,
    skill_count: usize,
    preset_count: usize,
    active_preset_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct SkillSummary {
    id: String,
    name: String,
    description: Option<String>,
    path: String,
    enabled: bool,
    tags: Vec<String>,
    source_type: String,
    source_ref: Option<String>,
    preset_ids: Vec<String>,
    presets: Vec<String>,
    deployed_to: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AgentMutationReport {
    agent: String,
    enabled: bool,
    changed: bool,
}

#[derive(Debug, Serialize)]
struct SkillAgentStatus {
    key: String,
    display_name: String,
    installed: bool,
    globally_enabled: bool,
    deployed: bool,
    target_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct SkillStatusReport {
    #[serde(flatten)]
    skill: SkillSummary,
    agents: Vec<SkillAgentStatus>,
}

#[derive(Debug, Serialize)]
struct SkillDeploymentReport {
    ok: bool,
    action: String,
    agents: Vec<String>,
    dry_run: bool,
    skill_count: usize,
    pair_count: usize,
    changed_pairs: usize,
    skills: Vec<String>,
    /// Paths left in place because they no longer match the deployment we
    /// recorded — someone else's content now lives there (#363).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    preserved: Vec<String>,
}

struct DeploymentVerification {
    succeeded: std::collections::HashSet<(String, String)>,
    failures: Vec<String>,
    preserved: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SkillDetail {
    #[serde(flatten)]
    summary: SkillSummary,
    skill_file: String,
    files: Vec<String>,
    markdown: String,
}

#[derive(Debug, Serialize)]
struct PresetInfo {
    id: String,
    name: String,
    description: Option<String>,
    icon: Option<String>,
    sort_order: i32,
    skill_count: usize,
    active: bool,
}

#[derive(Debug, Serialize)]
struct PresetAgentStatus {
    key: String,
    display_name: String,
    deployed: usize,
    total: usize,
    status: String,
}

#[derive(Debug, Serialize)]
struct PresetStatusReport {
    preset: PresetInfo,
    agents: Vec<PresetAgentStatus>,
}

#[derive(Debug, Serialize)]
struct PresetDeploymentReport {
    ok: bool,
    action: String,
    preset_id: String,
    preset_name: String,
    agents: Vec<String>,
    dry_run: bool,
    skill_count: usize,
    pair_count: usize,
    changed_pairs: usize,
    /// See [`SkillDeploymentReport::preserved`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    preserved: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PresetDeleteReport {
    ok: bool,
    preset_id: String,
    preset_name: String,
    dry_run: bool,
    deleted: bool,
}

#[derive(Debug, Serialize)]
struct InstallReport {
    ok: bool,
    skill_id: String,
    name: String,
    central_path: String,
    source_type: String,
    synced: bool,
    preset_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct UpdateReport {
    skill_id: String,
    name: String,
    source_type: String,
    refreshed: bool,
    error: Option<String>,
    /// Present when the update was held back because it would have removed
    /// these paths (#256). Nothing changed, and the CLI offers no way to
    /// accept: approving means seeing the list, which needs a person, so it
    /// only exists in the app. A bare `refreshed: false` would read as
    /// "already up to date".
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    held_back_removals: Vec<String>,
}

#[derive(Debug, Serialize)]
struct CheckReport {
    skill_id: String,
    name: String,
    source_type: String,
    update_status: String,
    last_check_error: Option<String>,
    skipped: bool,
}

#[derive(Debug, Serialize)]
struct RemoveReport {
    ok: bool,
    deleted: usize,
    failed: Vec<String>,
    dry_run: bool,
}

#[derive(Debug, Serialize)]
struct DeprecatedEnableReport {
    skill_id: String,
    name: String,
    enabled: bool,
    changed: bool,
    deprecated: bool,
    message: String,
}

#[derive(Debug, Serialize)]
struct SyncReport {
    ok: bool,
    preset_id: String,
    preset_name: String,
    tool: Option<String>,
    dry_run: bool,
    targets: Vec<scenario_service::SyncPreviewTarget>,
}

#[derive(Debug, Serialize)]
struct PresetDeactivateReport {
    ok: bool,
    preset_id: String,
    preset_name: String,
    removed_target_count: usize,
    active_preset_id: Option<String>,
    active_preset_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct SearchHit {
    install_ref: String,
    name: String,
    source: String,
    skill_id: String,
    installs: u64,
    skills_sh_url: String,
}

#[derive(Debug, Serialize)]
struct AdoptCandidate {
    path: String,
    name: String,
    reason: String,
}

#[derive(Debug, Serialize)]
struct AdoptReport {
    ok: bool,
    dry_run: bool,
    adopted: Vec<InstallReport>,
    candidates: Vec<AdoptCandidate>,
    skipped: Vec<AdoptCandidate>,
}

#[derive(Debug, Serialize)]
struct TagReport {
    skill_id: String,
    name: String,
    tags: Vec<String>,
}

#[derive(Debug, Serialize)]
struct GlobalTagReport {
    ok: bool,
    tag: String,
    renamed_to: Option<String>,
    affected_skills: usize,
    dry_run: bool,
    deleted: bool,
}

#[derive(Debug, Serialize)]
struct PresetMembershipReport {
    preset_id: String,
    preset_name: String,
    added: Vec<String>,
    removed: Vec<String>,
    missing: Vec<String>,
}

enum InstallKind {
    Local,
    Git,
    Skillssh,
}

enum SyncTarget {
    None,
    Active,
    Specific(String),
}

fn main() {
    let json = std::env::args()
        .skip(1)
        .take_while(|a| a != "--")
        .any(|a| a == "--json" || a.starts_with("--json="));

    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            if !e.use_stderr() {
                e.exit();
            }
            if json {
                let message = e.to_string();
                let envelope = serde_json::json!({
                    "ok": false,
                    "code": "INVALID_ARGUMENT",
                    "message": message,
                    "error": message,
                });
                eprintln!("{}", serde_json::to_string(&envelope).unwrap());
                std::process::exit(2);
            }
            e.exit();
        }
    };

    if let Err(err) = run(cli) {
        if json {
            eprintln!("{}", serde_json::to_string(&error_envelope(&err)).unwrap());
        } else {
            eprintln!("error: {err:#}");
        }
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    let is_projects_command = matches!(&cli.command, Commands::Projects(_));
    if let Some(skills_root) = cli.skills_root.as_ref().filter(|_| !is_projects_command) {
        let base = central_repo::external_base_dir(skills_root);
        central_repo::set_runtime_base_dir_override(Some(base));
        central_repo::set_runtime_skills_dir_override(Some(skills_root.clone()));
    }

    let store = app_state::initialize_cli_store()?;

    match cli.command {
        Commands::Repo(args) => run_repo(args, &store, cli.json),
        Commands::Tools(args) => run_tools(args, &store, cli.json),
        Commands::Projects(args) => run_projects(args, &store, cli.json),
        Commands::Skills(args) => run_skills(args, &store, cli.json),
        Commands::Presets(args) => run_presets(args, &store, cli.json),
        Commands::Git(args) => run_git(args, &store, cli.skills_root.is_some(), cli.json),
    }
}

// ── repo ──────────────────────────────────────────────────────────────────

fn run_repo(args: RepoArgs, store: &SkillStore, json: bool) -> anyhow::Result<()> {
    match args.command {
        RepoCommand::Status => print_json(&repo_status(store), json),
        RepoCommand::SetPath { path } => {
            central_repo::set_base_dir_override(Some(path))?;
            let store = app_state::initialize_cli_store()?;
            print_json(&repo_status(&store), json);
        }
        RepoCommand::ResetPath => {
            central_repo::set_base_dir_override(None)?;
            let store = app_state::initialize_cli_store()?;
            print_json(&repo_status(&store), json);
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct ProjectListEntry {
    id: String,
    name: String,
    path: String,
    workspace_type: String,
    skill_count: usize,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
struct ProjectActionItem {
    project_id: String,
    project_name: String,
    action: String,
    skill_id: Option<String>,
    skill_name: String,
    agent: String,
    relative_path: String,
    path: String,
    outcome: String,
}

#[derive(Debug, Clone, Serialize)]
struct ProjectActionFailure {
    project_id: String,
    project_name: String,
    action: String,
    skill_id: Option<String>,
    skill_name: String,
    agent: String,
    relative_path: String,
    path: Option<String>,
    message: String,
    outcome: String,
}

#[derive(Debug, Clone, Serialize)]
struct ProjectBatchReport {
    ok: bool,
    project_id: String,
    project_name: String,
    action: String,
    added: Vec<ProjectActionItem>,
    removed: Vec<ProjectActionItem>,
    skipped: Vec<ProjectActionItem>,
    failed: Vec<ProjectActionFailure>,
    would_remove: Vec<ProjectActionItem>,
}

#[derive(Debug)]
struct ProjectBatchFailure {
    report: ProjectBatchReport,
}

impl std::fmt::Display for ProjectBatchFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} project target(s) failed during {}",
            self.report.failed.len(),
            self.report.action
        )
    }
}

impl std::error::Error for ProjectBatchFailure {}

impl ProjectBatchReport {
    fn new(project: &app_lib::core::skill_store::ProjectRecord, action: &str) -> Self {
        Self {
            ok: true,
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            action: action.to_string(),
            added: Vec::new(),
            removed: Vec::new(),
            skipped: Vec::new(),
            failed: Vec::new(),
            would_remove: Vec::new(),
        }
    }
}

fn project_action_item(
    report: &ProjectBatchReport,
    variant: app_lib::core::project_skill_service::ProjectSkillVariant,
    outcome: &str,
) -> ProjectActionItem {
    ProjectActionItem {
        project_id: report.project_id.clone(),
        project_name: report.project_name.clone(),
        action: report.action.clone(),
        skill_id: variant.skill_id,
        skill_name: variant.skill_name,
        agent: variant.agent,
        relative_path: variant.relative_path,
        path: variant.absolute_path.to_string_lossy().into_owned(),
        outcome: outcome.to_string(),
    }
}

fn project_action_failure(
    report: &ProjectBatchReport,
    skill_id: Option<String>,
    skill_name: String,
    agent: String,
    relative_path: String,
    path: Option<String>,
    message: String,
) -> ProjectActionFailure {
    ProjectActionFailure {
        project_id: report.project_id.clone(),
        project_name: report.project_name.clone(),
        action: report.action.clone(),
        skill_id,
        skill_name,
        agent,
        relative_path,
        path,
        message,
        outcome: "failed".to_string(),
    }
}

fn project_action_not_found(
    report: &ProjectBatchReport,
    skill_relative_path: String,
    agent: String,
) -> ProjectActionItem {
    ProjectActionItem {
        project_id: report.project_id.clone(),
        project_name: report.project_name.clone(),
        action: report.action.clone(),
        skill_id: None,
        skill_name: Path::new(&skill_relative_path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| skill_relative_path.clone()),
        agent,
        relative_path: skill_relative_path,
        path: String::new(),
        outcome: "not_found".to_string(),
    }
}

struct ProjectAgentValidationFailure {
    agent: String,
    message: String,
    path: Option<String>,
}

struct ProjectAgentValidation {
    valid_agents: Vec<String>,
    failures: Vec<ProjectAgentValidationFailure>,
}

fn validate_project_agents(
    store: &SkillStore,
    project: &app_lib::core::skill_store::ProjectRecord,
    agents: &[String],
    require_available: bool,
    relative_path: &str,
) -> anyhow::Result<ProjectAgentValidation> {
    let targets =
        app_lib::core::project_skill_service::list_project_agent_targets(store, &project.id)
            .map_err(map_app_err)?;
    let mut valid_agents = Vec::new();
    let mut failures = Vec::new();
    for agent_key in agents {
        if valid_agents.contains(agent_key)
            || failures
                .iter()
                .any(|failure: &ProjectAgentValidationFailure| failure.agent == *agent_key)
        {
            continue;
        }
        let Some(target) = targets.iter().find(|target| target.key == *agent_key) else {
            failures.push(ProjectAgentValidationFailure {
                agent: agent_key.clone(),
                message: format!("unknown project agent: {agent_key}"),
                path: None,
            });
            continue;
        };
        let path = Some(
            target
                .skills_root
                .join(relative_path)
                .to_string_lossy()
                .into_owned(),
        );
        if require_available && !target.enabled {
            failures.push(ProjectAgentValidationFailure {
                agent: agent_key.clone(),
                message: format!("project agent is disabled: {agent_key}"),
                path,
            });
        } else if require_available && !target.installed {
            failures.push(ProjectAgentValidationFailure {
                agent: agent_key.clone(),
                message: format!("project agent is not installed: {agent_key}"),
                path,
            });
        } else {
            valid_agents.push(agent_key.clone());
        }
    }
    if valid_agents.is_empty() && failures.is_empty() {
        bail!("at least one --agent must be provided");
    }
    Ok(ProjectAgentValidation {
        valid_agents,
        failures,
    })
}

fn add_validation_failures(
    report: &mut ProjectBatchReport,
    failures: Vec<ProjectAgentValidationFailure>,
    skill_id: Option<String>,
    skill_name: String,
    relative_path: String,
) {
    for failure in failures {
        report.failed.push(project_action_failure(
            report,
            skill_id.clone(),
            skill_name.clone(),
            failure.agent,
            relative_path.clone(),
            failure.path,
            failure.message,
        ));
    }
}

fn finish_project_batch(mut report: ProjectBatchReport, json: bool) -> anyhow::Result<()> {
    report.ok = report.failed.is_empty();
    if !report.ok {
        if !json {
            print_project_batch(&report);
        }
        return Err(anyhow::Error::new(ProjectBatchFailure { report }));
    }
    if json {
        print_json(&report, true);
    } else {
        print_project_batch(&report);
    }
    Ok(())
}

fn print_project_batch(report: &ProjectBatchReport) {
    println!("{}", render_project_batch(report));
}

fn render_project_batch(report: &ProjectBatchReport) -> String {
    let mut lines = vec![format!(
        "{} project '{}' — added {}, removed {}, skipped {}, failed {}, preview {}",
        report.action,
        report.project_name,
        report.added.len(),
        report.removed.len(),
        report.skipped.len(),
        report.failed.len(),
        report.would_remove.len()
    )];
    for item in report
        .added
        .iter()
        .chain(report.removed.iter())
        .chain(report.skipped.iter())
        .chain(report.would_remove.iter())
    {
        lines.push(format!(
            "  {} {} [{}] {}",
            item.outcome, item.skill_name, item.agent, item.path
        ));
    }
    for failure in &report.failed {
        lines.push(format!(
            "  FAILED {} [{}] {}: {}",
            failure.skill_name, failure.agent, failure.relative_path, failure.message
        ));
    }
    lines.join("\n")
}

fn run_projects(args: ProjectsArgs, store: &SkillStore, json: bool) -> anyhow::Result<()> {
    match args.command {
        ProjectsCommand::Add { path } => {
            let record =
                app_lib::core::project_service::add_project(store, &path).map_err(map_app_err)?;
            if json {
                print_json(&record, true);
            } else {
                println!("Linked project '{}' at {}", record.name, record.path);
            }
        }
        ProjectsCommand::List => {
            let projects = list_project_entries(store).map_err(map_app_err)?;
            if json {
                print_json(&projects, true);
            } else if projects.is_empty() {
                println!("No projects linked.");
            } else {
                for project in projects {
                    println!(
                        "{}  {}  [{} skills]  id: {}",
                        project.name, project.path, project.skill_count, project.id
                    );
                }
            }
        }
        ProjectsCommand::Remove { project_ref } => {
            let projects = store
                .get_all_projects()
                .map_err(AppError::db)
                .map_err(map_app_err)?;
            let removed = resolve_project_reference(&projects, &project_ref)?;
            store
                .delete_project(&removed.id)
                .map_err(AppError::db)
                .map_err(map_app_err)?;
            if json {
                print_json(&removed, true);
            } else {
                println!(
                    "Unlinked project '{}' at {}. Project files were kept.",
                    removed.name, removed.path
                );
            }
        }
        ProjectsCommand::AddSkill {
            project_ref,
            skill_ref,
            agents,
        } => {
            let project = resolve_cli_project(store, &project_ref)?;
            let skill = resolve_skill(store, &skill_ref)?;
            let relative_path = app_lib::core::sync_engine::target_dir_name(
                Path::new(&skill.central_path),
                &skill.name,
            );
            let validation =
                validate_project_agents(store, &project, &agents, true, &relative_path)?;
            let mut report = ProjectBatchReport::new(&project, "add_skill");
            add_validation_failures(
                &mut report,
                validation.failures,
                Some(skill.id.clone()),
                skill.name.clone(),
                relative_path.clone(),
            );
            for agent in validation.valid_agents {
                match app_lib::core::project_skill_service::add_skill_to_project(
                    store,
                    &project.id,
                    &skill.id,
                    &agent,
                ) {
                    Ok(app_lib::core::project_skill_service::AddProjectSkillOutcome::Added(
                        variant,
                    )) => report.added.push(project_action_item(&report, variant, "added")),
                    Ok(
                        app_lib::core::project_skill_service::AddProjectSkillOutcome::AlreadyPresent(
                            variant,
                        ),
                    ) => report
                        .skipped
                        .push(project_action_item(&report, variant, "already_present")),
                    Err(error) => report.failed.push(project_action_failure(
                        &report,
                        Some(skill.id.clone()),
                        skill.name.clone(),
                        agent,
                        relative_path.clone(),
                        None,
                        error.to_string(),
                    )),
                }
            }
            finish_project_batch(report, json)?;
        }
        ProjectsCommand::AddPreset {
            project_ref,
            preset_ref,
            agents,
        } => {
            let project = resolve_cli_project(store, &project_ref)?;
            let preset = resolve_scenario(store, &preset_ref)?;
            let skills = store.get_skills_for_scenario(&preset.id)?;
            let validation = validate_project_agents(store, &project, &agents, true, "")?;
            let existing = app_lib::core::project_skill_service::scan_project_skill_variants(
                store,
                &project.id,
            )
            .map_err(map_app_err)?;
            let mut report = ProjectBatchReport::new(&project, "add_preset");

            for skill in skills {
                let relative_path = app_lib::core::sync_engine::target_dir_name(
                    Path::new(&skill.central_path),
                    &skill.name,
                );
                add_validation_failures(
                    &mut report,
                    validation
                        .failures
                        .iter()
                        .map(|failure| ProjectAgentValidationFailure {
                            agent: failure.agent.clone(),
                            message: failure.message.clone(),
                            path: failure.path.clone(),
                        })
                        .collect(),
                    Some(skill.id.clone()),
                    skill.name.clone(),
                    relative_path.clone(),
                );
                for agent in &validation.valid_agents {
                    if let Some(variant) = existing.iter().find(|variant| {
                        variant.skill_id.as_deref() == Some(skill.id.as_str())
                            && variant.agent == *agent
                    }) {
                        report.skipped.push(project_action_item(
                            &report,
                            variant.clone(),
                            "already_present",
                        ));
                        continue;
                    }
                    match app_lib::core::project_skill_service::add_skill_to_project(
                        store,
                        &project.id,
                        &skill.id,
                        agent,
                    ) {
                        Ok(app_lib::core::project_skill_service::AddProjectSkillOutcome::Added(
                            variant,
                        )) => report.added.push(project_action_item(&report, variant, "added")),
                        Ok(app_lib::core::project_skill_service::AddProjectSkillOutcome::AlreadyPresent(
                            variant,
                        )) => report.skipped.push(project_action_item(
                            &report,
                            variant,
                            "already_present",
                        )),
                        Err(error) => report.failed.push(project_action_failure(
                            &report,
                            Some(skill.id.clone()),
                            skill.name.clone(),
                            agent.clone(),
                            relative_path.clone(),
                            project_agent_target_path(store, &project, agent, &relative_path),
                            error.to_string(),
                        )),
                    }
                }
            }
            finish_project_batch(report, json)?;
        }
        ProjectsCommand::RemoveSkill(args) => {
            let project = resolve_cli_project(store, &args.project_ref)?;
            let validation = validate_project_agents(
                store,
                &project,
                &args.agents,
                false,
                &args.skill_relative_path,
            )?;
            let mut report = ProjectBatchReport::new(&project, "remove_skill");
            add_validation_failures(
                &mut report,
                validation.failures,
                None,
                args.skill_relative_path.clone(),
                args.skill_relative_path.clone(),
            );
            for agent in validation.valid_agents {
                match app_lib::core::project_skill_service::preview_remove_skill_from_project(
                    store,
                    &project.id,
                    &args.skill_relative_path,
                    &agent,
                ) {
                    Ok(variant) if args.safety.dry_run => {
                        report.would_remove.push(project_action_item(
                            &report,
                            variant,
                            "would_remove",
                        ));
                    }
                    Ok(variant) => {
                        match app_lib::core::project_skill_service::remove_skill_from_project(
                            store,
                            &project.id,
                            &args.skill_relative_path,
                            &agent,
                        ) {
                            Ok(()) => report
                                .removed
                                .push(project_action_item(&report, variant, "removed")),
                            Err(error) => report.failed.push(project_action_failure(
                                &report,
                                variant.skill_id,
                                variant.skill_name,
                                agent,
                                args.skill_relative_path.clone(),
                                Some(variant.absolute_path.to_string_lossy().into_owned()),
                                error.to_string(),
                            )),
                        }
                    }
                    Err(error) if matches!(error.kind, ErrorKind::NotFound) => report.skipped.push(
                        project_action_not_found(&report, args.skill_relative_path.clone(), agent),
                    ),
                    Err(error) => report.failed.push(project_action_failure(
                        &report,
                        None,
                        args.skill_relative_path.clone(),
                        agent,
                        args.skill_relative_path.clone(),
                        None,
                        error.to_string(),
                    )),
                }
            }
            finish_project_batch(report, json)?;
        }
        ProjectsCommand::RemovePreset(args) => {
            let project = resolve_cli_project(store, &args.project_ref)?;
            let preset = resolve_scenario(store, &args.preset_ref)?;
            let skills = store.get_skills_for_scenario(&preset.id)?;
            let validation = validate_project_agents(store, &project, &args.agents, false, "")?;
            let variants = app_lib::core::project_skill_service::scan_project_skill_variants(
                store,
                &project.id,
            )
            .map_err(map_app_err)?;
            let mut report = ProjectBatchReport::new(&project, "remove_preset");

            for skill in skills {
                let fallback_path = app_lib::core::sync_engine::target_dir_name(
                    Path::new(&skill.central_path),
                    &skill.name,
                );
                add_validation_failures(
                    &mut report,
                    validation
                        .failures
                        .iter()
                        .map(|failure| ProjectAgentValidationFailure {
                            agent: failure.agent.clone(),
                            message: failure.message.clone(),
                            path: failure.path.clone(),
                        })
                        .collect(),
                    Some(skill.id.clone()),
                    skill.name.clone(),
                    fallback_path.clone(),
                );
                for agent in &validation.valid_agents {
                    let Some(variant) = variants
                        .iter()
                        .find(|variant| {
                            variant.skill_id.as_deref() == Some(skill.id.as_str())
                                && variant.agent == *agent
                        })
                        .cloned()
                    else {
                        report.skipped.push(project_action_not_found(
                            &report,
                            fallback_path.clone(),
                            agent.clone(),
                        ));
                        continue;
                    };
                    if args.safety.dry_run {
                        report.would_remove.push(project_action_item(
                            &report,
                            variant,
                            "would_remove",
                        ));
                        continue;
                    }
                    match app_lib::core::project_skill_service::remove_skill_from_project(
                        store,
                        &project.id,
                        &variant.relative_path,
                        agent,
                    ) {
                        Ok(()) => report
                            .removed
                            .push(project_action_item(&report, variant, "removed")),
                        Err(error) => report.failed.push(project_action_failure(
                            &report,
                            variant.skill_id,
                            variant.skill_name,
                            agent.clone(),
                            variant.relative_path,
                            Some(variant.absolute_path.to_string_lossy().into_owned()),
                            error.to_string(),
                        )),
                    }
                }
            }
            finish_project_batch(report, json)?;
        }
    }
    Ok(())
}

fn project_agent_target_path(
    store: &SkillStore,
    project: &app_lib::core::skill_store::ProjectRecord,
    agent: &str,
    relative_path: &str,
) -> Option<String> {
    app_lib::core::project_skill_service::list_project_agent_targets(store, &project.id)
        .ok()?
        .into_iter()
        .find(|target| target.key == agent)
        .map(|target| {
            target
                .skills_root
                .join(relative_path)
                .to_string_lossy()
                .into_owned()
        })
}

fn resolve_cli_project(
    store: &SkillStore,
    reference: &str,
) -> anyhow::Result<app_lib::core::skill_store::ProjectRecord> {
    let projects = store
        .get_all_projects()
        .map_err(AppError::db)
        .map_err(map_app_err)?;
    resolve_project_reference(&projects, reference)
}

fn list_project_entries(store: &SkillStore) -> Result<Vec<ProjectListEntry>, AppError> {
    let records = store.get_all_projects().map_err(AppError::db)?;
    let configs = project_agent_configs(store);
    Ok(records
        .into_iter()
        .map(|record| {
            let skills = if record.workspace_type == "linked" {
                app_lib::core::project_scanner::read_linked_workspace_skills(
                    Path::new(&record.path),
                    record.disabled_path.as_deref().map(Path::new),
                    record.linked_agent_key.as_deref().unwrap_or(&record.name),
                    record.linked_agent_name.as_deref().unwrap_or(&record.name),
                    true,
                )
            } else {
                app_lib::core::project_scanner::read_project_skills(
                    Path::new(&record.path),
                    &configs,
                )
            };
            let skill_count = skills
                .iter()
                .map(|skill| skill.relative_path.to_lowercase())
                .collect::<HashSet<_>>()
                .len();
            ProjectListEntry {
                id: record.id,
                name: record.name,
                path: record.path,
                workspace_type: record.workspace_type,
                skill_count,
                created_at: record.created_at,
                updated_at: record.updated_at,
            }
        })
        .collect())
}

fn project_agent_configs(
    store: &SkillStore,
) -> Vec<app_lib::core::project_scanner::AgentSkillConfig> {
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
            Some(app_lib::core::project_scanner::AgentSkillConfig {
                key,
                display_name,
                relative_skills_dir,
            })
        })
        .collect()
}

fn resolve_project_reference(
    projects: &[app_lib::core::skill_store::ProjectRecord],
    reference: &str,
) -> anyhow::Result<app_lib::core::skill_store::ProjectRecord> {
    if let Some(project) = projects.iter().find(|project| project.id == reference) {
        return Ok(project.clone());
    }

    let canonical_reference = Path::new(reference).canonicalize().ok();
    let path_matches: Vec<_> = projects
        .iter()
        .filter(|project| {
            project.path == reference
                || canonical_reference.as_ref().is_some_and(|canonical| {
                    Path::new(&project.path)
                        .canonicalize()
                        .map(|path| &path == canonical)
                        .unwrap_or(false)
                })
        })
        .collect();
    match path_matches.as_slice() {
        [project] => return Ok((*project).clone()),
        [] => {}
        candidates => {
            let details = candidates
                .iter()
                .map(|project| {
                    format!(
                        "{} (id: {}, path: {})",
                        project.name, project.id, project.path
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            bail!("Project path '{reference}' is ambiguous; use an ID. Candidates: {details}");
        }
    }

    let matches: Vec<_> = projects
        .iter()
        .filter(|project| project.name == reference)
        .collect();
    match matches.as_slice() {
        [project] => Ok((*project).clone()),
        [] => bail!("No project matches '{reference}'"),
        candidates => {
            let details = candidates
                .iter()
                .map(|project| {
                    format!(
                        "{} (id: {}, path: {})",
                        project.name, project.id, project.path
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            bail!(
                "Project name '{reference}' is ambiguous; use an ID or path. Candidates: {details}"
            )
        }
    }
}

fn repo_status(store: &SkillStore) -> RepoStatus {
    RepoStatus {
        base_dir: central_repo::base_dir().to_string_lossy().to_string(),
        skills_dir: central_repo::skills_dir().to_string_lossy().to_string(),
        db_path: central_repo::db_path().to_string_lossy().to_string(),
        metadata_dir: sync_metadata::metadata_dir().to_string_lossy().to_string(),
        skill_count: store.get_all_skills().unwrap_or_default().len(),
        preset_count: store.get_all_scenarios().unwrap_or_default().len(),
        active_preset_id: store.get_active_scenario_id().unwrap_or(None),
    }
}

// ── tools ─────────────────────────────────────────────────────────────────

fn run_tools(args: ToolsArgs, store: &SkillStore, json: bool) -> anyhow::Result<()> {
    match args.command {
        ToolsCommand::List => print_json(&tool_service::list_tool_info(store), json),
        ToolsCommand::Enable { agents } => {
            print_json(&run_set_agents_enabled(store, &agents, true)?, json)
        }
        ToolsCommand::Disable { agents } => {
            print_json(&run_set_agents_enabled(store, &agents, false)?, json)
        }
    }
    Ok(())
}

fn run_set_agents_enabled(
    store: &SkillStore,
    agents: &[String],
    enabled: bool,
) -> anyhow::Result<Vec<AgentMutationReport>> {
    if agents.is_empty() {
        bail!("no agent key provided");
    }
    let infos = tool_service::list_tool_info(store);
    let mut resolved = Vec::new();
    for key in agents {
        let info = infos
            .iter()
            .find(|info| info.key == *key)
            .ok_or_else(|| anyhow!("unknown agent: {key}"))?;
        if !resolved
            .iter()
            .any(|existing: &String| existing == &info.key)
        {
            resolved.push(info.key.clone());
        }
    }

    let mut reports = Vec::new();
    for key in resolved {
        let before = infos.iter().find(|info| info.key == key).unwrap().enabled;
        tool_cmd::set_tool_enabled_internal(store, &key, enabled).map_err(map_app_err)?;
        store.log_audit(
            AuditDraft::new(if enabled {
                "enable_agent"
            } else {
                "disable_agent"
            })
            .tool(key.clone())
            .ok(),
        );
        reports.push(AgentMutationReport {
            agent: key,
            enabled,
            changed: before != enabled,
        });
    }
    Ok(reports)
}

// ── skills ────────────────────────────────────────────────────────────────

fn run_skills(args: SkillsArgs, store: &SkillStore, json: bool) -> anyhow::Result<()> {
    match args.command {
        SkillsCommand::List {
            query,
            tags,
            preset,
            deployed_to,
            untagged,
            no_preset,
            source,
        } => print_json(
            &list_skills_filtered(
                store,
                query.as_deref(),
                &tags,
                preset.as_deref(),
                deployed_to.as_deref(),
                untagged,
                no_preset,
                source.as_deref(),
            )?,
            json,
        ),
        SkillsCommand::Show { reference } => print_json(&show_skill(store, &reference)?, json),
        SkillsCommand::Export {
            reference,
            dest,
            force,
        } => {
            let result = export_skill(store, &reference, &dest, force)?;
            print_json(
                &serde_json::json!({"ok": true, "destination": result}),
                json,
            );
        }
        SkillsCommand::Install {
            reference,
            local,
            git,
            skillssh,
            name,
            sync,
            sync_preset,
        } => {
            let kind = classify_ref(&reference, local, git, skillssh)?;
            let sync_target = if let Some(ref s) = sync_preset {
                SyncTarget::Specific(s.clone())
            } else if sync {
                SyncTarget::Active
            } else {
                SyncTarget::None
            };
            let report = run_install(store, &reference, name.as_deref(), kind, sync_target)?;
            print_json(&report, json);
        }
        SkillsCommand::Update { reference, all } => {
            let reports = run_update(store, reference.as_deref(), all)?;
            print_json(&reports, json);
        }
        SkillsCommand::Check {
            reference,
            all,
            force,
        } => {
            let reports = run_check(store, reference.as_deref(), all, force)?;
            print_json(&reports, json);
        }
        SkillsCommand::Remove {
            references,
            yes,
            dry_run,
        } => {
            let report = run_remove(store, &references, yes, dry_run)?;
            print_json(&report, json);
        }
        SkillsCommand::Enable { references } => {
            let reports = run_deprecated_set_enabled(store, &references, true)?;
            print_json(&reports, json);
        }
        SkillsCommand::Disable { references } => {
            let reports = run_deprecated_set_enabled(store, &references, false)?;
            print_json(&reports, json);
        }
        SkillsCommand::Deploy {
            references,
            agents,
            dry_run,
        } => {
            let report = run_skill_deployment(store, &references, &agents, true, dry_run)?;
            print_json(&report, json);
        }
        SkillsCommand::Undeploy {
            references,
            agents,
            dry_run,
        } => {
            let report = run_skill_deployment(store, &references, &agents, false, dry_run)?;
            print_json(&report, json);
        }
        SkillsCommand::Status { reference } => {
            print_json(&skill_status(store, &reference)?, json);
        }
        SkillsCommand::Sync {
            preset,
            tool,
            dry_run,
        } => {
            let report = run_sync(store, preset.as_deref(), tool.as_deref(), dry_run)?;
            print_json(&report, json);
        }
        SkillsCommand::Search { query, limit } => {
            let hits = run_search(store, &query, limit)?;
            print_json(&hits, json);
        }
        SkillsCommand::SetSource {
            reference,
            git_url,
            subpath,
            branch,
            force,
            dry_run,
        } => {
            let skill = resolve_skill(store, &reference)?;
            let report = cmd::set_git_source_internal(
                store,
                &skill.id,
                &git_url,
                subpath.as_deref(),
                branch.as_deref(),
                store.proxy_url().as_deref(),
                force,
                dry_run,
            )
            .map_err(map_app_err)?;
            print_json(&report, json);
        }
        SkillsCommand::Adopt {
            paths,
            git_url,
            git_subpath,
            dry_run,
        } => {
            let report = run_adopt(
                store,
                &paths,
                git_url.as_deref(),
                git_subpath.as_deref(),
                dry_run,
            )?;
            print_json(&report, json);
        }
        SkillsCommand::Tag(args) => run_tag(args, store, json)?,
    }
    Ok(())
}

fn list_skills(store: &SkillStore) -> anyhow::Result<Vec<SkillSummary>> {
    let tags_map = store.get_tags_map()?;
    let targets = store.get_all_targets()?;
    let scenarios = store.get_all_scenarios()?;
    let scenario_lookup: std::collections::HashMap<String, String> =
        scenarios.into_iter().map(|s| (s.id, s.name)).collect();

    let mut items = Vec::new();
    for skill in store.get_all_skills()? {
        let preset_ids = store.get_scenarios_for_skill(&skill.id)?;
        let preset_names = preset_ids
            .iter()
            .filter_map(|id| scenario_lookup.get(id).cloned())
            .collect();
        let mut deployed_to: Vec<String> = targets
            .iter()
            .filter(|target| target.skill_id == skill.id && target.status == "ok")
            .map(|target| target.tool.clone())
            .collect();
        deployed_to.sort();
        deployed_to.dedup();
        items.push(SkillSummary {
            id: skill.id.clone(),
            name: skill.name.clone(),
            description: skill.description.clone(),
            path: skill.central_path.clone(),
            enabled: skill.enabled,
            tags: tags_map.get(&skill.id).cloned().unwrap_or_default(),
            source_type: skill.source_type.clone(),
            source_ref: skill.source_ref.clone(),
            preset_ids,
            presets: preset_names,
            deployed_to,
        });
    }
    Ok(items)
}

#[allow(clippy::too_many_arguments)]
fn list_skills_filtered(
    store: &SkillStore,
    query: Option<&str>,
    tags: &[String],
    preset_ref: Option<&str>,
    deployed_to: Option<&str>,
    untagged: bool,
    no_preset: bool,
    source: Option<&str>,
) -> anyhow::Result<Vec<SkillSummary>> {
    let preset_id = preset_ref
        .map(|reference| resolve_scenario(store, reference).map(|preset| preset.id))
        .transpose()?;
    if let Some(agent) = deployed_to {
        if tool_adapters::find_adapter_with_store(store, agent).is_none() {
            bail!("unknown agent: {agent}");
        }
    }
    let query = query
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase);
    let source = source
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase);
    let wanted_tags: Vec<String> = tags
        .iter()
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect();

    Ok(list_skills(store)?
        .into_iter()
        .filter(|skill| {
            query.as_ref().map_or(true, |needle| {
                skill.name.to_lowercase().contains(needle)
                    || skill
                        .description
                        .as_deref()
                        .unwrap_or_default()
                        .to_lowercase()
                        .contains(needle)
            })
        })
        .filter(|skill| wanted_tags.iter().all(|tag| skill.tags.contains(tag)))
        .filter(|skill| !untagged || skill.tags.is_empty())
        .filter(|skill| !no_preset || skill.preset_ids.is_empty())
        .filter(|skill| {
            preset_id
                .as_ref()
                .map_or(true, |id| skill.preset_ids.contains(id))
        })
        .filter(|skill| {
            deployed_to.as_ref().map_or(true, |agent| {
                skill.deployed_to.iter().any(|key| key == agent)
            })
        })
        .filter(|skill| {
            source.as_ref().map_or(true, |needle| {
                skill.source_type.to_lowercase().contains(needle)
                    || skill
                        .source_ref
                        .as_deref()
                        .unwrap_or_default()
                        .to_lowercase()
                        .contains(needle)
            })
        })
        .collect())
}

fn show_skill(store: &SkillStore, reference: &str) -> anyhow::Result<SkillDetail> {
    let skill = resolve_skill(store, reference)?;

    let summary = list_skills(store)?
        .into_iter()
        .find(|item| item.id == skill.id)
        .ok_or_else(|| anyhow!("skill summary missing"))?;

    let skill_dir = PathBuf::from(&skill.central_path);
    let skill_file = [skill_dir.join("SKILL.md"), skill_dir.join("skill.md")]
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| anyhow!("no SKILL.md found for {}", skill.name))?;
    let markdown = std::fs::read_to_string(&skill_file)?;

    Ok(SkillDetail {
        summary,
        skill_file: skill_file.to_string_lossy().to_string(),
        files: collect_files(&skill_dir)?,
        markdown,
    })
}

fn skill_status(store: &SkillStore, reference: &str) -> anyhow::Result<SkillStatusReport> {
    let skill = resolve_skill(store, reference)?;
    let summary = list_skills(store)?
        .into_iter()
        .find(|item| item.id == skill.id)
        .ok_or_else(|| anyhow!("skill summary missing"))?;
    let targets = store.get_targets_for_skill(&skill.id)?;
    let mut agents: Vec<SkillAgentStatus> = tool_service::list_tool_info(store)
        .into_iter()
        .map(|agent| {
            let target = targets.iter().find(|target| target.tool == agent.key);
            SkillAgentStatus {
                key: agent.key,
                display_name: agent.display_name,
                installed: agent.installed,
                globally_enabled: agent.enabled,
                deployed: target.is_some_and(|target| target.status == "ok"),
                target_path: target.map(|target| target.target_path.clone()),
            }
        })
        .collect();
    let mut unregistered_targets: Vec<_> = targets
        .iter()
        .filter(|target| !agents.iter().any(|agent| agent.key == target.tool))
        .collect();
    unregistered_targets.sort_by(|left, right| left.tool.cmp(&right.tool));
    for target in unregistered_targets {
        agents.push(SkillAgentStatus {
            key: target.tool.clone(),
            display_name: target.tool.clone(),
            installed: false,
            globally_enabled: false,
            deployed: target.status == "ok",
            target_path: Some(target.target_path.clone()),
        });
    }
    Ok(SkillStatusReport {
        skill: summary,
        agents,
    })
}

fn resolve_skill_references(
    store: &SkillStore,
    references: &[String],
) -> anyhow::Result<Vec<app_lib::core::skill_store::SkillRecord>> {
    if references.is_empty() {
        bail!("no skill ref provided");
    }
    let mut skills = Vec::new();
    for reference in references {
        let skill = resolve_skill(store, reference)?;
        if !skills
            .iter()
            .any(|existing: &app_lib::core::skill_store::SkillRecord| existing.id == skill.id)
        {
            skills.push(skill);
        }
    }
    Ok(skills)
}

fn run_skill_deployment(
    store: &SkillStore,
    references: &[String],
    requested_agents: &[String],
    deploy: bool,
    dry_run: bool,
) -> anyhow::Result<SkillDeploymentReport> {
    let skills = resolve_skill_references(store, references)?;
    if requested_agents.is_empty() {
        bail!("no agent key provided");
    }
    let existing_targets = store.get_all_targets()?;
    let skill_ids: Vec<String> = skills.iter().map(|skill| skill.id.clone()).collect();
    let agent_keys = if deploy {
        select_preset_agents(store, requested_agents, true)?
            .into_iter()
            .map(|agent| agent.key)
            .collect()
    } else {
        select_agent_keys_for_removal(store, requested_agents, &skill_ids, &existing_targets)?
    };
    let pair_count = skills.len() * agent_keys.len();
    let existing: std::collections::HashSet<(String, String)> = existing_targets
        .iter()
        .filter(|target| !deploy || target.status == "ok")
        .map(|target| (target.skill_id.clone(), target.tool.clone()))
        .collect();
    let changed: std::collections::HashSet<(String, String)> = skills
        .iter()
        .flat_map(|skill| {
            agent_keys
                .iter()
                .map(move |agent| (skill.id.clone(), agent.clone()))
        })
        .filter(|pair| {
            let present = existing.contains(pair);
            if deploy {
                !present
            } else {
                present
            }
        })
        .collect();
    let changed_pairs = changed.len();

    let mut preserved: Vec<String> = Vec::new();
    if !dry_run {
        scenario_service::apply_skills_to_tools(
            store,
            &skill_ids,
            &agent_keys,
            if deploy {
                scenario_service::BatchApplyMode::Add
            } else {
                scenario_service::BatchApplyMode::Remove
            },
        )
        .map_err(map_app_err)?;
        let verification =
            verify_deployment_state(store, &skill_ids, &agent_keys, deploy, &existing_targets)?;
        for skill in &skills {
            for agent in &agent_keys {
                if verification
                    .succeeded
                    .contains(&(skill.id.clone(), agent.clone()))
                    && changed.contains(&(skill.id.clone(), agent.clone()))
                {
                    store.log_audit(
                        AuditDraft::new(if deploy { "deploy" } else { "undeploy" })
                            .skill(skill.id.clone(), skill.name.clone())
                            .tool(agent.clone())
                            .ok(),
                    );
                }
            }
        }
        preserved = verification.preserved.clone();
        if !verification.failures.is_empty() {
            bail!(
                "deployment incomplete: {} pair(s) verified, {} verification issue(s): {}",
                verification.succeeded.len(),
                verification.failures.len(),
                verification.failures.join("; ")
            );
        }
    }

    Ok(SkillDeploymentReport {
        ok: true,
        action: if deploy { "deploy" } else { "undeploy" }.to_string(),
        agents: agent_keys,
        dry_run,
        skill_count: skills.len(),
        pair_count,
        changed_pairs,
        skills: skills.into_iter().map(|skill| skill.name).collect(),
        preserved,
    })
}

fn verify_deployment_state(
    store: &SkillStore,
    skill_ids: &[String],
    agent_keys: &[String],
    deployed: bool,
    previous_targets: &[app_lib::core::skill_store::SkillTargetRecord],
) -> anyhow::Result<DeploymentVerification> {
    let current_targets = store.get_all_targets()?;
    let mut failures = Vec::new();
    let mut preserved = Vec::new();
    let mut succeeded = std::collections::HashSet::new();

    for skill_id in skill_ids {
        for agent_key in agent_keys {
            let current = current_targets
                .iter()
                .find(|target| target.skill_id == *skill_id && target.tool == *agent_key);
            if deployed {
                match current.filter(|target| target.status == "ok") {
                    Some(target) => {
                        if let Err(error) = std::fs::symlink_metadata(&target.target_path) {
                            failures.push(format!(
                                "{skill_id}@{agent_key}: target is missing ({error})"
                            ));
                        } else {
                            succeeded.insert((skill_id.clone(), agent_key.clone()));
                        }
                    }
                    None => {
                        failures.push(format!("{skill_id}@{agent_key}: target was not created"))
                    }
                }
                continue;
            }

            if current.is_some() {
                failures.push(format!(
                    "{skill_id}@{agent_key}: target record still exists"
                ));
                continue;
            }

            let mut pair_succeeded = true;
            for previous in previous_targets
                .iter()
                .filter(|target| target.skill_id == *skill_id && target.tool == *agent_key)
            {
                let still_referenced = current_targets
                    .iter()
                    .any(|target| target.target_path == previous.target_path);
                if !still_referenced {
                    match std::fs::symlink_metadata(&previous.target_path) {
                        Ok(_) => {
                            // A path that survived undeploy is a failure only
                            // if it is still our deployment. If something else
                            // took it over, keeping it was the correct call and
                            // reporting it as a failure would train users to
                            // ignore the warning (#363).
                            let preserved_deliberately = !sync_engine::matches_recorded_deployment(
                                Path::new(&previous.target_path),
                                &previous.mode,
                            )
                            .unwrap_or(true);
                            if preserved_deliberately {
                                preserved.push(previous.target_path.clone());
                            } else {
                                pair_succeeded = false;
                                failures.push(format!(
                                    "{skill_id}@{agent_key}: target path still exists"
                                ));
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            pair_succeeded = false;
                            failures.push(format!(
                                "{skill_id}@{agent_key}: cannot verify removal ({error})"
                            ));
                        }
                    }
                }
            }
            if pair_succeeded {
                succeeded.insert((skill_id.clone(), agent_key.clone()));
            }
        }
    }

    Ok(DeploymentVerification {
        succeeded,
        failures,
        preserved,
    })
}

fn export_skill(
    store: &SkillStore,
    reference: &str,
    dest: &Path,
    force: bool,
) -> anyhow::Result<String> {
    let skill = resolve_skill(store, reference)?;
    let source = PathBuf::from(&skill.central_path);

    // `dest` is an arbitrary user-supplied path, so an unguarded export is a
    // recursive delete of whatever they typed (#363) — `--dest ~/Documents`
    // used to wipe it and leave a SKILL.md. Nothing at an export destination
    // is ever "ours", so overwriting has to be asked for explicitly.
    if !force {
        let state = sync_engine::classify_target(dest, Some(&source))
            .with_context(|| format!("Cannot inspect export destination {}", dest.display()))?;
        if state != sync_engine::TargetState::Absent {
            bail!(
                "Export destination {} already exists; refusing to overwrite it. \
                 Choose a path that does not exist, or pass --force to replace it.",
                dest.display()
            );
        }
    }

    let policy = if force {
        sync_engine::ReplacePolicy::UserConfirmed
    } else {
        sync_engine::ReplacePolicy::NoClobber
    };
    sync_engine::sync_skill(&source, dest, sync_engine::SyncMode::Copy, policy)?;
    Ok(dest.to_string_lossy().to_string())
}

fn resolve_skill(
    store: &SkillStore,
    reference: &str,
) -> anyhow::Result<app_lib::core::skill_store::SkillRecord> {
    let matches: Vec<_> = store
        .get_all_skills()?
        .into_iter()
        .filter(|skill| {
            skill.id == reference
                || skill.name == reference
                || skill.central_path == reference
                || Path::new(&skill.central_path)
                    .file_name()
                    .and_then(|v| v.to_str())
                    == Some(reference)
        })
        .collect();

    match matches.len() {
        1 => Ok(matches.into_iter().next().unwrap()),
        0 => Err(anyhow!("skill not found: {reference}")),
        _ => Err(anyhow!("skill reference is ambiguous: {reference}")),
    }
}

fn collect_files(root: &Path) -> anyhow::Result<Vec<String>> {
    let mut out = Vec::new();
    collect_files_inner(root, root, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_files_inner(root: &Path, current: &Path, out: &mut Vec<String>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_files_inner(root, &path, out)?;
        } else {
            out.push(path.strip_prefix(root)?.to_string_lossy().to_string());
        }
    }
    Ok(())
}

// ── install ───────────────────────────────────────────────────────────────

fn classify_ref(
    reference: &str,
    force_local: bool,
    force_git: bool,
    force_skillssh: bool,
) -> anyhow::Result<InstallKind> {
    if force_local {
        return Ok(InstallKind::Local);
    }
    if force_git {
        return Ok(InstallKind::Git);
    }
    if force_skillssh {
        return Ok(InstallKind::Skillssh);
    }

    if reference.starts_with("./")
        || reference.starts_with("../")
        || reference.starts_with('/')
        || reference.starts_with("~/")
    {
        return Ok(InstallKind::Local);
    }

    if reference.contains("://") || reference.ends_with(".git") || reference.starts_with("git@") {
        return Ok(InstallKind::Git);
    }

    if is_skillssh_shorthand(reference) {
        return Ok(InstallKind::Skillssh);
    }

    bail!(
        "ambiguous ref '{}'; pass --local, --git, or --skillssh to disambiguate",
        reference
    )
}

fn is_skillssh_shorthand(s: &str) -> bool {
    // owner/repo, owner/repo/skill, owner/repo@skill
    fn seg_ok(s: &str) -> bool {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-'))
    }
    let (head, _at_skill) = match s.split_once('@') {
        Some((h, t)) if seg_ok(t) => (h, Some(t)),
        Some(_) => return false,
        None => (s, None),
    };
    let parts: Vec<&str> = head.split('/').collect();
    (parts.len() == 2 || parts.len() == 3) && parts.iter().all(|p| seg_ok(p))
}

fn resolve_sync_target(store: &SkillStore, target: &SyncTarget) -> anyhow::Result<Option<String>> {
    match target {
        SyncTarget::None => Ok(None),
        SyncTarget::Active => Ok(store.get_active_scenario_id()?),
        SyncTarget::Specific(ref_) => {
            let scenario = resolve_scenario(store, ref_)?;
            Ok(Some(scenario.id))
        }
    }
}

fn run_install(
    store: &SkillStore,
    reference: &str,
    name: Option<&str>,
    kind: InstallKind,
    sync: SyncTarget,
) -> anyhow::Result<InstallReport> {
    let preset_id = resolve_sync_target(store, &sync)?;
    let synced = preset_id.is_some();

    let (skill_id, install_name, central_path, source_type) = match kind {
        InstallKind::Local => install_local_action(store, reference, name, preset_id.as_deref())?,
        InstallKind::Git => install_git_action(store, reference, name, preset_id.as_deref())?,
        InstallKind::Skillssh => install_skillssh_action(store, reference, preset_id.as_deref())?,
    };

    Ok(InstallReport {
        ok: true,
        skill_id,
        name: install_name,
        central_path,
        source_type,
        synced,
        preset_id,
    })
}

fn install_local_action(
    store: &SkillStore,
    reference: &str,
    name: Option<&str>,
    active_scenario: Option<&str>,
) -> anyhow::Result<(String, String, String, String)> {
    let path = expand_path(reference)?;
    if !path.exists() {
        bail!("local path does not exist: {}", path.display());
    }

    let _lock = RepoLock::acquire_foreground("cli install local")?;
    let result = installer::install_from_local(&path, name)?;
    let metadata = cmd::InstallSourceMetadata {
        source_type: "local".to_string(),
        source_ref: Some(path.to_string_lossy().to_string()),
        source_ref_resolved: None,
        source_subpath: None,
        source_branch: None,
        source_revision: None,
        remote_revision: None,
        update_status: "local_only".to_string(),
    };
    let central_path = result.central_path.to_string_lossy().to_string();
    let install_name = result.name.clone();
    let skill_id = cmd::store_installed_skill_unlocked(store, &result, &metadata, active_scenario)
        .map_err(map_app_err)?;
    Ok((skill_id, install_name, central_path, "local".to_string()))
}

fn install_git_action(
    store: &SkillStore,
    repo_url: &str,
    name: Option<&str>,
    active_scenario: Option<&str>,
) -> anyhow::Result<(String, String, String, String)> {
    git_fetcher::validate_git_url(repo_url)?;
    let proxy_url = store.proxy_url();
    let parsed = git_fetcher::parse_git_source_resolved(repo_url, proxy_url.as_deref());
    let cancel = Arc::new(AtomicBool::new(false));
    let temp_dir = git_fetcher::clone_repo_ref_scoped(
        &parsed.clone_url,
        parsed.branch.as_deref(),
        parsed.subpath.as_deref(),
        Some(&cancel),
        proxy_url.as_deref(),
        None,
    )?;
    let result = (|| -> anyhow::Result<(String, String, String)> {
        let _lock = RepoLock::acquire_foreground("cli install git")?;
        let skill_dir = cmd::resolve_skill_dir(&temp_dir, parsed.subpath.as_deref(), None)
            .map_err(map_app_err)?;
        let revision = git_fetcher::get_head_revision(&temp_dir)?;
        let install_result = installer::install_from_git_dir(&skill_dir, name)?;
        let metadata = cmd::InstallSourceMetadata {
            source_type: "git".to_string(),
            source_ref: Some(parsed.original_url.clone()),
            source_ref_resolved: Some(parsed.clone_url.clone()),
            source_subpath: git_fetcher::relative_subpath(&temp_dir, &skill_dir),
            source_branch: parsed.branch.clone(),
            source_revision: Some(revision.clone()),
            remote_revision: Some(revision),
            update_status: "up_to_date".to_string(),
        };
        let central_path = install_result.central_path.to_string_lossy().to_string();
        let install_name = install_result.name.clone();
        let skill_id =
            cmd::store_installed_skill_unlocked(store, &install_result, &metadata, active_scenario)
                .map_err(map_app_err)?;
        Ok((skill_id, install_name, central_path))
    })();
    git_fetcher::cleanup_temp(&temp_dir);
    let (skill_id, install_name, central_path) = result?;
    Ok((skill_id, install_name, central_path, "git".to_string()))
}

fn install_skillssh_action(
    store: &SkillStore,
    shorthand: &str,
    active_scenario: Option<&str>,
) -> anyhow::Result<(String, String, String, String)> {
    let (source, skill_id_field) = parse_skillssh_shorthand(shorthand)?;
    let proxy_url = store.proxy_url();
    let repo_url = format!("https://github.com/{}.git", source);
    let cancel = Arc::new(AtomicBool::new(false));
    let temp_dir =
        git_fetcher::clone_repo_ref(&repo_url, None, Some(&cancel), proxy_url.as_deref())?;
    let result = (|| -> anyhow::Result<(String, String, String)> {
        let _lock = RepoLock::acquire_foreground("cli install skillssh")?;
        let skill_dir =
            cmd::resolve_skill_dir(&temp_dir, None, Some(&skill_id_field)).map_err(map_app_err)?;
        let revision = git_fetcher::get_head_revision(&temp_dir)?;
        let source_ref = format!("{}/{}", source, skill_id_field);
        let (install_name, destination) =
            cmd::resolve_skillssh_install_target(store, &source_ref, &skill_id_field)
                .map_err(map_app_err)?;
        let install_result =
            installer::install_skill_dir_to_destination(&skill_dir, &install_name, &destination)?;
        let metadata = cmd::InstallSourceMetadata {
            source_type: "skillssh".to_string(),
            source_ref: Some(source_ref),
            source_ref_resolved: Some(repo_url.clone()),
            source_subpath: git_fetcher::relative_subpath(&temp_dir, &skill_dir),
            source_branch: None,
            source_revision: Some(revision.clone()),
            remote_revision: Some(revision),
            update_status: "up_to_date".to_string(),
        };
        let central_path = install_result.central_path.to_string_lossy().to_string();
        let skill_id =
            cmd::store_installed_skill_unlocked(store, &install_result, &metadata, active_scenario)
                .map_err(map_app_err)?;
        Ok((skill_id, install_name, central_path))
    })();
    git_fetcher::cleanup_temp(&temp_dir);
    let (skill_id, install_name, central_path) = result?;
    Ok((skill_id, install_name, central_path, "skillssh".to_string()))
}

/// Parse `owner/repo`, `owner/repo@skill`, or `owner/repo/skill` into
/// (source = "owner/repo", skill_id) — matching SkillsMP / install_from_skillssh.
fn parse_skillssh_shorthand(s: &str) -> anyhow::Result<(String, String)> {
    if let Some((head, skill_id)) = s.split_once('@') {
        if head.split('/').count() != 2 {
            bail!("invalid shorthand: '{s}' (expected owner/repo@skill)");
        }
        return Ok((head.to_string(), skill_id.to_string()));
    }
    let parts: Vec<&str> = s.split('/').collect();
    match parts.len() {
        2 => Ok((s.to_string(), parts[1].to_string())),
        3 => Ok((format!("{}/{}", parts[0], parts[1]), parts[2].to_string())),
        _ => bail!("invalid shorthand: '{s}'"),
    }
}

fn expand_path(s: &str) -> anyhow::Result<PathBuf> {
    if let Some(rest) = s.strip_prefix("~/") {
        let home = dirs_home()?;
        return Ok(home.join(rest));
    }
    if s == "~" {
        return dirs_home();
    }
    Ok(PathBuf::from(s))
}

fn dirs_home() -> anyhow::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME env var not set"))
}

// ── update / check ────────────────────────────────────────────────────────

fn run_update(
    store: &SkillStore,
    reference: Option<&str>,
    all: bool,
) -> anyhow::Result<Vec<UpdateReport>> {
    let targets = select_skill_ids(store, reference, all)?;
    let proxy_url = store.proxy_url();
    let mut reports = Vec::new();

    for skill in targets {
        let report = match skill.source_type.as_str() {
            "git" | "skillssh" => {
                match cmd::update_git_skill_internal(
                    store,
                    &skill.id,
                    proxy_url.as_deref(),
                    None,
                    None,
                ) {
                    Ok(r) => UpdateReport {
                        skill_id: skill.id.clone(),
                        name: skill.name.clone(),
                        source_type: skill.source_type.clone(),
                        refreshed: r.content_changed,
                        error: None,
                        held_back_removals: r
                            .pending_removals
                            .iter()
                            .map(|p| format!("{}: {}", p.location, p.path))
                            .collect(),
                    },
                    Err(e) => UpdateReport {
                        skill_id: skill.id.clone(),
                        name: skill.name.clone(),
                        source_type: skill.source_type.clone(),
                        refreshed: false,
                        error: Some(e.message.clone()),
                        held_back_removals: Vec::new(),
                    },
                }
            }
            "local" | "import" => {
                match cmd::reimport_local_skill_internal(store, &skill.id, None) {
                    Ok(r) => UpdateReport {
                        skill_id: skill.id.clone(),
                        name: skill.name.clone(),
                        source_type: skill.source_type.clone(),
                        refreshed: r.pending_removals.is_empty(),
                        error: None,
                        held_back_removals: r
                            .pending_removals
                            .iter()
                            .map(|p| format!("{}: {}", p.location, p.path))
                            .collect(),
                    },
                    Err(e) => UpdateReport {
                        skill_id: skill.id.clone(),
                        name: skill.name.clone(),
                        source_type: skill.source_type.clone(),
                        refreshed: false,
                        error: Some(e.message.clone()),
                        held_back_removals: Vec::new(),
                    },
                }
            }
            other => UpdateReport {
                skill_id: skill.id.clone(),
                name: skill.name.clone(),
                source_type: skill.source_type.clone(),
                refreshed: false,
                error: Some(format!("source type '{other}' cannot be refreshed")),
                held_back_removals: Vec::new(),
            },
        };
        reports.push(report);
    }

    Ok(reports)
}

fn run_check(
    store: &SkillStore,
    reference: Option<&str>,
    all: bool,
    force: bool,
) -> anyhow::Result<Vec<CheckReport>> {
    let targets = select_skill_ids(store, reference, all)?;
    let proxy_url = store.proxy_url();
    let mut reports = Vec::new();

    for skill in targets {
        if !matches!(skill.source_type.as_str(), "git" | "skillssh") {
            reports.push(CheckReport {
                skill_id: skill.id.clone(),
                name: skill.name.clone(),
                source_type: skill.source_type.clone(),
                update_status: skill.update_status.clone(),
                last_check_error: skill.last_check_error.clone(),
                skipped: true,
            });
            continue;
        }
        let report =
            match cmd::check_skill_update_internal(store, &skill.id, force, proxy_url.as_deref()) {
                Ok(dto) => CheckReport {
                    skill_id: dto.id,
                    name: dto.name,
                    source_type: dto.source_type,
                    update_status: dto.update_status,
                    last_check_error: dto.last_check_error,
                    skipped: false,
                },
                Err(e) => CheckReport {
                    skill_id: skill.id.clone(),
                    name: skill.name.clone(),
                    source_type: skill.source_type.clone(),
                    update_status: "error".to_string(),
                    last_check_error: Some(e.message.clone()),
                    skipped: false,
                },
            };
        reports.push(report);
    }

    Ok(reports)
}

fn select_skill_ids(
    store: &SkillStore,
    reference: Option<&str>,
    all: bool,
) -> anyhow::Result<Vec<app_lib::core::skill_store::SkillRecord>> {
    if let Some(r) = reference {
        if all {
            bail!("pass either a ref or --all, not both");
        }
        Ok(vec![resolve_skill(store, r)?])
    } else if all {
        Ok(store.get_all_skills()?)
    } else {
        bail!("pass a skill ref or --all")
    }
}

// ── remove ────────────────────────────────────────────────────────────────

fn run_remove(
    store: &SkillStore,
    references: &[String],
    yes: bool,
    dry_run: bool,
) -> anyhow::Result<RemoveReport> {
    if references.is_empty() {
        bail!("no skill ref provided");
    }
    let mut ids = Vec::new();
    let mut failed = Vec::new();
    for r in references {
        match resolve_skill(store, r) {
            Ok(skill) => ids.push(skill.id),
            Err(e) => failed.push(format!("{r}: {e}")),
        }
    }

    if dry_run {
        return Ok(RemoveReport {
            ok: true,
            deleted: ids.len(),
            failed,
            dry_run: true,
        });
    }
    if !failed.is_empty() {
        bail!("could not resolve every skill: {}", failed.join("; "));
    }
    if !yes {
        bail!("refusing to delete {} skill(s) without --yes", ids.len());
    }

    let result = cmd::delete_managed_skills_by_ids(store, &ids).map_err(map_app_err)?;
    for missing in result.failed {
        failed.push(format!("{missing}: not found"));
    }
    Ok(RemoveReport {
        ok: true,
        deleted: result.deleted,
        failed,
        dry_run: false,
    })
}

// ── enable / disable ──────────────────────────────────────────────────────

fn run_deprecated_set_enabled(
    store: &SkillStore,
    references: &[String],
    requested_enabled: bool,
) -> anyhow::Result<Vec<DeprecatedEnableReport>> {
    if references.is_empty() {
        bail!("no skill ref provided");
    }
    let mut reports = Vec::new();
    for r in references {
        let skill = resolve_skill(store, r)?;
        // `skills enable` repairs legacy enabled=false rows; `skills disable`
        // is a true no-op. Flipping enabled to true on disable would be the
        // opposite of what the user asked for.
        let changed = if requested_enabled && !skill.enabled {
            store.update_skill_enabled(&skill.id, true)?;
            true
        } else {
            false
        };
        let enabled_after = if requested_enabled {
            true
        } else {
            skill.enabled
        };
        let message = if requested_enabled {
            "Deprecated compatibility command: use `skills deploy --agent <key>` to make a skill available to an agent."
        } else {
            "Deprecated compatibility command: use `skills undeploy --agent <key>` to remove a skill from an agent."
        };
        reports.push(DeprecatedEnableReport {
            skill_id: skill.id,
            name: skill.name,
            enabled: enabled_after,
            changed,
            deprecated: true,
            message: message.to_string(),
        });
    }
    if reports.iter().any(|report| report.changed) {
        sync_metadata::write_all_from_db(store)?;
    }
    Ok(reports)
}

// ── sync ──────────────────────────────────────────────────────────────────

fn run_sync(
    store: &SkillStore,
    preset_ref: Option<&str>,
    tool_key: Option<&str>,
    dry_run: bool,
) -> anyhow::Result<SyncReport> {
    let preset = match preset_ref {
        Some(s) => resolve_scenario(store, s)?,
        None => {
            let active = store
                .get_active_scenario_id()?
                .ok_or_else(|| anyhow!("no active preset; pass --preset"))?;
            store
                .get_all_scenarios()?
                .into_iter()
                .find(|s| s.id == active)
                .ok_or_else(|| anyhow!("active preset not found"))?
        }
    };

    let preview =
        scenario_service::preview_scenario_sync(store, &preset.id).map_err(map_app_err)?;

    let filtered: Vec<_> = if let Some(t) = tool_key {
        preview.into_iter().filter(|p| p.tool == t).collect()
    } else {
        preview
    };

    if dry_run {
        return Ok(SyncReport {
            ok: true,
            preset_id: preset.id,
            preset_name: preset.name,
            tool: tool_key.map(|s| s.to_string()),
            dry_run: true,
            targets: filtered,
        });
    }

    // Make preset active if it isn't, then sync.
    let active = store.get_active_scenario_id()?;
    if active.as_deref() != Some(preset.id.as_str()) {
        store.set_active_scenario(&preset.id)?;
    }

    if let Some(t) = tool_key {
        // Build targets locally and filter to the requested tool so we don't
        // fan out to every enabled adapter (which is what
        // sync_active_scenario_to_tool ends up doing via
        // sync_skill_to_active_scenario).
        let all_targets = scenario_service::collect_scenario_sync_targets(store, &preset.id)
            .map_err(map_app_err)?;
        let desired: Vec<_> = all_targets.into_iter().filter(|tg| tg.tool == t).collect();
        let refusals =
            scenario_service::sync_desired_targets(store, &desired).map_err(map_app_err)?;
        scenario_service::refusals_to_error(refusals).map_err(map_app_err)?;
    } else {
        let refusals =
            scenario_service::apply_scenario_to_default(store, &preset.id).map_err(map_app_err)?;
        scenario_service::refusals_to_error(refusals).map_err(map_app_err)?;
    }

    Ok(SyncReport {
        ok: true,
        preset_id: preset.id,
        preset_name: preset.name,
        tool: tool_key.map(|s| s.to_string()),
        dry_run: false,
        targets: filtered,
    })
}

// ── search ────────────────────────────────────────────────────────────────

fn run_search(
    store: &SkillStore,
    query: &str,
    limit: Option<usize>,
) -> anyhow::Result<Vec<SearchHit>> {
    let proxy_url = store.proxy_url();
    let bounded = limit.unwrap_or(60).clamp(1, 300);
    let hits = skillssh_api::search_skills(query, bounded, proxy_url.as_deref())?;
    Ok(hits
        .into_iter()
        .map(|s| {
            let install_ref = format!("{}/{}", s.source, s.skill_id);
            let skills_sh_url = format!("https://skills.sh/{}/{}", s.source, s.skill_id);
            SearchHit {
                install_ref,
                name: s.name,
                source: s.source,
                skill_id: s.skill_id,
                installs: s.installs,
                skills_sh_url,
            }
        })
        .collect())
}

// ── adopt ─────────────────────────────────────────────────────────────────

fn run_adopt(
    store: &SkillStore,
    paths: &[PathBuf],
    git_url: Option<&str>,
    git_subpath: Option<&str>,
    dry_run: bool,
) -> anyhow::Result<AdoptReport> {
    if paths.is_empty() {
        bail!("provide at least one path to scan");
    }
    if git_url.is_some() && paths.len() != 1 {
        bail!("--git-url requires exactly one path");
    }
    if git_subpath.is_some() && git_url.is_none() {
        bail!("--git-subpath requires --git-url");
    }

    // Resolve the source subpath for git-based adopts up front so we fail fast
    // before any filesystem work. parse_git_source pulls a subpath out of GitHub
    // /tree/branch/path URLs; --git-subpath is the explicit override (pass ""
    // to mean "skill lives at the repo root").
    let resolved_git: Option<(String, Option<String>, Option<String>, Option<String>)> =
        if let Some(url) = git_url {
            git_fetcher::validate_git_url(url)?;
            let parsed = git_fetcher::parse_git_source(url);
            let subpath = match git_subpath {
                Some(s) => {
                    if s.is_empty() {
                        None
                    } else {
                        Some(s.to_string())
                    }
                }
                None => parsed.subpath.clone(),
            };
            if subpath.is_none() && git_subpath.is_none() {
                bail!(
                    "--git-url has no subpath and --git-subpath was not provided. \
                     Pass --git-subpath \"\" if the skill lives at the repo root, \
                     --git-subpath <path> for a subdirectory, or use a URL like \
                     https://github.com/owner/repo/tree/branch/path/to/skill"
                );
            }
            Some((
                parsed.clone_url,
                subpath,
                parsed.branch,
                Some(url.to_string()),
            ))
        } else {
            None
        };

    // Build exclusion set: existing central paths, sync target paths, canonicals
    let mut excluded: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for skill in store.get_all_skills()? {
        let p = PathBuf::from(&skill.central_path);
        excluded.insert(p.clone());
        if let Ok(c) = p.canonicalize() {
            excluded.insert(c);
        }
    }
    for target in store.get_all_targets()? {
        let p = PathBuf::from(&target.target_path);
        excluded.insert(p.clone());
        if let Ok(c) = p.canonicalize() {
            excluded.insert(c);
        }
    }
    let central_root = central_repo::skills_dir();
    let central_root_canonical = central_root.canonicalize().unwrap_or(central_root.clone());

    let mut candidates: Vec<AdoptCandidate> = Vec::new();
    let mut skipped: Vec<AdoptCandidate> = Vec::new();

    for path in paths {
        let path = expand_path(&path.to_string_lossy())?;
        if !path.is_dir() {
            skipped.push(AdoptCandidate {
                path: path.to_string_lossy().to_string(),
                name: String::new(),
                reason: "not a directory".to_string(),
            });
            continue;
        }

        // If the user pointed directly at a single skill dir, treat it as one
        // candidate rather than scanning its children (which would be the
        // skill's own files/references and miss the SKILL.md at the root).
        if skill_metadata::is_valid_skill_dir(&path) {
            classify_adopt_candidate(
                &path,
                false, // path itself can't be a symlink-into-central in this branch
                &excluded,
                &central_root_canonical,
                &mut candidates,
                &mut skipped,
            );
            continue;
        }

        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let is_symlink = entry.file_type()?.is_symlink();
            classify_adopt_candidate(
                &dir,
                is_symlink,
                &excluded,
                &central_root_canonical,
                &mut candidates,
                &mut skipped,
            );
        }
    }

    if dry_run {
        return Ok(AdoptReport {
            ok: true,
            dry_run: true,
            adopted: Vec::new(),
            candidates,
            skipped,
        });
    }

    if git_url.is_some() && candidates.len() != 1 {
        bail!(
            "--git-url requires exactly one adoptable skill, found {}",
            candidates.len()
        );
    }

    let mut adopted = Vec::new();
    for c in &candidates {
        let dir = PathBuf::from(&c.path);
        let _lock = RepoLock::acquire_foreground("cli adopt")?;
        let result = installer::install_from_local(&dir, None)?;
        let metadata = if let Some((clone_url, subpath, branch, original_url)) = &resolved_git {
            cmd::InstallSourceMetadata {
                source_type: "git".to_string(),
                source_ref: original_url.clone(),
                source_ref_resolved: Some(clone_url.clone()),
                source_subpath: subpath.clone(),
                source_branch: branch.clone(),
                source_revision: None,
                remote_revision: None,
                update_status: "unknown".to_string(),
            }
        } else {
            cmd::InstallSourceMetadata {
                source_type: "local".to_string(),
                source_ref: Some(dir.to_string_lossy().to_string()),
                source_ref_resolved: None,
                source_subpath: None,
                source_branch: None,
                source_revision: None,
                remote_revision: None,
                update_status: "local_only".to_string(),
            }
        };
        let central_path = result.central_path.to_string_lossy().to_string();
        let install_name = result.name.clone();
        let source_type = metadata.source_type.clone();
        let skill_id = cmd::store_installed_skill_unlocked(store, &result, &metadata, None)
            .map_err(map_app_err)?;
        adopted.push(InstallReport {
            ok: true,
            skill_id,
            name: install_name,
            central_path,
            source_type,
            synced: false,
            preset_id: None,
        });
    }

    Ok(AdoptReport {
        ok: true,
        dry_run: false,
        adopted,
        candidates: Vec::new(),
        skipped,
    })
}

fn classify_adopt_candidate(
    dir: &Path,
    is_symlink: bool,
    excluded: &std::collections::HashSet<PathBuf>,
    central_root_canonical: &Path,
    candidates: &mut Vec<AdoptCandidate>,
    skipped: &mut Vec<AdoptCandidate>,
) {
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    if excluded.contains(dir) || excluded.contains(&canonical) {
        skipped.push(AdoptCandidate {
            path: dir.to_string_lossy().to_string(),
            name,
            reason: "already managed (in DB or sync target)".to_string(),
        });
        return;
    }

    if is_symlink && canonical.starts_with(central_root_canonical) {
        skipped.push(AdoptCandidate {
            path: dir.to_string_lossy().to_string(),
            name,
            reason: "symlink into central repo (already managed)".to_string(),
        });
        return;
    }

    if !skill_metadata::is_valid_skill_dir(dir) {
        skipped.push(AdoptCandidate {
            path: dir.to_string_lossy().to_string(),
            name,
            reason: "no SKILL.md / skill.md".to_string(),
        });
        return;
    }

    candidates.push(AdoptCandidate {
        path: dir.to_string_lossy().to_string(),
        name,
        reason: "ready".to_string(),
    });
}

// ── tag ───────────────────────────────────────────────────────────────────

fn run_tag(args: TagArgs, store: &SkillStore, json: bool) -> anyhow::Result<()> {
    match args.command {
        TagCommand::Add { reference, tags } => {
            let skill = resolve_skill(store, &reference)?;
            let mut current = store
                .get_tags_map()?
                .get(&skill.id)
                .cloned()
                .unwrap_or_default();
            for t in tags {
                let tag = t.trim();
                if !tag.is_empty() && !current.iter().any(|c| c == tag) {
                    current.push(tag.to_string());
                }
            }
            cmd::set_skill_tags_internal(store, &skill.id, &current).map_err(map_app_err)?;
            print_json(
                &TagReport {
                    skill_id: skill.id,
                    name: skill.name,
                    tags: current,
                },
                json,
            );
        }
        TagCommand::Remove { reference, tags } => {
            let skill = resolve_skill(store, &reference)?;
            let mut current = store
                .get_tags_map()?
                .get(&skill.id)
                .cloned()
                .unwrap_or_default();
            current.retain(|c| !tags.iter().any(|t| t.trim() == c));
            cmd::set_skill_tags_internal(store, &skill.id, &current).map_err(map_app_err)?;
            print_json(
                &TagReport {
                    skill_id: skill.id,
                    name: skill.name,
                    tags: current,
                },
                json,
            );
        }
        TagCommand::Set { reference, tags } => {
            let skill = resolve_skill(store, &reference)?;
            cmd::set_skill_tags_internal(store, &skill.id, &tags).map_err(map_app_err)?;
            let current = store
                .get_tags_map()?
                .get(&skill.id)
                .cloned()
                .unwrap_or_default();
            print_json(
                &TagReport {
                    skill_id: skill.id,
                    name: skill.name,
                    tags: current,
                },
                json,
            );
        }
        TagCommand::Rename { old_name, new_name } => {
            let old_name = old_name.trim().to_string();
            let new_name = new_name.trim().to_string();
            let affected =
                cmd::rename_tag_internal(store, &old_name, &new_name).map_err(map_app_err)?;
            print_json(
                &GlobalTagReport {
                    ok: true,
                    tag: old_name,
                    renamed_to: Some(new_name),
                    affected_skills: affected.len(),
                    dry_run: false,
                    deleted: false,
                },
                json,
            );
        }
        TagCommand::Delete { name, yes, dry_run } => {
            let name = name.trim().to_string();
            let affected_skills = store
                .get_tags_map()?
                .values()
                .filter(|tags| tags.iter().any(|tag| tag == &name))
                .count();
            if !dry_run && !yes {
                bail!("refusing to delete tag without --yes");
            }
            if !dry_run {
                cmd::delete_tag_internal(store, &name).map_err(map_app_err)?;
            }
            print_json(
                &GlobalTagReport {
                    ok: true,
                    tag: name,
                    renamed_to: None,
                    affected_skills,
                    dry_run,
                    deleted: !dry_run,
                },
                json,
            );
        }
        TagCommand::List { reference } => {
            if let Some(r) = reference {
                let skill = resolve_skill(store, &r)?;
                let tags = store
                    .get_tags_map()?
                    .get(&skill.id)
                    .cloned()
                    .unwrap_or_default();
                print_json(
                    &TagReport {
                        skill_id: skill.id,
                        name: skill.name,
                        tags,
                    },
                    json,
                );
            } else {
                print_json(&store.get_all_tags()?, json);
            }
        }
    }
    Ok(())
}

// ── presets ───────────────────────────────────────────────────────────────

fn run_presets(args: PresetArgs, store: &SkillStore, json: bool) -> anyhow::Result<()> {
    match args.command {
        PresetCommand::List => print_json(&list_presets(store)?, json),
        PresetCommand::Current => print_json(&current_preset(store)?, json),
        PresetCommand::Show { reference } => {
            let preset = resolve_scenario(store, &reference)?;
            print_json(&preset_info_for(store, preset)?, json);
        }
        PresetCommand::Create {
            name,
            description,
            icon,
        } => {
            let preset = preset_cmd::create_preset_internal(
                store,
                &name,
                description.as_deref(),
                icon.as_deref(),
            )
            .map_err(map_app_err)?;
            print_json(&preset_info_for(store, preset)?, json);
        }
        PresetCommand::Update {
            reference,
            name,
            description,
            icon,
        } => {
            if name.is_none() && description.is_none() && icon.is_none() {
                bail!("pass at least one of --name, --description, or --icon");
            }
            let preset = resolve_scenario(store, &reference)?;
            let next_name = name.unwrap_or_else(|| preset.name.clone());
            let next_description = match description {
                Some(value) if value.trim().is_empty() => None,
                Some(value) => Some(value),
                None => preset.description.clone(),
            };
            let next_icon = match icon {
                Some(value) if value.trim().is_empty() => None,
                Some(value) => Some(value),
                None => preset.icon.clone(),
            };
            preset_cmd::update_preset_internal(
                store,
                &preset.id,
                &next_name,
                next_description.as_deref(),
                next_icon.as_deref(),
            )
            .map_err(map_app_err)?;
            let updated = resolve_scenario(store, &preset.id)?;
            print_json(&preset_info_for(store, updated)?, json);
        }
        PresetCommand::Delete {
            reference,
            yes,
            dry_run,
        } => {
            let preset = resolve_scenario(store, &reference)?;
            if !dry_run && !yes {
                bail!("refusing to delete preset without --yes");
            }
            if !dry_run {
                preset_cmd::delete_preset_internal(store, &preset.id).map_err(map_app_err)?;
            }
            print_json(
                &PresetDeleteReport {
                    ok: true,
                    preset_id: preset.id,
                    preset_name: preset.name,
                    dry_run,
                    deleted: !dry_run,
                },
                json,
            );
        }
        PresetCommand::Preview { reference } => {
            let preset = resolve_scenario(store, &reference)?;
            let preview =
                scenario_service::preview_scenario_sync(store, &preset.id).map_err(map_app_err)?;
            print_json(&preview, json);
        }
        PresetCommand::Apply { reference } => {
            let preset = resolve_scenario(store, &reference)?;
            let refusals = scenario_service::apply_scenario_to_default(store, &preset.id)
                .map_err(map_app_err)?;
            scenario_service::refusals_to_error(refusals).map_err(map_app_err)?;
            print_json(&current_preset(store)?, json);
        }
        PresetCommand::Deactivate { reference } => {
            let preset = resolve_scenario(store, &reference)?;
            let active = store.get_active_scenario_id()?;
            let is_active = active.as_deref() == Some(preset.id.as_str());
            let count_before = count_synced_targets_for_preset(store, &preset.id)?;

            if is_active {
                let next_active = replacement_preset_after_deactivate(store, &preset.id)?;
                if let Some(next) = next_active.as_ref() {
                    for refusal in scenario_service::apply_scenario_to_default(store, &next.id)
                        .map_err(map_app_err)?
                    {
                        eprintln!("warning: {refusal}");
                    }
                } else {
                    scenario_service::unsync_scenario_skills(store, &preset.id)
                        .map_err(map_app_err)?;
                    store.clear_active_scenario()?;
                }
            } else {
                // Closing a non-active preset still tears down sync targets for
                // any skills it shares with the active preset. Unsync this
                // preset first, then re-sync the active preset so the shared
                // targets are restored.
                scenario_service::unsync_scenario_skills(store, &preset.id).map_err(map_app_err)?;
                if let Some(active_id) = active.as_deref() {
                    // The delete already happened; a refusal here must not fail
                    // the command, only be reported.
                    for refusal in scenario_service::sync_scenario_skills(store, active_id)
                        .map_err(map_app_err)?
                    {
                        eprintln!("warning: {refusal}");
                    }
                }
            }

            let count_after = count_synced_targets_for_preset(store, &preset.id)?;
            let removed_target_count = count_before.saturating_sub(count_after);

            let active_after = current_preset(store)?;
            print_json(
                &PresetDeactivateReport {
                    ok: true,
                    preset_id: preset.id,
                    preset_name: preset.name,
                    removed_target_count,
                    active_preset_id: active_after.as_ref().map(|preset| preset.id.clone()),
                    active_preset_name: active_after.map(|preset| preset.name),
                },
                json,
            );
        }
        PresetCommand::Deploy {
            reference,
            agents,
            dry_run,
        } => {
            let report = run_preset_deployment(store, &reference, &agents, true, dry_run)?;
            print_json(&report, json);
        }
        PresetCommand::Undeploy {
            reference,
            agents,
            dry_run,
        } => {
            let report = run_preset_deployment(store, &reference, &agents, false, dry_run)?;
            print_json(&report, json);
        }
        PresetCommand::Status { reference, agents } => {
            print_json(&preset_status(store, &reference, &agents)?, json);
        }
        PresetCommand::AddSkill { preset, skills } => {
            let s = resolve_scenario(store, &preset)?;
            let resolved = resolve_skill_references(store, &skills)?;
            let ids: Vec<String> = resolved.iter().map(|skill| skill.id.clone()).collect();
            preset_cmd::set_preset_skills_internal(store, &s.id, &ids, true)
                .map_err(map_app_err)?;
            print_json(
                &PresetMembershipReport {
                    preset_id: s.id,
                    preset_name: s.name,
                    added: resolved.into_iter().map(|skill| skill.name).collect(),
                    removed: Vec::new(),
                    missing: Vec::new(),
                },
                json,
            );
        }
        PresetCommand::RemoveSkill { preset, skills } => {
            let s = resolve_scenario(store, &preset)?;
            let resolved = resolve_skill_references(store, &skills)?;
            let ids: Vec<String> = resolved.iter().map(|skill| skill.id.clone()).collect();
            preset_cmd::set_preset_skills_internal(store, &s.id, &ids, false)
                .map_err(map_app_err)?;
            print_json(
                &PresetMembershipReport {
                    preset_id: s.id,
                    preset_name: s.name,
                    added: Vec::new(),
                    removed: resolved.into_iter().map(|skill| skill.name).collect(),
                    missing: Vec::new(),
                },
                json,
            );
        }
    }
    Ok(())
}

fn preset_info_for(
    store: &SkillStore,
    preset: app_lib::core::skill_store::ScenarioRecord,
) -> anyhow::Result<PresetInfo> {
    let active = store.get_active_scenario_id()?;
    Ok(PresetInfo {
        skill_count: store.get_skill_ids_for_scenario(&preset.id)?.len(),
        active: active.as_deref() == Some(preset.id.as_str()),
        id: preset.id,
        name: preset.name,
        description: preset.description,
        icon: preset.icon,
        sort_order: preset.sort_order,
    })
}

fn select_preset_agents(
    store: &SkillStore,
    requested: &[String],
    require_available: bool,
) -> anyhow::Result<Vec<tool_service::ToolInfo>> {
    let infos = tool_service::list_tool_info(store);
    if requested.is_empty() {
        return Ok(infos
            .into_iter()
            .filter(|agent| {
                agent.installed
                    && agent.enabled
                    && matches!(agent.category, tool_adapters::ToolCategory::Coding)
            })
            .collect());
    }

    let mut selected = Vec::new();
    for key in requested {
        let agent = infos
            .iter()
            .find(|agent| agent.key == *key)
            .ok_or_else(|| anyhow!("unknown agent: {key}"))?;
        if require_available && !agent.installed {
            bail!("agent is not installed: {}", agent.display_name);
        }
        if require_available && !agent.enabled {
            bail!("agent is disabled: {}", agent.display_name);
        }
        if !selected
            .iter()
            .any(|existing: &tool_service::ToolInfo| existing.key == agent.key)
        {
            selected.push(agent.clone());
        }
    }
    Ok(selected)
}

fn select_agent_keys_for_removal(
    store: &SkillStore,
    requested: &[String],
    skill_ids: &[String],
    existing_targets: &[app_lib::core::skill_store::SkillTargetRecord],
) -> anyhow::Result<Vec<String>> {
    let deployed_keys: std::collections::HashSet<String> = existing_targets
        .iter()
        .filter(|target| skill_ids.contains(&target.skill_id))
        .map(|target| target.tool.clone())
        .collect();
    if requested.is_empty() {
        let mut keys: Vec<String> = deployed_keys.into_iter().collect();
        keys.sort();
        return Ok(keys);
    }

    let known_keys: std::collections::HashSet<String> = tool_service::list_tool_info(store)
        .into_iter()
        .map(|agent| agent.key)
        .collect();
    let mut selected = Vec::new();
    for key in requested {
        if !known_keys.contains(key) && !deployed_keys.contains(key) {
            bail!("unknown agent: {key}");
        }
        if !selected.contains(key) {
            selected.push(key.clone());
        }
    }
    Ok(selected)
}

fn preset_status(
    store: &SkillStore,
    reference: &str,
    requested_agents: &[String],
) -> anyhow::Result<PresetStatusReport> {
    let preset = resolve_scenario(store, reference)?;
    let preset_info = preset_info_for(store, preset.clone())?;
    let skill_ids = store.get_skill_ids_for_scenario(&preset.id)?;
    let all_targets = store.get_all_targets()?;
    let targets: std::collections::HashSet<(String, String)> = all_targets
        .iter()
        .filter(|target| target.status == "ok")
        .map(|target| (target.skill_id.clone(), target.tool.clone()))
        .collect();
    let infos = tool_service::list_tool_info(store);
    let agent_keys = if requested_agents.is_empty() {
        let mut keys: Vec<String> = infos
            .iter()
            .filter(|agent| {
                agent.installed
                    && agent.enabled
                    && matches!(agent.category, tool_adapters::ToolCategory::Coding)
            })
            .map(|agent| agent.key.clone())
            .collect();
        for target in all_targets
            .iter()
            .filter(|target| target.status == "ok" && skill_ids.contains(&target.skill_id))
        {
            if !keys.contains(&target.tool) {
                keys.push(target.tool.clone());
            }
        }
        keys
    } else {
        select_agent_keys_for_removal(store, requested_agents, &skill_ids, &all_targets)?
    };
    let agents = agent_keys
        .into_iter()
        .map(|agent_key| {
            let deployed = skill_ids
                .iter()
                .filter(|skill_id| targets.contains(&((*skill_id).clone(), agent_key.clone())))
                .count();
            let total = skill_ids.len();
            let status = if total == 0 {
                "empty"
            } else if deployed == 0 {
                "inactive"
            } else if deployed == total {
                "active"
            } else {
                "partial"
            };
            let display_name = infos
                .iter()
                .find(|agent| agent.key == agent_key)
                .map(|agent| agent.display_name.clone())
                .unwrap_or_else(|| agent_key.clone());
            PresetAgentStatus {
                key: agent_key,
                display_name,
                deployed,
                total,
                status: status.to_string(),
            }
        })
        .collect();
    Ok(PresetStatusReport {
        preset: preset_info,
        agents,
    })
}

fn run_preset_deployment(
    store: &SkillStore,
    reference: &str,
    requested_agents: &[String],
    deploy: bool,
    dry_run: bool,
) -> anyhow::Result<PresetDeploymentReport> {
    let preset = resolve_scenario(store, reference)?;
    let skill_ids = store.get_skill_ids_for_scenario(&preset.id)?;
    let existing_targets = store.get_all_targets()?;
    let agent_keys = if deploy {
        select_preset_agents(store, requested_agents, true)?
            .into_iter()
            .map(|agent| agent.key)
            .collect()
    } else {
        select_agent_keys_for_removal(store, requested_agents, &skill_ids, &existing_targets)?
    };
    if deploy && agent_keys.is_empty() {
        bail!("no enabled, installed coding agents found");
    }
    let pair_count = skill_ids.len() * agent_keys.len();
    let existing: std::collections::HashSet<(String, String)> = existing_targets
        .iter()
        .filter(|target| !deploy || target.status == "ok")
        .map(|target| (target.skill_id.clone(), target.tool.clone()))
        .collect();
    let changed: std::collections::HashSet<(String, String)> = skill_ids
        .iter()
        .flat_map(|skill_id| {
            agent_keys
                .iter()
                .map(move |agent| (skill_id.clone(), agent.clone()))
        })
        .filter(|pair| {
            let present = existing.contains(pair);
            if deploy {
                !present
            } else {
                present
            }
        })
        .collect();
    let changed_pairs = changed.len();

    let mut preserved: Vec<String> = Vec::new();
    if !dry_run {
        scenario_service::apply_skills_to_tools(
            store,
            &skill_ids,
            &agent_keys,
            if deploy {
                scenario_service::BatchApplyMode::Add
            } else {
                scenario_service::BatchApplyMode::Remove
            },
        )
        .map_err(map_app_err)?;
        let verification =
            verify_deployment_state(store, &skill_ids, &agent_keys, deploy, &existing_targets)?;
        for skill_id in &skill_ids {
            let skill = store
                .get_skill_by_id(skill_id)?
                .ok_or_else(|| anyhow!("skill missing"))?;
            for agent in &agent_keys {
                if verification
                    .succeeded
                    .contains(&(skill.id.clone(), agent.clone()))
                    && changed.contains(&(skill.id.clone(), agent.clone()))
                {
                    store.log_audit(
                        AuditDraft::new(if deploy {
                            "deploy_preset"
                        } else {
                            "undeploy_preset"
                        })
                        .skill(skill.id.clone(), skill.name.clone())
                        .tool(agent.clone())
                        .detail(format!("preset={} ({})", preset.name, preset.id))
                        .ok(),
                    );
                }
            }
        }
        preserved = verification.preserved.clone();
        if !verification.failures.is_empty() {
            bail!(
                "deployment incomplete: {} pair(s) verified, {} verification issue(s): {}",
                verification.succeeded.len(),
                verification.failures.len(),
                verification.failures.join("; ")
            );
        }
    }

    Ok(PresetDeploymentReport {
        ok: true,
        action: if deploy { "deploy" } else { "undeploy" }.to_string(),
        preset_id: preset.id,
        preset_name: preset.name,
        agents: agent_keys,
        dry_run,
        skill_count: skill_ids.len(),
        pair_count,
        changed_pairs,
        preserved,
    })
}

fn list_presets(store: &SkillStore) -> anyhow::Result<Vec<PresetInfo>> {
    let active = store.get_active_scenario_id()?;
    let scenarios = store.get_all_scenarios()?;
    Ok(scenarios
        .into_iter()
        .map(|scenario| PresetInfo {
            skill_count: store
                .get_skill_ids_for_scenario(&scenario.id)
                .unwrap_or_default()
                .len(),
            active: active.as_deref() == Some(scenario.id.as_str()),
            id: scenario.id,
            name: scenario.name,
            description: scenario.description,
            icon: scenario.icon,
            sort_order: scenario.sort_order,
        })
        .collect())
}

fn current_preset(store: &SkillStore) -> anyhow::Result<Option<PresetInfo>> {
    let scenarios = list_presets(store)?;
    Ok(scenarios.into_iter().find(|s| s.active))
}

fn count_synced_targets_for_preset(store: &SkillStore, preset_id: &str) -> anyhow::Result<usize> {
    let skill_ids = store.get_skill_ids_for_scenario(preset_id)?;
    let mut count = 0;
    for skill_id in skill_ids {
        count += store.get_targets_for_skill(&skill_id)?.len();
    }
    Ok(count)
}

fn replacement_preset_after_deactivate(
    store: &SkillStore,
    deactivated_id: &str,
) -> anyhow::Result<Option<app_lib::core::skill_store::ScenarioRecord>> {
    let scenarios = store.get_all_scenarios()?;
    Ok(scenarios
        .into_iter()
        .find(|scenario| scenario.id != deactivated_id))
}

fn resolve_scenario(
    store: &SkillStore,
    reference: &str,
) -> anyhow::Result<app_lib::core::skill_store::ScenarioRecord> {
    let scenarios = store.get_all_scenarios()?;
    if reference == "current" {
        let active = store
            .get_active_scenario_id()?
            .ok_or_else(|| anyhow!("no active preset"))?;
        return scenarios
            .into_iter()
            .find(|scenario| scenario.id == active)
            .ok_or_else(|| anyhow!("active preset not found"));
    }
    let matches: Vec<_> = scenarios
        .into_iter()
        .filter(|s| s.id == reference || s.name == reference)
        .collect();
    match matches.len() {
        1 => Ok(matches.into_iter().next().unwrap()),
        0 => Err(anyhow!("preset not found: {reference}")),
        _ => Err(anyhow!("preset reference is ambiguous: {reference}")),
    }
}

// ── git ───────────────────────────────────────────────────────────────────

fn run_git(
    args: GitArgs,
    store: &SkillStore,
    has_skills_root: bool,
    json: bool,
) -> anyhow::Result<()> {
    match args.command {
        GitCommand::Status => {
            print_json(&git_backup::get_status(&central_repo::skills_dir())?, json)
        }
        GitCommand::Init => {
            // No settings store on this path; the hostname default matches
            // what the GUI derives, and the GUI reconciles the repo identity
            // on its next backup anyway.
            git_backup::init_repo(
                &central_repo::skills_dir(),
                &git_backup::default_device_name(),
            )?;
            print_json(&git_backup::get_status(&central_repo::skills_dir())?, json);
        }
        GitCommand::Clone { url } => {
            let target = central_repo::skills_dir();
            if has_skills_root {
                git_backup::clone_into_strict(&target, &url)?;
            } else {
                git_backup::clone_into(&target, &url)?;
            }
            print_json(&git_backup::get_status(&target)?, json);
        }
        GitCommand::SetRemote { url } => {
            git_backup::set_remote(&central_repo::skills_dir(), &url)?;
            print_json(&git_backup::get_status(&central_repo::skills_dir())?, json);
        }
        GitCommand::Pull => {
            // Same engine gate as the GUI sync (object merge by default,
            // merge_engine=system opts out). A raw line merge from this CLI
            // would read as an old-client violation on other devices (§6).
            let dir = central_repo::skills_dir();
            {
                let _lock = RepoLock::acquire_foreground("git pull")?;
                let device = store
                    .get_setting("backup_device_name")
                    .ok()
                    .flatten()
                    .map(|v| git_backup::sanitize_device_name(&v))
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(git_backup::default_device_name);
                let _ = git_backup::configure_device_identity(&dir, &device);
                merge::gated_pull_unlocked(store, &dir)?;
            }
            // Reconcile the DB from the merged metadata (takes its own lock).
            sync_metadata::reindex_from_metadata(store)?;
            print_json(&git_backup::get_status(&dir)?, json);
        }
        GitCommand::Push => {
            git_backup::push(&central_repo::skills_dir())?;
            print_json(&git_backup::get_status(&central_repo::skills_dir())?, json);
        }
        GitCommand::Commit { message } => {
            git_backup::commit_all(&central_repo::skills_dir(), &message)?;
            let tag = git_backup::create_snapshot_tag(&central_repo::skills_dir())?;
            print_json(&serde_json::json!({"ok": true, "tag": tag}), json);
        }
        GitCommand::Versions { limit } => print_json(
            &git_backup::list_snapshot_versions(&central_repo::skills_dir(), limit)?,
            json,
        ),
        GitCommand::Restore { tag } => {
            git_backup::restore_snapshot_version(&central_repo::skills_dir(), &tag)?;
            print_json(&git_backup::get_status(&central_repo::skills_dir())?, json);
        }
        GitCommand::PruneSyncRefs => {
            let removed = git_backup::prune_hidden_refs_on_remote(&central_repo::skills_dir())?;
            print_json(&serde_json::json!({ "removed": removed }), json);
        }
    }
    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────

/// Keep the `AppError` itself in the chain rather than only its sentence:
/// `main` downcasts it to emit a machine-readable JSON envelope for the kinds
/// that carry details (a deployment refusal names the paths in the way).
/// The `--json` failure envelope.
///
/// Most failures are one sentence and `COMMAND_FAILED`. A few carry specifics
/// the caller has to act on rather than repeat — a deployment refusal names
/// the paths in the way — and those pass their structure straight through, so
/// an agent can say which directory is blocking and offer the way out (#363).
fn error_envelope(err: &anyhow::Error) -> serde_json::Value {
    let message = format!("{err:#}");
    if let Some(batch_failure) = err.downcast_ref::<ProjectBatchFailure>() {
        return serde_json::json!({
            "ok": false,
            "code": "PROJECT_BATCH_PARTIAL_FAILURE",
            "message": message.clone(),
            "error": message,
            "details": { "report": &batch_failure.report },
        });
    }
    match err.downcast_ref::<AppError>() {
        Some(app_err) if app_err.details.is_some() => {
            let mut value = serde_json::to_value(app_err).unwrap();
            let object = value.as_object_mut().unwrap();
            object.insert("ok".into(), serde_json::Value::Bool(false));
            // Derive the code from the kind that is already on the wire, so a
            // future detail-carrying kind cannot ship a contradicting code.
            let code = object
                .get("kind")
                .and_then(|kind| kind.as_str())
                .unwrap_or("command_failed")
                .to_ascii_uppercase();
            object.insert("code".into(), serde_json::Value::String(code));
            object.insert("error".into(), serde_json::Value::String(message));
            value
        }
        _ => serde_json::json!({
            "ok": false,
            "code": "COMMAND_FAILED",
            "message": message.clone(),
            "error": message,
        }),
    }
}

fn map_app_err(e: AppError) -> anyhow::Error {
    anyhow::Error::new(e)
}

fn print_json<T: Serialize>(value: &T, json: bool) {
    let rendered = if json {
        serde_json::to_string(value).unwrap()
    } else {
        serde_json::to_string_pretty(value).unwrap()
    };
    println!("{rendered}");
}

#[cfg(test)]
mod tests {
    /// An agent has to name the directory that is in the way and say the
    /// contents survived. Flattening the refusal into one sentence is what
    /// made that impossible, so the paths must reach the envelope as data.
    #[test]
    fn a_deployment_refusal_keeps_its_paths_in_the_json_envelope() {
        let err = map_app_err(AppError::target_conflict(
            "Refusing to deploy: 1 of 2 target(s) …",
            vec![app_lib::core::error::TargetConflictDetail {
                path: "/home/me/.claude/skills/db".to_string(),
                reason: "is not a managed deployment".to_string(),
            }],
        ));

        let envelope = error_envelope(&err);
        assert_eq!(envelope["ok"], false);
        assert_eq!(envelope["code"], "TARGET_CONFLICT");
        assert_eq!(envelope["kind"], "target_conflict");
        assert_eq!(
            envelope["details"]["conflicts"][0]["path"],
            "/home/me/.claude/skills/db"
        );
    }

    /// Everything else keeps the shape callers already parse.
    #[test]
    fn an_ordinary_failure_keeps_the_command_failed_envelope() {
        let envelope = error_envelope(&anyhow!("no agent key provided"));
        assert_eq!(envelope["code"], "COMMAND_FAILED");
        assert_eq!(envelope["message"], "no agent key provided");
        assert!(envelope.get("details").is_none());
    }

    #[test]
    fn a_partial_project_operation_keeps_its_report_in_the_json_envelope() {
        let tmp = tempdir().unwrap();
        let project = test_project("p1", "repo", &tmp.path().join("repo"));
        let mut report = ProjectBatchReport::new(&project, "add_skill");
        report.ok = false;
        report.added.push(ProjectActionItem {
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            action: "add_skill".to_string(),
            skill_id: Some("skill-1".to_string()),
            skill_name: "demo".to_string(),
            agent: "codex".to_string(),
            relative_path: "demo".to_string(),
            path: "/repo/.codex/skills/demo".to_string(),
            outcome: "added".to_string(),
        });
        report.skipped.push(project_action_not_found(
            &report,
            "missing".to_string(),
            "claude_code".to_string(),
        ));
        report.failed.push(project_action_failure(
            &report,
            Some("skill-1".to_string()),
            "demo".to_string(),
            "codex".to_string(),
            "demo".to_string(),
            Some("/repo/.codex/skills/demo".to_string()),
            "target is not a managed deployment".to_string(),
        ));

        let envelope = error_envelope(&anyhow::Error::new(ProjectBatchFailure {
            report: report.clone(),
        }));

        assert_eq!(envelope["ok"], false);
        assert_eq!(envelope["code"], "PROJECT_BATCH_PARTIAL_FAILURE");
        assert_eq!(envelope["details"]["report"]["failed"][0]["agent"], "codex");
        assert_eq!(
            envelope["details"]["report"]["failed"][0]["message"],
            "target is not a managed deployment"
        );
        assert_eq!(
            envelope["details"]["report"]["added"][0]["outcome"],
            "added"
        );
        assert_eq!(
            envelope["details"]["report"]["skipped"][0]["outcome"],
            "not_found"
        );
        let rendered = render_project_batch(&report);
        assert!(rendered.contains("added demo"));
        assert!(rendered.contains("not_found missing"));
        assert!(rendered.contains("FAILED demo"));
    }

    use super::*;
    use app_lib::core::skill_store::{ProjectRecord, ScenarioRecord, SkillRecord};
    use app_lib::core::tool_adapters::{CustomToolDef, ToolCategory};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn parses_agent_friendly_commands_and_aliases() {
        let cli = Cli::try_parse_from([
            "skills-manager-cli",
            "--json",
            "skills",
            "deploy",
            "browser",
            "--to",
            "codex",
            "--agent",
            "claude_code",
            "--dry-run",
        ])
        .unwrap();
        assert!(cli.json);
        assert!(matches!(
            cli.command,
            Commands::Skills(SkillsArgs {
                command: SkillsCommand::Deploy {
                    agents,
                    dry_run: true,
                    ..
                }
            }) if agents == vec!["codex", "claude_code"]
        ));

        let cli = Cli::try_parse_from([
            "skills-manager-cli",
            "skills",
            "list",
            "--query",
            "react",
            "--tag",
            "frontend",
            "--preset",
            "Web Dev",
            "--deployed-to",
            "claude_code",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Commands::Skills(SkillsArgs {
                command: SkillsCommand::List {
                    query: Some(query),
                    tags,
                    preset: Some(preset),
                    deployed_to: Some(agent),
                    ..
                }
            }) if query == "react"
                && tags == vec!["frontend"]
                && preset == "Web Dev"
                && agent == "claude_code"
        ));

        let cli = Cli::try_parse_from([
            "skills-manager-cli",
            "agents",
            "enable",
            "codex",
            "claude_code",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Commands::Tools(ToolsArgs {
                command: ToolsCommand::Enable { agents }
            }) if agents == vec!["codex", "claude_code"]
        ));

        let cli = Cli::try_parse_from([
            "skills-manager-cli",
            "presets",
            "open",
            "Web Dev",
            "--agent",
            "codex",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Commands::Presets(PresetArgs {
                command: PresetCommand::Deploy {
                    reference,
                    agents,
                    ..
                }
            }) if reference == "Web Dev" && agents == vec!["codex"]
        ));
    }

    #[test]
    fn parses_project_commands_and_global_json_flag() {
        let add =
            Cli::try_parse_from(["skills-manager-cli", "--json", "projects", "add", "./repo"])
                .unwrap();
        assert!(add.json);
        assert!(matches!(
            add.command,
            Commands::Projects(ProjectsArgs {
                command: ProjectsCommand::Add { path }
            }) if path == Path::new("./repo")
        ));

        let list = Cli::try_parse_from(["skills-manager-cli", "projects", "list"]).unwrap();
        assert!(matches!(
            list.command,
            Commands::Projects(ProjectsArgs {
                command: ProjectsCommand::List
            })
        ));

        let remove =
            Cli::try_parse_from(["skills-manager-cli", "projects", "remove", "my-project"])
                .unwrap();
        assert!(matches!(
            remove.command,
            Commands::Projects(ProjectsArgs {
                command: ProjectsCommand::Remove { project_ref }
            }) if project_ref == "my-project"
        ));
    }

    #[test]
    fn parses_project_skill_commands_and_remove_safety() {
        let add = Cli::try_parse_from([
            "skills-manager-cli",
            "projects",
            "add-skill",
            "repo",
            "demo",
            "--agent",
            "codex",
            "--agent",
            "claude_code",
        ])
        .unwrap();
        assert!(matches!(
            add.command,
            Commands::Projects(ProjectsArgs {
                command: ProjectsCommand::AddSkill {
                    project_ref,
                    skill_ref,
                    agents,
                }
            }) if project_ref == "repo"
                && skill_ref == "demo"
                && agents == vec!["codex", "claude_code"]
        ));

        let dry_run = Cli::try_parse_from([
            "skills-manager-cli",
            "projects",
            "remove-skill",
            "repo",
            "demo",
            "--agent",
            "codex",
            "--dry-run",
        ])
        .unwrap();
        assert!(matches!(
            dry_run.command,
            Commands::Projects(ProjectsArgs {
                command: ProjectsCommand::RemoveSkill(args)
            }) if args.project_ref == "repo"
                && args.skill_relative_path == "demo"
                && args.agents == vec!["codex"]
                && args.safety.dry_run
                && !args.safety.yes
        ));

        let confirmed = Cli::try_parse_from([
            "skills-manager-cli",
            "projects",
            "remove-skill",
            "repo",
            "demo",
            "--agent",
            "codex",
            "--yes",
        ])
        .unwrap();
        assert!(matches!(
            confirmed.command,
            Commands::Projects(ProjectsArgs {
                command: ProjectsCommand::RemoveSkill(args)
            }) if args.safety.yes && !args.safety.dry_run
        ));

        assert!(Cli::try_parse_from([
            "skills-manager-cli",
            "projects",
            "add-skill",
            "repo",
            "demo",
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "skills-manager-cli",
            "projects",
            "remove-skill",
            "repo",
            "demo",
            "--agent",
            "codex",
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "skills-manager-cli",
            "projects",
            "remove-skill",
            "repo",
            "demo",
            "--agent",
            "codex",
            "--dry-run",
            "--yes",
        ])
        .is_err());
    }

    #[test]
    fn parses_project_preset_commands_and_remove_safety() {
        let add = Cli::try_parse_from([
            "skills-manager-cli",
            "projects",
            "add-preset",
            "repo",
            "current",
            "--agent",
            "codex",
        ])
        .unwrap();
        assert!(matches!(
            add.command,
            Commands::Projects(ProjectsArgs { command: ProjectsCommand::AddPreset { project_ref, preset_ref, agents } })
                if project_ref == "repo" && preset_ref == "current" && agents == vec!["codex"]
        ));

        let dry_run = Cli::try_parse_from([
            "skills-manager-cli",
            "projects",
            "remove-preset",
            "repo",
            "web",
            "--agent",
            "codex",
            "--dry-run",
        ])
        .unwrap();
        assert!(
            matches!(dry_run.command, Commands::Projects(ProjectsArgs { command: ProjectsCommand::RemovePreset(args) }) if args.safety.dry_run && !args.safety.yes)
        );
        let confirmed = Cli::try_parse_from([
            "skills-manager-cli",
            "projects",
            "remove-preset",
            "repo",
            "web",
            "--agent",
            "codex",
            "--yes",
        ])
        .unwrap();
        assert!(
            matches!(confirmed.command, Commands::Projects(ProjectsArgs { command: ProjectsCommand::RemovePreset(args) }) if args.safety.yes && !args.safety.dry_run)
        );

        assert!(Cli::try_parse_from([
            "skills-manager-cli",
            "projects",
            "add-preset",
            "repo",
            "web"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "skills-manager-cli",
            "projects",
            "remove-preset",
            "repo",
            "web",
            "--agent",
            "codex",
        ])
        .is_err());
    }

    fn setup_project_skill_cli(
        tmp: &tempfile::TempDir,
    ) -> (SkillStore, ProjectRecord, PathBuf, PathBuf, PathBuf) {
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let first_global_root = tmp.path().join("first-global-skills");
        let second_global_root = tmp.path().join("second-global-skills");
        fs::create_dir_all(&first_global_root).unwrap();
        fs::create_dir_all(&second_global_root).unwrap();
        tool_service::set_custom_tools(
            &store,
            &[
                CustomToolDef {
                    key: "first_agent".to_string(),
                    display_name: "First Agent".to_string(),
                    skills_dir: first_global_root.to_string_lossy().into_owned(),
                    project_relative_skills_dir: Some(".first/skills".to_string()),
                    category: ToolCategory::Coding,
                },
                CustomToolDef {
                    key: "second_agent".to_string(),
                    display_name: "Second Agent".to_string(),
                    skills_dir: second_global_root.to_string_lossy().into_owned(),
                    project_relative_skills_dir: Some(".second/skills".to_string()),
                    category: ToolCategory::Coding,
                },
            ],
        )
        .unwrap();
        store.set_setting("sync_mode", "copy").unwrap();

        let project_root = tmp.path().join("repo");
        fs::create_dir(&project_root).unwrap();
        let project = app_lib::core::project_service::add_project(&store, &project_root).unwrap();
        let central_path = tmp.path().join("central/demo");
        fs::create_dir_all(&central_path).unwrap();
        fs::write(central_path.join("SKILL.md"), "central skill").unwrap();
        store
            .insert_skill(&SkillRecord {
                id: "skill-demo".to_string(),
                name: "demo".to_string(),
                description: Some("test skill".to_string()),
                source_type: "local".to_string(),
                source_ref: Some(central_path.to_string_lossy().into_owned()),
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
            })
            .unwrap();
        (
            store,
            project,
            central_path,
            project_root.join(".first/skills"),
            project_root.join(".second/skills"),
        )
    }

    #[test]
    fn projects_add_skill_preserves_existing_target_and_reports_outcomes() {
        let tmp = tempdir().unwrap();
        let (store, project, _central_path, first_target, second_target) =
            setup_project_skill_cli(&tmp);
        app_lib::core::project_skill_service::add_skill_to_project(
            &store,
            &project.id,
            "skill-demo",
            "first_agent",
        )
        .unwrap();
        fs::write(first_target.join("demo/SKILL.md"), "local edit").unwrap();

        run_projects(
            ProjectsArgs {
                command: ProjectsCommand::AddSkill {
                    project_ref: project.id.clone(),
                    skill_ref: "skill-demo".to_string(),
                    agents: vec!["first_agent".to_string(), "second_agent".to_string()],
                },
            },
            &store,
            false,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(first_target.join("demo/SKILL.md")).unwrap(),
            "local edit"
        );
        assert_eq!(
            fs::read_to_string(second_target.join("demo/SKILL.md")).unwrap(),
            "central skill"
        );

        let mut report = ProjectBatchReport::new(&project, "add_skill");
        let existing = app_lib::core::project_skill_service::preview_remove_skill_from_project(
            &store,
            &project.id,
            "demo",
            "first_agent",
        )
        .unwrap();
        let added = app_lib::core::project_skill_service::preview_remove_skill_from_project(
            &store,
            &project.id,
            "demo",
            "second_agent",
        )
        .unwrap();
        report
            .skipped
            .push(project_action_item(&report, existing, "already_present"));
        report
            .added
            .push(project_action_item(&report, added, "added"));
        let rendered = render_project_batch(&report);
        assert!(rendered.contains("already_present demo"));
        assert!(rendered.contains("added demo"));
        assert!(rendered.contains("first_agent"));
        assert!(rendered.contains("second_agent"));
        assert_eq!(
            serde_json::to_value(&report).unwrap()["skipped"][0]["outcome"],
            "already_present"
        );
        assert_eq!(
            serde_json::to_value(&report).unwrap()["added"][0]["outcome"],
            "added"
        );
    }

    #[test]
    fn projects_add_skill_continues_after_invalid_agent_and_returns_batch_envelope() {
        let tmp = tempdir().unwrap();
        let (store, project, _central_path, first_target, second_target) =
            setup_project_skill_cli(&tmp);
        store
            .set_setting("disabled_tools", r#"["first_agent"]"#)
            .unwrap();

        let error = run_projects(
            ProjectsArgs {
                command: ProjectsCommand::AddSkill {
                    project_ref: project.id.clone(),
                    skill_ref: "skill-demo".to_string(),
                    agents: vec!["first_agent".to_string(), "second_agent".to_string()],
                },
            },
            &store,
            true,
        )
        .unwrap_err();

        assert!(!first_target.join("demo").exists());
        assert_eq!(
            fs::read_to_string(second_target.join("demo/SKILL.md")).unwrap(),
            "central skill"
        );
        let envelope = error_envelope(&error);
        let expected_disabled_path = Path::new(&project.path)
            .join(".first/skills/demo")
            .to_string_lossy()
            .into_owned();
        assert_eq!(envelope["code"], "PROJECT_BATCH_PARTIAL_FAILURE");
        assert_eq!(
            envelope["details"]["report"]["added"][0]["agent"],
            "second_agent"
        );
        assert_eq!(
            envelope["details"]["report"]["failed"][0]["agent"],
            "first_agent"
        );
        assert_eq!(
            envelope["details"]["report"]["failed"][0]["path"],
            expected_disabled_path
        );
    }

    #[test]
    fn projects_preset_batches_preserve_order_partial_results_and_project_only_removals() {
        let tmp = tempdir().unwrap();
        let (store, project, central_path, first_target, second_target) =
            setup_project_skill_cli(&tmp);
        let missing_source = tmp.path().join("central/broken");
        store
            .insert_skill(&SkillRecord {
                id: "skill-broken".to_string(),
                name: "broken".to_string(),
                description: None,
                source_type: "local".to_string(),
                source_ref: None,
                source_ref_resolved: None,
                source_subpath: None,
                source_branch: None,
                source_revision: None,
                remote_revision: None,
                central_path: missing_source.to_string_lossy().into_owned(),
                content_hash: None,
                enabled: true,
                created_at: 2,
                updated_at: 2,
                status: "ok".to_string(),
                update_status: "local_only".to_string(),
                last_checked_at: None,
                last_check_error: None,
            })
            .unwrap();
        store
            .insert_scenario(&ScenarioRecord {
                id: "preset-main".to_string(),
                name: "Main".to_string(),
                description: None,
                icon: None,
                sort_order: 0,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
        store
            .add_skill_to_scenario("preset-main", "skill-demo")
            .unwrap();
        store
            .add_skill_to_scenario("preset-main", "skill-broken")
            .unwrap();
        store
            .reorder_scenario_skills(
                "preset-main",
                &["skill-demo".to_string(), "skill-broken".to_string()],
            )
            .unwrap();
        store
            .insert_scenario(&ScenarioRecord {
                id: "preset-overlap".to_string(),
                name: "Overlap".to_string(),
                description: None,
                icon: None,
                sort_order: 1,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
        store
            .add_skill_to_scenario("preset-overlap", "skill-demo")
            .unwrap();
        app_lib::core::project_skill_service::add_skill_to_project(
            &store,
            &project.id,
            "skill-demo",
            "first_agent",
        )
        .unwrap();
        fs::write(first_target.join("demo/SKILL.md"), "existing copy").unwrap();

        let error = run_projects(
            ProjectsArgs {
                command: ProjectsCommand::AddPreset {
                    project_ref: project.id.clone(),
                    preset_ref: "preset-main".to_string(),
                    agents: vec!["first_agent".to_string(), "second_agent".to_string()],
                },
            },
            &store,
            true,
        )
        .unwrap_err();
        let report = &error_envelope(&error)["details"]["report"];
        assert_eq!(report["skipped"][0]["agent"], "first_agent");
        assert_eq!(report["added"][0]["agent"], "second_agent");
        assert_eq!(report["failed"].as_array().unwrap().len(), 2);
        assert_eq!(
            fs::read_to_string(first_target.join("demo/SKILL.md")).unwrap(),
            "existing copy"
        );
        assert_eq!(
            fs::read_to_string(second_target.join("demo/SKILL.md")).unwrap(),
            "central skill"
        );
        fs::create_dir_all(second_target.join("nested")).unwrap();
        fs::rename(
            second_target.join("demo"),
            second_target.join("nested/demo"),
        )
        .unwrap();

        let dry_run_error = run_projects(
            ProjectsArgs {
                command: ProjectsCommand::RemovePreset(RemoveProjectPresetArgs {
                    project_ref: project.id.clone(),
                    preset_ref: "preset-main".to_string(),
                    agents: vec![
                        "first_agent".to_string(),
                        "second_agent".to_string(),
                        "unknown_agent".to_string(),
                    ],
                    safety: ProjectRemoveSafetyArgs {
                        dry_run: true,
                        yes: false,
                    },
                }),
            },
            &store,
            true,
        )
        .unwrap_err();
        let would_remove = &error_envelope(&dry_run_error)["details"]["report"]["would_remove"];
        assert_eq!(would_remove[0]["agent"], "first_agent");
        assert_eq!(would_remove[0]["relative_path"], "demo");
        assert_eq!(would_remove[1]["agent"], "second_agent");
        assert_eq!(would_remove[1]["relative_path"], "nested/demo");
        assert!(first_target.join("demo").is_dir());
        assert!(second_target.join("nested/demo").is_dir());

        store
            .set_setting("disabled_tools", r#"["first_agent"]"#)
            .unwrap();
        run_projects(
            ProjectsArgs {
                command: ProjectsCommand::RemovePreset(RemoveProjectPresetArgs {
                    project_ref: project.id.clone(),
                    preset_ref: "preset-main".to_string(),
                    agents: vec!["first_agent".to_string(), "second_agent".to_string()],
                    safety: ProjectRemoveSafetyArgs {
                        dry_run: false,
                        yes: true,
                    },
                }),
            },
            &store,
            false,
        )
        .unwrap();
        assert!(!first_target.join("demo").exists());
        assert!(!second_target.join("nested/demo").exists());
        assert!(central_path.is_dir());
        assert!(central_path.join("SKILL.md").is_file());
    }

    #[test]
    fn projects_remove_skill_dry_run_reports_exact_preview_and_fails_for_invalid_target() {
        let tmp = tempdir().unwrap();
        let (store, project, _central_path, first_target, _) = setup_project_skill_cli(&tmp);
        app_lib::core::project_skill_service::add_skill_to_project(
            &store,
            &project.id,
            "skill-demo",
            "first_agent",
        )
        .unwrap();

        let error = run_projects(
            ProjectsArgs {
                command: ProjectsCommand::RemoveSkill(RemoveProjectSkillArgs {
                    project_ref: project.id.clone(),
                    skill_relative_path: "demo".to_string(),
                    agents: vec!["first_agent".to_string(), "unknown_agent".to_string()],
                    safety: ProjectRemoveSafetyArgs {
                        dry_run: true,
                        yes: false,
                    },
                }),
            },
            &store,
            true,
        )
        .unwrap_err();

        assert!(first_target.join("demo").is_dir());
        let envelope = error_envelope(&error);
        let expected_preview_path = fs::canonicalize(first_target.join("demo"))
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(envelope["code"], "PROJECT_BATCH_PARTIAL_FAILURE");
        assert_eq!(
            envelope["details"]["report"]["would_remove"][0]["outcome"],
            "would_remove"
        );
        assert_eq!(
            envelope["details"]["report"]["would_remove"][0]["path"],
            expected_preview_path
        );
        assert_eq!(
            envelope["details"]["report"]["failed"][0]["agent"],
            "unknown_agent"
        );
    }

    #[test]
    fn projects_remove_skill_dry_run_preserves_copy_and_yes_removes_only_project_copy() {
        let tmp = tempdir().unwrap();
        let (store, project, central_path, first_target, _) = setup_project_skill_cli(&tmp);
        app_lib::core::project_skill_service::add_skill_to_project(
            &store,
            &project.id,
            "skill-demo",
            "first_agent",
        )
        .unwrap();

        let dry_run = ProjectsArgs {
            command: ProjectsCommand::RemoveSkill(RemoveProjectSkillArgs {
                project_ref: project.id.clone(),
                skill_relative_path: "demo".to_string(),
                agents: vec!["first_agent".to_string()],
                safety: ProjectRemoveSafetyArgs {
                    dry_run: true,
                    yes: false,
                },
            }),
        };
        run_projects(dry_run, &store, false).unwrap();
        assert!(first_target.join("demo").is_dir());

        let missing_dry_run = ProjectsArgs {
            command: ProjectsCommand::RemoveSkill(RemoveProjectSkillArgs {
                project_ref: project.id.clone(),
                skill_relative_path: "missing".to_string(),
                agents: vec!["first_agent".to_string()],
                safety: ProjectRemoveSafetyArgs {
                    dry_run: true,
                    yes: false,
                },
            }),
        };
        run_projects(missing_dry_run, &store, false).unwrap();

        let confirmed = ProjectsArgs {
            command: ProjectsCommand::RemoveSkill(RemoveProjectSkillArgs {
                project_ref: project.id.clone(),
                skill_relative_path: "demo".to_string(),
                agents: vec!["first_agent".to_string()],
                safety: ProjectRemoveSafetyArgs {
                    dry_run: false,
                    yes: true,
                },
            }),
        };
        run_projects(confirmed, &store, false).unwrap();
        assert!(!first_target.join("demo").exists());
        assert!(central_path.is_dir());
    }

    fn test_project(id: &str, name: &str, path: &Path) -> ProjectRecord {
        ProjectRecord {
            id: id.to_string(),
            name: name.to_string(),
            path: path.to_string_lossy().into_owned(),
            workspace_type: "project".to_string(),
            linked_agent_key: None,
            linked_agent_name: None,
            disabled_path: None,
            sort_order: 0,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn project_reference_resolution_prefers_id_then_path_then_unique_name() {
        let tmp = tempdir().unwrap();
        let alpha = test_project("id-a", "alpha", &tmp.path().join("alpha"));
        let beta = test_project("alpha", "beta", &tmp.path().join("beta"));
        let projects = vec![alpha.clone(), beta.clone()];

        assert_eq!(
            resolve_project_reference(&projects, "alpha").unwrap().id,
            "alpha"
        );
        assert_eq!(
            resolve_project_reference(&projects, &alpha.path)
                .unwrap()
                .id,
            "id-a"
        );
        assert_eq!(
            resolve_project_reference(&projects, "beta").unwrap().id,
            "alpha"
        );
    }

    #[test]
    fn project_reference_resolution_rejects_ambiguous_names_and_missing_refs() {
        let tmp = tempdir().unwrap();
        let alpha = test_project("id-a", "alpha", &tmp.path().join("one/alpha"));
        let other_alpha = test_project("id-b", "alpha", &tmp.path().join("two/alpha"));

        let error =
            resolve_project_reference(&[alpha.clone(), other_alpha.clone()], "alpha").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("ambiguous"));
        assert!(message.contains("id-a"));
        assert!(message.contains("id-b"));
        assert!(resolve_project_reference(&[alpha], "missing").is_err());
    }

    #[test]
    fn project_list_is_empty_then_reports_skill_count_and_json_metadata() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        assert!(list_project_entries(&store).unwrap().is_empty());

        let project_path = tmp.path().join("repo");
        fs::create_dir_all(project_path.join(".claude/skills/demo")).unwrap();
        fs::write(
            project_path.join(".claude/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: test\n---\n",
        )
        .unwrap();
        let project = app_lib::core::project_service::add_project(&store, &project_path).unwrap();

        let listed = list_project_entries(&store).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, project.id);
        assert_eq!(listed[0].skill_count, 1);
        let json = serde_json::to_value(&listed[0]).unwrap();
        assert_eq!(json["name"], "repo");
        assert_eq!(json["skill_count"], 1);
        assert!(json["created_at"].is_number());
    }

    #[test]
    fn project_list_preserves_store_order() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let mut later = test_project("id-later", "later", &tmp.path().join("later"));
        later.sort_order = 1;
        later.created_at = 1;
        let mut earlier = test_project("id-earlier", "earlier", &tmp.path().join("earlier"));
        earlier.sort_order = 0;
        earlier.created_at = 2;
        store.insert_project(&later).unwrap();
        store.insert_project(&earlier).unwrap();

        let listed = list_project_entries(&store).unwrap();

        assert_eq!(
            listed
                .iter()
                .map(|project| project.id.as_str())
                .collect::<Vec<_>>(),
            vec!["id-earlier", "id-later"]
        );
    }

    #[test]
    fn duplicate_project_add_uses_existing_cli_error_envelope() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let project_path = tmp.path().join("repo");
        fs::create_dir(&project_path).unwrap();
        app_lib::core::project_service::add_project(&store, &project_path).unwrap();

        let error = app_lib::core::project_service::add_project(&store, &project_path).unwrap_err();
        let envelope = error_envelope(&map_app_err(error));

        assert_eq!(envelope["ok"], false);
        assert_eq!(envelope["code"], "COMMAND_FAILED");
        assert!(envelope["message"]
            .as_str()
            .unwrap()
            .contains("already linked"));
    }

    #[test]
    fn removing_project_record_leaves_project_files_intact() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let project_path = tmp.path().join("repo");
        fs::create_dir(&project_path).unwrap();
        let project = app_lib::core::project_service::add_project(&store, &project_path).unwrap();

        let resolved =
            resolve_project_reference(&store.get_all_projects().unwrap(), &project.id).unwrap();
        store.delete_project(&resolved.id).unwrap();

        assert!(store.get_all_projects().unwrap().is_empty());
        assert!(project_path.is_dir());
        assert!(project_path.join(".claude/skills").is_dir());
        assert!(project_path.join(".claude/skills-disabled").is_dir());
    }

    #[test]
    fn skill_and_preset_deployment_round_trip() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let source = tmp.path().join("central/demo");
        let target_root = tmp.path().join("agent-skills");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target_root).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: test skill\n---\n",
        )
        .unwrap();
        fs::write(source.join("payload.txt"), "managed").unwrap();

        let test_agent = CustomToolDef {
            key: "test_agent".to_string(),
            display_name: "Test Agent".to_string(),
            skills_dir: target_root.to_string_lossy().to_string(),
            project_relative_skills_dir: None,
            category: ToolCategory::Coding,
        };
        tool_service::set_custom_tools(&store, &[test_agent.clone()]).unwrap();
        store.set_setting("sync_mode", "copy").unwrap();
        store
            .insert_skill(&SkillRecord {
                id: "skill-demo".to_string(),
                name: "demo".to_string(),
                description: Some("test skill".to_string()),
                source_type: "local".to_string(),
                source_ref: Some(source.to_string_lossy().to_string()),
                source_ref_resolved: None,
                source_subpath: None,
                source_branch: None,
                source_revision: None,
                remote_revision: None,
                central_path: source.to_string_lossy().to_string(),
                content_hash: None,
                enabled: true,
                created_at: 1,
                updated_at: 1,
                status: "ok".to_string(),
                update_status: "local_only".to_string(),
                last_checked_at: None,
                last_check_error: None,
            })
            .unwrap();

        let dry_run = run_skill_deployment(
            &store,
            &["demo".to_string()],
            &["test_agent".to_string()],
            true,
            true,
        )
        .unwrap();
        assert_eq!(dry_run.changed_pairs, 1);
        assert!(!target_root.join("demo").exists());
        assert!(store.get_all_targets().unwrap().is_empty());

        let deployed = run_skill_deployment(
            &store,
            &["demo".to_string()],
            &["test_agent".to_string()],
            true,
            false,
        )
        .unwrap();
        assert_eq!(deployed.changed_pairs, 1);
        assert_eq!(
            fs::read_to_string(target_root.join("demo/payload.txt")).unwrap(),
            "managed"
        );
        let status = skill_status(&store, "demo").unwrap();
        assert!(status
            .agents
            .iter()
            .any(|agent| agent.key == "test_agent" && agent.deployed));

        let dry_remove = run_skill_deployment(
            &store,
            &["demo".to_string()],
            &["test_agent".to_string()],
            false,
            true,
        )
        .unwrap();
        assert_eq!(dry_remove.changed_pairs, 1);
        assert!(target_root.join("demo").exists());

        run_skill_deployment(
            &store,
            &["demo".to_string()],
            &["test_agent".to_string()],
            false,
            false,
        )
        .unwrap();
        assert!(!target_root.join("demo").exists());
        assert!(store.get_all_targets().unwrap().is_empty());
        let audit_count = store.list_audit(None).unwrap().len();
        let noop_remove = run_skill_deployment(
            &store,
            &["demo".to_string()],
            &["test_agent".to_string()],
            false,
            false,
        )
        .unwrap();
        assert_eq!(noop_remove.changed_pairs, 0);
        assert_eq!(store.list_audit(None).unwrap().len(), audit_count);

        store
            .insert_scenario(&ScenarioRecord {
                id: "preset-web".to_string(),
                name: "Web Dev".to_string(),
                description: None,
                icon: None,
                sort_order: 0,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
        store
            .add_skill_to_scenario("preset-web", "skill-demo")
            .unwrap();

        let deployed =
            run_preset_deployment(&store, "Web Dev", &["test_agent".to_string()], true, false)
                .unwrap();
        assert_eq!(deployed.changed_pairs, 1);
        let status = preset_status(&store, "Web Dev", &["test_agent".to_string()]).unwrap();
        assert_eq!(status.agents[0].status, "active");

        store
            .set_setting(
                "disabled_tools",
                &serde_json::to_string(&vec!["test_agent"]).unwrap(),
            )
            .unwrap();
        let status = preset_status(&store, "Web Dev", &[]).unwrap();
        assert!(status
            .agents
            .iter()
            .any(|agent| agent.key == "test_agent" && agent.status == "active"));

        tool_service::set_custom_tools(&store, &[]).unwrap();
        let status = skill_status(&store, "demo").unwrap();
        assert!(status
            .agents
            .iter()
            .any(|agent| { agent.key == "test_agent" && agent.deployed && !agent.installed }));

        run_preset_deployment(&store, "Web Dev", &[], false, false).unwrap();
        tool_service::set_custom_tools(&store, &[test_agent]).unwrap();
        let status = preset_status(&store, "Web Dev", &["test_agent".to_string()]).unwrap();
        assert_eq!(status.agents[0].status, "inactive");
        assert!(!target_root.join("demo").exists());

        store.set_setting("disabled_tools", "[]").unwrap();
        let missing_source = tmp.path().join("central/broken");
        store
            .insert_skill(&SkillRecord {
                id: "skill-broken".to_string(),
                name: "broken".to_string(),
                description: Some("missing source".to_string()),
                source_type: "local".to_string(),
                source_ref: Some(missing_source.to_string_lossy().to_string()),
                source_ref_resolved: None,
                source_subpath: None,
                source_branch: None,
                source_revision: None,
                remote_revision: None,
                central_path: missing_source.to_string_lossy().to_string(),
                content_hash: None,
                enabled: true,
                created_at: 1,
                updated_at: 1,
                status: "ok".to_string(),
                update_status: "local_only".to_string(),
                last_checked_at: None,
                last_check_error: None,
            })
            .unwrap();
        let audit_count = store.list_audit(None).unwrap().len();
        let error = run_skill_deployment(
            &store,
            &["demo".to_string(), "broken".to_string()],
            &["test_agent".to_string()],
            true,
            false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("deployment incomplete"));
        assert!(target_root.join("demo").exists());
        assert!(!target_root.join("broken").exists());
        let audit = store.list_audit(None).unwrap();
        assert_eq!(audit.len(), audit_count + 1);
        assert_eq!(audit[0].action, "deploy");
        assert_eq!(audit[0].skill_id.as_deref(), Some("skill-demo"));
    }
}
