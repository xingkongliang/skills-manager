---
name: scroll-craft
description: >
  Build premium scroll-driven landing pages for service, product, food, and
  drink brands. Plan the visitor journey, page grammar, emotional peak, and
  bespoke signature move. Create dimensional heroes with independent visual
  planes, restrained motion, and separate mobile composition. Use supplied
  photos and footage or generate photoreal assets through kie.ai, write semantic
  HTML, and verify desktop, mobile, and reduced-motion scroll states visually.
  Use for "scrollcraft", "scroll craft", "layered hero", "premium hero",
  "cinematic hero", "scrollytelling", "scroll animation site", "a site where
  scrolling plays a video", "Apple-style landing page", "3D scroll world",
  "interactive landing page", "make my brand a scroll experience", "this looks
  like a template", or requests for a distinctive website that feels like an
  experience rather than a document.
allowed-tools: Bash, Read, Write, Edit, Glob, Grep, AskUserQuestion
---

# scroll-craft

Scroll is the only input every visitor already knows how to use. This skill
treats it as a timeline: the wheel is a scrubber, the page is a film with real
text on top, and each section behaves differently enough that the visitor keeps
going to find out what the next one does.

**What you produce:** an interview brief, a page grammar, a customer-journey map,
a feeling curve with one engineered peak, a scroll score, one signature move,
generated assets, one real HTML page on a token-driven design floor, and a strip
of screenshots proving it holds up at every scroll position.

## What this is not

It is not "generate a flythrough and drop text on it." That approach produces
one device applied to a whole page, and every site built that way is
recognisable at a glance: same claymation diorama, same centred copy, same
`01 / 06` counter, same "scroll to explore" nudge. Five sections that behave
identically are one section shown five times.

Four rules follow from that, and they are the spine of this skill:

1. **Variety is the product.** A page uses at least four device families and
   never the same device twice in a row. Read [references/devices.md](references/devices.md).
2. **The world is photographic unless the brand is genuinely illustrated.**
   Soft matte low-poly clay diorama is banned as a default. Read
   [references/worlds.md](references/worlds.md).
3. **No continuous chain.** A single unbroken camera flight is the most
   expensive and most fragile thing you can build, and it exists only to hide
   cuts between scenes. Vary the device instead and the cut disappears for free,
   because the visitor is not watching one film. Chain only when the brief is
   literally "one continuous journey."
4. **A different world is not a different page.** The device kit varies how a
   page looks. Structure is a separate axis, and it has to be decided
   deliberately or every build inherits the same skeleton. The first four builds
   did exactly that. Read [references/uniqueness.md](references/uniqueness.md).

## Standing hero preference

**Dimensional layering is a baseline requirement for a premium marketing hero.**
Plan independently moving background, subject, foreground, and atmospheric
planes before requesting assets. A beautiful single background with text fades
does not satisfy this preference. Depth must come from visible separation,
occlusion, and controlled differences in movement, while the headline stays
readable and the scene tells one clear story.

Read [references/hero-depth.md](references/hero-depth.md) before planning the hero
or generating its assets. It covers clean plates, genuine alpha cutouts, shared
contact anchors, typography between planes, restrained scroll choreography,
mobile art direction, and visual acceptance, with the approved Sonder example.
Use the composition that fits the chosen grammar; do not repeat Sonder's scene
or impose pinning on a grammar that forbids it. Honor explicit static or simpler
directions, and keep depth in the static composition when motion is reduced.

## Approved collection standard

Read [references/approved-collection.md](references/approved-collection.md)
before planning a premium marketing site. It distills the approved ten-site
rebuild: real brand research, layer contracts, meaningful pointer/scroll depth,
appropriate photographic or 3D rendering, distinct navigation and endings,
useful interactions, mobile art direction, and evidence from the final package.
These are execution principles and worked examples, not a reusable page skeleton.

## Step 0: The brief and creative authority

**Establish the brief before generating anything.** Reuse answers and assets
already provided. When the user explicitly delegates creative direction (for
example, "use your judgment" or "I want to get out of your way"), write a
`Self-authored under explicit creative delegation` brief and proceed. Cover the
eight topics below, distinguish evidence from assumptions, and do not invent
user quotations or force another interview/approval checkpoint.

Otherwise, interview for the missing decisions. Ask actual questions, record
the answers, and avoid inferring an entire brand from its name.

