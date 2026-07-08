//! Two-repository integration tests for the object merge engine (design
//! §10): compose/convergence, true conflicts with cross-device pending,
//! resolutions, ff guard, crash recovery, old-client detection and the
//! legacy fallback.
//!
//! Every test owns the global central-repo override (serialized by the
//! test lock) and switches it between "device A" and "device B" before
//! operating as that device.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::core::merge::apply::{self, object_merge_pull_unlocked, recover_on_startup};
use crate::core::merge::pending::{self, REF_APPLYING, REF_PRE_MERGE, conflict_ref};
use crate::core::merge::protocol;
use crate::core::merge::resolve::{ResolveAction, resolve_conflict_unlocked};
use crate::core::skill_store::SkillStore;
use crate::core::sync_metadata::{self, SkillMetaFile, SourceMeta};
use crate::core::{central_repo, git_backup};

struct Device {
    base: PathBuf,
    skills: PathBuf,
    store: SkillStore,
    name: &'static str,
}

struct Env {
    _guard: std::sync::MutexGuard<'static, ()>,
    _tmp: tempfile::TempDir,
    remote: PathBuf,
}

impl Drop for Env {
    fn drop(&mut self) {
        central_repo::set_test_base_dir_override(None);
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=test@example.com", "-c", "user.name=Raw Git"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn setup() -> Env {
    let guard = central_repo::test_base_dir_lock();
    let tmp = tempfile::tempdir().unwrap();
    let remote = tmp.path().join("remote.git");
    assert!(Command::new("git")
        .args(["init", "--bare", "--initial-branch=main"])
        .arg(&remote)
        .output()
        .unwrap()
        .status
        .success());
    Env { _guard: guard, _tmp: tmp, remote }
}

impl Env {
    /// First device: initializes the repo, seeds it, wires the remote.
    fn device_a(&self) -> Device {
        let base = self._tmp.path().join("a");
        let skills = base.join("skills");
        std::fs::create_dir_all(&skills).unwrap();
        let store = SkillStore::new(&base.join("db.sqlite")).unwrap();
        let dev = Device { base, skills, store, name: "Device A" };
        dev.activate();
        git(&dev.skills, &["init", "-b", "main"]);
        git_backup::configure_device_identity(&dev.skills, dev.name).unwrap();
        git(&dev.skills, &["remote", "add", "origin", self.remote.to_str().unwrap()]);
        dev
    }

    /// Second device: clones the remote after `device_a` pushed.
    fn device_b(&self) -> Device {
        let base = self._tmp.path().join("b");
        std::fs::create_dir_all(&base).unwrap();
        let skills = base.join("skills");
        assert!(Command::new("git")
            .arg("clone")
            .arg(&self.remote)
            .arg(&skills)
            .output()
            .unwrap()
            .status
            .success());
        let store = SkillStore::new(&base.join("db.sqlite")).unwrap();
        let dev = Device { base, skills, store, name: "Device B" };
        dev.activate();
        git_backup::configure_device_identity(&dev.skills, dev.name).unwrap();
        dev.reindex();
        dev
    }
}

impl Device {
    fn activate(&self) {
        central_repo::set_test_base_dir_override(Some(self.base.clone()));
    }

    fn write_skill(&self, id: &str, path: &str, content: &str) {
        self.write_skill_full(id, path, content, true, &[]);
    }

    fn write_skill_full(&self, id: &str, path: &str, content: &str, enabled: bool, tags: &[&str]) {
        let dir = self.skills.join(path);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), content).unwrap();
        let meta = SkillMetaFile {
            schema_version: 1,
            skill_id: id.to_string(),
            path: path.to_string(),
            path_key: sync_metadata::path_key(path),
            enabled,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            source: SourceMeta {
                source_type: "import".to_string(),
                ref_: None,
                subpath: None,
                branch: None,
            },
        };
        let meta_dir = self.skills.join(".skills-manager/skills");
        std::fs::create_dir_all(&meta_dir).unwrap();
        std::fs::write(
            meta_dir.join(format!("{id}.json")),
            sync_metadata::canonical_json_bytes(&meta).unwrap(),
        )
        .unwrap();
        std::fs::write(
            self.skills.join(".skills-manager/schema.json"),
            b"{\n  \"schema_version\": 1,\n  \"app_min_version\": \"2.0.0\",\n  \"created_by\": \"skills-manager\"\n}\n",
        )
        .unwrap();
    }

    fn remove_skill(&self, id: &str, path: &str) {
        let dir = self.skills.join(path);
        if dir.exists() {
            std::fs::remove_dir_all(dir).unwrap();
        }
        let meta = self.skills.join(format!(".skills-manager/skills/{id}.json"));
        if meta.exists() {
            std::fs::remove_file(meta).unwrap();
        }
    }

    fn reindex(&self) {
        self.activate();
        sync_metadata::reindex_from_metadata_unlocked(&self.store).unwrap();
    }

    fn commit(&self, message: &str) {
        self.activate();
        self.reindex();
        if git_backup::has_uncommitted_changes(&self.skills).unwrap() {
            git_backup::commit_all_unlocked(&self.skills, message).unwrap();
        }
    }

    fn push(&self) {
        git(&self.skills, &["push", "-u", "origin", "main"]);
    }

    fn pull(&self) -> apply::MergeSummary {
        self.activate();
        let summary = object_merge_pull_unlocked(&self.store, &self.skills).unwrap();
        // Mirror the command layer: reconcile the DB from merged metadata.
        sync_metadata::reindex_from_metadata_unlocked(&self.store).unwrap();
        summary
    }

    fn pull_err(&self) -> anyhow::Error {
        self.activate();
        object_merge_pull_unlocked(&self.store, &self.skills).unwrap_err()
    }

    fn skill_md(&self, path: &str) -> String {
        std::fs::read_to_string(self.skills.join(path).join("SKILL.md")).unwrap()
    }

    fn tree_oid(&self) -> String {
        git(&self.skills, &["rev-parse", "HEAD^{tree}"])
    }

    fn head_message(&self) -> String {
        git(&self.skills, &["log", "-1", "--format=%B"])
    }

    fn meta_of(&self, id: &str) -> Option<SkillMetaFile> {
        let raw = std::fs::read_to_string(
            self.skills.join(format!(".skills-manager/skills/{id}.json")),
        )
        .ok()?;
        serde_json::from_str(&raw).ok()
    }
}

