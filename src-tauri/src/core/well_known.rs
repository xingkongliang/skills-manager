use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

pub const TEMP_DIR_PREFIX: &str = "skills-manager-well-known-";

const MAX_DOWNLOAD_BYTES: u64 = 50 * 1024 * 1024;
const MAX_EXTRACTED_BYTES: u64 = 100 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 4096;
// Bound implicit directories too, not just ZIP records. Canonical bundles use
// shallow reference/ paths; Unicode names remain supported.
const MAX_PATH_BYTES: usize = 512;
const MAX_PATH_DEPTH: usize = 16;
const MAX_TOTAL_PATH_BYTES: usize = 64 * 1024;
const MAX_TOTAL_COMPONENTS: usize = 16 * 1024;
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

    let Some(mut segments) = parsed.path_segments() else {
        bail!("Expected a skills.sh /site/<domain>/<skill> URL");
    };
    let (Some("site"), Some(domain), Some(skill), None) =
        (segments.next(), segments.next(), segments.next(), segments.next())
    else {
        bail!("Expected a skills.sh /site/<domain>/<skill> URL");
    };
    if !is_safe_segment(domain) || !is_safe_segment(skill) {
        bail!("Invalid skills.sh site reference");
    }

    Ok(SiteRef {
        source_url: format!("https://{domain}/"),
        skill_name: skill.to_string(),
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
        let payload: Value = serde_json::from_slice(&read_limited(response, MAX_DOWNLOAD_BYTES)?)
            .with_context(|| format!("Failed to parse skills index at {url}"))?;
        if payload.get("skills").and_then(Value::as_array).is_some() {
            return Ok(DiscoveryIndex {
                url,
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
    if files.len() > MAX_ARCHIVE_ENTRIES {
        bail!("Skill contains too many files");
    }
    let mut paths = PathBudget::default();
    let mut seen = std::collections::HashSet::new();
    for file in files {
        let file = file.as_str()
            .ok_or_else(|| anyhow::anyhow!("skills index contains an invalid file path"))?;
        paths.add(file)?;
        if !seen.insert(file) {
            bail!("Skill contains duplicate files");
        }
    }
    let mut remaining = MAX_DOWNLOAD_BYTES;
    for file in files {
        let file = file.as_str().expect("preflight checked file paths");
        let destination = skill_dir.join(safe_relative_path(file)?);
        let url = base.join(file)?;
        let bytes = get_bytes_limited(client, &url, remaining)?;
        remaining -= bytes.len() as u64;
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(destination)?;
        std::io::Write::write_all(&mut output, &bytes)?;
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
    get_bytes_limited(client, url, MAX_DOWNLOAD_BYTES)
}

fn get_bytes_limited(client: &Client, url: &reqwest::Url, limit: u64) -> Result<Vec<u8>> {
    let response = client
        .get(url.clone())
        .send()
        .with_context(|| format!("Failed to download {url}"))?
        .error_for_status()
        .with_context(|| format!("Failed to download {url}"))?;
    if response.content_length().is_some_and(|size| size > limit) {
        bail!("Download exceeds the {} byte limit", limit);
    }
    read_limited(response, limit)
}

fn read_limited(reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("Download exceeds the {} byte limit", limit);
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
    extract_zip_with_limits(bytes, destination, MAX_EXTRACTED_BYTES, MAX_ARCHIVE_ENTRIES)
}

fn extract_zip_with_limits(bytes: &[u8], destination: &Path, max_bytes: u64, max_entries: usize) -> Result<()> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    if archive.len() > max_entries {
        bail!("Archive contains too many entries");
    }
    // Validate every header before creating any files. Never materialize links or
    // special files, and apply the same path rules on Unix and Windows.
    let mut declared_bytes = 0u64;
    let mut paths = PathBudget::default();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        paths.add(entry.name().trim_end_matches('/'))?;
        if let Some(mode) = entry.unix_mode() {
            let kind = mode & 0o170000;
            if kind != 0 && kind != 0o100000 && kind != 0o040000 {
                bail!("Archive contains a link or special file");
            }
        }
        declared_bytes = declared_bytes.checked_add(entry.size())
            .ok_or_else(|| anyhow::anyhow!("Archive size overflow"))?;
        if declared_bytes > max_bytes {
            bail!("Archive exceeds extraction size limit");
        }
    }
    let mut remaining = max_bytes;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let relative = safe_relative_path(entry.name().trim_end_matches('/'))?;
        let target = destination.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // The caller owns an empty private TempDir. create_new additionally
        // rejects duplicate files and file/directory collisions.
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(target)?;
        let written = std::io::copy(&mut (&mut entry).take(remaining), &mut file)?;
        remaining -= written;
        let mut extra = [0u8; 1];
        if entry.read(&mut extra)? != 0 {
            bail!("Archive exceeds extraction size limit");
        }
    }
    Ok(())
}

fn safe_relative_path(path: &str) -> Result<&Path> {
    if path.is_empty() || path.len() > MAX_PATH_BYTES || path.contains('\\') || path.contains(':')
        || path.chars().any(|character| character.is_control())
        || path.split('/').count() > MAX_PATH_DEPTH
        || path.split('/').any(|part| part.is_empty() || part == "." || part == ".."
            || part.ends_with(['.', ' ']) || is_windows_device(part))
    {
        bail!("Invalid skill file path");
    }
    let path = Path::new(path);
    if path.components().any(|component| {
        matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))
    }) {
        bail!("Invalid skill file path");
    }
    Ok(path)
}

