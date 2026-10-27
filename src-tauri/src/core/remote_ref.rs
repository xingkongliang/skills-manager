use crate::core::error::AppError;
use crate::core::git_fetcher;
use crate::core::skill_store::SkillRecord;

/// The one answer to "which remote is this, and where inside it".
///
/// A Source's identity used to be spelled three ways — one type for a parsed
/// URL, one rebuilt from a skill record, one used as a map key — with a
/// conversion between each and a comment at the map key explaining that subpath
/// must be ignored when comparing. One type now owns it: the same value the
/// installer writes, the updater reads back, and the batch groups by.
#[derive(Debug, Clone)]
pub struct RemoteRef {
    /// The URL as the user gave it, kept for display and for `source_ref`.
    pub original_url: String,
    /// The URL actually handed to git.
    pub clone_url: String,
    pub branch: Option<String>,
    /// Directory inside the repository, when the Source is a subdirectory.
    pub subpath: Option<String>,
    /// skills.sh names a skill inside a repository without saying which
    /// directory; this is the id used to find it when nothing else can.
    pub locator: Option<String>,
}

/// What makes two skills come from the same Source: the repository and the
/// branch. Deliberately excludes `subpath` — two directories of one repository
/// share a head revision, so they share one fetch. Deliberately excludes
/// `locator` for the same reason.
///
/// Equality and hashing fold the spellings git treats as one repository
/// (trailing `.git`, a trailing slash, letter case) and nothing else: an `ssh`
/// and an `https` reference to the same repository stay distinct, because they
/// can resolve to different credentials and merging them would send one of the
/// two down a path it cannot authenticate. The stored `clone_url` stays exactly
/// as it must be handed to git — lower-casing it for the comparison key would
/// break a case-sensitive host.
#[derive(Debug, Clone)]
pub struct RemoteIdentity {
    pub clone_url: String,
    pub branch: Option<String>,
}

impl PartialEq for RemoteIdentity {
    fn eq(&self, other: &Self) -> bool {
        comparison_key(&self.clone_url) == comparison_key(&other.clone_url)
            && self.branch == other.branch
    }
}

impl Eq for RemoteIdentity {}

impl std::hash::Hash for RemoteIdentity {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        comparison_key(&self.clone_url).hash(state);
        self.branch.hash(state);
    }
}

impl RemoteIdentity {
    /// A stable string for a setting or a UI key. Folded exactly like equality
    /// and carries the branch, so two spellings that `==` treats as one Source
    /// never produce two keys — and a branch is never silently dropped.
    pub fn key(&self) -> String {
        format!(
            "{}#{}",
            comparison_key(&self.clone_url),
            self.branch.as_deref().unwrap_or("")
        )
    }
}

impl RemoteRef {
    pub fn from_url(url: &str) -> Self {
        let trimmed = url.trim().to_string();
        let (clone_url, branch, subpath) = git_fetcher::normalize_url(&trimmed);
        RemoteRef {
            original_url: trimmed,
            clone_url,
            branch,
            subpath: clean_subpath(subpath),
            locator: None,
        }
    }

    pub fn identity(&self) -> RemoteIdentity {
        RemoteIdentity {
            clone_url: self.clone_url.clone(),
            branch: self.branch.clone(),
        }
    }

    /// Whether both references name the same remote. The whole reason this is a
    /// method on the type rather than a field comparison at each call site: the
    /// answer must be the same everywhere, including the parts that are ignored.
    pub fn same_remote(&self, other: &RemoteRef) -> bool {
        self.identity() == other.identity()
    }

    /// The directory this skill was installed from, as `(revision, subpath)` —
    /// `None` for a skill installed from the repository root, which keeps
    /// comparing the whole repository's head.
    pub fn installed_subtree(&self, skill: &SkillRecord) -> Option<(String, String)> {
        let subpath = self.subpath.as_deref().filter(|s| !s.is_empty())?;
        Some((skill.source_revision.clone()?, subpath.to_string()))
    }