/// Common two-device baseline: A seeds one skill, pushes, B clones.
fn seeded_pair(env: &Env) -> (Device, Device) {
    let a = env.device_a();
    a.write_skill("skill-1", "alpha", "base content");
    a.commit("seed");
    a.push();
    let b = env.device_b();
    (a, b)
}

// ── compose + convergence (§2.1 / §10 收敛性) ──

#[test]
fn content_edit_plus_rename_compose_and_both_directions_converge() {
    let env = setup();
    let (a, b) = seeded_pair(&env);

    // A edits content, B renames the directory.
    a.activate();
    a.write_skill("skill-1", "alpha", "edited on A");
    a.commit("edit content");
    let x_sha = git(&a.skills, &["rev-parse", "HEAD"]);

    b.activate();
    std::fs::rename(b.skills.join("alpha"), b.skills.join("beta")).unwrap();
    b.write_skill("skill-1", "beta", "base content");
    b.commit("rename skill");
    b.push(); // remote main = B's rename

    let summary_a = a.pull(); // A merges (ours = content edit, theirs = rename)
    assert!(!summary_a.up_to_date);
    assert!(summary_a.new_conflicts.is_empty(), "{summary_a:?}");
    // Composition: renamed path with A's content.
    assert_eq!(a.skill_md("beta"), "edited on A");
    assert!(!a.skills.join("alpha").exists());
    let meta = a.meta_of("skill-1").unwrap();
    assert_eq!(meta.path, "beta");
    assert_eq!(meta.path_key, sync_metadata::path_key("beta"));
    assert_eq!(summary_a.updated.len(), 1);
    assert_eq!(summary_a.updated[0].from_device, "Device B");

    // Expose A's pre-merge tip as the remote main so B merges the exact
    // mirrored pair (ours = rename, theirs = content edit).
    git(&a.skills, &["push", "-f", "origin", &format!("{x_sha}:refs/heads/main")]);
    let summary_b = b.pull();
    assert!(summary_b.new_conflicts.is_empty(), "{summary_b:?}");
    assert_eq!(b.skill_md("beta"), "edited on A");

    // Tree-OID convergence (§10): both devices merged the same pair of
    // commits from opposite viewpoints and must land on the same tree.
    assert_eq!(a.tree_oid(), b.tree_oid());
}

// ── true conflict → pending → cross-device visibility → resolutions ──

#[test]
fn true_conflict_keeps_ours_declares_trailer_and_pins_theirs() {
    let env = setup();
    let (a, b) = seeded_pair(&env);

    b.activate();
    b.write_skill("skill-1", "alpha", "edited on B");
    b.commit("edit on B");
    b.push();

    a.activate();
    a.write_skill("skill-1", "alpha", "edited on A");
    a.commit("edit on A");
    let summary = a.pull();

    // Ours kept, conflict declared, nothing blocked.
    assert_eq!(a.skill_md("alpha"), "edited on A");
    assert_eq!(summary.new_conflicts, vec!["skill-1"]);
    assert_eq!(summary.kept_local, vec!["skill-1"]);
    assert_eq!(summary.pending_total, 1);
    let head_msg = a.head_message();
    assert!(head_msg.contains("Skills-Manager-Conflicts: skill-1"), "{head_msg}");
    assert!(protocol::has_protocol_trailer(&head_msg));

    // The theirs version is pinned via the durable ref and the projection.
    let repo = git2::Repository::open(&a.skills).unwrap();
    let pinned = pending::ref_target(&repo, &conflict_ref("skill-1")).unwrap();
    let rows = a.store.list_pending_conflicts().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].theirs_commit, pinned.to_string());
    assert_eq!(rows[0].theirs_path.as_deref(), Some("alpha"));

    // A second pull is idempotent: nothing new, still one pending.
    a.push();
    let summary2 = a.pull();
    assert!(summary2.up_to_date);
    assert_eq!(summary2.pending_total, 1);

    // B pulls A's merge (a fast-forward): adopts the kept version and sees
    // the same pending, with the conflict ref rebuilt from the declaring
    // commit's second parent (clone-safe: refs never travel over push/pull).
    let summary_b = b.pull();
    assert!(summary_b.fast_forward);
    assert_eq!(b.skill_md("alpha"), "edited on A");
    assert_eq!(summary_b.pending_total, 1);
    let repo_b = git2::Repository::open(&b.skills).unwrap();
    let pinned_b = pending::ref_target(&repo_b, &conflict_ref("skill-1")).unwrap();
    let theirs_tree = repo_b.find_commit(pinned_b).unwrap().tree().unwrap();
    let entry = theirs_tree.get_path(Path::new("alpha/SKILL.md")).unwrap();
    let blob = repo_b.find_blob(entry.id()).unwrap();
    assert_eq!(blob.content(), b"edited on B");
}

