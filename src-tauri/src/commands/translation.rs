use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};
use reqwest::blocking::Client;
use reqwest::Url;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::State;

use crate::core::{
    ai_credentials, central_repo, error::AppError, skill_store::SkillStore,
    skillssh_api::build_http_client,
};

const API_URL_SETTING: &str = "ai_translation_api_url";
const MODEL_SETTING: &str = "ai_translation_model";
const DEFAULT_API_URL: &str = "https://api.openai.com/v1";
const DEFAULT_MODEL: &str = "gpt-5.6-luna";
const MAX_BATCH_SOURCE_CHARS: usize = 700;
const MAX_PARALLEL_REQUESTS: usize = 16;
const TRANSLATION_CACHE_VERSION: &str = "ai-translation-v3-numbered-units";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTranslationSettings {
    pub api_url: String,
    pub model: String,
    pub has_api_key: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAiTranslationSettings {
    pub api_url: String,
    pub model: String,
    pub api_key: Option<String>,
    #[serde(default)]
    pub clear_api_key: bool,
}

#[derive(Debug, Clone)]
struct MarkdownTextSegment {
    range: Range<usize>,
    text: String,
}

#[tauri::command]
pub async fn get_ai_translation_settings(
    store: State<'_, Arc<SkillStore>>,
) -> Result<AiTranslationSettings, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let api_url = setting_or_default(&store, API_URL_SETTING, DEFAULT_API_URL)?;
        let model = setting_or_default(&store, MODEL_SETTING, DEFAULT_MODEL)?;
        let has_api_key = ai_credentials::load_api_key()
            .map_err(AppError::internal)?
            .is_some();
        Ok(AiTranslationSettings {
            api_url,
            model,
            has_api_key,
        })
    })
    .await?
}

#[tauri::command]
pub async fn save_ai_translation_settings(
    input: SaveAiTranslationSettings,
    store: State<'_, Arc<SkillStore>>,
) -> Result<AiTranslationSettings, AppError> {
    let api_url = validate_api_url(&input.api_url)
        .map_err(|error| AppError::invalid_input(error.to_string()))?;
    let model = input.model.trim().to_owned();
    if model.is_empty() {
        return Err(AppError::invalid_input("模型名称不能为空"));
    }

    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .set_setting(API_URL_SETTING, &api_url)
            .map_err(AppError::db)?;
        store
            .set_setting(MODEL_SETTING, &model)
            .map_err(AppError::db)?;

        if input.clear_api_key {
            ai_credentials::delete_api_key().map_err(AppError::internal)?;
        } else if let Some(api_key) = input.api_key.map(|value| value.trim().to_owned()) {
            if !api_key.is_empty() {
                ai_credentials::store_api_key(&api_key).map_err(AppError::internal)?;
            }
        }

        let has_api_key = ai_credentials::load_api_key()
            .map_err(AppError::internal)?
            .is_some();
        Ok(AiTranslationSettings {
            api_url,
            model,
            has_api_key,
        })
    })
    .await?
}

#[tauri::command]
pub async fn translate_skill_document(
    content: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<String, AppError> {
    if content.trim().is_empty() {
        return Ok(content);
    }

    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let api_url = setting_or_default(&store, API_URL_SETTING, DEFAULT_API_URL)?;
        let model = setting_or_default(&store, MODEL_SETTING, DEFAULT_MODEL)?;
        let cache_root = central_repo::cache_dir().join("translations");
        let cache_started = Instant::now();
        if let Some(cached) = load_cached_translation(&cache_root, &api_url, &model, &content)
            .map_err(AppError::internal)?
        {
            log::info!(
                "AI translation cache hit: model={} source_chars={} elapsed_ms={}",
                model,
                content.chars().count(),
                cache_started.elapsed().as_millis()
            );
            return Ok(cached);
        }

        let api_key = ai_credentials::load_api_key().map_err(AppError::internal)?;
        let endpoint = chat_completions_endpoint(&api_url)
            .map_err(|error| AppError::invalid_input(error.to_string()))?;

        if endpoint.host_str() == Some("api.openai.com") && api_key.is_none() {
            return Err(AppError::invalid_input("请先在设置中配置 AI 翻译 API Key"));
        }

        let client = build_http_client(store.proxy_url().as_deref(), 120);
        let translated =
            match translate_markdown(&client, endpoint, api_key.as_deref(), &model, &content) {
                Ok(translated) => translated,
                Err(error) => {
                    log::error!(
                        "AI translation failed: model={} source_chars={} error={error:#}",
                        model,
                        content.chars().count()
                    );
                    return Err(AppError::network(error));
                }
            };
        save_cached_translation(&cache_root, &api_url, &model, &content, &translated)
            .map_err(AppError::internal)?;
        Ok(translated)
    })
    .await?
}

