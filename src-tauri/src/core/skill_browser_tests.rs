use super::*;

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("references")).unwrap();
    std::fs::write(root.path().join("SKILL.md"), "# Start\n[More](references/guide.md)").unwrap();
    std::fs::write(root.path().join("references/guide.md"), "# Guide").unwrap();
    root
}

#[test]
fn opens_entry_and_reads_other_files() {
    let root = fixture();
    let session = BrowserSession::local(root.path().to_owned(), "Local".into()).unwrap();
    assert_eq!(session.default_path().unwrap().as_deref(), Some("SKILL.md"));
    let preview = session.read("references/guide.md").unwrap();
    assert_eq!(preview.kind, "markdown");
    assert_eq!(preview.content.as_deref(), Some("# Guide"));
    assert!(preview.content_hash.is_some());
}

#[test]
fn rejects_cross_platform_absolute_and_parent_paths() {
    let root = fixture();
    let session = BrowserSession::local(root.path().to_owned(), "Local".into()).unwrap();
    for path in ["../SKILL.md", "/etc/passwd", "C:/Windows/test", "//server/share", "references/../../out", "a\\b", "a\0b"] {
        assert!(session.resolve(path).is_err(), "accepted {path:?}");
    }
}

#[test]
fn paginates_and_does_not_hide_authored_dotfiles() {
    let root = fixture();
    std::fs::create_dir(root.path().join("node_modules")).unwrap();
    std::fs::write(root.path().join(".example"), "sample").unwrap();
    for n in 0..205 { std::fs::write(root.path().join(format!("{n:03}.txt")), "x").unwrap(); }
    let session = BrowserSession::local(root.path().to_owned(), "Local".into()).unwrap();
    let first = session.list("", 0).unwrap();
    assert_eq!(first.entries.len(), 200);
    assert_eq!(first.entries[0].name, "references");
    assert_eq!(first.entries[1].name, "SKILL.md");
    assert!(first.entries.iter().any(|e| e.name == ".example"));
    assert!(!first.entries.iter().any(|e| e.name == "node_modules"));
    assert_eq!(session.list("", first.next_cursor.unwrap()).unwrap().entries.len(), 8);
}

#[test]
fn limits_file_bytes_and_rejects_invalid_utf8() {
    let root = fixture();
    std::fs::write(root.path().join("huge.txt"), vec![b'a'; MAX_TEXT_BYTES + 1]).unwrap();
    std::fs::write(root.path().join("bad.txt"), [0xff, 0xfe]).unwrap();
    let session = BrowserSession::local(root.path().to_owned(), "Local".into()).unwrap();
    assert_eq!(session.read("huge.txt").unwrap().reason.as_deref(), Some("too_large"));
    assert_eq!(session.read("bad.txt").unwrap().reason.as_deref(), Some("encoding"));
}

#[cfg(unix)]
#[test]
fn does_not_follow_links_outside_skill_or_into_cycles() {
    let root = fixture();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
    std::os::unix::fs::symlink(root.path(), root.path().join("loop")).unwrap();
    let session = BrowserSession::local(root.path().to_owned(), "Local".into()).unwrap();
    assert!(session.read("escape/secret.txt").is_err());
    assert!(session.list("loop", 0).is_err());
}

#[cfg(unix)]
#[test]
fn a_directory_swapped_after_resolution_cannot_redirect_the_open_handle() {
    let root = fixture();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("guide.md"), "secret").unwrap();
    let session = BrowserSession::local(root.path().to_owned(), "Local".into()).unwrap();
    let resolved = session.resolve("references/guide.md").unwrap();
    std::fs::rename(root.path().join("references"), root.path().join("saved")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("references")).unwrap();
    assert!(session.open_regular_file(&resolved).is_err());
}

#[test]
fn resolves_relative_links_and_anchors_without_escaping() {
    let root = fixture();
    let session = BrowserSession::local(root.path().to_owned(), "Local".into()).unwrap();
    let link = session.resolve_link("references/guide.md", "../SKILL.md#start").unwrap();
    assert_eq!(link.relative_path, "SKILL.md");
    assert_eq!(link.fragment.as_deref(), Some("start"));
    for link in ["../../secret", "file:///etc/passwd", "javascript:alert(1)", "%252e%252e/out"] {
        assert!(session.resolve_link("SKILL.md", link).is_err());
    }
}

