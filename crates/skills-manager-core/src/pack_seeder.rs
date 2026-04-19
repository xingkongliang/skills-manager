use anyhow::Result;
use serde::Serialize;
use uuid::Uuid;

use crate::skill_store::{DisclosureMode, ScenarioRecord, SkillStore};

/// Result of seeding default packs.
#[derive(Debug, Clone, Serialize)]
pub struct SeedResult {
    pub packs_created: usize,
    pub skills_assigned: usize,
    pub scenarios_created: usize,
    pub scenario_packs_assigned: usize,
    pub skipped: bool,
    pub missing_skills: Vec<String>,
}

/// A default pack definition.
struct PackDef {
    name: &'static str,
    description: &'static str,
    icon: &'static str,
    color: &'static str,
    is_essential: bool,
    skill_names: &'static [&'static str],
}

/// A default scenario definition.
struct ScenarioDef {
    name: &'static str,
    description: &'static str,
    disclosure_mode: DisclosureMode,
    pack_names: &'static [&'static str],
}

/// Progressive Disclosure v9 pack taxonomy.
///
/// The `essential` pack is the only one marked `is_essential=true`. It is always
/// materialized in full regardless of disclosure mode and contains the
/// meta-management skills an agent needs to discover other packs.
const DEFAULT_PACKS: &[PackDef] = &[
    // ── Essential (always full-disclosed) ──
    PackDef {
        name: "essential",
        description: "Skill-discovery + meta-management, always full-disclosed",
        icon: "sparkles",
        color: "#eab308",
        is_essential: true,
        skill_names: &[
            "find-skills",
            "skill-creator",
            "scenario",
            "discover",
            "web-access",
            "smart-search",
            "pack-router-gen",
        ],
    },
    // ── Route packs ──
    PackDef {
        name: "route-gstack",
        description: "gstack workflow: framing, review, QA, ship",
        icon: "layers",
        color: "#10b981",
        is_essential: false,
        skill_names: &[
            "office-hours",
            "autoplan",
            "plan-ceo-review",
            "plan-eng-review",
            "plan-design-review",
            "plan-devex-review",
            "review",
            "qa",
            "qa-only",
            "ship",
            "investigate",
            "document-release",
            "retro",
            "health",
            "benchmark",
            "checkpoint",
            "learn",
            "learned",
            "careful",
            "freeze",
            "unfreeze",
            "guard",
            "cso",
            "canary",
            "setup-deploy",
            "setup-browser-cookies",
            "open-gstack-browser",
            "gstack",
            "gstack-upgrade",
            "devex-review",
            "pair-agent",
            "browse",
        ],
    },
    PackDef {
        name: "route-ecc",
        description: "ECC tactical: build-fix, refactor, checkpoints",
        icon: "wrench",
        color: "#64748b",
        is_essential: false,
        skill_names: &[
            "checkpoint",
            "eval",
            "build-fix",
            "refactor-clean",
            "simplify",
            "quality-gate",
            "learn",
            "learned",
            "save-session",
            "resume-session",
            "sessions",
        ],
    },
    // ── Dev packs ──
    PackDef {
        name: "dev-frontend",
        description: "Frontend + UI/UX design",
        icon: "palette",
        color: "#ec4899",
        is_essential: false,
        skill_names: &[
            "frontend-design",
            "stitch-design",
            "stitch-loop",
            "shadcn-ui",
            "taste-design",
            "canvas-design",
            "brand-guidelines",
            "web-design-guidelines",
            "vercel-react-best-practices",
            "vercel-composition-patterns",
            "react-components",
            "web-artifacts-builder",
            "remotion",
            "design-consultation",
            "design-html",
            "design-md",
            "design-review",
            "design-shotgun",
            "enhance-prompt",
        ],
    },
    PackDef {
        name: "dev-backend",
        description: "Backend, data, infra, security",
        icon: "database",
        color: "#0ea5e9",
        is_essential: false,
        skill_names: &[
            "supabase-postgres-best-practices",
            "data-science",
            "mlops",
            "devops",
            "red-teaming",
            "software-development",
        ],
    },
    PackDef {
        name: "ai-engineering",
        description: "Claude API, MCP, skill + CLI authoring",
        icon: "cpu",
        color: "#a855f7",
        is_essential: false,
        skill_names: &[
            "claude-api",
            "mcp-builder",
            "skill-creator",
            "cli-creator",
            "claude-code-router",
            "template-skill",
        ],
    },
    // ── Browser + research + knowledge ──
    PackDef {
        name: "browser-automation",
        description: "Browser automation and web interaction",
        icon: "globe",
        color: "#f59e0b",
        is_essential: false,
        skill_names: &[
            "bb-browser",
            "agent-browser",
            "opencli",
            "opencli-usage",
            "opencli-autofix",
            "opencli-explorer",
            "opencli-oneshot",
            "opencli-browser",
            "connect-chrome",
            "verify-deploy",
            "webapp-testing",
            "dogfood",
        ],
    },
    PackDef {
        name: "web-research",
        description: "Deep research and content discovery",
        icon: "search",
        color: "#06b6d4",
        is_essential: false,
        skill_names: &[
            "smart-search",
            "agent-reach",
            "codex-deep-search",
            "perp-search",
            "last30days",
            "x-tweet-fetcher",
            "follow-builders",
            "autoresearch",
            "defuddle",
            "obsidian-defuddle",
        ],
    },
    PackDef {
        name: "knowledge-library",
        description: "Knowledge management and personal library",
        icon: "book-open",
        color: "#6366f1",
        is_essential: false,
        skill_names: &[
            "library",
            "obsidian-cli",
            "obsidian-markdown",
            "notebooklm",
            "readwise-cli",
            "readwise-mcp",
            "readwise-to-notebooklm",
            "reader-recap",
            "feed-catchup",
            "build-persona",
            "triage",
            "quiz",
            "book-review",
            "highlight-graph",
            "now-reading-page",
        ],
    },
    // ── Docs + orchestration ──
    PackDef {
        name: "docs-office",
        description: "Office-format docs and writing",
        icon: "file-text",
        color: "#f97316",
        is_essential: false,
        skill_names: &[
            "pdf",
            "docx",
            "pptx",
            "xlsx",
            "documentation-writer",
            "prd",
            "internal-comms",
        ],
    },
    PackDef {
        name: "agent-orchestration",
        description: "Multi-agent coordination and orchestration",
        icon: "network",
        color: "#8b5cf6",
        is_essential: false,
        skill_names: &[
            "paseo",
            "paseo-loop",
            "paseo-orchestrator",
            "paseo-committee",
            "paseo-handoff",
            "paseo-chat",
            "paperclip",
            "loop",
        ],
    },
    // ── Marketing sub-packs ──
    PackDef {
        name: "mkt-strategy",
        description: "Marketing strategy and positioning",
        icon: "megaphone",
        color: "#f97316",
        is_essential: false,
        skill_names: &[
            "marketing",
            "marketing-ideas",
            "marketing-psychology",
            "product-marketing-context",
            "launch-strategy",
            "content-strategy",
            "site-architecture",
        ],
    },
    PackDef {
        name: "mkt-seo",
        description: "SEO audits, AI SEO, schema, programmatic",
        icon: "trending-up",
        color: "#f97316",
        is_essential: false,
        skill_names: &[
            "seo-audit",
            "ai-seo",
            "schema-markup",
            "programmatic-seo",
            "competitor-alternatives",
        ],
    },
    PackDef {
        name: "mkt-copy",
        description: "Copywriting, email, ad creative, sales enablement",
        icon: "pen-tool",
        color: "#f97316",
        is_essential: false,
        skill_names: &[
            "copywriting",
            "copy-editing",
            "cold-email",
            "email-sequence",
            "ad-creative",
            "sales-enablement",
            "social-content",
        ],
    },
    PackDef {
        name: "mkt-cro",
        description: "Conversion-rate optimization and experimentation",
        icon: "target",
        color: "#f97316",
        is_essential: false,
        skill_names: &[
            "page-cro",
            "signup-flow-cro",
            "onboarding-cro",
            "paywall-upgrade-cro",
            "form-cro",
            "popup-cro",
            "churn-prevention",
            "ab-test-setup",
            "analytics-tracking",
        ],
    },
    PackDef {
        name: "mkt-revenue",
        description: "Pricing, ads, referrals, RevOps",
        icon: "dollar-sign",
        color: "#f97316",
        is_essential: false,
        skill_names: &[
            "pricing-strategy",
            "paid-ads",
            "referral-program",
            "revops",
            "lead-magnets",
            "free-tool-strategy",
        ],
    },
];

