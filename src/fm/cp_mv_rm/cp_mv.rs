use std::{
    fs,
    io::ErrorKind,
    os::unix::fs::{lchown, symlink, MetadataExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};

use pathdiff::diff_paths;
use rustix::{
    fs::{
        copy_file_range, fallocate, fstat, fsync, open, seek, sendfile, sync, FallocateFlags, Mode,
        OFlags, SeekFrom,
    },
    io::{read, write, Errno},
};

use crate::{
    app::PubSub,
    fm::{
        archive_mounter::{unarchive_parent_map, unarchive_path_map, ArchiveEntry},
        cp_mv_rm::{
            database::{
                DBDirListEntry, DBFileEntry, DBFileStatus, DBJobStatus, DBRenameDirEntry,
                DBSkipDirEntry, DataBase, OnConflict,
            },
            dlg_cp_mv::DlgCpMvType,
        },
    },
    shutil,
};

#[derive(Debug, Clone)]
pub enum CpMvEvent {
    Suspend(Receiver<()>),
    Skip,
    Abort,
    NoDb,
}

#[derive(Debug, Clone, Copy)]
enum CopyMethod {
    CopyFileRange,
    Sendfile,
    ReadWrite,
}

#[derive(Debug, Clone)]
pub struct CpMvInfo {
    pub cur_source: PathBuf,
    pub cur_target: PathBuf,
    pub cur_size: u64,
    pub cur_bytes: u64,
    pub cur_time: Duration,
    pub num_files: usize,
    pub total_bytes: u64,
    pub total_time: Duration,
}

#[derive(Debug, Clone)]
pub struct CpMvResult {
    pub files: Vec<DBFileEntry>,
    pub dirs: Vec<DBDirListEntry>,
    pub status: DBJobStatus,
}

#[derive(Debug, Clone)]
struct Timers {
    pub start: Instant,
    pub last_write: Instant,
    pub cur_start: Instant,
}

pub fn cp_mv(
    job_id: i64,
    mode: DlgCpMvType,
    entries: &[DBFileEntry],
    cwd: &Path,
    dest: &Path,
    on_conflict: OnConflict,
    ev_rx: Receiver<CpMvEvent>,
    info_tx: Sender<CpMvInfo>,
    pubsub_tx: Sender<PubSub>,
    db_file: Option<&Path>,
    archive_dirs: &[ArchiveEntry],
) -> CpMvResult {
    let mut job_status_result = DBJobStatus::InProgress;

    let mut file_list = Vec::from(entries);
    file_list.sort_unstable_by(|a, b| a.file.cmp(&b.file));

    let actual_dest = unarchive_path_map(dest, archive_dirs);

    let default_block_size: i64 = 128 * 1024;
    let block_size = match fs::metadata(&actual_dest) {
        Ok(metadata) => {
            let fs_block_size = metadata.blksize() as i64;

            (default_block_size + (fs_block_size - default_block_size).rem_euclid(fs_block_size))
                as u64
        }
        Err(_) => default_block_size as u64,
    };

    let mut info = CpMvInfo {
        cur_source: PathBuf::new(),
        cur_target: PathBuf::new(),
        cur_size: 0,
        cur_bytes: 0,
        cur_time: Duration::ZERO,
        num_files: 0,
        total_bytes: 0,
        total_time: Duration::ZERO,
    };

    let mut database = db_file.and_then(|db_file| DataBase::new(db_file).ok());

    let mut dir_list = match &database {
        Some(db) => db.get_dir_list(job_id),
        None => Vec::new(),
    };

    let mut rename_dir_stack = match &database {
        Some(db) => db.get_rename_dir_stack(job_id),
        None => Vec::new(),
    };

    let mut skip_dir_stack = match &database {
        Some(db) => db.get_skip_dir_stack(job_id),
        None => Vec::new(),
    };

    let replace_first_path = database
        .as_ref()
        .and_then(|db| db.get_replace_first_path(job_id));

    let replace_first_path = replace_first_path.unwrap_or_else(|| {
        let replace_first_path = !actual_dest.is_dir();

        if let Some(db) = &database {
            db.set_replace_first_path(job_id, replace_first_path);
        }

        replace_first_path
    });

    let now = Instant::now();
    let mut timers = Timers {
        start: now,
        last_write: now,
        cur_start: now,
    };

    let mut total_bytes = 0;

    for entry in file_list.iter_mut() {
        match entry.status {
            DBFileStatus::Error | DBFileStatus::Skipped | DBFileStatus::Done => {
                total_bytes += entry.size;
                info.total_bytes = total_bytes;
                info.num_files += 1;
                continue;
            }
            _ => {}
        }

        match cp_mv_entry(
            job_id,
            mode,
            entry,
            cwd,
            dest,
            on_conflict,
            &ev_rx,
            &info_tx,
            &pubsub_tx,
            block_size,
            &mut info,
            &mut dir_list,
            &mut rename_dir_stack,
            &mut skip_dir_stack,
            replace_first_path,
            &mut timers,
            &mut database,
            archive_dirs,
        ) {
            Ok((file_status, job_status)) => {
                entry.status = file_status;

                if let Some(db) = &database {
                    db.set_file_status(entry);
                }

                if let DBJobStatus::Aborted = job_status {
                    job_status_result = DBJobStatus::Aborted;

                    if let Some(db) = &database {
                        db.set_job_status(job_id, DBJobStatus::Aborted);
                    }

                    break;
                }
            }
            Err(e) => {
                entry.message = format!("({}) {}", e, e.root_cause());
                entry.status = DBFileStatus::Error;

                if let Some(db) = &database {
                    db.set_file_status(entry);
                }
            }
        }

        total_bytes += entry.size;
        info.total_bytes = total_bytes;
        info.num_files += 1;
    }

    for entry in dir_list.iter_mut().rev() {
        match entry.status {
            DBFileStatus::Error | DBFileStatus::Skipped | DBFileStatus::Done => {
                continue;
            }
            _ => {}
        }

        match handle_dir_entry(
            job_id,
            mode,
            entry,
            cwd,
            &ev_rx,
            &info_tx,
            &pubsub_tx,
            &mut info,
            &mut timers,
            &mut database,
            archive_dirs,
        ) {
            Ok((file_status, job_status)) => {
                entry.status = file_status;

                if let Some(db) = &database {
                    db.set_dir_list_entry_status(entry);
                }

                if let DBJobStatus::Aborted = job_status {
                    job_status_result = DBJobStatus::Aborted;

                    if let Some(db) = &database {
                        db.set_job_status(job_id, DBJobStatus::Aborted);
                    }

                    break;
                }
            }
            Err(e) => {
                entry.message = format!("({}) {}", e, e.root_cause());
                entry.status = DBFileStatus::Error;

                if let Some(db) = &database {
                    db.set_dir_list_entry_status(entry);
                }
            }
        }
    }

    if let DBJobStatus::InProgress = job_status_result {
        job_status_result = DBJobStatus::Done;

        if let Some(db) = &database {
            db.set_job_status(job_id, DBJobStatus::Done);
        }
    }

    if database.is_none() {
        sync();
    }

    CpMvResult {
        files: file_list,
        dirs: dir_list,
        status: job_status_result,
    }
}

fn cp_mv_entry(
    job_id: i64,
    mode: DlgCpMvType,
    entry: &mut DBFileEntry,
    cwd: &Path,
    dest: &Path,
    on_conflict: OnConflict,
    ev_rx: &Receiver<CpMvEvent>,
    info_tx: &Sender<CpMvInfo>,
    pubsub_tx: &Sender<PubSub>,
    block_size: u64,
    info: &mut CpMvInfo,
    dir_list: &mut Vec<DBDirListEntry>,
    rename_dir_stack: &mut Vec<DBRenameDirEntry>,
    skip_dir_stack: &mut Vec<DBSkipDirEntry>,
    replace_first_path: bool,
    timers: &mut Timers,
    database: &mut Option<DataBase>,
    archive_dirs: &[ArchiveEntry],
) -> Result<(DBFileStatus, DBJobStatus)> {
    timers.cur_start = Instant::now();

    let cur_file = PathBuf::from(&entry.file);

    // TODO: I don't like this unwrap() call
    let rel_file = diff_paths(&cur_file, cwd).unwrap();

    let mut skip_dir = false;
    while !skip_dir_stack.is_empty() {
        let dir_to_skip = skip_dir_stack.last().unwrap().clone();
        if cur_file
            .ancestors()
            .any(|ancestor| ancestor == dir_to_skip.file)
        {
            skip_dir = true;
            break;
        } else {
            skip_dir_stack.pop();

            if let Some(db) = &database {
                db.pop_skip_dir_stack(dir_to_skip.id);
            }
        }
    }

    let mut cur_target = match replace_first_path {
        true => {
            let mut components = rel_file.components();
            components.next();

            dest.join(components.as_path())
        }
        false => dest.join(&rel_file),
    };

    while !rename_dir_stack.is_empty() {
        let rename_dir_entry = rename_dir_stack.last().unwrap().clone();
        if cur_target
            .ancestors()
            .any(|ancestor| ancestor == rename_dir_entry.existing_target)
        {
            cur_target = rename_dir_entry
                .cur_target
                // TODO: I don't like this unwrap() call
                .join(diff_paths(&cur_target, &rename_dir_entry.existing_target).unwrap());
            break;
        } else {
            rename_dir_stack.pop();

            if let Some(db) = &database {
                db.pop_rename_dir_stack(rename_dir_entry.id);
            }
        }
    }

    let mut actual_file = unarchive_parent_map(&cur_file, archive_dirs);
    let mut actual_target = unarchive_parent_map(&cur_target, archive_dirs);

    let mut target_is_dir = entry.target_is_dir;
    let mut target_is_symlink = entry.target_is_symlink;
    let mut resume = false;

    match &entry.status {
        DBFileStatus::InProgress => {
            if let Some(x) = &entry.cur_target {
                cur_target = x.clone();
                actual_target = unarchive_parent_map(&cur_target, archive_dirs);
            }

            if actual_target.exists() {
                resume = true;

                if entry.message.is_empty() {
                    entry.message = String::from("Resumed");
                } else if !entry.message.starts_with("Resumed") {
                    entry.message = format!("Resumed -- {}", entry.message);
                }
            }
        }
        _ => {
            if actual_target.exists() && !skip_dir {
                target_is_dir = actual_target.is_dir();
                target_is_symlink = actual_target.is_symlink();

                if !(entry.is_dir && target_is_dir) {
                    if same_file(&actual_file, &actual_target).context("samefile")? {
                        if matches!(mode, DlgCpMvType::Mv)
                            || !(matches!(on_conflict, OnConflict::RenameExisting)
                                || matches!(on_conflict, OnConflict::RenameCopy))
                        {
                            entry.message = String::from("Same file");
                            return Ok((DBFileStatus::Skipped, DBJobStatus::InProgress));
                        }
                    }

                    match on_conflict {
                        OnConflict::Overwrite => {
                            if target_is_dir && !target_is_symlink {
                                fs::remove_dir(&actual_target).context("rmdir")?;
                            } else {
                                fs::remove_file(&actual_target).context("remove")?;
                            }

                            if let Some(_db) = &database {
                                // TODO: I don't like this unwrap() call
                                fsync_parent(
                                    &fs::canonicalize(actual_target.parent().unwrap())
                                        .context("fsync")?,
                                )
                                .context("fsync")?;
                            }

                            entry.message = String::from("Overwrite");
                        }
                        OnConflict::RenameExisting => {
                            let mut i = 0;
                            // TODO: I don't like this unwrap() call
                            let name = actual_target
                                .file_name()
                                .unwrap()
                                .to_string_lossy()
                                .to_string();
                            let mut existing_target = actual_target.clone();
                            while existing_target.exists() {
                                let new_name = format!("{}.fcdsave{}", name, i);
                                // TODO: I don't like this unwrap() call
                                existing_target = existing_target.parent().unwrap().join(new_name);
                                i += 1;
                            }

                            if same_file(&actual_file, &actual_target).context("samefile")? {
                                actual_file = existing_target.clone();
                            }

                            fs::rename(&actual_target, &existing_target).context("rename")?;

                            if let Some(_db) = &database {
                                // TODO: I don't like this unwrap() call
                                fsync_parent(
                                    &fs::canonicalize(existing_target.parent().unwrap())
                                        .context("fsync")?,
                                )
                                .context("fsync")?;
                            }

                            // TODO: I don't like this unwrap() call
                            entry.message = format!(
                                "Renamed to {}",
                                existing_target.file_name().unwrap().to_string_lossy()
                            )
                        }
                        OnConflict::RenameCopy => {
                            let mut i = 0;
                            // TODO: I don't like this unwrap() call
                            let name = cur_target
                                .file_name()
                                .unwrap()
                                .to_string_lossy()
                                .to_string();
                            let existing_target = cur_target.clone();
                            while unarchive_path_map(&cur_target, archive_dirs).exists() {
                                let new_name = format!("{}.fcdnew{}", name, i);
                                // TODO: I don't like this unwrap() call
                                cur_target = cur_target.parent().unwrap().join(new_name);
                                i += 1;
                            }

                            actual_target = unarchive_path_map(&cur_target, archive_dirs);

                            // TODO: I don't like this unwrap() call
                            entry.message = format!(
                                "Renamed to {}",
                                cur_target.file_name().unwrap().to_string_lossy()
                            );
                            if entry.is_dir {
                                rename_dir_stack.push(DBRenameDirEntry {
                                    id: 0,
                                    job_id,
                                    existing_target: existing_target.clone(),
                                    cur_target: cur_target.clone(),
                                });

                                if let Some(db) = &database {
                                    db.push_rename_dir_stack(rename_dir_stack.last_mut().unwrap());
                                }
                            }
                        }
                        OnConflict::Skip => {
                            entry.message = String::from("Target exists");
                            return Ok((DBFileStatus::Skipped, DBJobStatus::InProgress));
                        }
                    }
                }
            }

            entry.status = DBFileStatus::InProgress;
            entry.target_is_dir = target_is_dir;
            entry.target_is_symlink = target_is_symlink;
            entry.cur_target = Some(cur_target.clone());
        }
    }

    if let Some(db) = &database {
        db.update_file(entry);
    }

    if !ev_rx.is_empty() {
        if let Ok(event) = ev_rx.try_recv() {
            match event {
                CpMvEvent::Suspend(suspend_rx) => {
                    let t1 = Instant::now();
                    let _ = suspend_rx.recv();
                    let t2 = Instant::now();
                    let dt = t2.duration_since(t1);
                    timers.cur_start += dt;
                    timers.start += dt;
                }
                CpMvEvent::Skip => {
                    let _ = fs::remove_file(&actual_target);
                    return Ok((DBFileStatus::Skipped, DBJobStatus::InProgress));
                }
                CpMvEvent::Abort => {
                    let _ = fs::remove_file(&actual_target);
                    return Ok((DBFileStatus::InProgress, DBJobStatus::Aborted));
                }
                CpMvEvent::NoDb => {
                    if let Some(db) = &database {
                        db.delete_job(job_id);
                    }

                    *database = None;
                }
            }
        }
    }

    info.cur_source = rel_file.clone();
    info.cur_target = cur_target.clone();
    info.cur_size = entry.size;
    info.cur_bytes = 0;

    if timers.last_write.elapsed().as_millis() >= 50 {
        timers.last_write = Instant::now();
        info.cur_time = timers.last_write.duration_since(timers.cur_start);
        info.total_time = timers.last_write.duration_since(timers.start);
        let _ = info_tx.send(info.clone());
        let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
    }

    if skip_dir {
        return Ok((DBFileStatus::Done, DBJobStatus::InProgress));
    }

    // TODO: I don't like this unwrap() call
    let parent_dir = fs::canonicalize(actual_target.parent().unwrap()).context("parent_dir")?;

    let mut perform_copy = true;
    if matches!(mode, DlgCpMvType::Mv) && !target_is_dir {
        perform_copy = false;
        match fs::rename(&actual_file, &actual_target) {
            Ok(_) => {
                if entry.is_dir {
                    skip_dir_stack.push(DBSkipDirEntry {
                        id: 0,
                        job_id,
                        file: cur_file.clone(),
                    });

                    if let Some(db) = &database {
                        fsync_parent(&parent_dir).context("fsync")?;

                        // TODO: I don't like this unwrap() call
                        let source_parent = fs::canonicalize(actual_file.parent().unwrap())
                            .context("parent_dir")?;

                        fsync_parent(&source_parent).context("fsync")?;

                        db.push_skip_dir_stack(skip_dir_stack.last_mut().unwrap());
                    }
                }
            }
            Err(_e) => {
                perform_copy = true;
            }
        }
    }

    if perform_copy {
        if entry.is_symlink {
            fs::read_link(&actual_file)
                .and_then(|link_target| symlink(&link_target, &actual_target))
                .context("symlink")?;
        } else if entry.is_dir {
            let mut new_dir = false;
            if !target_is_dir {
                fs::create_dir_all(&actual_target).context("makedirs")?;
                new_dir = true;
            }

            dir_list.push(DBDirListEntry {
                id: 0,
                job_id,
                file: entry.clone(),
                cur_file: cur_file.clone(),
                cur_target: cur_target.clone(),
                new_dir,
                status: DBFileStatus::ToDo,
                message: String::from(""),
            });

            if let Some(db) = &database {
                db.push_dir_list(dir_list.last_mut().unwrap());
            }
        } else if entry.is_file {
            match copy_file(
                job_id,
                &actual_file,
                &actual_target,
                entry.size,
                block_size,
                resume,
                ev_rx,
                info_tx,
                pubsub_tx,
                info,
                timers,
                database,
            ) {
                Ok((DBFileStatus::Skipped, _)) => {
                    let _ = fs::remove_file(&actual_target);
                    return Ok((DBFileStatus::Skipped, DBJobStatus::InProgress));
                }
                Ok((_, DBJobStatus::Aborted)) => {
                    let _ = fs::remove_file(&actual_target);
                    return Ok((DBFileStatus::InProgress, DBJobStatus::Aborted));
                }
                Ok(_) => {}
                Err(e) => return Err(e),
            }
        } else {
            entry.message = String::from("Special file");
            return Ok((DBFileStatus::Error, DBJobStatus::InProgress));
        }

        if !entry.is_dir {
            if let Err(e) = lchown(&actual_target, Some(entry.uid), Some(entry.gid)) {
                match e.kind() {
                    ErrorKind::PermissionDenied => {
                        if let Err(e) = lchown(&actual_target, None, Some(entry.gid)) {
                            match e.kind() {
                                ErrorKind::PermissionDenied | ErrorKind::Unsupported => {}
                                _ => return Err(e).context("lchown"),
                            }
                        }
                    }
                    ErrorKind::Unsupported => {}
                    _ => return Err(e).context("lchown"),
                }
            }

            shutil::copystat(&actual_file, &actual_target).context("copystat")?;
        }

        if let Some(_db) = &database {
            fsync_parent(&parent_dir).context("fsync")?;
        }
    }

    if matches!(mode, DlgCpMvType::Mv) && perform_copy && !entry.is_dir {
        fs::remove_file(&actual_file).context("remove")?;

        if let Some(_db) = &database {
            // TODO: I don't like this unwrap() call
            let source_parent =
                fs::canonicalize(actual_file.parent().unwrap()).context("parent_dir")?;

            fsync_parent(&source_parent).context("fsync")?;
        }
    }

    Ok((DBFileStatus::Done, DBJobStatus::InProgress))
}

fn handle_dir_entry(
    job_id: i64,
    mode: DlgCpMvType,
    entry: &DBDirListEntry,
    cwd: &Path,
    ev_rx: &Receiver<CpMvEvent>,
    info_tx: &Sender<CpMvInfo>,
    pubsub_tx: &Sender<PubSub>,
    info: &mut CpMvInfo,
    timers: &mut Timers,
    database: &mut Option<DataBase>,
    archive_dirs: &[ArchiveEntry],
) -> Result<(DBFileStatus, DBJobStatus)> {
    timers.cur_start = Instant::now();

    if !ev_rx.is_empty() {
        if let Ok(event) = ev_rx.try_recv() {
            match event {
                CpMvEvent::Suspend(suspend_rx) => {
                    let t1 = Instant::now();
                    let _ = suspend_rx.recv();
                    let t2 = Instant::now();
                    let dt = t2.duration_since(t1);
                    timers.cur_start += dt;
                    timers.start += dt;
                }
                CpMvEvent::Skip => {
                    return Ok((DBFileStatus::Skipped, DBJobStatus::InProgress));
                }
                CpMvEvent::Abort => {
                    return Ok((DBFileStatus::InProgress, DBJobStatus::Aborted));
                }
                CpMvEvent::NoDb => {
                    if let Some(db) = &database {
                        db.delete_job(job_id);
                    }

                    *database = None;
                }
            }
        }
    }

    // TODO: I don't like this unwrap() call
    let rel_file = diff_paths(&entry.cur_file, cwd).unwrap();

    info.cur_source = rel_file.clone();
    info.cur_target = entry.cur_target.clone();
    info.cur_size = entry.file.size;
    info.cur_bytes = 0;

    let actual_file = unarchive_parent_map(&entry.cur_file, archive_dirs);
    let actual_target = unarchive_parent_map(&entry.cur_target, archive_dirs);

    if timers.last_write.elapsed().as_millis() >= 50 {
        timers.last_write = Instant::now();
        info.cur_time = timers.last_write.duration_since(timers.cur_start);
        info.total_time = timers.last_write.duration_since(timers.start);
        let _ = info_tx.send(info.clone());
        let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
    }

    if entry.new_dir {
        if let Err(e) = lchown(&actual_target, Some(entry.file.uid), Some(entry.file.gid)) {
            match e.kind() {
                ErrorKind::PermissionDenied => {
                    if let Err(e) = lchown(&actual_target, None, Some(entry.file.gid)) {
                        match e.kind() {
                            ErrorKind::PermissionDenied | ErrorKind::Unsupported => {}
                            _ => return Err(e).context("lchown"),
                        }
                    }
                }
                ErrorKind::Unsupported => {}
                _ => return Err(e).context("lchown"),
            }
        }

        shutil::copystat(&actual_file, &actual_target).context("copystat")?;

        if let Some(_db) = &database {
            // TODO: I don't like this unwrap() call
            let parent_dir =
                fs::canonicalize(actual_target.parent().unwrap()).context("parent_dir")?;

            fsync_parent(&parent_dir).context("fsync")?;
        }
    }

    if let DlgCpMvType::Mv = mode {
        fs::remove_dir(&actual_file).context("rmdir")?;

        if let Some(_db) = &database {
            // TODO: I don't like this unwrap() call
            let source_parent =
                fs::canonicalize(actual_file.parent().unwrap()).context("parent_dir")?;

            fsync_parent(&source_parent).context("fsync")?;
        }
    }

    Ok((DBFileStatus::Done, DBJobStatus::InProgress))
}

fn copy_file(
    job_id: i64,
    actual_file: &Path,
    actual_target: &Path,
    file_size: u64,
    block_size: u64,
    resume: bool,
    ev_rx: &Receiver<CpMvEvent>,
    info_tx: &Sender<CpMvInfo>,
    pubsub_tx: &Sender<PubSub>,
    info: &mut CpMvInfo,
    timers: &mut Timers,
    database: &mut Option<DataBase>,
) -> Result<(DBFileStatus, DBJobStatus)> {
    let source_fd = open(actual_file, OFlags::RDONLY, Mode::RUSR).context("source_fd")?;

    let target_fd = match resume {
        true => match open(actual_target, OFlags::WRONLY, Mode::WUSR) {
            Ok(fd) => fd,
            Err(Errno::OPNOTSUPP) => {
                open(actual_target, OFlags::TRUNC | OFlags::WRONLY, Mode::WUSR)
                    .context("target_fd")?
            }
            Err(e) => return Err(e).context("target_fd"),
        },
        false => {
            let fd = open(
                actual_target,
                OFlags::CREATE | OFlags::EXCL | OFlags::TRUNC | OFlags::WRONLY,
                Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP | Mode::ROTH | Mode::WOTH,
            )
            .context("target_fd")?;

            let _ = fallocate(&fd, FallocateFlags::KEEP_SIZE, 0, file_size);

            if let Some(_db) = &database {
                fsync(&fd).context("fsync")?;

                // TODO: I don't like this unwrap() call
                let parent_dir =
                    fs::canonicalize(actual_target.parent().unwrap()).context("parent_dir")?;

                fsync_parent(&parent_dir).context("fsync")?;
            }

            fd
        }
    };

    let mut bytes_written = match resume {
        true => {
            let size = fstat(&target_fd)?.st_size as u64;
            let pos = (size / block_size).saturating_sub(1) * block_size;

            if pos != 0 {
                seek(&source_fd, SeekFrom::Start(pos)).context("lseek")?;
                seek(&target_fd, SeekFrom::Start(pos)).context("lseek")?;
                info.cur_bytes += pos;
                info.total_bytes += pos;
            }

            pos
        }
        false => 0,
    };

    let mut copy_method = CopyMethod::CopyFileRange;

    let mut buf = vec![0; block_size as usize];

    loop {
        if !ev_rx.is_empty() {
            if let Ok(event) = ev_rx.try_recv() {
                match event {
                    CpMvEvent::Suspend(suspend_rx) => {
                        let t1 = Instant::now();
                        let _ = suspend_rx.recv();
                        let t2 = Instant::now();
                        let dt = t2.duration_since(t1);
                        timers.cur_start += dt;
                        timers.start += dt;
                    }
                    CpMvEvent::Skip => {
                        return Ok((DBFileStatus::Skipped, DBJobStatus::InProgress));
                    }
                    CpMvEvent::Abort => {
                        return Ok((DBFileStatus::InProgress, DBJobStatus::Aborted));
                    }
                    CpMvEvent::NoDb => {
                        if let Some(db) = &database {
                            db.delete_job(job_id);
                        }

                        *database = None;
                    }
                }
            }
        }

        let (bytes_copied, done) = match copy_method {
            CopyMethod::CopyFileRange => {
                match copy_file_range(&source_fd, None, &target_fd, None, block_size as usize) {
                    Ok(bytes_copied) => (bytes_copied, bytes_copied == 0),
                    Err(_) => {
                        copy_method = CopyMethod::Sendfile;

                        (0, false)
                    }
                }
            }
            CopyMethod::Sendfile => {
                match sendfile(&target_fd, &source_fd, None, block_size as usize) {
                    Ok(bytes_copied) => (bytes_copied, bytes_copied == 0),
                    Err(_) => {
                        copy_method = CopyMethod::ReadWrite;

                        (0, false)
                    }
                }
            }
            CopyMethod::ReadWrite => {
                let mut bytes_copied = 0;

                buf.resize(block_size as usize, 0);
                let bytes_read = read(&source_fd, &mut buf).context("read")?;
                if bytes_read != 0 {
                    buf.resize(bytes_read, 0);

                    while bytes_copied < bytes_read {
                        bytes_copied += write(&target_fd, &buf[bytes_copied..]).context("write")?;
                    }
                }

                (bytes_copied, bytes_copied == 0)
            }
        };

        if done {
            break;
        }

        if let Some(_db) = &database {
            fsync(&target_fd).context("fsync")?
        }

        bytes_written += bytes_copied as u64;

        info.cur_bytes = bytes_written;
        info.total_bytes += bytes_copied as u64;

        if timers.last_write.elapsed().as_millis() >= 50 {
            timers.last_write = Instant::now();
            info.cur_time = timers.last_write.duration_since(timers.cur_start);
            info.total_time = timers.last_write.duration_since(timers.start);
            let _ = info_tx.send(info.clone());
            let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
        }
    }

    Ok((DBFileStatus::Done, DBJobStatus::InProgress))
}

fn same_file(file1: &Path, file2: &Path) -> Result<bool> {
    // TODO: Instead of canonicalizing the path it would be more reliable to check the device number and inode number
    match (fs::canonicalize(file1), fs::canonicalize(file2)) {
        (Ok(file), Ok(target)) if file == target => Ok(true),
        (e @ Err(_), _) | (_, e @ Err(_)) => Ok(e.map(|_| false)?),
        _ => Ok(false),
    }
}

fn fsync_parent(parent_dir: &Path) -> rustix::io::Result<()> {
    let parent_fd = open(parent_dir, OFlags::RDONLY | OFlags::DIRECTORY, Mode::RUSR)?;

    fsync(parent_fd)
}
