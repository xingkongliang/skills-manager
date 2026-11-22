# 12_MODULE_SKILLS_ROUTER_v1_2_1.md

## Status
MODULE_SKILLS_ROUTER · v1.2.1 · candidate_extension

## Placement
This file is an extension router for the frozen `PROJECT_CORE_AI_KNOWLEDGE_SET_00_11` baseline.
It does not replace 00–11. It routes additional skills as optional/conditional modules.

## Authority rule
- Baseline 00–11 remains `canonical_bundle_core_inside_declared_scope`.
- This module layer is `candidate_extension` until approved by Raquel.
- Raw `.skill` files remain source/reference unless consolidated into upload-ready modules.
- Do not upload every raw skill by default; prefer consolidated modules.

## Skill module lanes

```yaml
module_lanes:
  M0_base_orchestrator:
    files: ["00_PROJECT_CORE_AI_SELAH_OPAL_BOS_ORCHESTRATOR_v1_0_1.skill.md"]
    role: "root orchestration / Selah OPAL-BOS"
    status: "already_installed_core"

  M1_governance_ops:
    files: ["02_ARCHITECTURE_PACKS_CORE_EXTRACT_v1_0.md", "05_STYLE_OPS_CORE_EXTRACT_v1_0.md", "07_STYLE_ALTEREGO_ROUTER_LOCKED_v1_2_2.md", "08_OPAL_BOS_SKILL_BASE.skill", "09_skills_dependencies_delta.yaml"]
    role: "BOS / ÆQUITAS / OPAL / KOP operations"
    status: "already_installed_core_or_pending"

  M2_continuity_memory:
    files: ["04_METANO_IA_CORE_EXTRACT_v1_0.md", "metanoia-ia.skill", "metanoiaelementor-style.skill"]
    role: "portable continuity, handoff, no-destruction, layered concise response style"
    status: "04 already core; raw metanoia skills are lineage/reference unless explicitly merged"

  M3_editorial_production:
    upload_ready: "13_LUMINA_EDITORIAL_PRODUCTION_EXTENSION_v1_0.skill.md"
    source_skills: ["director-editorial-sistemico-style.skill", "arte-maquetacion-integral.skill", "production-director-style.skill"]
    role: "editorial direction, layout, print/export, DOCX/PDF/ZIP/Canva planning"
    status: "recommended upload as #13 after this router"

  M4_web_frontend_delivery:
    upload_ready: "14_FRONTEND_WEB_UI_EXTENSION_v1_0.skill.md"
    source_skills: ["frontend-design.skill", "metanoiaelementor-style.skill"]
    role: "frontend/UI/HTML/CSS/Elementor-style delivery"
    status: "optional upload if web/Elementor/UI work becomes frequent"

  M5_codex_automation_deep:
    source_skills: ["codex-automation-engineer-style.skill"]
    role: "deep automation style for Codex/GEMA engineering"
    status: "reference only for now; Style Ops + KOP already cover most of this"
```

## Activation priorities

```yaml
priority:
  1: "00 root orchestrator + current user instruction"
  2: "N0 governance: BOS / ÆQUITAS / user approval / evidence boundaries"
  3: "12 module router when user asks for skills/modules/rearrange"
  4: "13 editorial extension when user asks for editorial artifact, workbook, cuadernillo, maquetación, print/export, Canva, DOCX/PDF/HTML bundle"
  5: "14 frontend extension when user asks for website, landing page, HTML/CSS, dashboard, Elementor, UI polish"
  6: "raw skills only as lineage/reference, not as automatic authority"
```

## Default decision
Use only the smallest necessary module.
When the user asks for editorial compilation, activate #13.
When the user asks for web interface or Elementor-style frontend, activate #14.
When the user asks for Codex/GEMA automation, use installed Style Ops/KOP first; consult raw `codex-automation-engineer-style.skill` only as reference.

## Do not
- Do not replace the frozen 00–11 baseline.
- Do not upload all raw `.skill` files without consolidation.
- Do not let style-only skills override canon or truth boundaries.
- Do not claim full-native archival completeness.
