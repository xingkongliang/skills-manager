
# Arquitectura unificada

## Capas documentales

```mermaid
flowchart TD
    N0["N0 · evidencia y límites"] --> N1["N1 · schemas, módulos y QA"]
    N1 --> N2["N2 · cuadernillos y workbooks"]
    N2 --> G["Gate · SELAH + ÆQUITAS"]
    G --> H["Handoff · D / aprobación"]
```

`N0` gobierna la verdad disponible. `N1` transforma esa verdad en contratos ejecutables. `N2` contiene objetos humanos, nunca autoridad por sí mismos.

## RAG por función

```mermaid
flowchart TD
    S["Fuentes selladas"] --> R["Registro y estados"]
    R --> I["Índice RAG"]
    I --> Q["Router de consulta"]
    Q --> L["Líneas editoriales"]
    Q --> E["Engines y adapters"]
    L --> A["Compositor DRAME"]
    E --> A
    A --> V["Validación y handoff"]
```

La recuperación exige filtros de `status`, `authority`, `domain`, `package_version` y, cuando aplique, `line_version`. La similitud semántica nunca decide autoridad.

## Ejecución gobernada

```mermaid
sequenceDiagram
    participant U as Dirección
    participant P as Prometheus
    participant R as RAG Router
    participant M as Módulo
    participant Q as SELAH/ÆQUITAS
    U->>P: objetivo + scope + aprobación
    P->>R: consulta con filtros
    R->>M: evidencia y contrato
    M->>Q: delta + trazabilidad
    Q-->>U: ACCEPT / PATCH / REJECT
```

Prometheus coordina una sola corrida; no sustituye la decisión editorial. SELAH bloquea mutaciones no autorizadas y ÆQUITAS califica el objeto resultante.
