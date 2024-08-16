use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};

use crate::{
    app::PubSub,
    fm::{
        archive_mounter::{self, ArchiveEntry},
        cp_mv_rm::database::{DBEntriesEntry, DBFileEntry, DBFileStatus},
    },
};

#[derive(Debug, Clone, Copy)]
pub enum ReadMetadata {
    Yes,
    No,
}

#[derive(Debug, Clone)]
pub enum DirScanEvent {
    Suspend(Receiver<()>),
    Skip,
    Abort,
}

#[derive(Debug, Clone)]
pub struct DirScanInfo {
    pub current: PathBuf,
    pub num_files: usize,
    pub total_size: Option<u64>,
}

pub fn dirscan(
    cwd: &Path,
    entries: &[DBEntriesEntry],
    archive_dirs: &[ArchiveEntry],
    read_metadata: ReadMetadata,
    ev_rx: Receiver<DirScanEvent>,
    info_tx: Sender<DirScanInfo>,
    pubsub_tx: Sender<PubSub>,
) -> Option<Vec<DBFileEntry>> {
    let mut result = Vec::new();

    let mut info = DirScanInfo {
        current: PathBuf::from(cwd),
        num_files: 0,
        total_size: match read_metadata {
            ReadMetadata::Yes => Some(0),
            ReadMetadata::No => None,
        },
    };

    let mut last_write = Instant::now();
    for entry in entries.iter() {
        if !ev_rx.is_empty() {
            if let Ok(event) = ev_rx.try_recv() {
                match event {
                    DirScanEvent::Suspend(suspend_rx) => {
                        let _ = suspend_rx.recv();
                    }
                    DirScanEvent::Skip => {
                        result.clear();
                        result.push(DBFileEntry {
                            id: 0,
                            job_id: 0,
                            file: PathBuf::from(cwd),
                            is_file: false,
                            is_dir: true,
                            is_symlink: false,
                            size: 0,
                            uid: 0,
                            gid: 0,
                            status: DBFileStatus::Skipped,
                            message: String::from(""),
                            target_is_dir: false,
                            target_is_symlink: false,
                            cur_target: None,
                        });
                        info.num_files = 1;
                        info.total_size = match read_metadata {
                            ReadMetadata::Yes => Some(0),
                            ReadMetadata::No => None,
                        };
                        break;
                    }
                    DirScanEvent::Abort => return None,
                }
            }
        }

        info.current = PathBuf::from(cwd);
        info.num_files += 1;
        info.total_size = match read_metadata {
            ReadMetadata::Yes => info.total_size.map(|total_size| total_size + entry.size),
            ReadMetadata::No => None,
        };

        if last_write.elapsed().as_millis() >= 50 {
            last_write = Instant::now();
            let _ = info_tx.send(info.clone());
            let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
        }

        result.push(DBFileEntry {
            id: 0,
            job_id: 0,
            file: entry.file.clone(),
            is_file: entry.is_file,
            is_dir: entry.is_dir,
            is_symlink: entry.is_symlink,
            size: entry.size,
            uid: entry.uid,
            gid: entry.gid,
            status: DBFileStatus::ToDo,
            message: String::from(""),
            target_is_dir: false,
            target_is_symlink: false,
            cur_target: None,
        });

        if entry.is_dir {
            match recursive_dirscan(
                &entry.file,
                archive_dirs,
                read_metadata,
                &mut info,
                last_write,
                ev_rx.clone(),
                info_tx.clone(),
                pubsub_tx.clone(),
            ) {
                Ok(Some((recursive_result, recursive_last_write))) => {
                    if let Some(last_result) = recursive_result.last() {
                        if let DBFileStatus::Skipped = last_result.status {
                            result.pop();
                        }
                    }

                    result.extend(recursive_result);
                    last_write = recursive_last_write;
                }
                Ok(None) => return None,
                Err(e) => {
                    let last_result = result.last_mut().unwrap();
                    last_result.message = format!("(dirscan) {}", e);
                    last_result.status = DBFileStatus::Error;
                }
            }
        }
    }

    Some(result)
}

