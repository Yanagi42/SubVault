use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use crate::ids::hex_encode;
use sha2::{Digest, Sha256};

pub const MAX_OBJECT_BYTES: u64 = 64 * 1024 * 1024;

const NFS_SUPER_MAGIC: i64 = 0x6969;
const SMB_SUPER_MAGIC: i64 = 0xFF534D42;
const SMB2_SUPER_MAGIC: i64 = 0xFE534D42;

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

pub fn network_filesystem(magic: i64) -> bool {
    matches!(magic, NFS_SUPER_MAGIC | SMB_SUPER_MAGIC | SMB2_SUPER_MAGIC)
}

pub fn fsync_dir(path: &Path) -> io::Result<()> {
    let file = File::open(path)?;
    file.sync_all()
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let tmp_name = format!(
        ".{}.tmp",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("write")
    );
    let tmp = parent.join(tmp_name);
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    fsync_dir(parent)
}

pub fn sha256_file(path: &Path) -> io::Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = std::io::Read::read(&mut file, &mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    Ok((hex_encode(&hasher.finalize()), total))
}
