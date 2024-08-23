use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Result};

use rusqlite::{
    self,
    types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef},
    Connection,
};

const DB_SIGNATURE: &str = "fcd";
const DB_VERSION: &str = "1";

#[derive(Debug, Clone, Copy)]
pub enum OnConflict {
    Overwrite,
    Skip,
    RenameExisting,
    RenameCopy,
}

impl FromSql for OnConflict {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value {
            ValueRef::Text(b"OVERWRITE") => Ok(OnConflict::Overwrite),
            ValueRef::Text(b"SKIP") => Ok(OnConflict::Skip),
            ValueRef::Text(b"RENAME_EXISTING") => Ok(OnConflict::RenameExisting),
            ValueRef::Text(b"RENAME_COPY") => Ok(OnConflict::RenameCopy),
            _ => Err(FromSqlError::InvalidType),
        }
    }
}

impl ToSql for OnConflict {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(ValueRef::Text(match &self {
            OnConflict::Overwrite => b"OVERWRITE",
            OnConflict::Skip => b"SKIP",
            OnConflict::RenameExisting => b"RENAME_EXISTING",
            OnConflict::RenameCopy => b"RENAME_COPY",
        })))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DBJobOperation {
    Cp,
    Mv,
    Rm,
}

impl FromSql for DBJobOperation {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value {
            ValueRef::Text(b"CP") => Ok(DBJobOperation::Cp),
            ValueRef::Text(b"MV") => Ok(DBJobOperation::Mv),
            ValueRef::Text(b"RM") => Ok(DBJobOperation::Rm),
            _ => Err(FromSqlError::InvalidType),
        }
    }
}

impl ToSql for DBJobOperation {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(ValueRef::Text(match &self {
            DBJobOperation::Cp => b"CP",
            DBJobOperation::Mv => b"MV",
            DBJobOperation::Rm => b"RM",
        })))
    }
}

impl fmt::Display for DBJobOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            DBJobOperation::Cp => write!(f, "Copy"),
            DBJobOperation::Mv => write!(f, "Move"),
            DBJobOperation::Rm => write!(f, "Delete"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DBJobStatus {
    Dirscan,
    InProgress,
    Aborted,
    Done,
}

impl FromSql for DBJobStatus {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value {
            ValueRef::Text(b"DIRSCAN") => Ok(DBJobStatus::Dirscan),
            ValueRef::Text(b"IN_PROGRESS") => Ok(DBJobStatus::InProgress),
            ValueRef::Text(b"ABORTED") => Ok(DBJobStatus::Aborted),
            ValueRef::Text(b"DONE") => Ok(DBJobStatus::Done),
            _ => Err(FromSqlError::InvalidType),
        }
    }
}

impl ToSql for DBJobStatus {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(ValueRef::Text(match &self {
            DBJobStatus::Dirscan => b"DIRSCAN",
            DBJobStatus::InProgress => b"IN_PROGRESS",
            DBJobStatus::Aborted => b"ABORTED",
            DBJobStatus::Done => b"DONE",
        })))
    }
}

impl fmt::Display for DBJobStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            DBJobStatus::Dirscan => write!(f, "DIRSCAN"),
            DBJobStatus::InProgress => write!(f, "IN_PROGRESS"),
            DBJobStatus::Aborted => write!(f, "ABORTED"),
            DBJobStatus::Done => write!(f, "DONE"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DBFileStatus {
    ToDo,
    InProgress,
    Error,
    Skipped,
    Done,
}

impl FromSql for DBFileStatus {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value {
            ValueRef::Text(b"TO_DO") => Ok(DBFileStatus::ToDo),
            ValueRef::Text(b"IN_PROGRESS") => Ok(DBFileStatus::InProgress),
            ValueRef::Text(b"ERROR") => Ok(DBFileStatus::Error),
            ValueRef::Text(b"SKIPPED") => Ok(DBFileStatus::Skipped),
            ValueRef::Text(b"DONE") => Ok(DBFileStatus::Done),
            _ => Err(FromSqlError::InvalidType),
        }
    }
}

