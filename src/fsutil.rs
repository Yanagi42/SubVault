use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use crate::ids::hex_encode;
use sha2::{Digest, Sha256};

pub const MAX_OBJECT_BYTES: u64 = 64 * 1024 * 1024;

const NFS_SUPER_MAGIC: i64 = 0x6969;
const SMB_SUPER_MAGIC: i64 = 0xFF534D42;
const SMB2_SUPER_MAGIC: i64 = 0xFE534D42;

pub use crate::platform::filesystem_magic;

pub fn network_filesystem(magic: i64) -> bool {
    matches!(magic, NFS_SUPER_MAGIC | SMB_SUPER_MAGIC | SMB2_SUPER_MAGIC)
}

pub use crate::platform::fsync_dir;

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
