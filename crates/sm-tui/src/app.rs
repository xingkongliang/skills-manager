use crate::db::{self, Db, Scenario, Skill};
use anyhow::Result;

/// Which panel is currently focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Scenarios,
    Skills,
    Prompt,
}

impl Panel {
    pub fn next(self) -> Self {
        match self {
            Panel::Scenarios => Panel::Skills,
            Panel::Skills => Panel::Prompt,
            Panel::Prompt => Panel::Scenarios,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Panel::Scenarios => Panel::Prompt,
            Panel::Skills => Panel::Scenarios,
            Panel::Prompt => Panel::Skills,
        }
    }
}

/// Application state.
pub struct App {
    pub scenarios: Vec<Scenario>,
    pub scenario_index: usize,
    pub skills: Vec<Skill>,
    pub skill_index: usize,
    pub active_panel: Panel,
    pub prompt_scroll: u16,
    pub search_mode: bool,
    pub search_query: String,
    pub filtered_indices: Vec<usize>,
    pub filter_cursor: usize,
    pub should_quit: bool,
    /// Tick counter for the "Copied!" flash message (counts down to 0).
    pub copied_flash: u8,
    db: Db,
}

impl App {
    pub fn new() -> Result<Self> {
        let db = Db::open()?;
        let scenarios = db.scenarios()?;
        let active_id = db.active_scenario_id()?;

        // Find the index of the active scenario
        let scenario_index = active_id
            .as_deref()
            .and_then(|id| scenarios.iter().position(|s| s.id == id))
            .unwrap_or(0);

        let skills = if let Some(s) = scenarios.get(scenario_index) {
            db.skills_for_scenario(&s.id)?
        } else {
            vec![]
        };

        Ok(Self {
            scenarios,
            scenario_index,
            skills,
            skill_index: 0,
            active_panel: Panel::Scenarios,
            prompt_scroll: 0,
            search_mode: false,
            search_query: String::new(),
            filtered_indices: vec![],
            filter_cursor: 0,
            should_quit: false,
            copied_flash: 0,
            db,
        })
    }

    /// Reload skills for the currently selected scenario.
    fn reload_skills(&mut self) {
        if let Some(s) = self.scenarios.get(self.selected_scenario_index()) {
            self.skills = self.db.skills_for_scenario(&s.id).unwrap_or_default();
        } else {
            self.skills = vec![];
        }
        self.skill_index = 0;
    }

    /// The actual scenario index (respects search filter).
    fn selected_scenario_index(&self) -> usize {
        if self.search_mode && !self.filtered_indices.is_empty() {
            self.filtered_indices[self.filter_cursor]
        } else {
            self.scenario_index
        }
    }

    /// Get the currently selected scenario.
    pub fn selected_scenario(&self) -> Option<&Scenario> {
        self.scenarios.get(self.selected_scenario_index())
    }

    /// Get the rendered prompt text for the selected scenario.
    pub fn prompt_text(&self) -> String {
        self.selected_scenario()
            .and_then(|s| s.prompt_template.as_deref())
            .map(db::render_prompt)
            .unwrap_or_default()
    }

    // ── Navigation ──

    pub fn move_up(&mut self) {
        match self.active_panel {
            Panel::Scenarios => {
                if self.search_mode {
                    if self.filter_cursor > 0 {
                        self.filter_cursor -= 1;
                        self.reload_skills();
                        self.prompt_scroll = 0;
                    }
                } else if self.scenario_index > 0 {
                    self.scenario_index -= 1;
                    self.reload_skills();
                    self.prompt_scroll = 0;
                }
            }
            Panel::Skills => {
                if self.skill_index > 0 {
                    self.skill_index -= 1;
                }
            }
            Panel::Prompt => {
                self.prompt_scroll = self.prompt_scroll.saturating_sub(1);
            }
        }
    }

    pub fn move_down(&mut self) {
        match self.active_panel {
            Panel::Scenarios => {
                if self.search_mode {
                    if !self.filtered_indices.is_empty()
                        && self.filter_cursor < self.filtered_indices.len() - 1
                    {
                        self.filter_cursor += 1;
                        self.reload_skills();
                        self.prompt_scroll = 0;
                    }
                } else if self.scenario_index + 1 < self.scenarios.len() {
                    self.scenario_index += 1;
                    self.reload_skills();
                    self.prompt_scroll = 0;
                }
            }
            Panel::Skills => {
                if !self.skills.is_empty() && self.skill_index + 1 < self.skills.len() {
                    self.skill_index += 1;
                }
            }
            Panel::Prompt => {
                self.prompt_scroll += 1;
            }
        }
    }

    pub fn focus_next(&mut self) {
        self.active_panel = self.active_panel.next();
    }

    pub fn focus_prev(&mut self) {
        self.active_panel = self.active_panel.prev();
    }

    // ── Search ──

    pub fn enter_search(&mut self) {
        self.search_mode = true;
        self.search_query.clear();
        self.update_filter();
    }

    pub fn exit_search(&mut self) {
        self.search_mode = false;
        self.search_query.clear();
        self.filtered_indices.clear();
    }

    pub fn search_push(&mut self, c: char) {
        self.search_query.push(c);
        self.update_filter();
    }

    pub fn search_pop(&mut self) {
        self.search_query.pop();
        self.update_filter();
    }

    fn update_filter(&mut self) {
        let q = self.search_query.to_lowercase();
        self.filtered_indices = self
            .scenarios
            .iter()
            .enumerate()
            .filter(|(_, s)| q.is_empty() || s.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();
        self.filter_cursor = 0;
        self.reload_skills();
        self.prompt_scroll = 0;
    }

    // ── Actions ──

    pub fn copy_prompt(&mut self) -> Result<()> {
        let text = self.prompt_text();
        if text.is_empty() {
            return Ok(());
        }
        let mut clipboard = arboard::Clipboard::new()?;
        clipboard.set_text(&text)?;
        self.copied_flash = 20; // ~2 seconds at 100ms poll rate
        Ok(())
    }

    /// Tick the flash counter down. Returns true if flash was active.
    pub fn tick_flash(&mut self) -> bool {
        if self.copied_flash > 0 {
            self.copied_flash -= 1;
            true
        } else {
            false
        }
    }
}
