use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::io::AsRawFd;
use std::path::Path;

pub const LOCK_TEXT: &str =
    "# This file is a flock target. Its text is not a lock, a lease, or a pid authority.\n";

pub struct VaultLock {
    file: File,
}

impl VaultLock {
    /// Exclusive non-blocking flock. The file's text is never a lease:
    /// a dead pid or an old expiry written into it does not grant the lock,
    /// and a live pid written into it does not withhold the lock.
    pub fn try_acquire(path: &Path) -> io::Result<Option<Self>> {
        let existed = path.exists();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        if !existed {
            file.write_all(LOCK_TEXT.as_bytes())?;
            file.sync_all()?;
        }
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            Ok(Some(Self { file }))
        } else {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EWOULDBLOCK)
                || err.raw_os_error() == Some(libc::EAGAIN)
            {
                Ok(None)
            } else {
                Err(err)
            }
        }
    }
}

impl Drop for VaultLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

pub fn sigstop() {
    unsafe {
        libc::raise(libc::SIGSTOP);
    }
}
