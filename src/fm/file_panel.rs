use std::{
    cell::RefCell,
    fs::{self, read_dir},
    path::{Path, PathBuf},
    rc::Rc,
    thread,
};

use anyhow::{anyhow, bail};
use crossbeam_channel::{Receiver, Sender};
use ratatui::{
    prelude::*,
    widgets::{
        block::{Position, Title},
        *,
    },
};
use termion::event::*;

use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use regex::RegexBuilder;
use unicode_width::UnicodeWidthStr;

use crate::{
    app::PubSub,
    component::{Component, Focus},
    config::Config,
    fm::{
        app::{human_readable_size, tar_stem, tar_suffix, LABELS},
        archive_mounter::{self, ArchiveMounterCommand},
        bookmarks::{Bookmarks, BOOKMARK_KEYS},
        entry::{
            count_directories, filter_file_list, get_file_list, sort_by_function,
            style_from_palette, Entry, HiddenFiles, SortBy, SortOrder, ARCHIVE_EXTENSIONS,
        },
        panel::{Panel, PanelComponent},
    },
    fnmatch,
    shutil::disk_usage,
    tilde_layout::tilde_layout,
};

#[derive(Debug, Clone)]
enum ComponentPubSub {
    FileList(Vec<Entry>),
}

#[derive(Debug, Clone)]
enum ArchiveMountRequest {
    Explicit(PathBuf),
    Implicit(PathBuf),
    None,
}

#[derive(Debug)]
pub struct FilePanel {
    config: Rc<Config>,
    bookmarks: Rc<RefCell<Bookmarks>>,
    pubsub_tx: Sender<PubSub>,
    rect: Rect,
    component_pubsub_tx: Sender<ComponentPubSub>,
    component_pubsub_rx: Receiver<ComponentPubSub>,
    file_list_tx: Sender<PathBuf>,
    file_list_rx: Receiver<PathBuf>,
    cwd: PathBuf,
    old_cwd: PathBuf,
    leader: Option<char>,
    free: u64,
    is_loading: bool,
    file_list: Vec<Entry>,
    shown_file_list: Vec<Entry>,
    tagged_files: Vec<Entry>,
    cursor_position: usize,
    first_line: usize,
    hidden_files: HiddenFiles,
    file_filter: String,
    sort_method: SortBy,
    sort_order: SortOrder,
    selected_file: Option<PathBuf>,
    focus: Focus,
    archive_mounter_command_tx: Option<Sender<ArchiveMounterCommand>>,
    archive_mount_request: ArchiveMountRequest,
}

impl FilePanel {
    pub fn new(
        config: &Rc<Config>,
        bookmarks: &Rc<RefCell<Bookmarks>>,
        pubsub_tx: Sender<PubSub>,
        initial_path: &Path,
        archive_mounter_command_tx: Option<Sender<ArchiveMounterCommand>>,
        focus: Focus,
    ) -> FilePanel {
        let (component_pubsub_tx, component_pubsub_rx) = crossbeam_channel::unbounded();
        let (file_list_tx, file_list_rx) = crossbeam_channel::unbounded();

        let mut panel = FilePanel {
            config: Rc::clone(config),
            bookmarks: Rc::clone(bookmarks),
            pubsub_tx,
            rect: Rect::default(),
            component_pubsub_tx,
            component_pubsub_rx,
            file_list_tx,
            file_list_rx,
            cwd: PathBuf::new(),
            old_cwd: PathBuf::new(),
            leader: None,
            free: 0,
            is_loading: false,
            file_list: Vec::new(),
            shown_file_list: Vec::new(),
            tagged_files: Vec::new(),
            cursor_position: 0,
            first_line: 0,
            hidden_files: HiddenFiles::Hide,
            file_filter: String::from(""),
            sort_method: SortBy::Name,
            sort_order: SortOrder::Normal,
            selected_file: None,
            focus,
            archive_mounter_command_tx,
            archive_mount_request: ArchiveMountRequest::None,
        };

        panel.file_list_thread();
        panel.chdir(initial_path);
        panel.old_cwd.clone_from(&panel.cwd);

        panel
    }

    fn handle_component_pubsub(&mut self) {
        if let Ok(event) = self.component_pubsub_rx.try_recv() {
            match event {
                ComponentPubSub::FileList(file_list) => {
                    self.is_loading = false;

                    self.file_list = file_list;

                    self.filter_and_sort_file_list(
                        self.selected_file
                            .as_ref()
                            .map(|selected_file| self.archive_path(selected_file))
                            .as_deref(),
                    );

                    self.tagged_files
                        .retain(|entry| self.file_list.contains(entry));

                    if let Focus::Focused = self.focus {
                        self.pubsub_tx
                            .send(PubSub::SelectedEntry(self.get_selected_entry()))
                            .unwrap();
                    }
                }
            }
        }
    }

