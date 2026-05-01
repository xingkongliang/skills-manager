use serde_json::{json, Value};
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
        let bundled: std::path::PathBuf = resource_dir.join("scripts").join("ai-agent.mjs");
        if bundled.exists() {
            return Ok(bundled);
        }
    }

    // Fallback: dev mode — resolve relative to Cargo manifest
    let dev_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("scripts").join("ai-agent.mjs"))
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

async fn run_ai_bridge(app: tauri::AppHandle, input: Value) -> Result<Value, AppError> {
    let script_path = resolve_script_path(&app)?;
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
        let details = if stderr.trim().is_empty() {
            &stdout
        } else {
            &stderr
        };
        let details_preview = details.chars().take(1000).collect::<String>();
        let normalized = details_preview.to_lowercase();
        let friendly = if normalized.contains("@tencent-ai/agent-sdk") {
            "AI bridge dependency is missing: @tencent-ai/agent-sdk"
        } else if normalized.contains("codebuddy cli is required") {
            "CodeBuddy CLI is required by the Agent SDK but was not found in PATH. Install CodeBuddy Code or set CODEBUDDY_CODE_PATH."
        } else {
            &details_preview
        };
        return Err(AppError::internal(format!(
            "Bridge script failed (exit {}): {}",
            output.status.code().unwrap_or(-1),
            friendly
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

fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|v| {
        let trimmed = v.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn parse_f64_setting(value: Option<String>, default: f64) -> f64 {
    value
        .as_deref()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .unwrap_or(default)
}

fn parse_temperature_setting(value: Option<String>) -> f64 {
    const DEFAULT_TEMPERATURE: f64 = 0.2;

    let temperature = parse_f64_setting(value, DEFAULT_TEMPERATURE);
    if (0.0..=2.0).contains(&temperature) {
        temperature
    } else {
        DEFAULT_TEMPERATURE
    }
}

fn parse_u64_setting(value: Option<String>, default: u64) -> u64 {
    value
        .as_deref()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

fn build_codebuddy_input(
    task: String,
    payload: Value,
    api_key: Option<String>,
    internet_env: Option<String>,
) -> Result<Value, AppError> {
    let api_key = non_empty(api_key)
        .ok_or_else(|| AppError::invalid_input("CodeBuddy API key not configured"))?;

    let mut input = json!({
        "provider": "codebuddy",
        "task": task,
        "payload": payload,
        "codebuddy": {
            "apiKey": api_key,
        },
    });
    if let Some(env) = non_empty(internet_env) {
        input["codebuddy"]["internetEnvironment"] = json!(env);
    }
    if let Ok(codebuddy_path) = std::env::var("CODEBUDDY_CODE_PATH") {
        let codebuddy_path = codebuddy_path.trim();
        if !codebuddy_path.is_empty() {
            input["codebuddy"]["codebuddyCodePath"] = json!(codebuddy_path);
        }
    }
    Ok(input)
}

#[tauri::command]
pub async fn invoke_ai_task(
    app: tauri::AppHandle,
    task: String,
    payload: Value,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Value, AppError> {
    let store = store.inner().clone();
    let input = tauri::async_runtime::spawn_blocking(move || {
        let provider = non_empty(
            store
                .get_setting("ai_default_provider")
                .map_err(AppError::db)?,
        )
        .unwrap_or_else(|| "codebuddy".to_string());

        match provider.as_str() {
            "codebuddy" => {
                let api_key = store
                    .get_setting("codebuddy_api_key")
                    .map_err(AppError::db)?;
                let internet_env = store
                    .get_setting("codebuddy_internet_environment")
                    .map_err(AppError::db)?;
                build_codebuddy_input(task, payload, api_key, internet_env)
            }
            "openai_compatible" => {
                let base_url = non_empty(
                    store
                        .get_setting("openai_compatible_base_url")
                        .map_err(AppError::db)?,
                )
                .ok_or_else(|| {
                    AppError::invalid_input("OpenAI-compatible Base URL not configured")
                })?;
                let api_key = non_empty(
                    store
                        .get_setting("openai_compatible_api_key")
                        .map_err(AppError::db)?,
                )
                .ok_or_else(|| {
                    AppError::invalid_input("OpenAI-compatible API key not configured")
                })?;
                let model = non_empty(
                    store
                        .get_setting("openai_compatible_model")
                        .map_err(AppError::db)?,
                )
                .ok_or_else(|| AppError::invalid_input("OpenAI-compatible model not configured"))?;
                let temperature = parse_temperature_setting(
                    store
                        .get_setting("openai_compatible_temperature")
                        .map_err(AppError::db)?,
                );
                let max_tokens = parse_u64_setting(
                    store
                        .get_setting("openai_compatible_max_tokens")
                        .map_err(AppError::db)?,
                    2000,
                );

                Ok(json!({
                    "provider": "openai_compatible",
                    "task": task,
                    "payload": payload,
                    "openaiCompatible": {
                        "baseUrl": base_url,
                        "apiKey": api_key,
                        "model": model,
                        "temperature": temperature,
                        "maxTokens": max_tokens,
                    },
                }))
            }
            _ => Err(AppError::invalid_input(format!(
                "Unsupported AI provider: {}",
                provider
            ))),
        }
    })
    .await??;

    log::debug!(
        "[AI] task={}, provider={}",
        input
            .get("task")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown"),
        input
            .get("provider")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
    );

    run_ai_bridge(app, input).await
}

#[cfg(test)]
mod tests {
    use super::{parse_f64_setting, parse_temperature_setting};

    #[test]
    fn parse_f64_setting_discards_non_finite_values() {
        for value in ["NaN", "inf", "-inf", "Infinity", "-Infinity"] {
            assert_eq!(parse_f64_setting(Some(value.to_string()), 0.2), 0.2);
        }
    }

    #[test]
    fn parse_temperature_setting_defaults_out_of_range_values() {
        for value in ["-0.1", "2.1", "NaN", "inf", "-inf"] {
            assert_eq!(parse_temperature_setting(Some(value.to_string())), 0.2);
        }
    }

    #[test]
    fn parse_temperature_setting_accepts_values_in_range() {
        assert_eq!(parse_temperature_setting(Some("0".to_string())), 0.0);
        assert_eq!(parse_temperature_setting(Some("1.5".to_string())), 1.5);
        assert_eq!(parse_temperature_setting(Some("2".to_string())), 2.0);
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
    let input = tauri::async_runtime::spawn_blocking(move || {
        let api_key = store
            .get_setting("codebuddy_api_key")
            .map_err(AppError::db)?;
        let internet_env = store
            .get_setting("codebuddy_internet_environment")
            .map_err(AppError::db)?;
        build_codebuddy_input(task, payload, api_key, internet_env)
    })
    .await??;

    run_ai_bridge(app, input).await
}
