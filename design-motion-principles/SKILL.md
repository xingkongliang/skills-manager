---
name: design-motion-principles
description: "Motion and interaction design expert based on Emil Kowalski, Jakub Krehel, and Jhey Tompkins' techniques. Two modes. Build interactive components with purposeful motion, or audit existing animations to catch AI-slop motion patterns (audit emits a branded HTML report with looping demos). Use when creating, adding, animating, or reviewing UI motion: transitions, hover states, micro-interactions, enter/exit animations, or any motion design work in React, Framer Motion, CSS, or HTML. Provides per-designer perspectives with context-aware weighting."
---

# Design Motion Principles

You are a senior design engineer specializing in motion and interaction design. This skill operates in two modes:

- **Create**: Build interactive components with purposeful motion → `workflows/create.md`
- **Audit**: Review existing motion design and report findings → `workflows/audit.md`

**Scope**: Web and app UI motion. HTML/CSS, React, Framer Motion / Motion, iOS/Android transitions, design system animations. The frequency framework still applies to other motion work (game engines, Lottie, Rive, video), but designer-specific techniques may not translate.

---

## STEP 0: Detect Mode (DO THIS FIRST)

| Signal in the request | Mode |
|-----------------------|------|
| "build", "create", "add animation", "animate this", "implement", "make it feel…" | **Create** |
| "audit", "review", "evaluate", "check", "feedback on", "is this motion good" | **Audit** |
| Ambiguous (e.g. "look at this modal animation") | Ask the user |

For ambiguous requests, if `AskUserQuestion` is available, present:
- **Create**: Build or improve the component's motion
- **Audit**: Review existing motion and report findings

Otherwise ask in plain text: "Should I build/improve the motion (Create mode), or review existing motion and report findings (Audit mode)?"

**Once the mode is known, read the matching workflow file and follow it exactly.**

---

## The Three Designers

- **Emil Kowalski** (Linear, ex-Vercel). Restraint, speed, purposeful motion. Best for productivity tools.
- **Jakub Krehel** (jakub.kr). Subtle production polish, professional refinement. Best for shipped consumer apps.
- **Jhey Tompkins** (@jh3yy). Playful experimentation, CSS innovation. Best for creative sites, kids apps, portfolios.

> These three lenses distill each designer's *publicly published* work, courses, articles, talks, and open-source projects. The weighting framework and the "lens" framing are this skill's interpretation of their principles, named in tribute; they are not authored or endorsed by the designers themselves.

Each designer answers a different question:
- **Emil**: *"Should this animate at all?"*
- **Jakub**: *"Is this subtle and polished enough for production?"*
- **Jhey**: *"What could this become?"*

**Critical insight**: These perspectives are context-dependent, not universal rules. A kids' app should prioritize Jakub + Jhey (polish + delight), not Emil's productivity-focused speed rules. Both modes weight the designers by project context before doing anything.

---

## Context-to-Perspective Mapping

| Project Type | Primary | Secondary | Selective |
|--------------|---------|-----------|-----------|
| Productivity tool (Linear, Raycast) | Emil | Jakub | Jhey (onboarding only) |
| Kids app / Educational | Jakub | Jhey | Emil (high-freq game interactions) |
| Creative portfolio | Jakub | Jhey | Emil (high-freq interactions) |
| Marketing/landing page | Jakub | Jhey | Emil (forms, nav) |
| SaaS dashboard | Emil | Jakub | Jhey (empty states) |
| Mobile app | Jakub | Emil | Jhey (delighters) |
| E-commerce | Jakub | Emil | Jhey (product showcase) |

---

## Core Principles (Both Modes)

### The Frequency Gate

Before adding or approving any animation, ask how often the user triggers it:

| Frequency | Recommendation |
|-----------|----------------|
| Rare (monthly) | Delightful, expressive motion welcome |
| Occasional (daily) | Subtle, fast motion |
| Frequent (100s/day) | No animation or instant transition |
| Keyboard-initiated | Never animate |

### Duration Guidelines (Context-Dependent)

| Context | Guideline |
|---------|-----------|
| Productivity UI (Emil) | Under 300ms, 180ms ideal |
| Production polish (Jakub) | 200-500ms for smoothness |
| Creative/kids/playful (Jhey) | Whatever serves the effect |

**Do not universally flag or cap durations.** Check the context weighting first.

### The Golden Rule

> "The best animation is that which goes unnoticed."

If users comment "nice animation!" on every interaction, it's probably too prominent for production. (Exception: kids apps and playful contexts where delight IS the goal.)

### Accessibility is NOT Optional

Every animation, generated in Create mode or reviewed in Audit mode, must handle `prefers-reduced-motion`. No exceptions. See `references/accessibility.md`.

---

## Reference Index

| File | Contents | Load When |
|------|----------|-----------|
| [Motion Cookbook](references/motion-cookbook.md) | All motion recipes, enter/exit, easing, springs, clip-path, @property, FLIP, scroll-driven | Create mode (always); Audit mode for implementation recommendations |
| [Creation Gotchas](references/creation-gotchas.md) | Claude's failure modes when writing motion | Create mode (always) |
| [Audit Checklist](references/audit-checklist.md) | Systematic audit checklist | Audit mode (always) |
| [Anti-Checklist](references/anti-checklist.md) | Quality gate. AI-slop motion categories + anti-patterns to flag | Audit mode (always) |
| [Emil Kowalski](references/emil-kowalski.md) | Restraint philosophy, frequency rule, decision frameworks | Either mode, if Emil is weighted |
| [Jakub Krehel](references/jakub-krehel.md) | Production polish philosophy and decision frameworks | Either mode, if Jakub is weighted |
| [Jhey Tompkins](references/jhey-tompkins.md) | Playful experimentation philosophy and frameworks | Either mode, if Jhey is weighted |
| [Accessibility](references/accessibility.md) | prefers-reduced-motion, vestibular safety | Both modes (mandatory) |
| [Performance](references/performance.md) | GPU optimization, will-change, layout thrash | Either mode, for complex animations |
| [Output Format](references/output-format.md) | Audit report template. HTML mode (default) + terminal mode (flag) | Audit mode only |
| [Demo Shell](references/demo-shell.html) | Visual container template for per-finding demo cards in the HTML report | Audit mode, HTML output |

## Workflow Index

| Workflow | Purpose |
|----------|---------|
| [Create](workflows/create.md) | Build interactive components with purposeful motion |
| [Audit](workflows/audit.md) | Review existing motion design, produce a per-designer report |


---

# Create Workflow


# Workflow: Create Mode

Build interactive components with purposeful motion. Light discovery, then generate against the cookbook.

## Required Reading

Read before generating:
1. `references/motion-cookbook.md`: the recipe source for all motion code
2. `references/accessibility.md`: `prefers-reduced-motion` is mandatory in everything you generate
3. `references/creation-gotchas.md`: Claude's failure modes when writing motion; self-check against these

---

## STEP 1: Light Discovery

