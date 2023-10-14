use ratatui::{
    backend::{Backend, TermionBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame, Terminal,
};
use std::{
    io,
    io::{BufRead, BufReader},
    sync::mpsc,
    thread,
    time::Duration,
};
use termion::{
    event::Key,
    input::{MouseTerminal, TermRead},
    raw::IntoRawMode,
    screen::IntoAlternateScreen,
};

use std::fs::File;
use std::io::prelude::*;

use anyhow::Result;

enum Event {
    Input(Key),
    Tick,
}

fn main() -> Result<()> {
    // setup terminal
    let stdout = io::stdout().into_raw_mode()?.into_alternate_screen()?;
    let stdout = MouseTerminal::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut file = File::open("lorem.txt")?;
    let buffered = BufReader::new(file);
    let items: Vec<ListItem> = buffered
        .lines()
        .map(|e| ListItem::new(e.unwrap()))
        .collect();
    let mut state = ListState::default();
    state.select(Some(0));

    let events = events(Duration::from_millis(5000));
    loop {
        terminal.draw(|f| ui(f, &items, &mut state))?;

        match events.recv()? {
            Event::Input(key) => match key {
                Key::Char('q') => break,
                Key::Up => {
                    state.select(Some(match state.selected() {
                        Some(i) if i > 0 => i - 1,
                        Some(_i) => 0,
                        None => 0,
                    }));
                    *state.offset_mut() = state.selected().unwrap();
                }
                Key::Down => {
                    state.select(Some(match state.selected() {
                        Some(i) if (i + 1) < items.len() => i + 1,
                        Some(i) => i,
                        None => 0,
                    }));
                    *state.offset_mut() = state.selected().unwrap();
                },
                _ => (),
            },
            Event::Tick => {}
        }
    }

    Ok(())
}

fn ui<B: Backend>(f: &mut Frame<B>, items: &[ListItem], state: &mut ListState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ]
            .as_ref(),
        )
        .split(f.size());

    let block = Block::default()
        .title(Span::styled(
            "TODO: File name",
            Style::default().fg(Color::Black),
        ))
        .style(Style::default().bg(Color::Cyan));
    f.render_widget(block, chunks[0]);

    let items = List::new(items)
        .block(Block::default().style(Style::default().bg(Color::Blue)))
        .highlight_style(
            Style::default()
                .bg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        );

    // We can now render the item list
    f.render_stateful_widget(items, chunks[1], state);

    let block = Block::default()
        .title(Span::styled(
            "TODO: Bottom bar",
            Style::default().fg(Color::Black),
        ))
        .style(Style::default().bg(Color::Cyan));
    f.render_widget(block, chunks[2]);
}

fn events(tick_rate: Duration) -> mpsc::Receiver<Event> {
    let (tx, rx) = mpsc::channel();
    let keys_tx = tx.clone();
    thread::spawn(move || {
        let stdin = io::stdin();
        for key in stdin.keys().flatten() {
            if let Err(err) = keys_tx.send(Event::Input(key)) {
                eprintln!("{}", err);
                return;
            }
        }
    });
    thread::spawn(move || loop {
        if let Err(err) = tx.send(Event::Tick) {
            eprintln!("{}", err);
            break;
        }
        thread::sleep(tick_rate);
    });
    rx
}
