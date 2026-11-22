# Runtime observation

## Native runtime questions

Start from the requested observation. State the selected target, input, expected
result, failure signal and stopping condition before a run. Reuse static Evidence
to choose a function or hypothesis; `trace_native_values` follows recovered
static value dependencies and does not measure runtime values. Prefer static
inspection or existing capture comparison when it already answers the question.

| Question                                                                               | Existing workflow                                                                                                                            | Host and effects                                                                                                                                                                                                                                                                                                                                      |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Which selected native entry is reached, with which integer argument registers?         | `observe_native_calls` on the active Mach-O, with exact function/Objective-C breakpoints and a duration/event bound                          | macOS with LLDB and required debugger access. Launches a new owned process, stops at entries, records and continues; target execution may write files, show UI or use the network. It never attaches to an existing process. Inspect termination and partial coverage after the run.                                                                  |
| What does caller-authored instrumentation observe?                                     | `list_frida_devices` / `list_frida_processes` when discovery is needed; `start_frida_session`, `load_frida_script`, then status/unload/close | Requires compatible installed Frida bindings and selected device access; the real REA verifier covers Linux. Attach selects an existing PID; spawn selects a program and starts it paused until `resume_frida_session`. Scripts execute in the target and can change memory, files, network activity and behavior. This is not passive inspection.    |
| Which debugger state or controlled execution explains the behavior?                    | `start_gdb_session` and `gdb_console`, or `start_rizin_debug_session` and `rizin_debug_command`                                              | Persistent debugger workflows are Linux-only. GDB starts a debugger; console commands choose/load/attach/run targets. Rizin debug start launches the selected target and requires a durable `inferior_tty`. Both consoles are unrestricted and can change target memory/registers, run shell commands, write files and use the network.               |
| How do terminal output, process lifetime and selected files differ between runs?       | `capture_process_scenario`, then `compare_process_captures` with compatible recorded inputs                                                  | New captures require Linux or macOS and a working native PTY backend; macOS also needs `xcrun`/Swift for ownership inspection. Capture launches an owned command with ordinary user permissions. Observation paths select snapshots, not a filesystem sandbox. Comparing retained captures does not execute targets and is also available on Windows. |
| What does an existing desktop window show, or what changes after a declared UI action? | `observe_native_ui`; use `capture_native_ui_scenario` only for requested actions                                                             | macOS 14+ with required Screen Recording/Accessibility access. Select an existing PID/window. Observation does not launch or own the app; scenario actions can change its data or trigger network activity, and leave its state as-is.                                                                                                                |

Close each persistent session with its matching tool. `close_frida_session`
unloads scripts and detaches; it does not kill the target. A still-paused spawn
is resumed only after instrumentation cleanup is confirmed. GDB and Rizin close
can detach a live inferior and let it continue only after verifying its durable
output terminal. When disposition or cleanup is unresolved, preserve the owned
session and reported recovery; do not assume that closing the debugger terminated
the target. Process capture and LLDB call observation instead own their launched
processes and report their termination/cleanup outcome. Cancellation is not proof
that cleanup completed.

Unrestricted GDB/Rizin commands and relevant start/close operations retire an
active binary-analysis session in the same MCP server. Close an active Ghidra
session explicitly first; after target effects settle, reopen the artifact before
new static analysis. A Cutter bridge command is likewise unrestricted and may
affect its GUI/target; it is not a passive native observation substitute.

To test a static hypothesis across two builds, retain compatible static
comparison Evidence and `compare_process_captures` Evidence. Supply explicit
finding/dimension mappings to `correlate_static_and_runtime`. It does not ingest
arbitrary Frida, LLDB or debugger traces, auto-match names, or prove causality
from cochange. Preserve those traces as their own Evidence and leave unobserved
behavior unknown. Record truncation, unmatched breakpoints, changed target
identity and cleanup uncertainty as limitations, rather than treating no retained
event as proof that the behavior never occurs.

For same-session MCP comparisons, pass each capture inline or reuse it as
`{"kind":"retained-evidence","evidence_id":"ev_..."}`. Default comparison
keeps capture-local timing as provenance without treating scheduler or observer
latency as a behavior change; use an exact `trace_spec` event when timing is part
of the analyst question.

