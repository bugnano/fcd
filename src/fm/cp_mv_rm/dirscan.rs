use std::{
    fs::{self, FileType, Metadata},
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
    pub files: usize,
    pub bytes: Option<u64>,
}

pub fn dirscan(
    job_id: i64,
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
        files: 0,
        bytes: match read_metadata {
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
                            job_id,
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
                        info.files = 0;
                        info.bytes = match read_metadata {
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
        info.files += 1;
        info.bytes = match read_metadata {
            ReadMetadata::Yes => info.bytes.map(|bytes| bytes + entry.size),
            ReadMetadata::No => None,
        };

        if last_write.elapsed().as_millis() >= 50 {
            last_write = Instant::now();
            let _ = info_tx.send(info.clone());
            let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
        }

        result.push(DBFileEntry {
            id: 0,
            job_id,
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
                job_id,
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
                    let mut last_result = result.last_mut().unwrap();
                    // TODO -- last_result.message = f'({when}) {e.strerror} ({e.errno})'
                    last_result.status = DBFileStatus::Error;
                }
            }
        }
    }

    Some(result)
}

fn recursive_dirscan(
    job_id: i64,
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

    let old_files = info.files;
    let old_bytes = info.bytes;

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
                            job_id,
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
                        info.files = old_files;
                        info.bytes = old_bytes;
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
                                    job_id,
                                    file: archive_mounter::archive_path_map(
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
                                    // TODO -- message: f'({when}) {e.strerror} ({e.errno})'
                                    message: e.to_string(),
                                    target_is_dir: false,
                                    target_is_symlink: false,
                                    cur_target: None,
                                });
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
                        last_write = Instant::now();
                        let _ = info_tx.send(info.clone());
                        let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
                    }

                    let file = archive_mounter::archive_path_map(&entry.path(), archive_dirs);

                    result.push(DBFileEntry {
                        id: 0,
                        job_id,
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
                            job_id,
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
                                let mut last_result = result.last_mut().unwrap();
                                // TODO -- last_result.message = f'({when}) {e.strerror} ({e.errno})'
                                last_result.status = DBFileStatus::Error;
                            }
                        }
                    }
                }
                Err(e) => result.push(DBFileEntry {
                    id: 0,
                    job_id,
                    file: archive_mounter::archive_path_map(&entry.path(), archive_dirs),
                    is_file: false,
                    is_dir: false,
                    is_symlink: false,
                    size: 0,
                    uid: 0,
                    gid: 0,
                    status: DBFileStatus::Error,
                    // TODO -- message: f'({when}) {e.strerror} ({e.errno})'
                    message: e.to_string(),
                    target_is_dir: false,
                    target_is_symlink: false,
                    cur_target: None,
                }),
            }
        }
    }

    Ok(Some((result, last_write)))
}
