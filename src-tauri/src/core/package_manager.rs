use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::{DirEntry, WalkDir};

use super::git_fetcher;
use super::path_guard;
use super::skill_metadata;
use super::skill_store::{
    PackageBindingRecord, PackageComponentRecord, PackageRecord, PackageSurfaceRecord, SkillStore,
};
use super::{central_repo, sync_engine, tool_adapters};

const PROJECT_MANIFEST_RELATIVE_PATH: &str = ".skillapse/project.json";
const MAX_SETUP_OUTPUT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct PackageDetails {
    pub package: PackageRecord,
    pub components: Vec<PackageComponentRecord>,
    pub surfaces: Vec<PackageSurfaceRecord>,
    pub bindings: Vec<PackageBindingRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SurfaceCoverage {
    pub name: String,
    pub kind: String,
    pub relative_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanOperation {
    pub kind: String,
    pub description: String,
    pub target: String,
    pub command: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BindingPlan {
    pub binding_id: String,
    pub package_name: String,
    pub package_revision: String,
    pub tool: String,
    pub scope: String,
    pub compatibility: String,
    pub surface_id: Option<String>,
    pub surface_kind: Option<String>,
    pub covered_components: Vec<String>,
    pub missing_components: Vec<String>,
    pub risk_items: Vec<String>,
    pub operations: Vec<PlanOperation>,
    pub plan_hash: String,
    pub can_apply: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplyResult {
    pub binding: PackageBindingRecord,
    pub plan: BindingPlan,
    pub output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectManifest {
    version: u32,
    #[serde(default)]
    packages: Vec<ProjectManifestPackage>,
    #[serde(default)]
    bindings: Vec<ProjectManifestBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectManifestPackage {
    id: String,
    source: String,
    revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectManifestBinding {
    package: String,
    tool: String,
    #[serde(default = "default_surface_policy")]
    surface: String,
    #[serde(default)]
    components: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedHookFile {
    path: PathBuf,
    hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CodexBundleTargetRef {
    skill_targets: Vec<PathBuf>,
    hook_files: Vec<ManagedHookFile>,
    hooks_file: PathBuf,
    added_hooks: BTreeMap<String, Vec<serde_json::Value>>,
}

fn default_surface_policy() -> String {
    "auto".to_string()
}

#[derive(Debug)]
struct ScannedInventory {
    name: String,
    manifest_kind: String,
    components: Vec<PackageComponentRecord>,
    surfaces: Vec<PackageSurfaceRecord>,
}

pub fn list_packages(store: &SkillStore) -> Result<Vec<PackageDetails>> {
    store
        .get_all_packages()?
        .into_iter()
        .map(|package| details_for_record(store, package))
        .collect()
}

pub fn package_details(store: &SkillStore, package_id: &str) -> Result<PackageDetails> {
    let package = store
        .get_package_by_id(package_id)?
        .ok_or_else(|| anyhow!("Package not found: {package_id}"))?;
    details_for_record(store, package)
}

fn details_for_record(store: &SkillStore, package: PackageRecord) -> Result<PackageDetails> {
    Ok(PackageDetails {
        components: store.get_package_components(&package.id)?,
        surfaces: store.get_package_surfaces(&package.id)?,
        bindings: store.get_package_bindings(&package.id)?,
        package,
    })
}

pub fn import_git_package(
    store: &SkillStore,
    source_url: &str,
    requested_revision: Option<&str>,
) -> Result<PackageDetails> {
    import_git_package_with_id(store, source_url, requested_revision, None)
}

fn import_git_package_with_id(
    store: &SkillStore,
    source_url: &str,
    requested_revision: Option<&str>,
    preferred_id: Option<&str>,
) -> Result<PackageDetails> {
    if source_url.trim_start().starts_with('-') {
        bail!("Package URL must not start with '-'");
    }
    if super::git_credentials::split_credentials_from_url(source_url).is_some() {
        bail!("Package URLs must not contain embedded credentials; use your Git credential helper");
    }
    git_fetcher::validate_git_url(source_url)?;
    let parsed = git_fetcher::parse_git_source(source_url);
    let source = parsed.clone_url;
    let revision = requested_revision
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or(parsed.branch);

    let existing = store.get_package_by_source_url(&source)?;
    let id = existing
        .as_ref()
        .map(|package| package.id.clone())
        .or_else(|| {
            preferred_id
                .filter(|id| uuid::Uuid::parse_str(id).is_ok())
                .map(str::to_string)
        })
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let created_at = existing
        .as_ref()
        .map(|package| package.created_at)
        .unwrap_or_else(now_ms);

    let pinned_commit = revision
        .as_ref()
        .filter(|value| value.len() == 40 && value.chars().all(|ch| ch.is_ascii_hexdigit()))
        .cloned();
    let checkout = git_fetcher::clone_repo_ref(
        &source,
        if pinned_commit.is_some() {
            None
        } else {
            revision.as_deref()
        },
        None,
        store.proxy_url().as_deref(),
    )?;
    let result = (|| {
        if let Some(commit) = pinned_commit.as_deref() {
            let current = git_fetcher::get_head_revision(&checkout)?;
            if current != commit {
                git_fetcher::fetch_and_checkout_revision(
                    &checkout,
                    commit,
                    store.proxy_url().as_deref(),
                )
                .with_context(|| {
                    format!("Pinned package revision {commit} is not available from the remote")
                })?;
            }
        }
        install_package_checkout(store, &id, created_at, &source, revision, &checkout)
    })();
    git_fetcher::cleanup_temp(&checkout);
    result
}

pub fn update_package(store: &SkillStore, package_id: &str) -> Result<PackageDetails> {
    let package = store
        .get_package_by_id(package_id)?
        .ok_or_else(|| anyhow!("Package not found: {package_id}"))?;
    import_git_package(
        store,
        &package.source_url,
        package.requested_revision.as_deref(),
    )
}

fn install_package_checkout(
    store: &SkillStore,
    package_id: &str,
    created_at: i64,
    source_url: &str,
    requested_revision: Option<String>,
    checkout: &Path,
) -> Result<PackageDetails> {
    let resolved_revision = git_fetcher::get_head_revision(checkout)?;
    let packages_dir = central_repo::packages_dir();
    fs::create_dir_all(&packages_dir)?;
    let destination = packages_dir.join(package_id);
    let stage = packages_dir.join(format!(".{package_id}.stage-{}", uuid::Uuid::new_v4()));
    let backup = packages_dir.join(format!(".{package_id}.backup-{}", uuid::Uuid::new_v4()));

    copy_package_tree(checkout, &stage)?;
    let mut inventory = scan_package(&stage, package_id)?;
    if inventory.name == "package" {
        inventory.name = package_name_from_source(source_url);
    }
    if inventory.components.is_empty() && inventory.surfaces.is_empty() {
        let _ = fs::remove_dir_all(&stage);
        bail!("Repository does not contain supported skills, plugins, rules, hooks, agents, commands, or MCP definitions");
    }

    let now = now_ms();
    let package = PackageRecord {
        id: package_id.to_string(),
        name: inventory.name.clone(),
        source_url: source_url.to_string(),
        requested_revision,
        resolved_revision,
        cache_path: destination.to_string_lossy().to_string(),
        manifest_kind: inventory.manifest_kind.clone(),
        status: "ready".to_string(),
        created_at,
        updated_at: now,
    };

    if destination.exists() {
        fs::rename(&destination, &backup).with_context(|| {
            format!("Failed to stage existing package {}", destination.display())
        })?;
    }
    if let Err(error) = fs::rename(&stage, &destination) {
        if backup.exists() {
            let _ = fs::rename(&backup, &destination);
        }
        return Err(error).context("Failed to activate package snapshot");
    }

    // IDs and paths are relative, so the inventory scanned from staging is
    // valid after the atomic rename.
    inventory
        .components
        .iter_mut()
        .for_each(|component| component.package_id = package_id.to_string());
    inventory
        .surfaces
        .iter_mut()
        .for_each(|surface| surface.package_id = package_id.to_string());

    if let Err(error) =
        store.replace_package_inventory(&package, &inventory.components, &inventory.surfaces)
    {
        let _ = fs::remove_dir_all(&destination);
        if backup.exists() {
            let _ = fs::rename(&backup, &destination);
        }
        return Err(error).context("Failed to persist package inventory");
    }
    if backup.exists() {
        store.mark_package_bindings_drifted(package_id)?;
        if let Err(error) = fs::remove_dir_all(&backup) {
            log::warn!(
                "Failed to remove old package snapshot {}: {error}",
                backup.display()
            );
        }
    }
    package_details(store, package_id)
}

pub fn delete_package(store: &SkillStore, package_id: &str) -> Result<()> {
    let package = store
        .get_package_by_id(package_id)?
        .ok_or_else(|| anyhow!("Package not found: {package_id}"))?;
    let bindings = store.get_package_bindings(package_id)?;
    if !bindings.is_empty() {
        bail!("Remove package bindings before deleting the package");
    }
    let path = PathBuf::from(&package.cache_path);
    if !path_guard::is_path_safe(&central_repo::packages_dir(), &path) {
        bail!("Refusing to delete package outside managed cache");
    }
    let trash =
        central_repo::packages_dir().join(format!(".{package_id}.delete-{}", uuid::Uuid::new_v4()));
    if fs::symlink_metadata(&path).is_ok() {
        fs::rename(&path, &trash)?;
    }
    if let Err(error) = store.delete_package(package_id) {
        if trash.exists() {
            let _ = fs::rename(&trash, &path);
        }
        return Err(error);
    }
    if trash.exists() {
        fs::remove_dir_all(&trash)?;
    }
    Ok(())
}

pub fn create_binding(
    store: &SkillStore,
    package_id: &str,
    tool: &str,
    scope: &str,
    project_id: Option<&str>,
    surface_policy: &str,
    requested_components: &[String],
) -> Result<BindingPlan> {
    validate_binding_input(store, package_id, tool, scope, project_id, surface_policy)?;
    let now = now_ms();
    let requested_components_json = serde_json::to_string(requested_components)?;
    let mut binding = store
        .get_package_binding(package_id, tool, scope, project_id)?
        .unwrap_or_else(|| PackageBindingRecord {
            id: uuid::Uuid::new_v4().to_string(),
            package_id: package_id.to_string(),
            tool: tool.to_string(),
            scope: scope.to_string(),
            project_id: project_id.map(str::to_string),
            surface_policy: surface_policy.to_string(),
            requested_components_json: requested_components_json.clone(),
            desired_enabled: true,
            resolved_surface_id: None,
            compatibility: "unsupported".to_string(),
            state: "not_applied".to_string(),
            target_ref: None,
            applied_revision: None,
            approved_plan_hash: None,
            last_error: None,
            created_at: now,
            updated_at: now,
        });
    if binding.target_ref.is_some()
        && (binding.surface_policy != surface_policy
            || binding.requested_components_json != requested_components_json)
    {
        bail!("Remove the installed binding before changing its surface or components");
    }
    binding.surface_policy = surface_policy.to_string();
    binding.requested_components_json = requested_components_json;
    binding.desired_enabled = true;
    binding.updated_at = now;
    store.upsert_package_binding(&binding)?;
    if scope == "project_shared" {
        write_project_manifest_binding(store, &binding)?;
    }
    preview_binding(store, &binding.id)
}

fn validate_binding_input(
    store: &SkillStore,
    package_id: &str,
    tool: &str,
    scope: &str,
    project_id: Option<&str>,
    surface_policy: &str,
) -> Result<()> {
    if store.get_package_by_id(package_id)?.is_none() {
        bail!("Package not found: {package_id}");
    }
    if tool_adapters::find_adapter_with_store(store, tool).is_none() {
        bail!("Unknown tool: {tool}");
    }
    if !matches!(
        scope,
        "user" | "project_shared" | "project_local" | "managed"
    ) {
        bail!("Invalid scope: {scope}");
    }
    if !matches!(surface_policy, "auto" | "native" | "portable" | "setup") {
        bail!("Invalid surface policy: {surface_policy}");
    }
    match scope {
        "project_shared" | "project_local" => {
            let project_id =
                project_id.ok_or_else(|| anyhow!("Project is required for {scope}"))?;
            if store.get_project_by_id(project_id)?.is_none() {
                bail!("Project not found: {project_id}");
            }
        }
        _ if project_id.is_some() => bail!("Project is only valid for project scopes"),
        _ => {}
    }
    Ok(())
}

pub fn preview_binding(store: &SkillStore, binding_id: &str) -> Result<BindingPlan> {
    let mut binding = store
        .get_package_binding_by_id(binding_id)?
        .ok_or_else(|| anyhow!("Binding not found: {binding_id}"))?;
    let package = store
        .get_package_by_id(&binding.package_id)?
        .ok_or_else(|| anyhow!("Package not found: {}", binding.package_id))?;
    let surfaces = store.get_package_surfaces(&package.id)?;
    let requested: Vec<String> = serde_json::from_str(&binding.requested_components_json)
        .context("Invalid binding component selection")?;

    let selected = select_surface(&binding, &surfaces);
    let mut plan = build_plan(store, &package, &binding, selected.as_ref(), &requested)?;
    plan.plan_hash = hash_plan(&plan)?;

    binding.resolved_surface_id = plan.surface_id.clone();
    binding.compatibility = plan.compatibility.clone();
    if binding.target_ref.is_none() {
        binding.state = "planned".to_string();
    }
    binding.approved_plan_hash = None;
    binding.last_error = None;
    binding.updated_at = now_ms();
    store.upsert_package_binding(&binding)?;
    Ok(plan)
}

fn select_surface(
    binding: &PackageBindingRecord,
    surfaces: &[PackageSurfaceRecord],
) -> Option<PackageSurfaceRecord> {
    let mut candidates: Vec<_> = surfaces
        .iter()
        .filter(|surface| surface.tool == binding.tool || surface.tool == "*")
        .filter(|surface| match binding.surface_policy.as_str() {
            "native" => surface.kind == "native_plugin",
            "portable" => surface.kind == "portable_skills",
            "setup" => surface.kind == "setup_script",
            _ => true,
        })
        .cloned()
        .collect();
    candidates.sort_by_key(|surface| {
        let kind_rank = match surface.kind.as_str() {
            "native_plugin" => 4,
            "host_bundle" => 3,
            "portable_skills" => 2,
            "setup_script" => 1,
            _ => 0,
        };
        let host_rank = if surface.tool == binding.tool { 1 } else { 0 };
        -(host_rank * 100_000 + kind_rank * 10_000 + surface.priority)
    });
    candidates.into_iter().next()
}

fn build_plan(
    store: &SkillStore,
    package: &PackageRecord,
    binding: &PackageBindingRecord,
    surface: Option<&PackageSurfaceRecord>,
    requested: &[String],
) -> Result<BindingPlan> {
    let mut plan = BindingPlan {
        binding_id: binding.id.clone(),
        package_name: package.name.clone(),
        package_revision: package.resolved_revision.clone(),
        tool: binding.tool.clone(),
        scope: binding.scope.clone(),
        compatibility: "unsupported".to_string(),
        surface_id: surface.map(|value| value.id.clone()),
        surface_kind: surface.map(|value| value.kind.clone()),
        covered_components: Vec::new(),
        missing_components: requested.to_vec(),
        risk_items: Vec::new(),
        operations: Vec::new(),
        plan_hash: String::new(),
        can_apply: false,
    };
    let Some(surface) = surface else {
        plan.risk_items
            .push("No compatible host surface was found".to_string());
        return Ok(plan);
    };

    let coverage: Vec<SurfaceCoverage> =
        serde_json::from_str(&surface.coverage_json).context("Invalid package surface coverage")?;
    let effective_requested: Vec<String> = if requested.is_empty() && surface.kind == "host_bundle"
    {
        coverage.iter().map(|item| item.name.clone()).collect()
    } else if requested.is_empty() {
        let mut names: Vec<_> = store
            .get_package_components(&package.id)?
            .into_iter()
            .filter(|component| {
                component
                    .host_hint
                    .as_deref()
                    .map(|host| host == binding.tool)
                    .unwrap_or(true)
            })
            .map(|component| component.name)
            .collect();
        names.sort();
        names.dedup();
        names
    } else {
        requested.to_vec()
    };
    let installs_whole_surface = matches!(
        surface.kind.as_str(),
        "native_plugin" | "host_bundle" | "setup_script"
    );
    let selected_coverage: Vec<_> = if installs_whole_surface {
        coverage.clone()
    } else {
        coverage
            .iter()
            .filter(|item| effective_requested.iter().any(|name| name == &item.name))
            .cloned()
            .collect()
    };
    plan.covered_components = selected_coverage
        .iter()
        .map(|item| item.name.clone())
        .collect();
    plan.covered_components.sort();
    plan.covered_components.dedup();
    plan.missing_components = effective_requested
        .iter()
        .filter(|name| !plan.covered_components.contains(name))
        .cloned()
        .collect();
    if installs_whole_surface && !requested.is_empty() {
        let extras: Vec<_> = plan
            .covered_components
            .iter()
            .filter(|name| !requested.contains(name))
            .cloned()
            .collect();
        if !extras.is_empty() {
            plan.risk_items.push(format!(
                "This surface installs bundled components outside the selection: {}",
                extras.join(", ")
            ));
        }
    }

    match surface.kind.as_str() {
        "portable_skills" => {
            build_portable_plan(store, package, binding, &selected_coverage, &mut plan)?
        }
        "host_bundle" => {
            build_host_bundle_plan(store, package, binding, &selected_coverage, &mut plan)?
        }
        "native_plugin" => build_native_plan(store, package, binding, surface, &mut plan)?,
        "setup_script" => build_setup_plan(package, binding, surface, &mut plan)?,
        _ => {}
    }

    if plan.operations.is_empty() {
        plan.compatibility = "unsupported".to_string();
        plan.can_apply = false;
    } else if plan.missing_components.is_empty() {
        plan.compatibility = "full".to_string();
        plan.can_apply = true;
    } else {
        plan.compatibility = "partial".to_string();
        plan.can_apply = true;
    }
    Ok(plan)
}

fn build_host_bundle_plan(
    store: &SkillStore,
    package: &PackageRecord,
    binding: &PackageBindingRecord,
    coverage: &[SurfaceCoverage],
    plan: &mut BindingPlan,
) -> Result<()> {
    if binding.tool != "codex" || binding.scope != "user" {
        plan.risk_items
            .push("Codex hook bundles currently support user scope only".to_string());
        return Ok(());
    }
    build_portable_plan(store, package, binding, coverage, plan)?;
    let adapter = tool_adapters::find_adapter_with_store(store, &binding.tool)
        .ok_or_else(|| anyhow!("Unknown tool: {}", binding.tool))?;
    let config_root = adapter
        .skills_dir()
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("Codex skills path has no config directory"))?;
    let source_hooks = Path::new(&package.cache_path).join(".codex/hooks");
    let source_manifest = Path::new(&package.cache_path).join(".codex/hooks.json");
    if !source_hooks.is_dir() || !source_manifest.is_file() {
        plan.risk_items
            .push("Codex hook bundle is incomplete".to_string());
        return Ok(());
    }
    plan.risk_items
        .push("Codex hooks execute repository scripts from the user config directory".to_string());
    plan.risk_items.push(
        "Review and trust newly installed hooks with /hooks before relying on them".to_string(),
    );
    plan.operations.push(PlanOperation {
        kind: "copy_hook_assets".to_string(),
        description: "Copy Codex hook assets with collision checks".to_string(),
        target: config_root.join("hooks").to_string_lossy().to_string(),
        command: None,
    });
    plan.operations.push(PlanOperation {
        kind: "merge_hooks".to_string(),
        description: "Merge package hooks into the existing Codex hook configuration".to_string(),
        target: config_root.join("hooks.json").to_string_lossy().to_string(),
        command: None,
    });
    Ok(())
}

fn build_portable_plan(
    store: &SkillStore,
    package: &PackageRecord,
    binding: &PackageBindingRecord,
    coverage: &[SurfaceCoverage],
    plan: &mut BindingPlan,
) -> Result<()> {
    if binding.scope == "managed" {
        plan.risk_items
            .push("Managed scope is read-only in this build".to_string());
        return Ok(());
    }
    let adapter = tool_adapters::find_adapter_with_store(store, &binding.tool)
        .ok_or_else(|| anyhow!("Unknown tool: {}", binding.tool))?;
    let target_root = binding_target_root(store, binding, &adapter)?;
    let package_root = Path::new(&package.cache_path);
    for item in coverage {
        let source = package_root.join(&item.relative_path);
        if !skill_metadata::is_valid_skill_dir(&source) {
            continue;
        }
        let safe_name = skill_metadata::sanitize_skill_name(&item.name)
            .ok_or_else(|| anyhow!("Unsafe skill name: {}", item.name))?;
        let target = target_root.join(safe_name);
        plan.operations.push(PlanOperation {
            kind: "link_skill".to_string(),
            description: format!("Link skill {}", item.name),
            target: target.to_string_lossy().to_string(),
            command: None,
        });
    }
    Ok(())
}

fn build_native_plan(
    store: &SkillStore,
    package: &PackageRecord,
    binding: &PackageBindingRecord,
    surface: &PackageSurfaceRecord,
    plan: &mut BindingPlan,
) -> Result<()> {
    if binding.scope == "managed" {
        plan.risk_items
            .push("Managed scope is read-only in this build".to_string());
        return Ok(());
    }
    if binding.tool == "codex" && binding.scope != "user" {
        plan.risk_items
            .push("Codex native plugins currently support user scope only".to_string());
        return Ok(());
    }
    if !matches!(binding.tool.as_str(), "codex" | "claude_code") {
        return Ok(());
    }
    let package_root = Path::new(&package.cache_path);
    let (marketplace_path, marketplace_name) = native_marketplace(package_root, &binding.tool)?;
    let Some((marketplace_path, marketplace_name)) = marketplace_path.zip(marketplace_name) else {
        plan.risk_items.push(format!(
            "{} plugin manifest exists, but no compatible marketplace manifest was found",
            binding.tool
        ));
        return Ok(());
    };
    let plugin_name = native_plugin_name(package_root, surface)?;
    let selector = format!("{plugin_name}@{marketplace_name}");
    let project = binding_project(store, binding)?;

    let add_command = if binding.tool == "codex" {
        vec![
            "codex".to_string(),
            "plugin".to_string(),
            "marketplace".to_string(),
            "add".to_string(),
            marketplace_path.to_string_lossy().to_string(),
            "--json".to_string(),
        ]
    } else {
        vec![
            "claude".to_string(),
            "plugin".to_string(),
            "marketplace".to_string(),
            "add".to_string(),
            "--scope".to_string(),
            claude_scope(&binding.scope)?.to_string(),
            marketplace_path.to_string_lossy().to_string(),
        ]
    };
    let install_command = if binding.tool == "codex" {
        vec![
            "codex".to_string(),
            "plugin".to_string(),
            "add".to_string(),
            selector.clone(),
            "--json".to_string(),
        ]
    } else {
        vec![
            "claude".to_string(),
            "plugin".to_string(),
            "install".to_string(),
            "--scope".to_string(),
            claude_scope(&binding.scope)?.to_string(),
            selector.clone(),
        ]
    };
    plan.risk_items
        .push("Native plugins may enable hooks, MCP servers, apps, or commands".to_string());
    plan.operations.push(PlanOperation {
        kind: "add_marketplace".to_string(),
        description: format!("Register marketplace {marketplace_name}"),
        target: project
            .as_ref()
            .map(|value| value.path.clone())
            .unwrap_or_else(|| "user".to_string()),
        command: Some(add_command),
    });
    plan.operations.push(PlanOperation {
        kind: "install_plugin".to_string(),
        description: format!("Install native plugin {selector}"),
        target: binding.scope.clone(),
        command: Some(install_command),
    });
    Ok(())
}

fn build_setup_plan(
    package: &PackageRecord,
    binding: &PackageBindingRecord,
    surface: &PackageSurfaceRecord,
    plan: &mut BindingPlan,
) -> Result<()> {
    if binding.scope != "user" {
        plan.risk_items
            .push("Repository setup scripts are supported only in user scope".to_string());
        return Ok(());
    }
    let command: Vec<String> = surface
        .install_command_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_default();
    if command.is_empty() {
        return Ok(());
    }
    plan.risk_items.push(
        "This repository provides an executable setup script; review the command before applying"
            .to_string(),
    );
    plan.risk_items
        .push("Setup-based installs have no safe target-specific automatic removal".to_string());
    plan.operations.push(PlanOperation {
        kind: "run_setup".to_string(),
        description: format!("Run {} setup for {}", package.name, binding.tool),
        target: package.cache_path.clone(),
        command: Some(command),
    });
    Ok(())
}

fn binding_target_root(
    store: &SkillStore,
    binding: &PackageBindingRecord,
    adapter: &tool_adapters::ToolAdapter,
) -> Result<PathBuf> {
    match binding.scope.as_str() {
        "user" => Ok(adapter.skills_dir()),
        "project_shared" | "project_local" => {
            let project = binding_project(store, binding)?
                .ok_or_else(|| anyhow!("Project is required for project scope"))?;
            let project_root = Path::new(&project.path);
            let target = project_root.join(adapter.project_relative_skills_dir());
            if !path_guard::is_path_safe(project_root, &target) {
                bail!("Tool skills path escapes the selected project");
            }
            Ok(target)
        }
        _ => bail!("Unsupported target scope: {}", binding.scope),
    }
}

fn binding_project(
    store: &SkillStore,
    binding: &PackageBindingRecord,
) -> Result<Option<super::skill_store::ProjectRecord>> {
    binding
        .project_id
        .as_deref()
        .map(|id| {
            store
                .get_project_by_id(id)?
                .ok_or_else(|| anyhow!("Project not found: {id}"))
        })
        .transpose()
}

fn claude_scope(scope: &str) -> Result<&'static str> {
    match scope {
        "user" => Ok("user"),
        "project_shared" => Ok("project"),
        "project_local" => Ok("local"),
        _ => bail!("Unsupported Claude plugin scope: {scope}"),
    }
}

pub fn apply_binding(
    store: &SkillStore,
    binding_id: &str,
    approved_plan_hash: &str,
) -> Result<ApplyResult> {
    let plan = preview_binding(store, binding_id)?;
    if !plan.can_apply {
        bail!("Binding has no compatible installation plan");
    }
    if plan.plan_hash != approved_plan_hash {
        bail!("Installation plan changed; review and approve the new plan");
    }
    let mut binding = store
        .get_package_binding_by_id(binding_id)?
        .ok_or_else(|| anyhow!("Binding not found: {binding_id}"))?;
    let package = store
        .get_package_by_id(&binding.package_id)?
        .ok_or_else(|| anyhow!("Package not found: {}", binding.package_id))?;

    let result = match plan.surface_kind.as_deref() {
        Some("portable_skills") => apply_portable_operations(store, &binding, &package, &plan),
        Some("host_bundle") => apply_codex_host_bundle(store, &binding, &package, &plan),
        Some("native_plugin") => apply_command_operations(store, &binding, &package, &plan),
        Some("setup_script") => apply_command_operations(store, &binding, &package, &plan),
        _ => bail!("Unsupported surface"),
    };

    match result {
        Ok((target_ref, output)) => {
            binding.state = if plan.compatibility == "partial" {
                "partial".to_string()
            } else {
                "installed".to_string()
            };
            binding.compatibility = plan.compatibility.clone();
            binding.target_ref = Some(target_ref);
            binding.applied_revision = Some(package.resolved_revision.clone());
            binding.approved_plan_hash = Some(plan.plan_hash.clone());
            binding.last_error = None;
            binding.updated_at = now_ms();
            store.upsert_package_binding(&binding)?;
            if binding.scope == "project_shared" {
                write_project_manifest_binding(store, &binding)?;
            }
            Ok(ApplyResult {
                binding,
                plan,
                output,
            })
        }
        Err(error) => {
            binding.state = "failed".to_string();
            binding.last_error = Some(error.to_string());
            binding.approved_plan_hash = None;
            binding.updated_at = now_ms();
            store.upsert_package_binding(&binding)?;
            Err(error)
        }
    }
}

fn apply_portable_operations(
    store: &SkillStore,
    binding: &PackageBindingRecord,
    package: &PackageRecord,
    plan: &BindingPlan,
) -> Result<(String, Option<String>)> {
    let surface = store
        .get_package_surfaces(&package.id)?
        .into_iter()
        .find(|surface| Some(surface.id.as_str()) == plan.surface_id.as_deref())
        .ok_or_else(|| anyhow!("Resolved surface disappeared"))?;
    let coverage: Vec<SurfaceCoverage> = serde_json::from_str(&surface.coverage_json)?;
    let selected: Vec<_> = coverage
        .into_iter()
        .filter(|item| item.kind == "skill" && plan.covered_components.contains(&item.name))
        .collect();
    let adapter = tool_adapters::find_adapter_with_store(store, &binding.tool)
        .ok_or_else(|| anyhow!("Unknown tool: {}", binding.tool))?;
    let target_root = binding_target_root(store, binding, &adapter)?;
    fs::create_dir_all(&target_root)?;

    let package_root = Path::new(&package.cache_path);
    let mut created: Vec<PathBuf> = Vec::new();
    let mut targets = Vec::new();
    let mut seen_names = BTreeSet::new();
    for item in selected {
        if !seen_names.insert(item.name.clone()) {
            continue;
        }
        let source = package_root.join(&item.relative_path);
        let target_name = skill_metadata::sanitize_skill_name(&item.name)
            .ok_or_else(|| anyhow!("Unsafe skill name: {}", item.name))?;
        let target = target_root.join(target_name);
        let already_managed = managed_target_matches(&source, &target)?;
        if fs::symlink_metadata(&target).is_ok() && !already_managed {
            bail!(
                "Target already exists and is not managed by this binding: {}",
                target.display()
            );
        }
        let applied_mode = match sync_engine::sync_skill(
            &source,
            &target,
            sync_engine::SyncMode::Symlink,
            sync_engine::ReplacePolicy::NoClobber,
        ) {
            Ok(mode) => mode,
            Err(error) => {
                for created_target in &created {
                    let _ = sync_engine::remove_target(created_target);
                }
                return Err(error);
            }
        };
        if !matches!(applied_mode, sync_engine::SyncMode::Symlink) {
            let _ = sync_engine::remove_target(&target);
            for created_target in &created {
                let _ = sync_engine::remove_target(created_target);
            }
            bail!("Managed package deployment requires symlink or junction support");
        }
        if !already_managed {
            created.push(target.clone());
        }
        targets.push(target);
    }
    if matches!(binding.scope.as_str(), "project_shared" | "project_local") {
        if let Some(project) = binding_project(store, binding)? {
            exclude_generated_targets(Path::new(&project.path), &targets)?;
        }
    }
    Ok((serde_json::to_string(&targets)?, None))
}

fn apply_codex_host_bundle(
    store: &SkillStore,
    binding: &PackageBindingRecord,
    package: &PackageRecord,
    plan: &BindingPlan,
) -> Result<(String, Option<String>)> {
    // ponytail: bundle updates are remove+apply until an atomic multi-file upgrader is needed.
    if binding.target_ref.is_some() {
        bail!("Remove the installed Codex hook bundle before applying an update");
    }
    let (skill_targets_json, _) = apply_portable_operations(store, binding, package, plan)?;
    let skill_targets: Vec<PathBuf> = serde_json::from_str(&skill_targets_json)?;
    match install_codex_hooks(store, binding, package, skill_targets.clone()) {
        Ok(target_ref) => Ok((serde_json::to_string(&target_ref)?, None)),
        Err(error) => {
            let rollback_binding = PackageBindingRecord {
                target_ref: Some(skill_targets_json),
                ..binding.clone()
            };
            let _ = remove_portable_targets(&rollback_binding, package);
            Err(error)
        }
    }
}

fn install_codex_hooks(
    store: &SkillStore,
    binding: &PackageBindingRecord,
    package: &PackageRecord,
    skill_targets: Vec<PathBuf>,
) -> Result<CodexBundleTargetRef> {
    let adapter = tool_adapters::find_adapter_with_store(store, &binding.tool)
        .ok_or_else(|| anyhow!("Unknown tool: {}", binding.tool))?;
    let config_root = adapter
        .skills_dir()
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("Codex skills path has no config directory"))?;
    let package_root = Path::new(&package.cache_path);
    let source_hooks_dir = package_root.join(".codex/hooks");
    let source_manifest = package_root.join(".codex/hooks.json");
    if !path_guard::is_path_safe(package_root, &source_hooks_dir)
        || !path_guard::is_path_safe(package_root, &source_manifest)
    {
        bail!("Codex hook bundle escapes the package cache");
    }

    let source_json: serde_json::Value = serde_json::from_slice(&fs::read(&source_manifest)?)
        .context("Invalid package .codex/hooks.json")?;
    let source_groups = parse_hook_groups(&source_json)?;
    let hooks_file = config_root.join("hooks.json");
    let mut target_json = if hooks_file.is_file() {
        serde_json::from_slice(&fs::read(&hooks_file)?)
            .context("Existing Codex hooks.json is invalid; repair it before installing")?
    } else {
        serde_json::json!({})
    };
    let added_hooks = merge_hook_groups(&mut target_json, &source_groups)?;

    let target_hooks_dir = config_root.join("hooks");
    let assets = collect_hook_assets(&source_hooks_dir, &target_hooks_dir)?;
    let mut copied: Vec<ManagedHookFile> = Vec::new();
    for (source, target, hash) in assets {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Err(error) = fs::copy(&source, &target) {
            for file in &copied {
                let _ = fs::remove_file(&file.path);
            }
            return Err(error)
                .with_context(|| format!("Failed to copy Codex hook asset {}", target.display()));
        }
        copied.push(ManagedHookFile { path: target, hash });
    }
    if let Err(error) = atomic_write_json(&hooks_file, &target_json) {
        for file in &copied {
            let _ = fs::remove_file(&file.path);
        }
        return Err(error);
    }
    Ok(CodexBundleTargetRef {
        skill_targets,
        hook_files: copied,
        hooks_file,
        added_hooks,
    })
}

fn collect_hook_assets(
    source_root: &Path,
    target_root: &Path,
) -> Result<Vec<(PathBuf, PathBuf, String)>> {
    let mut assets = Vec::new();
    for entry in WalkDir::new(source_root).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_symlink() {
            bail!(
                "Codex hook assets must not contain symlinks: {}",
                entry.path().display()
            );
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(source_root)?;
        let target = target_root.join(relative);
        if fs::symlink_metadata(&target).is_ok() {
            bail!(
                "Codex hook asset already exists and is not managed by this binding: {}",
                target.display()
            );
        }
        assets.push((entry.path().to_path_buf(), target, hash_file(entry.path())?));
    }
    if assets.is_empty() {
        bail!("Codex hook bundle contains no hook assets");
    }
    Ok(assets)
}

fn parse_hook_groups(
    value: &serde_json::Value,
) -> Result<BTreeMap<String, Vec<serde_json::Value>>> {
    let hooks = value
        .get("hooks")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| anyhow!("Codex hooks.json must contain a hooks object"))?;
    hooks
        .iter()
        .map(|(event, groups)| {
            let groups = groups
                .as_array()
                .cloned()
                .ok_or_else(|| anyhow!("Codex hook event {event} must be an array"))?;
            Ok((event.clone(), groups))
        })
        .collect()
}

fn merge_hook_groups(
    target: &mut serde_json::Value,
    additions: &BTreeMap<String, Vec<serde_json::Value>>,
) -> Result<BTreeMap<String, Vec<serde_json::Value>>> {
    const LEGACY_EVENTS: &[&str] = &[
        "SessionStart",
        "SessionEnd",
        "SubagentStart",
        "SubagentStop",
        "PreToolUse",
        "PermissionRequest",
        "PostToolUse",
        "PreCompact",
        "PostCompact",
        "UserPromptSubmit",
        "Stop",
    ];
    let root = target
        .as_object_mut()
        .ok_or_else(|| anyhow!("Existing Codex hooks.json must be an object"))?;
    let mut legacy_groups = BTreeMap::new();
    for event in LEGACY_EVENTS {
        let Some(value) = root.remove(*event) else {
            continue;
        };
        let groups = value
            .as_array()
            .cloned()
            .ok_or_else(|| anyhow!("Existing Codex hook event {event} must be an array"))?;
        legacy_groups.insert((*event).to_string(), groups);
    }
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow!("Existing Codex hooks field must be an object"))?;
    for (event, groups) in legacy_groups {
        let target_groups = hooks
            .entry(event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| anyhow!("Existing Codex hook event must be an array"))?;
        for group in groups {
            if !target_groups.contains(&group) {
                target_groups.push(group);
            }
        }
    }
    let mut added = BTreeMap::new();
    for (event, groups) in additions {
        let target_groups = hooks
            .entry(event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| anyhow!("Existing Codex hook event {event} must be an array"))?;
        for group in groups {
            if !target_groups.contains(group) {
                target_groups.push(group.clone());
                added
                    .entry(event.clone())
                    .or_insert_with(Vec::new)
                    .push(group.clone());
            }
        }
    }
    Ok(added)
}

fn atomic_write_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            bail!(
                "Refusing to replace non-file JSON target: {}",
                path.display()
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
        _ => {}
    }
    let temp = path.with_extension(format!("skillapse-{}.tmp", uuid::Uuid::new_v4()));
    let backup = path.with_extension(format!("skillapse-{}.backup", uuid::Uuid::new_v4()));
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    fs::write(&temp, bytes)?;
    if path.exists() {
        fs::rename(path, &backup)?;
    }
    if let Err(error) = fs::rename(&temp, path) {
        if backup.exists() {
            let _ = fs::rename(&backup, path);
        }
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    if backup.exists() {
        if let Err(error) = fs::remove_file(&backup) {
            log::warn!("Failed to remove JSON backup {}: {error}", backup.display());
        }
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String> {
    Ok(hex::encode(Sha256::digest(fs::read(path)?)))
}

fn managed_target_matches(source: &Path, target: &Path) -> Result<bool> {
    let Ok(metadata) = fs::symlink_metadata(target) else {
        return Ok(false);
    };
    if metadata.file_type().is_symlink() {
        let current = fs::read_link(target)?;
        let resolved = if current.is_absolute() {
            current
        } else {
            target
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(current)
        };
        if resolved.canonicalize().ok() == source.canonicalize().ok() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn apply_command_operations(
    store: &SkillStore,
    binding: &PackageBindingRecord,
    package: &PackageRecord,
    plan: &BindingPlan,
) -> Result<(String, Option<String>)> {
    let project = binding_project(store, binding)?;
    let cwd = project
        .as_ref()
        .map(|value| PathBuf::from(&value.path))
        .unwrap_or_else(|| PathBuf::from(&package.cache_path));
    let mut combined = String::new();
    for operation in &plan.operations {
        let command = operation
            .command
            .as_ref()
            .ok_or_else(|| anyhow!("Missing command for {}", operation.kind))?;
        let output = match run_checked_command(command, &cwd, Path::new(&package.cache_path)) {
            Ok(output) => output,
            Err(error)
                if operation.kind == "add_marketplace"
                    && ["already exists", "already registered", "already configured"]
                        .iter()
                        .any(|needle| error.to_string().to_ascii_lowercase().contains(needle)) =>
            {
                String::new()
            }
            Err(error) => return Err(error),
        };
        if !output.trim().is_empty() {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str(&output);
        }
    }
    let target_ref = serde_json::to_string(&plan.operations)?;
    Ok((target_ref, (!combined.is_empty()).then_some(combined)))
}

fn run_checked_command(command: &[String], cwd: &Path, package_root: &Path) -> Result<String> {
    let executable = command.first().ok_or_else(|| anyhow!("Empty command"))?;
    let executable_path = Path::new(executable);
    let program = if executable_path.components().count() > 1 {
        let resolved = package_root.join(executable_path);
        if !path_guard::is_path_safe(package_root, &resolved) {
            bail!("Setup executable escapes package cache");
        }
        resolved
    } else {
        executable_path.to_path_buf()
    };
    let output = Command::new(&program)
        .args(&command[1..])
        .current_dir(cwd)
        .output()
        .with_context(|| format!("Failed to execute {}", program.display()))?;
    let stdout = truncate_output(String::from_utf8_lossy(&output.stdout).to_string());
    let stderr = truncate_output(String::from_utf8_lossy(&output.stderr).to_string());
    if !output.status.success() {
        let detail = if stderr.trim().is_empty() {
            stdout
        } else {
            stderr
        };
        bail!("Command failed ({}): {}", output.status, detail.trim());
    }
    Ok(if stderr.trim().is_empty() {
        stdout
    } else if stdout.trim().is_empty() {
        stderr
    } else {
        format!("{stdout}\n{stderr}")
    })
}

fn truncate_output(value: String) -> String {
    let mut value = super::log_sanitize::sanitize(&value);
    if value.len() > MAX_SETUP_OUTPUT_BYTES {
        let mut end = MAX_SETUP_OUTPUT_BYTES;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
        value.push_str("\n… output truncated");
    }
    value
}

pub fn remove_binding(store: &SkillStore, binding_id: &str, forget_setup: bool) -> Result<()> {
    let binding = store
        .get_package_binding_by_id(binding_id)?
        .ok_or_else(|| anyhow!("Binding not found: {binding_id}"))?;
    let package = store
        .get_package_by_id(&binding.package_id)?
        .ok_or_else(|| anyhow!("Package not found: {}", binding.package_id))?;
    let surfaces = store.get_package_surfaces(&package.id)?;
    let surface = binding
        .resolved_surface_id
        .as_deref()
        .and_then(|id| surfaces.iter().find(|surface| surface.id == id).cloned())
        .or_else(|| select_surface(&binding, &surfaces));

    if binding.target_ref.is_some() {
        match surface.as_ref().map(|value| value.kind.as_str()) {
            Some("portable_skills") => remove_portable_targets(&binding, &package)?,
            Some("host_bundle") => remove_codex_host_bundle(store, &binding, &package)?,
            Some("native_plugin") => {
                remove_native_plugin(store, &binding, &package, surface.as_ref().unwrap())?
            }
            Some("setup_script") if forget_setup => {}
            Some("setup_script") => remove_setup_package(&binding, &package)?,
            _ => bail!("Installed surface metadata is missing; refusing an unsafe removal"),
        }
    }
    if binding.scope == "project_shared" {
        remove_project_manifest_binding(store, &binding)?;
    }
    store.delete_package_binding(binding_id)
}

fn remove_codex_host_bundle(
    store: &SkillStore,
    binding: &PackageBindingRecord,
    package: &PackageRecord,
) -> Result<()> {
    let target_ref: CodexBundleTargetRef = serde_json::from_str(
        binding
            .target_ref
            .as_deref()
            .ok_or_else(|| anyhow!("Codex bundle target metadata is missing"))?,
    )?;
    let adapter = tool_adapters::find_adapter_with_store(store, &binding.tool)
        .ok_or_else(|| anyhow!("Unknown tool: {}", binding.tool))?;
    let config_root = adapter
        .skills_dir()
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("Codex skills path has no config directory"))?;
    let hook_root = config_root.join("hooks");
    if target_ref.hooks_file != config_root.join("hooks.json") {
        bail!("Refusing to edit Codex hooks outside the bound config directory");
    }
    for file in &target_ref.hook_files {
        if !path_guard::is_path_safe(&hook_root, &file.path) {
            bail!("Refusing to remove a hook asset outside the bound Codex directory");
        }
        let metadata = fs::symlink_metadata(&file.path).with_context(|| {
            format!(
                "Managed Codex hook asset is missing: {}",
                file.path.display()
            )
        })?;
        if !metadata.file_type().is_file() || hash_file(&file.path)? != file.hash {
            bail!(
                "Managed Codex hook asset was modified; refusing to remove it: {}",
                file.path.display()
            );
        }
    }

    if target_ref.hooks_file.is_file() {
        let mut value: serde_json::Value = serde_json::from_slice(&fs::read(
            &target_ref.hooks_file,
        )?)
        .context("Existing Codex hooks.json is invalid; repair it before removing the binding")?;
        remove_hook_groups(&mut value, &target_ref.added_hooks)?;
        atomic_write_json(&target_ref.hooks_file, &value)?;
    }
    for file in &target_ref.hook_files {
        fs::remove_file(&file.path)?;
    }
    let portable_binding = PackageBindingRecord {
        target_ref: Some(serde_json::to_string(&target_ref.skill_targets)?),
        ..binding.clone()
    };
    remove_portable_targets(&portable_binding, package)
}

fn remove_hook_groups(
    target: &mut serde_json::Value,
    removals: &BTreeMap<String, Vec<serde_json::Value>>,
) -> Result<()> {
    let Some(hooks) = target
        .as_object_mut()
        .and_then(|root| root.get_mut("hooks"))
        .and_then(serde_json::Value::as_object_mut)
    else {
        return Ok(());
    };
    for (event, groups) in removals {
        let Some(target_groups) = hooks
            .get_mut(event)
            .and_then(serde_json::Value::as_array_mut)
        else {
            continue;
        };
        for group in groups {
            if let Some(index) = target_groups.iter().position(|current| current == group) {
                target_groups.remove(index);
            }
        }
    }
    Ok(())
}

fn remove_portable_targets(binding: &PackageBindingRecord, package: &PackageRecord) -> Result<()> {
    let targets: Vec<PathBuf> = binding
        .target_ref
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_default();
    let package_root = Path::new(&package.cache_path).canonicalize().ok();
    for target in targets {
        let metadata = match fs::symlink_metadata(&target) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if !metadata.file_type().is_symlink() {
            bail!(
                "Refusing to remove non-symlink target: {}",
                target.display()
            );
        }
        let link = fs::read_link(&target)?;
        let resolved = if link.is_absolute() {
            link
        } else {
            target.parent().unwrap_or_else(|| Path::new(".")).join(link)
        };
        let points_into_package = match package_root.as_ref() {
            Some(root) => resolved
                .canonicalize()
                .map(|path| path.starts_with(root))
                .unwrap_or(false),
            None => false,
        };
        if !points_into_package {
            bail!(
                "Refusing to remove target not linked to package cache: {}",
                target.display()
            );
        }
        sync_engine::remove_target(&target)?;
    }
    Ok(())
}

fn remove_native_plugin(
    store: &SkillStore,
    binding: &PackageBindingRecord,
    package: &PackageRecord,
    surface: &PackageSurfaceRecord,
) -> Result<()> {
    let (_, marketplace_name) = native_marketplace(Path::new(&package.cache_path), &binding.tool)?;
    let marketplace_name =
        marketplace_name.ok_or_else(|| anyhow!("Marketplace metadata missing"))?;
    let selector = format!(
        "{}@{}",
        native_plugin_name(Path::new(&package.cache_path), surface)?,
        marketplace_name
    );
    let command = if binding.tool == "codex" {
        vec![
            "codex".to_string(),
            "plugin".to_string(),
            "remove".to_string(),
            selector,
            "--json".to_string(),
        ]
    } else {
        vec![
            "claude".to_string(),
            "plugin".to_string(),
            "uninstall".to_string(),
            "--scope".to_string(),
            claude_scope(&binding.scope)?.to_string(),
            selector,
        ]
    };
    let cwd = binding_project(store, binding)?
        .map(|project| PathBuf::from(project.path))
        .unwrap_or_else(|| PathBuf::from(&package.cache_path));
    run_remove_command(&command, &cwd, Path::new(&package.cache_path))?;
    let marketplace_command = if binding.tool == "codex" {
        vec![
            "codex".to_string(),
            "plugin".to_string(),
            "marketplace".to_string(),
            "remove".to_string(),
            marketplace_name,
            "--json".to_string(),
        ]
    } else {
        vec![
            "claude".to_string(),
            "plugin".to_string(),
            "marketplace".to_string(),
            "remove".to_string(),
            "--scope".to_string(),
            claude_scope(&binding.scope)?.to_string(),
            marketplace_name,
        ]
    };
    run_remove_command(&marketplace_command, &cwd, Path::new(&package.cache_path))?;
    Ok(())
}

fn run_remove_command(command: &[String], cwd: &Path, package_root: &Path) -> Result<()> {
    match run_checked_command(command, cwd, package_root) {
        Ok(_) => Ok(()),
        Err(error) => {
            let detail = error.to_string().to_ascii_lowercase();
            if [
                "not installed",
                "not found",
                "does not exist",
                "unknown marketplace",
            ]
            .iter()
            .any(|needle| detail.contains(needle))
            {
                Ok(())
            } else {
                Err(error)
            }
        }
    }
}

fn remove_setup_package(binding: &PackageBindingRecord, package: &PackageRecord) -> Result<()> {
    if binding.tool == "codex" || binding.tool == "claude_code" {
        let uninstall = Path::new(&package.cache_path).join("bin/gstack-uninstall");
        if uninstall.is_file() {
            bail!(
                "Setup removal is package-wide, not target-specific; review and run {} manually",
                uninstall.display()
            );
        }
    }
    bail!("This setup-based package has no safe automatic uninstall command")
}

fn native_marketplace(
    package_root: &Path,
    tool: &str,
) -> Result<(Option<PathBuf>, Option<String>)> {
    let relative = match tool {
        "codex" => ".agents/plugins/marketplace.json",
        "claude_code" => ".claude-plugin/marketplace.json",
        _ => return Ok((None, None)),
    };
    let manifest = package_root.join(relative);
    if !manifest.is_file() {
        return Ok((None, None));
    }
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&manifest)?)?;
    let name = value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    Ok((Some(package_root.to_path_buf()), name))
}

fn native_plugin_name(package_root: &Path, surface: &PackageSurfaceRecord) -> Result<String> {
    let manifest_path = surface
        .manifest_path
        .as_deref()
        .ok_or_else(|| anyhow!("Native plugin manifest is missing"))?;
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(package_root.join(manifest_path))?)?;
    value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("Plugin manifest has no name"))
}

fn hash_plan(plan: &BindingPlan) -> Result<String> {
    #[derive(Serialize)]
    struct HashablePlan<'a> {
        binding_id: &'a str,
        package_revision: &'a str,
        tool: &'a str,
        scope: &'a str,
        compatibility: &'a str,
        surface_id: &'a Option<String>,
        covered_components: &'a [String],
        missing_components: &'a [String],
        risk_items: &'a [String],
        operations: &'a [PlanOperation],
    }
    let bytes = serde_json::to_vec(&HashablePlan {
        binding_id: &plan.binding_id,
        package_revision: &plan.package_revision,
        tool: &plan.tool,
        scope: &plan.scope,
        compatibility: &plan.compatibility,
        surface_id: &plan.surface_id,
        covered_components: &plan.covered_components,
        missing_components: &plan.missing_components,
        risk_items: &plan.risk_items,
        operations: &plan.operations,
    })?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn write_project_manifest_binding(
    store: &SkillStore,
    binding: &PackageBindingRecord,
) -> Result<()> {
    let project = binding_project(store, binding)?
        .ok_or_else(|| anyhow!("Project is required for project-shared binding"))?;
    let package = store
        .get_package_by_id(&binding.package_id)?
        .ok_or_else(|| anyhow!("Package not found: {}", binding.package_id))?;
    let project_root = Path::new(&project.path);
    let path = project_root.join(PROJECT_MANIFEST_RELATIVE_PATH);
    let mut manifest = read_project_manifest(&path)?;
    manifest.packages.retain(|item| item.id != package.id);
    manifest.packages.push(ProjectManifestPackage {
        id: package.id.clone(),
        source: package.source_url.clone(),
        revision: package.resolved_revision.clone(),
    });
    manifest
        .bindings
        .retain(|item| !(item.package == package.id && item.tool == binding.tool));
    manifest.bindings.push(ProjectManifestBinding {
        package: package.id,
        tool: binding.tool.clone(),
        surface: binding.surface_policy.clone(),
        components: serde_json::from_str(&binding.requested_components_json)?,
    });
    write_project_manifest(project_root, &path, &manifest)
}

fn remove_project_manifest_binding(
    store: &SkillStore,
    binding: &PackageBindingRecord,
) -> Result<()> {
    let project = binding_project(store, binding)?
        .ok_or_else(|| anyhow!("Project is required for project-shared binding"))?;
    let project_root = Path::new(&project.path);
    let path = project_root.join(PROJECT_MANIFEST_RELATIVE_PATH);
    if !path.exists() {
        return Ok(());
    }
    let mut manifest = read_project_manifest(&path)?;
    manifest
        .bindings
        .retain(|item| !(item.package == binding.package_id && item.tool == binding.tool));
    let still_used = manifest
        .bindings
        .iter()
        .any(|item| item.package == binding.package_id);
    if !still_used {
        manifest
            .packages
            .retain(|item| item.id != binding.package_id);
    }
    write_project_manifest(project_root, &path, &manifest)
}

pub fn sync_project_manifest(store: &SkillStore, project_id: &str) -> Result<Vec<BindingPlan>> {
    let project = store
        .get_project_by_id(project_id)?
        .ok_or_else(|| anyhow!("Project not found: {project_id}"))?;
    let path = Path::new(&project.path).join(PROJECT_MANIFEST_RELATIVE_PATH);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let manifest = read_project_manifest(&path)?;
    let mut local_ids = BTreeMap::new();
    for declaration in &manifest.packages {
        let details = import_git_package_with_id(
            store,
            &declaration.source,
            Some(&declaration.revision),
            Some(&declaration.id),
        )?;
        local_ids.insert(declaration.id.clone(), details.package.id);
    }

    let mut plans = Vec::new();
    for manifest_binding in manifest.bindings {
        let package_id = local_ids
            .get(&manifest_binding.package)
            .ok_or_else(|| {
                anyhow!(
                    "Manifest binding references undeclared package: {}",
                    manifest_binding.package
                )
            })?
            .clone();
        validate_binding_input(
            store,
            &package_id,
            &manifest_binding.tool,
            "project_shared",
            Some(project_id),
            &manifest_binding.surface,
        )?;
        let now = now_ms();
        let mut binding = store
            .get_package_binding(
                &package_id,
                &manifest_binding.tool,
                "project_shared",
                Some(project_id),
            )?
            .unwrap_or_else(|| PackageBindingRecord {
                id: uuid::Uuid::new_v4().to_string(),
                package_id: package_id.clone(),
                tool: manifest_binding.tool.clone(),
                scope: "project_shared".to_string(),
                project_id: Some(project_id.to_string()),
                surface_policy: manifest_binding.surface.clone(),
                requested_components_json: "[]".to_string(),
                desired_enabled: true,
                resolved_surface_id: None,
                compatibility: "unsupported".to_string(),
                state: "not_applied".to_string(),
                target_ref: None,
                applied_revision: None,
                approved_plan_hash: None,
                last_error: None,
                created_at: now,
                updated_at: now,
            });
        binding.surface_policy = manifest_binding.surface;
        binding.requested_components_json = serde_json::to_string(&manifest_binding.components)?;
        binding.desired_enabled = true;
        binding.approved_plan_hash = None;
        binding.updated_at = now;
        store.upsert_package_binding(&binding)?;
        plans.push(preview_binding(store, &binding.id)?);
    }
    Ok(plans)
}

fn read_project_manifest(path: &Path) -> Result<ProjectManifest> {
    if !path.exists() {
        return Ok(ProjectManifest {
            version: 1,
            packages: Vec::new(),
            bindings: Vec::new(),
        });
    }
    let manifest: ProjectManifest = serde_json::from_slice(&fs::read(path)?)
        .with_context(|| format!("Invalid Skillapse manifest: {}", path.display()))?;
    if manifest.version != 1 {
        bail!(
            "Unsupported Skillapse project manifest version: {}",
            manifest.version
        );
    }
    Ok(manifest)
}

fn write_project_manifest(
    project_root: &Path,
    path: &Path,
    manifest: &ProjectManifest,
) -> Result<()> {
    if !path_guard::is_path_safe(project_root, path) {
        bail!("Refusing to write project manifest outside project root");
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("Invalid manifest path"))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".project.json.{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temp, serde_json::to_vec_pretty(manifest)?)?;
    fs::rename(&temp, path)?;
    Ok(())
}

fn exclude_generated_targets(project_root: &Path, targets: &[PathBuf]) -> Result<()> {
    let exclude = project_root.join(".git/info/exclude");
    if !exclude.parent().is_some_and(Path::is_dir) {
        return Ok(());
    }
    let mut current = fs::read_to_string(&exclude).unwrap_or_default();
    for target in targets {
        let Ok(relative) = target.strip_prefix(project_root) else {
            continue;
        };
        let entry = format!("/{}", relative.to_string_lossy().replace('\\', "/"));
        if !current.lines().any(|line| line.trim() == entry) {
            if !current.ends_with('\n') && !current.is_empty() {
                current.push('\n');
            }
            current.push_str(&entry);
            current.push('\n');
        }
    }
    fs::write(exclude, current)?;
    Ok(())
}

fn copy_package_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in WalkDir::new(source)
        .follow_links(false)
        .into_iter()
        .filter_entry(scan_entry)
    {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = destination.join(relative);
        if entry.file_type().is_symlink() {
            continue;
        }
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn scan_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    !matches!(
        entry.file_name().to_string_lossy().as_ref(),
        ".git" | "node_modules" | "target" | ".venv" | "venv" | "__pycache__"
    )
}

fn scan_package(root: &Path, package_id: &str) -> Result<ScannedInventory> {
    let mut components = Vec::new();
    let mut skill_coverage_by_root: BTreeMap<(String, String, i32), Vec<SurfaceCoverage>> =
        BTreeMap::new();
    let mut seen_components = HashSet::new();

    for entry in WalkDir::new(root)
        .max_depth(10)
        .follow_links(false)
        .into_iter()
        .filter_entry(scan_entry)
    {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let filename = entry.file_name().to_string_lossy();
        if filename != "SKILL.md" && filename != "skill.md" {
            continue;
        }
        let Some(dir) = entry.path().parent() else {
            continue;
        };
        let relative_dir = relative_string(root, dir)?;
        if !seen_components.insert(("skill".to_string(), relative_dir.clone())) {
            continue;
        }
        let metadata = skill_metadata::parse_skill_md(dir);
        let name = metadata.name.unwrap_or_else(|| {
            dir.file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| "skill".to_string())
        });
        let (tool, surface_root, priority) = classify_skill_surface(&relative_dir);
        components.push(PackageComponentRecord {
            id: stable_id(package_id, "skill", &relative_dir),
            package_id: package_id.to_string(),
            kind: "skill".to_string(),
            name: name.clone(),
            relative_path: relative_dir.clone(),
            host_hint: (tool != "*").then_some(tool.clone()),
            required: false,
        });
        skill_coverage_by_root
            .entry((tool, surface_root, priority))
            .or_default()
            .push(SurfaceCoverage {
                name,
                kind: "skill".to_string(),
                relative_path: relative_dir,
            });
    }

    scan_conventional_components(root, package_id, &mut components, &mut seen_components)?;

    let mut surfaces = Vec::new();
    for ((tool, surface_root, priority), mut coverage) in skill_coverage_by_root {
        coverage.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then(a.relative_path.cmp(&b.relative_path))
        });
        surfaces.push(PackageSurfaceRecord {
            id: stable_id(package_id, "portable", &format!("{tool}:{surface_root}")),
            package_id: package_id.to_string(),
            tool,
            kind: "portable_skills".to_string(),
            root_path: surface_root,
            manifest_path: None,
            priority,
            coverage_json: serde_json::to_string(&coverage)?,
            install_command_json: None,
        });
    }

    let mut manifest_kinds = Vec::new();
    add_native_surface(
        root,
        package_id,
        "claude_code",
        ".claude-plugin/plugin.json",
        &components,
        &mut surfaces,
        &mut manifest_kinds,
    )?;
    add_native_surface(
        root,
        package_id,
        "codex",
        ".codex-plugin/plugin.json",
        &components,
        &mut surfaces,
        &mut manifest_kinds,
    )?;
    add_codex_host_bundle_surface(root, package_id, &components, &mut surfaces)?;
    add_setup_surfaces(root, package_id, &components, &mut surfaces)?;

    let name = infer_package_name(root, &manifest_kinds)?.unwrap_or_else(|| "package".to_string());
    let manifest_kind = if manifest_kinds.is_empty() {
        if surfaces
            .iter()
            .any(|surface| surface.kind == "setup_script")
        {
            "setup_script".to_string()
        } else {
            "none".to_string()
        }
    } else {
        manifest_kinds.join("+")
    };
    Ok(ScannedInventory {
        name,
        manifest_kind,
        components,
        surfaces,
    })
}

