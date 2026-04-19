use anyhow::{bail, Context, Result};
use skills_manager_core::skill_store::SkillStore;
use skills_manager_core::{
    central_repo, dedup, pack_seeder, sync_engine, tool_adapters, ScenarioRecord,
};
use std::path::PathBuf;

// ── Helpers ──────────────────────────────────────────────

fn open_store() -> Result<SkillStore> {
    let db_path = central_repo::db_path();
    if !db_path.exists() {
        bail!("Skills Manager DB not found at {}", db_path.display());
    }
    SkillStore::new(&db_path).context("Failed to open Skills Manager database")
}

/// Find a scenario by name (case-insensitive).
fn find_scenario_by_name(store: &SkillStore, name: &str) -> Result<ScenarioRecord> {
    let scenarios = store.get_all_scenarios()?;
    let lower = name.to_lowercase();
    if let Some(s) = scenarios.iter().find(|s| s.name.to_lowercase() == lower) {
        return Ok(s.clone());
    }
    let available: Vec<&str> = scenarios.iter().map(|s| s.name.as_str()).collect();
    bail!(
        "Scenario '{}' not found. Available: {}",
        name,
        available.join(", ")
    );
}

/// Get the active scenario, or bail if none is set.
fn get_active_scenario(store: &SkillStore) -> Result<ScenarioRecord> {
    let active_id = store
        .get_active_scenario_id()?
        .context("No active scenario set")?;
    let scenarios = store.get_all_scenarios()?;
    scenarios
        .into_iter()
        .find(|s| s.id == active_id)
        .context("Active scenario not found in database")
}

/// Resolve a scenario: if name is given, look it up; otherwise use active.
fn resolve_scenario(store: &SkillStore, name: Option<&str>) -> Result<ScenarioRecord> {
    match name {
        Some(n) => find_scenario_by_name(store, n),
        None => get_active_scenario(store),
    }
}

// ── Commands ─────────────────────────────────────────────

pub fn cmd_list() -> Result<()> {
    let store = open_store()?;
    let scenarios = store.get_all_scenarios()?;
    let active_id = store.get_active_scenario_id()?.unwrap_or_default();

    println!("Scenarios:");
    for s in &scenarios {
        let count = store.get_effective_skills_for_scenario(&s.id)?.len();
        let marker = if s.id == active_id { ">" } else { " " };
        println!("  {} {:<24} {:>3} skills", marker, s.name, count);
    }

    Ok(())
}

pub fn cmd_current() -> Result<()> {
    let store = open_store()?;
    let scenario = get_active_scenario(&store)?;
    let count = store.get_effective_skills_for_scenario(&scenario.id)?.len();
    println!("{} ({} skills)", scenario.name, count);
    Ok(())
}

