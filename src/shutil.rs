use std::{ffi::CString, io, mem::MaybeUninit, path::Path};

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