fn add_codex_host_bundle_surface(
    root: &Path,
    package_id: &str,
    components: &[PackageComponentRecord],
    surfaces: &mut Vec<PackageSurfaceRecord>,
) -> Result<()> {
    let manifest = root.join(".codex/hooks.json");
    let hooks_dir = root.join(".codex/hooks");
    if !manifest.is_file() || !hooks_dir.is_dir() {
        return Ok(());
    }
    let mut coverage: Vec<SurfaceCoverage> = components
        .iter()
        .filter(|component| {
            component.relative_path.starts_with(".codex/skills/")
                || component.relative_path == ".codex/hooks.json"
        })
        .map(|component| SurfaceCoverage {
            name: component.name.clone(),
            kind: component.kind.clone(),
            relative_path: component.relative_path.clone(),
        })
        .collect();
    if !coverage.iter().any(|item| item.kind == "skill")
        || !coverage.iter().any(|item| item.kind == "hook")
    {
        return Ok(());
    }
    coverage.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));
    surfaces.push(PackageSurfaceRecord {
        id: stable_id(package_id, "bundle", "codex"),
        package_id: package_id.to_string(),
        tool: "codex".to_string(),
        kind: "host_bundle".to_string(),
        root_path: ".codex".to_string(),
        manifest_path: Some(".codex/hooks.json".to_string()),
        priority: 95,
        coverage_json: serde_json::to_string(&coverage)?,
        install_command_json: None,
    });
    Ok(())
}

