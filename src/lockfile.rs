use std::io;
use std::path::Path;

pub const LOCK_TEXT: &str =
    "# This file is a flock target. Its text is not a lock, a lease, or a pid authority.\n";

#[cfg(target_os = "linux")]
pub struct VaultLock {
    file: std::fs::File,
}

#[cfg(target_os = "windows")]
pub struct VaultLock {
    _inner: crate::windows_lock::WindowsVaultLock,
}

impl VaultLock {
    /// Exclusive non-blocking OS lock (`flock` on Linux, `LockFileEx` on Windows).
    /// The file's text is never a lease: a dead pid or an old expiry written into
    /// it does not grant the lock, and a live pid written into it does not
    /// withhold the lock.
    #[cfg(target_os = "linux")]
    pub fn try_acquire(path: &Path) -> io::Result<Option<Self>> {
        use std::fs::OpenOptions;
        use std::io::Write;
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

    #[cfg(target_os = "windows")]
    pub fn try_acquire(path: &Path) -> io::Result<Option<Self>> {
        match crate::windows_lock::try_acquire(path) {
            crate::windows_lock::WindowsLock::Acquired(lock) => {
                lock.ensure_lock_text(LOCK_TEXT.as_bytes())?;
                Ok(Some(Self { _inner: lock }))
            }
            crate::windows_lock::WindowsLock::Busy => Ok(None),
            crate::windows_lock::WindowsLock::Failure(err) => Err(err),
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for VaultLock {
    fn drop(&mut self) {
        crate::platform::unlock_exclusive(&self.file);
    }
}

#[cfg(target_os = "linux")]
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