    fn file_list_thread(&mut self) {
        let file_list_rx = self.file_list_rx.clone();
        let component_pubsub_tx = self.component_pubsub_tx.clone();
        let pubsub_tx = self.pubsub_tx.clone();

        thread::spawn(move || {
            loop {
                let cwd = match file_list_rx.is_empty() {
                    // Block this thread until we recevie something
                    true => match file_list_rx.recv() {
                        Ok(cwd) => cwd,

                        // When the main thread exits, the channel returns an error
                        Err(_) => return,
                    },

                    // We're only interested in the latest message in the queue
                    false => file_list_rx.try_iter().last().unwrap(),
                };

                // Step 1: Get the current file list without counting the directories
                let file_list = get_file_list(&cwd, Some(file_list_rx.clone())).unwrap_or_default();

                // Send the current result only if there are no newer file list requests in the queue,
                // otherwise discard the current result
                if file_list_rx.is_empty() {
                    // First send the component event
                    let _ = component_pubsub_tx.send(ComponentPubSub::FileList(file_list.clone()));

                    // Then notify the app that there is an component event
                    let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);

                    // Step 2: Get the current file list counting the directories
                    let file_list = count_directories(&file_list, Some(file_list_rx.clone()));

                    // Send the current result only if there are no newer file list requests in the queue,
                    // otherwise discard the current result
                    if file_list_rx.is_empty() {
                        // First send the component event
                        let _ = component_pubsub_tx.send(ComponentPubSub::FileList(file_list));

                        // Then notify the app that there is an component event
                        let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
                    }
                }
            }
        });
    }

    fn chdir_old_cwd(&mut self) {
        let old_cwd = self.old_cwd.clone();

        self.chdir(&old_cwd)
    }

    fn get_selected_file(&self) -> Option<PathBuf> {
        match self.shown_file_list.is_empty() {
            true => None,
            false => Some(self.shown_file_list[self.cursor_position].file.clone()),
        }
    }

    fn load_file_list(&mut self, selected_file: Option<&Path>) {
        self.selected_file = selected_file.map(PathBuf::from);
        self.free = match disk_usage(&self.cwd) {
            Ok(usage) => usage.free,
            Err(_) => 0,
        };

        self.is_loading = true;
        self.file_list_tx.send(self.cwd.clone()).unwrap();
    }

    fn filter_and_sort_file_list(&mut self, selected_file: Option<&Path>) {
        let offset_from_first = self.cursor_position.saturating_sub(self.first_line);

        self.shown_file_list =
            filter_file_list(&self.file_list, self.hidden_files, &self.file_filter);

        self.shown_file_list
            .sort_by(|a, b| sort_by_function(self.sort_method)(a, b, self.sort_order));

        match selected_file {
            Some(file) => match self
                .shown_file_list
                .iter()
                .position(|entry| self.archive_path(&entry.file) == file)
            {
                Some(i) => self.cursor_position = self.clamp_cursor(i),
                None => self.cursor_position = 0,
            },
            None => self.cursor_position = 0,
        }

        self.first_line = self.cursor_position.saturating_sub(offset_from_first);
        self.clamp_first_line();
    }

    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.shown_file_list.len().saturating_sub(1))
    }

    fn clamp_first_line(&mut self) {
        if (self.first_line + (self.rect.height as usize)) > self.shown_file_list.len() {
            self.first_line = self
                .shown_file_list
                .len()
                .saturating_sub(self.rect.height as usize);
        }
    }

    fn handle_up(&mut self) {
        let old_cursor_position = self.cursor_position;

        self.cursor_position = self.clamp_cursor(self.cursor_position.saturating_sub(1));

        if self.cursor_position != old_cursor_position {
            self.pubsub_tx
                .send(PubSub::SelectedEntry(self.get_selected_entry()))
                .unwrap();
        }
    }

    fn handle_down(&mut self) {
        let old_cursor_position = self.cursor_position;

        self.cursor_position = self.clamp_cursor(self.cursor_position.saturating_add(1));

        if self.cursor_position != old_cursor_position {
            self.pubsub_tx
                .send(PubSub::SelectedEntry(self.get_selected_entry()))
                .unwrap();
        }
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
}

impl Component for FilePanel {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        if let Some(c) = self.leader {
            // When pressing a key after a leader, the leader is automatically reset
            self.leader = None;
            self.pubsub_tx.send(PubSub::Leader(self.leader)).unwrap();

