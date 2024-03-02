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
    config::{load_config, Config},
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
    config: Config,
    events_rx: Receiver<Events>,
    pubsub_tx: Sender<PubSub>,
    pubsub_rx: Receiver<PubSub>,
    top_bar: TopBar,
    viewer: FileViewer,
    button_bar: ButtonBar,
    dialog: Option<Box<dyn Component>>,
}

impl App {
    pub fn new(filename: &Path, tabsize: u8) -> Result<App> {
        let config = load_config()?;

        let (_events_tx, events_rx) = init_events()?;
        let (pubsub_tx, pubsub_rx) = crossbeam_channel::unbounded();

        Ok(App {
            config,
            events_rx,
            pubsub_tx: pubsub_tx.clone(),
            pubsub_rx,
            top_bar: TopBar::new(&config)?,
            viewer: FileViewer::new(&config, pubsub_tx.clone(), filename, tabsize)?,
            button_bar: ButtonBar::new(&config, LABELS)?,
            dialog: None,
        })
    }

    fn handle_event(&mut self, event: &Events) -> Result<Action> {
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
                SIGWINCH => return Ok(Action::Redraw),
                SIGINT => return Ok(Action::CtrlC),
                SIGTERM => return Ok(Action::SigTerm),
                SIGCONT => return Ok(Action::SigCont),
                _ => unreachable!(),
            },
        }

        Ok(Action::Continue)
    }

    fn handle_pubsub(&mut self, pubsub: &PubSub) -> Result<Action> {
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

        Ok(Action::Continue)
    }
}

impl app::App for App {
    fn handle_events(&mut self) -> Result<Action> {
        select! {
            recv(self.events_rx) -> event => self.handle_event(&event?),
            recv(self.pubsub_rx) -> pubsub => self.handle_pubsub(&pubsub?),
        }
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
