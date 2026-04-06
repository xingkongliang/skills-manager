use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
    Frame,
};

use crate::app::{App, MiddleMode, Panel, PanelAreas};

const ACCENT: Color = Color::Cyan;
const ACTIVE_BORDER: Color = Color::Cyan;
const INACTIVE_BORDER: Color = Color::DarkGray;
const SELECTED_BG: Color = Color::Rgb(30, 40, 55);
const MUTED: Color = Color::DarkGray;

pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(f.area());

    let main_area = chunks[0];
    let status_bar = chunks[1];

    // Two-column layout: left scenarios, right stacked
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(main_area);

    // Right column: top middle panel, bottom prompt
    let right_panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(columns[1]);

    // Store panel areas for mouse hit-testing
    app.panel_areas = PanelAreas {
        scenarios: columns[0],
        middle: right_panels[0],
        prompt: right_panels[1],
    };

    draw_scenarios(f, app, columns[0]);
    draw_middle(f, app, right_panels[0]);
    draw_prompt(f, app, right_panels[1]);
    draw_status_bar(f, app, status_bar);
}

fn panel_block(title: &str, focused: bool) -> Block<'_> {
    let border_color = if focused { ACTIVE_BORDER } else { INACTIVE_BORDER };
    Block::default()
        .title(format!(" {} ", title))
        .title_style(Style::default().fg(if focused { ACCENT } else { Color::White }))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
}

fn draw_scenarios(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.active_panel == Panel::Scenarios;
    let title = if app.search_mode {
        format!("Scenarios [/{}]", app.search_query)
    } else {
        "Scenarios".to_string()
    };

    let items: Vec<ListItem> = if app.search_mode {
        app.filtered_indices
            .iter()
            .map(|&i| scenario_list_item(&app.scenarios[i]))
            .collect()
    } else {
        app.scenarios.iter().map(scenario_list_item).collect()
    };

    let total = items.len();
    let selected = if app.search_mode {
        if app.filtered_indices.is_empty() {
            None
        } else {
            Some(app.filter_cursor)
        }
    } else if app.scenarios.is_empty() {
        None
    } else {
        Some(app.scenario_index)
    };

    let list = List::new(items)
        .block(panel_block(&title, focused))
        .highlight_style(
            Style::default()
                .bg(SELECTED_BG)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    state.select(selected);
    f.render_stateful_widget(list, area, &mut state);

    // Scrollbar
    let inner_height = area.height.saturating_sub(2) as usize; // borders
    if total > inner_height {
        let mut sb_state = ScrollbarState::new(total).position(selected.unwrap_or(0));
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .thumb_symbol("█"),
            area,
            &mut sb_state,
        );
    }
}

fn scenario_list_item(s: &crate::db::Scenario) -> ListItem<'static> {
    let icon = s.icon.as_deref().unwrap_or("  ");
    let text = format!("{} {}", icon, s.name);
    ListItem::new(text)
}

fn draw_middle(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.active_panel == Panel::Middle;
    match app.middle_mode {
        MiddleMode::Recipes => draw_recipes(f, app, area, focused),
        MiddleMode::Skills => draw_skills(f, app, area, focused),
    }
}

fn draw_recipes(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    if app.recipes.is_empty() {
        let msg = Paragraph::new("No recipes in this scenario")
            .style(Style::default().fg(MUTED))
            .block(panel_block("Recipes", focused));
        f.render_widget(msg, area);
        return;
    }

    let items: Vec<ListItem> = app
        .recipes
        .iter()
        .map(|r| {
            let icon = r.icon.as_deref().unwrap_or("📋");
            let text = format!("  {} {}", icon, r.name);
            ListItem::new(text)
        })
        .collect();

    let total = items.len();
    let list = List::new(items)
        .block(panel_block("Recipes", focused))
        .highlight_style(
            Style::default()
                .bg(SELECTED_BG)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸");

    let mut state = ListState::default();
    state.select(Some(app.middle_index));
    f.render_stateful_widget(list, area, &mut state);

    let inner_height = area.height.saturating_sub(2) as usize;
    if total > inner_height {
        let mut sb_state = ScrollbarState::new(total).position(app.middle_index);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .thumb_symbol("█"),
            area,
            &mut sb_state,
        );
    }
}

fn draw_skills(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    if app.skills.is_empty() {
        let msg = Paragraph::new("No skills in this scenario")
            .style(Style::default().fg(MUTED))
            .block(panel_block("Skills", focused));
        f.render_widget(msg, area);
        return;
    }

    let items: Vec<ListItem> = app
        .skills
        .iter()
        .map(|sk| {
            let name = &sk.name;
            let desc = sk
                .description
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(40)
                .collect::<String>();
            let line = if desc.is_empty() {
                Line::from(vec![
                    Span::styled("  ☑ ", Style::default().fg(ACCENT)),
                    Span::raw(name.clone()),
                ])
            } else {
                Line::from(vec![
                    Span::styled("  ☑ ", Style::default().fg(ACCENT)),
                    Span::raw(name.clone()),
                    Span::styled(format!("  {}", desc), Style::default().fg(MUTED)),
                ])
            };
            ListItem::new(line)
        })
        .collect();

    let total = items.len();
    let list = List::new(items)
        .block(panel_block("Skills", focused))
        .highlight_style(
            Style::default()
                .bg(SELECTED_BG)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸");

    let mut state = ListState::default();
    state.select(Some(app.middle_index));
    f.render_stateful_widget(list, area, &mut state);

    let inner_height = area.height.saturating_sub(2) as usize;
    if total > inner_height {
        let mut sb_state = ScrollbarState::new(total).position(app.middle_index);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .thumb_symbol("█"),
            area,
            &mut sb_state,
        );
    }
}

fn draw_prompt(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.active_panel == Panel::Prompt;
    let text = app.prompt_text();

    if text.is_empty() {
        let msg = Paragraph::new("No prompt template")
            .style(Style::default().fg(MUTED))
            .block(panel_block("Prompt Preview", focused));
        f.render_widget(msg, area);
        return;
    }

    let line_count = text.lines().count();
    let paragraph = Paragraph::new(text)
        .block(panel_block("Prompt Preview", focused))
        .wrap(Wrap { trim: false })
        .scroll((app.prompt_scroll, 0));
    f.render_widget(paragraph, area);

    // Prompt scrollbar (estimate wrapped lines ~ raw lines for scrollbar)
    let inner_height = area.height.saturating_sub(2) as usize;
    if line_count > inner_height {
        let mut sb_state =
            ScrollbarState::new(line_count).position(app.prompt_scroll as usize);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .thumb_symbol("█"),
            area,
            &mut sb_state,
        );
    }
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let spans = if app.copied_flash > 0 {
        vec![Span::styled(
            " ✓ Copied to clipboard!",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )]
    } else if app.search_mode {
        vec![
            Span::styled(
                " / ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw("type to filter  "),
            Span::styled("Enter", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" confirm  "),
            Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" cancel"),
        ]
    } else {
        vec![
            Span::styled("←→", Style::default().fg(ACCENT)),
            Span::raw(" panel  "),
            Span::styled("↑↓", Style::default().fg(ACCENT)),
            Span::raw(" select  "),
            Span::styled("Enter", Style::default().fg(ACCENT)),
            Span::raw(" copy  "),
            Span::styled("/", Style::default().fg(ACCENT)),
            Span::raw(" search  "),
            Span::styled("q", Style::default().fg(ACCENT)),
            Span::raw(" quit"),
        ]
    };

    let bar = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(Color::Rgb(20, 20, 30)));
    f.render_widget(bar, area);
}
