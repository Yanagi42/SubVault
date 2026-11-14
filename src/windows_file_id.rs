//! Windows file identity from an existing handle.
//!
//! Compiled only for `cfg(windows)`. `place_output` and
//! `directory_device_and_inode` do not call it. A non-Linux build still stops
//! in `platform` at `compile_error`, so this does not make a vault command
//! Ready and does not relax a formal write. Nothing here was compiled or run
//! on the Linux host that added it.
//!
//! The call is `GetFileInformationByHandleEx` with `FileIdInfo` (`0x12`),
//! which fills a `FILE_ID_INFO`: 8-byte volume serial and 16-byte
//! `FILE_ID_128`. The safe entry takes `BorrowedHandle<'_>`: the handle must
//! already be open, and only for the owner's borrow. A raw pointer can become
//! that type only through `BorrowedHandle::borrow_raw`, which is unsafe and
//! is not wrapped here. This function does not open a path, does not
//! lowercase a path, and does not decide whether a junction or an ancestor
//! alias was followed. Two identities compare equal only when the
//! volume serial and all 16 identifier bytes match. That comparison is not a
//! verified security-path, junction, or ancestor-alias result.
//!
//! A zero return is `GetLastError` and stays a failure. The buffer is not
//! read on failure, and a short identifier is not returned as an identity.

#![allow(dead_code)]

use std::ffi::c_void;
use std::io;
use std::os::windows::io::{AsRawHandle, BorrowedHandle};

use crate::file_identity::{
    classify_file_id_query, file_identity, FileIdInfo, IdentityClass, IdentityReject,
};

const FILE_ID_INFO_CLASS: i32 = 0x12;

#[link(name = "kernel32")]
extern "system" {
    fn GetFileInformationByHandleEx(
        file: *mut c_void,
        class: i32,
        out: *mut c_void,
        size: u32,
    ) -> i32;
    fn GetLastError() -> u32;
}

pub enum IdentityOfHandle {
    Identity(crate::file_identity::FileIdentity),
    Failure(io::Error),
}

pub fn identity_of_handle(handle: BorrowedHandle<'_>) -> IdentityOfHandle {
    let raw = handle.as_raw_handle();
    let mut info = FileIdInfo {
        volume_serial_number: 0,
        file_id: crate::file_identity::FileId128 {
            identifier: [0u8; 16],
        },
    };
    let size = std::mem::size_of::<FileIdInfo>() as u32;
    // SAFETY: `BorrowedHandle` keeps `raw` valid for this borrow. Std says
    // the value may be passed to FFI as an argument and must not be captured
    // or consumed. This call does not store or close `raw`. Null and -1 are
    // not treated as invalid: in `BorrowedHandle` both bit patterns can be
    // real handles. `info` is a full `FILE_ID_INFO`, `size` is its length,
    // and the class is `FileIdInfo` (`0x12`).
    let ok = unsafe {
        GetFileInformationByHandleEx(
            raw,
            FILE_ID_INFO_CLASS,
            &mut info as *mut FileIdInfo as *mut c_void,
            size,
        )
    };
    if ok == 0 {
        let code = unsafe { GetLastError() };
        return match classify_file_id_query(false, code) {
            IdentityClass::Failure | IdentityClass::Identity => {
                IdentityOfHandle::Failure(io::Error::from_raw_os_error(code as i32))
            }
        };
    }
    match classify_file_id_query(true, 0) {
        IdentityClass::Identity => {
            match file_identity(info.volume_serial_number, &info.file_id.identifier) {
                Ok(id) => IdentityOfHandle::Identity(id),
                Err(IdentityReject::Truncated) => IdentityOfHandle::Failure(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "file id is not 16 bytes",
                )),
            }
        }
        IdentityClass::Failure => IdentityOfHandle::Failure(io::Error::other(
            "file id classification rejected a successful query",
        )),
    }
}
