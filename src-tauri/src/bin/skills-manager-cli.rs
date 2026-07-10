use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::{anyhow, bail};
use app_lib::commands::{projects as project_cmd, skills as cmd};
use app_lib::core::{
    app_state, central_repo, error::AppError, git_backup, git_fetcher, installer, merge,
    repo_lock::RepoLock, scenario_service, skill_metadata, skill_store::SkillStore, skillssh_api,
    sync_engine, sync_metadata, tool_adapters, tool_service,
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
    Tools(ToolsArgs),
    Skills(SkillsArgs),
    #[command(alias = "scenarios")]
    Presets(PresetArgs),
    Projects(ProjectArgs),
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
}

#[derive(Args, Debug)]
struct SkillsArgs {
    #[command(subcommand)]
    command: SkillsCommand,
}

#[derive(Subcommand, Debug)]
enum SkillsCommand {
    List,
    Show {
        reference: String,
    },
    Export {
        reference: String,
        #[arg(long)]
        dest: PathBuf,
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
    /// Deprecated no-op: use presets add-skill to enable a skill in a preset.
    Enable {
        references: Vec<String>,
    },
    /// Deprecated no-op: use presets remove-skill to disable a skill in a preset.
    Disable {
        references: Vec<String>,
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
    Preview {
        reference: String,
    },
    #[command(alias = "activate", alias = "enable", alias = "start", alias = "open")]
    Apply {
        reference: String,
    },
    #[command(alias = "disable", alias = "stop", alias = "close", alias = "off")]
    Deactivate {
        reference: String,
    },
    AddSkill {
        preset: String,
        skills: Vec<String>,
    },
    RemoveSkill {
        preset: String,
        skills: Vec<String>,
    },
}

#[derive(Args, Debug)]
struct ProjectArgs {
    #[command(subcommand)]
    command: ProjectCommand,
}

#[derive(Subcommand, Debug)]
enum ProjectCommand {
    List,
    Add {
        path: String,
    },
    Scan {
        root: String,
    },
    Targets {
        project: String,
    },
    Skills {
        project: String,
        /// Agent/tool key filter, e.g. codex
        #[arg(long, alias = "agent")]
        tool: Option<String>,
    },
    ApplyPreset {
        project: String,
        preset: String,
        /// Agent/tool key. Repeat to target more than one agent.
        #[arg(long, alias = "agent")]
        tool: Vec<String>,
        #[arg(long)]
        dry_run: bool,
    },
    #[command(alias = "unlink-preset")]
    RemovePreset {
        project: String,
        preset: String,
        /// Agent/tool key. Repeat to target more than one agent.
        #[arg(long, alias = "agent")]
        tool: Vec<String>,
        #[arg(long)]
        dry_run: bool,
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
    presets: Vec<String>,
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
struct PresetMembershipReport {
    preset_id: String,
    preset_name: String,
    added: Vec<String>,
    removed: Vec<String>,
    missing: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ProjectPresetTarget {
    skill_id: String,
    skill_name: String,
    tool: String,
    target_path: Option<String>,
    action: String,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProjectPresetReport {
    ok: bool,
    project_id: String,
    project_name: String,
    project_path: String,
    preset_id: String,
    preset_name: String,
    dry_run: bool,
    persistent_link: bool,
    added: usize,
    removed: usize,
    skipped: usize,
    failed: usize,
    targets: Vec<ProjectPresetTarget>,
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
                let envelope = serde_json::json!({"ok": false, "error": e.to_string()});
                eprintln!("{}", serde_json::to_string(&envelope).unwrap());
                std::process::exit(2);
            }
            e.exit();
        }
    };

    if let Err(err) = run(cli) {
        if json {
            let envelope = serde_json::json!({"ok": false, "error": format!("{err:#}")});
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
        Commands::Projects(args) => run_projects(args, &store, cli.json),
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
    }
    Ok(())
}

// ── skills ────────────────────────────────────────────────────────────────

fn run_skills(args: SkillsArgs, store: &SkillStore, json: bool) -> anyhow::Result<()> {
    match args.command {
        SkillsCommand::List => print_json(&list_skills(store)?, json),
        SkillsCommand::Show { reference } => print_json(&show_skill(store, &reference)?, json),
        SkillsCommand::Export { reference, dest } => {
            let result = export_skill(store, &reference, &dest)?;
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
    let scenarios = store.get_all_scenarios()?;
    let scenario_lookup: std::collections::HashMap<String, String> =
        scenarios.into_iter().map(|s| (s.id, s.name)).collect();

    let mut items = Vec::new();
    for skill in store.get_all_skills()? {
        let preset_names = store
            .get_scenarios_for_skill(&skill.id)?
            .into_iter()
            .filter_map(|id| scenario_lookup.get(&id).cloned())
            .collect();
        items.push(SkillSummary {
            id: skill.id.clone(),
            name: skill.name.clone(),
            description: skill.description.clone(),
            path: skill.central_path.clone(),
            enabled: skill.enabled,
            tags: tags_map.get(&skill.id).cloned().unwrap_or_default(),
            source_type: skill.source_type.clone(),
            source_ref: skill.source_ref.clone(),
            presets: preset_names,
        });
    }
    Ok(items)
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

fn export_skill(store: &SkillStore, reference: &str, dest: &Path) -> anyhow::Result<String> {
    let skill = resolve_skill(store, reference)?;
    sync_engine::sync_skill(
        Path::new(&skill.central_path),
        dest,
        sync_engine::SyncMode::Copy,
    )?;
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
    } else {
        // No ref → treat as --all (the flag is just explicit confirmation)
        let _ = all;
        Ok(store.get_all_skills()?)
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
            "Deprecated no-op: skills are enabled by adding them to a preset; this command only restores legacy sync inclusion."
        } else {
            "Deprecated no-op: skills are disabled by removing them from a preset; this command does not modify the legacy enabled flag."
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
        scenario_service::sync_desired_targets(store, &desired).map_err(map_app_err)?;
    } else {
        scenario_service::apply_scenario_to_default(store, &preset.id).map_err(map_app_err)?;
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
                if !current.iter().any(|c| c == &t) {
                    current.push(t);
                }
            }
            store.set_tags_for_skill(&skill.id, &current)?;
            sync_metadata::ensure_skill_metadata(store, &skill.id)?;
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
            current.retain(|c| !tags.iter().any(|t| t == c));
            store.set_tags_for_skill(&skill.id, &current)?;
            sync_metadata::ensure_skill_metadata(store, &skill.id)?;
            print_json(
                &TagReport {
                    skill_id: skill.id,
                    name: skill.name,
                    tags: current,
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
        PresetCommand::Preview { reference } => {
            let preset = resolve_scenario(store, &reference)?;
            let preview =
                scenario_service::preview_scenario_sync(store, &preset.id).map_err(map_app_err)?;
            print_json(&preview, json);
        }
        PresetCommand::Apply { reference } => {
            let preset = resolve_scenario(store, &reference)?;
            scenario_service::apply_scenario_to_default(store, &preset.id).map_err(map_app_err)?;
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
                    scenario_service::apply_scenario_to_default(store, &next.id)
                        .map_err(map_app_err)?;
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
                    scenario_service::sync_scenario_skills(store, active_id)
                        .map_err(map_app_err)?;
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
        PresetCommand::AddSkill { preset, skills } => {
            let s = resolve_scenario(store, &preset)?;
            let mut added = Vec::new();
            let mut missing = Vec::new();
            for r in skills {
                match resolve_skill(store, &r) {
                    Ok(skill) => {
                        store.add_skill_to_scenario(&s.id, &skill.id)?;
                        added.push(skill.name);
                    }
                    Err(_) => missing.push(r),
                }
            }
            sync_metadata::write_all_from_db(store)?;
            print_json(
                &PresetMembershipReport {
                    preset_id: s.id,
                    preset_name: s.name,
                    added,
                    removed: Vec::new(),
                    missing,
                },
                json,
            );
        }
        PresetCommand::RemoveSkill { preset, skills } => {
            let s = resolve_scenario(store, &preset)?;
            let mut removed = Vec::new();
            let mut missing = Vec::new();
            for r in skills {
                match resolve_skill(store, &r) {
                    Ok(skill) => {
                        store.remove_skill_from_scenario(&s.id, &skill.id)?;
                        removed.push(skill.name);
                    }
                    Err(_) => missing.push(r),
                }
            }
            sync_metadata::write_all_from_db(store)?;
            print_json(
                &PresetMembershipReport {
                    preset_id: s.id,
                    preset_name: s.name,
                    added: Vec::new(),
                    removed,
                    missing,
                },
                json,
            );
        }
    }
    Ok(())
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
    if let Some(default_id) = store.get_setting("default_scenario")? {
        if default_id != deactivated_id {
            if let Some(default) = scenarios.iter().find(|scenario| scenario.id == default_id) {
                return Ok(Some(default.clone()));
            }
        }
    }

    Ok(scenarios
        .into_iter()
        .find(|scenario| scenario.id != deactivated_id))
}

// ── projects ──────────────────────────────────────────────────────────────

fn run_projects(args: ProjectArgs, store: &SkillStore, json: bool) -> anyhow::Result<()> {
    match args.command {
        ProjectCommand::List => print_json(
            &project_cmd::get_projects_internal(store).map_err(map_app_err)?,
            json,
        ),
        ProjectCommand::Add { path } => {
            let project = project_cmd::add_project_internal(store, path).map_err(map_app_err)?;
            print_json(&project, json);
        }
        ProjectCommand::Scan { root } => {
            let projects =
                project_cmd::scan_projects_internal(store, &root).map_err(map_app_err)?;
            print_json(&projects, json);
        }
        ProjectCommand::Targets { project } => {
            let p = resolve_project(store, &project)?;
            let targets =
                project_cmd::project_agent_targets_internal(store, &p.id).map_err(map_app_err)?;
            print_json(&targets, json);
        }
        ProjectCommand::Skills { project, tool } => {
            let p = resolve_project(store, &project)?;
            let mut skills =
                project_cmd::get_project_skills_internal(store, &p.id).map_err(map_app_err)?;
            if let Some(tool) = tool {
                skills.retain(|skill| skill.agent == tool);
            }
            print_json(&skills, json);
        }
        ProjectCommand::ApplyPreset {
            project,
            preset,
            tool,
            dry_run,
        } => {
            let report = apply_project_preset(store, &project, &preset, &tool, dry_run, true)?;
            print_json(&report, json);
        }
        ProjectCommand::RemovePreset {
            project,
            preset,
            tool,
            dry_run,
        } => {
            let report = apply_project_preset(store, &project, &preset, &tool, dry_run, false)?;
            print_json(&report, json);
        }
    }
    Ok(())
}

fn resolve_project(store: &SkillStore, reference: &str) -> anyhow::Result<project_cmd::ProjectDto> {
    let projects = project_cmd::get_projects_internal(store).map_err(map_app_err)?;
    let direct_matches: Vec<_> = projects
        .into_iter()
        .filter(|project| {
            project.id == reference || project.name == reference || project.path == reference
        })
        .collect();
    if direct_matches.len() == 1 {
        return Ok(direct_matches.into_iter().next().unwrap());
    }
    if direct_matches.len() > 1 {
        bail!("ambiguous project ref '{}'", reference);
    }

    let ref_path = Path::new(reference);
    if let Ok(ref_canon) = ref_path.canonicalize() {
        let matches: Vec<_> = project_cmd::get_projects_internal(store)
            .map_err(map_app_err)?
            .into_iter()
            .filter(|project| {
                Path::new(&project.path)
                    .canonicalize()
                    .map(|path| path == ref_canon)
                    .unwrap_or(false)
            })
            .collect();
        if matches.len() == 1 {
            return Ok(matches.into_iter().next().unwrap());
        }
        if matches.len() > 1 {
            bail!("ambiguous project path '{}'", reference);
        }
    }

    Err(anyhow!("project not found: {}", reference))
}

fn selected_project_tools(
    store: &SkillStore,
    project: &project_cmd::ProjectDto,
    requested: &[String],
) -> anyhow::Result<Vec<String>> {
    let targets =
        project_cmd::project_agent_targets_internal(store, &project.id).map_err(map_app_err)?;
    let available: std::collections::HashSet<String> = targets
        .iter()
        .filter(|target| target.installed && target.enabled)
        .map(|target| target.key.clone())
        .collect();

    let selected = if requested.is_empty() {
        available.into_iter().collect::<Vec<_>>()
    } else {
        let mut missing = Vec::new();
        let mut selected = Vec::new();
        for tool in requested {
            if available.contains(tool) {
                selected.push(tool.clone());
            } else {
                missing.push(tool.clone());
            }
        }
        if !missing.is_empty() {
            bail!(
                "project target is not available/enabled: {}",
                missing.join(", ")
            );
        }
        selected
    };

    if selected.is_empty() {
        bail!("no enabled installed project targets");
    }
    Ok(selected)
}

fn project_target_path(
    store: &SkillStore,
    project: &project_cmd::ProjectDto,
    tool: &str,
    skill_name: &str,
) -> Option<String> {
    let dir_name = project_cmd::slugify_skill_names(vec![skill_name.to_string()])
        .into_iter()
        .next()
        .unwrap_or_else(|| "skill".to_string());
    if project.workspace_type == "linked" {
        return Some(
            Path::new(&project.path)
                .join(dir_name)
                .to_string_lossy()
                .to_string(),
        );
    }
    let adapter = tool_adapters::find_adapter_with_store(store, tool)?;
    Some(
        Path::new(&project.path)
            .join(adapter.project_relative_skills_dir())
            .join(dir_name)
            .to_string_lossy()
            .to_string(),
    )
}

fn apply_project_preset(
    store: &SkillStore,
    project_ref: &str,
    preset_ref: &str,
    requested_tools: &[String],
    dry_run: bool,
    add: bool,
) -> anyhow::Result<ProjectPresetReport> {
    let project = resolve_project(store, project_ref)?;
    let preset = resolve_scenario(store, preset_ref)?;
    let tools = selected_project_tools(store, &project, requested_tools)?;
    let preset_skill_ids = store.get_skill_ids_for_scenario(&preset.id)?;
    let project_skills =
        project_cmd::get_project_skills_internal(store, &project.id).map_err(map_app_err)?;

    let mut targets = Vec::new();
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for skill_id in preset_skill_ids {
        let Some(skill) = store.get_skill_by_id(&skill_id)? else {
            continue;
        };
        for tool in &tools {
            let existing = project_skills.iter().find(|project_skill| {
                project_skill.agent == *tool
                    && project_skill.center_skill_id.as_deref() == Some(skill.id.as_str())
            });
            let target_path = existing
                .map(|project_skill| project_skill.path.clone())
                .or_else(|| project_target_path(store, &project, tool, &skill.name));

            if add {
                if existing.is_some() {
                    skipped += 1;
                    targets.push(ProjectPresetTarget {
                        skill_id: skill.id.clone(),
                        skill_name: skill.name.clone(),
                        tool: tool.clone(),
                        target_path,
                        action: "skipped_existing".to_string(),
                        error: None,
                    });
                    continue;
                }
                if dry_run {
                    // A real run would reject a directory that already exists
                    // but isn't a matched central skill (the `existing` check
                    // above only sees tracked skills). Preview that failure so
                    // dry-run and the real run agree.
                    if let Some(conflict) = project_cmd::project_export_conflict_path(
                        store,
                        &project.id,
                        tool,
                        &skill.name,
                    )
                    .map_err(map_app_err)?
                    {
                        failed += 1;
                        targets.push(ProjectPresetTarget {
                            skill_id: skill.id.clone(),
                            skill_name: skill.name.clone(),
                            tool: tool.clone(),
                            target_path: Some(conflict),
                            action: "would_fail".to_string(),
                            error: Some(format!(
                                "Skill \"{}\" already exists in this workspace for agent {}",
                                skill.name, tool
                            )),
                        });
                        continue;
                    }
                    added += 1;
                    targets.push(ProjectPresetTarget {
                        skill_id: skill.id.clone(),
                        skill_name: skill.name.clone(),
                        tool: tool.clone(),
                        target_path,
                        action: "would_add".to_string(),
                        error: None,
                    });
                    continue;
                }
                match project_cmd::export_skill_to_project_internal(
                    store,
                    &skill.id,
                    &project.id,
                    Some(vec![tool.clone()]),
                ) {
                    Ok(()) => {
                        added += 1;
                        targets.push(ProjectPresetTarget {
                            skill_id: skill.id.clone(),
                            skill_name: skill.name.clone(),
                            tool: tool.clone(),
                            target_path,
                            action: "added".to_string(),
                            error: None,
                        });
                    }
                    Err(err) => {
                        failed += 1;
                        targets.push(ProjectPresetTarget {
                            skill_id: skill.id.clone(),
                            skill_name: skill.name.clone(),
                            tool: tool.clone(),
                            target_path,
                            action: "failed".to_string(),
                            error: Some(err.message),
                        });
                    }
                }
            } else if let Some(existing) = existing {
                if dry_run {
                    removed += 1;
                    targets.push(ProjectPresetTarget {
                        skill_id: skill.id.clone(),
                        skill_name: skill.name.clone(),
                        tool: tool.clone(),
                        target_path,
                        action: "would_remove".to_string(),
                        error: None,
                    });
                    continue;
                }
                match project_cmd::delete_project_skill_internal(
                    store,
                    &project.id,
                    &existing.relative_path,
                    tool,
                ) {
                    Ok(()) => {
                        removed += 1;
                        targets.push(ProjectPresetTarget {
                            skill_id: skill.id.clone(),
                            skill_name: skill.name.clone(),
                            tool: tool.clone(),
                            target_path,
                            action: "removed".to_string(),
                            error: None,
                        });
                    }
                    Err(err) => {
                        failed += 1;
                        targets.push(ProjectPresetTarget {
                            skill_id: skill.id.clone(),
                            skill_name: skill.name.clone(),
                            tool: tool.clone(),
                            target_path,
                            action: "failed".to_string(),
                            error: Some(err.message),
                        });
                    }
                }
            } else {
                skipped += 1;
                targets.push(ProjectPresetTarget {
                    skill_id: skill.id.clone(),
                    skill_name: skill.name.clone(),
                    tool: tool.clone(),
                    target_path,
                    action: "skipped_missing".to_string(),
                    error: None,
                });
            }
        }
    }

    Ok(ProjectPresetReport {
        ok: failed == 0,
        project_id: project.id,
        project_name: project.name,
        project_path: project.path,
        preset_id: preset.id,
        preset_name: preset.name,
        dry_run,
        persistent_link: false,
        added,
        removed,
        skipped,
        failed,
        targets,
    })
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
    use app_lib::core::skill_store::{ProjectRecord, ScenarioRecord, SkillRecord};
    use std::fs;
    use tempfile::tempdir;

    /// Backs the store, a preset holding one central skill, and a linked
    /// workspace whose single agent target is always installed + enabled —
    /// so preset application is deterministic without touching the real
    /// central repo or system-installed tools.
    struct Fixture {
        _tmp: tempfile::TempDir,
        store: SkillStore,
        project_path: PathBuf,
        project_id: String,
        preset_id: String,
    }

    fn setup() -> Fixture {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();

        // Central skill referenced by the preset.
        let central = tmp.path().join("central").join("preset-skill");
        fs::create_dir_all(&central).unwrap();
        fs::write(central.join("SKILL.md"), "# Preset Skill\n").unwrap();
        store
            .insert_skill(&SkillRecord {
                id: "skill-1".into(),
                name: "Preset Skill".into(),
                description: None,
                source_type: "local".into(),
                source_ref: None,
                source_ref_resolved: None,
                source_subpath: None,
                source_branch: None,
                source_revision: None,
                remote_revision: None,
                central_path: central.to_string_lossy().to_string(),
                content_hash: None,
                enabled: true,
                created_at: 0,
                updated_at: 0,
                status: "ok".into(),
                update_status: "local_only".into(),
                last_checked_at: None,
                last_check_error: None,
            })
            .unwrap();

        // Preset (scenario) containing that skill.
        store
            .insert_scenario(&ScenarioRecord {
                id: "preset-1".into(),
                name: "Preset One".into(),
                description: None,
                icon: None,
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            })
            .unwrap();
        store.add_skill_to_scenario("preset-1", "skill-1").unwrap();

        // Linked workspace: its lone agent target reports installed + enabled.
        let project_path = tmp.path().join("workspace");
        let disabled_path = tmp.path().join("workspace-disabled");
        fs::create_dir_all(&project_path).unwrap();
        fs::create_dir_all(&disabled_path).unwrap();
        store
            .insert_project(&ProjectRecord {
                id: "project-1".into(),
                name: "Workspace One".into(),
                path: project_path.to_string_lossy().to_string(),
                workspace_type: "linked".into(),
                linked_agent_key: Some("agent".into()),
                linked_agent_name: Some("Agent".into()),
                disabled_path: Some(disabled_path.to_string_lossy().to_string()),
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            })
            .unwrap();

        Fixture {
            _tmp: tmp,
            store,
            project_path,
            project_id: "project-1".into(),
            preset_id: "preset-1".into(),
        }
    }

    #[test]
    fn resolve_project_matches_by_id_name_and_path() {
        let fx = setup();
        let path_string = fx.project_path.to_string_lossy().to_string();
        for reference in [fx.project_id.as_str(), "Workspace One", path_string.as_str()] {
            let project = resolve_project(&fx.store, reference).unwrap();
            assert_eq!(project.id, fx.project_id, "ref {reference}");
        }
    }

    #[test]
    fn resolve_project_errors_when_missing() {
        let fx = setup();
        let err = match resolve_project(&fx.store, "does-not-exist") {
            Ok(_) => panic!("expected a missing-project error"),
            Err(err) => err,
        };
        assert!(
            err.to_string().contains("project not found"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn apply_project_preset_dry_run_add_previews_new_skill() {
        let fx = setup();
        let report =
            apply_project_preset(&fx.store, &fx.project_id, &fx.preset_id, &[], true, true).unwrap();
        assert!(report.ok);
        assert!(report.dry_run);
        assert_eq!(report.added, 1);
        assert_eq!(report.failed, 0);
        assert_eq!(report.targets.len(), 1);
        assert_eq!(report.targets[0].action, "would_add");
    }

    #[test]
    fn apply_project_preset_dry_run_add_flags_existing_directory_conflict() {
        let fx = setup();
        // A directory of the same slug already exists but is not a tracked
        // central skill (no SKILL.md, so it is never read as a project skill).
        // A real run would fail with "already exists"; the dry run must agree
        // instead of optimistically reporting "would_add".
        fs::create_dir_all(fx.project_path.join("preset-skill")).unwrap();

        let report =
            apply_project_preset(&fx.store, &fx.project_id, &fx.preset_id, &[], true, true).unwrap();
        assert!(!report.ok);
        assert_eq!(report.added, 0);
        assert_eq!(report.failed, 1);
        assert_eq!(report.targets.len(), 1);
        assert_eq!(report.targets[0].action, "would_fail");
        assert!(report.targets[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("already exists"));
    }
}