pub fn cmd_switch(name: &str, scenario: Option<&str>) -> Result<()> {
    let store = open_store()?;

    match scenario {
        None => {
            // Global switch: switch ALL managed agents to the named scenario.
            let target = find_scenario_by_name(&store, name)?;

            let current_name = get_active_scenario(&store)
                .map(|s| s.name)
                .unwrap_or_else(|_| "none".to_string());

            println!("Switching all agents: {} -> {}", current_name, target.name);

            let adapters = tool_adapters::enabled_installed_adapters(&store);
            let configured_mode = store.get_setting("sync_mode").ok().flatten();

            if let Ok(Some(old_id)) = store.get_active_scenario_id() {
                if old_id != target.id {
                    unsync_scenario(&store, &old_id, &adapters, configured_mode.as_deref())?;
                }
            }

            store.set_active_scenario(&target.id)?;

            // Also update all managed agent configs.
            let agent_configs = store.get_all_agent_configs()?;
            for config in &agent_configs {
                if config.managed {
                    store.set_agent_scenario(&config.tool_key, &target.id)?;
                }
            }

            let synced_per_adapter =
                sync_scenario(&store, &target.id, &adapters, configured_mode.as_deref())?;
            for (display_name, count) in synced_per_adapter {
                println!("  + {} ({} skills)", display_name, count);
            }

            println!("Done. Active: {}", target.name);
        }
        Some(scenario_name) => {
            // Per-agent switch: `name` is the agent key, `scenario_name` is the scenario.
            let agent_key = name;
            let target = find_scenario_by_name(&store, scenario_name)?;

            // Validate agent key exists.
            let all_configs = store.get_all_agent_configs()?;
            let agent_config = all_configs
                .iter()
                .find(|c| c.tool_key == agent_key)
                .ok_or_else(|| {
                    let available: Vec<&str> =
                        all_configs.iter().map(|c| c.tool_key.as_str()).collect();
                    anyhow::anyhow!(
                        "Agent '{}' not found. Available: {}",
                        agent_key,
                        available.join(", ")
                    )
                })?;

            let old_scenario_name = agent_config
                .scenario_id
                .as_deref()
                .and_then(|id| {
                    store
                        .get_all_scenarios()
                        .ok()
                        .and_then(|ss| ss.into_iter().find(|s| s.id == id).map(|s| s.name))
                })
                .unwrap_or_else(|| "none".to_string());

            println!(
                "Switching {}: {} -> {}",
                agent_key, old_scenario_name, target.name
            );

            let all_adapters = tool_adapters::enabled_installed_adapters(&store);
            let configured_mode = store.get_setting("sync_mode").ok().flatten();

            // Find the adapter for this specific agent.
            let agent_adapters: Vec<_> = all_adapters
                .iter()
                .filter(|a| a.key == agent_key)
                .cloned()
                .collect();

            // Unsync old scenario for this agent only.
            if let Some(old_id) = &agent_config.scenario_id {
                if old_id != &target.id {
                    unsync_scenario(&store, old_id, &agent_adapters, configured_mode.as_deref())?;
                }
            }

            store.set_agent_scenario(agent_key, &target.id)?;

            let synced_per_adapter = sync_agent(
                &store,
                agent_key,
                &agent_adapters,
                configured_mode.as_deref(),
            )?;
            for (display_name, count) in synced_per_adapter {
                println!("  + {} ({} skills)", display_name, count);
            }

            println!("Done. {} active: {}", agent_key, target.name);
        }
    }

    Ok(())
}

pub fn cmd_skills(name: Option<&str>) -> Result<()> {
    let store = open_store()?;
    let scenario = resolve_scenario(&store, name)?;
    let skills = store.get_effective_skills_for_scenario(&scenario.id)?;

    println!("{} ({} skills):", scenario.name, skills.len());
    for skill in &skills {
        println!("  {}", skill.name);
    }
    Ok(())
}

pub fn cmd_diff(a: &str, b: &str) -> Result<()> {
    let store = open_store()?;
    let sa = find_scenario_by_name(&store, a)?;
    let sb = find_scenario_by_name(&store, b)?;

    let skills_a = store.get_effective_skills_for_scenario(&sa.id)?;
    let skills_b = store.get_effective_skills_for_scenario(&sb.id)?;

    let ids_a: std::collections::HashSet<&str> = skills_a.iter().map(|s| s.id.as_str()).collect();
    let ids_b: std::collections::HashSet<&str> = skills_b.iter().map(|s| s.id.as_str()).collect();

    let only_a: Vec<&str> = skills_a
        .iter()
        .filter(|s| !ids_b.contains(s.id.as_str()))
        .map(|s| s.name.as_str())
        .collect();
    let only_b: Vec<&str> = skills_b
        .iter()
        .filter(|s| !ids_a.contains(s.id.as_str()))
        .map(|s| s.name.as_str())
        .collect();

    println!("Only in {}:", sa.name);
    if only_a.is_empty() {
        println!("  (none)");
    } else {
        for name in &only_a {
            println!("  + {}", name);
        }
    }

    println!();

    println!("Only in {}:", sb.name);
    if only_b.is_empty() {
        println!("  (none)");
    } else {
        for name in &only_b {
            println!("  + {}", name);
        }
    }

    Ok(())
}

pub fn cmd_packs(name: Option<&str>) -> Result<()> {
    let store = open_store()?;
    let scenario = resolve_scenario(&store, name)?;
    let packs = store.get_packs_for_scenario(&scenario.id)?;

    println!("{} ({} packs):", scenario.name, packs.len());
    for pack in &packs {
        let skill_count = store.count_skills_for_pack(&pack.id)?;
        println!("  {:<24} {:>3} skills", pack.name, skill_count);
    }
    Ok(())
}

