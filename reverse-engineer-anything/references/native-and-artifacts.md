# Native, managed, and packaged artifacts

For release comparisons on one MCP connection, retain `list_procedures` Evidence
from each target and reuse its exact IDs after target switches or `close_binary`.
For a handoff or new process, export the complete bundle and import it there,
including its source closure. `reset_investigation` explicitly clears retained
records; optional `save_path` checkpoints them before clearing.
`match_native_functions` pairs unique exact names and, when supplied,
complete observed body bytes. Reuse optional same-context `list_names`,
`procedure_info`, `read_bytes` and existing `analyze_function` dossiers as
`left_observations`/`right_observations`; an MCP `analyze_function` summary
reply contributes its `parent_evidence_id` as a `retained-evidence` reference,
not the summary itself. Avoid decompiling every function merely to match it. Its summary retains a parent for
`inspect_analysis_view` `function-matches` collections `matches` and `functions`.
Select columns, batch IDs and page individual facts; aliases remain unresolved.
Unpaired observations do not prove additions/removals. Existing matched dossier
pairs include inputs for `compare_functions`; otherwise analyze just chosen pairs.

Aggregate completed comparisons with `find_changed_behavior`, selecting each
comparison's exact `retained-evidence` ID instead of resending full records.
Inline comparison Evidence remains accepted. References belong to the current
server: after handoff, import the original bundle including its source closure.
Do not substitute dossier IDs or summary views. Static differences remain
behavior candidates, with runtime behavior unknown unless separately observed.

For a matching representation refusal, inspect `details.limit` and raise
`native.matching_representation_bytes` with `update_analysis_settings` before
retrying the unchanged MCP request. CLI launch seed:
`REA_NATIVE_MATCHING_REPRESENTATION_BYTES`. The default is 96 MiB; the policy
range is 1 byte to 1 GiB and does not bound process RSS. Keep the complete
inventories: splitting them changes global name/body uniqueness.

## Apple app resources

Use `open_bundle_resources` with the selected app path, then `inspect_plist`,
`decode_interface_builder`, `inspect_asset_catalog`, or `inspect_keyed_archive`.
The resource session reads the app without resolving `CFBundleExecutable` or
starting a deep provider. Preserve `resource_root` provenance, per-resource
hashes where reported, and null executable identity; a directory path is not
an executable digest. Export retained Evidence before closing or switching
roots. Binary analysis snapshots do not apply. Use `open_binary` to switch to
executable analysis, and `close_binary` to release either session kind.

The four matching CLI commands accept the app path directly. For an archive
inside the app, use `inspect-keyed-archive APP_PATH RELATIVE_ARCHIVE_PATH`.
Host requirements for plist and compiled asset/Interface Builder tooling still
apply; a resource-only session does not establish provider availability.

## Desktop installation directories

