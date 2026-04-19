use anyhow::{Context, Result};
use std::path::Path;

pub mod disclosure;

#[derive(Debug, Clone, Copy)]
pub enum SyncMode {
    Symlink,
    Copy,
}

impl SyncMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncMode::Symlink => "symlink",
            SyncMode::Copy => "copy",
        }
    }
}

pub fn sync_mode_for_tool(_tool_key: &str, configured_mode: Option<&str>) -> SyncMode {
    match configured_mode {
        Some("copy") => SyncMode::Copy,
        Some("symlink") => SyncMode::Symlink,
        _ => SyncMode::Symlink,
    }
}

pub fn sync_skill(source: &Path, target: &Path, mode: SyncMode) -> Result<SyncMode> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create parent dir {:?}", parent))?;
    }

    // Remove existing target
    remove_target(target).ok();

    match mode {
        SyncMode::Symlink => {
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(source, target).with_context(|| {
                    format!("Failed to create symlink {:?} -> {:?}", target, source)
                })?;
                Ok(SyncMode::Symlink)
            }
            #[cfg(not(unix))]
            {
                copy_dir_recursive(source, target)?;
                Ok(SyncMode::Copy)
            }
        }
        SyncMode::Copy => {
            copy_dir_recursive(source, target)?;
            Ok(SyncMode::Copy)
        }
    }
}

