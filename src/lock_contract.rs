//! Non-blocking exclusive lock outcomes.
//!
//! `Acquired`, `Busy`, and `Failure` stay separate. A held range is not an
//! I/O error, and an I/O error is not reported as the lock being free.
//! The Linux session uses this for `flock`. The numbers for `LockFileEx`
//! are classified here so a Linux test can check the table. Those tests do
//! not call `LockFileEx`.

use std::io;

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug)]
pub enum LockAcquire {
    Acquired,
    Busy,
    Failure(io::Error),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockClass {
    Acquired,
    Busy,
    Failure,
}

/// `WinError.h`: another process has locked a portion of the file.
#[cfg_attr(not(windows), allow(dead_code))]
pub const ERROR_LOCK_VIOLATION: u32 = 33;

/// `WinError.h`: the process cannot access the file because it is being used
/// by another process. This is an open-sharing failure, not a byte-range lock.
#[allow(dead_code)]
pub const ERROR_SHARING_VIOLATION: u32 = 32;

/// `WinError.h`: overlapped I/O operation is in progress.
/// `LockFileEx` documents this for an asynchronous request that has not finished.
#[allow(dead_code)]
pub const ERROR_IO_PENDING: u32 = 997;

/// `rc == 0` is acquired. `EWOULDBLOCK` and `EAGAIN` are busy. Every other
/// result, including a missing errno, is a failure.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn classify_flock(rc: i32, raw_os_error: Option<i32>, busy_errnos: &[i32]) -> LockClass {
    if rc == 0 {
        LockClass::Acquired
    } else if raw_os_error.is_some_and(|code| busy_errnos.contains(&code)) {
        LockClass::Busy
    } else {
        LockClass::Failure
    }
}

/// A nonzero `LockFileEx` result is acquired. `ERROR_LOCK_VIOLATION` is busy.
/// `ERROR_SHARING_VIOLATION`, `ERROR_IO_PENDING`, and every other code stay failures.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn classify_lockfileex(succeeded: bool, win32_error: u32) -> LockClass {
    if succeeded {
        LockClass::Acquired
    } else if win32_error == ERROR_LOCK_VIOLATION {
        LockClass::Busy
    } else {
        LockClass::Failure
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_flock_result_is_acquired() {
        assert_eq!(classify_flock(0, Some(5), &[11, 35]), LockClass::Acquired);
        assert_eq!(classify_flock(0, None, &[11, 35]), LockClass::Acquired);
    }

    #[test]
    fn would_block_and_again_are_busy_and_other_errnos_are_failures() {
        assert_eq!(classify_flock(-1, Some(11), &[11, 35]), LockClass::Busy);
        assert_eq!(classify_flock(-1, Some(35), &[11, 35]), LockClass::Busy);
        assert_eq!(classify_flock(-1, Some(5), &[11, 35]), LockClass::Failure);
        assert_eq!(classify_flock(-1, None, &[11, 35]), LockClass::Failure);
    }

    #[test]
    fn lockfileex_success_is_acquired_and_only_lock_violation_is_busy() {
        assert_eq!(classify_lockfileex(true, 0), LockClass::Acquired);
        assert_eq!(
            classify_lockfileex(true, ERROR_LOCK_VIOLATION),
            LockClass::Acquired
        );
        assert_eq!(
            classify_lockfileex(false, ERROR_LOCK_VIOLATION),
            LockClass::Busy
        );
        assert_eq!(
            classify_lockfileex(false, ERROR_SHARING_VIOLATION),
            LockClass::Failure
        );
        assert_eq!(
            classify_lockfileex(false, ERROR_IO_PENDING),
            LockClass::Failure
        );
        assert_eq!(classify_lockfileex(false, 0), LockClass::Failure);
    }
}
