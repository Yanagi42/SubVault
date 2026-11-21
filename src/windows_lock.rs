//! Windows byte-range lock. Compiled for `cfg(windows)` and used by `VaultLock`.
//!
//! The calls follow the documented signatures:
//! `CreateFileW` with `bInheritHandle = FALSE` and without
//! `FILE_FLAG_OVERLAPPED` (synchronous handle);
//! `LockFileEx` / `UnlockFileEx` on one fixed range, offset 0 length 1,
//! with `LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY`.
//! Share read and share write are set so a second open can reach `LockFileEx`.
//! A share mode of 0 fails the second open with `ERROR_SHARING_VIOLATION`
//! (32), which this contract keeps as `Failure`, not `Busy`.
//!
//! `LockFileEx` remarks: if a process exits with a region locked, or closes a
//! file that still has outstanding locks, the system unlocks them, but the
//! time depends on available resources. Drop calls `UnlockFileEx` before
//! `CloseHandle`.

use std::ffi::c_void;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use crate::lock_contract::{classify_lockfileex, LockClass};

const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const FILE_SHARE_READ: u32 = 0x0000_0001;
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
const OPEN_ALWAYS: u32 = 4;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
const LOCKFILE_FAIL_IMMEDIATELY: u32 = 0x0000_0001;
const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x0000_0002;
const LOCK_OFFSET: u32 = 0;
const LOCK_LENGTH_LOW: u32 = 1;
const LOCK_LENGTH_HIGH: u32 = 0;

#[repr(C)]
struct SecurityAttributes {
    n_length: u32,
    lp_security_descriptor: *mut c_void,
    b_inherit_handle: i32,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct OverlappedOffset {
    offset: u32,
    offset_high: u32,
}

#[repr(C)]
union OverlappedOffsetOrPointer {
    offset: OverlappedOffset,
    pointer: *mut c_void,
}

/// Documented `OVERLAPPED`: `Internal`, `InternalHigh`, the offset/pointer
/// union, then `hEvent`. `hEvent` stays null.
#[repr(C)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset_or_pointer: OverlappedOffsetOrPointer,
    h_event: *mut c_void,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<Overlapped>() == 32);

