//! MCP (Model Context Protocol) server over stdio.
//!
//! Every command the CLI already understands is exposed as an MCP tool, so an
//! agent that speaks MCP can manage the library without the `manage-skills`
//! skill and without resolving the binary path first. There is one
//! implementation of each operation: a tool call re-runs this same binary with
//! `--json` and returns its stdout, so the MCP surface cannot drift from what
//! `skills-manager-cli` actually does.
//!
//! Design notes:
//!
//! - **A child process per call.** The command handlers print their result and
//!   take `&SkillStore`; calling them in-process would write JSON into the
//!   protocol stream and hold the store open for the life of the server. A
//!   short-lived child is the smallest correct wrapper, and it means a
//!   long-running server never pins the database or a repo lock.
//! - **Read the CLI's own safety rails.** Destructive tools pass through
//!   `--yes`/`--dry-run` rather than inventing a second policy. A refusal comes
//!   back as `isError: true` with the CLI's error envelope, which names the
//!   path or the flag that would let the call succeed.
//! - **No new dependencies.** The transport is newline-delimited JSON-RPC 2.0,
//!   which is a `read_line` loop plus `serde_json`.

use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};

/// Newest protocol revision this server implements. Older clients that ask for
/// a revision we also understand get that one back (see [`negotiate`]).
pub const PROTOCOL_VERSION: &str = "2025-06-18";

const SERVER_NAME: &str = "skills-manager";

const INSTRUCTIONS: &str = "\
Skills Manager owns the user's shared agent-skill library and the per-agent \
deployments made from it. Prefer these tools over writing into an agent's \
skills folder: they keep the source, tags, preset membership and update state \
intact. Every result is the CLI's JSON output. Arguments are validated against \
each tool's schema; the tools that take `dry_run` are the ones whose schema \
lists it, and `skills_remove`, `tags_delete` and `presets_delete` refuse \
without `yes: true`.";

/// Run the stdio server until stdin closes.
pub fn serve(skills_root: Option<PathBuf>) -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut invoke = |args: &[String]| run_cli(skills_root.as_deref(), args);

    for line in stdin.lock().lines() {
        let line = line.context("reading an MCP message from stdin")?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle_message(&message, &mut invoke),
            Err(err) => Some(rpc_error(
                Value::Null,
                -32700,
                &format!("could not parse JSON: {err}"),
            )),
        };
        if let Some(response) = response {
            writeln!(stdout, "{}", serde_json::to_string(&response)?)
                .context("writing an MCP message to stdout")?;
            stdout.flush().context("flushing stdout")?;
        }
    }
    Ok(())
}

struct CliOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

/// The child's arguments. `--skills-root` and `--json` are global flags, so
/// they go before the subcommand; clap accepts them there and the order is
/// what keeps a tool call reading the same library the parent was pointed at.
fn child_args(skills_root: Option<&Path>, args: &[String]) -> Vec<OsString> {
    let mut child: Vec<OsString> = Vec::with_capacity(args.len() + 3);
    if let Some(root) = skills_root {
        child.push("--skills-root".into());
        // OsString, not display(): a non-UTF-8 library path must reach the
        // child exactly as the CLI received it.
        child.push(root.as_os_str().to_os_string());
    }
    child.push("--json".into());
    child.extend(args.iter().map(OsString::from));
    child
}

fn run_cli(skills_root: Option<&Path>, args: &[String]) -> Result<CliOutput> {
    let exe = std::env::current_exe().context("resolving the skills-manager-cli path")?;
    let output = Command::new(exe)
        .args(child_args(skills_root, args))
        .stdin(Stdio::null())
        .output()
        .context("running skills-manager-cli")?;
    Ok(CliOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

/// Handle one decoded message. `None` means "do not reply" — that is every
/// notification, and JSON-RPC forbids a response to those.
fn handle_message(
    message: &Value,
    invoke: &mut dyn FnMut(&[String]) -> Result<CliOutput>,
) -> Option<Value> {
    let id = message.get("id")?.clone();
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));

    match method {
        "initialize" => Some(result(id, initialize_result(&params))),
        "ping" => Some(result(id, json!({}))),
        "tools/list" => Some(result(id, json!({ "tools": tool_definitions() }))),
        "tools/call" => Some(call_tool(id, &params, invoke)),
        _ => Some(rpc_error(id, -32601, &format!("unknown method: {method}"))),
    }
}

fn initialize_result(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    json!({
        "protocolVersion": negotiate(requested),
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

/// Echo a revision we implement, and answer anything else with ours.
fn negotiate(requested: Option<&str>) -> &'static str {
    match requested {
        Some("2024-11-05") => "2024-11-05",
        Some("2025-03-26") => "2025-03-26",
        _ => PROTOCOL_VERSION,
    }
}

fn call_tool(
    id: Value,
    params: &Value,
    invoke: &mut dyn FnMut(&[String]) -> Result<CliOutput>,
) -> Value {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return rpc_error(id, -32602, "tools/call needs a `name`");
    };
    let no_args = Map::new();
    let args = params
        .get("arguments")
        .and_then(Value::as_object)
        .unwrap_or(&no_args);

    let Some(spec) = tools().into_iter().find(|tool| tool.name == name) else {
        return rpc_error(id, -32602, &format!("unknown tool: {name}"));
    };
    if let Err(err) = validate_args(&spec.schema, args) {
        return rpc_error(id, -32602, &err.to_string());
    }

    let argv = match tool_argv(name, args) {
        Ok(argv) => argv,
        Err(err) => return rpc_error(id, -32602, &err.to_string()),
    };

    match invoke(&argv) {
        Ok(out) if out.success => tool_result(id, &out.stdout, false),
        Ok(out) => {
            // The CLI writes its error envelope to stderr. Prefer it over
            // stdout, which is empty on failure, but fall back to stdout for
            // the rare command that fails after printing.
            let text = if out.stderr.is_empty() {
                out.stdout
            } else {
                out.stderr
            };
            tool_result(id, &text, true)
        }
        Err(err) => tool_result(id, &format!("{err:#}"), true),
    }
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

fn tool_result(id: Value, text: &str, is_error: bool) -> Value {
    let text = if text.trim().is_empty() {
        if is_error {
            "skills-manager-cli failed without a message"
        } else {
            "{}"
        }
    } else {
        text
    };
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{ "type": "text", "text": text }],
            "isError": is_error,
        },
    })
}

