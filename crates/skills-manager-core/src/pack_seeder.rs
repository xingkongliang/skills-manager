use anyhow::Result;
use serde::Serialize;
use uuid::Uuid;

use crate::skill_store::SkillStore;

/// Result of seeding default packs.
#[derive(Debug, Clone, Serialize)]
pub struct SeedResult {
    pub packs_created: usize,
    pub skills_assigned: usize,
    pub scenario_packs_assigned: usize,
    pub skipped: bool,
}

/// A default pack definition.
struct PackDef {
    name: &'static str,
    description: &'static str,
    icon: &'static str,
    color: &'static str,
    skill_names: &'static [&'static str],
}

/// All default pack definitions.
const DEFAULT_PACKS: &[PackDef] = &[
    PackDef {
        name: "base",
        description: "Core skills loaded in every scenario",
        icon: "box",
        color: "#3b82f6",
        skill_names: &[
            "skill-retrieval",
            "web-access",
            "smart-search",
            "agent-reach",
            "bb-browser",
            "defuddle",
            "obsidian-defuddle",
            "opencli",
            "opencli-usage",
            "opencli-browser",
            "opencli-autofix",
            "opencli-oneshot",
            "opencli-explorer",
            "agent-browser",
            "discover",
            "find-skills",
            "scenario",
        ],
    },
    PackDef {
        name: "gstack",
        description: "Full gstack workflow skills",
        icon: "layers",
        color: "#10b981",
        skill_names: &[
            "browse",
            "investigate",
            "ship",
            "review",
            "qa",
            "qa-only",
            "office-hours",
            "plan-ceo-review",
            "plan-eng-review",
            "plan-design-review",
            "plan-devex-review",
            "devex-review",
            "design-review",
            "design-consultation",
            "design-html",
            "design-shotgun",
            "design-md",
            "checkpoint",
            "health",
            "document-release",
            "land-and-deploy",
            "retro",
            "learn",
            "learned",
            "setup-deploy",
            "setup-browser-cookies",
            "open-gstack-browser",
            "guard",
            "freeze",
            "unfreeze",
            "careful",
            "cso",
            "canary",
            "benchmark",
            "gstack",
            "gstack-upgrade",
            "pair-agent",
            "codex",
            "autoplan",
            "connect-chrome",
            "stitch-design",
            "stitch-loop",
            "enhance-prompt",
            "taste-design",
            "remotion",
        ],
    },
    PackDef {
        name: "agent-orchestration",
        description: "Multi-agent coordination and orchestration",
        icon: "network",
        color: "#8b5cf6",
        skill_names: &[
            "paseo",
            "paseo-loop",
            "paseo-orchestrator",
            "paseo-committee",
            "paseo-chat",
            "paseo-handoff",
            "paperclip",
        ],
    },
    PackDef {
        name: "browser-tools",
        description: "Browser automation and web interaction",
        icon: "globe",
        color: "#f59e0b",
        skill_names: &[
            "agent-browser",
            "bb-browser",
            "opencli-browser",
            "x-tweet-fetcher",
            "webapp-testing",
            "web-access",
            "connect-chrome",
            "verify-deploy",
        ],
    },
    PackDef {
        name: "research",
        description: "Deep research and content discovery",
        icon: "search",
        color: "#06b6d4",
        skill_names: &[
            "last30days",
            "perp-search",
            "autoresearch",
            "follow-builders",
            "feed-catchup",
            "codex-deep-search",
            "reader-recap",
            "readwise-cli",
            "readwise-mcp",
            "readwise-to-notebooklm",
        ],
    },
    PackDef {
        name: "design",
        description: "UI/UX design and frontend tooling",
        icon: "palette",
        color: "#ec4899",
        skill_names: &[
            "stitch-design",
            "frontend-design",
            "shadcn-ui",
            "canvas-design",
            "brand-guidelines",
            "taste-design",
            "enhance-prompt",
            "remotion",
            "stitch-loop",
            "web-design-guidelines",
            "react-components",
            "react_components",
            "web-artifacts-builder",
        ],
    },
    PackDef {
        name: "knowledge",
        description: "Knowledge management and personal library",
        icon: "book-open",
        color: "#6366f1",
        skill_names: &[
            "obsidian-cli",
            "obsidian-markdown",
            "notebooklm",
            "readwise-cli",
            "readwise-mcp",
            "readwise-to-notebooklm",
            "library",
            "triage",
            "build-persona",
        ],
    },
    PackDef {
        name: "marketing",
        description: "Marketing, PRDs, and communications",
        icon: "megaphone",
        color: "#f97316",
        skill_names: &["marketing", "prd", "internal-comms", "documentation-writer"],
    },
    PackDef {
        name: "ops",
        description: "Developer tooling and skill management",
        icon: "wrench",
        color: "#64748b",
        skill_names: &[
            "claude-code-router",
            "template-skill",
            "yt-dlp",
            "cli-creator",
            "mcp-builder",
            "skill-creator",
        ],
    },
];

