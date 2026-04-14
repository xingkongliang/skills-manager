use serde_json::Value;
use std::sync::Arc;
use tauri::{Manager, State};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use crate::core::error::AppError;
use crate::core::skill_store::SkillStore;

/// Resolve the path to the bridge script.
/// In dev: relative to the project root (CARGO_MANIFEST_DIR/../scripts/).
/// In production: bundled as a Tauri resource.
fn resolve_script_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, AppError> {
    // Try resource path first (production build)
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled: std::path::PathBuf = resource_dir.join("scripts").join("codebuddy-agent.mjs");
        if bundled.exists() {
            return Ok(bundled);
        }
    }

    // Fallback: dev mode — resolve relative to Cargo manifest
    let dev_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("scripts").join("codebuddy-agent.mjs"))
        .ok_or_else(|| AppError::internal("Cannot resolve script path"))?;

    if dev_path.exists() {
        Ok(dev_path)
    } else {
        Err(AppError::internal(format!(
            "Bridge script not found at {}",
            dev_path.display()
        )))
    }
}

#[tauri::command]
pub async fn invoke_codebuddy_agent(
    app: tauri::AppHandle,
    task: String,
    payload: Value,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Value, AppError> {
    let store = store.inner().clone();
    let (api_key, internet_env): (Option<String>, Option<String>) = tauri::async_runtime::spawn_blocking(move || {
        let api_key = store.get_setting("codebuddy_api_key").map_err(AppError::db)?;
        let internet_env = store
            .get_setting("codebuddy_internet_environment")
            .map_err(AppError::db)?
            .filter(|v| !v.is_empty());
        Ok::<_, AppError>((api_key, internet_env))
    })
    .await??;

    let api_key = api_key
        .filter(|k| !k.is_empty())
        .ok_or_else(|| AppError::invalid_input("CodeBuddy API key not configured"))?;

    log::debug!("[AI] task={}, env={:?}", task, internet_env);

    let script_path = resolve_script_path(&app)?;

    let mut input = serde_json::json!({
        "task": task,
        "apiKey": api_key,
        "payload": payload,
    });
    if let Some(ref env) = internet_env {
        input["internetEnvironment"] = serde_json::json!(env);
    }
    let input_str = serde_json::to_string(&input)
        .map_err(|e| AppError::internal(format!("Failed to serialize input: {e}")))?;

    let mut child = Command::new("node")
        .arg(&script_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AppError::internal("Node.js is required for AI features but was not found in PATH")
            } else {
                AppError::internal(format!("Failed to spawn node process: {e}"))
            }
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input_str.as_bytes())
            .await
            .map_err(|e| AppError::internal(format!("Failed to write to stdin: {e}")))?;
        // 必须显式 drop stdin，否则 Node.js 的 for await 会永远等待
        drop(stdin);
    }

    // tokio::Child is killed on drop, so timeout automatically cleans up the process
    let output = timeout(Duration::from_secs(60), child.wait_with_output())
        .await
        .map_err(|_| AppError::internal("AI request timed out (60s)"))?
        .map_err(|e| AppError::internal(format!("Failed to read process output: {e}")))?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    if !output.status.success() {
        log::error!(
            "[AI] Bridge script failed (exit {}):\nstderr: {}\nstdout: {}",
            output.status.code().unwrap_or(-1),
            stderr,
            stdout,
        );
        let details = if stderr.trim().is_empty() { &stdout } else { &stderr };
        return Err(AppError::internal(format!(
            "Bridge script failed (exit {}): {}",
            output.status.code().unwrap_or(-1),
            details.chars().take(1000).collect::<String>()
        )));
    }

    if !stderr.trim().is_empty() {
        log::warn!("[AI] Bridge script stderr: {stderr}");
    }

    let result: Value = serde_json::from_str(stdout.trim()).map_err(|e| {
        AppError::internal(format!(
            "Failed to parse bridge output: {e}\nOutput (first 2000 chars):\n{}",
            stdout.chars().take(2000).collect::<String>()
        ))
    })?;

    if result.get("ok") == Some(&Value::Bool(true)) {
        Ok(result.get("data").cloned().unwrap_or(Value::Null))
    } else {
        let error_msg = result
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error from AI bridge");
        Err(AppError::internal(error_msg.to_string()))
    }
}
