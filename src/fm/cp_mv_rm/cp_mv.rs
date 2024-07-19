
#[derive(Debug, Clone, Copy)]
enum CpMvEntryResult {
    Done,
    Skipped(String),
}

#[derive(Debug, Clone, Copy)]
pub enum CpMvEvent {
    Suspend(Receiver<()>),
    Skip,
    Abort,
    NoDb,
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
) {
    let file_list = Vec::from(entries);
    file_list.sort_unstable_by_key(|entry| entry.file);

    let unarchive_path = |file| {
        match &archive_mounter_command_tx {
            Some(command_tx) => archive_mounter::unarchive_path(command_tx, file),
            None => PathBuf::from(file),
        }
    };

    let actual_dest = unarchive_path(dest);

    // TODO: Find the right block size
    let default_block_size: i64 = 8 * 1024 * 1024;
    let block_size = match fs::metadata(actual_dest) {
        Ok(metadata) => {
            let fs_block_size = metadata.blksize() as i64;

            (default_block_size + (fs_block_size - default_block_size).rem_euclid(fs_block_size)) as u64
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

    let replace_first_path = db_command_tx.and_then(|command_tx| database::get_replace_first_path(command_tx, job_id));

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
            },
            _ => {},
        }

        match cp_mv_entry(&mut entry, block_size, &mut info, &mut dir_list, &mut rename_dir_stack, &mut skip_dir_stack, replace_first_path, &mut timers) {
            Ok(status) => {
                entry.status = status;

                if let Some(command_tx) = &db_command_tx {
                    database::set_file_status(command_tx, &entry);
                }

                if matches!(status, DBFileStatus::Aborted) {
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

    for entry in reversed(dir_list):
        try:
            if ev_interrupt.is_set():
                raise InterruptError()

            if dbfile and ev_nodb.is_set():
                db.delete_job(job_id)
                del db
                dbfile = None

            timers['cur_start'] = time.monotonic()

            t1 = time.monotonic()
            ev_suspend.wait()
            t2 = time.monotonic()
            dt = round(t2 - t1)
            timers['cur_start'] += dt
            timers['start'] += dt

            if ev_abort.is_set():
                raise AbortedError()

            if ev_skip.is_set():
                ev_skip.clear()
                raise SkippedError('ev_skip')

            file = entry['file']
            cur_target = entry['cur_target']
            cur_file = entry['cur_file']
            rel_file = cur_file.relative_to(cwd)
            info['cur_source'] = str(rel_file)
            info['cur_target'] = str(cur_target)
            info['cur_size'] = file['lstat'].st_size
            info['cur_bytes'] = 0

            actual_file = unarchive_path(cur_file, include_self=False)[0]
            actual_target = unarchive_path(cur_target, include_self=False)[0]

            now = time.monotonic()
            if (now - timers['last_write']) > 0.05:
                timers['last_write'] = now
                info['cur_time'] = int(round(now - timers['cur_start']))
                info['time'] = int(round(now - timers['start']))
                q.put(info.copy())
                try:
                    os.write(fd, b'\n')
                except OSError:
                    pass

            when = ''
            try:
                parent_dir = actual_target.resolve().parent

                if entry['new_dir']:
                    when = 'lchown'
                    try:
                        os.lchown(actual_target, file['lstat'].st_uid, file['lstat'].st_gid)
                    except OSError as e:
                        if e.errno == errno.EPERM:
                            try:
                                os.lchown(actual_target, -1, file['lstat'].st_gid)
                            except OSError as e:
                                if e.errno in (errno.EPERM, errno.ENOSYS, errno.ENOTSUP):
                                    pass
                                else:
                                    raise
                        elif e.errno in (errno.ENOSYS, errno.ENOTSUP):
                            pass
                        else:
                            raise

                    when = 'copystat'
                    try:
                        shutil.copystat(actual_file, actual_target, follow_symlinks=False)
                    except OSError as e:
                        if e.errno in (errno.ENOSYS, errno.ENOTSUP):
                            pass
                        else:
                            raise

                when = 'fsync'
                parent_fd = os.open(parent_dir, 0)
                try:
                    if dbfile:
                        os.fsync(parent_fd)
                finally:
                    os.close(parent_fd)

                if mode == 'mv':
                    when = 'rmdir'
                    os.rmdir(actual_file)

                    when = 'fsync'
                    parent_fd = os.open(actual_file.parent, 0)
                    try:
                        if dbfile:
                            os.fsync(parent_fd)
                    finally:
                        os.close(parent_fd)
            except OSError as e:
                message = f'({when}) {e.strerror} ({e.errno})'
                error_list.append({'file': file['file'], 'message': message})
                if dbfile:
                    db.set_file_status(file, 'ERROR', message)

            if dbfile:
                db.set_job_status(job_id, 'DONE')
        except InterruptError as e:
            break
        except AbortedError as e:
            if dbfile:
                db.set_job_status(job_id, 'ABORTED')

            break
        except SkippedError as e:
            if str(e) == 'no_log':
                if file['status'] == 'ERROR':
                    error_list.append({'file': file['file'], 'message': file['message']})
                elif file['status'] == 'SKIPPED':
                    skipped_list.append({'file': file['file'], 'message': file['message']})
                else:
                    completed_list.append({'file': file['file'], 'message': ''})
                    if dbfile and file['status'] != 'DONE':
                        db.set_file_status(file, 'DONE', '')
            else:
                message = str(e)
                if message == 'ev_skip':
                    message = ''

                skipped_list.append({'file': file['file'], 'message': message})
                if dbfile:
                    db.set_file_status(file, 'SKIPPED', message)

    if not dbfile:
        os.sync()

    q.put({'result': completed_list, 'error': error_list, 'skipped': skipped_list, 'aborted': aborted_list})
    try:
        os.write(fd, b'\n')
    except OSError:
        pass
    os.close(fd)
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
    let unarchive_path = |file| {
        match &archive_mounter_command_tx {
            Some(command_tx) => archive_mounter::unarchive_path(command_tx, file),
            None => PathBuf::from(file),
        }
    };

    timers.cur_start = Instant::now();

    let cur_file = PathBuf::from(entry.file);
    let rel_file = diff_paths(cur_file, cwd);

    let mut skip_dir = false;
    while !skip_dir_stack.is_empty() {
        let dir_to_skip = skip_dir_stack.last().unwrap();
        if cur_file.ancestors().any(|ancestor| ancestor == dir_to_skip.file) {
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
        },
        false => dest.join(rel_file),
    };

    while !rename_dir_stack.is_empty() {
        let rename_dir_entry = rename_dir_stack.last().unwrap();
        if cur_target.ancestors().any(|ancestor| ancestor == rename_dir_entry.existing_target) {
            cur_target = rename_dir_entry.cur_target.join(diff_paths(cur_target, rename_dir_entry.existing_target));
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
                        if matches!(mode, DlgCpMvType::Mv) || !(matches!(on_conflict, OnConflict::RenameExisting) || matches!(on_conflict, OnConflict::RenameCopy)) {
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
                                fsync_parent(fs::canonicalize(&actual_target.parent().unwrap()).context("fsync")?).context("fsync")?;
                            }

                            entry.message = String::from("Overwrite");
                        }
                        OnConflict::RenameExisting => {
                            let mut i = 0;
                            // TODO: I don't like this unwrap() call
                            let mut name = actual_target.file_name().unwrap().to_string_lossy().to_string();
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
                                fsync_parent(fs::canonicalize(&existing_target.parent().unwrap()).context("fsync")?).context("fsync")?;
                            }

                            // TODO: I don't like this unwrap() call
                            entry.message = format!("Renamed to {}", existing_target.file_name().unwrap().to_string_lossy())
                        }
                        OnConflict::RenameCopy => {
                            let mut i = 0;
                            // TODO: I don't like this unwrap() call
                            let mut name = cur_target.file_name().unwrap().to_string_lossy().to_string();
                            let mut existing_target = cur_target;
                            while unarchive_path(cur_target).try_exists().is_ok() {
                                let new_name = format!("{}.fcdnew{}" name, i);
                                // TODO: I don't like this unwrap() call
                                cur_target = cur_target.parent().unwrap().join(new_name);
                                i += 1;
                            }

                            actual_target = unarchive_path(cur_target);

                            // TODO: I don't like this unwrap() call
                            entry.message = format!("Renamed to {}", cur_target.file_name().unwrap().to_string_lossy());
                            if entry.is_dir {
                                rename_dir_stack.push(DBRenameDirEntry {
                                    id: 0,
                                    job_id,
                                    existing_target,
                                    cur_target,
                                });

                                if let Some(command_tx) = &db_command_tx {
                                    database::push_rename_dir_stack(command_tx, rename_dir_stack.last_mut().unwrap());
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
                },
                CpMvEvent::Abort => {
                    let _ = fs::remove_file(actual_target);
                    return Ok(DBFileStatus::Aborted);
                },
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

                        database::push_skip_dir_stack(command_tx, skip_dir_stack.last_mut().unwrap());
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
            fs::read_link(actual_file).and_then(|link_target| {
                symlink(link_target, actual_target)
            }).context("symlink")?;
        } else if entry.is_dir {
            let mut new_dir = false;
            if !target_is_dir {
                fs::create_dir_all(actual_target).context("makedirs")?;
                new_dir = true;
            }

            dir_list.push(DBDirListEntry {
                id: 0,
                job_id;
                // TODO: We should use the whole entry instead, and handle the id relationship at the DB level
                file_id: entry.id,
                cur_file,
                cur_target,
                new_dir,
            });

            if let Some(command_tx) = &db_command_tx {
                database::push_dir_list(command_tx, dir_list.last_mut().unwrap());
            }
        } else if entry.is_file {
            when = "copyfile";
            if let Err(e) = copyfile(actual_file, actual_target, file['lstat'].st_size, block_size, resume, info, timers, fd, q, ev_skip, ev_suspend, ev_interrupt, ev_abort, dbfile) {
                // TODO: OSError handler from Python
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
                                _ => return Err(e).context(lchown),
                            }
                        }
                    }
                    ErrorKind::Unsupported => {}
                    _ => return Err(e).context(lchown),
                }
            }

            shutil::copystat(actual_file, actual_target).context("copystat")?;
        }
    }

    if matches!(mode, DlgCpMvType::Mv) && !entry.is_dir {
        if perform_copy {
            fs::remove_file(actual_file).context("remove")?;
        }
    }

    // TODO: Maybe we should fsync the directory also right after opening the file for writing
    if let Some(command_tx) = &db_command_tx {
        fsync_parent(parent_dir).context("fsync")?;
    }

    Ok(DBFileStatus::Done)
}

