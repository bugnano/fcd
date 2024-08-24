use std::{
    ffi::OsStr,
    fs,
    io::Read,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use anyhow::{anyhow, bail, Result};
use crossbeam_channel::{Receiver, Sender};

use path_clean::PathClean;
use tempfile::tempdir;
use wait_timeout::ChildExt;

use crate::shutil::which;

#[derive(Debug, Clone)]
pub enum ArchiveMounterCommand {
    GetExeName(Sender<String>),
    MountArchive(PathBuf, Sender<Result<PathBuf>>, Receiver<()>),
    UmountParents(Vec<PathBuf>, Sender<()>),
    UmountUnrelated(Vec<PathBuf>),
    UnarchivePath(PathBuf, Sender<PathBuf>),
    ArchivePath(PathBuf, Sender<PathBuf>),
    GetArchiveDirs(Sender<Vec<ArchiveEntry>>),
}

#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub archive_file: PathBuf,
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
                            cancel_rx,
                        ) => {
                            let _ = mount_archive_tx
                                .send(archive_mounter.mount_archive(&archive, cancel_rx));
                        }
                        ArchiveMounterCommand::UmountParents(parents, completed_tx) => {
                            archive_mounter.umount_parents(&parents);

                            let _ = completed_tx.send(());
                        }
                        ArchiveMounterCommand::UmountUnrelated(dirs) => {
                            archive_mounter.umount_unrelated(&dirs);
                        }
                        ArchiveMounterCommand::UnarchivePath(file, unarchive_path_tx) => {
                            let _ = unarchive_path_tx.send(archive_mounter.unarchive_path(&file));
                        }
                        ArchiveMounterCommand::ArchivePath(file, archive_path_tx) => {
                            let _ = archive_path_tx.send(archive_mounter.archive_path(&file));
                        }
                        ArchiveMounterCommand::GetArchiveDirs(archive_dirs_tx) => {
                            let _ = archive_dirs_tx.send(archive_mounter.get_archive_dirs());
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
    let (cancel_tx, cancel_rx) = crossbeam_channel::unbounded();

    command_tx
        .send(ArchiveMounterCommand::MountArchive(
            PathBuf::from(archive),
            mount_archive_tx,
            cancel_rx,
        ))
        .unwrap();

    (mount_archive_rx, cancel_tx)
}

pub fn umount_parents<T: AsRef<Path>>(command_tx: &Sender<ArchiveMounterCommand>, parents: &[T]) {
    let (completed_tx, completed_rx) = crossbeam_channel::unbounded();

    command_tx
        .send(ArchiveMounterCommand::UmountParents(
            parents
                .iter()
                .map(|file| PathBuf::from(file.as_ref()))
                .collect(),
            completed_tx,
        ))
        .unwrap();

    let _ = completed_rx.recv();
}