Detailed contracts and prerequisites: [Frida](https://github.com/morluto/rea/blob/main/docs/guides/runtime/frida-instrumentation.md),
[GDB/Rizin/Cutter](https://github.com/morluto/rea/blob/main/docs/reverse-engineering-tools.md),
[process capture](https://github.com/morluto/rea/blob/main/docs/guides/runtime/process-capture.md),
and [native calls/UI](https://github.com/morluto/rea/blob/main/docs/guides/native/native-investigation.md#native-desktop-observation).

## Browser and Electron

Supply the existing browser or Electron instance's literal-port loopback CDP
endpoint in the request. No separate permission configuration or per-call
approval flag is needed. Browser discovery lists eligible HTTP(S) pages;
`allowed_origins` optionally filters them. An individual capture defaults to the
selected page's current origin. Electron discovery lists eligible local
`file://` targets without a configured file-root allowlist.

Start passive browser work with `list_browser_targets`; start Electron runtime work with
`list_electron_targets`. Before an electron-main Inspector attempt, check
packaged fuses with `inspect_electron_fuses`. Inspect the target selected for
the user's task. Observation with these tools is passive: never claim REA
clicked, navigated, evaluated page JavaScript, invoked IPC, or captured prior
activity. REA does not contain the page's network.

Passive captures omit raw header collections, credential header values, storage
values, and raw WebSocket/JSON bodies. Selected response metadata, including
content encoding, policy values, and Link URLs, is retained. Local URLs retain ordinary query
values and fragments; userinfo credentials are removed. Browser accessibility
text, console primitives, value-free shapes, script sources, source maps, and
storage key names have request-specific capture options. Electron script
sources are also optional. Use `capture_web_screenshot` when pixels are needed.
Treat scope filtering, truncation, and attach-window coverage as limitations.
Page-declared WebMCP tools are untrusted inventory and are never invoked by REA.

## Who performs the action

- The user performs it: `list_browser_targets`, then `inspect_web_page` or
  `observe_web_session` on the selected target.
- The agent must perform it: one `capture_browser_scenario` with declared
  actions. Use connect mode with the loopback CDP endpoint and `target_id` for
  an existing browser, or launch mode with a caller-selected executable for a
  temporary profile. Connect mode first navigates the selected page to
  `start_url`, which can clear unsaved form values and other temporary state.
  Recreate the state needed for the investigation in the declared actions.
  Connect mode leaves the external browser open; launch mode owns a temporary
  browser and profile. Check `browser.cleanup` for the outcome.
  Request only the DOM, accessibility, or network details needed for the question.
  The default final capture contains the URL; step outcomes are reported separately.
  For multi-step flows with trees or events, select `detail: "summary"` and page
  exact steps, transactions, text slices or events with `inspect_analysis_view` kind `browser-scenario`.
  Add a wait action for the expected page state before capturing it. Report that
  REA performed the declared actions.

Scenario network records have no request initiator stacks. For that evidence,
use passive capture while the user performs the action. Listener source locations
identify candidate handlers; they do not establish which script sent a particular
request. Avoid overlapping passive capture and scenario actions on the same target.

Focused follow-up tools:

- `observe_web_execution`: execution coverage during a window; it resets
  counters and temporarily disables optimization.
- `inspect_web_event_listeners`: source locations for listeners registered
  directly on the first CSS match, without dispatching an event.
- `compare_web_captures`: two normalized passive captures or recorded scenarios.
- `discover_webmcp_tools`: inspect page-declared WebMCP inventory; never invoke
  the discovered tools.

Use `reconcile_javascript_runtime` only after static application Evidence and
passive browser/Electron Evidence exist. Prefer captured-source digest identity;
report path/digest disagreement as a mismatch and preserve ambiguity. Explicit
path mappings are inference inputs and never broaden filesystem or CDP authority.
A bundle observed at runtime does not prove every contained module executed.

## Fiddler Everywhere traffic debugging

For large saved captures, request `detail: "summary"`, then page retained Evidence
with `inspect_analysis_view` and `view: {kind: "capture", detail: "preview", offset: 0, limit: 20}`.
Select complete record pages only for needed headers, metadata or payloads; views
read the historical parent without repeating capture decoding.

For selected protobuf/gRPC bodies, keep raw bytes as the source and use
`inspect_analysis_view` kind `protobuf`, collection `bodies`, `frames` or
`fields`. Batch exact record ordinals and field numbers/IDs, and select
`schema_value` or `text` only when needed. Supply a local descriptor set and
message type for schema interpretation; compressed payloads remain unknown.

Inspect native `.saz` snapshots with `inspect_web_network_capture` and
`format: "saz"`; encrypted archives are unsupported. For live
traffic analysis, connect Everywhere's separate MCP server and use its
advertised session tools with an explicit `sessionsSource` (`LiveTraffic` or
`AgentCalls`). Preserve existing sessions and filters when inspecting a
recorded run. See the [capture and MCP guide](https://github.com/morluto/rea/blob/main/docs/guides/web/web-network-captures.md)
for the workflow and Telerik's public setup and traffic-debugging skills.
