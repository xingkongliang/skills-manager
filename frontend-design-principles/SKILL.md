---
name: frontend-design-principles
description: Use when building or reviewing frontend UI — dashboards, admin panels, landing pages, marketing sites, web apps. Drives domain-specific design decisions (typography, color world, layout, CSS token naming, depth and spacing systems) instead of generic AI defaults; routes to app.md (product/data UIs) or marketing.md (public/creative pages) by context.
---

# Frontend Design Principles

Build frontend interfaces with craft and intention.

## Scope & Routing

After this file, load the context-specific guide:

- **`app.md`** — dashboards, admin and settings panels, internal tools, SaaS products, data-heavy interfaces (tables, forms, lists), anything users work in repeatedly.
- **`marketing.md`** — landing pages, marketing sites, announcements, creative artifacts, anything where first impression matters most.

If unclear, ask. Blended projects need both: a SaaS marketing site uses `marketing.md`; its product dashboard uses `app.md`.

## Why This Process Exists

You default to generic output — training patterns for dashboards and landing pages are strong. The process below helps, but process alone isn't craft: you have to catch yourself.

## Where Defaults Hide

Defaults disguise themselves as infrastructure — the parts that feel like they just need to work, not be designed. There are no structural decisions. Everything is design.

- **Typography isn't a container — it IS the design.** A bakery tool and a trading terminal both want "clean, readable type," but warm-and-handmade is not cold-and-precise. Reaching for your usual font means you're not designing.
- **Navigation isn't around the product — it IS the product.** Where you are, where you can go, what matters most. A page floating in space is a component demo, not software.
- **Data isn't presentation — it's meaning.** A progress ring and a stacked label both show "3 of 10"; one tells a story, one fills space. Ask what the number means to the person looking at it.
- **Token names are design decisions, not implementation detail.** Someone reading only your CSS variables should be able to guess the product:

```css
/* ❌ generic — could be any project */
--gray-700: …;  --surface-2: …;
/* ✅ domain — evokes this product's world */
--ink: …;  --parchment: …;
```

## Required: Before Generating

Do not write code until these are done.

### 1. Answer the intent questions

Answer with specifics or ask the user — do not guess, do not default:

- [ ] **Who is this human?** Not "users." The actual person — where are they, what's on their mind, what did they do 5 minutes ago and 5 minutes after?
- [ ] **What must they accomplish?** The verb: grade these submissions, find the broken deployment, approve the payment.
- [ ] **What should this feel like?** Words that mean something. Not "clean and modern" (every AI says that) — warm like a notebook? cold like a terminal? dense like a trading floor?

### 2. Produce the four required outputs

- [ ] **Domain:** concepts, metaphors, vocabulary from this product's world — territory, not features. Min 5.
- [ ] **Color world:** colors that exist naturally in this domain. Go to the actual world, not "warm/cool." List 5+.
- [ ] **Signature:** one element — visual, structural, or interaction — that could only exist for THIS product.
- [ ] **Defaults to reject:** 3 obvious choices for this interface type, visual AND structural. You can't avoid patterns you haven't named.

### 3. Propose direction and confirm

Reference your domain concepts, colors, signature, and what replaces each rejected default.

```
Domain: [5+ concepts from the product's world]
Color world: [5+ colors that exist in this domain]
Signature: [one element unique to this product]
Rejecting: [default] → [alternative] ×3

Direction: [approach connecting the above]

Does that direction feel right?
```

**Test:** remove the product name from your proposal — could someone still identify what it's for? If not, explore deeper. Wait for confirmation before generating code.

## Required: Before Showing

Your first output is probably generic; the work is catching it before the user has to. Look at what you made and ask "if they said this lacks craft, what would they mean?" — fix that first. Run these; iterate if any fails:

- [ ] **Swap test:** swap your typeface/layout for the usual one — would anyone notice? Where swapping wouldn't matter is where you defaulted.
- [ ] **Squint test:** blur your eyes. Is hierarchy still readable? Anything jumping out harshly? Craft whispers.
- [ ] **Signature test:** point to specific components where your signature appears. "The overall feel" doesn't count.
- [ ] **Token test:** read your CSS variables aloud. Do they belong to this product's world, or any project?

## Principles

- **Every choice must be a choice.** For every decision — layout, color temperature, typeface, spacing scale, hierarchy — you can state WHY. "It's common / clean / it works" is a default, not a choice.
- **Sameness is failure.** If another AI given a similar prompt would produce substantially the same output, you failed. Design from intent and sameness is impossible — no two intents are identical. Linear's cards don't look like Notion's; Vercel's metrics don't look like Stripe's.
- **Intent must be systemic.** "Warm" then cold colors is not following through. If warm: surfaces, text, borders, accents, semantic colors, type — all warm. Check every token against the stated intent.
- **Infinite expression.** A metric can be a hero number, sparkline, gauge, delta, trend badge, or something new. Never ship the same sidebar width + card grid + icon-left-number-big every time — it reads as AI-generated instantly.

## Craft Foundations

The quality floor, regardless of direction:

- **Subtle layering.** Surfaces barely different but distinguishable — whisper-quiet elevation shifts, not dramatic jumps (study Vercel, Supabase, Linear). Borders light enough to disappear until you look for structure; if they're the first thing you notice, they're too strong.
- **Color lives somewhere.** Spend time in the product's physical world before reaching for a palette — materials, light, objects. It should feel like it came FROM somewhere, not applied TO something. Gray builds structure; color carries meaning. One intentional accent beats five thoughtless ones.

## Universal Anti-Patterns

Always wrong, regardless of context:

- **Dramatic drop shadows** — `box-shadow: 0 25px 50px …` looks cheap
- **Arbitrary asymmetric padding** — every value should be intentional
- **Thick decorative borders** — 2px+ for visual weight
- **Multiple accent colors** — one accent, used consistently
- **Mixing depth strategies** — borders OR shadows, not both randomly
- **Inconsistent spacing** — the clearest sign of no system

## Communication

Be invisible. Don't announce modes or narrate process ("Let me explore the domain…", "Running the checks…"). Jump into the work; state suggestions with reasoning.

## Deep Dives

- `references/principles.md` — spacing, depth, typography, color implementation, dark mode (concrete values and CSS)
- `app.md` — dashboards, admin panels, SaaS products
- `marketing.md` — landing pages, marketing sites, creative artifacts