Establish two things, project context and designer weighting, before generating. Keep it to 1-2 questions.

### Infer First, Ask Second

Check what you can already see:
- **The request**: what component, what interaction, what stack (React / Framer Motion / CSS / HTML)?
- **CLAUDE.md, package.json, existing components**: project type and existing animation conventions (durations, easing, libraries)

### Propose Context + Weighting

Map the project type to a perspective weighting using the Context-to-Perspective Mapping table in SKILL.md. State your inference in one short block:

```
Building: [what, e.g. "a notification toast, React + Framer Motion"]
Project context: [inferred, e.g. "productivity SaaS dashboard"]
Proposed weighting: Primary [Designer] · Secondary [Designer]
```

If `AskUserQuestion` is available and the weighting is genuinely ambiguous, offer:
- **Confirm**: proceed with the proposed weighting
- **Adjust**: change primary/secondary designer

Otherwise ask in plain text: "Does this weighting sound right, or should I adjust?"

### Wait Gate

For non-trivial components, **confirm context before generating**. For a small, well-specified request ("add a press-scale to this button"), state the inference in one line and skip straight to STEP 3, don't manufacture a question.

---

## STEP 2: Load Weighted Knowledge

Based on the confirmed weighting, read the relevant designer file(s):
- **Read `references/emil-kowalski.md`** if Emil is primary/secondary, restraint, the frequency rule, when NOT to animate
- **Read `references/jakub-krehel.md`** if Jakub is primary/secondary, production polish judgment, subtlety bar
- **Read `references/jhey-tompkins.md`** if Jhey is primary/secondary, playful expression, what motion could become

The designer files give you the **judgment** (should this animate, what feel). The cookbook gives you the **code**.

If the component involves complex or numerous animations, also read `references/performance.md`.

---

## STEP 3: Generate

Build the component. Apply, in order:

1. **The frequency gate (Emil)**: Should this animate at all? High-frequency or keyboard-initiated interactions get minimal or no motion. Decide before adding anything.
2. **Recipes from the cookbook**: Use the weighted designer's patterns. Enter = opacity + translateY + blur. Exit subtler than enter. Custom easing or springs, never bare `ease`.
3. **Accessibility**: Every animation ships with `prefers-reduced-motion` handling, in the same code. No exceptions, no follow-up.
4. **Performance**: Animate `transform` / `opacity` / `filter` only. Never `width` / `height` / `top` / `left`.
5. **Context-appropriate timing**: Emil-weighted → under 300ms. Jakub → 200-500ms polish. Jhey → whatever serves the effect.

---

## STEP 4: Self-Check

Before presenting, verify the generated code against every item in `references/creation-gotchas.md`. Fix anything that matches a gotcha.

Then briefly tell the user the motion decisions you made and why. Which designer weighting drove the timing, easing, and whether something was deliberately left un-animated.

---

## Success Criteria

- [ ] Context and weighting confirmed (or inference stated for trivial requests)
- [ ] Frequency gate applied, motion is purposeful, not decorative-by-default
- [ ] Recipes drawn from the cookbook, matched to the designer weighting
- [ ] `prefers-reduced-motion` handled in all generated motion
- [ ] Only `transform` / `opacity` / `filter` animated
- [ ] Code self-checked against creation-gotchas.md
- [ ] Motion decisions explained to the user


---

# Audit Workflow


# Workflow: Audit Mode

Review existing motion design and produce a per-designer report. Reconnaissance first, then a full audit, then a structured report. Never apply rules blindly.

## Required Reading

Read as you reach each step (not all upfront):
1. `references/audit-checklist.md`: your systematic guide (STEP 2)
2. The weighted designer file(s), `emil-kowalski.md`, `jakub-krehel.md`, `jhey-tompkins.md` (STEP 2)
3. `references/accessibility.md`: mandatory every audit (STEP 2)
4. `references/anti-checklist.md`: the quality gate: AI-slop motion categories + anti-patterns to flag (STEP 2)
5. `references/output-format.md`: the report template, HTML mode + terminal mode (STEP 3)
6. `references/demo-shell.html`: the demo-card template for HTML-mode per-finding demos (STEP 3)

---

## STEP 1: Context Reconnaissance (DO THIS FIRST)

Before auditing any code, understand the project context.

### Gather Context

Check these sources:
1. **CLAUDE.md**: Any explicit context about the project's purpose or design intent
2. **package.json**: What type of app? (Next.js marketing site vs Electron productivity app vs mobile PWA)
3. **Existing animations**: Grep for `motion`, `animate`, `transition`, `@keyframes`. What durations are used? What patterns exist?
4. **Component structure**: Is this a creative portfolio, SaaS dashboard, marketing site, kids app, mobile app?

### Motion Gap Analysis (CRITICAL - Don't Skip)

After finding existing animations, actively search for **missing** animations. These are UI changes that happen without any transition:

**Search for conditional renders without AnimatePresence:**
```bash
# Find conditional renders: {condition && <Component />}
grep -n "&&\s*(" --include="*.tsx" --include="*.jsx" -r .

# Find ternary UI swaps: {condition ? <A /> : <B />}
grep -n "?\s*<" --include="*.tsx" --include="*.jsx" -r .
```

**For each conditional render found, check:**
- Is it wrapped in `<AnimatePresence>`?
- Does the component inside have enter/exit animations?
- If NO to both → this is a **motion gap** that needs fixing

**Common motion gap patterns:**
- `{isOpen && <Modal />}`: Modal appears/disappears instantly
- `{mode === "a" && <ControlsA />}`: Controls swap without transition
- `{isLoading ? <Spinner /> : <Content />}`: Loading state snaps
- `style={{ height: isExpanded ? 200 : 0 }}`: Height changes without CSS transition
- Inline styles with dynamic values but no `transition` property

**Where to look for motion gaps:**
- Inspector/settings panels with mode switches
- Conditional form fields
- Tab content areas
- Expandable/collapsible sections
- Toast/notification systems
- Loading states
- Error states

### State Your Inference

After gathering context, tell the user what you found and propose a weighting:

```
## Reconnaissance Complete

**Project type**: [What you inferred, e.g., "Kids educational app, mobile-first PWA"]
**Existing animation style**: [What you observed, e.g., "Spring animations (500-600ms), framer-motion, active:scale patterns"]
**Likely intent**: [Your inference, e.g., "Delight and engagement for young children"]

**Motion gaps found**: [Number] conditional renders without AnimatePresence
- [List the files/areas with gaps, e.g., "Settings panel mode switches", "Loading states"]

**Proposed perspective weighting**:
- **Primary**: [Designer], [Why]
- **Secondary**: [Designer], [Why]
- **Selective**: [Designer], [When applicable]

Does this approach sound right? Should I adjust the weighting before proceeding with the full audit?
```

Use the Context-to-Perspective Mapping table in SKILL.md to propose the weighting.

### Wait for User Confirmation

**STOP and wait for the user to confirm or adjust.** Do not proceed to the full audit until they respond.

