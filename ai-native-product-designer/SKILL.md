---
name: ai-native-product-designer
description: AI Native Product Designer role - LLM-first workflow, AI code prototyping, Figma as finish line, self-serve research, outcome ownership. Use for leveling, workflow design, and AI readiness reviews. Includes quick reference, full rubric appendix, checklist, role comparison.
argument-hint: "[optional context: team, product area, maturity]"
---

# AI Native Product Designer

> **Role definition:** An AI Native Product Designer owns product experiences end to end - from problem definition through launch and iteration. They use AI tools as a core part of their workflow: starting in an LLM, prototyping in code-generation environments, and bringing validated concepts into Figma for systems and polish. They are accountable for outcomes, not just artifacts.

## When to use this skill

Use this skill when the user (or task) involves any of the following:

- Defining or leveling an **AI native** design role, team expectations, or hiring criteria
- Auditing whether a team or designer is **LLM-first**, uses **AI code tools for validation**, and treats **Figma as production polish** (not the first canvas)
- Designing **rituals** (PRD drafts with AI, edge-case surfacing, prototype fidelity, handoff quality)
- Preparing **interview rubrics**, performance criteria, or self-assessment against a modern product-design bar
- Comparing **traditional** vs **AI native** ways of working for a specific workflow

Do not treat this document as legal or HR advice; adapt language to your org.

## How you should respond

Unless the user asks for something else explicitly:

1. **Lead with outcomes.** Tie recommendations to user behavior, launch risk, or time-to-alignment - not tool fandom.
2. **Default output shape** (unless the user specifies a format):
   - **Summary:** 3-6 bullets on the biggest gaps or strengths vs this framework
   - **Gap list:** numbered, each gap tied to one cluster or row in the appendix rubric
   - **Prioritized actions:** top 3 changes for the next 1-2 sprints (each action specific enough to assign an owner)
   - **Optional:** one example prompt or ritual per top action (short, copy-paste ready)
3. **Use the appendix** for depth: pull exact row language when scoring someone or writing a job description.
4. If context is missing, ask **one** clarifying question (team size, B2B vs consumer, regulated or not) before a long assessment.

## Terminology

- **AI native** (two words, lowercase "native" in prose) describes the **role and workflow** (LLM-first, code-assisted validation, Figma for systems and handoff).
- **Native** alone in the rubric means the **top proficiency level** (Developing / Fluent / Native). Do not confuse "Native level" with "AI native designer."

## Quick reference (cheat sheet)

**Three-layer stack**

```
Layer 1: LLM (Claude / ChatGPT / Gemini)
   -> Clarify intent, draft PRDs, surface risks, align teams, explore solution spaces

Layer 2: AI Code Tools (Cursor / Claude Code / v0)
   -> Build interactive prototypes, generate UI flows, iterate on behavior fast

Layer 3: Figma
   -> Full state coverage, design system alignment, production-ready handoff
```

**Principle:** Figma is where design **finishes**, not where it starts.

**Proficiency in one line each**

- **Developing:** AI sometimes; default workflow still Figma-first.
- **Fluent:** AI is the default across the cycle; habits are in place.
- **Native (level):** AI workflow is deep; teaches others; shapes team patterns.

**Self-check (answer yes / no mentally or in chat)**

1. Do I open an LLM before Figma when starting a new design problem?
2. Have I built an interactive prototype using an AI code tool in the last 30 days?
3. Can I write a lightweight PRD draft with AI that a PM would review and use?
4. Have I talked directly to a customer in the last 2 weeks - without a research team involved?
5. Do I design for the 80% case first - and explicitly decide what gets hidden for the edge case?
6. Does every screen I hand off have complete state coverage - including error, empty, and loading?
7. Can I explain what success looks like for my current project in a specific, measurable way?
8. Do I check post-launch data on features I shipped and use it to drive the next iteration?
9. Have I shared a prompt, pattern, or workflow discovery with my design team in the last sprint?
10. Am I accountable for behavior change - not just design delivery?