pub fn cmd_pack_add(pack_name: &str, scenario_name: &str) -> Result<()> {
    let store = open_store()?;
    let scenario = find_scenario_by_name(&store, scenario_name)?;
    let pack = find_pack_by_name(&store, pack_name)?;

    store.add_pack_to_scenario(&scenario.id, &pack.id)?;
    println!("Added pack '{}' to scenario '{}'", pack.name, scenario.name);

    resync_if_active(&store, &scenario.id)?;

    Ok(())
}

pub fn cmd_pack_remove(pack_name: &str, scenario_name: &str) -> Result<()> {
    let store = open_store()?;
    let scenario = find_scenario_by_name(&store, scenario_name)?;
    let pack = find_pack_by_name(&store, pack_name)?;

    store.remove_pack_from_scenario(&scenario.id, &pack.id)?;
    println!(
        "Removed pack '{}' from scenario '{}'",
        pack.name, scenario.name
    );

    resync_if_active(&store, &scenario.id)?;

    Ok(())
}

/// If `scenario_id` is the active scenario, unsync and re-sync it.
fn resync_if_active(store: &SkillStore, scenario_id: &str) -> Result<()> {
    if let Ok(Some(active_id)) = store.get_active_scenario_id() {
        if active_id == scenario_id {
            println!("Re-syncing active scenario...");
            let adapters = tool_adapters::enabled_installed_adapters(store);
            let configured_mode = store.get_setting("sync_mode").ok().flatten();
            unsync_scenario(store, scenario_id, &adapters, configured_mode.as_deref())?;
            sync_scenario(store, scenario_id, &adapters, configured_mode.as_deref())?;
            println!("Done.");
        }
    }
    Ok(())
}

// ── Pack context / router commands ───────────────────────

pub fn cmd_pack_context(name: &str) -> Result<()> {
    let store = open_store()?;
    let pack = find_pack_by_name(&store, name)?;
    let skills = store.get_skills_for_pack(&pack.id)?;

    println!("# Pack: {}\n", pack.name);
    if let Some(d) = &pack.description {
        println!("Description: {d}\n");
    }
    if let Some(r) = &pack.router_description {
        println!("Current router: {r}\n");
    }
    println!("## Skills ({})\n", skills.len());
    for s in &skills {
        println!(
            "- {}: {}",
            s.name,
            s.description.clone().unwrap_or_default()
        );
    }
    Ok(())
}

pub fn cmd_pack_set_router(
    name: &str,
    description: Option<&str>,
    body_file: Option<&std::path::Path>,
) -> Result<()> {
    if description.is_none() && body_file.is_none() {
        anyhow::bail!("set-router requires at least one of --description or --body");
    }
    let store = open_store()?;
    let pack = find_pack_by_name(&store, name)?;
    let body = body_file.map(std::fs::read_to_string).transpose()?;
    let ts = chrono::Utc::now().timestamp();
    store.set_pack_router(&pack.id, description, body.as_deref(), ts)?;
    println!("Router updated for pack '{}'.", pack.name);
    Ok(())
}

pub fn cmd_pack_list_routers() -> Result<()> {
    let store = open_store()?;
    for pack in store.get_all_packs()? {
        let status = if pack.router_description.is_some() {
            "✓"
        } else {
            "—"
        };
        println!(
            "{status}  {name:<24} {desc}",
            status = status,
            name = pack.name,
            desc = pack
                .router_description
                .as_deref()
                .unwrap_or("<not generated>"),
        );
    }
    Ok(())
}

pub fn cmd_pack_gen_router(name: &str) -> Result<()> {
    let store = open_store()?;
    let pack = find_pack_by_name(&store, name)?;
    let skills = store
        .get_skills_for_pack(&pack.id)?
        .into_iter()
        .map(|s| (s.name, s.description))
        .collect();
    let marker = skills_manager_core::pending_router_gen::PendingMarker {
        pack_id: pack.id.clone(),
        pack_name: pack.name.clone(),
        created_at: chrono::Utc::now().timestamp(),
        skills,
    };
    let sm_root = central_repo::base_dir();
    skills_manager_core::pending_router_gen::write_marker(&sm_root, &marker)?;
    println!(
        "Pending marker written. Open Claude Code — the pack-router-gen skill will handle '{}'.",
        pack.name
    );
    Ok(())
}

pub fn cmd_pack_regen_all_routers() -> Result<()> {
    let store = open_store()?;
    for pack in store.get_all_packs()? {
        if pack.is_essential {
            continue;
        }
        cmd_pack_gen_router(&pack.name)?;
    }
    Ok(())
}

