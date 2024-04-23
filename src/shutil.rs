use std::{
    env,
    ffi::CString,
    fs, io,
    mem::MaybeUninit,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
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
pub fn disk_usage(path: &Path) -> Result<DiskUsage, io::Error> {
    let path_str = CString::new(path.as_os_str().as_encoded_bytes())?;

    let st = unsafe {
        let mut st = MaybeUninit::<libc::statvfs>::zeroed();

        if libc::statvfs(path_str.as_ptr(), st.as_mut_ptr()) != 0 {
            return Err(io::Error::last_os_error());
        }

        st.assume_init()
    };

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
                .and_then(|path| fs::metadata(path).ok())
                .map(|metadata| metadata.is_file() && (metadata.mode() & 0o111) != 0)
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