/// Progressive Disclosure v9 scenario presets.
const DEFAULT_SCENARIOS: &[ScenarioDef] = &[
    ScenarioDef {
        name: "minimal",
        description: "Essential only — meta-management skills, full disclosure",
        disclosure_mode: DisclosureMode::Full,
        pack_names: &["essential"],
    },
    ScenarioDef {
        name: "core",
        description: "Essential + gstack workflow",
        disclosure_mode: DisclosureMode::Hybrid,
        pack_names: &["essential", "route-gstack"],
    },
    ScenarioDef {
        name: "standard",
        description: "General-purpose dev scenario",
        disclosure_mode: DisclosureMode::Hybrid,
        pack_names: &[
            "essential",
            "route-gstack",
            "dev-frontend",
            "browser-automation",
            "web-research",
            "knowledge-library",
        ],
    },
    ScenarioDef {
        name: "standard-marketing",
        description: "Standard dev + marketing basics",
        disclosure_mode: DisclosureMode::Hybrid,
        pack_names: &[
            "essential",
            "route-gstack",
            "dev-frontend",
            "browser-automation",
            "web-research",
            "knowledge-library",
            "mkt-strategy",
            "mkt-copy",
            "mkt-cro",
        ],
    },
    ScenarioDef {
        name: "full-dev",
        description: "Full-stack dev with AI engineering + orchestration",
        disclosure_mode: DisclosureMode::Hybrid,
        pack_names: &[
            "essential",
            "route-gstack",
            "dev-frontend",
            "dev-backend",
            "browser-automation",
            "web-research",
            "knowledge-library",
            "ai-engineering",
            "docs-office",
            "agent-orchestration",
        ],
    },
    ScenarioDef {
        name: "full-dev-marketing",
        description: "Full-stack dev + complete marketing suite",
        disclosure_mode: DisclosureMode::Hybrid,
        pack_names: &[
            "essential",
            "route-gstack",
            "dev-frontend",
            "dev-backend",
            "browser-automation",
            "web-research",
            "knowledge-library",
            "ai-engineering",
            "docs-office",
            "agent-orchestration",
            "mkt-strategy",
            "mkt-seo",
            "mkt-copy",
            "mkt-cro",
            "mkt-revenue",
        ],
    },
    ScenarioDef {
        name: "everything",
        description: "All packs, full disclosure (including route-ecc)",
        disclosure_mode: DisclosureMode::Full,
        pack_names: &[
            "essential",
            "route-gstack",
            "route-ecc",
            "dev-frontend",
            "dev-backend",
            "ai-engineering",
            "browser-automation",
            "web-research",
            "knowledge-library",
            "docs-office",
            "agent-orchestration",
            "mkt-strategy",
            "mkt-seo",
            "mkt-copy",
            "mkt-cro",
            "mkt-revenue",
        ],
    },
];