#[test]
fn resolve_keep_local_closes_pending_across_devices() {
    let env = setup();
    let (a, b) = seeded_pair(&env);
    b.activate();
    b.write_skill("skill-1", "alpha", "edited on B");
    b.commit("edit on B");
    b.push();
    a.activate();
    a.write_skill("skill-1", "alpha", "edited on A");
    a.commit("edit on A");
    a.pull();

    a.activate();
    let safety = resolve_conflict_unlocked(&a.store, &a.skills, "skill-1", ResolveAction::KeepLocal)
        .unwrap();
    assert!(safety.starts_with("sm-v-"));
    assert_eq!(a.skill_md("alpha"), "edited on A");
    assert!(a.head_message().contains("Skills-Manager-Resolved: skill-1"));
    assert!(a.store.list_pending_conflicts().unwrap().is_empty());
    let repo = git2::Repository::open(&a.skills).unwrap();
    assert!(pending::ref_target(&repo, &conflict_ref("skill-1")).is_none());

    // B follows: fast-forward onto the resolution, pending cleared there too.
    a.push();
    let summary_b = b.pull();
    assert_eq!(summary_b.pending_total, 0);
    assert_eq!(b.skill_md("alpha"), "edited on A");
    assert!(b.store.list_pending_conflicts().unwrap().is_empty());
}

#[test]
fn resolve_use_remote_adopts_pinned_version() {
    let env = setup();
    let (a, b) = seeded_pair(&env);
    b.activate();
    b.write_skill("skill-1", "alpha", "edited on B");
    b.commit("edit on B");
    b.push();
    a.activate();
    a.write_skill("skill-1", "alpha", "edited on A");
    a.commit("edit on A");
    a.pull();

    a.activate();
    resolve_conflict_unlocked(&a.store, &a.skills, "skill-1", ResolveAction::UseRemote).unwrap();
    assert_eq!(a.skill_md("alpha"), "edited on B");
    let meta = a.meta_of("skill-1").unwrap();
    assert_eq!(meta.path, "alpha");
    assert!(a.store.list_pending_conflicts().unwrap().is_empty());
    // Clean tree: everything was committed by the resolution.
    assert!(!git_backup::has_uncommitted_changes(&a.skills).unwrap());
}

#[test]
fn resolve_use_remote_avoids_existing_ignored_target_path() {
    let env = setup();
    let (a, b) = seeded_pair(&env);

    // Remote side renames the skill to beta and edits content; local side
    // edits alpha. The content conflict keeps local alpha and pins remote beta.
    b.activate();
    std::fs::rename(b.skills.join("alpha"), b.skills.join("beta")).unwrap();
    b.write_skill("skill-1", "beta", "edited on B");
    b.commit("rename and edit on B");
    b.push();

    a.activate();
    a.write_skill("skill-1", "alpha", "edited on A");
    a.commit("edit on A");
    let summary = a.pull();
    assert_eq!(summary.new_conflicts, vec!["skill-1"]);
    assert_eq!(a.skill_md("alpha"), "edited on A");

    // A local ignored directory already occupies beta. UseRemote must not
    // delete alpha and then fail moving into beta; it should choose beta (2).
    std::fs::write(a.skills.join(".git/info/exclude"), "beta/\n").unwrap();
    std::fs::create_dir_all(a.skills.join("beta")).unwrap();
    std::fs::write(a.skills.join("beta/local.log"), "local ignored data").unwrap();

    resolve_conflict_unlocked(&a.store, &a.skills, "skill-1", ResolveAction::UseRemote).unwrap();

    assert_eq!(
        std::fs::read_to_string(a.skills.join("beta/local.log")).unwrap(),
        "local ignored data"
    );
    assert!(!a.skills.join("alpha").exists());
    assert_eq!(
        std::fs::read_to_string(a.skills.join("beta (2)/SKILL.md")).unwrap(),
        "edited on B"
    );
    let meta = a.meta_of("skill-1").unwrap();
    assert_eq!(meta.path, "beta (2)");
    assert!(a.store.list_pending_conflicts().unwrap().is_empty());
}

