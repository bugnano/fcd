use std::{
    cell::RefCell,
    cmp::max,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process,
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
        archive_mounter::{self, ArchiveEntry, ArchiveMounterCommand},
        bookmarks::Bookmarks,
        command_bar::{
            cmdbar::{CmdBar, CmdBarType},
            command_bar_error::CommandBarError,
            component::CommandBarComponent,
            filter::Filter,
            leader::Leader,
        },
        cp_mv_rm::{
            database::{
                DBEntriesEntry, DBFileStatus, DBJobEntry, DBJobOperation, DBJobStatus, DataBase,
                OnConflict,
            },
            dlg_cp_mv::DlgCpMv,
            dlg_cp_mv_progress::DlgCpMvProgress,
            dlg_dirscan::DlgDirscan,
            dlg_pending_job::DlgPendingJob,
            dlg_question::DlgQuestion,
            dlg_report::DlgReport,
            dlg_rm_progress::DlgRmProgress,
        },
        dlg_mount_archive::DlgMountArchive,
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
    command_bar: Option<Box<dyn CommandBarComponent>>,
    button_bar: ButtonBar,
    dialog: Option<Box<dyn Component>>,
    fg_app: Option<Box<dyn app::App>>,
    panel_focus_position: usize,
    quickviewer_position: usize,
    printwd: Option<PathBuf>,
    db_file: Option<PathBuf>,
    tabsize: u8,
    ctrl_o: bool,
    archive_mounter_command_tx: Option<Sender<ArchiveMounterCommand>>,
    pending_jobs: Vec<DBJobEntry>,
    pending_job: Option<DBJobEntry>,
    pending_archives: Vec<PathBuf>,
}

