use std::{io, panic, path::Path, rc::Rc, thread};

use anyhow::Result;
use crossbeam_channel::{select, unbounded, Receiver, Sender};
use ratatui::{prelude::*, widgets::*};
use termion::{event::*, input::TermRead, terminal_size};

use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;

use crate::{
    button_bar::ButtonBar,
    component::{Component, Focus},
    config::load_config,
    config::Config,
    dlg_error::{DialogType, DlgError},
    dlg_goto::DlgGoto,
    dlg_text_search::{DlgTextSearch, TextSearch},
    text_viewer::TextViewer,
    top_bar::TopBar,
};

pub enum Events {
    Input(Event),
    Signal(i32),
}

pub enum PubSub {
    // App-wide events
    Error(String),
    Warning(String, String),
    CloseDialog,

    // Text viewer events
    Highlight(Vec<Vec<(Color, String)>>),

    // Dialog goto events
    Goto(String),

    // Dialog text search events
    TextSearch(TextSearch),
}

pub enum Action {
    Continue,
    Redraw,
    Quit,
    CtrlC,
    SigTerm,
    CtrlZ,
    SigCont,
}

#[derive(Debug)]
pub struct App {
    config: Config,
    events_rx: Receiver<Events>,
    pubsub_tx: Sender<PubSub>,
    pubsub_rx: Receiver<PubSub>,
    top_bar: TopBar,
    text_viewer: TextViewer,
    button_bar: ButtonBar,
    dialog: Option<Box<dyn Component>>,
}

impl App {
    pub fn new(filename: &Path, tabsize: u8) -> Result<App> {
        let config = load_config()?;

        let (_events_tx, events_rx) = init_events()?;
        let (pubsub_tx, pubsub_rx) = unbounded();

        let (w, h) = terminal_size().unwrap();
        let chunks = get_chunks(&Rect::new(0, 0, w, h));

        Ok(App {
            config,
            events_rx,
            pubsub_tx: pubsub_tx.clone(),
            pubsub_rx,
            top_bar: TopBar::new(&config, filename)?,
            text_viewer: TextViewer::new(
                &config,
                pubsub_tx.clone(),
                &chunks[1],
                filename,
                tabsize,
            )?,
            button_bar: ButtonBar::new(&config)?,
            dialog: None,
        })
    }

