---
name: prompt-images
description: >
  Prompting techniques for AI image generation and editing models on Replicate.
  Use when writing prompts for image models or building image generation features.
---

# Prompting image models on Replicate

Distilled from Replicate's blog posts on prompting image models (2024-2026). Techniques are model-agnostic and focus on transferable principles.

## Choose a model with the API, not from memory

This skill describes general prompting techniques. To choose a model, use the [find-models](../find-models/SKILL.md) skill and query the Replicate API. The image model landscape changes weekly. Don't assume specific models exist or are still state-of-the-art based on names you've seen before. Always search the API for current options, then read the schema before running anything.

For pricing and feature comparison, see the [compare-models](../compare-models/SKILL.md) skill.


## Writing prompts

### Use natural language, not keyword lists

Write full sentences describing what you want. Modern image models understand grammar and context far better than keyword-stuffed prompts.

Good: "A woman standing in a Tokyo alleyway at dusk, neon signs reflecting off wet pavement"
Bad: "woman, Tokyo, alleyway, dusk, neon, wet pavement"

### Be specific and unambiguous

Name exact colors, materials, lighting setups, camera equipment, and spatial relationships. Vague terms like "make it better" or "artistic" give unpredictable results.

Good: "A brutalist concrete building reflected in a perfectly still puddle after rain. A single figure with a red umbrella walks along the edge, the only color in an otherwise monochrome scene. Overcast sky, flat diffused light, tilt-shift lens effect on the edges."
Bad: "Cool building with a person near it, rainy day"

### Name subjects directly

Use descriptive phrases like "the woman with short black hair" or "the red car." Avoid pronouns, which are often too ambiguous for image models.

### Use long, detailed prompts

Most modern models accept thousands of tokens. Long descriptive prompts with clear structure outperform short ones. A prompt with 12+ specific requirements (text on objects, labeled diagrams, color-coded elements, specific materials) can work if each requirement is stated clearly. But be aware: the longer and more complex the prompt, the more likely something will be missed.

### Start simple, then iterate

Begin with basic changes. Test small edits first, then build on what works. Most editing models support iterative editing, so take advantage of that.


## Photographic language

Modern image models understand camera and photography terminology deeply. Using this vocabulary gives you precise control over the look.

### Camera and lens

- Film stocks: Kodak Portra 800, Fuji Velvia 50, Ilford HP5
- Lens characteristics: 50mm Summilux wide open, 85mm f/1.4, 24mm wide-angle
- Depth of field: shallow (subject sharp, background blurred), deep (everything in focus)
- Shooting techniques: golden hour, blue hour, long exposure, double exposure

### Lighting setups

- Rembrandt lighting: classic portrait lighting with a triangle of light on the cheek
- Soft diffused studio lighting: crisp highlights and gentle shadows
- Rim lighting / backlight: subject outlined with light from behind
- Flat diffused light: overcast, even illumination, minimal shadows
- Volumetric lighting: visible light beams, fog, haze

### Composition

- Rule of thirds, centered composition, symmetry
- Wide shot, medium shot, close-up, macro
- High angle, low angle, eye level, bird's-eye view
- Tilt-shift for miniature effects


## Text rendering

Rendering text in images is a common task. These techniques improve accuracy across models.

- Wrap desired text in double quotation marks within the prompt: "Design a poster with the title \"BLUE NOTE SESSIONS\" in bold condensed sans-serif"
- Stick to readable fonts. Highly stylized text may not work as well.
- When editing text in an existing image, use the pattern: "Change 'old text' to 'new text'"
- Match text length when possible: big shifts in character count can change layout
- Be explicit about preserving font style if it matters
- For complex typography (posters, editorial layouts), look for models that treat text as part of the composition rather than stamping it on top
- Some models can inpaint text: mask the text region, prompt with new text, and it matches the original font and style


## Style transfer

- Name the exact style: "impressionist painting," "1960s pop art," "Sumi-e ink wash"
- Reference specific artists or movements for clearer guidance
- If a style label doesn't work, describe its key traits: "visible brushstrokes, thick paint texture, rich color depth"
- State what should stay the same: "keep the original composition"
- When a style is hard to describe in words, some models support example-based editing: provide a before/after pair, then a third image. The model infers the transformation and applies it.
- Some models accept style reference images: upload visuals capturing the color palette, texture, composition, and mood you want


## Character consistency

Maintaining the same character across multiple generations is one of the hardest challenges in image generation.

- Start with a clear reference description: "the woman with short black hair and green eyes wearing a navy blazer"
- Say what's changing (setting, activity, style) and what should stay the same (face, expression, clothing)
- Use reference images when the model supports them. Some models handle multiple reference images simultaneously for stronger consistency.
- Break complex character changes into steps: change outfit first, then change scene
- Generate synthetic training data: create many images of a character, pick the best ones, and use them for fine-tuning or as references


## Image editing

### General principles

- Specify what to keep: explicitly state what should remain unchanged. Use phrases like "keeping the pose and expression unchanged" or "maintain the original composition."
- Choose verbs carefully: "transform" suggests a full rework. Use specific actions like "change the clothes to a blue jacket" or "replace the background with a beach."
- Be precise about scope: "Change the background to a beach while keeping the person in the exact same position, maintain identical subject placement, camera angle, framing, and perspective. Only replace the environment around them."

### Object removal