fn scan_conventional_components(
    root: &Path,
    package_id: &str,
    components: &mut Vec<PackageComponentRecord>,
    seen: &mut HashSet<(String, String)>,
) -> Result<()> {
    let rules: &[(&str, &str, Option<&str>)] = &[
        ("hooks.json", "hook", None),
        ("hooks/hooks.json", "hook", None),
        (".codex/hooks.json", "hook", Some("codex")),
        (".mcp.json", "mcp", None),
    ];
    for (relative, kind, host_hint) in rules {
        let path = root.join(relative);
        if path.is_file() {
            push_component(root, package_id, &path, kind, *host_hint, components, seen)?;
        }
    }
    for (directory, kind, extension, host_hint) in [
        ("agents", "agent", None, None),
        ("commands", "command", Some("md"), None),
        ("rules", "rule", Some("md"), None),
        (".claude/agents", "agent", None, Some("claude_code")),
        (
            ".claude/commands",
            "command",
            Some("md"),
            Some("claude_code"),
        ),
        (".claude/rules", "rule", Some("md"), Some("claude_code")),
        (".codex/agents", "agent", None, Some("codex")),
        (".codex/rules", "rule", Some("md"), Some("codex")),
        (".cursor/rules", "rule", None, Some("cursor")),
    ] {
        let base = root.join(directory);
        if !base.is_dir() {
            continue;
        }
        for entry in WalkDir::new(&base)
            .max_depth(3)
            .follow_links(false)
            .into_iter()
        {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            if extension.is_some_and(|expected| {
                entry.path().extension().and_then(|value| value.to_str()) != Some(expected)
            }) {
                continue;
            }
            push_component(
                root,
                package_id,
                entry.path(),
                kind,
                host_hint,
                components,
                seen,
            )?;
        }
    }
    Ok(())
}

