//! Join and leave Sync (SID-100, SID-111). Joining merges this host's
//! library into the shared one by name: where both have an item, Sync's
//! version wins, and items only this host has are added for everyone.
//! Histories are never merged: the host takes the remote history and commits
//! its extras on top as one "Joined from <host>" commit. A full pre-join copy
//! of the library and database, never deleted automatically, makes the join
//! undoable. Leaving keeps the library and the agents as they are.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Serialize;
use serde_json::Value;

use super::error::AppError;
use super::git_backup_store::{
    apply_device_identity, effective_device_name, reindex_library_unlocked,
};
use super::host::HostCtx;
use super::merge::snapshot::METADATA_DIR;
use super::skill_store::SkillStore;
use super::sync_metadata::{
    self, AgentMetaFile, ScenarioMetaFile, ScenarioSkillMetaFile, SkillMetaFile,
};
use super::{central_repo, content_hash, git_backup, git_credentials, resource_store, sync_apply};

/// Folder name prefix of a pre-join copy, under the base folder.
const COPY_PREFIX: &str = "pre-sync-";
const INSTRUCTIONS_DIR: &str = "instructions";
const MCPS_DIR: &str = "mcps";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JoinItem {
    /// "skill", "preset", "instruction" or "mcp".
    pub kind: &'static str,
    pub name: String,
}

/// The review list shown before joining.
#[derive(Debug, Serialize)]
pub struct JoinPreview {
    /// The Sync remote is empty: this host starts it.
    pub remote_empty: bool,
    /// This host's library is empty: it takes Sync as it is.
    pub local_empty: bool,
    /// This host's items that Sync gains.
    pub adds: Vec<JoinItem>,
    /// This host's items that Sync's version replaces.
    pub replaced: Vec<JoinItem>,
    /// The preset active after joining.
    pub active_preset: Option<String>,
}

/// Show what joining `url` would do. Writes nothing.
pub fn preview(store: &SkillStore, url: &str) -> Result<JoinPreview, AppError> {
    let skills_dir = central_repo::skills_dir();
    git_backup::with_repo_lock("preview join", || {
        sync_metadata::write_all_from_db_unlocked(store)?;
        let clone = TempClone::fetch(url)?;
        Ok(plan(store, &skills_dir, &clone.0).preview)
    })
    .map_err(git_error)
}

/// Join the Sync remote at `url`.
pub fn join(ctx: &HostCtx, url: &str) -> Result<JoinPreview, AppError> {
    let store = &ctx.store;
    let skills_dir = central_repo::skills_dir();
    let preview = git_backup::with_repo_lock("join sync", || {
        sync_metadata::write_all_from_db_unlocked(store)?;
        save_pre_join_copy(store, &skills_dir)?;
        let clone = TempClone::fetch(url)?;
        let plan = plan(store, &skills_dir, &clone.0);
        let host = effective_device_name(store);
        let before = sync_apply::capture(store);
        if plan.preview.remote_empty {
            // This host starts Sync.
            if !skills_dir.join(".git").exists() {
                git_backup::init_repo_unlocked(&skills_dir, &host)?;
            }
            git_backup::set_remote_unlocked(&skills_dir, url)?;
            git_backup::commit_changes_unlocked(&skills_dir, &format!("Joined from {host}"))?;
        } else {
            if !plan.preview.local_empty {
                add_extras(&plan, &skills_dir, &clone.0)?;
                git_backup::configure_device_identity(&clone.0, &host)?;
                git_backup::commit_all_unlocked(&clone.0, &format!("Joined from {host}"))?;
            }
            replace_dir(&clone.0, &skills_dir)?;
        }
        reindex_library_unlocked(store)?;
        if let Some(name) = &plan.preview.active_preset {
            let presets = store.get_all_scenarios()?;
            if let Some(preset) = presets.iter().find(|p| same_name(&p.name, name)) {
                store.set_active_scenario(&preset.id)?;
            }
        }
        sync_apply::reapply_active_preset(store, &before);
        apply_device_identity(store, &skills_dir);
        store.set_setting("git_backup_remote_url", url)?;
        // The joined state is safe locally; a failed push is retried by the
        // next sync.
        if let Err(e) = git_backup::push_unlocked(&skills_dir) {
            log::warn!("join: push failed, the next sync retries it: {e:#}");
        }
        Ok(plan.preview)
    })
    .map_err(git_error)?;
    sync_apply::reapply_resources(ctx);
    Ok(preview)
}