    /// Rebuild the reference from a stored skill. `source_ref_resolved` is the
    /// clone URL the installer settled on; failing that the original reference
    /// is re-parsed, preferring the branch and subpath recorded at install time,
    /// which survive tree URLs whose branch contains a slash.
    pub fn from_skill(skill: &SkillRecord) -> Result<Self, AppError> {
        if let Some(resolved) = &skill.source_ref_resolved {
            return Ok(RemoteRef {
                original_url: skill
                    .source_ref
                    .clone()
                    .unwrap_or_else(|| resolved.clone()),
                clone_url: resolved.clone(),
                branch: skill.source_branch.clone(),
                subpath: clean_subpath(skill.source_subpath.clone()),
                locator: skill_ssh_locator(skill),
            });
        }

        match skill.source_type.as_str() {
            "git" => {
                let source_ref = skill
                    .source_ref
                    .as_ref()
                    .ok_or_else(|| AppError::invalid_input("Git skill is missing its source URL"))?;
                let parsed = git_fetcher::parse_git_source(source_ref);
                Ok(RemoteRef {
                    clone_url: parsed.clone_url,
                    branch: skill.source_branch.clone().or(parsed.branch),
                    subpath: clean_subpath(skill.source_subpath.clone().or(parsed.subpath)),
                    locator: None,
                    original_url: parsed.original_url,
                })
            }
            "skillssh" => {
                let source_ref = skill.source_ref.as_ref().ok_or_else(|| {
                    AppError::invalid_input("skills.sh skill is missing its source reference")
                })?;
                let (repo_source, fallback_skill_id) = source_ref
                    .rsplit_once('/')
                    .ok_or_else(|| AppError::invalid_input("Invalid skills.sh source reference"))?;
                Ok(RemoteRef {
                    original_url: source_ref.clone(),
                    clone_url: format!("https://github.com/{}.git", repo_source),
                    branch: skill.source_branch.clone(),
                    subpath: clean_subpath(skill.source_subpath.clone()),
                    locator: Some(fallback_skill_id.to_string()),
                })
            }
            _ => Err(AppError::invalid_input(
                "Skill does not support git-based updates",
            )),
        }
    }
}

/// Fold the spellings git treats as one repository, keeping everything that can
/// change which repository git actually reads.
///
/// Only `.git` and a trailing slash are folded. Case is not: host names are
/// case-insensitive, but paths are not on every host, and
/// `git_fetcher::canonicalize_clone_url` deliberately does not fold it either.
/// Merging two paths that differ only in case would hand one repository's head
/// to the other's skills.
fn comparison_key(clone_url: &str) -> String {
    let trimmed = clone_url.trim().trim_end_matches('/');
    let without_git = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    without_git.trim_end_matches('/').to_string()
}

/// Whether a skill with this Channel is fetched from git and therefore has a
/// remote to resolve, a revision to compare, and a checkout to update from.
pub fn is_git_channel(source_type: &str) -> bool {
    matches!(source_type, "git" | "skillssh")
}

/// An empty subpath and no subpath are the same thing. Nothing else is
/// rewritten: on unix a trailing space and a backslash are legal parts of a
/// directory name, so tidying the string here would make the narrow checkout
/// select a directory the caller then cannot find.
fn clean_subpath(subpath: Option<String>) -> Option<String> {
    subpath.filter(|s| !s.is_empty())
}

