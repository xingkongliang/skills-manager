# Connection and task recovery

Read this guide only for an observed connection, command, approval or provider
blocker. Keep the user's target and question in view. Explain what prevented the
operation and the next useful step; after repair, continue the investigation.

Commands below use an installed `rea`. If none is available, use the package
invocation in `SKILL.md` when acquiring REA is authorized. Follow the connected
server's schemas and the installed CLI's help rather than assuming repository
main and an older release expose the same arguments. Tool-name prefixes belong
to the client: find the canonical REA operation in its actual tool listing and
server association before concluding that it is absent. A shared prefix alone
does not establish which server an approval wildcard covers. See
[client tool names and discovery](https://github.com/morluto/rea/blob/main/docs/getting-started/client-configuration.md#client-tool-names-and-discovery).

## Choose the remedy from the observed failure

| Observation                                                     | Meaning and next step                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| --------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| No REA tools in the active session                              | If REA was installed as a client plugin, follow the plugin section below. Otherwise inspect registration with `rea doctor --client codex --json`. A skill installation alone does not register MCP.                                                                                                                                                                                                                                                                                                                                                                                     |
| Missing, malformed or stale registration                        | Prepare a scoped setup plan; apply only authorized changes with backups.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| Aligned registration, but no connected tools                    | Restart/reconnect the client. Doctor checks registration; it cannot establish the current agent's connection. Inspect the client's launch error if reconnection fails.                                                                                                                                                                                                                                                                                                                                                                                                                  |
| MCP connection dropped or REA could not start                   | If the connection drops, retry that call once only when it is safe to repeat or known not to have started. If it may already have changed the target, or the same failure returns, tell the user REA's connection failed and ask them to restart the MCP client. If REA could not start its connection, retry once, then tell the user and ask them to restart the MCP client. Do not retry a startup failure that returns the same unchanged result. Run `rea doctor` when REA could not start or tools stay missing after restart. Doctor cannot restore a live connection by itself. |
| `MCP tool call requires approval, but approval policy is never` | Codex rejected the call before REA executed it. Do not install an engine or repeat setup to solve this client-policy decision. See the approval section below.                                                                                                                                                                                                                                                                                                                                                                                                                          |
| REA returns `provider_unavailable` or an unavailable tool entry | Read that operation's provider diagnostics or `binary_session.result.tool_availability`. Expand each group's tool names and inherit the shared client requirements unless overridden. Use the operation's availability, reason, and remediation; a complete tool catalog does not imply every operation is available. Request `{"detail": "complete"}` only for provider diagnostics.                                                                                                                                                                                                   |
| `rea` is missing in a child shell, or Node is incompatible      | Inspect the actual shell PATH and launcher/runtime. A prefix installation may be absent from a login shell's PATH; this does not establish a broken MCP registration.                                                                                                                                                                                                                                                                                                                                                                                                                   |

If the client cannot launch its shell/sandbox helper or read the installed skill,
inspect its launch diagnostic. For example, Codex can refuse helper aliases when
CODEX_HOME is under `/tmp`; use an authorized profile outside the OS temporary
directory. REA setup cannot repair the client's executor. Preserve the sandbox
instead of silently disabling it to make an analysis command work.

Static JavaScript and ordinary ZIP inventory need neither Hopper nor Ghidra.
HAR inspection is offline and does not require a live browser. Native
decompilation, Android analysis and firmware extraction have their own
bring-your-own prerequisites. Use supported inventory/static evidence when it
answers part of the question, and name the unavailable evidence separately.
Do not substitute another target. Label observations from ordinary shell tools
as such; only actual REA output supplies REA Evidence.

## Distinguish client and server deadlines

A host message such as `timed out awaiting tools/call` with no REA result means
the client's deadline expired before a result arrived. It does not establish
that REA finished, canceled successfully, or released all resources. Do not
repeat the unchanged analysis. Inspect available session/provider state and
retained Evidence only when useful; an idle `binary_session` is not proof that
every target-free operation stopped. Never invent a missing Evidence reference.

Allow the host tool deadline to cover execution, cancellation/cleanup, and
response delivery. In Codex, the setting is
`mcp_servers.<server-id>.tool_timeout_sec` in **seconds**, not
`startup_timeout_sec`. Select a value for the actual operation budget and
headroom, not a universal timeout. See the
[official MCP configuration reference](https://learn.chatgpt.com/docs/extend/mcp?surface=cli).
Apply configuration changes only within the authorized scope, preserving
unrelated entries and backups; reconnect/restart as required by the host.
When configuration is not editable, report the setting needed or use an
authorized CLI fallback. Do not silently broaden permissions or switch targets.

A returned REA `provider_timeout` instead means the **server/provider** reached
its deadline. Follow its `remediation.action`, reuse accessible partial Evidence,
and resolve reported incomplete cleanup before retrying. Change a request
field or a `REA_*` setting only when the returned remediation explicitly
recommends that change. Restart REA when that text says the value is read at
startup. A mention that a control does not raise the deadline is not a
recommendation to change it. Do not invent a setting the remediation does not
name. Raising only the client deadline does not change
this server budget. For static JavaScript,
`analysis_timeout_ms` (CLI `rea analyze-javascript-application <path>
--analysis-timeout-ms <milliseconds>`) controls each source's
startup, analysis and transfer deadline in **milliseconds**. Increase that
budget or reduce semantic work / select exact inventoried `source_paths` when
the task permits. Omit `source_paths` for all sources; `[]` is invalid.
`detail: "summary"` reduces delivery, not analysis work. A per-source budget
does not bound the elapsed time for an entire multi-source application.
Partial-Evidence inspection can avoid recomputation; an unavailable reader is
not itself a reason to block a corrected static-analysis retry.
The shorthand `rea analyze <path>` does not expose this timeout flag; use the
explicit JavaScript command when changing that budget.

For invalid arguments, use the connected tool's current input schema, or the
installed CLI's command help. Refresh the catalog if stale; do not guess fields
from an older skill or return the entire catalog to repair one field. Correct
all reported fields before retrying.

## Recovery ladder

Follow the returned remediation. Change a request field or a `REA_*` setting
only when that text explicitly recommends the change. Restart REA when it says
the value is read at startup. A mention that a control does not raise a limit
is not a recommendation to change it. Do not invent a setting. Host memory and
`NODE_OPTIONS` do not raise a REA admission unless that remediation says so.

- MCP connection dropped: retry that call once only when it is safe to repeat
  or known not to have started. If it may already have changed the target, or
  it fails again, tell the user REA's connection failed and ask them to restart
  the MCP client. Do not run `rea doctor` for the drop itself.
- REA could not start its connection: retry once, then tell the user and ask
  them to restart the MCP client. Run `rea doctor` when REA could not start or
  tools stay missing after that restart. Do not retry a startup failure that
  returns the same unchanged result.
- `cleanup_incomplete`, or retained cleanup owners in the error details: retry
  that exact cleanup owner. Do not start new work that needs those resources
  until that owner confirms release. Repeating the original call is not that
  retry.
- Capacity, when the result is `resource_constraint` and `retryable` is true:
  wait for the active admission to finish, then retry within the reported
  limits. Do not restart REA to free the slot.
- `configuration_invalid`: when the error reports `update_mode: "runtime"`,
  call `update_analysis_settings` for that setting. When it reports
  `update_mode: "parameter"`, change that call's argument; the setter does not
  accept it. Otherwise correct the named setting in the environment that starts
  REA (the shell or the MCP registration) and start REA again. Run `rea doctor`
  to see the `runtime-configuration:` check. Re-run `rea setup` for that client
  when the registration must carry the new setting, then reconnect.
- `invalid_request`: correct the reported fields from the current schema. Do
  not repeat the unchanged call.

## REA installed as a client plugin

A Claude Code, Codex, or Cursor Marketplace plugin owns its `rea` server and
this skill. Doctor's `--client` and `--skill` checks and `rea setup` read and
write only setup-owned state, so for a plugin-only install they report
`missing`, and applying setup would add a second `rea` server. If this skill
was loaded from a plugin, or the client lists a `rea` plugin, do not prepare a
setup plan for missing tools. Instead:

- Check that the plugin is installed and enabled with the client's own
  listing: `claude plugin list` and `/mcp` in Claude Code, `codex plugin list`
  in Codex, and Cursor's plugin settings for a Marketplace install.
- Restart or reconnect the client, then read its launch error for the `rea`
  server if tools are still absent. Claude Code and Codex start the plugin's
  bundled runtime; a missing build or version mismatch requires a reinstall,
  or a fresh build when developing from source. A Cursor Marketplace install
  starts `npx` at the version pinned in the plugin's `mcp.json`. A failed
  download needs network access to that published version.
- Check analysis engines through the connected server's
  `binary_session.result.tool_availability`. `rea doctor --provider <id> --json`
  from a shell reports that shell's environment, which can differ from the
  environment the client gave the plugin server. If you still run it through
  the package runner, use the version pinned in the plugin manifest's server
  entry (`rea-agents@<version>`), not `@latest`, so it matches the server the
  plugin starts.

Use the setup repair below only when the user wants a setup-owned registration
instead of the plugin, and remove or disable the plugin first.

## Repair registration when needed

For Codex:

```bash
rea doctor --client codex --json
rea setup --client codex --dry-run --json
```

Use the current supported client's ID: `claude_code`, `claude_desktop`, `codex`,
`cursor`, `gemini_cli`, `windsurf`, `devin`, `opencode`, `antigravity`,
`copilot_cli`, `commandcode`, `qwen_code`, `vscode`, `trae`, `grok_build`, `omp`, `pi`,
`hermes`, `qoder`, `droid`, `zcode`, `dsh`, `freebuff`, or `grok_bot`. If unknown, inspect `doctor --json` before choosing a
scope. `grok_bot` uses a chat registration step rather than local `mcp.json`.
For unsupported clients, use manual stdio registration with the selected
installation’s absolute runtime and entry script, or continue through the CLI.
Acquire packages from a trusted directory outside the target project. Keep a
package-runner installation and its runtime available while registrations use it.

Show the plan's exact paths, backups, changes and bundled-skill replacement.
Obtain approval for writes when that concrete scope is not already authorized,
then apply the same scope with `rea setup --client codex --yes`. Add
`--install-hopper` only when separately needed and explicitly approved. Do not
install or upgrade unrelated prerequisites. Restart/reconnect, verify tools
actually appear, and resume the original task. If they remain absent, inspect
the launch failure rather than looping through setup.

## Codex tool approval is separate from REA readiness

REA tools have no permission-grant handshake. Codex can still require approval
because an operation creates owned resources, launches a helper or has cleanup
effects. An advertised tool or successful doctor result does not override that
client policy. `workspace-write` alone does not resolve an MCP approval
rejection, and `approval_policy = "never"` does not mean every MCP tool is
preapproved.

When interactive approval is available, use the client's approval flow for the
requested operation. In an unattended run, continue through an authorized CLI
workflow if the execution environment permits it, or report the blocked
operation and the approval needed to proceed. Do not retry the same rejected
call, silently broaden approvals, or rewrite MCP effects as read-only.

For an operator who explicitly wants to preapprove named operations in a
disposable Codex profile, current Codex supports per-tool configuration. For
example, opening and closing an analysis session can be scoped independently:

```toml
[mcp_servers.rea.tools.open_binary]
approval_mode = "approve"

[mcp_servers.rea.tools.close_binary]
approval_mode = "approve"
```

Use the actual server ID and only the operations whose effects were approved;
this example does not approve inspection, extraction or runtime execution.
Show the proposed config edit and preserve unrelated entries and backups before
writing. Managed policy can constrain client settings. Consult the
[Codex configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference)
for the installed client's supported settings. This is an optional recovery
choice, not a prerequisite for every REA task.

## Repair an editable REA limit

If `details.limit` names an editable setting and its bounds, call
`update_analysis_settings` directly with a `changes` entry containing `setting`,
`action: "set"` and `value`. `inspect_analysis_settings` is optional when those
facts are already present. Retry the same input after work and cleanup settle.
Runtime defaults affect new work only; explicit per-call arguments take
precedence and must be corrected directly when selected. Reset restores launch
values. Updates survive target close/open but disappear on REA restart. They do
not edit host deadlines, response buffers, registrations or fixed limits.

## Continue through an installed CLI

Check the command environment only if the launcher fails:

```bash
command -v rea
command -v node
node --version
rea --version
```

Use the public launcher at its known installation path with a supported existing
Node on PATH when the child shell loses a prefix installation. Avoid internal
`dist/cli.js` entrypoints or global toolchain changes. MCP may have its own valid
launcher even when a shell cannot resolve `rea`.

For a user-supplied JavaScript tree or ASAR, select output files in the authorized
workspace and save complete Evidence before reading a view:

```bash
rea analyze-javascript-application /absolute/path/to/app --json > app-evidence.json
jq -c '{source: {kind: "inline", evidence: .}, view: {kind: "summary"}}' app-evidence.json > app-view.json
rea inspect-analysis-view app-view.json
```

These commands do not register MCP or install an analysis engine. CLI JavaScript
JSON is complete and can be very large; redirect it rather than loading it into
context. Use an existing JSON processor if `jq` is absent, without installing
one just for this recipe. Inspect a module page or item using the same inline
source and the [JavaScript recipes](javascript-applications.md). Preserve graph
coverage, limitations and unknowns. Native CLI analysis still requires its
selected engine, and all CLI operations retain the host's execution permissions.