#[test]
fn resolve_keep_both_duplicates_under_device_suffix() {
    let env = setup();
    let (a, b) = seeded_pair(&env);
    b.activate();
    b.write_skill("skill-1", "alpha", "edited on B");
    b.commit("edit on B");
    b.push();
    a.activate();
    a.write_skill("skill-1", "alpha", "edited on A");
    a.commit("edit on A");
    a.pull();

    a.activate();
    resolve_conflict_unlocked(&a.store, &a.skills, "skill-1", ResolveAction::KeepBoth).unwrap();
    // Local version untouched; theirs extracted next to it with a new id.
    assert_eq!(a.skill_md("alpha"), "edited on A");
    let dup = a.skills.join("alpha (Device B)");
    assert!(dup.is_dir(), "expected duplicate dir at {}", dup.display());
    assert_eq!(
        std::fs::read_to_string(dup.join("SKILL.md")).unwrap(),
        "edited on B"
    );
    // The duplicate has its own metadata with a fresh id.
    let meta_dir = a.skills.join(".skills-manager/skills");
    let metas: Vec<SkillMetaFile> = std::fs::read_dir(&meta_dir)
        .unwrap()
        .flatten()
        .filter_map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok())
        .collect();
    assert_eq!(metas.len(), 2);
    let dup_meta = metas.iter().find(|m| m.skill_id != "skill-1").unwrap();
    assert_eq!(dup_meta.path, "alpha (Device B)");
    assert!(a.store.list_pending_conflicts().unwrap().is_empty());
    assert!(!git_backup::has_uncommitted_changes(&a.skills).unwrap());
}

// ── deletion propagation ──

#[test]
fn clean_deletion_propagates_and_delete_vs_edit_conflicts() {
    let env = setup();
    let a = env.device_a();
    a.write_skill("del-clean", "clean", "v1");
    a.write_skill("del-edit", "edited", "v1");
    a.commit("seed");
    a.push();
    let b = env.device_b();

    // B deletes both skills and pushes.
    b.activate();
    b.remove_skill("del-clean", "clean");
    b.remove_skill("del-edit", "edited");
    b.commit("delete both");
    b.push();

    // A edits one of them before pulling.
    a.activate();
    a.write_skill("del-edit", "edited", "v2 on A");
    a.commit("edit");
    let summary = a.pull();

    // Untouched deletion propagates; edited one conflicts and stays.
    assert!(!a.skills.join("clean").exists());
    assert!(a.meta_of("del-clean").is_none());
    assert_eq!(a.skill_md("edited"), "v2 on A");
    assert_eq!(summary.new_conflicts, vec!["del-edit"]);
}

// ── ff guard (§4 ff 防护) ──

#[test]
fn ff_guard_blocks_when_remote_touches_pending_skill() {
    let env = setup();
    let (a, b) = seeded_pair(&env);

    // Build a pending conflict on A, push the declaring merge.
    b.activate();
    b.write_skill("skill-1", "alpha", "edited on B");
    b.commit("edit on B");
    b.push();
    a.activate();
    a.write_skill("skill-1", "alpha", "edited on A");
    a.commit("edit on A");
    a.pull();
    a.push();

    // B follows (adopts declaration), then edits the pending skill again and
    // adds an unrelated skill.
    b.activate();
    b.pull();
    b.write_skill("skill-1", "alpha", "edited on B again");
    b.write_skill("skill-2", "gamma", "new skill");
    b.commit("touch pending + unrelated");
    b.push();

    // A is exactly at the declaring merge (base == ours). A plain ff would
    // silently adopt B's re-edit of the pinned skill — the guard forces a
    // full merge that keeps the pinned local version.
    a.activate();
    let summary = a.pull();
    assert!(!summary.fast_forward, "ff must be blocked");
    assert_eq!(a.skill_md("alpha"), "edited on A");
    assert_eq!(a.skill_md("gamma"), "new skill");
    assert_eq!(summary.pending_total, 1);
    // The theirs pointer advanced to B's new tip.
    let repo = git2::Repository::open(&a.skills).unwrap();
    let pinned = pending::ref_target(&repo, &conflict_ref("skill-1")).unwrap();
    let tree = repo.find_commit(pinned).unwrap().tree().unwrap();
    let entry = tree.get_path(Path::new("alpha/SKILL.md")).unwrap();
    assert_eq!(
        repo.find_blob(entry.id()).unwrap().content(),
        b"edited on B again"
    );
}

// ── R3 反例: pending placeholder derived from theirs trailers only ──

#[test]
fn pending_placeholder_from_theirs_trailer_wins_path_collision() {
    let env = setup();
    let a = env.device_a();
    a.write_skill("skill-1", "spot", "S1 base");
    a.write_skill("skill-2", "other", "S2 base");
    a.commit("seed");
    a.push();
    let b = env.device_b();

    // A and B diverge on skill-1 → A merges → pending declared, S1 pinned at
    // "spot" in A's tree; A pushes the declaring merge M1.
    b.activate();
    b.write_skill("skill-1", "spot", "S1 on B");
    b.commit("S1 on B");
    b.push();
    a.activate();
    a.write_skill("skill-1", "spot", "S1 on A");
    a.commit("S1 on A");
    a.pull();
    a.push();

    // B, offline and with NO local pending state (its projection knows
    // nothing about M1), deletes its S1 and migrates skill-2 onto the freed
    // key "spot", then pulls M1. (Case-variant folding of the same key is
    // covered by the decision unit tests; a case-only directory rename via
    // plain fs calls would trip git's ignorecase index behavior on macOS and
    // test git, not the merge.)
    b.activate();
    b.remove_skill("skill-1", "spot");
    std::fs::rename(b.skills.join("other"), b.skills.join("spot")).unwrap();
    b.write_skill("skill-2", "spot", "S2 base");
    b.commit("migrate S2 onto spot");
    let summary = b.pull();

    // From B's perspective the declaration lives only in theirs history —
    // the trailer alone makes S1 the immovable placeholder (pinned to the
    // declaring side's version); the migrant S2 yields (§3 / R3 反例).
    let s1 = b.meta_of("skill-1").unwrap();
    let s2 = b.meta_of("skill-2").unwrap();
    assert_eq!(s1.path, "spot");
    assert_eq!(b.skill_md("spot"), "S1 on A");
    assert_eq!(s2.path, "spot (2)");
    assert_eq!(
        std::fs::read_to_string(b.skills.join("spot (2)/SKILL.md")).unwrap(),
        "S2 base"
    );
    assert_eq!(summary.pending_total, 1);
}

