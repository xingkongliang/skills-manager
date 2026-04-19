mod commands;

use clap::{Parser, Subcommand};

/// sm — Skills Manager CLI
///
/// Manage AI agent skill scenarios from the terminal.
/// Switches scenarios by syncing skills to agent directories.
#[derive(Parser)]
#[command(name = "sm", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all scenarios with skill counts
    #[command(alias = "ls")]
    List,

    /// Show the active scenario
    #[command(alias = "c")]
    Current,

    /// Switch scenario. Without agent name, switches all agents.
    #[command(alias = "sw")]
    Switch {
        /// Scenario name (or agent name when used with second arg)
        name: String,
        /// If provided, first arg is agent name and this is scenario name
        scenario: Option<String>,
    },

    /// List skills in a scenario (default: active)
    #[command(alias = "sk")]
    Skills {
        /// Scenario name (defaults to active scenario)
        name: Option<String>,
    },

    /// Compare two scenarios
    #[command(alias = "d")]
    Diff {
        /// First scenario name
        a: String,
        /// Second scenario name
        b: String,
    },

    /// List packs in a scenario (default: active)
    Packs {
        /// Scenario name (defaults to active scenario)
        name: Option<String>,
    },

    /// Manage packs in a scenario
    Pack {
        #[command(subcommand)]
        action: PackAction,
    },

    /// List agents with their assigned scenarios
    Agents,

    /// Manage a specific agent
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },

    /// Deduplicate agent skill directories against the central store.
    /// Replaces identical copies with symlinks.
    Dedup {
        /// Actually replace copies with symlinks (default: dry run)
        #[arg(long)]
        apply: bool,
        /// Only dedup a specific agent (e.g., claude_code)
        #[arg(long)]
        agent: Option<String>,
    },

    /// Seed default packs (base, gstack, research, etc.)
    SeedPacks {
        /// Re-seed even if packs already exist (deletes old packs first)
        #[arg(long)]
        force: bool,
    },

    /// Import orphan skills from central store that have no DB record
    #[command(alias = "fo")]
    FixOrphans,
}

#[derive(Subcommand)]
enum PackAction {
    /// Add a pack to a scenario
    Add {
        /// Pack name
        pack: String,
        /// Scenario name
        scenario: String,
    },
    /// Remove a pack from a scenario
    Remove {
        /// Pack name
        pack: String,
        /// Scenario name
        scenario: String,
    },
    /// Show pack details (description, router, skills)
    Context {
        /// Pack name
        name: String,
    },
    /// Set or update a pack's router description/body
    SetRouter {
        /// Pack name
        name: String,
        /// New router description (single-line summary)
        #[arg(long)]
        description: Option<String>,
        /// Path to a file whose contents become the router body
        #[arg(long)]
        body: Option<std::path::PathBuf>,
    },
    /// List packs and their router status
    ListRouters,
    /// Write a pending-router-gen marker for a pack
    GenRouter {
        /// Pack name
        name: String,
    },
    /// Write pending-router-gen markers for every non-essential pack
    RegenAllRouters,
    /// Evaluate router-description accuracy against canned queries
    EvalRouters,
}

#[derive(Subcommand)]
enum AgentAction {
    /// Show detailed info about an agent (skills breakdown)
    Info {
        /// Agent key (e.g., claude_code)
        agent: String,
    },
    /// Add an extra pack to an agent
    AddPack {
        /// Agent key (e.g., claude_code)
        agent: String,
        /// Pack name
        pack: String,
    },
    /// Remove an extra pack from an agent
    RemovePack {
        /// Agent key (e.g., claude_code)
        agent: String,
        /// Pack name
        pack: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::List => commands::cmd_list(),
        Commands::Current => commands::cmd_current(),
        Commands::Switch { name, scenario } => commands::cmd_switch(&name, scenario.as_deref()),
        Commands::Skills { name } => commands::cmd_skills(name.as_deref()),
        Commands::Diff { a, b } => commands::cmd_diff(&a, &b),
        Commands::Packs { name } => commands::cmd_packs(name.as_deref()),
        Commands::Pack { action } => match action {
            PackAction::Add { pack, scenario } => commands::cmd_pack_add(&pack, &scenario),
            PackAction::Remove { pack, scenario } => commands::cmd_pack_remove(&pack, &scenario),
            PackAction::Context { name } => commands::cmd_pack_context(&name),
            PackAction::SetRouter {
                name,
                description,
                body,
            } => commands::cmd_pack_set_router(&name, description.as_deref(), body.as_deref()),
            PackAction::ListRouters => commands::cmd_pack_list_routers(),
            PackAction::GenRouter { name } => commands::cmd_pack_gen_router(&name),
            PackAction::RegenAllRouters => commands::cmd_pack_regen_all_routers(),
            PackAction::EvalRouters => commands::cmd_pack_eval_routers(),
        },
        Commands::Agents => commands::cmd_agents(),
        Commands::Agent { action } => match action {
            AgentAction::Info { agent } => commands::cmd_agent_info(&agent),
            AgentAction::AddPack { agent, pack } => commands::cmd_agent_add_pack(&agent, &pack),
            AgentAction::RemovePack { agent, pack } => {
                commands::cmd_agent_remove_pack(&agent, &pack)
            }
        },
        Commands::Dedup { apply, agent } => commands::cmd_dedup(apply, agent.as_deref()),
        Commands::SeedPacks { force } => commands::cmd_seed_packs(force),
        Commands::FixOrphans => commands::cmd_fix_orphans(),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
