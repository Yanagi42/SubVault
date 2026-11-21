//! Tested OS calls for the vault core (Linux and Windows).
//!
//! - Linux (`target_os = "linux"`): non-blocking `flock`, `(dev, ino)` directory
//!   identity, `O_CREAT|O_EXCL|O_NOFOLLOW`, `renameat2(RENAME_NOREPLACE)`,
//!   parent fd `fsync`, `statfs` NFS/CIFS magic refusal, and `/dev/urandom`.
//! - Windows (`target_os = "windows"`): non-blocking `CreateFileW` + `LockFileEx`
//!   (via `windows_lock`), 128-bit `GetFileInformationByHandleEx(FileIdInfo)`
//!   directory identity (via `windows_file_id`), `CREATE_NEW | FILE_FLAG_OPEN_REPARSE_POINT`,
//!   no-replace directory publish via `SetFileInformationByHandle(FileRenameInfo)`,
//!   `GetDriveTypeW` / UNC remote-filesystem refusal, and `BCryptGenRandom`.
//!
//! Other targets (macOS, Android, other Unix) still stop at `compile_error!`:
//! they are not tested and have no success stand-in.

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
compile_error!(
    "SubVault's tested OS calls are Linux (target_os = \"linux\") and Windows (target_os = \"windows\") only. Other targets are not tested and have no success stand-in."
);

