use std::fs;
use std::path::Path;

use crate::core::skill_store::SkillRecord;
use tempfile::tempdir;

use super::{locate, MoveEvidence, Placement};

fn write_skill(dir: &Path, name: &str, body: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\n---\n{body}\n"),
    )
    .unwrap();
}

fn skill(name: &str, subpath: Option<&str>, content_hash: Option<String>) -> SkillRecord {
    SkillRecord {
        id: name.to_string(),
        name: name.to_string(),
        description: None,
        source_type: "git".to_string(),
        source_ref: Some("https://example.test/mono.git".to_string()),
        source_ref_resolved: Some("https://example.test/mono.git".to_string()),
        source_subpath: subpath.map(str::to_string),
        source_branch: None,
        source_revision: Some("rev-1".to_string()),
        remote_revision: None,
        central_path: format!("/library/{name}"),
        content_hash,
        enabled: true,
        created_at: 1,
        updated_at: 1,
        status: "ok".to_string(),
        update_status: "source_gone".to_string(),
        last_checked_at: None,
        last_check_error: None,
    }
}

#[test]
fn a_skill_still_at_its_recorded_path_is_at() {
    let checkout = tempdir().unwrap();
    write_skill(&checkout.path().join("skills/b"), "b", "body");

    let placement = locate(&skill("b", Some("skills/b"), None), checkout.path());

    assert_eq!(
        placement,
        Placement::At {
            subpath: "skills/b".to_string()
        }
    );
}

#[test]
fn a_directory_that_is_no_longer_a_skill_is_not_at() {
    let checkout = tempdir().unwrap();
    // The path survives as a container of other skills (#367 left this open).
    write_skill(&checkout.path().join("skills/b/nested"), "nested", "body");

    let placement = locate(&skill("b", Some("skills/b"), None), checkout.path());

    assert_eq!(placement, Placement::Gone, "a container is not the skill");
}

#[test]
fn the_same_content_elsewhere_is_a_high_confidence_move() {
    let checkout = tempdir().unwrap();
    write_skill(&checkout.path().join("moved/b"), "b", "body");
    let hash = crate::core::content_hash::hash_directory(&checkout.path().join("moved/b")).unwrap();

    let placement = locate(&skill("b", Some("skills/b"), Some(hash)), checkout.path());

    assert_eq!(
        placement,
        Placement::Moved {
            candidates: vec![super::MoveCandidate {
                subpath: "moved/b".to_string(),
                evidence: MoveEvidence::ContentHash,
            }]
        }
    );
}

#[test]
fn a_matching_directory_name_is_a_low_confidence_move_with_every_candidate() {
    let checkout = tempdir().unwrap();
    write_skill(&checkout.path().join("a/b"), "b", "one");
    write_skill(&checkout.path().join("c/b"), "b", "two");

    let placement = locate(&skill("b", Some("skills/b"), None), checkout.path());

    let Placement::Moved { candidates } = placement else {
        panic!("expected a move, got {placement:?}");
    };
    assert_eq!(candidates.len(), 2, "low confidence does not rank");
    assert!(candidates
        .iter()
        .all(|candidate| candidate.evidence == MoveEvidence::DirectoryName));
}

#[test]
fn content_beats_a_name_match() {
    let checkout = tempdir().unwrap();
    write_skill(&checkout.path().join("same-name"), "other", "body");
    write_skill(&checkout.path().join("hidden"), "b", "body");
    let hash = crate::core::content_hash::hash_directory(&checkout.path().join("hidden")).unwrap();

    let placement = locate(&skill("b", Some("skills/b"), Some(hash)), checkout.path());

    let Placement::Moved { candidates } = placement else {
        panic!("expected a move, got {placement:?}");
    };
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].subpath, "hidden");
    assert_eq!(candidates[0].evidence, MoveEvidence::ContentHash);
}

#[test]
fn an_empty_checkout_answers_gone() {
    let checkout = tempdir().unwrap();

    assert_eq!(
        locate(&skill("b", Some("skills/b"), None), checkout.path()),
        Placement::Gone
    );
}

#[test]
fn a_skill_installed_from_the_repository_root_is_found_at_the_root() {
    let checkout = tempdir().unwrap();
    write_skill(checkout.path(), "b", "body");

    let placement = locate(&skill("b", None, None), checkout.path());

    assert_eq!(
        placement,
        Placement::At {
            subpath: String::new()
        }
    );
}
