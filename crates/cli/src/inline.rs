use std::io::{self, Stdout};

use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        cursor::Show,
        event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, size},
    },
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    widgets::{List, ListItem, ListState, Paragraph},
    Frame, Terminal, TerminalOptions, Viewport,
};
use rfsee_tf_idf::RfcSearchResult;

type InlineTerminal = Terminal<CrosstermBackend<Stdout>>;

struct Session {
    terminal: Option<InlineTerminal>,
}

impl Session {
    fn new(height: u16) -> io::Result<Self> {
        // Raw mode delivers input immediately, including Ctrl-C as a key event.
        // https://docs.rs/crossterm/0.29.0/crossterm/terminal/index.html#raw-mode
        enable_raw_mode()?;
        let mut session = Self { terminal: None };
        // Inline viewports reserve rows at the cursor on the normal screen.
        // https://docs.rs/ratatui/0.30.0/ratatui/enum.Viewport.html#variant.Inline
        session.terminal = Some(Terminal::with_options(
            CrosstermBackend::new(io::stdout()),
            TerminalOptions {
                viewport: Viewport::Inline(height),
            },
        )?);
        session.terminal.as_mut().unwrap().clear()?;
        Ok(session)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(terminal) = &mut self.terminal {
            let origin = terminal.get_frame().area().as_position();
            let _ = terminal.clear();
            let _ = terminal.set_cursor_position(origin);
            let _ = terminal.show_cursor();
        }
        let _ = disable_raw_mode();
    }
}

pub(crate) fn pick(results: &[RfcSearchResult]) -> io::Result<Option<usize>> {
    if results.is_empty() {
        return Ok(None);
    }

    // Restore input and cursor visibility before the panic diagnostic is printed.
    // https://ratatui.rs/recipes/apps/panic-hooks/
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), Show);
        previous_hook(info);
    }));

    let height = ((results.len().min(10) + 2) as u16).min(size()?.1.saturating_sub(1).max(1));
    let mut session = Session::new(height)?;
    let terminal = session.terminal.as_mut().unwrap();
    let mut state = ListState::default().with_selected(Some(0));
    let items: Vec<_> = results
        .iter()
        .map(|result| {
            // Keep each result on one row, including titles containing control characters.
            let title: String = result
                .title
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            ListItem::new(title)
        })
        .collect();

    loop {
        terminal.draw(|frame| draw(frame, &items, &mut state))?;
        // Ignore release events so one keypress does not navigate twice.
        // https://docs.rs/crossterm/0.29.0/crossterm/event/enum.KeyEventKind.html
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Release {
                continue;
            }
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => return Ok(None),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None)
                }
                KeyCode::Enter => return Ok(state.selected()),
                KeyCode::Up | KeyCode::Char('k') => {
                    state.select(Some(state.selected().unwrap_or(0).saturating_sub(1)))
                }
                KeyCode::Down | KeyCode::Char('j') => state.select(Some(
                    (state.selected().unwrap_or(0) + 1).min(results.len() - 1),
                )),
                KeyCode::Home => state.select(Some(0)),
                KeyCode::End => state.select(Some(results.len() - 1)),
                _ => {}
            }
        }
    }
}

fn draw(frame: &mut Frame, items: &[ListItem<'_>], state: &mut ListState) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    frame.render_widget(
        Paragraph::new(format!(
            "RFC results — {}/{}",
            state.selected().map_or(0, |i| i + 1),
            items.len()
        )),
        header,
    );
    frame.render_stateful_widget(
        List::new(items.iter().cloned())
            .highlight_symbol("> ")
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        body,
        state,
    );
    frame.render_widget(
        Paragraph::new("↑/↓ move · Enter open browser · Esc quit"),
        footer,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn scrolls_to_selection_and_handles_small_resized_viewport() {
        let mut terminal = Terminal::new(TestBackend::new(60, 5)).unwrap();
        let items: Vec<_> = (1..=20)
            .map(|i| ListItem::new(format!("RFC {i}")))
            .collect();
        let mut state = ListState::default().with_selected(Some(19));
        terminal
            .draw(|frame| draw(frame, &items, &mut state))
            .unwrap();
        let screen = format!("{:?}", terminal.backend().buffer());
        assert!(screen.contains("> RFC 20"));
        assert!(screen.contains("20/20"));
        assert!(state.offset() > 0);

        terminal.backend_mut().resize(8, 2);
        terminal
            .draw(|frame| draw(frame, &items, &mut state))
            .unwrap();
        assert_eq!(state.selected(), Some(19));
    }
}