fn setting_or_default(
    store: &SkillStore,
    key: &str,
    default_value: &str,
) -> Result<String, AppError> {
    Ok(store
        .get_setting(key)
        .map_err(AppError::db)?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default_value.to_owned()))
}

fn validate_api_url(value: &str) -> anyhow::Result<String> {
    let trimmed = value.trim().trim_end_matches('/');
    let parsed = Url::parse(trimmed).context("API 地址格式无效")?;
    if !matches!(parsed.scheme(), "http" | "https") {
        anyhow::bail!("API 地址必须以 http:// 或 https:// 开头");
    }
    if parsed.host_str().is_none() {
        anyhow::bail!("API 地址缺少主机名");
    }
    Ok(trimmed.to_owned())
}

fn chat_completions_endpoint(api_url: &str) -> anyhow::Result<Url> {
    let base = validate_api_url(api_url)?;
    let endpoint = if base.ends_with("/chat/completions") {
        base
    } else if base.ends_with("/v1") {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    };
    Url::parse(&endpoint).context("无法构造 Chat Completions 地址")
}

fn translate_markdown(
    client: &Client,
    endpoint: Url,
    api_key: Option<&str>,
    model: &str,
    content: &str,
) -> anyhow::Result<String> {
    let segments = collect_markdown_text_segments(content);
    if segments.is_empty() {
        return Ok(content.to_owned());
    }

    let source_texts: Vec<&str> = segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect();
    let source_chars = source_texts
        .iter()
        .map(|text| text.chars().count())
        .sum::<usize>();
    let batch_count = build_translation_batches(&source_texts, MAX_BATCH_SOURCE_CHARS).len();
    let started = Instant::now();
    log::info!(
        "AI translation started: model={} segments={} source_chars={} batches={}",
        model,
        source_texts.len(),
        source_chars,
        batch_count
    );
    let translated = request_ai_translation(client, endpoint, api_key, model, &source_texts)?;
    let output = apply_translated_segments(content, &segments, &translated)?;
    log::info!(
        "AI translation completed: model={} segments={} source_chars={} batches={} elapsed_ms={}",
        model,
        source_texts.len(),
        source_chars,
        batch_count,
        started.elapsed().as_millis()
    );
    Ok(output)
}

fn request_ai_translation(
    client: &Client,
    endpoint: Url,
    api_key: Option<&str>,
    model: &str,
    source_texts: &[&str],
) -> anyhow::Result<Vec<String>> {
    let batches = build_translation_batches(source_texts, MAX_BATCH_SOURCE_CHARS);
    if batches.is_empty() {
        return Ok(Vec::new());
    }
    if batches.len() == 1 {
        return request_ai_translation_batch(client, endpoint, api_key, model, source_texts);
    }

    let mut translated = Vec::with_capacity(source_texts.len());
    for wave in batches.chunks(MAX_PARALLEL_REQUESTS) {
        let wave_results = std::thread::scope(|scope| {
            let handles = wave
                .iter()
                .map(|range| {
                    let endpoint = endpoint.clone();
                    let batch = &source_texts[range.clone()];
                    scope.spawn(move || {
                        request_ai_translation_batch(client, endpoint, api_key, model, batch)
                    })
                })
                .collect::<Vec<_>>();

            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .map_err(|_| anyhow::anyhow!("AI translation worker panicked"))?
                })
                .collect::<anyhow::Result<Vec<_>>>()
        })?;

        for batch in wave_results {
            translated.extend(batch);
        }
    }

    if translated.len() != source_texts.len() {
        anyhow::bail!(
            "AI returned {} translated segments, but the source contains {} segments",
            translated.len(),
            source_texts.len()
        );
    }
    Ok(translated)
}

