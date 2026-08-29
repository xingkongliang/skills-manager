use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::path::{Component, Path, PathBuf};

pub const TEMP_DIR_PREFIX: &str = "skills-manager-well-known-";

const MAX_DOWNLOAD_BYTES: u64 = 50 * 1024 * 1024;
const SITE_HOSTS: &[&str] = &["skills.sh", "www.skills.sh"];

#[derive(Debug, Clone)]
pub struct SiteRef {
    pub source_url: String,
    pub skill_name: String,
}

#[derive(Debug)]
pub struct DownloadedSkill {
    pub temp_dir: PathBuf,
    pub skill_dir: PathBuf,
    pub resolved_url: String,
    pub revision: Option<String>,
}

struct DiscoveryIndex {
    url: reqwest::Url,
    well_known_path: &'static str,
    payload: Value,
}

pub fn parse_site_ref(input: &str) -> Result<SiteRef> {
    let parsed = reqwest::Url::parse(input.trim()).context("Invalid skills.sh URL")?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("skills.sh URL is missing a host"))?;
    if !SITE_HOSTS.contains(&host) {
        bail!("Expected a skills.sh /site/<domain>/<skill> URL");
    }

    let segments: Vec<&str> = parsed
        .path_segments()
        .map(|segments| segments.collect())
        .unwrap_or_default();
    if segments.len() != 3 || segments[0] != "site" {
        bail!("Expected a skills.sh /site/<domain>/<skill> URL");
    }
    if !is_safe_segment(segments[1]) || !is_safe_segment(segments[2]) {
        bail!("Invalid skills.sh site reference");
    }

    Ok(SiteRef {
        source_url: format!("https://{}/", segments[1]),
        skill_name: segments[2].to_string(),
    })
}

pub fn is_site_ref(input: &str) -> bool {
    parse_site_ref(input).is_ok()
}

pub fn download_site_skill(input: &str, proxy_url: Option<&str>) -> Result<DownloadedSkill> {
    let site = parse_site_ref(input)?;
    let client = crate::core::skillssh_api::build_http_client(proxy_url, 30);
    let index = fetch_index(&client, &site.source_url)?;
    let entry = index
        .payload
        .get("skills")
        .and_then(Value::as_array)
        .and_then(|skills| {
            skills.iter().find(|skill| {
                skill.get("name").and_then(Value::as_str) == Some(site.skill_name.as_str())
            })
        })
        .ok_or_else(|| anyhow::anyhow!("Skill '{}' was not found at {}", site.skill_name, site.source_url))?;

    let temp = tempfile::Builder::new()
        .prefix(TEMP_DIR_PREFIX)
        .tempdir()
        .context("Failed to create skills download directory")?;
    let skill_dir = temp.path().join("skill");
    std::fs::create_dir_all(&skill_dir)?;

    let (resolved_url, revision) = if index.payload.get("$schema").is_some() {
        download_v2_entry(&client, &index, entry, &skill_dir)?
    } else {
        download_v1_entry(&client, &index, entry, &site.skill_name, &skill_dir)?
    };

    let temp_dir = temp.keep();
    let skill_dir = temp_dir.join("skill");
    Ok(DownloadedSkill {
        temp_dir,
        skill_dir,
        resolved_url,
        revision,
    })
}

pub fn skill_dir_from_temp(temp_dir: &Path) -> Result<PathBuf> {
    let skill_dir = temp_dir.join("skill");
    if !skill_dir.is_dir() || !skill_dir.join("SKILL.md").is_file() {
        bail!("Downloaded skill is missing SKILL.md");
    }
    Ok(skill_dir)
}

pub fn cleanup_temp(temp_dir: &Path) {
    let _ = std::fs::remove_dir_all(temp_dir);
}

fn fetch_index(client: &Client, source_url: &str) -> Result<DiscoveryIndex> {
    for well_known_path in [".well-known/agent-skills", ".well-known/skills"] {
        let url = reqwest::Url::parse(source_url)?.join(&format!("{well_known_path}/index.json"))?;
        let response = client.get(url.clone()).send();
        let Ok(response) = response else { continue };
        if !response.status().is_success() {
            continue;
        }
        let payload: Value = response
            .json()
            .with_context(|| format!("Failed to parse skills index at {url}"))?;
        if payload.get("skills").and_then(Value::as_array).is_some() {
            return Ok(DiscoveryIndex {
                url,
                well_known_path,
                payload,
            });
        }
    }
    bail!("No supported skills index found at {source_url}");
}