fn push_component(
    root: &Path,
    package_id: &str,
    path: &Path,
    kind: &str,
    host_hint: Option<&str>,
    components: &mut Vec<PackageComponentRecord>,
    seen: &mut HashSet<(String, String)>,
) -> Result<()> {
    let relative = relative_string(root, path)?;
    if !seen.insert((kind.to_string(), relative.clone())) {
        return Ok(());
    }
    let name = path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| kind.to_string());
    components.push(PackageComponentRecord {
        id: stable_id(package_id, kind, &relative),
        package_id: package_id.to_string(),
        kind: kind.to_string(),
        name,
        relative_path: relative,
        host_hint: host_hint.map(str::to_string),
        required: false,
    });
    Ok(())
}

fn classify_skill_surface(relative_dir: &str) -> (String, String, i32) {
    let mappings = [
        (".claude/skills/", "claude_code", ".claude/skills", 90),
        (".codex/skills/", "codex", ".codex/skills", 90),
        (".agents/skills/", "codex", ".agents/skills", 85),
        (".cursor/skills/", "cursor", ".cursor/skills", 90),
        (".omp/skills/", "omp_agent", ".omp/skills", 90),
        (".opencode/skills/", "opencode", ".opencode/skills", 90),
        (".factory/skills/", "droid", ".factory/skills", 90),
        (".kiro/skills/", "kiro", ".kiro/skills", 90),
    ];
    for (prefix, tool, root, priority) in mappings {
        if relative_dir.starts_with(prefix) || relative_dir == root {
            return (tool.to_string(), root.to_string(), priority);
        }
    }
    if relative_dir.starts_with("skills/") || relative_dir == "skills" {
        return ("*".to_string(), "skills".to_string(), 70);
    }
    ("*".to_string(), ".".to_string(), 40)
}

