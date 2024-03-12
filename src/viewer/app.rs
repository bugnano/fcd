use std::{path::Path, rc::Rc};

use anyhow::Result;
use crossbeam_channel::{select, Receiver, Sender};
use ratatui::prelude::*;
use termion::event::*;

use signal_hook::consts::signal::*;

use crate::{
    app::{self, Action, Events, PubSub},
    button_bar::ButtonBar,
    component::{Component, Focus},
    config::Config,
    dlg_error::{DialogType, DlgError},
    viewer::{
        dlg_goto::DlgGoto, dlg_hex_search::DlgHexSearch, dlg_text_search::DlgTextSearch,
        file_viewer::FileViewer, top_bar::TopBar,
    },
};

const LABELS: &[&str] = &[
    " ",      //
    "UnWrap", //
    "Quit",   //
    "Hex",    //
    "Goto",   //
    " ",      //
    "Search", //
    " ",      //
    " ",      //
    "Quit",   //
];

#[derive(Debug)]
pub struct App {
    config: Rc<Config>,
    pubsub_tx: Sender<PubSub>,
    pubsub_rx: Receiver<PubSub>,
    top_bar: TopBar,
    viewer: FileViewer,
    button_bar: ButtonBar,
    dialog: Option<Box<dyn Component>>,
}

impl App {
    pub fn new(config: &Rc<Config>, filename: &Path, tabsize: u8) -> Result<App> {
        let (pubsub_tx, pubsub_rx) = crossbeam_channel::unbounded();

        Ok(App {
            config: Rc::clone(config),
            pubsub_tx: pubsub_tx.clone(),
            pubsub_rx,
            top_bar: TopBar::new(config)?,
            viewer: FileViewer::new(config, pubsub_tx.clone(), filename, tabsize)?,
            button_bar: ButtonBar::new(config, LABELS)?,
            dialog: None,
        })
    }

    fn handle_event(&mut self, event: &Events) -> Result<Action> {
        let mut action = Action::Continue;

        match event {
            Events::Input(input) => match input {
                Event::Key(key) => {
                    let key_handled = match &mut self.dialog {
                        Some(dlg) => dlg.handle_key(key)?,
                        None => self.viewer.handle_key(key)?,
                    };

                    if !key_handled {
                        match key {
                            Key::Char('q')
                            | Key::Char('Q')
                            | Key::Char('v')
                            | Key::F(3)
                            | Key::F(10) => action = Action::Quit,
                            //Key::Char('p') => panic!("at the disco"),
                            Key::Ctrl('c') => action = Action::CtrlC,
                            Key::Ctrl('l') => action = Action::Redraw,
                            Key::Ctrl('z') => action = Action::CtrlZ,
                            _ => log::debug!("{:?}", key),
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    self.top_bar.handle_mouse(mouse)?;

                    match &mut self.dialog {
                        Some(dlg) => dlg.handle_mouse(mouse)?,
                        None => self.viewer.handle_mouse(mouse)?,
                    };

                    self.button_bar.handle_mouse(mouse)?;
                }
                Event::Unsupported(_) => (),
            },
            Events::Signal(signal) => match *signal {
                SIGWINCH => (),
                SIGINT => action = Action::CtrlC,
                SIGTERM => action = Action::SigTerm,
                SIGCONT => action = Action::SigCont,
                _ => unreachable!(),
            },
        }

        Ok(action)
    }

    fn handle_pubsub(&mut self, pubsub: &PubSub) -> Result<Action> {
        let mut action = Action::Continue;

        self.top_bar.handle_pubsub(pubsub)?;
        self.viewer.handle_pubsub(pubsub)?;
        self.button_bar.handle_pubsub(pubsub)?;

        if let Some(dlg) = &mut self.dialog {
            dlg.handle_pubsub(pubsub)?;
        }

        match pubsub {
            PubSub::Error(msg) => {
                self.dialog = Some(Box::new(DlgError::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    msg,
                    "Error",
                    DialogType::Error,
                )?));
            }
            PubSub::Warning(title, msg) => {
                self.dialog = Some(Box::new(DlgError::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    msg,
                    title,
                    DialogType::Warning,
                )?));
            }
            PubSub::Info(title, msg) => {
                self.dialog = Some(Box::new(DlgError::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    msg,
                    title,
                    DialogType::Info,
                )?));

                // Given that the Info dialog is used to show information,
                // stop processing further PubSub events in this loop,
                // in order to show the dialog
                action = Action::NextLoop;
            }
            PubSub::CloseDialog => self.dialog = None,
            PubSub::DlgGoto(goto_type) => {
                self.dialog = Some(Box::new(DlgGoto::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    *goto_type,
                )?));
            }
            PubSub::DlgTextSearch(text_search) => {
                self.dialog = Some(Box::new(DlgTextSearch::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    text_search,
                )?));
            }
            PubSub::DlgHexSearch(hex_search) => {
                self.dialog = Some(Box::new(DlgHexSearch::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    hex_search,
                )?));
            }
            _ => (),
        }

        Ok(action)
    }
}

impl app::App for App {
    fn handle_events(&mut self, events_rx: &mut Receiver<Events>) -> Result<Action> {
        let mut action = select! {
            recv(events_rx) -> event => self.handle_event(&event?)?,
            recv(self.pubsub_rx) -> pubsub => self.handle_pubsub(&pubsub?)?,
        };

        // Key handlers may generate multiple pubsub events.
        // Let's handle them all here, so that there's only 1 redraw per keypress
        if let Action::Continue = action {
            while let Ok(pubsub) = self.pubsub_rx.try_recv() {
                action = self.handle_pubsub(&pubsub)?;
                if !matches!(action, Action::Continue) {
                    break;
                }
            }
        }

        Ok(action)
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

        self.top_bar.render(f, &chunks[0], Focus::Normal);
        self.viewer.render(f, &chunks[1], Focus::Focused);
        self.button_bar.render(f, &chunks[2], Focus::Normal);

        if let Some(dlg) = &mut self.dialog {
            dlg.render(f, &chunks[1], Focus::Focused);
        }
    }
}