#[link(name = "kernel32")]
extern "system" {
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *const c_void,
        disposition: u32,
        flags: u32,
        template: *mut c_void,
    ) -> *mut c_void;
    fn LockFileEx(
        file: *mut c_void,
        flags: u32,
        reserved: u32,
        length_low: u32,
        length_high: u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn UnlockFileEx(
        file: *mut c_void,
        reserved: u32,
        length_low: u32,
        length_high: u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn GetFileSizeEx(file: *mut c_void, file_size: *mut i64) -> i32;
    fn WriteFile(
        file: *mut c_void,
        buffer: *const u8,
        bytes_to_write: u32,
        bytes_written: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn FlushFileBuffers(file: *mut c_void) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn GetLastError() -> u32;
}

fn invalid_handle() -> *mut c_void {
    (-1isize) as usize as *mut c_void
}

fn lock_overlapped() -> Overlapped {
    Overlapped {
        internal: 0,
        internal_high: 0,
        offset_or_pointer: OverlappedOffsetOrPointer {
            offset: OverlappedOffset {
                offset: LOCK_OFFSET,
                offset_high: 0,
            },
        },
        h_event: ptr::null_mut(),
    }
}

pub struct WindowsVaultLock {
    handle: *mut c_void,
}

// SAFETY: A Win32 file handle is process-scoped rather than thread-affine,
// and `WindowsVaultLock` owns the sole reference until `Drop` unlocks & closes it.
unsafe impl Send for WindowsVaultLock {}

impl WindowsVaultLock {
    /// Populate the informational lock banner only when we hold the exclusive
    /// byte-range lock and the newly created file is still empty.
    pub fn ensure_lock_text(&self, text: &[u8]) -> io::Result<()> {
        let mut size: i64 = 0;
        let ok = unsafe { GetFileSizeEx(self.handle, &mut size) };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        if size > 0 {
            return Ok(());
        }
        let mut offset = 0usize;
        while offset < text.len() {
            let chunk = &text[offset..];
            let to_write = chunk.len().min(u32::MAX as usize) as u32;
            let mut written: u32 = 0;
            let ok = unsafe {
                WriteFile(
                    self.handle,
                    chunk.as_ptr(),
                    to_write,
                    &mut written,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                let code = unsafe { GetLastError() };
                return Err(io::Error::from_raw_os_error(code as i32));
            }
            if written == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "failed to write writer.lock text",
                ));
            }
            offset += written as usize;
        }
        let flushed = unsafe { FlushFileBuffers(self.handle) };
        if flushed == 0 {
            let code = unsafe { GetLastError() };
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        Ok(())
    }
}

impl Drop for WindowsVaultLock {
    fn drop(&mut self) {
        let invalid = invalid_handle();
        if self.handle.is_null() || self.handle == invalid {
            return;
        }
        let mut overlapped = lock_overlapped();
        // BOOL is discarded. A failed unlock is not reported as success.
        let _unlock = unsafe {
            UnlockFileEx(
                self.handle,
                0,
                LOCK_LENGTH_LOW,
                LOCK_LENGTH_HIGH,
                &mut overlapped,
            )
        };
        // Close is the fallback. Its BOOL is discarded as well.
        let _closed = unsafe { CloseHandle(self.handle) };
        self.handle = invalid;
    }
}

pub enum WindowsLock {
    Acquired(WindowsVaultLock),
    Busy,
    Failure(io::Error),
}

pub fn try_acquire(path: &Path) -> WindowsLock {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return WindowsLock::Failure(io::Error::new(
            io::ErrorKind::InvalidInput,
            "lock path contains a nul",
        ));
    }
    wide.push(0);
    let security = SecurityAttributes {
        n_length: std::mem::size_of::<SecurityAttributes>() as u32,
        lp_security_descriptor: ptr::null_mut(),
        b_inherit_handle: 0,
    };
    // SAFETY: `wide` is NUL-terminated UTF-16. `security` is fully
    // initialized, with `bInheritHandle` FALSE and a null descriptor.
    // `FILE_ATTRIBUTE_NORMAL` does not include `FILE_FLAG_OVERLAPPED`.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &security as *const SecurityAttributes as *const c_void,
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    if handle.is_null() || handle == invalid_handle() {
        let code = unsafe { GetLastError() };
        return WindowsLock::Failure(io::Error::from_raw_os_error(code as i32));
    }
    let mut overlapped = lock_overlapped();
    // SAFETY: `handle` came from `CreateFileW` and is not invalid. The
    // overlapped value has `hEvent` null and the same fixed offset used later
    // by `UnlockFileEx`. Reserved is 0. Length is the constant one-byte range.
    let locked = unsafe {
        LockFileEx(
            handle,
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            LOCK_LENGTH_LOW,
            LOCK_LENGTH_HIGH,
            &mut overlapped,
        )
    };
    if locked == 0 {
        let code = unsafe { GetLastError() };
        let _closed = unsafe { CloseHandle(handle) };
        return match classify_lockfileex(false, code) {
            LockClass::Busy => WindowsLock::Busy,
            LockClass::Acquired | LockClass::Failure => {
                WindowsLock::Failure(io::Error::from_raw_os_error(code as i32))
            }
        };
    }
    match classify_lockfileex(true, 0) {
        LockClass::Acquired => WindowsLock::Acquired(WindowsVaultLock { handle }),
        LockClass::Busy | LockClass::Failure => {
            let mut overlapped = lock_overlapped();
            let _unlock = unsafe {
                UnlockFileEx(
                    handle,
                    0,
                    LOCK_LENGTH_LOW,
                    LOCK_LENGTH_HIGH,
                    &mut overlapped,
                )
            };
            let _closed = unsafe { CloseHandle(handle) };
            WindowsLock::Failure(io::Error::other(
                "lock classification rejected a successful LockFileEx",
            ))
        }
    }
}
