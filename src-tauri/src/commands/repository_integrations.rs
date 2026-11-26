use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::core::{error::AppError, skill_store::SkillStore};

#[derive(Serialize)]
pub struct OpenSpecObservation {
    cli_path: Option<String>,
    project_config_found: bool,
    workflow_found: bool,
    workflow_check_supported: bool,
}

#[derive(Serialize)]
pub struct PonytailObservation {
    plugin_files_found: Option<bool>,
    cursor_hook_entries_found: Option<bool>,
}

fn selected_project(
    store: &SkillStore,
    project_id: Option<String>,
) -> Result<Option<PathBuf>, AppError> {
    project_id
        .map(|id| {
            store
                .get_project_by_id(&id)
                .map_err(AppError::db)?
                .filter(|project| project.workspace_type == "project")
                .map(|project| PathBuf::from(project.path))
                .ok_or_else(|| AppError::not_found("Project not found"))
        })
        .transpose()
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    const MAX_BYTES: u64 = 1024 * 1024;
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return None;
    }
    serde_json::from_reader(fs::File::open(path).ok()?.take(MAX_BYTES)).ok()
}

fn codex_ponytail_cache() -> Option<bool> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))?;
    let cache = home.join("plugins/cache/ponytail/ponytail");
    let versions = match fs::read_dir(cache) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(false),
        Err(_) => return None,
    };
    Some(versions.flatten().any(|version| {
        [".codex-plugin/plugin.json", "plugin.json"]
            .iter()
            .any(|file| {
                read_json(&version.path().join(file))
                    .is_some_and(|manifest| manifest["name"] == "ponytail")
            })
    }))
}

fn cursor_ponytail_entries(base: &Path) -> Option<bool> {
    let config = base.join(".cursor/hooks.json");
    if !config.exists() {
        return Some(false);
    }
    let value = read_json(&config)?;
    let has_entry = |event: &str, script: &str| {
        value["hooks"][event].as_array().is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry["command"]
                    .as_str()
                    .is_some_and(|command| command.replace('\\', "/").contains(script))
            })
        })
    };
    Some(
        has_entry("sessionStart", "/hooks/ponytail-activate.js")
            && has_entry("beforeSubmitPrompt", "/hooks/ponytail-mode-tracker.js"),
    )
}

#[tauri::command]
pub async fn observe_ponytail(
    project_id: Option<String>,
    agent: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<PonytailObservation, AppError> {
    let project_path = selected_project(&store, project_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut observation = PonytailObservation {
            plugin_files_found: None,
            cursor_hook_entries_found: None,
        };
        match agent.as_str() {
            "codex" => observation.plugin_files_found = codex_ponytail_cache(),
            "claude_code" => {
                let home = std::env::var_os("CLAUDE_CONFIG_DIR")
                    .map(PathBuf::from)
                    .or_else(|| dirs::home_dir().map(|home| home.join(".claude")));
                if let Some(home) = home {
                    let registry = home.join("plugins/installed_plugins.json");
                    observation.plugin_files_found = if registry.exists() {
                        read_json(&registry).and_then(|value| {
                            value["plugins"]
                                .as_object()
                                .map(|plugins| plugins.contains_key("ponytail@ponytail"))
                        })
                    } else {
                        Some(false)
                    };
                }
            }
            "cursor" => {
                if let Some(base) = project_path.or_else(dirs::home_dir) {
                    observation.cursor_hook_entries_found = cursor_ponytail_entries(&base);
                }
            }
            _ => {}
        }
        Ok(observation)
    })
    .await?
}

fn openspec_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    #[cfg(windows)]
    let names = ["openspec.exe", "openspec.cmd", "openspec.bat"];
    #[cfg(not(windows))]
    let names = ["openspec"];

    std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .flat_map(|dir| names.map(|name| dir.join(name)))
        .find(|file| {
            let Ok(metadata) = file.metadata() else {
                return false;
            };
            if !metadata.is_file() {
                return false;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            true
        })
}

fn openspec_workflow_paths(agent: &str) -> &'static [&'static str] {
    match agent {
        "codex" => &[
            ".agents/skills/openspec-propose/SKILL.md",
            ".codex/skills/openspec-propose/SKILL.md",
        ],
        "claude_code" => &[
            ".claude/skills/openspec-propose/SKILL.md",
            ".claude/commands/opsx/propose.md",
        ],
        "cursor" => &[
            ".cursor/skills/openspec-propose/SKILL.md",
            ".cursor/commands/opsx-propose.md",
        ],
        "opencode" => &[
            ".opencode/skills/openspec-propose/SKILL.md",
            ".opencode/commands/opsx-propose.md",
        ],
        _ => &[],
    }
}

#[tauri::command]
pub async fn observe_openspec(
    project_id: Option<String>,
    agent: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<OpenSpecObservation, AppError> {
    let project = selected_project(&store, project_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let project_path = project.as_deref();
        let workflow_paths = openspec_workflow_paths(&agent);
        Ok(OpenSpecObservation {
            cli_path: openspec_on_path().map(|path| path.to_string_lossy().into_owned()),
            project_config_found: project_path.is_some_and(|path| {
                [
                    "openspec/config.yaml",
                    "openspec/config.yml",
                    ".openspec.yaml",
                ]
                .iter()
                .any(|file| path.join(file).is_file())
            }),
            workflow_found: project_path
                .is_some_and(|path| workflow_paths.iter().any(|file| path.join(file).is_file())),
            workflow_check_supported: !workflow_paths.is_empty(),
        })
    })
    .await?
}
