# Mapa de Voz — Scoring, Dimensiones y Bridge LUMINA
*Referencia canónica · Lazy-load en Fase 2, Fase 4*

---

## Dimensiones (DIMS[])

5 ejes de voz literaria. El orden es el índice de scoring.

| idx | id | label | Concepto operativo |
|-----|----|-------|-------------------|
| 0 | intensidad | Intensidad | Profundidad emocional e impacto visceral |
| 1 | originalidad | Originalidad | Mezcla de géneros, ángulos no tomados, territorio inexplorado |
| 2 | ritmo | Ritmo | Musicalidad, cadencia, conciencia sónica |
| 3 | perspectiva | Perspectiva | Profundidad interior, autoobservación íntima |
| 4 | riesgo | Riesgo | Valentía creativa, disposición al peligro estético |

---

## Algoritmo de Scoring

### Paso 1 — Acumulación por respuesta
Cada opción seleccionada lleva un vector `s[5]` con valores 1–5 por dimensión.
```
totals[i] = Σ answer.s[i]   para todas las respuestas
```

### Paso 2 — Normalización
```
n_questions = 5
max_per_dim = n_questions × 5 = 25

norm[i] = round( (totals[i] / max_per_dim) × 100 )
```

Rango efectivo: 20% (mín, 5/25) → 100% (máx, 25/25).

### Paso 3 — Asignación de perfil
```
domIdx = indexOf( max(totals) )
profile = PROFILES[domIdx]
```
Método: winner-takes-all por total raw (no normalizado).

### Paso 4 — Radar data
```
radar = DIMS.map((label, i) => ({ dim: label, v: norm[i] }))
```

---

## Validaciones

- `totals.length` debe ser 5 siempre
- Si `totals` está vacío (fase intro): `domIdx = 0` (default seguro)
- `norm[i]` nunca puede ser < 0 ni > 100
- Si hay empate en `max(totals)`: `domIdx` = primer índice empatado (comportamiento nativo `indexOf`)

---

## Preguntas canónicas y vectores de score

### Q0 · Motivación al escribir
| Opción | Intensidad | Originalidad | Ritmo | Perspectiva | Riesgo |
|--------|-----------|-------------|-------|-------------|--------|
| Contar una historia que aún no existe | 3 | 5 | 3 | 4 | 4 |
| Entender algo que me confunde o me duele | 5 | 3 | 2 | 5 | 3 |
| Provocar algo en quien lee | 4 | 4 | 4 | 3 | 5 |
| Capturar un instante antes de que desaparezca | 4 | 4 | 5 | 4 | 2 |

### Q1 · Relación con las palabras
| Opción | I | O | R | P | Ri |
|--------|---|---|---|---|----|
| Las busco con obsesión | 3 | 4 | 3 | 3 | 4 |
| Las dejo fluir y corrijo después | 4 | 5 | 5 | 4 | 4 |
| Las elijo primero por su sonido | 3 | 5 | 5 | 3 | 5 |
| Uso las mínimas posibles | 4 | 4 | 4 | 5 | 3 |

### Q2 · Género musical
| Opción | I | O | R | P | Ri |
|--------|---|---|---|---|----|
| Jazz | 3 | 5 | 4 | 4 | 5 |
| Música clásica | 3 | 3 | 3 | 3 | 2 |
| Rock | 5 | 4 | 5 | 3 | 5 |
| Electrónica experimental | 4 | 5 | 4 | 5 | 5 |

### Q3 · Cuando la voz "no es tuya"
| Opción | I | O | R | P | Ri |
|--------|---|---|---|---|----|
| Cuando sueno a un escritor que admiro | 3 | 2 | 3 | 3 | 2 |
| Cuando el texto suena frío | 2 | 2 | 2 | 3 | 1 |
| Cuando me censuro | 3 | 3 | 3 | 4 | 2 |
| Cuando cambio de tono sin darme cuenta | 3 | 3 | 2 | 3 | 3 |

### Q4 · Escritura que te hace sentir vivo/a
| Opción | I | O | R | P | Ri |
|--------|---|---|---|---|----|
| Una sola oración que golpea | 5 | 4 | 4 | 4 | 4 |
| Historias que desorientan y dan sentido | 4 | 5 | 4 | 5 | 5 |
| Textos que mezclan géneros | 4 | 5 | 5 | 5 | 5 |
| Escritura tan honesta que da vergüenza | 5 | 4 | 3 | 5 | 4 |

---

## Bridge LUMINA — Mapping conceptual

Mapping de DIMS de voz literaria a variables LUMINA_CORE v2.0 (V,C,T,E,M):

```
DIMS[0] Intensidad   → V (Vocación / Claridad del propósito emocional)
DIMS[1] Originalidad → C (Competencias diferenciales / habilidades únicas)
DIMS[2] Ritmo        → T (Tendencia ejecutiva / consistencia en la práctica)
DIMS[3] Perspectiva  → E (Ética / Coherencia de valores y mirada)
DIMS[4] Riesgo       → M (Metacognición / auto-observación del proceso creativo)
```

**Nota de uso:** Este mapping es conceptual, no paramétrico.
Los scores de Mapa de Voz NO se suman directamente al computed_index LUMINA.
Se emiten como delta de observación en el `COMET_VAULT` para enriquecer
el perfil vocacional, no para modificar la fórmula central.

**Fórmula LUMINA (no alterada):**
```
computed_index = V(0.20) + C(0.20) + T(0.20) + E(0.25) + M(0.15)
```

**Delta de integración (schema para stacking engine):**
```json
{
  "delta_source": "mapa-de-voz",
  "delta_type": "enrichment_observation",
  "fields": {
    "voice_V": [norm[0] / 100],
    "voice_C": [norm[1] / 100],
    "voice_T": [norm[2] / 100],
    "voice_E": [norm[3] / 100],
    "voice_M": [norm[4] / 100]
  },
  "profile_assigned": "[profile_id]",
  "integration_mode": "informational_only"
}
```

---

## Extensión del banco de preguntas

Para agregar una pregunta nueva:
1. Añadir objeto a `QUESTIONS[]` con `q`, `hint`, `opts[4]` (cada opt con `s[5]`)
2. Actualizar `n_questions` en el denominador de normalización
3. Verificar que los nuevos scores no crean un perfil inalcanzable
4. Emitir dry-run report antes de persistir (QC-04)
5. Bump semver PATCH
