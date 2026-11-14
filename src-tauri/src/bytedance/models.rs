use crate::core::error::AppError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Board {
    All,
    Biweekly,
    Monthly,
}

impl Board {
    pub fn order_by(self) -> &'static str {
        match self {
            Self::All => "download_desc",
            Self::Biweekly => "biweekly_download_desc",
            Self::Monthly => "monthly_download_desc",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Envelope<T> {
    pub message: String,
    pub data: T,
}

#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub message: String,
    pub data: Vec<Package>,
    pub count: u64,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Deserialize)]
pub struct Version {
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct Permission {
    pub download: bool,
}

#[derive(Debug, Deserialize)]
pub struct Package {
    pub identifier: String,
    pub repository: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub newest_version: Option<Version>,
    #[serde(default)]
    pub download_total: u64,
    #[serde(default)]
    pub download_biweekly: u64,
    #[serde(default)]
    pub download_monthly: u64,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub all_versions_disabled: bool,
    #[serde(default)]
    pub no_permission: bool,
    #[serde(default)]
    pub user_permission: Option<Permission>,
}

pub fn validate_identity(identifier: &str, repository: &str) -> Result<(), AppError> {
    if !identifier.starts_with("skills:")
        || identifier.len() <= 7
        || repository.trim().is_empty()
        || identifier.chars().any(char::is_control)
        || repository.chars().any(char::is_control)
    {
        return Err(AppError::invalid_input("Invalid ByteDance skill identity"));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct Component {
    pub version: String,
    #[serde(default)]
    pub disabled: bool,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
pub struct Asset {
    pub download_link: String,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub blob_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub identifier: String,
    pub repository: String,
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub downloads: u64,
    pub download_total: u64,
    pub download_biweekly: u64,
    pub download_monthly: u64,
    pub disabled: bool,
    pub no_permission: bool,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub skills: Vec<Skill>,
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
}
