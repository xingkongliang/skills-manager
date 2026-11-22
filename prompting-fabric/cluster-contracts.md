# Contratos del Clúster de Skills — `prompting-fabric` v1.0
# Fuente: portable-prompt-factory-v2.2-NEUTRAL-UNIFIED · BUNDLE_RAG_v3_0_0_RC2
# Leer desde SKILL.md §6 · Solo bajo demanda

---

## Clúster Nuclear BOS · LUMINA · OPAL · ÆQUITAS (SK-01..SK-08)

### SK-01 · Voz Autoral LUMINA
**Regla operativa:** Registro contemplativo, riguroso, invitacional y somáticamente concreto.
**Prohibiciones:** Lenguaje prescriptivo ("debes", "tienes que"), urgencia comercial, clichés motivacionales.
**QA:** `AQ1-Fidelity`
**Tokens canónicos LUMINA:** `#CC9C50` Oro · `#2C1810` Tinta · `#F5F0EC` Papel · `#7B9EA6` Clínica · `#1E7D63` Espiritual · Tipografía: `Playfair Display` + `Montserrat`

### SK-02 · Derivación Estética del Objeto
**Regla operativa:** Extraer paleta, atmósfera y narrativa visual DIRECTAMENTE del Objeto Central en 5 pasos:
1. Identificar el Objeto Central.
2. Listar sus propiedades visuales/sensoriales.
3. Mapear a paleta de colores.
4. Derivar atmósfera y tono.
5. Construir narrativa visual coherente.
**Prohibición:** Elegir colores arbitrarios no derivados del Objeto.
**QA:** `AQ3-Necessity`

### SK-03 · Motor de Prompts IA Visual
**Plantilla:** `{{estilo}} composition, {{composición}}, {{iluminación}}, {{paleta}} color palette derived from {{objeto}}, {{atmósfera}} tone`
**Regla crítica:** Describir la experiencia visual, no solo el objeto.
**Compatibilidad:** Midjourney, DALL-E 3, Flux, Stable Diffusion, Firefly.

### SK-04 · Auditoría ÆQUITAS
**Criterios de evaluación:**
- `AQ1 · Fidelity` — Output fiel al Objeto Central e intención declarada
- `AQ2 · Coherence` — Consistencia interna entre secciones
- `AQ3 · Necessity` — Cada elemento justificado; sin ruido
- `AQ4 · Precision` — Términos técnicos exactos; sin ambigüedad
- `AQ5 · Elegance` — Solución más simple que resuelve el problema

**Salida:** `PASS | REVIEW | FAIL` por criterio + justificación de 1 línea.
**QA:** `QC-07`
**Cuándo:** Siempre en Fase 3; opcionalmente en Fase 2 para borradores complejos.

### SK-05 · Dry-Run BOS
**Simular ANTES de persistir cualquier artefacto o cambio de rama:**
```json
{
  "meta": {"ts": "<ISO-8601>", "mode": "dry-run", "v": "2.2"},
  "summary": {"create": 0, "update": 0, "delete": 0, "conflicts": 0},
  "conflicts": [],
  "actions": [],
  "artifact_paths": []
}
```
**QA:** `QC-01`

### SK-06 · Batch + Delta
**Reglas:** Máximo 5 ítems por llamada. Emitir únicamente diffs. Upserts idempotentes garantizados.
**Formato delta:** Solo mostrar las líneas que cambian, con marcador `+ / -`.
**QA:** `QC-09`

### SK-07 · Orquestación Transmedia
**Hasta 7 salidas coordinadas por pipeline:**
1. `texto_principal` — Contenido editorial N2
2. `visual_prompt` — Prompt IA para imagen (→ SK-03)
3. `direccion_artistica` — Brief para diseñador
4. `caption_social` — Copy para redes
5. `script_audio` — Guión para podcast/voz
6. `json_spec` — Especificación técnica BOS
7. `pdf_imprimible` — Layout para imprenta (→ S8)
**QA:** `QC-05`

