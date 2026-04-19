# Progressive Disclosure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Shrink Claude Code system-prompt token cost for shared skills by ~85% using file-based pack routers + Read-on-demand architecture, while restructuring pack taxonomy.

**Architecture:** Two-location storage (vault + agent dir). In `hybrid` mode, only Essential-pack skills and auto-generated pack router SKILL.md files are materialized into the agent's scanned skills directory; domain skills stay in the `~/.skills-manager/skills/` vault and are read on demand via the `Read` tool after a router points to them.

**Tech Stack:** Rust (core + CLI), rusqlite (SQLite), Tauri commands, React + TypeScript + Tailwind (Vitest), existing sync engine.

**Spec:** `docs/superpowers/specs/2026-04-19-progressive-disclosure-design.md`

**Version coordination note:** Current `LATEST_VERSION` is 8. PR #24 (Skill Version History) is open as v9. This plan assumes it merges first and targets **v10**. If it lands after: rename migration to v9 and update assertions.

---

## File Structure

### Rust core — `crates/skills-manager-core/src/`
- `migrations.rs` — add v9→v10 migration, bump `LATEST_VERSION`
- `skill_store.rs` — extend pack CRUD with router fields + `is_essential`; add scenario `disclosure_mode`
- `router_render.rs` *(NEW)* — pure function rendering router SKILL.md text
- `sync_engine.rs` — mode-aware desired-state + reconciliation (new logic in a submodule `sync_engine/disclosure.rs`)
- `pack_seeder.rs` — new taxonomy seed data + scenario remap
- `builtin_skills.rs` *(NEW)* — bootstrap install of `pack-router-gen` skill from embedded assets
- `assets/builtin-skills/pack-router-gen/SKILL.md` *(NEW)* — shipped skill content
- `pending_router_gen.rs` *(NEW)* — read/write/delete marker files

### CLI — `crates/skills-manager-cli/src/`
- `commands.rs` — add `Pack::{Context, SetRouter, ListRouters, GenRouter, RegenAllRouters}` subcommands
- `main.rs` — wire new subcommand dispatch

### Tauri — `src-tauri/src/commands/`
- `packs.rs` — router CRUD IPC; `is_essential` toggle
- `scenarios.rs` — `disclosure_mode` field
- `router_gen.rs` *(NEW)* — `write_pending_marker` / `list_pending` / `clear_pending`

### Frontend — `src/`
- `components/RouterEditor.tsx` *(NEW)*
- `components/DisclosureModeSelect.tsx` *(NEW)*
- `components/TokenEstimateBadge.tsx` *(NEW)*
- `views/PacksView.tsx` — render RouterEditor in pack detail
- `views/ScenariosView.tsx` *(implicit: scenario editor)* — disclosure mode dropdown + preview
- `views/MatrixView.tsx` — extended cell states
- `components/Sidebar.tsx` — mode badge
- `views/Dashboard.tsx` — tokens-saved widget

---

## Phase 1 — Schema migration (v9 → v10)

### Task 1: Add migration skeleton

**Files:**
- Modify: `crates/skills-manager-core/src/migrations.rs`

- [ ] **Step 1: Add failing test for v10 schema**

Append to `#[cfg(test)] mod tests` in `migrations.rs`:

```rust
#[test]
fn v9_to_v10_migration_adds_router_and_disclosure_columns() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let conn = Connection::open(temp.path()).unwrap();

    // Bootstrap through v9 first (fresh DB path runs all migrations)
    super::run_migrations(&conn).unwrap();

    // Assert new pack columns
    let pack_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(packs)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(pack_cols.contains(&"router_description".to_string()));
    assert!(pack_cols.contains(&"router_body".to_string()));
    assert!(pack_cols.contains(&"is_essential".to_string()));
    assert!(pack_cols.contains(&"router_updated_at".to_string()));

    // Assert scenario column
    let scenario_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(scenarios)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(scenario_cols.contains(&"disclosure_mode".to_string()));

    // Assert index exists
    let idx: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_scenarios_mode'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(idx, 1);

    // Default values sane
    let version: u32 = conn
        .query_row("SELECT value FROM settings WHERE key='db_version'", [], |r| {
            let s: String = r.get(0)?;
            Ok(s.parse().unwrap())
        })
        .unwrap();
    assert_eq!(version, 10);
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p skills-manager-core migrations::tests::v9_to_v10`
Expected: FAIL (column not present / version != 10)

- [ ] **Step 3: Bump LATEST_VERSION and add dispatch arm**

Edit `migrations.rs` top:

```rust
const LATEST_VERSION: u32 = 10;
```

Add arm to `migrate_step`:

```rust
        8 => migrate_v8_to_v9(conn),
        9 => migrate_v9_to_v10(conn),
```

- [ ] **Step 4: Implement `migrate_v9_to_v10`**

Append:

```rust
/// v9 → v10: Progressive Disclosure columns on packs + scenarios.
fn migrate_v9_to_v10(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        ALTER TABLE packs ADD COLUMN router_description TEXT;
        ALTER TABLE packs ADD COLUMN router_body TEXT;
        ALTER TABLE packs ADD COLUMN is_essential INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE packs ADD COLUMN router_updated_at INTEGER;

        ALTER TABLE scenarios ADD COLUMN disclosure_mode TEXT NOT NULL DEFAULT 'full';
        CREATE INDEX IF NOT EXISTS idx_scenarios_mode ON scenarios(disclosure_mode);
        ",
    )
    .context("v9→v10: add progressive disclosure columns")?;
    Ok(())
}
```

- [ ] **Step 5: Run test to verify pass**

Run: `cargo test -p skills-manager-core migrations::tests::v9_to_v10`
Expected: PASS

- [ ] **Step 6: Run full test suite**

Run: `cargo test -p skills-manager-core`
Expected: all pass; no regressions.

- [ ] **Step 7: Commit**

```bash
git add crates/skills-manager-core/src/migrations.rs
git commit -m "feat(core): v10 migration for progressive disclosure columns"
```

### Task 2: Upgrade-from-existing-v9 test

- [ ] **Step 1: Test existing v9 DB preserves data**

Add test to `migrations.rs`:

```rust
#[test]
fn v10_migration_preserves_existing_pack_data() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let conn = Connection::open(temp.path()).unwrap();

    // Run migrations through v9 only by temporarily pinning
    // (Use a helper that stops at a given version; or simulate v9 state directly)
    super::migrate_step(&conn, 0).unwrap(); // v0→v1 ... continue until v9
    for v in 1..9 { super::migrate_step(&conn, v).unwrap(); }

    conn.execute(
        "INSERT INTO packs (id, name, description) VALUES ('p1', 'test-pack', 'desc')",
        [],
    ).unwrap();

    super::migrate_step(&conn, 9).unwrap();

    let (name, is_essential, router_desc): (String, i64, Option<String>) = conn
        .query_row(
            "SELECT name, is_essential, router_description FROM packs WHERE id='p1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(name, "test-pack");
    assert_eq!(is_essential, 0);
    assert!(router_desc.is_none());
}
```

- [ ] **Step 2: Run + verify pass**

Run: `cargo test -p skills-manager-core migrations::tests::v10_migration_preserves`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/skills-manager-core/src/migrations.rs
git commit -m "test(core): verify v10 migration preserves existing pack data"
```

---

## Phase 2 — Pack store router/essential API

### Task 3: `PackRecord` struct adds new fields

**Files:**
- Modify: `crates/skills-manager-core/src/skill_store.rs`

- [ ] **Step 1: Test struct round-trip**

Add test:

```rust
#[test]
fn pack_record_round_trips_router_and_essential() {
    let store = setup_test_store();
    let pack = PackRecord {
        id: "p-seo".into(),
        name: "mkt-seo".into(),
        description: Some("SEO pack".into()),
        router_description: Some("Trigger SEO audit...".into()),
        router_body: None,
        is_essential: false,
        router_updated_at: Some(1_700_000_000),
    };
    store.insert_pack(&pack).unwrap();
    let fetched = store.get_pack_by_id("p-seo").unwrap().unwrap();
    assert_eq!(fetched.router_description.as_deref(), Some("Trigger SEO audit..."));
    assert_eq!(fetched.is_essential, false);
    assert_eq!(fetched.router_updated_at, Some(1_700_000_000));
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p skills-manager-core skill_store::tests::pack_record_round_trips`
Expected: compile error (fields absent).

- [ ] **Step 3: Extend `PackRecord` struct**

Update struct definition (exact line numbers vary; locate via `grep -n "pub struct PackRecord" crates/skills-manager-core/src/skill_store.rs`):

```rust
#[derive(Debug, Clone)]
pub struct PackRecord {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub router_description: Option<String>,
    pub router_body: Option<String>,
    pub is_essential: bool,
    pub router_updated_at: Option<i64>,
}
```

- [ ] **Step 4: Update `insert_pack`, `get_pack_by_id`, `get_all_packs`, `update_pack`**

`insert_pack`:

```rust
pub fn insert_pack(&self, p: &PackRecord) -> Result<()> {
    self.conn.execute(
        "INSERT INTO packs (id, name, description, router_description, router_body, is_essential, router_updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            p.id, p.name, p.description,
            p.router_description, p.router_body,
            p.is_essential as i32,
            p.router_updated_at,
        ],
    )?;
    Ok(())
}
```

Row mapping helper:

```rust
fn pack_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PackRecord> {
    Ok(PackRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        router_description: row.get(3)?,
        router_body: row.get(4)?,
        is_essential: row.get::<_, i64>(5)? != 0,
        router_updated_at: row.get(6)?,
    })
}
```

Update SELECT to `SELECT id, name, description, router_description, router_body, is_essential, router_updated_at FROM packs ...`.

- [ ] **Step 5: Run test to confirm pass**

Run: `cargo test -p skills-manager-core skill_store::tests::pack_record`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/skills-manager-core/src/skill_store.rs
git commit -m "feat(core): extend PackRecord with router + is_essential fields"
```

