use std::io::{self, Stdout};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use crate::codex::CommitMessage;
use crate::codex::Model;

pub enum Action {
    Select(CommitMessage),
    Regenerate,
    Cancel,
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        Ok(Self {
            terminal: Terminal::new(CrosstermBackend::new(stdout))?,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

pub fn select(messages: &[CommitMessage]) -> Result<Action> {
    let entries: Vec<_> = messages
        .iter()
        .map(|message| (message.subject.clone(), message.render()))
        .collect();
    match pick("Commit message variants", &entries, 0, true)? {
        PickAction::Select(index) => Ok(Action::Select(messages[index].clone())),
        PickAction::Regenerate => Ok(Action::Regenerate),
        PickAction::Cancel => Ok(Action::Cancel),
    }
}

pub fn select_model(models: &[Model], current: Option<&str>) -> Result<Option<Model>> {
    let entries: Vec<_> = models
        .iter()
        .map(|model| {
            (
                format!("{} ({})", model.display_name, model.slug),
                model.description.clone(),
            )
        })
        .collect();
    let selected = current
        .and_then(|slug| models.iter().position(|model| model.slug == slug))
        .unwrap_or(0);
    match pick("Codex models", &entries, selected, false)? {
        PickAction::Select(index) => Ok(Some(models[index].clone())),
        PickAction::Cancel | PickAction::Regenerate => Ok(None),
    }
}

enum PickAction {
    Select(usize),
    Regenerate,
    Cancel,
}

fn pick(
    title: &str,
    entries: &[(String, String)],
    initial: usize,
    can_regenerate: bool,
) -> Result<PickAction> {
    let mut terminal = TerminalGuard::enter()?;
    let mut selected = initial;
    loop {
        terminal.terminal.draw(|frame| {
            let areas = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(entries.len() as u16 + 2),
                    Constraint::Min(5),
                    Constraint::Length(1),
                ])
                .split(frame.area());
            let items = entries.iter().enumerate().map(|(index, (label, _))| {
                ListItem::new(Line::from(format!("{}. {}", index + 1, label)))
            });
            let list = List::new(items)
                .block(Block::default().title(title).borders(Borders::ALL))
                .highlight_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("› ");
            let mut state = ListState::default().with_selected(Some(selected));
            frame.render_stateful_widget(list, areas[0], &mut state);

            let preview = entries[selected].1.as_str();
            frame.render_widget(
                Paragraph::new(preview)
                    .block(Block::default().title("Preview").borders(Borders::ALL))
                    .wrap(Wrap { trim: false }),
                areas[1],
            );
            let help = if can_regenerate {
                "↑/↓ or j/k: move   Enter: select   r: regenerate   q/Esc: cancel"
            } else {
                "↑/↓ or j/k: move   Enter: select   q/Esc: cancel"
            };
            frame.render_widget(Paragraph::new(help), areas[2]);
        })?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => selected = selected.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    selected = (selected + 1).min(entries.len() - 1)
                }
                KeyCode::Enter => return Ok(PickAction::Select(selected)),
                KeyCode::Char('r') if can_regenerate => return Ok(PickAction::Regenerate),
                KeyCode::Esc | KeyCode::Char('q') => return Ok(PickAction::Cancel),
                _ => {}
            }
        }
    }
}