### SK-08 · Análisis Simbólico OPAL
**3 niveles:** `signos` → `símbolos/arquetipos` → `sistemas simbólicos`
**5 arcos:** `Origen → Transformación → Conflicto → Reconciliación → Trascendencia`
**Output:** Matriz cruzada niveles × arcos.
**QA:** `QC-06`

---

## Clúster de Ejecución Especializada (S1..S11)

### S1 · algorithmic-art (`p5.js`)
**Flujo 2 fases:**
1. Crear *Filosofía Algorítmica* `.md` (4–6 párrafos: maestría computacional, emergencia, intención artística).
2. Programar artefacto HTML único autocontenido con `p5.js` CDN inline. Estructura de controles fija:
   - `Seed` → `randomSeed()` / `noiseSeed()` + botones prev/next/random/jump
   - `Parameters` → sliders en vivo con etiquetas descriptivas
   - `Colors` → paleta opcional intercambiable
   - `Actions` → Regenerate / Reset / Download PNG
**Prohibición:** NO importar assets externos. TODO inline. NO usar `loadImage()` con URLs externas.

### S2 · canvas-design (`PDF/PNG estático`)
**Flujo 2 fases:**
1. Crear *Filosofía Visual* `.md` (4–6 párrafos: espacio, forma, ritmo, ejecución maestra, referencia cultural).
2. Expresar en lienzo: 90% diseño visual de calidad museo, 10% tipografía clínica/esencial.
**Regla de oro:** Segunda pasada obligatoria de refinamiento minimalista. Sin solapamientos ni desbordes.
**Regla de proporción:** El texto NUNCA domina sobre el diseño visual.

### S3 · doc-coauthoring (`PRD, RFC, Spec técnica`)
**Flujo 3 etapas:**
1. *Context Gathering* → meta-contexto del proyecto + volcado libre + 5–10 preguntas aclaratorias.
2. *Refinement & Structure* → sección a sección: 5–20 opciones → curaduría (keep/remove/combine) → redacción y edición quirúrgica.
3. *Reader Testing* → verificar con instancia fresca sin contexto. Eliminar puntos ciegos.
**Output:** Documento estructurado versionado con acceptance criteria.

### S4 · brand-identity-studio v1/v2 (`Branding, Manual de Marca`)
**Flujo en Español Latino Mexicano:**
1. `CUESTIONARIO` — 5 bloques: Identidad, Valores, Audiencia, Competencia, Visión + tabla resumen de confirmación.
2. `BRAND TOKENS` — HEX + RGB + CMYK + par tipográfico (display + cuerpo) + valores de la marca.
3. `BUNDLE DE ENTREGABLES` — 10 piezas estándar:
   - Manual de marca · Brand Kit · Paleta oficial · Hoja membretada · Tarjeta de presentación
   - Firma de email · Social media kit · Brand guidelines · Cuaderno de trabajo · One-pager
   - + Opcionales: PPTX institucional, Plan PMBOK, Diagrama Swimlane
**Regla:** Respetar activos previos del usuario. Confirmar antes de sobrescribir.
**Persistencia:** Tokens bajo `rectoriales_Rama7` cuando aplique.

### S5 · office-suite (`PPTX, DOCX, XLSX`)
**PPTX via `pptxgenjs`:**
- Fijar `pres.layout` al inicio
- Colores hex de 6 dígitos SIN `#` (ej. `"CC9C50"` no `"#CC9C50"`)
- NO reusar objetos de opciones entre slides
- `offset` de sombra ≥ 0 siempre
- CERO líneas decorativas bajo títulos
- Validar con `validate.py` + inspección visual `pdftoppm`

**DOCX via `docx-js`:**
- Anchos duales DXA en tablas
- `ShadingType.CLEAR` para celdas sin color
- `PageBreak` dentro de `Paragraph`, no como elemento independiente
- Redlining: `<w:ins>` / `<w:del>` para cambios tracked

