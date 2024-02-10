use std::{
    fs::{self, Metadata},
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
    thread,
};

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
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

#[derive(Debug, Clone)]
pub struct Entry {
    file: PathBuf,
    key: String,
    extension: String,
    label: String,
    palette: Palette,
    lstat: Metadata,
    stat: Metadata,
    mtime: String,
    length: u64,
    size: String,
    details: String,
    link_target: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
pub enum CountDirectories {
    Yes,
    No,
}

pub fn get_file_list(
    cwd: &Path,
    count_directories: CountDirectories,
    file_list_rx: Option<Receiver<PathBuf>>,
) -> Result<Vec<Entry>> {
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

            let (length, size) = if stat.is_dir() {
                match count_directories {
                    CountDirectories::Yes => match fs::read_dir(entry.path()) {
                        Ok(entries) => {
                            let num_entries = entries.count();

                            (num_entries as u64, num_entries.to_string())
                        }
                        Err(_) => (0, String::from("?")),
                    },
                    CountDirectories::No => (0, String::from("DIR")),
                }
            } else if metadata.file_type().is_char_device()
                || metadata.file_type().is_block_device()
            {
                let rdev = metadata.rdev();
                let major = unsafe { libc::major(rdev) };
                let minor = unsafe { libc::minor(rdev) };

                (rdev, format!("{},{}", major, minor))
            } else {
                let length = metadata.len();

                (length, human_readable_size(length))
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

            let mtime = match metadata.modified() {
                Ok(modified) => format_date(modified),
                Err(_) => String::from("???????"),
            };

            Some(Entry {
                file: shown_file,
                key: natsort_key(&file_name),
                extension,
                label,
                palette,
                lstat: metadata,
                stat,
                mtime,
                length,
                size,
                details,
                link_target,
            })
        })
        .collect::<Vec<Entry>>())
}

pub fn color_from_palette(config: &Config, palette: Palette) -> Color {
    match palette {
        Palette::DirSymlink => config.file_manager.dir_symlink_fg,
        Palette::Archive => config.file_manager.archive_fg,
        Palette::Symlink => config.file_manager.symlink_fg,
        Palette::Stalelink => config.file_manager.stalelink_fg,
        Palette::Directory => config.file_manager.directory_fg,
        Palette::Device => config.file_manager.device_fg,
        Palette::Special => config.file_manager.special_fg,
        Palette::Executable => config.file_manager.executable_fg,
        Palette::Panel => config.panel.fg,
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

                    self.shown_file_list = file_list.clone();
                    self.file_list = file_list;
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
                let file_list =
                    get_file_list(&cwd, CountDirectories::No, Some(file_list_rx.clone()))
                        .unwrap_or_default();

                // Send the current result only if there are no newer file list requests in the queue,
                // otherwise discard the current result
                if file_list_rx.is_empty() {
                    // First send the component event
                    let _ = component_pubsub_tx.send(ComponentPubSub::FileList(file_list));

                    // Then notify the app that there is an component event
                    let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);

                    // Step 2: Get the current file list counting the directories
                    let file_list =
                        get_file_list(&cwd, CountDirectories::Yes, Some(file_list_rx.clone()))
                            .unwrap_or_default();

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
                        let filename_width = (upper_inner.width as usize)
                            .saturating_sub(entry.size.width())
                            .saturating_sub(9);

                        let filename = tilde_layout(&entry.label, filename_width);

                        // The reason why I add {:width$} whitespaces after the
                        // filename instead of putting the filename directly
                        // inside {:width$} is because the {:width$} formatting
                        // has a bug with some 0-width Unicode characters
                        Span::styled(
                            format!(
                                "{}{:width$} {} {}",
                                &filename,
                                "",
                                &entry.size,
                                &entry.mtime,
                                width = filename_width.saturating_sub(filename.width())
                            ),
                            Style::default().fg(color_from_palette(&self.config, entry.palette)),
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
