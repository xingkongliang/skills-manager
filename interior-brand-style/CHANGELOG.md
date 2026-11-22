# CHANGELOG — interior-brand-style

## 1.0.0-CANDIDATE — 2026-10-13

### Initial candidate build

**Architect:** CLD_04  
**Authority:** Human D / Aleja  
**Status:** DESIGN-SOUND — peer audit pending

#### Added
- SKILL.md: skill completo con 5 fases, 7 reglas de gobernanza, variables de runtime
- SKILL.md: tres modos de operación (INTERIOR_ONLY, BRAND_ONLY, INTEGRATED)
- SKILL.md: Brand-to-Space translation layer para modo INTEGRATED
- SKILL.md: taxonomía de espacios por sector (8 sectores)
- SKILL.md: integración con ramas R01-R07 y 4 pilares del metasistema
- SKILL.md: activadores de skill y condiciones de no-activación
- SKILL.md: peer-audit receipt template
- evals.json: 10 casos deterministas cubriendo gobernanza, neutralidad, routing, conflictos, epistémica
- grading.json: 6 dimensiones ponderadas con regla de promoción
- manifest.json: metadatos completos de paquete, linaje, cobertura
- CHANGELOG.md: este archivo

#### Design decisions
- Neutral por defecto: sin hardcoding de clientes, marcas ni paletas específicas
- Variables inyectables en runtime para adaptación a cualquier proyecto
- R05 (Vocativo) marcado como HOLD — no activable sin levantamiento explícito de D
- Doble eje (canon Iridiscenter vs CODEX R6) preservado como tensión — D decide
- Dry-run obligatorio antes de cualquier entregable en Fase 3
- Canon y deployment bloqueados hasta Gate de D

#### Known gaps (deferred)
- Baseline run no ejecutado (requiere evaluador independiente)
- Peer audit pendiente
- generate_review.py no construido
- Aplicación a rama R05 bloqueada hasta DEC correspondiente
- DEC-013 (R04 / Dra. Claudia) — activación de consultorio_salud pendiente de D

#### Not changed
- No modificación a ningún archivo de AEQ-MEM, BOS, OPAL, o corpus canon
- No deployment, instalación, ni runtime