pub fn remove_target(target: &Path) -> Result<()> {
    if target.is_symlink() {
        std::fs::remove_file(target)?;
    } else if target.is_dir() {
        std::fs::remove_dir_all(target)?;
    } else if target.exists() {
        std::fs::remove_file(target)?;
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ft.is_symlink() {
            // Recreate symlinks as-is instead of trying to copy them as files.
            // Many skills contain internal symlinks (e.g., skill-name/skill-name -> parent).
            // Skip circular symlinks that point back to src or an ancestor.
            if let Ok(link_target) = std::fs::read_link(entry.path()) {
                if let Ok(resolved) = entry.path().canonicalize() {
                    if src.starts_with(&resolved) {
                        continue;
                    }
                }
                #[cfg(unix)]
                std::os::unix::fs::symlink(&link_target, &dest_path).ok();
            }
        } else if ft.is_dir() {
            let name = entry.file_name();
            if name == ".git" {
                continue;
            }
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

/// Summary of changes made by `reconcile_agent_dir`.
#[derive(Debug, Default)]
pub struct ReconcileReport {
    pub added: usize,
    pub removed: usize,
    pub rendered_routers: usize,
}

/// Reconcile an agent's skills directory against a desired state derived from
/// the given packs and disclosure mode.
///
/// Behavior:
/// - Materializes missing skills via symlink (copying on non-unix).
/// - Writes/updates router SKILL.md files for non-essential packs.
/// - Removes SM-managed entries that are no longer desired.
///
/// Native (non-SM-managed) entries are left untouched.
pub fn reconcile_agent_dir(
    agent_skills_dir: &Path,
    packs: &[disclosure::PackWithSkills<'_>],
    mode: crate::skill_store::DisclosureMode,
    vault_root: &Path,
) -> Result<ReconcileReport> {
    use disclosure::{resolve_desired_state, EntryKind};
    use std::collections::HashSet;
    use std::fs;

    fs::create_dir_all(agent_skills_dir).ok();

    let desired = resolve_desired_state(agent_skills_dir, packs, mode);
    let desired_paths: HashSet<_> = desired.iter().map(|e| e.target_path.clone()).collect();
    let mut report = ReconcileReport::default();

    // Add missing / update stale routers
    for entry in &desired {
        match &entry.kind {
            EntryKind::Skill { skill_name } => {
                let source = vault_root.join(skill_name);
                if !entry.target_path.exists() {
                    sync_skill(&source, &entry.target_path, SyncMode::Symlink)
                        .with_context(|| format!("sync skill {skill_name}"))?;
                    report.added += 1;
                }
            }
            EntryKind::Router { pack_name } => {
                let pack = packs
                    .iter()
                    .find(|p| p.pack.name == *pack_name)
                    .expect("router entry must correspond to a known pack");
                let content = crate::router_render::render_router_skill_md(
                    pack.pack,
                    pack.skills,
                    vault_root,
                );
                let target_dir = entry.target_path.clone();
                fs::create_dir_all(&target_dir).ok();
                let md_path = target_dir.join("SKILL.md");
                let needs_write = match fs::read_to_string(&md_path) {
                    Ok(existing) => existing != content,
                    Err(_) => true,
                };
                if needs_write {
                    let is_new = !md_path.exists();
                    fs::write(&md_path, &content)
                        .with_context(|| format!("write router {pack_name}"))?;
                    report.rendered_routers += 1;
                    if is_new {
                        report.added += 1;
                    }
                }
            }
        }
    }

    // Remove SM-managed entries no longer desired
    if agent_skills_dir.exists() {
        for entry in fs::read_dir(agent_skills_dir)? {
            let entry = entry?;
            let p = entry.path();
            if desired_paths.contains(&p) {
                continue;
            }
            if !is_sm_managed(&p)? {
                continue;
            }
            if p.is_dir() {
                fs::remove_dir_all(&p)?;
            } else {
                fs::remove_file(&p)?;
            }
            report.removed += 1;
        }
    }

    Ok(report)
}

/// Heuristic detection of entries SM previously wrote:
/// - a symlink that resolves into `.skills-manager/skills/`
/// - a `pack-*` directory whose `SKILL.md` contains our router markers
fn is_sm_managed(path: &Path) -> Result<bool> {
    use std::fs;
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            if let Ok(target) = fs::read_link(path) {
                return Ok(target.to_string_lossy().contains(".skills-manager/skills"));
            }
        }
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.starts_with("pack-") {
        if let Ok(s) = fs::read_to_string(path.join("SKILL.md")) {
            return Ok(s.contains("Router for pack") || s.contains("# Pack:"));
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // ── sync_mode_for_tool ──

    #[test]
    fn sync_mode_defaults_to_symlink() {
        assert!(matches!(
            sync_mode_for_tool("claude-code", None),
            SyncMode::Symlink
        ));
    }

    #[test]
    fn sync_mode_cursor_defaults_to_symlink() {
        assert!(matches!(
            sync_mode_for_tool("cursor", None),
            SyncMode::Symlink
        ));
    }

    #[test]
    fn sync_mode_explicit_copy_overrides_default() {
        assert!(matches!(
            sync_mode_for_tool("claude-code", Some("copy")),
            SyncMode::Copy
        ));
    }

    #[test]
    fn sync_mode_explicit_symlink_is_respected() {
        assert!(matches!(
            sync_mode_for_tool("cursor", Some("symlink")),
            SyncMode::Symlink
        ));
    }

    #[test]
    fn sync_mode_unknown_config_falls_back_to_symlink() {
        assert!(matches!(
            sync_mode_for_tool("cursor", Some("invalid")),
            SyncMode::Symlink
        ));
        assert!(matches!(
            sync_mode_for_tool("claude-code", Some("invalid")),
            SyncMode::Symlink
        ));
    }

    #[test]
    fn sync_mode_as_str() {
        assert_eq!(SyncMode::Symlink.as_str(), "symlink");
        assert_eq!(SyncMode::Copy.as_str(), "copy");
    }

    // ── sync_skill (filesystem) ──

    #[test]
    fn sync_skill_copy_creates_directory_with_files() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();

        let mode = sync_skill(&src, &tgt, SyncMode::Copy).unwrap();
        assert!(matches!(mode, SyncMode::Copy));
        assert!(tgt.join("SKILL.md").exists());
        assert_eq!(fs::read_to_string(tgt.join("SKILL.md")).unwrap(), "# hello");
    }

    #[cfg(unix)]
    #[test]
    fn sync_skill_symlink_creates_symlink() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();

        let mode = sync_skill(&src, &tgt, SyncMode::Symlink).unwrap();
        assert!(matches!(mode, SyncMode::Symlink));
        assert!(tgt.is_symlink());
    }

    #[cfg(not(unix))]
    #[test]
    fn sync_skill_symlink_falls_back_to_copy_on_windows() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();

        let mode = sync_skill(&src, &tgt, SyncMode::Symlink).unwrap();
        assert!(matches!(mode, SyncMode::Copy));
        assert!(tgt.join("SKILL.md").exists());
        assert_eq!(fs::read_to_string(tgt.join("SKILL.md")).unwrap(), "# hello");
    }

    #[test]
    fn sync_skill_replaces_existing_target() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("new.md"), "new").unwrap();

        // Pre-existing target directory
        fs::create_dir_all(&tgt).unwrap();
        fs::write(tgt.join("old.md"), "old").unwrap();

        sync_skill(&src, &tgt, SyncMode::Copy).unwrap();
        assert!(tgt.join("new.md").exists());
        assert!(!tgt.join("old.md").exists());
    }

    // ── copy_dir_recursive ──

    #[test]
    fn copy_dir_recursive_skips_dot_git() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(src.join(".git")).unwrap();
        fs::write(src.join(".git/config"), "git config").unwrap();
        fs::create_dir_all(src.join("subdir")).unwrap();
        fs::write(src.join("subdir/file.md"), "content").unwrap();
        fs::write(src.join("root.md"), "root").unwrap();

        let dst = tmp.path().join("dst");
        copy_dir_recursive(&src, &dst).unwrap();

        assert!(!dst.join(".git").exists());
        assert!(dst.join("subdir/file.md").exists());
        assert!(dst.join("root.md").exists());
    }

    // ── remove_target ──

    #[test]
    fn remove_target_removes_directory() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join("to_remove");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("file.txt"), "data").unwrap();

        remove_target(&dir).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn remove_target_removes_file() {
        let tmp = tempdir().unwrap();
        let file = tmp.path().join("file.txt");
        fs::write(&file, "data").unwrap();

        remove_target(&file).unwrap();
        assert!(!file.exists());
    }

    #[cfg(unix)]
    #[test]
    fn remove_target_removes_symlink() {
        let tmp = tempdir().unwrap();
        let real = tmp.path().join("real");
        fs::create_dir_all(&real).unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        remove_target(&link).unwrap();
        assert!(!link.exists());
        assert!(real.exists()); // original untouched
    }

    #[test]
    fn remove_target_nonexistent_is_ok() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("does_not_exist");
        assert!(remove_target(&path).is_ok());
    }

    // ── reconcile_agent_dir integration ──

    use super::disclosure::PackWithSkills;
    use crate::skill_store::{DisclosureMode, PackRecord, SkillRecord};

    fn tpack(name: &str, essential: bool, router_description: Option<&str>) -> PackRecord {
        PackRecord {
            id: format!("p-{name}"),
            name: name.into(),
            description: None,
            icon: None,
            color: None,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
            router_description: router_description.map(str::to_string),
            router_body: None,
            is_essential: essential,
            router_updated_at: None,
        }
    }

    fn tskill(name: &str) -> SkillRecord {
        SkillRecord {
            id: format!("s-{name}"),
            name: name.into(),
            description: Some(format!("{name} description.")),
            source_type: "local".into(),
            source_ref: None,
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: format!("/vault/{name}"),
            content_hash: None,
            enabled: true,
            created_at: 0,
            updated_at: 0,
            status: "active".into(),
            update_status: "idle".into(),
            last_checked_at: None,
            last_check_error: None,
        }
    }

    /// Build a vault root that looks like `.../.skills-manager/skills/` so the
    /// `is_sm_managed` symlink heuristic recognizes our pre-populated symlinks.
    fn sm_vault(tmp: &Path) -> std::path::PathBuf {
        let p = tmp.join(".skills-manager/skills");
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn stub_skill_dir(vault_root: &Path, name: &str) -> std::path::PathBuf {
        let dir = vault_root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), format!("# {name}\n")).unwrap();
        dir
    }

    #[cfg(unix)]
    #[test]
    fn switching_full_to_hybrid_removes_non_essential_and_writes_routers() {
        let tmp = tempdir().unwrap();
        let agent_dir = tmp.path().join("claude-skills");
        fs::create_dir_all(&agent_dir).unwrap();
        let vault_root = sm_vault(tmp.path());
        let src_find = stub_skill_dir(&vault_root, "find-skills");
        let src_fe = stub_skill_dir(&vault_root, "frontend-design");

        // Pre-populate agent dir with SM-managed symlinks (simulating prior full sync).
        std::os::unix::fs::symlink(&src_find, agent_dir.join("find-skills")).unwrap();
        std::os::unix::fs::symlink(&src_fe, agent_dir.join("frontend-design")).unwrap();

        let ess = tpack("essential", true, None);
        let dom = tpack("dev-fe", false, Some("When user wants frontend work"));
        let ess_skills = vec![tskill("find-skills")];
        let dom_skills = vec![tskill("frontend-design")];
        let packs = vec![
            PackWithSkills {
                pack: &ess,
                skills: &ess_skills,
            },
            PackWithSkills {
                pack: &dom,
                skills: &dom_skills,
            },
        ];

        let report =
            reconcile_agent_dir(&agent_dir, &packs, DisclosureMode::Hybrid, &vault_root).unwrap();

        // Essential skill remains; non-essential gone; router exists.
        assert!(
            agent_dir.join("find-skills").exists(),
            "essential skill remains materialized"
        );
        assert!(
            !agent_dir.join("frontend-design").exists(),
            "non-essential unlinked in hybrid"
        );
        let router_md = agent_dir.join("pack-dev-fe/SKILL.md");
        assert!(router_md.exists(), "router written for non-essential pack");
        let content = fs::read_to_string(&router_md).unwrap();
        assert!(content.contains("name: pack-dev-fe"));
        assert!(content.contains("When user wants frontend work"));

        assert_eq!(report.removed, 1);
        assert!(report.rendered_routers >= 1);
    }

    #[cfg(unix)]
    #[test]
    fn switching_hybrid_to_full_removes_routers_and_adds_skills() {
        let tmp = tempdir().unwrap();
        let agent_dir = tmp.path().join("claude-skills");
        fs::create_dir_all(&agent_dir).unwrap();
        let vault_root = sm_vault(tmp.path());
        let src_find = stub_skill_dir(&vault_root, "find-skills");
        let _src_fe = stub_skill_dir(&vault_root, "frontend-design");

        // Pre-populate as if hybrid synced: essential symlinked, router dir present.
        std::os::unix::fs::symlink(&src_find, agent_dir.join("find-skills")).unwrap();
        let router_dir = agent_dir.join("pack-dev-fe");
        fs::create_dir_all(&router_dir).unwrap();
        fs::write(
            router_dir.join("SKILL.md"),
            "---\nname: pack-dev-fe\ndescription: stale\n---\n\n# Pack: dev-fe\n",
        )
        .unwrap();

        let ess = tpack("essential", true, None);
        let dom = tpack("dev-fe", false, Some("desc"));
        let ess_skills = vec![tskill("find-skills")];
        let dom_skills = vec![tskill("frontend-design")];
        let packs = vec![
            PackWithSkills {
                pack: &ess,
                skills: &ess_skills,
            },
            PackWithSkills {
                pack: &dom,
                skills: &dom_skills,
            },
        ];

        let report =
            reconcile_agent_dir(&agent_dir, &packs, DisclosureMode::Full, &vault_root).unwrap();

        assert!(agent_dir.join("find-skills").exists());
        assert!(
            agent_dir.join("frontend-design").exists(),
            "domain skill materialized in full mode"
        );
        assert!(
            !agent_dir.join("pack-dev-fe").exists(),
            "router removed in full mode"
        );
        assert_eq!(report.removed, 1);
        assert!(report.added >= 1);
    }

    #[cfg(unix)]
    #[test]
    fn router_rewritten_when_pack_description_changes() {
        let tmp = tempdir().unwrap();
        let agent_dir = tmp.path().join("claude-skills");
        fs::create_dir_all(&agent_dir).unwrap();
        let vault_root = sm_vault(tmp.path());
        let _ = stub_skill_dir(&vault_root, "seo-audit");

        let ess = tpack("essential", true, None);
        let ess_skills: Vec<SkillRecord> = vec![];
        let dom_skills = vec![tskill("seo-audit")];

        // First reconcile with original description.
        let dom1 = tpack("mkt-seo", false, Some("Original trigger"));
        let packs1 = vec![
            PackWithSkills {
                pack: &ess,
                skills: &ess_skills,
            },
            PackWithSkills {
                pack: &dom1,
                skills: &dom_skills,
            },
        ];
        reconcile_agent_dir(&agent_dir, &packs1, DisclosureMode::Hybrid, &vault_root).unwrap();
        let md = agent_dir.join("pack-mkt-seo/SKILL.md");
        let first = fs::read_to_string(&md).unwrap();
        assert!(first.contains("Original trigger"));

        // Second reconcile with updated description; content should change.
        let dom2 = tpack("mkt-seo", false, Some("Updated trigger text"));
        let packs2 = vec![
            PackWithSkills {
                pack: &ess,
                skills: &ess_skills,
            },
            PackWithSkills {
                pack: &dom2,
                skills: &dom_skills,
            },
        ];
        let report =
            reconcile_agent_dir(&agent_dir, &packs2, DisclosureMode::Hybrid, &vault_root).unwrap();
        let second = fs::read_to_string(&md).unwrap();
        assert!(second.contains("Updated trigger text"));
        assert!(!second.contains("Original trigger"));
        assert_ne!(first, second);
        assert!(report.rendered_routers >= 1);
    }
}