If `AskUserQuestion` is available, present the decision as tappable options:
- **Confirm weighting**: Proceed with the proposed primary/secondary/selective designers
- **Adjust primary**: Swap which designer is primary (e.g., prioritize delight over restraint)
- **Adjust secondary**: Change the secondary lens while keeping primary
- **Rebuild weighting**: The project type inference was wrong; start over

Otherwise ask in plain text: "Does this weighting sound right, or should I adjust?"

If they adjust (e.g., "prioritize delight and engagement"), update your weighting accordingly.

---

## STEP 2: Full Audit (After User Confirms)

Once the user confirms, perform the complete audit by reading the reference files in this order:

### 2a. Read the Audit Checklist First
**Read `references/audit-checklist.md`**: Use this as your systematic guide. It provides the structured checklist of what to evaluate.

### 2b. Read Designer Files for Your Weighted Perspectives
Based on your context weighting, read the relevant designer files:
- **Read `references/emil-kowalski.md`** if Emil is primary/secondary. Restraint philosophy, frequency rules, decision frameworks
- **Read `references/jakub-krehel.md`** if Jakub is primary/secondary. Production polish philosophy, what to check
- **Read `references/jhey-tompkins.md`** if Jhey is primary/secondary. Playful experimentation philosophy, opportunities to surface

### 2c. Read Topical References as Needed
- **Read `references/accessibility.md`**: MANDATORY. Always check for prefers-reduced-motion. No exceptions.
- **Read `references/anti-checklist.md`**: Apply this as the audit's quality gate. AI-slop categories at the top (pulsing indicators, hover-scale-on-everything, stagger-spam, etc.) trigger findings; perspective-specific and general anti-patterns sit below. Each category includes a frequency heuristic so single intentional uses don't trip the gate.
- **Read `references/performance.md`**: If you see complex animations, check for GPU optimization issues
- **Read `references/motion-cookbook.md`**: Reference when making specific implementation recommendations (the recommended fix code, including the per-finding demo motion in HTML mode)

---

## STEP 3: Output Format (HTML by default)

The audit produces a **self-contained HTML report** with auto-looping CSS demos beside Critical and Important findings. **Read `references/output-format.md`** for the full template (both HTML mode and terminal mode).

### Default behavior. Write and open the HTML report

1. **Resolve the write location.** The file is written to `motion-audits/{project-name}-{ISO-date}.html` in the audited project's root.
   - **Audited project root**: run `git rev-parse --show-toplevel` from the agent's cwd. If it succeeds, use that path. If it fails (no `.git` ancestor), use cwd.
   - **`{project-name}`**: the `name` field from `package.json` at the project root if it exists; else the `name` field from `pyproject.toml`; else the basename of the project root. Strip any scoping prefix (`@scope/pkg` → `pkg`) and sanitize to lowercase kebab-case (`[a-z0-9-]`, replace others with `-`).
   - **`{ISO-date}`**: today's date as `YYYY-MM-DD`.
   - Example: `<project-root>/motion-audits/my-app-2026-05-20.html`.
   - Do not modify `.gitignore`. The user sees `motion-audits/` in `git status` and decides whether to ignore it.

2. **Read `references/demo-shell.html`** and use it as the template for each demo card. Embed one card per Critical + Important finding (Opportunities do not get demo cards). Use the suffixed-naming contract, `@keyframes motion-{n}-...` and `.demo-card-{n}__motion-target`, `{n}` = the finding's 1-indexed position across the whole report. So multiple findings don't collide on CSS names.

3. **Generate per-finding motion code** by reading the audited code, the relevant lens reference, and `references/motion-cookbook.md` for the recipe. Use the shell's 0% / 66% / 100% cadence at `animation-duration: 3s` (~2s motion, ~1s hold, loop). The `@keyframes` 100% state must match the motion-target's default static rendering so the shell's `prefers-reduced-motion` guard shows the correct final visual.

4. **Write the file.** Create `motion-audits/` if it doesn't exist. Write the complete self-contained HTML document.

5. **Open in the default browser** via OS-detected Bash dispatch:

   ```bash
   path="<absolute path to the HTML file>"
   if [ -n "$WSL_DISTRO_NAME" ] || grep -qi microsoft /proc/version 2>/dev/null; then
     win_path=$(wslpath -w "$path")
     cmd.exe /c start "" "$win_path" 2>/dev/null
   else
     case "$(uname -s)" in
       Darwin)               open "$path" ;;
       Linux)                xdg-open "$path" ;;
       MINGW*|MSYS*|CYGWIN*) start "" "$path" ;;
       *)                    echo "Unknown platform. Open this file manually: $path" ;;
     esac
   fi
   ```

   If the open command returns non-zero or the platform is unrecognized, print `Open this file in your browser: {absolute path}` and continue. Never abort the audit because of a failed browser-open.

6. **Print the 3-line terminal summary:**

   ```
   🎬 Motion audit complete, 🔴 {N} Critical · 🟡 {N} Important · 🟢 {N} Opportunities
   📄 Report: {absolute path}
   💡 Want the full report inline instead? Re-run with --terminal or say "show inline".
   ```

### Terminal mode (flag-triggered)

When the user signals terminal mode (`--terminal` / `--inline` / `--no-html` flag, or "show the full report inline" / "skip the HTML" / "terminal only"), **skip the HTML write and the browser-open** and render the decorated-markdown report inline per `references/output-format.md` terminal mode. Do not print the 3-line summary in this case.

Do not summarize the audit content in either mode, users want full per-lens perspectives.

---

## Agent Gotchas (Self-Check Before Writing the Report)

Common failure modes during HTML report generation. Most break silently or only manifest when a second finding lands in the same report.

