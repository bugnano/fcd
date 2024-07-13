PRAGMA foreign_keys = ON;

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
    is_file INTEGER NOT NULL,
    is_dir INTEGER NOT NULL,
    is_symlink INTEGER NOT NULL,
    size INTEGER NOT NULL,
    mtime INTEGER NOT NULL,
    mtime_nsec INTEGER NOT NULL,
    mode INTEGER NOT NULL,
    uid INTEGER NOT NULL,
    gid INTEGER NOT NULL,
    status TEXT NOT NULL,
    message TEXT NOT NULL,

    warning TEXT NOT NULL,
    target_is_dir INTEGER NOT NULL,
    target_is_symlink INTEGER NOT NULL,
    cur_target TEXT,
    FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
) STRICT;

CREATE TABLE IF NOT EXISTS dir_list (
    id INTEGER NOT NULL PRIMARY KEY,
    job_id INTEGER NOT NULL,
    file_id INTEGER NOT NULL,
    cur_file TEXT NOT NULL,
    cur_target TEXT NOT NULL,
    new_dir INTEGER NOT NULL,
    FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE,
    FOREIGN KEY (file_id) REFERENCES files(id) ON DELETE CASCADE
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

CREATE TABLE IF NOT EXISTS kv (
    k TEXT NOT NULL PRIMARY KEY,
    v TEXT
) STRICT;
