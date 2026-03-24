use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::skillssh_api::{build_http_client, SkillsShSkill};

#[derive(Debug, Deserialize)]
struct SkillsMpSkill {
    name: Option<String>,
    source: Option<String>,
    #[serde(default)]
    stars: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchMode {
    Keyword,
    Ai,
}

impl SearchMode {
    fn endpoint(&self) -> &str {
        match self {
            Self::Keyword => "https://skillsmp.com/api/v1/skills/search",
            Self::Ai => "https://skillsmp.com/api/v1/skills/ai-search",
        }
    }
}

pub fn search(
    api_key: &str,
    query: &str,
    mode: SearchMode,
    page: Option<u32>,
    limit: Option<u32>,
    proxy_url: Option<&str>,
) -> Result<Vec<SkillsShSkill>> {
    let client = build_http_client(proxy_url, 15);

    let mut url = format!(
        "{}?q={}",
        mode.endpoint(),
        urlencoding::encode(query),
    );
    if let Some(p) = page {
        url.push_str(&format!("&page={}", p));
    }
    if let Some(l) = limit {
        url.push_str(&format!("&limit={}", l.min(100)));
    }

    let resp: serde_json::Value = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .context("Failed to fetch skillsmp.com")?
        .error_for_status()
        .context("SkillsMP request failed")?
        .json()
        .context("Failed to parse SkillsMP response")?;

    // Check for error responses
    if let Some(err) = resp.get("error") {
        let code = err
            .get("code")
            .and_then(|v| v.as_str())
            .unwrap_or("UNKNOWN");
        let msg = err
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");
        anyhow::bail!("SkillsMP API error ({}): {}", code, msg);
    }

    // Parse skills from response — try "skills" array first, then "results"
    let raw_skills: Vec<SkillsMpSkill> = if let Some(arr) = resp.get("skills").and_then(|v| v.as_array()) {
        serde_json::from_value(serde_json::Value::Array(arr.clone()))
            .unwrap_or_else(|e| { log::warn!("SkillsMP: failed to parse skills array: {e}"); Vec::new() })
    } else if let Some(arr) = resp.get("results").and_then(|v| v.as_array()) {
        serde_json::from_value(serde_json::Value::Array(arr.clone()))
            .unwrap_or_else(|e| { log::warn!("SkillsMP: failed to parse results array: {e}"); Vec::new() })
    } else if let Some(arr) = resp.as_array() {
        serde_json::from_value(serde_json::Value::Array(arr.clone()))
            .unwrap_or_else(|e| { log::warn!("SkillsMP: failed to parse root array: {e}"); Vec::new() })
    } else {
        Vec::new()
    };

    Ok(raw_skills
        .into_iter()
        .filter_map(|s| {
            let source = s.source?;
            let name = s.name?;
            // SkillsMP source formats:
            //   "owner/repo/skill" → source="owner/repo", skill_id="skill"
            //   "owner/repo"       → source="owner/repo", skill_id=name
            //   "owner"            → source="owner", skill_id=name
            let slash_count = source.matches('/').count();
            let (src, skill_id) = if slash_count >= 2 {
                // Split at last slash: "owner/repo/skill" → ("owner/repo", "skill")
                let pos = source.rfind('/').unwrap();
                (source[..pos].to_string(), source[pos + 1..].to_string())
            } else {
                // "owner/repo" or "owner" — keep full source, use name as skill_id
                (source.clone(), name.clone())
            };
            let id = format!("{}/{}", src, skill_id);
            Some(SkillsShSkill {
                id,
                skill_id,
                name,
                source: src,
                installs: s.stars,
            })
        })
        .collect())
}