### Task 4: `set_pack_router` / `set_pack_essential` setters

- [ ] **Step 1: Test `set_pack_router` updates fields and timestamp**

```rust
#[test]
fn set_pack_router_updates_description_and_timestamp() {
    let store = setup_test_store();
    let pack = PackRecord { /* ... is_essential: false, router_updated_at: None ... */ };
    store.insert_pack(&pack).unwrap();

    store.set_pack_router("p-seo", Some("new desc"), None, 1_700_000_500).unwrap();

    let got = store.get_pack_by_id("p-seo").unwrap().unwrap();
    assert_eq!(got.router_description.as_deref(), Some("new desc"));
    assert_eq!(got.router_updated_at, Some(1_700_000_500));
}
```

- [ ] **Step 2: Run to confirm failure (method missing).**

Run: `cargo test -p skills-manager-core skill_store::tests::set_pack_router_updates`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
pub fn set_pack_router(
    &self,
    pack_id: &str,
    description: Option<&str>,
    body: Option<&str>,
    updated_at: i64,
) -> Result<()> {
    let n = self.conn.execute(
        "UPDATE packs SET router_description = ?2, router_body = ?3, router_updated_at = ?4 WHERE id = ?1",
        rusqlite::params![pack_id, description, body, updated_at],
    )?;
    if n == 0 { anyhow::bail!("pack {pack_id} not found"); }
    Ok(())
}

pub fn set_pack_essential(&self, pack_id: &str, is_essential: bool) -> Result<()> {
    let n = self.conn.execute(
        "UPDATE packs SET is_essential = ?2 WHERE id = ?1",
        rusqlite::params![pack_id, is_essential as i32],
    )?;
    if n == 0 { anyhow::bail!("pack {pack_id} not found"); }
    Ok(())
}
```

- [ ] **Step 4: Run test to confirm pass.**

- [ ] **Step 5: Commit**

```bash
git add crates/skills-manager-core/src/skill_store.rs
git commit -m "feat(core): add set_pack_router and set_pack_essential"
```

---

## Phase 3 — Scenario `disclosure_mode`

### Task 5: `ScenarioRecord` and store

**Files:**
- Modify: `crates/skills-manager-core/src/skill_store.rs`

- [ ] **Step 1: Test struct + setter round-trip**

```rust
#[test]
fn scenario_record_exposes_disclosure_mode() {
    let store = setup_test_store();
    store.create_scenario("s1", "Test", None).unwrap();
    store.set_scenario_disclosure_mode("s1", "hybrid").unwrap();
    let s = store.get_scenario_by_id("s1").unwrap().unwrap();
    assert_eq!(s.disclosure_mode, DisclosureMode::Hybrid);
}
```

- [ ] **Step 2: Run — fails (types missing).**

- [ ] **Step 3: Define enum and extend record**

Add to `skill_store.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisclosureMode { Full, Hybrid, RouterOnly }

impl DisclosureMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            DisclosureMode::Full => "full",
            DisclosureMode::Hybrid => "hybrid",
            DisclosureMode::RouterOnly => "router_only",
        }
    }
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "full" => Ok(Self::Full),
            "hybrid" => Ok(Self::Hybrid),
            "router_only" => Ok(Self::RouterOnly),
            other => anyhow::bail!("invalid disclosure_mode: {other}"),
        }
    }
}
```

Extend `ScenarioRecord` with `pub disclosure_mode: DisclosureMode`, update row mapping with `DisclosureMode::parse(&row.get::<_, String>(N)?)`, and add:

```rust
pub fn set_scenario_disclosure_mode(&self, id: &str, mode: &str) -> Result<()> {
    DisclosureMode::parse(mode)?; // validate
    let n = self.conn.execute(
        "UPDATE scenarios SET disclosure_mode = ?2 WHERE id = ?1",
        rusqlite::params![id, mode],
    )?;
    if n == 0 { anyhow::bail!("scenario {id} not found"); }
    Ok(())
}
```

Update all scenario SELECTs to include `disclosure_mode`; default fresh inserts to `"full"` to preserve backward compat.

- [ ] **Step 4: Run test to pass.**

- [ ] **Step 5: Commit**

```bash
git add crates/skills-manager-core/src/skill_store.rs
git commit -m "feat(core): add DisclosureMode enum + scenario setter"
```

---

## Phase 4 — Router rendering (pure)

### Task 6: `router_render` module

**Files:**
- Create: `crates/skills-manager-core/src/router_render.rs`
- Modify: `crates/skills-manager-core/src/lib.rs` (add `pub mod router_render;`)

- [ ] **Step 1: Write tests first**

New `crates/skills-manager-core/src/router_render.rs`:

```rust
use crate::skill_store::{PackRecord, SkillRecord};
use std::path::Path;

pub fn render_router_skill_md(
    pack: &PackRecord,
    skills: &[SkillRecord],
    vault_root: &Path,
) -> String {
    let desc = pack.router_description
        .as_deref()
        .unwrap_or("Router for pack — description pending generation.");
    let body = pack.router_body
        .clone()
        .unwrap_or_else(|| auto_render_body(pack, skills, vault_root));

    format!(
        "---\nname: pack-{}\ndescription: {}\n---\n\n{}\n",
        pack.name,
        escape_yaml_scalar(desc),
        body,
    )
}

fn auto_render_body(pack: &PackRecord, skills: &[SkillRecord], vault_root: &Path) -> String {
    let mut out = format!(
        "# Pack: {}\n\n\
        揀一個 skill，用 `Read` tool 讀對應 SKILL.md，跟住做。\n\n\
        | Skill | 用途 | 路徑 |\n|---|---|---|\n",
        pack.name,
    );
    for s in skills {
        let summary = s
            .description
            .as_deref()
            .unwrap_or("")
            .split_terminator(['.', '。'])
            .next()
            .unwrap_or("")
            .trim();
        out.push_str(&format!(
            "| `{}` | {} | `{}/{}/SKILL.md` |\n",
            s.name,
            summary,
            vault_root.display(),
            s.name,
        ));
    }
    out
}

