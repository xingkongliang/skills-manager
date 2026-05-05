use std::path::{Path, PathBuf};

use app_lib::core::{
    app_state, central_repo, git_backup, scenario_service, sync_engine, tool_service,
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
    Scenarios(ScenarioArgs),
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
    Show { reference: String },
    Export { reference: String, #[arg(long)] dest: PathBuf },
}

#[derive(Args, Debug)]
struct ScenarioArgs {
    #[command(subcommand)]
    command: ScenarioCommand,
}

#[derive(Subcommand, Debug)]
enum ScenarioCommand {
    List,
    Current,
    Preview { reference: String },
    Apply { reference: String },
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
    Clone { url: String },
    SetRemote { url: String },
    Pull,
    Push,
    Commit { #[arg(short, long)] message: String },
    Versions { #[arg(long)] limit: Option<usize> },
    Restore { tag: String },
}

#[derive(Debug, Serialize)]
struct RepoStatus {
    base_dir: String,
    skills_dir: String,
    db_path: String,
    metadata_dir: String,
    skill_count: usize,
    scenario_count: usize,
    active_scenario_id: Option<String>,
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
    scenarios: Vec<String>,
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
struct ScenarioInfo {
    id: String,
    name: String,
    description: Option<String>,
    icon: Option<String>,
    sort_order: i32,
    skill_count: usize,
    active: bool,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if let Some(skills_root) = &cli.skills_root {
        let base = skills_root
            .parent()
            .ok_or_else(|| anyhow::anyhow!("--skills-root requires a non-root path"))?
            .to_path_buf();
        central_repo::set_runtime_base_dir_override(Some(base));
        central_repo::set_runtime_skills_dir_override(Some(skills_root.clone()));
    }

    let store = app_state::initialize_store()?;

    match cli.command {
        Commands::Repo(args) => match args.command {
            RepoCommand::Status => print_json(&repo_status(&store), cli.json),
            RepoCommand::SetPath { path } => {
                central_repo::set_base_dir_override(Some(path))?;
                let store = app_state::initialize_store()?;
                print_json(&repo_status(&store), cli.json)
            }
            RepoCommand::ResetPath => {
                central_repo::set_base_dir_override(None)?;
                let store = app_state::initialize_store()?;
                print_json(&repo_status(&store), cli.json)
            }
        },
        Commands::Tools(args) => match args.command {
            ToolsCommand::List => print_json(&tool_service::list_tool_info(&store), cli.json),
        },
        Commands::Skills(args) => match args.command {
            SkillsCommand::List => print_json(&list_skills(&store)?, cli.json),
            SkillsCommand::Show { reference } => print_json(&show_skill(&store, &reference)?, cli.json),
            SkillsCommand::Export { reference, dest } => {
                let result = export_skill(&store, &reference, &dest)?;
                print_json(&serde_json::json!({"ok": true, "destination": result}), cli.json)
            }
        },
        Commands::Scenarios(args) => match args.command {
            ScenarioCommand::List => print_json(&list_scenarios(&store)?, cli.json),
            ScenarioCommand::Current => print_json(&current_scenario(&store)?, cli.json),
            ScenarioCommand::Preview { reference } => {
                let scenario = resolve_scenario(&store, &reference)?;
                let preview = scenario_service::preview_scenario_sync(&store, &scenario.id)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                print_json(&preview, cli.json)
            }
            ScenarioCommand::Apply { reference } => {
                let scenario = resolve_scenario(&store, &reference)?;
                scenario_service::apply_scenario_to_default(&store, &scenario.id)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                print_json(&current_scenario(&store)?, cli.json)
            }
        },
        Commands::Git(args) => match args.command {
            GitCommand::Status => print_json(&git_backup::get_status(&central_repo::skills_dir())?, cli.json),
            GitCommand::Init => {
                git_backup::init_repo(&central_repo::skills_dir())?;
                print_json(&git_backup::get_status(&central_repo::skills_dir())?, cli.json)
            }
            GitCommand::Clone { url } => {
                git_backup::clone_into(&central_repo::skills_dir(), &url)?;
                print_json(&git_backup::get_status(&central_repo::skills_dir())?, cli.json)
            }
            GitCommand::SetRemote { url } => {
                git_backup::set_remote(&central_repo::skills_dir(), &url)?;
                print_json(&git_backup::get_status(&central_repo::skills_dir())?, cli.json)
            }
            GitCommand::Pull => {
                git_backup::pull(&central_repo::skills_dir())?;
                print_json(&git_backup::get_status(&central_repo::skills_dir())?, cli.json)
            }
            GitCommand::Push => {
                git_backup::push(&central_repo::skills_dir())?;
                print_json(&git_backup::get_status(&central_repo::skills_dir())?, cli.json)
            }
            GitCommand::Commit { message } => {
                git_backup::commit_all(&central_repo::skills_dir(), &message)?;
                let tag = git_backup::create_snapshot_tag(&central_repo::skills_dir())?;
                print_json(&serde_json::json!({"ok": true, "tag": tag}), cli.json)
            }
            GitCommand::Versions { limit } => {
                print_json(&git_backup::list_snapshot_versions(&central_repo::skills_dir(), limit)?, cli.json)
            }
            GitCommand::Restore { tag } => {
                git_backup::restore_snapshot_version(&central_repo::skills_dir(), &tag)?;
                print_json(&git_backup::get_status(&central_repo::skills_dir())?, cli.json)
            }
        },
    }

    Ok(())
}

fn repo_status(store: &app_lib::core::skill_store::SkillStore) -> RepoStatus {
    RepoStatus {
        base_dir: central_repo::base_dir().to_string_lossy().to_string(),
        skills_dir: central_repo::skills_dir().to_string_lossy().to_string(),
        db_path: central_repo::db_path().to_string_lossy().to_string(),
        metadata_dir: app_lib::core::sync_metadata::metadata_dir().to_string_lossy().to_string(),
        skill_count: store.get_all_skills().unwrap_or_default().len(),
        scenario_count: store.get_all_scenarios().unwrap_or_default().len(),
        active_scenario_id: store.get_active_scenario_id().unwrap_or(None),
    }
}

fn list_skills(store: &app_lib::core::skill_store::SkillStore) -> anyhow::Result<Vec<SkillSummary>> {
    let tags_map = store.get_tags_map()?;
    let scenarios = store.get_all_scenarios()?;
    let scenario_lookup: std::collections::HashMap<String, String> = scenarios
        .into_iter()
        .map(|s| (s.id, s.name))
        .collect();

    let mut items = Vec::new();
    for skill in store.get_all_skills()? {
        let scenario_names = store
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
            scenarios: scenario_names,
        });
    }
    Ok(items)
}

fn show_skill(store: &app_lib::core::skill_store::SkillStore, reference: &str) -> anyhow::Result<SkillDetail> {
    let skill = resolve_skill(store, reference)?;

    let summary = list_skills(store)?
        .into_iter()
        .find(|item| item.id == skill.id)
        .ok_or_else(|| anyhow::anyhow!("skill summary missing"))?;

    let skill_dir = PathBuf::from(&skill.central_path);
    let skill_file = [skill_dir.join("SKILL.md"), skill_dir.join("skill.md")]
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| anyhow::anyhow!("no SKILL.md found for {}", skill.name))?;
    let markdown = std::fs::read_to_string(&skill_file)?;

    Ok(SkillDetail {
        summary,
        skill_file: skill_file.to_string_lossy().to_string(),
        files: collect_files(&skill_dir)?,
        markdown,
    })
}

fn export_skill(
    store: &app_lib::core::skill_store::SkillStore,
    reference: &str,
    dest: &Path,
) -> anyhow::Result<String> {
    let skill = resolve_skill(store, reference)?;
    sync_engine::sync_skill(Path::new(&skill.central_path), dest, sync_engine::SyncMode::Copy)?;
    Ok(dest.to_string_lossy().to_string())
}

fn resolve_skill(
    store: &app_lib::core::skill_store::SkillStore,
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
                    .and_then(|value| value.to_str())
                    == Some(reference)
        })
        .collect();

    match matches.len() {
        1 => Ok(matches.into_iter().next().unwrap()),
        0 => Err(anyhow::anyhow!("skill not found: {reference}")),
        _ => Err(anyhow::anyhow!("skill reference is ambiguous: {reference}")),
    }
}

