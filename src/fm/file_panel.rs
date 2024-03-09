use std::{
    cmp::Ordering,
    fs::{self, read_dir, Metadata},
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
    thread,
};

use anyhow::{anyhow, bail, Result};
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
use libc::{S_IXGRP, S_IXOTH, S_IXUSR};
use path_clean::PathClean;
use unicode_width::UnicodeWidthStr;
use uzers::{Groups, Users, UsersCache};

use crate::{
    app::PubSub,
    component::{Component, Focus},
    config::Config,
    fm::{
        app::{format_date, human_readable_size, natsort_key, tar_suffix},
        panel::{Panel, PanelComponent},
    },
    shutil::disk_usage,
    stat::filemode,
    tilde_layout::tilde_layout,
};

const ARCHIVE_EXTENSIONS: &[&str] = &[
    ".tar",
    ".tar.gz",
    ".tgz",
    ".taz",
    ".tar.z",
    ".taz",
    ".tar.bz2",
    ".tz2",
    ".tbz2",
    ".tbz",
    ".tar.lz",
    ".tar.lzma",
    ".tlz",
    ".tar.lzo",
    ".tar.xz",
    ".tar.zst",
    ".tzst",
    ".rpm",
    ".deb",
    ".iso",
    ".zip",
    ".zipx",
    ".jar",
    ".apk",
    ".shar",
    ".lha",
    ".lzh",
    ".rar",
    ".cab",
    ".7z",
];

#[derive(Debug, Clone, Copy)]
pub enum Palette {
    DirSymlink,
    Archive,
    Symlink,
    Stalelink,
    Directory,
    Device,
    Special,
    Executable,
    Panel,
}

pub fn style_from_palette(config: &Config, palette: Palette) -> Style {
    Style::default().fg(match palette {
        Palette::DirSymlink => config.file_manager.dir_symlink_fg,
        Palette::Archive => config.file_manager.archive_fg,
        Palette::Symlink => config.file_manager.symlink_fg,
        Palette::Stalelink => config.file_manager.stalelink_fg,
        Palette::Directory => config.file_manager.directory_fg,
        Palette::Device => config.file_manager.device_fg,
        Palette::Special => config.file_manager.special_fg,
        Palette::Executable => config.file_manager.executable_fg,
        Palette::Panel => config.panel.fg,
    })
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub file: PathBuf,
    pub file_name: String,
    pub key: String,
    pub extension: String,
    pub label: String,
    pub palette: Palette,
    pub lstat: Metadata,
    pub stat: Metadata,
    pub shown_mtime: String,
    pub size: Option<u64>,
    pub shown_size: String,
    pub details: String,
    pub link_target: Option<PathBuf>,
}

impl PartialEq for Entry {
    fn eq(&self, other: &Entry) -> bool {
        // TODO: Is the `.file` member still the one to compare when using unarchive_path?
        self.file == other.file
    }
}