**After the checklist:** For every **no**, propose **one concrete next step** that could happen this sprint (owner, artifact, or timebox). Avoid vague advice ("use AI more").

## Real-world alignment (example)

Public **Product Designer** listings increasingly mirror this stack (LLM for intent and light specs, **Claude** / **Cursor** / **Claude Code** for flows, **Figma** for systems and polish; outcomes over artifacts; lightweight research; sharing prompts). One public example is **Ramp** (not affiliated with this skill - use as market reference only).

---

## Appendix: Full competency rubric

Use this section for scoring, job descriptions, and detailed coaching.

### Why this role has different proficiency levels

Traditional design roles treat AI as an emerging skill. In this role, **AI fluency is a baseline requirement** - not a differentiator. The proficiency scale reflects depth of AI integration, not just awareness.

### Skill proficiency levels

| Level | What it means |
| --- | --- |
| Developing | Uses AI tools occasionally; still defaults to traditional design-first workflow. |
| Fluent | AI tools are the default starting point across the full design cycle; integrated and habitual. |
| Native | AI workflow is deeply embedded; teaches others; invents new patterns; pushes the frontier of what is possible. |

### Skill Cluster 1: AI-first design workflow

The ability to use LLMs as the primary thinking and alignment tool before any visual design begins.

| Skill | What it means in practice | Developing | Fluent | Native |
| --- | --- | --- | --- | --- |
| **LLM as design starting point** | Opening an LLM before Figma - using it to clarify intent, explore the problem space, and surface assumptions. | Uses LLMs for isolated tasks (copy, naming); still starts design work in Figma. | Starts every design problem in an LLM; uses it to frame the problem, draft approaches, and align with PM before any visual work. | Has developed personal prompt libraries and workflows that consistently accelerate from ambiguity to clarity faster than peers. |
| **Prompt engineering for design** | Writing prompts that generate useful design-relevant output - problem framing, PRD drafts, edge case lists, user flow sketches. | Uses basic prompts; output requires heavy editing. | Writes structured prompts with context, constraints, and output format; iterates the prompt, not just the output. | Builds and shares prompt systems used by the design team; coaches others on prompt design for product and UX use cases. |
| **PRD drafting with AI** | Using an LLM to draft lightweight product requirement documents that align the team on scope, intent, and open questions. | Can generate a rough PRD outline with AI assistance. | Produces a well-structured PRD draft from a conversation with an LLM; reviews with PM for alignment before visual work starts. | Owns the PRD-to-design handshake for the team; uses AI-generated PRDs to reduce alignment meetings significantly. |
| **Risk and edge case surfacing** | Using an LLM to systematically identify what could go wrong - error states, permission edge cases, unhappy paths - before UI work begins. | Asks AI for edge cases; takes the first list without pushing deeper. | Uses AI to generate a comprehensive edge case matrix; validates against engineering constraints; brings findings to team alignment. | Designs edge case surfacing as a repeatable team ritual; prevents entire categories of post-launch bugs by front-loading this work. |
| **Team alignment via AI artifacts** | Using AI-generated documents (PRDs, briefs, risk lists) as alignment tools with PM and engineering before entering Figma. | Shares AI outputs informally; alignment happens in meetings instead. | Uses AI-generated artifacts to run structured async alignment; reduces back-and-forth before visual work starts. | Designs team-level workflows where AI artifacts replace most early-phase meetings; alignment is async and documented. |

### Skill Cluster 2: AI-assisted prototyping and building

The ability to use AI code tools to build interactive prototypes that validate UX assumptions faster than Figma can.

