---
name: superfuture-design-review
description: Run a sharp, prioritized design critique of a UI covering visual hierarchy, typography, spacing, color and contrast, motion, component states, responsiveness, accessibility, and brand consistency. Returns findings ranked from blocking to polish, each with a specific fix. Use in Figma Agent or Figma Make when asked to review, critique, or polish a screen.
---

# design-review

A senior-level craft critique. Opinionated and specific — not a generic checklist dump.

## 1. Evaluate against the rubric

Review the design against every dimension below. Check actual numbers where possible (contrast ratios, line-heights, measure in ch, tap-target px, animation durations) rather than vibes.

### Visual hierarchy & layout
- One clear focal point; obvious primary action. Eye knows where to go first.
- Scan path follows importance (size, weight, color, position).
- Related items grouped (proximity); unrelated items separated. Gestalt holds up.
- Everything aligns to a grid / shared edges — no stray, near-but-not-quite alignments.
- Whitespace used as structure, not leftover. Generous around focal elements.

### Typography
- Limited type scale (≈5–7 steps), each step clearly distinct (≥ ~1.2 ratio).
- Line-height: tighter for display (1.0–1.2), ~1.4–1.6 for body.
- Measure (line length) ~45–75 characters for body text.
- Display/large text has tightened letter-spacing; small caps/labels slightly loose.
- ≤2 typefaces (or a deliberate pairing). Consistent weights.
- No widows/orphans on headlines; no rivers; numerals consistent (tabular where aligning).

### Spacing & rhythm
- Consistent spacing scale (e.g. 4/8px steps) — no arbitrary 13px/27px gaps.
- Vertical rhythm consistent between sections; padding symmetric where expected.
- Density appropriate to content; touch UIs roomier than dense dashboards.

### Color & contrast
- WCAG AA: body text ≥ 4.5:1; large text (≥24px / 19px bold) and UI/icons ≥ 3:1.
- Never rely on color alone to convey meaning (add icon/label/shape).
- Restrained palette; accent color used sparingly for emphasis/CTAs.
- Sufficient contrast on disabled/placeholder without them reading as active.

### Motion
- Purposeful (guides attention, shows continuity) — not decoration that delays.
- UI transitions fast: ~150–250ms; larger/entrance ~300–500ms.
- Natural easing: ease-out for enters, ease-in for exits; avoid linear (except continuous).
- Honors `prefers-reduced-motion` — provides a reduced/instant variant.
- No layout shift, jank, or animations blocking interaction.

### Component states
- Every interactive element has hover, focus-visible, active, and disabled states.
- Visible focus ring (don't remove outline without a replacement) — keyboard users.
- Loading, empty, and error states designed (not just the happy path).
- Tap targets ≥ 44×44px (iOS) / 48dp (Android); adequate spacing between.
- Buttons/links look the part; primary vs secondary clearly differentiated.

### Accessibility
- Semantic structure (headings in order, `button` vs `a`, landmarks); ARIA only to fill gaps.
- All images have meaningful `alt` (or empty alt if decorative); icons have labels.
- Form inputs have associated labels; errors announced and linked.
- Full keyboard operability; logical focus order; visible focus.
- Color contrast (see above); supports zoom/200% and reduced motion.

### Responsiveness
- Fluid type/spacing (clamp) or sensible breakpoints; no fixed widths that overflow.
- No horizontal scroll; nothing clipped or overlapping at common widths (320–1440+).
- Touch vs pointer affordances appropriate; images don't distort (aspect-ratio).

### Content & copy
- Headlines clear and specific; scannable; jargon-free.
- CTA labels describe the action ("Start free trial", not "Submit").
- Microcopy guides (placeholders, helper text, error messages are human).
- No misleading states (e.g. a "Sent ✓" that didn't actually send).

### Consistency & brand
- Design tokens reused (color, type, spacing, radius, shadow) — no one-off values.
- Consistent corner radii, shadow elevation, icon style/stroke weight.
- Matches the product's brand voice and visual language across screens.

## 2. Report — ranked, concrete, scannable

Group findings by severity, most important first. **Lead with the few that matter; don't list everything.** For each finding give:

- **What** — the specific issue (name the exact element or layer)
- **Why** — the craft or usability reason it matters (one line)
- **Fix** — a concrete, specific change (exact value, not "increase spacing")

Severity tiers:

- 🔴 **Blocking** — broken, inaccessible, or fails WCAG AA / unusable on a target device.
- 🟠 **Important** — noticeably hurts hierarchy, readability, usability, or consistency.
- 🟡 **Polish** — refinement that sharpens the craft.

End with **"Strengths"** (2–4 things done well — critique builds on what works) and the single highest-leverage change to make first.

## 3. Apply fixes

When asked to apply, make the changes for the **clear, safe** findings (contrast values, spacing, focus states, reduced-motion, semantic structure, alt text). Leave subjective or restructuring changes as recommendations unless confirmed. Re-verify contrast and values after editing.

## Tone

Direct and respectful, like reviewing a colleague's work: precise about problems, never vague, and always paired with the fix. Calibrate depth to the surface — a marketing hero gets motion/type scrutiny; a form gets states/accessibility scrutiny.

---

*From the design-review skill at crit.officialjp.com. Invoke in Figma Agent with `/design-review` after selecting the frames to review.*