// ── Router eval harness (MVP, string-match proxy) ────────

/// Canned queries per pack. A "hit" means the pack's `router_description`
/// contains at least one meaningful (>3 char) word from the query.
///
/// This is a cheap regression guard — not a substitute for a live Claude
/// routing eval, which is out of scope for MVP.
const EVAL_QUERIES: &[(&str, &[&str])] = &[
    (
        "mkt-seo",
        &["audit my SEO", "add schema markup", "not ranking on Google"],
    ),
    (
        "dev-frontend",
        &[
            "build a landing page",
            "tailwind component",
            "stitch mockup",
        ],
    ),
    (
        "web-research",
        &["deep research", "search twitter", "last 30 days trends"],
    ),
    (
        "browser-automation",
        &["scrape this page", "browser automation", "login to site"],
    ),
    ("mkt-copy", &["write cold email", "ad copy", "sales deck"]),
    (
        "mkt-cro",
        &[
            "landing page conversion",
            "signup optimization",
            "popup for leads",
        ],
    ),
    (
        "knowledge-library",
        &["readwise highlights", "obsidian note", "feed catchup"],
    ),
    (
        "docs-office",
        &["convert to pdf", "word document", "pptx slides"],
    ),
    (
        "ai-engineering",
        &["build mcp server", "claude api tool use", "prompt caching"],
    ),
    (
        "agent-orchestration",
        &["paseo committee", "loop agent", "handoff to another"],
    ),
];

pub fn cmd_pack_eval_routers() -> Result<()> {
    let store = open_store()?;

    let mut total_hits = 0usize;
    let mut total_queries = 0usize;

    println!("{:<22} {:>8} {:>8}", "Pack", "Hits", "Total");
    println!("{}", "-".repeat(42));

    for (pack_name, queries) in EVAL_QUERIES {
        let pack = match store
            .get_all_packs()?
            .into_iter()
            .find(|p| p.name.eq_ignore_ascii_case(pack_name))
        {
            Some(p) => p,
            None => continue,
        };

        let desc = pack
            .router_description
            .as_deref()
            .unwrap_or("")
            .to_lowercase();

        let mut hits = 0usize;
        for q in *queries {
            let q_lower = q.to_lowercase();
            // Hit = at least one meaningful word (>3 chars) from the query
            // appears in the router description.
            if q_lower
                .split_whitespace()
                .any(|w| w.len() > 3 && desc.contains(w))
            {
                hits += 1;
            }
        }

        let total = queries.len();
        total_hits += hits;
        total_queries += total;
        println!("{:<22} {:>8} {:>8}", pack_name, hits, total);
    }

    println!("{}", "-".repeat(42));
    let pct = if total_queries > 0 {
        (total_hits * 100) / total_queries
    } else {
        0
    };
    println!(
        "{:<22} {:>8} {:>8}   ({}%)",
        "TOTAL", total_hits, total_queries, pct
    );

    Ok(())
}

// ── Pack helper ──

fn find_pack_by_name(store: &SkillStore, name: &str) -> Result<skills_manager_core::PackRecord> {
    let packs = store.get_all_packs()?;
    let lower = name.to_lowercase();
    if let Some(p) = packs.iter().find(|p| p.name.to_lowercase() == lower) {
        return Ok(p.clone());
    }
    let available: Vec<&str> = packs.iter().map(|p| p.name.as_str()).collect();
    bail!(
        "Pack '{}' not found. Available: {}",
        name,
        available.join(", ")
    );
}

// ── Agent commands ───────────────────────────────────────

pub fn cmd_agents() -> Result<()> {
    let store = open_store()?;
    let configs = store.get_all_agent_configs()?;
    let scenarios = store.get_all_scenarios()?;

    if configs.is_empty() {
        println!("No agent configs found.");
        return Ok(());
    }

    println!("Agents:");
    for config in &configs {
        let marker = if config.managed { "●" } else { "○" };

        if !config.managed {
            println!("  {} {:<20} (unmanaged)", marker, config.tool_key);
            continue;
        }

        let scenario_name = config
            .scenario_id
            .as_deref()
            .and_then(|id| {
                scenarios
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| s.name.as_str())
            })
            .unwrap_or("(none)");

        let extra_packs = store.get_agent_extra_packs(&config.tool_key)?;
        let pack_suffix = match extra_packs.len() {
            0 => String::new(),
            1 => " +1 pack".to_string(),
            n => format!(" +{} packs", n),
        };

        println!(
            "  {} {:<20} {}{}",
            marker, config.tool_key, scenario_name, pack_suffix
        );
    }

    Ok(())
}