| Skill | What it means in practice | Developing | Fluent | Native |
| --- | --- | --- | --- | --- |
| **AI code tool prototyping** | Using AI code generation tools to build interactive flow prototypes without writing production code by hand. | Has experimented with AI code tools; can run a basic prototype with significant AI help. | Regularly uses tools like Cursor or Claude Code to build interactive prototypes of flows and simple interfaces; iterates quickly. | Chooses the right prototype fidelity for each validation goal; can direct AI to build surprisingly complex interactions accurately. |
| **Directing AI code generation** | Knowing how to guide AI-generated code toward the right UX behavior - not just functional code, but correct UX quality. | Accepts AI output as-is; struggles to correct behavior without deleting and re-prompting. | Provides structured direction to AI: specifies interaction model, edge cases, and quality bar; iterates on behavior until it matches the intent. | Treats AI like a fast junior engineer - writes briefs that get high-quality first-pass code; reviews and directs at the UX level, not the syntax level. |
| **Prototype-to-product handoff** | Knowing what to move from an AI prototype into the actual product vs. what to rebuild in engineering. | Treats prototypes as throw-away; rebuilds everything from scratch. | Actively partners with engineering to identify which prototype code is clean enough to move into the product; saves development time. | Designs prototype-to-production workflows the whole team uses; reduces the gap between "validated prototype" and "shipped feature". |
| **Interaction fidelity judgment** | Knowing when a Figma static mock is enough vs. when an interactive code prototype is necessary to validate the right assumption. | Defaults to Figma for everything; only builds code prototypes when asked. | Chooses prototype fidelity based on what needs to be validated; does not over-build for low-risk decisions or under-build for complex interactions. | Defines team-wide prototype fidelity standards; coaches others on matching fidelity to the validation question. |
| **Front-end literacy** | Understanding components, states, props, responsive behavior, and basic layout constraints well enough to direct AI-generated code precisely. | Knows that components and states exist; struggles to direct AI to implement them correctly. | Understands component structure, state management basics, and layout primitives well enough to direct AI tools to produce correct implementations. | Bridges design and engineering conversations fluently; can read a PR and assess whether an interaction matches the design intent. |

### Skill Cluster 3: Product thinking and problem definition

The ability to define the right problem before designing any solution - in partnership with PM and engineering. **Scope here:** what you decide (problem statement, options, metrics intent, build tradeoffs). **Cluster 7** covers how you run partnership rituals day to day (crit, cadence, org learning).

| Skill | What it means in practice | Developing | Fluent | Native |
| --- | --- | --- | --- | --- |
| **Problem framing** | Defining the actual user and business problem before jumping to solutions - and keeping the team anchored to it throughout the project. | Can describe the problem; tends to jump to solutions quickly. | Writes precise problem statements that distinguish symptom from cause; uses them to push back on premature solutions. | Designs problem definition as a team ritual; prevents entire sprints of work on the wrong problem. |
| **Solution space exploration** | Generating and comparing multiple solution directions before committing to one - using AI to accelerate breadth. | Explores 1-2 options; commits to the first reasonable one. | Uses AI to rapidly generate 5-8 solution directions; evaluates them against user goals and constraints before narrowing. | Teaches teams to use AI for divergent thinking; designs exploration frameworks that surface non-obvious solutions. |
| **End-to-end ownership** | Staying involved from problem definition through post-launch iteration - not handing off and moving on. | Finishes designs and hands off; rarely involved in post-launch iteration. | Owns the full lifecycle: problem to design to launch to iteration; uses post-launch data to close the loop. | Designs team accountability models where design ownership extends through measurable behavior change. |
| **PM partnership** | Working with PMs to define success metrics, scope tradeoffs, and prioritize solution directions - before and during design. | Receives requirements from PM; executes. | Co-defines success metrics with PM; pushes back on scope; contributes to the product strategy, not just the UI. | Designs PM-designer working models; coaches teams on how to run effective problem definition sessions. |
| **Engineering collaboration** | Working directly with engineers throughout - not just at handoff - to make build decisions that serve the UX. | Hands off to engineers; answers questions reactively. | Involves engineers in design decisions early; understands implementation cost; makes tradeoffs that preserve UX quality. | Designs engineering collaboration patterns for the team; models the designer-as-technical-partner relationship. |

### Skill Cluster 4: Self-serve user research

The ability to run fast, lightweight research that informs direction without becoming a bottleneck.

