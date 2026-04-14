pub mod central_repo;
pub mod content_hash;
pub mod crypto;
pub mod error;
pub mod git_backup;
pub mod git_fetcher;
pub mod install_cancel;
pub mod installer;
pub mod migrations;
pub mod plugins;
pub mod project_scanner;
pub mod scanner;
pub mod skill_metadata;
pub mod skill_store;
pub mod skillsmp_api;
pub mod skillssh_api;
pub mod sync_engine;
pub mod tool_adapters;

// Re-export commonly used types
pub use error::{AppError, ErrorKind};
pub use skill_store::{
    AgentConfigRecord, AgentSkillOwnership, DiscoveredSkillRecord, ManagedPluginRecord, PackRecord,
    ProjectRecord, ScenarioPluginRecord, ScenarioRecord, ScenarioSkillToolToggleRecord,
    SkillRecord, SkillStore, SkillTargetRecord,
};
pub use sync_engine::SyncMode;
