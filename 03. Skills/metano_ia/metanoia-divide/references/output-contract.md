# Contrato de salida

## Brief director predeterminado

Usar esta estructura, omitiendo secciones vacías:

```markdown
# DIVIDE · [proyecto o decisión]

Estado: [RESEARCH_ONLY | PARTIAL | READY_FOR_REVIEW | BLOCKED]

## Decisión apoyada
[Una o dos frases]

## Cobertura
| Plano | Confirmado | Pendiente | Riesgo principal |
|---|---:|---:|---|
| DI · Demanda | | | |
| VI · Proveedores | | | |
| DE · Evidencia | | | |

## Oportunidades
| Candidato | Evidencia DI | Evidencia VI | Estado DE | Próxima prueba |
|---|---|---|---|---|

## Brechas y conflictos
- [...]

## Siguiente acción
[Una acción concreta y proporcionada]
```

## Paquete de ingestión

Usar JSON UTF-8 con esta forma:

```json
{
  "schema_version": "metanoia-divide/1.0",
  "run": {
    "run_id": "RUN-...",
    "project_id": "PROJECT-...",
    "mode": "duo-scan",
    "region": "MX",
    "currency": "MXN",
    "started_at": "2026-07-28T12:00:00-06:00",
    "completed_at": "2026-07-28T12:10:00-06:00",
    "adapter_version": "1.0"
  },
  "demand_signals": [],
  "vendors": [],
  "evidence": [],
  "opportunities": [],
  "conflicts": [],
  "next_actions": []
}
```

## Reglas de serialización

- Usar IDs estables con prefijos `DI-`, `VI-`, `EV-`, `DE-` y `RUN-`.
- Usar `null` para desconocido; no usar `0`, cadena vacía, `N/A` o `pendiente` como sustitutos.
- Usar fechas ISO 8601.
- Usar monedas ISO 4217.
- Conservar precio, envío e impuestos en campos separados.
- Enlazar todos los `evidence_refs`, `demand_signal_refs` y `vendor_refs`.
- Incluir una brecha explícita cuando una oportunidad no tenga proveedor.
- No emitir `SALE_APPROVED` en paquetes de investigación.

## Dry run de escritura

Antes de escribir en una hoja o base, informar:

| Operación | Cantidad | Criterio |
|---|---:|---|
| Crear | | ID nuevo y válido |
| Actualizar | | Mismo ID, evidencia más reciente |
| Omitir | | Duplicado o sin cambio |
| Conflicto | | Valores incompatibles sin regla de resolución |
| Rechazar | | Falla de esquema, referencia o gate |

Después de escribir, releer una muestra de cada tipo modificado y registrar el `run_id`.