fn request_ai_translation_batch(
    client: &Client,
    endpoint: Url,
    api_key: Option<&str>,
    model: &str,
    source_texts: &[&str],
) -> anyhow::Result<Vec<String>> {
    let source_units = source_texts
        .iter()
        .enumerate()
        .map(|(index, text)| (format!("unit_{index}"), Value::String((*text).to_owned())))
        .collect::<serde_json::Map<String, Value>>();
    let source_json = serde_json::to_string(&source_units)?;
    let body = json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": "You are a professional Simplified Chinese technical editor. The user sends a JSON object whose keys are fixed unit IDs and whose values are complete Markdown headings, paragraphs, or list items. Translate every value into fluent, idiomatic Chinese in full context; never translate fragment-by-fragment or word-by-word. Rewrite English contractions naturally in Chinese and never turn apostrophes into quotation marks. Keep skill names, command names, product names, inline-code literals, file paths, placeholders, and URLs unchanged. Preserve Markdown structure, markers, links, emphasis, list nesting, and line breaks. Return only one valid JSON object containing every original unit ID exactly once. Never merge, omit, rename, or add unit IDs. Do not add explanations or Markdown fences."
            },
            {
                "role": "user",
                "content": source_json
            }
        ],
        "stream": false
    });

    let mut request = client.post(endpoint).json(&body);
    if let Some(api_key) = api_key.filter(|value| !value.trim().is_empty()) {
        request = request.bearer_auth(api_key);
    }

    let response = request.send().context("AI 翻译请求失败")?;
    let status = response.status();
    let response_body = response.text().context("读取 AI 翻译响应失败")?;
    let payload: Value = serde_json::from_str(&response_body)
        .with_context(|| format!("AI 接口返回了无效 JSON（HTTP {}）", status.as_u16()))?;

    if !status.is_success() {
        let message = payload
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("未知错误");
        anyhow::bail!("AI 接口返回 HTTP {}：{}", status.as_u16(), message);
    }

    let output = extract_chat_message_content(&payload)
        .context("AI 接口响应中缺少 choices[0].message.content")?;
    let translated = parse_translation_units(&output, source_texts.len())?;
    for (index, (source, translated)) in source_texts.iter().zip(&translated).enumerate() {
        validate_translated_markdown_unit(source, translated).with_context(|| {
            format!("AI translation changed Markdown structure in unit {index}")
        })?;
    }
    Ok(translated)
}

fn build_translation_batches(source_texts: &[&str], max_chars: usize) -> Vec<Range<usize>> {
    if source_texts.is_empty() {
        return Vec::new();
    }

    let max_chars = max_chars.max(1);
    let mut batches = Vec::new();
    let mut batch_start = 0usize;
    let mut batch_chars = 0usize;

    for (index, text) in source_texts.iter().enumerate() {
        let text_chars = text.chars().count();
        if index > batch_start && batch_chars.saturating_add(text_chars) > max_chars {
            batches.push(batch_start..index);
            batch_start = index;
            batch_chars = 0;
        }
        batch_chars = batch_chars.saturating_add(text_chars);
    }

    batches.push(batch_start..source_texts.len());
    batches
}

