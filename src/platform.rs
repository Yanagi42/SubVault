//! Tested Linux calls for the vault core.
//!
//! The session still takes the existing non-blocking flock and holds it until
//! the command returns. Directory identity is still this Linux device and
//! inode. Exclusive create and directory fsync still return their errors.
//! Nothing here chooses a Windows lock, a file id, or a UI session. Directory
//! publish inside one parent uses `renameat2(RENAME_NOREPLACE)` and does not
//! copy across volumes. The compile gate is `target_os = "linux"`. Android and other
//! Unix targets stop here, before these calls. `windows_lock` and
//! `windows_file_id` are separate `cfg(windows)` modules and are not called
//! from this module.

#[cfg(not(target_os = "linux"))]
compile_error!(
    "SubVault's tested OS calls are Linux only (target_os = \"linux\"). Other targets, including other Unix, are not tested and have no success stand-in."
);

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::{CString, OsStr};
    use std::fs::{self, File, OpenOptions};
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::os::unix::io::AsRawFd;
    use std::path::{Path, PathBuf};

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

    /// Publish `from` as `to` in one already-open parent directory.
    /// Both arguments are single directory entries. The only call is
    /// `renameat2(RENAME_NOREPLACE)` with that same parent handle. An existing
    /// destination is left untouched. There is no plain `rename` and no copy.
    pub fn rename_dir_noreplace(parent: &File, from: &OsStr, to: &OsStr) -> io::Result<()> {
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

    fn single_entry(name: &OsStr) -> io::Result<CString> {
        let bytes = name.as_bytes();
        if bytes.is_empty()
            || bytes == b"."
            || bytes == b".."
            || bytes.contains(&b'/')
            || bytes.contains(&b'\\')
            || bytes.contains(&0)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "directory entry must be a single non-dot name without a separator",
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
            let id = directory_device_and_inode(&upper).expect("upper id");
            assert_eq!(directory_device_and_inode(&linked).expect("link id"), id);
            assert_ne!(directory_device_and_inode(&lower).expect("lower id"), id);
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
            for name in ["a/b", "a\\b", "", ".", ".."] {
                let err = rename_dir_noreplace(&parent, OsStr::new("staging"), OsStr::new(name))
                    .expect_err(name);
                assert_eq!(classify_rename_dir(&err), RenameDirClass::Invalid, "{name}");
                assert!(err.raw_os_error().is_none(), "{name}: {err}");
            }
            assert!(dir.join("staging").is_dir());
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

#[cfg(target_os = "linux")]
pub use linux::{
    classify_rename_dir, directory_device_and_inode, exclusive_create_refused, filesystem_magic,
    final_directory_entry, fsync_dir, is_enospc, open_new_nofollow, raise_sigstop,
    rename_dir_noreplace, try_lock_exclusive, unlock_exclusive, RenameDirClass,
};
