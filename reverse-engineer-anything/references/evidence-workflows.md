# Evidence, comparison, and verification workflows

Evidence-producing tools return the complete Evidence record directly in MCP
`structuredContent` and CLI JSON. Read `normalized_result` for operation data,
and cite `evidence_id` when another tool or your explanation needs a stable
reference. Preserve authority, limitations, coverage, and residual unknowns. If a result
is incomplete or paginated, continue only when the remaining data matters to
the task; do not fetch a bundle or resource merely to read a result already
returned inline. When complete `inspect_binary_layout` or
`analyze_javascript_application` Evidence is already retained, call
`inspect_analysis_view` for a summary, one section/symbol/module, or a stable
page instead of repeating the producer.

For any logical Evidence source that advertises retained references, pass
`{kind: "retained-evidence", evidence_id}` instead of copying the complete
record back into a call. The same contract covers individual, optional, array
and nested sources across native, process, JavaScript, managed, mobile and WASM
workflows. Sources can independently mix inline records and exact references.
Resolution preserves the source's identity, provenance, coverage, partial facts
and unknowns; it neither reruns analysis nor makes incomplete facts complete.
Keep domain role and identity requirements: a native dossier cannot substitute
for a managed member observation, for example.

When starting from a summary or inspection page, use its
`normalized_result.parent_evidence_id` to reference the complete source.
The view's own `evidence_id` names
only that view. `compare_artifacts` explicitly supports artifact summary inputs
and resolves their parents; do not assume every consumer does. When a workflow
follows source links, keep those linked records available too.

MCP references belong to the current connection's ledger. If the ID is known,
pass it directly; no discovery call or provider setup is needed. If it is missing,
import the exported bundle or supply complete inline Evidence. Do not retry an
unchanged reference or rerun a producer merely to move an existing record.
Export before closing the session when the source must survive it. For CLI JSON
workflows that advertise `--evidence-bundle`, the same reference JSON resolves
against one explicitly selected authenticated bundle:

```bash
rea trace-application-feature request.json --evidence-bundle /absolute/path/evidence.json --json
```