The skill is a range instrument, not a house style. The human brings intent and
whatever assets they own; the interview is where that turns into the right kind
of page: one unbroken world, distinct scenes, printed chapters, a live surface.
The skill can do any of them. The interview decides which.

Keep it short. Eight questions, asked in one pass:

1. **Vibe in three to five words**, plus up to three references from any medium.
   A film, an album cover, a shop, a magazine, a game. Not "sites you like":
   naming sites is how a page ends up looking like an existing site.
2. **The scroll journey, section by section, in their words.** What the visitor
   should hit first, what comes next, what the last thing is. Their sequence,
   not a menu you offered.
3. **The energy curve.** Where it should feel calm, where it should feel
   intense. A page that is loud the whole way is as flat as one that is quiet
   the whole way.
4. **How should someone feel while scrolling, stage by stage, and what is the
   ONE moment they should remember?** Energy is loudness. This is emotion, and
   the two do not line up: on a loud page the quiet act can be the most intense.
   The stage-by-stage answer becomes the feeling curve, the one moment becomes
   the peak. Both are required in BRIEF.md. See [references/feel.md](references/feel.md).
5. **One thing this site should do that no site they have seen does.** This is
   the seed of the signature move. Push for a real answer; "be memorable" is not
   one.
6. **How far from premium-minimal they want to go.** Offer the range in
   [uniqueness.md §5](references/uniqueness.md): brutalist, maximalist, playful,
   retro, dense, editorial, premium-minimal. Their answer governs the aesthetic
   family, not your taste.
7. **One unbroken world, or distinct scenes?** Should the whole page feel like
   one continuous place the scroll flies through (worldflight, see
   [references/worldflight.md](references/worldflight.md)), or like separate
   scenes, chapters, or cuts? This is the single biggest structural fork, and it
   is their call, not a device you pick later. Offer both plainly; neither is
   the default.
8. **What assets do they already have?** Footage, photos, product shots, a
   brand kit, clips of themselves. Real assets anchor the world and cut
   generation cost; the answer decides what gets graded and encoded versus
   generated. "Nothing" is a fine answer and means a fully generated world.

Write the answers into `<workspace>/builds/<name>/BRIEF.md` before any act planning, in
their words, not paraphrased into marketing prose. Everything downstream reads
from that file.

BRIEF.md must contain, at minimum:

- The eight topics, with verbatim user answers where supplied and clearly labeled authored decisions where delegated.
- **The feeling curve.** One line per act: the emotion, then what on screen
  causes it. Written before the acts exist, added to as the score fills in.
- **The peak.** The one moment, written as the sentence a visitor would say to
  a friend, plus which act it lives in.
- **The completed tell-someone sentence.** "It's the site where ___", filled
  with an experience, not a device name.
- Any authored silence, so the verification pass can tell it from dead scroll.

[references/feel.md](references/feel.md) is the spec for all four.

**If the human is genuinely unreachable** and the run is fully autonomous, write
BRIEF.md yourself: answer all eight questions in the brand's voice, mark the file
`Self-authored, not interviewed` at the top, and say so in the final report. A
self-authored brief without delegation is a fallback. Explicit delegation above
is a normal supported workflow and does not require the human to be unreachable.

## Bootstrap

Environment, not a stage of the work. Do it once the interview is answered and
before Step 1.

**Run the preflight rather than checking by hand.** It knows the failure modes
that otherwise surface later as misleading errors, chiefly a stripped ffmpeg
that reports a missing filter as a syntax error in your command:

```bash
node <skill>/scripts/doctor.mjs
```

It reports node, a full ffmpeg build, playwright and Chrome, the API key, and
the resolved workspace. Required failures exit non-zero. Say plainly which items
are missing rather than working around them silently.

### The workspace

Builds and the fingerprint registry live in one directory, and **it is resolved,
never assumed**:

```bash
node <skill>/scripts/workspace.mjs --ensure     # prints it, creates it, seeds the registry
```

Resolution order, first hit wins:

1. `SCROLLCRAFT_HOME`
2. the nearest `.scrollcraft.json` walking up from the cwd, `{ "workspace": "..." }`
3. `<project root>/scrollcraft`, where the project root is the nearest ancestor
   holding a `.git`

So a build folder is `<workspace>/builds/<name>/` and the registry is
`<workspace>/FINGERPRINTS.md`. The registry starts **empty**: the gate exists to
stop you repeating yourself, so your first build has nothing to clear.

If you already keep builds somewhere else, drop a `.scrollcraft.json` at your
project root pointing at it and nothing moves.