pub fn get_file_list(cwd: &Path, file_list_rx: Option<Receiver<PathBuf>>) -> Result<Vec<Entry>> {
    let users_cache = UsersCache::new();

    Ok(read_dir(cwd)?
        .filter_map(|e| match e {
            Ok(entry) => match entry.metadata() {
                Ok(metadata) => Some((entry, metadata)),
                Err(_) => None,
            },
            Err(_) => None,
        })
        .map_while(|(entry, metadata)| {
            if let Some(rx) = &file_list_rx {
                // Stop processing the current file list if a new file listing request arrived in the meantime
                if !rx.is_empty() {
                    return None;
                }
            }

            let shown_file = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();
            let extension: String = natsort_key(&tar_suffix(&file_name));

            let (stat, label, palette) = if metadata.is_symlink() {
                match fs::metadata(entry.path()) {
                    Ok(stat) => {
                        if stat.is_dir() {
                            (stat, format!("~{}", file_name), Palette::DirSymlink)
                        } else {
                            (
                                stat,
                                format!("@{}", file_name),
                                match ARCHIVE_EXTENSIONS.contains(&extension.as_str()) {
                                    true => Palette::Archive,
                                    false => Palette::Symlink,
                                },
                            )
                        }
                    }
                    Err(_) => (
                        metadata.clone(),
                        format!("!{}", file_name),
                        Palette::Stalelink,
                    ),
                }
            } else if metadata.is_dir() {
                (
                    metadata.clone(),
                    format!("/{}", file_name),
                    Palette::Directory,
                )
            } else if metadata.file_type().is_char_device() {
                (metadata.clone(), format!("-{}", file_name), Palette::Device)
            } else if metadata.file_type().is_block_device() {
                (metadata.clone(), format!("+{}", file_name), Palette::Device)
            } else if metadata.file_type().is_fifo() {
                (
                    metadata.clone(),
                    format!("|{}", file_name),
                    Palette::Special,
                )
            } else if metadata.file_type().is_socket() {
                (
                    metadata.clone(),
                    format!("={}", file_name),
                    Palette::Special,
                )
            } else if (metadata.mode() & (S_IXUSR | S_IXGRP | S_IXOTH)) != 0 {
                (
                    metadata.clone(),
                    format!("*{}", file_name),
                    match ARCHIVE_EXTENSIONS.contains(&extension.as_str()) {
                        true => Palette::Archive,
                        false => Palette::Executable,
                    },
                )
            } else {
                (
                    metadata.clone(),
                    format!(" {}", file_name),
                    match ARCHIVE_EXTENSIONS.contains(&extension.as_str()) {
                        true => Palette::Archive,
                        false => Palette::Panel,
                    },
                )
            };

            let (size, shown_size) = if stat.is_dir() {
                (None, String::from("DIR"))
            } else if metadata.file_type().is_char_device()
                || metadata.file_type().is_block_device()
            {
                let rdev = metadata.rdev();
                let major = unsafe { libc::major(rdev) };
                let minor = unsafe { libc::minor(rdev) };

                (
                    // This works as long as size is u64 and major and minor are u32
                    Some(((major as u64) << 32) | (minor as u64)),
                    format!("{},{}", major, minor),
                )
            } else {
                let size = metadata.len();

                (Some(size), human_readable_size(size))
            };

            let uid = match users_cache.get_user_by_uid(metadata.uid()) {
                Some(uid) => uid.name().to_string_lossy().to_string(),
                None => metadata.uid().to_string(),
            };

            let gid = match users_cache.get_group_by_gid(metadata.gid()) {
                Some(gid) => gid.name().to_string_lossy().to_string(),
                None => metadata.gid().to_string(),
            };

            let details = format!(
                "{} {} {} {}",
                filemode(metadata.mode()),
                metadata.nlink(),
                uid,
                gid
            );

            let (details, link_target) = if metadata.is_symlink() {
                match fs::read_link(entry.path()) {
                    Ok(link_target) => (
                        format!("{} -> {}", details, link_target.to_string_lossy()),
                        Some(match link_target.is_absolute() {
                            true => link_target.clean(),
                            false => [
                                shown_file.parent().unwrap_or(Path::new("/")),
                                link_target.as_path(),
                            ]
                            .iter()
                            .collect::<PathBuf>()
                            .clean(),
                        }),
                    ),
                    Err(_) => (format!("{} -> ?", details), Some(shown_file.clone())),
                }
            } else {
                (format!("{} {}", details, file_name), None)
            };

            let shown_mtime = match metadata.modified() {
                Ok(modified) => format_date(modified),
                Err(_) => String::from("???????"),
            };

            Some(Entry {
                file: shown_file,
                key: natsort_key(&file_name),
                file_name,
                extension,
                label,
                palette,
                lstat: metadata,
                stat,
                shown_mtime,
                size,
                shown_size,
                details,
                link_target,
            })
        })
        .collect::<Vec<Entry>>())
}

