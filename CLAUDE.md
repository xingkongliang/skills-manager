# Skills Manager Fork

## Project Overview

Fork of [xingkongliang/skills-manager](https://github.com/xingkongliang/skills-manager) — a Tauri (Rust + React) desktop app that manages AI agent skills across multiple coding tools.

**Goal**: Add Skill Packs, Plugin Management, Per-Agent Matrix View, and CLI mode.

## Quick Start

```bash
pnpm install
cargo tauri dev
```

## Architecture

- **Backend**: Rust (Tauri), SQLite via rusqlite
- **Frontend**: React + TypeScript + Tailwind CSS
- **Build**: Vite + Tauri CLI
- **DB**: `~/.skills-manager/skills-manager.db`

### Key Directories

- `src-tauri/src/core/` — business logic (sync, DB, adapters)
- `src-tauri/src/commands/` — Tauri IPC command handlers
- `src/views/` — React pages (Dashboard, MySkills, Settings, etc.)
- `src/components/` — shared React components

### Key Files

- `src-tauri/src/core/sync_engine.rs` — symlink/copy sync logic
- `src-tauri/src/core/skill_store.rs` — all SQLite operations
- `src-tauri/src/commands/scenarios.rs` — scenario switch + sync
- `src-tauri/src/core/tool_adapters.rs` — agent directory config

## Development Plan

See `DEVELOPMENT_PLAN.md` for the full plan. Summary:

1. **Phase 1**: Refactor core into standalone crate + add Skill Packs
2. **Phase 2**: CLI binary (replace shell script `~/.local/bin/sm`)
3. **Phase 3**: Plugin management (enable/disable Claude Code plugins per scenario)
4. **Phase 4**: Packs UI (frontend)
5. **Phase 5**: Agent × Skill matrix view + Plugin UI

## Conventions

- Rust: follow existing code style (snake_case, anyhow errors)
- Frontend: follow existing patterns (React hooks, Tailwind)
- DB migrations: add to `src-tauri/src/core/migrations.rs`
- New Tauri commands: add to `src-tauri/src/commands/`, register in `lib.rs`

## Testing

```bash
cargo test                    # Rust tests
pnpm run lint                 # Frontend lint
cargo tauri dev               # Manual testing
```

## Current State

- DB has 132 shared skills, 7 scenarios (minimal → everything)
- `sm` shell script at `~/.local/bin/sm` handles basic scenario switching
- Hermes native skills (26) managed independently outside SM
- Plugin skills always loaded (not yet manageable by SM)

## Development Workflow — Skills Toolkit

### Execution Order

```
Phase 1 (core + packs) — blocker, do first
    ├── Phase 2 (CLI)        ─┐
    ├── Phase 3 (plugins)     ├── parallel via worktrees
    └── Phase 4 (packs UI)   ─┘
                └── Phase 5 (matrix + plugin UI)
```

### Per-Feature Loop

```
brainstorming → writing-plans → [plan-eng-review if architectural]
→ executing-plans (TDD: test → implement → verify)
→ simplify → review → git-commit-push-pr
```

### Core Skills (every phase)

| Purpose | Skill |
|---------|-------|
| Plan | `superpowers:brainstorming` → `superpowers:writing-plans` |
| Execute | `superpowers:executing-plans` |
| Parallel branches | `superpowers:using-git-worktrees` |
| Parallel tasks | `superpowers:subagent-driven-development` |
| TDD | `superpowers:test-driven-development` |
| Debug | `superpowers:systematic-debugging` |
| Verify | `superpowers:verification-before-completion` |
| Self-review | `superpowers:requesting-code-review` |
| Handle review | `superpowers:receiving-code-review` |
| Ship | `compound-engineering:git-commit-push-pr` |

### Phase-Specific Skills

| Phase | Skill | Purpose |
|-------|-------|---------|
| Phase 1 (Core) | `superpowers:writing-plans` | crate split architecture |
| Phase 2 (CLI) | Context7 MCP | `clap` framework docs |
| Phase 4-5 (Frontend) | `compound-engineering:frontend-design` | React UI (PacksView, MatrixView) |
| Phase 4-5 (Frontend) | `webapp-testing` | Playwright UI testing |

### Quality Gates (on-demand)

| Purpose | Skill | When |
|---------|-------|------|
| Code review | gstack `/review` | before PR |
| Eng review | gstack `/plan-eng-review` | before Phase 1 architecture lock |
| Simplify | `/simplify` | after implementation |
| QA | gstack `/qa` | after frontend features |

## Progressive Disclosure (2026-04-19+)

Default scenario disclosure mode is `hybrid` — only Essential-pack skills and auto-generated pack router SKILL.md files land in `~/.claude/skills/`. Non-essential skills stay in the vault (`~/.skills-manager/skills/`) and are read on demand via pack routers.

### Commands
- `sm pack context <pack>` — dump pack metadata for router generation
- `sm pack set-router <pack> --description <text> [--body <file>]` — persist router content
- `sm pack list-routers` — show status of each pack's router
- `sm pack gen-router <pack>` — queue a marker for the `pack-router-gen` skill to process
- `sm pack regen-all-routers` — batch queue markers for all non-essential packs

### Debugging
- Check which skills are materialized vs routed in MatrixView
- Sidebar shows the active scenario's disclosure_mode
- Dashboard shows estimated tokens saved vs Full mode