### The rest

1. `KIE_AI_API_KEY`, **only if you are generating assets.** A build from the
   user's own photos and footage needs no key and no spend, and that is a
   first-class route, not a fallback. Confirm balance with
   `node <skill>/scripts/kie.mjs probe`. A still costs cents and a 5s clip costs
   more; a six-act page with two clips is a small spend, not a large one.
2. A brand kit if one exists (colours, logo, type, existing product shots). If
   the brand has a folder in this repo, read it before generating anything, and
   obey its hard rules. A brand that forbids invented numbers means no stat
   counters, however good they look.

Copy `engine/scrollcraft.js` and `engine/scrollcraft.css` into the build folder.
Never edit the engine per-project; it is the mechanism. Theme it with tokens and
write your own markup.

## Step 1: The brief, journey first

The subject is the user's to state. Ask it open, in plain prose, never as a
fabricated multiple-choice list of industries: a made-up menu biases them and
reads as you deciding their business for them.

Step 0 already covered vibe, sequence, energy and range. Do not ask any of it
again. Ask only what you cannot sensibly default:

1. **What is this, and who is it for?** One or two sentences in their words.
2. **What must the visitor believe by the end?** The single sentence the page
   exists to install. Not a feature list. If they give three, make them pick.
3. **What does the visitor do next?** One action. One label for it, used
   everywhere on the page.
4. **What do you already have?** Logo, palette, photography, product shots,
   footage, a brand doc. Real assets beat generated ones every time.
5. **Art direction**: offer the worlds in [references/worlds.md](references/worlds.md)
   as a real choice, and say they can go their own way.

Then write the **journey** before anything else: four to seven beats, each one a
shift in what the visitor knows or feels.

```
1  Recognition   they see their own morning
2  Tension       the cost of it, named plainly
3  Turn          the thing that changes
4  Substance     why it holds up
5  Range         what they can choose
6  Commitment    the one action
```

Beats are the spine. Sections serve beats; a section that serves no beat is cut,
however nice the shot is. Resolve the journey before generating assets. Show it
to the user when their decisions are needed; under explicit creative delegation,
record the chosen journey and proceed within the authorized scope.

## Step 2: Grammar, gate, then score

Three things in order, and the first two come before any act planning. Full
detail in [references/uniqueness.md](references/uniqueness.md).

**Pick a grammar.** Start with the eight defined grammars and their constraints.
A new grammar is allowed when its navigation, sequence, ending, and explicit
bans describe a different structure; a new label alone earns no credit. Filmic one-shot is the one the first four
builds all used, so choosing it again means saying in the report why the other
seven did not fit the interview. Nav, hero and close all follow from the
grammar; they are not decided separately.

**Invent the signature move.** One bespoke interaction that lives on this site
alone, coded in the page, not a parameter change to a kit device. Question 5 of
the interview is the seed. The engine stays untouched.

**Run the fingerprint gate.** Read your registry at
`<workspace>/FINGERPRINTS.md` (see **The workspace** in Bootstrap; run
`node <skill>/scripts/workspace.mjs` to print the path). The planned build must differ
from **every** existing row on at least 4 of 6 dimensions: grammar, nav
treatment, hero device, act-sequence shape, close pattern, signature move. Four
against each row individually. If it fails, change the plan, not the log.

**Write the feeling curve before the score table.** One line per act: the
emotion, then what causes it. Curve first, acts second, because a device chosen
before the feeling is a device looking for a reason. Two adjacent acts with the
same feeling means one is filler, and it is cheaper to cut it here than after
the assets exist. Name the peak in the same pass and give it the largest span on
the page. Full method in [references/feel.md](references/feel.md).

Then assign each beat a device. Do it deliberately and write it down as a table:

| Beat | Device | Why this one |
|---|---|---|
| Recognition | `scrub` | The camera moving under the reader's own hand is the strongest possible open |
| Tension | `pin` + kinetic | Copy assembles line by line while the frame holds still |
| Turn | `reveal` | A wipe is a change of state, which is what this beat is |
| Substance | `scrub` (macro) | Texture at a scale the eye cannot get otherwise |
| Range | `pan` | Lateral travel reads as "options", vertical reads as "argument" |
| Commitment | `pin` + pointer | The page stops moving and starts responding |

That table is a **filmic** score. It is the right shape for one grammar and the
wrong shape for the other seven, so read your grammar's leans-on and bans list
before filling in a row.