**XLSX via `openpyxl`:**
- Fórmulas SIEMPRE vivas (NUNCA valores hardcodeados)
- Funciones compatibles: `INDEX/MATCH`, prefijo `_xlfn.` donde aplique
- PROHIBIDO `XLOOKUP` (incompatibilidad con versiones legacy)
- Recalcular con `recalc.py` — cero errores antes de entregar

### S6 · frontend-design (`Web UI, Dashboard, React, HTML`)
**Dirección estética intencional:** Elegir un tono extremo (brutalista, editorial, retro-futurista, lujo refinado, orgánico, minimal suizo).
**Prohibición AI-slop:** Sin Inter/Roboto/Arial genéricas · sin gradientes púrpura cliché · sin cards apiladas idénticas.
**Reglas técnicas:** Variables CSS semánticas · jerarquía tipográfica marcada · composición asimétrica o grilla de precisión · micro-interacciones CSS · entrega en UN SOLO archivo autocontenido.

### S7 · mcp-builder (`MCP TypeScript/Python`)
**Flujo 4 fases:**
1. Planeación → balance API coverage vs. workflow tools; prefijos consistentes; no duplicar funciones.
2. Implementación → TypeScript (`Zod`) o Python (`FastMCP`/`Pydantic`); `outputSchema` explícito; paginación; errores accionables con contexto; anotaciones: `readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`.
3. Build + test con `MCP Inspector`.
4. Evaluación → Suite XML con 10 preguntas *read-only*, complejas, independientes y verificables.

### S8 · arte-maquetacion-integral (`Imprenta, SVG, Canva`)
**Producción gráfica:**
- Vitraux vectoriales: `DiaXX_Vitral.svg` con numeración `0–J` como texto editable (NUNCA paths); líneas `0.25–1 pt`; exportar `.svg`, `.eps`, `.png` a `300 dpi`.
- Maquetación InDesign: `MaestroImprenta.indd` / `.idml`, sangrado `3 mm`, CMYK verificado, `PDF_Impresion_X1a.pdf` con fuentes 100% incrustadas.
- Plantillas Canva con guías de uso.
**Reglas QA:** NO sobrescribir sin `--force`. Generar `QA_Report.csv` (ISO 8601) + `run_log.txt` + `PackageManifest.txt`.

### S9 · eic-notes-protocol-v1.4 (`Editor-in-Chief Notes`)
**Motor EIC:** Validación contra `editor_in_chief_notes.schema.json`.
**Maquetación:** LaTeX/PDF via `preamble.tex` + `generate_eic_notes.py` → `EIC_Notes_LUMINA_Print.pdf`.
**Auditoría:** `audit_tests.py` con cobertura total de schema.

### S10 · mapa-de-voz v1.1/1.2 VOIEON (`Diagnóstico Literario`)
**5 dimensiones:** `Intensidad (0)` · `Originalidad (1)` · `Ritmo (2)` · `Perspectiva (3)` · `Riesgo (4)`
**Cálculo:** `totals[i] = sum(answer.s[i])` → `norm[i] = Math.round((totals[i] / (n_questions × 5)) × 100)` → perfil por máximo bruto (empate al primer índice).
**Perfiles:** Fuego Interior (`◈`) · Voz del Borde (`✦`) · Voz del Pulso (`〜`) · Testigo Íntimo (`◎`) · Voz del Saltador (`↗`) — 3 ejercicios exactos por perfil.
**Bridge LUMINA:** `Intensidad→V · Originalidad→C · Ritmo→T · Perspectiva→E · Riesgo→M`
**Persistencia:** `localStorage` con namespace `mv_` + exportación JSON.

### S11 · style-ops-cluster (`Modulación de Registro Operativo`)
**4 modos:**
1. `director-editorial-sistémico` → contemplativo, preciso, editorial no terapéutico
2. `production-director` → orientado a compuertas (B01/B02), manifiestos y decisiones ejecutivas
3. `documentacion-tecnica` → especificaciones ejecutables, SOPs, trazabilidad
4. `codex-automation-engineer` → scripts deterministas, validación de schemas, cero efectos secundarios silenciosos
