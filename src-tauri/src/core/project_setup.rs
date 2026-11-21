//! Project setup for one exact checkout, behind a single host-scoped request
//! with a tagged `action`. Later actions join [`Request`]; they never add
//! commands.
//!
//! Discovery is read-only: it never registers projects, deploys, or writes
//! the store, so it can run on a store opened read-only.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::core::error::AppError;
use crate::core::host::HostCtx;
use crate::core::skill_store::ProjectRecord;
use crate::core::sync_service;

/// Library options per page.
pub const PAGE_SIZE: usize = 50;
/// Registered projects offered when the checkout is not one of them.
pub const MAX_CANDIDATES: usize = 10;
/// The `resource_state` kind that holds each project's saved setup.
const SETUP_KIND: &str = "project_setup";

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Request {
    Discover {
        checkout_path: String,
        #[serde(default)]
        cursor: Option<String>,
    },
}

pub fn dispatch(ctx: &HostCtx, request: Value) -> Result<Value, AppError> {
    let request: Request = serde_json::from_value(request)
        .map_err(|e| AppError::invalid_input(format!("Invalid Project setup request: {e}")))?;
    match request {
        Request::Discover {
            checkout_path,
            cursor,
        } => discover(ctx, &checkout_path, cursor.as_deref()),
    }
}

fn discover(ctx: &HostCtx, checkout_path: &str, cursor: Option<&str>) -> Result<Value, AppError> {
    let checkout = canonical_checkout(checkout_path)?;
    // Both sides canonical and compared whole, so a linked worktree or a
    // folder inside a project never resolves to the project around it.
    let projects: Vec<(ProjectRecord, PathBuf)> = ctx
        .store
        .get_all_projects()
        .map_err(AppError::db)?
        .into_iter()
        .filter(|project| project.workspace_type == "project")
        .filter_map(|project| {
            let path = Path::new(&project.path).canonicalize().ok()?;
            Some((project, path))
        })
        .collect();
    let exact = projects.iter().find(|(_, path)| *path == checkout);
    let candidates: Vec<Value> = match exact {
        Some(_) => Vec::new(),
        None => projects
            .iter()
            .filter(|(_, path)| checkout.starts_with(path) || path.starts_with(&checkout))
            .take(MAX_CANDIDATES)
            .map(|(project, path)| project_json(project, path))
            .collect(),
    };
    let saved_setup = match exact {
        Some((project, _)) => ctx
            .store
            .resource_state_get(SETUP_KIND, &project.id)
            .map_err(AppError::db)?,
        None => None,
    };
    let status = sync_service::sync_status(&ctx.store);
    Ok(json!({
        "host": status.host,
        "checkout_path": checkout,
        "project": exact.map(|(project, path)| project_json(project, path)),
        "candidates": candidates,
        "saved_setup": saved_setup,
        "library": library_page(ctx, cursor)?,
        // Read from this host's last sync; discovery never contacts the remote.
        "sync": {
            "state": status.state,
            "reason": status.reason,
            "remote": status.remote,
            "last_sync_at": status.last_sync,
            "checked_at": chrono::Utc::now().timestamp_millis(),
            "verified": false,
        },
        "conflicts": {
            "skills": status.conflicts,
            "resources": status.resource_conflicts,
            "same_name_presets": status.same_name_presets,
            "edited_here": status.edited_here,
        },
    }))
}

fn canonical_checkout(checkout_path: &str) -> Result<PathBuf, AppError> {
    let path = Path::new(checkout_path);
    if !path.is_absolute() {
        return Err(AppError::invalid_input(
            "checkout_path must be an absolute path",
        ));
    }
    path.canonicalize()
        .map_err(|e| AppError::invalid_input(format!("Cannot open checkout {checkout_path}: {e}")))
}

fn project_json(project: &ProjectRecord, path: &Path) -> Value {
    json!({
        "id": project.id,
        "name": project.name,
        "path": path,
        "agent_keys": project.agent_keys,
        "deploy_mode": project.deploy_mode,
    })
}

/// One page of Skills, Instruction bundles and MCP connections, in that
/// order. The cursor is the offset of the page's first item.
fn library_page(ctx: &HostCtx, cursor: Option<&str>) -> Result<Value, AppError> {
    let start: usize = match cursor {
        None => 0,
        Some(cursor) => cursor
            .parse()
            .map_err(|_| AppError::invalid_input("Invalid Library cursor"))?,
    };
    let option =
        |kind: &str, item: &Value| json!({ "kind": kind, "id": item["id"], "name": item["name"] });
    let mut items: Vec<Value> = ctx
        .store
        .get_all_skills()
        .map_err(AppError::db)?
        .into_iter()
        .map(|skill| json!({ "kind": "skill", "id": skill.id, "name": skill.name }))
        .collect();
    let instructions = crate::core::instructions::dispatch(ctx, json!({ "action": "list" }))?;
    items.extend(listed(&instructions, "items").map(|item| option("instruction", item)));
    let mcps = crate::core::mcps::dispatch(ctx, json!({ "action": "list" }))?;
    items.extend(listed(&mcps, "definitions").map(|item| option("mcp", item)));

    let end = items.len().min(start.saturating_add(PAGE_SIZE));
    let next_cursor = (end < items.len()).then(|| end.to_string());
    Ok(json!({
        "items": items.get(start..end).unwrap_or_default(),
        "next_cursor": next_cursor,
    }))
}

