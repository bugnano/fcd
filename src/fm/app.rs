use std::{
    cell::RefCell,
    cmp::max,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    time::SystemTime,
};

use anyhow::Result;
use crossbeam_channel::{select, Receiver, Sender};
use ratatui::prelude::*;
use termion::event::*;

use chrono::{DateTime, Datelike, Local};
use signal_hook::consts::signal::*;
use unicode_normalization::UnicodeNormalization;

use crate::{
    app::{self, Action, Events, PubSub},
    button_bar::ButtonBar,
    component::{Component, Focus},
    config::Config,
    dlg_error::{DialogType, DlgError},
    fm::{
        archive_mounter::ArchiveMounter,
        bookmarks::Bookmarks,
        command_bar::{
            cmdbar::{CmdBar, CmdBarType},
            filter::Filter,
            leader::Leader,
        },
        file_panel::FilePanel,
        panel::PanelComponent,
        quickview::QuickView,
    },
    viewer::{
        self, dlg_goto::DlgGoto, dlg_hex_search::DlgHexSearch, dlg_text_search::DlgTextSearch,
    },
};

pub const LABELS: &[&str] = &[
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
    config: Rc<Config>,
    pubsub_tx: Sender<PubSub>,
    pubsub_rx: Receiver<PubSub>,
    panels: Vec<Box<dyn PanelComponent>>,
    command_bar: Option<Box<dyn Component>>,
    button_bar: ButtonBar,
    dialog: Option<Box<dyn Component>>,
    fg_app: Option<Box<dyn app::App>>,
    panel_focus_position: usize,
    quickviewer_position: usize,
    printwd: Option<PathBuf>,
    tabsize: u8,
    ctrl_o: bool,
    archive_mounter: Option<ArchiveMounter>,
}

