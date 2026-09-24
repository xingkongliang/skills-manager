# Project Skill and Preset CLI Design

## Goal

Let agents add and remove managed skills and preset contents in registered projects through `skills-manager-cli`, using the same copy/delete behavior as the desktop Projects UI.

## Current behavior

- `projects add/list/remove` registers project directories in the app's shared database.
- The UI adds a library skill with `export_skill_to_project`, after choosing one or more installed, enabled project agents.
- The UI preset bar adds each central skill in a preset to the selected project agents; it skips copies already present and reports partial failures.
- Turning a preset off removes the project copies of its member skills from the selected project agents. The database does not store a project-to-preset relationship.
- The Tauri export and delete handlers currently contain the file-operation rules. The CLI cannot call those handlers as normal shared functions.

## CLI surface

Add four subcommands:

```text
skills-manager-cli projects add-skill <project-ref> <skill-ref> --agent <agent> [--agent <agent>...]
skills-manager-cli projects remove-skill <project-ref> <skill-relative-path> --agent <agent> [--agent <agent>...] (--dry-run | --yes)
skills-manager-cli projects add-preset <project-ref> <preset-ref> --agent <agent> [--agent <agent>...]
skills-manager-cli projects remove-preset <project-ref> <preset-ref> --agent <agent> [--agent <agent>...] (--dry-run | --yes)
```

- `project-ref` resolves by ID, exact stored path, or exact name. Ambiguous names fail with candidate IDs and paths.
- `skill-ref` uses the existing CLI central-skill resolver (ID, name, directory basename, or central path).
- `skill-relative-path` is the path under one selected agent's skills root, matching the `relative_path` returned by project skill scanning. It does not include the project root or the agent root prefix. The same relative path is applied to each explicitly selected agent.
- `preset-ref` resolves by ID or exact name; `current` means the active preset, matching existing preset CLI behavior.
- Every command requires one or more explicit `--agent` values. Add operations require each agent to be a valid, enabled, installed target for the project. Remove operations accept any valid project target that can be resolved for that workspace, even if the agent is currently disabled or uninstalled, so stale project copies can still be cleaned up. Linked workspaces accept only their registered agent key.
- Project commands continue to use the app's shared database even if `--skills-root` is provided.

## Shared architecture

- Extract the project file operations currently inside Tauri `export_skill_to_project` and `delete_project_skill` into synchronous core project-service functions that accept `&SkillStore` and resolved IDs/paths.
- Keep existing Tauri commands as adapters: they validate their arguments, call the shared service, and preserve their current `ProjectDto`/error behavior.
- Add CLI orchestration in `skills-manager-cli.rs`: resolve project, skill or preset, validate selected agents, invoke core operations, and format human/JSON reports.
- Preset orchestration remains a CLI/application-use-case concern: enumerate preset skills in stored order and call the shared single-skill operation serially. Do not add a persistent preset-project membership table or migration.

## Add semantics

### Add one skill

- Read the managed skill from the central library and use the same directory naming and configured sync mode as the UI.
- Require selected agents to be enabled and installed.
- Never overwrite an existing enabled or disabled project copy. Existing copies are reported as already present; a request targeting multiple agents may add to the missing targets and report existing ones as skipped.
- Use no-clobber writes and validate all selected agents before touching the filesystem.

### Add a preset

- Resolve the preset, then process its skills in stored order for each explicit agent.
- Skip a skill-agent pair already represented in that project, as the UI preset bar does.
- Continue after an individual failure. Preserve earlier successful writes; do not attempt rollback.
- Return a report with project, preset, added pairs, skipped pairs, and failed pairs (including agent and message). If any item fails, return a nonzero command status while preserving the full report in the command output.

Adding a preset copies its current skill members into project skill directories. It does not link the project to the preset, and later edits to preset membership do not automatically change the project.

## Remove semantics and safety

### Remove one project skill

- The caller supplies the exact `skill-relative-path` reported by project scanning and at least one `--agent`.
- `--dry-run` lists each existing target directory that would be removed and performs no writes.
- Actual removal requires `--yes`; it removes only the matching project skill directory. It never deletes the central-library skill.
- Removal remains available for a valid project agent target that is disabled or no longer installed, matching the UI's ability to clean up an existing copy.
- Validate the relative path contains only normal path components and confirm the target remains under the selected agent's enabled or disabled skills root.

### Remove a preset from a project

- Resolve the preset and find project variants corresponding to each current preset member for each selected agent.
- `--dry-run` lists the exact project-relative skill paths to remove and performs no writes. Actual removal requires `--yes`.
- Removal remains available for a valid project agent target that is disabled or no longer installed.
- Remove the matching project copies even if the same central skill is also a member of another preset, matching the current UI's deactivate behavior. There is no persistent membership relationship that could distinguish which preset originally caused a copy to be added.
- Missing copies are skipped; individual failures do not roll back successful removals. Return an added/removed/skipped/failed style batch report with nonzero status when failures occur.

## Output and errors

- Human output names the project, skill/preset, and agent. Bulk operations list added, skipped, and failed pairs.
- `--json` returns structured per-target results so an agent can report partial completion accurately.
- Unknown project/skill/preset/agent, disabled or uninstalled agent, duplicate target, unsafe path, and file-operation errors must be distinguishable by message.
- Remove dry-run output must show exact paths before the user confirms with `--yes`.

## `manage-skills` documentation

Update `skills/manage-skills/SKILL.md` with the four commands and examples. Document explicit `--agent`, safe add/no-overwrite behavior, the remove dry-run plus `--yes` requirement, partial preset reports, and the fact that adding a preset copies current skills rather than creating persistent project membership.

## Verification

- Core tests for shared add/remove operations, no-clobber, enabled/disabled targets, linked workspaces, and path safety.
- CLI tests for argument parsing, project/skill/preset resolution, explicit agent validation, add/skip/remove behavior, dry-run non-mutation, `--yes`, partial batch results, JSON output, and nonzero partial-failure status.
- Run the Rust test suite and CLI help smoke checks.
- Review the Tauri handlers to confirm they still use the shared service and retain UI behavior.

## Out of scope

- No project-preset membership or provenance schema.
- No automatic deployment to agents outside the specified project.
- No preset editing or skill central-library removal through project commands.