fn download_v1_entry(
    client: &Client,
    index: &DiscoveryIndex,
    entry: &Value,
    skill_name: &str,
    skill_dir: &Path,
) -> Result<(String, Option<String>)> {
    let files = entry
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("skills index entry is missing files"))?;
    let base = index
        .url
        .join("./")?
        .join(&format!("{skill_name}/"))?;
    for file in files {
        let file = file
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("skills index contains an invalid file path"))?;
        let relative = safe_relative_path(file)?;
        let destination = skill_dir.join(&relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let url = base.join(file)?;
        std::fs::write(destination, get_bytes(client, &url)?)?;
    }
    if !skill_dir.join("SKILL.md").is_file() {
        bail!("skills index entry is missing SKILL.md");
    }
    Ok((base.join("SKILL.md")?.to_string(), None))
}

fn download_v2_entry(
    client: &Client,
    index: &DiscoveryIndex,
    entry: &Value,
    skill_dir: &Path,
) -> Result<(String, Option<String>)> {
    let kind = entry
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("skills index entry is missing type"))?;
    let artifact = entry
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("skills index entry is missing url"))?;
    let url = index.url.join(artifact)?;
    let bytes = get_bytes(client, &url)?;
    let digest = entry.get("digest").and_then(Value::as_str);
    if let Some(digest) = digest {
        verify_digest(&bytes, digest)?;
    }

    match kind {
        "skill-md" => {
            std::fs::write(skill_dir.join("SKILL.md"), bytes)?;
        }
        "archive" => extract_zip(&bytes, skill_dir)?,
        other => bail!("Unsupported skills index entry type: {other}"),
    }

    if !skill_dir.join("SKILL.md").is_file() {
        bail!("Downloaded skill is missing SKILL.md");
    }
    Ok((url.to_string(), digest.map(str::to_string)))
}

fn get_bytes(client: &Client, url: &reqwest::Url) -> Result<Vec<u8>> {
    let response = client
        .get(url.clone())
        .send()
        .with_context(|| format!("Failed to download {url}"))?
        .error_for_status()
        .with_context(|| format!("Failed to download {url}"))?;
    if response.content_length().is_some_and(|size| size > MAX_DOWNLOAD_BYTES) {
        bail!("Download exceeds the {} MiB limit", MAX_DOWNLOAD_BYTES / 1024 / 1024);
    }
    let bytes = response.bytes()?.to_vec();
    if bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
        bail!("Download exceeds the {} MiB limit", MAX_DOWNLOAD_BYTES / 1024 / 1024);
    }
    Ok(bytes)
}

fn verify_digest(bytes: &[u8], expected: &str) -> Result<()> {
    let Some(expected) = expected.strip_prefix("sha256:") else {
        bail!("Unsupported skills index digest");
    };
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual != expected {
        bail!("Downloaded skill failed its SHA-256 digest check");
    }
    Ok(())
}

fn extract_zip(bytes: &[u8], destination: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(relative) = entry.enclosed_name() else {
            bail!("Archive contains an unsafe path");
        };
        let target = destination.join(relative);
        if !target.starts_with(destination) {
            bail!("Archive contains an unsafe path");
        }
        if entry.is_dir() {
            std::fs::create_dir_all(target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::File::create(target)?;
        std::io::copy(&mut entry, &mut file)?;
    }
    Ok(())
}

fn safe_relative_path(path: &str) -> Result<PathBuf> {
    if path.is_empty() || path.contains('\\') {
        bail!("Invalid skill file path");
    }
    let path = Path::new(path);
    if path.components().any(|component| {
        matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))
    }) {
        bail!("Invalid skill file path");
    }
    Ok(path.to_path_buf())
}

fn is_safe_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || matches!(char, '.' | '-'))
        && !segment.starts_with('.')
        && !segment.ends_with('.')
        && !segment.starts_with('-')
        && !segment.ends_with('-')
}

#[cfg(test)]
mod tests {
    use super::{is_site_ref, parse_site_ref, safe_relative_path};

    #[test]
    fn parses_website_synced_skills_sh_refs() {
        let parsed = parse_site_ref("https://www.skills.sh/site/uizze.com/ui-radar").unwrap();
        assert_eq!(parsed.source_url, "https://uizze.com/");
        assert_eq!(parsed.skill_name, "ui-radar");
    }

    #[test]
    fn does_not_treat_github_urls_as_site_refs() {
        assert!(!is_site_ref("https://github.com/uizze/uizze"));
    }

    #[test]
    fn rejects_unsafe_skill_file_paths() {
        assert!(safe_relative_path("../SKILL.md").is_err());
        assert!(safe_relative_path("references/guide.md").is_ok());
    }
}
