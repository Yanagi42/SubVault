use std::path::Path;

use crate::fsutil::sha256_file;
use crate::ids::canonical_object_relpath;

/// What is true about one object file. Callers decide the exit code.
#[derive(Clone, Debug)]
pub(crate) struct ObjectFacts {
    pub canonical_relpath: Option<String>,
    pub id_rejected: bool,
    pub relpath_differs: bool,
    pub missing: bool,
    pub hash_matches: bool,
    pub length_matches: bool,
    pub actual_hash: Option<String>,
    pub actual_len: Option<u64>,
    pub io_error: Option<String>,
}

pub(crate) fn object_facts(
    root: &Path,
    object_id: &str,
    stored_relpath: Option<&str>,
    expected_hash: &str,
    expected_len: i64,
) -> ObjectFacts {
    let Some(relpath) = canonical_object_relpath(object_id) else {
        return ObjectFacts {
            canonical_relpath: None,
            id_rejected: true,
            relpath_differs: false,
            missing: false,
            hash_matches: false,
            length_matches: false,
            actual_hash: None,
            actual_len: None,
            io_error: None,
        };
    };
    let relpath_differs = stored_relpath.is_some_and(|stored| stored != relpath);
    let path = root.join(&relpath);
    if !path.is_file() {
        return ObjectFacts {
            canonical_relpath: Some(relpath),
            id_rejected: false,
            relpath_differs,
            missing: true,
            hash_matches: false,
            length_matches: false,
            actual_hash: None,
            actual_len: None,
            io_error: None,
        };
    }
    match sha256_file(&path) {
        Ok((hash, len)) => ObjectFacts {
            canonical_relpath: Some(relpath),
            id_rejected: false,
            relpath_differs,
            missing: false,
            hash_matches: hash == expected_hash,
            length_matches: len as i64 == expected_len,
            actual_hash: Some(hash),
            actual_len: Some(len),
            io_error: None,
        },
        Err(err) => ObjectFacts {
            canonical_relpath: Some(relpath),
            id_rejected: false,
            relpath_differs,
            missing: false,
            hash_matches: false,
            length_matches: false,
            actual_hash: None,
            actual_len: None,
            io_error: Some(err.to_string()),
        },
    }
}
