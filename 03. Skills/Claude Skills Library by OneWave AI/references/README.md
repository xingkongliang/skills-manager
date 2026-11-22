# Claude Skills Library

Production-ready skills for Claude Code. Built and maintained by [OneWave AI](https://www.onewave-ai.com).

**230 skills** across three pillars: **business** (sales, marketing, consulting, ops), **everyday life** (personal finance, travel, fitness, job hunting), and **coding** (engineering, design, AI agent architecture).

Companion repo: **[Open Agent Stack](https://github.com/OneWave-AI/open-agent-stack)** -- 27 artifacts that need more than a single file. Highlights: **[vibe-stack](https://github.com/OneWave-AI/open-agent-stack/tree/main/plugins/vibe-stack)** (14 skills: idea to shipped app, with a real motion system and an AI-slop detector), **[sales-desk](https://github.com/OneWave-AI/open-agent-stack/tree/main/plugins/sales-desk)** (the rep's whole daily loop), and **[7 design-token themes](https://github.com/OneWave-AI/open-agent-stack/tree/main/design-styles)**. Skills here stay single-file and zero-dependency; anything with a manifest, a team, or a build step lives there.

---

## Featured: /agent-army

Deploy 3 to 50+ independent Claude agents in parallel, each with its own 1M token context window. Each agent spawns sub-agents underneath. A top-tier commander (Fable or Opus) orchestrates the swarms, and you pick a power level -- Max Power, Heavy, Balanced, or Economy -- that sets the Opus/Sonnet/Haiku model mix for each layer. The system runs in waves -- execute, audit, propagate -- and checks its own work between each.

Built for tasks where one agent isn't enough: large refactors, full-site audits, bulk content generation, codebase migrations. Battle-tested on 60+ concurrent agents in a single session.

**[View agent-army](https://github.com/OneWave-AI/claude-skills/tree/main/agent-army)**

---

## Getting Started

```bash
# Claude Code: install a single skill
claude skill install OneWave-AI/claude-skills/<skill-name>

# Or clone the full library
git clone https://github.com/OneWave-AI/claude-skills.git ~/.claude/skills

# OpenAI Codex CLI (same files, unchanged)
git clone https://github.com/OneWave-AI/claude-skills.git ~/.codex/skills
```

Each skill is a self-contained `SKILL.md` file. No dependencies, no build step.

These skills follow the [Agent Skills open standard](https://agentskills.io), so the same files work unchanged in **Claude Code, OpenAI Codex CLI, Gemini CLI, Cursor**, and 30+ other tools -- point your tool's skills directory at the clone and go.

---

## Categories

### AI Agent Architecture
Skills for building, orchestrating, and managing autonomous AI agents.

| Skill | Description |
|-------|-------------|
| `agent-army` | 2-layer parallel agent hierarchy for large tasks at maximum speed |
| `agent-to-agent` | A2A communication protocol -- message passing, shared context, handoffs |
| `agent-swarm-deployer` | Deploy swarms of sub-agents for massive parallel data processing |
| `agent-team-builder` | Design and deploy custom agent teams for business workflows |
| `sub-agent-orchestrator` | Parent/child agent orchestration with task delegation |
| `scout` | Recommends the best skill for any task |
| `scout-pro` | Enhanced scout with skill chains, pattern recognition, usage learning |
| `skill-navigator` | Guide to all skills with combination recommendations |
| `skill-composer-studio` | Chain multiple skills into custom workflows |
| `mcp-server-builder` | Build, wire, and debug an MCP server that connects Claude to your systems |
| `jev-integrate` | Wire a System One model (Jev/Von) into a feature -- typed decisions, gates, thresholds |
| `jev-eval` | Sweep criteria wordings and thresholds for a typed-decision config, with a runnable harness |
| `jev-audit` | Find LLM calls that are really classifications, and price the swap |

### Agent Workflow and Context Engineering
How to actually run a coding agent: what to put in context, what to write to disk, and the guardrails that keep the output honest.

| Skill | Description |
|-------|-------------|
| `plan-in-files` | Plans that live on disk and survive a context reset, crash, or handoff |
| `session-handoff` | Compress a session into a handoff doc the next session can resume from |
| `context-budget` | Spend the context window deliberately -- read narrow, silence noise, compact on time |
| `ai-coding-guardrails` | The ten rules that catch an agent's known failure modes before the diff lands |
| `grill-the-brief` | Interrogate a vague request before writing code, with defaults so nothing stalls |
| `claude-md-writer` | Write the CLAUDE.md / AGENTS.md that teaches an agent your repo's commands and traps |
| `fresh-library-docs` | Check the installed version and real signatures before calling an API from memory |
| `agent-eval-harness` | A 20-case eval set that catches prompt, model, and tool regressions before customers do |

### Anthropic / Claude Products
Skills built around specific Anthropic product releases.

| Skill | Description |
|-------|-------------|
| `overnight-repo-auditor` | Uses Managed Agents (14.5hr runtime) for autonomous overnight codebase audits |
| `multi-agent-client-onboarding` | Agent SDK: 3 parallel agents for client assessment |
| `gmail-to-crm-pipeline` | MCP Connectors: Gmail to CRM lead qualification pipeline |
| `full-codebase-migrator` | 1M context window: ingest entire codebases for migration planning |
| `claude-design-system-architect` | Generate a premium design system (tokens, type, motion) exported to Tailwind/CSS |
| `claude-landing-composer` | Build premium animated landing pages in Next.js + Framer Motion, anti-template |
| `claude-design-critic` | Audit a UI and de-AI it — design + copy fixes toward editorial/premium |

### Claude Cowork
Folder-first knowledge-worker workflows built for Claude Cowork -- now with cloud sessions, scheduled tasks that run with no device online, and Microsoft 365 write tools (July 2026).

| Skill | Description |
|-------|-------------|
| `cowork-deal-room` | Multi-step deal room due diligence -- contracts, financials, risk scoring |
| `cowork-folder-organizer` | Turn a messy folder or shared drive into an organized, indexed workspace with an undo trail |
| `cowork-inbox-triage` | Daily inbox sweep -- classify, draft replies in your voice, queue follow-ups; scheduled-task ready |
| `cowork-hiring-screener` | Score a folder of resumes against the JD -- ranked shortlist with evidence, interview kits, drafts |
| `cowork-expense-audit` | Reconcile receipts to statements, categorize, flag policy violations, output the expense report |
| `cowork-rfp-response` | Shred an RFP into a compliance matrix, mine your past proposals, draft the response honestly |
| `cowork-invoice-chaser` | AR aging report plus escalating dunning drafts matched to invoice age; weekly scheduled task |
| `cowork-contract-renewal-radar` | Extract every auto-renew clause and notice window, calendar the real decision deadlines |
| `cowork-data-room-builder` | Prep YOUR data room for a raise or acquisition -- checklist, gap report, organized structure |
| `cowork-vendor-comparison` | Normalize vendor quotes into one matrix -- true TCO, buried terms, negotiation leverage |
| `cowork-qbr-builder` | Client folder -> QBR with value receipts, honest misses, health read, expansion asks |
| `cowork-sop-writer` | Turn a process walkthrough into a clean SOP -- steps, decision points, tribal knowledge captured |
| `cowork-calendar-defrag` | Measure meeting load and fragmentation, propose cuts and focus blocks, draft the decline messages |
| `cowork-tax-prep-organizer` | Sweep folders for tax docs, build the CPA-ready package and the missing-form chase list |
| `cowork-medical-bill-auditor` | Match bills to EOBs, catch overbilling and unsubmitted claims, draft dispute letters |
| `cowork-home-inventory` | Photos + receipts -> insurance-grade home inventory with values, serials, and the gap list |

### Sales and Revenue
| Skill | Description |
|-------|-------------|
| `deal-closer-playbook` | Closing strategy with buying committee mapping and objection handling |
| `renewal-predictor` | Predict renewal likelihood from health score signals |
| `expansion-revenue-finder` | Identify upsell and cross-sell opportunities in existing accounts |
| `pipeline-health-analyzer` | Identify stalled deals, predict close probability |
| `deal-review-framework` | MEDDIC/BANT deal assessment with risk scoring |
| `deal-momentum-analyzer` | Score deal velocity from engagement patterns |
| `sales-forecast-builder` | Weighted pipeline forecast with scenario modeling |
| `sales-call-prep-assistant` | Pre-call research briefs with discovery questions |
| `sales-methodology-implementer` | MEDDIC, BANT, Sandler, Challenger, SPIN implementation |
| `lead-scoring-model` | Build custom lead scoring from historical win/loss data |
| `inbound-lead-qualifier` | Score inbound leads by ICP fit, intent, and urgency |
| `cold-email-sequence-generator` | Multi-touch outbound campaigns optimized for response |
| `personalization-at-scale` | Personalized first lines for hundreds of prospects |
| `champion-identifier` | Find internal champions in target accounts |
| `intent-signal-aggregator` | Monitor buyer intent signals across the web |
| `objection-pattern-detector` | Mine lost deals for recurring objection patterns |
| `lookalike-customer-finder` | Find companies matching your best customer profile |
| `quota-setting-calculator` | Top-down vs bottom-up quota models |
| `sales-comp-plan-designer` | Variable compensation design with accelerators |
| `sales-coaching-plan-generator` | Individualized rep development plans |
| `ramping-rep-tracker` | 30/60/90/120 day ramp milestones |
| `rep-performance-scorecard` | Multi-dimensional rep evaluation |
| `territory-planning-optimizer` | Account assignment by revenue potential |
| `icp-deep-scanner` | Read-only deep scan of connected tools → data-grounded ICP + persona library |
| `customer-panel-of-experts` | Your buyer personas debate any decision (launch, price, product) and recommend |
| `prospect-panel-simulator` | Simulate prospects to pressure-test emails, decks, and pages before sending |
| `pricing-change-strategist` | Plan a price increase: segmentation, scenarios, grandfathering, full comms kit |

### Consulting and Professional Services
| Skill | Description |
|-------|-------------|
| `client-proposal-generator` | Full consulting proposals from a brief |
| `sow-generator` | Professional Statements of Work with legal boilerplate |
| `client-health-dashboard` | RAG status across all client accounts |
| `churn-autopsy` | Post-mortem analysis when a client churns |
| `onboarding-checklist` | Customized client onboarding plans |
| `ai-readiness-assessment` | Assess how ready a business is for AI adoption |
| `saas-replacement-planner` | Evaluate which SaaS tools can be replaced with AI agents |
| `roi-calculator` | AI implementation ROI with sensitivity analysis |
| `meeting-intelligence` | Extract decisions, action items, and sentiment from transcripts |
| `meeting-to-tasks` | Convert transcripts to action items with owner assignment |
| `weekly-business-report` | Auto-generated weekly KPI reports |

### Engineering and DevOps
| Skill | Description |
|-------|-------------|
| `code-review-pro` | Severity-ranked security (OWASP Top 10:2025), correctness, and performance review |
| `debug-like-expert` | Methodical investigation with hypothesis testing |
| `api-load-tester` | Progressive load testing with bottleneck analysis |
| `database-migrator` | Cross-provider database migration with validation |
| `incident-responder` | Production incident response automation |
| `runbook-generator` | Operational runbooks from codebase analysis |
| `data-pipeline-builder` | ETL/ELT pipeline design and implementation |
| `dependency-auditor` | Security vulnerabilities and outdated packages |
| `test-coverage-improver` | Find and fill test coverage gaps |
| `docker-debugger` | Container troubleshooting and optimization |
| `typescript-migrator` | JavaScript to TypeScript migration |
| `env-setup-wizard` | Environment variable management |
| `error-boundary-creator` | React error boundaries and fallback UIs |
| `git-pr-reviewer` | Pull request quality review |
| `regex-debugger` | Visual regex breakdown and debugging |
| `performance-profiler` | Application performance profiling |
| `api-endpoint-scaffolder` | REST API endpoint generation |
| `react-component-generator` | React components with TypeScript and a11y |
| `database-schema-designer` | Query-driven schemas, indexes, ERDs, and safe migrations (PostgreSQL 18 default) |
| `landing-page-optimizer` | Conversion and performance optimization |
| `browser-test-loop` | Drive a real browser to prove a UI change works -- console errors included |
| `static-analysis-sweep` | Semgrep, secret, and CVE scanning over a diff, triaged into fix-now vs noise |

### Design Systems and UI
Build, install, and enforce design systems. Pairs with the 7 ready-made design-token themes in [Open Agent Stack `design-styles/`](https://github.com/OneWave-AI/open-agent-stack/tree/main/design-styles) -- aurora-mesh, cirrus, liquid-glass, mono-brutalist, neo-terminal, sand-terra, tidal.

| Skill | Description |
|-------|-------------|
| `design-style-installer` | Install a complete token theme (from Open Agent Stack or local tokens.json) and repaint the project |
| `design-tokens-sync` | Find and fix token drift -- stale copies, hardcoded values, orphans -- plus a CI guard |
| `motion-language-designer` | Duration/easing scales, choreography rules, signature moves -> Framer Motion + CSS |
| `dark-mode-converter` | Real dark mode via semantic tokens and elevation logic, not naive inversion |
| `typography-scale-builder` | Fluid type scale with per-step line-height/tracking, roles, vertical rhythm |
| `design-system-generator` | Design tokens, components, documentation |
| `css-animation-creator` | Professional animations and micro-interactions |
| `responsive-layout-builder` | CSS Grid, Flexbox, container queries |
| `screenshot-to-code` | Screenshots to React 19 + Tailwind v4 code, with a render-and-compare loop |
| `color-palette-extractor` | Extract palettes from images or sites -- HEX/RGB/Tailwind/CSS variables |
| `font-pairing-suggester` | Font pairings with hierarchy and loading strategy |
| `brand-consistency-checker` | Scan documents and slides for off-brand colors, fonts, logos |
| `accessibility-auditor` | WCAG compliance audit and fixes |

### Security and Compliance
| Skill | Description |
|-------|-------------|
| `compliance-checker` | GDPR, HIPAA, SOC2, CCPA, PCI-DSS audit |
| `security-pentest-planner` | Penetration test planning (OWASP Top 10) |
| `tech-due-diligence` | Technical due diligence for M&A/investment |
| `contract-analyzer` | Review contracts for concerning clauses |
| `contract-redliner` | Generate redline suggestions with replacement language |

### Marketing and Content
| Skill | Description |
|-------|-------------|
| `seo-optimizer` | Keyword analysis, readability, competitor comparison |
| `seo-keyword-cluster-builder` | Topic cluster architecture |
| `landing-page-copywriter` | High-converting copy using PAS, AIDA, StoryBrand |
| `brand-voice-analyzer` | Extract and codify brand voice from existing content |
| `content-repurposer` | Transform content into 8+ formats |
| `social-repurposer` | Adapt content for different platforms |
| `social-selling-content-generator` | LinkedIn thought leadership posts |
| `linkedin-post-optimizer` | Professional narrative with hooks |
| `utm-parameter-generator` | Standardized UTM tracking |
| `competitor-content-analyzer` | Track competitor content strategy |
| `competitor-price-tracker` | Monitor competitor pricing changes |
| `competitor-intel-agent` | Comprehensive competitor monitoring |
| `customer-review-aggregator` | Aggregate and analyze reviews from G2, Capterra, etc. |
| `podcast-content-suite` | Transform podcasts into content marketing |
| `webinar-content-repurposer` | Webinar to blog, social, email |
| `email-template-generator` | Professional email templates |
| `email-subject-line-optimizer` | A/B test subject lines |
| `product-launch-war-room` | Adversarial GTM war room: go/no-go, risk register, phased rollout, kill criteria |
| `hyperframes-ad-director` | Brief → finished HyperFrames video ad: hook, script, storyboard, scenes, cuts |
| `hyperframes-sales-demo-builder` | Personalized product-demo videos in HyperFrames for a specific account |
| `hyperframes-explainer-builder` | URL or docs → tight 60-90s product explainer with walkthrough scenes and VO |
| `hyperframes-testimonial-builder` | Real reviews and case-study results → social-proof videos: spotlights, review walls, proof reels |
| `hyperframes-local-promo` | 15-30s local-business promos: menu drops, offers, events, hiring clips |

### Strategy and Finance
| Skill | Description |
|-------|-------------|
| `pricing-strategy` | Pricing model design with competitive analysis |
| `market-sizing` | TAM/SAM/SOM with top-down and bottom-up estimates |
| `pitch-deck-reviewer` | Investor deck review with scoring |
| `board-deck-generator` | Board meeting presentation content |
| `investor-update-writer` | Monthly/quarterly investor updates |
| `executive-dashboard-generator` | Data to executive-ready reports |
| `financial-parser` | Extract data from invoices, receipts, statements |
| `portfolio-analyzer` | Investment portfolio risk and diversification |
| `budget-optimizer` | Spending analysis and savings strategies |
| `financial-goal-planner` | Savings targets and investment strategies |
| `tax-strategy-optimizer` | Pre-tax, Roth, charitable giving optimization |

### Operations and People
| Skill | Description |
|-------|-------------|
| `workflow-automator` | Design automated workflows from manual processes |
| `okr-generator` | OKRs following Google/Intel methodology |
| `customer-journey-mapper` | Full journey from first touch to advocacy |
| `hiring-scorecard` | Structured scorecards for any role |
| `knowledge-base-builder` | FAQ identification and tutorial creation |
| `technical-writer` | User guides, architecture docs, onboarding materials |
| `doc-coauthoring` | Structured documentation co-authoring workflow |
| `job-application-optimizer` | Tailor resumes to job postings |
| `raise-negotiation-prep` | Salary research and negotiation scripts |

### Work Tools
Skills that ship scripts and reference data, not just instructions -- the jobs a model gets wrong on its own.

| Skill | Description |
|-------|-------------|
| `bookkeeping-close` | Categorize, reconcile bank to ledger, and run month-end close from QuickBooks/Xero/bank exports -- never plugs a difference |
| `spreadsheet-model-auditor` | Script audit of any xlsx model: hardcodes, broken ranges, circular refs, inconsistent formulas, totals that do not foot |
| `bi-measure-builder` | DAX, Tableau calcs and LookML that total correctly -- filter context, LODs, time intelligence, with a checker script |
| `trade-quote-builder` | Estimates and bids for trades and field services -- waste, burden, overhead, margin vs markup, client xlsx/PDF |
| `crm-data-cleanup` | Vendor-neutral CRM dedupe and normalization -- confidence tiers, a dry-run merge plan, shared-inbox traps caught |
| `devils-advocate` | The strongest honest case against a decision you are leaning toward -- steelman, pre-mortem, no reflexive contrarianism |
| `statement-extract-and-prove` | Bank, card and brokerage statement PDFs to CSV/XLSX, proven: opening + transactions = closing, first broken row pinpointed |
| `spreadsheet-qa` | Answer questions about your own exports correctly -- profile, data dictionary, compute in code, show the rows and filters used |
| `template-deck-builder` | Decks built into your company's slide master and placeholders, storyline first, rendered and QA'd for overflow and off-brand |
| `sec-filing-puller` | 10-K/10-Q numbers from SEC EDGAR XBRL with a citation per number -- fiscal alignment, restatements, derived Q4, TTM |

### Small Business Essentials
The problems that actually kill small businesses -- cash, margins, visibility, people, the lease.

| Skill | Description |
|-------|-------------|
| `cash-flow-forecaster` | 13-week rolling cash forecast from bank exports and AR/AP -- crunch weeks circled with the levers to pull |
| `job-profitability-analyzer` | Which clients and jobs actually make money -- loaded labor costs, effective hourly rate per client |
| `local-seo-optimizer` | Win the Google local pack -- Business Profile audit, review engine, NAP sweep, location pages |
| `review-response-writer` | Respond to every review in the owner's voice -- gracious 5-stars, masterful 1-stars, escalation flags |
| `employee-handbook-builder` | Plain-English handbook that matches how the business actually runs, with state-law check flags |
| `inventory-reorder-planner` | Sales history -> reorder points, this week's purchase list, and the dead-stock cash report |
| `job-post-writer` | Job posts the right person actually applies to -- pruned requirements, honest sell, pay handled |
| `commercial-lease-analyzer` | True all-in occupancy cost, the small-tenant trap clauses, and the negotiation ask list |

---

## Gillby MCP: Water Intelligence

We built [Gillby](https://www.gillby.com) for anyone into boating, fishing, surfing or exploring the water in Florida and California—with more states to come. Gillby MCP offers free public water-data access within published limits, alongside open-source connection tools. Coverage varies by state and tool.

| Skill | Description |
|-------|-------------|
| [`gillby-water`](gillby-water/SKILL.md) | A general water-planning workflow for sourced conditions, tides, coastal places and authorized vessel context |

Want a specialist? The separate [Gillby MCP repository](https://github.com/OneWave-AI/gillby-mcp) contains **nine focused skills and six agent definitions**, including a surf guide. Those files are maintained there; installing this skills library does not install that bundle or connect the MCP server.

[Connect Gillby MCP](https://www.gillby.com/integrations#setup) · [Explore skills & agents](https://www.gillby.com/skills) · [Visit Gillby](https://www.gillby.com)

Public tools need no API key; personal boats, saved places and trips require authorization. The published tools have their own open-source licenses. Gillby’s hosted service, proprietary intelligence and underlying provider data remain subject to their own terms.

---

## Skill Format

Every skill is a single `SKILL.md` file with YAML frontmatter:

```yaml
---
name: skill-name                 # required — kebab-case, matches the folder
description: What the skill does and when to use it.   # required — drives auto-selection
tools: Read, Write, Bash, Agent  # optional — restrict tool access; omit to inherit all
model: inherit                   # optional — pin a model; omit to inherit the session model
---

# Skill prompt content here...
```

Claude Code loads this as a system prompt when the skill is invoked. Only `name` and `description` are required; most skills here use just those two. The `description` is what Claude reads to decide when to trigger the skill, so make it specific.

---

## About OneWave AI

[OneWave AI](https://www.onewave-ai.com) is a boutique AI consulting firm based in Florida, specializing in Claude and the Anthropic ecosystem. We help businesses from SMB to enterprise implement AI that ships real results -- from Claude for Enterprise deployment to custom agent architecture.

- [Claude Bootcamp & Team Training](https://www.onewave-ai.com/claude-bootcamp) -- in-person and virtual bootcamps, trainings, and workshops built on your real workflows
- [AI Training for Your Team](https://www.onewave-ai.com/ai-training) -- custom Claude + ChatGPT curriculum, then we deploy the tools you were trained on
- [Claude Consulting](https://www.onewave-ai.com/claude-consulting)
- [Services](https://www.onewave-ai.com/services)
- [Blog](https://www.onewave-ai.com/blog)
- [Contact](https://www.onewave-ai.com/contact)

---

## Contributing

1. Fork the repository
2. Create a new folder with your skill name
3. Add a `SKILL.md` following the format above
4. Submit a pull request

Skills should be production-ready, well-documented, and solve a real problem. No placeholder or stub skills.

---

## License

MIT