When an installation directory is supplied and its runtime is unknown, use
`inspect_desktop_application` with its absolute path. Default MCP summary
delivery retains complete Evidence. Read `files`, `omissions`, `candidates`, `components` or
`next_probes` with `inspect_analysis_view` kind `desktop-application`, selecting
exact paths, batched file/omission IDs or families and only needed fields and pages.
Summary `probe_issues` retains counts and bounded previews. Select files with
`has_issues: true` and fields `path`, `read_status`, `issues` to retrieve exact
failures directly; preview text is display data, while `file_id` selects full facts. Preserve source
coverage, read-range identity and unknowns; directory snapshot identity is not
a digest of every file's contents. Component path markers do not establish
runtime presence. For compact CLI output, use
`inspect-desktop-application PATH --detail summary --evidence_output BUNDLE`.
See [desktop installations](https://github.com/morluto/rea/blob/main/docs/desktop-applications.md).

## Release exposure

Use `audit_release_exposure` (`rea audit-release-exposure PATH`) when the question is what static analysis of a shipped `.app`, install directory, ASAR, or executable recovers. Each check has a stable id, a status, the observation behind it, and fixed remediation. Unknown and unassessed checks stay in the report. The report does not establish that the application is secure.

`text.credential_markers` reports a rule id and location only. Matched bytes, previews, lengths, and hashes of matched bytes are omitted.

`native.hardening`, `macho.hardened_runtime`, and `android.manifest` are not assessed here. Their next probes are `inspect_binary_hardening`, `inspect_signature`, and `inspect_android_package`.

See [release exposure](https://github.com/morluto/rea/blob/main/docs/release-exposure.md).

## Go build metadata

Use `inspect_go_binary` for an explicit local ELF, PE or Mach-O (universal
files need `architecture`) when the
question concerns the embedded compiler version, module/dependency versions,
local replacements, build settings, or static Go string literals packed without
NUL terminators. Pass `include_string_literals: true`
(`--include-string-literals`, optional `max_string_literals` /
`--max-string-literals`) to recover bounded inferred word-aligned `(ptr, len)`
UTF-8 string slices from read-only data and page them through `go`
`string_literals` views. No `open_binary`, Go installation or
target execution is required. The CLI equivalent is `inspect-go-binary PATH`.
Preserve artifact SHA-256, original file ranges, framed module bytes and every
unparsed module-text line. A missing record is an unknown, not proof of a
non-Go binary; observed metadata does not authenticate its claims, and static
`(ptr, len)` string recovery excludes compiler-inlined stack/immediate strings
without proving runtime reachability. Function,
source and type recovery remain a separate native-analysis question.

## Model files

Use `inspect_model_artifact` (`inspect-model-artifact PATH`) for local GGUF v2/v3
and safetensors declarations without a model runtime. Header-only mode does not
read weights or establish full-file identity. `hash: "full"` explicitly streams
all bytes. Retained `model-artifact` views page `metadata`, `tensors` and derived
`types`, with exact batch filters and selected fields. Arrays retain typed counts
and eight-element previews; omitted values, unknown types and runtime behavior
stay explicit. See [model files](https://github.com/morluto/rea/blob/main/docs/model-artifacts.md).

## IL2CPP metadata

Use `inspect_il2cpp_metadata` for an explicit local Unity `global-metadata.dat`
(versions 24, 27, 29, 31 and 39) when the question concerns assembly images,
names, member owners, string literals, namespaces or index ranges. No `open_binary`, Unity installation or target
execution is required. The CLI equivalent is `inspect-il2cpp-metadata PATH`.
MCP defaults to counts with complete retained Evidence. Query `sections`,
`images`, `types`, `methods`, `fields`, `properties`, `events`, `parameters` or
`string_literals` through `inspect_analysis_view`; use exact names,
batch indices and selected fields. Row indices are scoped by artifact SHA-256
and collection; page offsets count filtered matches. Parameters support batch
`method_indices` to read one method's parameters directly. Historical reports
without member collections retain unknown counts rather than empty inventories. Preserve
artifact SHA-256 and index widths. Unrecognized magic may mean encryption and
is unsupported, not decrypted. Linking names to `GameAssembly` or
`UnityFramework` code addresses remains a separate native-analysis question.
See [IL2CPP metadata](https://github.com/morluto/rea/blob/main/docs/il2cpp-metadata.md).

## Rust dependency metadata

Use `inspect_rust_binary` for an explicit local ELF, PE, or Mach-O (universal
files need `architecture`) when
the question concerns the cargo-auditable crate list embedded in `.dep-v0`
(Mach-O: `__DATA,.dep-v0`). No `open_binary`, Rust installation, or target
execution is required. The CLI equivalent is `inspect-rust-binary PATH`.
Preserve artifact SHA-256, package order, dependency indices, and the section
file range. A missing section is null metadata, not proof the binary has no
dependencies. An omitted package kind stays null. A rustc commit hash is
inferred from a read-only `/rustc/<40-hex>/` path and does not by itself
identify the compiler. See [Rust binaries](https://github.com/morluto/rea/blob/main/docs/rust-binaries.md).

## PE Authenticode certificate tables

Use `inspect_pe_certificates` for an explicit local PE32/PE32+ image when the
question concerns the Authenticode certificate table, WIN_CERTIFICATE types, or
embedded PKCS#7 signer and X.509 identity fields. No `open_binary`, Windows SDK,
or target execution is required. The CLI equivalent is
`inspect-pe-certificates PATH`. Preserve artifact SHA-256, table file ranges,
payload hashes, original validity-time spellings, and
`subject_key_identifier_hex` when the signer uses that form. A missing
`IMAGE_DIRECTORY_ENTRY_SECURITY` directory is observed absence of that table,
not proof the image is unsigned in every catalog or overlay sense. Signatures,
chains, timestamps, and Authenticode digest coverage remain unobserved. See
[PE Authenticode certificate tables](https://github.com/morluto/rea/blob/main/docs/pe-certificates.md).

## PE import and delay-load tables

Use `inspect_pe_imports` for an explicit local PE32/PE32+ image when the
question concerns imported DLL and function names, ordinals, delay-load
descriptors, or the pefile import hash. No `open_binary`, Windows SDK, or
target execution is required. The CLI equivalent is `inspect-pe-imports PATH`.
Preserve artifact SHA-256, directory file ranges, original DLL spellings, and
ILT/IAT RVAs. A missing `IMAGE_DIRECTORY_ENTRY_IMPORT` directory is observed
absence of that table, not proof the image never imported APIs through another
table or overlay. Bound snapshots, loaded IAT contents, and export-forward
resolution remain unobserved. See
[PE import and delay-load tables](https://github.com/morluto/rea/blob/main/docs/pe-imports.md).

## Windows Shell Link shortcuts

Use `inspect_windows_shortcut` for an explicit local `.lnk` file when the
question concerns the recorded target path, arguments, working directory, icon
location, volume serial, network share names, ExtraData blocks, or Tracker
droid identifiers. No `open_binary`, Windows SDK, COM, or target execution is
required. The CLI equivalent is `inspect-windows-shortcut PATH`. Preserve
artifact SHA-256, FILETIME raw decimals, IDList type bytes, and ExtraData
signatures. Unknown IDList items and ExtraData signatures remain raw with
SHA-256; they are not guessed. Version-1 UUID MAC and timestamp values are
labelled derived. A missing optional section is observed absence of that
structure. See
[Windows Shell Link shortcuts](https://github.com/morluto/rea/blob/main/docs/guides/native/windows-shortcut.md).

## PE TLS callbacks and load configuration

Use `inspect_pe_load_config` for an explicit local PE32/PE32+ image when the
question concerns code that runs before the entry point through TLS callbacks,
or the security cookie, SafeSEH, CFG, EH-continuation and XFG declarations in
the load configuration. No `open_binary`, Windows SDK, or target execution is
required. The CLI equivalent is `inspect-pe-load-config PATH`. Preserve artifact
SHA-256, directory and callback-array file ranges, and callback VAs as
hexadecimal strings. Fields outside the declared `Size` are `null`. Guard-flag
mitigations carry `evidence_kind: "inferred"`; enforcement remains unobserved.
See
[PE TLS callbacks and load configuration](https://github.com/morluto/rea/blob/main/docs/pe-load-config.md).

## Binary hardening

Use `inspect_binary_hardening` for an explicit local ELF, PE, or Mach-O file
when the question concerns PIE or ASLR, NX, RELRO, stack-protector or FORTIFY
signs, control-flow flags, or stripped state. No `open_binary`, pwntools, or
target execution is required. The CLI equivalent is
`inspect-binary-hardening PATH`. Universal Mach-O files return one image per
slice. A missing import is `unknown`, not proof the protection was omitted.
Authenticode and Mach-O code-signature rows report presence, not validity.
`hardened_runtime` stays unknown and names `inspect_signature`. See
[binary hardening](https://github.com/morluto/rea/blob/main/docs/binary-hardening.md).

## ELF note tables

Use `inspect_elf_notes` for an explicit local ELF32/ELF64 image when the
question concerns PT_NOTE/SHT_NOTE owners, GNU build-id, or GNU ABI-tag
fields. No `open_binary` or target execution is required. The CLI equivalent
is `inspect-elf-notes PATH`. Preserve artifact SHA-256, table file ranges,
descriptor hashes, and original owner spellings. A missing table is observed
absence of those declarations, not proof the image never carried notes in
another container. See
[ELF note tables](https://github.com/morluto/rea/blob/main/docs/elf-notes.md).

## ELF and Mach-O initializer functions

Use `inspect_native_initializers` for an explicit local ELF or Mach-O image
when the question is which functions run before `main` or at exit: ELF
`DT_PREINIT_ARRAY`, `DT_INIT`, `DT_INIT_ARRAY`, `DT_FINI`, `DT_FINI_ARRAY` and
legacy `.ctors`/`.dtors`, or Mach-O `__mod_init_func`, `__init_offsets` and
`__mod_term_func`. Pass `architecture` for a universal Mach-O. The CLI
equivalent is `inspect-native-initializers PATH`. PIE slots are resolved
through RELATIVE, RELR and section relocations, and Mach-O slots through
chained fixups. Keep each entry's `resolution`: `unresolved` means the value was
not established, and `unresolved_import` names an external symbol. Page with
`offset` and `limit`. See
[ELF and Mach-O initializer functions](https://github.com/morluto/rea/blob/main/docs/guides/native/native-initializers.md).

## ELF unwind function ranges

Use `inspect_elf_unwind_ranges` for an explicit local ELF32/ELF64 image when
the question concerns function starts and lengths that survive `strip`, which
functions have an LSDA, or which ranges have no exact symbol. No
`open_binary` or target execution is required. The CLI equivalent is
`inspect-elf-unwind-ranges PATH`. Preserve artifact SHA-256, FDE start and
end, containing section, exact symbol name, CIE index, LSDA, and personality.
Page later ranges with `offset` and `limit`. A missing `.eh_frame` is
observed absence of that declaration and of `PT_GNU_EH_FRAME`, not proof the
image has no functions. Header disagreement is a limitation and does not drop
decoded ranges. `.debug_frame` is reported present or absent and is not
decoded. CFA instructions are counted and not evaluated. Indirect EH pointers
are not dereferenced. See
[ELF unwind function ranges](https://github.com/morluto/rea/blob/main/docs/guides/native/elf-unwind-ranges.md).

## Apple IPS crash reports

Use `inspect_apple_ips` for an explicit local Apple `.ips` file when the
question concerns crash metadata, exception and termination facts, threads,
usedImages, lossless registers, or documented crash classes. No `open_binary`,
Xcode, atos, or target execution is required. The CLI equivalent is
`inspect-apple-ips PATH`. Preserve artifact SHA-256, `bug_type` as a string,
uppercase image UUIDs, and integer literals that exceed IEEE-754 range. Only
`bug_type` `"309"` yields a crash body; other DiagnosticReports types return
no crash result. Select `join_path` for one local Mach-O or dSYM to match
report images to slices by UUID and architecture. Preserve the selected
artifact identity, slice facts and unjoined reasons; a joined static address
does not resolve function names or dSYM symbols. See
[Apple IPS crash reports](https://github.com/morluto/rea/blob/main/docs/apple-ips.md).

## Linux kernel modules

Use `inspect_kernel_module` for an explicit local ELF32/ELF64 relocatable
object when the question concerns `.modinfo` license, vermagic, alias,
depends, or parm strings, the `__versions` symbol CRCs, or an appended module
signature trailer. No `open_binary`, Linux host, or `modinfo` program is
required. The CLI equivalent is `inspect-kernel-module PATH`. Preserve
artifact SHA-256, key order, duplicate keys, CRC hex, and section file ranges.
A missing section is observed absence when section names are known, not proof
the module never carried that metadata. `.gnu.linkonce.this_module` is reported
as present or absent; `struct module` stays unknown because its layout changes
with the kernel version. `__ksymtab` records are not decoded. The PKCS#7 range
and its SHA-256 are observed; certificate, signer, and digest coverage remain
unvalidated. An absent trailer is not proof the module is unsigned in another
container. gzip, xz, and zstd whole-file containers are unsupported and name
`external_decompression` as the next probe: use an external codec utility to
decompress into a separate file, then call `inspect_kernel_module` on that file. See
[Linux kernel modules](https://github.com/morluto/rea/blob/main/docs/kernel-modules.md).

## Linux kernel build config

Use `inspect_kernel_build_config` for an explicit local kernel image (`vmlinux`
ELF, raw arm64 `Image`, x86 `bzImage`/`zImage`, or `configs.ko`) when the
question is which kernel version it is or how it was configured. No
`open_binary`, Linux host, or `extract-ikconfig` is required. The CLI
equivalent is `inspect-kernel-build-config PATH`; pass `name` for one exact
`CONFIG_*` option and `offset`/`limit` to page the rest. Preserve the banner
version, builder, and compiler, the config SHA-256 and option count, and each
entry's value kind (`y`, `m`, `string`, `number`, `not_set`). A config is
accepted only when `IKCFG_ST` is immediately followed by a valid gzip member
and `IKCFG_ED`, so an ordinary `IKCFG_ST` string is not a config. Offsets with
`space: "inflated_payload"` are positions in an inflated bzImage payload, not
file offsets. An absent config describes only the searched bytes; the kernel
may have been built without `CONFIG_IKCONFIG` or with it as a module. Truncated
payloads are incomplete, not absent. A framed blob that does not decode, has a
bad CRC, or holds non-config text is `malformed`, not absent. xz, lz4, lzma, zstd, bzip2, and lzo
payloads are unsupported and name `external_decompression`: decompress with an
external utility, then call `inspect_kernel_build_config` on the result. See
[Linux kernel build config](https://github.com/morluto/rea/blob/main/docs/guides/native/kernel-build-config.md).

## CPython pyc files

Use `inspect_python_pyc` for an explicit local CPython 3.7–3.13 `.pyc` when the
question concerns the header magic, PEP 552 source hash or timestamp, nested
code-object names, or bytecode identity. No `open_binary`, Python installation,
or target execution is required. The CLI equivalent is `inspect-python-pyc PATH`.
Preserve artifact SHA-256, the 16-byte header fields, nested code names, and
bytecode SHA-256. Bytecode is not decompiled. A decoded header does not prove
the matching `.py` is present. See [CPython pyc inspection](https://github.com/morluto/rea/blob/main/docs/python-pyc.md).

## Erlang BEAM files

Use `inspect_erlang_beam` for an explicit local `FOR1`/`BEAM` file when the
question concerns IFF chunks, atom names, exports, imports, lambdas, or the
Code header. No `open_binary`, Erlang installation, or bytecode execution is
required. The CLI equivalent is `inspect-erlang-beam PATH`. Preserve artifact
SHA-256, chunk file ranges, 1-based atom indices, and original atom spellings.
Missing optional tables stay null. For a few facts, use `detail: "summary"` with
a `beam` view selecting a collection, exact names/indices and fields; read later
pages from `parent_evidence_id` or the complete Evidence bundle. Same-name
exports retain every arity and original atom index. CLI summary delivery requires
`--evidence_output`. Null table counts remain distinct from decoded empty tables. See
[Erlang BEAM files](https://github.com/morluto/rea/blob/main/docs/erlang-beam.md).

## SQLite snapshots

Use `inspect_sqlite_database` with an explicit local database snapshot for its
schema and optional rows from one exact ordinary table. It requires Node.js
24.x >=24.15 or 26+ with the native `DatabaseSync.limits` API. It also consumes the
existing sibling `-wal`; retain both original digests when citing its result.
Default output is a compact inventory; a selected table expands only its own
schema and samples 20 rows. Select `columns` to avoid irrelevant or large cells,
and request `schema_scope: "database"` only for complete schema details.
Choose `row_limit` when a finite sample is useful and keep truncation and
unspecified row order explicit. Virtual tables, shadow tables and views are
schema facts, not row-read targets. REA inspects private copies and leaves
the source files untouched; obtain a coherent quiescent database/WAL snapshot
before inspection. See [SQLite snapshots](https://github.com/morluto/rea/blob/main/docs/guides/data/sqlite-databases.md).

## Recorded Linux crashes

Use `inspect_recorded_crash` with an explicit Linux x86-64 ELF core path to
inspect historical thread registers, signals, raw notes and original file
ranges. It is independent of the active disassembler target. Source-bound
register values and note bytes are observations; missing notes, signal-thread
association and current executable/library identity remain unknown. PIDs are
historical metadata and never authorize live attach or control.

The optional `include_debugger_context` facet adds mapping candidates through
caller-supplied GDB/pwndbg in an owned core-only session. Display names do not
establish current file identity; zero reported flags leave permissions unknown.
Requested unavailable context fails with setup guidance. See
[recorded crashes](https://github.com/morluto/rea/blob/main/docs/guides/native/recorded-crashes.md)
for exact upstream profiles, bounds and verification coverage.

## Offline ELF layout

Use `inspect_binary_layout` with an explicit local ELF path for file-backed
sections, symbols, relocations, linkage names, and static mitigation inferences
without opening a disassembler target. When that complete record is already
retained, call `inspect_analysis_view` for a summary, mitigations or linkage
facet, one section or symbol, or a stable page. Do not re-run the decoder to
read a single object.

## Offline WASM

Use `inspect_wasm_artifact` with caller-supplied WABT for selected local bytes.
For counts, validation and WAT identity, select `detail: "summary"` on the first
MCP call. The complete original Evidence stays retained; use
`normalized_result.parent_evidence_id` for exact exports or text pages. Omitted
or complete detail keeps the existing complete result.
For a completed result, use `inspect_analysis_view` with its reported retained
reference for a summary or `view: {kind: "wasm", facet: "exports", offset: 0, limit: 32}`.
Facets also include sections, imports, candidates, glue, WAT, headers, details
and tool profile. Text offsets count UTF-16 code units; follow `next_offset`.
The complete source remains retained after an oversized MCP reply. No target
execution or network acquisition is implied, and WAT is decoded representation.

## Offline APK signatures

Use `inspect_apk_signature` for a selected local APK/ZIP/AAB. Default MCP summaries
retain complete Evidence. Select `view.collection: signers/digests/lineage`,
with scheme or ID batches, fields and paging; use `inspect_analysis_view` kind
`apk-signature` for retained reads after deletion. IDs retain artifact/scheme
scope and signer links; `fields: []` keeps identity links. Empty pages preserve
partial decoding reasons and unknown authenticity. CLI compact delivery needs
`--evidence_output` to preserve the complete parent.
It reads the APK
Signing Block (v2/v3/v3.1) and v1 `META-INF` signing without executing anything,
and reports each signer's certificate facts (for v1, the certificate the PKCS#7
block's SignerInfo names), the declared v3 rotation lineage,
the declared source-stamp certificate and, for SHA-256/SHA-512 digests, whether
the declared content digest matches the current bytes. That match is not a
signature verification: anyone who changes the bytes can recompute the digest.
`posture` summarises v1-only signing (which cannot tell a v1-only APK from one
whose v2/v3 block was removed), observed signer agreement across schemes,
whether the v1 signer equals the block-scheme signer (`false` is notable),
whether every differing signer appears in a declared (unverified) rotation
lineage, and the strongest scheme.
It never claims authenticity: no signature or lineage is cryptographically
verified.

## Native targets

After `open_binary`, use focused search, procedure, or function tools directly.
Use `binary_overview` when target metadata or inventory context helps answer the
question. Prefer literal search, names, decompilation, callers, callees, and
cross-references. Addresses and recovered pseudocode are analysis observations,
not original source. Provider unavailability and unsupported metadata remain
unknown rather than false.

For large pseudocode, request `procedure_pseudo_code` with `detail: "summary"`
on the first MCP call. It retains the complete observation and returns the
original selection, text availability and length without the body. Use
`normalized_result.parent_evidence_id` with `inspect_analysis_view` and
`view: {kind: "native", facet: "pseudocode", offset: 0, limit: 20000}`;
follow `coverage.next_offset` for later pages. Choose a smaller limit if one
page exceeds the client or transport budget. Offsets and limits count UTF-16
code units, not lines or tokens. Pages reuse the same observation without
repeating decompilation. Null and empty text remain distinct. Omitted or
`complete` detail preserves full inline delivery. Summary delivery requires a
retaining session and does not reduce the work or memory of decompilation.

For a completed `analyze_function` dossier, use `inspect_analysis_view` with
`view: {kind: "native", facet: "value_flow_operations", offset: 0, limit: 32}`
and the exact retained Evidence ID to read selected facts without reanalysis.
The default MCP reply is already a summary of that dossier: its
`normalized_result.parent_evidence_id` is the retained ID to page, and its
null counts mark facets the provider did not record or declared unavailable.
Native facets also include procedure, pseudocode, assembly, basic blocks,
comments, callers/callees, references, unresolved calls, referenced strings and
names, the native API record, and value-flow summaries. Pseudocode offsets and limits count UTF-16
code units; use the returned `next_offset` for continuation. Provider limitations
and unavailable facts remain explicit; the complete parent Evidence is retained.

`inspect_macho` on a large framework returns megabytes of imports and exports.
Select `detail: "summary"` for slice identity, UUID and per-collection counts,
then page rows with `view: {kind: "macho", facet: "exports", offset: 0, limit: 64}`
and `normalized_result.parent_evidence_id`. Facets are entrypoints,
architectures, build_metadata, load_commands, dependencies, imports, exports,
segments and provenance. `source_exhaustive: false` means unreturned symbols
may exist.

To see which functions or Objective-C methods a Mach-O actually calls in one
run, use `observe_native_calls` with explicit breakpoints and a bounded
`duration_ms`/`max_events`. It launches a new process under LLDB, so the target
runs and may change files or use the network. Hardened-runtime targets need the
`get-task-allow` entitlement.
Function breakpoints use exact LLDB runtime names: pass an `inspect_macho` export
or import `runtime_name` (`main`), not its symbol-table `name` (`_main`). REA does
not translate breakpoint names. Set `module` to the image
file name when selecting that image only; otherwise loaded system images also
match. Inspect retained locations and module paths before attributing a call.

Use `binary_session` with no arguments to check the open target, selected
provider, and alignment. Its default summary `result.tool_availability` carries
the complete tool inventory grouped by current status. Expand each group's
`names` and inherit the inventory-level client requirements unless a group
overrides them; availability, reasons, remediation, and modes remain inline.
Provider candidates keep availability and target-support status, code, and
reason. `omitted_fields` names what the summary leaves out; `{"detail":
"complete"}` adds each unavailable candidate's `remediation`, provider
diagnostics, candidate capability sets, and tool surface, `effects`, and
`annotations`. Those arrays name true flags from fixed
vocabularies, so an omitted known flag is false. `tools/list` remains the
canonical source for full tool schemas when target or provider state changes.

Provider capability groups carry provider identity and ordered limitations.
Prepend each set's `shared_limitations` to a group's `limitations` to recover the
operation's full ordered list. Candidate capabilities, returned with complete
detail, describe possible providers. Top-level session capabilities describe the current route, and
`analysis_provider_binding` identifies the selected provider.

## Continue editable Ghidra analysis across processes

For a task spanning agent sessions, check `binary_session` after selecting
Ghidra: the current provider's `capabilities[].shared_limitations` states whether
Program checkpoints are enabled or the session is ephemeral. Successful native
queries distinguish initial analysis from an actually restored packed Program;
do not infer that restoration happened merely because checkpoint mode is enabled.

In ephemeral mode, choose a private absolute directory and set
`REA_GHIDRA_CHECKPOINT_DIR` in the environment starting REA. Restart the MCP server
after changing that launch configuration; no current tool call or runtime
analysis-setting update enables it. Stock ELF/PE/Mach-O imports on Linux/macOS
are supported; see the [checkpoint setup and limitations](https://github.com/morluto/rea/blob/main/docs/getting-started/installation.md#automatically-checkpoint-an-owned-ghidra-program).

With the setting enabled, normal opens restore compatible saved Programs
automatically. Initial completed analysis is saved, and `close_binary` saves
later metadata edits; no separate save/restore tool is needed. Repair a reported
save failure and retry close. Cancellation or process failure can lose edits
after the last successful save; unfinished initial analysis cannot resume.
Keep the same checkpoint configuration for later processes. Evidence/Unknown
history and immutable REA query snapshots have separate persistence workflows.

## Reuse caller-selected Ghidra analysis

When the caller supplies an expanded project, configure `REA_GHIDRA_PROJECT_PATH`
(the `.gpr` marker) and optionally `REA_GHIDRA_PROGRAM_NAME` (one root program),
then restart/reconnect before `open_binary`. CLI uses the same configuration.
REA opens only a fingerprint-bound owned copy with `-process -noanalysis -readOnly`;
original project files are not opened by the engine or saved. Database edits,
import overrides and repair scripts are unavailable in this mode. Treat original
analysis completeness/settings as unknown and prior edits as caller-project state.
The program's recorded import digest must match the target; changed project content
requires reopening for a fresh profile. See the
[setup and preparation guide](https://github.com/morluto/rea/blob/main/docs/getting-started/installation.md#reuse-an-existing-ghidra-project).

## Ghidra startup deadline recovery

`provider_timeout` during the first Ghidra query can cover import, auto-analysis,
bridge startup, and readiness. The default 330,000 ms is configurable through
`REA_GHIDRA_STARTUP_TIMEOUT_MS`; a smaller query still needs the same cold import.

When the error names `ghidra.startup_timeout_ms`, call
`update_analysis_settings` with `action: "set"` and a larger value within the
reported bounds. Resolve retained cleanup before the next startup attempt.
No REA restart is needed; an already accepted startup keeps its original
budget. Follow the error's session-specific restart guidance, and preserve useful Evidence before closing
and reopening the target when remediation requires that lifecycle step.

For a per-function timeout, update `ghidra.decompile_timeout_seconds` and retry
the same function on the existing Program. Explicit `null` removes that
per-function deadline. Inspection is optional when the error supplies the
setting and bounds. Runtime updates disappear when REA exits; environment
changes only seed future REA processes.

The client's tool-call deadline must allow startup plus the requested query;
server settings and progress notifications cannot override it. CLI analysis
avoids the MCP client deadline, but retains the provider deadline. For example:

```sh
REA_GHIDRA_STARTUP_TIMEOUT_MS=1800000 rea analyze /absolute/path/to/app.exe --provider ghidra --json
```

Thirty minutes is an example budget, not a completion guarantee. Keep host CPU
and memory controls in place. Disable specific expensive analyzers only when
their missing coverage is acceptable and retain the resulting limitations.
Exported C and embedded resources remain useful independent evidence, but a
timeout does not establish complete analysis or absence of code. See the
[deadline and client guide](https://github.com/morluto/rea/blob/main/docs/reference/mcp-contracts.md#ghidra-first-query-deadlines-and-recovery).

## Bridge request ceiling

Hopper and Cutter share `REA_BRIDGE_MAX_REQUEST_BYTES`. Unset, both sides use
16777216 bytes. The payload budget excludes the terminating LF. A decimal
integer from 7464 to 536870887 raises or lowers that ceiling. The floor fits
the largest bounded symbol-name command. A tight Cutter status still returns
its session id and up to 64 completion receipts. Its path is included when it
fits, and is unknown when it does not, so an older descriptor path is not
paired with the new generation. The maximum is the encoded payload, one byte under Node's string ceiling so
the framing LF still fits. Cutter measures, signs, and writes a fitting
response in bounded chunks. The bridge searches only the newly read bytes
for the framing LF. The client unescapes each socket chunk into one response
string before parsing it, so this ceiling does not keep the paged wire frame
and another concatenated copy. Invalid values are configuration errors and do not fall back to 16777216. `bridge.max_request_bytes` can be changed at runtime with `update_analysis_settings`. The launch seed is `REA_BRIDGE_MAX_REQUEST_BYTES`. Start Hopper or Cutter from that same environment so the bridge keeps the ceiling it started with until that bridge starts again. An out-of-band bridge
without the setting stays at 16777216 bytes. The client reports that mismatch
during discovery only after the hello proves a live authenticated bridge, and
does not send a status request or the command. A stale descriptor is ignored.
Discovery fails if any such live ceiling differs, even when another live
session matches. A command response discarded for that ceiling stays an
unknown execution with `output_truncated`. A hello
that never sends its authenticated frame does not hold the accept loop.
At most 8 connection threads run; a further accept is closed, and a failure
to start a thread does not stop the listener. `completed_connection` is the
longest prefix of connections that have terminated. Dense scalar replies are
encoded in bounded batches.
The 25000 ms Cutter request deadline and the 256-candidate discovery budget
still have no runtime override.

## Selected native facts and call graphs

For a large `get_call_graph`, or repeated `procedure_address`,
`procedure_callees`, `procedure_callers` and `resolve_containing_procedure`
queries, select `detail: "summary"` when the first 64 recorded observations
suffice. Read further `native-observation` pages from the returned parent Evidence
ID. Graph message IDs index that page's exact-text dictionary; keep level,
source index, failed nodes, and partial/unknown relationship coverage.
Read referenced source context through `native-context` facets `overview`,
`analysis-profile`, `limitations` and `result-limitations`. Reuse inspected
context/profile values only for the same digest; until caveats are read,
interpretation beyond the returned facts remains unknown. Complete Evidence
remains retained and exportable. Small complete replies can cost fewer tokens
than additional context/page calls; count the completed task and discovery.

## Static memory and original file bytes

For Ghidra, `read_bytes` reads initialized analysis memory, including loader
fixups; it does not read a running process. Check `returned_bytes` and `complete`
before decoding a table. An incomplete read is missing coverage, not a zero-filled
tail. Use `list_segments` to inspect the selected block's initialization and address-space
facts, and retain unavailable facts as unknown.

Use `address_to_file_offset` to anchor one observed address to original file
bytes. A mapping for the first address does not establish a contiguous mapping
for an entire table, and loader fixups can make analysis bytes differ from source
bytes. Keep source ranges and analysis-memory observations separate.

A pointer table can refer to writable slots populated by a constructor rather
than directly to immutable resources. An uninitialized block or an observed zero
does not establish the slot's later runtime value. Follow `xrefs` and the relevant
writer's assembly or pseudocode; distinguish recorded assignments from inferred
execution order. Do not replace unreadable bytes with zeros or conclude that a
resource is absent without the necessary initialization evidence.

## DOS MZ with Ghidra

For an admitted DOS MZ target, use `inspect_native_load_image` to check measured
loaded bytes, file mappings, relocations and the entry against the immutable
snapshot. Keep `mismatch` and `unsupported` explicit. Verified import covers the
reported static image, not runtime DOS or PC-98 hardware behavior.

For a large load-image report, select `detail: "summary"` and query its
`parent_evidence_id` with `inspect_analysis_view` kind `native-load-image`.
Choose a collection such as `relocations` or `mappings`, select only needed
fields, and page with offset/limit. Original indices preserve same-address
alternatives. Null collections remain unrecorded or inapplicable; paging does
not upgrade unsupported verification. CLI requires `--evidence_output` for
summary delivery so complete source Evidence remains accessible.

For Ghidra decompilation, `procedure_pseudo_code` also accepts `procedure: "file-offset:0x..."`; CLI `rea decompile <path> --file-offset=0x... --provider ghidra` resolves the source byte offset internally. An unresolved 0x-prefixed numeric procedure selector also falls back to a unique file mapping. Existing symbols and analysis addresses win; multiple mappings fail as ambiguous. Preserve the returned file-offset and address locations and raw selection observations. ELF virtual addresses are distinct from file offsets; never assume an arbitrary offset plus image base is an analysis address.

Complete function body ranges use inclusive ends and can contain gaps. Do not
treat the enclosing span as code.
For packed targets, retain the original and separately derived artifact identities;
decompiling an unpacking stub does not recover the unpacked program.

When reading Objective-C dispatch metadata, distinguish class implementation
methods from protocol requirements: class methods report null requirement
flags, while decoded protocol declarations retain required and optional facts.
Property attributes remain raw name/value tokens; use the property's
`atomicity` field for the decoded `N` marker or the default atomic behavior.
Null class/meta flags, ivar counts, atomicity, or read-only values mean the
selected provider did not establish that fact. Symbol names alone do not prove
that a class is a root class or has no ivars.

## Managed PE/CLI

Start with `inspect_managed_artifact`. REA's canonical managed inspection is
execution-free: do not claim it loaded, reflected, executed, or resolved the
assembly. Keep managed/native boundaries and unavailable reconstruction facts
explicit. A bring-your-own reconstruction oracle is separate from the canonical
parser and must not become an implicit setup dependency.

For identity/classification questions, `inspect_managed_artifact` can explicitly
select `detail: "summary"` on the first MCP call. It retains complete metadata
and returns a linked overview with full classification/evidence, nullable
module/MVID, frameworks, metadata coverage/issues/limitations and returned-row
counts. Default complete requests remain unchanged. Read original rows with
`inspect_analysis_view`, source `normalized_result.parent_evidence_id`, and
`view: {kind: "managed", collection: "attributes", offset: 0, limit: 16}`.
Collections are `references`, `resources` and `attributes`. Paging does not
repeat parsing. Returned-row exhaustion
does not prove complete metadata or absence under partial/unavailable coverage;
preserve build-local tokens and their artifact/MVID scope.

`inspect_managed_members` always returns the assembly-wide `member_refs`, even
for one selected type. For questions about a selected type, select
`detail: "summary"` with `type_names`; it returns the selection (with unmatched
names), coverage and per-collection counts. Page rows with
`view: {kind: "managed-members", collection: "call_edges", offset: 0, limit: 64}`
over `types`, `exported_types`, `fields`, `methods`, `member_refs`, `call_edges`,
`field_accesses` or `user_strings`.
Use exact `names` and `declaring_types` filters before paging supported
collections. For a compact method lookup, request `fields: ["name",
"declaring_type", "signature"]`; omit `body` to avoid anchors. Identity tokens,
original `source_index`, module/MVID and source coverage remain. Check
`unresolved_filter_rows` before interpreting an empty match as absence.

### .NET NativeAOT

NativeAOT binaries contain native machine code, so ordinary CIL decompilers
cannot recover the original method bodies. When managed metadata/CIL is absent,
classify that as a native-analysis route rather than claiming source recovery.
Open the native image with the selected deep provider and inspect functions,
strings, calls, and references; describe decompiler output as pseudocode and
inference.

NativeAOT images can be PE/COFF, ELF, or Mach-O executables and shared
libraries. Use file bytes and loader metadata rather than suffixes to identify
them. For packaged apps, inventory/extract with REA's artifact tools and retain
the exact contained image identity. PDB, ELF debug, or dSYM sidecars are
additional evidence only when the provider accepts and matches them.

REA has a built-in read-only snapshot parser for x64 PE targets with .NET 8 RTR
9.1 metadata. An optional headless Ghidra adapter adds database annotations and
broader metadata recovery on Linux; it requires an explicitly configured
NativeAOT JAR. These are separate routes. The built-in parser also runs on
Windows Ghidra P0; read frozen literals from its report, because
`search_strings` does not include them. See the
[NativeAOT guide](https://github.com/morluto/rea/blob/main/docs/providers/ghidra-nativeaot.md)
for setup, supported layouts, and host restrictions. Preserve binary identity
and distinguish derived metadata from Ghidra database observations. Recovered
types and native pseudocode are not original C# source. Windows Ghidra P0 has
no database mutation authority.

## Packages and extraction

Use `inspect_artifact` for application bundles, archives, ZIP/APK/IPA/MSIX/AppX,
ASAR, or DMG inputs when the artifact graph and findings help answer the
question. It returns the complete artifact graph inline. Cite graph manifest
IDs when using them.

For an IPA or a macOS `.app`, ZIP, or DMG, pass the inventory Evidence to
`project_apple_application_graph`. It reports application roots and nested
bundles by path convention: app extensions, XPC services, frameworks, login
items, system and driver extensions, and plug-ins. It also lists privileged
helpers, launchd plists, symlinks, and each bundle's `info_plist_path` and
executable candidates. Roles are path conventions, not parsed plists; read the
listed plists with `inspect_plist`.

After `open_binary` on a `.app` or Mach-O, use `trace_dylib_resolution` to see
which file each `@rpath`, `@loader_path` and `@executable_path` load reaches for
every executable in the bundle. It also lists missing or weak loads and earlier
`@rpath` candidates that are absent. Narrow large bundles with `roots` or
`architecture`. System paths stay undetermined because the dyld shared cache
provides them.

Use `extract_artifact` when materialized files from a supported format are needed.
It extracts the active target and accepts optional `integrity_policy`; see the [MCP contracts](https://github.com/morluto/rea/blob/main/docs/reference/mcp-contracts.md).
It materializes all regular files into a fresh temporary directory chosen by
REA. Symlinks and encrypted entries are inventory facts, not extractable files.
The result reports the materialized directory; callers do not choose the
destination. No separate permission grant is needed.

For classic iNES cartridges, `open_binary` recognizes the byte signature without
an executable architecture. `inspect_artifact` retains header facts and each
PRG/CHR file unit's offset, length and SHA-256; its nested inventory's
`ines.reconstruction` verifies the actual ordered region bytes against the
whole-file digest. Extraction includes the original header, optional trainer
and uninterpreted trailing bytes. These file units are not mapper/CPU bank
maps. NES 2.0, UNIF, dirty legacy headers and truncated payloads are explicitly
refused. No emulator or 6502 analysis is performed. See the
[iNES guide](https://github.com/morluto/rea/blob/main/docs/ines-containers.md).

Native DMG traversal automatically uses a read-only mount on supported macOS
hosts, with an owned temporary mount directory and cleanup. It needs no approval
flag. Unsupported hosts or mount failures remain explicit limitations; do not
claim child inventory when only root identity is available. DMG extraction is
unavailable through `extract_artifact`. To access DMG files on macOS, create a
read-only mount with your own tools, select files from that mount for analysis,
and detach the mount you created when finished. REA's inspection mount is
temporary, not a retained export of the image's contents.

For headerless DOS COM, explicitly open with `format: "dos-com"` (CLI
`--target-format dos-com`). Inspect the load image before trusting function
analysis: the entry is `0x10100`, file offset zero, with imposed real-mode segment
context. PSP/stack/device state remains unmodeled. See [DOS guide](https://github.com/morluto/rea/blob/main/docs/providers/ghidra-dos.md).

Use `inspect_byte_entropy` to triage high- and low-entropy ranges before extraction.

For other headerless images (embedded firmware without ELF/Mach-O/PE/MZ
headers), open with `format: "raw-binary"`, `architecture` (`x86`, `x86_64`,
`arm`, `arm64`, `6502`, or `m68k`), and `entry_point` (`0x` hex). File offset 0 is mapped at
that address. Hopper and IDA refuse this interpretation; Ghidra uses
`BinaryLoader` with a generic language: big-endian for `m68k`, little-endian
otherwise. Big-endian ARM, Thumb-only, and Cortex-M variants are unsupported.
`6502` selects NMOS 6502; the entry and complete file mapping must fit within
`0x0000..0xffff`. 65C02, 65816, cartridge containers, bank switching, and mapper
semantics are unsupported. `m68k` requires an even entry and decodes the 68040
superset of the 68000 family; Amiga/Atari containers and vector-table entry
discovery are unsupported. Windows Ghidra P0 admits raw images only for `6502`. See [raw binary guide](https://github.com/morluto/rea/blob/main/docs/providers/raw-binary.md).

Intel HEX, S-record, UF2 and DfuSe files are containers, not raw images:
`inspect_artifact` lists address regions (UF2 per familyID, DfuSe per element
with its alternate setting), and `extract_artifact` writes each region. Open an
extracted region as `raw-binary` with its start address as `entry_point`. UF2
numbering gaps leave coverage unknown; a DfuSe suffix CRC mismatch follows
`integrity_policy`. See [UF2 and DfuSe](https://github.com/morluto/rea/blob/main/docs/uf2-dfuse.md).

### Function annotations in Ghidra

Fresh-import sessions admit annotations; existing-project reuse is read-only.
Use `annotate_native_function` for one function name and/or entry comments, with at least one explicit change. Review its annotation readback and refreshed dossier inline. Changes are atomic; empty comments clear them, omitted fields preserve them. Later MCP calls observe edits. Normal close saves them when owned Program checkpoint mode is enabled; ephemeral mode discards them. CLI `annotate-native-function` follows the same setting when closing after its updated result. Select the [checkpoint workflow](#continue-editable-ghidra-analysis-across-processes) before edits that must survive a later process. Original executable bytes are unchanged; edits invalidate immutable snapshots. Windows P0 does not admit database mutations. For naming only, `set_address_name` / `set_addresses_names` also work with Ghidra: they name function entries or data addresses (primary labels) and return compact per-address success booleans instead of a dossier. Ghidra rejects batch addresses that request the same name or a name another selected address holds; `raw_result.failures` gives each failure's reason.

A provider switch or different target can retire the active Ghidra database and discard
its edits, even if the new provider fails on a later call. Same-target,
same-profile reopen keeps the active database. Retained annotation Evidence
records what happened; it does not restore the edits.

For `compare_functions`, obtain `analyze_function` Evidence before and after the
edit while the MCP session is open; annotation Evidence is not a comparison
input. Export the needed Evidence before close if comparison will happen later.
Authenticated same-artifact coordinates preserve renamed entries as
`matched / same-entry`, with the canonical `entry_address` inline; name and
exact-text changes remain separate dimensions. This requires compatible
architecture, provider-owned mapping and selected-image commitments. Equal
numeric addresses alone cannot establish correspondence; inspect retained
source Evidence and coverage when the match remains ambiguous.

## Offline PE resource inventory

Use `inspect_pe_resources` with an absolute `path`, or
`rea inspect-pe-resources ./selected.exe --json`, for portable PE32/PE32+
resource inspection without opening a native target. Preserve returned Evidence:
type/name/language IDs and UTF-16 names, original directory/data/payload ranges,
code pages, payload hashes and static icon-group references. Same-language icon
resolution is explicit; missing references and size mismatch remain visible.
Other payloads are opaque, and this operation does not decode images or simulate
Windows resource loading. Consult the budgets and supported tree profile in
[offline binary diagnostics](https://github.com/morluto/rea/blob/main/docs/guides/native/binary-diagnostics.md).

## Offline PE and ELF export tables

Use `inspect_native_exports` with an absolute `path`, or
`rea inspect-native-exports ./libexample.so --json`, to read a PE export
directory or ELF dynamic symbol table on any host without opening a native
target. Ordinal-only, forwarded and hidden-version exports stay distinct;
partial or unsupported coverage means the inventory is incomplete, not that
exports are absent. Use `inspect_macho` for Mach-O. See
[offline binary diagnostics](https://github.com/morluto/rea/blob/main/docs/guides/native/binary-diagnostics.md).

For exact core-module import/export differences, use `compare_wasm_interfaces`
with two complete inspection Evidence records (or current-session retained
references) and explicit scope. Preserve unknown types/duplicate names; static
interface equality is not runtime compatibility. CLI accepts complete inline
Evidence in `rea compare-wasm-interfaces input.json --json`, or exact references
with `--evidence-bundle /absolute/path/evidence.json`. Use the complete parent
ID when the inspection returned a summary.

## Godot PCK

For a standalone PCK, `open_binary` then `inspect_artifact` with
`detail: "summary"` retains the complete static inventory. Use the returned
`parent_evidence_id` with `inspect_analysis_view` inventory occurrences filtered
by exact `logical_paths` or `occurrence_ids`, and join `artifact_id` to nodes
filtered by `artifact_ids`. Select only needed fields; identity links remain.
Prefer `inspect-artifact --detail summary --evidence_output PATH` for CLI tasks;
the same CLI discovers embedded packs via their EOF trailer without opening a
native analysis provider. Normal native opening remains unchanged.

Distinguish declared MD5 from observed SHA-256 and graph/page completeness from
`source_coverage`. Encrypted directories and sparse bundles have unknown entry
coverage; encrypted members have unknown content identity. An empty filtered
page cannot prove absence in unknown coverage. Retained views are historical,
not fresh filesystem checks. No decryption, Godot resource decoding, GDScript
decompilation or target execution is provided. See
[Godot PCK](https://github.com/morluto/rea/blob/main/docs/godot-pck.md).

UnityFS AssetBundle containers (`.bundle`, `.ab`, YooAsset packages and other
byte-identified files) list `CAB-*` SerializedFiles and `.resS` members with
header and node facts; none/LZ4/LZ4HC member blocks stream on demand. Bundles
declaring asset-bundle encryption keep header facts with unknown directory
coverage; LZMA member bytes retain named occurrences with unavailable hashes.
Virtual member offsets are not physical carrier locations. Format 6–8 readers
cache one decoded block and support retained filtered/paged views. No
decryption or SerializedFile object decoding is provided. See
[UnityFS](https://github.com/morluto/rea/blob/main/docs/unityfs.md).

WeChat mini-program packages (`.wxapkg` files with `0xBE`/`0xED` marks) list
members with declared names and absolute carrier offsets; member bytes are
stored verbatim. `V1MMWX` PC-client packages report `encrypted` directory
status with unknown coverage and are refused on extraction; no decryption is
attempted. See
[wxapkg](https://github.com/morluto/rea/blob/main/docs/wxapkg.md).

U-Boot legacy uImages use the same inspection/extraction workflows. Prefer a
retained summary and selected inventory views for header facts or component
IDs. Header addresses/CPU/OS are declarations; payloads remain as stored.
Multi-file components retain their aligned physical ranges; the full payload
is also materialized. Unknown name text retains its original 32 bytes. See
[U-Boot uImage](https://github.com/morluto/rea/blob/main/docs/uimage.md).

## ESP images

For an ESP32-family app or bootloader image, `open_binary` then
`inspect_artifact` with `detail: "summary"`. `root_format_metadata` holds the
observed `header` and `descriptor` (its `kind` is the app or bootloader role)
and the derived `chip`, `checksum`, `appended_sha256` and
`undescribed_trailing` ranges. Segment rows (`segment-NN.bin`) carry
`load_addr`, `classification` (`mapped`, `loaded`, `not-loaded` or `unknown`,
per ESP-IDF `should_map`/`should_load`; RAM ranges are not checked) and
`source_location`; select them with `inspect_analysis_view` and
`logical_paths`. Signature blocks are `unverified`; `public_key_sha256`
matches eFuse key digests.

Unknown `source_coverage` means truncation or undescribed trailing bytes, such
as the flash segments of a `--ram-only-header` image; do not infer absence
from it. Erased flash is described. Integrity mismatches are facts, not
refusals. ESP8266 images are refused, and flash-encrypted images are not
recognized. Segments are not disassembled, and RISC-V or Xtensa segments
cannot be opened as raw-binary. See
[ESP images](https://github.com/morluto/rea/blob/main/docs/esp-image.md).

## ESP flash images

A whole-flash dump opens as `esp-flash-image`. `root_format_metadata` holds
`partition_table` (its `method`: `default`, `caller` or a derived
`scan`), `bootloader` and `apps` ESP image facts, the `otadata` records,
`partitions_outside_dump` and `boot_selection`. Every table has a
`boot_selection`: its `basis` names the bootloader's branch (`no-otadata`
without otadata, `none` when the bootloader rejects the table, `unknown`
when a record is missing), and it lists its `assumptions`;
`running_partition` is always `unknown`. Each image names its entry
`path`. With `detail: "summary"` the root kind is `esp-flash-image-summary`
and each image is a compact `summary` (chip, entry point, segment count,
classified load ranges, signature block count, descriptor identity,
integrity); complete detail keeps every segment. Partition rows
(`partitions/<label>.bin`) carry the declared `type`, `subtype`, names,
`flash_address`, `size` and flags; `encrypted` is a storage flag, not
proof of ciphertext. `written_offset`/`written_end` bound the bytes that
are not erased: an app slot whose facts say `erased` can still hold an older
image, and a limitation says so. For a dump read from another flash address, call
`open_binary` with `format: "esp-flash-image"` and `flash_offset`; an
`esp-image` whose `flash_dump` is set names that offset in a next probe; a
relocated table can be named with `partition_table_offset`. No table found
means unknown, often flash encryption. Extraction writes `nvs_keys` as
ordinary data. See
[ESP flash images](https://github.com/morluto/rea/blob/main/docs/esp-flash-image.md).

## Bun standalone executables

A `bun build --compile` executable stores its module graph in Mach-O
`__BUN,__bun`, a PE or ELF `.bun` section, or an appended trailer.
`open_binary` then `inspect_artifact` reports root format `bun-standalone` with
the carrier, structural layout, module count, entry point, raw flags and
compile exec-argv; each module keeps its `/$bunfs/...` source name, loader,
encoding, sizes and SHA-256, with sourcemap, bytecode and module-info regions as
separate children. Use summary root metadata `entry_point_logical_path` as the
exact inventory `logical_paths` selector, then read the selected content node by
its artifact ID; the reader owns virtual-name normalization and indexed fallback. `extract-artifact` writes stored contents through the safe
output tree; route extracted JavaScript to `analyze_javascript_application`.
Layout labels describe record shapes, not a detected compiler version; unknown
or ambiguous layouts are unsupported. Bytecode is retained, not decoded, and the
executable is never run. See
[Bun standalone executables](https://github.com/morluto/rea/blob/main/docs/bun-standalone.md).

## Deno compile executables

A `deno compile` executable stores its application in a Mach-O `__SUI,d3n0l4nd`
section, an Intel Mach-O `<~sui-data~>` trailer, an ELF `SUI` note of type
`0x53554901`, or a PE `RT_RCDATA` resource named `D3N0L4ND`.
`open_binary` then `inspect_artifact` reports root format `deno-standalone`
with the carrier, entry point, recorded options, and one occurrence per virtual
file, including size and SHA-256. Transpiled output, CJS export analysis, and
source maps are separate occurrences with the file's `index` and their own
`role`. They normally use a `.deno-*` suffix on the file path and move to an
indexed path on a collision, so use the reported `logical_path`. Symlink targets
stay recorded path parts. A virtual filesystem that cannot be listed leaves
coverage unknown.
Environment-file values and CA certificate bytes are retained in local Evidence
with their counts and byte lengths. The npm snapshot length is reported and not
decoded. `extract-artifact` writes
stored files through the safe output tree; route extracted JavaScript to
`analyze_javascript_application`. Other layouts are unsupported, and the
executable is never run. See
[Deno compile executables](https://github.com/morluto/rea/blob/main/docs/deno-standalone.md).
