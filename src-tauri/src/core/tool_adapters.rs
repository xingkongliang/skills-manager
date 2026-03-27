use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct ToolAdapter {
    pub key: String,
    pub display_name: String,
    pub relative_skills_dir: String,
    pub relative_detect_dir: String,
    /// Additional directories to scan for skills (e.g. plugin/marketplace dirs).
    /// These are only used for discovery, not for deployment.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub additional_scan_dirs: Vec<String>,
    /// When set, overrides the computed skills_dir with this absolute path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub override_skills_dir: Option<String>,
    /// Whether this is a user-defined custom agent (not built-in).
    #[serde(default)]
    pub is_custom: bool,
}

/// Serializable custom tool definition stored in settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomToolDef {
    pub key: String,
    pub display_name: String,
    pub skills_dir: String,
}

impl ToolAdapter {
    fn home() -> PathBuf {
        dirs::home_dir().expect("Cannot determine home directory")
    }

    fn candidate_paths(relative: &str) -> Vec<PathBuf> {
        let mut candidates = vec![Self::home().join(relative)];

        if let Some(suffix) = relative.strip_prefix(".config/") {
            if let Some(config_dir) = dirs::config_dir() {
                let config_path = config_dir.join(suffix);
                if !candidates.contains(&config_path) {
                    candidates.push(config_path);
                }
            }
        }

        candidates
    }

    fn select_existing_or_default(paths: &[PathBuf]) -> PathBuf {
        paths
            .iter()
            .find(|path| path.exists())
            .cloned()
            .unwrap_or_else(|| paths[0].clone())
    }

    pub fn skills_dir(&self) -> PathBuf {
        if let Some(ref abs) = self.override_skills_dir {
            return PathBuf::from(abs);
        }
        let candidates = Self::candidate_paths(&self.relative_skills_dir);
        Self::select_existing_or_default(&candidates)
    }

    /// Returns all directories to scan for skills: the primary skills_dir plus any additional scan dirs.
    pub fn all_scan_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.skills_dir()];
        for rel in &self.additional_scan_dirs {
            let candidates = Self::candidate_paths(rel);
            for c in candidates {
                if c.exists() && !dirs.contains(&c) {
                    dirs.push(c);
                }
            }
        }
        dirs
    }

    pub fn is_installed(&self) -> bool {
        // Product decision: when users explicitly provide a skills path (override/custom),
        // we treat the tool as available so sync can proceed without probing vendor install state.
        if self.is_custom || self.override_skills_dir.is_some() {
            return true;
        }
        Self::candidate_paths(&self.relative_detect_dir)
            .iter()
            .any(|path| path.exists())
    }

    /// Whether this adapter's skills_dir has been overridden from the default.
    pub fn has_path_override(&self) -> bool {
        self.override_skills_dir.is_some()
    }
}