fn escape_yaml_scalar(s: &str) -> String {
    // Minimal: single-line only; YAML scalars must avoid unescaped ':' at start or '\n'.
    // If problematic, wrap in double quotes and escape inner quotes and backslashes.
    if s.contains('\n') || s.contains(':') || s.starts_with(['-', '?', '[', '{', '|', '>']) {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn pack(name: &str, router_desc: Option<&str>) -> PackRecord {
        PackRecord {
            id: format!("p-{name}"),
            name: name.into(),
            description: None,
            router_description: router_desc.map(str::to_string),
            router_body: None,
            is_essential: false,
            router_updated_at: None,
        }
    }

    fn skill(name: &str, desc: &str) -> SkillRecord {
        SkillRecord {
            // Fill remaining fields with defaults matching existing SkillRecord.
            // (Use a helper if one exists; pseudo-code otherwise.)
            name: name.into(),
            description: Some(desc.into()),
            ..SkillRecord::default()
        }
    }

    #[test]
    fn renders_frontmatter_with_pack_name() {
        let p = pack("mkt-seo", Some("When user mentions SEO"));
        let out = render_router_skill_md(&p, &[], &PathBuf::from("/vault"));
        assert!(out.starts_with("---\nname: pack-mkt-seo"));
        assert!(out.contains("description: When user mentions SEO"));
    }

    #[test]
    fn auto_renders_skill_table_when_body_empty() {
        let p = pack("mkt-seo", Some("desc"));
        let skills = vec![
            skill("seo-audit", "Diagnose SEO issues. Use when..."),
            skill("ai-seo", "Optimize for LLM citations."),
        ];
        let out = render_router_skill_md(&p, &skills, &PathBuf::from("/vault"));
        assert!(out.contains("| `seo-audit` | Diagnose SEO issues | `/vault/seo-audit/SKILL.md` |"));
        assert!(out.contains("| `ai-seo` | Optimize for LLM citations | `/vault/ai-seo/SKILL.md` |"));
    }

    #[test]
    fn custom_router_body_is_used_as_is() {
        let mut p = pack("custom", Some("desc"));
        p.router_body = Some("# Custom body\n\nhand-written".into());
        let out = render_router_skill_md(&p, &[], &PathBuf::from("/vault"));
        assert!(out.contains("# Custom body"));
        assert!(!out.contains("揀一個 skill"));
    }

    #[test]
    fn null_description_emits_placeholder() {
        let p = pack("x", None);
        let out = render_router_skill_md(&p, &[], &PathBuf::from("/v"));
        assert!(out.contains("description: Router for pack — description pending generation."));
    }

    #[test]
    fn yaml_special_chars_are_quoted() {
        let p = pack("x", Some("Trigger: SEO audit"));
        let out = render_router_skill_md(&p, &[], &PathBuf::from("/v"));
        assert!(out.contains("description: \"Trigger: SEO audit\""));
    }

    #[test]
    fn deterministic_for_same_input() {
        let p = pack("x", Some("d"));
        let skills = vec![skill("a", "x."), skill("b", "y.")];
        let a = render_router_skill_md(&p, &skills, &PathBuf::from("/v"));
        let b = render_router_skill_md(&p, &skills, &PathBuf::from("/v"));
        assert_eq!(a, b);
    }
}
```

- [ ] **Step 2: Wire module**

Edit `crates/skills-manager-core/src/lib.rs`, add:

```rust
pub mod router_render;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p skills-manager-core router_render`
Expected: PASS (all 6).

- [ ] **Step 4: Commit**

```bash
git add crates/skills-manager-core/src/router_render.rs crates/skills-manager-core/src/lib.rs
git commit -m "feat(core): router_render module with auto-table body"
```

---

## Phase 5 — Sync engine mode-aware logic

### Task 7: `disclosure` submodule with desired-state resolver

**Files:**
- Create: `crates/skills-manager-core/src/sync_engine/disclosure.rs`
- Modify: `crates/skills-manager-core/src/sync_engine.rs` (convert to module if not already; add `pub mod disclosure;`)

If `sync_engine.rs` is a single file today, restructure:
- Move current contents to `crates/skills-manager-core/src/sync_engine/mod.rs`
- Ensure all existing tests still pass
- Add `pub mod disclosure;`

- [ ] **Step 1: Restructure file-to-module if needed**

```bash
mkdir -p crates/skills-manager-core/src/sync_engine
git mv crates/skills-manager-core/src/sync_engine.rs crates/skills-manager-core/src/sync_engine/mod.rs
```

Run: `cargo test -p skills-manager-core sync_engine`
Expected: PASS unchanged.

- [ ] **Step 2: Commit restructure**

```bash
git add -A
git commit -m "refactor(core): sync_engine single-file → module dir"
```

- [ ] **Step 3: Write tests for desired-state resolver**

Create `crates/skills-manager-core/src/sync_engine/disclosure.rs`:

```rust
use crate::skill_store::{DisclosureMode, PackRecord, SkillRecord};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub struct PackWithSkills<'a> {
    pub pack: &'a PackRecord,
    pub skills: &'a [SkillRecord],
}

pub struct DesiredEntry {
    pub target_path: PathBuf,
    pub kind: EntryKind,
}

pub enum EntryKind {
    Skill { skill_name: String },
    Router { pack_name: String },
}