pub fn count_directories(
    file_list: &[Entry],
    file_list_rx: Option<Receiver<PathBuf>>,
) -> Vec<Entry> {
    file_list
        .iter()
        .map_while(|entry| {
            if let Some(rx) = &file_list_rx {
                // Stop processing the current file list if a new file listing request arrived in the meantime
                if !rx.is_empty() {
                    return None;
                }
            }

            match entry.stat.is_dir() {
                true => {
                    let (size, shown_size) = match read_dir(&entry.file) {
                        Ok(entries) => {
                            let num_entries = entries.count();

                            (Some(num_entries as u64), num_entries.to_string())
                        }
                        Err(_) => (None, String::from("?")),
                    };

                    Some(Entry {
                        size,
                        shown_size,
                        ..entry.clone()
                    })
                }
                false => Some(entry.clone()),
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
pub enum HiddenFiles {
    Show,
    Hide,
}

pub fn filter_file_list(
    file_list: &[Entry],
    hidden_files: HiddenFiles,
    file_filter: &str,
) -> Vec<Entry> {
    let file_filter = natsort_key(file_filter);
    let matcher = SkimMatcherV2::default();

    file_list
        .iter()
        .filter(|entry| {
            if matches!(hidden_files, HiddenFiles::Hide) && entry.key.starts_with('.') {
                return false;
            }

            if file_filter.is_empty() {
                return true;
            }

            matcher.fuzzy_match(&entry.key, &file_filter).is_some()
        })
        .cloned()
        .collect()
}

#[derive(Debug, Clone, Copy)]
pub enum SortOrder {
    Normal,
    Reverse,
}

pub fn sort_by_name(a: &Entry, b: &Entry, sort_order: SortOrder) -> Ordering {
    match (a.stat.is_dir(), b.stat.is_dir()) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => {
            let o = natord::compare(&a.key, &b.key)
                .then_with(|| natord::compare(&a.file_name, &b.file_name));

            match sort_order {
                SortOrder::Normal => o,
                SortOrder::Reverse => o.reverse(),
            }
        }
    }
}

pub fn sort_by_extension(a: &Entry, b: &Entry, sort_order: SortOrder) -> Ordering {
    match (a.stat.is_dir(), b.stat.is_dir()) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => {
            let o = natord::compare(&a.extension, &b.extension)
                .then_with(|| sort_by_name(a, b, SortOrder::Normal));

            match sort_order {
                SortOrder::Normal => o,
                SortOrder::Reverse => o.reverse(),
            }
        }
    }
}

pub fn sort_by_date(a: &Entry, b: &Entry, sort_order: SortOrder) -> Ordering {
    match (a.stat.is_dir(), b.stat.is_dir()) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => {
            let o = match (a.lstat.modified(), b.lstat.modified()) {
                (Ok(a_modified), Ok(b_modified)) => a_modified.cmp(&b_modified),
                (Err(_), Ok(_)) => Ordering::Less,
                (Ok(_), Err(_)) => Ordering::Greater,
                _ => Ordering::Equal,
            }
            .then_with(|| sort_by_name(a, b, SortOrder::Normal));

            match sort_order {
                SortOrder::Normal => o,
                SortOrder::Reverse => o.reverse(),
            }
        }
    }
}

pub fn sort_by_size(a: &Entry, b: &Entry, sort_order: SortOrder) -> Ordering {
    match (a.stat.is_dir(), b.stat.is_dir()) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => {
            let o = match (a.size, b.size) {
                (Some(a_size), Some(b_size)) => a_size.cmp(&b_size),
                (None, Some(_)) => Ordering::Less,
                (Some(_), None) => Ordering::Greater,
                _ => Ordering::Equal,
            }
            .then_with(|| sort_by_name(a, b, SortOrder::Normal));

            match sort_order {
                SortOrder::Normal => o,
                SortOrder::Reverse => o.reverse(),
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SortBy {
    Name,
    Extension,
    Date,
    Size,
}

pub fn sort_by_function(sort_by: SortBy) -> fn(&Entry, &Entry, SortOrder) -> Ordering {
    match sort_by {
        SortBy::Name => sort_by_name,
        SortBy::Extension => sort_by_extension,
        SortBy::Date => sort_by_date,
        SortBy::Size => sort_by_size,
    }
}

#[derive(Debug, Clone)]
enum ComponentPubSub {
    FileList(Vec<Entry>),
}

#[derive(Debug)]
pub struct FilePanel {
    config: Config,
    pubsub_tx: Sender<PubSub>,
    rect: Rect,
    component_pubsub_tx: Sender<ComponentPubSub>,
    component_pubsub_rx: Receiver<ComponentPubSub>,
    file_list_tx: Sender<PathBuf>,
    file_list_rx: Receiver<PathBuf>,
    cwd: PathBuf,
    old_cwd: PathBuf,
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
}

impl FilePanel {
    pub fn new(
        config: &Config,
        pubsub_tx: Sender<PubSub>,
        initial_path: &Path,
    ) -> Result<FilePanel> {
        let (component_pubsub_tx, component_pubsub_rx) = crossbeam_channel::unbounded();
        let (file_list_tx, file_list_rx) = crossbeam_channel::unbounded();

        let mut panel = FilePanel {
            config: *config,
            pubsub_tx,
            rect: Rect::default(),
            component_pubsub_tx,
            component_pubsub_rx,
            file_list_tx,
            file_list_rx,
            cwd: PathBuf::new(),
            old_cwd: PathBuf::new(),
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
        };

        panel.file_list_thread();
        panel.chdir(initial_path)?;
        panel.old_cwd = panel.cwd.clone();

        Ok(panel)
    }

    fn handle_component_pubsub(&mut self) -> Result<()> {
        if let Ok(event) = self.component_pubsub_rx.try_recv() {
            match event {
                ComponentPubSub::FileList(file_list) => {
                    self.is_loading = false;

                    self.file_list = file_list;

                    self.shown_file_list =
                        filter_file_list(&self.file_list, self.hidden_files, &self.file_filter);

                    self.shown_file_list
                        .sort_by(|a, b| sort_by_function(self.sort_method)(a, b, self.sort_order));

                    self.tagged_files
                        .retain(|entry| self.file_list.contains(entry));

                    if !self.shown_file_list.is_empty() {
                        self.pubsub_tx
                            .send(PubSub::UpdateQuickView(self.get_selected_file()))
                            .unwrap();
                    }
                }
            }
        }

        Ok(())
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

    fn chdir(&mut self, cwd: &Path) -> Result<()> {
        let new_cwd = cwd
            .ancestors()
            .find(|d| read_dir(d).is_ok())
            .ok_or_else(|| anyhow!("failed to change directory"))?
            .to_path_buf();

        if new_cwd != self.cwd {
            self.old_cwd = self.cwd.clone();
            self.cwd = new_cwd;

            self.file_filter.clear();
            self.tagged_files.clear();
            self.cursor_position = 0;
            self.first_line = 0;

            self.load_file_list()?;
        }

        Ok(())
    }

    fn load_file_list(&mut self) -> Result<()> {
        self.free = disk_usage(&self.cwd)?.free;

        self.is_loading = true;
        self.file_list_tx.send(self.cwd.clone())?;

        Ok(())
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
                .send(PubSub::UpdateQuickView(self.get_selected_file()))
                .unwrap();
        }
    }

    fn handle_down(&mut self) {
        let old_cursor_position = self.cursor_position;

        self.cursor_position = self.clamp_cursor(self.cursor_position.saturating_add(1));

        if self.cursor_position != old_cursor_position {
            self.pubsub_tx
                .send(PubSub::UpdateQuickView(self.get_selected_file()))
                .unwrap();
        }
    }
}

impl Component for FilePanel {
    fn handle_key(&mut self, key: &Key) -> Result<bool> {
        let mut key_handled = true;

        match key {
            Key::Left | Key::Char('h') => {
                let cwd = self.cwd.clone();

                if let Some(new_cwd) = cwd.parent() {
                    self.chdir(new_cwd)?
                }
            }
            Key::Right | Key::Char('\n') | Key::Char('l') => {
                if !self.shown_file_list.is_empty() {
                    let entry = self.shown_file_list[self.cursor_position].clone();

                    if entry.stat.is_dir() {
                        self.chdir(&entry.file)?;
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

                                        self.chdir(path)?
                                    }
                                    false => {
                                        let parent = path.parent().ok_or_else(|| {
                                            anyhow!("failed to read link target parent")
                                        })?;

                                        // Change directory only if we can change to that exact directory
                                        read_dir(parent)?;

                                        self.chdir(parent)?;

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
                                                    .send(PubSub::UpdateQuickView(
                                                        self.get_selected_file(),
                                                    ))
                                                    .unwrap();
                                            }
                                        }
                                    }
                                }

                                Ok(())
                            });
                    }
                    // TODO: Handle archives and regular files
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
                        .send(PubSub::UpdateQuickView(self.get_selected_file()))
                        .unwrap();
                }
            }
            Key::End | Key::Char('G') => {
                let old_cursor_position = self.cursor_position;

                self.cursor_position = self.clamp_cursor(self.shown_file_list.len());

                if self.cursor_position != old_cursor_position {
                    self.pubsub_tx
                        .send(PubSub::UpdateQuickView(self.get_selected_file()))
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
                        .send(PubSub::UpdateQuickView(self.get_selected_file()))
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
                        .send(PubSub::UpdateQuickView(self.get_selected_file()))
                        .unwrap();
                }
            }
            Key::Char('v') | Key::F(3) => {
                if !self.shown_file_list.is_empty() {
                    let entry = self.shown_file_list[self.cursor_position].clone();

                    match entry.stat.is_dir() {
                        true => self.chdir(&entry.file)?,
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
                    for entry in self.shown_file_list.iter() {
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
                    for entry in self.shown_file_list.iter() {
                        if !self.tagged_files.contains(entry) {
                            self.tagged_files.push(entry.clone());
                        }
                    }
                }
            }
            Key::Char('U') => {
                if !self.shown_file_list.is_empty() {
                    for entry in self.shown_file_list.iter() {
                        if let Some(i) = self.tagged_files.iter().position(|x| x == entry) {
                            self.tagged_files.swap_remove(i);
                        }
                    }
                }
            }
            _ => key_handled = false,
        }

        Ok(key_handled)
    }

    fn handle_pubsub(&mut self, event: &PubSub) -> Result<()> {
        match event {
            PubSub::ComponentThreadEvent => self.handle_component_pubsub()?,
            _ => (),
        }

        Ok(())
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
                            &format!(" {} ", self.cwd.to_string_lossy()),
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
                            match (self.tagged_files.contains(entry), is_selected) {
                                (true, true) => Style::default().fg(self.config.ui.markselect_fg),
                                (true, false) => Style::default().fg(self.config.ui.marked_fg),
                                _ => style_from_palette(&self.config, entry.palette),
                            },
                        )
                        .into()
                    })
                    .collect();

                let items = List::new(items).highlight_style(match focus {
                    Focus::Focused => Style::default()
                        .fg(self.config.ui.selected_fg)
                        .bg(self.config.ui.selected_bg),
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
    fn get_selected_file(&self) -> Option<PathBuf> {
        match self.shown_file_list.is_empty() {
            true => None,
            false => Some(self.shown_file_list[self.cursor_position].file.clone()),
        }
    }

    fn get_cwd(&self) -> Option<PathBuf> {
        Some(self.cwd.clone())
    }
}

impl PanelComponent for FilePanel {}