/// Undo a join: restore the newest pre-join copy (or the named one), then
/// leave Sync. The copy itself is kept.
pub fn undo_join(ctx: &HostCtx, copy: Option<&str>) -> Result<String, AppError> {
    let store = &ctx.store;
    let name = match copy {
        Some(name) => name.to_string(),
        None => pre_join_copies()
            .pop()
            .ok_or_else(|| AppError::not_found("No pre-join copy to restore"))?,
    };
    let dir = copy_dir(&name)?;
    let skills_dir = central_repo::skills_dir();
    git_backup::with_repo_lock("undo join", || {
        let before = sync_apply::capture(store);
        store.restore_from(&dir.join(db_file_name()))?;
        let restored = skills_dir.with_file_name(format!(".undo-join-{name}"));
        central_repo::copy_dir_recursive(&dir.join("library"), &restored)?;
        replace_dir(&restored, &skills_dir)?;
        sync_apply::reapply_active_preset(store, &before);
        Ok(())
    })
    .map_err(git_error)?;
    leave(store)?;
    sync_apply::reapply_resources(ctx);
    Ok(name)
}

/// Stop syncing: remove the remote and this host's stored access. The
/// library and the agents stay as they are (SID-111).
pub fn leave(store: &SkillStore) -> Result<(), AppError> {
    let skills_dir = central_repo::skills_dir();
    // Collect credential hosts before the URLs are gone.
    let mut hosts = HashSet::new();
    if let Some(url) = git_backup::raw_remote_url(&skills_dir) {
        hosts.extend(git_credentials::https_host(&url));
    }
    if let Ok(Some(url)) = store.get_setting("git_backup_remote_url") {
        hosts.extend(git_credentials::https_host(&url));
    }

    git_backup::remove_remote(&skills_dir).map_err(AppError::git)?;
    store
        .set_setting("git_backup_remote_url", "")
        .map_err(AppError::db)?;
    let _ = store.set_setting("github_auth_method", "");

    for host in hosts {
        if let Err(e) = git_credentials::delete_credential(&host) {
            log::warn!("leave sync: failed to delete keychain credential: {e:#}");
        }
    }
    Ok(())
}

/// Pre-join copies, oldest first.
pub fn pre_join_copies() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(central_repo::base_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| name.starts_with(COPY_PREFIX))
        .collect();
    names.sort();
    names
}

/// Delete a pre-join copy ("Delete backup").
pub fn delete_pre_join_copy(name: &str) -> Result<(), AppError> {
    fs::remove_dir_all(copy_dir(name)?).map_err(AppError::io)
}

fn copy_dir(name: &str) -> Result<PathBuf, AppError> {
    if !pre_join_copies().iter().any(|n| n == name) {
        return Err(AppError::not_found(format!(
            "No pre-join copy named {name}"
        )));
    }
    Ok(central_repo::base_dir().join(name))
}

fn db_file_name() -> PathBuf {
    PathBuf::from(central_repo::db_path().file_name().unwrap_or_default())
}

fn git_error(e: anyhow::Error) -> AppError {
    AppError::classify_git_error(format!("{e:#}"))
}

/// The library folder and the database, copied under the repo lock to
/// `pre-sync-<date>/`.
fn save_pre_join_copy(store: &SkillStore, skills_dir: &Path) -> anyhow::Result<PathBuf> {
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S%.3f");
    let dir = central_repo::base_dir().join(format!("{COPY_PREFIX}{stamp}"));
    fs::create_dir_all(&dir)?;
    if skills_dir.exists() {
        central_repo::copy_dir_recursive(skills_dir, &dir.join("library"))?;
    }
    store.backup_to(&dir.join(db_file_name()))?;
    log::info!("join: pre-join copy saved to {}", dir.display());
    Ok(dir)
}

