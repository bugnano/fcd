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
    palette::Palette,
    viewer::{
        dlg_goto::DlgGoto, dlg_hex_search::DlgHexSearch, dlg_text_search::DlgTextSearch,
        file_viewer::FileViewer, top_bar::TopBar,
    },
};

pub const LABELS: &[&str] = &[
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
    palette: Rc<Palette>,
    pubsub_tx: Sender<PubSub>,
    pubsub_rx: Receiver<PubSub>,
    top_bar: TopBar,
    viewer: FileViewer,
    button_bar: ButtonBar,
    dialog: Option<Box<dyn Component>>,
    ctrl_o: bool,
}

impl App {
    pub fn new(
        config: &Rc<Config>,
        palette: &Rc<Palette>,
        filename: &Path,
        tabsize: u8,
    ) -> Result<App> {
        let (pubsub_tx, pubsub_rx) = crossbeam_channel::unbounded();

        Ok(App {
            config: Rc::clone(config),
            palette: Rc::clone(palette),
            pubsub_tx: pubsub_tx.clone(),
            pubsub_rx,
            top_bar: TopBar::new(palette),
            viewer: FileViewer::new(palette, pubsub_tx.clone(), filename, tabsize)?,
            button_bar: ButtonBar::new(palette, LABELS),
            dialog: None,
            ctrl_o: false,
        })
    }

    fn handle_event(&mut self, event: &Events) -> Action {
        let mut action = Action::Continue;

        match event {
            Events::Input(input) => {
                match self.ctrl_o {
                    true => {
                        if let Event::Key(key) = input {
                            match key {
                                Key::Char('\n') => {
                                    self.ctrl_o = false;
                                    action = Action::ExitCtrlO;
                                }
                                Key::Ctrl('c') => action = Action::CtrlC,
                                Key::Ctrl('z') => action = Action::CtrlZ,
                                _ => (),
                            }
                        }
                    }
                    false => match input {
                        Event::Key(key) => {
                            let key_handled = match &mut self.dialog {
                                Some(dlg) => dlg.handle_key(key),
                                None => self.viewer.handle_key(key),
                            };

                            if !key_handled {
                                match key {
                                    Key::Char('q')
                                    | Key::Char('Q')
                                    | Key::Char('v')
                                    | Key::F(3)
                                    | Key::F(10)
                                    | Key::Char('3')
                                    | Key::Char('0') => action = Action::Quit,
                                    //Key::Char('p') => panic!("at the disco"),
                                    Key::Ctrl('c') => action = Action::CtrlC,
                                    Key::Ctrl('l') => action = Action::Redraw,
                                    Key::Ctrl('z') => action = Action::CtrlZ,
                                    Key::Ctrl('o') => {
                                        self.ctrl_o = true;
                                        action = Action::CtrlO;
                                    }
                                    _ => log::debug!("{:?}", key),
                                }
                            }
                        }
                        Event::Mouse(mouse) => {
                            self.top_bar.handle_mouse(mouse);

                            match &mut self.dialog {
                                Some(dlg) => dlg.handle_mouse(mouse),
                                None => self.viewer.handle_mouse(mouse),
                            };

                            self.button_bar.handle_mouse(mouse);
                        }
                        Event::Unsupported(_) => (),
                    },
                }
            }
            Events::Signal(signal) => match *signal {
                SIGWINCH => (),
                SIGINT => (),
                SIGTERM => action = Action::SigTerm,
                SIGCONT => action = Action::SigCont,
                _ => unreachable!(),
            },
        }

        action
    }

    fn handle_pubsub(&mut self, pubsub: &PubSub) -> Action {
        let mut action = Action::Continue;

        self.top_bar.handle_pubsub(pubsub);
        self.viewer.handle_pubsub(pubsub);
        self.button_bar.handle_pubsub(pubsub);

        if let Some(dlg) = &mut self.dialog {
            dlg.handle_pubsub(pubsub);
        }

        match pubsub {
            PubSub::Error(msg, next_action) => {
                self.dialog = Some(Box::new(DlgError::new(
                    &self.palette,
                    self.pubsub_tx.clone(),
                    msg,
                    "Error",
                    DialogType::Error,
                    next_action.clone(),
                )));
            }
            PubSub::Warning(title, msg) => {
                self.dialog = Some(Box::new(DlgError::new(
                    &self.palette,
                    self.pubsub_tx.clone(),
                    msg,
                    title,
                    DialogType::Warning,
                    None,
                )));
            }
            PubSub::Info(title, msg) => {
                self.dialog = Some(Box::new(DlgError::new(
                    &self.palette,
                    self.pubsub_tx.clone(),
                    msg,
                    title,
                    DialogType::Info,
                    None,
                )));

                // Given that the Info dialog is used to show information,
                // stop processing further PubSub events in this loop,
                // in order to show the dialog
                action = Action::NextLoop;
            }
            PubSub::CloseDialog => self.dialog = None,
            PubSub::Redraw => action = Action::Redraw,
            PubSub::DlgGoto(goto_type) => {
                self.dialog = Some(Box::new(DlgGoto::new(
                    &self.palette,
                    self.pubsub_tx.clone(),
                    *goto_type,
                )));
            }
            PubSub::DlgTextSearch(text_search) => {
                self.dialog = Some(Box::new(DlgTextSearch::new(
                    &self.palette,
                    self.pubsub_tx.clone(),
                    text_search,
                )));
            }
            PubSub::DlgHexSearch(hex_search) => {
                self.dialog = Some(Box::new(DlgHexSearch::new(
                    &self.palette,
                    self.pubsub_tx.clone(),
                    hex_search,
                )));
            }
            _ => (),
        }

        action
    }
}

impl app::App for App {
    fn handle_events(&mut self, events_rx: &mut Receiver<Events>) -> Action {
        let mut action = select! {
            recv(events_rx) -> event => self.handle_event(&event.unwrap()),
            recv(self.pubsub_rx) -> pubsub => self.handle_pubsub(&pubsub.unwrap()),
        };

        // Key handlers may generate multiple pubsub events.
        // Let's handle them all here, so that there's only 1 redraw per keypress
        if let Action::Continue = action {
            while let Ok(pubsub) = self.pubsub_rx.try_recv() {
                action = self.handle_pubsub(&pubsub);
                if !matches!(action, Action::Continue) {
                    break;
                }
            }
        }

        action
    }

    fn render(&mut self, f: &mut Frame) {
        let mut constraints = vec![Constraint::Length(1), Constraint::Min(1)];

        if self.config.options.show_button_bar {
            constraints.push(Constraint::Length(1));
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(&constraints)
            .split(f.area());

        self.top_bar.render(f, &chunks[0], Focus::Normal);
        self.viewer.render(f, &chunks[1], Focus::Focused);

        if self.config.options.show_button_bar {
            self.button_bar.render(f, &chunks[2], Focus::Normal);
        }

        if let Some(dlg) = &mut self.dialog {
            dlg.render(f, &chunks[1], Focus::Focused);
        }
    }
}
