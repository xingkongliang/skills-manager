use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::{Context, anyhow, bail};
use app_lib::commands::{presets as preset_cmd, skills as cmd, tools as tool_cmd};
use app_lib::core::{
    app_state, audit_log::AuditDraft, central_repo, error::AppError, git_backup, git_fetcher,
    installer, merge, repo_lock::RepoLock, scenario_service, skill_metadata,
    skill_store::SkillStore, skillssh_api, sync_engine, sync_metadata, tool_adapters, tool_service,
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
    Skills(SkillsArgs),
    #[command(alias = "scenarios")]
    Presets(PresetArgs),
    Git(GitArgs),
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
            let message = format!("{err:#}");
            let envelope = serde_json::json!({
                "ok": false,
                "code": "COMMAND_FAILED",
                "message": message,
                "error": message,
            });
            eprintln!("{}", serde_json::to_string(&envelope).unwrap());
        } else {
            eprintln!("error: {err:#}");
        }
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    if let Some(skills_root) = &cli.skills_root {
        let base = central_repo::external_base_dir(skills_root);
        central_repo::set_runtime_base_dir_override(Some(base));
        central_repo::set_runtime_skills_dir_override(Some(skills_root.clone()));
    }

    let store = app_state::initialize_cli_store()?;

    match cli.command {
        Commands::Repo(args) => run_repo(args, &store, cli.json),
        Commands::Tools(args) => run_tools(args, &store, cli.json),
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
    let temp_dir = git_fetcher::clone_repo_ref(
        &parsed.clone_url,
        parsed.branch.as_deref(),
        Some(&cancel),
        proxy_url.as_deref(),
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
                match cmd::update_git_skill_internal(store, &skill.id, proxy_url.as_deref(), None) {
                    Ok(r) => UpdateReport {
                        skill_id: skill.id.clone(),
                        name: skill.name.clone(),
                        source_type: skill.source_type.clone(),
                        refreshed: r.content_changed,
                        error: None,
                    },
                    Err(e) => UpdateReport {
                        skill_id: skill.id.clone(),
                        name: skill.name.clone(),
                        source_type: skill.source_type.clone(),
                        refreshed: false,
                        error: Some(e.message.clone()),
                    },
                }
            }
            "local" | "import" => match cmd::reimport_local_skill_internal(store, &skill.id) {
                Ok(_) => UpdateReport {
                    skill_id: skill.id.clone(),
                    name: skill.name.clone(),
                    source_type: skill.source_type.clone(),
                    refreshed: true,
                    error: None,
                },
                Err(e) => UpdateReport {
                    skill_id: skill.id.clone(),
                    name: skill.name.clone(),
                    source_type: skill.source_type.clone(),
                    refreshed: false,
                    error: Some(e.message.clone()),
                },
            },
            other => UpdateReport {
                skill_id: skill.id.clone(),
                name: skill.name.clone(),
                source_type: skill.source_type.clone(),
                refreshed: false,
                error: Some(format!("source type '{other}' cannot be refreshed")),
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

fn map_app_err(e: AppError) -> anyhow::Error {
    anyhow!(e.message)
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
    use super::*;
    use app_lib::core::skill_store::{ScenarioRecord, SkillRecord};
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
