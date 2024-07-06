use std::{
    fs,
    path::{Path, PathBuf},
    thread,
};

use anyhow::{bail, Result};
use crossbeam_channel::{Receiver, Sender};

use rusqlite::{params, Connection};

const DB_SIGNATURE: &str = "fcd";
const DB_VERSION: &str = "1";

#[derive(Debug, Clone, Copy)]
pub enum DBJobStatus {
    InProgress,
    Aborted,
    Done,
}

#[derive(Debug, Clone, Copy)]
pub enum DBFileStatus {
    ToDo,
    InProgress,
    Error,
    Skipped,
    Done,
}

#[derive(Debug, Clone, Copy)]
pub enum DBFileType {
    File,
    Dir,
    Symlink,
}

#[derive(Debug, Clone)]
pub struct DBFileEntry {
    pub id: i64,
    pub job_id: i64,
    pub file: PathBuf,
    pub file_type: DBFileType,
    pub size: u64,
    pub mtime: i64,
    pub mtime_nsec: i64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    // TODO: st_flags and xattrs
    pub status: DBFileStatus,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct DBDirListEntry {
    pub id: i64,
    pub job_id: i64,
    pub file: PathBuf,
    pub cur_file: PathBuf,
    pub cur_target: PathBuf,
    pub new_dir: bool,
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
    GetDirList(i64, Sender<Vec<DBDirListEntry>>),
    GetRenameDirStack(i64, Sender<Vec<DBRenameDirEntry>>),
    GetSkipDirStack(i64, Sender<Vec<DBSkipDirEntry>>),
    GetReplaceFirstPath(i64, Sender<Option<bool>>),
    SetReplaceFirstPath(i64, bool),
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
                        DBCommand::GetDirList(job_id, dir_list_tx) => {
                            let _ = dir_list_tx.send(db.get_dir_list(job_id));
                        }
                        DBCommand::GetRenameDirStack(job_id, rename_dir_stack_tx) => {
                            let _ = rename_dir_stack_tx.send(db.get_rename_dir_stack(job_id));
                        }
                        DBCommand::GetSkipDirStack(job_id, skip_dir_stack_tx) => {
                            let _ = skip_dir_stack_tx.send(db.get_skip_dir_stack(job_id));
                        }
                        DBCommand::GetReplaceFirstPath(job_id, replace_first_path_tx) => {
                            let _ = replace_first_path_tx.send(db.get_replace_first_path(job_id));
                        }
                        DBCommand::SetReplaceFirstPath(job_id, replace_first_path) => {
                            db.set_replace_first_path(job_id, replace_first_path);
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

pub fn get_dir_list(command_tx: &Sender<DBCommand>, job_id: i64) -> Vec<DBDirListEntry> {
    let (dir_list_tx, dir_list_rx) = crossbeam_channel::unbounded();

    command_tx
        .send(DBCommand::GetDirList(job_id, dir_list_tx))
        .unwrap();

    dir_list_rx.recv().unwrap()
}

pub fn get_rename_dir_stack(command_tx: &Sender<DBCommand>, job_id: i64) -> Vec<DBRenameDirEntry> {
    let (rename_dir_stack_tx, rename_dir_stack_rx) = crossbeam_channel::unbounded();

    command_tx
        .send(DBCommand::GetRenameDirStack(job_id, rename_dir_stack_tx))
        .unwrap();

    rename_dir_stack_rx.recv().unwrap()
}

pub fn get_skip_dir_stack(command_tx: &Sender<DBCommand>, job_id: i64) -> Vec<DBSkipDirEntry> {
    let (skip_dir_stack_tx, skip_dir_stack_rx) = crossbeam_channel::unbounded();

    command_tx
        .send(DBCommand::GetSkipDirStack(job_id, skip_dir_stack_tx))
        .unwrap();

    skip_dir_stack_rx.recv().unwrap()
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

impl DataBase {
    pub fn new(file: &Path) -> Result<DataBase> {
        let mut conn = Connection::open(file)?;

        let mut db = DataBase { conn };

        db.create_database()?;

        let signature: String =
            db.conn
                .query_row("SELECT v FROM misc WHERE k = ?1", ["signature"], |row| {
                    row.get(0)
                })?;

        if signature == DB_SIGNATURE {
            let version: String =
                db.conn
                    .query_row("SELECT v FROM misc WHERE k = ?1", ["version"], |row| {
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
        self.conn.execute_batch(
            "PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS jobs (
                id INTEGER NOT NULL PRIMARY KEY,
                operation TEXT NOT NULL,
                files TEXT NOT NULL,
                cwd TEXT NOT NULL,
                dest TEXT,
                on_conflict TEXT,
                archives TEXT,
                scan_error TEXT,
                scan_skipped TEXT,
                replace_first_path INTEGER,
                status TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS files (
                id INTEGER NOT NULL PRIMARY KEY,
                job_id INTEGER NOT NULL,
                file TEXT NOT NULL,
                file_type TEXT NOT NULL,
                size INTEGER NOT NULL,
                mtime INTEGER NOT NULL,
                mtime_nsec INTEGER NOT NULL,
                mode INTEGER NOT NULL,
                uid INTEGER NOT NULL,
                gid INTEGER NOT NULL,
                status TEXT NOT NULL,
                message TEXT NOT NULL,
                FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
            ) STRICT;

            CREATE TABLE IF NOT EXISTS dir_list (
                id INTEGER NOT NULL PRIMARY KEY,
                job_id INTEGER NOT NULL,
                file TEXT NOT NULL,
                cur_file TEXT NOT NULL,
                cur_target TEXT NOT NULL,
                new_dir INTEGER NOT NULL,
                FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
            ) STRICT;

            CREATE TABLE IF NOT EXISTS rename_dir_stack (
                id INTEGER NOT NULL PRIMARY KEY,
                job_id INTEGER NOT NULL,
                existing_target TEXT NOT NULL,
                cur_target TEXT NOT NULL,
                FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
            ) STRICT;

            CREATE TABLE IF NOT EXISTS skip_dir_stack (
                id INTEGER NOT NULL PRIMARY KEY,
                job_id INTEGER NOT NULL,
                file TEXT NOT NULL,
                FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
            ) STRICT;

            CREATE TABLE IF NOT EXISTS misc (
                k TEXT NOT NULL PRIMARY KEY,
                v TEXT
            ) STRICT;",
        )?;

        self.conn.execute(
            "INSERT OR IGNORE INTO misc (k, v) VALUES (?1, ?2);",
            ("signature", DB_SIGNATURE),
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO misc (k, v) VALUES (?1, ?2);",
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

        def update_file(self, file, status=None):
            if self.conn is None:
                return

            try:
                with self.conn:
                    if status is not None:
                        self.conn.execute("UPDATE files SET file = ?, status = ? WHERE id = ?", (
                            json.dumps(file),
                            status,
                            file["id"],
                        ))

                        file["status"] = status
                    else:
                        self.conn.execute("UPDATE files SET file = ? WHERE id = ?", (
                            json.dumps(file),
                            file["id"],
                        ))
            except sqlite3.OperationalError:
                pass

        def set_file_status(self, file, status, message=None):
            if self.conn is None:
                return

            try:
                with self.conn:
                    if message is not None:
                        self.conn.execute("UPDATE files SET status = ?, message = ? WHERE id = ?", (
                            status,
                            message,
                            file["id"],
                        ))
                    else:
                        self.conn.execute("UPDATE files SET status = ? WHERE id = ?", (
                            status,
                            file["id"],
                        ))

                    file["status"] = status
                    if message is not None:
                        file["message"] = message
            except sqlite3.OperationalError:
                pass

        def set_job_status(self, job_id, status):
            if self.conn is None:
                return

            try:
                with self.conn:
                    self.conn.execute("UPDATE jobs SET status = ? WHERE id = ?", (
                        status,
                        job_id,
                    ))
            except sqlite3.OperationalError:
                pass

        def delete_job(self, job_id):
            if self.conn is None:
                return

            try:
                with self.conn:
                    self.conn.execute("DELETE FROM jobs WHERE id = ?", (
                        job_id,
                    ))
            except sqlite3.OperationalError:
                pass

        def set_dir_list(self, job_id, dir_list):
            if self.conn is None:
                return

            try:
                with self.conn:
                    l = []
                    for x in dir_list:
                        file = x.copy()
                        file.update({"file": x["file"].copy(), "cur_file": str(x["cur_file"]), "cur_target": str(x["cur_target"])})
                        file["file"]["file"] = str(x["file"]["file"])
                        l.append(file)

                    self.conn.execute("UPDATE jobs SET dir_list = ? WHERE id = ?", (
                        json.dumps(l),
                        job_id,
                    ))
            except sqlite3.OperationalError:
                pass
    */

    pub fn get_dir_list(&self, job_id: i64) -> Vec<DBDirListEntry> {
        self.conn
            .prepare(
                "SELECT id, file, cur_file, cur_target, new_dir
                FROM dir_list
                WHERE job_id = ?1
                ORDER BY id",
            )
            .and_then(|mut stmt| {
                stmt.query_map([job_id], |row| {
                    Ok(DBDirListEntry {
                        id: row.get(0)?,
                        job_id,
                        file: PathBuf::from(row.get::<usize, String>(1)?),
                        cur_file: PathBuf::from(row.get::<usize, String>(2)?),
                        cur_target: PathBuf::from(row.get::<usize, String>(3)?),
                        new_dir: row.get(4)?,
                    })
                })
                .and_then(|rows| rows.collect())
            })
            .unwrap_or_default()
    }

    /*
        def set_rename_dir_stack(self, job_id, rename_dir_stack):
            if self.conn is None:
                return

            try:
                with self.conn:
                    self.conn.execute("UPDATE jobs SET rename_dir_stack = ? WHERE id = ?", (
                        json.dumps([list(map(str, x)) for x in rename_dir_stack]),
                        job_id,
                    ))
            except sqlite3.OperationalError:
                pass
    */

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

    /*
        def set_skip_dir_stack(self, job_id, skip_dir_stack):
            if self.conn is None:
                return

            try:
                with self.conn:
                    self.conn.execute("UPDATE jobs SET skip_dir_stack = ? WHERE id = ?", (
                        json.dumps([str(x) for x in skip_dir_stack]),
                        job_id,
                    ))
            except sqlite3.OperationalError:
                pass
    */

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
