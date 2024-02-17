use std::{
    cmp::Ordering,
    fs::{self, Metadata},
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
    thread,
};

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use libc::{S_IXGRP, S_IXOTH, S_IXUSR};
use path_clean::PathClean;
use ratatui::{
    prelude::*,
    widgets::{
        block::{Position, Title},
        *,
    },
};
use unicode_width::UnicodeWidthStr;
use uzers::{Groups, Users, UsersCache};

use crate::{
    app::PubSub,
    component::{Component, Focus},
    config::Config,
    fm::app::{format_date, human_readable_size, natsort_key, tar_suffix},
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

pub fn get_file_list(cwd: &Path, file_list_rx: Option<Receiver<PathBuf>>) -> Result<Vec<Entry>> {
    let users_cache = UsersCache::new();

    Ok(fs::read_dir(cwd)?
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
                    let (size, shown_size) = match fs::read_dir(&entry.file) {
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
pub struct Panel {
    config: Config,
    pubsub_tx: Sender<PubSub>,
    component_pubsub_tx: Sender<ComponentPubSub>,
    component_pubsub_rx: Receiver<ComponentPubSub>,
    file_list_tx: Sender<PathBuf>,
    file_list_rx: Receiver<PathBuf>,
    cwd: PathBuf,
    is_loading: bool,
    file_list: Vec<Entry>,
    shown_file_list: Vec<Entry>,
    hidden_files: HiddenFiles,
    file_filter: String,
    sort_method: SortBy,
    sort_order: SortOrder,
}

impl Panel {
    pub fn new(config: &Config, pubsub_tx: Sender<PubSub>, initial_path: &Path) -> Result<Panel> {
        let (component_pubsub_tx, component_pubsub_rx) = crossbeam_channel::unbounded();
        let (file_list_tx, file_list_rx) = crossbeam_channel::unbounded();

        let mut panel = Panel {
            config: *config,
            pubsub_tx,
            component_pubsub_tx,
            component_pubsub_rx,
            file_list_tx,
            file_list_rx,
            cwd: initial_path.to_path_buf(),
            is_loading: false,
            file_list: Vec::new(),
            shown_file_list: Vec::new(),
            hidden_files: HiddenFiles::Hide,
            file_filter: String::from(""),
            sort_method: SortBy::Name,
            sort_order: SortOrder::Normal,
        };

        panel.file_list_thread();
        panel.load_file_list()?;

        Ok(panel)
    }

    pub fn handle_component_pubsub(&mut self) -> Result<()> {
        if let Ok(event) = self.component_pubsub_rx.try_recv() {
            match event {
                ComponentPubSub::FileList(file_list) => {
                    self.is_loading = false;

                    self.file_list = file_list;

                    self.shown_file_list =
                        filter_file_list(&self.file_list, self.hidden_files, &self.file_filter);

                    self.shown_file_list
                        .sort_by(|a, b| sort_by_function(self.sort_method)(a, b, self.sort_order));
                }
            }
        }

        Ok(())
    }

    pub fn file_list_thread(&mut self) {
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

    pub fn load_file_list(&mut self) -> Result<()> {
        self.is_loading = true;
        self.file_list_tx.send(self.cwd.clone())?;

        Ok(())
    }
}

impl Component for Panel {
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
                    .map(|entry| {
                        let filename_max_width = (upper_inner.width as usize)
                            .saturating_sub(entry.shown_size.width())
                            .saturating_sub(9);

                        let filename = tilde_layout(&entry.label, filename_max_width);
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
                            style_from_palette(&self.config, entry.palette),
                        )
                        .into()
                    })
                    .collect();

                let items = List::new(items);

                f.render_widget(items, upper_inner);
            }
        }

        let lower_block = Block::default()
            .borders(Borders::ALL)
            .border_set(middle_border_set)
            .style(
                Style::default()
                    .fg(self.config.panel.fg)
                    .bg(self.config.panel.bg),
            );

        f.render_widget(lower_block, sections[1]);
    }
}