impl ToSql for DBFileStatus {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(ValueRef::Text(match &self {
            DBFileStatus::ToDo => b"TO_DO",
            DBFileStatus::InProgress => b"IN_PROGRESS",
            DBFileStatus::Error => b"ERROR",
            DBFileStatus::Skipped => b"SKIPPED",
            DBFileStatus::Done => b"DONE",
        })))
    }
}

#[derive(Debug, Clone)]
pub struct DBEntriesEntry {
    pub id: i64,
    pub job_id: i64,
    pub file: PathBuf,
    pub is_file: bool,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub uid: u32,
    pub gid: u32,
}

#[derive(Debug, Clone)]
pub struct DBFileEntry {
    pub id: i64,
    pub job_id: i64,
    pub file: PathBuf,
    pub is_file: bool,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub uid: u32,
    pub gid: u32,
    pub status: DBFileStatus,
    pub message: String,

    // These are set during the Cp/Mv operations
    pub target_is_dir: bool,
    pub target_is_symlink: bool,
    pub cur_target: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct DBDirListEntry {
    pub id: i64,
    pub job_id: i64,
    pub file: DBFileEntry,
    pub cur_file: PathBuf,
    pub cur_target: PathBuf,
    pub new_dir: bool,
    pub status: DBFileStatus,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct DBRenameDirEntry {
    pub id: i64,
    pub job_id: i64,
    pub existing_target: PathBuf,
    pub cur_target: PathBuf,
}

#[derive(Debug, Clone)]
pub struct DBSkipDirEntry {
    pub id: i64,
    pub job_id: i64,
    pub file: PathBuf,
}

#[derive(Debug, Clone)]
pub struct DBJobEntry {
    pub id: i64,
    pub pid: u32,
    pub operation: DBJobOperation,
    pub cwd: PathBuf,
    pub entries: Vec<DBEntriesEntry>,
    pub dest: Option<PathBuf>,
    pub on_conflict: Option<OnConflict>,
    pub replace_first_path: bool,
    pub archives: Vec<PathBuf>,
    pub status: DBJobStatus,
}

#[derive(Debug)]
pub struct DataBase {
    conn: Connection,
}

impl DataBase {
    pub fn new(file: &Path) -> Result<DataBase> {
        let mut conn = Connection::open(file)?;

        let mut db = DataBase { conn };

        db.create_database()?;

        let signature: String =
            db.conn
                .query_row("SELECT v FROM kv WHERE k = ?1", ["signature"], |row| {
                    row.get(0)
                })?;

        if signature == DB_SIGNATURE {
            let version: String =
                db.conn
                    .query_row("SELECT v FROM kv WHERE k = ?1", ["version"], |row| {
                        row.get(0)
                    })?;

            if version != DB_VERSION {
                if db.conn.close().is_err() {
                    bail!("Failed to close db");
                }

                fs::remove_file(file)?;

                conn = Connection::open(file)?;
                db.conn = conn;

                db.create_database()?;
            }
        } else {
            bail!("Unknown database signature");
        }

        Ok(db)
    }

    fn create_database(&self) -> Result<()> {
        self.conn
            .execute_batch(include_str!("create_database.sql"))?;

        self.conn.execute(
            "INSERT OR IGNORE INTO kv (k, v) VALUES (?1, ?2)",
            ("signature", DB_SIGNATURE),
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO kv (k, v) VALUES (?1, ?2)",
            ("version", DB_VERSION),
        )?;

        Ok(())
    }

    pub fn new_job(&mut self, job: &mut DBJobEntry) -> i64 {
        let Ok(tx) = self.conn.transaction() else {
            return 0;
        };

        let job_id = match tx.execute(
            "INSERT INTO jobs (
                pid,
                operation,
                cwd,
                dest,
                on_conflict,
                replace_first_path,
                status
            ) VALUES (
                ?1,
                ?2,
                ?3,
                ?4,
                ?5,
                ?6,
                ?7
            )",
            (
                job.pid,
                job.operation,
                job.cwd.to_string_lossy(),
                job.dest.as_ref().map(|x| x.to_string_lossy()),
                job.on_conflict,
                job.replace_first_path,
                job.status,
            ),
        ) {
            Ok(_) => tx.last_insert_rowid(),
            Err(_) => {
                return 0;
            }
        };

        {
            let Ok(mut stmt) = tx.prepare(
                "INSERT INTO entries (
                    job_id,
                    file,
                    is_file,
                    is_dir,
                    is_symlink,
                    size,
                    uid,
                    gid
                ) VALUES (
                    ?1,
                    ?2,
                    ?3,
                    ?4,
                    ?5,
                    ?6,
                    ?7,
                    ?8
                )",
            ) else {
                return 0;
            };

            for entry in job.entries.iter_mut() {
                match stmt.execute((
                    job_id,
                    entry.file.to_string_lossy(),
                    entry.is_file,
                    entry.is_dir,
                    entry.is_symlink,
                    entry.size,
                    entry.uid,
                    entry.gid,
                )) {
                    Ok(_) => {
                        entry.id = tx.last_insert_rowid();
                        entry.job_id = job_id;
                    }
                    Err(_) => {
                        return 0;
                    }
                }
            }
        }

        {
            let Ok(mut stmt) = tx.prepare("INSERT INTO archives (job_id, archive) VALUES (?1, ?2)")
            else {
                return 0;
            };

            for archive in job.archives.iter() {
                if stmt.execute((job_id, archive.to_string_lossy())).is_err() {
                    return 0;
                }
            }
        }

        if tx.commit().is_err() {
            return 0;
        }

        job.id = job_id;

        job_id
    }

