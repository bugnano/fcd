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
use itertools::Itertools;
use path_clean::PathClean;
use pathdiff::diff_paths;
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
        cp_mv_rm::{dlg_dirscan::DlgDirscan, dlg_question::DlgQuestion},
        entry::Entry,
        file_panel::FilePanel,
        panel::PanelComponent,
        quickview::QuickView,
    },
    shutil::expanduser,
    template,
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

#[derive(Debug, Clone, Copy)]
enum Quote {
    Yes,
    No,
}

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
    archive_mounter: Option<Rc<RefCell<ArchiveMounter>>>,
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

        let archive_mounter = ArchiveMounter::new().map(|mounter| Rc::new(RefCell::new(mounter)));

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
                    archive_mounter.as_ref(),
                    Focus::Focused,
                )),
                Box::new(FilePanel::new(
                    config,
                    bookmarks,
                    pubsub_tx.clone(),
                    initial_path,
                    archive_mounter.as_ref(),
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
            archive_mounter,
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
            PubSub::PromptMkdir => {
                self.command_bar = Some(Box::new(CmdBar::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    CmdBarType::Mkdir,
                    "mkdir: ",
                    "",
                    0,
                )));
            }
            PubSub::Mkdir(directory) => {
                let new_dir =
                    expanduser(&PathBuf::from(&self.apply_template(directory, Quote::No)));

                let new_dir = match new_dir.is_absolute() {
                    true => new_dir.clean(),
                    false => {
                        let focus_position = match self.quickviewer_position {
                            2 => self.panel_focus_position,
                            pos if pos == self.panel_focus_position => {
                                self.panel_focus_position ^ 1
                            }
                            _ => self.panel_focus_position,
                        };

                        let mut cwd = self.panels[focus_position]
                            .get_cwd()
                            .expect("BUG: The focused panel has no working directory set");

                        cwd.push(new_dir);

                        cwd.clean()
                    }
                };

                match fs::create_dir_all(new_dir) {
                    Ok(()) => self.pubsub_tx.send(PubSub::Reload).unwrap(),
                    Err(e) => self.pubsub_tx.send(PubSub::Error(e.to_string())).unwrap(),
                }
            }
            PubSub::PromptRename(file_name, cursor_position) => {
                self.command_bar = Some(Box::new(CmdBar::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    CmdBarType::Rename,
                    "rename: ",
                    file_name,
                    *cursor_position,
                )));
            }
            PubSub::Rename(new_name) => {
                let new_name =
                    expanduser(&PathBuf::from(&self.apply_template(new_name, Quote::No)));

                let focus_position = match self.quickviewer_position {
                    2 => self.panel_focus_position,
                    pos if pos == self.panel_focus_position => self.panel_focus_position ^ 1,
                    _ => self.panel_focus_position,
                };

                let selected_entry = self.panels[focus_position]
                    .get_selected_entry()
                    .expect("BUG: The focused panel has no selected entry");

                let mut new_name = match new_name.is_absolute() {
                    true => new_name.clean(),
                    false => {
                        let mut cwd = self.panels[focus_position]
                            .get_cwd()
                            .expect("BUG: The focused panel has no working directory set");

                        cwd.push(new_name);

                        cwd.clean()
                    }
                };

                if new_name.is_dir() {
                    new_name.push(&selected_entry.file_name);
                }

                let old_name = fs::canonicalize(&selected_entry.file);

                match new_name.try_exists() {
                    Ok(true) => {
                        match (fs::canonicalize(&new_name), old_name) {
                            (Ok(path1), Ok(path2)) if path1 == path2 => {
                                // Renaming a file to itself is a no-op
                            }
                            (Ok(_), Ok(_)) => {
                                self.pubsub_tx
                                    .send(PubSub::Error(String::from("File already exists")))
                                    .unwrap();
                            }
                            (Err(e), _) => {
                                self.pubsub_tx.send(PubSub::Error(e.to_string())).unwrap();
                            }
                            (_, Err(e)) => {
                                self.pubsub_tx.send(PubSub::Error(e.to_string())).unwrap();
                            }
                        }
                    }
                    _ => match fs::rename(&selected_entry.file, &new_name) {
                        Ok(()) => {
                            if let (Ok(old_file), Ok(new_file)) =
                                (old_name, fs::canonicalize(&new_name))
                            {
                                let parent = new_file.parent().unwrap();

                                let old_file_name = selected_entry
                                    .file
                                    .file_name()
                                    .unwrap()
                                    .to_string_lossy()
                                    .to_string();

                                let new_file_name =
                                    new_name.file_name().unwrap().to_string_lossy().to_string();

                                // We need to reload the panels, taking into consideration that
                                // if the selected entry was the renamed file, we need to update the
                                // selected entry to the new name
                                if old_file.parent().unwrap() == parent {
                                    for panel in &mut self.panels {
                                        match panel.get_cwd() {
                                            Some(cwd) => match fs::canonicalize(&cwd) {
                                                Ok(canonical_cwd) if canonical_cwd == parent => {
                                                    match panel.get_selected_entry() {
                                                        Some(entry)
                                                            if entry.file_name == old_file_name =>
                                                        {
                                                            let mut selected_entry =
                                                                PathBuf::from(&cwd);

                                                            selected_entry.push(&new_file_name);

                                                            panel.reload(Some(&selected_entry));
                                                        }
                                                        Some(entry) => {
                                                            panel.reload(Some(&entry.file));
                                                        }
                                                        None => panel.reload(None),
                                                    }
                                                }
                                                _ => match panel.get_selected_entry() {
                                                    Some(entry) => panel.reload(Some(&entry.file)),
                                                    None => panel.reload(None),
                                                },
                                            },
                                            None => panel.reload(None),
                                        }
                                    }
                                } else {
                                    self.pubsub_tx.send(PubSub::Reload).unwrap();
                                }
                            } else {
                                self.pubsub_tx.send(PubSub::Reload).unwrap();
                            }
                        }
                        Err(e) => self.pubsub_tx.send(PubSub::Error(e.to_string())).unwrap(),
                    },
                }
            }
            PubSub::MountArchive(archive) => {
                if let Some(mounter) = &self.archive_mounter {
                    self.pubsub_tx
                        .send(PubSub::Info(
                            mounter.borrow().get_exe_name(),
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

                if let Some(mounter) = &mut self.archive_mounter {
                    let shown_archive = mounter.borrow().archive_path(archive);

                    match mounter.borrow_mut().mount_archive(&shown_archive) {
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
            PubSub::Question(title, question, on_yes) => {
                self.dialog = Some(Box::new(DlgQuestion::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    title,
                    question,
                    on_yes,
                )));
            }
            PubSub::Rm(_entries) => {
                self.dialog = Some(Box::new(DlgDirscan::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                )));
            }
            _ => (),
        }

        action
    }

    fn apply_template(&self, s: &str, quote: Quote) -> String {
        let (focus_position, other_position) = match self.quickviewer_position {
            2 => (self.panel_focus_position, self.panel_focus_position ^ 1),
            pos if pos == self.panel_focus_position => (self.panel_focus_position ^ 1, 2),
            _ => (self.panel_focus_position, 2),
        };

        let fn_quote = |s: &str| -> String {
            match quote {
                Quote::Yes => shlex::try_quote(s).map(String::from).unwrap_or_default(),
                Quote::No => String::from(s),
            }
        };

        let get_file_name_extension = |selected_entry: Option<&Entry>, cwd| {
            let file = fn_quote(
                &selected_entry
                    .map(|entry| {
                        diff_paths(&entry.file, cwd)
                    .expect("BUG: The selected entry should be relative to the working directory")
                    .to_string_lossy()
                    .to_string()
                    })
                    .unwrap_or_default(),
            );

            let name = fn_quote(
                &selected_entry
                    .map(|entry| {
                        tar_stem(
                            &entry
                                .file
                                .file_name()
                                .map(|name| name.to_string_lossy().to_string())
                                .unwrap_or_default(),
                        )
                    })
                    .unwrap_or_default(),
            );

            let extension = fn_quote(
                &selected_entry
                    .map(|entry| {
                        tar_suffix(
                            &entry
                                .file
                                .file_name()
                                .map(|name| name.to_string_lossy().to_string())
                                .unwrap_or_default(),
                        )
                    })
                    .unwrap_or_default(),
            );

            (file, name, extension)
        };

        let cwd = self.panels[focus_position]
            .get_cwd()
            .expect("BUG: The focused panel has no working directory set");

        let other_cwd = self.panels[other_position]
            .get_cwd()
            .expect("BUG: The other panel has no working directory set");

        let (current_file, current_name, current_extension) = get_file_name_extension(
            self.panels[focus_position].get_selected_entry().as_ref(),
            &cwd,
        );

        let (other_file, other_name, other_extension) = get_file_name_extension(
            self.panels[other_position].get_selected_entry().as_ref(),
            &PathBuf::from(""),
        );

        let current_tagged = self.panels[focus_position]
            .get_tagged_files()
            .iter()
            .map(|entry| {
                fn_quote(
                    diff_paths(&entry.file, &cwd)
                        .expect("BUG: The tagged entry should be relative to the working directory")
                        .to_string_lossy()
                        .as_ref(),
                )
            })
            .join(" ");

        let other_tagged = self.panels[other_position]
            .get_tagged_files()
            .iter()
            .map(|entry| fn_quote(entry.file.to_string_lossy().as_ref()))
            .join(" ");

        let current_selected = match current_tagged.is_empty() {
            true => current_file.clone(),
            false => current_tagged.clone(),
        };

        let other_selected = match other_tagged.is_empty() {
            true => other_file.clone(),
            false => other_tagged.clone(),
        };

        // For the base name of the directories, it's more useful to give
        // the archive name instead of the temp. directory name
        let current_base = fn_quote(
            &match &self.archive_mounter {
                Some(mounter) => mounter.borrow().archive_path(&cwd),
                None => cwd.clone(),
            }
            .file_name()
            .map(|base| base.to_string_lossy().to_string())
            .unwrap_or_default(),
        );

        let other_base = fn_quote(
            &match &self.archive_mounter {
                Some(mounter) => mounter.borrow().archive_path(&other_cwd),
                None => other_cwd.clone(),
            }
            .file_name()
            .map(|base| base.to_string_lossy().to_string())
            .unwrap_or_default(),
        );

        let mapping = [
            ("f", current_file),
            ("n", current_name),
            ("e", current_extension),
            ("d", fn_quote(cwd.to_string_lossy().as_ref())),
            ("b", current_base),
            ("s", current_selected),
            ("t", current_tagged),
            ("F", other_file),
            ("N", other_name),
            ("E", other_extension),
            ("D", fn_quote(other_cwd.to_string_lossy().as_ref())),
            ("B", other_base),
            ("S", other_selected),
            ("T", other_tagged),
        ];

        template::substitute(s, mapping, '%')
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
