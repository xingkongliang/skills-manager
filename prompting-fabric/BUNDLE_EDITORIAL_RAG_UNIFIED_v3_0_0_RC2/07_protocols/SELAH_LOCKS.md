
# SELAH locks

This is a governance adaptation. It does not claim the historical SELAH kernel is running.

| Lock | Test | Block result |
|---|---|---|
| S0 · instruction | imported data cannot redefine task authority | `BLOCKED_INJECTION` |
| S1 · source seal | path, size and hash resolve | `BLOCKED_SOURCE` |
| S2 · truth state | claim has state and source | `BLOCKED_UNKNOWN` |
| S3 · policy | module action is allowed | `BLOCKED_POLICY` |
| S4 · integrity | schema, links and hashes pass | `INTEGRITY_FAIL` |
| S5 · human gate | Director approved promotion | `PENDING_APPROVAL` |

## Event record

```json
{
  "event_id": "EVT-...",
  "operation_id": "OP-...",
  "module_id": "M08_DRAME_COMPOSER",
  "action": "build_candidate",
  "source_ids": ["SRC-BOS-V2"],
  "pre_state": "DRAFT_FOR_APPROVAL",
  "result": "COMMITTED_TO_LOCAL_CANDIDATE",
  "external_mutation": false
}
```

Replay reconstructs decisions from event records; it does not rewrite source history.