- **Don't reuse keyframe or class names across findings.** Each demo uses `@keyframes motion-{n}-...` and `.demo-card-{n}__motion-target` where `{n}` is the 1-indexed position across the WHOLE report. Duplicate names mean the second finding shadows the first and the first demo breaks silently.
- **Don't redefine the shell's CSS variables.** Per-finding code uses `var(--bg)`, `var(--fg)`, `var(--border)`, `var(--accent)`, `var(--loop-dim)`, `var(--sans)`, `var(--mono)`. Hard-coding colors or fonts breaks dark mode and typography consistency.
- **Don't write per-finding overrides inside the `prefers-reduced-motion` block.** The shell's guard collapses all `[class*="__motion-target"]` animations. Make the `@keyframes` 100% state match the motion-target's default static rendering instead.
- **Don't include demo cards for Opportunities.** Demos are reserved for Critical and Important. Surface Opportunities in text only.
- **Don't animate the report itself.** No entrance, scroll, or mount animations on the report chrome, only the demo cards animate. Animating the report reproduces the AI-slop patterns the audit exists to catch.
- **Don't write to cwd if `git rev-parse --show-toplevel` succeeds.** The report goes to `{project-root}/motion-audits/`. Only fall back to cwd when git returns nonzero.
- **Don't abort the audit if browser-open fails.** A non-zero exit code is a "no default handler" condition, not an error. Print the path and continue.
- **Don't modify `.gitignore`.** The skill never touches it. The user adds `motion-audits/` themselves if they want.
- **Don't summarize per-lens findings.** Each section needs its own findings + working-well items + the `Through {Designer}'s lens:` summary.

---

## Success Criteria

- [ ] Context gathered (CLAUDE.md, package.json, existing animations, structure)
- [ ] Motion gap analysis run, conditional renders checked for missing animation
- [ ] Weighting proposed and confirmed by the user
- [ ] Audit checklist worked through systematically
- [ ] Anti-checklist applied. AI-slop categories checked against the codebase
- [ ] Accessibility checked, prefers-reduced-motion verified (mandatory)
- [ ] HTML report written to `motion-audits/`, opened in browser, 3-line summary printed (or terminal-mode report rendered inline when flagged)
- [ ] Report follows output-format.md with full per-lens sections; Critical + Important findings have looping demo cards


---

# Motion Cookbook


# Motion Cookbook

The single source of truth for motion **recipes**: implementation patterns and code. In Create mode this is your primary reference; in Audit mode load it when making implementation recommendations. Designer philosophy and decision frameworks live in the per-designer reference files; the code lives here.

---

## 1. Enter & Exit Animations

### Enter Animation Recipe (Jakub)
A standard enter animation combines three properties:
- **Opacity**: 0 → 1
- **TranslateY**: ~8px → 0 (or calc(-100% - 4px) for full container slides)
- **Blur**: 4px → 0px

```jsx
initial={{ opacity: 0, translateY: "calc(-100% - 4px)", filter: "blur(4px)" }}
animate={{ opacity: 1, translateY: 0, filter: "blur(0px)" }}
transition={{ type: "spring", duration: 0.45, bounce: 0 }}
```

**Why blur?** It creates a "materializing" effect that feels more physical than opacity alone. The element appears to come into focus, not just fade in.

### Exit Animation Subtlety (Jakub)
**Key Insight**: Exit animations should be subtler than enter animations.

When a component exits, it doesn't need the same amount of movement or attention as when entering. The user's focus is moving to what comes next, not what's leaving.

```jsx
// Instead of full exit movement:
exit={{ translateY: "calc(-100% - 4px)" }}

// Use a subtle fixed value:
exit={{ translateY: "-12px", opacity: 0, filter: "blur(4px)" }}
```

**Why this works**: Exits become softer, less jarring, and don't compete for attention with whatever is entering or remaining.

**When NOT to use subtle exits**:
- When the exit itself is meaningful (user-initiated dismissal)
- When you need to emphasize something leaving (error clearing, item deletion)
- Full-page transitions where directional continuity matters

### Fill Mode for Persistence (Jhey)
Use `animation-fill-mode` to prevent jarring visual resets:
- `forwards`: Element retains animation styling after completion
- `backwards`: Element retains style from first keyframe before animation starts
- `both`: Retains styling in both directions

**Critical for**: Fade-in sequences with delays. Without `backwards`, elements flash at full opacity before their delayed animation starts, then pop to invisible, then fade in.

---

## 2. Easing & Timing

### Duration Impacts Naturalness
> "Duration is all about timing, and timing has a big impact on the movement's naturalness.". Jhey Tompkins

### Custom Easing is Essential (Emil)
> "Easing is the most important part of any animation. It can make a bad animation feel great."

Built-in CSS easing (`ease`, `ease-in-out`) lacks strength. Always use custom Bézier curves for professional results. Resources: easing.dev, easings.co

### Easing Selection Guidelines (Jhey)
Each easing curve communicates something to the viewer. **Context matters more than rules.**

| Easing | Feel | Good For |
|--------|------|----------|
| `ease-out` | Fast start, gentle stop | Elements entering view (arriving) |
| `ease-in` | Gentle start, fast exit | Elements leaving view (departing) |
| `ease-in-out` | Gentle both ends | Elements changing state while visible |
| `linear` | Constant speed | Continuous loops, progress indicators |
| `spring` | Natural deceleration | Interactive elements, professional UI |

**The Context Rule**:
> "You wouldn't use 'Elastic' for a bank's website, but it might work perfectly for an energetic site for children."

Brand personality should drive easing choices. A playful brand can use bouncy, elastic easing. A professional brand should use subtle springs or ease-out.

**When NOT to use bouncy/elastic easing**:
- Professional/enterprise applications
- Frequently repeated interactions (gets tiresome)
- Error states or serious UI
- When users need to complete tasks quickly

### Spring Animations (Jakub)
Prefer spring animations over linear/ease for more natural-feeling motion:
```jsx
transition={{ type: "spring", duration: 0.45, bounce: 0 }}
transition={{ type: "spring", duration: 0.55, bounce: 0.1 }}
```

**Why `bounce: 0`?** It gives smooth deceleration without overshoot, professional and refined. Reserve bounce > 0 for playful contexts.

### The linear() Function (Jhey)
CSS `linear()` enables bounce, elastic, and spring effects in pure CSS:
```css
:root {
  --bounce-easing: linear(
    0, 0.004, 0.016, 0.035, 0.063, 0.098, 0.141 13.6%, 0.25, 0.391, 0.563, 0.765,
    1, 0.891 40.9%, 0.848, 0.813, 0.785, 0.766, 0.754, 0.75, 0.754, 0.766, 0.785,
    0.813, 0.848, 0.891 68.2%, 1 72.7%, 0.973, 0.953, 0.941, 0.938, 0.941, 0.953,
    0.973, 1, 0.988, 0.984, 0.988, 1
  );
}
```

Use Jake Archibald's linear() generator for custom curves: https://linear-easing-generator.netlify.app/

### Stagger Techniques (Jhey)
`animation-delay` only applies once (not per iteration). Approaches:

1. **Different delays with finite iterations**: Works for one-time sequences
2. **Pad keyframes** to create stagger within the animation:
```css
@keyframes spin {
  0%, 50% { transform: rotate(0deg); }
  100% { transform: rotate(360deg); }
}
```

3. **Negative delays** for "already in progress" effects:
```css
.element { animation-delay: calc(var(--index) * -0.2s); }
```
This makes animations appear mid-flight from the start. Useful for staggered continuous animations.

---

## 3. Visual Effects

### Shadows Instead of Borders (Jakub)
In light mode, prefer subtle multi-layer box-shadows over solid borders:
```css
.card {
  box-shadow:
    0px 0px 0px 1px rgba(0, 0, 0, 0.06),
    0px 1px 2px -1px rgba(0, 0, 0, 0.06),
    0px 2px 4px 0px rgba(0, 0, 0, 0.04);
}

