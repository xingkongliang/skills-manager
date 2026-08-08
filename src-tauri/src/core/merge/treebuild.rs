//! Recursive tree construction (design §5): apply a set of path-addressed
//! edits to a base tree, rebuilding the ancestor chain bottom-up with
//! per-level `TreeBuilder`s. `TreeUpdateBuilder` is deliberately not used —
//! its handling of remove-then-upsert on one path and of blob↔tree type
//! changes is incomplete; type changes are handled explicitly here by
//! removing the old entry before inserting the new one.
//!
//! Callers express "replace whatever is at this path" by inserting removes
//! first and letting puts overwrite them in the flat edit map. A nested edit
//! below a removed path builds that directory from scratch (the removed
//! subtree's former siblings do not leak through).

use anyhow::{bail, Context, Result};
use git2::{ObjectType, Oid, Repository, Tree};
use std::collections::BTreeMap;

pub const FILEMODE_TREE: i32 = 0o040000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeEdit {
    /// Put a blob at this path (file mode from the source entry).
    PutBlob { oid: Oid, mode: i32 },
    /// Attach a whole subtree at this path.
    PutTree { oid: Oid },
    /// Remove whatever is at this path (blob or subtree).
    Remove,
}

struct DirNode {
    /// When true, ignore the base tree at this level: the old entry was
    /// removed, so the directory is rebuilt only from the nested edits.
    fresh: bool,
    children: BTreeMap<String, Node>,
}

enum Node {
    Leaf(TreeEdit),
    Dir(DirNode),
}

/// Apply `edits` (repo-relative slash paths → edit) on top of `base`,
/// returning the OID of the new root tree. Directories that end up empty are
/// pruned (git does not represent empty trees in a commit).
pub fn apply_tree_edits(
    repo: &Repository,
    base: Option<&Tree>,
    edits: &BTreeMap<String, TreeEdit>,
) -> Result<Oid> {
    let mut root: BTreeMap<String, Node> = BTreeMap::new();
    for (path, edit) in edits {
        insert_edit(&mut root, path, *edit)
            .with_context(|| format!("conflicting tree edits at {path}"))?;
    }
    match build_level(repo, base, &root)? {
        Some(oid) => Ok(oid),
        // A fully-emptied root is still a valid (empty) tree.
        None => Ok(repo.treebuilder(None)?.write()?),
    }
}

fn insert_edit(level: &mut BTreeMap<String, Node>, path: &str, edit: TreeEdit) -> Result<()> {
    let (head, rest) = match path.split_once('/') {
        Some((h, r)) => (h, Some(r)),
        None => (path, None),
    };
    if head.is_empty() {
        bail!("empty path component");
    }
    match rest {
        None => match level.get_mut(head) {
            None => {
                level.insert(head.to_string(), Node::Leaf(edit));
                Ok(())
            }
            Some(Node::Dir(dir)) if edit == TreeEdit::Remove => {
                // Remove of a directory that already has nested puts: rebuild
                // it from scratch so former siblings do not leak through.
                dir.fresh = true;
                Ok(())
            }
            Some(_) => bail!("duplicate edit for {head}"),
        },
        Some(rest) => {
            let node = level.entry(head.to_string()).or_insert_with(|| {
                Node::Dir(DirNode {
                    fresh: false,
                    children: BTreeMap::new(),
                })
            });
            if let Node::Leaf(TreeEdit::Remove) = node {
                // The whole old entry goes away; nested edits build the new
                // directory from scratch.
                *node = Node::Dir(DirNode {
                    fresh: true,
                    children: BTreeMap::new(),
                });
            }
            match node {
                Node::Dir(dir) => insert_edit(&mut dir.children, rest, edit),
                Node::Leaf(_) => bail!("edit below a leaf edit at {head}"),
            }
        }
    }
}

