use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

const PACK_ROUTER_GEN_SKILL: &str =
    include_str!("../assets/builtin-skills/pack-router-gen/SKILL.md");

pub fn install_builtin_skills(vault_root: &Path) -> Result<()> {
    let dir = vault_root.join("pack-router-gen");
    fs::create_dir_all(&dir).context("create pack-router-gen dir")?;
    let path = dir.join("SKILL.md");
    fs::write(&path, PACK_ROUTER_GEN_SKILL).context("write pack-router-gen SKILL.md")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_pack_router_gen_skill() {
        let tmp = tempfile::tempdir().unwrap();
        install_builtin_skills(tmp.path()).unwrap();
        let p = tmp.path().join("pack-router-gen/SKILL.md");
        assert!(p.exists(), "SKILL.md file should be written");
        let content = fs::read_to_string(&p).unwrap();
        assert!(
            content.contains("name: pack-router-gen"),
            "frontmatter name present"
        );
        assert!(
            content.contains("sm pack set-router"),
            "CLI instructions present"
        );
    }

    #[test]
    fn install_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        install_builtin_skills(tmp.path()).unwrap();
        install_builtin_skills(tmp.path()).unwrap(); // second call should not error
        let p = tmp.path().join("pack-router-gen/SKILL.md");
        assert!(p.exists());
    }
}
