# Automation blueprint

Use JSON. Keep sheet names and headers exact.

```json
{
  "schemaVersion": "1.0",
  "project": {
    "name": "string",
    "version": "string",
    "status": "DRAFT|READY_FOR_DRY_RUN|BLOCKED",
    "timezone": "Area/City"
  },
  "workbook": {
    "filename": "string",
    "sha256": "string",
    "sheetCount": 0,
    "truthState": "EVIDENCE_PHYSICAL_VERIFIED|USER_PROVIDED|UNKNOWN"
  },
  "runtime": {
    "defaultMode": "DRY_RUN",
    "auditSheet": "_AUTOMATION_LOG",
    "configSheet": "_AUTOMATION_CONFIG",
    "simpleOnEdit": false
  },
  "sheets": [
    {
      "name": "Exact tab name",
      "role": ["MASTER_DATA"],
      "headerRow": 5,
      "idHeaders": ["Record_ID"],
      "requiredHeaders": ["Record_ID", "Status"],
      "inputHeaders": ["Status"],
      "formulaHeaders": [],
      "required": true
    }
  ],
  "rules": {
    "unique": [{"sheet": "Tab", "headers": ["Record_ID"], "severity": "CRITICAL"}],
    "references": [
      {
        "fromSheet": "Child",
        "fromHeader": "Parent_ID",
        "toSheet": "Parent",
        "toHeader": "Parent_ID",
        "allowBlank": true
      }
    ],
    "acceptedValues": [
      {"sheet": "Tab", "header": "Status", "values": ["PENDING", "PASS", "FAIL"]}
    ],
    "protectedOutcomes": [
      {
        "sheet": "Tab",
        "header": "Sale_Approved",
        "protectedValues": ["YES"],
        "policy": "Never set automatically without every declared gate."
      }
    ]
  },
  "automations": {"safe": [], "conditional": [], "blocked": []},
  "tests": [{"id": "T-001", "mode": "DRY_RUN", "expect": "No spreadsheet values change."}]
}
```

- `truthState` describes the inspected artifact, not global canon.
- `requiredHeaders` are compatibility contracts.
- `inputHeaders` may be written only by explicit `APPLY` actions.
- `formulaHeaders` remain read-only unless formula repair is explicitly requested.
- `protectedOutcomes` must be enforced in code and tests.
