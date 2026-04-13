use anyhow::{bail, Context, Result};
use skills_manager_core::skill_store::SkillStore;
use skills_manager_core::{central_repo, sync_engine, tool_adapters, ScenarioRecord};
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

pub fn cmd_switch(name: &str) -> Result<()> {
    let store = open_store()?;
    let target = find_scenario_by_name(&store, name)?;

    let current_name = get_active_scenario(&store)
        .map(|s| s.name)
        .unwrap_or_else(|_| "none".to_string());

    println!("Switching: {} -> {}", current_name, target.name);

    let adapters = tool_adapters::enabled_installed_adapters(&store);
    let configured_mode = store.get_setting("sync_mode").ok().flatten();

    if let Ok(Some(old_id)) = store.get_active_scenario_id() {
        if old_id != target.id {
            unsync_scenario(&store, &old_id, &adapters, configured_mode.as_deref())?;
        }
    }

    store.set_active_scenario(&target.id)?;

    let synced_per_adapter =
        sync_scenario(&store, &target.id, &adapters, configured_mode.as_deref())?;
    for (display_name, count) in synced_per_adapter {
        println!("  + {} ({} skills)", display_name, count);
    }

    println!("Done. Active: {}", target.name);
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