#[test]
fn remote_deletion_leaves_local_ignored_files_untouched() {
    // codex review finding 3 (empirical half): a plain remote deletion of a
    // skill dir must not take locally-ignored files inside it with it — the
    // FORCE checkout only removes tracked paths. (The dir→file typechange
    // case is separately blocked by the added-path pre-check.)
    let env = setup();
    let (a, b) = seeded_pair(&env);

    b.activate();
    b.remove_skill("skill-1", "alpha");
    b.commit("delete alpha");
    b.push();

    a.activate();
    a.write_skill("skill-2", "beta", "diverge so this is a real merge");
    a.commit("local");
    std::fs::write(a.skills.join(".git/info/exclude"), "debug.log\n").unwrap();
    std::fs::write(a.skills.join("alpha/debug.log"), "precious ignored data").unwrap();

    let summary = a.pull();
    assert!(summary.new_conflicts.is_empty(), "{summary:?}");
    // The tracked skill is gone…
    assert!(!a.skills.join("alpha/SKILL.md").exists());
    assert!(a.meta_of("skill-1").is_none());
    // …but the ignored file inside it survived the checkout.
    assert_eq!(
        std::fs::read_to_string(a.skills.join("alpha/debug.log")).unwrap(),
        "precious ignored data"
    );
}

#[test]
fn legacy_dirt_does_not_brick_the_merge() {
    // Real-world incident (2026-07-05, Linus-PC): shared history carried two
    // unclaimed skill dirs (committed by old versions with no metadata) and
    // the remote tip carried a committed atomic-write temp file under the
    // metadata namespace — every merge aborted with "zero changes" forever.
    let env = setup();
    let a = env.device_a();
    a.write_skill("skill-1", "alpha", "base");
    // Legacy dirt in the SEED (shared history): an unclaimed skill dir.
    std::fs::create_dir_all(a.skills.join("ghost")).unwrap();
    std::fs::write(a.skills.join("ghost/SKILL.md"), "no metadata owns me").unwrap();
    a.commit("seed with ghost");
    a.push();
    let b = env.device_b();

    // B (the remote side) additionally has a temp metadata leftover already
    // COMMITTED in history (as the incident device did, with the protocol
    // trailer) — raw git here, because the app's own commit paths now clean
    // temp files before committing and would defeat the scenario.
    b.activate();
    std::fs::write(
        b.skills.join(".skills-manager/skills/skill-1.json.tmp.0000-dead-beef"),
        "partial write leftover",
    )
    .unwrap();
    std::fs::write(b.skills.join("alpha/SKILL.md"), "edited on B").unwrap();
    git(&b.skills, &["add", "-A"]);
    git(&b.skills, &[
        "commit",
        "-m",
        "backup: edit + leaked tmp file\n\nSkills-Manager-Protocol: 2",
    ]);
    b.reindex();
    b.push();
    // Preconditions: the junk really is in the remote history.
    assert!(
        git(&b.skills, &["ls-tree", "-r", "HEAD", "--name-only"]).contains(".tmp."),
        "setup: tmp file must be committed"
    );

    // A diverges so this is a real merge, then pulls.
    a.activate();
    a.write_skill("skill-2", "beta", "local addition");
    a.commit("local");
    let summary = a.pull();
    assert!(summary.new_conflicts.is_empty(), "{summary:?}");

    // The ghost dir is tolerated (grandfathered) and still present…
    assert_eq!(a.skill_md("ghost"), "no metadata owns me");
    // …while the temp metadata file was dropped from the merged tree.
    let tracked = git(&a.skills, &["ls-tree", "-r", "HEAD", "--name-only"]);
    assert!(
        !tracked.contains(".tmp."),
        "temp metadata junk must not survive the merge: {tracked}"
    );
    assert_eq!(a.skill_md("alpha"), "edited on B");
}

#[test]
fn commit_never_picks_up_tmp_metadata_leftovers() {
    // The pushing-only device (never reconciles) must not commit atomic-write
    // leftovers in the first place.
    let env = setup();
    let a = env.device_a();
    a.write_skill("skill-1", "alpha", "base");
    std::fs::write(
        a.skills.join(".skills-manager/skills/skill-1.json.tmp.0000-dead-beef"),
        "leftover",
    )
    .unwrap();
    a.commit("seed");
    let tracked = git(&a.skills, &["ls-tree", "-r", "HEAD", "--name-only"]);
    assert!(!tracked.contains(".tmp."), "{tracked}");
    // The leftover is gone from disk too, not just unstaged.
    assert!(!a
        .skills
        .join(".skills-manager/skills/skill-1.json.tmp.0000-dead-beef")
        .exists());
}

