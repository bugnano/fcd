use std::{
    cmp::Ordering,
    fs::{self, read_dir, Metadata},
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
};

use anyhow::Result;
use crossbeam_channel::Receiver;
use ratatui::prelude::*;

use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use libc::{S_IXGRP, S_IXOTH, S_IXUSR};
use path_clean::PathClean;
use uzers::{Groups, Users, UsersCache};

use crate::{
    config::Config,
    fm::app::{format_date, human_readable_size, natsort_key, tar_suffix},
    stat::filemode,
};

pub const ARCHIVE_EXTENSIONS: &[&str] = &[
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