| Skill | What it means in practice | Developing | Fluent | Native |
| --- | --- | --- | --- | --- |
| **Customer conversations** | Talking directly with customers to validate assumptions - without waiting for a research team. | Relies on researchers to set up and run interviews. | Schedules and runs customer conversations independently; extracts directional insights quickly. | Treats customer conversations as a weekly habit; builds informal customer networks that provide fast, continuous feedback. |
| **Assumption mapping** | Explicitly identifying the assumptions a design is built on - and ranking them by risk before testing. | Holds assumptions implicitly; does not surface them for testing. | Lists key assumptions before testing; prioritizes which to validate first based on risk and reversibility. | Designs assumption mapping as a team ritual before any significant design work begins. |
| **Quick testing** | Running fast, informal tests (5-second tests, hallway tests, unmoderated sessions) that validate direction without a formal study. | Waits for a full research study; no lightweight testing habits. | Runs quick tests with 3-5 customers or colleagues to validate direction; adjusts based on findings before committing to full execution. | Designs a library of fast-test templates the team can self-serve from; makes lightweight testing a team habit. |
| **Research as velocity tool** | Using research to accelerate design decisions - not as a gate that slows things down. | Treats research as a quality gate; it slows sprints. | Uses research findings to make faster, more confident decisions; research eliminates rework rather than adding time. | Coaches teams on the research-velocity mindset; demonstrates how a 2-hour test saves 2-week rework cycles. |

### Skill Cluster 5: Interaction design and craft

The ability to design interactions that are simple for most users while encoding judgment for edge cases.

| Skill | What it means in practice | Developing | Fluent | Native |
| --- | --- | --- | --- | --- |
| **80/20 design thinking** | Designing the default experience for the common case - and hiding complexity for the edge case. | Designs for all cases equally; complexity surfaces in the default experience. | Identifies the 80% case and optimizes ruthlessly for it; edge cases are accessible but not in the way. | Teaches the 80/20 design principle as a team-wide quality standard; reduces product complexity without reducing capability. |
| **Complexity encoding** | Hiding system complexity from users through smart defaults, progressive disclosure, and AI-powered automation. | Surface-level: shows all options and controls to all users. | Encodes judgment under the hood; most users never see complexity that does not serve them. | Designs systems where AI and product logic handle complexity invisibly; the interface is simple because the system is smart. |
| **Interaction design** | Designing the specific behavior of every interactive element - not just how it looks, but how it responds. | Designs static screens; interaction details are left to engineers. | Specifies interaction behavior explicitly: timing, transitions, response to edge cases, loading states, errors. | Sets interaction design standards for the team; reviews engineering implementations against the intended UX. |
| **State design** | Designing every state of every component - default, hover, focus, loading, error, empty, success. | Designs the default state; other states discovered during engineering. | Designs all states upfront as part of the design; no state is an afterthought. | Treats incomplete state design as a blocking issue in design review; designs state libraries that scale across the product. |
| **UX quality bar** | Setting and maintaining a high standard for interaction quality - and being able to articulate what "good" means. | Knows good UX when they see it; struggles to articulate the standard. | Defines the UX quality bar explicitly; uses it in reviews to give specific, actionable feedback. | Raises the quality bar across the design org through reviews, patterns, and documented standards. |

### Skill Cluster 6: Figma and design systems

The ability to bring validated concepts into Figma for production-ready handoff - the final layer of the design stack.

