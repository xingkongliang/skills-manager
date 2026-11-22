# README · Module Skills Rearrange v1.2.1

This package rearranges newly supplied `.skill` files into a controlled module layer for Project Core AI.

## Decision
Do **not** replace the frozen 00–11 baseline. Add a modular extension layer.

## Upload recommendation
Primary upload:
1. `12_MODULE_SKILLS_ROUTER_v1_2_1.md`
2. `13_LUMINA_EDITORIAL_PRODUCTION_EXTENSION_v1_0.skill.md`

Optional upload only if web/Elementor/UI work becomes frequent:
3. `14_FRONTEND_WEB_UI_EXTENSION_v1_0.skill.md`

## Keep as reference only
- `metanoia-ia.skill` because `04_METANO_IA_CORE_EXTRACT_v1_0.md` already covers the active METANO_IA layer.
- `metanoiaelementor-style.skill` because it is a concise style, merged into #14.
- `codex-automation-engineer-style.skill` because Style Ops/KOP/OPAL-BOS already cover the automation core; consult as deep reference only.
- raw source `.skill` files are preserved in `REFERENCE_ONLY/RAW_SKILLS/`.

## Why
This avoids flooding GPT Builder knowledge with overlapping skills while preserving useful editorial, production, frontend and automation capabilities.