fn add_native_surface(
    root: &Path,
    package_id: &str,
    tool: &str,
    manifest_relative: &str,
    components: &[PackageComponentRecord],
    surfaces: &mut Vec<PackageSurfaceRecord>,
    manifest_kinds: &mut Vec<String>,
) -> Result<()> {
    let path = root.join(manifest_relative);
    if !path.is_file() {
        return Ok(());
    }
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)
        .with_context(|| format!("Invalid plugin manifest: {}", path.display()))?;
    if value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .is_none()
    {
        bail!("Plugin manifest has no name: {}", path.display());
    }
    let coverage: Vec<SurfaceCoverage> = components
        .iter()
        .filter(|component| {
            component
                .host_hint
                .as_deref()
                .map(|hint| hint == tool)
                .unwrap_or(true)
        })
        .map(|component| SurfaceCoverage {
            name: component.name.clone(),
            kind: component.kind.clone(),
            relative_path: component.relative_path.clone(),
        })
        .collect();
    surfaces.push(PackageSurfaceRecord {
        id: stable_id(package_id, "native", tool),
        package_id: package_id.to_string(),
        tool: tool.to_string(),
        kind: "native_plugin".to_string(),
        root_path: ".".to_string(),
        manifest_path: Some(manifest_relative.to_string()),
        priority: 100,
        coverage_json: serde_json::to_string(&coverage)?,
        install_command_json: None,
    });
    manifest_kinds.push(format!("{}_plugin", tool));
    Ok(())
}