fn list_scenarios(store: &app_lib::core::skill_store::SkillStore) -> anyhow::Result<Vec<ScenarioInfo>> {
    let active = store.get_active_scenario_id()?;
    let scenarios = store.get_all_scenarios()?;
    Ok(scenarios
        .into_iter()
        .map(|scenario| ScenarioInfo {
            skill_count: store.get_skill_ids_for_scenario(&scenario.id).unwrap_or_default().len(),
            active: active.as_deref() == Some(scenario.id.as_str()),
            id: scenario.id,
            name: scenario.name,
            description: scenario.description,
            icon: scenario.icon,
            sort_order: scenario.sort_order,
        })
        .collect())
}

fn current_scenario(store: &app_lib::core::skill_store::SkillStore) -> anyhow::Result<Option<ScenarioInfo>> {
    let scenarios = list_scenarios(store)?;
    Ok(scenarios.into_iter().find(|scenario| scenario.active))
}

fn resolve_scenario(
    store: &app_lib::core::skill_store::SkillStore,
    reference: &str,
) -> anyhow::Result<app_lib::core::skill_store::ScenarioRecord> {
    let scenarios = store.get_all_scenarios()?;
    if reference == "current" {
        let active = store
            .get_active_scenario_id()?
            .ok_or_else(|| anyhow::anyhow!("no active scenario"))?;
        return scenarios
            .into_iter()
            .find(|scenario| scenario.id == active)
            .ok_or_else(|| anyhow::anyhow!("active scenario not found"));
    }
    let matches: Vec<_> = scenarios
        .into_iter()
        .filter(|scenario| scenario.id == reference || scenario.name == reference)
        .collect();
    match matches.len() {
        1 => Ok(matches.into_iter().next().unwrap()),
        0 => Err(anyhow::anyhow!("scenario not found: {reference}")),
        _ => Err(anyhow::anyhow!("scenario reference is ambiguous: {reference}")),
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

fn print_json<T: Serialize>(value: &T, json: bool) {
    let rendered = if json {
        serde_json::to_string(value).unwrap()
    } else {
        serde_json::to_string_pretty(value).unwrap()
    };
    println!("{rendered}");
}