#[test]
fn snapshot_lives_until_last_reader_releases_it() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().to_owned();
    let session = Arc::new(BrowserSession::snapshot(path.clone(), "Git".into(), "abc".into(), path.clone()).unwrap());
    std::mem::forget(temp);
    let reader = session.clone();
    drop(session);
    assert!(path.exists());
    drop(reader);
    assert!(!path.exists());
}

#[test]
fn cancelled_open_cannot_publish_and_close_is_idempotent() {
    let root = fixture();
    let registry = BrowserSessionStore::default();
    registry.cancel("cancelled");
    assert!(registry.begin("cancelled").unwrap().load(Ordering::SeqCst));
    assert!(registry.publish("cancelled", BrowserSession::local(root.path().to_owned(), "Local".into()).unwrap()).is_err());
    registry.begin("active").unwrap();
    let info = registry.publish("active", BrowserSession::local(root.path().to_owned(), "Local".into()).unwrap()).unwrap();
    let reader = registry.get(&info.session_id).unwrap();
    registry.close(&info.session_id);
    registry.close(&info.session_id);
    assert!(registry.get(&info.session_id).is_err());
    assert_eq!(reader.read("SKILL.md").unwrap().kind, "markdown");
}

#[cfg(unix)]
#[test]
fn snapshot_checkout_restores_links_omitted_by_the_install_cache_copy() {
    let root = fixture();
    let repo = git2::Repository::init(root.path()).unwrap();
    std::os::unix::fs::symlink("SKILL.md", root.path().join("alias.md")).unwrap();
    let mut index = repo.index().unwrap();
    index.add_all(["*"], git2::IndexAddOption::DEFAULT, None).unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let signature = git2::Signature::now("Reader test", "reader@example.invalid").unwrap();
    let commit = repo.commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[]).unwrap();
    std::fs::remove_file(root.path().join("alias.md")).unwrap();
    checkout_snapshot(root.path(), &commit.to_string(), None, None).unwrap();
    assert!(std::fs::symlink_metadata(root.path().join("alias.md")).unwrap().file_type().is_symlink());
    let session = BrowserSession::local(root.path().to_owned(), "Git".into()).unwrap();
    assert!(session.read("alias.md").unwrap().content.unwrap().contains("# Start"));
}

#[test]
fn cancelled_snapshot_checkout_does_not_restore_files() {
    let root = fixture();
    let repo = git2::Repository::init(root.path()).unwrap();
    let mut index = repo.index().unwrap();
    index.add_all(["*"], git2::IndexAddOption::DEFAULT, None).unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let signature = git2::Signature::now("Reader test", "reader@example.invalid").unwrap();
    let commit = repo.commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[]).unwrap();
    std::fs::remove_file(root.path().join("SKILL.md")).unwrap();
    let cancel = Arc::new(AtomicBool::new(true));
    let result = checkout_snapshot(root.path(), &commit.to_string(), Some(&cancel), None);
    assert!(matches!(result, Err(error) if error.kind == crate::core::error::ErrorKind::Cancelled));
    assert!(!root.path().join("SKILL.md").exists());
}

#[test]
fn snapshot_checkout_works_without_system_git() {
    if std::env::var_os("SKILL_READER_GITLESS_PROBE").is_none() {
        let empty_path = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "core::skill_browser::tests::snapshot_checkout_works_without_system_git", "--nocapture"])
            .env("SKILL_READER_GITLESS_PROBE", "1").env("PATH", empty_path.path()).output().unwrap();
        assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        return;
    }
    let root = fixture();
    let repo = git2::Repository::init(root.path()).unwrap();
    let mut index = repo.index().unwrap();
    index.add_all(["*"], git2::IndexAddOption::DEFAULT, None).unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let signature = git2::Signature::now("Reader test", "reader@example.invalid").unwrap();
    let commit = repo.commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[]).unwrap();
    std::fs::remove_file(root.path().join("SKILL.md")).unwrap();
    checkout_snapshot(root.path(), &commit.to_string(), None, None).unwrap();
    assert!(std::fs::read_to_string(root.path().join("SKILL.md")).unwrap().contains("# Start"));
}
