#[derive(Debug, Clone, Copy)]
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
    pub files: usize,
    pub bytes: u64,
    pub time: Duration,
}

#[derive(Debug, Clone)]
struct Timers {
    pub start: Instant,
    pub last_write: Instant,
    pub cur_start: Instant,
}

pub fn cp_mv(
    mode: DlgCpMvType,
    entries: &[DBFileEntry],
    cwd: &Path,
    dest: &Path,
    on_conflict: OnConflict,
    ev_rx: Receiver<CpMvEvent>,
    info_tx: Sender<CpMvInfo>,
    pubsub_tx: Sender<PubSub>,
    db_command_tx: Option<Sender<DBCommand>>,
    job_id: i64,
    archive_mounter_command_tx: Option<Sender<ArchiveMounterCommand>>,
) -> (Vec<DBFileEntry>, Vec<DBDirListEntry>) {
    let file_list = Vec::from(entries);
    file_list.sort_unstable_by_key(|entry| entry.file);

    let unarchive_path = |file| match &archive_mounter_command_tx {
        Some(command_tx) => archive_mounter::unarchive_path(command_tx, file),
        None => PathBuf::from(file),
    };

    let actual_dest = unarchive_path(dest);

    // TODO: Find the right block size
    let default_block_size: i64 = 8 * 1024 * 1024;
    let block_size = match fs::metadata(actual_dest) {
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
        files: 0,
        bytes: 0,
        time: Duration::ZERO,
    };

    let dir_list = match &db_command_tx {
        Some(command_tx) => database::get_dir_list(command_tx, job_id),
        None => Vec::new(),
    };

    let rename_dir_stack = match &db_command_tx {
        Some(command_tx) => database::get_rename_dir_stack(command_tx, job_id),
        None => Vec::new(),
    };

    let skip_dir_stack = match &db_command_tx {
        Some(command_tx) => database::get_skip_dir_stack(command_tx, job_id),
        None => Vec::new(),
    };

    let replace_first_path =
        db_command_tx.and_then(|command_tx| database::get_replace_first_path(command_tx, job_id));

    let replace_first_path = replace_first_path.unwrap_or_else(|| {
        let replace_first_path = actual_dest.is_dir();

        if let Some(command_tx) = &db_command_tx {
            database::set_replace_first_path(command_tx, job_id, replace_first_path);
        }

        replace_first_path
    });

    let now = Instant::now();
    let mut timers = Timers {
        start: now.clone(),
        last_write: now.clone(),
        cur_start: now,
    };

    let mut total_bytes = 0;

    for entry in &mut file_list {
        match entry.status {
            DBFileStatus::Error | DBFileStatus::Skipped | DBFileStatus::Done => {
                // Alternatively we can remove the size of this entry from the total size,
                // which could result in a more accurate timing
                total_bytes += entry.size;
                info.bytes = total_bytes;
                info.files += 1;

                continue;
            }
            _ => {}
        }

        match cp_mv_entry(
            &mut entry,
            block_size,
            &mut info,
            &mut dir_list,
            &mut rename_dir_stack,
            &mut skip_dir_stack,
            replace_first_path,
            &mut timers,
        ) {
            Ok(status) => {
                entry.status = status;

                if let Some(command_tx) = &db_command_tx {
                    database::set_file_status(command_tx, &entry);
                }

                if let DBFileStatus::Aborted = status {
                    if let Some(command_tx) = &db_command_tx {
                        database::set_job_status(command_tx, job_id, DBJobStatus::Aborted);
                    }

                    break;
                }
            }
            Err(e) => {
                // TODO -- entry.message = f'({when}) {e.strerror} ({e.errno})'
                entry.status = DBFileStatus::Error;

                if let Some(command_tx) = &db_command_tx {
                    database::set_file_status(command_tx, &entry);
                }
            }
        }

        total_bytes += entry.size;
        info.bytes = total_bytes;
        info.files += 1;
    }

    for entry in dir_list.iter_mut().rev() {
        match entry.status {
            DBFileStatus::Error | DBFileStatus::Skipped | DBFileStatus::Done => {
                continue;
            }
            _ => {}
        }

        match handle_dir_entry(&entry, &mut info, &mut timers) {
            Ok(status) => {
                entry.status = status;

                if let Some(command_tx) = &db_command_tx {
                    database::set_dir_list_entry_status(command_tx, &entry);
                }

                if let DBFileStatus::Aborted = status {
                    if let Some(command_tx) = &db_command_tx {
                        database::set_job_status(command_tx, job_id, DBJobStatus::Aborted);
                    }

                    break;
                }
            }
            Err(e) => {
                // TODO -- entry.message = f'({when}) {e.strerror} ({e.errno})'
                entry.status = DBFileStatus::Error;

                if let Some(command_tx) = &db_command_tx {
                    database::set_dir_list_entry_status(command_tx, &entry);
                }
            }
        }
    }

    if let None = &db_command_tx {
        sync();
    }

    (file_list, dir_list)
}

