use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::PathBuf;

/// A scenario record from the database.
#[derive(Debug, Clone)]
pub struct Scenario {
    pub id: String,
    pub name: String,
    #[allow(dead_code)]
    pub description: Option<String>,
    pub icon: Option<String>,
    pub prompt_template: Option<String>,
    #[allow(dead_code)]
    pub sort_order: i32,
}

/// A skill record from the database.
#[derive(Debug, Clone)]
pub struct Skill {
    #[allow(dead_code)]
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

/// A recipe record from the database.
#[derive(Debug, Clone)]
pub struct Recipe {
    #[allow(dead_code)]
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub prompt_template: Option<String>,
}

/// Read-only access to the skills-manager SQLite database.
pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open() -> Result<Self> {
        let path = db_path()?;
        anyhow::ensure!(path.exists(), "Database not found at {}", path.display());
        let conn = Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("Failed to open database at {}", path.display()))?;
        Ok(Self { conn })
    }

    /// Get all scenarios ordered by sort_order then name.
    pub fn scenarios(&self) -> Result<Vec<Scenario>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, icon, prompt_template, sort_order
             FROM scenarios ORDER BY sort_order, name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Scenario {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                icon: row.get(3)?,
                prompt_template: row.get(4)?,
                sort_order: row.get(5)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get the active scenario ID from settings.
    pub fn active_scenario_id(&self) -> Result<Option<String>> {
        let result = self.conn.query_row(
            "SELECT value FROM settings WHERE key = 'active_scenario_id'",
            [],
            |row| row.get(0),
        );
        match result {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get skills bound to a scenario.
    pub fn skills_for_scenario(&self, scenario_id: &str) -> Result<Vec<Skill>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, s.description
             FROM skills s
             INNER JOIN scenario_skills ss ON s.id = ss.skill_id
             WHERE ss.scenario_id = ?1
             ORDER BY ss.sort_order, s.name",
        )?;
        let rows = stmt.query_map(params![scenario_id], |row| {
            Ok(Skill {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get recipes for a scenario. Returns empty vec if table doesn't exist (pre-v5 db).
    pub fn recipes_for_scenario(&self, scenario_id: &str) -> Result<Vec<Recipe>> {
        let mut stmt = match self.conn.prepare(
            "SELECT id, name, icon, prompt_template
             FROM scenario_recipes
             WHERE scenario_id = ?1
             ORDER BY sort_order, name",
        ) {
            Ok(s) => s,
            Err(_) => return Ok(vec![]), // table doesn't exist yet
        };
        let rows = stmt.query_map(params![scenario_id], |row| {
            Ok(Recipe {
                id: row.get(0)?,
                name: row.get(1)?,
                icon: row.get(2)?,
                prompt_template: row.get(3)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }
}

/// Resolve database path: ~/.skills-manager/skills-manager.db
fn db_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Cannot determine home directory")?;
    Ok(home.join(".skills-manager").join("skills-manager.db"))
}

/// Render prompt template to plain text for clipboard.
/// Replaces [skill::name] tags with just the skill name.
pub fn render_prompt(template: &str) -> String {
    let re = regex_lite(template);
    re
}

/// Simple tag replacement without pulling in the regex crate.
fn regex_lite(template: &str) -> String {
    let mut result = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("[skill::") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 8..];
        if let Some(end) = after.find(']') {
            let name = &after[..end];
            result.push_str(name);
            rest = &after[end + 1..];
        } else {
            result.push_str(&rest[start..]);
            rest = "";
        }
    }
    result.push_str(rest);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_prompt() {
        assert_eq!(
            render_prompt("Use [skill::code-review] and [skill::tdd] for this."),
            "Use code-review and tdd for this."
        );
    }

    #[test]
    fn test_render_prompt_no_tags() {
        assert_eq!(render_prompt("plain text"), "plain text");
    }

    #[test]
    fn test_render_prompt_unclosed_tag() {
        assert_eq!(render_prompt("broken [skill::foo"), "broken [skill::foo");
    }
}
