# LUMINA: Scaffold de Series Editorial Transmedia
## Cómo documentar, expandir y reutilizar como sistema vivo
**Registro:** Director Editorial Sistémico · **Tono:** Contemplativo, operativo · **Alcance:** R3 LUMINA y derivadas

---

## El scaffold, visto desde dentro

LUMINA no es un contenedor finito de 20 entries y un HTML. Es una **arquitectura de capas que responde** — cada capa responde a la anterior sin replicarla, sin ruido, sin manufactura visible. Lo ves bien cuando observas cómo el Cuadernillo Literatura (12 entries presentes, 8 ausentes) no es una "página incompleta," sino una **invitación a la continuidad.**

Las 8 ausentes (Entries 9–16, el ciclo binario S2 de exploración) no son un error editorial que debas ocultar o justificar con prosa. Son el espacio donde vive la próxima voz, la próxima inteligencia, el próximo lector. La arquitectura de LUMINA construye eso deliberadamente — deja respiradero.

---

## Estructura operativa del scaffold

```
LUMINA (Padre arquitectónico)
├── BOS (gobierno + origen)
├── ÆQUITAS (auditoría + coherencia)
├── LUMINA (organización + branding)
├── OPAL (decantación + dominio)
└── Ramas (R1–R7: siete líneas de producción paralelas)
    ├── R3 LUMINA Editorial (Literatura, Teología, Poesía)
    ├── WB_COMET-STRIKER (runtime interactivo)
    ├── Mapa de Voz (diagnostics + brand generation)
    └── ...expandibles
```

Cada rama **hereda el mismo contrato ÆQUITAS** (5 criterios: fidelidad, coherencia, necesidad, precisión, elegancia) pero elige su propia paleta, estructura de entries, ritmo, pedagogía.

Lo que ves en RC2 es la **primera rama materializada completa:** Literatura (dominio OPAL), con Cascada de Gobernanza funcionando, Comet-STRIKER como interfaz, Mapa de Voz como puerta de entrada.

---

## Cómo documentar una nueva serie (serie = rama expandida)

Si quisieras crear una **Rama 4: Teología Editorial** o **Rama 5: Cuadernillo Poesía,** el scaffold dice: no hagas "lo mismo pero distinto." Haz esto:

### 1. Define tu **objeto central** (no el contenido, el objeto)

**Literatura:**
- Objeto: exploración sonora de la verdad encarnada, vía lectura + reflexión
- Formato: 20 entries, estructura S1-S4 (apertura, exploración, síntesis, cierre)
- Voz: contemplativa, ritmo pausado, invitacional

**Teología (ejemplo):**
- Objeto: exploración conceptual de la fe encarnada, vía pregunta + tradición + experiencia
- Formato: 20 entries (heredar S1-S4, adaptar contenido)
- Voz: investigativa, ritmo binario (pregunta/respuesta), humilde pero firme

El objeto central **determina todo lo demás** — paleta, tipografía, interactividad, ritmo de publicación.

### 2. Deriva tu **paleta LUMINA local** desde ese objeto

**Literatura → Oro antiguo #CC9C50** (luz pergamino, tiempo, durabilidad)
**Teología → Púrpura contemplativo #6B3FA0** (misterio, tradición, profundidad)

**Regla derivada:** no elijas colores por moda. Pregunta al objeto qué necesita "vernos" y elige desde ahí. Mantén 2–3 tonos, máximo.

### 3. Construye tu **matriz de entries** (no escribas entries, estructura primero)

```markdown
## ESTRUCTURA: Teología Editorial v1.0

S1 Apertura (1–4):    pregunta fundacional · anclaje en tradición · umbral
S2 Exploración (5–16): ciclo trinario — tradición (5,8,11,14) + pregunta (6,9,12,15) + experiencia (7,10,13,16)
S3 Síntesis (17–18):   convergencias · mapeo de verdad
S4 Cierre (19–20):     compromiso · siguiente pregunta
```

Luego, valida esta estructura contra tu scorecard ÆQUITAS:
- ¿Cada entry respeta Fidelidad (voz contemplativa sin imperativos)?
- ¿Coherencia (usa tu paleta + tipografía declarada)?
- ¿Necesidad (ningún párrafo es ornamental)?
- ¿Precisión (cada pregunta abre un camino concreto)?
- ¿Elegancia (80% espacio negativo, ritmo respirado)?

### 4. **Documentación transmedia mínima** (qué escribir, no cuánto)

Por **cada rama nueva**, mantén 3 documentos solamente:

**a) `RAMA_X_ARCHITECTURE.md`** (500–800 palabras)
```
# Rama 5: Cuadernillo Poesía

## Objeto central
[2–3 párrafos describiendo qué es, no qué hace]

## Paleta canónica
[tokens de color derivados del objeto + tipografía + espaciado]

## Estructura de entries
[matriz S1-S4 con descripción de ciclo interno]

## Scorecard ÆQUITAS aplicado
[los 5 criterios, personalizados para poesía]

## Casos de uso
[3 personas tipo: quién entra, qué buscan, qué salen siendo]
```