| Skill | What it means in practice | Developing | Fluent | Native |
| --- | --- | --- | --- | --- |
| **Validated concept translation** | Taking a concept that has been tested and built in code, and translating it into a complete Figma design for production. | Designs in Figma from the start; prototype learnings are reflected inconsistently. | Translates code prototype learnings into Figma cleanly; the Figma file reflects validated UX, not initial assumptions. | Designs a workflow where moving from code prototype to Figma is frictionless and fast; no information is lost in translation. |
| **Full state coverage in Figma** | Designing every state of every component and screen in Figma - not just the happy path. | Designs primary flows; states are incomplete. | Designs all states in every Figma frame before handoff; engineering has no visual questions. | Reviews others' Figma files for state completeness; treats incomplete states as a handoff blocker. |
| **Design system alignment** | Ensuring every Figma design uses and extends shared components - and flags where new components are needed. | Uses components inconsistently; creates one-off designs frequently. | Uses shared components correctly; creates new components with system-level thinking; flags system gaps proactively. | Contributes to design system governance; extends shared components thoughtfully; reduces design drift across the product. |
| **Production readiness** | Preparing Figma files that engineering can implement without follow-up questions. | Hands off Figma links; answers engineering questions reactively. | Produces Figma files that include complete specs, annotations, and edge case coverage; engineering questions are rare. | Sets production readiness standards for the team; audits handoff quality before engineering begins. |

### Skill Cluster 7: Cross-functional collaboration and communication

The ability to work directly and continuously with PMs and engineers - not just at handoff. **Scope here:** rituals, communication norms, crit quality, and how learnings spread. **Cluster 3** covers the substance of PM/eng decisions (metrics, scope, tradeoffs).

| Skill | What it means in practice | Developing | Fluent | Native |
| --- | --- | --- | --- | --- |
| **Direct PM partnership** | Working side-by-side with PMs to define problems, explore solutions, and adjust based on results - not receiving requirements. | Executes PM requirements; limited input on problem definition. | Co-owns the problem-to-solution process with PM; contributes to product strategy, not just UI execution. | Designs the PM-design collaboration model for the team; coaches both sides on how to work well together. |
| **Engineering-level communication** | Speaking the language of engineering - understanding enough about implementation to have a technical conversation. | Communicates at the UX level; engineers must translate. | Understands components, states, APIs, and performance constraints well enough to make informed tradeoffs in design. | Bridges design and engineering thinking; reduces implementation surprises by involving engineers earlier. |
| **Design crit participation** | Giving and receiving feedback in design reviews that raises the quality bar - not just validates existing work. | Presents work; receives feedback passively. | Gives specific, principle-grounded critique; pushes back constructively; shares prompts and learnings proactively. | Designs crit culture and process; models the quality of critique that makes the whole org better. |
| **Pattern and learning sharing** | Actively contributing AI prompts, design patterns, and workflow discoveries back to the design org. | Uses patterns discovered by others; rarely contributes new ones. | Shares prompts, patterns, and workflow discoveries regularly; contributes to a shared design knowledge base. | Builds the team's shared AI design library; designs systems for knowledge sharing that scale beyond individual contributions. |

### Skill Cluster 8: Metrics and outcome ownership

The ability to define success, measure it, and iterate until customer behavior changes.

| Skill | What it means in practice | Developing | Fluent | Native |
| --- | --- | --- | --- | --- |
| **Success metric definition** | Co-defining with PM the specific, measurable outcomes a design must achieve - before building. | Launches features; success metrics are set by PM separately. | Contributes to success metric definition; understands how design decisions map to measurable user behavior. | Owns the design-to-metric connection; designs experiences with specific behavioral outcomes in mind from day one. |
| **Post-launch iteration** | Staying involved after launch to measure results, interpret data, and ship improvements - not moving on at handoff. | Moves to the next project after handoff; post-launch iterations are driven by PM. | Reviews post-launch data; identifies where behavior diverged from hypothesis; drives design iterations based on evidence. | Designs post-launch review rituals for the team; ensures every shipped feature has a measured outcome loop. |
| **Outcome vs. artifact mindset** | Measuring success by customer behavior change, not by design deliverables shipped. | Reports on what was designed and delivered. | Reports on what changed in customer behavior as a result of the design. | Coaches teams on the outcome mindset; challenges org-level incentives that reward artifact completion over behavior change. |
| **Data-informed design** | Using quantitative and qualitative signals - analytics, session recordings, support data - to inform design decisions. | Uses data when presented by PM or analyst. | Pulls relevant data independently; interprets it in the context of design decisions; uses it to validate or challenge direction. | Designs data literacy practices for the design team; builds a culture of design decisions grounded in evidence. |

