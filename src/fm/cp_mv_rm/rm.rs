use std::{
    fs,
    io::ErrorKind,
    path::PathBuf,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender};

use rustix::fs::sync;

use crate::{
    app::PubSub,
    fm::{
        archive_mounter::{unarchive_parent_map, ArchiveEntry},
        cp_mv_rm::database::{DBFileEntry, DBFileStatus, DBJobStatus},
    },
};

#[derive(Debug, Clone)]
pub enum RmEvent {
    Suspend(Receiver<()>),
    Skip,
    Abort,
}

#[derive(Debug, Clone)]
pub struct RmInfo {
    pub current: PathBuf,
    pub num_files: usize,
    pub total_time: Duration,
}

pub fn rm(
    entries: &[DBFileEntry],
    ev_rx: Receiver<RmEvent>,
    info_tx: Sender<RmInfo>,
    pubsub_tx: Sender<PubSub>,
    archive_dirs: &[ArchiveEntry],
) -> (Vec<DBFileEntry>, DBJobStatus) {
    let mut job_status_result = DBJobStatus::InProgress;

    let mut file_list = Vec::from(entries);
    file_list.sort_unstable_by(|a, b| b.file.cmp(&a.file));

    let mut info = RmInfo {
        current: PathBuf::from(""),
        num_files: 0,
        total_time: Duration::ZERO,
    };

    let now = Instant::now();
    let mut start = now.clone();
    let mut last_write = now.clone();
    for entry in file_list.iter_mut() {
        match entry.status {
            DBFileStatus::Error | DBFileStatus::Skipped | DBFileStatus::Done => {
                info.num_files += 1;
                continue;
            }
            _ => {}
        }

        if !ev_rx.is_empty() {
            if let Ok(event) = ev_rx.try_recv() {
                match event {
                    RmEvent::Suspend(suspend_rx) => {
                        let t1 = Instant::now();
                        let _ = suspend_rx.recv();
                        let t2 = Instant::now();
                        let dt = t2.duration_since(t1);
                        start += dt;
                    }
                    RmEvent::Skip => {
                        entry.status = DBFileStatus::Skipped;
                        info.num_files += 1;
                        continue;
                    }
                    RmEvent::Abort => {
                        job_status_result = DBJobStatus::Aborted;
                        break;
                    }
                }
            }
        }

        info.current = entry.file.clone();

        if last_write.elapsed().as_millis() >= 50 {
            last_write = Instant::now();
            info.total_time = last_write.duration_since(start);
            let _ = info_tx.send(info.clone());
            let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
        }

        let actual_file = unarchive_parent_map(&entry.file, archive_dirs);

        let rm_result = match entry.is_dir {
            true => fs::remove_dir(&actual_file),
            false => fs::remove_file(&actual_file),
        };

        match rm_result {
            Ok(()) => entry.status = DBFileStatus::Done,
            Err(e) => {
                match e.kind() {
                    ErrorKind::NotFound => {
                        // Deleting a non-existing file is a no-op
                        entry.status = DBFileStatus::Done;
                    }
                    _ => {
                        entry.message = format!("(rm) {}", e);
                        entry.status = DBFileStatus::Error;
                    }
                }
            }
        }

        info.num_files += 1;
    }

    if let DBJobStatus::InProgress = job_status_result {
        job_status_result = DBJobStatus::Done;
    }

    sync();

    (file_list, job_status_result)
}
