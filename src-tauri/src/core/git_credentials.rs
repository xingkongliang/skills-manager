//! Credential handling for the git backup remote.
//!
//! Policy (backup redesign §3.7): tokens must never live in URLs on disk
//! (`.git/config`, SQLite settings). Credentials embedded in a remote URL are
//! extracted into the OS keychain and injected into git at call time through
//! a static askpass script that only echoes environment variables.

use anyhow::{Context, Result};
use std::path::PathBuf;

use super::central_repo;

const KEYRING_SERVICE: &str = "skills-manager-git-backup";

/// Environment variable names consumed by the askpass script. The script
/// itself contains no secrets — it just echoes these back to git.
const ENV_USERNAME: &str = "SKILLS_MANAGER_ASKPASS_USERNAME";
const ENV_PASSWORD: &str = "SKILLS_MANAGER_ASKPASS_PASSWORD";

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RemoteCredential {
    pub username: String,
    pub password: String,
}

/// Split userinfo credentials out of an http(s) URL.
///
/// Returns the extracted credential plus the sanitized URL (no userinfo).
/// `None` when the URL is not http(s) or carries no userinfo. A token-only
/// form (`https://TOKEN@host/...`) is kept faithful: username = token,
/// password = empty — exactly what git derived from the embedded URL.
pub fn split_credentials_from_url(url: &str) -> Option<(RemoteCredential, String)> {
    let trimmed = url.trim();
    let lower = trimmed.to_lowercase();
    if !lower.starts_with("https://") && !lower.starts_with("http://") {
        return None;
    }
    let scheme_end = trimmed.find("://")? + 3;
    let rest = &trimmed[scheme_end..];
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];

    let at_pos = authority.rfind('@')?;
    let userinfo = &authority[..at_pos];
    let host_part = &authority[at_pos + 1..];

    let (raw_user, raw_pass) = match userinfo.split_once(':') {
        Some((u, p)) => (u, p),
        None => (userinfo, ""),
    };
    let decode = |s: &str| {
        urlencoding::decode(s)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| s.to_string())
    };

    let sanitized = format!(
        "{}{}{}",
        &trimmed[..scheme_end],
        host_part,
        &rest[authority_end..]
    );
    Some((
        RemoteCredential {
            username: decode(raw_user),
            password: decode(raw_pass),
        },
        sanitized,
    ))
}

/// Host (including port, if any) of an http(s) URL with userinfo stripped.
/// Used as the keychain account key.
pub fn https_host(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let lower = trimmed.to_lowercase();
    if !lower.starts_with("https://") && !lower.starts_with("http://") {
        return None;
    }
    let scheme_end = trimmed.find("://")? + 3;
    let rest = &trimmed[scheme_end..];
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let host = match authority.rfind('@') {
        Some(at) => &authority[at + 1..],
        None => authority,
    };
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

fn keyring_entry(host: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, host).context("Failed to open keychain entry")
}

pub fn store_credential(host: &str, cred: &RemoteCredential) -> Result<()> {
    let payload = serde_json::to_string(cred)?;
    keyring_entry(host)?
        .set_password(&payload)
        .with_context(|| format!("Failed to store git credential for {host} in OS keychain"))?;
    log::info!("git credentials: stored credential for {host} in OS keychain");
    Ok(())
}

pub fn load_credential(host: &str) -> Result<Option<RemoteCredential>> {
    match keyring_entry(host)?.get_password() {
        Ok(payload) => {
            Ok(Some(serde_json::from_str(&payload).with_context(|| {
                format!("Corrupted keychain entry for {host}")
            })?))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e).with_context(|| format!("Failed to read git credential for {host}")),
    }
}

pub fn delete_credential(host: &str) -> Result<()> {
    match keyring_entry(host)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            log::info!("git credentials: removed credential for {host}");
            Ok(())
        }
        Err(e) => Err(e).with_context(|| format!("Failed to delete git credential for {host}")),
    }
}

/// The askpass script git invokes for username/password prompts. Static
/// content, no secrets — safe on disk. Git for Windows executes shebang
/// scripts through its bundled sh, so a single POSIX script covers all
/// platforms.
const ASKPASS_SCRIPT: &str = "#!/bin/sh\n\
# Managed by Skills Manager. Supplies git credentials from the environment.\n\
case \"$1\" in\n\
  *[Uu]sername*) printf '%s\\n' \"${SKILLS_MANAGER_ASKPASS_USERNAME}\" ;;\n\
  *) printf '%s\\n' \"${SKILLS_MANAGER_ASKPASS_PASSWORD}\" ;;\n\
esac\n";