fn is_windows_device(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component);
    ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
        || (stem.get(..3).is_some_and(|prefix| prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT"))
            && matches!(stem.get(3..), Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")))
}

#[derive(Default)]
struct PathBudget {
    bytes: usize,
    components: usize,
}

impl PathBudget {
    fn add(&mut self, path: &str) -> Result<()> {
        safe_relative_path(path)?;
        self.bytes += path.len();
        self.components += path.split('/').count();
        if self.bytes > MAX_TOTAL_PATH_BYTES || self.components > MAX_TOTAL_COMPONENTS {
            bail!("Skill exceeds total path budget");
        }
        Ok(())
    }
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
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

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
        assert_eq!(safe_relative_path("references/guide.md").unwrap(), Path::new("references/guide.md"));
    }

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            writer.start_file(*name, SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn extracts_valid_bundle_at_exact_byte_limit() {
        let bytes = archive(&[("SKILL.md", b"skill"), ("references/guide.md", b"guide")]);
        let temp = tempfile::tempdir().unwrap();
        extract_zip_with_limits(&bytes, temp.path(), 10, 2).unwrap();
        assert_eq!(std::fs::read(temp.path().join("references/guide.md")).unwrap(), b"guide");
    }

    #[test]
    fn rejects_traversal_before_writing_any_entry() {
        for name in ["../escaped", "/escaped", "nested/../../escaped", "C:/escaped", "nested\\escaped", "./escaped"] {
            let bytes = archive(&[("SKILL.md", b"skill"), (name, b"bad")]);
            let temp = tempfile::tempdir().unwrap();
            assert!(extract_zip(&bytes, temp.path()).is_err(), "{name}");
            assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
        }
    }

    #[test]
    fn rejects_links_before_writing_any_entry() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer.add_symlink("linked", "../escaped", SimpleFileOptions::default()).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let temp = tempfile::tempdir().unwrap();
        assert!(extract_zip(&bytes, temp.path()).is_err());
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    }

    #[test]
    fn rejects_expansion_and_entry_count_over_limits() {
        let bytes = archive(&[("SKILL.md", &[b'a'; 4096]), ("references/guide.md", &[b'b'; 4096])]);
        for (max_bytes, max_entries) in [(8191, 2), (8192, 1)] {
            let temp = tempfile::tempdir().unwrap();
            assert!(extract_zip_with_limits(&bytes, temp.path(), max_bytes, max_entries).is_err());
            assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
        }
    }

    #[test]
    fn refuses_existing_files_instead_of_overwriting() {
        let bytes = archive(&[("SKILL.md", b"second")]);
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("SKILL.md"), b"first").unwrap();
        assert!(extract_zip(&bytes, temp.path()).is_err());
        assert_eq!(std::fs::read(temp.path().join("SKILL.md")).unwrap(), b"first");
    }

    #[test]
    fn rejects_mismatched_digest_and_accepts_matching_digest() {
        let bytes = archive(&[("SKILL.md", b"skill")]);
        assert!(verify_digest(&bytes, &format!("sha256:{}", "0".repeat(64))).is_err());
        assert!(verify_digest(&bytes, &format!("sha256:{:x}", Sha256::digest(&bytes))).is_ok());
    }

    #[test]
    fn bounds_stream_without_content_length() {
        assert_eq!(read_limited(Cursor::new(b"1234"), 4).unwrap(), b"1234");
        assert!(read_limited(Cursor::new(b"12345"), 4).is_err());
    }

    #[test]
    fn rejects_windows_special_names_but_preserves_unicode() {
        for name in ["NUL.md", "con", "COM1.txt", "LPT9", "COM¹.md", "file.", "file ", "nested/aux.txt", "nul\0.txt", "a\nb"] {
            assert!(safe_relative_path(name).is_err(), "{name:?}");
        }
        assert_eq!(safe_relative_path("参考/é.md").unwrap(), Path::new("参考/é.md"));
    }

    #[test]
    fn rejects_deep_long_and_aggregate_paths_before_extraction() {
        for name in [format!("{}SKILL.md", "a/".repeat(MAX_PATH_DEPTH)), "a".repeat(MAX_PATH_BYTES + 1)] {
            let bytes = archive(&[("SKILL.md", b"skill"), (&name, b"bad")]);
            let temp = tempfile::tempdir().unwrap();
            assert!(extract_zip(&bytes, temp.path()).is_err());
            assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
        }
        let names: Vec<String> = (0..200).map(|index| format!("{}-{index}.md", "x".repeat(400))).collect();
        let entries: Vec<_> = names.iter().map(|name| (name.as_str(), b"x".as_slice())).collect();
        let temp = tempfile::tempdir().unwrap();
        assert!(extract_zip(&archive(&entries), temp.path()).is_err());
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    }

    #[test]
    fn refuses_file_directory_collisions() {
        for entries in [vec![("reference", b"a".as_slice()), ("reference/guide.md", b"b")],
            vec![("reference/guide.md", b"a".as_slice()), ("reference", b"b")]] {
            let temp = tempfile::tempdir().unwrap();
            assert!(extract_zip(&archive(&entries), temp.path()).is_err());
        }
    }

    fn serve_chunked(bytes: Vec<u8>) -> (reqwest::Url, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = reqwest::Url::parse(&format!("http://{}/artifact.zip", listener.local_addr().unwrap())).unwrap();
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 4096];
            stream.read(&mut request).unwrap();
            stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").unwrap();
            for chunk in bytes.chunks(8192) {
                if write!(stream, "{:x}\r\n", chunk.len()).is_err()
                    || stream.write_all(chunk).is_err() || stream.write_all(b"\r\n").is_err() {
                    return; // Overflow consumer may close before the final chunk.
                }
            }
            let _ = stream.write_all(b"0\r\n\r\n");
        });
        (url, thread)
    }

    #[test]
    fn real_download_rejects_digest_mismatch_without_writes_and_cleans_tempdir() {
        let (url, server) = serve_chunked(archive(&[("SKILL.md", b"skill")]));
        let index = DiscoveryIndex { url: url.clone(), payload: Value::Null };
        let entry = serde_json::json!({"type":"archive","url":url.as_str(),"digest":format!("sha256:{}", "0".repeat(64))});
        let temp_path;
        {
            let temp = tempfile::tempdir().unwrap();
            temp_path = temp.path().to_path_buf();
            let error = download_v2_entry(&Client::new(), &index, &entry, temp.path()).unwrap_err();
            assert!(error.to_string().contains("SHA-256"));
            assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
        }
        assert!(!temp_path.exists());
        server.join().unwrap();
    }

    #[test]
    fn real_chunked_download_over_limit_never_extracts() {
        let (url, server) = serve_chunked(vec![b'a'; MAX_DOWNLOAD_BYTES as usize + 1]);
        let index = DiscoveryIndex { url: url.clone(), payload: Value::Null };
        let entry = serde_json::json!({"type":"archive","url":url.as_str()});
        let temp = tempfile::tempdir().unwrap();
        let error = download_v2_entry(&Client::new(), &index, &entry, temp.path()).unwrap_err();
        assert!(error.to_string().contains("limit"));
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
        server.join().unwrap();
    }

    #[test]
    fn v1_preflight_rejects_unsafe_duplicate_and_excessive_files_without_requests() {
        let index = DiscoveryIndex {
            url: reqwest::Url::parse("http://127.0.0.1:1/index.json").unwrap(),
            payload: Value::Null,
        };
        for files in [serde_json::json!(["SKILL.md", "../escaped"]),
            serde_json::json!(["SKILL.md", "SKILL.md"]),
            serde_json::json!(vec!["SKILL.md"; MAX_ARCHIVE_ENTRIES + 1])] {
            let temp = tempfile::tempdir().unwrap();
            let error = download_v1_entry(&Client::new(), &index, &serde_json::json!({"files":files}), "demo", temp.path()).unwrap_err();
            assert!(!error.to_string().contains("download"), "{error}");
            assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
        }
    }

    #[test]
    fn v1_real_multifile_download_enforces_aggregate_budget_and_discards_partial_bundle() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = reqwest::Url::parse(&format!("http://{}/index.json", listener.local_addr().unwrap())).unwrap();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                stream.read(&mut request).unwrap();
                // Each file fits the per-download limit; their sum does not.
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", 26 * 1024 * 1024).unwrap();
                for _ in 0..(26 * 1024 * 1024 / 8192) {
                    if stream.write_all(&[b'a'; 8192]).is_err() { break; }
                }
            }
        });
        let index = DiscoveryIndex { url, payload: Value::Null };
        let entry = serde_json::json!({"files":["SKILL.md", "reference/guide.md"]});
        let temp_path;
        {
            let temp = tempfile::tempdir().unwrap();
            temp_path = temp.path().to_path_buf();
            let error = download_v1_entry(&Client::new(), &index, &entry, "demo", temp.path()).unwrap_err();
            assert!(error.to_string().contains("limit"), "{error}");
            assert_eq!(std::fs::metadata(temp.path().join("SKILL.md")).unwrap().len(), 26 * 1024 * 1024);
            assert!(!temp.path().join("reference/guide.md").exists());
        }
        assert!(!temp_path.exists(), "partial download must never survive as an installable bundle");
        server.join().unwrap();
    }

    #[test]
    fn truncated_and_crc_corrupt_downloads_abort_and_discard_private_bundle() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer.start_file("SKILL.md", SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)).unwrap();
        writer.write_all(b"unique-skill-payload").unwrap();
        let original = writer.finish().unwrap().into_inner();
        let mut corrupt = original.clone();
        let offset = corrupt.windows(20).position(|bytes| bytes == b"unique-skill-payload").unwrap();
        corrupt[offset] ^= 1; // Preserve the original CRC in both headers.
        let truncated = original[..original.len() - 20].to_vec();
        for bytes in [corrupt, truncated] {
            let (url, server) = serve_chunked(bytes);
            let index = DiscoveryIndex { url: url.clone(), payload: Value::Null };
            let entry = serde_json::json!({"type":"archive","url":url.as_str()});
            let temp_path;
            {
                let temp = tempfile::tempdir().unwrap();
                temp_path = temp.path().to_path_buf();
                assert!(download_v2_entry(&Client::new(), &index, &entry, temp.path()).is_err());
            }
            assert!(!temp_path.exists(), "invalid archive must not survive as an installable bundle");
            server.join().unwrap();
        }
    }
}
