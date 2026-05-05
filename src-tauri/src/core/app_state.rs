use std::sync::Arc;

use anyhow::{Context, Result};

use super::{central_repo, scenario_service, skill_store::SkillStore, sync_metadata, tool_service};

pub fn initialize_store() -> Result<Arc<SkillStore>> {
    central_repo::ensure_central_repo().context("Failed to create central repo")?;

    let db_path = central_repo::db_path();
    let store = Arc::new(SkillStore::new(&db_path).context("Failed to initialize database")?);
    tool_service::migrate_legacy_tool_keys(&store)
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("Failed to migrate legacy tool keys")?;
    if sync_metadata::metadata_exists() {
        sync_metadata::reindex_from_metadata(&store)
            .context("Failed to reindex from sync metadata")?;
    }
    scenario_service::ensure_default_startup_scenario(&store)
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("Failed to initialize startup scenario")?;
    Ok(store)
}