#[allow(clippy::too_many_arguments)]
fn recursive_dirscan(
    cwd: &Path,
    archive_dirs: &[ArchiveEntry],
    read_metadata: ReadMetadata,
    info: &mut DirScanInfo,
    old_last_write: Instant,
    ev_rx: Receiver<DirScanEvent>,
    info_tx: Sender<DirScanInfo>,
    pubsub_tx: Sender<PubSub>,
) -> Result<Option<(Vec<DBFileEntry>, Instant)>> {
    let mut result = Vec::new();

    let old_num_files = info.num_files;
    let old_total_size = info.total_size;

    let mut last_write = old_last_write;
    for entry in fs::read_dir(archive_mounter::unarchive_path_map(cwd, archive_dirs))? {
        if !ev_rx.is_empty() {
            if let Ok(event) = ev_rx.try_recv() {
                match event {
                    DirScanEvent::Suspend(suspend_rx) => {
                        let _ = suspend_rx.recv();
                    }
                    DirScanEvent::Skip => {
                        result.clear();
                        result.push(DBFileEntry {
                            id: 0,
                            job_id: 0,
                            file: PathBuf::from(cwd),
                            is_file: false,
                            is_dir: true,
                            is_symlink: false,
                            size: 0,
                            uid: 0,
                            gid: 0,
                            status: DBFileStatus::Skipped,
                            message: String::from(""),
                            target_is_dir: false,
                            target_is_symlink: false,
                            cur_target: None,
                        });
                        info.num_files = old_num_files;
                        info.total_size = old_total_size;
                        return Ok(Some((result, last_write)));
                    }
                    DirScanEvent::Abort => return Ok(None),
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
                                result.push(DBFileEntry {
                                    id: 0,
                                    job_id: 0,
                                    file: archive_mounter::archive_parent_map(
                                        &entry.path(),
                                        archive_dirs,
                                    ),
                                    is_file: false,
                                    is_dir: false,
                                    is_symlink: false,
                                    size: 0,
                                    uid: 0,
                                    gid: 0,
                                    status: DBFileStatus::Error,
                                    message: format!("(dirscan) {}", e),
                                    target_is_dir: false,
                                    target_is_symlink: false,
                                    cur_target: None,
                                });
                                info.num_files += 1;
                                continue;
                            }
                        },
                        ReadMetadata::No => None,
                    };

                    info.current = PathBuf::from(cwd);
                    info.num_files += 1;
                    info.total_size = match metadata {
                        Some(ref metadata) => info
                            .total_size
                            .map(|total_size| total_size + metadata.len()),
                        None => None,
                    };

                    if last_write.elapsed().as_millis() >= 50 {
                        last_write = Instant::now();
                        let _ = info_tx.send(info.clone());
                        let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
                    }

                    let file = archive_mounter::archive_parent_map(&entry.path(), archive_dirs);

                    result.push(DBFileEntry {
                        id: 0,
                        job_id: 0,
                        file: file.clone(),
                        is_file: file_type.is_file(),
                        is_dir: file_type.is_dir(),
                        is_symlink: file_type.is_symlink(),
                        size: metadata
                            .as_ref()
                            .map(|metadata| metadata.len())
                            .unwrap_or(0),
                        uid: metadata
                            .as_ref()
                            .map(|metadata| metadata.uid())
                            .unwrap_or(0),
                        gid: metadata
                            .as_ref()
                            .map(|metadata| metadata.gid())
                            .unwrap_or(0),
                        status: DBFileStatus::ToDo,
                        message: String::from(""),
                        target_is_dir: false,
                        target_is_symlink: false,
                        cur_target: None,
                    });

                    if file_type.is_dir() {
                        match recursive_dirscan(
                            &file,
                            archive_dirs,
                            read_metadata,
                            info,
                            last_write,
                            ev_rx.clone(),
                            info_tx.clone(),
                            pubsub_tx.clone(),
                        ) {
                            Ok(Some((recursive_result, recursive_last_write))) => {
                                if let Some(last_result) = recursive_result.last() {
                                    if let DBFileStatus::Skipped = last_result.status {
                                        result.pop();
                                    }
                                }

                                result.extend(recursive_result);
                                last_write = recursive_last_write;
                            }
                            Ok(None) => return Ok(None),
                            Err(e) => {
                                let last_result = result.last_mut().unwrap();
                                last_result.message = format!("(dirscan) {}", e);
                                last_result.message = e.to_string();
                                last_result.status = DBFileStatus::Error;
                            }
                        }
                    }
                }
                Err(e) => {
                    result.push(DBFileEntry {
                        id: 0,
                        job_id: 0,
                        file: archive_mounter::archive_parent_map(&entry.path(), archive_dirs),
                        is_file: false,
                        is_dir: false,
                        is_symlink: false,
                        size: 0,
                        uid: 0,
                        gid: 0,
                        status: DBFileStatus::Error,
                        message: format!("(dirscan) {}", e),
                        target_is_dir: false,
                        target_is_symlink: false,
                        cur_target: None,
                    });
                    info.num_files += 1;
                }
            }
        }
    }

    Ok(Some((result, last_write)))
}
