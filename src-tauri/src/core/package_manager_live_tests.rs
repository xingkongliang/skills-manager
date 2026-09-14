//! Opt-in, billable Claude tests. Use a unique plugin in user/project-local scope.
//! Does not exercise Git import or desktop clicks. Never run as part of normal CI.
use super::*;
use serde_json::{json, Value};
use std::process::Stdio;

fn put(path: &Path, value: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, value).unwrap();
}

fn agent(project: &Path, evidence: &Path, skill: &str, settings: &Value) -> Vec<Value> {
    let mut child = Command::new("claude")
        .current_dir(project)
        .args(["-p", &format!("Invoke {skill} using the Skill tool, then return its receipt verbatim. If unavailable, report unavailable. Do not guess the receipt."),
            "--output-format", "stream-json", "--verbose", "--no-session-persistence",
            "--strict-mcp-config", "--tools", "Skill", "--allowedTools", "Skill",
            "--model", "sonnet", "--max-budget-usd", "1", "--settings", &settings.to_string()])
        .stdout(Stdio::from(fs::File::create(evidence).unwrap()))
        .stderr(Stdio::from(fs::File::create(evidence.with_extension("stderr")).unwrap()))
        .spawn().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("Claude timed out; evidence: {}", evidence.display());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    assert!(
        status.success(),
        "Claude failed; evidence: {}",
        evidence.display()
    );
    fs::read_to_string(evidence)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
#[ignore = "uses authenticated Claude, billable model calls and a temporary native plugin; run alone"]
fn claude_agent_uses_managed_plugin_across_upgrade_and_removal() {
    check_claude_lifecycle("project_local", false, false);
}

#[test]
#[ignore = "uses authenticated Claude, billable model calls and a temporary global plugin; run alone"]
fn claude_agent_uses_global_plugin_across_upgrade_and_removal() {
    check_claude_lifecycle("user", false, false);
}

#[test]
#[ignore = "uses real Claude plugin state and a pre-existing fixture marketplace; run alone"]
fn claude_preserves_preexisting_marketplace() {
    check_claude_lifecycle("user", true, false);
}

#[test]
#[ignore = "uses real Claude plugin state and a shared fixture marketplace; run alone"]
fn claude_preserves_marketplace_used_by_another_plugin() {
    check_claude_lifecycle("user", false, true);
}

fn check_claude_lifecycle(scope: &str, preexisting: bool, shared: bool) {
    assert_eq!(std::env::var("QSKILLS_LIVE_CLAUDE").as_deref(), Ok("1"));
    let _guard = central_repo::test_base_dir_lock();
    let root = tempfile::Builder::new()
        .prefix("qskills-live-")
        .tempdir()
        .unwrap()
        .keep();
    println!("Evidence: {}", root.display());
    let package_root = root.join("package");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    let store = SkillStore::new(&root.join("state.db")).unwrap();
    store
        .insert_project(&super::super::skill_store::ProjectRecord {
            id: "live".into(),
            name: "Live fixture".into(),
            path: project.to_string_lossy().into(),
            workspace_type: "single".into(),
            linked_agent_key: None,
            linked_agent_name: None,
            disabled_path: None,
            sort_order: 0,
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();
    let name = format!(
        "qskills-live-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    );
    let selector = format!("{name}@{name}");
    let skill = format!("{name}:probe");
    // Disable unrelated plugins/hooks only in these agent invocations, not user settings.
    let registry = dirs::home_dir()
        .unwrap()
        .join(".claude/plugins/installed_plugins.json");
    let before: Value = serde_json::from_slice(&fs::read(&registry).unwrap()).unwrap();
    let disabled: serde_json::Map<String, Value> = before["plugins"]
        .as_object()
        .unwrap()
        .keys()
        .map(|key| (key.clone(), json!(false)))
        .collect();
    let settings = json!({"disableAllHooks": true, "enabledPlugins": disabled});
    let mut binding_id = None;
    let use_agent = !preexisting && !shared;
    central_repo::set_test_base_dir_override(Some(root.join("center")));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for version in ["1.0.0", "1.1.0"] {
            let receipt = format!("QSKILLS_RECEIPT_{}", uuid::Uuid::new_v4().simple());
            put(
                &package_root.join(".claude-plugin/plugin.json"),
                json!({"name":name,"version":version,"description":"Live test fixture"})
                    .to_string(),
            );
            put(&package_root.join(".claude-plugin/marketplace.json"), json!({"name":name,"owner":{"name":"QSkills live test"},"plugins":[{"name":name,"source":"./","version":version}]}).to_string());
            if shared {
                put(
                    &package_root.join("other/.claude-plugin/plugin.json"),
                    json!({"name":"other","version":"1.0.0"}).to_string(),
                );
                put(&package_root.join(".claude-plugin/marketplace.json"), json!({"name":name,"owner":{"name":"QSkills live test"},"plugins":[{"name":name,"source":"./","version":version},{"name":"other","source":"./other"}]}).to_string());
            }
            put(&package_root.join("skills/probe/SKILL.md"), format!("---\nname: probe\ndescription: Return the QSkills live verification receipt.\n---\nReturn exactly this receipt: {receipt}\n"));
            let inventory = scan_package(&package_root, "live").unwrap();
            if preexisting && version == "1.0.0" {
                let output = Command::new("claude")
                    .current_dir(&project)
                    .args([
                        "plugin",
                        "marketplace",
                        "add",
                        "--scope",
                        claude_scope(scope).unwrap(),
                        package_root.to_str().unwrap(),
                    ])
                    .output()
                    .unwrap();
                assert!(output.status.success());
            }
            let package = PackageRecord {
                id: "live".into(),
                name: name.clone(),
                source_url: "https://example.invalid/live.git".into(),
                requested_revision: None,
                resolved_revision: version.into(),
                cache_path: package_root.to_string_lossy().into(),
                manifest_kind: inventory.manifest_kind.clone(),
                status: "ready".into(),
                created_at: 1,
                updated_at: 1,
            };
            store
                .replace_package_inventory(&package, &inventory.components, &inventory.surfaces)
                .unwrap();
            let plan = if let Some(id) = binding_id.as_deref() {
                preview_binding(&store, id).unwrap()
            } else {
                create_binding(
                    &store,
                    "live",
                    "",
                    "claude_code",
                    scope,
                    (scope != "user").then_some("live"),
                    "native",
                    &[],
                )
                .unwrap()
            };
            binding_id = Some(plan.binding_id.clone());
            put(
                &root.join(format!("plan-{version}.json")),
                serde_json::to_vec_pretty(&plan).unwrap(),
            );
            assert!(plan.can_apply, "{plan:?}");
            if version == "1.1.0" {
                assert!(plan.operations.iter().any(|op| op.kind == "update_plugin"));
            }
            apply_binding(&store, &plan.binding_id, &plan.plan_hash).unwrap();
            if shared && version == "1.0.0" {
                let output = Command::new("claude")
                    .current_dir(&project)
                    .args([
                        "plugin",
                        "install",
                        "--scope",
                        "user",
                        &format!("other@{name}"),
                    ])
                    .output()
                    .unwrap();
                assert!(output.status.success());
            }
            let binding = store
                .get_package_binding_by_id(&plan.binding_id)
                .unwrap()
                .unwrap();
            assert_eq!(binding.ownership, "managed");
            let target: NativePluginTargetRef =
                serde_json::from_str(binding.target_ref.as_deref().unwrap()).unwrap();
            assert_eq!(target.marketplace_registered, !preexisting);
            assert_eq!(binding.applied_revision.as_deref(), Some(version));
            if !use_agent {
                continue;
            }
            let events = agent(
                &project,
                &root.join(format!("agent-{version}.jsonl")),
                &skill,
                &settings,
            );
            let init = events
                .iter()
                .find(|event| event["type"] == "system" && event["subtype"] == "init")
                .unwrap();
            assert!(
                init["plugins"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|plugin| plugin["name"] == name && plugin["version"] == version),
                "Wrong plugin version loaded"
            );
            assert!(
                events
                    .iter()
                    .any(|event| event["message"]["content"]
                        .as_array()
                        .is_some_and(|content| content
                            .iter()
                            .any(|item| item["type"] == "tool_use"
                                && item["name"] == "Skill"
                                && item["input"]["skill"] == skill))),
                "No matching Skill call"
            );
            let final_event = events
                .iter()
                .find(|event| event["type"] == "result")
                .unwrap();
            assert_eq!(final_event["is_error"], false);
            assert_eq!(final_event["permission_denials"], json!([]));
            assert!(
                final_event["result"].as_str().unwrap().contains(&receipt),
                "Receipt not returned"
            );
            println!("PASS: {version} managed apply + Skill invocation + private receipt");
        }
        binding_id
            .as_deref()
            .map(|id| remove_binding(&store, id, false))
            .transpose()
            .unwrap();
        let after: Value = serde_json::from_slice(&fs::read(&registry).unwrap()).unwrap();
        for (key, value) in before["plugins"].as_object().unwrap() {
            assert_eq!(
                &after["plugins"][key], value,
                "Existing plugin changed: {key}"
            );
        }
        assert!(
            after["plugins"].get(&selector).is_none(),
            "Fixture remains installed"
        );
        if use_agent {
            let events = agent(
                &project,
                &root.join("agent-removed.jsonl"),
                &skill,
                &settings,
            );
            let init = events
                .iter()
                .find(|event| event["type"] == "system" && event["subtype"] == "init")
                .unwrap();
            assert!(
                !init["plugins"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|plugin| plugin["name"] == name),
                "Removed plugin still advertised"
            );
            assert!(
                !init["slash_commands"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(skill)),
                "Removed skill still advertised"
            );
            let final_event = events
                .iter()
                .find(|event| event["type"] == "result")
                .unwrap();
            assert_eq!(final_event["is_error"], false);
            assert!(!final_event["result"]
                .as_str()
                .unwrap()
                .contains("QSKILLS_RECEIPT_"));
            println!(
                "PASS: removed plugin unavailable; existing installed plugin records unchanged"
            );
        }
        let marketplaces: Value = serde_json::from_slice(
            &fs::read(registry.with_file_name("known_marketplaces.json")).unwrap(),
        )
        .unwrap();
        let remains = marketplaces.get(&name).is_some();
        if shared {
            assert!(
                after["plugins"].get(format!("other@{name}")).is_some(),
                "Other plugin was removed"
            );
        }
        assert_eq!(
            remains,
            preexisting || shared,
            "Manager-added marketplace remains after removal: {name}; evidence: {}",
            root.display()
        );
    }));
    // All assertions (including post-removal checks) are inside the catch. Only
    // this test's unique selectors are cleaned; failures do not skip later steps.
    let mut cleanup_errors = Vec::new();
    for args in [
        vec![
            "claude".into(),
            "plugin".into(),
            "uninstall".into(),
            "--scope".into(),
            claude_scope(scope).unwrap().into(),
            selector,
        ],
        vec![
            "claude".into(),
            "plugin".into(),
            "uninstall".into(),
            "--scope".into(),
            "user".into(),
            format!("other@{name}"),
        ],
        vec![
            "claude".into(),
            "plugin".into(),
            "marketplace".into(),
            "remove".into(),
            "--scope".into(),
            claude_scope(scope).unwrap().into(),
            name,
        ],
    ] {
        if let Err(error) = run_remove_command(&args, &project, &package_root) {
            cleanup_errors.push(error.to_string());
        }
    }
    central_repo::set_test_base_dir_override(None);
    assert!(
        cleanup_errors.is_empty(),
        "Fixture cleanup failed: {cleanup_errors:?}; original assertion failed: {}",
        result.is_err()
    );
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