### Cross-functional skills

Skills shared with adjacent roles that make an AI Native Product Designer more effective.

| Skill | Why it matters for this role | Adjacent role |
| --- | --- | --- |
| **Basic SQL / analytics** | Designers who can pull their own data do not wait for analysts - they validate design decisions faster. | Data analyst |
| **Front-end development basics** | Understanding React components, state, and CSS makes AI-generated prototypes more accurate and handoffs cleaner. | Front-end engineer |
| **Product management fundamentals** | Designers who think in terms of bets, hypotheses, and metrics make better partners to PMs. | Product manager |
| **UX research methods** | Self-serve research requires knowing how to design a valid test - even a quick one. | UX researcher |
| **Systems thinking** | Complex products are systems - designers must understand how changes ripple across the whole experience. | Systems / product architect |

### Frontier skills (near-term horizon)

Capabilities that matter as agentic products, AI surfaces, and design systems converge. Horizon shifts with the market; revisit quarterly.

| Skill | Why it is rising in importance | How to build it now |
| --- | --- | --- |
| **Agentic UX design** | AI agents performing multi-step tasks on behalf of users require new interaction models - status, control, trust, and recovery. | Study current agent UIs (Perplexity, Claude Projects, Devin); design a conceptual agent interface for your product. |
| **AI feature UX patterns** | Designing AI-powered features (suggestions, summaries, generation, automation) requires new patterns for trust, transparency, and control. | Build a personal pattern library of AI UX patterns: loading states, confidence indicators, correction flows. |
| **Prompt-as-interface design** | As products expose natural language interfaces, designing the prompting experience itself becomes a product design problem. | Design a prompt input component end-to-end: empty state, examples, error handling, response states. |
| **Evaluation-driven design** | AI output quality is variable - designers must design evaluation loops that let users signal quality and improve AI responses over time. | Read RLHF and alignment basics; identify one place in your product where a feedback signal could improve AI output. |
| **Design system and AI** | AI components (streaming text, citations, tool use displays) need standardized components - the design system of the future includes AI patterns. | Propose one AI-specific component for your design system; spec all states. |

### Self-assessment checklist (tabular)

Answer YES or NO. Every NO is a growth area. Afterward, assign one concrete next step per NO (see Quick reference).

| # | Question | Yes / No |
| --- | --- | --- |
| 1 | Do I open an LLM before Figma when starting a new design problem? | |
| 2 | Have I built an interactive prototype using an AI code tool in the last 30 days? | |
| 3 | Can I write a lightweight PRD draft with AI that a PM would review and use? | |
| 4 | Have I talked directly to a customer in the last 2 weeks - without a research team involved? | |
| 5 | Do I design for the 80% case first - and explicitly decide what gets hidden for the edge case? | |
| 6 | Does every screen I hand off have complete state coverage - including error, empty, and loading? | |
| 7 | Can I explain what success looks like for my current project in a specific, measurable way? | |
| 8 | Do I check post-launch data on features I shipped and use it to drive the next iteration? | |
| 9 | Have I shared a prompt, pattern, or workflow discovery with my design team in the last sprint? | |
| 10 | Am I accountable for behavior change - not just design delivery? | |

### Role comparison: AI native vs. traditional product designer

| Dimension | Traditional product designer | AI native product designer |
| --- | --- | --- |
| **Design starts in** | Figma | LLM (Claude / ChatGPT / Gemini) |
| **Prototyping tool** | Figma / InVision | Cursor + Claude Code / v0 |
| **Figma's role** | Primary design environment | Final polish + production layer |
| **Research approach** | Researcher-led formal studies | Self-serve, velocity-first |
| **PRD relationship** | Receives PRD from PM | Co-creates PRD with AI + PM |
| **Engineering involvement** | At handoff | Continuous from problem definition |
| **Success metric** | Designs delivered | Customer behavior changed |
| **AI skill status** | Emerging / nice to have | Core requirement / day-1 expectation |