    pub fn get_pending_jobs(&mut self, pid: u32, exe: std::io::Result<PathBuf>) -> Vec<DBJobEntry> {
        let Ok(tx) = self.conn.transaction() else {
            return Vec::new();
        };

        let mut jobs: Vec<DBJobEntry> = tx
            .prepare(
                "SELECT id,
                        pid,
                        operation,
                        cwd,
                        dest,
                        on_conflict,
                        replace_first_path,
                        status
                FROM jobs
                ORDER BY id DESC",
            )
            .and_then(|mut stmt| {
                stmt.query_map([], |row| {
                    Ok(DBJobEntry {
                        id: row.get(0)?,
                        pid: row.get(1)?,
                        operation: row.get(2)?,
                        cwd: PathBuf::from(row.get::<usize, String>(3)?),
                        entries: Vec::new(),
                        dest: row.get::<usize, Option<String>>(4)?.map(PathBuf::from),
                        on_conflict: row.get(5)?,
                        archives: Vec::new(),
                        replace_first_path: row.get(6)?,
                        status: row.get(7)?,
                    })
                })
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<DBJobEntry>>>())
            })
            .unwrap_or_default()
            .iter()
            .filter(|job| {
                // If the pid stored in the job is the same as the current pid, then
                // it's an interrupted job from a previous session, given that the
                // current app has not started yet.
                if job.pid == pid {
                    return true;
                }

                match (&exe, &fs::canonicalize(&format!("/proc/{}/exe", job.pid))) {
                    // If the pid stored in the job has the same executable as me,
                    // it means that the job is running in another instance,
                    // and has not been interrupted.
                    (Ok(exe1), Ok(exe2)) if exe1 == exe2 => false,
                    _ => true,
                }
            })
            .cloned()
            .collect();

        {
            // Set the pid of the pending jobs to the current one, so that we don't
            // show the pending jobs to other processes
            let Ok(mut stmt) = tx.prepare("UPDATE jobs SET pid = ?1 WHERE id = ?2") else {
                return Vec::new();
            };

            for job in jobs.iter_mut() {
                match stmt.execute((pid, job.id)) {
                    Ok(_) => job.pid = pid,
                    Err(_) => {
                        return Vec::new();
                    }
                }
            }
        }

        {
            let Ok(mut stmt) = tx.prepare(
                "SELECT id,
                    file,
                    is_file,
                    is_dir,
                    is_symlink,
                    size,
                    uid,
                    gid
                FROM entries
                WHERE job_id = ?1
                ORDER BY id",
            ) else {
                return Vec::new();
            };

            for job in jobs.iter_mut() {
                job.entries = stmt
                    .query_map([job.id], |row| {
                        Ok(DBEntriesEntry {
                            id: row.get(0)?,
                            job_id: job.id,
                            file: PathBuf::from(row.get::<usize, String>(1)?),
                            is_file: row.get(2)?,
                            is_dir: row.get(3)?,
                            is_symlink: row.get(4)?,
                            size: row.get(5)?,
                            uid: row.get(6)?,
                            gid: row.get(7)?,
                        })
                    })
                    .and_then(|rows| rows.collect())
                    .unwrap_or_default();
            }
        }