fn skill_ssh_locator(skill: &SkillRecord) -> Option<String> {
    if skill.source_type != "skillssh" {
        return None;
    }
    skill.source_ref.as_ref().and_then(|source_ref| {
        source_ref
            .rsplit_once('/')
            .map(|(_, skill_id)| skill_id.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(id: &str) -> SkillRecord {
        let mut skill = SkillRecord {
            id: id.to_string(),
            name: id.to_string(),
            description: None,
            source_type: "git".to_string(),
            source_ref: None,
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: format!("/fixture/{id}"),
            content_hash: None,
            enabled: true,
            created_at: 0,
            updated_at: 0,
            status: "ok".to_string(),
            update_status: "unknown".to_string(),
            last_checked_at: None,
            last_check_error: None,
        };
        skill.source_type = "git".into();
        skill.source_ref = Some("https://github.com/acme/skills".into());
        skill.source_ref_resolved = Some("https://github.com/acme/skills".into());
        skill
    }

    fn reference(url: &str) -> RemoteRef {
        RemoteRef::from_url(url)
    }

    #[test]
    fn spellings_git_treats_as_one_repository_are_one_remote() {
        let forms = [
            "https://github.com/acme/skills",
            "https://github.com/acme/skills.git",
            "https://github.com/acme/skills/",
        ];
        let first = reference(forms[0]).identity();
        for form in &forms[1..] {
            assert_eq!(
                reference(form).identity(),
                first,
                "{form} should name the same remote as {}",
                forms[0]
            );
        }
    }

    #[test]
    fn case_is_not_folded_because_paths_are_case_sensitive() {
        assert!(!reference("https://example.com/Acme/Skills")
            .same_remote(&reference("https://example.com/acme/skills")));
    }

    #[test]
    fn another_access_path_is_another_remote() {
        // ssh and https can resolve to different credentials; merging them would
        // send one of the two down a path it cannot authenticate.
        assert!(!reference("https://github.com/acme/skills")
            .same_remote(&reference("git@github.com:acme/skills.git")));
    }

    #[test]
    fn a_tree_url_keeps_its_branch_and_subpath() {
        let parsed = reference("https://github.com/acme/skills/tree/main/tools/my-skill");
        assert_eq!(parsed.branch.as_deref(), Some("main"));
        assert_eq!(parsed.subpath.as_deref(), Some("tools/my-skill"));
    }

    #[test]
    fn two_directories_of_one_repo_are_one_remote() {
        let a = reference("https://github.com/acme/skills/tree/main/tools/a");
        let b = reference("https://github.com/acme/skills/tree/main/tools/b");
        assert!(a.same_remote(&b));
    }

    #[test]
    fn another_branch_is_another_remote() {
        let a = reference("https://github.com/acme/skills/tree/main/tools/a");
        let b = reference("https://github.com/acme/skills/tree/dev/tools/a");
        assert!(!a.same_remote(&b));
    }

    #[test]
    fn a_skill_without_a_resolved_url_is_rebuilt_from_its_reference() {
        let mut record = skill("s1");
        record.source_ref_resolved = None;
        record.source_ref = Some("git@github.com:acme/skills.git".into());
        let rebuilt = RemoteRef::from_skill(&record).expect("parses");
        assert_eq!(rebuilt.clone_url, "git@github.com:acme/skills.git");
        assert!(
            rebuilt.same_remote(&reference("git@github.com:acme/skills")),
            "the comparison key folds `.git`"
        );
    }

    #[test]
    fn an_empty_subpath_is_no_subpath() {
        let mut record = skill("s1");
        record.source_subpath = Some(String::new());
        let rebuilt = RemoteRef::from_skill(&record).expect("parses");
        assert_eq!(rebuilt.subpath, None);
    }

    #[test]
    fn the_installed_subtree_is_the_recorded_revision_and_subpath() {
        let mut record = skill("s1");
        record.source_subpath = Some("tools/a".into());
        record.source_revision = Some("abc123".into());
        let reference = RemoteRef::from_skill(&record).expect("parses");
        assert_eq!(
            reference.installed_subtree(&record),
            Some(("abc123".to_string(), "tools/a".to_string()))
        );
    }

    #[test]
    fn a_root_install_has_no_subtree() {
        let mut record = skill("s1");
        record.source_revision = Some("abc123".into());
        let reference = RemoteRef::from_skill(&record).expect("parses");
        assert_eq!(reference.installed_subtree(&record), None);
    }
}