fn translation_cache_path(cache_root: &Path, api_url: &str, model: &str, content: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    for value in [TRANSLATION_CACHE_VERSION, api_url, model, content] {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    let digest = hasher.finalize();
    let key = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    cache_root.join(format!("{key}.md"))
}

fn load_cached_translation(
    cache_root: &Path,
    api_url: &str,
    model: &str,
    content: &str,
) -> anyhow::Result<Option<String>> {
    let path = translation_cache_path(cache_root, api_url, model, content);
    match std::fs::read_to_string(path) {
        Ok(cached) => Ok(Some(cached)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("Failed to read AI translation cache"),
    }
}

fn save_cached_translation(
    cache_root: &Path,
    api_url: &str,
    model: &str,
    content: &str,
    translated: &str,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(cache_root).context("Failed to create AI translation cache")?;
    let path = translation_cache_path(cache_root, api_url, model, content);
    std::fs::write(path, translated).context("Failed to write AI translation cache")
}

fn extract_chat_message_content(payload: &Value) -> Option<String> {
    let content = payload.pointer("/choices/0/message/content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_owned());
    }
    content.as_array().map(|parts| {
        parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<String>()
    })
}

fn parse_translation_array(output: &str) -> anyhow::Result<Vec<String>> {
    let trimmed = output.trim();
    if let Ok(values) = serde_json::from_str::<Vec<String>>(trimmed) {
        return Ok(values);
    }

    let without_fence = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    if let Ok(values) = serde_json::from_str::<Vec<String>>(without_fence) {
        return Ok(values);
    }

    let start = without_fence.find('[');
    let end = without_fence.rfind(']');
    if let (Some(start), Some(end)) = (start, end) {
        if start < end {
            return serde_json::from_str(&without_fence[start..=end])
                .context("AI 返回的译文不是有效的 JSON 字符串数组");
        }
    }
    anyhow::bail!("AI 返回的译文不是有效的 JSON 字符串数组")
}

fn parse_translation_units(output: &str, expected_len: usize) -> anyhow::Result<Vec<String>> {
    let trimmed = output.trim();
    let without_fence = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    let start = without_fence.find('{');
    let end = without_fence.rfind('}');
    let candidate = match (start, end) {
        (Some(start), Some(end)) if start < end => &without_fence[start..=end],
        _ => without_fence,
    };

    if let Ok(units) = serde_json::from_str::<serde_json::Map<String, Value>>(candidate) {
        let mut translated = Vec::with_capacity(expected_len);
        for index in 0..expected_len {
            let key = format!("unit_{index}");
            let value = units
                .get(&key)
                .and_then(Value::as_str)
                .with_context(|| format!("AI translation response is missing string key {key}"))?;
            translated.push(value.to_owned());
        }
        if units.len() != expected_len {
            anyhow::bail!(
                "AI translation response contains {} unit IDs, expected {}",
                units.len(),
                expected_len
            );
        }
        return Ok(translated);
    }

    let translated = parse_translation_array(output)?;
    if translated.len() != expected_len {
        anyhow::bail!(
            "AI returned {} translated segments, but the source contains {} segments",
            translated.len(),
            expected_len
        );
    }
    Ok(translated)
}

fn collect_markdown_text_segments(content: &str) -> Vec<MarkdownTextSegment> {
    let body_offset = frontmatter_end(content).unwrap_or(0);
    let body = &content[body_offset..];
    let mut segments = Vec::new();
    let mut block_start = None;
    let mut line_start = 0usize;
    let mut fenced_code: Option<(char, usize)> = None;
    let mut indented_code_block = false;

    for line in body.split_inclusive('\n') {
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        let trimmed = line_without_newline.trim();
        let line_end = line_start + line_without_newline.len();

        if let Some((marker, width)) = fenced_code {
            if is_fence_line(line_without_newline, marker, width) {
                fenced_code = None;
            }
            line_start += line.len();
            continue;
        }

        if let Some((marker, width)) = opening_fence(line_without_newline) {
            finish_markdown_block(
                body,
                body_offset,
                &mut segments,
                &mut block_start,
                line_start,
            );
            fenced_code = Some((marker, width));
            line_start += line.len();
            continue;
        }

        if trimmed.is_empty() {
            finish_markdown_block(
                body,
                body_offset,
                &mut segments,
                &mut block_start,
                line_start,
            );
            indented_code_block = false;
            line_start += line.len();
            continue;
        }

        if indented_code_block {
            line_start += line.len();
            continue;
        }
        if block_start.is_none()
            && (line_without_newline.starts_with("    ") || line_without_newline.starts_with('\t'))
        {
            indented_code_block = true;
            line_start += line.len();
            continue;
        }

        let starts_standalone_block =
            is_heading_line(line_without_newline) || is_top_level_list_item(line_without_newline);
        if starts_standalone_block && block_start.is_some() {
            finish_markdown_block(
                body,
                body_offset,
                &mut segments,
                &mut block_start,
                line_start,
            );
        }
        block_start.get_or_insert(line_start);

        if is_heading_line(line_without_newline) {
            finish_markdown_block(body, body_offset, &mut segments, &mut block_start, line_end);
        }

        line_start += line.len();
    }

    finish_markdown_block(
        body,
        body_offset,
        &mut segments,
        &mut block_start,
        body.len(),
    );
    segments
}

fn finish_markdown_block(
    body: &str,
    body_offset: usize,
    segments: &mut Vec<MarkdownTextSegment>,
    block_start: &mut Option<usize>,
    raw_end: usize,
) {
    let Some(start) = block_start.take() else {
        return;
    };
    let mut end = raw_end;
    while end > start && matches!(body.as_bytes()[end - 1], b'\r' | b'\n') {
        end -= 1;
    }
    if end <= start {
        return;
    }

    let text = &body[start..end];
    if !markdown_block_has_translatable_text(text) {
        return;
    }
    segments.push(MarkdownTextSegment {
        range: (start + body_offset)..(end + body_offset),
        text: text.to_owned(),
    });
}

fn markdown_block_has_translatable_text(markdown: &str) -> bool {
    Parser::new_ext(markdown, Options::empty()).any(|event| {
        matches!(
            event,
            Event::Text(text) if text.chars().any(char::is_alphabetic)
        )
    })
}

fn opening_fence(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let width = trimmed.chars().take_while(|value| *value == marker).count();
    (width >= 3).then_some((marker, width))
}

fn is_fence_line(line: &str, marker: char, width: usize) -> bool {
    line.trim_start()
        .chars()
        .take_while(|value| *value == marker)
        .count()
        >= width
}

fn is_heading_line(line: &str) -> bool {
    let marker_width = line.chars().take_while(|value| *value == '#').count();
    (1..=6).contains(&marker_width)
        && line
            .chars()
            .nth(marker_width)
            .is_some_and(char::is_whitespace)
}

fn is_top_level_list_item(line: &str) -> bool {
    if line.chars().next().is_some_and(char::is_whitespace) {
        return false;
    }
    if line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
        .is_some()
    {
        return true;
    }

    let digit_width = line.chars().take_while(char::is_ascii_digit).count();
    digit_width > 0
        && line[digit_width..]
            .strip_prefix(". ")
            .or_else(|| line[digit_width..].strip_prefix(") "))
            .is_some()
}

fn validate_translated_markdown_unit(source: &str, translated: &str) -> anyhow::Result<()> {
    let source_signature = markdown_structure_signature(source);
    let translated_signature = markdown_structure_signature(translated);
    if source_signature != translated_signature {
        anyhow::bail!(
            "expected Markdown signature {:?}, got {:?}",
            source_signature,
            translated_signature
        );
    }
    Ok(())
}

fn markdown_structure_signature(markdown: &str) -> Vec<String> {
    let mut signature = Vec::new();
    for event in Parser::new_ext(markdown, Options::empty()) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                signature.push(format!("heading:{level:?}"));
            }
            Event::Start(Tag::Paragraph) => signature.push("paragraph".to_owned()),
            Event::Start(Tag::BlockQuote(_)) => signature.push("blockquote".to_owned()),
            Event::Start(Tag::List(start)) => signature.push(format!("list:{start:?}")),
            Event::Start(Tag::Item) => signature.push("item".to_owned()),
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) if !matches!(link_type, LinkType::Autolink | LinkType::Email) => {
                signature.push(format!("link:{dest_url}"));
            }
            Event::Code(code) => signature.push(format!("code:{code}")),
            Event::HardBreak => signature.push("hard-break".to_owned()),
            Event::Rule => signature.push("rule".to_owned()),
            _ => {}
        }
    }
    signature
}

