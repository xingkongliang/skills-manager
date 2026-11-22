---
name: agent-army
description: "Deploy a 2-layer parallel agent hierarchy for large, parallelizable work — big refactors, multi-file migrations, codebase-wide audits, bulk generation. A top-tier commander (Fable or Opus) orchestrates the swarms; the user picks a power level (Max Power / Heavy / Balanced / Economy) that sets the Opus/Sonnet/Haiku model mix per layer. Layer 1 is 3-50+ specialist agents, each with its own full context window; Layer 2 is 2+ sub-agents per member. Includes git safety, tiered sizing, a pre-deploy gate, phantom-completion checks, and multi-wave follow-up."
user_invocable: true
---

# Agent Army

A 2-layer parallel execution framework. The Commander is the top-tier model — Fable by default, or Opus if that's the session model — and does the thinking: recon, composition, briefing, verification. It orchestrates swarms of subordinate models whose tiers the user picks at deploy time (see Power Level). Each Layer 1 agent has its own full context window (not a slice). Each spawns Layer 2 sub-agents under it. The result is many independent brains running at once — not one brain divided.

```
Commander (you — Fable or Opus, the session model)
 |
 |-- Layer 1: Team (3 to 50+, each = own 1M context, L1 tier from power level)
 |    |-- Agent A (1M) -- Sub-agent A1, A2, ...  (L2 tier)
 |    |-- Agent B (1M) -- Sub-agent B1, B2, ...  (L2 tier)
 |    |-- Agent C (1M) -- Sub-agent C1, C2, ...  (L2 tier)
 |    |-- ... (no cap)
```

**Swarm vs. army:** A swarm splits one context window across sub-agents — one brain, divided. An army gives each Layer 1 member its own window. That difference is the whole point of this skill.

## When to use

- Large refactors spanning many files
- Multi-file color / style / naming / API migrations
- Broad codebase audits (security, a11y, performance, dead code)
- Bulk content generation or transformation
- Any task with **6+ independent units of work** that can run simultaneously

## When NOT to use

- Fewer than 6 independent units of work — just do it directly
- Heavy sequential dependencies where each step needs the last one's output
- Single-file changes
- Tasks needing one coherent authorial voice across all output (parallel agents drift)

If the task doesn't clearly fit, say so and propose doing it inline instead of spinning up an army.

<mandatory-rules>
## MANDATORY RULES

1. EVERY Layer 1 agent MUST spawn 2+ sub-agents. No exceptions. If you're about to deploy an L1 agent with no sub-agents, STOP and restructure.
2. NEVER silently shrink the army. Match the user's chosen tier. If you must deviate, say so out loud and why.
2b. NEVER silently downgrade models. Pass `model:` explicitly on every Agent call at the user's chosen power level. Omitting it inherits the session model — that's a silent Max Power bill.
3. Sub-agent deployment instructions go INSIDE the Layer 1 brief. If they're missing, the sub-agents will never be created.
4. Report as agents complete: `[Agent N/M complete] name: X files modified, Y flags`.
5. Show the army plan and pass the Deployment Gate before deploying (Full Mode). Quick Mode composes the plan internally but still passes the Gate.
6. After every wave: run the build AND the phantom-completion check (see Verify). A green report from an agent is a claim, not proof.
7. "Keep going" / "don't stop" = continuous mode: launch a new agent the moment one completes. Don't wait, don't re-ask.
</mandatory-rules>

## Army Size

Confirm a tier before starting. Present this table:

```
| Tier         | L1 Agents | Total w/ Sub-agents | Est. Tokens  |
|--------------|-----------|---------------------|--------------|
| Conservative | 3         | ~9                  | ~200-500K    |
| Standard     | 5-10      | ~15-30              | ~500K-1.5M   |
| Aggressive   | 10-20     | ~30-60              | ~1.5-4M      |
| Maximum      | 20-50+    | ~60-100+            | ~4M+         |
| Custom       | you pick  | you pick            | varies       |
```

Default to **Standard** on "just do it." After recon (Step 3), recommend a specific number based on what you found — e.g. "35 files across 6 domains → Aggressive: 8 L1 agents, 2-3 sub-agents each (~22 total, ~2M tokens). Adjust?" Token estimates are rough and scale with task complexity.

## Power Level

Size sets how many agents; power sets which models. Ask for both in one question (AskUserQuestion works well — size and power as two questions in one call). Skip asking if the user already named a power level or explicit models.

```
| Power Level         | Commander          | Layer 1 | Layer 2 | Best For                                        |
|---------------------|--------------------|---------|---------|-------------------------------------------------|
| Max Power           | Fable/Opus (session)| Opus   | Opus    | gnarly refactors, correctness-critical migrations|
| Heavy (recommended) | Fable/Opus (session)| Opus   | Sonnet  | most large tasks — smart orchestration, fast exec|
| Balanced            | Fable/Opus (session)| Sonnet | Sonnet  | mechanical migrations with clear patterns        |
| Economy             | Fable/Opus (session)| Sonnet | Haiku   | high-volume, dead-simple find/replace            |
```

