use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::shutil::which;

#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub archive_file: PathBuf,
    pub temp_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ArchiveMounter {
    executable: PathBuf,
    archive_dirs: Vec<ArchiveEntry>,
}

impl ArchiveMounter {
    pub fn new() -> Option<ArchiveMounter> {
        let executable = which("archivefs").or_else(|| which("archivemount"));

        executable.map(|executable| ArchiveMounter { executable })
    }

    pub fn unarchive_path(file: &Path) -> PathBuf {
        //
    }
}