fn add_setup_surfaces(
    root: &Path,
    package_id: &str,
    components: &[PackageComponentRecord],
    surfaces: &mut Vec<PackageSurfaceRecord>,
) -> Result<()> {
    let setup = root.join("setup");
    if !setup.is_file() {
        return Ok(());
    }
    let content = fs::read_to_string(&setup).unwrap_or_default();
    if !content.contains("--host") {
        return Ok(());
    }
    let coverage: Vec<SurfaceCoverage> = components
        .iter()
        .filter(|component| component.kind == "skill")
        .map(|component| SurfaceCoverage {
            name: component.name.clone(),
            kind: component.kind.clone(),
            relative_path: component.relative_path.clone(),
        })
        .collect();
    for (tool, host) in [
        ("claude_code", "claude"),
        ("codex", "codex"),
        ("opencode", "opencode"),
        ("kiro", "kiro"),
        ("droid", "factory"),
    ] {
        if !content.contains(host) {
            continue;
        }
        let mut command = vec![
            "./setup".to_string(),
            "--host".to_string(),
            host.to_string(),
        ];
        if content.contains("--quiet") {
            command.push("--quiet".to_string());
        }
        if tool == "claude_code" && content.contains("--no-plan-tune-hooks") {
            command.push("--no-plan-tune-hooks".to_string());
        }
        surfaces.push(PackageSurfaceRecord {
            id: stable_id(package_id, "setup", tool),
            package_id: package_id.to_string(),
            tool: tool.to_string(),
            kind: "setup_script".to_string(),
            root_path: ".".to_string(),
            manifest_path: Some("setup".to_string()),
            priority: 60,
            coverage_json: serde_json::to_string(&coverage)?,
            install_command_json: Some(serde_json::to_string(&command)?),
        });
    }
    Ok(())
}