- Describe what should fill the space left behind, not just what to remove
- Some editing models handle removal cleanly; others leave structural artifacts. If one model struggles, try another.

### Background editing

- Describe the new background in detail: lighting, time of day, environment
- Specify that the subject should remain in the exact same position with the same lighting

### Perspective and angle changes

- These are among the hardest edits. Not all models handle them well.
- Some models restrict themselves to the initial composition and struggle with new angles

### Inpainting and outpainting

- For inpainting: mask the region to edit, then prompt with what should fill it
- Some models have a "magic prompt" or auto-rewrite feature. When this is on, you can focus on describing just the edited region. When it's off, describe the whole scene.
- Describing only the masked region makes the model emphasize the prompt more, which can produce better results for targeted edits
- ControlNet-style conditioning (edge detection, depth maps) helps preserve structure during generation


## Multi-image and storyboard generation

Some models can generate multiple related images in a single prompt.

- Ask for "a series," "a set," or specify a grid layout (e.g., "2x2 storyboard grid")
- Describe each panel individually with consistent character descriptions
- Maintain consistent style and character continuity by repeating exact descriptions
- Some models support example-based editing: show a before/after pair for one image, then apply the same transformation to others


## Product photography and commercial work

- Specify materials precisely: "brushed steel," "matte aluminum," "kraft paper," "frosted glass"
- Describe lighting setup: "soft diffused studio lighting, crisp highlights and gentle shadows"
- For brand assets and icons, look for models that produce native SVG output (real editable vector files)
- For layouts with branding and copy placement, look for models with strong typography and design composition


## Fine-tuning and LoRAs

- Use trigger words from your trained model in every prompt
- When combining multiple LoRAs, balance their influence with scale parameters (typically 0.9-1.1)
- Generate synthetic training data: generate many images, pick the best, retrain
- Use consistent-character workflows to generate training data from a single reference image


## Common pitfalls

1. **Keyword-stuffed prompts**: Modern models respond better to natural language sentences than comma-separated keyword lists. Write like you're describing a scene, not tagging a photo.

2. **Using "transform" when you want a small edit**: "Transform the person into a Viking" may swap the entire identity. Use targeted language: "change her outfit to Viking armor, keeping her face and expression unchanged."

3. **Not specifying what to keep**: When editing, always say what should stay the same. Without explicit instructions, models may change anything.

4. **Negative prompts on models not trained for them**: Some models were not trained with negative prompts. Using them on these models introduces noise rather than removing unwanted elements. Check the model's documentation.

5. **Too-high guidance scale (CFG)**: If images look "burnt" with excessive contrast, lower the guidance scale. Each model has a recommended range.

6. **Expecting real-time knowledge**: No image model has internet access. Some have strong world knowledge baked in from training data, but it's not live.

7. **Short prompts for complex scenes**: Modern models accept thousands of tokens. For complex compositions with many specific requirements, use that capacity.

8. **Ignoring aspect ratio**: Most models have specific resolutions they work best at (commonly ~1 megapixel). Going too large produces edge artifacts. Going too small produces harsh crops. Use the model's recommended aspect ratios.

9. **Wrong model for the task**: Not every model is good at every task. Some excel at text rendering but struggle with object removal. Some are great at style transfer but poor at background editing. If a model struggles with a specific edit type, try a different one rather than fighting the prompt. See the [compare-models](../compare-models/SKILL.md) skill for guidance.

10. **Not iterating**: The best results come from iterative workflows. Make a small change, evaluate, refine, repeat. Don't try to get everything right in a single generation.


## Background reading

The techniques above are distilled from Replicate's blog posts on prompting image models. The posts are anchored to specific models that were current at the time, but the techniques generalize across modern image models. Use these for additional context, then use the [find-models](../find-models/SKILL.md) skill to pick a model that fits your task today.

- [How to prompt Seedream 5.0](https://replicate.com/blog/how-to-prompt-seedream-5) (Feb 2026)
- [Recraft V4](https://replicate.com/blog/recraft-v4) (Feb 2026)
- [Run FLUX.2 on Replicate](https://replicate.com/blog/run-flux-2-on-replicate) (Nov 2025)
- [How to prompt Nano Banana Pro](https://replicate.com/blog/how-to-prompt-nano-banana-pro) (Nov 2025)
- [Which image editing model should I use?](https://replicate.com/blog/compare-image-editing-models) (Sep 2025)
- [Generate consistent characters](https://replicate.com/blog/generate-consistent-characters) (Jul 2025)
- [Use FLUX.1 Kontext to edit images with words](https://replicate.com/blog/flux-kontext) (May 2025)
- [Imagen 4](https://replicate.com/blog/google-imagen-4) (May 2025)
- [Ideogram 3.0 on Replicate](https://replicate.com/blog/ideogram-v3) (May 2025)
- [FLUX.1 Tools](https://replicate.com/blog/flux-tools) (Nov 2024)
- [Ideogram v2 inpainting](https://replicate.com/blog/ideogram-v2-inpainting) (Oct 2024)
- [Using synthetic data to improve Flux finetunes](https://replicate.com/blog/using-synthetic-data-to-improve-flux-finetunes) (Sep 2024)
- [FLUX.1: First Impressions](https://replicate.com/blog/flux-first-impressions) (Aug 2024)
- [How to get the best results from Stable Diffusion 3](https://replicate.com/blog/get-the-best-from-stable-diffusion-3) (Jun 2024)
