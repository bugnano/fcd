use std::{
    fs::{self, Metadata},
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
};

use anyhow::Result;
use crossbeam_channel::Sender;
use libc::{S_IXGRP, S_IXOTH, S_IXUSR};
use path_clean::PathClean;
use ratatui::{
    prelude::*,
    widgets::{
        block::{Position, Title},
        *,
    },
};
use uzers::{Groups, Users, UsersCache};

use crate::{
    app::PubSub,
    component::{Component, Focus},
    config::Config,
    fm::app::{human_readable_size, natsort_key, tar_suffix},
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

#[derive(Debug, Clone)]
pub struct Entry {
    file: PathBuf,
    key: String,
    extension: String,
    label: String,
    palette: String,
    lstat: Metadata,
    stat: Metadata,
    length: u64,
    size: String,
    details: String,
    link_target: Option<PathBuf>,
}

pub fn get_file_list(cwd: &Path, count_directories: bool) -> Result<Vec<Entry>> {
    let users_cache = UsersCache::new();

    Ok(fs::read_dir(cwd)?
        .filter_map(|e| match e {
            Ok(entry) => match entry.metadata() {
                Ok(metadata) => Some((entry, metadata)),
                Err(_) => None,
            },
            Err(_) => None,
        })
        .map(|(entry, metadata)| {
            let shown_file = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();
            let extension: String = natsort_key(&tar_suffix(&file_name));

            let (stat, label, palette) = if metadata.is_symlink() {
                match fs::metadata(entry.path()) {
                    Ok(stat) => {
                        if stat.is_dir() {
                            (stat, format!("~{}", file_name), String::from("dir_symlink"))
                        } else {
                            (
                                stat,
                                format!("@{}", file_name),
                                match ARCHIVE_EXTENSIONS.contains(&extension.as_str()) {
                                    true => String::from("archive"),
                                    false => String::from("symlink"),
                                },
                            )
                        }
                    }
                    Err(_) => (
                        metadata.clone(),
                        format!("!{}", file_name),
                        String::from("stalelink"),
                    ),
                }
            } else if metadata.is_dir() {
                (
                    metadata.clone(),
                    format!("/{}", file_name),
                    String::from("directory"),
                )
            } else if metadata.file_type().is_char_device() {
                (
                    metadata.clone(),
                    format!("-{}", file_name),
                    String::from("device"),
                )
            } else if metadata.file_type().is_block_device() {
                (
                    metadata.clone(),
                    format!("+{}", file_name),
                    String::from("device"),
                )
            } else if metadata.file_type().is_fifo() {
                (
                    metadata.clone(),
                    format!("|{}", file_name),
                    String::from("special"),
                )
            } else if metadata.file_type().is_socket() {
                (
                    metadata.clone(),
                    format!("={}", file_name),
                    String::from("special"),
                )
            } else if (metadata.mode() & (S_IXUSR | S_IXGRP | S_IXOTH)) != 0 {
                (
                    metadata.clone(),
                    format!("*{}", file_name),
                    match ARCHIVE_EXTENSIONS.contains(&extension.as_str()) {
                        true => String::from("archive"),
                        false => String::from("executable"),
                    },
                )
            } else {
                (
                    metadata.clone(),
                    format!(" {}", file_name),
                    match ARCHIVE_EXTENSIONS.contains(&extension.as_str()) {
                        true => String::from("archive"),
                        false => String::from("panel"),
                    },
                )
            };

            let (length, size) = if stat.is_dir() {
                if count_directories {
                    match fs::read_dir(entry.path()) {
                        Ok(entries) => {
                            let num_entries = entries.count();

                            (num_entries as u64, num_entries.to_string())
                        }
                        Err(_) => (0, String::from("?")),
                    }
                } else {
                    (0, String::from("DIR"))
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

            Entry {
                file: shown_file,
                key: natsort_key(&file_name),
                extension,
                label,
                palette,
                lstat: metadata,
                stat,
                length,
                size,
                details,
                link_target,
            }
        })
        .collect::<Vec<Entry>>())
}

#[derive(Debug)]
pub struct Panel {
    config: Config,
    pubsub_tx: Sender<PubSub>,
    cwd: PathBuf,
}

impl Panel {
    pub fn new(config: &Config, pubsub_tx: Sender<PubSub>, initial_path: &Path) -> Result<Panel> {
        Ok(Panel {
            config: *config,
            pubsub_tx,
            cwd: initial_path.to_path_buf(),
        })
    }
}

impl Component for Panel {
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

        f.render_widget(upper_block, sections[0]);

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
