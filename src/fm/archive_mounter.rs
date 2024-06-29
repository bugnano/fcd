use std::{
    ffi::OsStr,
    fs,
    io::Read,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
};

use anyhow::{anyhow, Result};
use crossbeam_channel::{Receiver, Sender};

use path_clean::PathClean;
use tempfile::tempdir;

use crate::shutil::which;

#[derive(Debug, Clone)]
pub enum ArchiveMounterCommand {
    GetExeName(Sender<String>),
    MountArchive(PathBuf, Sender<Result<PathBuf>>, Receiver<()>),
    UmountArchive(PathBuf),
    UnarchivePath(PathBuf, Sender<PathBuf>),
    ArchivePath(PathBuf, Sender<PathBuf>),
}

#[derive(Debug, Clone)]
struct ArchiveEntry {
    archive_file: PathBuf,
    temp_dir: PathBuf,
}

#[derive(Debug, Clone)]
struct ArchiveMounter {
    executable: PathBuf,
    archive_dirs: Vec<ArchiveEntry>,
}

pub fn start() -> Option<Sender<ArchiveMounterCommand>> {
    match ArchiveMounter::new() {
        Some(mut archive_mounter) => {
            let (command_tx, command_rx) = crossbeam_channel::unbounded();
            thread::spawn(move || loop {
                match command_rx.recv() {
                    Ok(command) => match command {
                        ArchiveMounterCommand::GetExeName(exe_name_tx) => {
                            let _ = exe_name_tx.send(archive_mounter.get_exe_name());
                        }
                        ArchiveMounterCommand::MountArchive(
                            archive,
                            mount_archive_tx,
                            abort_rx,
                        ) => {
                            let _ = mount_archive_tx.send(archive_mounter.mount_archive(&archive));
                        }
                        ArchiveMounterCommand::UmountArchive(archive) => {
                            archive_mounter.umount_archive(&archive);
                        }
                        ArchiveMounterCommand::UnarchivePath(file, unarchive_path_tx) => {
                            let _ = unarchive_path_tx.send(archive_mounter.unarchive_path(&file));
                        }
                        ArchiveMounterCommand::ArchivePath(file, archive_path_tx) => {
                            let _ = archive_path_tx.send(archive_mounter.archive_path(&file));
                        }
                    },

                    // When the main thread exits, the channel returns an error
                    Err(_) => return,
                }
            });

            Some(command_tx)
        }
        None => None,
    }
}

pub fn get_exe_name(command_tx: &Sender<ArchiveMounterCommand>) -> String {
    let (exe_name_tx, exe_name_rx) = crossbeam_channel::unbounded();

    command_tx
        .send(ArchiveMounterCommand::GetExeName(exe_name_tx))
        .unwrap();

    exe_name_rx.recv().unwrap()
}

pub fn mount_archive(
    command_tx: &Sender<ArchiveMounterCommand>,
    archive: &Path,
) -> (Receiver<Result<PathBuf>>, Sender<()>) {
    let (mount_archive_tx, mount_archive_rx) = crossbeam_channel::unbounded();
    let (abort_tx, abort_rx) = crossbeam_channel::unbounded();

    command_tx
        .send(ArchiveMounterCommand::MountArchive(
            PathBuf::from(archive),
            mount_archive_tx,
            abort_rx,
        ))
        .unwrap();

    (mount_archive_rx, abort_tx)
}

pub fn umount_archive(command_tx: &Sender<ArchiveMounterCommand>, archive: &Path) {
    command_tx
        .send(ArchiveMounterCommand::UmountArchive(PathBuf::from(archive)))
        .unwrap();
}

pub fn unarchive_path(command_tx: &Sender<ArchiveMounterCommand>, file: &Path) -> PathBuf {
    let (unarchive_path_tx, unarchive_path_rx) = crossbeam_channel::unbounded();

    command_tx
        .send(ArchiveMounterCommand::UnarchivePath(
            PathBuf::from(file),
            unarchive_path_tx,
        ))
        .unwrap();

    unarchive_path_rx.recv().unwrap()
}

pub fn archive_path(command_tx: &Sender<ArchiveMounterCommand>, file: &Path) -> PathBuf {
    let (archive_path_tx, archive_path_rx) = crossbeam_channel::unbounded();

    command_tx
        .send(ArchiveMounterCommand::ArchivePath(
            PathBuf::from(file),
            archive_path_tx,
        ))
        .unwrap();

    archive_path_rx.recv().unwrap()
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