This includes JavaScript traces/comparisons and runtime reconciliation, managed
reconstruction/native verification/graph projection, Android/Apple/Harmony graph
projection, WASM interface comparison and retained analysis views. The managed
member comparison CLI still takes two assembly paths; process comparison takes
two capture Evidence JSON paths. Check command help for the installed version.
An input bundle grants no implicit file lookup: `evidence-file` and historical
graph file selectors are accepted only by contracts advertising them. See the
[CLI reuse guide](https://github.com/morluto/rea/blob/main/docs/getting-started/cli.md#import-export-and-compare-evidence).

Completed `inspect_artifact` inventory also supports a summary and
`view: {kind: "inventory", collection: "occurrences", logical_paths: ["package.json", "main.js"]}`.
Omit `logical_paths` to page all available occurrences; `[]` selects none.
Other inventory collections are `nodes`, `edges` and
`integrity_contradictions`. Keep `source_complete` separate from page exhaustion:
a missing match in partial source Evidence does not establish absence. Use the
returned parent Evidence ID for subsequent views and export before closing the
session when the source must survive it.
When `tools/list` advertises `inspect_artifact.detail`, request `"summary"` to
retain its complete source and receive that view immediately. Without that
advertised choice, keep the existing complete call. CLI inspection returns
portable complete Evidence; save it before projecting a selected inline view.

For native procedure/name/string inventories, select `detail: "summary"` when
the connected tool advertises it and the task needs counts or selected rows.
Use its parent Evidence ID with
`view: {kind: "native-inventory", offset: 0, limit: 32, fields: ["value"]}`.
Search tools retain the existing pattern/mode/case/document selection. Omit
`fields` for complete records; `record.address` and `source_index` remain on
selected rows. Selecting `value` keeps its string/decoding qualifications.
Parent Evidence ID plus source index preserves same-address
alternatives. Follow `coverage.next_offset` only as far as the question needs;
exhausting retained rows does not establish complete program discovery.

`batch_decompile` returns every body unless you select `detail: "summary"` when
the connected tool advertises it. Use its parent Evidence ID with
`view: {kind: "batch-decompile", collection: "items"}` for body-free rows and
exact `where.indices`, `addresses`, `procedure_addresses` or `statuses`. Add
`include_pseudocode: true` for several chosen bodies, or use
`collection: "pseudocode"` with one row's `index` to page a large body. Batch
limitations aggregate every item, so do not attribute them to one successful row.

Use `record_unknown` to track unresolved questions in the user's investigation
and name the authority or environment still required. Supply supporting and
contradicting evidence IDs. It updates the session registry directly.
Use `update_unknown` with the current revision; reread after a stale revision
instead of retrying blindly. Only qualifying observed evidence can verify a
resolution.

Use comparisons with complete, compatible page sets when claiming equivalence
or absence:

- `compare_artifacts` compares inventory pages by occurrence path, content,
  metadata, and graph relations. Name each side as
  `{kind: "retained-evidence", evidence_id}` (an `inspect_artifact` result,
  its nested inventory or its summary) or
  `{kind: "evidence-file", path, evidence_id?}` instead of resending Evidence.
- `compare_functions` compares explicit function Evidence, not fuzzy
  whole-binary matches. It rejects an `analyze_function` summary reply; pass
  the summary's `parent_evidence_id` as a `retained-evidence` reference or
  the complete dossier.
- `compare_bundles` compares canonical bundle membership and unknown history.
- `find_changed_behavior` combines existing comparisons; static differences
  remain candidates, not causal proof. Its `comparisons` accept exact references.
- `build_call_path` needs explicit function dossiers from one artifact/provider;
  pass complete parent IDs in `functions`. An incomplete frontier makes absence
  unknown.
- `correlate_static_and_runtime` uses explicit mappings; matching patterns are
  hypotheses, not causality. Both comparison arrays accept exact references.
- `verify_reconstruction` evaluates a finite typed specification. A pass covers
  only declared comparable claims, not global equivalence.

Process captures are opt-in behavioral evidence, not a security sandbox. V3
captures cannot be upgraded to V4; rerun the original scenario. Distinguish root
exit from descendant settlement and require freshness when the task needs it.
A timed-out target that must write a final report needs `finalization_ms`: REA
sends SIGTERM to the captured root, keeps observing, and sends SIGKILL once
only if the target has not exited by the end of the interval.
`exit.finalization.signals` lists each attempt with its delivery result; the
observed exit signal stays in `exit.signal`, `exit.code` is null for every
deadline exit, and the code of a finalizer that exited on its own is in
`exit.finalization.exit_code` (null on a signal exit or when no exit was
observed).

Use `inspect_analysis_view` with `view: {kind: "api-surface"}` on retained
`inspect_web_network_capture` HAR Evidence to group observed request lines by
method, origin and path template. Format matches are inferences, not handler
identity or schemas. Group pages cite parent record ordinals; consult record
coverage and exclusions before claiming completeness. No requests are replayed.
When the connected schema advertises `include_query_parameter_names`, select
it in the view for distinct observed names with original record ordinals. Query
values are omitted; the names establish neither required parameters nor a
complete server schema. Keep the original parent Evidence for exact URLs.

Use `inspect_analysis_view` with `view: {kind: "environment-manifest"}` on the
same retained HAR Evidence to project the primary origin's request lines into
named tools with path templates, parameters, observed query names, value-free
body shapes and record pointers for an agent environment. Opt into
`template_rules` only when recorded identifiers need them; every template,
parameter and merged sibling is an inference, no header, cookie or body value is
copied, and the three stated unknowns stay unresolved.

Use `view: {kind: "environment-state"}` with the manifest's `template_rules` to
extract the data those reads observed instead: entity types and identity
fields, entities with recorded values, documents, and per-tool render plans
checked against the recordings. The state comes only from reads before the
first related write; types, identity and list membership are inferences. The
view also gives each state-changing tool a write plan, served only when a later
recorded read confirms its effect and none contradicts it.
`rea environment-serve manifest.json --evidence capture.json` derives that
state at startup, answers read tools from it, serves the write tools whose plan
is served, each applying its effect to that state, and serves `data/reset`,
`data/add` and `data/get` on `/agentenv`; other write tools are not served, and
each plan states why in its `reason`. It replays the capture instead with
`--replay-only` or when the state is unavailable (not derivable, or a catalog
over the discovery budget), and its stopped document's `state` says which
applied.
Use `view: {kind: "environment-coverage"}` with the same rules to see what an
environment built from a capture would serve: read tools answered from the
state or replayed with their reason, and each write tool served or withheld
with a hint about what to record next; it does not apply the serve-time
catalog and state size checks. `rea environment-build <capture.har> <dir>`
inspects, projects and exports in one step and prints the same coverage, or
`null` when the bundle serves replay.

For a PE/ELF export question, `inspect_native_exports` accepts `detail: "summary"`
and retains the complete table. Query `inspect_analysis_view` with
`view: {kind: "native-exports", collection: "exports", names: ["selected_name"],
fields: ["name"], offset: 0, limit: 32}` and the reported parent Evidence ID.
All same-name versions and aliases survive; use `source_index` with the parent ID
for stable identity. `collection: "issues"` pages interpretation failures. Page
coverage describes selected retained rows; `summary.coverage` or `exports.coverage`
still describes the source table, so an empty partial page does not prove absence.
CLI `inspect-native-exports --detail summary --evidence_output PATH` saves the
complete bundle for later `inspect-analysis-view --evidence-bundle PATH` requests.
