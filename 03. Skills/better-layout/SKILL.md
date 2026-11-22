---
name: better-layout
description: Layout structure for web interfaces, from grouping and alignment to reading order, progressive disclosure, and adaptive breakpoints. Use when structuring a page or component, spacing or aligning controls, deciding what collapses at small sizes, handling RTL layout direction, or reviewing frontend code for layout. Triggers on layout, spacing, alignment, grouping, negative space, whitespace, visual hierarchy, reading order, progressive disclosure, breakpoints, responsive layout, container queries, safe area, full-bleed, edge-to-edge, layout margins, RTL layout, logical properties.
---

# Layout that communicates structure

Layout communicates before a single word is read: position, spacing, and alignment carry hierarchy on their own, and generous space beats decoration. A good layout also survives stress: resize it, translate it, mirror it for RTL, and it should still hold together. Apply these principles when building or reviewing UI code, and express every change in the project's existing styling system (Tailwind, plain CSS, CSS-in-JS); never introduce a second styling approach.

Hit-area sizes and focus behavior are covered by the `better-accessibility` skill; visual polish (radius, shadows, animation) by the `better-ui` skill; line length and text spacing by the `better-typography` skill.

Treat the numeric values below as starting points for interfaces without an established density or spacing system. Preserve deliberate platform chrome, compact professional tools, and project tokens when they remain usable under hit-area, zoom, localization, and viewport stress tests.

## Quick Reference

| Category | When to Use |
| --- | --- |
| [Grouping & Alignment](grouping-and-alignment.md) | Space vs separators, alignment edges, logical properties, importance ordering |
| [Spacing & Adaptivity](spacing-and-adaptivity.md) | Spacing between targets, layout margins, progressive disclosure, full-bleed content, breakpoints, i18n growth |

## Core Principles

### 1. Group with Space, Not Lines

Negative space is the primary grouping tool; background shapes second; separator lines last, only where space alone can't carry the structure. The gap between groups must be at least 2× the gap within a group (`8px` intra-group → `16px`+ inter-group), or the grouping reads as noise.

### 2. Keep Controls Distinct from Content

Interactive elements must look interactive: a background shape, a border, or a consistent placement zone. Never style a control identically to adjacent static text.

### 3. Align to Shared Edges

Pick alignment edges and stick to them; every stray edge reads as noise. Use one project spacing step for each level of subordination (`16px` is a useful default). Use logical properties (`padding-inline-start`, `margin-inline-end`) for direction-dependent layout; reserve physical left/right for genuinely physical geometry.

### 4. Order by Importance

The most important content sits near the top and the leading edge; reading order flows top-to-bottom, leading-to-trailing. Think in leading/trailing, not left/right.

### 5. Hint at Hidden Content

Progressive disclosure needs a visible affordance. Use the project's established cue; without one, let the next item peek `16–32px` past the scroll edge or show a disclosure control. Content hidden with zero cue may as well not exist.

### 6. Breathing Room Between Targets

Without an established density system, start with `12px` between adjacent bordered or filled controls and `24px` of clearance around borderless text- and icon-only controls. Compact layouts may use less when `better-accessibility` hit areas do not overlap and the controls remain visually distinct.

### 7. Inset Buttons from the Edges

In content layouts, keep full-width buttons inside the layout margins (start near `16px` inline on mobile) with a visible radius. Edge-to-edge actions are acceptable when they intentionally follow established platform or application chrome, account for safe areas, and remain distinguishable from system UI.

### 8. Content Bleeds, Controls Float

Backgrounds and media extend to the viewport edges; controls and text stay inside the layout margins and safe areas (`env(safe-area-inset-*)`). Sticky chrome floats above the content layer, it doesn't dam it.

### 9. Hold Structure Until It Breaks

Breakpoints come from the content, not device presets. Keep the expanded layout as long as it genuinely fits and collapse late; prefer container queries for component-level adaptation. Test the smallest and largest sizes first.

### 10. Plan for Growth and Clipping

Plan for substantial and language-dependent string growth rather than relying on a universal percentage: no fixed widths or heights on text containers, and let rows wrap. Never park critical actions where resizing or scrolling clips them; keep them reachable in the normal flow or stable chrome appropriate to the product.

## Common Mistakes

| Mistake | Fix |
| --- | --- |
| Separator line where spacing would do | Remove the line, double the gap between groups |
| `margin-left` / `padding-right` in a localizable layout | `margin-inline-start` / `padding-inline-end` |
| Content-layout button accidentally touches the viewport | Inset within the project margins; preserve intentional platform chrome |
| Carousel/scroller that looks complete | Let the next item peek `16–32px` past the edge |
| Adjacent controls merge or expanded hit areas overlap | Increase the gap using the project scale; use `12px`/`24px` as starting points |
| Breakpoints at 768/1024 because they're the defaults | Break where the content actually stops fitting |
| Fixed-width text container sized to one language | `max-width` + wrapping; test pseudo-localization and representative locales |
| Primary action at the clip-prone bottom of a pane | Sticky positioning or stable chrome with safe-area padding |

## Review Output Format

Use this format only when the user asks for a standalone layout review. When `better-interface` orchestrates the review, provide domain evidence and findings to that skill and let its output format, severity scale, consolidation rules, cap, and verdict take precedence.

Present the standalone review in two parts.

### Findings

Group all confirmed findings by principle. Use a markdown table with **Severity**, **Location**, **Before**, **After**, and **Why** columns. Never use separate "Before:" / "After:" lines.

- **Severity**: `HIGH` blocks content or an action at a supported viewport; `MEDIUM` harms hierarchy, reading order, or adaptability; `LOW` is isolated alignment or spacing polish.
- **Location**: cite `path/to/file:line`. If the artifact has no source files, cite the exact screen and component instead.
- **Before / After**: show the current layout and an actionable replacement.
- **Why**: name the violated principle and its effect on comprehension or adaptability.

Consolidate a repeated systemic issue into one row and list every affected location. Omit principles with no findings.

### Example

#### Group with space, not lines
| Severity | Location | Before | After | Why |
| --- | --- | --- | --- | --- |
| LOW | `src/Settings.tsx:41` | `border-b` on every settings row | Remove borders; use `space-y-2` within groups and `space-y-8` between groups | Spacing communicates grouping with less visual noise |
| LOW | `src/ProfileForm.tsx:58` | `<hr>` between form sections | Replace with `mt-10` on each section heading | Section hierarchy should not depend on repeated rules |

#### Align to shared edges
| Severity | Location | Before | After | Why |
| --- | --- | --- | --- | --- |
| LOW | `src/Card.tsx:24` | Card text at `pl-4`, card icon at `pl-3` | Align both to the same `pl-4` edge | Shared edges create a legible structure |
| MEDIUM | `src/Nav.css:19` | `margin-left: 16px` | `margin-inline-start: 16px` | Physical properties break direction-aware layouts |

### Verification and Verdict

After the findings:

1. **Verification**: list the exact checks run and their observed results across the relevant viewport widths, reading order, zoom, and RTL state. If a check was not run, state what still needs verification.
2. **Verdict**: `Block` if any `HIGH` finding remains, `Needs changes` if only `MEDIUM` or `LOW` findings remain, and `Approve` only when no actionable findings remain.

When there are no findings, omit the tables, state "No actionable layout findings", report verification, and end with `Approve`.
