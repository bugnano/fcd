use std::{
    fs::{self, FileType, Metadata},
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};

use crate::{app::PubSub, fm::entry::Entry};

#[derive(Debug, Clone, Copy)]
pub enum ReadMetadata {
    Yes,
    No,
}

#[derive(Debug, Clone, Copy)]
pub enum DirScanEvent {
    Abort,
    Skip,
}

#[derive(Debug, Clone)]
pub struct DirScanInfo {
    pub current: PathBuf,
    pub files: usize,
    pub bytes: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct DirScanEntry {
    pub file: PathBuf,
    pub file_type: FileType,
    pub lstat: Option<Metadata>,
}

#[derive(Debug, Clone)]
pub struct DirScanResult {
    pub entries: Vec<DirScanEntry>,
    pub errors: Vec<(PathBuf, String)>,
    pub skipped: Vec<PathBuf>,
}

pub fn dirscan(
    entries: &[Entry],
    cwd: &Path,
    read_metadata: ReadMetadata,
    ev_rx: Receiver<DirScanEvent>,
    info_tx: Sender<DirScanInfo>,
    pubsub_tx: Sender<PubSub>,
) -> DirScanResult {
    let mut result = DirScanResult {
        entries: Vec::new(),
        errors: Vec::new(),
        skipped: Vec::new(),
    };

    let mut info = DirScanInfo {
        current: PathBuf::from(cwd),
        files: 0,
        bytes: match read_metadata {
            ReadMetadata::Yes => Some(0),
            ReadMetadata::No => None,
        },
    };

    let mut last_write = Instant::now();
    for entry in entries {
        if !ev_rx.is_empty() {
            if let Ok(event) = ev_rx.try_recv() {
                match event {
                    DirScanEvent::Abort => break,
                    DirScanEvent::Skip => {
                        result.entries.clear();
                        result.errors.clear();
                        result.skipped.clear();
                        result.skipped.push(PathBuf::from(cwd));
                        info.files = 0;
                        info.bytes = match read_metadata {
                            ReadMetadata::Yes => Some(0),
                            ReadMetadata::No => None,
                        };
                        break;
                    }
                }
            }
        }

        info.current = PathBuf::from(cwd);
        info.files += 1;
        info.bytes = match read_metadata {
            ReadMetadata::Yes => info.bytes.map(|bytes| bytes + entry.lstat.len()),
            ReadMetadata::No => None,
        };

        if last_write.elapsed().as_millis() >= 50 {
            let _ = info_tx.send(info.clone());
            let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
            last_write = Instant::now();
        }

        result.entries.push(DirScanEntry {
            file: entry.file.clone(),
            file_type: entry.lstat.file_type(),
            lstat: match read_metadata {
                ReadMetadata::Yes => Some(entry.lstat.clone()),
                ReadMetadata::No => None,
            },
        });

        if entry.lstat.is_dir() {
            match recursive_dirscan(
                &entry.file,
                read_metadata,
                &mut info,
                last_write,
                ev_rx.clone(),
                info_tx.clone(),
                pubsub_tx.clone(),
            ) {
                Ok(Some((recursive_result, recursive_last_write))) => {
                    if recursive_result.skipped.iter().any(|e| e == &entry.file) {
                        result.entries.pop();

                        info.files -= 1;
                        info.bytes = match read_metadata {
                            ReadMetadata::Yes => info.bytes.map(|bytes| bytes - entry.lstat.len()),
                            ReadMetadata::No => None,
                        };
                    }

                    result.entries.extend(recursive_result.entries);
                    result.errors.extend(recursive_result.errors);
                    result.skipped.extend(recursive_result.skipped);
                    last_write = recursive_last_write;
                }
                Ok(None) => break,
                Err(e) => {
                    result.entries.pop();

                    info.files -= 1;
                    info.bytes = match read_metadata {
                        ReadMetadata::Yes => info.bytes.map(|bytes| bytes - entry.lstat.len()),
                        ReadMetadata::No => None,
                    };

                    result.errors.push((entry.file.clone(), e.to_string()));
                }
            }
        }
    }

    result
}

fn recursive_dirscan(
    cwd: &Path,
    read_metadata: ReadMetadata,
    info: &mut DirScanInfo,
    old_last_write: Instant,
    ev_rx: Receiver<DirScanEvent>,
    info_tx: Sender<DirScanInfo>,
    pubsub_tx: Sender<PubSub>,
) -> Result<Option<(DirScanResult, Instant)>> {
    let mut result = DirScanResult {
        entries: Vec::new(),
        errors: Vec::new(),
        skipped: Vec::new(),
    };

    let old_files = info.files;
    let old_bytes = info.bytes;

    let mut last_write = old_last_write;
    for entry in fs::read_dir(cwd)? {
        if !ev_rx.is_empty() {
            if let Ok(event) = ev_rx.try_recv() {
                match event {
                    DirScanEvent::Abort => return Ok(None),
                    DirScanEvent::Skip => {
                        result.entries.clear();
                        result.errors.clear();
                        result.skipped.clear();
                        result.skipped.push(PathBuf::from(cwd));
                        info.files = old_files;
                        info.bytes = old_bytes;
                        return Ok(Some((result, last_write)));
                    }
                }
            }
        }

        if let Ok(entry) = entry {
            match entry.file_type() {
                Ok(file_type) => {
                    let metadata = match read_metadata {
                        ReadMetadata::Yes => match entry.metadata() {
                            Ok(metadata) => Some(metadata),
                            Err(e) => {
                                result.errors.push((entry.path(), e.to_string()));
                                continue;
                            }
                        },
                        ReadMetadata::No => None,
                    };

                    info.current = PathBuf::from(cwd);
                    info.files += 1;
                    info.bytes = match metadata {
                        Some(ref metadata) => info.bytes.map(|bytes| bytes + metadata.len()),
                        None => None,
                    };

                    if last_write.elapsed().as_millis() >= 50 {
                        let _ = info_tx.send(info.clone());
                        let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
                        last_write = Instant::now();
                    }

                    let file = entry.path();

                    result.entries.push(DirScanEntry {
                        file: file.clone(),
                        file_type,
                        lstat: metadata.clone(),
                    });

                    if file_type.is_dir() {
                        match recursive_dirscan(
                            &file,
                            read_metadata,
                            info,
                            last_write,
                            ev_rx.clone(),
                            info_tx.clone(),
                            pubsub_tx.clone(),
                        ) {
                            Ok(Some((recursive_result, recursive_last_write))) => {
                                if recursive_result.skipped.contains(&file) {
                                    result.entries.pop();

                                    info.files -= 1;
                                    info.bytes = match metadata {
                                        Some(metadata) => {
                                            info.bytes.map(|bytes| bytes - metadata.len())
                                        }
                                        None => None,
                                    };
                                }

                                result.entries.extend(recursive_result.entries);
                                result.errors.extend(recursive_result.errors);
                                result.skipped.extend(recursive_result.skipped);
                                last_write = recursive_last_write;
                            }
                            Ok(None) => return Ok(None),
                            Err(e) => {
                                result.entries.pop();

                                info.files -= 1;
                                info.bytes = match metadata {
                                    Some(metadata) => {
                                        info.bytes.map(|bytes| bytes - metadata.len())
                                    }
                                    None => None,
                                };

                                result.errors.push((file.clone(), e.to_string()));
                            }
                        }
                    }
                }
                Err(e) => result.errors.push((entry.path(), e.to_string())),
            }
        }
    }

    Ok(Some((result, last_write)))
}
