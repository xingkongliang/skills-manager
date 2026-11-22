---
name: metanoia-divide
description: Orquesta el doble escaneo DI·VI·DE para convertir señales de demanda y audiencia (Demand Intelligence), datos de proveedores y oferta (Vendor Intelligence), y evidencia trazable (Decision Evidence) en oportunidades gobernadas y registros candidatos. Úsalo cuando el usuario pida investigar, actualizar, comparar o auditar audience insights, intención de compra, ocasiones, proveedores, productos, MOQ, precios, muestras, compatibilidad o viabilidad; cuando necesite contrastar demanda contra oferta, preparar datos para Sheets, RAG o Apps Script, o cuando mencione METANOIA DIVIDE, dúo scanner, DE MANO o R06/R07. No lo uses para búsquedas aisladas sin una decisión que apoyar ni para aprobar automáticamente compras, proveedores o ventas.
---

# METANOIA DIVIDE

## Propósito

Operar un sistema neutral de inteligencia de mercado y abastecimiento:

- **DI · Demand Intelligence:** detectar audiencias, problemas, lenguaje, intención, ocasiones y señales de demanda.
- **VI · Vendor Intelligence:** localizar y comparar proveedores, oferta, condiciones, capacidad, compatibilidad y riesgos.
- **DE · Decision Evidence:** clasificar la evidencia, cruzar DI con VI y convertir hallazgos en candidatos con próximos pasos verificables.

Mantener el núcleo reutilizable. Incorporar vocabularios, objetos, umbrales y destinos mediante adaptadores de proyecto.

## Contrato operativo

1. Preservar las fuentes y el estado real de cada afirmación.
2. Separar dato observado, cálculo, inferencia, hipótesis y placeholder.
3. Tratar todo hallazgo como candidato hasta superar sus gates humanos y técnicos.
4. No convertir ausencia de información en cero ni baja calidad.
5. No promover resultados de investigación a canon, proveedor aprobado, producto aprobado o venta aprobada.
6. Mantener la regla `PROPOSAL_ELIGIBLE ≠ SALE_APPROVED`.
7. Trabajar en solo lectura por defecto. Escribir en sistemas externos únicamente cuando el usuario lo solicite.

## Elegir modo

| Modo | Resultado |
|---|---|
| `scan-demand` | Señales DI normalizadas y brechas de conocimiento |
| `scan-vendors` | Candidatos VI comparables, riesgos y verificaciones pendientes |
| `duo-scan` | DI + VI + cruce DE de oportunidades |
| `audit-evidence` | Vigencia, trazabilidad, contradicciones y placeholders |
| `prepare-ingestion` | Paquete estructurado para hoja, base, RAG o automatización |
| `design-adapter` | Mapeo neutral → objetos, vocabulario y gates de un proyecto |

Si el usuario no indica modo, elegir el mínimo que resuelva su decisión. Usar `duo-scan` solo cuando ambos lados del mercado sean necesarios.

## Ejecutar el flujo

### 1. Enmarcar la decisión

Determinar:

- proyecto, región, moneda y horizonte temporal;
- decisión que se quiere apoyar;
- audiencia, ocasión, categoría o necesidad;
- restricciones críticas;
- superficie de salida y autoridad de escritura.

Reutilizar el contexto disponible. Formular una sola pregunta si falta una variable que cambiaría materialmente la investigación; continuar con supuestos explícitos cuando no sea bloqueante.

### 2. Inspeccionar el estado existente

Leer primero hojas, registros, adaptadores, documentos y evidencia disponibles. Identificar duplicados, placeholders, campos vacíos, estados previos y reglas ya aprobadas. No rehacer conocimiento válido.

Cuando la información pueda haber cambiado —precios, inventario, promociones, condiciones, catálogos, proveedores o tendencias— verificarla con fuentes actuales. Preferir páginas oficiales, documentos primarios y evidencia directa; usar fuentes secundarias para contraste, no como sustituto automático.

### 3. Ejecutar DI

Capturar señales con estos mínimos:

- audiencia o segmento;
- problema, deseo, lenguaje o consulta;
- intención y ocasión;
- región y fecha observada;
- medida disponible y unidad, si existe;
- fuente y localizador;
- clase de afirmación, estado de verificación y vigencia;
- implicación posible sin presentarla como hecho.

Evitar convertir una sola mención, una tendencia general o una intuición editorial en demanda demostrada.

### 4. Ejecutar VI

Capturar candidatos con estos mínimos:

- identidad y ubicación del proveedor;
- producto, servicio, referencia o categoría;
- presentación, unidad y compatibilidad;
- precio, moneda, impuestos y envío cuando estén disponibles;
- MOQ, muestra, capacidad y lead time;
- condiciones, cumplimiento y riesgos;
- fuente, fecha, vigencia y estado de verificación.

Mantener `null` cuando un valor sea desconocido. Un proveedor localizado no equivale a proveedor verificado.

### 5. Aplicar DE

