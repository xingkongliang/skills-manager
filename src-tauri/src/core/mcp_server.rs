//! `skills-manager-cli mcp`: Project setup as a local stdio MCP server, for
//! coding agents such as Codex and Claude Code.
//!
//! Newline-delimited JSON-RPC 2.0 in both directions, answered in order. The
//! server handles `initialize`, `ping`, `tools/list` and `tools/call`; other
//! requests get "method not found" and notifications get no reply. Nothing
//! else may reach stdout. The server exits when stdin closes.
//!
//! Tool names, inputs and output fields are frozen at version 1: add optional
//! fields or new tools, never change what callers rely on.

use std::io::{self, BufRead, Write};
use std::panic::{self, AssertUnwindSafe};

use serde_json::{json, Value};

use super::error::AppError;
use super::host::HostCtx;
use super::project_setup;

/// Newest first: a client asking for another version gets the newest.
const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

const DISCOVER: &str = "discover_project_setup";

/// Answer JSON-RPC messages from `reader` on `writer` until `reader` ends.
pub fn run(mut reader: impl BufRead, mut writer: impl Write, ctx: &HostCtx) -> io::Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let reply = match serde_json::from_slice::<Value>(&line) {
            Ok(message) => answer(ctx, &message),
            Err(err) => Some(failure(
                Value::Null,
                PARSE_ERROR,
                format!("Parse error: {err}"),
            )),
        };
        if let Some(reply) = reply {
            let mut bytes = serde_json::to_vec(&reply)?;
            bytes.push(b'\n');
            writer.write_all(&bytes)?;
            writer.flush()?;
        }
    }
}

/// The reply to one message, or `None` for a notification or a response.
fn answer(ctx: &HostCtx, message: &Value) -> Option<Value> {
    let id = message.get("id")?.clone();
    let method = message.get("method")?.as_str().unwrap_or_default();
    let params = &message["params"];
    let result = match method {
        "initialize" => Ok(initialize(params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": [discover_tool()] })),
        "tools/call" => call_tool(ctx, params),
        _ => Err((METHOD_NOT_FOUND, format!("Method not found: {method}"))),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => failure(id, code, message),
    })
}

fn failure(id: Value, code: i64, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn initialize(params: &Value) -> Value {
    let asked = params["protocolVersion"].as_str();
    let version = PROTOCOL_VERSIONS
        .iter()
        .find(|v| Some(**v) == asked)
        .unwrap_or(&PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "agents-manager",
            "title": "Agents Manager",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": "Pass the active checkout's absolute path to discover_project_setup \
            to read its Project setup. A linked worktree is its own checkout.",
    })
}

/// A tool's own failure is a result the agent reads (`isError`), not a
/// protocol error; only an unknown tool is one.
fn call_tool(ctx: &HostCtx, params: &Value) -> Result<Value, (i64, String)> {
    let name = params["name"].as_str().unwrap_or_default();
    if name != DISCOVER {
        return Err((INVALID_PARAMS, format!("Unknown tool: {name}")));
    }
    let mut request = match &params["arguments"] {
        Value::Object(arguments) => arguments.clone(),
        Value::Null => Default::default(),
        _ => return Err((INVALID_PARAMS, "Tool arguments must be an object".into())),
    };
    request.insert("action".into(), json!("discover"));
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
        project_setup::dispatch(ctx, Value::Object(request))
    }))
    .unwrap_or_else(|_| Err(AppError::internal(format!("{name} panicked"))));
    Ok(match outcome {
        Ok(found) => json!({
            "content": [{ "type": "text", "text": found.to_string() }],
            "structuredContent": found,
            "isError": false,
        }),
        Err(error) => json!({
            "content": [{ "type": "text", "text": json!(error).to_string() }],
            "isError": true,
        }),
    })
}

fn discover_tool() -> Value {
    let nullable_object = json!({ "type": ["object", "null"] });
    let project = json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" },
            "name": { "type": "string" },
            "path": { "type": "string", "description": "Canonical checkout path." },
            "agent_keys": { "type": ["array", "null"], "items": { "type": "string" } },
            "deploy_mode": { "type": "string", "enum": ["link", "copy"] },
        },
        "required": ["id", "name", "path", "agent_keys", "deploy_mode"],
    });
    json!({
        "name": DISCOVER,
        "title": "Discover Project setup",
        "description": "Read the Project setup for one exact checkout on this computer: \
            its registered project, saved setup, Library options (50 per page), \
            read-only Sync status and conflicts. When the checkout is not registered, \
            up to 10 registered projects that contain it or sit inside it are listed \
            as candidates. Writes nothing.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "checkout_path": {
                    "type": "string",
                    "description": "Absolute path of the active checkout.",
                },
                "cursor": {
                    "type": "string",
                    "description": "library.next_cursor from the previous page.",
                },
            },
            "required": ["checkout_path"],
            "additionalProperties": false,
        },
        "outputSchema": {
            "type": "object",
            "properties": {
                "host": { "type": "string" },
                "checkout_path": { "type": "string" },
                "project": {
                    "anyOf": [project, { "type": "null" }],
                    "description": "The project registered at exactly this checkout.",
                },
                "candidates": { "type": "array", "items": project, "maxItems": 10 },
                "saved_setup": nullable_object,
                "library": {
                    "type": "object",
                    "properties": {
                        "items": {
                            "type": "array",
                            "maxItems": 50,
                            "items": {
                                "type": "object",
                                "properties": {
                                    "kind": { "type": "string", "enum": ["skill", "instruction", "mcp"] },
                                    "id": { "type": "string" },
                                    "name": { "type": "string" },
                                },
                                "required": ["kind", "id", "name"],
                            },
                        },
                        "next_cursor": { "type": ["string", "null"] },
                    },
                    "required": ["items", "next_cursor"],
                },
                "sync": {
                    "type": "object",
                    "description": "This host's last recorded Sync state. verified is \
                        false: discovery does not contact the Sync remote.",
                    "properties": {
                        "state": { "type": "string" },
                        "reason": { "type": ["string", "null"] },
                        "remote": { "type": ["string", "null"] },
                        "last_sync_at": { "type": ["integer", "null"] },
                        "checked_at": { "type": "integer" },
                        "verified": { "type": "boolean" },
                    },
                    "required": ["state", "reason", "remote", "last_sync_at", "checked_at", "verified"],
                },
                "conflicts": {
                    "type": "object",
                    "properties": {
                        "skills": { "type": "array" },
                        "resources": { "type": "array" },
                        "same_name_presets": { "type": "array" },
                        "edited_here": { "type": "array" },
                    },
                    "required": ["skills", "resources", "same_name_presets", "edited_here"],
                },
            },
            "required": [
                "host", "checkout_path", "project", "candidates",
                "saved_setup", "library", "sync", "conflicts",
            ],
        },
        "annotations": {
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": false,
        },
    })
}