Rules:
- The **Commander is always the session model** — Fable or Opus, the user's choice — and is never delegated. If the session is running Sonnet or Haiku, tell the user to switch (`/model` or `claude --model fable`) before deploying: commander quality is the ceiling on army quality.
- Pass the model explicitly on **every** Agent call: `model: "<L1 tier>"` for team members; L1 briefs instruct `model: "<L2 tier>"` for sub-agents.
- **Per-agent escalation** is allowed at any power level: a heavy or tangled file (1000+ lines, complex logic) can bump that one sub-agent up a tier. Note escalations in the army plan.
- **Audit waves run on Opus** regardless of power level — fresh, stronger eyes on the executors' work. Notify waves can drop to Haiku.

## Protocol

**Mode:** If scope is already concrete (file paths, exact changes, tier), skip to Step 3 (Quick Mode). Otherwise start at Step 1 (Full Mode).

### Step 1: Intake (Full Mode)

Confirm in one line if the user already gave context: "Goal: [X]. Scope: [Y]. Tier: [Z]. Power: [P]. Starting." Otherwise ask for: goal (one sentence), scope (files/dirs/"everything"), constraints (don't touch X, match Y), tier, and power level.

### Step 2: Git Checkpoint

1. `git status` — warn on uncommitted changes, offer to stash/commit first.
2. `git checkout -b agent-army/checkpoint-{timestamp}` then switch back. This is the rollback point.
3. No git repo? Warn that there's no safety net and get explicit confirmation.

### Step 3: Recon

1. Grep / Glob / Read to find every affected file.
2. Count the units of work.
3. `wc -l` each file. Flag 500+ line files as **heavy** → assign solo.
4. Classify into domains (by directory, import graph, file type, naming pattern).
5. Identify shared dependencies — **foundation files** imported across domains. Handle these first (Step 5).
6. Identify the build command (package.json, Makefile, etc.).

Output: `Files: N | Heavy: [list] | Domains: [list] | Shared deps: [list] | Build cmd: [cmd]`

### Step 4: Compose

1. **Foundation Agent** for shared deps — runs *before* the parallel wave so dependents don't conflict.
2. **Layer 1:** one agent per domain; split large domains across multiple agents.
3. **Layer 2:** 2+ sub-agents per L1 agent, assigned by **weighted file load**, not file count:
   - Small (<200 lines): 2-3 per sub-agent
   - Medium (200-500): 1-2 per sub-agent
   - Heavy (500+): 1 per sub-agent, solo
4. Name every agent. Assign every file to **exactly one** sub-agent — no overlaps, no gaps.

Output the army plan. Full Mode: pause for "Proceed?" Quick Mode: one-line summary, then deploy.

### Deployment Gate

Output this checklist in your response before deploying. Don't check it mentally — write it out.

```
DEPLOYMENT GATE:
[ ] Every L1 brief contains "You MUST spawn N sub-agents"
[ ] Every sub-agent is named with specific files assigned
[ ] Every file is owned by exactly one sub-agent (no overlap, no gap)
[ ] L1 briefs include full sub-agent deployment instructions
[ ] Tier matches the user's selection
[ ] Power level set: model: "<L1 tier>" on every L1 call; L2 tier + escalations in every brief
```

All must PASS. Any FAIL → fix the plan before deploying.

### Step 5: Deploy

1. Foundation Agent first (if any), at the L1 tier. Wait for completion.
2. Launch ALL Layer 1 agents in parallel — `run_in_background: true` and `model: "<L1 tier>"` on every call, in a single message.
3. Each L1 agent spawns its L2 sub-agents in parallel (per its brief).
4. Report each completion: `[Agent N/M complete] name: results`.

### Step 6: Verify

1. **Re-scan** — rerun the Step 3 searches for remaining violations.
2. **Phantom-completion check** — run `git diff --stat` and cross-reference against agent reports. Any agent that reported "COMPLETE, N files modified" with no matching diff lied or no-op'd — re-dispatch it. Trust the diff, not the report.
3. **Build** — run the build command. Record PASS/FAIL.
4. **Resolve cross-team flags** from the reports.
5. Output the army report:

```
Agents: N | Files modified: N (diff-confirmed) | Skipped: N | Build: PASS/FAIL
Flags: [list] | Phantom completions caught: N | Rollback: git checkout agent-army/checkpoint-{timestamp}
```

## Waves

Each wave is a new, smaller, differently-specialized army. Pause for user approval before each. **Max 4 waves.**

| Wave | Name          | Trigger                                                        | Purpose                                              |
|------|---------------|----------------------------------------------------------------|------------------------------------------------------|
| 1    | **Execute**   | always                                                         | make the changes                                     |
| 2    | **Audit**     | build fails, remaining violations, 20+ files, or flags > 0     | fresh agents review Wave 1 for correctness/edge cases|
| 3    | **Propagate** | changes touch APIs/types/interfaces; tests or docs reference old patterns | update tests, docs, configs, downstream callers |
| 4    | **Notify**    | user opts in                                                   | draft PR description, changelog, Slack summary       |

Each wave's report is the next wave's recon. After each, rerun Verify; stop when re-scan is clean and build passes, or at 4 waves.

### Resume / scratchpad

For multi-wave or interruptible runs, write `.army-state.md` after Wave 1: files modified (diff-confirmed), open flags, unresolved issues, decisions. If a run is killed, the next invocation reads this file and resumes from the last clean wave instead of restarting. Skip for single-wave tasks.

## Layer 1 Prompt Template

This is what spawned agents actually see — the most important section in the skill.

```
You are [AGENT_NAME], specialist on [DOMAIN].

Objective: [one sentence]
Approved patterns: [exact values — hex codes, class names, API shapes]
Forbidden patterns: [what to remove/avoid]
Your files: [absolute paths with line counts]
Rules: [constraints]. Skip files already using approved patterns (idempotency).
Flag issues outside your files in "Flags for Commander" — do NOT fix them.

CRITICAL: You MUST use the Agent tool to spawn the sub-agents listed below. Do
NOT do the work yourself. Do NOT skip spawning. Deploy ALL sub-agents in a single
message with multiple Agent tool calls, passing model: "[L2_TIER]" on every call
[note escalations: except "NAME" which gets model: "[higher tier]"].

Sub-agents:
- "[NAME]": [files with line counts]
- "[NAME]": [files with line counts]

Pass each sub-agent: objective, their files, approved/forbidden patterns, rules,
report format. After all complete, aggregate their reports and verify (by re-reading
or grepping) that no forbidden patterns remain in your domain before reporting back.
```

## Layer 2 Prompt Template

```
You are [SUB_AGENT_NAME], working under [TEAM_MEMBER_NAME].

Objective: [one sentence]
Files you own: [absolute paths with line counts — touch only these]
Approved / Forbidden patterns: [exact values to use and remove]
Rules: [constraints]. Skip files already correct. Flag issues outside your files —
do NOT fix them.

Process: read each file fully → check idempotency → apply approved, remove forbidden
→ re-read to confirm no forbidden patterns remain.

Report: Files Modified (file: N replacements), Files Skipped (already correct),
Flags for Commander, Issues, Status (COMPLETE/PARTIAL/FAILED).
```

## Report Format

Every agent returns:

```
## Report: [Name]
Files Modified:   [file: N replacements]
Files Skipped:    [file: reason]
Flags for Commander: [issue or "None"]
Issues:           [issue or "None"]
Status:           COMPLETE / PARTIAL / FAILED
```

## Anti-patterns

These are the failure modes this skill exists to prevent. If you catch yourself doing one, stop and correct.

- **Phantom fan-out** — an L1 agent does the work itself instead of spawning sub-agents. The whole speed advantage is gone. Enforced by Mandatory Rule 1 + the Gate.
- **Phantom completion** — an agent reports COMPLETE without editing anything. Caught by the `git diff --stat` cross-check, not by trusting the report.
- **Silent shrink** — quietly running 3 agents when the user picked Aggressive. Always surface deviations.
- **Model drift** — omitting `model:` on Agent calls, so every sub-agent silently inherits the commander's model. The user picked Economy and got billed Max Power. Enforced by Mandatory Rule 2b + the Gate.
- **Overlapping ownership** — two sub-agents editing the same file → conflicts and lost edits. Enforced by exactly-one-owner in the Gate.
- **Skipping the foundation** — touching dependents before shared files are updated → cascade of broken imports. Foundation Agent runs first.
- **Army for a small job** — 9 agents for 4 files is slower (orchestration overhead) and burns tokens. See "When NOT to use."

## Error Handling

- Sub-agent fails → L1 agent retries its files or spawns a replacement.
- L1 agent fails → Commander spawns a replacement with the same brief.
- Build failure → diff against the checkpoint branch to isolate the break, then run a fix wave.
- Phantom completion → re-dispatch that agent's files to a fresh agent.
- Idempotency on re-run → agents check for already-applied patterns first, so running the army twice never double-applies.

## Worked example

User: "Replace all neon Tailwind colors with our sand palette across the site."

**Recon:** 45 files, 5 domains, 2 shared dependencies (a theme config + a shared `<Button>`). Build cmd: `npm run build`. Recommend Aggressive: 1 Foundation Agent + 5 L1 agents, 2-4 sub-agents each (~22 total). User picks **Heavy** power: Fable commander, Opus L1, Sonnet L2.

**Wave 1 (Execute):** Foundation Agent updates the 2 shared files first. Then 5 L1 agents deploy in parallel, each fanning out to sub-agents. Reports: 43 modified, 2 skipped (already correct), 3 flags.

**Verify:** `git diff --stat` shows 42 files changed — one agent reported a file it never touched (phantom). Re-dispatched; now 43 confirmed. Build passes. 3 flags + 2 edge-case violations remain → Audit wave recommended.

**Wave 2 (Audit):** 3 fresh agents target the flagged files and violations. Fixed. Build clean, re-scan clean. 4 test files import the changed `<Button>` → Propagate recommended.

**Wave 3 (Propagate):** 3 agents update the 4 test files + 1 doc. Build still green.

**Final:** 3 waves, ~28 agents total, 48 files touched (diff-confirmed), 1 phantom caught, zero violations remaining, clean build. Rollback branch available.
