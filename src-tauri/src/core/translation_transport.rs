//! Translation request lifecycle and Google fallback. AI providers use saved backend settings.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::{Notify, Semaphore};
use crate::core::error::AppError;
use super::translation_provider::{Provider, TranslationConfig};

const ENDPOINT: &str = "https://translate.google.com/_/TranslateWebserverUi/data/batchexecute";
const MAX_RESPONSE: usize = 2 * 1024 * 1024;

pub struct TranslationTransport {
    gate: Semaphore,
    requests: Mutex<HashMap<String, (Arc<Notify>, bool, Instant)>>,
}
impl Default for TranslationTransport {
    fn default() -> Self { Self { gate: Semaphore::new(1), requests: Mutex::new(HashMap::new()) } }
}
#[derive(Serialize, Clone)]
pub struct TranslationResult { pub text: Option<String> }
#[derive(Serialize)]
pub struct TranslationResponse {
    pub results: Vec<TranslationResult>,
    pub error: Option<String>,
    pub retry_after_ms: Option<u64>,
}

pub fn validate_batch(texts: &[String], target: &str) -> Result<(), AppError> {
    if !matches!(target, "zh-CN" | "zh-TW" | "en" | "es" | "ko") || texts.is_empty() || texts.len() > 20
        || texts.iter().map(|s| s.encode_utf16().count()).sum::<usize>() > 3500 {
        return Err(AppError::invalid_input("reader.translationInvalidRequest"));
    }
    Ok(())
}

pub fn parse_google_batch(body: &str, count: usize) -> Result<Vec<TranslationResult>, AppError> {
    let mut results = vec![TranslationResult { text: None }; count];
    let mut found_frame = false;
    for line in body.lines().map(str::trim).filter(|s| s.starts_with('[')) {
        let Ok(Value::Array(frames)) = serde_json::from_str::<Value>(line) else { continue; };
        for frame in frames {
            if frame.get(0).and_then(Value::as_str) != Some("wrb.fr") { continue; }
            found_frame = true;
            let Some(id) = frame.as_array().and_then(|v| v.last()).and_then(Value::as_str).and_then(|s| usize::from_str_radix(s, 36).ok()).filter(|id| *id < count) else { continue; };
            let Some(payload) = frame.get(2).and_then(Value::as_str).and_then(|s| serde_json::from_str::<Value>(s).ok()) else { continue; };
            let text = payload.pointer("/1/0/0/5").and_then(Value::as_array).map(|parts| parts.iter().filter_map(|p| p.get(0).and_then(Value::as_str)).collect::<Vec<_>>().join(" "))
                .or_else(|| payload.pointer("/1/0/0/0").and_then(Value::as_str).map(str::to_owned));
            results[id].text = text.filter(|s| !s.trim().is_empty());
        }
    }
    if !found_frame { return Err(AppError::network("reader.translationProtocolChanged")); }
    Ok(results)
}

