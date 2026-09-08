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
        Ok(payload) => Ok(Some(serde_json::from_str(&payload).with_context(|| {
            format!("Corrupted keychain entry for {host}")
        })?)),
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

/// Attach a credentials callback to libgit2 network operations against `url`.
///
/// Sources, in order: the credential this app stored in the OS keychain for the
/// host, then the user's git credential helper (osxkeychain, Git Credential
/// Manager, libsecret) — the same place system git would have looked — then an
/// ssh-agent key for ssh remotes.
///
/// The helper fallback is the one that matters for skill sources. The keychain
/// only holds hosts this app connected itself, which in practice means the
/// backup remote; a skill living on a private GitLab or Gitea has no entry
/// there and would still fail with a keychain-only callback.
///
/// Without any callback libgit2 reports "no callback set" (#379). A desktop
/// launch hits that whenever the system-git attempt fails first: a GUI process
/// has a leaner PATH than a shell and cannot prompt, so it falls through to
/// libgit2 — which is why the same skill checks fine from the CLI.
///
/// Each source is offered once. libgit2 re-invokes this callback after every
/// rejection, so a source that answers unconditionally would spin forever.
pub fn install_git2_credentials(callbacks: &mut git2::RemoteCallbacks<'_>, url: &str) {
    // Resolve the host now, but read the keychain only from inside the callback:
    // libgit2 invokes it solely when the remote actually demands credentials, and
    // almost every skill source is a public repository that never will. Reading
    // eagerly would touch the keychain on every update check for nothing — and on
    // macOS each unsigned build that does so raises an authorization prompt.
    let host = https_host(url);
    let host_label = host.clone().unwrap_or_else(|| "this remote".to_string());
    let mut tried_stored = false;
    let mut tried_helper = false;
    let mut tried_agent = false;

    callbacks.credentials(move |url, username_from_url, allowed| {
        if allowed.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
            if !tried_stored {
                tried_stored = true;
                if let Some(cred) = host
                    .as_deref()
                    .and_then(|h| load_credential(h).ok().flatten())
                {
                    return git2::Cred::userpass_plaintext(&cred.username, &cred.password);
                }
            }
            if !tried_helper {
                tried_helper = true;
                if let Ok(config) = git2::Config::open_default() {
                    if let Ok(cred) =
                        git2::Cred::credential_helper(&config, url, username_from_url)
                    {
                        return Ok(cred);
                    }
                }
            }
        }
        if allowed.contains(git2::CredentialType::SSH_KEY) && !tried_agent {
            tried_agent = true;
            if let Some(user) = username_from_url {
                return git2::Cred::ssh_key_from_agent(user);
            }
        }
        if allowed.contains(git2::CredentialType::DEFAULT) {
            return git2::Cred::default();
        }
        // Phrased for the user, not for libgit2: this string reaches the UI.
        Err(git2::Error::from_str(&format!(
            "Authentication failed: no credentials available for {host_label}. \
             Sign in to that host with git (for example `gh auth setup-git` for \
             GitHub), then check for updates again."
        )))
    });
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
    /// The regression #379 reports: libgit2 got `None` for callbacks and
    /// answered "no callback set", which reached the user verbatim.
    ///
    /// Uses a host this app never stores credentials for, so it exercises the
    /// helper fallback and the final message without reading the app's own
    /// keychain entry — a test binary is unsigned and reading that entry would
    /// block on a macOS authorization prompt.
    #[test]
    #[ignore = "hits the network"]
    fn libgit2_is_given_a_credentials_callback() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(dir.path()).unwrap();
        let url = "https://gitlab.com/skills-manager-no-such-owner/nope.git";
        let mut remote = repo.remote_anonymous(url).unwrap();
        let mut callbacks = git2::RemoteCallbacks::new();
        install_git2_credentials(&mut callbacks, url);

        // RemoteConnection is not Debug, so unwrap the error by hand.
        let msg = match remote.connect_auth(git2::Direction::Fetch, Some(callbacks), None) {
            Ok(_) => panic!("a private remote must not connect anonymously"),
            Err(e) => e.to_string(),
        };

        assert!(
            !msg.contains("no callback set"),
            "libgit2 still has no credentials callback: {msg}"
        );
    }

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