Checks before you build:

- The grammar's bans hold. A grammar that forbids `pin` forbids it here too,
  however well it would have worked.
- Four or more distinct device families. Fewer means the page has one idea.
- No device family twice in a row.
- At most two `scrub` acts. Video is the heaviest thing on the page, and the
  third one stops being a surprise.
- No two adjacent acts carry the same feeling. If they do, one is filler.
- One act is the peak and it has the largest span by a visible margin. The act
  before it is quieter than it is.
- Every act earns its scroll span. Eight to fourteen viewport-heights is a
  pacing reference for longer cinematic pages, not a quota. Shorter editorial,
  gallery, or working-surface grammars should stay short when the journey is
  complete. Never add filler or empty pinning to hit a length target.
- The act count and total length do not land in the 6-to-7 acts at 13.6-13.8vh
  band that all four prior builds hit. That band is a fingerprint dimension now.

## Step 3: Generate the assets

Full pipeline, prompt scaffolds and model notes: [references/assets.md](references/assets.md).

Short version:

```bash
node <skill>/scripts/kie.mjs still "<style preamble>\n\n<scene>" out/01-hero.png --ar 16:9 [--ref brand-can.png]
node <skill>/scripts/kie.mjs shot  "<camera move>" out/01-hero.png out/01.mp4 --dur 5
bash  <skill>/scripts/encode.sh out/01.mp4 assets/01.mp4
bash  <skill>/scripts/encode.sh out/01.mp4 assets/01-m.mp4 mobile
```

Four things that decide whether this looks premium or generated:

- **One style preamble, reused verbatim in every prompt.** This is what makes
  six separate images look like one shoot. Write it once, never paraphrase it.
- **Look at every asset before you use it.** Read the PNG. Generation is cheap
  and rerolling is cheaper than shipping a bad frame.
- **Encode for scrubbing, not playback.** `encode.sh` sets a dense GOP because
  seeking walks from the previous keyframe. A normal web encode plays perfectly
  and scrubs like mud.
- **Layer the hero.** Cut the scene into planes that scroll at slightly
  different rates, with the product parked between the mid and foreground
  planes. Depth from differential movement is the cheapest premium signal on
  the page. The recipe and its traps are under `parallax` in
  [references/devices.md](references/devices.md).

## Step 4: Build the page

Write real HTML. Real `<h1>`, real `<p>`, real links, real reading order. The
engine reads `data-sc-*` attributes off your markup and drives it; it never
generates DOM. A runtime that builds the page from a config object is exactly
why every site built on one looks the same.

Start from `references/template.html`. The device patterns are in
[references/devices.md](references/devices.md); the spacing, type, depth and
colour rules are in [references/taste.md](references/taste.md). Read taste.md
before writing markup, not after, and build without announcing the checklist.

Theme by overriding tokens, six values and two fonts:

```css
:root {
  --sc-canvas: #0A0806;  --sc-surface: #16110E;
  --sc-ink:    #F5EBDD;  --sc-ink-soft: #A2968A;
  --sc-accent: #FF5A3D;  --sc-accent-ink: #15110F;
  --sc-font-display: "Archivo", system-ui, sans-serif;
  --sc-font-text:    "Geist", system-ui, sans-serif;
}
```

## Step 5: Verify by scrolling it