            match (c, key) {
                ('`', Key::Char('\'')) | ('`', Key::Char('`')) => {
                    self.chdir_old_cwd();
                }
                ('`', Key::Char(c)) if BOOKMARK_KEYS.contains(*c) => {
                    let bookmark =
                        self.bookmarks
                            .borrow()
                            .get(*c)
                            .and_then(|cwd| match read_dir(&cwd) {
                                Ok(_) => Some(cwd),
                                Err(_) => None,
                            });

                    if let Some(cwd) = bookmark {
                        self.chdir(&cwd);
                    }
                }
                ('m', Key::Char(c)) if BOOKMARK_KEYS.contains(*c) => {
                    self.bookmarks.borrow_mut().insert(*c, &self.cwd)
                }
                ('s', Key::Char('n')) => {
                    self.pubsub_tx
                        .send(PubSub::SortFiles(SortBy::Name, SortOrder::Normal))
                        .unwrap();
                }
                ('s', Key::Char('N')) => {
                    self.pubsub_tx
                        .send(PubSub::SortFiles(SortBy::Name, SortOrder::Reverse))
                        .unwrap();
                }
                ('s', Key::Char('e')) => {
                    self.pubsub_tx
                        .send(PubSub::SortFiles(SortBy::Extension, SortOrder::Normal))
                        .unwrap();
                }
                ('s', Key::Char('E')) => {
                    self.pubsub_tx
                        .send(PubSub::SortFiles(SortBy::Extension, SortOrder::Reverse))
                        .unwrap();
                }
                ('s', Key::Char('d')) => {
                    self.pubsub_tx
                        .send(PubSub::SortFiles(SortBy::Date, SortOrder::Normal))
                        .unwrap();
                }
                ('s', Key::Char('D')) => {
                    self.pubsub_tx
                        .send(PubSub::SortFiles(SortBy::Date, SortOrder::Reverse))
                        .unwrap();
                }
                ('s', Key::Char('s')) => {
                    self.pubsub_tx
                        .send(PubSub::SortFiles(SortBy::Size, SortOrder::Normal))
                        .unwrap();
                }
                ('s', Key::Char('S')) => {
                    self.pubsub_tx
                        .send(PubSub::SortFiles(SortBy::Size, SortOrder::Reverse))
                        .unwrap();
                }
                ('c', Key::Char('c')) | ('c', Key::Char('w')) => {
                    if !self.shown_file_list.is_empty() {
                        self.pubsub_tx
                            .send(PubSub::PromptRename(String::from(""), 0))
                            .unwrap();
                    }
                }
                ('c', Key::Char('e')) => {
                    if !self.shown_file_list.is_empty() {
                        let entry = &self.shown_file_list[self.cursor_position];
                        let file_name = tar_suffix(&entry.file_name.replace('%', "%%"));

                        self.pubsub_tx
                            .send(PubSub::PromptRename(file_name, 0))
                            .unwrap();
                    }
                }
                _ => key_handled = false,
            }
        } else {
            match key {
                Key::Char(c) if *c == '\'' || *c == '`' => {
                    self.leader = Some('`');
                    self.pubsub_tx.send(PubSub::Leader(Some(*c))).unwrap();
                }
                Key::Char('m') => {
                    if self.archive_path(&self.cwd) == self.cwd {
                        self.leader = Some('m');
                        self.pubsub_tx.send(PubSub::Leader(self.leader)).unwrap();
                    } else {
                        self.pubsub_tx
                            .send(PubSub::Error(String::from(
                                "Cannot bookmark inside an archive",
                            )))
                            .unwrap();
                    }
                }
                Key::Char('s') => {
                    self.leader = Some('s');
                    self.pubsub_tx.send(PubSub::Leader(self.leader)).unwrap();
                }
                Key::Char('c') => {
                    self.leader = Some('c');
                    self.pubsub_tx.send(PubSub::Leader(self.leader)).unwrap();
                }
                Key::Left | Key::Char('h') => {
                    let cwd = self.archive_path(&self.cwd);

                    if let Some(new_cwd) = cwd.parent() {
                        self.chdir(&self.unarchive_path(new_cwd));
                    }
                }
                Key::Right | Key::Char('\n') | Key::Char('l') => {
                    if !self.shown_file_list.is_empty() {
                        let entry = self.shown_file_list[self.cursor_position].clone();

                        if entry.stat.is_dir() {
                            self.chdir(&entry.file);
                        } else if let Some(path) = &entry.link_target {
                            let _ = path
                                .try_exists()
                                .map_err(anyhow::Error::new)
                                .and_then(|exists| {
                                    if !exists {
                                        bail!("!exists")
                                    }

                                    Ok(fs::metadata(path)?)
                                })
                                .and_then(|metadata| {
                                    match metadata.is_dir() {
                                        true => {
                                            // Change directory only if we can change to that exact directory
                                            read_dir(path)?;

                                            self.chdir(path);
                                        }
                                        false => {
                                            let parent = path.parent().ok_or_else(|| {
                                                anyhow!("failed to read link target parent")
                                            })?;

                                            // Change directory only if we can change to that exact directory
                                            read_dir(parent)?;

                                            self.chdir(parent);

                                            if self.cwd == parent {
                                                let old_cursor_position = self.cursor_position;
                                                let diff_cursor_first = self
                                                    .cursor_position
                                                    .saturating_sub(self.first_line);

                                                self.cursor_position = self.clamp_cursor(
                                                    self.shown_file_list
                                                        .iter()
                                                        .position(|entry| &entry.file == path)
                                                        .unwrap_or(old_cursor_position),
                                                );

                                                if self.cursor_position != old_cursor_position {
                                                    self.first_line = self
                                                        .cursor_position
                                                        .saturating_sub(diff_cursor_first);
                                                    self.clamp_first_line();

                                                    self.pubsub_tx
                                                        .send(PubSub::SelectedEntry(
                                                            self.get_selected_entry(),
                                                        ))
                                                        .unwrap();
                                                }
                                            }
                                        }
                                    }

                                    Ok(())
                                });
                        } else if ARCHIVE_EXTENSIONS.contains(&entry.extension.as_str())
                            && self.archive_mounter_command_tx.is_some()
                        {
                            self.archive_mount_request =
                                ArchiveMountRequest::Implicit(entry.file.clone());

                            self.pubsub_tx
                                .send(PubSub::MountArchive(entry.file.clone()))
                                .unwrap();
                        } else {
                            // TODO: Handle regular files
                        }
                    }
                }
                Key::Char('o') => {
                    if !self.shown_file_list.is_empty() {
                        match &self.archive_mounter_command_tx {
                            Some(_) => {
                                let entry = &self.shown_file_list[self.cursor_position];

                                self.archive_mount_request =
                                    ArchiveMountRequest::Explicit(entry.file.clone());

                                self.pubsub_tx
                                    .send(PubSub::MountArchive(entry.file.clone()))
                                    .unwrap();
                            }
                            None => {
                                self.pubsub_tx
                                    .send(PubSub::Error(String::from(
                                        "archivefs/archivemount executable not found",
                                    )))
                                    .unwrap();
                            }
                        }
                    }
                }
                Key::Up | Key::Char('k') => {
                    self.handle_up();
                }
                Key::Down | Key::Char('j') => {
                    self.handle_down();
                }
                Key::Home | Key::Char('g') => {
                    let old_cursor_position = self.cursor_position;

                    self.cursor_position = 0;

                    if self.cursor_position != old_cursor_position {
                        self.pubsub_tx
                            .send(PubSub::SelectedEntry(self.get_selected_entry()))
                            .unwrap();
                    }
                }
                Key::End | Key::Char('G') => {
                    let old_cursor_position = self.cursor_position;

                    self.cursor_position = self.clamp_cursor(self.shown_file_list.len());

                    if self.cursor_position != old_cursor_position {
                        self.pubsub_tx
                            .send(PubSub::SelectedEntry(self.get_selected_entry()))
                            .unwrap();
                    }
                }
                Key::PageUp | Key::Ctrl('b') => {
                    let rect_height = (self.rect.height as usize).saturating_sub(1);
                    let old_cursor_position = self.cursor_position;

                    self.cursor_position =
                        self.clamp_cursor(self.cursor_position.saturating_sub(rect_height));

                    self.first_line = self.first_line.saturating_sub(rect_height);
                    self.clamp_first_line();

                    if self.cursor_position != old_cursor_position {
                        self.pubsub_tx
                            .send(PubSub::SelectedEntry(self.get_selected_entry()))
                            .unwrap();
                    }
                }
                Key::PageDown | Key::Ctrl('f') => {
                    let rect_height = (self.rect.height as usize).saturating_sub(1);
                    let old_cursor_position = self.cursor_position;

                    self.cursor_position =
                        self.clamp_cursor(self.cursor_position.saturating_add(rect_height));

                    self.first_line = self.first_line.saturating_add(rect_height);
                    self.clamp_first_line();

                    if self.cursor_position != old_cursor_position {
                        self.pubsub_tx
                            .send(PubSub::SelectedEntry(self.get_selected_entry()))
                            .unwrap();
                    }
                }
                Key::Char('v') | Key::F(3) | Key::Char('3') => {
                    if !self.shown_file_list.is_empty() {
                        let entry = self.shown_file_list[self.cursor_position].clone();

                        match entry.stat.is_dir() {
                            true => self.chdir(&entry.file),
                            false => self.pubsub_tx.send(PubSub::ViewFile(entry.file)).unwrap(),
                        }
                    }
                }
                Key::Insert | Key::Char(' ') => {
                    if !self.shown_file_list.is_empty() {
                        let entry = &self.shown_file_list[self.cursor_position];

                        if let Some(i) = self.tagged_files.iter().position(|x| x == entry) {
                            self.tagged_files.swap_remove(i);
                        } else {
                            self.tagged_files.push(entry.clone());
                        }
                    }

                    self.handle_down();
                }
                Key::Char('t') => {
                    if !self.shown_file_list.is_empty() {
                        let entry = &self.shown_file_list[self.cursor_position];

                        if !self.tagged_files.contains(entry) {
                            self.tagged_files.push(entry.clone());
                        }
                    }

                    self.handle_down();
                }
                Key::Char('u') => {
                    if !self.shown_file_list.is_empty() {
                        let entry = &self.shown_file_list[self.cursor_position];

                        if let Some(i) = self.tagged_files.iter().position(|x| x == entry) {
                            self.tagged_files.swap_remove(i);
                        }
                    }

                    self.handle_down();
                }
                Key::Char('*') => {
                    if !self.shown_file_list.is_empty() {
                        for entry in &self.shown_file_list {
                            if let Some(i) = self.tagged_files.iter().position(|x| x == entry) {
                                self.tagged_files.swap_remove(i);
                            } else {
                                self.tagged_files.push(entry.clone());
                            }
                        }
                    }
                }
                Key::Char('T') => {
                    if !self.shown_file_list.is_empty() {
                        for entry in &self.shown_file_list {
                            if !self.tagged_files.contains(entry) {
                                self.tagged_files.push(entry.clone());
                            }
                        }
                    }
                }
                Key::Char('U') => {
                    if !self.shown_file_list.is_empty() {
                        for entry in &self.shown_file_list {
                            if let Some(i) = self.tagged_files.iter().position(|x| x == entry) {
                                self.tagged_files.swap_remove(i);
                            }
                        }
                    }
                }
                Key::Char('+') => self.pubsub_tx.send(PubSub::PromptTagGlob).unwrap(),
                Key::Char('-') | Key::Char('\\') => {
                    self.pubsub_tx.send(PubSub::PromptUntagGlob).unwrap();
                }
                Key::Ctrl('r') => self.pubsub_tx.send(PubSub::Reload).unwrap(),
                Key::Backspace => self.pubsub_tx.send(PubSub::ToggleHidden).unwrap(),
                Key::Char('f') | Key::Char('/') => {
                    self.pubsub_tx
                        .send(PubSub::PromptFileFilter(self.file_filter.clone()))
                        .unwrap();
                }
                Key::F(7) | Key::Char('7') => self.pubsub_tx.send(PubSub::PromptMkdir).unwrap(),
                Key::Char('r') => {
                    if !self.shown_file_list.is_empty() {
                        self.pubsub_tx
                            .send(PubSub::PromptRename(String::from(""), 0))
                            .unwrap();
                    }
                }
                Key::Char('i') | Key::Char('I') => {
                    if !self.shown_file_list.is_empty() {
                        let entry = &self.shown_file_list[self.cursor_position];
                        let file_name = entry.file_name.replace('%', "%%");

                        self.pubsub_tx
                            .send(PubSub::PromptRename(file_name, 0))
                            .unwrap();
                    }
                }
                Key::Char('a') => {
                    if !self.shown_file_list.is_empty() {
                        let entry = &self.shown_file_list[self.cursor_position];
                        let file_name = entry.file_name.replace('%', "%%");

                        self.pubsub_tx
                            .send(PubSub::PromptRename(
                                file_name.clone(),
                                tar_stem(&file_name).chars().count(),
                            ))
                            .unwrap();
                    }
                }
                Key::Char('A') => {
                    if !self.shown_file_list.is_empty() {
                        let entry = &self.shown_file_list[self.cursor_position];
                        let file_name = entry.file_name.replace('%', "%%");

                        self.pubsub_tx
                            .send(PubSub::PromptRename(file_name.clone(), file_name.len()))
                            .unwrap();
                    }
                }
                Key::F(8) | Key::Char('8') => {
                    let selected_files = self.get_selected_files();

                    if !selected_files.is_empty() {
                        let question = match selected_files.len() {
                            1 => format!("Delete {}?", selected_files[0].file_name),
                            n => format!("Delete {} files/directories?", n),
                        };

                        self.pubsub_tx
                            .send(PubSub::Question(
                                String::from("Delete"),
                                question,
                                Box::new(PubSub::Rm(selected_files)),
                            ))
                            .unwrap();
                    }
                }
                Key::F(5) | Key::Char('5') => {
                    let selected_files = self.get_selected_files();

                    if !selected_files.is_empty() {
                        self.pubsub_tx.send(PubSub::Cp(selected_files)).unwrap();
                    }
                }
                Key::F(6) | Key::Char('6') => {
                    let selected_files = self.get_selected_files();

                    if !selected_files.is_empty() {
                        self.pubsub_tx.send(PubSub::Mv(selected_files)).unwrap();
                    }
                }
                _ => key_handled = false,
            }
        }