/* Slightly darker on hover */
.card:hover {
  box-shadow:
    0px 0px 0px 1px rgba(0, 0, 0, 0.08),
    0px 1px 2px -1px rgba(0, 0, 0, 0.08),
    0px 2px 4px 0px rgba(0, 0, 0, 0.06);
}
```

**Why shadows over borders?**
- Shadows adapt to any background (images, gradients, varied colors) because they use transparency
- Borders are solid colors that may clash with dynamic backgrounds
- Multi-layer shadows create depth; single borders feel flat
- Shadows can be transitioned smoothly with `transition: box-shadow`

**When borders are fine**:
- Dark mode (shadows less visible anyway)
- When you need hard edges intentionally
- Simple interfaces where depth isn't needed

### Gradients & Color Spaces (Jakub)
- Use `oklch` for gradients to avoid muddy midpoints:
```css
element { background: linear-gradient(in oklch, blue, red); }
```

- **Color hints** control where the blend midpoint appears (different from color stops)
- Layer gradients with `background-blend-mode` for unique effects

**Why oklch?** It interpolates through perceptually uniform color space, avoiding the gray/muddy zone that sRGB hits when blending complementary colors.

### Blur as a Signal (Jakub)
Blur (via `filter: blur()`) combined with opacity and translate creates a "materializing" effect. Use blur to signal:
- **Entering focus**: blur → sharp
- **Losing relevance**: sharp → blur
- **State transitions**: blur during, sharp after

---

## 4. Optical Alignment

### Geometric vs. Optical (Jakub)
> "Sometimes it's necessary to break out of geometric alignment to make things feel visually balanced."

**Buttons with icons**: Reduce padding on the icon side so content appears centered:
```
[  Icon Text  ] ← Geometric (mathematically centered, feels off)
[ Icon Text   ] ← Optical (visually centered, feels right)
```

**Play button icons**: The triangle points right, creating visual weight on the left. Shift it slightly right to appear centered.

**Icons in general**: Many icon packs account for optical balance, but asymmetric shapes (arrows, play, chevrons) may need manual margin/padding adjustment.

**The rule**: If it looks wrong despite being mathematically correct, trust your eyes and adjust.

---

## 5. Icon & State Animations (Jakub)

### Contextual Icon Transitions
When icons change contextually (copy → check, loading → done), animate:
- Opacity
- Scale
- Blur

```jsx
<AnimatePresence mode="wait">
  {isCopied ? (
    <motion.div
      initial={{ opacity: 0, scale: 0.8, filter: "blur(4px)" }}
      animate={{ opacity: 1, scale: 1, filter: "blur(0px)" }}
      exit={{ opacity: 0, scale: 0.8, filter: "blur(4px)" }}
    >
      <CheckIcon />
    </motion.div>
  ) : (
    <motion.div ...>
      <CopyIcon />
    </motion.div>
  )}
</AnimatePresence>
```

**Why animate icon swaps?** Instant swaps feel jarring and can be missed. Animated transitions:
- Draw attention to the state change
- Feel responsive and polished
- Give the user confidence their action registered

---

## 6. Shared Layout Animations (Jakub)

### FLIP Technique via layoutId
Motion's `layoutId` prop enables smooth transitions between completely different components:
```jsx
// In one location:
<motion.div layoutId="card" className="small-card" />

// In another location:
<motion.div layoutId="card" className="large-card" />
```

Motion automatically animates between them using the FLIP technique (First, Last, Inverse, Play).

### Best Practices
- Keep elements with `layoutId` **outside** of `AnimatePresence` to avoid conflicts
- If inside `AnimatePresence`, the initial/exit animations will trigger during layout animation (looks bad with opacity)
- Multiple elements can animate if each has a unique `layoutId`
- Works for different heights, widths, positions, and even component types (card → modal)

---

## 7. CSS Custom Properties & @property (Jhey)

### Type Specification Unlocks Animation
The `@property` rule lets you declare types for CSS variables, enabling smooth interpolation:

```css
@property --hue {
  initial-value: 0;
  inherits: false;
  syntax: '<number>';
}

@keyframes rainbow {
  to { --hue: 360; }
}
```

**Available types**: length, number, percentage, color, angle, time, integer, transform-list

**Why this matters**: Without `@property`, CSS sees custom properties as strings. Strings can't interpolate. They just swap. With a declared type, the browser knows how to smoothly transition between values.

### Decompose Complex Transforms
Instead of animating a monolithic transform (which can't interpolate curved paths), split into typed properties:

```css
@property --x { syntax: '<percentage>'; initial-value: 0%; inherits: false; }
@property --y { syntax: '<percentage>'; initial-value: 0%; inherits: false; }

.ball {
  transform: translateX(var(--x)) translateY(var(--y));
  animation: throw 1s;
}

@keyframes throw {
  0% { --x: -500%; }
  50% { --y: -250%; }
  100% { --x: 500%; }
}
```

This creates curved motion paths that would be impossible with standard transform animation. The ball arcs through space rather than moving in straight lines.

### Scoped Variables for Dynamic Behavior (Jhey)
CSS custom properties respect scope, enabling powerful patterns:
```css
.item { --delay: 0; animation-delay: calc(var(--delay) * 100ms); }
.item:nth-child(1) { --delay: 0; }
.item:nth-child(2) { --delay: 1; }
.item:nth-child(3) { --delay: 2; }
```

Use scoped variables to create varied behavior from a single animation definition.

---

## 8. 3D CSS (Jhey)

### Think in Cuboids
> "Think in cubes instead of boxes". Jhey Tompkins

Complex 3D scenes are assemblies of cube-shaped elements (like LEGO). Decompose any 3D object into cuboids.

### Essential Setup
```css
.scene {
  transform-style: preserve-3d;
  perspective: 1000px;
}
```

### Responsive 3D
Use CSS variables for dimensions and `vmin` units:
```css
.cube {
  --size: 10vmin;
  width: var(--size);
  height: var(--size);
}
```

---

## 9. Clip-Path Animations (Emil)

### Why clip-path?
- Hardware-accelerated rendering
- No layout shifts
- No additional DOM elements needed
- Smoother than width/height animations

### Basic Syntax
```css
clip-path: inset(top right bottom left);
clip-path: circle(radius at x y);
clip-path: polygon(coordinates);
```

### Image Reveal Effect
```css
.reveal {
  clip-path: inset(0 0 100% 0); /* Hidden */
  animation: reveal 1s forwards cubic-bezier(0.77, 0, 0.175, 1);
}

@keyframes reveal {
  to { clip-path: inset(0 0 0 0); } /* Fully visible */
}
```

### Tab Transitions
Duplicate tab lists with different styling. Animate the overlay's clip-path to reveal only the active tab. Creates smooth color transitions without timing issues.

### Scroll-Driven with clip-path
```javascript
const clipPathY = useTransform(scrollYProgress, [0, 1], ["100%", "0%"]);
const motionClipPath = useMotionTemplate`inset(0 0 ${clipPathY} 0)`;
```

### Text Mask Effect
Stack elements with complementary clip-paths:
```css
.top { clip-path: inset(0 0 50% 0); }    /* Shows top half */
.bottom { clip-path: inset(50% 0 0 0); } /* Shows bottom half */
```
Adjust values on mouse interaction for seamless transitions.

---

## 10. Button & Interactive Feedback (Emil)

### Scale on Press
Add immediate tactile feedback:
```css
button:active {
  transform: scale(0.97);
}
```

### Don't Animate from scale(0)
```jsx
// BAD: Unnatural motion
initial={{ scale: 0 }}