// ── crash recovery (§5 启动恢复协议) ──

/// Builds a repo state crashed between branch-move (step 9) and checkout
/// (step 10): HEAD points at a commit whose tree is not in the working
/// tree, with applying/pre-merge refs still set.
fn crash_between_ref_move_and_checkout(dev: &Device) -> (git2::Oid, git2::Oid) {
    dev.activate();
    let repo = git2::Repository::open(&dev.skills).unwrap();
    let old_head = repo.head().unwrap().target().unwrap();

    // Simulated merge result: a commit adding a file, created without
    // touching the working tree.
    let old_commit = repo.find_commit(old_head).unwrap();
    let blob = repo.blob(b"from the interrupted merge").unwrap();
    let mut edits = std::collections::BTreeMap::new();
    edits.insert(
        "incoming/SKILL.md".to_string(),
        super::treebuild::TreeEdit::PutBlob { oid: blob, mode: 0o100644 },
    );
    let tree_oid =
        super::treebuild::apply_tree_edits(&repo, Some(&old_commit.tree().unwrap()), &edits)
            .unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let sig = git2::Signature::now("Device A", "a@test").unwrap();
    let merge_commit = repo
        .commit(
            None,
            &sig,
            &sig,
            &protocol::app_commit_message("sync: merge remote skill changes (1 updated, 0 kept local, 0 conflicts)"),
            &tree,
            &[&old_commit],
        )
        .unwrap();

    pending::write_ref(&repo, REF_PRE_MERGE, old_head, "test").unwrap();
    pending::write_ref(&repo, REF_APPLYING, merge_commit, "test").unwrap();
    repo.reference("refs/heads/main", merge_commit, true, "test").unwrap();
    (old_head, merge_commit)
}

#[test]
fn recovery_replays_checkout_after_crash() {
    let env = setup();
    let a = env.device_a();
    a.write_skill("skill-1", "alpha", "base");
    a.commit("seed");
    let (_old, merge_commit) = crash_between_ref_move_and_checkout(&a);

    // Working tree does not yet have the merged file.
    assert!(!a.skills.join("incoming/SKILL.md").exists());
    recover_on_startup(&a.store, &a.skills);

    assert!(a.skills.join("incoming/SKILL.md").exists());
    let repo = git2::Repository::open(&a.skills).unwrap();
    assert!(pending::ref_target(&repo, REF_APPLYING).is_none());
    assert_eq!(repo.head().unwrap().target().unwrap(), merge_commit);
    assert!(!git_backup::has_uncommitted_changes(&a.skills).unwrap());
}

#[test]
fn recovery_rescues_user_edits_made_after_crash() {
    let env = setup();
    let a = env.device_a();
    a.write_skill("skill-1", "alpha", "base");
    a.commit("seed");
    let _ = crash_between_ref_move_and_checkout(&a);

    // The user edits a tracked file after the crash, before restart.
    std::fs::write(a.skills.join("alpha/SKILL.md"), "user edit during crash").unwrap();
    recover_on_startup(&a.store, &a.skills);

    // Replay completed…
    assert!(a.skills.join("incoming/SKILL.md").exists());
    // …and the user edit survives in a rescue snapshot tag.
    let tags = git(&a.skills, &["tag", "--list", "sm-v-*"]);
    assert!(!tags.is_empty(), "rescue tag expected");
    let tag = tags.lines().last().unwrap();
    let rescued = git(&a.skills, &["show", &format!("{tag}:alpha/SKILL.md")]);
    assert_eq!(rescued, "user edit during crash");
}

#[test]
fn recovery_settles_partial_rollback_debris_back_onto_head() {
    // codex review finding 1: crash after the branch ref was rolled back to
    // old HEAD but before the working tree was restored — recovery must not
    // just delete the markers and leave half-merged debris to be committed
    // as ordinary edits.
    let env = setup();
    let a = env.device_a();
    a.write_skill("skill-1", "alpha", "base");
    a.commit("seed");

    a.activate();
    let repo = git2::Repository::open(&a.skills).unwrap();
    let head = repo.head().unwrap().target().unwrap();
    pending::write_ref(&repo, REF_PRE_MERGE, head, "test").unwrap();
    pending::write_ref(&repo, REF_APPLYING, head, "test").unwrap();
    drop(repo);
    // Debris from a partially applied (then crashed) checkout.
    std::fs::write(a.skills.join("alpha/SKILL.md"), "half-merged debris").unwrap();
    std::fs::write(a.skills.join("stray-from-merge.md"), "debris").unwrap();

    recover_on_startup(&a.store, &a.skills);

    // Working tree settled back to HEAD…
    assert_eq!(a.skill_md("alpha"), "base");
    assert!(!a.skills.join("stray-from-merge.md").exists());
    assert!(!git_backup::has_uncommitted_changes(&a.skills).unwrap());
    // …markers gone, debris preserved in a rescue snapshot.
    let repo = git2::Repository::open(&a.skills).unwrap();
    assert!(pending::ref_target(&repo, REF_APPLYING).is_none());
    let tags = git(&a.skills, &["tag", "--list", "sm-v-*"]);
    let tag = tags.lines().last().expect("rescue tag expected");
    assert_eq!(
        git(&a.skills, &["show", &format!("{tag}:alpha/SKILL.md")]),
        "half-merged debris"
    );
}

