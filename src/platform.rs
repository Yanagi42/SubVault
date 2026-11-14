//! Tested Linux calls for the vault core.
//!
//! The session still takes the existing non-blocking flock and holds it until
//! the command returns. Directory identity is still this Linux device and
//! inode. Exclusive create and directory fsync still return their errors.
//! Nothing here chooses a Windows lock, a file id, a cross-volume rename, or
//! a UI session. The compile gate is `target_os = "linux"`. Android and other
//! Unix targets stop here, before these calls. There is no second backend.

#[cfg(not(target_os = "linux"))]
compile_error!(
    "SubVault's tested OS calls are Linux only (target_os = \"linux\"). Other targets, including other Unix, are not tested and have no success stand-in."
);

#[cfg(target_os = "linux")]
mod linux {
    use std::fs::{self, File, OpenOptions};
    use std::io;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::os::unix::io::AsRawFd;
    use std::path::{Path, PathBuf};

    /// `LOCK_EX|LOCK_NB`. `Ok(true)` is the lock. `Ok(false)` is a busy lock.
    /// Any other errno is returned.
    pub fn try_lock_exclusive(file: &File) -> io::Result<bool> {
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            return Ok(true);
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EWOULDBLOCK) || err.raw_os_error() == Some(libc::EAGAIN)
        {
            Ok(false)
        } else {
            Err(err)
        }
    }

    /// Best-effort unlock. The caller still closes the file afterward. Linux
    /// releases this flock when the last descriptor closes or the process dies.
    pub fn unlock_exclusive(file: &File) {
        unsafe {
            libc::flock(file.as_raw_fd(), libc::LOCK_UN);
        }
    }

    pub fn raise_sigstop() {
        unsafe {
            libc::raise(libc::SIGSTOP);
        }
    }

    pub fn filesystem_magic(path: &Path) -> io::Result<i64> {
        let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
        let mut buf: libc::statfs = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::statfs(c.as_ptr(), &mut buf) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(buf.f_type)
    }

    /// `sync_all` on the opened directory. A failure is returned.
    pub fn fsync_dir(path: &Path) -> io::Result<()> {
        let file = File::open(path)?;
        file.sync_all()
    }

    /// Device and inode from `metadata`, which follows a final symlink.
    /// Callers that need the directory entry itself resolve that first.
    pub fn directory_device_and_inode(path: &Path) -> io::Result<(u64, u64)> {
        let meta = fs::metadata(path)?;
        Ok((meta.dev(), meta.ino()))
    }

    /// `O_CREAT|O_EXCL|O_NOFOLLOW`. An existing name, including a dangling
    /// symlink, is not opened.
    pub fn open_new_nofollow(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
    }

    pub fn exclusive_create_refused(err: &io::Error) -> bool {
        matches!(
            err.raw_os_error(),
            Some(code) if code == libc::EEXIST || code == libc::ELOOP
        )
    }

    pub fn is_enospc(err: &io::Error) -> bool {
        err.raw_os_error() == Some(libc::ENOSPC)
    }

    /// Final directory entry for an init target. Trailing slashes are a
    /// spelling of that entry. The root path is only slashes, and it stays `/`.
    pub fn final_directory_entry(path: &Path) -> PathBuf {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let bytes = path.as_os_str().as_bytes();
        let end = bytes
            .iter()
            .rposition(|byte| *byte != b'/')
            .map(|index| index + 1)
            .unwrap_or(0);
        let trimmed = &bytes[..end];
        if trimmed.is_empty() {
            return PathBuf::from("/");
        }
        if trimmed.len() == bytes.len() {
            return path.to_path_buf();
        }
        PathBuf::from(std::ffi::OsString::from_vec(trimmed.to_vec()))
    }
}

#[cfg(target_os = "linux")]
pub use linux::{
    directory_device_and_inode, exclusive_create_refused, filesystem_magic, final_directory_entry,
    fsync_dir, is_enospc, open_new_nofollow, raise_sigstop, try_lock_exclusive, unlock_exclusive,
};