Vincular cada oportunidad con al menos:

- una señal DI;
- un candidato VI o una brecha explícita de oferta;
- sus registros de evidencia;
- riesgos y supuestos;
- siguiente prueba o acción humana.

Usar estados:

- `OPEN`: relación inicial sin evidencia suficiente;
- `TEST_NEXT`: señal prometedora que requiere una prueba concreta;
- `PROPOSAL_ELIGIBLE`: evidencia suficiente para formular propuesta, no para vender;
- `HOLD`: falta un dato crítico o existe conflicto material;
- `REJECTED_BY_EVIDENCE`: una condición comprobada invalida la oportunidad.

No producir puntuaciones compuestas si el adaptador no define criterios y pesos. Cuando sí los defina, mostrar componentes, datos faltantes y sensibilidad; no ocultar incertidumbre detrás de un número.

### 6. Emitir el resultado

Entregar primero un brief director breve:

1. decisión apoyada y alcance;
2. cobertura de evidencia;
3. oportunidades o candidatos principales;
4. brechas, riesgos y conflictos;
5. siguiente acción recomendada.

Añadir tablas o paquete técnico solo cuando ayuden a ejecutar o cuando el usuario los solicite. Seguir [references/output-contract.md](references/output-contract.md) para nombres de registros y estructura JSON.

Para paquetes JSON, ejecutar:

```bash
python3 scripts/validate_divide_packet.py path/to/divide-packet.json
```

Corregir errores antes de entregar o ingerir el paquete.

## Clasificar la verdad

Registrar dos ejes independientes:

**`claim_type`**

- `OBSERVED`: aparece directamente en la fuente.
- `CALCULATED`: deriva de una operación reproducible.
- `INFERRED`: interpretación razonada de una o más señales.
- `HYPOTHESIS`: explicación o oportunidad aún no probada.
- `PLACEHOLDER`: valor temporal que no representa evidencia real.

**`verification_state`**

- `UNCHECKED`
- `SOURCE_CAPTURED`
- `SOURCE_VERIFIED`
- `CROSS_CHECKED`
- `STALE`
- `CONFLICTED`

No elevar `INFERRED` o `HYPOTHESIS` a `OBSERVED`. Marcar una fuente como `STALE` cuando su vigencia haya expirado aunque haya sido verificada antes.

## Diseñar adaptadores

Mantener el adaptador fuera del núcleo conceptual. Definir:

- `project_id` y versión;
- región, moneda y horizonte;
- objetos y vocabularios del proyecto;
- campos obligatorios y mapeos a destinos;
- fuentes permitidas o preferidas;
- reglas de deduplicación;
- scoring, umbrales y gates, si existen;
- autoridad humana;
- política de vigencia;
- targets de hoja, RAG o automatización.

Leer [references/layer-record-model.md](references/layer-record-model.md) al diseñar arquitectura, tablas o contratos. Para R06 × R07, leer además [references/adapter-de-mano.md](references/adapter-de-mano.md).

## Coordinar herramientas y otros skills

- Usar búsqueda web para datos actuales y citar las fuentes que sustentan hallazgos.
- Usar conectores de hojas o archivos para inspeccionar la fuente operativa; comprobar acceso con una lectura inocua antes de programar automatizaciones futuras.
- Usar Data Analytics para validación cuantitativa, calidad o diagnóstico.
- Usar Spreadsheets o Google Sheets cuando la tarea requiera crear o modificar una hoja.
- Usar METANO_IA · CodexLSX Auto Engineer para auditar el workbook y construir la capa personalizada de Apps Script.
- Entregar a Apps Script paquetes candidatos, pequeños y validables; reservarle importación, deduplicación, locks, logs, checkpoints y exportación RAG.

No hacer scraping indiscriminado ni insertar fórmulas web masivas por fila. Respetar términos de uso, límites de APIs, privacidad y credenciales.

## Aplicar cambios externos

Cuando el usuario solicite escribir:

1. leer el destino y resolver el mapeo exacto;
2. generar un `DRY_RUN` con altas, actualizaciones, omisiones y conflictos;
3. validar referencias, duplicados, región, moneda, unidades y fechas;
4. pedir decisión solo si existe un conflicto material no resuelto;
5. escribir únicamente registros candidatos;
6. registrar ejecución, fuente y rollback/checkpoint disponible;
7. releer una muestra y confirmar el resultado.

No modificar objetos de producción ni aprobar ventas como efecto lateral de una investigación.

## Recursos

- [references/layer-record-model.md](references/layer-record-model.md): arquitectura neutral, records y límites.
- [references/output-contract.md](references/output-contract.md): brief, tablas y paquete JSON.
- [references/adapter-de-mano.md](references/adapter-de-mano.md): primer adaptador candidato para R06 × R07.
- `scripts/validate_divide_packet.py`: validación determinista del paquete de ingestión.
- `evals/evals.json`: escenarios iniciales de forward-testing.
