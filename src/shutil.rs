use std::{
    env,
    ffi::CString,
    fs, io,
    mem::MaybeUninit,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

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