/// Swap `new` into `target`'s place.
fn replace_dir(new: &Path, target: &Path) -> anyhow::Result<()> {
    let aside = target.with_file_name(".join-replaced");
    if aside.exists() {
        fs::remove_dir_all(&aside)?;
    }
    if target.exists() {
        fs::rename(target, &aside).context("Failed to move the library aside")?;
    }
    if let Err(e) = fs::rename(new, target) {
        let _ = fs::rename(&aside, target);
        return Err(e).context("Failed to move the joined library into place");
    }
    let _ = fs::remove_dir_all(&aside);
    Ok(())
}

/// A clone of the remote in a temp folder next to the library, removed on
/// drop unless it was moved into place.
struct TempClone(PathBuf);

impl TempClone {
    fn fetch(url: &str) -> anyhow::Result<Self> {
        let dir = central_repo::skills_dir().with_file_name(".join-clone");
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        git_backup::clone_into_unlocked(&dir, url)?;
        Ok(Self(dir))
    }
}

impl Drop for TempClone {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// ── matching by name ──

/// One side's library, read from its metadata files.
#[derive(Default)]
struct Library {
    skills: Vec<SkillMetaFile>,
    presets: Vec<ScenarioMetaFile>,
    members: Vec<ScenarioSkillMetaFile>,
    agents: Vec<AgentMetaFile>,
    /// (id, name) of instruction bundles and MCP definitions.
    instructions: Vec<(String, String)>,
    mcps: Vec<(String, String)>,
}

impl Library {
    fn read(dir: &Path) -> Self {
        let meta = dir.join(METADATA_DIR);
        let resources = dir.join(resource_store::NAMESPACE);
        Self {
            skills: json_files(&meta.join("skills")),
            presets: json_files(&meta.join("scenarios")),
            members: fs::read_dir(meta.join("scenario-skills"))
                .into_iter()
                .flatten()
                .flatten()
                .flat_map(|preset| json_files(&preset.path()))
                .collect(),
            agents: json_files(&meta.join(sync_metadata::AGENTS_DIR)),
            instructions: fs::read_dir(resources.join(INSTRUCTIONS_DIR))
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| named(&e.path().join("definition.json")))
                .collect(),
            mcps: fs::read_dir(resources.join(MCPS_DIR))
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| named(&e.path()))
                .collect(),
        }
    }

    fn is_empty(&self) -> bool {
        self.skills.is_empty()
            && self.members.is_empty()
            && self.agents.is_empty()
            && self.instructions.is_empty()
            && self.mcps.is_empty()
    }
}

fn json_files<T: serde::de::DeserializeOwned>(dir: &Path) -> Vec<T> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    files
        .iter()
        .filter_map(|p| serde_json::from_str(&fs::read_to_string(p).ok()?).ok())
        .collect()
}

/// (id, name) of a resource definition file.
fn named(path: &Path) -> Option<(String, String)> {
    let value: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    Some((
        value["id"].as_str()?.to_owned(),
        value["name"].as_str()?.to_owned(),
    ))
}

fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

struct Plan {
    preview: JoinPreview,
    skills: Vec<SkillMetaFile>,
    presets: Vec<ScenarioMetaFile>,
    agents: Vec<AgentMetaFile>,
    instructions: Vec<String>,
    mcps: Vec<String>,
    /// This host's skill id → the shared id, for matched skills.
    shared_ids: HashMap<String, String>,
}