**b) `RAMA_X_ENTRIES_MANIFEST.json`** (array puro, sin narración)
```json
{
  "rama_id": "R5_POESIA",
  "branch_name": "Cuadernillo Poesía",
  "entry_count": 20,
  "entries": [
    {"id": 1, "title": "Umbral", "section": "S1", "words": "~600", "status": "DRAFT"},
    {"id": 2, "title": "Primera Escucha", "section": "S1", "words": "~600", "status": "DRAFT"},
    ...
  ]
}
```

**c) `RAMA_X_QA_REPORT.json`** (scorecard ejecución)
```json
{
  "rama": "R5_POESIA",
  "aequitas": {
    "fidelity": {"result": "PASS", "note": "..."},
    "coherence": {"result": "REVIEW", "note": "..."},
    "necessity": {"result": "PASS", "note": "..."},
    "precision": {"result": "PASS", "note": "..."},
    "elegance": {"result": "PASS", "note": "..."}
  },
  "overall": "REVIEW"
}
```

---

## Cómo usar RC2 como **referencia ejecutable**

RC2 es tu **versión canónica de "esto funciona así."** No la copies. Úsala como:

### Patrón de governance
RC2 tiene 10 conflictos registrados, semánticamente refinados (B01, B02, S2 gap). Esto te dice: **documentar conflictos es normal.** No los escondas. Léelos en `CONFLICT_REGISTER.json`, aprende la semántica (qué es blocker, qué es gap, qué es pending source).

### Patrón de auditoría
RC2 vino con un `AEQUITAS_REPORT.json` que declara REVIEW (no PASS, no BLOCKED). Esto es honestidad de producción. Tu Rama 5 también debería poder decir REVIEW sin vergüenza — significa "bueno, incompleto, sabemos qué falta."

### Patrón de manifest
`PackageManifest.json` + `SHA256SUMS.txt` en RC2 son **reproduciblidad garantizada.** Si replicas ese patrón en Rama 5, alguien en 2027 puede verificar exactamente qué se entregó, intacto.

### Patrón de contenido
Las 12 entries de Literatura en RC2 son **S1 apertura + S4 cierre**: precisamente el inicio y el cierre narrativo. Esto te dice: **puedes entregar incompleto si lo que entregas es *íntegro en su sección.*** No es un fracaso. Es provisión honesta.

---

## Serie CometStriker & derivadas

¿"CometStriker" es una serie? **Técnicamente, es un runtime** — el HTML workbook que ejecuta cualquier rama LUMINA. Pero vive como **serie de configuraciones:**

```
WB_COMET-STRIKER_CREATIVE-JOURNEY.html (v1.2.0, Literatura)
WB_COMET-STRIKER_THEOLOGY_INQUIRY.html (v1.0, Teología — heredada, config distinta)
WB_COMET-STRIKER_POETRY_SOUNDSCAPE.html (v1.0, Poesía — heredada, config distinta)
```

Cada una: **mismo código base, distinta paleta + estructura de entry + prompts de export.**

Documentar esto en serie significa:
1. Un `COMET_RUNTIME_SPEC.md` central (el motor)
2. Un `COMET_CONFIG_TEMPLATE.json` (dónde cambian los colores y ritmos)
3. Un `COMET_DEPLOYMENT_CHECKLIST.md` (cómo pasar de DRAFT a LIVE)

---

## Expansión gradual: Del Mapa de Voz a la serie completa

```
Fase 1 (Actual):  Mapa de Voz v1.2.0 + RC2 Literatura (12/20 entries)
Fase 2:           RC2 closure (S2 gap, Comet QA) → v3.0.0 CANON
Fase 3:           Rama 4 Teología (20/20 entries) + Comet config
Fase 4:           Rama 5 Poesía (20/20 entries) + Mapa de Voz x2 (para teología + poesía)
Fase 5:           MCP server LUMINA (multi-rama, multi-user)
```

Cada fase entrega **manifesto + scaffold + audit** — nunca contenido desnudo.

---

## Checklist de documentación mínima para cualquier rama nueva

```
☐ RAMA_X_ARCHITECTURE.md (objeto + paleta + estructura + casos uso)
☐ RAMA_X_ENTRIES_MANIFEST.json (índice puro, status por entry)
☐ RAMA_X_QA_REPORT.json (ÆQUITAS scorecard)
☐ RAMA_X_COMET_CONFIG.json (si usa runtime)
☐ RAMA_X_CHANGELOG.md (versión, qué nuevo, qué pendiente)
☐ RAMA_X_HANDOFF.md (al siguiente editor: estado, decisiones, siguientes pasos)
☐ SHA256SUMS.txt (integridad verificable)
```

**No más.** Todo lo demás es referencia cruzada hacia RC2 o hacia las notas de gobernanza (Cascada, DRAME, ÆQUITAS).

---

## Lo que hace LUMINA verdaderamente reutilizable

No es la tecnología (HTML, DOCX, PDF son triviales).
No es la estructura (20 entries, S1-S4 son patrones, no receta).

Es el **contrato editorial honesto:** 
- Declara objeto central antes de contenido
- Audita contra criterios universales (ÆQUITAS 5)
- Documenta lo que falta sin vergüenza (gaps, bloqueadores, semántica)
- Entrega manifesto + scaffold + archivos verificables
- Hereda pero no copia

Eso es lo que permite que Rama 3 (Literatura) viva con Rama 5 (Poesía) en la misma arquitectura, bajo el mismo gobierno, con voces completamente distintas.
