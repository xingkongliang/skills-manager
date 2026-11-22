
# Execution graph

```mermaid
flowchart TD
    C["M00 · Contract"] --> S["M01 · Sources"]
    S --> R["M02 · RAG"]
    R --> X["M03 · Router"]
    X --> L["M05 · Lines"]
    X --> K["M04 · Skills"]
    L --> D["M06 · DMIE/MMII"]
    L --> O["M07 · Comet"]
    K --> A["M08 · DRAME"]
    D --> A
    O --> A
    A --> E["M09 · EIC"]
    E --> Q["M10 · ÆQUITAS"]
    Q --> H["M11 · Handoff"]
    H --> M["M12 · METANO memory"]
```

## Router minimum output

```json
{
  "owner_module": "M08_DRAME_COMPOSER",
  "source_ids": ["SRC-BOS-V2"],
  "line_id": "LINE-LITERATURE",
  "skill_ids": ["SK-CRAFTER"],
  "mode": "dry-run",
  "blocked_by": []
}
```
