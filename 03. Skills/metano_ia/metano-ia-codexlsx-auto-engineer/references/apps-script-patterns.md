# Apps Script patterns

## Safety baseline

- Bind the project to the target Google Sheet.
- Use `onOpen(e)` only to create a menu.
- Keep `onEdit(e)` optional, single-range, and fast. Simple triggers cannot use services requiring authorization and have a 30-second limit.
- Put multi-sheet validation and writes behind explicit menu actions or installable triggers.
- Use `LockService.getDocumentLock()` before mutations.
- Use `PropertiesService.getDocumentProperties()` for small runtime state, never secrets.
- Read ranges in batches; transform arrays in memory; write one rectangular block.
- Resolve columns from normalized header text while preserving exact headers in reports.

## Idempotent write

1. Acquire document lock.
2. Read current row IDs.
3. Build desired records keyed by stable ID.
4. Compute `create`, `update`, `unchanged`, and `conflict`.
5. In `DRY_RUN`, return the delta without calling a setter.
6. In `APPLY`, write only creates/updates.
7. Append one audit record per batch.
8. Release the lock in `finally`.

## Gate protection

Treat approval as an output of verified conditions:

```javascript
const mayApprove = conditions.every(Boolean);
if (requestedApproval && !mayApprove) {
  findings.push({severity: "CRITICAL", code: "GATE_BYPASS"});
}
```

Report conflicts before offering a separate confirmed repair action.

## Structural preflight

Check required sheets, headers, duplicate IDs, orphan references, formula columns overwritten by literals, invalid controlled values, unexpected approval states, and workbook-specific reconciliations.

## Audit

Use `_AUTOMATION_LOG` with:

`Timestamp, Run_ID, Mode, Function, Actor, Created, Updated, Conflicts, Status, Details_JSON`

Never log credentials, personal contact information, or raw source documents.

## Deployment states

- `READY_FOR_IMPORT`: static project validated locally.
- `READY_FOR_DRY_RUN`: imported into the target Sheet, not yet executed.
- `DRY_RUN_PASS`: read-only checks executed successfully.
- `APPLY_APPROVED`: user explicitly approved a mutation.
- `DEPLOYED_VERIFIED`: live project executed and verified.
- `BLOCKED`: structural mismatch, missing authorization, or unsafe rule.
