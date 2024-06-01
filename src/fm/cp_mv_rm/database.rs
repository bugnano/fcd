use std::fs;

use anyhow::{bail, Result};

use rusqlite::{params, Connection};

const DB_SIGNATURE: &str = "rnr";
const DB_VERSION: &str = "1";

#[derive(Debug)]
pub struct DataBase {
    conn: Connection,
}

impl DataBase {
    pub fn new(file: &str) -> Result<DataBase> {
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
                dir_list TEXT,
                rename_dir_stack TEXT,
                skip_dir_stack TEXT,
                replace_first_path INTEGER,
                status TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS files (
                id INTEGER NOT NULL PRIMARY KEY,
                job_id INTEGER NOT NULL,
                file TEXT NOT NULL,
                status TEXT NOT NULL,
                message TEXT,
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

        def get_dir_list(self, job_id):
            dir_list = []

            if self.conn is None:
                return dir_list

            try:
                with self.conn:
                    c = self.conn.execute("SELECT dir_list FROM jobs WHERE id = ?", (job_id,))
                    record = c.fetchone()[0]
                    c.close()

                    if record:
                        for file in json.loads(record):
                            file["cur_file"] = Path(file["cur_file"])
                            file["cur_target"] = Path(file["cur_target"])
                            file["file"]["file"] = Path(file["file"]["file"])
                            file["file"]["lstat"] = os.stat_result(file["file"]["lstat"])
                            dir_list.append(file)
            except sqlite3.OperationalError:
                pass

            return dir_list

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

        def get_rename_dir_stack(self, job_id):
            rename_dir_stack = []

            if self.conn is None:
                return rename_dir_stack

            try:
                with self.conn:
                    c = self.conn.execute("SELECT rename_dir_stack FROM jobs WHERE id = ?", (job_id,))
                    record = c.fetchone()[0]
                    c.close()

                    if record:
                        for old_target, new_target in json.loads(record):
                            rename_dir_stack.append((Path(old_target), Path(new_target)))
            except sqlite3.OperationalError:
                pass

            return rename_dir_stack

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

        def get_skip_dir_stack(self, job_id):
            skip_dir_stack = []

            if self.conn is None:
                return skip_dir_stack

            try:
                with self.conn:
                    c = self.conn.execute("SELECT skip_dir_stack FROM jobs WHERE id = ?", (job_id,))
                    record = c.fetchone()[0]
                    c.close()

                    if record:
                        for dir_to_skip in json.loads(record):
                            skip_dir_stack.append(Path(dir_to_skip))
            except sqlite3.OperationalError:
                pass

            return skip_dir_stack

        def set_replace_first_path(self, job_id, replace_first_path):
            if self.conn is None:
                return

            try:
                with self.conn:
                    self.conn.execute("UPDATE jobs SET replace_first_path = ? WHERE id = ?", (
                        replace_first_path,
                        job_id,
                    ))
            except sqlite3.OperationalError:
                pass

        def get_replace_first_path(self, job_id):
            replace_first_path = None

            if self.conn is None:
                return replace_first_path

            try:
                with self.conn:
                    c = self.conn.execute("SELECT replace_first_path FROM jobs WHERE id = ?", (job_id,))
                    replace_first_path = c.fetchone()[0]
                    c.close()
            except sqlite3.OperationalError:
                pass

            return replace_first_path

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
