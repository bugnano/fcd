use std::{
    cmp::max,
    env,
    fs::read_dir,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Local};
use crossbeam_channel::{select, Receiver, Sender};
use ratatui::prelude::*;
use termion::event::*;
use unicode_normalization::UnicodeNormalization;

use signal_hook::consts::signal::*;

use crate::{
    app::{self, init_events, Action, Events, PubSub},
    button_bar::ButtonBar,
    component::{Component, Focus},
    config::{load_config, Config},
    dlg_error::{DialogType, DlgError},
    fm::panel::Panel,
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
    panels: Vec<Box<dyn Component>>,
    button_bar: ButtonBar,
    dialog: Option<Box<dyn Component>>,
    panel_focus_position: usize,
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

        let initial_path = match env::current_dir() {
            Ok(cwd) => cwd,
            Err(_) => {
                PathBuf::from(env::var("PWD").context("failed to get current working directory")?)
                    .ancestors()
                    .find(|cwd| read_dir(cwd).is_ok())
                    .unwrap()
                    .to_path_buf()
            }
        };

        Ok(App {
            config,
            events_rx,
            pubsub_tx: pubsub_tx.clone(),
            pubsub_rx,
            panels: vec![
                Box::new(Panel::new(&config, pubsub_tx.clone(), &initial_path)?),
                Box::new(Panel::new(&config, pubsub_tx.clone(), &initial_path)?),
            ],
            button_bar: ButtonBar::new(&config, LABELS)?,
            dialog: None,
            panel_focus_position: 0,
        })
    }

    fn handle_event(&mut self, event: &Events) -> Result<Action> {
        match event {
            Events::Input(input) => match input {
                Event::Key(key) => {
                    let key_handled = match &mut self.dialog {
                        Some(dlg) => dlg.handle_key(key)?,
                        None => self.panels[self.panel_focus_position].handle_key(key)?,
                    };

                    if !key_handled {
                        match key {
                            Key::Char('q') | Key::Char('Q') | Key::F(10) => {
                                return Ok(Action::Quit)
                            }
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
                        Some(dlg) => dlg.handle_mouse(mouse)?,
                        None => (),
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
        for panel in &mut self.panels {
            panel.handle_pubsub(pubsub)?;
        }

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
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(f.size());

        let panel_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Min(1)])
            .split(chunks[0]);

        self.panels[0].render(
            f,
            &panel_chunks[0],
            match self.panel_focus_position {
                0 => Focus::Focused,
                _ => Focus::Normal,
            },
        );
        self.panels[1].render(
            f,
            &panel_chunks[1],
            match self.panel_focus_position {
                1 => Focus::Focused,
                _ => Focus::Normal,
            },
        );

        self.button_bar.render(f, &chunks[2], Focus::Normal);

        if let Some(dlg) = &mut self.dialog {
            dlg.render(f, &chunks[0], Focus::Normal);
        }
    }
}

pub fn tar_stem(file: &str) -> String {
    let parts: Vec<&str> = file.split('.').collect();

    let min_parts = match file.starts_with('.') {
        true => 2,
        false => 1,
    };

    if (parts.len() > (min_parts + 1)) && (parts[parts.len() - 2].to_lowercase() == "tar") {
        parts[..parts.len() - 2].join(".")
    } else if parts.len() > min_parts {
        parts[..parts.len() - 1].join(".")
    } else {
        String::from(file)
    }
}

pub fn tar_suffix(file: &str) -> String {
    let parts: Vec<&str> = file.split('.').collect();

    let min_parts = match file.starts_with('.') {
        true => 2,
        false => 1,
    };

    if (parts.len() > (min_parts + 1)) && (parts[parts.len() - 2].to_lowercase() == "tar") {
        format!(".{}", parts[parts.len() - 2..].join("."))
    } else if parts.len() > min_parts {
        format!(".{}", parts[parts.len() - 1..].join("."))
    } else {
        String::from("")
    }
}

pub fn human_readable_size(size: u64) -> String {
    if size < 1024 {
        return format!("{}B", size);
    }

    // Note: If size is greater than 2**53, then this function doesn't work
    let mut size = size as f64;

    for suffix in ['K', 'M', 'G', 'T', 'P', 'E', 'Z', 'Y'] {
        size /= 1024.0;
        if size < 1024.0 {
            return format!(
                "{:.prec$}{}",
                size,
                suffix,
                prec = max(4_usize.saturating_sub((size as u64).to_string().len()), 1)
            );
        }
    }

    unreachable!();
}

pub fn format_date(d: SystemTime) -> String {
    let d: DateTime<Local> = DateTime::from(d);
    let today = Local::now();

    if d.date_naive() == today.date_naive() {
        format!("{}", d.format(" %H:%M "))
    } else if d.year() == today.year() {
        format!("{}", d.format(" %b %d"))
    } else {
        format!("{}", d.format("%Y-%m"))
    }
}

pub fn natsort_key(s: &str) -> String {
    caseless::default_case_fold_str(s).nfkd().collect()
}