pub fn cmd_agent_info(agent_key: &str) -> Result<()> {
    let store = open_store()?;
    let config = store
        .get_agent_config(agent_key)?
        .context(format!("Agent '{}' not found", agent_key))?;
    let adapter = tool_adapters::find_adapter_with_store(&store, agent_key)
        .context(format!("Unknown tool: {}", agent_key))?;
    let ownership = store.scan_agent_skill_ownership(agent_key, &adapter.skills_dir())?;

    let scenario_name = config
        .scenario_id
        .and_then(|id| {
            store
                .get_all_scenarios()
                .ok()?
                .into_iter()
                .find(|s| s.id == id)
        })
        .map(|s| s.name)
        .unwrap_or_else(|| "none".to_string());

    println!("{} ({}):", adapter.display_name, agent_key);
    println!(
        "  Scenario:   {} ({} skills)",
        scenario_name,
        ownership.managed.len()
    );
    println!(
        "  Discovered: {} skills (not imported)",
        ownership.discovered.len()
    );
    println!("  Native:     {} skills", ownership.native.len());
    if !ownership.native.is_empty() {
        for name in &ownership.native {
            println!("    {}", name);
        }
    }
    Ok(())
}

pub fn cmd_agent_add_pack(agent: &str, pack_name: &str) -> Result<()> {
    let store = open_store()?;

    // Validate agent exists.
    let all_configs = store.get_all_agent_configs()?;
    if !all_configs.iter().any(|c| c.tool_key == agent) {
        let available: Vec<&str> = all_configs.iter().map(|c| c.tool_key.as_str()).collect();
        bail!(
            "Agent '{}' not found. Available: {}",
            agent,
            available.join(", ")
        );
    }

    let pack = find_pack_by_name(&store, pack_name)?;
    store.add_agent_extra_pack(agent, &pack.id)?;
    println!("Added pack '{}' to agent '{}'", pack.name, agent);

    // Re-sync this agent if it has an adapter.
    let all_adapters = tool_adapters::enabled_installed_adapters(&store);
    let agent_adapters: Vec<_> = all_adapters
        .iter()
        .filter(|a| a.key == agent)
        .cloned()
        .collect();
    let configured_mode = store.get_setting("sync_mode").ok().flatten();

    if !agent_adapters.is_empty() {
        // Get the agent's current scenario_id for unsync.
        if let Some(config) = all_configs.iter().find(|c| c.tool_key == agent) {
            if let Some(scenario_id) = &config.scenario_id {
                unsync_scenario(
                    &store,
                    scenario_id,
                    &agent_adapters,
                    configured_mode.as_deref(),
                )?;
            }
        }
        let synced = sync_agent(&store, agent, &agent_adapters, configured_mode.as_deref())?;
        for (display_name, count) in synced {
            println!("  + {} ({} skills)", display_name, count);
        }
        println!("Done.");
    }

    Ok(())
}

pub fn cmd_agent_remove_pack(agent: &str, pack_name: &str) -> Result<()> {
    let store = open_store()?;

    // Validate agent exists.
    let all_configs = store.get_all_agent_configs()?;
    if !all_configs.iter().any(|c| c.tool_key == agent) {
        let available: Vec<&str> = all_configs.iter().map(|c| c.tool_key.as_str()).collect();
        bail!(
            "Agent '{}' not found. Available: {}",
            agent,
            available.join(", ")
        );
    }

    let pack = find_pack_by_name(&store, pack_name)?;
    store.remove_agent_extra_pack(agent, &pack.id)?;
    println!("Removed pack '{}' from agent '{}'", pack.name, agent);

    // Re-sync this agent if it has an adapter.
    let all_adapters = tool_adapters::enabled_installed_adapters(&store);
    let agent_adapters: Vec<_> = all_adapters
        .iter()
        .filter(|a| a.key == agent)
        .cloned()
        .collect();
    let configured_mode = store.get_setting("sync_mode").ok().flatten();

    if !agent_adapters.is_empty() {
        if let Some(config) = all_configs.iter().find(|c| c.tool_key == agent) {
            if let Some(scenario_id) = &config.scenario_id {
                unsync_scenario(
                    &store,
                    scenario_id,
                    &agent_adapters,
                    configured_mode.as_deref(),
                )?;
            }
        }
        let synced = sync_agent(&store, agent, &agent_adapters, configured_mode.as_deref())?;
        for (display_name, count) in synced {
            println!("  + {} ({} skills)", display_name, count);
        }
        println!("Done.");
    }

    Ok(())
}