#[test]
fn heal_rewrites_stale_conflict_ref_after_re_declaration() {
    // codex review finding 2: a conflict resolved and then re-declared while
    // this device was offline must not keep the pre-resolution pointer —
    // "use remote" would apply the outdated version.
    let env = setup();
    let a = env.device_a();
    a.write_skill("skill-1", "alpha", "base");
    a.commit("seed");
    a.activate();

    // History: declaration M1 (theirs parent P1) → resolution → new
    // declaration M2 (theirs parent P2), crafted with raw merges carrying
    // the app trailers.
    let mk_side = |name: &str, content: &str| {
        git(&a.skills, &["checkout", "-b", name]);
        std::fs::write(a.skills.join("alpha/SKILL.md"), content).unwrap();
        git(&a.skills, &["commit", "-am", &format!("side {name}")]);
        let sha = git(&a.skills, &["rev-parse", "HEAD"]);
        git(&a.skills, &["checkout", "main"]);
        sha
    };
    let p1 = mk_side("side1", "theirs v1");
    git(&a.skills, &[
        "merge", "--no-ff", "-s", "ours",
        "-m", "sync: merge\n\nSkills-Manager-Protocol: 2\nSkills-Manager-Conflicts: skill-1",
        "side1",
    ]);
    git(&a.skills, &[
        "commit", "--allow-empty",
        "-m", "resolve\n\nSkills-Manager-Protocol: 2\nSkills-Manager-Resolved: skill-1",
    ]);
    let p2 = mk_side("side2", "theirs v2");
    git(&a.skills, &[
        "merge", "--no-ff", "-s", "ours",
        "-m", "sync: merge\n\nSkills-Manager-Protocol: 2\nSkills-Manager-Conflicts: skill-1",
        "side2",
    ]);

    let repo = git2::Repository::open(&a.skills).unwrap();
    let head = repo.head().unwrap().target().unwrap();
    // Stale ref from the first (since resolved) declaration.
    pending::write_ref(
        &repo,
        &conflict_ref("skill-1"),
        git2::Oid::from_str(&p1).unwrap(),
        "stale",
    )
    .unwrap();

    let healed = pending::heal_conflict_refs(&repo, head).unwrap();
    assert_eq!(healed.len(), 1);
    let target = pending::ref_target(&repo, &conflict_ref("skill-1")).unwrap();
    assert_eq!(target.to_string(), p2, "ref must point at the current declaration's theirs side");

    // A legitimately advanced pointer (descendant of P2) is left alone.
    let healed_again = pending::heal_conflict_refs(&repo, head).unwrap();
    assert!(healed_again.is_empty());
}

#[test]
fn recovery_before_ref_move_treats_merge_as_not_happened() {
    let env = setup();
    let a = env.device_a();
    a.write_skill("skill-1", "alpha", "base");
    a.commit("seed");

    a.activate();
    let repo = git2::Repository::open(&a.skills).unwrap();
    let head = repo.head().unwrap().target().unwrap();
    // Crash between 7' and 9: applying exists, branch never moved.
    pending::write_ref(&repo, REF_PRE_MERGE, head, "test").unwrap();
    pending::write_ref(&repo, REF_APPLYING, head, "test").unwrap();
    pending::write_ref(
        &repo,
        &pending::staging_ref("ghost-attempt", "skill-x"),
        head,
        "test",
    )
    .unwrap();
    drop(repo);

    recover_on_startup(&a.store, &a.skills);
    let repo = git2::Repository::open(&a.skills).unwrap();
    assert!(pending::ref_target(&repo, REF_APPLYING).is_none());
    assert!(pending::ref_target(&repo, REF_PRE_MERGE).is_none());
    assert!(pending::list_staging_refs(&repo).is_empty());
    assert_eq!(repo.head().unwrap().target().unwrap(), head);
}

// ── old-client detection (§6) ──

#[test]
fn old_client_line_merge_blocks_object_merge() {
    let env = setup();
    let (a, b) = seeded_pair(&env);

    // B: raw-git commits + a raw line-level merge (double parent, tree has
    // protocol.json, no trailer) — the old-client signature.
    b.activate();
    git(&b.skills, &["checkout", "-b", "side"]);
    std::fs::write(b.skills.join("alpha/SKILL.md"), "side edit").unwrap();
    git(&b.skills, &["commit", "-am", "side edit"]);
    git(&b.skills, &["checkout", "main"]);
    std::fs::write(b.skills.join("alpha/extra.md"), "main edit").unwrap();
    git(&b.skills, &["add", "-A"]);
    git(&b.skills, &["commit", "-m", "main edit"]);
    git(&b.skills, &["merge", "--no-ff", "-m", "old client merge", "side"]);
    git(&b.skills, &["push", "origin", "main"]);

    a.activate();
    a.write_skill("skill-1", "alpha", "local divergence");
    a.commit("local edit");
    let err = a.pull_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("line-level merge"), "unexpected error: {msg}");
}