pub fn umount_unrelated<T: AsRef<Path>>(command_tx: &Sender<ArchiveMounterCommand>, dirs: &[T]) {
    command_tx
        .send(ArchiveMounterCommand::UmountUnrelated(
            dirs.iter()
                .map(|file| PathBuf::from(file.as_ref()))
                .collect(),
        ))
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

pub fn get_archive_dirs(command_tx: &Sender<ArchiveMounterCommand>) -> Vec<ArchiveEntry> {
    let (archive_dirs_tx, archive_dirs_rx) = crossbeam_channel::unbounded();

    command_tx
        .send(ArchiveMounterCommand::GetArchiveDirs(archive_dirs_tx))
        .unwrap();

    archive_dirs_rx.recv().unwrap()
}

pub fn unarchive_path_map(file: &Path, archive_dirs: &[ArchiveEntry]) -> PathBuf {
    archive_dirs
        .iter()
        .rev()
        .find(|entry| file.starts_with(&entry.archive_file))
        .map(|entry| {
            entry
                .temp_dir
                .join(file.strip_prefix(&entry.archive_file).unwrap())
        })
        .unwrap_or_else(|| PathBuf::from(file))
        .clean()
}

pub fn archive_path_map(file: &Path, archive_dirs: &[ArchiveEntry]) -> PathBuf {
    archive_dirs
        .iter()
        .rev()
        .find(|entry| file.starts_with(&entry.temp_dir))
        .map(|entry| {
            entry
                .archive_file
                .join(file.strip_prefix(&entry.temp_dir).unwrap())
        })
        .unwrap_or_else(|| PathBuf::from(file))
        .clean()
}

pub fn unarchive_parent_map(file: &Path, archive_dirs: &[ArchiveEntry]) -> PathBuf {
    match (file.parent(), file.file_name()) {
        (Some(parent), Some(file_name)) => unarchive_path_map(parent, archive_dirs).join(file_name),
        _ => PathBuf::from(file),
    }
}

pub fn archive_parent_map(file: &Path, archive_dirs: &[ArchiveEntry]) -> PathBuf {
    match (file.parent(), file.file_name()) {
        (Some(parent), Some(file_name)) => archive_path_map(parent, archive_dirs).join(file_name),
        _ => PathBuf::from(file),
    }
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

    pub fn mount_archive(&mut self, archive: &Path, cancel_rx: Receiver<()>) -> Result<PathBuf> {
        if let Some(entry) = self
            .archive_dirs
            .iter()
            .find(|entry| entry.archive_file == archive)
        {
            return Ok(entry.temp_dir.clone());
        }

        let temp_dir = tempdir()?.into_path();

        let child = Command::new(&self.executable)
            .args(["-o", "ro"])
            .args([archive.file_name().unwrap(), temp_dir.as_os_str()])
            .current_dir(self.unarchive_path(archive.parent().unwrap()))
            .stderr(Stdio::piped())
            .spawn();

        match child {
            Ok(mut child) => {
                loop {
                    match child.wait_timeout(Duration::from_millis(50)) {
                        Ok(None) => {
                            // Still mounting the archive, let's see if there's a Cancel request
                            if !cancel_rx.is_empty() {
                                let _ = child.kill();

                                let _ = Command::new("umount")
                                    .arg(&temp_dir)
                                    .current_dir(self.unarchive_path(archive.parent().unwrap()))
                                    .output();

                                let _ = fs::remove_dir(&temp_dir);

                                bail!("canceled");
                            }
                        }
                        Ok(Some(exit_status)) => {
                            break exit_status
                                .success()
                                .then_some(())
                                .ok_or_else(|| {
                                    let mut stderr = child.stderr.take().unwrap();
                                    let mut buf: Vec<u8> = Vec::new();

                                    stderr.read_to_end(&mut buf).unwrap_or(0);

                                    anyhow!("{}", OsStr::from_bytes(&buf).to_string_lossy())
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
                                        .current_dir(self.unarchive_path(archive.parent().unwrap()))
                                        .output();

                                    let _ = fs::remove_dir(&temp_dir);

                                    e
                                })
                        }
                        Err(e) => break Err(e.into()),
                    }
                }
            }
            Err(e) => Err(e.into()),
        }
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
                .current_dir(self.unarchive_path(archive.parent().unwrap()))
                .output();

            let _ = fs::remove_dir(&entry.temp_dir);

            // It's important to preserve the ordering of the Vec, so we can't use swap_remove here
            self.archive_dirs.remove(i);
        }
    }

    pub fn umount_parents<T: AsRef<Path>>(&mut self, parents: &[T]) {
        for file in parents.iter() {
            let archive_file = self.archive_path(file.as_ref());

            let archives_to_umount: Vec<PathBuf> = self
                .archive_dirs
                .iter()
                .rev()
                .filter(|&entry| entry.archive_file.starts_with(&archive_file))
                .map(|entry| entry.archive_file.clone())
                .collect();

            for archive in &archives_to_umount {
                self.umount_archive(archive);
            }
        }
    }

    pub fn umount_unrelated<T: AsRef<Path>>(&mut self, dirs: &[T]) {
        let archive_dirs: Vec<PathBuf> = dirs
            .iter()
            .map(|file| self.archive_path(file.as_ref()))
            .collect();

        let archives_to_keep: Vec<PathBuf> = self
            .archive_dirs
            .iter()
            .filter(|entry| {
                archive_dirs
                    .iter()
                    .any(|archive_file| archive_file.starts_with(&entry.archive_file))
            })
            .map(|entry| entry.archive_file.clone())
            .collect();

        let archives_to_umount: Vec<PathBuf> = self
            .archive_dirs
            .iter()
            .rev()
            .filter(|entry| !archives_to_keep.contains(&entry.archive_file))
            .map(|entry| entry.archive_file.clone())
            .collect();

        for archive in &archives_to_umount {
            self.umount_archive(archive);
        }
    }

    pub fn unarchive_path(&self, file: &Path) -> PathBuf {
        unarchive_path_map(file, &self.archive_dirs)
    }

    pub fn archive_path(&self, file: &Path) -> PathBuf {
        archive_path_map(file, &self.archive_dirs)
    }

    pub fn get_archive_dirs(&self) -> Vec<ArchiveEntry> {
        self.archive_dirs.clone()
    }
}

impl Drop for ArchiveMounter {
    fn drop(&mut self) {
        let archives_to_umount: Vec<PathBuf> = self
            .archive_dirs
            .iter()
            .rev()
            .map(|entry| entry.archive_file.clone())
            .collect();

        for archive in &archives_to_umount {
            self.umount_archive(archive);
        }
    }
}