// ── tools ─────────────────────────────────────────────────────────────────

struct ToolSpec {
    name: &'static str,
    description: &'static str,
    schema: Value,
}

impl ToolSpec {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": self.schema,
        })
    }
}

fn tool_definitions() -> Vec<Value> {
    tools().iter().map(ToolSpec::to_json).collect()
}

fn tools() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "repo_status",
            description: "Report the library path, database, metadata directory, skill and preset counts, and the active preset.",
            schema: schema(props(vec![]), &[]),
        },
        ToolSpec {
            name: "repo_set_path",
            description: "Move the library to another directory. Returns the new status; the move is recorded and the old location is left alone.",
            schema: schema(
                props(vec![("path", s("Absolute destination for the library"))]),
                &["path"],
            ),
        },
        ToolSpec {
            name: "repo_reset_path",
            description: "Drop a pending library move and go back to the recorded location.",
            schema: schema(props(vec![]), &[]),
        },
        ToolSpec {
            name: "agents_list",
            description: "List every known agent with its skills directory, whether it is installed on this machine, and whether it is enabled.",
            schema: schema(props(vec![]), &[]),
        },
        ToolSpec {
            name: "agents_enable",
            description: "Enable one or more agents so they appear in workspaces and receive deployments.",
            schema: schema(
                props(vec![("agents", arr("Agent keys, e.g. [\"claude-code\", \"codex\"]"))]),
                &["agents"],
            ),
        },
        ToolSpec {
            name: "agents_disable",
            description: "Disable one or more agents. Existing deployments are not removed.",
            schema: schema(
                props(vec![("agents", arr("Agent keys"))]),
                &["agents"],
            ),
        },
        ToolSpec {
            name: "agents_add_custom",
            description: "Register an agent this app does not know by default, with its global and optional project skills folder.",
            schema: schema(
                props(vec![
                    ("key", s("Unique agent key, e.g. hermes-work")),
                    ("path", s("Global skills folder, e.g. ~/.hermes/profiles/work/skills")),
                    ("name", s("Display name; defaults to the key")),
                    ("project_path", s("Project-relative skills folder, e.g. .hermes/skills")),
                ]),
                &["key", "path"],
            ),
        },
        ToolSpec {
            name: "skills_list",
            description: "List library skills, optionally filtered by free text, tags, preset, agent deployment, source, or missing tags/preset.",
            schema: schema(
                props(vec![
                    ("query", s("Free text over name, description and path")),
                    ("tags", arr("Only skills carrying every one of these tags")),
                    ("preset", s("Preset id or name")),
                    ("deployed_to", s("Only skills deployed to this agent key")),
                    ("untagged", bool_s("Only skills with no tags")),
                    ("no_preset", bool_s("Only skills in no preset")),
                    ("source", s("Filter by source type or source value")),
                ]),
                &[],
            ),
        },
        ToolSpec {
            name: "skills_show",
            description: "Show one skill's metadata, files, tags, preset membership and per-agent deployment state.",
            schema: schema(
                props(vec![("reference", s("Skill id, name, directory basename or central path"))]),
                &["reference"],
            ),
        },
        ToolSpec {
            name: "skills_export",
            description: "Copy a skill out of the library to a destination folder.",
            schema: schema(
                props(vec![
                    ("reference", s("Skill id, name, directory basename or central path")),
                    ("dest", s("Destination directory")),
                    ("force", bool_s("Overwrite the destination when it already exists")),
                ]),
                &["reference", "dest"],
            ),
        },
        ToolSpec {
            name: "skills_install",
            description: "Install a skill from a local folder, a Git URL, or skills.sh (owner/repo[@skill] or owner/repo/skill). The kind is detected when omitted.",
            schema: schema(
                props(vec![
                    ("reference", s("Local path, Git URL, or skills.sh ref")),
                    ("kind", enum_s(&["local", "git", "skillssh"], "Force how the reference is read")),
                    ("name", s("Override the installed skill name")),
                    ("sync", bool_s("Add to the active preset and sync enabled agents")),
                    ("sync_preset", s("Add to this preset (id or name) and sync enabled agents")),
                ]),
                &["reference"],
            ),
        },
        ToolSpec {
            name: "skills_update",
            description: "Update one skill or every skill from its source. Local sources are re-imported; Git sources are compared with upstream.",
            schema: schema(
                props(vec![
                    ("reference", s("Skill id, name, directory basename or central path")),
                    ("all", bool_s("Update every skill")),
                ]),
                &[],
            ),
        },
        ToolSpec {
            name: "skills_check",
            description: "Check one skill or all of them for upstream changes without writing anything.",
            schema: schema(
                props(vec![
                    ("reference", s("Skill id, name, directory basename or central path")),
                    ("all", bool_s("Check every skill")),
                    ("force", bool_s("Ignore the cached update-check result")),
                ]),
                &[],
            ),
        },
        ToolSpec {
            name: "skills_remove",
            description: "Delete skills from the library. Refuses without `yes: true`; use `dry_run` to see what would be deleted. Deployments are removed through the sync engine, never by deleting paths directly.",
            schema: schema(
                props(vec![
                    ("references", arr("Skill refs to delete")),
                    ("yes", bool_s("Confirm the deletion")),
                    ("dry_run", bool_s("Report what would be deleted and change nothing")),
                ]),
                &["references"],
            ),
        },
        ToolSpec {
            name: "skills_deploy",
            description: "Deploy library skills to one or more agents' global skill directories.",
            schema: schema(
                props(vec![
                    ("references", arr("Skill refs to deploy")),
                    ("agents", arr("Target agent keys")),
                    ("dry_run", bool_s("Report the plan and change nothing")),
                ]),
                &["references", "agents"],
            ),
        },
        ToolSpec {
            name: "skills_undeploy",
            description: "Remove managed deployments of skills from one or more agents. Paths the app does not own are refused, not deleted.",
            schema: schema(
                props(vec![
                    ("references", arr("Skill refs to undeploy")),
                    ("agents", arr("Agent keys to remove them from")),
                    ("dry_run", bool_s("Report the plan and change nothing")),
                ]),
                &["references", "agents"],
            ),
        },
        ToolSpec {
            name: "skills_status",
            description: "Show one skill's preset membership and its actual per-agent deployment state.",
            schema: schema(
                props(vec![("reference", s("Skill ref"))]),
                &["reference"],
            ),
        },
        ToolSpec {
            name: "skills_sync",
            description: "Converge deployed skills to a preset's membership for one agent or all enabled agents.",
            schema: schema(
                props(vec![
                    ("preset", s("Preset id or name; defaults to the active preset")),
                    ("tool", s("Agent key; defaults to every enabled agent")),
                    ("dry_run", bool_s("Report the plan and change nothing")),
                ]),
                &[],
            ),
        },
        ToolSpec {
            name: "skills_search",
            description: "Search the skills.sh marketplace. Results include an install ref that `skills_install` accepts as-is.",
            schema: schema(
                props(vec![
                    ("query", s("Search text")),
                    ("limit", int_s("Maximum hits (1-300, default 60)")),
                ]),
                &["query"],
            ),
        },
        ToolSpec {
            name: "skills_set_source",
            description: "Re-point an installed skill at a Git source in place, keeping its id, tags, preset membership and deployments.",
            schema: schema(
                props(vec![
                    ("reference", s("Skill ref")),
                    ("git_url", s("Git URL or owner/repo, optionally a GitHub tree URL with branch and subpath")),
                    ("subpath", s("Subpath inside the repo; pass an empty string for the repo root")),
                    ("branch", s("Branch to track")),
                    ("force", bool_s("Overwrite the central copy when the content differs")),
                    ("dry_run", bool_s("Resolve and compare without writing")),
                ]),
                &["reference", "git_url"],
            ),
        },
        ToolSpec {
            name: "skills_adopt",
            description: "Adopt skills found in agent folders into the library, so they gain a source, update tracking and tags.",
            schema: schema(
                props(vec![
                    ("paths", arr("Agent skills folders, or one skill folder")),
                    ("git_url", s("Record the adopted skill as coming from this Git URL (single skill only)")),
                    ("git_subpath", s("Subpath inside that repo; pass an empty string for the root")),
                    ("dry_run", bool_s("Report candidates and change nothing")),
                ]),
                &["paths"],
            ),
        },
        ToolSpec {
            name: "tags_list",
            description: "List tags, or the tags on one skill.",
            schema: schema(
                props(vec![("reference", s("Skill ref; omit for every tag in the library"))]),
                &[],
            ),
        },
        ToolSpec {
            name: "tags_add",
            description: "Add tags to a skill, keeping the ones it already has.",
            schema: schema(
                props(vec![
                    ("reference", s("Skill ref")),
                    ("tags", arr("Tags to add")),
                ]),
                &["reference", "tags"],
            ),
        },
        ToolSpec {
            name: "tags_remove",
            description: "Remove tags from a skill.",
            schema: schema(
                props(vec![
                    ("reference", s("Skill ref")),
                    ("tags", arr("Tags to remove")),
                ]),
                &["reference", "tags"],
            ),
        },
        ToolSpec {
            name: "tags_set",
            description: "Replace a skill's whole tag list.",
            schema: schema(
                props(vec![
                    ("reference", s("Skill ref")),
                    ("tags", arr("The complete new tag list")),
                ]),
                &["reference", "tags"],
            ),
        },
        ToolSpec {
            name: "tags_rename",
            description: "Rename a tag everywhere it is used.",
            schema: schema(
                props(vec![
                    ("old_name", s("Current tag")),
                    ("new_name", s("New tag")),
                ]),
                &["old_name", "new_name"],
            ),
        },
        ToolSpec {
            name: "tags_delete",
            description: "Delete a tag from every skill. Refuses without `yes: true`; `dry_run` reports the affected skills.",
            schema: schema(
                props(vec![
                    ("name", s("Tag to delete")),
                    ("yes", bool_s("Confirm the deletion")),
                    ("dry_run", bool_s("Report what would change and change nothing")),
                ]),
                &["name"],
            ),
        },
        ToolSpec {
            name: "presets_list",
            description: "List presets with their skill counts and deployment state.",
            schema: schema(props(vec![]), &[]),
        },
        ToolSpec {
            name: "presets_current",
            description: "Show the active preset, if there is one.",
            schema: schema(props(vec![]), &[]),
        },
        ToolSpec {
            name: "presets_show",
            description: "Show one preset, its members and its per-agent status.",
            schema: schema(
                props(vec![("reference", s("Preset id or name"))]),
                &["reference"],
            ),
        },
        ToolSpec {
            name: "presets_create",
            description: "Create an empty preset.",
            schema: schema(
                props(vec![
                    ("name", s("Preset name")),
                    ("description", s("Optional description")),
                    ("icon", s("Optional icon key")),
                ]),
                &["name"],
            ),
        },
        ToolSpec {
            name: "presets_update",
            description: "Rename a preset or change its description or icon.",
            schema: schema(
                props(vec![
                    ("reference", s("Preset id or name")),
                    ("name", s("New name")),
                    ("description", s("New description")),
                    ("icon", s("New icon key")),
                ]),
                &["reference"],
            ),
        },
        ToolSpec {
            name: "presets_delete",
            description: "Delete a preset. Refuses without `yes: true`; skills themselves are kept.",
            schema: schema(
                props(vec![
                    ("reference", s("Preset id or name")),
                    ("yes", bool_s("Confirm the deletion")),
                    ("dry_run", bool_s("Report what would change and change nothing")),
                ]),
                &["reference"],
            ),
        },
        ToolSpec {
            name: "presets_preview",
            description: "Resolve a preset to the exact skill/agent pairs deploying it would produce.",
            schema: schema(
                props(vec![("reference", s("Preset id or name"))]),
                &["reference"],
            ),
        },
        ToolSpec {
            name: "presets_deploy",
            description: "Deploy a preset additively, without removing other deployed presets.",
            schema: schema(
                props(vec![
                    ("reference", s("Preset id or name")),
                    ("agents", arr("Agent keys; omit for every enabled agent")),
                    ("dry_run", bool_s("Report the plan and change nothing")),
                ]),
                &["reference"],
            ),
        },
        ToolSpec {
            name: "presets_undeploy",
            description: "Undeploy a preset's pairs without changing its membership.",
            schema: schema(
                props(vec![
                    ("reference", s("Preset id or name")),
                    ("agents", arr("Agent keys; omit for every deployed agent")),
                    ("dry_run", bool_s("Report the plan and change nothing")),
                ]),
                &["reference"],
            ),
        },
        ToolSpec {
            name: "presets_status",
            description: "Show a preset's membership and what is actually deployed for it, per agent.",
            schema: schema(
                props(vec![
                    ("reference", s("Preset id or name")),
                    ("agents", arr("Agent keys to narrow the report")),
                ]),
                &["reference"],
            ),
        },
        ToolSpec {
            name: "presets_add_skill",
            description: "Add skills to a preset. Membership is a one-time copy; deploying the preset is a separate call.",
            schema: schema(
                props(vec![
                    ("preset", s("Preset id or name")),
                    ("skills", arr("Skill refs to add")),
                ]),
                &["preset", "skills"],
            ),
        },
        ToolSpec {
            name: "presets_remove_skill",
            description: "Remove skills from a preset. Deployed copies are not undeployed by this call.",
            schema: schema(
                props(vec![
                    ("preset", s("Preset id or name")),
                    ("skills", arr("Skill refs to remove")),
                ]),
                &["preset", "skills"],
            ),
        },
        ToolSpec {
            name: "git_status",
            description: "Show the backup repository's status.",
            schema: schema(props(vec![]), &[]),
        },
        ToolSpec {
            name: "git_init",
            description: "Initialise the backup repository in the library.",
            schema: schema(props(vec![]), &[]),
        },
        ToolSpec {
            name: "git_clone",
            description: "Clone a backup repository into the library location.",
            schema: schema(
                props(vec![("url", s("Git remote URL"))]),
                &["url"],
            ),
        },
        ToolSpec {
            name: "git_set_remote",
            description: "Point the backup repository at a different Git remote.",
            schema: schema(
                props(vec![("url", s("Git remote URL"))]),
                &["url"],
            ),
        },
        ToolSpec {
            name: "git_pull",
            description: "Pull the backup remote, merging skill-aware.",
            schema: schema(props(vec![]), &[]),
        },
        ToolSpec {
            name: "git_push",
            description: "Push the backup repository to its remote.",
            schema: schema(props(vec![]), &[]),
        },
        ToolSpec {
            name: "git_commit",
            description: "Commit the current library state to the backup repository.",
            schema: schema(
                props(vec![("message", s("Commit message"))]),
                &["message"],
            ),
        },
        ToolSpec {
            name: "git_versions",
            description: "List restorable backup snapshots, newest first.",
            schema: schema(
                props(vec![("limit", int_s("Maximum snapshots to return"))]),
                &[],
            ),
        },
        ToolSpec {
            name: "git_restore",
            description: "Restore the library to a snapshot tag. This rewrites the library, so call it only on an explicit request.",
            schema: schema(
                props(vec![("tag", s("Snapshot tag from git_versions"))]),
                &["tag"],
            ),
        },
        ToolSpec {
            name: "git_prune_sync_refs",
            description: "Remove sync refs that a mirror/--all push uploaded to the backup remote, keeping local sync refs.",
            schema: schema(props(vec![]), &[]),
        },
    ]
}

