use super::{
    adapter::adapt_page,
    client::{source_url, MarketClient},
    download, install,
    models::{Board, Package, SearchResponse},
    search_builder::SearchBuilder,
};
use crate::core::{central_repo, error::ErrorKind, installer, skill_store::SkillStore};
use serde_json::json;
use std::{
    io::{Cursor, Read, Write},
    net::TcpListener,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

fn package() -> serde_json::Value {
    json!({
        "identifier": "skills:github.com/team/repo/demo", "repository": "skills-public",
        "name": "demo", "group": "github.com/team/repo", "description": "Example",
        "newest_version": {"version": "1.2.3"},
        "download_total": 100, "download_biweekly": 20, "download_monthly": 30,
        "user_permission": {"download": true}
    })
}
fn page(items: serde_json::Value) -> SearchResponse {
    serde_json::from_value(json!({
        "message": "success", "count": 123, "page": 2, "page_size": 24, "data": items
    }))
    .unwrap()
}
fn zip(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(data.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
fn fixture(responses: Vec<(u16, Vec<u8>)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                header.push(byte[0]);
            }
            requests.push(String::from_utf8(header).unwrap());
            write!(
                stream,
                "HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        }
        requests
    });
    (base, thread)
}

#[test]
fn ranking_maps_period_counts_and_server_pagination() {
    for (board, expected) in [
        (Board::All, 100),
        (Board::Biweekly, 20),
        (Board::Monthly, 30),
    ] {
        let result = adapt_page(page(json!([package()])), board).unwrap();
        assert_eq!(result.skills[0].downloads, expected);
        assert_eq!((result.total, result.page, result.page_size), (123, 2, 24));
        assert_eq!(result.skills[0].version.as_deref(), Some("1.2.3"));
    }
}
#[test]
fn missing_optional_fields_and_null_description_are_supported() {
    let minimal = json!({"identifier": "skills:internal/demo", "repository": "hosted", "name": "demo", "description": null});
    let result = adapt_page(page(json!([minimal])), Board::All).unwrap();
    assert_eq!(result.skills[0].downloads, 0);
    assert_eq!(result.skills[0].description, "");
    assert!(result.skills[0].version.is_none());
}
#[test]
fn empty_success_differs_from_business_error() {
    assert!(adapt_page(page(json!([])), Board::All)
        .unwrap()
        .skills
        .is_empty());
    let mut response = page(json!([]));
    response.message = "permission denied".into();
    assert!(adapt_page(response, Board::All)
        .unwrap_err()
        .message
        .contains("permission denied"));
}
#[test]
fn duplicate_identity_is_deduplicated_but_repository_is_part_of_identity() {
    let mut other = package();
    other["repository"] = json!("skills-byted");
    let result = adapt_page(page(json!([package(), package(), other])), Board::All).unwrap();
    assert_eq!(result.skills.len(), 2);
    assert_ne!(result.skills[0].url, result.skills[1].url);
}
#[test]
fn only_skill_identities_are_accepted() {
    let mut item = package();
    item["identifier"] = json!("mcp:server");
    assert!(adapt_page(page(json!([item])), Board::All).is_err());
    assert!(serde_json::from_value::<Package>(json!({"name": "missing identity"})).is_err());
}
#[test]
fn client_is_reused_until_proxy_changes() {
    let first = MarketClient::shared(None).unwrap();
    let second = MarketClient::shared(None).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    let empty_proxy = MarketClient::shared(Some("   ")).unwrap();
    assert!(Arc::ptr_eq(&first, &empty_proxy));

    assert!(MarketClient::shared(Some("http://127.0.0.1:not-a-port")).is_err());
    assert!(Arc::ptr_eq(&first, &MarketClient::shared(None).unwrap()));

    let proxy = "http://127.0.0.1:7890";
    let changed = MarketClient::shared(Some(proxy)).unwrap();
    assert!(!Arc::ptr_eq(&first, &changed));
    assert!(Arc::ptr_eq(
        &changed,
        &MarketClient::shared(Some(proxy)).unwrap()
    ));
    MarketClient::shared(None).unwrap();
}

#[test]
fn urls_encode_search_and_identity_and_use_colon_for_version() {
    let base = "https://skills.byted.org";
    for board in [Board::All, Board::Biweekly, Board::Monthly] {
        let url = SearchBuilder::new(base)
            .board(board)
            .query(" 前端/a?# ")
            .page(3)
            .page_size(24)
            .build()
            .unwrap();
        assert!(url.path().contains("%2Fa%3F%23"));
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(pairs["order_by"], board.order_by());
        assert!(!pairs.contains_key("group"));
        assert_eq!(pairs["page"], "3");
    }
    assert!(SearchBuilder::new(base)
        .build()
        .unwrap()
        .path()
        .ends_with('/'));
    let client = MarketClient::for_test(base.into());
    let url = client
        .detail_url("skills:internal/team/demo", "hosted", Some("1.0"))
        .unwrap();
    assert!(url
        .path()
        .ends_with("skills%3Ainternal%2Fteam%2Fdemo%3A1.0"));
    assert!(url.query().unwrap().contains("with_asset=true"));
    assert!(source_url("skills:internal/team/a b", "hosted")
        .contains("/skills:internal/team/a%20b?repository=hosted"));
}
#[test]
fn download_urls_require_https_and_normalize_only_market_host() {
    assert!(download::download_url("http://example.com/demo.zip").is_err());
    assert!(download::download_url("https://user:secret@example.com/demo.zip").is_err());
    assert!(
        download::download_url("https://skills.byted.org/api/v1/download/team/demo.zip")
            .unwrap()
            .path()
            .starts_with("/api/v4/skills/download/")
    );
    assert!(
        download::download_url("https://example.com/api/v1/download/demo.zip")
            .unwrap()
            .path()
            .starts_with("/api/v1/download/")
    );
}
#[tokio::test]
async fn built_search_url_is_sent_and_maps_ranked_page() {
    let body = serde_json::to_vec(&json!({
        "message": "success",
        "count": 123,
        "page": 2,
        "page_size": 12,
        "data": [package()]
    }))
    .unwrap();
    let (base, thread) = fixture(vec![(200, body)]);
    let url = SearchBuilder::new(&base)
        .board(Board::Monthly)
        .query("demo/a")
        .page(2)
        .page_size(12)
        .build()
        .unwrap();
    let client = MarketClient::for_test(base.clone());
    let result = client.send(url, Board::Monthly).await.unwrap();
    assert_eq!(result.skills[0].downloads, 30);
    assert_eq!((result.total, result.page, result.page_size), (123, 2, 12));
    let requests = thread.join().unwrap();
    let path = requests[0].split_whitespace().nth(1).unwrap();
    let url = reqwest::Url::parse(&format!("{base}{path}")).unwrap();
    assert!(url.path().ends_with("/demo%2Fa"));
    let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(pairs["order_by"], Board::Monthly.order_by());
    assert_eq!(pairs["page"], "2");
    assert_eq!(pairs["page_size"], "12");
    assert!(!pairs.contains_key("group"));
}

#[tokio::test]
async fn http_and_business_failures_are_not_empty_results() {
    for (status, body) in [
        (403, b"forbidden".to_vec()),
        (200, b"<html>login</html>".to_vec()),
        (
            200,
            br#"{"message":"permission denied","data":null}"#.to_vec(),
        ),
    ] {
        let (base, thread) = fixture(vec![(status, body)]);
        let url = SearchBuilder::new(&base).build().unwrap();
        let client = MarketClient::for_test(base);
        assert!(client.send(url, Board::All).await.is_err());
        thread.join().unwrap();
    }
}
#[tokio::test]
async fn permission_failure_stops_before_version_download() {
    let mut item = package();
    item["user_permission"]["download"] = json!(false);
    let (base, thread) = fixture(vec![(
        200,
        serde_json::to_vec(&json!({"message": "success", "data": item})).unwrap(),
    )]);
    let result = download::prepare(
        &MarketClient::for_test(base),
        "skills:github.com/team/repo/demo",
        "skills-public",
        &Arc::new(AtomicBool::new(false)),
    )
    .await;
    assert_eq!(result.err().unwrap().kind, ErrorKind::InvalidInput);
    assert_eq!(thread.join().unwrap().len(), 1);
}
#[tokio::test]
async fn disabled_version_stops_before_asset_download() {
    let (base, thread) = fixture(vec![
        (200, serde_json::to_vec(&json!({"message": "success", "data": package()})).unwrap()),
        (200, serde_json::to_vec(&json!({"message": "success", "data": {"version": "1.2.3", "disabled": true, "assets": []}})).unwrap()),
    ]);
    assert!(download::prepare(
        &MarketClient::for_test(base),
        "skills:github.com/team/repo/demo",
        "skills-public",
        &Arc::new(AtomicBool::new(false))
    )
    .await
    .is_err());
    assert!(thread.join().unwrap()[1].contains("%3A1.2.3"));
}
#[tokio::test]
async fn downloads_and_installs_fixture_into_isolated_destination() {
    let archive = zip(&[
        (
            "demo/SKILL.md",
            "---\nname: demo\ndescription: fixture\n---\nExample",
        ),
        ("demo/scripts/test.txt", "content"),
    ]);
    let (base, thread) = fixture(vec![(200, archive)]);
    let cancel = AtomicBool::new(false);
    let bytes = download::read_archive(
        &MarketClient::for_test(base.clone()),
        reqwest::Url::parse(&base).unwrap(),
        &cancel,
    )
    .await
    .unwrap();
    let destination = tempfile::tempdir().unwrap();
    let extracted = tempfile::tempdir().unwrap();
    let source = download::extract(&bytes, extracted.path(), &cancel).unwrap();
    let result = installer::install_skill_dir_to_destination(
        &source,
        "demo",
        &destination.path().join("installed"),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(result.central_path.join("scripts/test.txt")).unwrap(),
        "content"
    );
    assert_eq!(result.description.as_deref(), Some("fixture"));
    thread.join().unwrap();
}
#[tokio::test]
async fn failed_download_returns_network_error() {
    let (base, thread) = fixture(vec![(404, b"missing".to_vec())]);
    let result = download::read_archive(
        &MarketClient::for_test(base.clone()),
        reqwest::Url::parse(&base).unwrap(),
        &AtomicBool::new(false),
    )
    .await;
    assert_eq!(result.unwrap_err().kind, ErrorKind::Network);
    thread.join().unwrap();
}
#[tokio::test]
async fn cancellation_interrupts_waiting_for_response() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_on_accept = cancel.clone();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        cancel_on_accept.store(true, Ordering::SeqCst);
        // Hold the socket open until the cancelled client drops it.
        let mut byte = [0];
        use tokio::io::AsyncReadExt;
        let mut stream = stream;
        while stream.read(&mut byte).await.unwrap_or(0) > 0 {}
    });
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        download::read_archive(
            &MarketClient::for_test(base.clone()),
            reqwest::Url::parse(&base).unwrap(),
            &cancel,
        ),
    )
    .await
    .unwrap();
    assert_eq!(result.unwrap_err().kind, ErrorKind::Cancelled);
    server.await.unwrap();
}
#[test]
fn cancellation_happens_before_extraction_writes() {
    let temp = tempfile::tempdir().unwrap();
    let error = download::extract(
        &zip(&[("SKILL.md", "demo")]),
        temp.path(),
        &AtomicBool::new(true),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled);
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}
#[test]
fn rejects_missing_and_multiple_skills() {
    for entries in [
        vec![("README.md", "missing")],
        vec![("one/SKILL.md", "one"), ("two/SKILL.md", "two")],
    ] {
        let temp = tempfile::tempdir().unwrap();
        assert!(download::extract(&zip(&entries), temp.path(), &AtomicBool::new(false)).is_err());
    }
}
#[test]
fn unsafe_and_duplicate_paths_fail_before_any_file_is_written() {
    for name in [
        "../outside",
        "/absolute",
        "a\\b",
        "a:b",
        "./file",
        "NUL.txt",
        "CONIN$",
        "COM¹.txt",
    ] {
        let temp = tempfile::tempdir().unwrap();
        assert!(download::extract(
            &zip(&[("SKILL.md", "demo"), (name, "bad")]),
            temp.path(),
            &AtomicBool::new(false)
        )
        .is_err());
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    }
    let temp = tempfile::tempdir().unwrap();
    assert!(download::extract(
        &zip(&[("SKILL.md", "one"), ("skill.md", "two")]),
        temp.path(),
        &AtomicBool::new(false)
    )
    .is_err());
}
#[test]
fn symlinks_are_rejected_before_writing() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_symlink(
            "link",
            "../outside",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    let temp = tempfile::tempdir().unwrap();
    assert!(download::extract(&bytes, temp.path(), &AtomicBool::new(false)).is_err());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn hidden_skill_assets_are_preserved() {
    let temp = tempfile::tempdir().unwrap();
    download::extract(
        &zip(&[("SKILL.md", "demo"), (".config/settings.json", "{}")]),
        temp.path(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(temp.path().join(".config/settings.json").is_file());
}

#[test]
fn oversized_declared_archive_size_fails_before_writing() {
    let mut bytes = zip(&[("SKILL.md", "demo")]);
    let central = bytes
        .windows(4)
        .position(|value| value == b"PK\x01\x02")
        .unwrap();
    let oversized = (100u32 * 1024 * 1024 + 1).to_le_bytes();
    bytes[central + 24..central + 28].copy_from_slice(&oversized);
    let temp = tempfile::tempdir().unwrap();
    assert!(download::extract(&bytes, temp.path(), &AtomicBool::new(false)).is_err());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn excessive_archive_entries_fail_before_writing() {
    let names: Vec<_> = (0..4097).map(|index| format!("file-{index}")).collect();
    let entries: Vec<_> = names.iter().map(|name| (name.as_str(), "")).collect();
    let temp = tempfile::tempdir().unwrap();
    assert!(download::extract(&zip(&entries), temp.path(), &AtomicBool::new(false)).is_err());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

struct RepoOverride;
impl Drop for RepoOverride {
    fn drop(&mut self) {
        central_repo::set_test_base_dir_override(None);
    }
}
fn downloaded() -> download::DownloadedSkill {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("SKILL.md"),
        "---\nname: demo\ndescription: fixture\n---\nExample",
    )
    .unwrap();
    download::DownloadedSkill {
        skill_dir: temp.path().to_path_buf(),
        _temp: temp,
        name: "demo".into(),
        version: "1.2.3".into(),
        download_url: "https://skills.byted.org/api/v4/skills/download/demo.zip".into(),
    }
}

#[test]
fn market_source_identity_roundtrips_and_rejects_wrong_hosts() {
    let source = source_url("skills:internal/team/my skill", "skills byted");
    assert_eq!(
        super::update::identity(&source).unwrap(),
        (
            "skills:internal/team/my skill".into(),
            "skills byted".into()
        )
    );
    for source in [
        "https://example.com/skill/skills:demo?repository=public",
        "http://skills.bytedance.net/skill/skills:demo?repository=public",
        "https://skills.bytedance.net/skill/skills:demo",
        "https://skills.bytedance.net/skill/skills:demo?repository=a&repository=b",
    ] {
        assert!(super::update::identity(source).is_err());
    }
}

#[test]
fn latest_version_checks_identity_permissions_and_availability() {
    let mut disabled = package();
    disabled["disabled"] = json!(true);
    let mut forbidden = package();
    forbidden["user_permission"]["download"] = json!(false);
    let mut missing = package();
    missing["newest_version"] = json!(null);
    let mut wrong = package();
    wrong["repository"] = json!("wrong");
    for (item, expected) in [
        (package(), None),
        (disabled, Some("disabled")),
        (forbidden, Some("permission")),
        (missing, Some("no installable version")),
        (wrong, Some("different package")),
    ] {
        let (base, server) = fixture(vec![(
            200,
            serde_json::to_vec(&json!({"message":"success", "data":item})).unwrap(),
        )]);
        let result = super::update::latest_version(
            &MarketClient::for_test(base),
            "skills:github.com/team/repo/demo",
            "skills-public",
        );
        if let Some(message) = expected {
            assert!(result.unwrap_err().message.contains(message));
        } else {
            assert_eq!(result.unwrap(), "1.2.3");
        }
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("/api/v2/package/"));
    }
    let (base, server) = fixture(vec![(503, b"unavailable".to_vec())]);
    assert!(super::update::latest_version(
        &MarketClient::for_test(base),
        "skills:github.com/team/repo/demo",
        "skills-public"
    )
    .is_err());
    server.join().unwrap();
}

#[test]
fn market_conversion_keeps_identity_and_only_commits_after_removal_approval() {
    let _serial = central_repo::test_base_dir_lock();
    let repo = tempfile::tempdir().unwrap();
    central_repo::set_test_base_dir_override(Some(repo.path().to_path_buf()));
    let _reset = RepoOverride;
    let store = SkillStore::new(&repo.path().join("test.db")).unwrap();
    let cancel = AtomicBool::new(false);
    let id = install::commit(
        &store,
        downloaded(),
        "skills:internal/team/demo",
        "skills-byted",
        &cancel,
    )
    .unwrap();
    let original = store.get_skill_by_id(&id).unwrap().unwrap();
    store
        .update_skill_after_reinstall(
            &id,
            &original.name,
            original.description.as_deref(),
            "local",
            Some("/old/source"),
            None,
            None,
            None,
            None,
            None,
            original.content_hash.as_deref(),
            "local_only",
        )
        .unwrap();
    let original = store.get_skill_by_id(&id).unwrap().unwrap();
    store
        .set_tags_for_skill(&id, &["important".into()])
        .unwrap();
    store
        .insert_scenario(&crate::core::skill_store::ScenarioRecord {
            id: "preset".into(),
            name: "Test preset".into(),
            description: None,
            icon: None,
            sort_order: 0,
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();
    store.add_skill_to_scenario("preset", &id).unwrap();
    let deployed = repo.path().join("agent/demo");
    std::fs::create_dir_all(&deployed).unwrap();
    std::fs::copy(
        Path::new(&original.central_path).join("SKILL.md"),
        deployed.join("SKILL.md"),
    )
    .unwrap();
    store
        .insert_target(&crate::core::skill_store::SkillTargetRecord {
            id: "copy-target".into(),
            skill_id: id.clone(),
            tool: "claude_code".into(),
            target_path: deployed.to_string_lossy().into_owned(),
            mode: "copy".into(),
            status: "ok".into(),
            synced_at: None,
            last_error: None,
            source_hash: None,
        })
        .unwrap();
    std::fs::write(
        Path::new(&original.central_path).join("notes.txt"),
        "keep until approved",
    )
    .unwrap();
    let apply = |identifier: &str, version: &str, approval: Option<&str>| {
        let mut next = downloaded();
        next.version = version.into();
        std::fs::create_dir_all(next.skill_dir.join("scripts")).unwrap();
        std::fs::write(next.skill_dir.join("scripts/new.txt"), "market version").unwrap();
        super::update::apply_downloaded(
            &store,
            &original,
            next,
            identifier,
            "skills-byted",
            true,
            &cancel,
            approval,
        )
        .unwrap()
    };
    let held = apply("skills:internal/team/demo", "2.0", None);
    assert!(!held.pending_removals.is_empty());
    let unchanged = store.get_skill_by_id(&id).unwrap().unwrap();
    assert_eq!(unchanged.source_type, "local");
    assert_eq!(unchanged.source_ref, original.source_ref);
    assert!(Path::new(&original.central_path).join("notes.txt").exists());
    let stale = apply(
        "skills:internal/team/demo",
        "3.0",
        held.removal_approval.as_deref(),
    );
    assert!(!stale.pending_removals.is_empty());
    let different_package = apply(
        "skills:internal/team/other",
        "2.0",
        held.removal_approval.as_deref(),
    );
    assert!(!different_package.pending_removals.is_empty());
    let applied = apply(
        "skills:internal/team/demo",
        "2.0",
        held.removal_approval.as_deref(),
    );
    assert_eq!(applied.skill.id, id);
    assert_eq!(applied.skill.name, original.name);
    assert_eq!(applied.skill.central_path, original.central_path);
    assert_eq!(applied.skill.source_type, "bytedance");
    assert_eq!(applied.skill.source_revision.as_deref(), Some("2.0"));
    assert_eq!(applied.skill.update_status, "up_to_date");
    assert_eq!(applied.skill.tags, vec!["important"]);
    assert_eq!(applied.skill.preset_ids, vec!["preset"]);
    assert_eq!(applied.skill.targets.len(), 1);
    assert_eq!(
        std::fs::read_to_string(deployed.join("scripts/new.txt")).unwrap(),
        "market version"
    );
    let target = store.get_targets_for_skill(&id).unwrap().pop().unwrap();
    assert_eq!(
        target.source_hash,
        store.get_skill_by_id(&id).unwrap().unwrap().content_hash
    );
    assert!(!Path::new(&original.central_path).join("notes.txt").exists());
    assert!(!std::fs::read_dir(central_repo::skills_dir())
        .unwrap()
        .any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("staged")));
}

#[test]
fn metadata_only_market_update_preserves_files_and_rejects_stale_or_cancelled_commit() {
    let _serial = central_repo::test_base_dir_lock();
    let repo = tempfile::tempdir().unwrap();
    central_repo::set_test_base_dir_override(Some(repo.path().to_path_buf()));
    let _reset = RepoOverride;
    let store = SkillStore::new(&repo.path().join("test.db")).unwrap();
    let cancel = AtomicBool::new(false);
    let id = install::commit(
        &store,
        downloaded(),
        "skills:internal/team/demo",
        "skills-byted",
        &cancel,
    )
    .unwrap();
    let original = store.get_skill_by_id(&id).unwrap().unwrap();
    let mut next = downloaded();
    next.version = "2.0".into();
    let result = super::update::apply_downloaded(
        &store,
        &original,
        next,
        "skills:internal/team/demo",
        "skills-byted",
        false,
        &cancel,
        None,
    )
    .unwrap();
    assert!(!result.content_changed);
    assert_eq!(result.skill.source_revision.as_deref(), Some("2.0"));
    assert!(super::update::apply_downloaded(
        &store,
        &original,
        downloaded(),
        "skills:internal/team/demo",
        "skills-byted",
        false,
        &cancel,
        None
    )
    .is_err());
    let current = store.get_skill_by_id(&id).unwrap().unwrap();
    assert!(super::update::apply_downloaded(
        &store,
        &current,
        downloaded(),
        "skills:internal/team/demo",
        "skills-byted",
        false,
        &AtomicBool::new(true),
        None
    )
    .is_err());
    assert_eq!(
        store.get_skill_by_id(&id).unwrap().unwrap().source_revision,
        current.source_revision
    );
}
#[test]
fn persists_source_and_version_and_keeps_duplicate_install_identity() {
    let _serial = central_repo::test_base_dir_lock();
    let repo = tempfile::tempdir().unwrap();
    central_repo::set_test_base_dir_override(Some(repo.path().to_path_buf()));
    let _reset = RepoOverride;
    let store = SkillStore::new(&repo.path().join("test.db")).unwrap();
    let identifier = "skills:internal/team/demo";
    let cancel = AtomicBool::new(false);
    let id = install::commit(&store, downloaded(), identifier, "skills-byted", &cancel).unwrap();
    let record = store.get_skill_by_id(&id).unwrap().unwrap();
    assert_eq!(record.source_type, "bytedance");
    assert_eq!(
        record.source_ref.as_deref(),
        Some(source_url(identifier, "skills-byted").as_str())
    );
    assert_eq!(record.source_revision.as_deref(), Some("1.2.3"));
    assert_eq!(record.update_status, "up_to_date");
    assert_eq!(record.remote_revision.as_deref(), Some("1.2.3"));
    assert!(record
        .source_ref_resolved
        .unwrap()
        .contains("/api/v4/skills/download/"));
    std::fs::write(
        std::path::Path::new(&record.central_path).join("user.txt"),
        "keep",
    )
    .unwrap();
    let repeated =
        install::commit(&store, downloaded(), identifier, "skills-byted", &cancel).unwrap();
    assert_eq!(repeated, id);
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(&record.central_path).join("user.txt"))
            .unwrap(),
        "keep"
    );
    let other =
        install::commit(&store, downloaded(), identifier, "skills-public", &cancel).unwrap();
    assert_ne!(id, other);
    let other = store.get_skill_by_id(&other).unwrap().unwrap();
    assert_ne!(record.central_path, other.central_path);
    assert_eq!(store.get_all_skills().unwrap().len(), 2);
}
#[test]
fn cancelled_install_leaves_no_database_or_library_files() {
    let _serial = central_repo::test_base_dir_lock();
    let repo = tempfile::tempdir().unwrap();
    central_repo::set_test_base_dir_override(Some(repo.path().to_path_buf()));
    let _reset = RepoOverride;
    let store = SkillStore::new(&repo.path().join("test.db")).unwrap();
    let prepared = downloaded();
    let temporary_path = prepared.skill_dir.clone();
    let error = install::commit(
        &store,
        prepared,
        "skills:internal/demo",
        "hosted",
        &AtomicBool::new(true),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled);
    assert!(store.get_all_skills().unwrap().is_empty());
    assert!(!central_repo::skills_dir().exists());
    assert!(!temporary_path.exists());
}
#[tokio::test]
#[ignore = "Live marketplace smoke test; downloads into a temporary directory only"]
async fn live_public_skill_archive() {
    let client = MarketClient::new(None).unwrap();
    let downloaded = download::prepare(
        &client,
        "skills:github.com/eronred/aso-skills/keyword-research",
        "skills-public",
        &Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap();
    assert!(downloaded.skill_dir.join("SKILL.md").is_file());
    assert!(!downloaded.version.is_empty());
    let destination = tempfile::tempdir().unwrap();
    let installed = installer::install_skill_dir_to_destination(
        &downloaded.skill_dir,
        &downloaded.name,
        &destination.path().join("skill"),
    )
    .unwrap();
    assert!(installed.central_path.join("SKILL.md").is_file());
}
