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
        },
        Commands::Agents => commands::cmd_agents(),
        Commands::Agent { action } => match action {
            AgentAction::Info { agent } => commands::cmd_agent_info(&agent),
            AgentAction::AddPack { agent, pack } => commands::cmd_agent_add_pack(&agent, &pack),
            AgentAction::RemovePack { agent, pack } => {
                commands::cmd_agent_remove_pack(&agent, &pack)
            }
        },
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
