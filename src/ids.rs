use std::io;

pub fn random_id(prefix: &str) -> io::Result<String> {
    let mut bytes = [0u8; 16];
    crate::platform::fill_random(&mut bytes)?;
    Ok(format!("{prefix}{}", hex_encode(&bytes)))
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn is_prefixed_id(value: &str, prefix: &str) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|rest| is_lower_hex(rest, 32))
}

pub fn is_command_id(value: &str) -> bool {
    if value.is_empty() || value.len() > 80 || value == "." || value == ".." {
        return false;
    }
    value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

pub fn is_media_type(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|b| b.is_ascii_graphic())
}

pub fn is_display_name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 200 && !value.contains(['\0', '/', '\\', '\n', '\r'])
}

pub fn canonical_object_relpath(object_id: &str) -> Option<String> {
    if is_prefixed_id(object_id, "obj_") {
        Some(format!("objects/{object_id}"))
    } else {
        None
    }
}