    pub fn handle_events(&mut self) -> Result<Action> {
        select! {
            recv(self.events_rx) -> events => match events? {
                Events::Input(event) => match event {
                    Event::Key(key) => {
                        let key_handled = match &mut self.dialog {
                            Some(dlg) => dlg.handle_key(&key)?,
                            None => self.text_viewer.handle_key(&key)?,
                        };

                        if !key_handled {
                            match key {
                                Key::Char('q')
                                | Key::Char('Q')
                                | Key::Char('v')
                                | Key::F(3)
                                | Key::F(10) => return Ok(Action::Quit),
                                Key::Char('p') => panic!("at the disco"),
                                Key::Ctrl('c') => return Ok(Action::CtrlC),
                                Key::Ctrl('l') => return Ok(Action::Redraw),
                                Key::Ctrl('z') => return Ok(Action::CtrlZ),
                                Key::Char(':') | Key::F(5) => {
                                    // TODO: Don't show the dialog if the file size is 0
                                    self.dialog = Some(Box::new(DlgGoto::new(
                                        &self.config,
                                        self.pubsub_tx.clone(),
                                        "Line number: ",
                                    )?));
                                }
                                Key::Char('/')
                                | Key::Char('?')
                                | Key::Char('f')
                                | Key::Char('F')
                                | Key::F(7) => {
                                    // TODO: Don't show the dialog if the file size is 0
                                    self.dialog = Some(Box::new(DlgTextSearch::new(
                                        &self.config,
                                        self.pubsub_tx.clone(),
                                    )?));
                                }
                                _ => log::debug!("{:?}", key),
                            }
                        }
                    }
                    Event::Mouse(mouse) => {
                        self.top_bar.handle_mouse(&mouse)?;

                        match &mut self.dialog {
                            Some(dlg) => dlg.handle_mouse(&mouse)?,
                            None => self.text_viewer.handle_mouse(&mouse)?,
                        };

                        self.button_bar.handle_mouse(&mouse)?;
                    }
                    Event::Unsupported(_) => (),
                },
                Events::Signal(signal) => match signal {
                    SIGWINCH => {
                        let (w, h) = terminal_size().unwrap();
                        let chunks = get_chunks(&Rect::new(0, 0, w, h));

                        self.top_bar.resize(&chunks[0]);
                        self.text_viewer.resize(&chunks[1]);
                        self.button_bar.resize(&chunks[2]);

                        if let Some(dlg) = &mut self.dialog {
                            dlg.resize(&chunks[1]);
                        }
                    }
                    SIGINT => return Ok(Action::CtrlC),
                    SIGTERM => return Ok(Action::SigTerm),
                    SIGCONT => return Ok(Action::SigCont),
                    _ => unreachable!(),
                },
            },
            recv(self.pubsub_rx) -> pubsub => {
                let event = pubsub?;

                self.top_bar.handle_pubsub(&event)?;
                self.text_viewer.handle_pubsub(&event)?;
                self.button_bar.handle_pubsub(&event)?;

                if let Some(dlg) = &mut self.dialog {
                    dlg.handle_pubsub(&event)?;
                }

                match event {
                    PubSub::Error(msg) => {
                        self.dialog = Some(Box::new(DlgError::new(
                            &self.config,
                            self.pubsub_tx.clone(),
                            &msg,
                            "Error",
                            DialogType::Error,
                        )?));
                    },
                    PubSub::Warning(title, msg) => {
                        self.dialog = Some(Box::new(DlgError::new(
                            &self.config,
                            self.pubsub_tx.clone(),
                            &msg,
                            &title,
                            DialogType::Warning,
                        )?));
                    },
                    PubSub::CloseDialog => self.dialog = None,
                    _ => (),
                }
            },
        }
        Ok(Action::Continue)
    }

    pub fn render(&mut self, f: &mut Frame) {
        let chunks = get_chunks(&f.size());

        self.top_bar.render(f, &chunks[0], Focus::Normal);
        self.text_viewer.render(f, &chunks[1], Focus::Normal);
        self.button_bar.render(f, &chunks[2], Focus::Normal);

        if let Some(dlg) = &mut self.dialog {
            dlg.render(f, &chunks[1], Focus::Normal);
        }
    }
}

fn init_events() -> Result<(Sender<Events>, Receiver<Events>)> {
    let (s, r) = unbounded();
    let input_tx = s.clone();
    let signals_tx = s.clone();

    thread::spawn(move || {
        let stdin = io::stdin();
        for event in stdin.events().flatten() {
            if let Err(err) = input_tx.send(Events::Input(event)) {
                eprintln!("{}", err);
                return;
            }
        }
    });

    let mut signals = Signals::new([SIGWINCH, SIGINT, SIGTERM, SIGCONT])?;

    thread::spawn(move || {
        for signal in &mut signals {
            if let Err(err) = signals_tx.send(Events::Signal(signal)) {
                eprintln!("{}", err);
                return;
            }
        }
    });

    Ok((s, r))
}

fn get_chunks(rect: &Rect) -> Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ]
            .as_ref(),
        )
        .split(*rect)
}

pub fn centered_rect(width: u16, height: u16, r: &Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((r.height.saturating_sub(height) + 1) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(*r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((r.width.saturating_sub(width) + 1) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(popup_layout[1])[1]
}

pub fn render_shadow(f: &mut Frame, r: &Rect, s: &Style) {
    let area1 = Rect::new(r.x + 2, r.y + r.height, r.width, 1).intersection(f.size());
    let area2 =
        Rect::new(r.x + r.width, r.y + 1, 2, r.height.saturating_sub(1)).intersection(f.size());

    let block = Block::default().style(*s);

    f.render_widget(block.clone(), area1);
    f.render_widget(block, area2);
}
