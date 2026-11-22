---
name: accessibility-auditor
description: Audit websites for accessibility issues and WCAG compliance. Use when checking accessibility, fixing a11y issues, or ensuring WCAG compliance.
---

# Accessibility Auditor

Audit a website for accessibility defects and bring it to WCAG 2.1 AA compliance.

## Contents

- `references/automated-testing.md` - CLI commands, jest-axe, Playwright, and tooling
- `references/wcag-checklist.md` - WCAG Level A/AA criteria and keyboard testing checklist
- `references/common-fixes.md` - Fix patterns for the 7 most common defects plus the sr-only class
- `references/aria-patterns.md` - ARIA markup for tabs, accordion, menu, alert, progress

## Workflow

1. Run automated checks. Execute the CLI scans and add jest-axe or Playwright assertions per `references/automated-testing.md`.
2. Test the keyboard. Tab through the page and verify order, activation, escape, focus visibility, and absence of traps using the checklist in `references/wcag-checklist.md`.
3. Test with a screen reader. Walk the page with NVDA or VoiceOver and confirm names, roles, and announced state.
4. Check WCAG criteria. Audit each Level A and AA criterion in `references/wcag-checklist.md`.
5. Apply fixes. Resolve each finding with the patterns in `references/common-fixes.md` and `references/aria-patterns.md`, then re-run step 1 to confirm zero violations.
