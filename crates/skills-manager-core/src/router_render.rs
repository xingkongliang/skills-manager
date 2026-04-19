use crate::skill_store::{PackRecord, SkillRecord};
use std::path::Path;

/// Render the SKILL.md content for a pack router.
///
/// If `pack.router_body` is set, use it as-is. Otherwise auto-render a skill
/// table from the pack's skills. The frontmatter uses `router_description`
/// (falling back to a placeholder).
pub fn render_router_skill_md(
    pack: &PackRecord,
    skills: &[SkillRecord],
    vault_root: &Path,
) -> String {
    let desc = pack
        .router_description
        .as_deref()
        .unwrap_or("Router for pack — description pending generation.");
    let body = pack
        .router_body
        .clone()
        .unwrap_or_else(|| auto_render_body(pack, skills, vault_root));

    format!(
        "---\nname: pack-{}\ndescription: {}\n---\n\n{}\n",
        pack.name,
        escape_yaml_scalar(desc),
        body,
    )
}

fn auto_render_body(pack: &PackRecord, skills: &[SkillRecord], vault_root: &Path) -> String {
    let mut out = format!(
        "# Pack: {}\n\n\
        揀一個 skill，用 `Read` tool 讀對應 SKILL.md，跟住做。\n\n\
        | Skill | 用途 | 路徑 |\n|---|---|---|\n",
        pack.name,
    );
    for s in skills {
        let summary = s
            .description
            .as_deref()
            .unwrap_or("")
            .split_terminator(['.', '。'])
            .next()
            .unwrap_or("")
            .trim();
        out.push_str(&format!(
            "| `{}` | {} | `{}/{}/SKILL.md` |\n",
            s.name,
            summary,
            vault_root.display(),
            s.name,
        ));
    }
    out
}

fn escape_yaml_scalar(s: &str) -> String {
    if s.contains('\n') || s.contains(':') || s.starts_with(['-', '?', '[', '{', '|', '>']) {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn pack(name: &str, router_desc: Option<&str>) -> PackRecord {
        PackRecord {
            id: format!("p-{name}"),
            name: name.into(),
            description: None,
            icon: None,
            color: None,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
            router_description: router_desc.map(str::to_string),
            router_body: None,
            is_essential: false,
            router_updated_at: None,
        }
    }

    fn skill(name: &str, desc: &str) -> SkillRecord {
        SkillRecord {
            id: format!("s-{name}"),
            name: name.into(),
            description: Some(desc.into()),
            source_type: "local".into(),
            source_ref: None,
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: format!("/vault/{name}"),
            content_hash: None,
            enabled: true,
            created_at: 0,
            updated_at: 0,
            status: "active".into(),
            update_status: "idle".into(),
            last_checked_at: None,
            last_check_error: None,
        }
    }

    #[test]
    fn renders_frontmatter_with_pack_name() {
        let p = pack("mkt-seo", Some("When user mentions SEO"));
        let out = render_router_skill_md(&p, &[], &PathBuf::from("/vault"));
        assert!(out.starts_with("---\nname: pack-mkt-seo"));
        assert!(out.contains("description: When user mentions SEO"));
    }

    #[test]
    fn auto_renders_skill_table_when_body_empty() {
        let p = pack("mkt-seo", Some("desc"));
        let skills = vec![
            skill("seo-audit", "Diagnose SEO issues. Use when..."),
            skill("ai-seo", "Optimize for LLM citations."),
        ];
        let out = render_router_skill_md(&p, &skills, &PathBuf::from("/vault"));
        assert!(out.contains("| `seo-audit` | Diagnose SEO issues | `/vault/seo-audit/SKILL.md` |"));
        assert!(
            out.contains("| `ai-seo` | Optimize for LLM citations | `/vault/ai-seo/SKILL.md` |")
        );
    }

    #[test]
    fn custom_router_body_is_used_as_is() {
        let mut p = pack("custom", Some("desc"));
        p.router_body = Some("# Custom body\n\nhand-written".into());
        let out = render_router_skill_md(&p, &[], &PathBuf::from("/vault"));
        assert!(out.contains("# Custom body"));
        assert!(!out.contains("揀一個 skill"));
    }

    #[test]
    fn null_description_emits_placeholder() {
        let p = pack("x", None);
        let out = render_router_skill_md(&p, &[], &PathBuf::from("/v"));
        assert!(out.contains("description: Router for pack — description pending generation."));
    }

    #[test]
    fn yaml_special_chars_are_quoted() {
        let p = pack("x", Some("Trigger: SEO audit"));
        let out = render_router_skill_md(&p, &[], &PathBuf::from("/v"));
        assert!(out.contains("description: \"Trigger: SEO audit\""));
    }

    #[test]
    fn deterministic_for_same_input() {
        let p = pack("x", Some("d"));
        let skills = vec![skill("a", "x."), skill("b", "y.")];
        let a = render_router_skill_md(&p, &skills, &PathBuf::from("/v"));
        let b = render_router_skill_md(&p, &skills, &PathBuf::from("/v"));
        assert_eq!(a, b);
    }
}