// GOOD: Natural, gentle motion
initial={{ scale: 0.9, opacity: 0 }}
animate={{ scale: 1, opacity: 1 }}
```

### Tooltip Delay Pattern
First tooltip in a group: delay + animation. Subsequent tooltips: instant.
```css
[data-instant] {
  transition-duration: 0ms;
}
```

### Blur as a Bridge
When state transitions aren't smooth enough, add blur to mask imperfections:
```css
.transitioning {
  filter: blur(2px);
}
```

---

## 11. CSS Transitions vs Keyframes (Emil)

### Interruptibility Problem
CSS keyframes can't be interrupted mid-animation. When users rapidly trigger actions, elements "jump" to new positions rather than smoothly retargeting.

**Solution**: Use CSS transitions with state-driven classes:
```jsx
useEffect(() => {
  setMounted(true);
}, []);
```

```css
.element {
  transform: translateY(100%);
  transition: transform 400ms ease;
}
.element.mounted {
  transform: translateY(0);
}
```

### Direct Style Updates for Performance
CSS variables cause style recalculation across all children. For frequent updates (drag operations), update styles directly:

```javascript
// BAD: CSS variable (expensive cascade)
element.style.setProperty('--drag-y', `${y}px`);

// GOOD: Direct style (no cascade)
element.style.transform = `translateY(${y}px)`;
```

### Momentum-Based Dismissal
Use velocity (distance / time) instead of distance thresholds:
```javascript
const velocity = dragDistance / elapsedTime;
if (velocity > 0.11) dismiss();
```

Fast, short gestures should work. Users shouldn't need to drag far.

### Damping for Natural Boundaries
When dragging past boundaries, reduce movement progressively. Things in real life slow down before stopping.

---

## 12. Spring Physics (Emil)

### Key Parameters
| Parameter | Effect |
|-----------|--------|
| **Stiffness** | How quickly spring reaches target (higher = faster) |
| **Damping** | How quickly oscillations settle (higher = less bounce) |
| **Mass** | Weight of object (higher = more momentum) |

### Spring for Mouse Position
```javascript
const springConfig = { stiffness: 300, damping: 30 };
const x = useSpring(mouseX, springConfig);
const y = useSpring(mouseY, springConfig);
```

Use `useSpring` for any value that should interpolate smoothly rather than snap, nothing in the real world changes instantly.

### Interruptibility
Great animations can be interrupted mid-play:
- Framer Motion supports interruption natively
- CSS transitions allow smooth interruption before completion
- Test by clicking rapidly. Animations should blend, not queue

---

## 13. Origin-Aware Animations (Emil)

Animations should originate from their logical source:

```css
/* Dropdown from button should expand from button, not center */
.dropdown {
  transform-origin: top center;
}
```

**Component library support:**
- Base UI: `--transform-origin` CSS variable
- Radix UI: `--radix-dropdown-menu-content-transform-origin`

---

## 14. Scroll-Driven Animations (Jhey)

### The Core Problem
Scroll-driven animations are tied to scroll **speed**. If users scroll slowly, animations play slowly. This feels wrong for most UI. You want animations to trigger at a scroll position, not be controlled by scroll speed.

### Duration Control Pattern
Use two coordinated animations:
1. **Trigger animation**: Scroll-driven, toggles a custom property when element enters view
2. **Main animation**: Traditional duration-based, activated via Style Query

This severs the connection between scroll speed and animation timing. The animation runs over a fixed duration once triggered, regardless of how fast the user scrolled.

### Progressive Enhancement
Always provide fallbacks:
```javascript
// IntersectionObserver fallback for browsers without scroll-driven animation support
if (!CSS.supports('animation-timeline', 'scroll()')) {
  // Use IntersectionObserver instead
}
```


---

# Audit Checklist


# Audit Checklist

Use this checklist when reviewing motion design in any UI code.

---

## Philosophy Check (Do First)

- [ ] **How often will users trigger this?** (Frequent = less/no animation. Emil's rule)
- [ ] **Is this keyboard-initiated?** (If yes, don't animate. Emil's rule)
- [ ] **Does this animation serve a purpose?** (orientation, feedback, continuity, not just decoration)
- [ ] **Will users notice this animation consciously?** (If yes for production UI, probably too much)
- [ ] **Have I tested this with `prefers-reduced-motion: reduce`?**
- [ ] **Does this feel natural after the 10th interaction?** (Test repeatedly, not just once)
- [ ] **Is the easing appropriate for my brand/context?**
- [ ] **Is the duration appropriate for context?** (Emil prefers under 300ms; Jakub/Jhey may use longer for polish or effect)

---

## Motion Gap Analysis (Check BEFORE Reviewing Existing Animations)

Conditional UI changes that **lack** animation are often worse than poorly-tuned animations:

- [ ] **Searched for conditional renders**: `{condition && <Component />}` patterns
- [ ] **Searched for ternary swaps**: `{condition ? <A /> : <B />}` patterns
- [ ] **Searched for dynamic inline styles**: `style={{ prop: dynamicValue }}` without transition
- [ ] **Each conditional render** either has AnimatePresence wrapper OR doesn't need animation (static content)
- [ ] **Mode switches** (tabs, toggles) animate their content changes, not just the switch itself
- [ ] **Settings panels** with conditional controls have enter/exit animations
- [ ] **Expandable sections** animate height, not just show/hide
- [ ] **Loading → Content** transitions are smooth, not instant swaps

---

## Enter/Exit States

- [ ] Enter animations combine opacity + translateY + blur
- [ ] Exit animations are subtler than enters (smaller translateY, same blur/opacity)
- [ ] `animation-fill-mode: backwards` used for delayed sequences
- [ ] Elements don't flash before their delayed animation starts

---

## Easing & Timing

- [ ] Appropriate easing for context (not default `ease` everywhere)
- [ ] Custom Bézier curves used instead of built-in easing (Emil's rule)
- [ ] Spring animations for interactive elements
- [ ] Durations appropriate for context (Emil: under 300ms; others: whatever serves the design)
- [ ] Consistent timing values across related animations
- [ ] Transform-origin matches interaction source (dropdowns from trigger)

---

## Visual Polish

- [ ] Shadows instead of borders where background varies
- [ ] Gradients using oklch color space for smooth blending
- [ ] Blur used intentionally as a state signal

---

## Optical Alignment

- [ ] Buttons with icons have adjusted padding
- [ ] Asymmetric icons (play, arrows) are visually centered
- [ ] Text and icons feel balanced

---

## State Transitions

- [ ] Icon swaps are animated (opacity, scale, blur)
- [ ] Loading states have smooth transitions
- [ ] Hover states have transitions (150-200ms minimum)
- [ ] Button press has scale feedback (`scale(0.97)` on `:active`)
- [ ] Elements don't animate from `scale(0)` (use `0.9+` instead)

---

## Interaction Patterns (Emil's Rules)

- [ ] Tooltips: first delayed + animated, subsequent instant
- [ ] Animations are interruptible (can change mid-animation)
- [ ] Clip-path used for reveals instead of width/height
- [ ] High-frequency actions have minimal or no animation
- [ ] Keyboard shortcuts don't animate

---

## Performance

- [ ] `will-change` used sparingly and specifically
- [ ] Animations use transform/opacity (not layout properties)
- [ ] Tested on low-end devices
- [ ] No continuous animations without purpose
- [ ] CSS transitions (not keyframes) for interruptible animations (Emil)
- [ ] Direct style updates for drag operations (not CSS variables) (Emil)
- [ ] Velocity-based thresholds (not distance) for swipe dismiss (Emil)

---

## Accessibility

- [ ] Respects `prefers-reduced-motion`
- [ ] No vestibular triggers (excessive zoom, spin, parallax)
- [ ] Looping animations can be paused
- [ ] Functional animations have non-motion alternatives

---

## Quick Reference: Severity Levels

**Critical (Must Fix)**:
- Missing `prefers-reduced-motion` support
- Animating layout properties (width, height, top, left)
- No exit animations (elements just disappear)
- **Motion gaps in primary UI**: Conditional controls/panels that snap in/out without animation
- Animating keyboard-initiated actions (Emil)
- Animations on high-frequency actions (100s/day)

**Important (Should Fix)**:
- Exit animations as prominent as enter animations
- Missing blur in enter animations
- Animating from `scale(0)` instead of `0.9+` (Emil)
- Default CSS easing instead of custom curves (Emil)
- Wrong transform-origin on dropdowns/popovers (Emil)

**Context-Dependent (Check Against Designer Perspective)**:
- Durations over 300ms (Emil flags this; Jakub/Jhey may approve for polish)

**Nice to Have**:
- Optical alignment refinements
- oklch color space for gradients
- Spring animations instead of ease
- Button scale feedback on press
- Tooltip delay pattern (first delayed, subsequent instant)


---

# Anti-Checklist (AI-slop motion)


# Anti-Checklist

This file is the audit's quality gate. The categories below describe motion patterns to **flag** in audited code. AI-slop tells at the top (where most 2026 motion problems live), followed by perspective-specific anti-patterns from Emil, Jakub, and Jhey, then general motion mistakes and code-shaped red flags. When audited code matches a pattern here, the audit surfaces a finding and the agent generates a per-finding motion suggestion by reading the relevant philosophy reference (`emil-kowalski.md`, `jakub-krehel.md`, `jhey-tompkins.md`).

The file frames patterns as "things to flag," not "mistakes to avoid", language that makes the audit's adversarial posture explicit.

---

## AI-Slop Motion Patterns

These are the recognizable motion fingerprints of AI-generated UIs in 2026. They're not always wrong in isolation, what makes them slop is *frequency* and *uniformity*. Finding one instance is normal polish; finding the same pattern slapped across the codebase is the tell. Each category includes a flagging heuristic below the definition so the audit isn't tripped by single intentional uses.

---

### Pulsing indicators

Glowing dots, breathing CTAs, throbbing rings, "live"/"online"/"recording"/"AI active" pulse animations, dark-mode pulse glows, any looped scale/opacity pulse used to draw attention to a status element.

**Flag when you see:**
- `@keyframes` rules with names containing `pulse`, `glow`, `breathe`, `throb`
- `animation: ... infinite` on small UI elements (dots, badges, status indicators)
- `box-shadow` or `opacity` loops on status icons
- Tailwind `animate-pulse` on indicator dots or active-state elements

**Heuristic:** Flag *any* instance. Pulsing indicators are almost always slop. The only exception is a single brand element with explicit design rationale stated in code comments or design docs.

**Fix lens:** Emil, purposeful restraint. See `references/emil-kowalski.md`.

---

### Blur-everywhere entrances

`filter: blur(Npx)` applied to every entering element on mount, sections, cards, images, paragraphs. Jakub's enter recipe (`opacity + translateY + blur`) is excellent in moderation; AI-slop versions apply it uniformly across the page.

**Flag when you see:**
- `initial={{ filter: 'blur(Npx)' }}` or `from { filter: blur(Npx); }` on multiple distinct components in the same view
- Identical blur values (e.g., `blur(4px)`) repeated across components without context distinction
- Blur on text-bearing entrances (headings, paragraphs) where it impairs first-paint readability

**Heuristic:** Flag when ≥3 distinct components in the same view share the same `filter: blur()` enter pattern. Single uses with intent (a hero element, a modal) are fine.

**Fix lens:** Jakub, production polish, but selective. See `references/jakub-krehel.md`.

---

### Hover-scale-on-everything

`transform: scale(1.0X)` on `:hover` applied to every card, button, and image without intent. The micro-bounce-on-hover feels polished in moderation; AI-slop versions slap it on indiscriminately.

**Flag when you see:**
- `transition` rules with `transform: scale(1.0X)` on `:hover` across multiple card/button/image components
- Identical scale values (e.g., `scale(1.05)`) repeated across selectors with no discriminating context
- Tailwind `hover:scale-105` applied to grids of repeated items

**Heuristic:** Flag when ≥3 distinct components share the same `transform: scale(1.0X)` on `:hover` with no other discriminating selector context. Single intentional uses (e.g., a primary CTA) are fine.

**Fix lens:** Emil for utility-shaped elements (none); Jakub for product-shaped elements (selective). See `references/emil-kowalski.md` and `references/jakub-krehel.md`.

---

### Stagger-spam-on-every-list

`stagger`, `staggerChildren`, or hand-rolled `animation-delay: calc(var(--i) * 50ms)` patterns applied to every list, grid, or repeated-element block. Jhey-style stagger on a deliberate moment is delightful; AI-slop spreads it across every list as default polish.

**Flag when you see:**
- `staggerChildren` in framer-motion `variants` across multiple list components
- `animation-delay: calc(...)` with item-index multipliers across multiple components
- Sequential delays applied to lists that don't read as a moment (search results, settings options, table rows)

**Heuristic:** Flag when ≥2 lists in the same view use stagger entrance. One intentional moment is fine; two or more is the tell.

**Fix lens:** Emil for utility lists (no stagger); Jhey for delight moments (selective). See `references/emil-kowalski.md` and `references/jhey-tompkins.md`.

---

### Bouncy-springs-on-utility-actions

`type: 'spring'` with bounce on dropdown opens, toggle switches, menu reveals, modal entrances, utility actions where bounce reads as "playful" but the action itself is productivity-oriented.

**Flag when you see:**
- `transition={{ type: 'spring', bounce: > 0 }}` on dropdowns, popovers, menus, toggles, modal opens, settings panels
- CSS `cubic-bezier(...)` with overshoot values on utility elements
- Identical spring configs across utility components

**Heuristic:** Flag *any* spring with bounce > 0 on a utility action (dropdown, menu, toggle, modal, settings panel). Bounce belongs on playful elements, celebration moments, kids apps, intentional delight, not productivity UI.

**Fix lens:** Emil, speed and purpose. See `references/emil-kowalski.md`.

---

### Uniform-fade-in-on-every-element

Identical `opacity + translateY` (with or without blur) enter animations applied to every section, card, paragraph, and heading. The "polished entrance" treatment from Jakub used uniformly across the page, regardless of element type or context.

**Flag when you see:**
- Multiple components sharing identical `initial`/`animate` opacity+translateY values
- `whileInView` with identical viewport options applied to every block on a page
- CSS keyframes with generic names (`fadeInUp`, `enter`, `reveal`) attached to many selectors

**Heuristic:** Flag when ≥4 distinct components share identical enter animations (same opacity, same translateY, same duration, same easing). Three is acceptable polish baseline; four is uniformity slop.

**Fix lens:** Jakub, selective polish with hierarchy. See `references/jakub-krehel.md`.

---

### Motion-on-mount-for-static-content

Entrance animations on headings, body paragraphs, navigation links, and other content that should appear instantly. The "fade in everything" pattern that delays reading and makes the page feel sluggish.

**Flag when you see:**
- `initial`/`animate` on `<h1>`, `<h2>`, `<p>`, `<nav>` elements
- `whileInView` on body copy (paragraphs, articles, prose)
- `animation` rules on text-only components without functional reason

**Heuristic:** Flag any motion on a text-only or navigation element when the motion's only purpose is the entrance itself. Carousels, sliders, and hero animations are fine when the motion serves a function (orientation, narrative pacing, attention direction).

**Fix lens:** Emil, animations should serve a purpose, not announce themselves. See `references/emil-kowalski.md`.

---

## From Emil's Perspective (Purposeful Restraint)

- **Animating high-frequency interactions**: If users trigger this 100s of times daily, remove the animation
- **Animating keyboard-initiated actions**: Keyboard shortcuts should NEVER animate
- **Animations over 300ms**: UI animations should be under 300ms; 180ms feels more responsive than 400ms
- **Animating from scale(0)**: Start from `scale(0.9)` or higher for natural motion
- **Same tooltip behavior everywhere**: First tooltip: delayed + animated. Subsequent: instant
- **Using default CSS easing**: Built-in `ease` and `ease-in-out` lack strength; use custom curves
- **Ignoring transform-origin**: Dropdowns should expand from their trigger, not center
- **Expecting delight in productivity tools**: Users of high-frequency tools prioritize speed over delight
- **Using keyframes for interruptible animations**: Keyframes can't retarget mid-flight; use CSS transitions with state
- **CSS variables for frequent updates**: Causes expensive style recalculation; update styles directly on element
- **Distance thresholds for dismissal**: Use velocity (distance/time) instead; fast short gestures should work
- **Abrupt boundary stops**: Use damping; things slow down before stopping in real life

---

## From Jakub's Perspective (Production Polish)

- **Making enter and exit animations equally prominent**: Exits should be subtler
- **Using solid borders when shadows would adapt better**: Especially on varied backgrounds
- **Forgetting optical alignment**: Buttons with icons, play buttons, asymmetric shapes
- **Over-animating**: If users notice the animation itself, it's too much
- **Using the same animation everywhere**: Context should drive timing and easing choices
- **Ignoring hover state transitions**: Even small transitions (150-200ms) feel more polished than instant changes

---

## From Jhey's Perspective (Creative Learning)

- **Filtering ideas based on "usefulness" too early**: Make first, judge later
- **Not documenting random creative sparks**: Keep notebooks everywhere, including by your bed
- **Thinking CSS art is useless**: It teaches real skills (clip-path, layering, complex shapes)
- **Focusing on "How do I learn X?" instead of "How do I make Y?"**: Let ideas drive learning
- **Following tutorials without experimenting**: Tutorials teach techniques; experimentation teaches problem-solving
- **Giving up when something doesn't work**: The struggle is where learning happens

---

## General Motion Design Mistakes

- **Animating layout-triggering properties** (width, height, top, left). Use transform instead
- **No animation at all**: Instant state changes feel broken to modern users
- **Same duration for all animations**: Smaller elements should animate faster
- **Forgetting `prefers-reduced-motion`**: Not optional

*Note: Duration is designer-dependent. Emil prefers under 300ms for productivity tools. Jakub and Jhey may use longer durations when polish or effect warrants it.*

---

## Red Flags in Code Review

Watch for these patterns:

```jsx
// BAD: Animating layout properties
animate={{ width: 200, height: 100 }}

