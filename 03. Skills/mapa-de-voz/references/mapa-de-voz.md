---
name: mapa-de-voz
description: >
  Diagnóstico literario interactivo que mapea la voz editorial de un escritor
  en 5 dimensiones (Intensidad, Originalidad, Ritmo, Perspectiva, Riesgo) y
  entrega un perfil arquetípico con 3 ejercicios calibrados.
  Activa este skill cuando el usuario mencione: mapa de voz, diagnóstico de
  voz, voz literaria, voz editorial, perfil de escritor, descubrir mi voz,
  voz propia, ejercicios de escritura, qué tipo de escritor soy, o cuando
  el contexto sea R3 Editorial LUMINA / Pilar 3 Iridiscenter / identidad
  autoral. También activa cuando se pida renderizar el artefacto React
  VozMap o cuando el usuario comparta resultados del diagnóstico para
  derivar brand_tokens desde su perfil de voz.
  ACTIVA PROACTIVAMENTE si el usuario trabaja dentro del metasistema
  Iridiscenter y menciona su voz, estilo o identidad como escritora.
---

# Mapa de Voz — Skill Operativo
**Versión:** 1.0.0 · **Rama LUMINA:** R3 Editorial · **Pilar:** Pilar 3 Iridiscenter
**Modo:** execute_only · **Idioma:** es · **Brand tokens:** LUMINA canonical

---

## FASE 0 — Clasificar intención (dry-run mental)

Antes de ejecutar, identificar el modo requerido:

| Modo | Señal del usuario | Acción |
|------|-------------------|--------|
| `render` | "muéstrame el diagnóstico", "quiero hacer el test" | → Generar artefacto React (Fase 1) |
| `interpret` | Comparte scores o resultado | → Leer references/profiles.md y entregar perfil (Fase 2) |
| `brand_derive` | "¿qué tokens corresponden a mi voz?" | → Ejecutar bridge LUMINA→brand (Fase 3) |
| `embed_lumina` | Contexto de evaluación vocacional activa | → Exportar scores como delta LUMINA (Fase 4) |
| `update_skill` | "mejora el skill", "agrega una pregunta" | → Seguir flujo skill-creator (Fase 5) |

No ejecutar fases no solicitadas. Registrar modo como supuesto si no es explícito.

---

## FASE 1 — Renderizar artefacto React

Generar el componente `VozMap` como artefacto `.jsx` autocontenido.

**Fuente canónica:** `/mnt/user-data/uploads/mapa-de-voz.jsx`
**Brand tokens aplicados:** ver `references/brand-tokens.md`

**Reglas de renderizado:**
- Aplicar paleta LUMINA canonical si el usuario tiene sesión Iridiscenter activa
  (Oro RB `#CC9C50`, Rose Gold `#C48B7A`, Crema `#F5F1EC`, Carbón `#202428`)
- Si no hay sesión activa, usar paleta original del archivo (`G` object)
- Fuentes: Playfair Display (display) + Lora (body) — ya en el componente como Cormorant/Lora; mantener o migrar según contexto
- El artefacto es self-contained: sin props requeridas, sin API calls, sin localStorage
- Incluir `recharts` via import — disponible en el entorno de artifacts

**Output esperado:**
```json
{
  "artifact_type": "jsx",
  "component": "VozMap",
  "brand_mode": "lumina_canonical | original",
  "phases": ["intro", "q", "results"],
  "questions": 5,
  "profiles": 5
}
```

---

## FASE 2 — Interpretar resultados

Cuando el usuario comparte scores o describe su perfil:

1. Leer `references/profiles.md` → identificar perfil dominante por `domIdx`
2. Leer `references/scoring.md` → validar normalización
3. Entregar en este orden:
   - Nombre del perfil + ícono + color
   - Descripción del perfil (con voz `director-editorial-sistemico`: elegante, invitacional, precisa)
   - Los 3 ejercicios calibrados para ese perfil
   - Tabla de scores normalizados (formato compacto)
4. Ofrecer bridge a brand_tokens si el usuario está en contexto Iridiscenter

**Formato de entrega:**

```
◈ La Voz del Fuego Interior

[descripción del perfil]

EJERCICIOS
→ [título] — [instrucción]
→ [título] — [instrucción]
→ [título] — [instrucción]

SCORES
Intensidad   [n]%  ████████░░
Originalidad [n]%  ██████░░░░
...
```

---

## FASE 3 — Bridge LUMINA → brand_tokens

Cuando se requiere derivar brand_tokens desde el perfil de voz:

**Leer:** `references/scoring.md` sección "Bridge LUMINA"

**Mapping conceptual:**
```
DIMS[0] Intensidad   → V (vocación / propósito emocional)
DIMS[1] Originalidad → C (competencias diferenciales)
DIMS[2] Ritmo        → T (tendencia ejecutiva / consistencia)
DIMS[3] Perspectiva  → E (ética / coherencia de valores)
DIMS[4] Riesgo       → M (metacognición / autoobservación)
```

**Output brand_tokens (schema mínimo):**
```json
{
  "profile_ref": "[profile_id]",
  "dominant_dim": "[dim]",
  "palette": {
    "primary":   "[hex]",
    "secondary": "[hex]",
    "accent":    "[hex]",
    "neutral":   "[hex]",
    "rationale": "[derivación desde perfil]"
  },
  "typography": {
    "display": "Playfair Display",
    "body":    "Montserrat",
    "rationale": "[derivación desde dims dominantes]"
  },
  "voice_tokens": [
    { "token": "[string]", "source": "[dim que lo origina]" }
  ],
  "positioning": {
    "tagline": "[máx 12 palabras]",
    "archetype": "[arquetipo]",
    "differentiator": "[diferenciador]"
  },
  "do_not": ["[contraindicado por perfil]"]
}
```

Cada campo lleva `rationale`. Sin justificación = no se emite. (Regla R1 LUMINA Branding)

---

## FASE 4 — Export delta LUMINA

Cuando el diagnóstico ocurre dentro de una sesión LUMINA activa:

Emitir delta para integrar en `COMET_VAULT`:
```json
{
  "source": "mapa-de-voz",
  "version": "1.0.0",
  "subject_id": "[subject_id]",
  "session_id": "[session_id]",
  "scores_raw": {
    "Intensidad":   [n],
    "Originalidad": [n],
    "Ritmo":        [n],
    "Perspectiva":  [n],
    "Riesgo":       [n]
  },
  "scores_norm": { "...": "[n]%" },
  "profile": "[profile_id]",
  "lumina_bridge": {
    "V": [norm[0]],
    "C": [norm[1]],
    "T": [norm[2]],
    "E": [norm[3]],
    "M": [norm[4]]
  },
  "brand_tokens_derived": "[bool]",
  "timestamp": "[ISO8601]"
}
```

Este delta puede usarse directamente en el stacking engine (`applyDelta`) del neutral bundle.

---

## FASE 5 — Actualizar el skill (skill-creator mode)

Si el usuario solicita mejoras al skill:

**Proceso (Claude.ai — sin subagents):**
1. Identificar el cambio: nueva pregunta / nuevo perfil / ajuste de scores / migración de paleta
2. Dry-run: simular el cambio y reportar impacto en `references/`
3. Confirmar con usuario antes de escribir
4. Actualizar los archivos de referencia afectados
5. Ejecutar QC checklist (ver abajo)
6. Emitir `changelog_entry` con semver bump

**Regla de consistencia:**
- `QUESTIONS.length` debe igualar el denominador de normalización (`n * 5`)
- `PROFILES.length` debe igualar `DIMS.length`
- Todo nuevo perfil requiere exactamente 3 ejercicios
- Todo cambio de DIMS requiere actualizar el bridge LUMINA

---

## QC CHECKLIST (ejecutar en cualquier modo)

```
QC-01  Modo identificado antes de ejecución
QC-02  Fuente canónica leída (jsx / profiles.md / scoring.md)
QC-03  Brand tokens aplicados correctamente (lumina | original)
QC-04  Scores normalizados: sum(raw) / (n_questions × 5) × 100
QC-05  Profile asignado por max(totals) — winner-takes-all
QC-06  Exactamente 3 ejercicios entregados por perfil
QC-07  Bridge LUMINA emitido si sesión activa
QC-08  Output en formato compacto (no texto libre largo)
QC-09  Voz de output: elegante, invitacional, precisa (director-editorial)
QC-10  Changelog entry si se modificó el skill
```

---

## Referencias (lazy-load)

| Archivo | Cuándo leer |
|---------|-------------|
| `references/profiles.md` | Fase 2 · Fase 3 — perfiles completos, ejercicios, colores |
| `references/scoring.md`  | Fase 2 · Fase 4 — algoritmo, DIMS, bridge LUMINA, normalización |
| `references/brand-tokens.md` | Fase 1 · Fase 3 — paleta LUMINA, tipografía, tokens visuales |

Leer solo el archivo necesario para el modo activo.
No cargar los tres en cada invocación.
