use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;
use walkdir::WalkDir;

use super::central_repo;
use super::repo_lock::RepoLock;
use super::skill_metadata;
use super::skill_store::{ScenarioRecord, SkillRecord, SkillStore};

const SCHEMA_VERSION: u32 = 1;
const APP_MIN_VERSION: &str = "2.0.0";

#[derive(Debug, Serialize, Deserialize)]
pub struct SchemaFile {
    pub schema_version: u32,
    pub app_min_version: String,
    pub created_by: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SourceMeta {
    #[serde(rename = "type")]
    pub source_type: String,
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
    pub subpath: Option<String>,
    pub branch: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SkillMetaFile {
    pub schema_version: u32,
    pub skill_id: String,
    pub path: String,
    pub path_key: String,
    pub enabled: bool,
    pub tags: Vec<String>,
    pub source: SourceMeta,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScenarioMetaFile {
    pub schema_version: u32,
    pub scenario_id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub sort_order: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScenarioSkillMetaFile {
    pub schema_version: u32,
    pub scenario_id: String,
    pub skill_id: String,
    pub sort_order: i32,
    pub tools: BTreeMap<String, bool>,
}

pub fn metadata_dir() -> PathBuf {
    central_repo::skills_dir().join(".skills-manager")
}

pub fn metadata_exists() -> bool {
    metadata_dir().join("schema.json").exists()
        || metadata_dir().join("skills").is_dir()
        || metadata_dir().join("scenarios").is_dir()
}

#[allow(dead_code)]
pub fn write_all_from_db(store: &SkillStore) -> Result<()> {
    let skills_dir = central_repo::skills_dir();
    let _lock = RepoLock::acquire(&skills_dir, "write sync metadata")?;
    write_all_from_db_unlocked(store)
}

pub(crate) fn with_repo_lock<T, F>(operation: &str, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    let skills_dir = central_repo::skills_dir();
    let _lock = RepoLock::acquire(&skills_dir, operation)?;
    f()
}

pub(crate) fn write_all_from_db_unlocked(store: &SkillStore) -> Result<()> {
    ensure_metadata_dirs()?;
    write_schema()?;
    write_skill_records_from_db(store)?;
    write_scenario_records_from_db(store)?;
    remove_stale_metadata_files(store)?;
    Ok(())
}

#[allow(dead_code)]
pub fn reindex_from_metadata(store: &SkillStore) -> Result<()> {
    let skills_dir = central_repo::skills_dir();
    let _lock = RepoLock::acquire(&skills_dir, "reindex sync metadata")?;
    reindex_from_metadata_unlocked(store)
}

pub(crate) fn reindex_from_metadata_unlocked(store: &SkillStore) -> Result<()> {
    if !metadata_exists() {
        return Ok(());
    }

    let skills = read_skill_files()?;
    ensure_unique_path_keys(&skills)?;

    let has_complete_scenario_snapshot = metadata_has_complete_scenario_snapshot();
    let scenarios = if has_complete_scenario_snapshot {
        read_scenario_files()?
    } else {
        Vec::new()
    };
    let memberships = if has_complete_scenario_snapshot {
        read_membership_files()?
    } else {
        Vec::new()
    };
    let now = chrono::Utc::now().timestamp_millis();
    let skills_root = central_repo::skills_dir();

    let metadata_ids: HashSet<String> = skills.iter().map(|m| m.skill_id.clone()).collect();
    for existing in store.get_all_skills()? {
        if !metadata_ids.contains(&existing.id) {
            store.delete_skill(&existing.id)?;
        }
    }

    let existing_by_id: HashMap<String, SkillRecord> = store
        .get_all_skills()?
        .into_iter()
        .map(|skill| (skill.id.clone(), skill))
        .collect();

    for meta in skills {
        let skill_dir = skills_root.join(&meta.path);
        if !skill_dir.is_dir() {
            store.delete_skill(&meta.skill_id)?;
            continue;
        }

        let parsed = skill_metadata::parse_skill_md(&skill_dir);
        let inferred_name = skill_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown-skill".to_string());
        let name = parsed
            .name
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(inferred_name);
        let previous = existing_by_id.get(&meta.skill_id);
        let source_ref = if matches!(meta.source.source_type.as_str(), "import" | "local") {
            previous.and_then(|s| s.source_ref.clone())
        } else {
            meta.source.ref_.clone()
        };
        let central_path = skill_dir.to_string_lossy().to_string();

        let record = SkillRecord {
            id: meta.skill_id.clone(),
            name,
            description: parsed.description,
            source_type: meta.source.source_type.clone(),
            source_ref,
            source_ref_resolved: previous.and_then(|s| s.source_ref_resolved.clone()),
            source_subpath: meta.source.subpath.clone(),
            source_branch: meta.source.branch.clone(),
            source_revision: previous.and_then(|s| s.source_revision.clone()),
            remote_revision: previous.and_then(|s| s.remote_revision.clone()),
            central_path,
            content_hash: super::content_hash::hash_directory(&skill_dir).ok(),
            enabled: meta.enabled,
            created_at: previous.map(|s| s.created_at).unwrap_or(now),
            updated_at: now,
            status: "ok".to_string(),
            update_status: previous
                .map(|s| s.update_status.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            last_checked_at: previous.and_then(|s| s.last_checked_at),
            last_check_error: previous.and_then(|s| s.last_check_error.clone()),
        };
        store.upsert_skill(&record)?;
        store.set_tags_for_skill(&meta.skill_id, &meta.tags)?;
    }

    if has_complete_scenario_snapshot {
        store.replace_scenarios_from_metadata(&scenarios)?;
        store.replace_scenario_memberships_from_metadata(&memberships)?;
    }
    Ok(())
}

#[allow(dead_code)]
pub fn ensure_skill_metadata(store: &SkillStore, skill_id: &str) -> Result<()> {
    let skills_dir = central_repo::skills_dir();
    let _lock = RepoLock::acquire(&skills_dir, "write skill metadata")?;
    ensure_skill_metadata_unlocked(store, skill_id)
}

pub(crate) fn ensure_skill_metadata_unlocked(store: &SkillStore, skill_id: &str) -> Result<()> {
    write_schema()?;
    let skill = store
        .get_skill_by_id(skill_id)?
        .ok_or_else(|| anyhow!("skill not found: {skill_id}"))?;
    let tags = store.get_tags_map()?.remove(skill_id).unwrap_or_default();
    write_skill_file(&skill, &tags)
}

pub fn cleanup_temporary_files() -> Result<()> {
    let root = metadata_dir();
    if !root.exists() {
        return Ok(());
    }
    for entry in WalkDir::new(root).into_iter().flatten() {
        if entry.file_type().is_file() {
            let name = entry.file_name().to_string_lossy();
            if name.contains(".tmp.") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}

fn write_schema() -> Result<()> {
    let schema = SchemaFile {
        schema_version: SCHEMA_VERSION,
        app_min_version: APP_MIN_VERSION.to_string(),
        created_by: "skills-manager".to_string(),
    };
    atomic_write_json(&metadata_dir().join("schema.json"), &schema)
}

fn ensure_metadata_dirs() -> Result<()> {
    fs::create_dir_all(metadata_dir().join("skills"))?;
    fs::create_dir_all(metadata_dir().join("scenarios"))?;
    fs::create_dir_all(metadata_dir().join("scenario-skills"))?;
    Ok(())
}

fn metadata_has_complete_scenario_snapshot() -> bool {
    metadata_dir().join("schema.json").is_file()
        && metadata_dir().join("scenarios").is_dir()
        && metadata_dir().join("scenario-skills").is_dir()
}

fn write_skill_records_from_db(store: &SkillStore) -> Result<()> {
    let mut tags = store.get_tags_map()?;
    for skill in store.get_all_skills()? {
        write_skill_file(&skill, &tags.remove(&skill.id).unwrap_or_default())?;
    }
    Ok(())
}

fn write_scenario_records_from_db(store: &SkillStore) -> Result<()> {
    for scenario in store.get_all_scenarios()? {
        write_scenario_file(&scenario)?;
        let skill_ids = store.get_skill_ids_for_scenario(&scenario.id)?;
        for (index, skill_id) in skill_ids.iter().enumerate() {
            let tools = store
                .get_scenario_skill_tool_toggles(&scenario.id, skill_id)?
                .into_iter()
                .map(|toggle| (toggle.tool, toggle.enabled))
                .collect::<BTreeMap<_, _>>();
            let member = ScenarioSkillMetaFile {
                schema_version: SCHEMA_VERSION,
                scenario_id: scenario.id.clone(),
                skill_id: skill_id.clone(),
                sort_order: index as i32,
                tools,
            };
            write_membership_file(&member)?;
        }
    }
    Ok(())
}

fn remove_stale_metadata_files(store: &SkillStore) -> Result<()> {
    let skill_ids: HashSet<String> = store
        .get_all_skills()?
        .into_iter()
        .map(|skill| skill.id)
        .collect();
    remove_stale_json_files(&metadata_dir().join("skills"), &skill_ids)?;

    let scenario_ids: HashSet<String> = store
        .get_all_scenarios()?
        .into_iter()
        .map(|scenario| scenario.id)
        .collect();
    remove_stale_json_files(&metadata_dir().join("scenarios"), &scenario_ids)?;

    let membership_root = metadata_dir().join("scenario-skills");
    if membership_root.exists() {
        let mut expected_memberships = HashSet::new();
        for scenario_id in &scenario_ids {
            for skill_id in store.get_skill_ids_for_scenario(scenario_id)? {
                expected_memberships.insert((scenario_id.clone(), skill_id));
            }
        }

        for entry in fs::read_dir(&membership_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let scenario_id = entry.file_name().to_string_lossy().to_string();
            if !scenario_ids.contains(&scenario_id) {
                fs::remove_dir_all(entry.path())?;
                continue;
            }

            let mut has_files = false;
            for member in fs::read_dir(entry.path())? {
                let member = member?;
                if !member.file_type()?.is_file() {
                    continue;
                }
                let path = member.path();
                if path.extension().map(|ext| ext == "json").unwrap_or(false) {
                    let skill_id = path
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if !expected_memberships.contains(&(scenario_id.clone(), skill_id)) {
                        fs::remove_file(&path)?;
                    } else {
                        has_files = true;
                    }
                }
            }
            if !has_files && fs::read_dir(entry.path())?.next().is_none() {
                fs::remove_dir(entry.path())?;
            }
        }
    }

    Ok(())
}

fn remove_stale_json_files(dir: &Path, expected_stems: &HashSet<String>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().map(|ext| ext == "json").unwrap_or(false) {
            let stem = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_default();
            if !expected_stems.contains(&stem) {
                fs::remove_file(&path)?;
            }
        }
    }
    Ok(())
}

fn write_skill_file(skill: &SkillRecord, tags: &[String]) -> Result<()> {
    let path = relative_skill_path(&skill.central_path)?;
    let tags = sorted_tags(tags);
    let source_ref = match skill.source_type.as_str() {
        "import" | "local" => None,
        _ => skill.source_ref.clone(),
    };
    let meta = SkillMetaFile {
        schema_version: SCHEMA_VERSION,
        skill_id: skill.id.clone(),
        path_key: path_key(&path),
        path,
        enabled: skill.enabled,
        tags,
        source: SourceMeta {
            source_type: skill.source_type.clone(),
            ref_: source_ref,
            subpath: skill.source_subpath.clone(),
            branch: skill.source_branch.clone(),
        },
    };
    atomic_write_json(
        &metadata_dir()
            .join("skills")
            .join(format!("{}.json", skill.id)),
        &meta,
    )
}

fn write_scenario_file(scenario: &ScenarioRecord) -> Result<()> {
    let meta = ScenarioMetaFile {
        schema_version: SCHEMA_VERSION,
        scenario_id: scenario.id.clone(),
        name: scenario.name.clone(),
        description: scenario.description.clone(),
        icon: scenario.icon.clone(),
        sort_order: scenario.sort_order,
    };
    atomic_write_json(
        &metadata_dir()
            .join("scenarios")
            .join(format!("{}.json", scenario.id)),
        &meta,
    )
}

fn write_membership_file(member: &ScenarioSkillMetaFile) -> Result<()> {
    atomic_write_json(
        &metadata_dir()
            .join("scenario-skills")
            .join(&member.scenario_id)
            .join(format!("{}.json", member.skill_id)),
        member,
    )
}

fn read_skill_files() -> Result<Vec<SkillMetaFile>> {
    read_json_files(metadata_dir().join("skills"))
}

fn read_scenario_files() -> Result<Vec<ScenarioMetaFile>> {
    read_json_files(metadata_dir().join("scenarios"))
}

fn read_membership_files() -> Result<Vec<ScenarioSkillMetaFile>> {
    let root = metadata_dir().join("scenario-skills");
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in WalkDir::new(root)
        .min_depth(2)
        .max_depth(2)
        .into_iter()
        .flatten()
    {
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|ext| ext == "json")
        {
            let raw = fs::read_to_string(entry.path())?;
            records.push(serde_json::from_str(&raw).with_context(|| {
                format!(
                    "invalid scenario membership metadata {}",
                    entry.path().display()
                )
            })?);
        }
    }
    Ok(records)
}

fn read_json_files<T: for<'de> Deserialize<'de>>(dir: PathBuf) -> Result<Vec<T>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let is_json = entry
            .path()
            .extension()
            .map(|ext| ext == "json")
            .unwrap_or(false);
        if !entry.file_type()?.is_file() || !is_json {
            continue;
        }
        let raw = fs::read_to_string(entry.path())?;
        records.push(
            serde_json::from_str(&raw)
                .with_context(|| format!("invalid metadata file {}", entry.path().display()))?,
        );
    }
    Ok(records)
}

fn ensure_unique_path_keys(skills: &[SkillMetaFile]) -> Result<()> {
    let mut seen = HashMap::new();
    for skill in skills {
        let computed = path_key(&skill.path);
        if computed != skill.path_key {
            bail!("path_key mismatch for skill {}", skill.skill_id);
        }
        if let Some(previous) = seen.insert(skill.path_key.clone(), skill.skill_id.clone()) {
            bail!(
                "case-insensitive path collision between {} and {}",
                previous,
                skill.skill_id
            );
        }
    }
    Ok(())
}

fn relative_skill_path(central_path: &str) -> Result<String> {
    let root = central_repo::skills_dir();
    let path = PathBuf::from(central_path);
    let relative = path
        .strip_prefix(&root)
        .with_context(|| format!("skill path is outside skills root: {}", path.display()))?;
    let value = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/");
    if value.is_empty() || value.starts_with("../") {
        bail!("invalid relative skill path: {value}");
    }
    Ok(value)
}

fn path_key(path: &str) -> String {
    path.split('/')
        .map(|part| caseless::default_case_fold_str(&part.nfc().collect::<String>()))
        .collect::<Vec<_>>()
        .join("/")
}

fn sorted_tags(tags: &[String]) -> Vec<String> {
    tags.iter()
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    let tmp = path.with_extension(format!("json.tmp.{}", uuid::Uuid::now_v7()));
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    sync_parent_dir(path)?;
    Ok(())
}

fn sync_parent_dir(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        // Windows requires FILE_FLAG_BACKUP_SEMANTICS to open a directory
        // handle. std::fs::File::open uses ordinary file semantics and fails
        // with Access is denied (os error 5) for directories.
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x02000000;

        let dir = fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(parent)?;
        dir.sync_all()?;
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let dir = fs::File::open(parent)?;
        dir.sync_all()?;
        Ok(())
    }
}