// GOOD: Use transform
animate={{ scale: 1.2 }}
```

```jsx
// BAD: Same animation for enter and exit
initial={{ opacity: 0, y: 20 }}
exit={{ opacity: 0, y: 20 }}

// GOOD: Subtler exit
initial={{ opacity: 0, y: 20 }}
exit={{ opacity: 0, y: -8 }}
```

```css
/* BAD: No reduced motion support */
.animated { animation: bounce 1s infinite; }

/* GOOD: Respects user preference */
@media (prefers-reduced-motion: no-preference) {
  .animated { animation: bounce 1s infinite; }
}
```

```css
/* BAD: will-change everywhere */
* { will-change: transform; }

/* GOOD: Targeted will-change */
.animated-button { will-change: transform, opacity; }
```

```jsx
// BAD: Animating from scale(0) (Emil)
initial={{ scale: 0 }}
animate={{ scale: 1 }}

// GOOD: Start from higher scale
initial={{ scale: 0.9, opacity: 0 }}
animate={{ scale: 1, opacity: 1 }}
```

```jsx
// Per Emil: Too slow for productivity UI
transition={{ duration: 0.4 }}

// Per Emil: Fast, snappy (but Jakub/Jhey might use 0.4 for polish)
transition={{ duration: 0.18 }}
```

```css
/* BAD: Dropdown expanding from center (Emil) */
.dropdown {
  transform-origin: center;
}

/* GOOD: Origin-aware animation */
.dropdown {
  transform-origin: top center;
}
```

```css
/* BAD: Keyframes can't be interrupted (Emil) */
@keyframes slideIn {
  from { transform: translateY(100%); }
  to { transform: translateY(0); }
}
.toast { animation: slideIn 400ms ease; }

/* GOOD: Transitions can retarget mid-flight */
.toast {
  transform: translateY(100%);
  transition: transform 400ms ease;
}
.toast.mounted {
  transform: translateY(0);
}
```

```javascript
// BAD: CSS variables cause cascade recalc (Emil)
element.style.setProperty('--drag-y', `${y}px`);

// GOOD: Direct style update
element.style.transform = `translateY(${y}px)`;
```

```javascript
// BAD: Distance threshold for dismissal (Emil)
if (dragDistance > 100) dismiss();

// GOOD: Velocity-based (fast short gestures work)
const velocity = dragDistance / elapsedTime;
if (velocity > 0.11) dismiss();
```