Apply the final-source and deployment checks in
[approved-collection.md §7](references/approved-collection.md#7-require-visual-and-functional-evidence-before-delivery).
Use safe headless automation with native pointer lock/capture disabled in every
context. Inspect actual pixels, phone compositions, and intermediate states;
test useful controls, downloads, and form outcomes in the final package.

Not optional, and not "it should work." A scroll page has no single state:
every position is a different frame, and the failures live between the two you
happened to look at. Full procedure: [references/verify.md](references/verify.md).

```bash
cd <build project> && npm i playwright-core     # once
node <skill>/scripts/serve.mjs --root . --port 4500 &
node <skill>/scripts/shoot.mjs --url http://localhost:4500 --out lab/shots
node <skill>/scripts/shoot.mjs --url http://localhost:4500 --out lab/mobile --width 390 --height 844
node <skill>/scripts/shoot.mjs --url http://localhost:4500 --out lab/reduced --reduced-motion
```

The harness walks each act at six positions, waits for the scrub video to
actually settle, and reports **dead scroll**, **cues that never reach full
opacity**, and **contrast measured on the composited page at the brightest
frame under each line**. It writes a contact sheet.

Then do the part the harness cannot: **read `sheet.png`.** It proves a clip
advances; it cannot tell you the composition is good, the motion is smooth, or
the page means anything. Also tab through for focus order.

**Then run the feel check** ([references/feel.md §6](references/feel.md)). Scroll
the page cold, write one word per act for what you felt, and only then open
BRIEF.md and diff it against the intended curve. Where they disagree the page is
wrong, not the brief. Confirm on the sheet that the peak is the largest visual
change and holds the most scroll room, and that the last screen resolves instead
of fading to nothing.

**And say what a green run does not cover: a real phone.** Headless Chrome
cannot reproduce an iPhone's video decoder, autoplay policy, Low Power Mode,
or touch scrolling; a build once shipped four green rounds while the hero clip
sat frozen on the actual device. Mobile is a first-class target throughout,
not a pass at the end: portrait phone clips, touch-tuned lerp, grown tap
targets are all authored (see assets.md and verify.md). When any mobile defect
is reported, deploy `references/device-diag.html` beside the site on the
**first** round and let the device answer, rather than theorising from a
machine that cannot reproduce the failure. The full iOS clip-lifecycle notes
live in verify.md, "The phone is a different machine". To open the page on a
phone over Wi-Fi, start the server with `--lan` (by default it binds to this
machine only), and stop it when you are done: `--lan` makes everything under
`--root` readable by anyone on that network.

Fix what you found and shoot it again. Report what you actually verified and
what you did not.

## Hard rules

Ship-blockers, not preferences. Each one is a thing that makes a page read as
machine-made.

| Never | Instead |
|---|---|
| Clay diorama / low-poly / claymation as the default world | Photographic. See worlds.md |
| A "scroll" cue, arrow, or animated mouse icon | Nothing. They are looking at the hero; they know |
| `01 / 06` section counters | Delete them. Sequence is not information here |
| An eyebrow above every section heading | At most one per three sections. The heading carries itself |
| Em dash anywhere visible | Period, comma, colon, or parentheses |
| Centred copy in every act | Vary the anchor: lead, trail, centre, split |
| The same device twice in a row | Score the journey properly in Step 2 |
| Generating before the brief and creative authority are clear | Run Step 0. Record actual answers or explicitly delegated authored decisions in `BRIEF.md` |
| A page with no engineered peak, or with three competing ones | One peak. It gets the asset budget, the silence before it, and the most scroll room. See feel.md §2 |
| An ending that trails off, fades out, or just becomes a footer | The close resolves and holds. The last feeling is the one they carry |
| Planning acts before the feeling curve exists | Curve first, devices second. See feel.md §1 |
| Shipping without one bespoke signature move | Invent one. A recoloured spotlight or a retuned tilt is not one. See uniqueness.md §3 |
| A build that clears fewer than 4 of 6 fingerprint dimensions against any existing row | Change the plan, not `FINGERPRINTS.md` |
| Editing the engine to get a bespoke behaviour | Bespoke JS in the page, driven off `--sc-p` and your own `data-sc-*` |
| Reaching for filmic one-shot because it is what the last build did | Pick from all eight grammars, and say why the other seven lost |
| A full-frame dark overlay to fix contrast | A scrim only where the text sits |
| Text baked into a generated image | Real markup, always. It is selectable, translatable and sharp |
| Invented statistics in a counter | Only real numbers. No number, no counter |
| `transition: all`, or animating width/height/top/left | `transform` and `opacity`; `clip-path` for wipes |
| Gradient text, neon glow, zero-offset coloured halo shadows | Weight and size for emphasis; shadows with offset and blur |
| Autoplaying audio, or any audio at all on a scrub clip | Strip the track. `encode.sh` already does |
| Shipping without running Step 5 | Run Step 5 |

## Output

The build folder, including `BRIEF.md`, then a short report: the grammar and why
the other seven lost, the signature move, the fingerprint gate result against
each existing row, the journey, the feeling curve and the peak, the feel-check
diff (intended curve against felt curve, and what you changed), the score table
(device per beat), what you
generated, what you verified with screenshots, and anything you could not
verify. Say if the brief was self-authored rather than interviewed. Give the
local URL. Keep it brief; the page is the deliverable.

Then append the build's row to `<workspace>/FINGERPRINTS.md`.

Changes to the skill itself, and the build findings that drove them, are logged
in [CHANGELOG.md](CHANGELOG.md).