#[test]
fn old_client_plain_write_passes_with_warning() {
    let env = setup();
    let (a, b) = seeded_pair(&env);

    // B: a raw single-parent commit editing skill content — tolerated after
    // validation, but flagged. It also carries a committed temp metadata
    // leftover: the input-tip validation must treat that as legacy dirt, not
    // block (codex review finding: the tip check ran before the plan-stage
    // junk drop and hard-failed on it).
    b.activate();
    std::fs::write(b.skills.join("alpha/SKILL.md"), "raw git edit").unwrap();
    std::fs::write(
        b.skills.join(".skills-manager/skills/skill-1.json.tmp.0000-dead-beef"),
        "leftover",
    )
    .unwrap();
    git(&b.skills, &["add", "-A"]);
    git(&b.skills, &["commit", "-m", "raw edit without trailer"]);
    git(&b.skills, &["push", "origin", "main"]);

    a.activate();
    a.write_skill("skill-2", "beta", "unrelated local addition");
    a.commit("local addition");
    let summary = a.pull();
    assert!(summary.old_client_warning.is_some(), "{summary:?}");
    assert_eq!(a.skill_md("alpha"), "raw git edit");
    // The committed junk was dropped from the merged tree, not adopted.
    let tracked = git(&a.skills, &["ls-tree", "-r", "HEAD", "--name-only"]);
    assert!(!tracked.contains(".tmp."), "{tracked}");
}

#[test]
fn old_client_conflict_markers_block() {
    let env = setup();
    let (a, b) = seeded_pair(&env);

    b.activate();
    std::fs::write(
        b.skills.join("alpha/SKILL.md"),
        "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> other\n",
    )
    .unwrap();
    git(&b.skills, &["commit", "-am", "botched manual merge"]);
    git(&b.skills, &["push", "origin", "main"]);

    a.activate();
    a.write_skill("skill-2", "beta", "unrelated");
    a.commit("local");
    let err = a.pull_err();
    assert!(format!("{err:#}").contains("conflict markers"));
}

// ── legacy fallback (§6 legacy) ──

#[test]
fn legacy_remote_without_protocol_falls_back_to_system_merge() {
    let env = setup();
    let a = env.device_a();
    a.write_skill("skill-1", "alpha", "base");
    a.commit("seed");
    a.push();
    let b = env.device_b();

    // B strips protocol.json entirely (a repo written only by pre-3d
    // clients) and adds a change.
    b.activate();
    std::fs::remove_file(b.skills.join(".skills-manager/protocol.json")).unwrap();
    std::fs::write(b.skills.join("legacy-note.md"), "from legacy client").unwrap();
    git(&b.skills, &["add", "-A"]);
    git(&b.skills, &["commit", "-m", "legacy write"]);
    git(&b.skills, &["push", "origin", "main"]);

    // Diverge A so the fallback produces a real merge commit.
    a.activate();
    a.write_skill("skill-2", "beta", "local divergence");
    a.commit("local");
    let summary = a.pull();
    assert!(summary.legacy_fallback, "{summary:?}");
    assert_eq!(summary.engine, "system");
    assert_eq!(
        std::fs::read_to_string(a.skills.join("legacy-note.md")).unwrap(),
        "from legacy client"
    );
    // codex review finding 4: the app's own line merge must carry the
    // protocol trailer, or other devices would block it as an old-client
    // double-parent violation (§6).
    let head_msg = a.head_message();
    assert!(protocol::has_protocol_trailer(&head_msg), "{head_msg}");
    assert!(
        git(&a.skills, &["rev-list", "--parents", "-1", "HEAD"]).split_whitespace().count() >= 3,
        "fallback should have produced a merge commit"
    );
}

// ── ignored-file checkout blocker (§5 忽略文件注) ──

#[test]
fn ignored_file_in_the_way_blocks_with_guidance() {
    let env = setup();
    let (a, b) = seeded_pair(&env);

    b.activate();
    b.write_skill("skill-2", "beta", "incoming skill");
    std::fs::write(b.skills.join("beta/debug.log"), "from B").unwrap();
    b.commit("add beta");
    b.push();

    // A ignores the path locally (repo-local exclude, not in the tree) and
    // has its own data exactly where the incoming skill lands. P1/P2 skip
    // ignored files, so only the checkout pre-check can protect it. Diverge
    // A so the pull is a real merge, not a fast-forward.
    a.activate();
    a.write_skill("skill-3", "gamma", "local skill");
    a.commit("local");
    std::fs::write(a.skills.join(".git/info/exclude"), "beta/\n").unwrap();
    std::fs::create_dir_all(a.skills.join("beta")).unwrap();
    std::fs::write(a.skills.join("beta/debug.log"), "precious local data").unwrap();

    let head_before = git(&a.skills, &["rev-parse", "HEAD"]);
    let err = a.pull_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("in the way"), "unexpected: {msg}");
    // Nothing changed: HEAD intact, local file intact — never auto-FORCEd.
    assert_eq!(git(&a.skills, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(
        std::fs::read_to_string(a.skills.join("beta/debug.log")).unwrap(),
        "precious local data"
    );
}