fn plan(store: &SkillStore, local_dir: &Path, remote_dir: &Path) -> Plan {
    let local = Library::read(local_dir);
    let remote = Library::read(remote_dir);
    let remote_empty = !remote_dir.join(".git").exists()
        || git2::Repository::open(remote_dir).is_ok_and(|repo| repo.head().is_err());
    let local_empty = local.is_empty();
    let mut plan = Plan {
        preview: JoinPreview {
            remote_empty,
            local_empty,
            adds: Vec::new(),
            replaced: Vec::new(),
            active_preset: None,
        },
        skills: Vec::new(),
        presets: Vec::new(),
        agents: Vec::new(),
        instructions: Vec::new(),
        mcps: Vec::new(),
        shared_ids: HashMap::new(),
    };
    // Joining switches to Sync's active preset (SID-98, SID-100); a remote
    // without one keeps this host's active preset by name.
    let shared_active = shared_active_preset(remote_dir)
        .and_then(|id| remote.presets.iter().find(|p| p.scenario_id == id))
        .map(|p| p.name.clone());
    let active_name = shared_active.or_else(|| {
        store
            .get_active_scenario_id()
            .ok()
            .flatten()
            .and_then(|id| local.presets.iter().find(|p| p.scenario_id == id))
            .map(|p| p.name.clone())
    });
    if remote_empty || local_empty {
        // One side becomes the whole: nothing to match.
        let presets = if remote_empty {
            &local.presets
        } else {
            &remote.presets
        };
        plan.preview.active_preset = active_name
            .filter(|name| presets.iter().any(|p| same_name(&p.name, name)))
            .or_else(|| first_preset(presets));
        return plan;
    }

    let item = |kind, name: &str| JoinItem {
        kind,
        name: name.to_string(),
    };
    for skill in &local.skills {
        match remote.skills.iter().find(|r| r.path_key == skill.path_key) {
            Some(shared) => {
                plan.shared_ids
                    .insert(skill.skill_id.clone(), shared.skill_id.clone());
                if !same_dir(&local_dir.join(&skill.path), &remote_dir.join(&shared.path)) {
                    plan.preview.replaced.push(item("skill", &skill.path));
                }
            }
            None => {
                plan.preview.adds.push(item("skill", &skill.path));
                plan.skills.push(skill.clone());
            }
        }
    }
    let shared_id = |id: &String| plan_id(&plan.shared_ids, id);
    for preset in &local.presets {
        let members = |lib: &Library, id: &str, map: &dyn Fn(&String) -> String| {
            let mut ids: Vec<String> = lib
                .members
                .iter()
                .filter(|m| m.scenario_id == id)
                .map(|m| map(&m.skill_id))
                .collect();
            ids.sort();
            ids
        };
        match remote
            .presets
            .iter()
            .find(|r| same_name(&r.name, &preset.name))
        {
            Some(shared) => {
                if members(&local, &preset.scenario_id, &shared_id)
                    != members(&remote, &shared.scenario_id, &|id| id.clone())
                {
                    plan.preview.replaced.push(item("preset", &preset.name));
                }
            }
            None => {
                plan.preview.adds.push(item("preset", &preset.name));
                plan.presets.push(preset.clone());
            }
        }
    }
    // Custom agents match by key (SID-105); Sync's definition wins.
    for agent in &local.agents {
        let name = &agent.agent.display_name;
        match remote
            .agents
            .iter()
            .find(|r| r.agent.key == agent.agent.key)
        {
            Some(shared) if shared.agent != agent.agent => {
                plan.preview.replaced.push(item("agent", name));
            }
            Some(_) => {}
            None => {
                plan.preview.adds.push(item("agent", name));
                plan.agents.push(agent.clone());
            }
        }
    }
    for (kind, sub, ours, theirs, added) in [
        (
            "instruction",
            INSTRUCTIONS_DIR,
            &local.instructions,
            &remote.instructions,
            &mut plan.instructions,
        ),
        ("mcp", MCPS_DIR, &local.mcps, &remote.mcps, &mut plan.mcps),
    ] {
        let path = |dir: &Path, id: &str| {
            let base = dir.join(resource_store::NAMESPACE).join(sub);
            if kind == "mcp" {
                base.join(format!("{id}.json"))
            } else {
                base.join(id)
            }
        };
        for (id, name) in ours {
            match theirs.iter().find(|(_, n)| same_name(n, name)) {
                Some((shared, _)) => {
                    if !same_definition(&path(local_dir, id), &path(remote_dir, shared)) {
                        plan.preview.replaced.push(item(kind, name));
                    }
                }
                None => {
                    plan.preview.adds.push(item(kind, name));
                    added.push(id.clone());
                }
            }
        }
    }

    // Release 1: keep the active preset (Sync has it after joining, matched
    // or added); a host without one takes the first shared preset.
    plan.preview.active_preset = active_name.or_else(|| first_preset(&remote.presets));
    plan
}