// ── Sync helpers ─────────────────────────────────────────

/// Remove SM-managed entries from all adapters for a given scenario.
/// Uses filesystem scanning to find entries pointing to ~/.skills-manager/skills/,
/// which is reliable regardless of DB state.
fn unsync_scenario(
    store: &SkillStore,
    scenario_id: &str,
    adapters: &[tool_adapters::ToolAdapter],
    configured_mode: Option<&str>,
) -> Result<()> {
    let sm_skills_dir = central_repo::skills_dir();
    let sm_skills_prefix = sm_skills_dir.to_string_lossy().to_string();

    let skill_names: std::collections::HashSet<String> = store
        .get_effective_skills_for_scenario(scenario_id)?
        .into_iter()
        .map(|s| s.name)
        .collect();

    for adapter in adapters {
        let skills_dir = adapter.skills_dir();
        if !skills_dir.exists() {
            continue;
        }
        let mode = sync_engine::sync_mode_for_tool(&adapter.key, configured_mode);

        let entries = match std::fs::read_dir(&skills_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_symlink() {
                if let Ok(target) = std::fs::read_link(&path) {
                    if target.to_string_lossy().contains(&sm_skills_prefix) {
                        let _ = sync_engine::remove_target(&path);
                    }
                }
            } else if matches!(mode, sync_engine::SyncMode::Copy) && path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if skill_names.contains(name) {
                        let _ = sync_engine::remove_target(&path);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Sync all effective skills for a scenario to all enabled adapters.
/// Returns a list of (adapter display name, synced count) pairs.
fn sync_scenario(
    store: &SkillStore,
    scenario_id: &str,
    adapters: &[tool_adapters::ToolAdapter],
    configured_mode: Option<&str>,
) -> Result<Vec<(String, usize)>> {
    let skills = store.get_effective_skills_for_scenario(scenario_id)?;
    let mut results = Vec::new();

    for adapter in adapters {
        let mode = sync_engine::sync_mode_for_tool(&adapter.key, configured_mode);
        let skills_dir = adapter.skills_dir();
        let mut synced = 0;

        for skill in &skills {
            let source = PathBuf::from(&skill.central_path);
            if !source.exists() {
                eprintln!(
                    "  Warning: skipping '{}' — source path does not exist: {}",
                    skill.name,
                    source.display()
                );
                continue;
            }
            let target_path = skills_dir.join(&skill.name);
            match sync_engine::sync_skill(&source, &target_path, mode) {
                Ok(_) => synced += 1,
                Err(e) => eprintln!(
                    "  Warning: failed to sync '{}' to {}: {}",
                    skill.name, adapter.display_name, e
                ),
            }
        }

        results.push((adapter.display_name.clone(), synced));
    }

    Ok(results)
}

/// Sync all effective skills for a specific agent to its adapter(s).
/// Uses `get_effective_skills_for_agent` so extra packs are included.
/// Returns a list of (adapter display name, synced count) pairs.
fn sync_agent(
    store: &SkillStore,
    tool_key: &str,
    adapters: &[tool_adapters::ToolAdapter],
    configured_mode: Option<&str>,
) -> Result<Vec<(String, usize)>> {
    let skills = store.get_effective_skills_for_agent(tool_key)?;
    let mut results = Vec::new();

    for adapter in adapters {
        let mode = sync_engine::sync_mode_for_tool(&adapter.key, configured_mode);
        let skills_dir = adapter.skills_dir();
        let mut synced = 0;

        for skill in &skills {
            let source = PathBuf::from(&skill.central_path);
            if !source.exists() {
                eprintln!(
                    "  Warning: skipping '{}' — source path does not exist: {}",
                    skill.name,
                    source.display()
                );
                continue;
            }
            let target_path = skills_dir.join(&skill.name);
            match sync_engine::sync_skill(&source, &target_path, mode) {
                Ok(_) => synced += 1,
                Err(e) => eprintln!(
                    "  Warning: failed to sync '{}' to {}: {}",
                    skill.name, adapter.display_name, e
                ),
            }
        }

        results.push((adapter.display_name.clone(), synced));
    }

    Ok(results)
}

// ── Dedup command ───────────────────────────────────────

pub fn cmd_dedup(apply: bool, agent: Option<&str>) -> Result<()> {
    let store = open_store()?;
    let all_adapters = tool_adapters::enabled_installed_adapters(&store);

    let dry_run = !apply;

    if dry_run {
        println!("Dedup dry run (use --apply to execute):\n");
    } else {
        println!("Dedup applying changes:\n");
    }

    let results = match agent {
        Some(agent_key) => {
            let adapter = all_adapters
                .iter()
                .find(|a| a.key == agent_key)
                .ok_or_else(|| {
                    let available: Vec<&str> =
                        all_adapters.iter().map(|a| a.key.as_str()).collect();
                    anyhow::anyhow!(
                        "Agent '{}' not found. Available: {}",
                        agent_key,
                        available.join(", ")
                    )
                })?;
            let r =
                dedup::dedup_agent_skills(&store, &adapter.key, &adapter.skills_dir(), dry_run)?;
            vec![(adapter.key.clone(), r)]
        }
        None => dedup::dedup_all_agents(&store, &all_adapters, dry_run),
    };

    let mut total_linked = 0;
    let mut total_replaced = 0;
    let mut total_native = 0;
    let mut total_skipped = 0;
    let mut total_errors = 0;

    for (tool_key, r) in &results {
        if r.is_empty() {
            continue;
        }

        println!("{}:", tool_key);

        if !r.already_linked.is_empty() {
            println!("  Already linked: {}", r.already_linked.len());
            total_linked += r.already_linked.len();
        }

        if !r.replaced_with_symlink.is_empty() {
            let verb = if dry_run { "Would replace" } else { "Replaced" };
            println!("  {} with symlink: {}", verb, r.replaced_with_symlink.len());
            for name in &r.replaced_with_symlink {
                println!("    {}", name);
            }
            total_replaced += r.replaced_with_symlink.len();
        }

        if !r.marked_native.is_empty() {
            let verb = if dry_run { "Would mark" } else { "Marked" };
            println!("  {} as native: {}", verb, r.marked_native.len());
            for name in &r.marked_native {
                println!("    {}", name);
            }
            total_native += r.marked_native.len();
        }

        if !r.skipped_unknown.is_empty() {
            println!("  Skipped (not in central): {}", r.skipped_unknown.len());
            total_skipped += r.skipped_unknown.len();
        }

        for err in &r.errors {
            eprintln!("  Error: {}", err);
            total_errors += 1;
        }

        println!();
    }

    println!("Summary:");
    println!("  Already linked:  {}", total_linked);
    println!(
        "  {}:  {}",
        if dry_run { "Would replace" } else { "Replaced" },
        total_replaced
    );
    println!(
        "  {}:    {}",
        if dry_run {
            "Would mark native"
        } else {
            "Marked native"
        },
        total_native
    );
    println!("  Skipped:         {}", total_skipped);
    if total_errors > 0 {
        println!("  Errors:          {}", total_errors);
    }

    Ok(())
}

// ── Seed Packs command ──────────────────────────────────

pub fn cmd_seed_packs(force: bool) -> Result<()> {
    let store = open_store()?;

    if force {
        println!("Force-seeding default packs (replacing any existing)...");
    } else {
        println!("Seeding default packs...");
    }

    let result = pack_seeder::seed_default_packs(&store, force)?;

    if result.skipped {
        println!("Skipped: packs already exist. Use --force to re-seed.");
        return Ok(());
    }

    println!("  Packs created:         {}", result.packs_created);
    println!("  Skills assigned:       {}", result.skills_assigned);
    println!(
        "  Scenario-pack links:   {}",
        result.scenario_packs_assigned
    );
    println!("Done.");

    Ok(())
}

// ── Fix Orphans command ────────────────────────────────

pub fn cmd_fix_orphans() -> Result<()> {
    let store = open_store()?;

    println!("Scanning central store for orphan skills...");
    let imported = dedup::import_orphan_central_skills(&store)?;

    if imported == 0 {
        println!("No orphans found. All central skills have DB records.");
    } else {
        println!("Imported {} orphan skill(s) into the database.", imported);
    }

    Ok(())
}
