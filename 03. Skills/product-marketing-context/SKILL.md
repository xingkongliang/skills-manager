---
name: product-marketing-context
description: Create and maintain product marketing context document. Other marketing skills reference it.
---

# Product Marketing Context

Help create and maintain a product marketing context document stored at `.claude/product-marketing-context.md`. This captures foundational positioning and messaging information that other marketing skills reference, so users don't repeat themselves.

## Workflow

**Step 1: Check for Existing Context**

Check if `.claude/product-marketing-context.md` exists. If it exists: read it, summarize what's captured, ask which sections they want to update, only gather info for those sections.

If it doesn't exist, offer two options:
1. **Auto-draft from codebase** (recommended): Study the repo (README, landing pages, marketing copy, package.json) and draft a V1. User reviews, corrects, fills gaps. Faster than starting from scratch.
2. **Start from scratch**: Walk through each section conversationally.

Most users prefer option 1. After presenting draft, ask: "What needs correcting? What's missing?"

**Step 2: Gather Information**

If auto-drafting: Read codebase (README, landing pages, marketing copy, about pages, meta descriptions, package.json), draft all sections, present draft, ask what needs correcting or is missing, iterate until satisfied.

If starting from scratch: Walk through each section conversationally one at a time. Don't dump all questions at once. Push for verbatim customer language - exact phrases are more valuable than polished descriptions.

## Sections to Capture

**Product Overview:** One-line description, what it does (2-3 sentences), product category (how customers search for you), product type (SaaS, marketplace, e-commerce, service), business model and pricing.

**Target Audience:** Target company type (industry, size, stage), target decision-makers (roles, departments), primary use case (main problem you solve), jobs to be done (2-3 things customers "hire" you for), specific use cases or scenarios.

**Personas (B2B only):** For each stakeholder (User, Champion, Decision Maker, Financial Buyer, Technical Influencer): What each cares about, their challenge, value you promise them.

**Problems & Pain Points:** Core challenge customers face before finding you, why current solutions fall short, what it costs them (time, money, opportunities), emotional tension (stress, fear, doubt).

**Competitive Landscape:** Direct competitors (same solution, same problem), secondary competitors (different solution, same problem), indirect competitors (conflicting approach), how each falls short for customers.

**Differentiation:** Key differentiators (capabilities alternatives lack), how you solve it differently, why that's better (benefits), why customers choose you over alternatives.

**Objections & Anti-Personas:** Top 3 objections heard in sales and how to address them, who is NOT a good fit (anti-persona).

**Switching Dynamics (JTBD Four Forces):** Push (frustrations driving them away), Pull (what attracts them to you), Habit (what keeps them stuck), Anxiety (worries about switching).

**Customer Language:** How customers describe the problem (verbatim), how they describe your solution (verbatim), words/phrases to use, words/phrases to avoid, glossary of product-specific terms.

**Brand Voice:** Tone (professional, casual, playful), communication style (direct, conversational, technical), brand personality (3-5 adjectives).

**Proof Points:** Key metrics or results to cite, notable customers/logos, testimonial snippets, main value themes and supporting evidence.

**Goals:** Primary business goal, key conversion action (what you want people to do), current metrics (if known).

## Tips

Be specific: Ask "What's the #1 frustration that brings them to you?" not "What problem do they solve?"

Capture exact words: Customer language beats polished descriptions.

Ask for examples: "Can you give me an example?" unlocks better answers.

Validate as you go: Summarize each section and confirm before moving on.

Skip what doesn't apply: Not every product needs all sections (e.g., Personas for B2C).