fn infer_package_name(root: &Path, manifest_kinds: &[String]) -> Result<Option<String>> {
    let manifest_paths = if manifest_kinds.iter().any(|kind| kind == "codex_plugin") {
        vec![
            ".codex-plugin/plugin.json",
            ".claude-plugin/plugin.json",
            "package.json",
        ]
    } else {
        vec![
            ".claude-plugin/plugin.json",
            ".codex-plugin/plugin.json",
            "package.json",
        ]
    };
    for relative in manifest_paths {
        let path = root.join(relative);
        if !path.is_file() {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&fs::read(&path)?) else {
            continue;
        };
        if let Some(name) = value.get("name").and_then(serde_json::Value::as_str) {
            return Ok(Some(name.to_string()));
        }
    }
    Ok(None)
}

fn package_name_from_source(source_url: &str) -> String {
    source_url
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("package")
        .trim_end_matches(".git")
        .to_string()
}

fn relative_string(root: &Path, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(root)?
        .to_string_lossy()
        .replace('\\', "/"))
}

fn stable_id(package_id: &str, kind: &str, key: &str) -> String {
    let digest = Sha256::digest(format!("{package_id}\0{kind}\0{key}").as_bytes());
    format!("{}-{}", kind, &hex::encode(digest)[..20])
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn scanner_prefers_host_specific_portable_surface() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join("review/SKILL.md"),
            "---\nname: review\n---\n",
        );
        write(
            &temp.path().join(".agents/skills/gstack-review/SKILL.md"),
            "---\nname: gstack-review\n---\n",
        );

        let inventory = scan_package(temp.path(), "p1").unwrap();
        let codex = inventory
            .surfaces
            .iter()
            .find(|surface| surface.tool == "codex")
            .unwrap();
        assert_eq!(codex.root_path, ".agents/skills");
        assert!(
            codex.priority
                > inventory
                    .surfaces
                    .iter()
                    .find(|s| s.tool == "*")
                    .unwrap()
                    .priority
        );
    }

    #[test]
    fn scanner_detects_native_plugins_and_setup_without_execution() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(".codex-plugin/plugin.json"),
            r#"{"name":"demo","skills":"./skills"}"#,
        );
        write(
            &temp.path().join("skills/demo/SKILL.md"),
            "---\nname: demo\n---\n",
        );
        write(
            &temp.path().join("setup"),
            "#!/bin/sh\n# supports --host codex claude\n",
        );
        write(
            &temp.path().join(".codex/hooks.json"),
            r#"{"hooks":{"SessionStart":[]}}"#,
        );
        let inventory = scan_package(temp.path(), "p1").unwrap();
        assert!(inventory
            .surfaces
            .iter()
            .any(|surface| { surface.tool == "codex" && surface.kind == "native_plugin" }));
        assert!(inventory
            .surfaces
            .iter()
            .any(|surface| { surface.tool == "claude_code" && surface.kind == "setup_script" }));
        assert!(inventory.components.iter().any(|component| {
            component.kind == "hook"
                && component.host_hint.as_deref() == Some("codex")
                && component.relative_path == ".codex/hooks.json"
        }));
    }

    #[test]
    fn codex_host_bundle_merges_and_removes_only_managed_hooks() {
        let temp = tempdir().unwrap();
        let package_root = temp.path().join("package");
        write(
            &package_root.join(".codex/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: demo\n---\n",
        );
        write(
            &package_root.join(".codex/hooks/start.sh"),
            "#!/bin/sh\necho demo\n",
        );
        write(
            &package_root.join(".codex/hooks.json"),
            r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"sh .codex/hooks/start.sh"}]}]}}"#,
        );

        let store = SkillStore::new(&temp.path().join("state.db")).unwrap();
        let codex_root = temp.path().join("home/.codex");
        store
            .set_setting(
                "custom_tool_paths",
                &serde_json::json!({ "codex": codex_root.join("skills") }).to_string(),
            )
            .unwrap();
        write(
            &codex_root.join("hooks.json"),
            r#"{"description":"keep","SessionStart":[{"hooks":[{"type":"command","command":"echo keep"}]}],"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"echo keep"}]}]}}"#,
        );

        let inventory = scan_package(&package_root, "package-1").unwrap();
        assert!(inventory
            .surfaces
            .iter()
            .any(|surface| { surface.tool == "codex" && surface.kind == "host_bundle" }));
        let package = PackageRecord {
            id: "package-1".into(),
            name: "Demo".into(),
            source_url: "https://example.com/demo.git".into(),
            requested_revision: Some("main".into()),
            resolved_revision: "abc123".into(),
            cache_path: package_root.to_string_lossy().to_string(),
            manifest_kind: "none".into(),
            status: "ready".into(),
            created_at: 1,
            updated_at: 1,
        };
        store
            .replace_package_inventory(&package, &inventory.components, &inventory.surfaces)
            .unwrap();

        let plan = create_binding(&store, &package.id, "codex", "user", None, "auto", &[]).unwrap();
        assert_eq!(plan.compatibility, "full");
        assert_eq!(plan.surface_kind.as_deref(), Some("host_bundle"));
        let applied = apply_binding(&store, &plan.binding_id, &plan.plan_hash).unwrap();
        assert_eq!(applied.binding.state, "installed");
        assert!(codex_root.join("skills/demo/SKILL.md").is_file());
        assert!(codex_root.join("hooks/start.sh").is_file());
        let merged: serde_json::Value =
            serde_json::from_slice(&fs::read(codex_root.join("hooks.json")).unwrap()).unwrap();
        assert_eq!(merged["description"], "keep");
        assert!(merged.get("SessionStart").is_none());
        assert_eq!(merged["hooks"]["SessionStart"].as_array().unwrap().len(), 2);

        remove_binding(&store, &plan.binding_id, false).unwrap();
        assert!(fs::symlink_metadata(codex_root.join("skills/demo")).is_err());
        assert!(!codex_root.join("hooks/start.sh").exists());
        let remaining: serde_json::Value =
            serde_json::from_slice(&fs::read(codex_root.join("hooks.json")).unwrap()).unwrap();
        assert_eq!(remaining["description"], "keep");
        assert!(remaining.get("SessionStart").is_none());
        assert_eq!(
            remaining["hooks"]["SessionStart"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            remaining["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            "echo keep"
        );
    }

    #[test]
    fn portable_project_binding_round_trips_without_orphans() {
        let temp = tempdir().unwrap();
        let project_root = temp.path().join("project");
        let package_root = temp.path().join("package");
        let skill_root = package_root.join("skills/demo");
        fs::create_dir_all(&project_root).unwrap();
        write(&skill_root.join("SKILL.md"), "---\nname: demo\n---\n");

        let store = SkillStore::new(&temp.path().join("state.db")).unwrap();
        store
            .insert_project(&crate::core::skill_store::ProjectRecord {
                id: "project-1".into(),
                name: "Project".into(),
                path: project_root.to_string_lossy().to_string(),
                workspace_type: "single".into(),
                linked_agent_key: None,
                linked_agent_name: None,
                disabled_path: None,
                sort_order: 0,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
        let package = PackageRecord {
            id: "package-1".into(),
            name: "Demo".into(),
            source_url: "https://example.com/demo.git".into(),
            requested_revision: Some("main".into()),
            resolved_revision: "abc123".into(),
            cache_path: package_root.to_string_lossy().to_string(),
            manifest_kind: "none".into(),
            status: "ready".into(),
            created_at: 1,
            updated_at: 1,
        };
        let component = PackageComponentRecord {
            id: "component-1".into(),
            package_id: package.id.clone(),
            kind: "skill".into(),
            name: "demo".into(),
            relative_path: "skills/demo".into(),
            host_hint: None,
            required: false,
        };
        let hook = PackageComponentRecord {
            id: "component-2".into(),
            package_id: package.id.clone(),
            kind: "hook".into(),
            name: "danger-hook".into(),
            relative_path: "hooks.json".into(),
            host_hint: None,
            required: false,
        };
        let surface = PackageSurfaceRecord {
            id: "surface-1".into(),
            package_id: package.id.clone(),
            tool: "*".into(),
            kind: "portable_skills".into(),
            root_path: "skills".into(),
            manifest_path: None,
            priority: 70,
            coverage_json: serde_json::to_string(&[SurfaceCoverage {
                name: "demo".into(),
                kind: "skill".into(),
                relative_path: "skills/demo".into(),
            }])
            .unwrap(),
            install_command_json: None,
        };
        store
            .replace_package_inventory(&package, &[component, hook], &[surface])
            .unwrap();

        let plan = create_binding(
            &store,
            &package.id,
            "codex",
            "project_local",
            Some("project-1"),
            "portable",
            &[],
        )
        .unwrap();
        assert_eq!(plan.compatibility, "partial");
        assert_eq!(plan.missing_components, ["danger-hook"]);
        let selected_plan = create_binding(
            &store,
            &package.id,
            "codex",
            "project_local",
            Some("project-1"),
            "portable",
            &["demo".into()],
        )
        .unwrap();
        assert_eq!(selected_plan.binding_id, plan.binding_id);
        assert_eq!(selected_plan.compatibility, "full");
        let duplicate = create_binding(
            &store,
            &package.id,
            "codex",
            "project_local",
            Some("project-1"),
            "portable",
            &["demo".into()],
        )
        .unwrap();
        assert_eq!(duplicate.binding_id, selected_plan.binding_id);

        let target = project_root.join(".codex/skills/demo");
        write(&target.join("keep.txt"), "user content");
        let error =
            apply_binding(&store, &selected_plan.binding_id, &selected_plan.plan_hash).unwrap_err();
        assert!(error.to_string().contains("not managed by this binding"));
        assert_eq!(
            fs::read_to_string(target.join("keep.txt")).unwrap(),
            "user content"
        );
        fs::remove_dir_all(&target).unwrap();

        let retry_plan = preview_binding(&store, &selected_plan.binding_id).unwrap();
        let applied = apply_binding(&store, &retry_plan.binding_id, &retry_plan.plan_hash).unwrap();
        assert_eq!(applied.binding.state, "installed");
        assert!(managed_target_matches(&skill_root, &target).unwrap());

        let reapply_plan = preview_binding(&store, &selected_plan.binding_id).unwrap();
        apply_binding(&store, &reapply_plan.binding_id, &reapply_plan.plan_hash).unwrap();
        assert!(managed_target_matches(&skill_root, &target).unwrap());
        assert!(create_binding(
            &store,
            &package.id,
            "codex",
            "project_local",
            Some("project-1"),
            "portable",
            &["different".into()],
        )
        .is_err());

        sync_engine::remove_target(&target).unwrap();
        write(&target.join("replacement.txt"), "user replacement");
        let error = remove_binding(&store, &selected_plan.binding_id, false).unwrap_err();
        assert!(error
            .to_string()
            .contains("Refusing to remove non-symlink target"));
        assert_eq!(
            fs::read_to_string(target.join("replacement.txt")).unwrap(),
            "user replacement"
        );
        fs::remove_dir_all(&target).unwrap();

        let restore_plan = preview_binding(&store, &selected_plan.binding_id).unwrap();
        apply_binding(&store, &restore_plan.binding_id, &restore_plan.plan_hash).unwrap();
        remove_binding(&store, &selected_plan.binding_id, false).unwrap();
        assert!(fs::symlink_metadata(&target).is_err());
        assert!(store
            .get_package_binding_by_id(&selected_plan.binding_id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn project_manifest_round_trip_is_atomic_and_versioned() {
        let temp = tempdir().unwrap();
        let path = temp.path().join(PROJECT_MANIFEST_RELATIVE_PATH);
        let manifest = ProjectManifest {
            version: 1,
            packages: vec![ProjectManifestPackage {
                id: "p1".into(),
                source: "https://example.com/p1.git".into(),
                revision: "main".into(),
            }],
            bindings: vec![],
        };
        write_project_manifest(temp.path(), &path, &manifest).unwrap();
        let loaded = read_project_manifest(&path).unwrap();
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.packages[0].id, "p1");
    }

    #[test]
    fn plan_hash_changes_when_command_changes() {
        let mut plan = BindingPlan {
            binding_id: "b".into(),
            package_name: "p".into(),
            package_revision: "abc123".into(),
            tool: "codex".into(),
            scope: "user".into(),
            compatibility: "full".into(),
            surface_id: Some("s".into()),
            surface_kind: Some("setup_script".into()),
            covered_components: vec!["x".into()],
            missing_components: vec![],
            risk_items: vec!["exec".into()],
            operations: vec![PlanOperation {
                kind: "run_setup".into(),
                description: "setup".into(),
                target: "/tmp/p".into(),
                command: Some(vec!["./setup".into(), "--host".into(), "codex".into()]),
            }],
            plan_hash: String::new(),
            can_apply: true,
        };
        let first = hash_plan(&plan).unwrap();
        plan.package_revision = "def456".into();
        assert_ne!(first, hash_plan(&plan).unwrap());
        plan.package_revision = "abc123".into();
        plan.operations[0]
            .command
            .as_mut()
            .unwrap()
            .push("--new".into());
        assert_ne!(first, hash_plan(&plan).unwrap());
    }
}
