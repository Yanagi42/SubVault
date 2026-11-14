use std::fs::{File, OpenOptions};
use std::io::{self, Write};
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
        match crate::platform::try_lock_exclusive(&file) {
            crate::lock_contract::LockAcquire::Acquired => Ok(Some(Self { file })),
            crate::lock_contract::LockAcquire::Busy => Ok(None),
            crate::lock_contract::LockAcquire::Failure(err) => Err(err),
        }
    }
}

impl Drop for VaultLock {
    fn drop(&mut self) {
        crate::platform::unlock_exclusive(&self.file);
    }
}

pub fn sigstop() {
    crate::platform::raise_sigstop();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("subvault-{name}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn a_second_acquire_is_busy_until_the_first_lock_is_dropped() {
        let path = scratch("writer-lock");
        let first = VaultLock::try_acquire(&path)
            .expect("open")
            .expect("first acquire");
        assert!(VaultLock::try_acquire(&path)
            .expect("second open")
            .is_none());
        drop(first);
        let second = VaultLock::try_acquire(&path)
            .expect("reopen")
            .expect("acquire after the first lock is dropped");
        drop(second);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_parent_is_a_failure_not_a_busy_lock() {
        let path = scratch("missing-parent").join("writer.lock");
        match VaultLock::try_acquire(&path) {
            Err(error) => assert!(error.raw_os_error().is_some()),
            Ok(None) => panic!("missing parent was reported as a busy lock"),
            Ok(Some(_)) => panic!("missing parent acquired a lock"),
        }
    }
}