impl TranslationTransport {
    pub fn cancel_all(&self) { for (token, _, _) in self.requests.lock().unwrap().values() { token.notify_one(); } }
    pub fn cancel(&self, id: &str) {
        if id.is_empty() || id.len() > 128 { return; }
        let mut requests = self.requests.lock().unwrap();
        let (token, _, _) = requests.entry(id.to_owned()).or_insert_with(|| (Arc::new(Notify::new()), false, Instant::now()));
        token.notify_one();
    }
    pub async fn translate(&self, request_id: String, texts: Vec<String>, target: String, proxy: Option<String>) -> Result<TranslationResponse, AppError> {
        self.translate_configured(request_id, texts, target, proxy, TranslationConfig::default()).await
    }
    pub async fn translate_configured(&self, request_id: String, texts: Vec<String>, target: String, proxy: Option<String>, config: TranslationConfig) -> Result<TranslationResponse, AppError> {
        validate_batch(&texts, &target)?;
        if request_id.is_empty() || request_id.len() > 128 { return Err(AppError::invalid_input("reader.translationInvalidRequest")); }
        let token = {
            let mut requests = self.requests.lock().unwrap();
            requests.retain(|_, (_, started, at)| *started || at.elapsed() < Duration::from_secs(1800));
            let (token, started, _) = requests.entry(request_id.clone()).or_insert_with(|| (Arc::new(Notify::new()), false, Instant::now()));
            if *started { return Err(AppError::invalid_input("reader.translationInvalidRequest")); }
            *started = true;
            token.clone()
        };
        let result = tokio::select! {
            biased;
            _ = token.notified() => Err(AppError::cancelled("reader.cancelled")),
            value = async {
                if config.settings.provider == Provider::Google { self.send(texts, target, proxy).await }
                else {
                    let _permit = self.gate.acquire().await.map_err(|_| AppError::cancelled("reader.cancelled"))?;
                    config.translate(texts, target, proxy).await
                }
            } => value,
        };
        self.requests.lock().unwrap().remove(&request_id);
        result
    }
    async fn send(&self, texts: Vec<String>, target: String, proxy: Option<String>) -> Result<TranslationResponse, AppError> {
        let _permit = self.gate.acquire().await.map_err(|_| AppError::cancelled("reader.cancelled"))?;
        let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(15)).redirect(reqwest::redirect::Policy::none()).user_agent("skills-manager");
        if let Some(proxy) = proxy.filter(|p| !p.is_empty()) { builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|_| AppError::network("reader.translationNetwork"))?); }
        let client = builder.build().map_err(|_| AppError::network("reader.translationNetwork"))?;
        let frames: Vec<Value> = texts.iter().enumerate().map(|(i, text)| json!(["MkEWBc", json!([[text, "auto", target, false], [null]]).to_string(), null, base36(i)])).collect();
        let body = format!("f.req={}&", urlencoding::encode(&json!([frames]).to_string()));
        if body.len() > 64 * 1024 { return Err(AppError::invalid_input("reader.translationInvalidRequest")); }
        let mut response = client.post(ENDPOINT).query(&[("rpcids", "MkEWBc"), ("source-path", "/"), ("f.sid", ""), ("bl", ""), ("hl", "en-US"), ("soc-app", "1"), ("soc-platform", "1"), ("soc-device", "1"), ("_reqid", "1000"), ("rt", "c")])
            .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded;charset=UTF-8").body(body).send().await.map_err(|_| AppError::network("reader.translationNetwork"))?;
        let status = response.status();
        if !status.is_success() {
            let retry = response.headers().get(reqwest::header::RETRY_AFTER).and_then(|s| s.to_str().ok()).and_then(|s| s.parse::<u64>().ok()).map(|s| s.min(30) * 1000);
            let code = if status.as_u16() == 429 { "reader.translationRateLimited" } else if status.as_u16() == 403 { "reader.translationForbidden" } else if status.is_server_error() { "reader.translationNetwork" } else { "reader.translationProtocolChanged" };
            return Ok(TranslationResponse { results: Vec::new(), error: Some(code.into()), retry_after_ms: retry });
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| AppError::network("reader.translationNetwork"))? {
            if bytes.len() + chunk.len() > MAX_RESPONSE { return Err(AppError::network("reader.translationProtocolChanged")); }
            bytes.extend_from_slice(&chunk);
        }
        let text = String::from_utf8(bytes).map_err(|_| AppError::network("reader.translationProtocolChanged"))?;
        let results = parse_google_batch(&text, texts.len())?;
        // Hold the single-flight gate over the minimum inter-request interval.
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok(TranslationResponse { results, error: None, retry_after_ms: None })
    }
}

fn base36(n: usize) -> String { char::from_digit(n as u32, 36).unwrap_or('0').to_string() }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_real_utf16_and_target_limits() {
        assert!(validate_batch(&vec!["x".into(); 21], "zh-CN").is_err());
        assert!(validate_batch(&["😀".repeat(1751)], "zh-CN").is_err());
        assert!(validate_batch(&["Hello".into()], "https://example.com").is_err());
    }
    #[test]
    fn detects_error_pages_and_preserves_partial_failures() {
        assert!(parse_google_batch("<html>blocked</html>", 1).is_err());
        let payload = json!([null, [[["你好", null, null, null, null, [["你好"]]]]]]);
        let body = format!(")]}}'\n\n123\n{}", json!([["wrb.fr", "MkEWBc", payload.to_string(), null, "0"], ["wrb.fr", "MkEWBc", null, null, "1"]]));
        let results = parse_google_batch(&body, 2).unwrap();
        assert_eq!(results[0].text.as_deref(), Some("你好"));
        assert!(results[1].text.is_none());
    }
    #[tokio::test]
    async fn cancellation_before_registration_never_starts_network() {
        let transport = TranslationTransport::default();
        transport.cancel("pre-cancelled");
        let result = tokio::time::timeout(Duration::from_millis(100), transport.translate("pre-cancelled".into(), vec!["Hello".into()], "zh-CN".into(), None)).await.unwrap();
        assert!(matches!(result, Err(error) if error.kind == crate::core::error::ErrorKind::Cancelled));
    }
}