/// Seed default packs into the database.
///
/// Idempotent: if any packs already exist, returns early unless `force` is true.
/// When `force` is true, all existing packs are deleted first.
pub fn seed_default_packs(store: &SkillStore, force: bool) -> Result<SeedResult> {
    let existing = store.get_all_packs()?;

    if !existing.is_empty() {
        if !force {
            return Ok(SeedResult {
                packs_created: 0,
                skills_assigned: 0,
                scenario_packs_assigned: 0,
                skipped: true,
            });
        }
        // Force mode: delete all existing packs (cascades to pack_skills and scenario_packs)
        for pack in &existing {
            store.delete_pack(&pack.id)?;
        }
    }

    // Get all skills from DB for name matching
    let all_skills = store.get_all_skills()?;

    let mut total_packs = 0;
    let mut total_skills_assigned = 0;

    // Create each pack and assign matching skills
    let mut pack_ids: Vec<(String, &PackDef)> = Vec::new();

    for def in DEFAULT_PACKS {
        let pack_id = Uuid::new_v4().to_string();
        store.insert_pack(
            &pack_id,
            def.name,
            Some(def.description),
            Some(def.icon),
            Some(def.color),
        )?;
        total_packs += 1;

        // Find matching skills by name (case-insensitive)
        for skill_name in def.skill_names {
            let lower = skill_name.to_lowercase();
            if let Some(skill) = all_skills.iter().find(|s| s.name.to_lowercase() == lower) {
                store.add_skill_to_pack(&pack_id, &skill.id)?;
                total_skills_assigned += 1;
            }
        }

        pack_ids.push((pack_id, def));
    }

    // Assign packs to scenarios based on overlap
    let scenarios = store.get_all_scenarios()?;
    let mut total_scenario_packs = 0;

    for scenario in &scenarios {
        let scenario_skill_ids: std::collections::HashSet<String> = store
            .get_effective_skill_ids_for_scenario(&scenario.id)?
            .into_iter()
            .collect();

        // Also include direct scenario_skills for matching
        let direct_skill_ids: std::collections::HashSet<String> = store
            .get_skill_ids_for_scenario(&scenario.id)?
            .into_iter()
            .collect();

        let combined_ids: std::collections::HashSet<&String> = scenario_skill_ids
            .iter()
            .chain(direct_skill_ids.iter())
            .collect();

        for (pack_id, def) in &pack_ids {
            // Count how many of this pack's defined skills are in the scenario
            let pack_skill_names: Vec<String> =
                def.skill_names.iter().map(|n| n.to_lowercase()).collect();

            // Find which of the pack's skills actually exist in DB
            let pack_db_skills: Vec<&str> = all_skills
                .iter()
                .filter(|s| pack_skill_names.contains(&s.name.to_lowercase()))
                .map(|s| s.id.as_str())
                .collect();

            if pack_db_skills.is_empty() {
                continue;
            }

            let overlap = pack_db_skills
                .iter()
                .filter(|id| combined_ids.contains(&id.to_string()))
                .count();

            let ratio = overlap as f64 / pack_db_skills.len() as f64;
            if ratio > 0.5 {
                store.add_pack_to_scenario(&scenario.id, pack_id)?;
                total_scenario_packs += 1;
            }
        }
    }

    Ok(SeedResult {
        packs_created: total_packs,
        skills_assigned: total_skills_assigned,
        scenario_packs_assigned: total_scenario_packs,
        skipped: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_store() -> SkillStore {
        let path = PathBuf::from(":memory:");
        SkillStore::new(&path).unwrap()
    }

    fn insert_test_skill(store: &SkillStore, name: &str) -> String {
        let id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let skill = crate::skill_store::SkillRecord {
            id: id.clone(),
            name: name.to_string(),
            description: None,
            source_type: "local".to_string(),
            source_ref: None,
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: format!("/tmp/skills/{}", name),
            content_hash: None,
            enabled: true,
            created_at: now,
            updated_at: now,
            status: "ok".to_string(),
            update_status: "unknown".to_string(),
            last_checked_at: None,
            last_check_error: None,
        };
        store.insert_skill(&skill).unwrap();
        id
    }

    #[test]
    fn seed_creates_packs_when_empty() {
        let store = test_store();
        // Insert a few skills that match pack definitions
        insert_test_skill(&store, "web-access");
        insert_test_skill(&store, "smart-search");
        insert_test_skill(&store, "paseo");
        insert_test_skill(&store, "marketing");

        let result = seed_default_packs(&store, false).unwrap();
        assert!(!result.skipped);
        assert_eq!(result.packs_created, 9);
        // 4 skills matched across packs (web-access in base + browser-tools = 2, smart-search in base = 1, paseo in agent-orch = 1, marketing in marketing = 1)
        assert!(result.skills_assigned >= 4);

        let packs = store.get_all_packs().unwrap();
        assert_eq!(packs.len(), 9);
    }

    #[test]
    fn seed_skips_when_packs_exist() {
        let store = test_store();
        store
            .insert_pack("existing", "existing-pack", None, None, None)
            .unwrap();

        let result = seed_default_packs(&store, false).unwrap();
        assert!(result.skipped);
        assert_eq!(result.packs_created, 0);
    }

    #[test]
    fn seed_force_replaces_existing() {
        let store = test_store();
        store
            .insert_pack("old", "old-pack", None, None, None)
            .unwrap();

        let result = seed_default_packs(&store, true).unwrap();
        assert!(!result.skipped);
        assert_eq!(result.packs_created, 9);

        // Old pack should be gone
        let packs = store.get_all_packs().unwrap();
        assert!(packs.iter().all(|p| p.name != "old-pack"));
    }

    #[test]
    fn seed_assigns_packs_to_scenarios() {
        let store = test_store();

        // Create some skills from base pack
        let s1 = insert_test_skill(&store, "web-access");
        let s2 = insert_test_skill(&store, "smart-search");
        let s3 = insert_test_skill(&store, "agent-reach");
        let s4 = insert_test_skill(&store, "bb-browser");
        let s5 = insert_test_skill(&store, "defuddle");

        // Create a scenario with those skills
        let now = chrono::Utc::now().timestamp_millis();
        let scenario = crate::skill_store::ScenarioRecord {
            id: Uuid::new_v4().to_string(),
            name: "test-scenario".to_string(),
            description: None,
            icon: None,
            sort_order: 0,
            created_at: now,
            updated_at: now,
        };
        store.insert_scenario(&scenario).unwrap();
        for sid in [&s1, &s2, &s3, &s4, &s5] {
            store.add_skill_to_scenario(&scenario.id, sid).unwrap();
        }

        let result = seed_default_packs(&store, false).unwrap();
        assert!(!result.skipped);
        // base pack has 17 defined skills, 5 matched in DB.
        // Scenario has all 5 of those -> 5/5 = 100% > 50%, so base should be assigned
        assert!(result.scenario_packs_assigned > 0);

        let packs = store.get_packs_for_scenario(&scenario.id).unwrap();
        let base_assigned = packs.iter().any(|p| p.name == "base");
        assert!(base_assigned, "base pack should be assigned to scenario");
    }

    #[test]
    fn seed_is_case_insensitive() {
        let store = test_store();
        insert_test_skill(&store, "Web-Access");
        insert_test_skill(&store, "SMART-SEARCH");

        let result = seed_default_packs(&store, false).unwrap();
        // web-access appears in base + browser-tools, smart-search in base = 3 total
        assert!(result.skills_assigned >= 2);
    }

    #[test]
    fn seed_handles_skills_in_multiple_packs() {
        let store = test_store();
        // agent-browser is in both base and browser-tools
        insert_test_skill(&store, "agent-browser");

        let result = seed_default_packs(&store, false).unwrap();
        assert!(result.skills_assigned >= 2);

        // Verify the skill is in both packs
        let packs = store.get_all_packs().unwrap();
        let base = packs.iter().find(|p| p.name == "base").unwrap();
        let browser = packs.iter().find(|p| p.name == "browser-tools").unwrap();

        let base_skills = store.get_skills_for_pack(&base.id).unwrap();
        let browser_skills = store.get_skills_for_pack(&browser.id).unwrap();

        assert!(base_skills.iter().any(|s| s.name == "agent-browser"));
        assert!(browser_skills.iter().any(|s| s.name == "agent-browser"));
    }
}
