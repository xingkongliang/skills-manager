use crate::db::{self, Db, Recipe, Scenario, Skill};
use anyhow::Result;

/// Which panel is currently focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Scenarios,
    Middle,
    Prompt,
}

impl Panel {
    pub fn next(self) -> Self {
        match self {
            Panel::Scenarios => Panel::Middle,
            Panel::Middle => Panel::Prompt,
            Panel::Prompt => Panel::Scenarios,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Panel::Scenarios => Panel::Prompt,
            Panel::Middle => Panel::Scenarios,
            Panel::Prompt => Panel::Middle,
        }
    }
}

/// What the middle panel is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiddleMode {
    Skills,
    Recipes,
}

/// Application state.
pub struct App {
    pub scenarios: Vec<Scenario>,
    pub scenario_index: usize,
    pub skills: Vec<Skill>,
    pub recipes: Vec<Recipe>,
    pub middle_mode: MiddleMode,
    pub middle_index: usize,
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

        let scenario_index = active_id
            .as_deref()
            .and_then(|id| scenarios.iter().position(|s| s.id == id))
            .unwrap_or(0);

        let (skills, recipes, middle_mode) = if let Some(s) = scenarios.get(scenario_index) {
            load_middle_data(&db, &s.id)
        } else {
            (vec![], vec![], MiddleMode::Skills)
        };

        Ok(Self {
            scenarios,
            scenario_index,
            skills,
            recipes,
            middle_mode,
            middle_index: 0,
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

    /// Reload the middle panel data for the currently selected scenario.
    fn reload_middle(&mut self) {
        if let Some(s) = self.scenarios.get(self.selected_scenario_index()) {
            let (skills, recipes, mode) = load_middle_data(&self.db, &s.id);
            self.skills = skills;
            self.recipes = recipes;
            self.middle_mode = mode;
        } else {
            self.skills = vec![];
            self.recipes = vec![];
            self.middle_mode = MiddleMode::Skills;
        }
        self.middle_index = 0;
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

    /// Get the rendered prompt text for what's currently shown.
    pub fn prompt_text(&self) -> String {
        match self.middle_mode {
            MiddleMode::Recipes => {
                // Use the selected recipe's prompt, or fall back to scenario prompt
                self.recipes
                    .get(self.middle_index)
                    .and_then(|r| r.prompt_template.as_deref())
                    .or_else(|| {
                        self.selected_scenario()
                            .and_then(|s| s.prompt_template.as_deref())
                    })
                    .map(db::render_prompt)
                    .unwrap_or_default()
            }
            MiddleMode::Skills => self
                .selected_scenario()
                .and_then(|s| s.prompt_template.as_deref())
                .map(db::render_prompt)
                .unwrap_or_default(),
        }
    }

    /// Count of items in the middle panel.
    fn middle_len(&self) -> usize {
        match self.middle_mode {
            MiddleMode::Skills => self.skills.len(),
            MiddleMode::Recipes => self.recipes.len(),
        }
    }

    // ── Navigation ──

    pub fn move_up(&mut self) {
        match self.active_panel {
            Panel::Scenarios => {
                if self.search_mode {
                    if self.filter_cursor > 0 {
                        self.filter_cursor -= 1;
                        self.reload_middle();
                        self.prompt_scroll = 0;
                    }
                } else if self.scenario_index > 0 {
                    self.scenario_index -= 1;
                    self.reload_middle();
                    self.prompt_scroll = 0;
                }
            }
            Panel::Middle => {
                if self.middle_index > 0 {
                    self.middle_index -= 1;
                    self.prompt_scroll = 0;
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
                        self.reload_middle();
                        self.prompt_scroll = 0;
                    }
                } else if self.scenario_index + 1 < self.scenarios.len() {
                    self.scenario_index += 1;
                    self.reload_middle();
                    self.prompt_scroll = 0;
                }
            }
            Panel::Middle => {
                let len = self.middle_len();
                if len > 0 && self.middle_index + 1 < len {
                    self.middle_index += 1;
                    self.prompt_scroll = 0;
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
        self.reload_middle();
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
        self.copied_flash = 20;
        Ok(())
    }

    pub fn tick_flash(&mut self) -> bool {
        if self.copied_flash > 0 {
            self.copied_flash -= 1;
            true
        } else {
            false
        }
    }
}

/// Load skills and recipes for a scenario, determine which mode to use.
fn load_middle_data(db: &Db, scenario_id: &str) -> (Vec<Skill>, Vec<Recipe>, MiddleMode) {
    let recipes = db.recipes_for_scenario(scenario_id).unwrap_or_default();
    let skills = db.skills_for_scenario(scenario_id).unwrap_or_default();
    let mode = if recipes.is_empty() {
        MiddleMode::Skills
    } else {
        MiddleMode::Recipes
    };
    (skills, recipes, mode)
}