fn askpass_script_path() -> PathBuf {
    central_repo::base_dir().join("git-askpass.sh")
}

fn ensure_askpass_script() -> Result<PathBuf> {
    let path = askpass_script_path();
    let up_to_date = std::fs::read_to_string(&path)
        .map(|current| current == ASKPASS_SCRIPT)
        .unwrap_or(false);
    if !up_to_date {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, ASKPASS_SCRIPT).context("Failed to write askpass script")?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(path)
}

/// Environment to inject into a git subprocess so it can authenticate against
/// `url` without credentials on disk. Empty when not applicable: non-http(s)
/// URL, URL still carrying embedded userinfo (git uses it directly), or no
/// stored credential for the host.
pub fn credential_env_for_url(url: &str) -> Vec<(String, String)> {
    let Some(host) = https_host(url) else {
        return Vec::new();
    };
    if split_credentials_from_url(url).is_some() {
        return Vec::new();
    }
    let cred = match load_credential(&host) {
        Ok(Some(cred)) => cred,
        Ok(None) => return Vec::new(),
        Err(e) => {
            log::warn!("git credentials: keychain lookup failed for {host}: {e:#}");
            return Vec::new();
        }
    };
    let script = match ensure_askpass_script() {
        Ok(path) => path,
        Err(e) => {
            log::warn!("git credentials: could not prepare askpass script: {e:#}");
            return Vec::new();
        }
    };
    vec![
        (
            "GIT_ASKPASS".to_string(),
            script.to_string_lossy().to_string(),
        ),
        (ENV_USERNAME.to_string(), cred.username),
        (ENV_PASSWORD.to_string(), cred.password),
        ("GIT_TERMINAL_PROMPT".to_string(), "0".to_string()),
    ]
}

/// Route all keyring access in this test process to keyring's in-memory mock
/// store, so tests never touch the developer's real OS keychain.
#[cfg(test)]
pub(crate) fn use_mock_keyring() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_extracts_user_and_password() {
        let (cred, sanitized) =
            split_credentials_from_url("https://alice:s3cret@github.com/acme/repo.git").unwrap();
        assert_eq!(cred.username, "alice");
        assert_eq!(cred.password, "s3cret");
        assert_eq!(sanitized, "https://github.com/acme/repo.git");
    }

    #[test]
    fn split_extracts_token_only_form() {
        let (cred, sanitized) =
            split_credentials_from_url("https://ghp_token123@github.com/acme/repo.git").unwrap();
        assert_eq!(cred.username, "ghp_token123");
        assert_eq!(cred.password, "");
        assert_eq!(sanitized, "https://github.com/acme/repo.git");
    }

    #[test]
    fn split_decodes_percent_encoding() {
        let (cred, _) =
            split_credentials_from_url("https://user:p%40ss%2Fword@example.com/r.git").unwrap();
        assert_eq!(cred.password, "p@ss/word");
    }

    #[test]
    fn split_none_without_userinfo() {
        assert!(split_credentials_from_url("https://github.com/acme/repo.git").is_none());
    }

    #[test]
    fn split_none_for_ssh() {
        assert!(split_credentials_from_url("git@github.com:acme/repo.git").is_none());
        assert!(split_credentials_from_url("ssh://git@github.com/acme/repo.git").is_none());
    }

    #[test]
    fn split_keeps_port_and_path() {
        let (_, sanitized) =
            split_credentials_from_url("https://u:p@gitlab.example.com:8443/g/r.git").unwrap();
        assert_eq!(sanitized, "https://gitlab.example.com:8443/g/r.git");
    }

    #[test]
    fn https_host_strips_userinfo_and_lowercases() {
        assert_eq!(
            https_host("https://u:p@GitHub.com/acme/repo.git").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            https_host("https://gitlab.example.com:8443/g/r.git").as_deref(),
            Some("gitlab.example.com:8443")
        );
        assert_eq!(https_host("git@github.com:acme/repo.git"), None);
    }

    #[test]
    fn askpass_script_answers_by_prompt() {
        // Verify the script routes "Username"/"Password" prompts to the right
        // environment variable — the contract git relies on.
        assert!(ASKPASS_SCRIPT.contains("*[Uu]sername*"));
        assert!(ASKPASS_SCRIPT.contains(ENV_USERNAME));
        assert!(ASKPASS_SCRIPT.contains(ENV_PASSWORD));
        // No secrets baked into the script itself.
        assert!(!ASKPASS_SCRIPT.contains("token"));
    }
}