fn copy_file(cur_file, cur_target, file_size, block_size, resume, info, timers, fd, q, ev_skip, ev_suspend, ev_interrupt, ev_abort, dbfile) {
    with open(cur_file, 'rb') as fh:
        if resume:
            try:
                target_fd = os.open(cur_target, os.O_WRONLY | (os.O_DSYNC if dbfile else 0), stat.S_IRUSR | stat.S_IWUSR | stat.S_IRGRP | stat.S_IWGRP | stat.S_IROTH | stat.S_IWOTH)
            except OSError as e:
                if e.errno == errno.EOPNOTSUPP:
                    target_fd = os.open(cur_target, os.O_TRUNC | os.O_WRONLY | (os.O_DSYNC if dbfile else 0), stat.S_IRUSR | stat.S_IWUSR | stat.S_IRGRP | stat.S_IWGRP | stat.S_IROTH | stat.S_IWOTH)
                else:
                    raise
        else:
            target_fd = os.open(cur_target, os.O_CREAT | os.O_EXCL | os.O_TRUNC | os.O_WRONLY | (os.O_DSYNC if dbfile else 0), stat.S_IRUSR | stat.S_IWUSR | stat.S_IRGRP | stat.S_IWGRP | stat.S_IROTH | stat.S_IWOTH)

        try:
            if resume:
                bytes_written = os.fstat(target_fd).st_size
                pos = max((int(bytes_written / block_size) - 1) * block_size, 0)
                os.lseek(target_fd, pos, os.SEEK_SET)
                fh.seek(pos)
                info['cur_bytes'] += pos
                info['bytes'] += pos
            else:
                try:
                    fallocate(target_fd, FALLOC_FL_KEEP_SIZE, 0, file_size)
                except OSError:
                    pass

            while True:
                if ev_interrupt.is_set():
                    raise InterruptError()

                t1 = time.monotonic()
                ev_suspend.wait()
                t2 = time.monotonic()
                dt = round(t2 - t1)
                timers['cur_start'] += dt
                timers['start'] += dt

                if ev_abort.is_set():
                    raise AbortedError()

                if ev_skip.is_set():
                    ev_skip.clear()
                    raise SkippedError('ev_skip')

                buf = fh.read(block_size)
                if not buf:
                    break

                buffer_length = len(buf)
                bytes_written = 0
                while bytes_written < buffer_length:
                    bytes_written += os.write(target_fd, buf[bytes_written:])

                info['cur_bytes'] += bytes_written
                info['bytes'] += bytes_written
                now = time.monotonic()
                if (now - timers['last_write']) > 0.05:
                    timers['last_write'] = now
                    info['cur_time'] = int(round(now - timers['cur_start']))
                    info['time'] = int(round(now - timers['start']))
                    q.put(info.copy())
                    try:
                        os.write(fd, b'\n')
                    except OSError:
                        pass
        finally:
            os.close(target_fd)
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
