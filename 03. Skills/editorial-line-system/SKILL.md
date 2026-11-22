---
name: editorial-line-system
description: Convert any input into a modern editorial black-and-white line-art character illustration system and produce PNG image-generation outputs by default. Use when the user asks to turn text, images, brand names, product ideas, UI concepts, or campaigns into minimalist geometric character illustrations, editorial layouts, website hero visuals, mobile UI mockups, packaging, magazine spreads, or PNG visuals in the specified monochrome line-art plus pastel accent style.
user_invocable: true
version: "1.0.0"
---

# Editorial Line System

把任何输入转换成一套现代编辑设计语言：黑白线稿人物、几何扁平造型、强排版层级、大留白、少量柔和色块，并优先通过 image generation 产出 PNG 图像，适配到杂志、包装、网站首屏、移动界面、品牌视觉板、海报或多面板视觉系统。

## Core Soul

This skill is not only a prompt template. It extracts the subject's everyday-life behavior, brand temperament, and editorial message, then rebuilds them in this fixed visual system.

The non-negotiable style DNA:
- Minimal black-and-white line art characters.
- Clean flat geometric proportions, simple faces, stylized bodies.
- Everyday urban lifestyle scenes: commuting, phone use, reading, shopping, selfies, walking, resting, listening, working, multitasking.
- Editorial design language: magazine-like typography, asymmetrical hierarchy, large negative space, strong layout blocks.
- Mostly monochrome characters; pastel accents are used selectively for backgrounds, packaging, UI surfaces, product labels, and section blocks.
- Accent palette: soft yellow, muted purple, warm orange, muted pink, cream/off-white. Never make the character itself colorful unless the user explicitly requests it.
- Flat vector-like output, no realistic lighting, no 3D, no glossy rendering, no busy gradients.

## Trigger

Use this skill when the user says or implies:
- "用这条提示词的风格"
- "转换成这种画风"
- "黑白线稿 / editorial / minimalist illustration"
- "做成品牌视觉系统 / 插图系统 / 角色 set sheet"
- "文字做成 PNG 图 / 海报 / 插图"
- "图片改成这种插画"
- gives a brand/product/topic and asks for a visual direction in this style

## Primary Output Rule

The default deliverable is a PNG image created through image generation.

Core priority:
1. If the user asks for a picture, PNG, poster, visual, illustration, campaign board, brand board, image conversion, or simply provides text to "make an image", use `image_gen` directly when available.
2. Do not use HTML-to-screenshot as the default path. HTML can imitate layout, but it usually cannot produce the true soul of this prompt: expressive editorial illustration, organic scene invention, and coherent character systems.
3. Use HTML only when the user explicitly asks for HTML, editable web layout, local browser rendering, or exact DOM/CSS output.
4. When text must appear inside the generated PNG, keep it short and high-level if possible. For long exact text, ask whether the user wants:
   - image-generated poster with possible text rendering imperfections
   - HTML/SVG text-safe rendering
   - separate image background plus exact text typeset afterward

## Input Classification

Classify the input first:

1. Text content
- User gives paragraphs, slogans, notes, article text, quotes, campaign copy, report content, or a short idea.
- Default output: PNG image through `image_gen`, using the text as the conceptual and typographic anchor.
- If the user asks for exact readable long text, explain that image generation may distort text and choose the safest route based on the user's requested fidelity.

2. Image input
- User provides an image or local image path.
- Use image generation/editing directly when available.
- Preserve the image's subject/composition while converting it into the editorial line-art system.

3. Brand name / product name
- User gives only a brand, product, app, project, person, or campaign name.
- Infer the "soul" before visualizing:
  - what urban behavior it owns
  - what type of person appears in the system
  - what daily scene proves the brand exists
  - what typography rhythm fits the brand
  - what accent colors feel native
- Output a brand visual system direction, then a ready-to-use image prompt or HTML concept if requested.

4. Mixed input
- User gives text plus brand/image/context.
- Preserve explicit content. Use the brand/context to choose scenes, layout hierarchy, and accent color.

5. Ambiguous input
- If the expected output is unclear, ask one concise question or provide 3-5 output choices:
  - PNG editorial poster
  - PNG brand visual system board
  - PNG image-to-illustration conversion
  - HTML editorial page, only if exact text/layout editing is more important than illustration quality

## Output Decision Rules

Do not over-ask when the intent is clear.

- Text without requested format -> create a PNG editorial image through `image_gen`.
- Text + "图片/海报/png/做成图" -> create the PNG directly through `image_gen`.
- Image -> execute image-to-illustration generation/editing when tools are available.
- Brand only -> infer "brand soul" and create a PNG visual system board unless the user asks only for a prompt.
- Campaign/product/app -> create a PNG multi-application system board: magazine spread + packaging/product card + website hero + mobile UI + character sheet.

## PNG Image Generation Rules