fn listed<'a>(reply: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    reply[key].as_array().into_iter().flatten()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use serde_json::{json, Value};

    use crate::core::error::{AppError, ErrorKind};
    use crate::core::host::{HostCtx, NoopEvents};
    use crate::core::host_dispatch::dispatch;
    use crate::core::skill_store::{ProjectRecord, SkillStore};
    use crate::core::test_support::{sample_skill, test_repo, write_skill_dir, TestRepo};

    fn project(id: &str, path: &Path) -> ProjectRecord {
        ProjectRecord {
            id: id.to_string(),
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            path: path.to_string_lossy().into_owned(),
            workspace_type: "project".to_string(),
            linked_agent_key: None,
            linked_agent_name: None,
            disabled_path: None,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
            agent_keys: Some(vec!["codex".to_string()]),
            deploy_mode: "link".to_string(),
        }
    }

    /// A temp library with its own store, and a folder for checkouts.
    struct Host {
        repo: TestRepo,
        ctx: HostCtx,
    }

    impl Host {
        fn new() -> Self {
            let repo = test_repo();
            let store = SkillStore::new(&repo._tmp.path().join("repo/test.db")).unwrap();
            let ctx = HostCtx::for_tests(store, Arc::new(NoopEvents));
            Self { repo, ctx }
        }

        fn dir(&self, relative: &str) -> PathBuf {
            let dir = self.repo._tmp.path().join("work").join(relative);
            std::fs::create_dir_all(&dir).unwrap();
            dir.canonicalize().unwrap()
        }

        fn discover(&self, args: Value) -> Result<Value, AppError> {
            let mut request = json!({ "action": "discover" });
            request
                .as_object_mut()
                .unwrap()
                .extend(args.as_object().unwrap().clone());
            dispatch(
                &self.ctx,
                "project_setup_request",
                &json!({ "request": request }),
            )
        }
    }

    #[test]
    fn an_exact_checkout_resolves_to_its_project() {
        let host = Host::new();
        let checkout = host.dir("app");
        // Stored through a path that is not canonical; asked by the real one.
        let stored = checkout.join("../app");
        host.repo
            .store
            .insert_project(&project("p1", &stored))
            .unwrap();

        let found = host.discover(json!({ "checkout_path": checkout })).unwrap();

        assert_eq!(found["checkout_path"], json!(checkout));
        assert_eq!(found["project"]["id"], "p1");
        assert_eq!(found["project"]["agent_keys"], json!(["codex"]));
        assert_eq!(found["candidates"], json!([]));
    }

    #[test]
    fn a_linked_worktree_never_resolves_to_its_main_checkout() {
        let host = Host::new();
        let main = host.dir("app");
        let worktree = host.dir("app/.worktrees/feature");
        host.repo
            .store
            .insert_project(&project("main", &main))
            .unwrap();

        let found = host.discover(json!({ "checkout_path": worktree })).unwrap();

        assert_eq!(found["project"], Value::Null);
        assert_eq!(found["saved_setup"], Value::Null);
        // Offered for the caller to tell apart, never chosen for it.
        assert_eq!(found["candidates"][0]["id"], "main");
        assert_eq!(found["candidates"][0]["path"], json!(main));
    }

    #[test]
    fn an_unregistered_checkout_is_reported_without_creating_records() {
        let host = Host::new();
        let checkout = host.dir("fresh");
        for n in 0..12 {
            let inside = host.dir(&format!("fresh/pkg{n}"));
            host.repo
                .store
                .insert_project(&project(&format!("p{n}"), &inside))
                .unwrap();
        }

        let found = host.discover(json!({ "checkout_path": checkout })).unwrap();

        assert_eq!(found["project"], Value::Null);
        assert_eq!(found["candidates"].as_array().unwrap().len(), 10);
        assert_eq!(host.repo.store.get_all_projects().unwrap().len(), 12);
        let entries: Vec<_> = std::fs::read_dir(&checkout).unwrap().collect();
        assert_eq!(entries.len(), 12, "discovery added files to the checkout");
    }

    #[test]
    fn a_registered_project_returns_its_saved_setup() {
        let host = Host::new();
        let checkout = host.dir("app");
        host.repo
            .store
            .insert_project(&project("p1", &checkout))
            .unwrap();
        let setup = json!({ "items": [{ "kind": "skill", "id": "s1", "agents": ["codex"] }] });
        host.repo
            .store
            .resource_state_put("project_setup", "p1", &setup)
            .unwrap();

        let found = host.discover(json!({ "checkout_path": checkout })).unwrap();

        assert_eq!(found["saved_setup"], setup);
        assert_eq!(found["sync"]["verified"], false);
        assert_eq!(found["sync"]["state"], "not_set_up");
    }

    #[test]
    fn library_options_come_in_pages_of_50() {
        let host = Host::new();
        let checkout = host.dir("app");
        for n in 0..55 {
            let name = format!("skill-{n:02}");
            let dir = write_skill_dir(&name);
            host.repo
                .store
                .insert_skill(&sample_skill(&format!("s{n:02}"), &name, &dir))
                .unwrap();
        }

        let first = host.discover(json!({ "checkout_path": checkout })).unwrap();
        let cursor = first["library"]["next_cursor"].clone();
        let second = host
            .discover(json!({ "checkout_path": checkout, "cursor": cursor }))
            .unwrap();

        assert_eq!(first["library"]["items"].as_array().unwrap().len(), 50);
        assert_eq!(
            first["library"]["items"][0],
            json!({ "kind": "skill", "id": "s00", "name": "skill-00" })
        );
        let rest = second["library"]["items"].as_array().unwrap();
        assert_eq!(rest.len(), 5);
        assert_eq!(rest[4]["id"], "s54");
        assert_eq!(second["library"]["next_cursor"], Value::Null);
    }

    #[test]
    fn a_relative_checkout_path_is_refused() {
        let host = Host::new();
        let error = host
            .discover(json!({ "checkout_path": "work/app" }))
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidInput);
    }
}