/// Seed default packs and scenarios into the database.
///
/// Idempotent: if any packs already exist, returns early unless `force` is true.
/// When `force` is true, all existing packs are deleted first. Named scenarios
/// from the v9 taxonomy are created if missing; if a scenario with that name
/// already exists, its disclosure_mode is updated and the v9 pack set is re-linked.
///
/// Missing skills (referenced by name in a pack but absent from the DB) are
/// logged and skipped — this keeps the seeder resilient to partial imports.
pub fn seed_default_packs(store: &SkillStore, force: bool) -> Result<SeedResult> {
    let existing_packs = store.get_all_packs()?;

    if !existing_packs.is_empty() {
        if !force {
            return Ok(SeedResult {
                packs_created: 0,
                skills_assigned: 0,
                scenarios_created: 0,
                scenario_packs_assigned: 0,
                skipped: true,
                missing_skills: Vec::new(),
            });
        }
        // Force mode: delete all existing packs (cascades to pack_skills and scenario_packs)
        for pack in &existing_packs {
            store.delete_pack(&pack.id)?;
        }
    }

    // Get all skills from DB for name matching
    let all_skills = store.get_all_skills()?;

    let mut total_packs = 0;
    let mut total_skills_assigned = 0;
    let mut missing_skills: Vec<String> = Vec::new();

    // name -> pack_id mapping for later scenario linking
    let mut pack_id_by_name: std::collections::HashMap<&'static str, String> =
        std::collections::HashMap::new();

    // Create each pack and assign matching skills
    for def in DEFAULT_PACKS {
        let pack_id = Uuid::new_v4().to_string();
        store.insert_pack(
            &pack_id,
            def.name,
            Some(def.description),
            Some(def.icon),
            Some(def.color),
        )?;
        if def.is_essential {
            store.set_pack_essential(&pack_id, true)?;
        }
        total_packs += 1;

        // Find matching skills by name (case-insensitive); skip + record missing ones.
        for skill_name in def.skill_names {
            let lower = skill_name.to_lowercase();
            match all_skills.iter().find(|s| s.name.to_lowercase() == lower) {
                Some(skill) => {
                    store.add_skill_to_pack(&pack_id, &skill.id)?;
                    total_skills_assigned += 1;
                }
                None => {
                    let key = (*skill_name).to_string();
                    if !missing_skills.contains(&key) {
                        missing_skills.push(key);
                    }
                }
            }
        }

        pack_id_by_name.insert(def.name, pack_id);
    }

    // Create or update named scenarios, then link packs.
    let mut scenarios_created = 0;
    let mut total_scenario_packs = 0;
    let existing_scenarios = store.get_all_scenarios()?;

    for sdef in DEFAULT_SCENARIOS {
        // Find or create scenario by name.
        let scenario_id = match existing_scenarios.iter().find(|s| s.name == sdef.name) {
            Some(existing) => existing.id.clone(),
            None => {
                let now = chrono::Utc::now().timestamp_millis();
                let record = ScenarioRecord {
                    id: Uuid::new_v4().to_string(),
                    name: sdef.name.to_string(),
                    description: Some(sdef.description.to_string()),
                    icon: None,
                    sort_order: 0,
                    created_at: now,
                    updated_at: now,
                    disclosure_mode: sdef.disclosure_mode,
                };
                store.insert_scenario(&record)?;
                scenarios_created += 1;
                record.id
            }
        };

        // Persist disclosure mode (covers both new and existing scenarios).
        store.set_scenario_disclosure_mode(&scenario_id, sdef.disclosure_mode.as_str())?;

        // Link packs. `add_pack_to_scenario` uses INSERT OR IGNORE so it's safe
        // to re-run. When re-seeding an existing scenario we don't unlink prior
        // packs — force-mode already wiped all packs (cascading scenario_packs),
        // and fresh runs start with nothing linked.
        for pack_name in sdef.pack_names {
            if let Some(pack_id) = pack_id_by_name.get(pack_name) {
                store.add_pack_to_scenario(&scenario_id, pack_id)?;
                total_scenario_packs += 1;
            } else {
                // Should not happen unless the taxonomy is internally inconsistent.
                log::warn!(
                    "scenario '{}' references unknown pack '{}'",
                    sdef.name,
                    pack_name
                );
            }
        }
    }

    if !missing_skills.is_empty() {
        log::warn!(
            "pack_seeder: {} skill(s) referenced by packs are not present in the DB: {}",
            missing_skills.len(),
            missing_skills.join(", ")
        );
    }

    Ok(SeedResult {
        packs_created: total_packs,
        skills_assigned: total_skills_assigned,
        scenarios_created,
        scenario_packs_assigned: total_scenario_packs,
        skipped: false,
        missing_skills,
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

    fn pack_by_name<'a>(
        packs: &'a [crate::skill_store::PackRecord],
        name: &str,
    ) -> Option<&'a crate::skill_store::PackRecord> {
        packs.iter().find(|p| p.name == name)
    }

    fn scenario_by_name<'a>(
        scenarios: &'a [ScenarioRecord],
        name: &str,
    ) -> Option<&'a ScenarioRecord> {
        scenarios.iter().find(|s| s.name == name)
    }

    #[test]
    fn seed_creates_all_packs_when_empty() {
        let store = test_store();
        let result = seed_default_packs(&store, false).unwrap();
        assert!(!result.skipped);
        assert_eq!(result.packs_created, DEFAULT_PACKS.len());

        let packs = store.get_all_packs().unwrap();
        assert_eq!(packs.len(), DEFAULT_PACKS.len());
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
        assert_eq!(result.packs_created, DEFAULT_PACKS.len());

        // Old pack should be gone
        let packs = store.get_all_packs().unwrap();
        assert!(packs.iter().all(|p| p.name != "old-pack"));
    }

    #[test]
    fn seed_creates_essential_pack_marked_essential() {
        let store = test_store();
        // Seed the skills the essential pack references so we also verify skill linkage.
        for n in [
            "find-skills",
            "skill-creator",
            "scenario",
            "discover",
            "web-access",
            "smart-search",
            "pack-router-gen",
        ] {
            insert_test_skill(&store, n);
        }

        seed_default_packs(&store, true).unwrap();

        let packs = store.get_all_packs().unwrap();
        let essential = pack_by_name(&packs, "essential").expect("essential pack missing");
        assert!(
            essential.is_essential,
            "essential pack should be marked is_essential"
        );

        let skills = store.get_skills_for_pack(&essential.id).unwrap();
        let names: Vec<_> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"find-skills"));
        assert!(names.contains(&"skill-creator"));
        assert!(names.contains(&"pack-router-gen"));
    }

    #[test]
    fn seed_only_essential_pack_is_essential() {
        let store = test_store();
        seed_default_packs(&store, true).unwrap();

        let packs = store.get_all_packs().unwrap();
        let essential_count = packs.iter().filter(|p| p.is_essential).count();
        assert_eq!(essential_count, 1);
        assert_eq!(
            pack_by_name(&packs, "essential").unwrap().is_essential,
            true
        );
    }

    #[test]
    fn seed_marks_minimal_full_and_standard_hybrid() {
        let store = test_store();
        seed_default_packs(&store, true).unwrap();

        let scenarios = store.get_all_scenarios().unwrap();

        let min = scenario_by_name(&scenarios, "minimal").expect("minimal scenario missing");
        assert_eq!(min.disclosure_mode, DisclosureMode::Full);

        let std = scenario_by_name(&scenarios, "standard").expect("standard scenario missing");
        assert_eq!(std.disclosure_mode, DisclosureMode::Hybrid);

        let everything = scenario_by_name(&scenarios, "everything").expect("everything scenario");
        assert_eq!(everything.disclosure_mode, DisclosureMode::Full);
    }

    #[test]
    fn seed_creates_five_marketing_subpacks() {
        let store = test_store();
        seed_default_packs(&store, true).unwrap();

        let packs = store.get_all_packs().unwrap();
        for n in [
            "mkt-strategy",
            "mkt-seo",
            "mkt-copy",
            "mkt-cro",
            "mkt-revenue",
        ] {
            assert!(
                pack_by_name(&packs, n).is_some(),
                "missing marketing sub-pack: {n}"
            );
        }
    }

    #[test]
    fn seed_creates_all_default_scenarios() {
        let store = test_store();
        seed_default_packs(&store, true).unwrap();

        let scenarios = store.get_all_scenarios().unwrap();
        for sdef in DEFAULT_SCENARIOS {
            assert!(
                scenario_by_name(&scenarios, sdef.name).is_some(),
                "scenario {} not created",
                sdef.name
            );
        }
    }

    #[test]
    fn seed_links_minimal_scenario_to_essential_pack_only() {
        let store = test_store();
        seed_default_packs(&store, true).unwrap();

        let scenarios = store.get_all_scenarios().unwrap();
        let min = scenario_by_name(&scenarios, "minimal").unwrap();
        let packs = store.get_packs_for_scenario(&min.id).unwrap();
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].name, "essential");
    }

    #[test]
    fn seed_links_everything_scenario_to_all_packs() {
        let store = test_store();
        seed_default_packs(&store, true).unwrap();

        let scenarios = store.get_all_scenarios().unwrap();
        let everything = scenario_by_name(&scenarios, "everything").unwrap();
        let packs = store.get_packs_for_scenario(&everything.id).unwrap();
        // everything scenario must include every pack.
        assert_eq!(packs.len(), DEFAULT_PACKS.len());
    }

    #[test]
    fn seed_preserves_existing_scenario_by_name_and_updates_mode() {
        let store = test_store();
        // Pre-create a scenario named "minimal" with Full mode (simulating startup).
        let now = chrono::Utc::now().timestamp_millis();
        let preexisting = ScenarioRecord {
            id: Uuid::new_v4().to_string(),
            name: "minimal".to_string(),
            description: None,
            icon: None,
            sort_order: 0,
            created_at: now,
            updated_at: now,
            disclosure_mode: DisclosureMode::Full,
        };
        store.insert_scenario(&preexisting).unwrap();

        seed_default_packs(&store, true).unwrap();

        // The same scenario id should still exist (we did not delete it).
        let re = store
            .get_scenario_by_id(&preexisting.id)
            .unwrap()
            .expect("preexisting minimal scenario still present");
        assert_eq!(re.name, "minimal");
        assert_eq!(re.disclosure_mode, DisclosureMode::Full);

        // And it should have the essential pack linked.
        let packs = store.get_packs_for_scenario(&preexisting.id).unwrap();
        assert!(packs.iter().any(|p| p.name == "essential"));
    }

    #[test]
    fn seed_records_missing_skills_without_failing() {
        let store = test_store();
        // Only seed one skill out of many — most will be missing.
        insert_test_skill(&store, "find-skills");

        let result = seed_default_packs(&store, false).unwrap();
        assert!(!result.skipped);
        // At least one was linked.
        assert!(result.skills_assigned >= 1);
        // And many were reported missing.
        assert!(!result.missing_skills.is_empty());
        // Known marketing skill that won't exist in a fresh DB.
        assert!(result
            .missing_skills
            .iter()
            .any(|n| n == "marketing-psychology"));
    }

    #[test]
    fn seed_assigns_skill_when_name_case_differs() {
        let store = test_store();
        insert_test_skill(&store, "Find-Skills");
        insert_test_skill(&store, "SKILL-CREATOR");

        let result = seed_default_packs(&store, false).unwrap();
        assert!(result.skills_assigned >= 2);
    }

    #[test]
    fn seed_is_idempotent_across_force_runs() {
        let store = test_store();
        seed_default_packs(&store, true).unwrap();
        let packs_first = store.get_all_packs().unwrap().len();
        let scenarios_first = store.get_all_scenarios().unwrap().len();

        seed_default_packs(&store, true).unwrap();
        assert_eq!(store.get_all_packs().unwrap().len(), packs_first);
        assert_eq!(store.get_all_scenarios().unwrap().len(), scenarios_first);
    }
}
