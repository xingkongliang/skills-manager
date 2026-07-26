//! AI translation API key storage.
//!
//! The key never enters SQLite or frontend-readable settings. It is stored in
//! the operating system keychain and loaded only by the backend when making a
//! translation request.

use anyhow::{Context, Result};

const KEYRING_SERVICE: &str = "skills-manager-ai-translation";
const KEYRING_ACCOUNT: &str = "default";

fn keyring_entry() -> Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .context("Failed to open AI translation keychain entry")
}

pub fn store_api_key(api_key: &str) -> Result<()> {
    keyring_entry()?
        .set_password(api_key)
        .context("Failed to store AI translation API key in OS keychain")?;
    log::info!("AI translation API key stored in OS keychain");
    Ok(())
}

pub fn load_api_key() -> Result<Option<String>> {
    match keyring_entry()?.get_password() {
        Ok(api_key) => Ok(Some(api_key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error).context("Failed to read AI translation API key"),
    }
}

pub fn delete_api_key() -> Result<()> {
    match keyring_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            log::info!("AI translation API key removed from OS keychain");
            Ok(())
        }
        Err(error) => Err(error).context("Failed to remove AI translation API key"),
    }
}