fn shared_active_preset(dir: &Path) -> Option<String> {
    let raw = fs::read_to_string(
        dir.join(METADATA_DIR)
            .join(sync_metadata::ACTIVE_PRESET_FILE),
    )
    .ok()?;
    serde_json::from_str::<sync_metadata::ActivePresetFile>(&raw)
        .ok()
        .map(|f| f.scenario_id)
}

fn plan_id(shared_ids: &HashMap<String, String>, id: &String) -> String {
    shared_ids.get(id).cloned().unwrap_or_else(|| id.clone())
}

fn first_preset(presets: &[ScenarioMetaFile]) -> Option<String> {
    presets
        .iter()
        .min_by_key(|p| (p.sort_order, p.name.to_lowercase()))
        .map(|p| p.name.clone())
}

fn same_dir(a: &Path, b: &Path) -> bool {
    matches!(
        (content_hash::hash_directory(a), content_hash::hash_directory(b)),
        (Ok(x), Ok(y)) if x == y
    )
}

/// Definitions match when they differ only in id, revision and time.
fn same_definition(a: &Path, b: &Path) -> bool {
    if a.is_dir() {
        return same_dir(a, b);
    }
    let read = |p: &Path| -> Option<Value> {
        let mut value: Value = serde_json::from_str(&fs::read_to_string(p).ok()?).ok()?;
        for key in ["id", "revision", "updatedAt", "updated_at"] {
            value.as_object_mut()?.remove(key);
        }
        Some(value)
    };
    read(a).is_some_and(|x| read(b).is_some_and(|y| x == y))
}

