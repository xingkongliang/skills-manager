use super::{
    client::{ensure_success, source_url},
    models::{validate_identity, Board, Page, SearchResponse, Skill},
};
use crate::core::error::AppError;
use std::collections::HashSet;

pub fn adapt_page(response: SearchResponse, board: Board) -> Result<Page, AppError> {
    ensure_success(&response.message)?;
    if response.page == 0 || !(1..=100).contains(&response.page_size) {
        return Err(AppError::network("Invalid ByteDance pagination response"));
    }
    let mut seen = HashSet::new();
    let mut skills = Vec::new();
    for item in response.data {
        validate_identity(&item.identifier, &item.repository)?;
        if item.name.trim().is_empty() {
            return Err(AppError::network("ByteDance skill has no name"));
        }
        let id = format!("{}:{}", item.repository, item.identifier);
        if !seen.insert(id.clone()) {
            continue;
        }
        let downloads = match board {
            Board::All => item.download_total,
            Board::Biweekly => item.download_biweekly,
            Board::Monthly => item.download_monthly,
        };
        skills.push(Skill {
            id,
            url: source_url(&item.identifier, &item.repository),
            identifier: item.identifier,
            repository: item.repository,
            name: item.name,
            description: item.description.unwrap_or_default(),
            version: item.newest_version.map(|v| v.version),
            downloads,
            download_total: item.download_total,
            download_biweekly: item.download_biweekly,
            download_monthly: item.download_monthly,
            disabled: item.disabled || item.all_versions_disabled,
            no_permission: item.no_permission,
        });
    }
    Ok(Page {
        skills,
        total: response.count,
        page: response.page,
        page_size: response.page_size,
    })
}
