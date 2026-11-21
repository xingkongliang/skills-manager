use std::io;
use std::path::PathBuf;

use app_lib::core::{app_state, central_repo, host::HostCtx, mcp_server, serve};
use clap::{Args, Parser, Subcommand};

use crate::args::{GitArgs, PresetArgs, RepoArgs, RepoCommand, SkillsArgs, SyncArgs, ToolsArgs};
use crate::output::{error_envelope, print_json};
use crate::presets::run_presets;
use crate::repo::{run_git, run_repo};
use crate::skills::run_skills;
use crate::sync::run_sync;
use crate::tools::run_tools;

mod args;
mod output;
mod presets;
mod repo;
mod reports;
mod resources;
mod skills;
mod sync;
mod tools;

#[derive(Parser, Debug)]
#[command(name = "skills-manager-cli")]
#[command(
    about = "Agents Manager CLI for skills, instructions and MCP configuration",
    version
)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    skills_root: Option<PathBuf>,
    /// Use this folder as the whole Skills Manager base (library, database,
    /// settings) instead of the configured one. For hermetic tests.
    #[arg(long, global = true, hide = true, conflicts_with = "skills_root")]
    base_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Repo(RepoArgs),
    #[command(name = "agents", visible_alias = "tools")]
    Tools(ToolsArgs),
    Skills(SkillsArgs),
    /// Manage instruction files, bundles and reviewed deployments.
    Instructions(resources::ResourceArgs),
    /// Manage MCP definitions and agent configurations.
    Mcps(resources::ResourceArgs),
    #[command(alias = "scenarios")]
    Presets(PresetArgs),
    /// Share this host's library and presets with other hosts through the
    /// Sync remote.
    Sync(SyncArgs),
    /// The old step-by-step commands, now under `sync`.
    #[command(hide = true)]
    Git(GitArgs),
    /// Answer the app's commands over stdin/stdout, for a Skills Manager app
    /// on another machine connected over ssh.
    Serve(ServeArgs),
    /// Run a local MCP server on stdin/stdout for coding agents. It opens the
    /// library read-only.
    Mcp,
}

#[derive(Args, Debug)]
struct ServeArgs {
    /// Speak the protocol on stdin/stdout (the only transport).
    #[arg(long, required = true)]
    stdio: bool,
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
    if let Commands::Repo(RepoArgs {
        command: command @ (RepoCommand::SetPath { .. } | RepoCommand::ResetPath),
    }) = &cli.command
    {
        if cli.skills_root.is_some() || cli.base_dir.is_some() {
            anyhow::bail!(
                "repo set-path / reset-path cannot be combined with --skills-root or --base-dir"
            );
        }
        let path = match command {
            RepoCommand::SetPath { path } => Some(path.clone()),
            RepoCommand::ResetPath => None,
            RepoCommand::Status => unreachable!(),
        };
        central_repo::set_base_dir_override(path)?;
        let store = app_state::initialize_cli_store_moving_repo()?;
        print_json(&crate::repo::repo_status(&store), cli.json);
        return Ok(());
    }

    if let Some(skills_root) = &cli.skills_root {
        let base = central_repo::external_base_dir(skills_root);
        central_repo::set_runtime_base_dir_override(Some(base));
        central_repo::set_runtime_skills_dir_override(Some(skills_root.clone()));
    }
    if let Some(base_dir) = &cli.base_dir {
        central_repo::set_runtime_base_dir_override(Some(base_dir.clone()));
    }

    let read_only = matches!(&cli.command,
        Commands::Instructions(args) | Commands::Mcps(args) if args.dry_run)
        || matches!(&cli.command, Commands::Mcp);
    let store = if read_only {
        std::sync::Arc::new(
            app_lib::core::skill_store::SkillStore::open_resource_read_only(
                &central_repo::db_path(),
            )?,
        )
    } else if matches!(&cli.command, Commands::Serve(_)) {
        app_state::initialize_cli_store_moving_repo()?
    } else {
        app_state::initialize_cli_store()?
    };

    match cli.command {
        Commands::Repo(args) => run_repo(args, &store, cli.json),
        Commands::Tools(args) => run_tools(args, &store, cli.json),
        Commands::Skills(args) => run_skills(args, &store, cli.json),
        Commands::Instructions(args) => resources::run("instructions", args, store, cli.json),
        Commands::Mcps(args) => resources::run("mcps", args, store, cli.json),
        Commands::Presets(args) => run_presets(args, &store, cli.json),
        Commands::Sync(args) => run_sync(args, &store, cli.json),
        Commands::Git(args) => run_git(args, &store, cli.json),
        // stdout carries the protocol; nothing else may print there.
        Commands::Serve(_) => Ok(serve::serve_stdio(store)?),
        Commands::Mcp => {
            // Reading Sync status runs `git status`, which may otherwise
            // refresh the library's index file.
            std::env::set_var("GIT_OPTIONAL_LOCKS", "0");
            let ctx = HostCtx::headless(store);
            Ok(mcp_server::run(
                io::stdin().lock(),
                io::stdout().lock(),
                &ctx,
            )?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::{PresetCommand, SkillsCommand, ToolsCommand};
    use app_lib::core::{skill_store::SkillStore, tool_service};
    use tempfile::tempdir;

    #[test]
    fn agents_add_custom_registers_the_agent_and_rejects_a_duplicate() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let skills = tmp.path().join("hermes-work/skills");
        let args = |key: &str| match Cli::try_parse_from([
            "skills-manager-cli",
            "agents",
            "add-custom",
            key,
            "--path",
            skills.to_str().unwrap(),
            "--name",
            "Hermes Work",
            "--project-path",
            ".hermes/skills",
        ])
        .unwrap()
        .command
        {
            Commands::Tools(args) => args,
            other => panic!("unexpected command {other:?}"),
        };

        run_tools(args("hermes-work"), &store, true).unwrap();
        let custom = tool_service::get_custom_tools(&store);
        assert_eq!(custom.len(), 1);
        assert_eq!(custom[0].key, "hermes-work");
        assert_eq!(custom[0].display_name, "Hermes Work");
        assert_eq!(
            custom[0].project_relative_skills_dir.as_deref(),
            Some(".hermes/skills")
        );

        assert!(run_tools(args("hermes-work"), &store, true).is_err());
        assert_eq!(tool_service::get_custom_tools(&store).len(), 1);
    }

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
}
