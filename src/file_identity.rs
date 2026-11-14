//! Volume serial plus the full 128-bit file identifier.
//!
//! `FILE_ID_INFO` is an 8-byte `VolumeSerialNumber` and a `FILE_ID_128`
//! (`BYTE Identifier[16]`). `GetFileInformationByHandleEx` says a successful
//! call fills that struct, and a failed call returns zero; the only documented
//! follow-up is `GetLastError`. No failure code means "same file".
//!
//! The Linux command path does not call this module. Its directory identity
//! stays `(device, inode)` and is not converted into this value. Comparing
//! two of these values does not inspect a path, a junction, or an ancestor.

#![cfg_attr(not(windows), allow(dead_code))]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileIdentity {
    volume_serial: u64,
    file_id: [u8; 16],
}

#[repr(C)]
pub struct FileId128 {
    pub identifier: [u8; 16],
}

/// Documented `FILE_ID_INFO` layout. This Rust layout was not sized by a
/// Windows compiler.
#[repr(C)]
pub struct FileIdInfo {
    pub volume_serial_number: u64,
    pub file_id: FileId128,
}

const _: () = assert!(std::mem::size_of::<FileIdInfo>() == 24);
const _: () = assert!(std::mem::align_of::<FileIdInfo>() == 8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityReject {
    Truncated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityClass {
    Identity,
    Failure,
}

/// Accept only the full 16-byte identifier. A shorter or longer slice is
/// rejected, including an 8-byte file index.
pub fn file_identity(volume_serial: u64, file_id: &[u8]) -> Result<FileIdentity, IdentityReject> {
    if file_id.len() != 16 {
        return Err(IdentityReject::Truncated);
    }
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(file_id);
    Ok(FileIdentity {
        volume_serial,
        file_id: bytes,
    })
}

impl FileIdentity {
    pub fn same_file(self, other: Self) -> bool {
        self.volume_serial == other.volume_serial && self.file_id == other.file_id
    }
}

/// A nonzero `GetFileInformationByHandleEx` result is an identity query.
/// Every Win32 failure stays a failure. The code is not mapped to equality.
pub fn classify_file_id_query(succeeded: bool, _win32_error: u32) -> IdentityClass {
    if succeeded {
        IdentityClass::Identity
    } else {
        IdentityClass::Failure
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_volume_and_all_sixteen_bytes_are_the_same_file() {
        let id = [7u8, 1, 2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let left = file_identity(0x1111_2222_3333_4444, &id).expect("full id");
        let right = file_identity(0x1111_2222_3333_4444, &id).expect("full id");
        assert!(left.same_file(right));
    }

    #[test]
    fn a_high_byte_or_volume_difference_is_a_different_file() {
        let mut low = [1u8; 16];
        let mut high = [1u8; 16];
        high[15] = 2;
        let volume = 9;
        let left = file_identity(volume, &low).expect("full id");
        let right = file_identity(volume, &high).expect("full id");
        assert!(!left.same_file(right));
        low[0] = 3;
        let other_low = file_identity(volume, &low).expect("full id");
        assert!(!file_identity(volume, &[1u8; 16])
            .expect("full id")
            .same_file(other_low));
        let other_volume = file_identity(volume + 1, &[1u8; 16]).expect("full id");
        assert!(!file_identity(volume, &[1u8; 16])
            .expect("full id")
            .same_file(other_volume));
    }

    #[test]
    fn a_short_or_long_identifier_is_not_a_file_identity() {
        assert_eq!(file_identity(1, &[0u8; 8]), Err(IdentityReject::Truncated));
        assert_eq!(file_identity(1, &[0u8; 15]), Err(IdentityReject::Truncated));
        assert_eq!(file_identity(1, &[0u8; 17]), Err(IdentityReject::Truncated));
        assert_eq!(file_identity(1, &[]), Err(IdentityReject::Truncated));
    }

    #[test]
    fn a_failed_file_id_query_is_not_an_identity() {
        for code in [0u32, 1, 6, 87] {
            assert_eq!(classify_file_id_query(false, code), IdentityClass::Failure);
        }
        assert_eq!(classify_file_id_query(true, 87), IdentityClass::Identity);
    }
}
