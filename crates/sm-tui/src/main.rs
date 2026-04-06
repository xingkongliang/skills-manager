mod app;
mod db;
mod ui;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io;
use std::time::Duration;

fn main() -> Result<()> {
    let mut app = app::App::new()?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Main loop
    let result = run(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut app::App) -> Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        // Poll with timeout so the flash message can tick down
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Search mode input handling
                if app.search_mode {
                    match key.code {
                        KeyCode::Esc => app.exit_search(),
                        KeyCode::Enter => {
                            if !app.filtered_indices.is_empty() {
                                app.scenario_index =
                                    app.filtered_indices[app.filter_cursor];
                            }
                            app.exit_search();
                        }
                        KeyCode::Backspace => app.search_pop(),
                        KeyCode::Char(c) => app.search_push(c),
                        KeyCode::Up => app.move_up(),
                        KeyCode::Down => app.move_down(),
                        _ => {}
                    }
                    continue;
                }

                // Normal mode
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        app.should_quit = true;
                        break;
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.should_quit = true;
                        break;
                    }
                    KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                    KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                    KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => app.focus_next(),
                    KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => app.focus_prev(),
                    KeyCode::Char('/') => app.enter_search(),
                    KeyCode::Enter => {
                        app.copy_prompt()?;
                    }
                    _ => {}
                }
            }
        } else {
            // No event — tick the flash counter
            app.tick_flash();
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