fn s(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn bool_s(description: &str) -> Value {
    json!({ "type": "boolean", "description": description })
}

fn int_s(description: &str) -> Value {
    json!({ "type": "integer", "minimum": 1, "description": description })
}

fn arr(description: &str) -> Value {
    json!({ "type": "array", "items": { "type": "string" }, "description": description })
}

fn enum_s(values: &[&str], description: &str) -> Value {
    json!({ "type": "string", "enum": values, "description": description })
}

fn props(items: Vec<(&str, Value)>) -> Value {
    Value::Object(
        items
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

fn schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

// ── argument validation ───────────────────────────────────────────────────

/// Enforce the schema that `tools/list` advertises. Without this, a misspelt
/// `dry_run` (or `"dry_run": "true"`) is silently dropped and the call
/// mutates for real, which is exactly the direction a caller must not be able
/// to get wrong.
fn validate_args(schema: &Value, args: &Map<String, Value>) -> Result<()> {
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    for (key, value) in args {
        let Some(property) = properties.get(key) else {
            bail!("unknown argument `{key}`");
        };
        validate_value(key, property, value)?;
    }

    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for key in required.iter().filter_map(Value::as_str) {
            if !args.contains_key(key) {
                bail!("missing required argument `{key}`");
            }
        }
    }
    Ok(())
}

fn validate_value(key: &str, property: &Value, value: &Value) -> Result<()> {
    match property
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("string")
    {
        "string" => {
            let Some(text) = value.as_str() else {
                bail!("`{key}` must be a string");
            };
            if let Some(allowed) = property.get("enum").and_then(Value::as_array) {
                if !allowed.iter().any(|item| item.as_str() == Some(text)) {
                    let names: Vec<&str> = allowed.iter().filter_map(Value::as_str).collect();
                    bail!("`{key}` must be one of: {}", names.join(", "));
                }
            }
            Ok(())
        }
        "boolean" => {
            if value.is_boolean() {
                Ok(())
            } else {
                bail!("`{key}` must be a boolean")
            }
        }
        "integer" => {
            let Some(number) = value.as_u64() else {
                bail!("`{key}` must be a positive integer");
            };
            if let Some(minimum) = property.get("minimum").and_then(Value::as_u64) {
                if number < minimum {
                    bail!("`{key}` must be at least {minimum}");
                }
            }
            Ok(())
        }
        "array" => {
            let Some(items) = value.as_array() else {
                bail!("`{key}` must be an array of strings");
            };
            if items.iter().any(|item| !item.is_string()) {
                bail!("`{key}` must contain only strings");
            }
            Ok(())
        }
        other => bail!("`{key}` has an unsupported schema type `{other}`"),
    }
}

// ── argument mapping ──────────────────────────────────────────────────────

/// Translate a tool call into arguments for this binary. Kept separate from
/// [`serve`] so a test can inspect the argv without running anything.
fn tool_argv(tool: &str, args: &Map<String, Value>) -> Result<Vec<String>> {
    let a = Args(args);
    let mut argv: Vec<String> = Vec::new();

    macro_rules! words {
        ($($word:expr),+ $(,)?) => {
            argv.extend([$($word),+].into_iter().map(String::from))
        };
    }

    match tool {
        "repo_status" => words!("repo", "status"),
        "repo_set_path" => {
            words!("repo", "set-path");
            argv.push(a.required("path")?);
        }
        "repo_reset_path" => words!("repo", "reset-path"),

        "agents_list" => words!("agents", "list"),
        "agents_enable" => {
            words!("agents", "enable");
            argv.extend(a.required_list("agents")?);
        }
        "agents_disable" => {
            words!("agents", "disable");
            argv.extend(a.required_list("agents")?);
        }
        "agents_add_custom" => {
            words!("agents", "add-custom");
            argv.push(a.required("key")?);
            flag(&mut argv, "--path", a.required("path")?);
            optional(&mut argv, "--name", a.text("name"));
            optional(&mut argv, "--project-path", a.text("project_path"));
        }

        "skills_list" => {
            words!("skills", "list");
            optional(&mut argv, "--query", a.text("query"));
            for tag in a.list("tags")? {
                flag(&mut argv, "--tag", tag);
            }
            optional(&mut argv, "--preset", a.text("preset"));
            optional(&mut argv, "--deployed-to", a.text("deployed_to"));
            switch(&mut argv, "--untagged", a.flag("untagged"));
            switch(&mut argv, "--no-preset", a.flag("no_preset"));
            optional(&mut argv, "--source", a.text("source"));
        }
        "skills_show" => {
            words!("skills", "show");
            argv.push(a.required("reference")?);
        }
        "skills_export" => {
            words!("skills", "export");
            argv.push(a.required("reference")?);
            flag(&mut argv, "--dest", a.required("dest")?);
            switch(&mut argv, "--force", a.flag("force"));
        }
        "skills_install" => {
            words!("skills", "install");
            argv.push(a.required("reference")?);
            match a.text("kind").as_deref() {
                None => {}
                Some("local") => argv.push("--local".into()),
                Some("git") => argv.push("--git".into()),
                Some("skillssh") => argv.push("--skillssh".into()),
                Some(other) => bail!("`kind` must be local, git or skillssh, not `{other}`"),
            }
            optional(&mut argv, "--name", a.text("name"));
            switch(&mut argv, "--sync", a.flag("sync"));
            optional(&mut argv, "--sync-preset", a.text("sync_preset"));
        }
        "skills_update" => {
            words!("skills", "update");
            optional_positional(&mut argv, "reference", a.text("reference"))?;
            switch(&mut argv, "--all", a.flag("all"));
        }
        "skills_check" => {
            words!("skills", "check");
            optional_positional(&mut argv, "reference", a.text("reference"))?;
            switch(&mut argv, "--all", a.flag("all"));
            switch(&mut argv, "--force", a.flag("force"));
        }
        "skills_remove" => {
            words!("skills", "remove");
            argv.extend(a.required_list("references")?);
            switch(&mut argv, "--yes", a.flag("yes"));
            switch(&mut argv, "--dry-run", a.flag("dry_run"));
        }
        "skills_deploy" | "skills_undeploy" => {
            let verb = if tool == "skills_deploy" {
                "deploy"
            } else {
                "undeploy"
            };
            words!("skills");
            argv.push(verb.to_string());
            argv.extend(a.required_list("references")?);
            for agent in a.required_list("agents")? {
                flag(&mut argv, "--agent", agent);
            }
            switch(&mut argv, "--dry-run", a.flag("dry_run"));
        }
        "skills_status" => {
            words!("skills", "status");
            argv.push(a.required("reference")?);
        }
        "skills_sync" => {
            words!("skills", "sync");
            optional(&mut argv, "--preset", a.text("preset"));
            optional(&mut argv, "--tool", a.text("tool"));
            switch(&mut argv, "--dry-run", a.flag("dry_run"));
        }
        "skills_search" => {
            words!("skills", "search");
            argv.push(a.required("query")?);
            optional(
                &mut argv,
                "--limit",
                a.number("limit")?.map(|n| n.to_string()),
            );
        }
        "skills_set_source" => {
            words!("skills", "set-source");
            argv.push(a.required("reference")?);
            flag(&mut argv, "--git-url", a.required("git_url")?);
            optional(&mut argv, "--subpath", a.text("subpath"));
            optional(&mut argv, "--branch", a.text("branch"));
            switch(&mut argv, "--force", a.flag("force"));
            switch(&mut argv, "--dry-run", a.flag("dry_run"));
        }
        "skills_adopt" => {
            words!("skills", "adopt");
            argv.extend(a.required_list("paths")?);
            optional(&mut argv, "--git-url", a.text("git_url"));
            optional(&mut argv, "--git-subpath", a.text("git_subpath"));
            switch(&mut argv, "--dry-run", a.flag("dry_run"));
        }

        "tags_list" => {
            words!("skills", "tag", "list");
            optional_positional(&mut argv, "reference", a.text("reference"))?;
        }
        "tags_add" | "tags_remove" | "tags_set" => {
            let verb = tool.trim_start_matches("tags_");
            words!("skills", "tag");
            argv.push(verb.to_string());
            argv.push(a.required("reference")?);
            argv.extend(a.required_list("tags")?);
        }
        "tags_rename" => {
            words!("skills", "tag", "rename");
            argv.push(a.required("old_name")?);
            argv.push(a.required("new_name")?);
        }
        "tags_delete" => {
            words!("skills", "tag", "delete");
            argv.push(a.required("name")?);
            switch(&mut argv, "--yes", a.flag("yes"));
            switch(&mut argv, "--dry-run", a.flag("dry_run"));
        }

        "presets_list" => words!("presets", "list"),
        "presets_current" => words!("presets", "current"),
        "presets_show" => {
            words!("presets", "show");
            argv.push(a.required("reference")?);
        }
        "presets_create" => {
            words!("presets", "create");
            argv.push(a.required("name")?);
            optional(&mut argv, "--description", a.text("description"));
            optional(&mut argv, "--icon", a.text("icon"));
        }
        "presets_update" => {
            words!("presets", "update");
            argv.push(a.required("reference")?);
            optional(&mut argv, "--name", a.text("name"));
            optional(&mut argv, "--description", a.text("description"));
            optional(&mut argv, "--icon", a.text("icon"));
        }
        "presets_delete" => {
            words!("presets", "delete");
            argv.push(a.required("reference")?);
            switch(&mut argv, "--yes", a.flag("yes"));
            switch(&mut argv, "--dry-run", a.flag("dry_run"));
        }
        "presets_preview" => {
            words!("presets", "preview");
            argv.push(a.required("reference")?);
        }
        "presets_deploy" | "presets_undeploy" => {
            let verb = if tool == "presets_deploy" {
                "deploy"
            } else {
                "undeploy"
            };
            words!("presets");
            argv.push(verb.to_string());
            argv.push(a.required("reference")?);
            for agent in a.list("agents")? {
                flag(&mut argv, "--agent", agent);
            }
            switch(&mut argv, "--dry-run", a.flag("dry_run"));
        }
        "presets_status" => {
            words!("presets", "status");
            argv.push(a.required("reference")?);
            for agent in a.list("agents")? {
                flag(&mut argv, "--agent", agent);
            }
        }
        "presets_add_skill" => {
            words!("presets", "add-skill");
            argv.push(a.required("preset")?);
            argv.extend(a.required_list("skills")?);
        }
        "presets_remove_skill" => {
            words!("presets", "remove-skill");
            argv.push(a.required("preset")?);
            argv.extend(a.required_list("skills")?);
        }

        "git_status" => words!("git", "status"),
        "git_init" => words!("git", "init"),
        "git_clone" => {
            words!("git", "clone");
            argv.push(a.required("url")?);
        }
        "git_set_remote" => {
            words!("git", "set-remote");
            argv.push(a.required("url")?);
        }
        "git_pull" => words!("git", "pull"),
        "git_push" => words!("git", "push"),
        "git_commit" => {
            words!("git", "commit");
            flag(&mut argv, "--message", a.required("message")?);
        }
        "git_versions" => {
            words!("git", "versions");
            optional(
                &mut argv,
                "--limit",
                a.number("limit")?.map(|n| n.to_string()),
            );
        }
        "git_restore" => {
            words!("git", "restore");
            argv.push(a.required("tag")?);
        }
        "git_prune_sync_refs" => words!("git", "prune-sync-refs"),

        other => bail!("unknown tool: {other}"),
    }

    Ok(argv)
}

fn flag(argv: &mut Vec<String>, name: &str, value: String) {
    argv.push(name.to_string());
    argv.push(value);
}

fn optional(argv: &mut Vec<String>, name: &str, value: Option<String>) {
    if let Some(value) = value {
        flag(argv, name, value);
    }
}

fn switch(argv: &mut Vec<String>, name: &str, on: bool) {
    if on {
        argv.push(name.to_string());
    }
}

/// A positional that starts with `-` is a flag to clap, not a value. Without
/// this, `references: ["alpha", "--yes"]` would reach `skills remove` as
/// `--yes` and skip the confirmation the tool exists to require.
fn no_leading_dash(key: &str, value: String) -> Result<String> {
    if value.starts_with('-') {
        bail!("`{key}` must not start with '-' (the CLI would read it as a flag)");
    }
    Ok(value)
}

fn optional_positional(argv: &mut Vec<String>, key: &str, value: Option<String>) -> Result<()> {
    if let Some(value) = value {
        argv.push(no_leading_dash(key, value)?);
    }
    Ok(())
}

struct Args<'a>(&'a Map<String, Value>);

impl<'a> Args<'a> {
    fn text(&self, key: &str) -> Option<String> {
        self.0.get(key).and_then(Value::as_str).map(str::to_string)
    }

    fn flag(&self, key: &str) -> bool {
        self.0.get(key).and_then(Value::as_bool).unwrap_or(false)
    }

    fn number(&self, key: &str) -> Result<Option<u64>> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_u64()
                .map(Some)
                .ok_or_else(|| anyhow!("`{key}` must be an integer")),
        }
    }

    fn list(&self, key: &str) -> Result<Vec<String>> {
        let Some(value) = self.0.get(key) else {
            return Ok(Vec::new());
        };
        if value.is_null() {
            return Ok(Vec::new());
        }
        let items = value
            .as_array()
            .ok_or_else(|| anyhow!("`{key}` must be an array of strings"))?;
        items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| anyhow!("`{key}` must contain only strings"))
            })
            .collect()
    }

    fn required(&self, key: &str) -> Result<String> {
        let value = self
            .text(key)
            .ok_or_else(|| anyhow!("missing required string argument `{key}`"))?;
        no_leading_dash(key, value)
    }

    fn required_list(&self, key: &str) -> Result<Vec<String>> {
        let values = self.list(key)?;
        if values.is_empty() {
            return Err(anyhow!("missing required argument `{key}`"));
        }
        values
            .into_iter()
            .map(|value| no_leading_dash(key, value))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap()
    }

    fn argv(tool: &str, args: Value) -> Vec<String> {
        tool_argv(tool, &map(args)).unwrap()
    }

    /// Every advertised tool must reach the CLI. A name in `tools()` with no
    /// arm in `tool_argv` would fail here, not at the user's first call.
    #[test]
    fn every_advertised_tool_maps_to_argv() {
        for spec in tools() {
            let args = synthesize(&spec.schema);
            let argv = tool_argv(spec.name, &map(args))
                .unwrap_or_else(|err| panic!("{} does not map: {err}", spec.name));
            assert!(!argv.is_empty(), "{} produced an empty argv", spec.name);
        }
    }

    #[test]
    fn unknown_tools_are_rejected() {
        let err = tool_argv("skills_teleport", &Map::new()).unwrap_err();
        assert!(err.to_string().contains("unknown tool"));
    }

    /// The flag spellings are the fragile part: clap turns `deployed_to` into
    /// `--deployed-to`, and a typo here only shows up at the first call.
    #[test]
    fn skills_list_maps_every_filter() {
        assert_eq!(
            argv(
                "skills_list",
                json!({
                    "query": "x",
                    "tags": ["a", "b"],
                    "preset": "p",
                    "deployed_to": "codex",
                    "untagged": true,
                    "no_preset": true,
                    "source": "git"
                })
            ),
            [
                "skills",
                "list",
                "--query",
                "x",
                "--tag",
                "a",
                "--tag",
                "b",
                "--preset",
                "p",
                "--deployed-to",
                "codex",
                "--untagged",
                "--no-preset",
                "--source",
                "git"
            ]
        );
    }

    #[test]
    fn install_kind_is_validated() {
        assert_eq!(
            argv("skills_install", json!({ "reference": "x", "kind": "git" })),
            ["skills", "install", "x", "--git"]
        );
        assert!(
            tool_argv(
                "skills_install",
                &map(json!({ "reference": "x", "kind": "bogus" }))
            )
            .is_err(),
            "an unknown kind must not fall back to detection"
        );
    }

    #[test]
    fn list_arguments_must_be_strings() {
        assert!(
            tool_argv(
                "skills_deploy",
                &map(json!({ "references": ["a", 2], "agents": ["codex"] }))
            )
            .is_err(),
            "a non-string list item must be rejected, not dropped"
        );
    }

    #[test]
    fn positional_values_cannot_smuggle_flags() {
        // `--yes` as a reference would otherwise reach clap as the
        // confirmation flag and delete the skill without one.
        let err = tool_argv(
            "skills_remove",
            &map(json!({ "references": ["alpha", "--yes"] })),
        )
        .unwrap_err();
        assert!(err.to_string().contains("must not start with '-'"));
    }

    #[test]
    fn arguments_are_checked_against_the_schema() {
        let deploy = tools()
            .into_iter()
            .find(|tool| tool.name == "skills_deploy")
            .unwrap();
        assert!(validate_args(
            &deploy.schema,
            &map(json!({ "references": ["a"], "agents": ["x"], "dry_run": true }))
        )
        .is_ok());
        // A string "true" is not a boolean; that is the difference between a
        // dry run and the real thing.
        assert!(validate_args(
            &deploy.schema,
            &map(json!({ "references": ["a"], "agents": ["x"], "dry_run": "true" }))
        )
        .is_err());
        // A misspelt key must not be ignored.
        assert!(validate_args(
            &deploy.schema,
            &map(json!({ "references": ["a"], "agents": ["x"], "dryrun": true }))
        )
        .is_err());
    }

    #[test]
    fn malformed_arguments_are_a_protocol_error() {
        let reply = handle_message(
            &json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call",
                    "params": {"name": "skills_deploy",
                               "arguments": {"references": ["a"], "agents": ["x"],
                                             "dry_run": "true"}}}),
            &mut |_| unreachable!("a malformed call must not run the CLI"),
        )
        .unwrap();
        assert_eq!(reply["error"]["code"], -32602);
    }

    /// MCP clients key tools by name, and the spec allows only these
    /// characters. A duplicate name would silently shadow one of the two.
    #[test]
    fn tool_names_are_unique_and_valid() {
        let mut names: Vec<&str> = tools().iter().map(|tool| tool.name).collect();
        let total = names.len();
        assert!(total > 0);
        for name in &names {
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "{name} is not a valid MCP tool name"
            );
        }
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "two tools share a name");
    }

    #[test]
    fn initialize_answers_with_a_supported_revision() {
        let reply = handle_message(
            &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                    "params": {"protocolVersion": "2024-11-05"}}),
            &mut |_| unreachable!("initialize must not run the CLI"),
        )
        .unwrap();
        assert_eq!(reply["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(reply["result"]["serverInfo"]["name"], SERVER_NAME);
    }

    #[test]
    fn notifications_get_no_reply() {
        assert!(handle_message(
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            &mut |_| unreachable!(),
        )
        .is_none());
    }

    #[test]
    fn tool_calls_run_the_cli_and_return_its_json() {
        let mut seen: Vec<Vec<String>> = Vec::new();
        let reply = handle_message(
            &json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call",
                    "params": {"name": "skills_deploy",
                               "arguments": {"references": ["alpha"], "agents": ["codex", "cursor"]}}}),
            &mut |args| {
                seen.push(args.to_vec());
                Ok(CliOutput {
                    success: true,
                    stdout: "{\"ok\":true}".into(),
                    stderr: String::new(),
                })
            },
        )
        .unwrap();

        assert_eq!(
            seen,
            vec![vec![
                "skills", "deploy", "alpha", "--agent", "codex", "--agent", "cursor"
            ]]
        );
        assert_eq!(reply["result"]["content"][0]["text"], "{\"ok\":true}");
        assert_eq!(reply["result"]["isError"], false);
    }

    #[test]
    fn a_failed_command_is_a_tool_error_not_a_protocol_error() {
        let reply = handle_message(
            &json!({"jsonrpc": "2.0", "id": 8, "method": "tools/call",
                    "params": {"name": "skills_remove", "arguments": {"references": ["alpha"]}}}),
            &mut |_| {
                Ok(CliOutput {
                    success: false,
                    stdout: String::new(),
                    stderr: "{\"ok\":false,\"code\":\"REFUSED\"}".into(),
                })
            },
        )
        .unwrap();

        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(
            reply["result"]["content"][0]["text"],
            "{\"ok\":false,\"code\":\"REFUSED\"}"
        );
    }

    #[test]
    fn the_skills_root_reaches_the_cli() {
        let root = Path::new("/tmp/library");
        let argv = ["skills".to_string(), "list".to_string()];
        assert_eq!(
            child_args(Some(root), &argv),
            ["--skills-root", "/tmp/library", "--json", "skills", "list"]
                .map(OsString::from)
                .to_vec()
        );
        assert_eq!(
            child_args(None, &argv),
            ["--json", "skills", "list"].map(OsString::from).to_vec()
        );
    }

    /// Fill every required property with a value of the right shape, so the
    /// coverage test exercises the real mapping rather than `null`s.
    fn synthesize(schema: &Value) -> Value {
        let required: Vec<&str> = schema["required"]
            .as_array()
            .map(|items| items.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut object = Map::new();
        for key in required {
            let property = &schema["properties"][key];
            object.insert(key.to_string(), sample(property));
        }
        Value::Object(object)
    }

    fn sample(property: &Value) -> Value {
        match property["type"].as_str().unwrap_or("string") {
            "array" => json!(["sample"]),
            "boolean" => json!(true),
            "integer" => json!(1),
            _ => json!("sample"),
        }
    }
}
