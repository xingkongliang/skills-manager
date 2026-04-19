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
