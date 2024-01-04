use std::path::Path;

use anyhow::Result;
use crossbeam_channel::{select, Receiver, Sender};
use ratatui::prelude::*;
use termion::event::*;

use signal_hook::consts::signal::*;

use crate::{
    app::{self, init_events, Action, Events, PubSub},
    button_bar::ButtonBar,
    component::{Component, Focus},
    config::load_config,
    config::Config,
    dlg_error::{DialogType, DlgError},
    viewer::{dlg_goto::DlgGoto, dlg_hex_search::DlgHexSearch, dlg_text_search::DlgTextSearch},
};

const LABELS: &[&str] = &[
    " ",      //
    " ",      //
    "View",   //
    "Edit",   //
    "Copy",   //
    "Move",   //
    "Mkdir",  //
    "Delete", //
    " ",      //
    "Quit",   //
];

#[derive(Debug)]
pub struct App {
    config: Config,
    events_rx: Receiver<Events>,
    pubsub_tx: Sender<PubSub>,
    pubsub_rx: Receiver<PubSub>,
    button_bar: ButtonBar,
    dialog: Option<Box<dyn Component>>,
}

impl App {
    pub fn new(
        printwd: Option<&Path>,
        database: Option<&Path>,
        use_db: bool,
        tabsize: u8,
    ) -> Result<App> {
        let config = load_config()?;

        let (_events_tx, events_rx) = init_events()?;
        let (pubsub_tx, pubsub_rx) = crossbeam_channel::unbounded();

        Ok(App {
            config,
            events_rx,
            pubsub_tx: pubsub_tx.clone(),
            pubsub_rx,
            button_bar: ButtonBar::new(&config, LABELS)?,
            dialog: None,
        })
    }
}

impl app::App for App {
    fn handle_events(&mut self) -> Result<Action> {
        select! {
            recv(self.events_rx) -> events => match events? {
                Events::Input(event) => match event {
                    Event::Key(key) => {
                        let key_handled = match &mut self.dialog {
                            Some(dlg) => dlg.handle_key(&key)?,
                            None => false,
                        };

                        if !key_handled {
                            match key {
                                Key::Char('q')
                                | Key::Char('Q')
                                | Key::Char('v')
                                | Key::F(3)
                                | Key::F(10) => return Ok(Action::Quit),
                                //Key::Char('p') => panic!("at the disco"),
                                Key::Ctrl('c') => return Ok(Action::CtrlC),
                                Key::Ctrl('l') => return Ok(Action::Redraw),
                                Key::Ctrl('z') => return Ok(Action::CtrlZ),
                                _ => log::debug!("{:?}", key),
                            }
                        }
                    }
                    Event::Mouse(mouse) => {
                        match &mut self.dialog {
                            Some(dlg) => dlg.handle_mouse(&mouse)?,
                            None => (),
                        };

                        self.button_bar.handle_mouse(&mouse)?;
                    }
                    Event::Unsupported(_) => (),
                },
                Events::Signal(signal) => match signal {
                    SIGWINCH => return Ok(Action::Redraw),
                    SIGINT => return Ok(Action::CtrlC),
                    SIGTERM => return Ok(Action::SigTerm),
                    SIGCONT => return Ok(Action::SigCont),
                    _ => unreachable!(),
                },
            },
            recv(self.pubsub_rx) -> pubsub => {
                let event = pubsub?;

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
                    PubSub::Info(title, msg) => {
                        self.dialog = Some(Box::new(DlgError::new(
                            &self.config,
                            self.pubsub_tx.clone(),
                            &msg,
                            &title,
                            DialogType::Info,
                        )?));
                    },
                    PubSub::CloseDialog => self.dialog = None,
                    PubSub::DlgGoto(goto_type) => {
                        self.dialog = Some(Box::new(DlgGoto::new(
                            &self.config,
                            self.pubsub_tx.clone(),
                            goto_type,
                        )?));
                    },
                    PubSub::DlgTextSearch(text_search) => {
                        self.dialog = Some(Box::new(DlgTextSearch::new(
                            &self.config,
                            self.pubsub_tx.clone(),
                            &text_search,
                        )?));
                    },
                    PubSub::DlgHexSearch(hex_search) => {
                        self.dialog = Some(Box::new(DlgHexSearch::new(
                            &self.config,
                            self.pubsub_tx.clone(),
                            &hex_search,
                        )?));
                    },
                    _ => (),
                }
            },
        }
        Ok(Action::Continue)
    }

    fn render(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(f.size());

        self.button_bar.render(f, &chunks[2], Focus::Normal);

        if let Some(dlg) = &mut self.dialog {
            dlg.render(f, &chunks[1], Focus::Normal);
        }
    }
}
