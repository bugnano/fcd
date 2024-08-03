use std::{
    fs,
    path::{Path, PathBuf},
    thread,
};

use anyhow::{bail, Result};
use crossbeam_channel::{Receiver, Sender};

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

impl ToSql for DBJobOperation {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(ValueRef::Text(match &self {
            DBJobOperation::Cp => b"CP",
            DBJobOperation::Mv => b"MV",
            DBJobOperation::Rm => b"RM",
        })))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DBJobStatus {
    Dirscan, // TODO
    InProgress,
    AbortedDirscan, // TODO -- Not sure if useful
    Aborted,
    Done,
}

impl ToSql for DBJobStatus {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(ValueRef::Text(match &self {
            DBJobStatus::Dirscan => b"DIRSCAN",
            DBJobStatus::InProgress => b"IN_PROGRESS",
            DBJobStatus::AbortedDirscan => b"ABORTED_DIRSCAN",
            DBJobStatus::Aborted => b"ABORTED",
            DBJobStatus::Done => b"DONE",
        })))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DBFileStatus {
    ToDo,
    InProgress,
    Error,
    Skipped,
    Aborted, // Not in rnr
    Done,
}

impl FromSql for DBFileStatus {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value {
            ValueRef::Text(b"TO_DO") => Ok(DBFileStatus::ToDo),
            ValueRef::Text(b"IN_PROGRESS") => Ok(DBFileStatus::InProgress),
            ValueRef::Text(b"ERROR") => Ok(DBFileStatus::Error),
            ValueRef::Text(b"SKIPPED") => Ok(DBFileStatus::Skipped),
            ValueRef::Text(b"ABORTED") => Ok(DBFileStatus::Aborted),
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
            DBFileStatus::Aborted => b"ABORTED",
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
    pub pid: i64,
    pub operation: DBJobOperation,
    pub cwd: PathBuf,
    pub entries: Vec<DBEntriesEntry>,
    pub dest: Option<PathBuf>,
    pub on_conflict: Option<OnConflict>,
    pub archives: Vec<PathBuf>,
    pub replace_first_path: Option<bool>,
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

    pub fn new_job(&mut self, &mut job: DBJobEntry) -> i64 {
        match self.conn.transaction() {
            Ok(tx) => {
                let job_id = match tx.execute(
                    "INSERT INTO jobs (
                        pid,
                        operation,
                        cwd,
                        dest,
                        on_conflict,
                        status
                    ) VALUES (
                        ?1,
                        ?2,
                        ?3,
                        ?4,
                        ?5,
                        ?6
                    )",
                    (
                        job.pid,
                        job.operation,
                        job.cwd.to_string_lossy(),
                        job.dest.map(|x| x.to_string_lossy()),
                        job.on_conflict,
                        job.status,
                    ),
                ) {
                    Ok(_) => tx.last_insert_rowid(),
                    Err(_) => return 0,
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
                    let Ok(mut stmt) =
                        tx.prepare("INSERT INTO archives (job_id, archive) VALUES (?1, ?2)")
                    else {
                        return 0;
                    };

                    for archive in job.archives.iter() {
                        if let Err(_) = stmt.execute((job_id, archive.as_ref().to_string_lossy())) {
                            return 0;
                        }
                    }
                }

                if let Err(_) = tx.commit() {
                    return 0;
                }

                job.id = job_id;

                job_id
            }
            Err(_) => 0,
        }
    }

    pub fn get_jobs(&self) -> Vec<DBJobEntry> {
        let mut jobs = self
            .conn
            .prepare(
                "SELECT id,
                        pid,
                        operation,
                        cwd,
                        dest,
                        on_conflict,
                        replace_first_path,
                        status,
                FROM jobs
                ORDER BY id",
            )
            .and_then(|mut stmt| {
                stmt.query_map([], |row| {
                    Ok(DBJobEntry {
                        id: row.get(0)?,
                        pid: row.get(1)?,
                        operation: row.get(2)?,
                        cwd: PathBuf::from(row.get::<usize, String>(3)?),
                        entries: Vec::new(),
                        dest: PathBuf::from(row.get::<usize, String>(4)?),
                        on_conflict: row.get(5)?,
                        archives: Vec::new(),
                        replace_first_path: row.get(6)?,
                        status: row.get(7)?,
                    })
                })
                .and_then(|rows| rows.collect())
            })
            .unwrap_or_default();

        if let Ok(mut stmt) = self.conn.prepare(
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
        ) {
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

        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT archive,
            FROM archives
            WHERE job_id = ?1
            ORDER BY id",
        ) {
            for job in jobs.iter_mut() {
                job.archives = stmt
                    .query_map([job.id], |row| PathBuf::from(row.get::<usize, String>(0)?))
                    .and_then(|rows| rows.collect())
                    .unwrap_or_default();
            }
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
                        files.job_id,
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
                        files.cur_target,
                FROM dir_list
                JOIN files ON files.id = dir_list.file_id
                WHERE job_id = ?1
                ORDER BY id",
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
                            job_id: row.get(7)?,
                            file: PathBuf::from(row.get::<usize, String>(8)?),
                            is_file: row.get(9)?,
                            is_dir: row.get(10)?,
                            is_symlink: row.get(11)?,
                            size: row.get(12)?,
                            uid: row.get(13)?,
                            gid: row.get(14)?,
                            status: row.get(15)?,
                            message: row.get(16)?,
                            target_is_dir: row.get(17)?,
                            target_is_symlink: row.get(18)?,
                            cur_target: row.get::<usize, Option<String>>(19)?.map(PathBuf::from),
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

    pub fn get_replace_first_path(&self, job_id: i64) -> Option<bool> {
        self.conn
            .query_row(
                "SELECT replace_first_path
                FROM jobs
                WHERE id = ?1",
                [job_id],
                |row| row.get(0),
            )
            .unwrap_or(None)
    }

    pub fn set_replace_first_path(&self, job_id: i64, replace_first_path: bool) {
        let _ = self.conn.execute(
            "UPDATE jobs SET replace_first_path = ?1 WHERE id = ?2",
            (replace_first_path, job_id),
        );
    }

    /*
        def get_file_list(self, job_id):
            file_list = []

            if self.conn is None:
                return file_list

            try:
                with self.conn:
                    c = self.conn.execute("SELECT * FROM files WHERE job_id = ?", (job_id,))
                    for row in c:
                        file = json.loads(row["file"])
                        file["id"] = row["id"]
                        file["status"] = row["status"]
                        file["message"] = row["message"]
                        file["lstat"] = os.stat_result(file["lstat"])

                        file_list.append(file)

                    c.close()
            except sqlite3.OperationalError:
                pass

            return file_list


        def __del__(self):
            if self.conn is None:
                return

            try:
                self.conn.commit()
                self.conn.close()
            except sqlite3.OperationalError:
                pass
    */
}
