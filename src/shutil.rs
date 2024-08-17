use std::{
    env, fs,
    path::{Path, PathBuf},
};

use rustix::fs::{
    access, fchmod, ioctl_getflags, ioctl_setflags, lgetxattr, llistxattr, lsetxattr, lstat, open,
    statvfs, utimensat, Access, AtFlags, FileType, Mode, OFlags, Timespec, Timestamps, XattrFlags,
    CWD,
};
use uzers::{get_current_uid, get_user_by_name, get_user_by_uid, os::unix::UserExt};

#[derive(Debug, Clone, Copy)]
pub struct DiskUsage {
    /// Total space in bytes
    pub total: u64,

    /// Used space in bytes
    pub used: u64,

    /// Free space in bytes
    pub free: u64,
}

/// Return disk usage statistics about the given path.
pub fn disk_usage(path: &Path) -> rustix::io::Result<DiskUsage> {
    let st = statvfs(path)?;

    Ok(DiskUsage {
        total: st.f_blocks * st.f_frsize,
        used: (st.f_blocks - st.f_bfree) * st.f_frsize,
        free: st.f_bavail * st.f_frsize,
    })
}

pub fn which<T: AsRef<Path>>(cmd: T) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths).find_map(|mut path| {
            path.push(&cmd);

            let path = fs::canonicalize(&path).ok();

            let is_executable = path
                .as_deref()
                .and_then(|path| {
                    fs::metadata(path).ok().map(|metadata| {
                        metadata.is_file() && access(path, Access::EXISTS | Access::EXEC_OK).is_ok()
                    })
                })
                .unwrap_or(false);

            match is_executable {
                true => path,
                false => None,
            }
        })
    })
}

/// Expand paths beginning with '~' or '~user'.
pub fn expanduser(path: &Path) -> PathBuf {
    let str_path = path.to_string_lossy().to_string();

    if !str_path.starts_with('~') {
        return PathBuf::from(path);
    }

    let i = match str_path.find('/') {
        Some(i) => i,
        None => str_path.len(),
    };

    let mut userhome = match i {
        1 => match env::var_os("HOME") {
            Some(home_dir) => PathBuf::from(home_dir),
            None => match get_user_by_uid(get_current_uid()) {
                Some(user) => PathBuf::from(user.home_dir()),
                None => return PathBuf::from(path),
            },
        },
        i => match get_user_by_name(&str_path[1..i]) {
            Some(user) => PathBuf::from(user.home_dir()),
            None => return PathBuf::from(path),
        },
    };

    if i < str_path.len() {
        userhome.push(&str_path[i + 1..]);
    }

    match userhome.is_absolute() {
        true => userhome,
        false => PathBuf::from(path),
    }
}

/// Copy file metadata
pub fn copystat(src: &Path, dst: &Path) -> rustix::io::Result<()> {
    let st = lstat(src)?;

    utimensat(
        CWD,
        dst,
        &Timestamps {
            last_access: Timespec {
                tv_sec: st.st_atime as i64,
                tv_nsec: st.st_atime_nsec as i64,
            },
            last_modification: Timespec {
                tv_sec: st.st_mtime as i64,
                tv_nsec: st.st_mtime_nsec as i64,
            },
        },
        AtFlags::SYMLINK_NOFOLLOW,
    )?;

    // We must copy extended attributes before the file is (potentially)
    // chmod()'ed read-only, otherwise setxattr() will error with -EACCES.
    let mut names = vec![0; 65536];
    if let Ok(len_names) = llistxattr(src, &mut names) {
        let mut value = vec![0; 65536];

        names.resize(len_names, 0);
        for name in names.split(|c| *c == 0) {
            if !name.is_empty() {
                if let Ok(len_value) = lgetxattr(src, name, &mut value) {
                    let _ = lsetxattr(dst, name, &value[..len_value], XattrFlags::empty());
                }
            }
        }
    }

    if FileType::from_raw_mode(st.st_mode) != FileType::Symlink {
        let fi = open(src, OFlags::RDONLY | OFlags::NOFOLLOW, Mode::RUSR)?;
        let fo = open(dst, OFlags::RDONLY | OFlags::NOFOLLOW, Mode::RUSR)?;

        let _ = fchmod(&fo, Mode::from_raw_mode(st.st_mode));

        if let Ok(flags) = ioctl_getflags(&fi) {
            let _ = ioctl_setflags(&fo, flags);
        }
    }

    Ok(())
}