        key_handled
    }

    fn handle_pubsub(&mut self, event: &PubSub) {
        match event {
            PubSub::ComponentThreadEvent => self.handle_component_pubsub(),
            PubSub::Esc => {
                self.leader = None;

                if !self.file_filter.is_empty() {
                    self.file_filter.clear();

                    self.filter_and_sort_file_list(self.get_selected_file().as_deref());

                    if let Focus::Focused = self.focus {
                        self.pubsub_tx
                            .send(PubSub::SelectedEntry(self.get_selected_entry()))
                            .unwrap();
                    }
                }
            }
            PubSub::SortFiles(sort_method, sort_order) => {
                self.sort_method = *sort_method;
                self.sort_order = *sort_order;

                self.filter_and_sort_file_list(self.get_selected_file().as_deref());

                if let Focus::Focused = self.focus {
                    self.pubsub_tx
                        .send(PubSub::SelectedEntry(self.get_selected_entry()))
                        .unwrap();
                }
            }
            PubSub::FileFilter(filter) => {
                if let Focus::Focused = self.focus {
                    if filter != &self.file_filter {
                        self.file_filter.clone_from(filter);

                        self.filter_and_sort_file_list(self.get_selected_file().as_deref());

                        if !self.shown_file_list.is_empty() {
                            let matcher = SkimMatcherV2::default();

                            let scores: Vec<i64> = self
                                .shown_file_list
                                .iter()
                                .map(|entry| {
                                    matcher.fuzzy_match(&entry.key, filter).unwrap_or(i64::MIN)
                                })
                                .collect();

                            let new_cursor_position = self.clamp_cursor(
                                scores
                                    .iter()
                                    .enumerate()
                                    .max_by_key(|(_i, &score)| score)
                                    .map(|(i, _score)| i)
                                    .unwrap_or_default(),
                            );

                            if new_cursor_position != self.cursor_position
                                && scores[new_cursor_position] > scores[self.cursor_position]
                            {
                                self.cursor_position = new_cursor_position;
                            }
                        }

                        if let Focus::Focused = self.focus {
                            self.pubsub_tx
                                .send(PubSub::SelectedEntry(self.get_selected_entry()))
                                .unwrap();
                        }
                    }
                }
            }
            PubSub::ToggleHidden => {
                let hidden_files = self.hidden_files;

                self.hidden_files = match hidden_files {
                    HiddenFiles::Show => HiddenFiles::Hide,
                    HiddenFiles::Hide => HiddenFiles::Show,
                };

                self.filter_and_sort_file_list(self.get_selected_file().as_deref());

                if let Focus::Focused = self.focus {
                    self.pubsub_tx
                        .send(PubSub::SelectedEntry(self.get_selected_entry()))
                        .unwrap();
                }
            }
            PubSub::TagGlob(glob) => {
                if let Focus::Focused = self.focus {
                    if let Ok(re) = RegexBuilder::new(&fnmatch::translate(glob))
                        .case_insensitive(true)
                        .build()
                    {
                        for entry in &self.shown_file_list {
                            if re.is_match(&entry.file_name) && !self.tagged_files.contains(entry) {
                                self.tagged_files.push(entry.clone());
                            }
                        }
                    }
                }
            }
            PubSub::UntagGlob(glob) => {
                if let Focus::Focused = self.focus {
                    if let Ok(re) = RegexBuilder::new(&fnmatch::translate(glob))
                        .case_insensitive(true)
                        .build()
                    {
                        for entry in &self.shown_file_list {
                            if re.is_match(&entry.file_name) {
                                if let Some(i) = self.tagged_files.iter().position(|x| x == entry) {
                                    self.tagged_files.swap_remove(i);
                                }
                            }
                        }
                    }
                }
            }
            PubSub::Reload => {
                self.reload(self.get_selected_file().as_deref());
            }
            PubSub::ArchiveMounted(archive_file, temp_dir) => match &self.archive_mount_request {
                ArchiveMountRequest::Explicit(archive) | ArchiveMountRequest::Implicit(archive) => {
                    if archive == archive_file {
                        self.archive_mount_request = ArchiveMountRequest::None;

                        self.chdir(temp_dir);
                    }
                }
                ArchiveMountRequest::None => (),
            },
            PubSub::ArchiveMountError(archive_file, error) => match &self.archive_mount_request {
                ArchiveMountRequest::Explicit(archive) => {
                    if archive == archive_file {
                        self.archive_mount_request = ArchiveMountRequest::None;

                        self.pubsub_tx
                            .send(PubSub::Error(String::from(error)))
                            .unwrap();
                    }
                }
                ArchiveMountRequest::Implicit(archive) => {
                    if archive == archive_file {
                        self.archive_mount_request = ArchiveMountRequest::None;

                        // TODO: Fallback to file opener
                    }
                }
                ArchiveMountRequest::None => (),
            },
            _ => (),
        }
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, focus: Focus) {
        let middle_border_set = symbols::border::Set {
            top_left: symbols::line::NORMAL.vertical_right,
            top_right: symbols::line::NORMAL.vertical_left,
            ..symbols::border::PLAIN
        };

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(*chunk);

        let upper_block = Block::default()
            .title(
                Title::from(Line::from(vec![
                    Span::raw(symbols::line::NORMAL.horizontal),
                    Span::styled(
                        tilde_layout(
                            &format!(" {} ", self.archive_path(&self.cwd).to_string_lossy()),
                            chunk.width.saturating_sub(4).into(),
                        ),
                        match focus {
                            Focus::Focused => Style::default()
                                .fg(self.config.panel.reverse_fg)
                                .bg(self.config.panel.reverse_bg),
                            _ => Style::default()
                                .fg(self.config.panel.fg)
                                .bg(self.config.panel.bg),
                        },
                    ),
                    Span::raw(symbols::line::NORMAL.horizontal),
                ]))
                .position(Position::Top)
                .alignment(Alignment::Left),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .style(
                Style::default()
                    .fg(self.config.panel.fg)
                    .bg(self.config.panel.bg),
            );

        let upper_inner = upper_block.inner(sections[0]);
        let upper_height = (upper_inner.height as usize).saturating_sub(1);

        self.rect = upper_inner;
        self.clamp_first_line();

        if self.first_line > self.cursor_position {
            self.first_line = self.cursor_position;
        }

        if (self.cursor_position - self.first_line) > upper_height {
            self.first_line = self.cursor_position.saturating_sub(upper_height);
        }

        f.render_widget(upper_block, sections[0]);

        match self.is_loading {
            true => {
                f.render_widget(
                    Block::default()
                        .title("Loading...")
                        .style(Style::default().fg(self.config.panel.fg)),
                    upper_inner,
                );
            }
            false => {
                let items: Vec<ListItem> = self
                    .shown_file_list
                    .iter()
                    .skip(self.first_line)
                    .take(upper_inner.height.into())
                    .enumerate()
                    .map(|(i, entry)| {
                        let filename_max_width = (upper_inner.width as usize)
                            .saturating_sub(entry.shown_size.width())
                            .saturating_sub(9);

                        let is_selected = self.first_line + i == self.cursor_position;

                        let filename = if is_selected && !matches!(focus, Focus::Focused) {
                            tilde_layout(
                                &std::iter::once('\u{2192}')
                                    .chain(entry.label.chars().skip(1))
                                    .collect::<String>(),
                                filename_max_width,
                            )
                        } else {
                            tilde_layout(&entry.label, filename_max_width)
                        };

                        let filename_width = filename.width();

                        // The reason why I add {:width$} whitespaces after the
                        // filename instead of putting the filename directly
                        // inside {:width$} is because the {:width$} formatting
                        // has a bug with some 0-width Unicode characters
                        Span::styled(
                            format!(
                                "{}{:width$} {} {}",
                                &filename,
                                "",
                                &entry.shown_size,
                                &entry.shown_mtime,
                                width = filename_max_width.saturating_sub(filename_width)
                            ),
                            match (
                                self.tagged_files.contains(entry),
                                is_selected,
                                matches!(focus, Focus::Focused),
                            ) {
                                (true, true, true) => {
                                    Style::default().fg(self.config.ui.markselect_fg)
                                }
                                (true, true, false) => {
                                    Style::default().fg(self.config.ui.marked_fg)
                                }
                                (true, false, _) => Style::default().fg(self.config.ui.marked_fg),
                                (false, true, true) => {
                                    Style::default().fg(self.config.ui.selected_fg)
                                }
                                (false, _, _) => style_from_palette(&self.config, entry.palette),
                            },
                        )
                        .into()
                    })
                    .collect();

                let items = List::new(items).highlight_style(match focus {
                    Focus::Focused => Style::default().bg(self.config.ui.selected_bg),
                    _ => Style::default(),
                });

                let mut state = ListState::default();
                state.select(Some(self.cursor_position - self.first_line));

                f.render_stateful_widget(items, upper_inner, &mut state);
            }
        }

        let lower_block = Block::default()
            .title(
                Title::from(Line::from(vec![
                    Span::raw(symbols::line::NORMAL.horizontal),
                    Span::raw(tilde_layout(
                        &format!(" Free: {} ", human_readable_size(self.free)),
                        chunk.width.saturating_sub(4).into(),
                    )),
                    Span::raw(symbols::line::NORMAL.horizontal),
                ]))
                .position(Position::Bottom)
                .alignment(Alignment::Right),
            )
            .title(
                Title::from(match self.tagged_files.is_empty() {
                    true => Span::raw(symbols::line::NORMAL.horizontal),
                    false => Span::styled(
                        tilde_layout(
                            &format!(
                                " {} in {} file{} ",
                                human_readable_size(
                                    self.tagged_files
                                        .iter()
                                        .map(|entry| {
                                            match entry.lstat.is_dir() {
                                                true => 0,
                                                false => entry.lstat.len(),
                                            }
                                        })
                                        .sum()
                                ),
                                self.tagged_files.len(),
                                if self.tagged_files.len() == 1 {
                                    ""
                                } else {
                                    "s"
                                }
                            ),
                            chunk.width.saturating_sub(4).into(),
                        ),
                        Style::default().fg(self.config.ui.marked_fg),
                    ),
                })
                .position(Position::Top)
                .alignment(Alignment::Center),
            )
            .borders(Borders::ALL)
            .border_set(middle_border_set)
            .style(
                Style::default()
                    .fg(self.config.panel.fg)
                    .bg(self.config.panel.bg),
            );

        let lower_inner = lower_block.inner(sections[1]);

        f.render_widget(lower_block, sections[1]);

        if (!self.is_loading) && (!self.shown_file_list.is_empty()) {
            f.render_widget(
                Block::new()
                    .title(tilde_layout(
                        &self.shown_file_list[self.cursor_position].details,
                        lower_inner.width.into(),
                    ))
                    .style(
                        Style::default()
                            .fg(self.config.panel.fg)
                            .bg(self.config.panel.bg),
                    ),
                lower_inner,
            );
        }
    }
}

