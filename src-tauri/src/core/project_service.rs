use std::fs;
use std::path::Path;

use super::error::AppError;
use super::skill_store::{ProjectRecord, SkillStore};

/// Register a project directory using the same filesystem layout as the app UI.
pub fn add_project(store: &SkillStore, path: &Path) -> Result<ProjectRecord, AppError> {
    if !path.is_dir() {
        return Err(AppError::invalid_input("Directory does not exist"));
    }

    let canonical_path = fs::canonicalize(path)?;
    let canonical_path_string = canonical_path.to_string_lossy().into_owned();
    let existing_projects = store.get_all_projects().map_err(AppError::db)?;

    if let Some(existing) = existing_projects.into_iter().find(|project| {
        project.path == canonical_path_string
            || fs::canonicalize(&project.path)
                .map(|existing_path| existing_path == canonical_path)
                .unwrap_or(false)
    }) {
        return Err(AppError::invalid_input(format!(
            "Project '{}' is already linked at {}",
            existing.name, existing.path
        )));
    }

    fs::create_dir_all(canonical_path.join(".claude/skills"))?;
    fs::create_dir_all(canonical_path.join(".claude/skills-disabled"))?;

    let name = canonical_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string());
    let now = chrono::Utc::now().timestamp_millis();
    let record = ProjectRecord {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        path: canonical_path_string,
        workspace_type: "project".to_string(),
        linked_agent_key: None,
        linked_agent_name: None,
        disabled_path: None,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    };

    store.insert_project(&record).map_err(AppError::db)?;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use crate::core::skill_store::SkillStore;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn add_project_creates_skill_roots_and_persists_canonical_path() {
        let tmp = tempdir().unwrap();
        let project_dir = tmp.path().join("repo");
        fs::create_dir(&project_dir).unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();

        let record = super::add_project(&store, &project_dir).unwrap();

        assert_eq!(
            record.path,
            fs::canonicalize(&project_dir)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        );
        assert_eq!(record.name, "repo");
        assert!(project_dir.join(".claude/skills").is_dir());
        assert!(project_dir.join(".claude/skills-disabled").is_dir());
        let projects = store.get_all_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, record.id);
    }

    #[test]
    fn add_project_rejects_missing_directory() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();

        let error = super::add_project(&store, &tmp.path().join("missing")).unwrap_err();

        assert!(error.to_string().contains("Directory does not exist"));
        assert!(store.get_all_projects().unwrap().is_empty());
    }

    #[test]
    fn add_project_rejects_canonical_duplicate_path() {
        let tmp = tempdir().unwrap();
        let project_dir = tmp.path().join("repo");
        fs::create_dir(&project_dir).unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        super::add_project(&store, &project_dir).unwrap();

        let error = super::add_project(&store, &project_dir.join("..//repo")).unwrap_err();

        assert!(error.to_string().contains("already linked"));
        assert_eq!(store.get_all_projects().unwrap().len(), 1);
    }
}
