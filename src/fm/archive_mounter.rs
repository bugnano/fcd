use std::{
    ffi::OsStr,
    fs,
    io::Read,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{anyhow, Result};

use path_clean::PathClean;
use tempfile::tempdir;

use crate::shutil::which;

#[derive(Debug, Clone)]
struct ArchiveEntry {
    archive_file: PathBuf,
    temp_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ArchiveMounter {
    executable: PathBuf,
    archive_dirs: Vec<ArchiveEntry>,
}

impl ArchiveMounter {
    pub fn new() -> Option<ArchiveMounter> {
        let executable = which("archivefs").or_else(|| which("archivemount"));

        executable.map(|executable| ArchiveMounter {
            executable,
            archive_dirs: Vec::new(),
        })
    }

    pub fn get_exe_name(&self) -> String {
        self.executable
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string()
    }

    pub fn mount_archive(&mut self, archive: &Path) -> Result<PathBuf> {
        if let Some(entry) = self
            .archive_dirs
            .iter()
            .find(|entry| entry.archive_file == archive)
        {
            return Ok(entry.temp_dir.clone());
        }

        let temp_dir = tempdir()?.into_path();

        Command::new(&self.executable)
            .args(["-o", "ro"])
            .args([archive.file_name().unwrap(), temp_dir.as_os_str()])
            .current_dir(&self.unarchive_path(archive.parent().unwrap()))
            .stderr(Stdio::piped())
            .spawn()
            .map_err(anyhow::Error::new)
            .and_then(|mut child| {
                child
                    .wait()
                    .map_err(anyhow::Error::new)
                    .and_then(|exit_status| {
                        exit_status.success().then_some(()).ok_or_else(|| {
                            let mut stderr = child.stderr.take().unwrap();
                            let mut buf: Vec<u8> = Vec::new();

                            stderr.read_to_end(&mut buf).unwrap_or(0);

                            anyhow!("{}", OsStr::from_bytes(&buf).to_string_lossy())
                        })
                    })
            })
            .map(|()| {
                self.archive_dirs.push(ArchiveEntry {
                    archive_file: PathBuf::from(archive),
                    temp_dir: temp_dir.clone(),
                });

                temp_dir.clone()
            })
            .map_err(|e| {
                let _ = Command::new("umount")
                    .arg(&temp_dir)
                    .current_dir(&self.unarchive_path(archive.parent().unwrap()))
                    .output();

                let _ = fs::remove_dir(&temp_dir);

                e
            })
    }

    pub fn umount_archive(&mut self, archive: &Path) {
        let pos_and_entry = self
            .archive_dirs
            .iter()
            .enumerate()
            .find(|(_i, entry)| entry.archive_file == archive);

        if let Some((i, entry)) = pos_and_entry {
            let _ = Command::new("umount")
                .arg(&entry.temp_dir)
                .current_dir(&self.unarchive_path(archive.parent().unwrap()))
                .output();

            let _ = fs::remove_dir(&entry.temp_dir);

            // It's important to preserve the ordering of the Vec, so we can't use swap_remove here
            self.archive_dirs.remove(i);
        }
    }

    pub fn unarchive_path(&self, file: &Path) -> PathBuf {
        self.archive_dirs
            .iter()
            .rev()
            .find(|entry| {
                file.ancestors()
                    .any(|ancestor| ancestor == entry.archive_file)
            })
            .map(|entry| {
                entry
                    .temp_dir
                    .join(file.strip_prefix(&entry.archive_file).unwrap())
            })
            .unwrap_or_else(|| PathBuf::from(file))
            .clean()
    }

    pub fn archive_path(&self, file: &Path) -> PathBuf {
        self.archive_dirs
            .iter()
            .rev()
            .find(|entry| file.ancestors().any(|ancestor| ancestor == entry.temp_dir))
            .map(|entry| {
                entry
                    .archive_file
                    .join(file.strip_prefix(&entry.temp_dir).unwrap())
            })
            .unwrap_or_else(|| PathBuf::from(file))
            .clean()
    }
}

impl Drop for ArchiveMounter {
    fn drop(&mut self) {
        let archives_to_unmount: Vec<PathBuf> = self
            .archive_dirs
            .iter()
            .rev()
            .map(|entry| entry.archive_file.clone())
            .collect();

        for archive in &archives_to_unmount {
            self.umount_archive(archive);
        }
    }
}
