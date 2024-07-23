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
pub enum DBJobStatus {
    DirScan, // TODO
    InProgress,
    Aborted,
    Done,
}

impl ToSql for DBJobStatus {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(ValueRef::Text(match &self {
            DBJobStatus::DirScan => b"DIRSCAN",
            DBJobStatus::InProgress => b"IN_PROGRESS",
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
pub enum DBCommand {
    GetReplaceFirstPath(i64, Sender<Option<bool>>),
    SetReplaceFirstPath(i64, bool),
    DeleteJob(i64),
    SetJobStatus(i64, DBJobStatus),
    GetDirList(i64, Sender<Vec<DBDirListEntry>>),
    PushDirList(DBDirListEntry, Sender<i64>),
    SetDirListEntryStatus(DBDirListEntry),
    GetSkipDirStack(i64, Sender<Vec<DBSkipDirEntry>>),
    PushSkipDirStack(DBSkipDirEntry, Sender<i64>),
    PopSkipDirStack(i64),
    GetRenameDirStack(i64, Sender<Vec<DBRenameDirEntry>>),
    PushRenameDirStack(DBRenameDirEntry, Sender<i64>),
    PopRenameDirStack(i64),
    UpdateFile(DBFileEntry),
    SetFileStatus(DBFileEntry),
}

#[derive(Debug)]
struct DataBase {
    conn: Connection,
}

pub fn start(file: &Path) -> Result<Sender<DBCommand>> {
    match DataBase::new(file) {
        Ok(mut db) => {
            let (command_tx, command_rx) = crossbeam_channel::unbounded();
            thread::spawn(move || loop {
                match command_rx.recv() {
                    Ok(command) => match command {
                        DBCommand::GetReplaceFirstPath(job_id, replace_first_path_tx) => {
                            let _ = replace_first_path_tx.send(db.get_replace_first_path(job_id));
                        }
                        DBCommand::SetReplaceFirstPath(job_id, replace_first_path) => {
                            db.set_replace_first_path(job_id, replace_first_path);
                        }
                        DBCommand::DeleteJob(job_id) => {
                            db.delete_job(job_id);
                        }
                        DBCommand::SetJobStatus(job_id, status) => {
                            db.set_job_status(job_id, status);
                        }
                        DBCommand::GetDirList(job_id, dir_list_tx) => {
                            let _ = dir_list_tx.send(db.get_dir_list(job_id));
                        }
                        DBCommand::PushDirList(mut dir_list_entry, dir_list_id_tx) => {
                            let _ = dir_list_id_tx.send(db.push_dir_list(&mut dir_list_entry));
                        }
                        DBCommand::SetDirListEntryStatus(dir_list_entry) => {
                            db.set_dir_list_entry_status(&dir_list_entry);
                        }
                        DBCommand::GetSkipDirStack(job_id, skip_dir_stack_tx) => {
                            let _ = skip_dir_stack_tx.send(db.get_skip_dir_stack(job_id));
                        }
                        DBCommand::PushSkipDirStack(
                            mut skip_dir_stack_entry,
                            skip_dir_stack_id_tx,
                        ) => {
                            let _ = skip_dir_stack_id_tx
                                .send(db.push_skip_dir_stack(&mut skip_dir_stack_entry));
                        }
                        DBCommand::PopSkipDirStack(skip_dir_stack_id) => {
                            db.pop_skip_dir_stack(skip_dir_stack_id);
                        }
                        DBCommand::GetRenameDirStack(job_id, rename_dir_stack_tx) => {
                            let _ = rename_dir_stack_tx.send(db.get_rename_dir_stack(job_id));
                        }
                        DBCommand::PushRenameDirStack(
                            mut rename_dir_stack_entry,
                            rename_dir_stack_id_tx,
                        ) => {
                            let _ = rename_dir_stack_id_tx
                                .send(db.push_rename_dir_stack(&mut rename_dir_stack_entry));
                        }
                        DBCommand::PopRenameDirStack(rename_dir_stack_id) => {
                            db.pop_rename_dir_stack(rename_dir_stack_id);
                        }
                        DBCommand::UpdateFile(file) => {
                            db.update_file(&file);
                        }
                        DBCommand::SetFileStatus(file) => {
                            db.set_file_status(&file);
                        }
                    },

                    // When the main thread exits, the channel returns an error
                    Err(_) => return,
                }
            });

            Ok(command_tx)
        }
        Err(e) => Err(e),
    }
}

pub fn get_replace_first_path(command_tx: &Sender<DBCommand>, job_id: i64) -> Option<bool> {
    let (replace_first_path_tx, replace_first_path_rx) = crossbeam_channel::unbounded();

    command_tx
        .send(DBCommand::GetReplaceFirstPath(
            job_id,
            replace_first_path_tx,
        ))
        .unwrap();

    replace_first_path_rx.recv().unwrap()
}

pub fn set_replace_first_path(
    command_tx: &Sender<DBCommand>,
    job_id: i64,
    replace_first_path: bool,
) {
    command_tx
        .send(DBCommand::SetReplaceFirstPath(job_id, replace_first_path))
        .unwrap();
}

pub fn delete_job(command_tx: &Sender<DBCommand>, job_id: i64) {
    command_tx.send(DBCommand::DeleteJob(job_id)).unwrap();
}

pub fn set_job_status(command_tx: &Sender<DBCommand>, job_id: i64, status: DBJobStatus) {
    command_tx
        .send(DBCommand::SetJobStatus(job_id, status))
        .unwrap();
}

pub fn get_dir_list(command_tx: &Sender<DBCommand>, job_id: i64) -> Vec<DBDirListEntry> {
    let (dir_list_tx, dir_list_rx) = crossbeam_channel::unbounded();

    command_tx
        .send(DBCommand::GetDirList(job_id, dir_list_tx))
        .unwrap();

    dir_list_rx.recv().unwrap()
}

pub fn push_dir_list(command_tx: &Sender<DBCommand>, dir_list_entry: &mut DBDirListEntry) -> i64 {
    let (dir_list_id_tx, dir_list_id_rx) = crossbeam_channel::unbounded();

    command_tx
        .send(DBCommand::PushDirList(
            dir_list_entry.clone(),
            dir_list_id_tx,
        ))
        .unwrap();

    let last_id = dir_list_id_rx.recv().unwrap();

    dir_list_entry.id = last_id;

    last_id
}

pub fn set_dir_list_entry_status(command_tx: &Sender<DBCommand>, dir_list_entry: &DBDirListEntry) {
    command_tx
        .send(DBCommand::SetDirListEntryStatus(dir_list_entry.clone()))
        .unwrap();
}

pub fn get_skip_dir_stack(command_tx: &Sender<DBCommand>, job_id: i64) -> Vec<DBSkipDirEntry> {
    let (skip_dir_stack_tx, skip_dir_stack_rx) = crossbeam_channel::unbounded();

    command_tx
        .send(DBCommand::GetSkipDirStack(job_id, skip_dir_stack_tx))
        .unwrap();

    skip_dir_stack_rx.recv().unwrap()
}

pub fn push_skip_dir_stack(
    command_tx: &Sender<DBCommand>,
    skip_dir_stack_entry: &mut DBSkipDirEntry,
) -> i64 {
    let (skip_dir_stack_id_tx, skip_dir_stack_id_rx) = crossbeam_channel::unbounded();

    command_tx
        .send(DBCommand::PushSkipDirStack(
            skip_dir_stack_entry.clone(),
            skip_dir_stack_id_tx,
        ))
        .unwrap();

    let last_id = skip_dir_stack_id_rx.recv().unwrap();

    skip_dir_stack_entry.id = last_id;

    last_id
}

pub fn pop_skip_dir_stack(command_tx: &Sender<DBCommand>, skip_dir_stack_id: i64) {
    command_tx
        .send(DBCommand::PopSkipDirStack(skip_dir_stack_id))
        .unwrap();
}

pub fn get_rename_dir_stack(command_tx: &Sender<DBCommand>, job_id: i64) -> Vec<DBRenameDirEntry> {
    let (rename_dir_stack_tx, rename_dir_stack_rx) = crossbeam_channel::unbounded();

    command_tx
        .send(DBCommand::GetRenameDirStack(job_id, rename_dir_stack_tx))
        .unwrap();

    rename_dir_stack_rx.recv().unwrap()
}

pub fn push_rename_dir_stack(
    command_tx: &Sender<DBCommand>,
    rename_dir_stack_entry: &mut DBRenameDirEntry,
) -> i64 {
    let (rename_dir_stack_id_tx, rename_dir_stack_id_rx) = crossbeam_channel::unbounded();

    command_tx
        .send(DBCommand::PushRenameDirStack(
            rename_dir_stack_entry.clone(),
            rename_dir_stack_id_tx,
        ))
        .unwrap();

    let last_id = rename_dir_stack_id_rx.recv().unwrap();

    rename_dir_stack_entry.id = last_id;

    last_id
}

pub fn pop_rename_dir_stack(command_tx: &Sender<DBCommand>, rename_dir_stack_id: i64) {
    command_tx
        .send(DBCommand::PopRenameDirStack(rename_dir_stack_id))
        .unwrap();
}

pub fn update_file(command_tx: &Sender<DBCommand>, file: &DBFileEntry) {
    command_tx
        .send(DBCommand::UpdateFile(file.clone()))
        .unwrap();
}

pub fn set_file_status(command_tx: &Sender<DBCommand>, file: &DBFileEntry) {
    command_tx
        .send(DBCommand::SetFileStatus(file.clone()))
        .unwrap();
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
            "INSERT OR IGNORE INTO kv (k, v) VALUES (?1, ?2);",
            ("signature", DB_SIGNATURE),
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO kv (k, v) VALUES (?1, ?2);",
            ("version", DB_VERSION),
        )?;

        Ok(())
    }

    /*
        def new_job(self, operation, file_list, scan_error, scan_skipped, files, cwd, dest=None, on_conflict=None, archives=None):
            job_id = None

            if self.conn is None:
                return job_id

            try:
                with self.conn:
                    c = self.conn.execute("SELECT MAX(id) FROM jobs")
                    job_id = c.fetchone()[0] or 0
                    job_id += 1
                    c.close()
                    self.conn.execute("INSERT INTO jobs VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)", (
                        job_id,
                        operation,
                        json.dumps([str(x) for x in files]),
                        cwd,
                        dest,
                        on_conflict,
                        json.dumps(archives),
                        json.dumps(scan_error),
                        json.dumps(scan_skipped),
                        None,
                        None,
                        None,
                        None,
                        "IN_PROGRESS",
                    ))

                    c = self.conn.execute("SELECT MAX(id) FROM files")
                    file_id = c.fetchone()[0] or 0
                    file_id += 1
                    c.close()
                    for file in file_list:
                        self.conn.execute("INSERT INTO files VALUES (?, ?, ?, ?, ?)", (
                            file_id,
                            job_id,
                            json.dumps(file),
                            "TO_DO",
                            None,
                        ))

                        file["id"] = file_id
                        file["status"] = "TO_DO"

                        file_id += 1
            except sqlite3.OperationalError:
                pass

            return job_id
    */

    pub fn update_file(&self, file: &DBFileEntry) {
        let _ = self.conn.execute(
            "UPDATE files
            SET status = ?1,
                message = ?2,
                target_is_dir = ?3,
                target_is_symlink = ?4,
                cur_target = ?5
            WHERE id = ?6",
            (
                file.status,
                &file.message,
                file.target_is_dir,
                file.target_is_symlink,
                file.cur_target.as_ref().map(|x| x.to_string_lossy()),
                file.id,
            ),
        );
    }

    pub fn set_file_status(&self, file: &DBFileEntry) {
        let _ = self.conn.execute(
            "UPDATE files SET status = ?1, message = ?2 WHERE id = ?3",
            (file.status, &file.message, file.id),
        );
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
        match self.conn.execute(
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
            (
                dir_list_entry.job_id,
                dir_list_entry.file.id,
                dir_list_entry.cur_file.to_string_lossy(),
                dir_list_entry.cur_target.to_string_lossy(),
                dir_list_entry.new_dir,
                dir_list_entry.status,
                &dir_list_entry.message,
            ),
        ) {
            Ok(_) => {
                let last_id = self.conn.last_insert_rowid();

                dir_list_entry.id = last_id;

                last_id
            }
            Err(_) => 0,
        }
    }

    pub fn set_dir_list_entry_status(&self, dir_list_entry: &DBDirListEntry) {
        let _ = self.conn.execute(
            "UPDATE dir_list SET status = ?1, message = ?2 WHERE id = ?3",
            (
                dir_list_entry.status,
                &dir_list_entry.message,
                dir_list_entry.id,
            ),
        );
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
        match self.conn.execute(
            "INSERT INTO rename_dir_stack (job_id, existing_target, cur_target) VALUES (?1, ?2, ?3)",
            (
                rename_dir_stack_entry.job_id,
                rename_dir_stack_entry.existing_target.to_string_lossy(),
                rename_dir_stack_entry.cur_target.to_string_lossy(),
            ),
        ) {
            Ok(_) => {
                let last_id = self.conn.last_insert_rowid();

                rename_dir_stack_entry.id = last_id;

                last_id
            }
            Err(_) => 0,
        }
    }

    pub fn pop_rename_dir_stack(&self, rename_dir_stack_id: i64) {
        let _ = self.conn.execute(
            "DELETE FROM rename_dir_stack WHERE id = ?1",
            [rename_dir_stack_id],
        );
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
        match self.conn.execute(
            "INSERT INTO skip_dir_stack (job_id, file) VALUES (?1, ?2)",
            (
                skip_dir_stack_entry.job_id,
                skip_dir_stack_entry.file.to_string_lossy(),
            ),
        ) {
            Ok(_) => {
                let last_id = self.conn.last_insert_rowid();

                skip_dir_stack_entry.id = last_id;

                last_id
            }
            Err(_) => 0,
        }
    }

    pub fn pop_skip_dir_stack(&self, skip_dir_stack_id: i64) {
        let _ = self.conn.execute(
            "DELETE FROM skip_dir_stack WHERE id = ?1",
            [skip_dir_stack_id],
        );
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
        def get_jobs(self):
            jobs = []

            if self.conn is None:
                return jobs

            try:
                with self.conn:
                    c = self.conn.execute("SELECT * FROM jobs")
                    jobs.extend(c.fetchall())
                    c.close()
            except sqlite3.OperationalError:
                pass

            return jobs

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