fn frontmatter_end(content: &str) -> Option<usize> {
    if content.starts_with("---\n") {
        return content[4..].find("\n---\n").map(|end| end + 9);
    }
    if content.starts_with("---\r\n") {
        return content[5..].find("\r\n---\r\n").map(|end| end + 12);
    }
    None
}

fn apply_translated_segments(
    content: &str,
    segments: &[MarkdownTextSegment],
    translations: &[String],
) -> anyhow::Result<String> {
    if segments.len() != translations.len() {
        anyhow::bail!("译文片段数量与原文不一致");
    }

    let mut output = String::with_capacity(content.len());
    let mut copied_until = 0;
    for (segment, translated) in segments.iter().zip(translations) {
        output.push_str(&content[copied_until..segment.range.start]);
        output.push_str(translated);
        copied_until = segment.range.end;
    }
    output.push_str(&content[copied_until..]);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn translation_batches_limit_payload_size_and_preserve_order() {
        let source = vec!["a".repeat(1_200), "b".repeat(1_200), "c".repeat(200)];
        let source_refs: Vec<&str> = source.iter().map(String::as_str).collect();
        let batches = build_translation_batches(&source_refs, 1_500);

        assert_eq!(batches, vec![0..1, 1..3]);
        let flattened: Vec<&str> = batches
            .iter()
            .flat_map(|range| source_refs[range.clone()].iter().copied())
            .collect();
        assert_eq!(flattened, source_refs);
    }

    #[test]
    fn default_translation_batches_stay_below_provider_timeout_threshold() {
        assert!(
            MAX_BATCH_SOURCE_CHARS <= 700,
            "empirical DeepSeek requests above 700 source characters can exceed the 120-second timeout"
        );
    }

    #[test]
    fn translation_cache_persists_and_invalidates_with_inputs() {
        let temp = tempfile::tempdir().unwrap();
        let cache_root = temp.path();
        let api_url = "https://api.example.com/v1";
        let model = "fast-model";
        let source = "# Example\n\nEnglish content.";
        let translated = "# 示例\n\n中文内容。";

        assert_eq!(
            load_cached_translation(cache_root, api_url, model, source).unwrap(),
            None
        );
        save_cached_translation(cache_root, api_url, model, source, translated).unwrap();
        assert_eq!(
            load_cached_translation(cache_root, api_url, model, source).unwrap(),
            Some(translated.to_owned())
        );
        assert_eq!(
            load_cached_translation(cache_root, api_url, "other-model", source).unwrap(),
            None
        );
        assert_eq!(
            load_cached_translation(cache_root, api_url, model, "# Example\n\nChanged content.")
                .unwrap(),
            None
        );
    }

    #[test]
    fn translation_batches_are_requested_concurrently() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = Url::parse(&format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let active_requests = Arc::new(AtomicUsize::new(0));
        let max_active_requests = Arc::new(AtomicUsize::new(0));
        let server_active = active_requests.clone();
        let server_max_active = max_active_requests.clone();

        let server = std::thread::spawn(move || {
            let mut workers = Vec::new();
            for _ in 0..4 {
                let (stream, _) = listener.accept().unwrap();
                let active = server_active.clone();
                let max_active = server_max_active.clone();
                workers.push(std::thread::spawn(move || {
                    serve_translation_response(stream, active, max_active);
                }));
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });

        let source = (0..4)
            .map(|index| format!("{index}{}", "x".repeat(MAX_BATCH_SOURCE_CHARS)))
            .collect::<Vec<_>>();
        let source_refs = source.iter().map(String::as_str).collect::<Vec<_>>();
        let client = Client::builder().build().unwrap();
        let translated =
            request_ai_translation(&client, endpoint, None, "test-model", &source_refs).unwrap();

        server.join().unwrap();
        assert_eq!(translated, vec!["translated"; 4]);
        assert!(
            max_active_requests.load(Ordering::SeqCst) >= 2,
            "expected multiple translation requests to overlap"
        );
    }

    fn serve_translation_response(
        mut stream: TcpStream,
        active_requests: Arc<AtomicUsize>,
        max_active_requests: Arc<AtomicUsize>,
    ) {
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        let mut expected_len = None;
        loop {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if expected_len.is_none() {
                if let Some(header_end) =
                    request.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_len = headers
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length: ")
                                .or_else(|| line.strip_prefix("Content-Length: "))
                        })
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(0);
                    expected_len = Some(header_end + 4 + content_len);
                }
            }
            if expected_len.is_some_and(|length| request.len() >= length) {
                break;
            }
        }

        let active = active_requests.fetch_add(1, Ordering::SeqCst) + 1;
        max_active_requests.fetch_max(active, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(100));

        let payload =
            json!({"choices": [{"message": {"content": "[\"translated\"]"}}]}).to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream.write_all(response.as_bytes()).unwrap();
        active_requests.fetch_sub(1, Ordering::SeqCst);
    }

    #[test]
    fn endpoint_accepts_base_v1_and_full_urls() {
        assert_eq!(
            chat_completions_endpoint("https://api.openai.com/v1")
                .unwrap()
                .as_str(),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_endpoint("http://127.0.0.1:11434")
                .unwrap()
                .as_str(),
            "http://127.0.0.1:11434/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_endpoint("https://example.com/v1/chat/completions")
                .unwrap()
                .as_str(),
            "https://example.com/v1/chat/completions"
        );
    }

    #[test]
    fn markdown_structure_and_code_are_preserved() {
        let source = "---\nname: demo\n---\n# Heading\n\n- **Bold text** and `inline-code`\n\n```rust\nlet value = 1;\n```\n";
        let segments = collect_markdown_text_segments(source);
        assert_eq!(
            segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>(),
            vec!["# Heading", "- **Bold text** and `inline-code`"]
        );
        let translated = vec![
            "# 标题".to_owned(),
            "- **粗体文本**和 `inline-code`".to_owned(),
        ];
        let result = apply_translated_segments(source, &segments, &translated).unwrap();

        assert!(result.starts_with("---\nname: demo\n---\n"));
        assert!(result.contains("# 标题"));
        assert!(result.contains("- **粗体文本**和 `inline-code`"));
        assert!(result.contains("```rust\nlet value = 1;\n```"));
        assert!(validate_translated_markdown_unit(&segments[1].text, &translated[1]).is_ok());
    }

    #[test]
    fn markdown_translation_units_keep_complete_sentence_context() {
        let source =
            "# Ask Matt\n\nYou don't remember every **skill**, so ask.\n\nUse `ask-matt` now.\n";
        let units = collect_markdown_text_segments(source);
        let texts = units
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            texts,
            vec![
                "# Ask Matt",
                "You don't remember every **skill**, so ask.",
                "Use `ask-matt` now.",
            ]
        );
    }

    #[test]
    fn top_level_list_items_keep_their_nested_context() {
        let source = "1. Branch through a prototype:\n   - Use `/handoff` first.\n   - Then use `/prototype`.\n2. Continue with `/implement`.\n";
        let units = collect_markdown_text_segments(source);
        assert_eq!(
            units
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>(),
            vec![
                "1. Branch through a prototype:\n   - Use `/handoff` first.\n   - Then use `/prototype`.",
                "2. Continue with `/implement`.",
            ]
        );
    }

    #[test]
    fn markdown_validation_rejects_changed_inline_code_and_links() {
        let source = "Use [`ask-matt`](https://example.com/skill) now.";
        assert!(validate_translated_markdown_unit(
            source,
            "现在使用 [`ask-matt`](https://example.com/skill)。"
        )
        .is_ok());
        assert!(validate_translated_markdown_unit(
            source,
            "现在使用 [`询问-matt`](https://example.com/skill)。"
        )
        .is_err());
        assert!(validate_translated_markdown_unit(
            source,
            "现在使用 [`ask-matt`](https://example.cn/skill)。"
        )
        .is_err());
    }

    #[test]
    fn markdown_validation_allows_emphasis_only_changes() {
        let source = "2. **Branch** — **can you settle every question?** Use `/handoff`.";
        let translated = "2. **分支**——你能解决每个问题吗？使用 `/handoff`。";

        assert!(validate_translated_markdown_unit(source, translated).is_ok());
    }

    #[test]
    fn parses_plain_fenced_and_wrapped_json_arrays() {
        assert_eq!(
            parse_translation_array(r#"["甲","乙"]"#).unwrap(),
            vec!["甲", "乙"]
        );
        assert_eq!(
            parse_translation_array("```json\n[\"甲\"]\n```").unwrap(),
            vec!["甲"]
        );
        assert_eq!(
            parse_translation_array("结果如下： [\"甲\"]").unwrap(),
            vec!["甲"]
        );
    }

    #[test]
    fn parses_numbered_translation_units_in_source_order() {
        let output = r#"{"unit_1":"第二段","unit_0":"第一段"}"#;
        assert_eq!(
            parse_translation_units(output, 2).unwrap(),
            vec!["第一段", "第二段"]
        );
    }

    #[test]
    fn numbered_translation_units_cannot_be_merged_or_omitted() {
        let output = r#"{"unit_0":"合并后的内容"}"#;
        assert!(parse_translation_units(output, 2).is_err());
    }
}