When generating PNG images:
- Use `image_gen` directly if available.
- Think like an art director first: convert the input into a visual metaphor, urban behavior, product/application context, and character system.
- Do not create a plain text card. The visible text should sit inside a larger editorial illustration system.
- Use multi-panel composition when the input contains multiple clauses, concepts, scenes, or use cases.
- For philosophical or abstract text, turn each clause into a small everyday urban scene. The person should be doing something ordinary, but the action should embody the abstract idea.
- Use strong magazine-style typography as part of the composition, but avoid depending on tiny body copy for meaning.
- If exact Chinese text is essential, keep the visible text large, sparse, and short. For long text, summarize visually and preserve exact text outside the image or use HTML only when the user requests text precision.
- Default aspect ratio should match the user's request. If not specified:
  - poster / quote / social visual: vertical PNG, 1024x1792 or 1080x1920 equivalent
  - brand system / campaign board: horizontal PNG, 1792x1024 equivalent
  - square social card: 1024x1024

## Text-to-HTML Rules

Use this section only when the user explicitly asks for HTML, browser-rendered output, or exact editable text layout.

When generating HTML:
- Make a complete standalone HTML file if the user wants a file. Otherwise return the HTML code.
- Use responsive layout. Width adapts to content; for long text use a vertical editorial page, for short slogans use a poster/card.
- Use CSS only or minimal inline SVG for black line-art people and objects.
- Keep the first screen useful. Do not make a marketing landing page unless asked.
- Typography must carry the layout: large headline, smaller supporting text, clear grid, strict spacing.
- Use no more than 2 pastel accent colors in one composition.
- Characters are small-to-medium editorial actors, not mascot stickers.
- Avoid AI-demo tropes: purple-blue gradients, floating blobs, glass cards, fake dashboards, decorative clutter.
- Preserve user text exactly when it is meant to appear on the visual. If rewriting is needed, ask first.

Recommended HTML formats:
- Short quote/slogan: 1080x1440 or 1080x1920 poster-like canvas.
- Article/long text: 900-1100px wide adaptive editorial long page.
- Brand system: horizontal multi-panel board, 1792x1024 feel.
- Product/app concept: hero + UI screens + character set section.

## Image-to-Illustration Prompt Builder

For image input, write the prompt in English unless the user requests Chinese.

Must include:
- Preserve the main subject, pose, composition, and recognizable objects from the reference image.
- Convert into minimalist black-and-white line art characters or objects.
- Clean flat geometric style, simple facial features, stylized proportions.
- Strong editorial layout with large negative space and bold typography if text is needed.
- Pastel accent blocks in yellow, muted purple, warm orange, muted pink, or cream.
- Flat vector-like finish, smooth edges, high contrast.

Negative constraints:
- No realistic rendering, no 3D, no painterly texture, no glossy lighting, no anime style, no childish cartoon mascot, no excessive color, no busy background.

## Brand Soul Builder

For a brand/product/name-only input, produce:

1. Soul interpretation
- One or two sentences naming the brand's everyday-life scene and emotional temperature.

2. Character world
- Who appears in the illustration system.
- What they are doing.
- What objects prove the brand or product.

3. Layout system
- Which applications fit best: magazine spread, website hero, app screens, packaging, poster, social card, character sheet.

4. Color accent
- Choose 1-3 pastel accent colors and explain their role briefly.

5. Ready prompt
- End with a usable image prompt or HTML direction.

## Master Image Prompt Template

Use and adapt this structure:

```text
A modern editorial illustration system for [SUBJECT], featuring minimalist black-and-white line art characters across [APPLICATIONS]. The scene translates [CORE IDEA] into everyday urban life moments: [SCENES]. Characters are clean, flat, geometric, with simple facial features, stylized proportions, and mostly monochrome bodies. The design emphasizes bold typography, large negative space, asymmetrical editorial hierarchy, and strong contrast blocks. Use soft pastel accents selectively for backgrounds, product surfaces, packaging, UI panels, and section dividers: [ACCENT_COLORS]. Include [SPECIFIC_ELEMENTS]. Straight-on multi-panel layout view, high-resolution vector-like lines, smooth edges, flat design, no realistic lighting.

Negative prompt: realistic lighting, 3D render, glossy effects, painterly texture, anime, childish mascot, excessive color, busy background, cluttered layout, heavy gradients, illegible typography.
```

## PNG Poster Prompt Template

Use when the user asks for a vertical PNG poster:

```text
Create a vertical PNG editorial poster for [SUBJECT/TEXT]. Minimalist black-and-white line art characters interact with the idea through everyday urban gestures: [ACTION]. Use a clean magazine grid, large headline typography, generous negative space, and a few pastel accent blocks in [ACCENT_COLORS]. Characters remain mostly monochrome, drawn in clean flat geometric line art with simple faces and stylized proportions. If visible text is needed, make it large, sparse, and editorial, preserving the essential wording: [TEXT]. Contemporary lifestyle branding aesthetic, high contrast, vector-like smooth edges, flat design, no realistic lighting.

Negative prompt: 3D, realistic shading, glossy gradients, stock-photo look, childish cartoon, clutter, illegible text, excessive colors.
```

## Quality Checklist

Before final output, check:
- Did the output transform the input into a visual system, not merely paste the original prompt?
- Are characters mostly black-and-white?
- Are pastel colors selective and functional?
- Is the layout editorial, with hierarchy and negative space?
- If text appears visually, is it preserved exactly?
- If the input is a brand, did you infer the daily-life scene and brand soul?
- If the target format is unclear, did you ask or offer choices?