impl Panel for FilePanel {
    fn change_focus(&mut self, focus: Focus) {
        self.focus = focus;

        if let Focus::Focused = focus {
            self.pubsub_tx
                .send(PubSub::ButtonLabels(
                    LABELS.iter().map(|&label| String::from(label)).collect(),
                ))
                .unwrap();
        }
    }

    fn get_selected_entry(&self) -> Option<Entry> {
        match self.shown_file_list.is_empty() {
            true => None,
            false => Some(self.shown_file_list[self.cursor_position].clone()),
        }
    }

    fn get_cwd(&self) -> Option<PathBuf> {
        Some(self.cwd.clone())
    }

    fn get_tagged_files(&self) -> Vec<Entry> {
        let mut tagged_files = self.tagged_files.clone();

        tagged_files.sort_by(|a, b| sort_by_function(self.sort_method)(a, b, self.sort_order));

        tagged_files
    }

    fn get_selected_files(&self) -> Vec<Entry> {
        match self.tagged_files.is_empty() {
            true => {
                let mut selected_files = Vec::new();

                if !self.shown_file_list.is_empty() {
                    selected_files.push(self.shown_file_list[self.cursor_position].clone());
                }

                selected_files
            }
            false => self.get_tagged_files(),
        }
    }

    fn chdir(&mut self, cwd: &Path) {
        let new_cwd = self.unarchive_path(
            self.archive_path(cwd)
                .ancestors()
                .find(|d| read_dir(self.unarchive_path(d)).is_ok())
                .ok_or_else(|| anyhow!("failed to change directory"))
                .unwrap(),
        );

        if new_cwd != self.cwd {
            self.old_cwd.clone_from(&self.cwd);
            self.cwd = new_cwd;

            self.file_filter.clear();
            self.tagged_files.clear();
            self.cursor_position = 0;
            self.first_line = 0;

            self.load_file_list(Some(&self.archive_path(&self.old_cwd)));
        }
    }

    fn reload(&mut self, selected_file: Option<&Path>) {
        let new_cwd = self.unarchive_path(
            self.archive_path(&self.cwd)
                .ancestors()
                .find(|d| read_dir(self.unarchive_path(d)).is_ok())
                .ok_or_else(|| anyhow!("failed to change directory"))
                .unwrap(),
        );

        if new_cwd != self.cwd {
            self.chdir(&new_cwd);
        } else {
            self.load_file_list(
                selected_file
                    .map(|selected_file| self.archive_path(selected_file))
                    .as_deref(),
            );
        }
    }
}

impl PanelComponent for FilePanel {}
