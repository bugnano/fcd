use std::{io, panic, sync::mpsc, thread, time::Duration};

use anyhow::Result;
use ratatui::widgets::*;
use termion::{event::Key, input::TermRead};

pub enum Event {
    Input(Key),
    Tick,
}

pub fn should_quit(
    events: &mpsc::Receiver<Event>,
    items: &[Row],
    state: &mut TableState,
) -> Result<bool> {
    match events.recv()? {
        Event::Input(key) => match key {
            Key::Char('q') => return Ok(true),
            Key::Char('p') => panic!("at the disco"),
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
            }
            _ => (),
        },
        Event::Tick => (),
    }

    Ok(false)
}

pub fn init_events(tick_rate: Duration) -> mpsc::Receiver<Event> {
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