pub fn default_tool_adapters() -> Vec<ToolAdapter> {
    vec![
        ToolAdapter {
            key: "cursor".into(),
            display_name: "Cursor".into(),
            relative_skills_dir: ".cursor/skills".into(),
            relative_detect_dir: ".cursor".into(),
            additional_scan_dirs: vec![],
            override_skills_dir: None,
            is_custom: false,
        },
        ToolAdapter {
            key: "claude_code".into(),
            display_name: "Claude Code".into(),
            relative_skills_dir: ".claude/skills".into(),
            relative_detect_dir: ".claude".into(),
            additional_scan_dirs: vec![
                ".claude/plugins/cache".into(),
                ".claude/plugins/marketplaces".into(),
            ],
            override_skills_dir: None,
            is_custom: false,
        },
        ToolAdapter {
            key: "codex".into(),
            display_name: "Codex".into(),
            relative_skills_dir: ".codex/skills".into(),
            relative_detect_dir: ".codex".into(),
            additional_scan_dirs: vec![],
            override_skills_dir: None,
            is_custom: false,
        },
        ToolAdapter {
            key: "opencode".into(),
            display_name: "OpenCode".into(),
            relative_skills_dir: ".config/opencode/skills".into(),
            relative_detect_dir: ".config/opencode".into(),
            additional_scan_dirs: vec![],
            override_skills_dir: None,
            is_custom: false,
        },
        ToolAdapter {
            key: "antigravity".into(),
            display_name: "Antigravity".into(),
            relative_skills_dir: ".gemini/antigravity/global_skills".into(),
            relative_detect_dir: ".gemini/antigravity".into(),
            additional_scan_dirs: vec![],
            override_skills_dir: None,
            is_custom: false,
        },
        ToolAdapter {
            key: "amp".into(),
            display_name: "Amp".into(),
            relative_skills_dir: ".config/agents/skills".into(),
            relative_detect_dir: ".config/agents".into(),
            additional_scan_dirs: vec![],
            override_skills_dir: None,
            is_custom: false,
        },
        ToolAdapter {
            key: "kilo_code".into(),
            display_name: "Kilo Code".into(),
            relative_skills_dir: ".kilocode/skills".into(),
            relative_detect_dir: ".kilocode".into(),
            additional_scan_dirs: vec![],
            override_skills_dir: None,
            is_custom: false,
        },
        ToolAdapter {
            key: "roo_code".into(),
            display_name: "Roo Code".into(),
            relative_skills_dir: ".roo/skills".into(),
            relative_detect_dir: ".roo".into(),
            additional_scan_dirs: vec![],
            override_skills_dir: None,
            is_custom: false,
        },
        ToolAdapter {
            key: "goose".into(),
            display_name: "Goose".into(),
            relative_skills_dir: ".config/goose/skills".into(),
            relative_detect_dir: ".config/goose".into(),
            additional_scan_dirs: vec![],
            override_skills_dir: None,
            is_custom: false,
        },
        ToolAdapter {
            key: "gemini_cli".into(),
            display_name: "Gemini CLI".into(),
            relative_skills_dir: ".gemini/skills".into(),
            relative_detect_dir: ".gemini".into(),
            additional_scan_dirs: vec![],
            override_skills_dir: None,
            is_custom: false,
        },
        ToolAdapter {
            key: "github_copilot".into(),
            display_name: "GitHub Copilot".into(),
            relative_skills_dir: ".copilot/skills".into(),
            relative_detect_dir: ".copilot".into(),
            additional_scan_dirs: vec![],
            override_skills_dir: None,
            is_custom: false,
        },
        ToolAdapter {
            key: "openclaw".into(),
            display_name: "OpenClaw".into(),
            relative_skills_dir: ".openclaw/skills".into(),
            relative_detect_dir: ".openclaw".into(),
            additional_scan_dirs: vec![],
            override_skills_dir: None,
            is_custom: false,
        },
        ToolAdapter {
            key: "droid".into(),
            display_name: "Droid".into(),
            relative_skills_dir: ".factory/skills".into(),
            relative_detect_dir: ".factory".into(),
            additional_scan_dirs: vec![],
            override_skills_dir: None,
            is_custom: false,
        },
        ToolAdapter {
            key: "windsurf".into(),
            display_name: "Windsurf".into(),
            relative_skills_dir: ".codeium/windsurf/skills".into(),
            relative_detect_dir: ".codeium/windsurf".into(),
            additional_scan_dirs: vec![],
            override_skills_dir: None,
            is_custom: false,
        },
        ToolAdapter {
            key: "trae".into(),
            display_name: "TRAE IDE".into(),
            relative_skills_dir: ".trae/skills".into(),
            relative_detect_dir: ".trae".into(),
            additional_scan_dirs: vec![],
            override_skills_dir: None,
            is_custom: false,
        },
    ]
}

/// Read custom tool path overrides from store.
pub fn custom_tool_paths(store: &crate::core::skill_store::SkillStore) -> HashMap<String, String> {
    store
        .get_setting("custom_tool_paths")
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default()
}

/// Read user-defined custom tools from store.
pub fn custom_tools(store: &crate::core::skill_store::SkillStore) -> Vec<CustomToolDef> {
    store
        .get_setting("custom_tools")
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default()
}

/// Returns all tool adapters: built-in (with path overrides applied) + custom tools.
pub fn all_tool_adapters(store: &crate::core::skill_store::SkillStore) -> Vec<ToolAdapter> {
    let overrides = custom_tool_paths(store);
    let customs = custom_tools(store);

    let mut adapters: Vec<ToolAdapter> = default_tool_adapters()
        .into_iter()
        .map(|mut a| {
            if let Some(path) = overrides.get(&a.key) {
                a.override_skills_dir = Some(path.clone());
            }
            a
        })
        .collect();

    for ct in customs {
        adapters.push(ToolAdapter {
            key: ct.key,
            display_name: ct.display_name,
            relative_skills_dir: String::new(),
            relative_detect_dir: String::new(),
            additional_scan_dirs: vec![],
            override_skills_dir: Some(ct.skills_dir),
            is_custom: true,
        });
    }

    adapters
}

#[allow(dead_code)]
pub fn find_adapter(key: &str) -> Option<ToolAdapter> {
    default_tool_adapters().into_iter().find(|a| a.key == key)
}

/// Find an adapter by key, considering custom tools and path overrides.
pub fn find_adapter_with_store(
    store: &crate::core::skill_store::SkillStore,
    key: &str,
) -> Option<ToolAdapter> {
    if let Some(mut adapter) = default_tool_adapters().into_iter().find(|a| a.key == key) {
        if let Some(path) = custom_tool_paths(store).get(key) {
            adapter.override_skills_dir = Some(path.clone());
        }
        return Some(adapter);
    }

    custom_tools(store)
        .into_iter()
        .find(|ct| ct.key == key)
        .map(|ct| ToolAdapter {
            key: ct.key,
            display_name: ct.display_name,
            relative_skills_dir: String::new(),
            relative_detect_dir: String::new(),
            additional_scan_dirs: vec![],
            override_skills_dir: Some(ct.skills_dir),
            is_custom: true,
        })
}

/// Returns adapters that are installed and not in the disabled list.
pub fn enabled_installed_adapters(
    store: &crate::core::skill_store::SkillStore,
) -> Vec<ToolAdapter> {
    let disabled: Vec<String> = store
        .get_setting("disabled_tools")
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default();
    all_tool_adapters(store)
        .into_iter()
        .filter(|a| a.is_installed() && !disabled.contains(&a.key))
        .collect()
}