/// How a failed directory publish is reported. `Exists` is an existing
/// directory entry. The other classes stay apart from each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenameDirClass {
    Exists,
    Unavailable,
    CrossDevice,
    Invalid,
    NoSpace,
    Permission,
    Busy,
    Other,
}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::{CString, OsStr};
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, Read};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::os::unix::io::AsRawFd;
    use std::path::{Path, PathBuf};

    use serde_json::{json, Map, Value};

    use super::RenameDirClass;

    const NFS_SUPER_MAGIC: i64 = 0x6969;
    const SMB_SUPER_MAGIC: i64 = 0xFF534D42;
    const SMB2_SUPER_MAGIC: i64 = 0xFE534D42;

    /// Linux directory identity: `(st_dev, st_ino)`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct DirectoryId(pub u64, pub u64);

    /// Already-open parent directory handle for publish.
    pub type DirHandle = File;

    pub fn fill_random(bytes: &mut [u8]) -> io::Result<()> {
        File::open("/dev/urandom")?.read_exact(bytes)
    }

    pub fn network_filesystem(magic: i64) -> bool {
        matches!(magic, NFS_SUPER_MAGIC | SMB_SUPER_MAGIC | SMB2_SUPER_MAGIC)
    }

    /// `LOCK_EX|LOCK_NB`. Acquired, busy (`EWOULDBLOCK` or `EAGAIN`), and any
    /// other errno stay three results. Success does not read `errno`.
    pub fn try_lock_exclusive(file: &File) -> crate::lock_contract::LockAcquire {
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            return crate::lock_contract::LockAcquire::Acquired;
        }
        let err = io::Error::last_os_error();
        match crate::lock_contract::classify_flock(
            rc,
            err.raw_os_error(),
            &[libc::EWOULDBLOCK, libc::EAGAIN],
        ) {
            crate::lock_contract::LockClass::Busy => crate::lock_contract::LockAcquire::Busy,
            crate::lock_contract::LockClass::Acquired
            | crate::lock_contract::LockClass::Failure => {
                crate::lock_contract::LockAcquire::Failure(err)
            }
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

    /// Open `parent` for one no-replace rename.
    pub fn open_dir_for_publish(parent: &Path) -> io::Result<DirHandle> {
        File::open(parent)
    }

    /// Parent directory already opened for one no-replace rename.
    /// The caller syncs this same file, then drops it.
    /// `Drop` closes the fd. It does not sync.
    pub struct PublishedDir {
        parent: DirHandle,
    }

    impl PublishedDir {
        pub fn hold(parent: DirHandle) -> Self {
            Self { parent }
        }

        /// `fstat` of this open file. This is not a path lookup.
        pub fn device_and_inode(&self) -> io::Result<(u64, u64)> {
            let meta = self.parent.metadata()?;
            Ok((meta.dev(), meta.ino()))
        }

        pub fn checkpoint_identity(&self, point: &mut Map<String, Value>) {
            if let Ok((dev, ino)) = self.device_and_inode() {
                point.insert("parent_dev".into(), json!(dev));
                point.insert("parent_ino".into(), json!(ino));
            }
        }

        /// `sync_all` on this already-open directory file.
        pub fn sync(&self) -> io::Result<()> {
            self.parent.sync_all()
        }
    }

    /// `sync_all` on a regular file opened by path.
    pub fn sync_file(path: &Path) -> io::Result<()> {
        let file = File::open(path)?;
        file.sync_all()
    }

    /// `sync_all` on a directory opened by path. A failure is returned.
    /// Publish does not use this. It syncs the `PublishedDir` it already holds.
    pub fn fsync_dir(path: &Path) -> io::Result<()> {
        let file = File::open(path)?;
        file.sync_all()
    }

    /// Device and inode from `metadata`, which follows a final symlink.
    /// Callers that need the directory entry itself resolve that first.
    pub fn directory_id(path: &Path) -> io::Result<DirectoryId> {
        let meta = fs::metadata(path)?;
        Ok(DirectoryId(meta.dev(), meta.ino()))
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

    #[cfg(test)]
    pub fn test_enospc_error() -> io::Error {
        io::Error::from_raw_os_error(libc::ENOSPC)
    }

    /// Publish `from` as `to` in one already-open parent directory.
    /// Both arguments are single directory entries. The only call is
    /// `renameat2(RENAME_NOREPLACE)` with that same parent handle. An existing
    /// destination is left untouched. There is no plain `rename` and no copy.
    pub fn rename_dir_noreplace(parent: &DirHandle, from: &OsStr, to: &OsStr) -> io::Result<()> {
        renameat2_noreplace(parent, from, parent, to)
    }

    fn renameat2_noreplace(
        old_dir: &File,
        old: &OsStr,
        new_dir: &File,
        new: &OsStr,
    ) -> io::Result<()> {
        let old_name = single_entry(old)?;
        let new_name = single_entry(new)?;
        let rc = unsafe {
            libc::renameat2(
                old_dir.as_raw_fd(),
                old_name.as_ptr(),
                new_dir.as_raw_fd(),
                new_name.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// One directory entry on this Linux. `/` and NUL are not name bytes.
    /// `.` and `..` are the dot entries. A backslash is an ordinary byte.
    fn single_entry(name: &OsStr) -> io::Result<CString> {
        let bytes = name.as_bytes();
        if bytes.is_empty()
            || bytes == b"."
            || bytes == b".."
            || bytes.contains(&b'/')
            || bytes.contains(&0)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "directory entry must be a single name other than . or ..",
            ));
        }
        CString::new(bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))
    }

    pub fn classify_rename_dir(err: &io::Error) -> RenameDirClass {
        match err.raw_os_error() {
            Some(code) if code == libc::EEXIST => RenameDirClass::Exists,
            Some(code) if code == libc::ENOSYS => RenameDirClass::Unavailable,
            Some(code) if code == libc::EXDEV => RenameDirClass::CrossDevice,
            Some(code) if code == libc::EINVAL => RenameDirClass::Invalid,
            Some(code) if code == libc::ENOSPC => RenameDirClass::NoSpace,
            Some(code) if code == libc::EACCES || code == libc::EPERM => RenameDirClass::Permission,
            Some(code) if code == libc::EBUSY => RenameDirClass::Busy,
            None if err.kind() == io::ErrorKind::InvalidInput => RenameDirClass::Invalid,
            _ => RenameDirClass::Other,
        }
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

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::ffi::{CString, OsStr};
        use std::io::Write;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::io::AsRawFd;
        use std::path::{Path, PathBuf};

        #[test]
        fn hard_linked_names_share_device_and_inode_and_distinct_names_do_not() {
            let dir = std::env::temp_dir().join(format!(
                "subvault-file-id-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            ));
            fs::create_dir(&dir).expect("temp dir");
            let upper = dir.join("Name");
            let mut created = File::create(&upper).expect("create");
            created.write_all(b"same-bytes").expect("write");
            drop(created);
            let linked = dir.join("other-name");
            fs::hard_link(&upper, &linked).expect("hard link");
            let lower = dir.join("name");
            File::create(&lower).expect("lower name");
            let id = directory_id(&upper).expect("upper id");
            assert_eq!(directory_id(&linked).expect("link id"), id);
            assert_ne!(directory_id(&lower).expect("lower id"), id);
            let _ = fs::remove_dir_all(&dir);
        }

        fn scratch_dir(label: &str) -> PathBuf {
            let dir = std::env::temp_dir().join(format!(
                "subvault-rename-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            ));
            fs::create_dir(&dir).expect("temp dir");
            dir
        }

        fn dev_ino(path: &Path) -> (u64, u64) {
            let meta = fs::symlink_metadata(path).expect("metadata");
            (meta.dev(), meta.ino())
        }

        #[test]
        fn noreplace_keeps_an_existing_entry_and_a_free_name_keeps_the_source_inode() {
            let dir = scratch_dir("same");
            let parent = File::open(&dir).expect("parent");
            fs::create_dir(dir.join("staging")).expect("staging");
            fs::write(dir.join("staging").join("SENTINEL"), b"staged\n").expect("sentinel");
            let staged = dev_ino(&dir.join("staging"));
            fs::create_dir(dir.join("empty")).expect("empty");
            let empty = dev_ino(&dir.join("empty"));
            let err = rename_dir_noreplace(&parent, OsStr::new("staging"), OsStr::new("empty"))
                .expect_err("empty directory exists");
            assert_eq!(classify_rename_dir(&err), RenameDirClass::Exists);
            assert_eq!(err.raw_os_error(), Some(libc::EEXIST));
            assert_eq!(dev_ino(&dir.join("empty")), empty);
            assert!(fs::read_dir(dir.join("empty")).unwrap().next().is_none());
            assert_eq!(
                fs::read(dir.join("staging").join("SENTINEL")).unwrap(),
                b"staged\n"
            );

            fs::create_dir(dir.join("full")).expect("full");
            fs::write(dir.join("full").join("KEEP"), b"competitor\n").expect("keep");
            let full = dev_ino(&dir.join("full"));
            let keep = dev_ino(&dir.join("full").join("KEEP"));
            let err = rename_dir_noreplace(&parent, OsStr::new("staging"), OsStr::new("full"))
                .expect_err("nonempty directory exists");
            assert_eq!(classify_rename_dir(&err), RenameDirClass::Exists);
            assert_eq!(dev_ino(&dir.join("full")), full);
            assert_eq!(dev_ino(&dir.join("full").join("KEEP")), keep);
            assert_eq!(
                fs::read(dir.join("full").join("KEEP")).unwrap(),
                b"competitor\n"
            );

            fs::write(dir.join("file"), b"file-keep\n").expect("file");
            let file = dev_ino(&dir.join("file"));
            let err = rename_dir_noreplace(&parent, OsStr::new("staging"), OsStr::new("file"))
                .expect_err("file exists");
            assert_eq!(classify_rename_dir(&err), RenameDirClass::Exists);
            assert_eq!(dev_ino(&dir.join("file")), file);
            assert_eq!(fs::read(dir.join("file")).unwrap(), b"file-keep\n");

            fs::write(dir.join("elsewhere"), b"target-keep\n").expect("target");
            std::os::unix::fs::symlink("elsewhere", dir.join("link")).expect("symlink");
            let link = dev_ino(&dir.join("link"));
            let elsewhere = dev_ino(&dir.join("elsewhere"));
            let err = rename_dir_noreplace(&parent, OsStr::new("staging"), OsStr::new("link"))
                .expect_err("symlink exists");
            assert_eq!(classify_rename_dir(&err), RenameDirClass::Exists);
            assert_eq!(dev_ino(&dir.join("link")), link);
            assert_eq!(
                fs::read_link(dir.join("link")).unwrap(),
                Path::new("elsewhere")
            );
            assert_eq!(dev_ino(&dir.join("elsewhere")), elsewhere);
            assert_eq!(fs::read(dir.join("elsewhere")).unwrap(), b"target-keep\n");
            assert_eq!(dev_ino(&dir.join("staging")), staged);

            rename_dir_noreplace(&parent, OsStr::new("staging"), OsStr::new("published"))
                .expect("free name");
            assert!(!dir.join("staging").exists());
            assert_eq!(dev_ino(&dir.join("published")), staged);
            assert_eq!(
                fs::read(dir.join("published").join("SENTINEL")).unwrap(),
                b"staged\n"
            );
            let _ = fs::remove_dir_all(&dir);
        }

        #[test]
        fn a_separator_or_dot_name_is_refused_before_a_publish() {
            let dir = scratch_dir("name");
            let parent = File::open(&dir).expect("parent");
            fs::create_dir(dir.join("staging")).expect("staging");
            fs::create_dir(dir.join("kept")).expect("kept");
            let kept = dev_ino(&dir.join("kept"));
            for name in ["a/b", "", ".", ".."] {
                let err = rename_dir_noreplace(&parent, OsStr::new("staging"), OsStr::new(name))
                    .expect_err(name);
                assert_eq!(classify_rename_dir(&err), RenameDirClass::Invalid, "{name}");
                assert!(err.raw_os_error().is_none(), "{name}: {err}");
            }
            let nul = std::ffi::OsStr::from_bytes(b"a\0b");
            let err = rename_dir_noreplace(&parent, OsStr::new("staging"), nul).expect_err("nul");
            assert_eq!(classify_rename_dir(&err), RenameDirClass::Invalid);
            assert!(err.raw_os_error().is_none(), "{err}");
            assert!(!dir.join("a").exists());
            assert!(dir.join("staging").is_dir());
            assert_eq!(dev_ino(&dir.join("kept")), kept);
            let staged = dev_ino(&dir.join("staging"));
            fs::write(dir.join("staging").join("SENTINEL"), b"slash-name\n").expect("sentinel");
            rename_dir_noreplace(&parent, OsStr::new("staging"), OsStr::new("a\\b"))
                .expect("backslash is one Linux name");
            let published = dir.join("a\\b");
            assert_eq!(dev_ino(&published), staged);
            assert_eq!(
                fs::read(published.join("SENTINEL")).unwrap(),
                b"slash-name\n"
            );
            assert!(!dir.join("a").exists());
            assert!(!dir.join("b").exists());
            assert_eq!(dev_ino(&dir.join("kept")), kept);
            let _ = fs::remove_dir_all(&dir);
        }

        #[test]
        fn cross_device_renameat2_keeps_both_trees_and_is_not_unavailable() {
            let source_root = scratch_dir("exdev-src");
            let dest_root = PathBuf::from(format!(
                "/dev/shm/subvault-rename-exdev-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            ));
            fs::create_dir(&dest_root).expect("shm dir");
            let source_id = dev_ino(&source_root);
            let dest_id = dev_ino(&dest_root);
            assert_ne!(
                source_id.0, dest_id.0,
                "this host did not provide two devices"
            );
            fs::create_dir(source_root.join("staging")).expect("staging");
            fs::write(source_root.join("staging").join("SENTINEL"), b"source\n").expect("source");
            fs::create_dir(dest_root.join("occupied")).expect("occupied");
            fs::write(dest_root.join("occupied").join("KEEP"), b"dest\n").expect("dest");
            let source_dir = File::open(&source_root).expect("source parent");
            let dest_dir = File::open(&dest_root).expect("dest parent");
            let err = renameat2_noreplace(
                &source_dir,
                OsStr::new("staging"),
                &dest_dir,
                OsStr::new("occupied"),
            )
            .expect_err("cross device");
            assert_eq!(err.raw_os_error(), Some(libc::EXDEV));
            assert_eq!(classify_rename_dir(&err), RenameDirClass::CrossDevice);
            assert_ne!(classify_rename_dir(&err), RenameDirClass::Unavailable);
            assert_eq!(
                fs::read(source_root.join("staging").join("SENTINEL")).unwrap(),
                b"source\n"
            );
            assert_eq!(
                fs::read(dest_root.join("occupied").join("KEEP")).unwrap(),
                b"dest\n"
            );
            assert!(!dest_root.join("staging").exists());
            let source_name = CString::new("staging").unwrap();
            let other_name = CString::new("occupied").unwrap();
            let bad = unsafe {
                libc::renameat2(
                    source_dir.as_raw_fd(),
                    source_name.as_ptr(),
                    source_dir.as_raw_fd(),
                    other_name.as_ptr(),
                    !0,
                )
            };
            assert_eq!(bad, -1);
            let invalid = io::Error::last_os_error();
            assert_eq!(invalid.raw_os_error(), Some(libc::EINVAL));
            assert_eq!(classify_rename_dir(&invalid), RenameDirClass::Invalid);
            assert_ne!(classify_rename_dir(&invalid), RenameDirClass::Unavailable);
            assert_eq!(
                fs::read(source_root.join("staging").join("SENTINEL")).unwrap(),
                b"source\n"
            );
            let _ = fs::remove_dir_all(&source_root);
            let _ = fs::remove_dir_all(&dest_root);
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::ffi::{c_void, OsStr, OsString};
    use std::fs::{self, File, OpenOptions};
    use std::io;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
    use std::path::{Path, PathBuf};
    use std::ptr;

    use serde_json::{json, Map, Value};

    use crate::file_identity::FileIdentity;
    use crate::windows_file_id::{identity_of_handle, IdentityOfHandle};

    use super::RenameDirClass;

    const FILE_LIST_DIRECTORY: u32 = 0x0000_0001;
    const FILE_ADD_SUBDIRECTORY: u32 = 0x0000_0004;
    const FILE_READ_ATTRIBUTES: u32 = 0x0000_0080;
    const DELETE_ACCESS: u32 = 0x0001_0000;
    const SYNCHRONIZE: u32 = 0x0010_0000;

    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;

    const OPEN_EXISTING: u32 = 3;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0000_0010;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

    const FILE_BASIC_INFO_CLASS: i32 = 0;
    const FILE_RENAME_INFO_CLASS: i32 = 3;
    const VOLUME_NAME_DOS: u32 = 0x0;

    const DRIVE_REMOTE: u32 = 4;

    const ERROR_INVALID_FUNCTION: i32 = 1;
    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_NOT_SAME_DEVICE: i32 = 17;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    const ERROR_LOCK_VIOLATION: i32 = 33;
    const ERROR_HANDLE_DISK_FULL: i32 = 39;
    const ERROR_NOT_SUPPORTED: i32 = 50;
    const ERROR_FILE_EXISTS: i32 = 80;
    const ERROR_INVALID_PARAMETER: i32 = 87;
    const ERROR_DISK_FULL: i32 = 112;
    const ERROR_INVALID_NAME: i32 = 123;
    const ERROR_BAD_PATHNAME: i32 = 161;
    const ERROR_BUSY: i32 = 170;
    const ERROR_ALREADY_EXISTS: i32 = 183;
    const ERROR_FILENAME_EXCED_RANGE: i32 = 206;
    const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;

    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;

    #[repr(C)]
    struct FileBasicInfo {
        creation_time: i64,
        last_access_time: i64,
        last_write_time: i64,
        change_time: i64,
        file_attributes: u32,
    }

    const _: () = assert!(std::mem::size_of::<FileBasicInfo>() == 40);

    #[repr(C)]
    struct FileRenameInfoRaw {
        flags_or_replace: u32,
        root_directory: *mut c_void,
        file_name_length: u32,
        file_name: [u16; 1],
    }

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
        fn FlushFileBuffers(file: *mut c_void) -> i32;
        fn GetFileInformationByHandleEx(
            file: *mut c_void,
            class: i32,
            out: *mut c_void,
            size: u32,
        ) -> i32;
        fn SetFileInformationByHandle(
            file: *mut c_void,
            class: i32,
            info: *const c_void,
            size: u32,
        ) -> i32;
        fn GetFinalPathNameByHandleW(
            file: *mut c_void,
            path: *mut u16,
            path_len: u32,
            flags: u32,
        ) -> u32;
        fn GetVolumePathNameW(
            file_name: *const u16,
            volume_path_name: *mut u16,
            buffer_length: u32,
        ) -> i32;
        fn GetDriveTypeW(root_path_name: *const u16) -> u32;
        fn GetLastError() -> u32;
    }

    #[link(name = "bcrypt")]
    extern "system" {
        fn BCryptGenRandom(
            algorithm: *mut c_void,
            buffer: *mut u8,
            buffer_len: u32,
            flags: u32,
        ) -> i32;
    }

    /// Windows directory identity: 8-byte volume serial + 16-byte `FILE_ID_128`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct DirectoryId(pub FileIdentity);

    /// Already-open parent directory handle for one no-replace rename.
    pub struct DirHandle {
        handle: OwnedHandle,
    }

    fn invalid_handle() -> *mut c_void {
        (-1isize) as usize as *mut c_void
    }

    fn encode_wide_nul(os: &OsStr) -> io::Result<Vec<u16>> {
        let mut wide: Vec<u16> = os.encode_wide().collect();
        if wide.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path contains an interior nul",
            ));
        }
        wide.push(0);
        Ok(wide)
    }

    fn open_handle(path: &Path, access: u32, flags: u32) -> io::Result<OwnedHandle> {
        let wide = encode_wide_nul(path.as_os_str())?;
        let raw = unsafe {
            CreateFileW(
                wide.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null(),
                OPEN_EXISTING,
                flags,
                ptr::null_mut(),
            )
        };
        if raw.is_null() || raw == invalid_handle() {
            let code = unsafe { GetLastError() };
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        // SAFETY: `CreateFileW` returned a fresh valid handle that we exclusively own.
        Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
    }

    fn handle_is_directory(handle: BorrowedHandle<'_>) -> io::Result<bool> {
        let mut info = FileBasicInfo {
            creation_time: 0,
            last_access_time: 0,
            last_write_time: 0,
            change_time: 0,
            file_attributes: 0,
        };
        let ok = unsafe {
            GetFileInformationByHandleEx(
                handle.as_raw_handle(),
                FILE_BASIC_INFO_CLASS,
                &mut info as *mut FileBasicInfo as *mut c_void,
                std::mem::size_of::<FileBasicInfo>() as u32,
            )
        };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        Ok((info.file_attributes & FILE_ATTRIBUTE_DIRECTORY) != 0)
    }

    fn final_path_of_handle(handle: BorrowedHandle<'_>) -> io::Result<Vec<u16>> {
        let mut cap: u32 = 260;
        loop {
            let mut buf = vec![0u16; cap as usize];
            let written = unsafe {
                GetFinalPathNameByHandleW(
                    handle.as_raw_handle(),
                    buf.as_mut_ptr(),
                    cap,
                    VOLUME_NAME_DOS,
                )
            };
            if written == 0 {
                let code = unsafe { GetLastError() };
                return Err(io::Error::from_raw_os_error(code as i32));
            }
            if written < cap {
                buf.truncate(written as usize);
                return Ok(buf);
            }
            cap = written + 1;
        }
    }

    fn flush_dir_handle(handle: BorrowedHandle<'_>) -> io::Result<()> {
        let ok = unsafe { FlushFileBuffers(handle.as_raw_handle()) };
        if ok != 0 {
            return Ok(());
        }
        let code = unsafe { GetLastError() } as i32;
        // Win32 `FlushFileBuffers` does not permit non-admin flushing of directory
        // handles (returns ERROR_ACCESS_DENIED or ERROR_INVALID_FUNCTION); NTFS
        // commits directory metadata updates through its `$LogFile` journal.
        if code == ERROR_ACCESS_DENIED || code == ERROR_INVALID_FUNCTION {
            return Ok(());
        }
        Err(io::Error::from_raw_os_error(code))
    }

    pub fn fill_random(bytes: &mut [u8]) -> io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let len = u32::try_from(bytes.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "random length overflow"))?;
        let status = unsafe {
            BCryptGenRandom(
                ptr::null_mut(),
                bytes.as_mut_ptr(),
                len,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "BCryptGenRandom failed with NTSTATUS 0x{:08x}",
                status as u32
            )))
        }
    }

    fn is_unc_wide(wide: &[u16]) -> bool {
        // Plain UNC: `\\server\...` (not `\\?\` and not `\\.\`)
        if wide.len() >= 3
            && wide[0] == b'\\' as u16
            && wide[1] == b'\\' as u16
            && wide[2] != b'?' as u16
            && wide[2] != b'.' as u16
        {
            return true;
        }
        // Verbatim UNC: `\\?\UNC\...` (case-insensitive)
        if wide.len() >= 8
            && wide[0] == b'\\' as u16
            && wide[1] == b'\\' as u16
            && wide[2] == b'?' as u16
            && wide[3] == b'\\' as u16
            && (wide[4] == b'U' as u16 || wide[4] == b'u' as u16)
            && (wide[5] == b'N' as u16 || wide[5] == b'n' as u16)
            && (wide[6] == b'C' as u16 || wide[6] == b'c' as u16)
            && wide[7] == b'\\' as u16
        {
            return true;
        }
        false
    }

    pub fn filesystem_magic(path: &Path) -> io::Result<i64> {
        let raw_wide = encode_wide_nul(path.as_os_str())?;
        if is_unc_wide(&raw_wide) {
            return Ok(i64::from(DRIVE_REMOTE));
        }
        let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let resolved_wide = encode_wide_nul(resolved.as_os_str())?;
        if is_unc_wide(&resolved_wide) {
            return Ok(i64::from(DRIVE_REMOTE));
        }
        let mut vol_buf = vec![0u16; resolved_wide.len().max(260) + 16];
        let ok = unsafe {
            GetVolumePathNameW(
                resolved_wide.as_ptr(),
                vol_buf.as_mut_ptr(),
                vol_buf.len() as u32,
            )
        };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        let drive_type = unsafe { GetDriveTypeW(vol_buf.as_ptr()) };
        Ok(i64::from(drive_type))
    }

    pub fn network_filesystem(magic: i64) -> bool {
        magic == i64::from(DRIVE_REMOTE)
    }

    /// Open `parent` as a directory handle for one no-replace rename.
    pub fn open_dir_for_publish(parent: &Path) -> io::Result<DirHandle> {
        let handle = open_handle(
            parent,
            FILE_LIST_DIRECTORY | FILE_ADD_SUBDIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_FLAG_BACKUP_SEMANTICS,
        )?;
        if !handle_is_directory(handle.as_handle())? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "publish parent is not a directory",
            ));
        }
        Ok(DirHandle { handle })
    }

    /// Parent directory already opened for one no-replace rename.
    pub struct PublishedDir {
        parent: DirHandle,
    }

    impl PublishedDir {
        pub fn hold(parent: DirHandle) -> Self {
            Self { parent }
        }

        pub fn checkpoint_identity(&self, point: &mut Map<String, Value>) {
            if let IdentityOfHandle::Identity(id) =
                identity_of_handle(self.parent.handle.as_handle())
            {
                point.insert("parent_volume_serial".into(), json!(id.volume_serial()));
                point.insert(
                    "parent_file_id".into(),
                    json!(crate::ids::hex_encode(id.file_id_bytes())),
                );
            }
        }

        pub fn sync(&self) -> io::Result<()> {
            flush_dir_handle(self.parent.handle.as_handle())
        }
    }

    /// Flush a regular file on Windows (`FlushFileBuffers` requires write access).
    pub fn sync_file(path: &Path) -> io::Result<()> {
        let file = OpenOptions::new().write(true).open(path)?;
        file.sync_all()
    }

    /// Best-effort directory flush on Windows (see `flush_dir_handle`).
    pub fn fsync_dir(path: &Path) -> io::Result<()> {
        let handle = open_handle(
            path,
            FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_FLAG_BACKUP_SEMANTICS,
        )?;
        if !handle_is_directory(handle.as_handle())? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "fsync_dir target is not a directory",
            ));
        }
        flush_dir_handle(handle.as_handle())
    }

    /// 128-bit NTFS/ReFS identity (`FILE_ID_INFO`) from a handle opened with
    /// `FILE_FLAG_BACKUP_SEMANTICS` (follows final symlinks/junctions, matching
    /// Linux `fs::metadata`).
    pub fn directory_id(path: &Path) -> io::Result<DirectoryId> {
        let handle = open_handle(path, FILE_READ_ATTRIBUTES, FILE_FLAG_BACKUP_SEMANTICS)?;
        match identity_of_handle(handle.as_handle()) {
            IdentityOfHandle::Identity(id) => Ok(DirectoryId(id)),
            IdentityOfHandle::Failure(err) => Err(err),
        }
    }

    /// `CREATE_NEW | FILE_FLAG_OPEN_REPARSE_POINT`. An existing file, directory,
    /// symlink, or dangling reparse point is not overwritten or followed.
    pub fn open_new_nofollow(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
    }

    pub fn exclusive_create_refused(err: &io::Error) -> bool {
        matches!(
            err.raw_os_error(),
            Some(ERROR_FILE_EXISTS | ERROR_ALREADY_EXISTS)
        )
    }

    pub fn is_enospc(err: &io::Error) -> bool {
        matches!(
            err.raw_os_error(),
            Some(ERROR_DISK_FULL | ERROR_HANDLE_DISK_FULL)
        )
    }

    #[cfg(test)]
    pub fn test_enospc_error() -> io::Error {
        io::Error::from_raw_os_error(ERROR_DISK_FULL)
    }

    /// Validate a single Windows directory entry name: rejects empty, `.`, `..`,
    /// `/`, `\`, `:`, and NUL.
    fn single_entry(name: &OsStr) -> io::Result<Vec<u16>> {
        let wide: Vec<u16> = name.encode_wide().collect();
        let dot = [b'.' as u16];
        let dotdot = [b'.' as u16, b'.' as u16];
        if wide.is_empty()
            || wide == dot
            || wide == dotdot
            || wide
                .iter()
                .any(|&u| u == 0 || u == b'/' as u16 || u == b'\\' as u16 || u == b':' as u16)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "directory entry must be a single Windows name other than . or ..",
            ));
        }
        Ok(wide)
    }

    fn join_wide_child(parent_wide: &[u16], child: &[u16]) -> Vec<u16> {
        let mut out = Vec::with_capacity(parent_wide.len() + 1 + child.len() + 1);
        out.extend_from_slice(parent_wide);
        if out.last().copied() != Some(b'\\' as u16) {
            out.push(b'\\' as u16);
        }
        out.extend_from_slice(child);
        out.push(0);
        out
    }

    fn set_rename_info(
        src_handle: BorrowedHandle<'_>,
        root_dir: *mut c_void,
        target_wide: &[u16],
    ) -> io::Result<()> {
        let header_offset = std::mem::offset_of!(FileRenameInfoRaw, file_name);
        let name_bytes = target_wide
            .len()
            .checked_mul(2)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target name too long"))?;
        let total_bytes = header_offset + (target_wide.len() + 1) * 2;
        let words = total_bytes.div_ceil(std::mem::size_of::<u64>());
        let mut backing = vec![0u64; words];
        let base = backing.as_mut_ptr() as *mut u8;
        // SAFETY: `backing` is 8-byte aligned and at least `total_bytes` long,
        // holding the `FileRenameInfoRaw` prefix plus NUL-terminated UTF-16 `target_wide`.
        unsafe {
            let hdr = base as *mut FileRenameInfoRaw;
            (*hdr).flags_or_replace = 0; // ReplaceIfExists = FALSE
            (*hdr).root_directory = root_dir;
            (*hdr).file_name_length = name_bytes;
            let dst = base.add(header_offset) as *mut u16;
            ptr::copy_nonoverlapping(target_wide.as_ptr(), dst, target_wide.len());
            *dst.add(target_wide.len()) = 0;
        }
        let ok = unsafe {
            SetFileInformationByHandle(
                src_handle.as_raw_handle(),
                FILE_RENAME_INFO_CLASS,
                base as *const c_void,
                total_bytes as u32,
            )
        };
        if ok != 0 {
            Ok(())
        } else {
            let code = unsafe { GetLastError() };
            Err(io::Error::from_raw_os_error(code as i32))
        }
    }

    /// Publish `from` as `to` inside one already-open parent directory on Windows
    /// without replacing any existing file, directory, symlink, or junction.
    ///
    /// Resolves the live path of `parent` via `GetFinalPathNameByHandleW` (so a
    /// moved parent stays bound to its open handle, not a replacement directory at
    /// the old path) and calls `SetFileInformationByHandle(FileRenameInfo)` with
    /// `ReplaceIfExists = FALSE`. There is no overwrite flag and no cross-volume copy.
    pub fn rename_dir_noreplace(parent: &DirHandle, from: &OsStr, to: &OsStr) -> io::Result<()> {
        let from_wide = single_entry(from)?;
        let to_wide = single_entry(to)?;
        let parent_live = final_path_of_handle(parent.handle.as_handle())?;
        let src_wide = join_wide_child(&parent_live, &from_wide);
        let raw_src = unsafe {
            CreateFileW(
                src_wide.as_ptr(),
                DELETE_ACCESS | SYNCHRONIZE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        if raw_src.is_null() || raw_src == invalid_handle() {
            let code = unsafe { GetLastError() };
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        // SAFETY: `CreateFileW` succeeded and returned an owned handle.
        let src_handle = unsafe { OwnedHandle::from_raw_handle(raw_src) };
        // Prefer relative-to-open-handle rename (`RootDirectory = parent`); if
        // `kernelbase` rejects non-NULL `RootDirectory` with `ERROR_INVALID_PARAMETER`,
        // fall back to full live path (`RootDirectory = NULL`, still `ReplaceIfExists = FALSE`).
        match set_rename_info(
            src_handle.as_handle(),
            parent.handle.as_raw_handle(),
            &to_wide,
        ) {
            Ok(()) => Ok(()),
            Err(err) if err.raw_os_error() == Some(ERROR_INVALID_PARAMETER) => {
                let mut dst_full = join_wide_child(&parent_live, &to_wide);
                dst_full.pop(); // `set_rename_info` appends the trailing NUL
                set_rename_info(src_handle.as_handle(), ptr::null_mut(), &dst_full)
            }
            Err(err) => Err(err),
        }
    }

    pub fn classify_rename_dir(err: &io::Error) -> RenameDirClass {
        match err.raw_os_error() {
            Some(ERROR_FILE_EXISTS | ERROR_ALREADY_EXISTS) => RenameDirClass::Exists,
            Some(ERROR_NOT_SUPPORTED | ERROR_INVALID_FUNCTION) => RenameDirClass::Unavailable,
            Some(ERROR_NOT_SAME_DEVICE) => RenameDirClass::CrossDevice,
            Some(
                ERROR_INVALID_NAME
                | ERROR_INVALID_PARAMETER
                | ERROR_BAD_PATHNAME
                | ERROR_FILENAME_EXCED_RANGE,
            ) => RenameDirClass::Invalid,
            Some(ERROR_DISK_FULL | ERROR_HANDLE_DISK_FULL) => RenameDirClass::NoSpace,
            Some(ERROR_ACCESS_DENIED | ERROR_PRIVILEGE_NOT_HELD) => RenameDirClass::Permission,
            Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION | ERROR_BUSY) => {
                RenameDirClass::Busy
            }
            None if err.kind() == io::ErrorKind::InvalidInput => RenameDirClass::Invalid,
            _ => RenameDirClass::Other,
        }
    }

    fn is_win_sep(u: u16) -> bool {
        u == b'\\' as u16 || u == b'/' as u16
    }

    /// Final directory entry for a Windows target, stripping redundant trailing
    /// slashes while preserving drive roots (`C:\`, `\\?\C:\`) and root `\`.
    pub fn final_directory_entry(path: &Path) -> PathBuf {
        let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        if wide.is_empty() {
            return path.to_path_buf();
        }
        // Determine the minimum prefix length we must not strip past.
        let min_len = if wide.len() >= 7
            && wide[0] == b'\\' as u16
            && wide[1] == b'\\' as u16
            && wide[2] == b'?' as u16
            && wide[3] == b'\\' as u16
            && wide[5] == b':' as u16
            && is_win_sep(wide[6])
        {
            7 // `\\?\C:\`
        } else if wide.len() >= 3 && wide[1] == b':' as u16 && is_win_sep(wide[2]) {
            3 // `C:\`
        } else if is_win_sep(wide[0]) {
            1 // `\` or `/`
        } else {
            0
        };
        let mut end = wide.len();
        while end > min_len && is_win_sep(wide[end - 1]) {
            end -= 1;
        }
        if end == wide.len() {
            path.to_path_buf()
        } else {
            PathBuf::from(OsString::from_wide(&wide[..end]))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::Write;

        fn scratch_dir(label: &str) -> PathBuf {
            let dir = std::env::temp_dir().join(format!(
                "subvault-win-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            ));
            fs::create_dir(&dir).expect("temp dir");
            dir
        }

        #[test]
        fn fill_random_produces_non_zero_bytes() {
            let mut a = [0u8; 16];
            let mut b = [0u8; 16];
            fill_random(&mut a).expect("rng a");
            fill_random(&mut b).expect("rng b");
            assert_ne!(a, [0u8; 16]);
            assert_ne!(a, b);
        }

        #[test]
        fn hard_links_and_case_aliases_share_128bit_file_identity() {
            let dir = scratch_dir("id");
            let upper = dir.join("Name.txt");
            let mut created = File::create(&upper).expect("create");
            created.write_all(b"same-bytes").expect("write");
            drop(created);
            let linked = dir.join("other-name.txt");
            fs::hard_link(&upper, &linked).expect("hard link");
            let distinct = dir.join("distinct.txt");
            File::create(&distinct).expect("distinct");

            let id_upper = directory_id(&upper).expect("upper id");
            let id_lower = directory_id(&dir.join("name.txt")).expect("case-insensitive alias");
            let id_linked = directory_id(&linked).expect("hard link id");
            let id_distinct = directory_id(&distinct).expect("distinct id");
            assert_eq!(id_upper, id_lower);
            assert_eq!(id_upper, id_linked);
            assert_ne!(id_upper, id_distinct);

            let _ = fs::remove_dir_all(&dir);
        }

        #[test]
        fn noreplace_keeps_existing_entries_case_aliases_and_preserves_staged_file_id() {
            let dir = scratch_dir("noreplace");
            let parent = open_dir_for_publish(&dir).expect("parent");
            fs::create_dir(dir.join("staging")).expect("staging");
            fs::write(dir.join("staging").join("SENTINEL"), b"staged\n").expect("sentinel");
            let staged_id = directory_id(&dir.join("staging")).expect("staged id");

            // 1. Existing empty directory
            fs::create_dir(dir.join("empty")).expect("empty");
            let empty_id = directory_id(&dir.join("empty")).expect("empty id");
            let err = rename_dir_noreplace(&parent, OsStr::new("staging"), OsStr::new("empty"))
                .expect_err("empty dir exists");
            assert_eq!(classify_rename_dir(&err), RenameDirClass::Exists, "{err}");
            assert_eq!(directory_id(&dir.join("empty")).unwrap(), empty_id);

            // 2. Case-different alias on case-insensitive NTFS
            let err = rename_dir_noreplace(&parent, OsStr::new("staging"), OsStr::new("EMPTY"))
                .expect_err("case alias exists");
            assert_eq!(classify_rename_dir(&err), RenameDirClass::Exists, "{err}");
            assert_eq!(directory_id(&dir.join("empty")).unwrap(), empty_id);

            // 3. Existing non-empty directory
            fs::create_dir(dir.join("full")).expect("full");
            fs::write(dir.join("full").join("KEEP"), b"competitor\n").expect("keep");
            let full_id = directory_id(&dir.join("full")).expect("full id");
            let err = rename_dir_noreplace(&parent, OsStr::new("staging"), OsStr::new("full"))
                .expect_err("full dir exists");
            assert_eq!(classify_rename_dir(&err), RenameDirClass::Exists, "{err}");
            assert_eq!(directory_id(&dir.join("full")).unwrap(), full_id);
            assert_eq!(
                fs::read(dir.join("full").join("KEEP")).unwrap(),
                b"competitor\n"
            );

            // 4. Existing regular file
            fs::write(dir.join("file"), b"file-keep\n").expect("file");
            let err = rename_dir_noreplace(&parent, OsStr::new("staging"), OsStr::new("file"))
                .expect_err("file exists");
            assert_eq!(classify_rename_dir(&err), RenameDirClass::Exists, "{err}");
            assert_eq!(fs::read(dir.join("file")).unwrap(), b"file-keep\n");

            // 5. Invalid single-entry names refused before syscall
            for bad in ["", ".", "..", "a\\b", "a/b", "a:b"] {
                let err = rename_dir_noreplace(&parent, OsStr::new("staging"), OsStr::new(bad))
                    .expect_err(bad);
                assert_eq!(classify_rename_dir(&err), RenameDirClass::Invalid, "{bad}");
            }

            // 6. Free name succeeds and preserves exact 128-bit FILE_ID_INFO
            rename_dir_noreplace(&parent, OsStr::new("staging"), OsStr::new("published"))
                .expect("free name");
            assert!(!dir.join("staging").exists());
            assert_eq!(directory_id(&dir.join("published")).unwrap(), staged_id);
            assert_eq!(
                fs::read(dir.join("published").join("SENTINEL")).unwrap(),
                b"staged\n"
            );

            let published = PublishedDir::hold(parent);
            published.sync().expect("published sync");
            drop(published);

            let _ = fs::remove_dir_all(&dir);
        }

        #[test]
        fn moved_parent_handle_publishes_inside_moved_directory() {
            let root = scratch_dir("moved-parent");
            let original = root.join("parent-a");
            let moved = root.join("parent-b");
            fs::create_dir(&original).expect("parent-a");
            fs::create_dir(original.join("staging")).expect("staging");
            fs::write(original.join("staging").join("SENTINEL"), b"moved-ok\n").expect("sentinel");

            // Open parent handle BEFORE moving the parent directory.
            let parent = open_dir_for_publish(&original).expect("open parent");
            fs::rename(&original, &moved).expect("move parent while handle open");

            // Create a replacement decoy directory at the old path.
            fs::create_dir(&original).expect("replacement decoy");
            fs::create_dir(original.join("staging")).expect("decoy staging");
            fs::write(original.join("staging").join("SENTINEL"), b"decoy\n").expect("decoy");

            // Publish via the held parent handle -> MUST rename inside `moved`, leaving `original` untouched.
            rename_dir_noreplace(&parent, OsStr::new("staging"), OsStr::new("published"))
                .expect("rename via moved handle");
            assert_eq!(
                fs::read(moved.join("published").join("SENTINEL")).unwrap(),
                b"moved-ok\n"
            );
            assert!(!original.join("published").exists());
            assert_eq!(
                fs::read(original.join("staging").join("SENTINEL")).unwrap(),
                b"decoy\n"
            );

            drop(parent);
            let _ = fs::remove_dir_all(&root);
        }

        #[test]
        fn final_directory_entry_strips_trailing_slashes_and_keeps_roots() {
            assert_eq!(
                final_directory_entry(Path::new(r"C:\Users\Public\vault\")),
                PathBuf::from(r"C:\Users\Public\vault")
            );
            assert_eq!(
                final_directory_entry(Path::new(r"C:\Users\Public\vault///")),
                PathBuf::from(r"C:\Users\Public\vault")
            );
            assert_eq!(
                final_directory_entry(Path::new(r"C:\")),
                PathBuf::from(r"C:\")
            );
            assert_eq!(
                final_directory_entry(Path::new(r"\\?\C:\")),
                PathBuf::from(r"\\?\C:\")
            );
            assert_eq!(
                final_directory_entry(Path::new(r"\\?\C:\vault\")),
                PathBuf::from(r"\\?\C:\vault")
            );
        }

        #[test]
        fn unc_paths_are_classified_as_network_filesystem() {
            assert!(network_filesystem(
                filesystem_magic(Path::new(r"\\127.0.0.1\share\vault")).expect("unc magic")
            ));
            assert!(network_filesystem(
                filesystem_magic(Path::new(r"\\?\UNC\127.0.0.1\share\vault"))
                    .expect("verbatim unc magic")
            ));
            let local = std::env::temp_dir();
            assert!(!network_filesystem(
                filesystem_magic(&local).expect("local drive magic")
            ));
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::{
    classify_rename_dir, directory_id, exclusive_create_refused, filesystem_magic, fill_random,
    final_directory_entry, fsync_dir, is_enospc, network_filesystem, open_dir_for_publish,
    open_new_nofollow, raise_sigstop, rename_dir_noreplace, sync_file, try_lock_exclusive,
    unlock_exclusive, DirectoryId, PublishedDir,
};

#[cfg(all(target_os = "linux", test))]
pub use linux::test_enospc_error;

#[cfg(target_os = "windows")]
pub use windows::{
    classify_rename_dir, directory_id, exclusive_create_refused, filesystem_magic, fill_random,
    final_directory_entry, fsync_dir, is_enospc, network_filesystem, open_dir_for_publish,
    open_new_nofollow, rename_dir_noreplace, sync_file, DirectoryId, PublishedDir,
};

#[cfg(all(target_os = "windows", test))]
pub use windows::test_enospc_error;