fn cp_mv_entry(
    entry: &mut DBFileEntry,
    block_size: u64,
    info: &mut CpMvInfo,
    dir_list: &mut Vec<DBDirListEntry>,
    rename_dir_stack: &mut Vec<DBRenameDirEntry>,
    skip_dir_stack: &mut Vec<DBSkipDirEntry>,
    replace_first_path: bool,
    timers: &mut Timers,
) -> Result<DBFileStatus> {
    timers.cur_start = Instant::now();

    let cur_file = PathBuf::from(entry.file);
    let rel_file = diff_paths(cur_file, cwd);

    let mut skip_dir = false;
    while !skip_dir_stack.is_empty() {
        let dir_to_skip = skip_dir_stack.last().unwrap();
        if cur_file
            .ancestors()
            .any(|ancestor| ancestor == dir_to_skip.file)
        {
            skip_dir = true;
            break;
        } else {
            skip_dir_stack.pop();

            if let Some(command_tx) = &db_command_tx {
                database::pop_skip_dir_stack(command_tx, dir_to_skip.id);
            }
        }
    }

    let mut cur_target = match replace_first_path {
        true => {
            let components = rel_file.components();
            components.next();

            dest.join(components.as_path())
        }
        false => dest.join(rel_file),
    };

    while !rename_dir_stack.is_empty() {
        let rename_dir_entry = rename_dir_stack.last().unwrap();
        if cur_target
            .ancestors()
            .any(|ancestor| ancestor == rename_dir_entry.existing_target)
        {
            cur_target = rename_dir_entry
                .cur_target
                .join(diff_paths(cur_target, rename_dir_entry.existing_target));
            break;
        } else {
            rename_dir_stack.pop();

            if let Some(command_tx) = &db_command_tx {
                database::pop_rename_dir_stack(command_tx, rename_dir_entry.id);
            }
        }
    }

    let mut actual_file = match (cur_file.parent(), cur_file.file_name()) {
        (Some(parent), Some(file_name)) => unarchive_path(parent).join(file_name),
        _ => cur_file,
    };

    let mut actual_target = match (cur_target.parent(), cur_target.file_name()) {
        (Some(parent), Some(file_name)) => unarchive_path(parent).join(file_name),
        _ => cur_target,
    };

    let mut target_is_dir = entry.target_is_dir;
    let mut target_is_symlink = entry.target_is_symlink;
    let mut resume = false;

    match &entry.status {
        DBFileStatus::InProgress | DBFileStatus::Aborted => {
            if let Some(x) = &entry.cur_target {
                cur_target = x;

                actual_target = match (cur_target.parent(), cur_target.file_name()) {
                    (Some(parent), Some(file_name)) => unarchive_path(parent).join(file_name),
                    _ => cur_target,
                };
            }

            if actual_target.try_exists().is_ok() {
                resume = true;

                if entry.message.is_empty() {
                    entry.message = String::from("Resumed");
                } else if !entry.message.starts_with("Resumed") {
                    entry.message = format!("Resumed -- {}", entry.message);
                }
            }
        }
        _ => {
            if actual_target.try_exists().is_ok() && !skip_dir {
                target_is_dir = actual_target.is_dir();
                target_is_symlink = actual_target.is_symlink();

                if !(entry.is_dir && target_is_dir) {
                    if same_file(&actual_file, &actual_target).context("samefile")? {
                        if matches!(mode, DlgCpMvType::Mv)
                            || !(matches!(on_conflict, OnConflict::RenameExisting)
                                || matches!(on_conflict, OnConflict::RenameCopy))
                        {
                            entry.message = String::from("Same file");
                            return Ok(DBFileStatus::Skipped);
                        }
                    }

                    match on_conflict {
                        OnConflict::Overwrite => {
                            if target_is_dir && !target_is_symlink {
                                fs::remove_dir(actual_target).context("rmdir")?;
                            } else {
                                fs::remove_file(actual_target).context("remove")?;
                            }

                            if let Some(_command_tx) = &db_command_tx {
                                // TODO: I don't like this unwrap() call
                                fsync_parent(
                                    fs::canonicalize(&actual_target.parent().unwrap())
                                        .context("fsync")?,
                                )
                                .context("fsync")?;
                            }

                            entry.message = String::from("Overwrite");
                        }
                        OnConflict::RenameExisting => {
                            let mut i = 0;
                            // TODO: I don't like this unwrap() call
                            let mut name = actual_target
                                .file_name()
                                .unwrap()
                                .to_string_lossy()
                                .to_string();
                            let mut existing_target = actual_target;
                            while existing_target.try_exists().is_ok() {
                                let new_name = format!("{}.fcdsave{}", name, i);
                                // TODO: I don't like this unwrap() call
                                existing_target = existing_target.parent().unwrap().join(new_name);
                                i += 1;
                            }

                            if same_file(&actual_file, &actual_target).context("samefile")? {
                                actual_file = existing_target;
                            }

                            fs::rename(actual_target, existing_target).context("rename")?;

                            if let Some(_command_tx) = &db_command_tx {
                                // TODO: I don't like this unwrap() call
                                fsync_parent(
                                    fs::canonicalize(&existing_target.parent().unwrap())
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
                            let mut name = cur_target
                                .file_name()
                                .unwrap()
                                .to_string_lossy()
                                .to_string();
                            let mut existing_target = cur_target;
                            while unarchive_path(cur_target).try_exists().is_ok() {
                                let new_name = format!("{}.fcdnew{}", name, i);
                                // TODO: I don't like this unwrap() call
                                cur_target = cur_target.parent().unwrap().join(new_name);
                                i += 1;
                            }

                            actual_target = unarchive_path(cur_target);

                            // TODO: I don't like this unwrap() call
                            entry.message = format!(
                                "Renamed to {}",
                                cur_target.file_name().unwrap().to_string_lossy()
                            );
                            if entry.is_dir {
                                rename_dir_stack.push(DBRenameDirEntry {
                                    id: 0,
                                    job_id,
                                    existing_target,
                                    cur_target,
                                });

                                if let Some(command_tx) = &db_command_tx {
                                    database::push_rename_dir_stack(
                                        command_tx,
                                        rename_dir_stack.last_mut().unwrap(),
                                    );
                                }
                            }
                        }
                        OnConflict::Skip => {
                            entry.message = String::from("Target exists");
                            return Ok(DBFileStatus::Skipped);
                        }
                    }
                }
            }

            entry.status = DBFileStatus::InProgress;
            entry.target_is_dir = target_is_dir;
            entry.target_is_symlink = target_is_symlink;
            entry.cur_target = Some(cur_target);
        }
    }

    if let Some(command_tx) = &db_command_tx {
        database::update_file(command_tx, &entry);
    }

    if !ev_rx.is_empty() {
        if let Ok(event) = ev_rx.try_recv() {
            match event {
                CpMvEvent::Suspend(suspend_rx) => {
                    let t1 = Instant.now();
                    let _ = suspend_rx.recv();
                    let t2 = Instant.now();
                    let dt = t2.duration_since(t1);
                    timers.cur_start += dt;
                    timers.start += dt;
                }
                CpMvEvent::Skip => {
                    let _ = fs::remove_file(actual_target);
                    return Ok(DBFileStatus::Skipped);
                }
                CpMvEvent::Abort => {
                    let _ = fs::remove_file(actual_target);
                    return Ok(DBFileStatus::Aborted);
                }
                CpMvEvent::NoDb => {
                    if let Some(command_tx) = &db_command_tx {
                        database::delete_job(command_tx, job_id);
                    }

                    db_command_tx = None;
                }
            }
        }
    }

    info.cur_source = rel_file;
    info.cur_target = cur_target;
    info.cur_size = entry.size;
    info.cur_bytes = 0;

    if timers.last_write.elapsed().as_millis() >= 50 {
        timers.last_write = Instant::now();
        info.cur_time = timers.last_write.duration_since(timers.cur_start);
        info.time = timers.last_write.duration_since(timers.start);
        let _ = info_tx.send(info.clone());
        let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
    }

    if skip_dir {
        return Ok(DBFileStatus::Done);
    }

    // TODO: I don't like this unwrap() call
    let parent_dir = fs::canonicalize(&actual_target.parent().unwrap()).context("parent_dir")?;

    let mut perform_copy = true;
    if matches!(mode, DlgCpMvType::Mv) && !target_is_dir {
        perform_copy = false;
        match fs::rename(actual_file, actual_target) {
            Ok(_) => {
                if entry.is_dir {
                    skip_dir_stack.push(DBSkipDirEntry {
                        id: 0,
                        job_id,
                        file: cur_file,
                    });

                    if let Some(command_tx) = &db_command_tx {
                        fsync_parent(parent_dir).context("fsync")?;

                        // TODO: I don't like this unwrap() call
                        let source_parent = fs::canonicalize(&actual_file.parent().unwrap())
                            .context("parent_dir")?;

                        fsync_parent(source_parent).context("fsync")?;

                        database::push_skip_dir_stack(
                            command_tx,
                            skip_dir_stack.last_mut().unwrap(),
                        );
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
            fs::read_link(actual_file)
                .and_then(|link_target| symlink(link_target, actual_target))
                .context("symlink")?;
        } else if entry.is_dir {
            let mut new_dir = false;
            if !target_is_dir {
                fs::create_dir_all(actual_target).context("makedirs")?;
                new_dir = true;
            }

            dir_list.push(DBDirListEntry {
                id: 0,
                job_id,
                file: entry.clone(),
                cur_file,
                cur_target,
                new_dir,
                status: DBFileStatus::ToDo,
                message: String::from(""),
            });

            if let Some(command_tx) = &db_command_tx {
                database::push_dir_list(command_tx, dir_list.last_mut().unwrap());
            }
        } else if entry.is_file {
            match copy_file(
                actual_file,
                actual_target,
                entry.size,
                block_size,
                resume,
                info,
                timers,
            ) {
                Ok(DBFileStatus::Skipped) => {
                    let _ = fs::remove_file(actual_target);
                    return Ok(DBFileStatus::Skipped);
                }
                Ok(DBFileStatus::Aborted) => {
                    let _ = fs::remove_file(actual_target);
                    return Ok(DBFileStatus::Aborted);
                }
                Ok(_) => {}
                Err(e) => return Err(e),
            }
        } else {
            entry.message = String::from("Special file");
            return Ok(DBFileStatus::Error);
        }

        if !entry.is_dir {
            if let Err(e) = lchown(actual_target, Some(entry.uid), Some(entry.gid)) {
                match e.kind() {
                    ErrorKind::PermissionDenied => {
                        if let Err(e) = lchown(actual_target, None, Some(entry.gid)) {
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

            shutil::copystat(actual_file, actual_target).context("copystat")?;
        }

        if let Some(command_tx) = &db_command_tx {
            fsync_parent(parent_dir).context("fsync")?;
        }
    }

    if matches!(mode, DlgCpMvType::Mv) && perform_copy && !entry.is_dir {
        fs::remove_file(actual_file).context("remove")?;

        if let Some(command_tx) = &db_command_tx {
            // TODO: I don't like this unwrap() call
            let source_parent =
                fs::canonicalize(&actual_file.parent().unwrap()).context("parent_dir")?;

            fsync_parent(source_parent).context("fsync")?;
        }
    }

    Ok(DBFileStatus::Done)
}

fn handle_dir_entry(
    entry: &DBDirListEntry,
    info: &mut CpMvInfo,
    timers: &mut Timers,
) -> Result<DBFileStatus> {
    timers.cur_start = Instant::now();

    if !ev_rx.is_empty() {
        if let Ok(event) = ev_rx.try_recv() {
            match event {
                CpMvEvent::Suspend(suspend_rx) => {
                    let t1 = Instant.now();
                    let _ = suspend_rx.recv();
                    let t2 = Instant.now();
                    let dt = t2.duration_since(t1);
                    timers.cur_start += dt;
                    timers.start += dt;
                }
                CpMvEvent::Skip => {
                    return Ok(DBFileStatus::Skipped);
                }
                CpMvEvent::Abort => {
                    return Ok(DBFileStatus::Aborted);
                }
                CpMvEvent::NoDb => {
                    if let Some(command_tx) = &db_command_tx {
                        database::delete_job(command_tx, job_id);
                    }

                    db_command_tx = None;
                }
            }
        }
    }

    let rel_file = diff_paths(entry.cur_file, cwd);

    info.cur_source = rel_file;
    info.cur_target = entry.cur_target;
    info.cur_size = entry.file.size;
    info.cur_bytes = 0;

    let actual_file = match (entry.cur_file.parent(), entry.cur_file.file_name()) {
        (Some(parent), Some(file_name)) => unarchive_path(parent).join(file_name),
        _ => cur_file,
    };

    let actual_target = match (entry.cur_target.parent(), entry.cur_target.file_name()) {
        (Some(parent), Some(file_name)) => unarchive_path(parent).join(file_name),
        _ => cur_target,
    };

    if timers.last_write.elapsed().as_millis() >= 50 {
        timers.last_write = Instant::now();
        info.cur_time = timers.last_write.duration_since(timers.cur_start);
        info.time = timers.last_write.duration_since(timers.start);
        let _ = info_tx.send(info.clone());
        let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
    }

    if entry.new_dir {
        if let Err(e) = lchown(actual_target, Some(entry.file.uid), Some(entry.file.gid)) {
            match e.kind() {
                ErrorKind::PermissionDenied => {
                    if let Err(e) = lchown(actual_target, None, Some(entry.file.gid)) {
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

        shutil::copystat(actual_file, actual_target).context("copystat")?;

        if let Some(command_tx) = &db_command_tx {
            // TODO: I don't like this unwrap() call
            let parent_dir =
                fs::canonicalize(&actual_target.parent().unwrap()).context("parent_dir")?;

            fsync_parent(parent_dir).context("fsync")?;
        }
    }

    if let DlgCpMvType::Mv = mode {
        fs::remove_dir(actual_file).context("rmdir")?;

        if let Some(command_tx) = &db_command_tx {
            // TODO: I don't like this unwrap() call
            let source_parent =
                fs::canonicalize(&actual_file.parent().unwrap()).context("parent_dir")?;

            fsync_parent(source_parent).context("fsync")?;
        }
    }

    Ok(DBFileStatus::Done)
}

fn copy_file(
    actual_file: &Path,
    actual_target: &Path,
    file_size: u64,
    block_size: u64,
    resume: bool,
    info: &mut CpMvInfo,
    timers: &mut Timers,
) -> Result<DBFileStatus> {
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

            let _ = fallocate(fd, FallocateFlags::KEEP_SIZE, 0, file_size);

            if let Some(command_tx) = &db_command_tx {
                fsync(fd).context("fsync")?;

                // TODO: I don't like this unwrap() call
                let parent_dir =
                    fs::canonicalize(&actual_target.parent().unwrap()).context("parent_dir")?;

                fsync_parent(parent_dir).context("fsync")?;
            }

            fd
        }
    };

    let mut bytes_written = match resume {
        true => {
            let size = fstat(target_fd)?.st_size as u64;
            let pos = (size / block_size).saturating_sub(1) * block_size;

            if pos != 0 {
                seek(source_fd, SeekFrom::Start(pos)).context("lseek")?;
                seek(target_fd, SeekFrom::Start(pos)).context("lseek")?;
                info.cur_bytes += pos;
                info.bytes += pos;
            }

            pos
        }
        false => 0,
    };

    let mut copy_method = CopyMethod::CopyFileRange;

    let mut buf = vec![0; block_size];

    loop {
        if !ev_rx.is_empty() {
            if let Ok(event) = ev_rx.try_recv() {
                match event {
                    CpMvEvent::Suspend(suspend_rx) => {
                        let t1 = Instant.now();
                        let _ = suspend_rx.recv();
                        let t2 = Instant.now();
                        let dt = t2.duration_since(t1);
                        timers.cur_start += dt;
                        timers.start += dt;
                    }
                    CpMvEvent::Skip => {
                        return Ok(DBFileStatus::Skipped);
                    }
                    CpMvEvent::Abort => {
                        return Ok(DBFileStatus::Aborted);
                    }
                    CpMvEvent::NoDb => {
                        if let Some(command_tx) = &db_command_tx {
                            database::delete_job(command_tx, job_id);
                        }

                        db_command_tx = None;
                    }
                }
            }
        }

        let bytes_copied = match copy_method {
            CopyMethod::CopyFileRange => {
                match copy_file_range(source_fd, None, target_fd, None, block_size) {
                    Ok(bytes_copied) => bytes_copied,
                    Err(_) => {
                        copy_method = CopyMethod::Sendfile;

                        0
                    }
                }
            }
            CopyMethod::Sendfile => match sendfile(target_fd, source_fd, None, block_size) {
                Ok(bytes_copied) => bytes_copied,
                Err(_) => {
                    copy_method = CopyMethod::ReadWrite;

                    0
                }
            },
            CopyMethod::ReadWrite => {
                let mut bytes_copied = 0;

                buf.resize(block_size, 0);
                let bytes_read = read(source_fd, &mut buf).context("read")?;
                if bytes_read != 0 {
                    buf.resize(bytes_read, 0);

                    while bytes_copied < bytes_read {
                        bytes_copied += write(target_fd, &buf[bytes_copied..]).context("write")?;
                    }
                }

                bytes_copied
            }
        };

        if bytes_copied == 0 {
            break;
        }

        if let Some(command_tx) = &db_command_tx {
            fsync(target_fd).context("fsync")?
        }

        bytes_written += bytes_copied;

        info.cur_bytes += bytes_written;
        info.bytes += bytes_written;

        if timers.last_write.elapsed().as_millis() >= 50 {
            timers.last_write = Instant::now();
            info.cur_time = timers.last_write.duration_since(timers.cur_start);
            info.time = timers.last_write.duration_since(timers.start);
            let _ = info_tx.send(info.clone());
            let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
        }
    }

    Ok(DBFileStatus::Done)
}

fn same_file(file1: &Path, file2: &Path) -> Result<bool> {
    // TODO: Instead of canonicalizing the path it would be more reliable to check the device number and inode number
    match (fs::canonicalize(&file1), fs::canonicalize(&file2)) {
        (Ok(file), Ok(target)) if file == target => Ok(true),
        (e @ Err(_), _) | (_, e @ Err(_)) => Ok(e.map(|_| false)?),
        _ => Ok(false),
    }
}

fn fsync_parent(parent_dir: &Path) -> rustix::io::Result<()> {
    let parent_fd = open(parent_dir, OFlags::RDONLY | OFlags::DIRECTORY, Mode::RUSR)?;

    fsync(parent_fd)?
}
