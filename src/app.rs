use std::{io, panic, sync::mpsc, thread, time::Duration};

use anyhow::Result;
use ratatui::prelude::*;
use termion::{event::*, input::TermRead};

use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;

use crate::{
    button_bar::ButtonBar, component::Component, text_viewer::TextViewer, top_bar::TopBar,
};

pub enum Events {
    Input(Event),
    Tick,
    Signal(i32),
}

pub enum Action {
    Continue,
    Quit,
    CtrlC,
    Term,
}

#[derive(Debug)]
pub struct App {
    events_rx: mpsc::Receiver<Events>,
    top_bar: TopBar,
    text_viewer: TextViewer,
    button_bar: ButtonBar,
}

impl App {
    pub fn new() -> Result<App> {
        Ok(App {
            events_rx: init_events()?,
            top_bar: TopBar::new()?,
            text_viewer: TextViewer::new()?,
            button_bar: ButtonBar::new()?,
        })
    }

    pub fn handle_events(&mut self) -> Result<Action> {
        let events = self.events_rx.recv()?;

        let mut event_handled = false;

        if !event_handled {
            event_handled = self.top_bar.handle_events(&events)?;
        }

        if !event_handled {
            event_handled = self.text_viewer.handle_events(&events)?;
        }

        if !event_handled {
            event_handled = self.button_bar.handle_events(&events)?;
        }

        if !event_handled {
            match events {
                Events::Input(event) => match event {
                    Event::Key(key) => match key {
                        Key::Char('q') => return Ok(Action::Quit),
                        Key::Char('p') => panic!("at the disco"),
                        Key::Ctrl('c') => return Ok(Action::CtrlC),
                        Key::Ctrl('l') => (), // Redraw
                        _ => (),
                    },
                    Event::Mouse(_mouse) => (),
                    Event::Unsupported(_) => (),
                },
                Events::Tick => (),
                Events::Signal(signal) => match signal {
                    SIGWINCH => (),
                    SIGINT => return Ok(Action::CtrlC),
                    SIGTERM => return Ok(Action::Term),
                    _ => unreachable!(),
                },
            }
        }

        Ok(Action::Continue)
    }

    pub fn render(&mut self, f: &mut Frame) {
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

        self.top_bar.render(f, &chunks[0]);
        self.text_viewer.render(f, &chunks[1]);
        self.button_bar.render(f, &chunks[2]);
    }
}

fn init_events() -> Result<mpsc::Receiver<Events>> {
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

    let tick_rate = Duration::from_millis(5000);

    thread::spawn(move || loop {
        if let Err(err) = tx.send(Events::Tick) {
            eprintln!("{}", err);
            break;
        }
        thread::sleep(tick_rate);
    });

    let mut signals = Signals::new(&[SIGWINCH, SIGINT, SIGTERM])?;

    thread::spawn(move || {
        for signal in &mut signals {
            if let Err(err) = signals_tx.send(Events::Signal(signal)) {
                eprintln!("{}", err);
                return;
            }
        }
    });

    Ok(rx)
}