impl App {
    pub fn new(
        config: &Rc<Config>,
        bookmarks: &Rc<RefCell<Bookmarks>>,
        initial_path: &Path,
        printwd: Option<&Path>,
        database: Option<&Path>,
        use_db: bool,
        tabsize: u8,
    ) -> Result<App> {
        let (pubsub_tx, pubsub_rx) = crossbeam_channel::unbounded();

        Ok(App {
            config: Rc::clone(config),
            pubsub_tx: pubsub_tx.clone(),
            pubsub_rx,
            panels: vec![
                Box::new(FilePanel::new(
                    config,
                    bookmarks,
                    pubsub_tx.clone(),
                    initial_path,
                    Focus::Focused,
                )),
                Box::new(FilePanel::new(
                    config,
                    bookmarks,
                    pubsub_tx.clone(),
                    initial_path,
                    Focus::Normal,
                )),
                Box::new(QuickView::new(
                    config,
                    pubsub_tx.clone(),
                    tabsize,
                    Focus::Normal,
                )),
            ],
            command_bar: None,
            button_bar: ButtonBar::new(config, LABELS),
            dialog: None,
            fg_app: None,
            panel_focus_position: 0,
            quickviewer_position: 2,
            printwd: printwd.map(PathBuf::from),
            tabsize,
            ctrl_o: false,
            archive_mounter: ArchiveMounter::new(),
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
                                None => {
                                    let mut key_handled = match &mut self.command_bar {
                                        Some(command_bar) => command_bar.handle_key(key),
                                        None => false,
                                    };

                                    if !key_handled {
                                        key_handled =
                                            self.panels[self.panel_focus_position].handle_key(key);
                                    }

                                    key_handled
                                }
                            };

                            if !key_handled {
                                match key {
                                    Key::Char('q') | Key::Char('Q') | Key::F(10) => {
                                        action = Action::Quit;

                                        // This assumes that there are always 2 panels visible
                                        let cwd = if self.panel_focus_position
                                            == self.quickviewer_position
                                        {
                                            self.panels[self.panel_focus_position ^ 1].get_cwd()
                                        } else {
                                            self.panels[self.panel_focus_position].get_cwd()
                                        };

                                        if let (Some(pwd), Some(cwd)) = (&self.printwd, cwd) {
                                            let _ =
                                                fs::write(pwd, cwd.as_os_str().as_encoded_bytes());
                                        }
                                    }
                                    //Key::Char('p') => panic!("at the disco"),
                                    Key::Ctrl('c') => action = Action::CtrlC,
                                    Key::Ctrl('l') => action = Action::Redraw,
                                    Key::Ctrl('z') => action = Action::CtrlZ,
                                    Key::Ctrl('o') => {
                                        self.ctrl_o = true;
                                        action = Action::CtrlO;
                                    }
                                    Key::Esc => self.pubsub_tx.send(PubSub::Esc).unwrap(),
                                    Key::BackTab => {
                                        self.panels[self.panel_focus_position]
                                            .change_focus(Focus::Normal);

                                        // This assumes that there are always 2 panels visible
                                        self.panel_focus_position ^= 1;

                                        self.panels[self.panel_focus_position]
                                            .change_focus(Focus::Focused);
                                    }
                                    Key::Char('\t') => {
                                        self.panels[self.panel_focus_position]
                                            .change_focus(Focus::Normal);

                                        // This assumes that there are always 2 panels visible
                                        self.panel_focus_position ^= 1;

                                        self.panels[self.panel_focus_position]
                                            .change_focus(Focus::Focused);
                                    }
                                    Key::Ctrl('u') => {
                                        // This assumes that there are always 2 panels visible
                                        self.panels.swap(0, 1);
                                        self.panel_focus_position ^= 1;

                                        if self.quickviewer_position < 2 {
                                            self.quickviewer_position ^= 1;
                                        }
                                    }
                                    Key::Alt('q') => {
                                        // This assumes that there are always 2 panels visible
                                        if self.quickviewer_position < 2 {
                                            let quickviewer_position = self.quickviewer_position;

                                            if self.panel_focus_position == quickviewer_position {
                                                self.panels[self.panel_focus_position]
                                                    .change_focus(Focus::Normal);
                                            }

                                            self.panels.swap(self.quickviewer_position, 2);

                                            self.quickviewer_position = 2;

                                            if self.panel_focus_position == quickviewer_position {
                                                self.panels[self.panel_focus_position]
                                                    .change_focus(Focus::Focused);
                                            }
                                        } else {
                                            self.quickviewer_position =
                                                self.panel_focus_position ^ 1;

                                            self.panels.swap(self.quickviewer_position, 2);
                                        }

                                        self.pubsub_tx
                                            .send(PubSub::ToggleQuickView(
                                                self.panels[self.panel_focus_position]
                                                    .get_selected_entry(),
                                            ))
                                            .unwrap();
                                    }
                                    Key::Alt('i') => {
                                        // This assumes that there are always 2 panels visible
                                        let other_panel = match self.quickviewer_position {
                                            2 => self.panel_focus_position ^ 1,
                                            _ => 2,
                                        };

                                        if let Some(cwd) =
                                            self.panels[self.panel_focus_position].get_cwd()
                                        {
                                            self.panels[other_panel].chdir(&cwd);
                                        }
                                    }
                                    Key::Alt('o') => {
                                        // This assumes that there are always 2 panels visible
                                        let other_panel = match self.quickviewer_position {
                                            2 => self.panel_focus_position ^ 1,
                                            _ => 2,
                                        };

                                        if let Some(cwd) =
                                            self.panels[self.panel_focus_position].get_cwd()
                                        {
                                            let target_cwd = match self.panels
                                                [self.panel_focus_position]
                                                .get_selected_entry()
                                            {
                                                Some(entry) => match entry.stat.is_dir() {
                                                    true => entry.file,
                                                    false => {
                                                        PathBuf::from(cwd.parent().unwrap_or(&cwd))
                                                    }
                                                },
                                                None => PathBuf::from(cwd.parent().unwrap_or(&cwd)),
                                            };

                                            self.panels[other_panel].chdir(&target_cwd);
                                        }
                                    }
                                    _ => log::debug!("{:?}", key),
                                }
                            }
                        }
                        Event::Mouse(mouse) => {
                            match &mut self.dialog {
                                Some(dlg) => dlg.handle_mouse(mouse),
                                None => (),
                            };

                            self.button_bar.handle_mouse(mouse);
                        }
                        Event::Unsupported(_) => (),
                    },
                }
            }
            Events::Signal(signal) => match *signal {
                SIGWINCH => (),
                SIGINT => action = Action::CtrlC,
                SIGTERM => action = Action::SigTerm,
                SIGCONT => action = Action::SigCont,
                _ => unreachable!(),
            },
        }

        action
    }

    fn handle_pubsub(&mut self, pubsub: &PubSub) -> Action {
        let mut action = Action::Continue;

        for panel in &mut self.panels {
            panel.handle_pubsub(pubsub);
        }

        if let Some(command_bar) = &mut self.command_bar {
            command_bar.handle_pubsub(pubsub);
        }

        self.button_bar.handle_pubsub(pubsub);

        if let Some(dlg) = &mut self.dialog {
            dlg.handle_pubsub(pubsub);
        }

        match pubsub {
            PubSub::Error(msg) => {
                self.dialog = Some(Box::new(DlgError::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    msg,
                    "Error",
                    DialogType::Error,
                )));
            }
            PubSub::Warning(title, msg) => {
                self.dialog = Some(Box::new(DlgError::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    msg,
                    title,
                    DialogType::Warning,
                )));
            }
            PubSub::Info(title, msg) => {
                self.dialog = Some(Box::new(DlgError::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    msg,
                    title,
                    DialogType::Info,
                )));

                // Given that the Info dialog is used to show information,
                // stop processing further PubSub events in this loop,
                // in order to show the dialog
                action = Action::NextLoop;
            }
            PubSub::CloseDialog => self.dialog = None,
            PubSub::Esc => self.command_bar = None,
            PubSub::DlgGoto(goto_type) => {
                self.dialog = Some(Box::new(DlgGoto::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    *goto_type,
                )));
            }
            PubSub::DlgTextSearch(text_search) => {
                self.dialog = Some(Box::new(DlgTextSearch::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    text_search,
                )));
            }
            PubSub::DlgHexSearch(hex_search) => {
                self.dialog = Some(Box::new(DlgHexSearch::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    hex_search,
                )));
            }
            PubSub::ViewFile(file) => {
                // TODO: If there's an external viewer configured, run that viewer
                if let Ok(app) = viewer::app::App::new(&self.config, file, self.tabsize) {
                    self.fg_app = Some(Box::new(app));
                }
            }
            PubSub::CloseCommandBar => self.command_bar = None,
            PubSub::Leader(leader) => {
                self.command_bar = match leader {
                    Some(c) => Some(Box::new(Leader::new(&self.config, *c))),
                    None => None,
                }
            }
            PubSub::PromptFileFilter(filter) => {
                self.command_bar = Some(Box::new(Filter::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    filter,
                )));
            }
            PubSub::PromptTagGlob => {
                self.command_bar = Some(Box::new(CmdBar::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    CmdBarType::TagGlob,
                    "tag: ",
                    "*",
                    1,
                )));
            }
            PubSub::PromptUntagGlob => {
                self.command_bar = Some(Box::new(CmdBar::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    CmdBarType::UntagGlob,
                    "untag: ",
                    "*",
                    1,
                )));
            }
            PubSub::MountArchive(archive) => {
                if let Some(archive_mounter) = &self.archive_mounter {
                    self.pubsub_tx
                        .send(PubSub::Info(
                            archive_mounter.get_exe_name(),
                            String::from("Opening archive..."),
                        ))
                        .unwrap();

                    self.pubsub_tx
                        .send(PubSub::DoMountArchive(archive.clone()))
                        .unwrap();
                }
            }
            PubSub::DoMountArchive(archive) => {
                self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                if let Some(archive_mounter) = &mut self.archive_mounter {
                    let shown_archive = archive_mounter.archive_path(&archive);

                    match archive_mounter.mount_archive(&shown_archive) {
                        Ok(temp_dir) => {
                            self.pubsub_tx
                                .send(PubSub::ArchiveMounted(archive.clone(), temp_dir))
                                .unwrap();
                        }
                        Err(e) => {
                            self.pubsub_tx
                                .send(PubSub::ArchiveMountError(archive.clone(), e.to_string()))
                                .unwrap();
                        }
                    }
                }
            }

            _ => (),
        }

        action
    }
}

impl app::App for App {
    fn handle_events(&mut self, events_rx: &mut Receiver<Events>) -> Action {
        if let Some(app) = &mut self.fg_app {
            let mut action = app.handle_events(events_rx);

            if let Action::Quit = action {
                self.fg_app = None;
                action = Action::Continue;
            }

            return action;
        }

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
        if let Some(app) = &mut self.fg_app {
            app.render(f);
            return;
        }

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

        if let Some(command_bar) = &mut self.command_bar {
            command_bar.render(
                f,
                &chunks[1],
                match &self.dialog {
                    Some(_) => Focus::Normal,
                    None => Focus::Focused,
                },
            );
        }

        self.button_bar.render(f, &chunks[2], Focus::Normal);

        if let Some(dlg) = &mut self.dialog {
            dlg.render(f, &chunks[0], Focus::Focused);
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
