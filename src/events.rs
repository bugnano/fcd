use std::{io, panic, sync::mpsc, thread, time::Duration};

use anyhow::Result;
use termion::{event::*, input::TermRead};

use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;

use crate::{app::App, component::Component};

pub enum Events {
    Input(Event),
    Tick,
    Signal(i32),
}

pub fn handle_events(events_rx: &mpsc::Receiver<Events>, app: &mut App) -> Result<bool> {
    let events = events_rx.recv()?;

    let event_handled = app.handle_events(&events)?;

    if !event_handled {
        match events {
            Events::Input(event) => match event {
                Event::Key(key) => match key {
                    Key::Char('q') => return Ok(true),
                    Key::Char('p') => panic!("at the disco"),
                    _ => (),
                },
                Event::Mouse(_mouse) => (),
                Event::Unsupported(_) => (),
            },
            Events::Tick => (),
            Events::Signal(signal) => match signal {
                SIGWINCH => (),
                _ => unreachable!(),
            },
        }
    }

    Ok(false)
}

pub fn init_events(tick_rate: Duration, mut signals: Signals) -> mpsc::Receiver<Events> {
    let (tx, rx) = mpsc::channel();
    let input_tx = tx.clone();
    let signals_tx = tx.clone();

    thread::spawn(move || {
        let stdin = io::stdin();
        for event in stdin.events().flatten() {
            if let Err(err) = input_tx.send(Events::Input(event)) {
                eprintln!("{}", err);
                return;
            }
        }
    });

    thread::spawn(move || loop {
        if let Err(err) = tx.send(Events::Tick) {
            eprintln!("{}", err);
            break;
        }
        thread::sleep(tick_rate);
    });

    thread::spawn(move || {
        for signal in &mut signals {
            if let Err(err) = signals_tx.send(Events::Signal(signal)) {
                eprintln!("{}", err);
                return;
            }
        }
    });

    rx
}
