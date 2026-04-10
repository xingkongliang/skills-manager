# Skill Format Detection Spec

Date: 2026-04-10

## Purpose

This document records the external skill-format conventions we verified and the recommended detection policy for this repository.

It is intended to guide future changes to:

- central skill repository scanning
- project workspace skill scanning
- local import validation
- git/repo skill discovery

## External references

### skills.sh

- Docs: https://skills.sh/docs
- CLI docs: https://skills.sh/docs/cli

Observed facts:

- The public docs describe skills as reusable capabilities installed via the `skills` CLI.
- The docs do not define `README.md` or `CLAUDE.md` as valid skill entry files.
- The public docs emphasize install and usage flow, but the stricter file-level format is mostly reflected by ecosystem examples and upstream repositories.

### vercel-labs/skills

- Repository: https://github.com/vercel-labs/skills

Observed facts from the README:

- "Skills are directories containing a `SKILL.md` file with YAML frontmatter."
- Required fields are documented inside `SKILL.md` frontmatter.
- Troubleshooting and discovery guidance refer to `SKILL.md`.
- Skill discovery is based on standard skill locations plus explicit plugin manifests, not on `README.md` or `CLAUDE.md`.

Implication:

- `SKILL.md` is the canonical entry file for a skill.
- `skill.md` can be treated as a legacy-compatible variant when product compatibility matters.

### vercel-labs/agent-skills

- Repository: https://github.com/vercel-labs/agent-skills

Observed facts from the README:

- "Each skill contains:"
- `SKILL.md` - Instructions for the agent
- `scripts/` - optional
- `references/` - optional

Implication:

- The official Vercel example collection follows the same `SKILL.md`-first convention.

### Ecosystem examples on skills.sh

Observed from publicly listed skills:

- Many published skills expose `SKILL.md` as the visible entry file.
- Normative creator/validator skills in the ecosystem commonly state that `SKILL.md` is the single source of truth.
- Some ecosystem guidance explicitly says not to keep `README.md` in skill folders and to merge that content into `SKILL.md`.

Implication:

- Treating `README.md` as a skill marker is not aligned with the emerging ecosystem norm.

## Detection policy recommendation

### Canonical rule

A directory should be treated as a skill directory only when it contains:

- `SKILL.md`

### Practical compatibility rule

For backward compatibility, products may also accept:

- `skill.md`

This should be treated as legacy-compatible behavior, not as the preferred or canonical format.

### Files that should not act as skill markers

These files should not make a directory count as a skill:

- `README.md`
- `readme.md`
- `CLAUDE.md`

Reasons:

- They are often repository docs, category docs, or agent memory/instruction files.
- In nested skill layouts, they can cause false positives on namespace/group directories.
- They do not match the Vercel/skills.sh convention for skill entry points.

## Recommended repository behavior

### Central repository scanning

Recommended rule:

- recognize directories containing:
  - `SKILL.md`
  - `skill.md`

Reason:

- This keeps compatibility with existing local skill sets while still rejecting ambiguous docs-only folders.
- The real false-positive sources are `README.md` and `CLAUDE.md`, not `skill.md`.

### Project workspace scanning

Recommended rule:

- recognize directories containing:
  - `SKILL.md`
  - `skill.md`

Reason:

- Recursive scanning is especially sensitive to false positives.
- Namespace folders such as `skills/research/` should not be detected as skills just because they contain a `README.md` or `CLAUDE.md`.
- Accepting `skill.md` does not materially increase false-positive risk because it is already a strong skill marker.

### Local import validation and git/repository discovery

Recommended rule:

- accept:
  - `SKILL.md`
  - `skill.md`

Reason:

- This keeps implementation and mental model simpler.
- It supports legacy repositories without reintroducing the harmful `README.md` / `CLAUDE.md` ambiguity.

## Current guidance for this repository

If the product goal is "align with skills.sh / Vercel conventions" while keeping engineering complexity reasonable, the preferred model is:

1. `SKILL.md` is the canonical skill marker.
2. `skill.md` is accepted as a legacy-compatible marker across normal detection paths.
3. `README.md` and `CLAUDE.md` are never treated as skill markers.

## Practical consequence

This policy avoids two classes of bugs:

- false positives
  - category folders or docs-only directories being detected as skills
- false negatives after recursion short-circuiting
  - nested real skills being skipped because a parent folder was incorrectly classified as a skill

It also avoids an unnecessary source of product complexity:

- strict/compat split logic for `skill.md`

## Change checklist

When adjusting skill detection in this repository, review these areas together:

- `src-tauri/src/core/skill_metadata.rs`
- `src-tauri/src/core/project_scanner.rs`
- `src-tauri/src/commands/skills.rs`
- `src-tauri/src/commands/projects.rs`
- `src-tauri/src/commands/git_backup.rs`
- related tests for import, scanning, and project workspace detection