impl App {
    pub fn new(
        config: &Rc<Config>,
        bookmarks: &Rc<RefCell<Bookmarks>>,
        initial_path: &Path,
        printwd: Option<&Path>,
        db_file: Option<&Path>,
        tabsize: u8,
    ) -> Result<App> {
        let (pubsub_tx, pubsub_rx) = crossbeam_channel::unbounded();

        let archive_mounter_command_tx = archive_mounter::start();

        let pending_jobs: Vec<DBJobEntry> = db_file
            .and_then(|db_file| DataBase::new(db_file).ok())
            .map(|mut db| db.get_pending_jobs(process::id(), fs::canonicalize("/proc/self/exe")))
            .unwrap_or_default();

        pubsub_tx.send(PubSub::NextPendingJob).unwrap();

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
                    archive_mounter_command_tx.clone(),
                    Focus::Focused,
                )),
                Box::new(FilePanel::new(
                    config,
                    bookmarks,
                    pubsub_tx.clone(),
                    initial_path,
                    archive_mounter_command_tx.clone(),
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
            db_file: db_file.map(PathBuf::from),
            tabsize,
            ctrl_o: false,
            archive_mounter_command_tx,
            pending_jobs,
            pending_job: None,
            pending_archives: Vec::new(),
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
                            let focus_command_bar = self
                                .command_bar
                                .as_ref()
                                .map(|command_bar| command_bar.is_focusable())
                                .unwrap_or(false);

                            let key_handled = match focus_command_bar {
                                true => self.command_bar.as_mut().unwrap().handle_key(key),
                                false => match &mut self.dialog {
                                    Some(dlg) => dlg.handle_key(key),
                                    None => self.panels[self.panel_focus_position].handle_key(key),
                                },
                            };

                            if !key_handled {
                                match key {
                                    Key::Char('q')
                                    | Key::Char('Q')
                                    | Key::F(10)
                                    | Key::Char('0') => {
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
            PubSub::Error(msg, next_action) => {
                self.dialog = Some(Box::new(DlgError::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    msg,
                    "Error",
                    DialogType::Error,
                    next_action.clone(),
                )));
            }
            PubSub::Warning(title, msg) => {
                self.dialog = Some(Box::new(DlgError::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    msg,
                    title,
                    DialogType::Warning,
                    None,
                )));
            }
            PubSub::Info(title, msg) => {
                self.dialog = Some(Box::new(DlgError::new(
                    &self.config,
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
                    Err(e) => {
                        self.pubsub_tx
                            .send(PubSub::Error(e.to_string(), None))
                            .unwrap();
                    }
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
                                    .send(PubSub::Error(String::from("File already exists"), None))
                                    .unwrap();
                            }
                            (Err(e), _) => {
                                self.pubsub_tx
                                    .send(PubSub::Error(e.to_string(), None))
                                    .unwrap();
                            }
                            (_, Err(e)) => {
                                self.pubsub_tx
                                    .send(PubSub::Error(e.to_string(), None))
                                    .unwrap();
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
                        Err(e) => self
                            .pubsub_tx
                            .send(PubSub::Error(e.to_string(), None))
                            .unwrap(),
                    },
                }
            }
            PubSub::MountArchive(archive) => {
                if let Some(command_tx) = &self.archive_mounter_command_tx {
                    self.dialog = Some(Box::new(DlgMountArchive::new(
                        &self.config,
                        self.pubsub_tx.clone(),
                        archive,
                        command_tx,
                    )));
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
            PubSub::Rm(cwd, entries) => {
                let mut archive_dirs = match &self.archive_mounter_command_tx {
                    Some(command_tx) => archive_mounter::get_archive_dirs(command_tx),
                    None => Vec::new(),
                };

                if let Some(command_tx) = &self.archive_mounter_command_tx {
                    // If the files that we're deleting are (parents of) mounted archives,
                    // we need to umount those archives before deleting.
                    let parents: Vec<PathBuf> = entries
                        .iter()
                        .map(|entry| archive_mounter::archive_path_map(&entry.file, &archive_dirs))
                        .collect();

                    archive_mounter::umount_parents(command_tx, &parents);

                    // Given that we umounted some archives, we need to fetch the new archives list
                    archive_dirs = archive_mounter::get_archive_dirs(command_tx);
                }

                let archive_cwd = archive_mounter::archive_path_map(cwd, &archive_dirs);

                // We only care about the archives that are (parents of) cwd
                archive_dirs = archive_dirs
                    .iter()
                    .filter(|entry| {
                        let ancestor_of_cwd = archive_cwd
                            .ancestors()
                            .any(|ancestor| entry.archive_file == ancestor);

                        ancestor_of_cwd
                    })
                    .cloned()
                    .collect();

                let mut job = DBJobEntry {
                    id: 0,
                    pid: process::id(),
                    operation: DBJobOperation::Rm,
                    cwd: archive_cwd,
                    dest: None,
                    on_conflict: None,
                    replace_first_path: false,
                    status: DBJobStatus::Dirscan,
                    entries: self.db_entries_from_entries(entries, &archive_dirs),
                    archives: archive_dirs
                        .iter()
                        .map(|archive_dir| archive_dir.archive_file.clone())
                        .collect(),
                };

                self.db_file
                    .as_deref()
                    .and_then(|db_file| DataBase::new(db_file).ok())
                    .map(|mut db| db.new_job(&mut job));

                self.dialog = Some(Box::new(DlgDirscan::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    &job,
                    &archive_dirs,
                    self.db_file.as_deref(),
                )));
            }
            PubSub::DoRm(job, files, archive_dirs) => {
                self.dialog = Some(Box::new(DlgRmProgress::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    job,
                    files,
                    archive_dirs,
                    self.db_file.as_deref(),
                )));
            }
            PubSub::Cp(cwd, entries) | PubSub::Mv(cwd, entries) => {
                let other_position = match self.quickviewer_position {
                    2 => self.panel_focus_position ^ 1,
                    _ => 2,
                };

                let other_cwd = self.panels[other_position]
                    .get_cwd()
                    .expect("BUG: The other panel has no working directory set");

                let dest = self
                    .archive_path(&other_cwd)
                    .to_string_lossy()
                    .replace('%', "%%");

                let operation = match pubsub {
                    PubSub::Cp(_cwd, _entries) => DBJobOperation::Cp,
                    PubSub::Mv(_cwd, _entries) => DBJobOperation::Mv,
                    _ => unreachable!(),
                };

                self.dialog = Some(Box::new(DlgCpMv::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    cwd,
                    entries,
                    &dest,
                    operation,
                )));
            }
            PubSub::DoDirscan(cwd, entries, str_dest, on_conflict, operation) => {
                let archive_dest =
                    expanduser(&PathBuf::from(&self.apply_template(str_dest, Quote::No)));

                let archive_dest_parent = archive_dest
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or(PathBuf::from("/"));

                let dest = self.unarchive_path(&archive_dest);
                let dest_parent = self.unarchive_path(&archive_dest_parent);

                let mut do_dirscan = true;

                match dest.is_dir() {
                    true => {
                        match (fs::canonicalize(cwd), fs::canonicalize(&dest)) {
                            (Ok(canonical_cwd), Ok(canonical_dest))
                                if canonical_cwd == canonical_dest =>
                            {
                                if matches!(operation, DBJobOperation::Mv)
                                    || matches!(on_conflict, OnConflict::Overwrite)
                                    || matches!(on_conflict, OnConflict::Skip)
                                {
                                    // no-op
                                    do_dirscan = false;
                                }
                            }
                            _ => {}
                        }
                    }
                    false => {
                        if entries.len() == 1 {
                            match dest_parent.is_dir() {
                                true => {
                                    if entries[0].file.file_name().unwrap_or_default()
                                        == dest.file_name().unwrap_or_default()
                                    {
                                        match (
                                            fs::canonicalize(cwd),
                                            fs::canonicalize(&dest_parent),
                                        ) {
                                            (Ok(canonical_cwd), Ok(canonical_dest))
                                                if canonical_cwd == canonical_dest =>
                                            {
                                                if matches!(operation, DBJobOperation::Mv)
                                                    || matches!(on_conflict, OnConflict::Overwrite)
                                                    || matches!(on_conflict, OnConflict::Skip)
                                                {
                                                    // no-op
                                                    do_dirscan = false;
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                false => {
                                    self.pubsub_tx
                                        .send(PubSub::Error(
                                            format!(
                                                "{} is not a directory",
                                                archive_dest_parent.to_string_lossy()
                                            ),
                                            None,
                                        ))
                                        .unwrap();

                                    do_dirscan = false;
                                }
                            }
                        } else {
                            self.pubsub_tx
                                .send(PubSub::Error(
                                    format!(
                                        "{} is not a directory",
                                        archive_dest.to_string_lossy()
                                    ),
                                    None,
                                ))
                                .unwrap();

                            do_dirscan = false;
                        }
                    }
                }

                if do_dirscan {
                    let mut archive_dirs = match &self.archive_mounter_command_tx {
                        Some(command_tx) => archive_mounter::get_archive_dirs(command_tx),
                        None => Vec::new(),
                    };

                    let archive_cwd = archive_mounter::archive_path_map(cwd, &archive_dirs);

                    // We only care about the archives that are (parents of) cwd or dest
                    archive_dirs = archive_dirs
                        .iter()
                        .filter(|entry| {
                            let ancestor_of_cwd = archive_cwd
                                .ancestors()
                                .any(|ancestor| entry.archive_file == ancestor);

                            let ancestor_of_dest = archive_dest
                                .ancestors()
                                .any(|ancestor| entry.archive_file == ancestor);

                            ancestor_of_cwd || ancestor_of_dest
                        })
                        .cloned()
                        .collect();

                    let mut job = DBJobEntry {
                        id: 0,
                        pid: process::id(),
                        operation: *operation,
                        cwd: archive_cwd,
                        dest: Some(archive_dest.clone()),
                        on_conflict: Some(*on_conflict),
                        replace_first_path: !dest.is_dir(),
                        status: DBJobStatus::Dirscan,
                        entries: self.db_entries_from_entries(entries, &archive_dirs),
                        archives: archive_dirs
                            .iter()
                            .map(|archive_dir| archive_dir.archive_file.clone())
                            .collect(),
                    };

                    self.db_file
                        .as_deref()
                        .and_then(|db_file| DataBase::new(db_file).ok())
                        .map(|mut db| db.new_job(&mut job));

                    self.dialog = Some(Box::new(DlgDirscan::new(
                        &self.config,
                        self.pubsub_tx.clone(),
                        &job,
                        &archive_dirs,
                        self.db_file.as_deref(),
                    )));
                }
            }
            PubSub::DoCp(job, files, archive_dirs) | PubSub::DoMv(job, files, archive_dirs) => {
                let operation = match pubsub {
                    PubSub::DoCp(_job, _files, _archive_dirs) => DBJobOperation::Cp,
                    PubSub::DoMv(_job, _files, _archive_dirs) => DBJobOperation::Mv,
                    _ => unreachable!(),
                };

                self.dialog = Some(Box::new(DlgCpMvProgress::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    job,
                    files,
                    archive_dirs,
                    self.db_file.as_deref(),
                    operation,
                )));
            }
            PubSub::JobCompleted(job, files, dirs) => {
                self.pubsub_tx.send(PubSub::Reload).unwrap();

                let job_aborted = matches!(job.status, DBJobStatus::Aborted);

                let skipped_files = files
                    .iter()
                    .any(|entry| matches!(entry.status, DBFileStatus::Skipped));

                let skipped_dirs = dirs
                    .iter()
                    .any(|entry| matches!(entry.status, DBFileStatus::Skipped));

                let messages_files = files.iter().any(|entry| !entry.message.is_empty());
                let messages_dirs = dirs.iter().any(|entry| !entry.message.is_empty());

                if job_aborted || skipped_files || skipped_dirs || messages_files || messages_dirs {
                    self.dialog = Some(Box::new(DlgReport::new(
                        &self.config,
                        self.pubsub_tx.clone(),
                        job,
                        files,
                        dirs,
                        self.db_file.as_deref(),
                    )));
                } else {
                    self.db_file
                        .as_deref()
                        .and_then(|db_file| DataBase::new(db_file).ok())
                        .map(|db| db.delete_job(job.id));

                    self.pubsub_tx.send(PubSub::NextPendingJob).unwrap();
                }
            }
            PubSub::PromptSaveReport(cwd, path) => {
                let str_path = path.to_string_lossy().replace('%', "%%");
                let chars: Vec<char> = str_path.chars().collect();

                self.command_bar = Some(Box::new(CmdBar::new(
                    &self.config,
                    self.pubsub_tx.clone(),
                    CmdBarType::SaveReport(cwd.clone()),
                    "save: ",
                    &str_path,
                    chars
                        .iter()
                        .rposition(|c| *c == '/')
                        .unwrap_or(str_path.len()),
                )));
            }
            PubSub::SaveReport(cwd, str_path) => {
                let archive_path =
                    expanduser(&PathBuf::from(&self.apply_template(str_path, Quote::No)));

                let path = match archive_path.is_absolute() {
                    true => self.unarchive_path(&archive_path),
                    false => {
                        let mut path = cwd.clone();
                        path.push(&archive_path);

                        self.unarchive_path(&path)
                    }
                };

                self.pubsub_tx.send(PubSub::DoSaveReport(path)).unwrap();
            }
            PubSub::CommandBarError(msg) => {
                self.command_bar = Some(Box::new(CommandBarError::new(&self.config, msg)));
            }
            PubSub::NextPendingJob => {
                match self.pending_jobs.pop() {
                    Some(job) => {
                        self.dialog = Some(Box::new(DlgPendingJob::new(
                            &self.config,
                            self.pubsub_tx.clone(),
                            &job,
                            self.db_file.as_deref(),
                        )));
                    }
                    None => {
                        // TODO: Umount all archives
                    }
                }
            }
            PubSub::MountArchivesForJob(job) => {
                self.pending_job = Some(job.clone());
                self.pending_archives = job.archives.iter().rev().cloned().collect();

                self.pubsub_tx.send(PubSub::NextPendingArchive).unwrap();
            }
            PubSub::NextPendingArchive => match self.pending_archives.pop() {
                Some(archive) => match &self.archive_mounter_command_tx {
                    Some(_command_tx) => {
                        self.pubsub_tx.send(PubSub::MountArchive(archive)).unwrap();
                    }
                    None => {
                        self.pending_archives.clear();
                        self.pending_job = None;

                        self.pubsub_tx
                            .send(PubSub::Error(
                                String::from("archivefs/archivemount executable not found"),
                                Some(Box::new(PubSub::NextPendingJob)),
                            ))
                            .unwrap();
                    }
                },
                None => {
                    let job = self
                        .pending_job
                        .take()
                        .expect("BUG: pending_job is None when processing its archives");

                    let archive_dirs = match &self.archive_mounter_command_tx {
                        Some(command_tx) => archive_mounter::get_archive_dirs(command_tx),
                        None => Vec::new(),
                    };

                    match job.status {
                        DBJobStatus::Dirscan => {
                            self.dialog = Some(Box::new(DlgDirscan::new(
                                &self.config,
                                self.pubsub_tx.clone(),
                                &job,
                                &archive_dirs,
                                self.db_file.as_deref(),
                            )));
                        }
                        DBJobStatus::InProgress => {
                            let files = self
                                .db_file
                                .as_deref()
                                .and_then(|db_file| DataBase::new(db_file).ok())
                                .map(|db| db.get_file_list(job.id))
                                .unwrap_or_default();

                            match job.operation {
                                DBJobOperation::Cp => {
                                    self.pubsub_tx
                                        .send(PubSub::DoCp(job, files, archive_dirs))
                                        .unwrap();
                                }
                                DBJobOperation::Mv => {
                                    self.pubsub_tx
                                        .send(PubSub::DoMv(job, files, archive_dirs))
                                        .unwrap();
                                }
                                DBJobOperation::Rm => {
                                    self.pubsub_tx
                                        .send(PubSub::DoRm(job, files, archive_dirs))
                                        .unwrap();
                                }
                            }
                        }
                        DBJobStatus::Aborted | DBJobStatus::Done => {
                            let files = self
                                .db_file
                                .as_deref()
                                .and_then(|db_file| DataBase::new(db_file).ok())
                                .map(|db| db.get_file_list(job.id))
                                .unwrap_or_default();

                            let dirs = self
                                .db_file
                                .as_deref()
                                .and_then(|db_file| DataBase::new(db_file).ok())
                                .map(|db| db.get_dir_list(job.id))
                                .unwrap_or_default();

                            self.pubsub_tx
                                .send(PubSub::JobCompleted(job, files, dirs))
                                .unwrap();
                        }
                    }
                }
            },
            PubSub::ArchiveMounted(_archive_file, _temp_dir) => {
                if self.pending_job.is_some() {
                    self.pubsub_tx.send(PubSub::NextPendingArchive).unwrap();
                }
            }
            PubSub::ArchiveMountError(_archive_file, error) => {
                if self.pending_job.is_some() {
                    self.pending_archives.clear();
                    self.pending_job = None;

                    self.pubsub_tx
                        .send(PubSub::Error(
                            String::from(error),
                            Some(Box::new(PubSub::NextPendingJob)),
                        ))
                        .unwrap();
                }
            }
            _ => (),
        }

        action
    }

    fn unarchive_path(&self, file: &Path) -> PathBuf {
        match &self.archive_mounter_command_tx {
            Some(command_tx) => archive_mounter::unarchive_path(command_tx, file),
            None => PathBuf::from(file),
        }
    }

    fn archive_path(&self, file: &Path) -> PathBuf {
        match &self.archive_mounter_command_tx {
            Some(command_tx) => archive_mounter::archive_path(command_tx, file),
            None => PathBuf::from(file),
        }
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
            &self
                .archive_path(&cwd)
                .file_name()
                .map(|base| base.to_string_lossy().to_string())
                .unwrap_or_default(),
        );

        let other_base = fn_quote(
            &self
                .archive_path(&other_cwd)
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

    fn db_entries_from_entries(
        &self,
        entries: &[Entry],
        archive_dirs: &[ArchiveEntry],
    ) -> Vec<DBEntriesEntry> {
        entries
            .iter()
            .map(|entry| DBEntriesEntry {
                id: 0,
                job_id: 0,
                file: archive_mounter::archive_parent_map(&entry.file, archive_dirs),
                is_file: entry.lstat.is_file(),
                is_dir: entry.lstat.is_dir(),
                is_symlink: entry.lstat.is_symlink(),
                size: entry.lstat.len(),
                uid: entry.lstat.uid(),
                gid: entry.lstat.gid(),
            })
            .collect()
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
            .split(f.area());

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

        let focus_command_bar = self
            .command_bar
            .as_ref()
            .map(|command_bar| command_bar.is_focusable())
            .unwrap_or(false);

        if let Some(command_bar) = &mut self.command_bar {
            command_bar.render(
                f,
                &chunks[1],
                match focus_command_bar {
                    true => Focus::Focused,
                    false => Focus::Normal,
                },
            );
        }

        self.button_bar.render(f, &chunks[2], Focus::Normal);

        if let Some(dlg) = &mut self.dialog {
            dlg.render(
                f,
                &chunks[0],
                match focus_command_bar {
                    true => Focus::Normal,
                    false => Focus::Focused,
                },
            );
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

pub fn format_seconds(t: u64) -> String {
    let seconds = t % 60;
    let minutes = (t / 60) % 60;
    let hours = (t / 3600) % 24;
    let days = t / 86400;

    match days {
        0 => format!("{:02}:{:02}:{:02}", hours, minutes, seconds),
        _ => format!("{}d{:02}:{:02}:{:02}", days, hours, minutes, seconds),
    }
}

pub fn natsort_key(s: &str) -> String {
    caseless::default_case_fold_str(s).nfkd().collect()
}