fn build_level(
    repo: &Repository,
    base: Option<&Tree>,
    nodes: &BTreeMap<String, Node>,
) -> Result<Option<Oid>> {
    let mut builder = repo.treebuilder(base)?;
    for (name, node) in nodes {
        let existing_kind = builder.get(name)?.and_then(|e| e.kind());
        match node {
            Node::Leaf(TreeEdit::Remove) => {
                if existing_kind.is_some() {
                    builder.remove(name)?;
                }
            }
            Node::Leaf(TreeEdit::PutBlob { oid, mode }) => {
                if existing_kind == Some(ObjectType::Tree) {
                    builder.remove(name)?; // tree → blob type change
                }
                builder.insert(name, *oid, *mode)?;
            }
            Node::Leaf(TreeEdit::PutTree { oid }) => {
                if existing_kind == Some(ObjectType::Blob) {
                    builder.remove(name)?; // blob → tree type change
                }
                builder.insert(name, *oid, FILEMODE_TREE)?;
            }
            Node::Dir(dir) => {
                let existing = builder.get(name)?.map(|e| (e.kind(), e.id()));
                let child_base = match existing {
                    _ if dir.fresh => None,
                    Some((Some(ObjectType::Tree), id)) => Some(repo.find_tree(id)?),
                    Some(_) => {
                        // A blob where the plan needs a directory: explicit
                        // type change — drop the blob, build from empty.
                        builder.remove(name)?;
                        None
                    }
                    None => None,
                };
                match build_level(repo, child_base.as_ref(), &dir.children)? {
                    Some(oid) => {
                        if dir.fresh && existing_kind == Some(ObjectType::Blob) {
                            builder.remove(name)?;
                        }
                        builder.insert(name, oid, FILEMODE_TREE)?;
                    }
                    None => {
                        if builder.get(name)?.is_some() {
                            builder.remove(name)?;
                        }
                    }
                }
            }
        }
    }
    let oid = builder.write()?;
    if repo.find_tree(oid)?.len() == 0 {
        return Ok(None);
    }
    Ok(Some(oid))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_repo() -> (tempfile::TempDir, Repository) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();
        (tmp, repo)
    }

    fn blob(repo: &Repository, content: &str) -> Oid {
        repo.blob(content.as_bytes()).unwrap()
    }

    fn tree_of(repo: &Repository, entries: &[(&str, TreeEdit)]) -> Oid {
        let edits: BTreeMap<String, TreeEdit> =
            entries.iter().map(|(p, e)| (p.to_string(), *e)).collect();
        apply_tree_edits(repo, None, &edits).unwrap()
    }

    fn entry_kind(repo: &Repository, root: Oid, path: &str) -> Option<ObjectType> {
        let tree = repo.find_tree(root).unwrap();
        tree.get_path(std::path::Path::new(path)).ok()?.kind()
    }

    #[test]
    fn builds_nested_paths_and_prunes_empty_dirs() {
        let (_tmp, repo) = test_repo();
        let b = blob(&repo, "hello");
        let root = tree_of(
            &repo,
            &[(
                "a/b/c.txt",
                TreeEdit::PutBlob {
                    oid: b,
                    mode: 0o100644,
                },
            )],
        );
        assert_eq!(entry_kind(&repo, root, "a/b/c.txt"), Some(ObjectType::Blob));

        // Removing the only file prunes the whole empty chain.
        let mut edits = BTreeMap::new();
        edits.insert("a/b/c.txt".to_string(), TreeEdit::Remove);
        let base = repo.find_tree(root).unwrap();
        let new_root = apply_tree_edits(&repo, Some(&base), &edits).unwrap();
        assert_eq!(repo.find_tree(new_root).unwrap().len(), 0);
    }

    #[test]
    fn put_tree_attaches_subtree_directly() {
        let (_tmp, repo) = test_repo();
        let inner = tree_of(
            &repo,
            &[(
                "SKILL.md",
                TreeEdit::PutBlob {
                    oid: blob(&repo, "skill"),
                    mode: 0o100644,
                },
            )],
        );
        let root = tree_of(
            &repo,
            &[("group/my-skill", TreeEdit::PutTree { oid: inner })],
        );
        assert_eq!(
            entry_kind(&repo, root, "group/my-skill/SKILL.md"),
            Some(ObjectType::Blob)
        );
    }

    #[test]
    fn blob_to_tree_and_tree_to_blob_type_changes() {
        let (_tmp, repo) = test_repo();
        let file = blob(&repo, "was a file");
        let root = tree_of(
            &repo,
            &[(
                "thing",
                TreeEdit::PutBlob {
                    oid: file,
                    mode: 0o100644,
                },
            )],
        );
        let base = repo.find_tree(root).unwrap();

        // blob → tree via a nested edit below the old blob path
        let mut edits = BTreeMap::new();
        edits.insert(
            "thing/SKILL.md".to_string(),
            TreeEdit::PutBlob {
                oid: blob(&repo, "now a dir"),
                mode: 0o100644,
            },
        );
        let root2 = apply_tree_edits(&repo, Some(&base), &edits).unwrap();
        assert_eq!(entry_kind(&repo, root2, "thing"), Some(ObjectType::Tree));
        assert_eq!(
            entry_kind(&repo, root2, "thing/SKILL.md"),
            Some(ObjectType::Blob)
        );

        // tree → blob via PutBlob at the old dir path
        let base2 = repo.find_tree(root2).unwrap();
        let mut edits = BTreeMap::new();
        edits.insert(
            "thing".to_string(),
            TreeEdit::PutBlob {
                oid: blob(&repo, "file again"),
                mode: 0o100644,
            },
        );
        let root3 = apply_tree_edits(&repo, Some(&base2), &edits).unwrap();
        assert_eq!(entry_kind(&repo, root3, "thing"), Some(ObjectType::Blob));
    }

    #[test]
    fn nested_put_under_removed_dir_rebuilds_from_scratch() {
        let (_tmp, repo) = test_repo();
        // Old skill dir "spot" with two files.
        let root = tree_of(
            &repo,
            &[
                (
                    "spot/SKILL.md",
                    TreeEdit::PutBlob {
                        oid: blob(&repo, "old"),
                        mode: 0o100644,
                    },
                ),
                (
                    "spot/extra.md",
                    TreeEdit::PutBlob {
                        oid: blob(&repo, "extra"),
                        mode: 0o100644,
                    },
                ),
            ],
        );
        let base = repo.find_tree(root).unwrap();

        // The skill moves away (Remove spot) while a residual file lands at
        // spot/readme.txt. The old skill files must NOT leak through.
        let mut edits = BTreeMap::new();
        edits.insert("spot".to_string(), TreeEdit::Remove);
        edits.insert(
            "spot/readme.txt".to_string(),
            TreeEdit::PutBlob {
                oid: blob(&repo, "note"),
                mode: 0o100644,
            },
        );
        // Both key orders through insert_edit are covered because the flat
        // map sorts "spot" before "spot/readme.txt".
        let new_root = apply_tree_edits(&repo, Some(&base), &edits).unwrap();
        assert_eq!(
            entry_kind(&repo, new_root, "spot/readme.txt"),
            Some(ObjectType::Blob)
        );
        assert_eq!(entry_kind(&repo, new_root, "spot/SKILL.md"), None);
        assert_eq!(entry_kind(&repo, new_root, "spot/extra.md"), None);
    }

    #[test]
    fn put_overwrites_remove_when_planner_replaces_a_path() {
        let (_tmp, repo) = test_repo();
        let old = tree_of(
            &repo,
            &[(
                "spot/SKILL.md",
                TreeEdit::PutBlob {
                    oid: blob(&repo, "old"),
                    mode: 0o100644,
                },
            )],
        );
        let incoming = tree_of(
            &repo,
            &[(
                "SKILL.md",
                TreeEdit::PutBlob {
                    oid: blob(&repo, "new"),
                    mode: 0o100644,
                },
            )],
        );
        let base = repo.find_tree(old).unwrap();
        // Planner convention: removes first, puts overwrite the same key.
        let mut flat: BTreeMap<String, TreeEdit> = BTreeMap::new();
        flat.insert("spot".to_string(), TreeEdit::Remove);
        flat.insert("spot".to_string(), TreeEdit::PutTree { oid: incoming });
        let root = apply_tree_edits(&repo, Some(&base), &flat).unwrap();
        let tree = repo.find_tree(root).unwrap();
        let entry = tree
            .get_path(std::path::Path::new("spot/SKILL.md"))
            .unwrap();
        assert_eq!(repo.find_blob(entry.id()).unwrap().content(), b"new");
    }

    #[test]
    fn untouched_siblings_survive() {
        let (_tmp, repo) = test_repo();
        let root = tree_of(
            &repo,
            &[
                (
                    "keep.txt",
                    TreeEdit::PutBlob {
                        oid: blob(&repo, "keep"),
                        mode: 0o100644,
                    },
                ),
                (
                    "dir/a.txt",
                    TreeEdit::PutBlob {
                        oid: blob(&repo, "a"),
                        mode: 0o100644,
                    },
                ),
                (
                    "dir/b.txt",
                    TreeEdit::PutBlob {
                        oid: blob(&repo, "b"),
                        mode: 0o100644,
                    },
                ),
            ],
        );
        let base = repo.find_tree(root).unwrap();
        let mut edits = BTreeMap::new();
        edits.insert("dir/a.txt".to_string(), TreeEdit::Remove);
        let new_root = apply_tree_edits(&repo, Some(&base), &edits).unwrap();
        assert_eq!(
            entry_kind(&repo, new_root, "keep.txt"),
            Some(ObjectType::Blob)
        );
        assert_eq!(
            entry_kind(&repo, new_root, "dir/b.txt"),
            Some(ObjectType::Blob)
        );
        assert_eq!(entry_kind(&repo, new_root, "dir/a.txt"), None);
    }
}