        {
            let Ok(mut stmt) = tx.prepare(
                "SELECT archive
                FROM archives
                WHERE job_id = ?1
                ORDER BY id",
            ) else {
                return Vec::new();
            };

            for job in jobs.iter_mut() {
                job.archives = stmt
                    .query_map([job.id], |row| {
                        Ok(PathBuf::from(row.get::<usize, String>(0)?))
                    })
                    .and_then(|rows| rows.collect())
                    .unwrap_or_default();
            }
        }

        if tx.commit().is_err() {
            return Vec::new();
        }

        jobs
    }

    pub fn delete_job(&self, job_id: i64) {
        let _ = self
            .conn
            .execute("DELETE FROM jobs WHERE id = ?1", [job_id]);
    }

    pub fn set_job_status(&self, job_id: i64, status: DBJobStatus) {
        let _ = self.conn.execute(
            "UPDATE jobs SET status = ?1 WHERE id = ?2",
            (status, job_id),
        );
    }

    pub fn get_file_list(&self, job_id: i64) -> Vec<DBFileEntry> {
        self.conn
            .prepare(
                "SELECT id,
                        file,
                        is_file,
                        is_dir,
                        is_symlink,
                        size,
                        uid,
                        gid,
                        status,
                        message,
                        target_is_dir,
                        target_is_symlink,
                        cur_target
                FROM files
                WHERE job_id = ?1
                ORDER BY id",
            )
            .and_then(|mut stmt| {
                stmt.query_map([job_id], |row| {
                    Ok(DBFileEntry {
                        id: row.get(0)?,
                        job_id,
                        file: PathBuf::from(row.get::<usize, String>(1)?),
                        is_file: row.get(2)?,
                        is_dir: row.get(3)?,
                        is_symlink: row.get(4)?,
                        size: row.get(5)?,
                        uid: row.get(6)?,
                        gid: row.get(7)?,
                        status: row.get(8)?,
                        message: row.get(9)?,
                        target_is_dir: row.get(10)?,
                        target_is_symlink: row.get(11)?,
                        cur_target: row.get::<usize, Option<String>>(12)?.map(PathBuf::from),
                    })
                })
                .and_then(|rows| rows.collect())
            })
            .unwrap_or_default()
    }

    pub fn set_file_list(&mut self, job_id: i64, files: &mut [DBFileEntry]) {
        let Ok(tx) = self.conn.transaction() else {
            return;
        };

        {
            let Ok(mut stmt) = tx.prepare(
                "INSERT INTO files (
                    job_id,
                    file,
                    is_file,
                    is_dir,
                    is_symlink,
                    size,
                    uid,
                    gid,
                    status,
                    message,
                    target_is_dir,
                    target_is_symlink,
                    cur_target
                ) VALUES (
                    ?1,
                    ?2,
                    ?3,
                    ?4,
                    ?5,
                    ?6,
                    ?7,
                    ?8,
                    ?9,
                    ?10,
                    ?11,
                    ?12,
                    ?13
                )",
            ) else {
                return;
            };

            for entry in files.iter_mut() {
                match stmt.execute((
                    job_id,
                    entry.file.to_string_lossy(),
                    entry.is_file,
                    entry.is_dir,
                    entry.is_symlink,
                    entry.size,
                    entry.uid,
                    entry.gid,
                    entry.status,
                    &entry.message,
                    entry.target_is_dir,
                    entry.target_is_symlink,
                    entry
                        .cur_target
                        .as_ref()
                        .map(|cur_target| cur_target.to_string_lossy()),
                )) {
                    Ok(_) => {
                        entry.id = tx.last_insert_rowid();
                        entry.job_id = job_id;
                    }
                    Err(_) => {
                        return;
                    }
                }
            }
        }

        let Ok(_) = tx.execute(
            "UPDATE jobs SET status = ?1 WHERE id = ?2",
            (DBJobStatus::InProgress, job_id),
        ) else {
            return;
        };

        let _ = tx.commit();
    }

    pub fn update_file_list(&mut self, files: &[DBFileEntry]) {
        let Ok(tx) = self.conn.transaction() else {
            return;
        };

        {
            let Ok(mut stmt) = tx.prepare(
                "UPDATE files
                SET status = ?1,
                    message = ?2
                WHERE id = ?3",
            ) else {
                return;
            };

            for entry in files.iter() {
                if stmt
                    .execute((entry.status, &entry.message, entry.id))
                    .is_err()
                {
                    return;
                }
            }
        }

        let _ = tx.commit();
    }

    pub fn update_file(&self, file: &DBFileEntry) {
        if let Ok(mut stmt) = self.conn.prepare_cached(
            "UPDATE files
            SET status = ?1,
                message = ?2,
                target_is_dir = ?3,
                target_is_symlink = ?4,
                cur_target = ?5
            WHERE id = ?6",
        ) {
            let _ = stmt.execute((
                file.status,
                &file.message,
                file.target_is_dir,
                file.target_is_symlink,
                file.cur_target.as_ref().map(|x| x.to_string_lossy()),
                file.id,
            ));
        }
    }

    pub fn set_file_status(&self, file: &DBFileEntry) {
        if let Ok(mut stmt) = self
            .conn
            .prepare_cached("UPDATE files SET status = ?1, message = ?2 WHERE id = ?3")
        {
            let _ = stmt.execute((file.status, &file.message, file.id));
        }
    }

    pub fn get_dir_list(&self, job_id: i64) -> Vec<DBDirListEntry> {
        self.conn
            .prepare(
                "SELECT dir_list.id,
                        dir_list.cur_file,
                        dir_list.cur_target,
                        dir_list.new_dir,
                        dir_list.status,
                        dir_list.message,
                        files.id,
                        files.file,
                        files.is_file,
                        files.is_dir,
                        files.is_symlink,
                        files.size,
                        files.uid,
                        files.gid,
                        files.status,
                        files.message,
                        files.target_is_dir,
                        files.target_is_symlink,
                        files.cur_target
                FROM dir_list
                JOIN files ON files.id = dir_list.file_id
                WHERE dir_list.job_id = ?1
                ORDER BY dir_list.id",
            )
            .and_then(|mut stmt| {
                stmt.query_map([job_id], |row| {
                    Ok(DBDirListEntry {
                        id: row.get(0)?,
                        job_id,
                        cur_file: PathBuf::from(row.get::<usize, String>(1)?),
                        cur_target: PathBuf::from(row.get::<usize, String>(2)?),
                        new_dir: row.get(3)?,
                        status: row.get(4)?,
                        message: row.get(5)?,
                        file: DBFileEntry {
                            id: row.get(6)?,
                            job_id,
                            file: PathBuf::from(row.get::<usize, String>(7)?),
                            is_file: row.get(8)?,
                            is_dir: row.get(9)?,
                            is_symlink: row.get(10)?,
                            size: row.get(11)?,
                            uid: row.get(12)?,
                            gid: row.get(13)?,
                            status: row.get(14)?,
                            message: row.get(15)?,
                            target_is_dir: row.get(16)?,
                            target_is_symlink: row.get(17)?,
                            cur_target: row.get::<usize, Option<String>>(18)?.map(PathBuf::from),
                        },
                    })
                })
                .and_then(|rows| rows.collect())
            })
            .unwrap_or_default()
    }

    pub fn push_dir_list(&self, dir_list_entry: &mut DBDirListEntry) -> i64 {
        self.conn
            .prepare_cached(
                "INSERT INTO dir_list (
                    job_id,
                    file_id,
                    cur_file,
                    cur_target,
                    new_dir,
                    status,
                    message
                ) VALUES (
                    ?1,
                    ?2,
                    ?3,
                    ?4,
                    ?5,
                    ?6,
                    ?7
                )",
            )
            .and_then(|mut stmt| {
                stmt.execute((
                    dir_list_entry.job_id,
                    dir_list_entry.file.id,
                    dir_list_entry.cur_file.to_string_lossy(),
                    dir_list_entry.cur_target.to_string_lossy(),
                    dir_list_entry.new_dir,
                    dir_list_entry.status,
                    &dir_list_entry.message,
                ))
            })
            .map(|_| {
                let last_id = self.conn.last_insert_rowid();

                dir_list_entry.id = last_id;

                last_id
            })
            .unwrap_or(0)
    }

    pub fn set_dir_list_entry_status(&self, dir_list_entry: &DBDirListEntry) {
        if let Ok(mut stmt) = self
            .conn
            .prepare_cached("UPDATE dir_list SET status = ?1, message = ?2 WHERE id = ?3")
        {
            let _ = stmt.execute((
                dir_list_entry.status,
                &dir_list_entry.message,
                dir_list_entry.id,
            ));
        }
    }

    pub fn get_rename_dir_stack(&self, job_id: i64) -> Vec<DBRenameDirEntry> {
        self.conn
            .prepare(
                "SELECT id, existing_target, cur_target
                FROM rename_dir_stack
                WHERE job_id = ?1
                ORDER BY id",
            )
            .and_then(|mut stmt| {
                stmt.query_map([job_id], |row| {
                    Ok(DBRenameDirEntry {
                        id: row.get(0)?,
                        job_id,
                        existing_target: PathBuf::from(row.get::<usize, String>(1)?),
                        cur_target: PathBuf::from(row.get::<usize, String>(2)?),
                    })
                })
                .and_then(|rows| rows.collect())
            })
            .unwrap_or_default()
    }

    pub fn push_rename_dir_stack(&self, rename_dir_stack_entry: &mut DBRenameDirEntry) -> i64 {
        self.conn
            .prepare_cached("INSERT INTO rename_dir_stack (job_id, existing_target, cur_target) VALUES (?1, ?2, ?3)")
            .and_then(|mut stmt| {
                stmt.execute((
                    rename_dir_stack_entry.job_id,
                    rename_dir_stack_entry.existing_target.to_string_lossy(),
                    rename_dir_stack_entry.cur_target.to_string_lossy(),
                ))
            })
            .map(|_| {
                let last_id = self.conn.last_insert_rowid();

                rename_dir_stack_entry.id = last_id;

                last_id
            })
            .unwrap_or(0)
    }

    pub fn pop_rename_dir_stack(&self, rename_dir_stack_id: i64) {
        if let Ok(mut stmt) = self
            .conn
            .prepare_cached("DELETE FROM rename_dir_stack WHERE id = ?1")
        {
            let _ = stmt.execute([rename_dir_stack_id]);
        }
    }

    pub fn get_skip_dir_stack(&self, job_id: i64) -> Vec<DBSkipDirEntry> {
        self.conn
            .prepare(
                "SELECT id, file
                FROM skip_dir_stack
                WHERE job_id = ?1
                ORDER BY id",
            )
            .and_then(|mut stmt| {
                stmt.query_map([job_id], |row| {
                    Ok(DBSkipDirEntry {
                        id: row.get(0)?,
                        job_id,
                        file: PathBuf::from(row.get::<usize, String>(1)?),
                    })
                })
                .and_then(|rows| rows.collect())
            })
            .unwrap_or_default()
    }

    pub fn push_skip_dir_stack(&self, skip_dir_stack_entry: &mut DBSkipDirEntry) -> i64 {
        self.conn
            .prepare_cached("INSERT INTO skip_dir_stack (job_id, file) VALUES (?1, ?2)")
            .and_then(|mut stmt| {
                stmt.execute((
                    skip_dir_stack_entry.job_id,
                    skip_dir_stack_entry.file.to_string_lossy(),
                ))
            })
            .map(|_| {
                let last_id = self.conn.last_insert_rowid();

                skip_dir_stack_entry.id = last_id;

                last_id
            })
            .unwrap_or(0)
    }

    pub fn pop_skip_dir_stack(&self, skip_dir_stack_id: i64) {
        if let Ok(mut stmt) = self
            .conn
            .prepare_cached("DELETE FROM skip_dir_stack WHERE id = ?1")
        {
            let _ = stmt.execute([skip_dir_stack_id]);
        }
    }
}