/// Copy this host's extras into the clone, with memberships pointing at the
/// shared skill ids.
fn add_extras(plan: &Plan, local_dir: &Path, clone_dir: &Path) -> anyhow::Result<()> {
    let meta = |dir: &Path| dir.join(METADATA_DIR);
    for skill in &plan.skills {
        central_repo::copy_dir_recursive(
            &local_dir.join(&skill.path),
            &clone_dir.join(&skill.path),
        )?;
        write_json(
            &meta(clone_dir)
                .join("skills")
                .join(format!("{}.json", skill.skill_id)),
            skill,
        )?;
    }
    let local = Library::read(local_dir);
    for preset in &plan.presets {
        write_json(
            &meta(clone_dir)
                .join("scenarios")
                .join(format!("{}.json", preset.scenario_id)),
            preset,
        )?;
        for member in local
            .members
            .iter()
            .filter(|m| m.scenario_id == preset.scenario_id)
        {
            let skill_id = plan_id(&plan.shared_ids, &member.skill_id);
            let member = ScenarioSkillMetaFile {
                skill_id: skill_id.clone(),
                ..member.clone()
            };
            write_json(
                &meta(clone_dir)
                    .join("scenario-skills")
                    .join(&preset.scenario_id)
                    .join(format!("{skill_id}.json")),
                &member,
            )?;
        }
    }
    for agent in &plan.agents {
        write_json(
            &meta(clone_dir)
                .join(sync_metadata::AGENTS_DIR)
                .join(format!("{}.json", agent.agent.key)),
            agent,
        )?;
    }
    let resources = |dir: &Path, sub: &str| dir.join(resource_store::NAMESPACE).join(sub);
    for id in &plan.instructions {
        central_repo::copy_dir_recursive(
            &resources(local_dir, INSTRUCTIONS_DIR).join(id),
            &resources(clone_dir, INSTRUCTIONS_DIR).join(id),
        )?;
    }
    for id in &plan.mcps {
        let file = format!("{id}.json");
        fs::create_dir_all(resources(clone_dir, MCPS_DIR))?;
        fs::copy(
            resources(local_dir, MCPS_DIR).join(&file),
            resources(clone_dir, MCPS_DIR).join(&file),
        )?;
    }
    Ok(())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, sync_metadata::canonical_json_bytes(value)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::skill_store::ScenarioRecord;
    use crate::core::test_support::sample_skill;
    use crate::core::tool_adapters::{all_tool_adapters, custom_tool_paths, default_tool_adapters};
    use std::process::Command;
    use std::sync::Arc;

    /// Hosts that share one bare Sync remote. Each switches the global base
    /// folder override to itself before acting.
    struct Hosts {
        _lock: std::sync::MutexGuard<'static, ()>,
        tmp: tempfile::TempDir,
    }

    struct Host {
        base: PathBuf,
        ctx: HostCtx,
    }

    impl Drop for Hosts {
        fn drop(&mut self) {
            central_repo::set_test_base_dir_override(None);
        }
    }

    impl Hosts {
        fn new() -> Self {
            let lock = central_repo::test_base_dir_lock();
            let tmp = tempfile::tempdir().unwrap();
            let out = Command::new("git")
                .args(["init", "--bare", "--initial-branch=main"])
                .arg(tmp.path().join("remote.git"))
                .output()
                .unwrap();
            assert!(out.status.success());
            Self { _lock: lock, tmp }
        }

        fn url(&self) -> String {
            self.tmp
                .path()
                .join("remote.git")
                .to_string_lossy()
                .into_owned()
        }

        fn host(&self, name: &str) -> Host {
            let base = self.tmp.path().join(name);
            central_repo::set_test_base_dir_override(Some(base.clone()));
            fs::create_dir_all(central_repo::skills_dir()).unwrap();
            let store = SkillStore::new(&base.join("test.db")).unwrap();
            store.set_setting("backup_device_name", name).unwrap();
            let host = Host {
                base,
                ctx: HostCtx::headless(Arc::new(store)),
            };
            // Join and undo reapply the active preset, which deploys to every
            // installed agent. Keep each agent's folder inside this fixture so
            // no test writes to the real home.
            for adapter in default_tool_adapters() {
                host.redirect(&adapter.key, &adapter.relative_skills_dir);
            }
            host.assert_confined();
            host
        }
    }

    impl Host {
        fn store(&self) -> &SkillStore {
            &self.ctx.store
        }

        fn activate(&self) {
            central_repo::set_test_base_dir_override(Some(self.base.clone()));
        }

        /// Point an agent's skills folder at `rel` under this host's fake home.
        fn redirect(&self, key: &str, rel: &str) {
            let mut paths = custom_tool_paths(self.store());
            paths.insert(
                key.into(),
                self.home().join(rel).to_string_lossy().into_owned(),
            );
            let json = serde_json::to_string(&paths).unwrap();
            self.store()
                .set_setting("custom_tool_paths", &json)
                .unwrap();
        }

        fn home(&self) -> PathBuf {
            self.base.join("home")
        }

        /// Every agent folder and recorded deployment is inside this fixture.
        fn assert_confined(&self) {
            for adapter in all_tool_adapters(self.store()) {
                let dir = adapter.skills_dir();
                assert!(dir.starts_with(&self.base), "{} -> {dir:?}", adapter.key);
            }
            for target in self.store().get_all_targets().unwrap() {
                let path = Path::new(&target.target_path);
                assert!(path.starts_with(&self.base), "deployed to {path:?}");
            }
        }

        fn deployed(&self, rel_skills_dir: &str, skill: &str) -> Option<String> {
            let dir = self.home().join(rel_skills_dir).join(skill);
            fs::read_to_string(dir.join("SKILL.md")).ok()
        }

        fn add_skill(&self, id: &str, content: &str) {
            self.activate();
            let dir = central_repo::skills_dir().join(id);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("SKILL.md"), content).unwrap();
            self.store()
                .insert_skill(&sample_skill(id, id, &dir))
                .unwrap();
        }

        fn add_preset(&self, id: &str, name: &str, skills: &[&str]) {
            self.store()
                .insert_scenario(&ScenarioRecord {
                    id: id.into(),
                    name: name.into(),
                    description: None,
                    icon: None,
                    sort_order: 0,
                    created_at: 1,
                    updated_at: 1,
                })
                .unwrap();
            for skill in skills {
                self.store().add_skill_to_scenario(id, skill).unwrap();
            }
        }

        fn skill_md(&self, path: &str) -> Option<String> {
            fs::read_to_string(self.base.join("skills").join(path).join("SKILL.md")).ok()
        }

        fn preset_names(&self) -> Vec<String> {
            let mut names: Vec<String> = self
                .store()
                .get_all_scenarios()
                .unwrap()
                .into_iter()
                .map(|p| p.name)
                .collect();
            names.sort();
            names
        }

        fn preset_skills(&self, name: &str) -> Vec<String> {
            let presets = self.store().get_all_scenarios().unwrap();
            let preset = presets.iter().find(|p| p.name == name).unwrap();
            let mut paths: Vec<String> = self
                .store()
                .get_skills_for_scenario(&preset.id)
                .unwrap()
                .into_iter()
                .map(|s| {
                    Path::new(&s.central_path)
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            paths.sort();
            paths
        }
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn names(items: &[JoinItem]) -> Vec<String> {
        items
            .iter()
            .map(|i| format!("{} {}", i.kind, i.name))
            .collect()
    }

    /// Host A starts Sync with alpha, beta and "Same", plus preset Web.
    fn started(hosts: &Hosts) -> Host {
        let a = hosts.host("a");
        a.add_skill("alpha", "alpha from A");
        a.add_skill("beta", "beta from A");
        a.add_skill("same", "identical");
        a.add_preset("web-a", "Web", &["alpha"]);
        let preview = join(&a.ctx, &hosts.url()).unwrap();
        assert!(preview.remote_empty);
        a
    }

    #[test]
    fn joining_matches_by_name_sync_wins_and_extras_are_added() {
        let hosts = Hosts::new();
        let a = started(&hosts);
        let a_head = git(&a.base.join("skills"), &["rev-parse", "HEAD"]);

        let b = hosts.host("b");
        git(&b.base.join("skills"), &["init", "-b", "main"]);
        b.add_skill("alpha", "alpha from B");
        b.add_skill("same", "identical");
        b.add_skill("gamma", "gamma from B");
        b.add_preset("web-b", "Web", &["gamma"]);
        b.add_preset("ops-b", "Ops", &["alpha", "gamma"]);
        b.store().set_active_scenario("ops-b").unwrap();
        // B has history of its own, unrelated to the remote's.
        let skills = b.base.join("skills");
        git(&skills, &["add", "-A"]);
        git(
            &skills,
            &[
                "-c",
                "user.name=b",
                "-c",
                "user.email=b@test",
                "commit",
                "-m",
                "b's own history",
            ],
        );
        let b_own = git(&skills, &["rev-parse", "HEAD"]);
        assert!(!b_own.is_empty());

        let preview = super::preview(b.store(), &hosts.url()).unwrap();
        assert_eq!(names(&preview.adds), ["skill gamma", "preset Ops"]);
        assert_eq!(names(&preview.replaced), ["skill alpha", "preset Web"]);
        assert_eq!(preview.active_preset.as_deref(), Some("Ops"));
        assert!(b.skill_md("beta").is_none(), "preview writes nothing");

        join(&b.ctx, &hosts.url()).unwrap();
        assert_eq!(b.skill_md("alpha").as_deref(), Some("alpha from A"));
        assert_eq!(b.skill_md("beta").as_deref(), Some("beta from A"));
        assert_eq!(b.skill_md("gamma").as_deref(), Some("gamma from B"));
        assert_eq!(b.preset_names(), ["Ops", "Web"]);
        assert_eq!(b.preset_skills("Web"), ["alpha"]);
        assert_eq!(b.preset_skills("Ops"), ["alpha", "gamma"]);
        // The active preset deploys into this host's own agent folders.
        assert_eq!(
            b.deployed(".claude/skills", "gamma").as_deref(),
            Some("gamma from B")
        );
        b.assert_confined();
        let active = b.store().get_active_scenario_id().unwrap().unwrap();
        assert_eq!(active, "ops-b");

        // B took the remote history and added one commit on top; its own
        // unrelated history is not merged in, and the remote has the result.
        assert_eq!(git(&skills, &["log", "-1", "--format=%s"]), "Joined from b");
        assert_eq!(git(&skills, &["rev-parse", "HEAD~1"]), a_head);
        assert!(git(&skills, &["rev-list", "HEAD"])
            .lines()
            .all(|c| c != b_own));
        assert_eq!(
            git(Path::new(&hosts.url()), &["rev-parse", "main"]),
            git(&skills, &["rev-parse", "HEAD"])
        );
        assert_eq!(pre_join_copies().len(), 1);
    }

    #[test]
    fn joining_matches_custom_agents_by_key() {
        // SID-105: Sync's definition wins, and this host's other agents join.
        use crate::core::tool_adapters::{custom_tools, CustomToolDef};
        let set_agents = |host: &Host, agents: &[(&str, &str)]| {
            let agents: Vec<_> = agents
                .iter()
                .map(|(key, name)| CustomToolDef {
                    key: key.to_string(),
                    display_name: name.to_string(),
                    skills_dir: format!(".{key}/skills"),
                    project_relative_skills_dir: None,
                    category: Default::default(),
                })
                .collect();
            let json = serde_json::to_string(&agents).unwrap();
            host.store().set_setting("custom_tools", &json).unwrap();
            for agent in &agents {
                host.redirect(&agent.key, &agent.skills_dir);
            }
        };
        let hosts = Hosts::new();
        let a = hosts.host("a");
        a.add_skill("alpha", "alpha");
        set_agents(&a, &[("x", "X")]);
        join(&a.ctx, &hosts.url()).unwrap();

        let b = hosts.host("b");
        b.add_skill("beta", "beta");
        set_agents(&b, &[("x", "X on B"), ("y", "Y")]);
        let preview = super::preview(b.store(), &hosts.url()).unwrap();
        assert_eq!(names(&preview.adds), ["skill beta", "agent Y"]);
        assert_eq!(names(&preview.replaced), ["agent X on B"]);
        join(&b.ctx, &hosts.url()).unwrap();
        let mut agents: Vec<_> = custom_tools(b.store())
            .into_iter()
            .map(|a| (a.key, a.display_name))
            .collect();
        agents.sort();
        assert_eq!(agents, [("x".into(), "X".into()), ("y".into(), "Y".into())]);
        b.assert_confined();
    }

    #[test]
    fn an_empty_host_takes_sync_as_it_is() {
        let hosts = Hosts::new();
        let _a = started(&hosts);
        let c = hosts.host("c");
        let preview = join(&c.ctx, &hosts.url()).unwrap();
        assert!(preview.local_empty && preview.adds.is_empty());
        assert_eq!(c.skill_md("alpha").as_deref(), Some("alpha from A"));
        assert_eq!(c.preset_names(), ["Web"]);
    }

    #[test]
    fn undo_join_restores_files_and_database_rows_then_leaves() {
        let hosts = Hosts::new();
        let _a = started(&hosts);
        let b = hosts.host("b");
        b.add_skill("alpha", "alpha from B");
        b.add_preset("ops-b", "Ops", &["alpha"]);
        join(&b.ctx, &hosts.url()).unwrap();
        assert_eq!(b.skill_md("alpha").as_deref(), Some("alpha from A"));

        b.activate();
        undo_join(&b.ctx, None).unwrap();
        assert_eq!(b.skill_md("alpha").as_deref(), Some("alpha from B"));
        b.assert_confined();
        assert!(b.skill_md("beta").is_none());
        assert_eq!(b.preset_names(), ["Ops"]);
        assert_eq!(b.preset_skills("Ops"), ["alpha"]);
        assert!(git_backup::raw_remote_url(&b.base.join("skills")).is_none());
        // The copy is never deleted automatically.
        assert_eq!(pre_join_copies().len(), 1);
        delete_pre_join_copy(&pre_join_copies()[0]).unwrap();
        assert!(pre_join_copies().is_empty());
    }
}