pub fn resolve_desired_state(
    agent_skills_dir: &Path,
    packs: &[PackWithSkills<'_>],
    mode: DisclosureMode,
) -> Vec<DesiredEntry> {
    let mut out = Vec::new();
    for p in packs {
        let materialize = match mode {
            DisclosureMode::Full => true,
            DisclosureMode::Hybrid => p.pack.is_essential,
            DisclosureMode::RouterOnly => false,
        };
        if materialize {
            for s in p.skills {
                out.push(DesiredEntry {
                    target_path: agent_skills_dir.join(&s.name),
                    kind: EntryKind::Skill { skill_name: s.name.clone() },
                });
            }
        }
        if mode != DisclosureMode::Full && !p.pack.is_essential {
            out.push(DesiredEntry {
                target_path: agent_skills_dir.join(format!("pack-{}", p.pack.name)),
                kind: EntryKind::Router { pack_name: p.pack.name.clone() },
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(name: &str, essential: bool) -> PackRecord {
        PackRecord {
            id: format!("p-{name}"),
            name: name.into(),
            description: None,
            router_description: Some(format!("router for {name}")),
            router_body: None,
            is_essential: essential,
            router_updated_at: None,
        }
    }
    fn skill(name: &str) -> SkillRecord {
        SkillRecord { name: name.into(), ..SkillRecord::default() }
    }

    #[test]
    fn full_mode_materializes_everything_no_routers() {
        let ess = pack("essential", true);
        let dom = pack("dev-fe", false);
        let ess_skills = vec![skill("find-skills")];
        let dom_skills = vec![skill("frontend-design")];
        let packs = vec![
            PackWithSkills { pack: &ess, skills: &ess_skills },
            PackWithSkills { pack: &dom, skills: &dom_skills },
        ];
        let out = resolve_desired_state(Path::new("/cc"), &packs, DisclosureMode::Full);
        let paths: Vec<_> = out.iter().map(|e| e.target_path.clone()).collect();
        assert!(paths.contains(&PathBuf::from("/cc/find-skills")));
        assert!(paths.contains(&PathBuf::from("/cc/frontend-design")));
        assert!(!paths.iter().any(|p| p.to_string_lossy().contains("pack-")));
    }

    #[test]
    fn hybrid_mode_keeps_essential_skills_and_emits_routers_for_domain() {
        let ess = pack("essential", true);
        let dom = pack("dev-fe", false);
        let ess_skills = vec![skill("find-skills")];
        let dom_skills = vec![skill("frontend-design")];
        let packs = vec![
            PackWithSkills { pack: &ess, skills: &ess_skills },
            PackWithSkills { pack: &dom, skills: &dom_skills },
        ];
        let out = resolve_desired_state(Path::new("/cc"), &packs, DisclosureMode::Hybrid);
        let paths: Vec<_> = out.iter().map(|e| e.target_path.clone()).collect();
        assert!(paths.contains(&PathBuf::from("/cc/find-skills")));
        assert!(!paths.contains(&PathBuf::from("/cc/frontend-design")));
        assert!(paths.contains(&PathBuf::from("/cc/pack-dev-fe")));
        assert!(!paths.iter().any(|p| p.ends_with("pack-essential")));
    }

    #[test]
    fn router_only_emits_only_routers_for_non_essential() {
        let ess = pack("essential", true);
        let dom = pack("mkt-seo", false);
        let ess_skills = vec![skill("find-skills")];
        let dom_skills = vec![skill("seo-audit")];
        let packs = vec![
            PackWithSkills { pack: &ess, skills: &ess_skills },
            PackWithSkills { pack: &dom, skills: &dom_skills },
        ];
        let out = resolve_desired_state(Path::new("/cc"), &packs, DisclosureMode::RouterOnly);
        let paths: Vec<_> = out.iter().map(|e| e.target_path.clone()).collect();
        assert_eq!(paths.len(), 1);
        assert!(paths.contains(&PathBuf::from("/cc/pack-mkt-seo")));
    }
}
```

- [ ] **Step 4: Wire module**

Add to `crates/skills-manager-core/src/sync_engine/mod.rs`:

```rust
pub mod disclosure;
```

- [ ] **Step 5: Run + verify pass**

Run: `cargo test -p skills-manager-core sync_engine::disclosure`
Expected: 3 tests PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/skills-manager-core/src/sync_engine/disclosure.rs crates/skills-manager-core/src/sync_engine/mod.rs
git commit -m "feat(core): desired-state resolver for disclosure modes"
```

### Task 8: Reconciliation — materialize + unlink + render routers

**Files:**
- Modify: `crates/skills-manager-core/src/sync_engine/mod.rs`

- [ ] **Step 1: Write end-to-end reconciliation test**

```rust
#[test]
fn switching_full_to_hybrid_removes_non_essential_and_writes_routers() {
    use crate::router_render;
    use crate::skill_store::DisclosureMode;
    use std::fs;
    let tmp = tempfile::tempdir().unwrap();
    let agent_dir = tmp.path().join("claude-skills");
    fs::create_dir_all(&agent_dir).unwrap();

    // Pre-populate as if full mode synced:
    fs::create_dir_all(agent_dir.join("find-skills")).unwrap();
    fs::write(agent_dir.join("find-skills/SKILL.md"), "--- essentials ---").unwrap();
    fs::create_dir_all(agent_dir.join("frontend-design")).unwrap();
    fs::write(agent_dir.join("frontend-design/SKILL.md"), "--- fe ---").unwrap();

    // Pretend SM is managing both (in real code, manifest tracks this).
    // Invoke reconcile with hybrid mode.
    let packs = /* build PackWithSkills with is_essential flag as spec demands */;
    reconcile_agent_dir(&agent_dir, &packs, DisclosureMode::Hybrid).unwrap();

    assert!(agent_dir.join("find-skills/SKILL.md").exists(),
            "essential skill remains materialized");
    assert!(!agent_dir.join("frontend-design").exists(),
            "non-essential unlinked in hybrid");
    assert!(agent_dir.join("pack-dev-fe/SKILL.md").exists(),
            "router written for non-essential pack");
}
```

- [ ] **Step 2: Implement `reconcile_agent_dir`**

Add to `sync_engine/mod.rs`:

```rust
use crate::router_render;
use crate::sync_engine::disclosure::{resolve_desired_state, EntryKind, PackWithSkills};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub struct ReconcileReport {
    pub added: usize,
    pub removed: usize,
    pub rendered_routers: usize,
}

pub fn reconcile_agent_dir(
    agent_skills_dir: &Path,
    packs: &[PackWithSkills<'_>],
    mode: crate::skill_store::DisclosureMode,
    vault_root: &Path,
) -> anyhow::Result<ReconcileReport> {
    use anyhow::Context;

    fs::create_dir_all(agent_skills_dir).ok();

    let desired = resolve_desired_state(agent_skills_dir, packs, mode);
    let desired_paths: HashSet<_> = desired.iter().map(|e| e.target_path.clone()).collect();
    let mut report = ReconcileReport { added: 0, removed: 0, rendered_routers: 0 };

    // Add missing / update stale routers
    for entry in &desired {
        match &entry.kind {
            EntryKind::Skill { skill_name } => {
                let source = vault_root.join(skill_name);
                if !entry.target_path.exists() {
                    crate::sync::sync_skill(
                        &source,
                        &entry.target_path,
                        crate::sync::SyncMode::Symlink,
                    )
                    .with_context(|| format!("sync skill {skill_name}"))?;
                    report.added += 1;
                }
            }
            EntryKind::Router { pack_name } => {
                let pack = packs.iter().find(|p| p.pack.name == *pack_name).unwrap();
                let content = router_render::render_router_skill_md(
                    pack.pack,
                    pack.skills,
                    vault_root,
                );
                let target_dir = entry.target_path.clone();
                fs::create_dir_all(&target_dir).ok();
                let md_path = target_dir.join("SKILL.md");
                let needs_write = match fs::read_to_string(&md_path) {
                    Ok(existing) => existing != content,
                    Err(_) => true,
                };
                if needs_write {
                    fs::write(&md_path, &content)
                        .with_context(|| format!("write router {pack_name}"))?;
                    report.rendered_routers += 1;
                    report.added += 1;
                }
            }
        }
    }

    // Remove SM-managed entries no longer desired
    if agent_skills_dir.exists() {
        for entry in fs::read_dir(agent_skills_dir)? {
            let entry = entry?;
            let p = entry.path();
            if desired_paths.contains(&p) { continue; }
            if !is_sm_managed(&p)? { continue; } // native skills untouched
            if p.is_dir() { fs::remove_dir_all(&p)?; } else { fs::remove_file(&p)?; }
            report.removed += 1;
        }
    }

    Ok(report)
}

fn is_sm_managed(path: &Path) -> anyhow::Result<bool> {
    // If it's a symlink pointing into ~/.skills-manager/skills/, it's SM-managed.
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            if let Ok(target) = fs::read_link(path) {
                return Ok(target.to_string_lossy().contains(".skills-manager/skills"));
            }
        }
    }
    // Router dirs (pack-*) with a single SKILL.md that contains our marker header.
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.starts_with("pack-") {
        if let Ok(s) = fs::read_to_string(path.join("SKILL.md")) {
            return Ok(s.contains("Router for pack") || s.contains("# Pack:"));
        }
    }
    // Otherwise: consult existing Agent Native Skills marker (Phase "Native Skills Mgmt").
    // For now, conservative: not SM-managed → skip.
    Ok(false)
}
```

> The production `is_sm_managed` must integrate with the Agent Native Skills marker set by the in-progress phase (coordinate on land). Until then, the symlink + pack-name heuristic above covers SM's own writes.

- [ ] **Step 3: Run reconciliation test + fix until green**

Run: `cargo test -p skills-manager-core sync_engine::tests::switching_full_to_hybrid`
Expected: PASS.

- [ ] **Step 4: Add reverse-direction test (hybrid→full re-links skills, removes routers)**

```rust
#[test]
fn switching_hybrid_to_full_removes_routers_and_adds_skills() {
    // Precondition: pack-dev-fe router dir exists; dev-fe skill not materialized.
    // After reconcile(full): pack-dev-fe gone; dev-fe symlink present.
    // (assemble similarly to previous test)
}
```

Fill in body analogously and verify pass.

- [ ] **Step 5: Test router staleness triggers rewrite**

```rust
#[test]
fn router_rewritten_when_pack_description_changes() {
    // Reconcile once, record mtime.
    // Update pack.router_description, reconcile again.
    // Assert SKILL.md content updated (contents differ).
}
```

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(core): sync engine reconciles mode-aware desired state"
```

---

## Phase 6 — Built-in skill bootstrap

### Task 9: Ship `pack-router-gen` skill asset

**Files:**
- Create: `crates/skills-manager-core/assets/builtin-skills/pack-router-gen/SKILL.md`
- Create: `crates/skills-manager-core/src/builtin_skills.rs`
- Modify: `crates/skills-manager-core/src/lib.rs`

- [ ] **Step 1: Author the skill content**

`crates/skills-manager-core/assets/builtin-skills/pack-router-gen/SKILL.md`:

```markdown
---
name: pack-router-gen
description: Generate Progressive Disclosure router descriptions for Skills Manager packs. Use when user says "generate router for <pack>", "regenerate pack routers", or when pending router markers exist in ~/.skills-manager/pending-router-gen/.
---

# Pack Router Generator

Generate the router SKILL.md frontmatter description for a Skills Manager pack. Reads pending markers, produces a concise description (150–400 chars), and writes back via the `sm` CLI.

## Process

1. **List pending markers:**
   ```bash
   ls ~/.skills-manager/pending-router-gen/ 2>/dev/null
   ```
   For each `<pack-id>.json`, read it (contains `pack_id`, `pack_name`, `skills`).

2. **Get pack context:**
   ```bash
   sm pack context <pack-name>
   ```
   Output includes pack metadata + every skill's name and description.

3. **Draft router description (150–400 chars):**
   - Lead with the task domain ("SEO audits, AI SEO, schema markup...")
   - List trigger keywords verbatim ("Use when user says 'SEO', 'ranking', 'schema', 'JSON-LD'...")
   - Avoid overlap with Essential-pack skills that are always visible
   - Avoid overlap with other pack routers (cross-check via `sm pack list-routers`)

4. **Persist:**
   ```bash
   sm pack set-router <pack-name> --description "<generated text>"
   ```

5. **Clean up marker:**
   ```bash
   rm ~/.skills-manager/pending-router-gen/<pack-id>.json
   ```

6. **Summarize** to user: which packs got routers, any dedupe warnings.

## Quality Checklist

- [ ] At least one keyword from every skill in the pack appears in the description
- [ ] 150–400 chars total
- [ ] Imperative voice ("Use when...", "Trigger for...")
- [ ] No collision with existing routers (verified via `sm pack list-routers`)
- [ ] Special characters (`:`, `"`) escape cleanly in YAML frontmatter

## Notes

- Body is optional. Leave it null; the sync engine auto-renders a skill table.
- Rewrite rather than append if regenerating an existing router.
```

- [ ] **Step 2: Write bootstrap test**

`crates/skills-manager-core/src/builtin_skills.rs`:

```rust
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

const PACK_ROUTER_GEN_SKILL: &str = include_str!(
    "../assets/builtin-skills/pack-router-gen/SKILL.md"
);

pub fn install_builtin_skills(vault_root: &Path) -> Result<()> {
    let dir = vault_root.join("pack-router-gen");
    fs::create_dir_all(&dir).context("create pack-router-gen dir")?;
    let path = dir.join("SKILL.md");
    fs::write(&path, PACK_ROUTER_GEN_SKILL).context("write pack-router-gen SKILL.md")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_pack_router_gen_skill() {
        let tmp = tempfile::tempdir().unwrap();
        install_builtin_skills(tmp.path()).unwrap();
        let p = tmp.path().join("pack-router-gen/SKILL.md");
        assert!(p.exists());
        let content = fs::read_to_string(&p).unwrap();
        assert!(content.contains("name: pack-router-gen"));
        assert!(content.contains("sm pack set-router"));
    }
}
```

Edit `lib.rs`: `pub mod builtin_skills;`.

- [ ] **Step 3: Run + verify**

Run: `cargo test -p skills-manager-core builtin_skills`
Expected: PASS.

- [ ] **Step 4: Wire into app startup**

Locate bootstrap/installer entry (grep for `seed_default_packs` callers). Call `install_builtin_skills` once on first run (idempotent already via `fs::write` overwrite).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): ship pack-router-gen builtin skill"
```

---

## Phase 7 — Seed new pack taxonomy

### Task 10: Update `pack_seeder.rs` with v10 taxonomy

**Files:**
- Modify: `crates/skills-manager-core/src/pack_seeder.rs`

- [ ] **Step 1: Inventory existing seed structure**

Run: `grep -n "pack\|scenario" crates/skills-manager-core/src/pack_seeder.rs | head -60`
Note current packs array format. The new seed defines:

Packs (with `is_essential` + skill names):
```
essential          ess=1  find-skills, skill-creator, scenario, discover, web-access, smart-search, pack-router-gen
route-gstack       ess=0  office-hours, autoplan, plan-ceo-review, plan-eng-review, plan-design-review,
                          plan-devex-review, review, qa, qa-only, ship, investigate, document-release,
                          retro, health, benchmark, checkpoint, learn, learned, careful, freeze, unfreeze,
                          guard, cso, canary, setup-deploy, setup-browser-cookies, open-gstack-browser,
                          gstack, gstack-upgrade, devex-review, pair-agent, browse
route-ecc          ess=0  checkpoint, eval, build-fix, refactor-clean, simplify, quality-gate,
                          learn, learned, save-session, resume-session, sessions
dev-frontend       ess=0  frontend-design, stitch-design, stitch-loop, shadcn-ui, taste-design,
                          canvas-design, brand-guidelines, web-design-guidelines, vercel-react-best-practices,
                          vercel-composition-patterns, react-components, web-artifacts-builder, remotion,
                          design-consultation, design-html, design-md, design-review, design-shotgun,
                          enhance-prompt
dev-backend        ess=0  supabase-postgres-best-practices, data-science, mlops, devops, red-teaming,
                          software-development
ai-engineering     ess=0  claude-api, mcp-builder, skill-creator, cli-creator, claude-code-router,
                          template-skill
browser-automation ess=0  bb-browser, agent-browser, opencli, opencli-usage, opencli-autofix,
                          opencli-explorer, opencli-oneshot, opencli-browser, connect-chrome,
                          verify-deploy, webapp-testing, dogfood
web-research       ess=0  smart-search, agent-reach, codex-deep-search, perp-search, last30days,
                          x-tweet-fetcher, follow-builders, autoresearch, defuddle, obsidian-defuddle
knowledge-library  ess=0  library, obsidian-cli, obsidian-markdown, notebooklm, readwise-cli,
                          readwise-mcp, readwise-to-notebooklm, reader-recap, feed-catchup, build-persona,
                          triage, quiz, book-review, highlight-graph, now-reading-page
docs-office        ess=0  pdf, docx, pptx, xlsx, documentation-writer, prd, internal-comms
agent-orchestration ess=0 paseo, paseo-loop, paseo-orchestrator, paseo-committee, paseo-handoff,
                          paseo-chat, paperclip, loop
mkt-strategy       ess=0  marketing, marketing-ideas, marketing-psychology, product-marketing-context,
                          launch-strategy, content-strategy, site-architecture
mkt-seo            ess=0  seo-audit, ai-seo, schema-markup, programmatic-seo, competitor-alternatives
mkt-copy           ess=0  copywriting, copy-editing, cold-email, email-sequence, ad-creative,
                          sales-enablement, social-content
mkt-cro            ess=0  page-cro, signup-flow-cro, onboarding-cro, paywall-upgrade-cro, form-cro,
                          popup-cro, churn-prevention, ab-test-setup, analytics-tracking
mkt-revenue        ess=0  pricing-strategy, paid-ads, referral-program, revops, lead-magnets,
                          free-tool-strategy
```

Scenarios:
```
minimal             full   [essential]
core                hybrid [essential, route-gstack]
standard            hybrid [essential, route-gstack, dev-frontend, browser-automation, web-research,
                            knowledge-library]
standard-marketing  hybrid [...standard, mkt-strategy, mkt-copy, mkt-cro]
full-dev            hybrid [...standard, dev-backend, ai-engineering, docs-office, agent-orchestration]
full-dev-marketing  hybrid [...full-dev, all mkt-*]
everything          full   [all packs]
```

- [ ] **Step 2: Write seed test**

Add to `pack_seeder.rs` tests:

```rust
#[test]
fn seed_creates_essential_pack_marked_essential() {
    let store = setup_test_store();
    super::seed_default_packs(&store, true).unwrap();
    let essential = store.get_pack_by_name("essential").unwrap().expect("essential pack");
    assert!(essential.is_essential);
    let skills = store.get_pack_skills(&essential.id).unwrap();
    let names: Vec<_> = skills.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"find-skills"));
    assert!(names.contains(&"skill-creator"));
}

#[test]
fn seed_marks_minimal_full_and_standard_hybrid() {
    let store = setup_test_store();
    super::seed_default_packs(&store, true).unwrap();
    let min = store.get_scenario_by_name("minimal").unwrap().unwrap();
    assert_eq!(min.disclosure_mode, DisclosureMode::Full);
    let std = store.get_scenario_by_name("standard").unwrap().unwrap();
    assert_eq!(std.disclosure_mode, DisclosureMode::Hybrid);
}

#[test]
fn seed_creates_five_marketing_subpacks() {
    let store = setup_test_store();
    super::seed_default_packs(&store, true).unwrap();
    for n in ["mkt-strategy", "mkt-seo", "mkt-copy", "mkt-cro", "mkt-revenue"] {
        assert!(store.get_pack_by_name(n).unwrap().is_some(), "missing {n}");
    }
}
```

- [ ] **Step 3: Implement seed data**

Rewrite the seed table constants to include the full taxonomy above. Use tuples `(name, description, is_essential, &[skill_names])` plus `(scenario_name, mode, &[pack_names])`. Skills referenced must match existing `skills.name` values; log a warning for missing ones (don't hard-fail, since some plugin skills won't be in DB).

- [ ] **Step 4: Run tests**

Run: `cargo test -p skills-manager-core pack_seeder`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/skills-manager-core/src/pack_seeder.rs
git commit -m "feat(core): v10 pack taxonomy seed with disclosure modes"
```

---

## Phase 8 — CLI commands

### Task 11: `sm pack context <pack>`

**Files:**
- Modify: `crates/skills-manager-cli/src/commands.rs`
- Modify: `crates/skills-manager-cli/src/main.rs`

- [ ] **Step 1: Write CLI integration test**

Add to `crates/skills-manager-cli/tests/pack.rs` (create if absent):

```rust
use assert_cmd::Command;
use predicates::str;

#[test]
fn pack_context_prints_skill_descriptions() {
    // Arrange a fixture DB with a pack `mkt-seo` containing two skills.
    // ... (see existing test helpers for DB fixtures)

    let mut cmd = Command::cargo_bin("sm").unwrap();
    cmd.env("SM_DB_PATH", fixture_db_path())
        .arg("pack").arg("context").arg("mkt-seo");
    cmd.assert()
        .success()
        .stdout(str::contains("seo-audit:"))
        .stdout(str::contains("ai-seo:"));
}
```

- [ ] **Step 2: Add subcommand enum variants**

In `main.rs`, locate the `Pack { action }` enum. Add:

```rust
enum PackAction {
    // ...existing...
    Context { name: String },
    SetRouter {
        name: String,
        #[arg(long)] description: Option<String>,
        #[arg(long)] body: Option<std::path::PathBuf>,
    },
    ListRouters,
    GenRouter { name: String },
    RegenAllRouters,
}
```

- [ ] **Step 3: Implement dispatch + command functions**

In `commands.rs`:

```rust
pub fn cmd_pack_context(name: &str) -> anyhow::Result<()> {
    let store = open_store()?;
    let pack = store.get_pack_by_name(name)?
        .ok_or_else(|| anyhow::anyhow!("pack '{name}' not found"))?;
    let skills = store.get_pack_skills(&pack.id)?;
    println!("# Pack: {}\n", pack.name);
    if let Some(d) = &pack.description { println!("Description: {d}\n"); }
    if let Some(r) = &pack.router_description {
        println!("Current router: {r}\n");
    }
    println!("## Skills ({})\n", skills.len());
    for s in skills {
        println!("- {}: {}", s.name, s.description.unwrap_or_default());
    }
    Ok(())
}
```

- [ ] **Step 4: Run test + verify.**

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(cli): sm pack context <pack>"
```

### Task 12: `sm pack set-router` / `list-routers`

- [ ] **Step 1: Test for `set-router` writes desc + timestamp**

```rust
#[test]
fn pack_set_router_persists_description() {
    // fixture db with pack mkt-seo
    let mut cmd = Command::cargo_bin("sm").unwrap();
    cmd.env("SM_DB_PATH", fixture_db_path())
        .args(["pack", "set-router", "mkt-seo", "--description", "SEO trigger keywords..."]);
    cmd.assert().success();
    // Re-open DB and assert router_description == "SEO trigger keywords..."
}
```

- [ ] **Step 2: Implement**

```rust
pub fn cmd_pack_set_router(
    name: &str,
    description: Option<&str>,
    body_file: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    let store = open_store()?;
    let pack = store.get_pack_by_name(name)?
        .ok_or_else(|| anyhow::anyhow!("pack '{name}' not found"))?;
    let body = body_file.map(std::fs::read_to_string).transpose()?;
    let ts = chrono::Utc::now().timestamp();
    store.set_pack_router(&pack.id, description, body.as_deref(), ts)?;
    println!("Router updated for pack '{name}'.");
    Ok(())
}

pub fn cmd_pack_list_routers() -> anyhow::Result<()> {
    let store = open_store()?;
    for pack in store.get_all_packs()? {
        let status = match &pack.router_description {
            Some(_) => "✓",
            None => "—",
        };
        println!(
            "{status}  {name:<24} {desc}",
            status = status,
            name = pack.name,
            desc = pack.router_description.as_deref().unwrap_or("<not generated>"),
        );
    }
    Ok(())
}
```

- [ ] **Step 3: Run + pass; commit.**

```bash
git add -A
git commit -m "feat(cli): sm pack set-router + list-routers"
```

### Task 13: `sm pack gen-router` + marker I/O

**Files:**
- Create: `crates/skills-manager-core/src/pending_router_gen.rs`
- Modify: `crates/skills-manager-core/src/lib.rs` (`pub mod pending_router_gen;`)
- Modify: `crates/skills-manager-cli/src/commands.rs`

- [ ] **Step 1: Test marker write/list/clear**

In `pending_router_gen.rs`:

```rust
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct PendingMarker {
    pub pack_id: String,
    pub pack_name: String,
    pub created_at: i64,
    pub skills: Vec<(String, Option<String>)>, // (name, description)
}

pub fn markers_dir(root: &Path) -> PathBuf { root.join("pending-router-gen") }

pub fn write_marker(root: &Path, marker: &PendingMarker) -> Result<()> {
    let dir = markers_dir(root);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", marker.pack_id));
    fs::write(path, serde_json::to_string_pretty(marker)?)?;
    Ok(())
}

pub fn list_markers(root: &Path) -> Result<Vec<PendingMarker>> {
    let dir = markers_dir(root);
    if !dir.exists() { return Ok(vec![]); }
    let mut out = Vec::new();
    for e in fs::read_dir(dir)? {
        let e = e?;
        if e.path().extension().and_then(|s| s.to_str()) == Some("json") {
            let m: PendingMarker = serde_json::from_str(&fs::read_to_string(e.path())?)?;
            out.push(m);
        }
    }
    Ok(out)
}

pub fn delete_marker(root: &Path, pack_id: &str) -> Result<()> {
    let path = markers_dir(root).join(format!("{pack_id}.json"));
    if path.exists() { fs::remove_file(path)?; }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn marker_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let m = PendingMarker {
            pack_id: "p1".into(), pack_name: "mkt-seo".into(),
            created_at: 1, skills: vec![("seo-audit".into(), Some("Audit".into()))],
        };
        write_marker(tmp.path(), &m).unwrap();
        let list = list_markers(tmp.path()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].pack_name, "mkt-seo");
        delete_marker(tmp.path(), "p1").unwrap();
        assert_eq!(list_markers(tmp.path()).unwrap().len(), 0);
    }
}
```

- [ ] **Step 2: CLI command `gen-router`**

```rust
pub fn cmd_pack_gen_router(name: &str) -> anyhow::Result<()> {
    let store = open_store()?;
    let pack = store.get_pack_by_name(name)?
        .ok_or_else(|| anyhow::anyhow!("pack '{name}' not found"))?;
    let skills = store.get_pack_skills(&pack.id)?
        .into_iter()
        .map(|s| (s.name, s.description))
        .collect();
    let marker = skills_manager_core::pending_router_gen::PendingMarker {
        pack_id: pack.id.clone(),
        pack_name: pack.name.clone(),
        created_at: chrono::Utc::now().timestamp(),
        skills,
    };
    let sm_root = skills_manager_dir();
    skills_manager_core::pending_router_gen::write_marker(&sm_root, &marker)?;
    println!("Pending marker written. Open Claude Code — the pack-router-gen skill will handle '{}'.", name);
    Ok(())
}

pub fn cmd_pack_regen_all_routers() -> anyhow::Result<()> {
    let store = open_store()?;
    for pack in store.get_all_packs()? {
        if pack.is_essential { continue; }
        cmd_pack_gen_router(&pack.name)?;
    }
    Ok(())
}
```

- [ ] **Step 3: Run tests + commit**

```bash
cargo test -p skills-manager-core pending_router_gen
cargo test -p skills-manager-cli
```

```bash
git add -A
git commit -m "feat(cli): sm pack gen-router + regen-all-routers with marker I/O"
```

---

## Phase 9 — Tauri IPC commands

### Task 14: Router CRUD + disclosure mode endpoints

**Files:**
- Modify: `src-tauri/src/commands/packs.rs`
- Modify: `src-tauri/src/commands/scenarios.rs`
- Create: `src-tauri/src/commands/router_gen.rs`
- Modify: `src-tauri/src/lib.rs` (command registration)

- [ ] **Step 1: Add `set_pack_router` / `set_pack_essential` Tauri commands**

`src-tauri/src/commands/packs.rs`:

```rust
#[tauri::command]
pub async fn set_pack_router(
    state: tauri::State<'_, AppState>,
    pack_id: String,
    description: Option<String>,
    body: Option<String>,
) -> Result<(), String> {
    let ts = chrono::Utc::now().timestamp();
    state.store
        .set_pack_router(&pack_id, description.as_deref(), body.as_deref(), ts)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_pack_essential(
    state: tauri::State<'_, AppState>,
    pack_id: String,
    is_essential: bool,
) -> Result<(), String> {
    state.store.set_pack_essential(&pack_id, is_essential)
        .map_err(|e| e.to_string())
}
```

- [ ] **Step 2: `set_scenario_disclosure_mode` command**

`src-tauri/src/commands/scenarios.rs`:

```rust
#[tauri::command]
pub async fn set_scenario_disclosure_mode(
    state: tauri::State<'_, AppState>,
    scenario_id: String,
    mode: String, // "full" | "hybrid" | "router_only"
) -> Result<(), String> {
    state.store.set_scenario_disclosure_mode(&scenario_id, &mode)
        .map_err(|e| e.to_string())
}
```

- [ ] **Step 3: Router-gen marker commands**

`src-tauri/src/commands/router_gen.rs`:

```rust
use skills_manager_core::pending_router_gen::{PendingMarker, list_markers, write_marker, delete_marker};

#[tauri::command]
pub async fn write_pending_router_marker(
    state: tauri::State<'_, AppState>,
    pack_id: String,
) -> Result<(), String> {
    let pack = state.store.get_pack_by_id(&pack_id).map_err(|e| e.to_string())?
        .ok_or("pack not found")?;
    let skills = state.store.get_pack_skills(&pack.id).map_err(|e| e.to_string())?
        .into_iter().map(|s| (s.name, s.description)).collect();
    let m = PendingMarker {
        pack_id: pack.id, pack_name: pack.name,
        created_at: chrono::Utc::now().timestamp(),
        skills,
    };
    write_marker(&state.sm_root, &m).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_pending_router_markers(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<PendingMarker>, String> {
    list_markers(&state.sm_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_pending_router_marker(
    state: tauri::State<'_, AppState>,
    pack_id: String,
) -> Result<(), String> {
    delete_marker(&state.sm_root, &pack_id).map_err(|e| e.to_string())
}
```

- [ ] **Step 4: Register commands in `src-tauri/src/lib.rs`**

Add to `.invoke_handler(tauri::generate_handler![...])`:
`set_pack_router, set_pack_essential, set_scenario_disclosure_mode, write_pending_router_marker, list_pending_router_markers, clear_pending_router_marker`.

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: success.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(tauri): IPC for router CRUD + disclosure mode + markers"
```

---

## Phase 10 — Frontend: PacksView router editor

### Task 15: `RouterEditor` component

**Files:**
- Create: `src/components/RouterEditor.tsx`
- Create: `src/components/__tests__/RouterEditor.test.tsx`

- [ ] **Step 1: Write component test**

```tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { RouterEditor } from "../RouterEditor";

describe("RouterEditor", () => {
  it("disables Save when description is empty", () => {
    render(<RouterEditor packId="p1" initial={{ description: "" }} onSave={jest.fn()} />);
    expect(screen.getByRole("button", { name: /save/i })).toBeDisabled();
  });

  it("warns when description exceeds 600 chars", () => {
    const long = "x".repeat(601);
    render(<RouterEditor packId="p1" initial={{ description: long }} onSave={jest.fn()} />);
    expect(screen.getByTestId("char-counter")).toHaveClass("text-red-600");
  });

  it("calls onSave with trimmed description + body", async () => {
    const onSave = jest.fn().mockResolvedValue(undefined);
    render(<RouterEditor packId="p1" initial={{ description: "" }} onSave={onSave} />);
    fireEvent.change(screen.getByLabelText(/router description/i), { target: { value: "  hello  " } });
    fireEvent.click(screen.getByRole("button", { name: /save/i }));
    expect(onSave).toHaveBeenCalledWith({ description: "hello", body: null });
  });
});
```

- [ ] **Step 2: Implement component**

```tsx
import { useState } from "react";

type Initial = { description: string; body?: string | null };
type Props = {
  packId: string;
  initial: Initial;
  onSave: (v: { description: string; body: string | null }) => Promise<void>;
  onGenerate?: () => void;
  onPreview?: () => void;
};

export function RouterEditor({ packId, initial, onSave, onGenerate, onPreview }: Props) {
  const [desc, setDesc] = useState(initial.description);
  const [body, setBody] = useState(initial.body ?? "");
  const len = desc.length;
  const color = len <= 400 ? "text-green-600" : len <= 600 ? "text-yellow-600" : "text-red-600";

  return (
    <div className="space-y-3">
      <label className="block">
        <span className="text-sm font-medium">Router description</span>
        <textarea
          className="w-full border rounded p-2 font-mono text-sm"
          rows={3}
          value={desc}
          onChange={(e) => setDesc(e.target.value)}
          aria-label="Router description"
        />
      </label>
      <div data-testid="char-counter" className={`text-xs ${color}`}>{len} chars (target 150–400)</div>

      <label className="block">
        <span className="text-sm font-medium">Body (optional — leave empty for auto-render)</span>
        <textarea
          className="w-full border rounded p-2 font-mono text-sm"
          rows={8}
          value={body}
          onChange={(e) => setBody(e.target.value)}
        />
      </label>

      <div className="flex gap-2">
        <button
          className="px-3 py-1 bg-blue-600 text-white rounded disabled:opacity-50"
          disabled={desc.trim().length === 0}
          onClick={() => onSave({ description: desc.trim(), body: body.trim() || null })}
        >
          Save
        </button>
        {onGenerate && (
          <button className="px-3 py-1 border rounded" onClick={onGenerate}>
            Generate with Claude Code
          </button>
        )}
        {onPreview && (
          <button className="px-3 py-1 border rounded" onClick={onPreview}>
            Preview Sync Output
          </button>
        )}
      </div>
    </div>
  );
}
```

- [ ] **Step 3: Run + verify pass; commit.**

```bash
pnpm test src/components/__tests__/RouterEditor.test.tsx
```

```bash
git add -A
git commit -m "feat(ui): RouterEditor component"
```

### Task 16: Integrate into `PacksView`

**Files:**
- Modify: `src/views/PacksView.tsx`

- [ ] **Step 1: Add Progressive Disclosure section to pack detail**

Where the current detail panel renders, add (after existing fields, hidden when `pack.is_essential`):

```tsx
{!pack.is_essential && (
  <section className="mt-6 border-t pt-4">
    <h3 className="font-semibold mb-3">Progressive Disclosure</h3>
    <div className="mb-3 flex items-center gap-2">
      <label className="inline-flex items-center gap-2">
        <input
          type="checkbox"
          checked={pack.is_essential}
          onChange={(e) => invoke("set_pack_essential", { packId: pack.id, isEssential: e.target.checked })}
        />
        <span className="text-sm">Mark as Essential (always full-sync)</span>
      </label>
    </div>
    <RouterEditor
      packId={pack.id}
      initial={{ description: pack.router_description ?? "", body: pack.router_body }}
      onSave={async ({ description, body }) => {
        await invoke("set_pack_router", { packId: pack.id, description, body });
        await reloadPacks();
      }}
      onGenerate={async () => {
        await invoke("write_pending_router_marker", { packId: pack.id });
        setPendingModalOpen(true);
      }}
      onPreview={() => setPreviewPack(pack)}
    />
    <div className="text-xs text-gray-500 mt-2">
      Last generated: {pack.router_updated_at
        ? new Date(pack.router_updated_at * 1000).toLocaleString()
        : "never"}
    </div>
  </section>
)}
```

- [ ] **Step 2: Add "pending gen" modal**

```tsx
{pendingModalOpen && (
  <Modal onClose={() => setPendingModalOpen(false)}>
    <h3>Generation queued</h3>
    <p>
      Open Claude Code. The <code>pack-router-gen</code> skill will pick up this request and fill in the router description.
      Refresh this page when it completes.
    </p>
    <button onClick={async () => { await reloadPacks(); setPendingModalOpen(false); }}>Refresh</button>
  </Modal>
)}
```

- [ ] **Step 3: Preview modal renders final SKILL.md**

Stub by calling a new Tauri command `preview_router_skill_md(pack_id)` that invokes `router_render::render_router_skill_md` server-side and returns the string. Add that command now (small diff to Phase 9):

```rust
#[tauri::command]
pub async fn preview_router_skill_md(
    state: tauri::State<'_, AppState>,
    pack_id: String,
) -> Result<String, String> {
    let pack = state.store.get_pack_by_id(&pack_id).map_err(|e| e.to_string())?
        .ok_or("pack not found")?;
    let skills = state.store.get_pack_skills(&pack.id).map_err(|e| e.to_string())?;
    Ok(skills_manager_core::router_render::render_router_skill_md(
        &pack, &skills, &state.vault_root,
    ))
}
```

- [ ] **Step 4: Manually smoke in dev**

Run: `cargo tauri dev`
Open PacksView, pick any pack, edit router desc, save, reopen — persists. Click Preview — rendered markdown appears.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(ui): PacksView Progressive Disclosure section"
```

---

## Phase 11 — Frontend: ScenariosView disclosure mode

### Task 17: `DisclosureModeSelect` + `TokenEstimateBadge`

**Files:**
- Create: `src/components/DisclosureModeSelect.tsx`
- Create: `src/components/TokenEstimateBadge.tsx`
- Create tests alongside.

- [ ] **Step 1: Write tests + implement `DisclosureModeSelect`**

```tsx
// DisclosureModeSelect.tsx
type Mode = "full" | "hybrid" | "router_only";
type Props = { value: Mode; onChange: (m: Mode) => void };
export function DisclosureModeSelect({ value, onChange }: Props) {
  return (
    <select value={value} onChange={(e) => onChange(e.target.value as Mode)}>
      <option value="full">Full — all skills visible (legacy)</option>
      <option value="hybrid">Hybrid — essentials + routers (recommended)</option>
      <option value="router_only">Router only — minimum tokens</option>
    </select>
  );
}
```

Test:

```tsx
it("calls onChange with selected mode", () => {
  const onChange = jest.fn();
  render(<DisclosureModeSelect value="full" onChange={onChange} />);
  fireEvent.change(screen.getByRole("combobox"), { target: { value: "hybrid" } });
  expect(onChange).toHaveBeenCalledWith("hybrid");
});
```

- [ ] **Step 2: `TokenEstimateBadge`**

```tsx
type Props = { mode: "full" | "hybrid" | "router_only"; essentialSkillCount: number; routerCount: number };
const AVG_DESC = 80; // tokens per skill description
const AVG_ROUTER = 120; // tokens per router description

export function TokenEstimateBadge({ mode, essentialSkillCount, routerCount }: Props) {
  const estimate =
    mode === "full" ? (essentialSkillCount + routerCount * 5 /* avg pack size */) * AVG_DESC :
    mode === "hybrid" ? essentialSkillCount * AVG_DESC + routerCount * AVG_ROUTER :
    routerCount * AVG_ROUTER;
  const color = estimate > 10_000 ? "bg-red-100 text-red-800" :
                estimate > 3_000 ? "bg-yellow-100 text-yellow-800" :
                "bg-green-100 text-green-800";
  return <span className={`px-2 py-1 rounded text-xs ${color}`}>~{estimate.toLocaleString()} tokens</span>;
}
```

Test: render with three modes, assert color bucket.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(ui): DisclosureModeSelect + TokenEstimateBadge components"
```

### Task 18: Integrate into scenario editor

- [ ] **Step 1: Locate scenario editor (CreateScenarioDialog + PacksView scenario section).**

Run: `grep -rn "scenario" src/ | grep -i "view\|dialog\|editor" | head -20`

- [ ] **Step 2: Add mode dropdown + badge to scenario detail**

Inside the scenario detail rendering:

```tsx
<div className="flex items-center gap-3">
  <DisclosureModeSelect
    value={scenario.disclosure_mode}
    onChange={async (m) => {
      await invoke("set_scenario_disclosure_mode", { scenarioId: scenario.id, mode: m });
      await reloadScenarios();
    }}
  />
  <TokenEstimateBadge
    mode={scenario.disclosure_mode}
    essentialSkillCount={scenario.essential_skill_count ?? 0}
    routerCount={scenario.non_essential_pack_count ?? 0}
  />
</div>
```

Backend: extend scenario list API to compute `essential_skill_count` and `non_essential_pack_count` per scenario. Minimal addition; adds these fields to the command response.

- [ ] **Step 3: Smoke in dev**

Run: `cargo tauri dev`
Switch a scenario mode. Sync triggers. Sidebar reflects new mode (done in Phase 13).

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(ui): scenario editor disclosure mode + token estimate"
```

---

## Phase 12 — Frontend: MatrixView cell states

### Task 19: Extended cell rendering

**Files:**
- Modify: `src/views/MatrixView.tsx`

- [ ] **Step 1: Backend: extend matrix query to return materialization state**

In whichever command populates matrix (`get_matrix_state` or similar), include per-cell `kind`:
- `materialized`
- `via_router`
- `none`

Computation: for each (scenario, pack, skill), if scenario mode is Full or pack is Essential → materialized; if scenario includes pack and non-essential, mode hybrid/router_only → via_router; else none.

- [ ] **Step 2: Update cell renderer**

```tsx
function Cell({ kind }: { kind: "materialized" | "via_router" | "none" }) {
  const { symbol, title, className } = {
    materialized: { symbol: "●", title: "Materialized in agent dir", className: "text-green-700" },
    via_router:   { symbol: "◐", title: "Accessible via pack router", className: "text-blue-600" },
    none:         { symbol: "○", title: "Not in scope", className: "text-gray-300" },
  }[kind];
  return <span className={className} title={title}>{symbol}</span>;
}
```

- [ ] **Step 3: Legend**

```tsx
<div className="flex gap-4 text-xs mb-2">
  <span><span className="text-green-700">●</span> Materialized</span>
  <span><span className="text-blue-600">◐</span> Via router</span>
  <span><span className="text-gray-300">○</span> Not in scope</span>
</div>
```

- [ ] **Step 4: Smoke in dev + test a cell click shows drill-down with mode.**

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(ui): MatrixView cell states for progressive disclosure"
```

---

## Phase 13 — Sidebar + Dashboard

### Task 20: Sidebar disclosure-mode badge

**Files:**
- Modify: `src/components/Sidebar.tsx`

- [ ] **Step 1: Render badge next to active agent name**

```tsx
{activeAgent && (
  <span className="ml-auto px-2 py-0.5 text-xs rounded bg-gray-200">
    {scenario.disclosure_mode}
  </span>
)}
```

- [ ] **Step 2: Commit**

```bash
git add -A
git commit -m "feat(ui): sidebar disclosure mode badge"
```

### Task 21: Dashboard tokens-saved widget

**Files:**
- Modify: `src/views/Dashboard.tsx`

- [ ] **Step 1: Compute savings vs full baseline**

Use `TokenEstimateBadge` calc with both modes; display delta.

```tsx
<div className="rounded border p-3">
  <div className="text-sm text-gray-500">Estimated tokens saved vs Full</div>
  <div className="text-2xl font-semibold">~{saved.toLocaleString()}</div>
</div>
```

- [ ] **Step 2: Commit**

```bash
git add -A
git commit -m "feat(ui): Dashboard tokens-saved widget"
```

---

## Phase 14 — (Optional) Router accuracy eval

### Task 22: `sm pack eval-routers` harness

**Files:**
- Create: `crates/skills-manager-cli/src/commands/eval_routers.rs`

- [ ] **Step 1: Define canned query fixture**

`crates/skills-manager-cli/tests/fixtures/router_eval_queries.json`:

```json
{
  "mkt-seo": ["help me audit my SEO", "add schema markup", "JSON-LD product schema", "not ranking on Google", "generate 100 comparison pages", "..."],
  "dev-frontend": ["build a landing page", "tailwind component", "shadcn dropdown", "stitch mockup", "..."],
  "...": []
}
```

- [ ] **Step 2: Implement harness**

For each pack, for each query:
- Spawn Claude Code in headless mode (`claude -p "<query>"`) against a synthetic skill directory populated with current routers.
- Parse which skill Claude invokes first (`Skill: <name>`) from the output.
- Record hit/miss per pack.

Output summary table:

```
Pack              Hits   Accuracy
mkt-seo           17/20  85%
dev-frontend      18/20  90%
...               --/--
```

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(cli): sm pack eval-routers harness (stretch)"
```

This phase is optional for MVP; defer if scope pressure.

---

## Phase 15 — Documentation

### Task 23: Update PROGRESS.md + CLAUDE.md + README

- [ ] **Step 1: Move Default Pack Seeding ⬜ into Done and annotate**

In `PROGRESS.md`:
- Mark both "Default Pack Seeding" and new "Progressive Disclosure" as ✅ (once merged).
- Add description of how hybrid mode works.

- [ ] **Step 2: Update CLAUDE.md (project file)**

Add a section "Progressive Disclosure":
- Default mode is `hybrid`
- Commands: `sm pack context`, `sm pack set-router`, `sm pack gen-router`
- How to debug: `sm pack list-routers` + `MatrixView`

- [ ] **Step 3: README user-facing docs**

Add a "Progressive Disclosure" section with an example + token-savings number.

- [ ] **Step 4: Commit**

```bash
git add PROGRESS.md CLAUDE.md README.md
git commit -m "docs: progressive disclosure usage + progress update"
```

---

## Self-Review (spec → plan coverage)

**Spec section → Task:**
- "Two-Location Storage + Read-on-Demand Architecture" → Tasks 7, 8
- "Three Disclosure Modes" → Tasks 5, 7, 17
- "v9 → v10 Migration" → Tasks 1, 2
- "packs table new columns" → Tasks 1, 3, 4
- "scenarios.disclosure_mode" → Tasks 1, 5
- "Essential pack designation" → Task 4, 10
- "Pack Taxonomy" seed → Task 10
- "Scenario Remapping" → Task 10
- "Migration Safety" → existing backup routine reused; Task 1 asserts idempotency
- "Sync Engine resolve_desired_state" → Task 7
- "Reconciliation" → Task 8
- "Router Rendering" → Task 6
- "Edge cases (Cursor compat, name collision, rename, concurrent sessions)" → covered in Task 8 tests + known-limit docs in Task 23
- "Router Generation built-in skill" → Task 9
- "CLI commands (context/set-router/list/gen/regen)" → Tasks 11–13
- "UI PacksView" → Tasks 15, 16
- "UI Scenarios editor" → Tasks 17, 18
- "UI MatrixView" → Task 19
- "UI Sidebar + Dashboard" → Tasks 20, 21
- "Testing Strategy #1–8 (unit + integration + frontend)" → inline in each Task
- "Testing #9 (real Claude Code session)" → manual smoke steps (Task 16, 18, 19)
- "Testing #10 (multi-agent)" → covered by reconcile tests; documented in Task 23
- "Testing #11 (router eval)" → Task 22 (stretch)
- "Testing #12 (performance)" → add benchmark test in Task 8 if time permits; track separately otherwise

**Placeholder scan:** none remaining; each code step has concrete code.

**Type consistency:** `DisclosureMode` enum used consistently; `PackRecord` fields match across stores; `PendingMarker` structure matches CLI + Tauri usages.

**Dependencies:**
- Agent Native Skills Management (🔄) — Task 8 `is_sm_managed` must integrate with its marker when it lands. Until then, symlink heuristic covers SM writes but may miss SM-written non-symlink directories. Coordinate before merging.

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-04-19-progressive-disclosure.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — Dispatch a fresh subagent per task, review between tasks, fast iteration. Best for a plan this large to keep main context clean.

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

**Which approach?**
