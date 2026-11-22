# Modelo de capas y registros

## Arquitectura neutral

Separar responsabilidades para evitar un scanner monolítico:

| Capa | Responsabilidad | No debe hacer |
|---|---|---|
| Núcleo DIVIDE | Investigar, normalizar, clasificar y cruzar DI–VI–DE | Contener reglas rígidas de un solo proyecto |
| Adaptador | Traducir vocabulario, objetos, umbrales y destinos | Alterar la semántica de evidencia |
| Fuentes y RAG | Preservar evidencia, documentos y localizadores | Aprobar afirmaciones por mera presencia |
| Automatización | Importar, deduplicar, validar, registrar y exportar | Investigar indiscriminadamente o decidir ventas |
| Superficie | Facilitar revisión y decisión humana | Ocultar estados o incertidumbre |
| Gobernanza | Definir gates, autoridad, vigencia y rollback | Convertir score en autorización automática |

## Registros canónicos

### `DI_SIGNAL`

| Campo | Tipo | Regla |
|---|---|---|
| `signal_id` | string | Único y estable |
| `project_id` | string | Adaptador que lo interpreta |
| `audience` | string/null | No inventar segmento |
| `occasion` | string/null | Ocasión observada o declarada |
| `problem_language` | string | Lenguaje de la fuente, resumido |
| `intent` | string/null | Informativa, comparativa, transaccional u otra |
| `signal_type` | string | Búsqueda, venta, reseña, consulta, tendencia, entrevista, etc. |
| `value` | number/null | `null` si no existe medida |
| `unit` | string/null | Obligatoria cuando existe `value` |
| `region` | string | Geografía de validez |
| `observed_at` | date/datetime | Fecha de la señal |
| `evidence_refs` | string[] | Referencias existentes |
| `claim_type` | enum | Ver SKILL.md |
| `verification_state` | enum | Ver SKILL.md |
| `valid_until` | date/null | Vigencia si aplica |

### `VI_VENDOR`

| Campo | Tipo | Regla |
|---|---|---|
| `vendor_id` | string | Único y estable |
| `project_id` | string | Adaptador que lo interpreta |
| `vendor_name` | string | Nombre observado |
| `vendor_location` | string/null | No inferir por dominio web |
| `offer_name` | string | Producto o servicio |
| `category` | string/null | Vocabulario del adaptador |
| `unit` | string/null | Presentación comparable |
| `moq` | number/null | Desconocido = `null` |
| `price` | number/null | No mezclar total y unitario |
| `currency` | string/null | ISO 4217 cuando exista precio |
| `shipping` | number/null | Separar del precio |
| `sample_available` | boolean/null | Desconocido = `null` |
| `lead_time_days` | number/null | Rango puede documentarse en notas |
| `compatibility` | string/null | Resultado y criterio de compatibilidad |
| `compliance` | string/null | Registro, certificado o pendiente |
| `risks` | string[] | Riesgos observados o inferidos etiquetados |
| `evidence_refs` | string[] | Referencias existentes |
| `claim_type` | enum | Normalmente `OBSERVED` o `PLACEHOLDER` |
| `verification_state` | enum | Estado real |
| `valid_until` | date/null | Vigencia de oferta |

### `EVIDENCE_RECORD`

| Campo | Tipo | Regla |
|---|---|---|
| `evidence_id` | string | Único |
| `source_type` | string | Web, PDF, catálogo, correo, entrevista, hoja, API, etc. |
| `source_ref` | string | URL, archivo o identificador |
| `locator` | string/null | Página, celda, sección o fragmento |
| `captured_at` | datetime | Momento de captura |
| `last_checked_at` | datetime/null | Última verificación |
| `region` | string/null | Validez geográfica |
| `claim_type` | enum | Naturaleza de la afirmación |
| `verification_state` | enum | Estado de verificación |
| `valid_until` | date/null | Vigencia |
| `notes` | string/null | Límites, contradicciones o contexto |

### `DE_OPPORTUNITY`

| Campo | Tipo | Regla |
|---|---|---|
| `opportunity_id` | string | Único |
| `project_id` | string | Adaptador |
| `title` | string | Descriptivo, no publicitario |
| `demand_signal_refs` | string[] | Al menos una referencia |
| `vendor_refs` | string[] | Puede estar vacío solo si se declara brecha de oferta |
| `evidence_refs` | string[] | Evidencia de la relación |
| `assumptions` | string[] | Hipótesis visibles |
| `risks` | string[] | Riesgos visibles |
| `decision_state` | enum | OPEN, TEST_NEXT, PROPOSAL_ELIGIBLE, HOLD, REJECTED_BY_EVIDENCE |
| `next_action` | string | Prueba o decisión concreta |
| `human_gate` | string/null | Autoridad requerida |

### `RUN_LOG`

Registrar `run_id`, modo, adaptador, timestamps, fuentes consultadas, records creados/actualizados/omitidos, conflictos, errores y actor/autorización.

## Pruebas de límite

Aplicar estas preguntas antes de ubicar una función:

1. ¿Se reutiliza sin conocer DE MANO u otro proyecto? Entonces pertenece al núcleo.
2. ¿Cambia si cambia el proyecto, región, vocabulario o gate? Entonces pertenece al adaptador.
3. ¿Es una afirmación o documento original? Entonces pertenece a evidencia/RAG.
4. ¿Es una operación repetible sobre registros? Entonces puede pertenecer a automatización.
5. ¿Es una decisión irreversible o comercial? Entonces requiere gobernanza humana.
6. ¿Solo mejora cómo revisa la persona? Entonces pertenece a la superficie.

## Mapeo recomendado para hojas

| Registro | Pestaña neutral |
|---|---|
| `DI_SIGNAL` | `DIV_DI_SIGNAL` |
| `VI_VENDOR` | `DIV_VI_VENDOR` |
| `EVIDENCE_RECORD` | `DIV_EVIDENCE` |
| `DE_OPPORTUNITY` | `DIV_DE_OPPORTUNITY` |
| `RUN_LOG` | `DIV_RUN_LOG` |

Un adaptador puede cambiar los nombres físicos, pero debe conservar el significado y la clave estable.
