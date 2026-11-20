//! Deterministic, bounded-memory static text extractor for PDF content streams.
//!
//! Preserves original PDF bytes untouched elsewhere; this module only reads
//! already-accepted PDF bytes and emits source-stated candidates with exact
//! 1-based `pdf_page`, stream object number, and extracted-stream byte offsets.
//! No OCR, no JavaScript, no external links, and no password guessing.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use crate::body;

pub const EXTRACTOR: &str = "subvault-pdf";
pub const EXTRACTOR_VERSION: &str = "1";
pub const NORMALIZATION: &str = "pdf-static-text-1";
pub const JOIN: &str = "pdf-page-lines-1";
pub const RULES_VERSION: &str = "stated-pdf-lines-1";
pub const OFFSET_UNIT: &str = "utf8_bytes_in_extracted_pdf_page_stream";
pub const MAX_PDF_BYTES: u64 = 8 * 1024 * 1024;
const MAX_STREAM_DECOMPRESSED: usize = 1024 * 1024;
const MAX_TOTAL_DECOMPRESSED: usize = 4 * 1024 * 1024;
const MAX_PAGES: usize = 256;
const MAX_LINE_BYTES: usize = 1024;
const MAX_CANDIDATES: usize = 64;

#[derive(Clone, Debug)]
pub struct PdfCandidate {
    pub kind: String,
    pub usable: bool,
    pub excerpt: String,
    pub value: Map<String, Value>,
    pub locator: Value,
}

#[derive(Clone, Debug)]
pub struct PdfExtraction {
    pub status: &'static str, // "matched" | "no_match" | "conflict" | "unsupported"
    pub reason: Option<&'static str>,
    pub page_count: usize,
    pub candidates: Vec<PdfCandidate>,
    pub adopt_amount_idx: Option<usize>,
    pub adopt_period_idx: Option<usize>,
}

#[derive(Clone, Debug)]
struct PdfObject<'a> {
    num: u32,
    dict: &'a [u8],
    stream: Option<&'a [u8]>,
}

#[derive(Clone, Debug)]
struct PdfLine {
    page: usize,
    line_no: usize,
    stream_object: u32,
    text: String,
    byte_start: usize,
    byte_end: usize,
}

pub fn has_valid_pdf_envelope(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF-") && find_subslice(bytes, b"%%EOF").is_some()
}

pub fn extract(bytes: &[u8]) -> PdfExtraction {
    if !has_valid_pdf_envelope(bytes) {
        return unsupported("invalid_pdf_header", 0);
    }

    let objects = parse_objects(bytes);
    if has_encrypt_marker(bytes, &objects) {
        return unsupported("encrypted_pdf", 0);
    }

    let pages = collect_page_streams(&objects);
    if pages.len() > MAX_PAGES {
        return unsupported("too_many_pages", pages.len());
    }

    let mut total_decompressed = 0usize;
    let mut lines = Vec::new();

    for (page_idx, stream_nums) in pages.iter().enumerate() {
        let page_num = page_idx + 1;
        let mut line_no = 0usize;
        let mut page_stream_offset = 0usize;

        for &stream_num in stream_nums {
            let Some(obj) = objects.get(&stream_num) else {
                continue;
            };
            let Some(raw_stream) = obj.stream else {
                continue;
            };
            let decoded = match decode_stream(obj.dict, raw_stream) {
                Ok(buf) => buf,
                Err(reason) => return unsupported(reason, pages.len()),
            };
            total_decompressed = total_decompressed.saturating_add(decoded.len());
            if total_decompressed > MAX_TOTAL_DECOMPRESSED {
                return unsupported("decompressed_budget_exceeded", pages.len());
            }

            for raw_line in extract_text_lines(&decoded) {
                let trimmed = raw_line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed.len() > MAX_LINE_BYTES {
                    return unsupported("line_over_budget", pages.len());
                }
                line_no += 1;
                let byte_start = page_stream_offset;
                let byte_end = byte_start + trimmed.len();
                page_stream_offset = byte_end + 1;
                lines.push(PdfLine {
                    page: page_num,
                    line_no,
                    stream_object: stream_num,
                    text: trimmed.to_string(),
                    byte_start,
                    byte_end,
                });
            }
        }
    }

    classify_lines(&lines, pages.len())
}

fn unsupported(reason: &'static str, page_count: usize) -> PdfExtraction {
    PdfExtraction {
        status: "unsupported",
        reason: Some(reason),
        page_count,
        candidates: Vec::new(),
        adopt_amount_idx: None,
        adopt_period_idx: None,
    }
}

fn has_encrypt_marker(bytes: &[u8], objects: &BTreeMap<u32, PdfObject<'_>>) -> bool {
    // Check trailer section after last `trailer` keyword
    if let Some(pos) = rfind_subslice(bytes, b"trailer") {
        if has_name_token(&bytes[pos..], b"/Encrypt") {
            return true;
        }
    }
    // Also check cross-reference stream / encryption dictionary objects
    objects
        .values()
        .any(|obj| has_name_token(obj.dict, b"/Encrypt"))
}

fn has_name_token(haystack: &[u8], token: &[u8]) -> bool {
    let mut start = 0;
    while let Some(pos) = find_subslice(&haystack[start..], token) {
        let abs = start + pos;
        let after = abs + token.len();
        if after >= haystack.len() || is_pdf_delimiter(haystack[after]) {
            return true;
        }
        start = abs + 1;
    }
    false
}

fn is_pdf_delimiter(b: u8) -> bool {
    matches!(
        b,
        b' ' | b'\t'
            | b'\r'
            | b'\n'
            | b'\x0c'
            | 0
            | b'('
            | b')'
            | b'<'
            | b'>'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b'/'
            | b'%'
    )
}

fn parse_objects(bytes: &[u8]) -> BTreeMap<u32, PdfObject<'_>> {
    let mut map = BTreeMap::new();
    let mut i = 0usize;
    let n = bytes.len();

    while i < n {
        // Look for `<num> <gen> obj` at start of file or after whitespace/delimiter
        if bytes[i].is_ascii_digit() && (i == 0 || is_pdf_delimiter(bytes[i - 1])) {
            if let Some((num, body_start)) = parse_obj_header(bytes, i) {
                if let Some(end_rel) = find_endobj(&bytes[body_start..]) {
                    let body = &bytes[body_start..body_start + end_rel];
                    let (dict, stream) = split_dict_and_stream(body);
                    map.insert(num, PdfObject { num, dict, stream });
                    i = body_start + end_rel + b"endobj".len();
                    continue;
                }
            }
        }
        i += 1;
    }

    map
}

fn parse_obj_header(bytes: &[u8], start: usize) -> Option<(u32, usize)> {
    let mut p = start;
    while p < bytes.len() && bytes[p].is_ascii_digit() {
        p += 1;
    }
    if p == start {
        return None;
    }
    let num: u32 = std::str::from_utf8(&bytes[start..p]).ok()?.parse().ok()?;
    let sp1 = p;
    while p < bytes.len() && matches!(bytes[p], b' ' | b'\t' | b'\r' | b'\n') {
        p += 1;
    }
    if p == sp1 {
        return None;
    }
    let gen_start = p;
    while p < bytes.len() && bytes[p].is_ascii_digit() {
        p += 1;
    }
    if p == gen_start {
        return None;
    }
    let sp2 = p;
    while p < bytes.len() && matches!(bytes[p], b' ' | b'\t' | b'\r' | b'\n') {
        p += 1;
    }
    if p == sp2 || !bytes[p..].starts_with(b"obj") {
        return None;
    }
    let after = p + 3;
    if after < bytes.len() && !is_pdf_delimiter(bytes[after]) {
        return None;
    }
    Some((num, after))
}

fn find_endobj(slice: &[u8]) -> Option<usize> {
    let mut start = 0;
    while let Some(pos) = find_subslice(&slice[start..], b"endobj") {
        let abs = start + pos;
        let before_ok = abs == 0 || is_pdf_delimiter(slice[abs - 1]);
        let after = abs + 6;
        let after_ok = after >= slice.len() || is_pdf_delimiter(slice[after]);
        if before_ok && after_ok {
            return Some(abs);
        }
        start = abs + 1;
    }
    None
}

fn split_dict_and_stream(body: &[u8]) -> (&[u8], Option<&[u8]>) {
    let mut start = 0;
    while let Some(pos) = find_subslice(&body[start..], b"stream") {
        let abs = start + pos;
        let before_ok = abs == 0 || is_pdf_delimiter(body[abs - 1]);
        let after = abs + 6;
        if before_ok && after < body.len() {
            let stream_start = if body[after..].starts_with(b"\r\n") {
                Some(after + 2)
            } else if body[after] == b'\n' {
                Some(after + 1)
            } else {
                None
            };
            if let Some(s_start) = stream_start {
                if let Some(end_pos) = rfind_subslice(&body[s_start..], b"endstream") {
                    let mut raw = &body[s_start..s_start + end_pos];
                    if raw.ends_with(b"\r\n") {
                        raw = &raw[..raw.len() - 2];
                    } else if raw.ends_with(b"\n") || raw.ends_with(b"\r") {
                        raw = &raw[..raw.len() - 1];
                    }
                    return (&body[..abs], Some(raw));
                }
            }
        }
        start = abs + 1;
    }
    (body, None)
}

fn collect_page_streams(objects: &BTreeMap<u32, PdfObject<'_>>) -> Vec<Vec<u32>> {
    // 1. Try walking /Catalog -> /Pages -> /Kids tree in deterministic document order
    let mut ordered_pages = Vec::new();
    let mut visited = BTreeSet::new();

    if let Some(catalog) = objects.values().find(|o| is_dict_type(o.dict, b"/Catalog")) {
        if let Some(pages_ref) = first_indirect_ref_after(catalog.dict, b"/Pages") {
            walk_page_tree(objects, pages_ref, &mut visited, &mut ordered_pages);
        }
    }

    // 2. Fallback: scan all /Type /Page objects in object-number order
    if ordered_pages.is_empty() {
        for obj in objects.values() {
            if is_dict_type(obj.dict, b"/Page") {
                ordered_pages.push(obj.num);
            }
        }
    }

    ordered_pages
        .into_iter()
        .map(|page_num| {
            objects
                .get(&page_num)
                .map(|page_obj| content_refs(page_obj.dict))
                .unwrap_or_default()
        })
        .collect()
}

fn walk_page_tree(
    objects: &BTreeMap<u32, PdfObject<'_>>,
    node_num: u32,
    visited: &mut BTreeSet<u32>,
    out: &mut Vec<u32>,
) {
    if !visited.insert(node_num) || out.len() > MAX_PAGES {
        return;
    }
    let Some(obj) = objects.get(&node_num) else {
        return;
    };
    if is_dict_type(obj.dict, b"/Page") {
        out.push(node_num);
        return;
    }
    for kid in indirect_refs_array_after(obj.dict, b"/Kids") {
        walk_page_tree(objects, kid, visited, out);
    }
}

fn is_dict_type(dict: &[u8], expected: &[u8]) -> bool {
    let mut start = 0;
    while let Some(pos) = find_subslice(&dict[start..], b"/Type") {
        let mut p = start + pos + 5;
        if p < dict.len() && !is_pdf_delimiter(dict[p]) {
            start += pos + 1;
            continue;
        }
        while p < dict.len() && matches!(dict[p], b' ' | b'\t' | b'\r' | b'\n') {
            p += 1;
        }
        if dict[p..].starts_with(expected) {
            let after = p + expected.len();
            if after >= dict.len() || is_pdf_delimiter(dict[after]) {
                return true;
            }
        }
        start += pos + 1;
    }
    false
}

fn first_indirect_ref_after(dict: &[u8], key: &[u8]) -> Option<u32> {
    let pos = find_key_value_start(dict, key)?;
    parse_indirect_ref(&dict[pos..]).map(|(num, _)| num)
}

fn content_refs(dict: &[u8]) -> Vec<u32> {
    let Some(pos) = find_key_value_start(dict, b"/Contents") else {
        return Vec::new();
    };
    let tail = &dict[pos..];
    if tail.starts_with(b"[") {
        parse_ref_list(tail)
    } else if let Some((num, _)) = parse_indirect_ref(tail) {
        vec![num]
    } else {
        Vec::new()
    }
}

fn indirect_refs_array_after(dict: &[u8], key: &[u8]) -> Vec<u32> {
    let Some(pos) = find_key_value_start(dict, key) else {
        return Vec::new();
    };
    let tail = &dict[pos..];
    if tail.starts_with(b"[") {
        parse_ref_list(tail)
    } else {
        Vec::new()
    }
}

fn find_key_value_start(dict: &[u8], key: &[u8]) -> Option<usize> {
    let mut start = 0;
    while let Some(pos) = find_subslice(&dict[start..], key) {
        let mut p = start + pos + key.len();
        if p < dict.len() && !is_pdf_delimiter(dict[p]) {
            start += pos + 1;
            continue;
        }
        while p < dict.len() && matches!(dict[p], b' ' | b'\t' | b'\r' | b'\n') {
            p += 1;
        }
        return Some(p);
    }
    None
}

fn parse_ref_list(slice: &[u8]) -> Vec<u32> {
    let Some(end) = slice.iter().position(|&b| b == b']') else {
        return Vec::new();
    };
    let mut p = 1usize;
    let mut refs = Vec::new();
    while p < end {
        while p < end && matches!(slice[p], b' ' | b'\t' | b'\r' | b'\n') {
            p += 1;
        }
        if let Some((num, consumed)) = parse_indirect_ref(&slice[p..end]) {
            refs.push(num);
            p += consumed;
        } else {
            p += 1;
        }
    }
    refs
}

fn parse_indirect_ref(slice: &[u8]) -> Option<(u32, usize)> {
    let mut p = 0usize;
    while p < slice.len() && slice[p].is_ascii_digit() {
        p += 1;
    }
    if p == 0 {
        return None;
    }
    let num: u32 = std::str::from_utf8(&slice[..p]).ok()?.parse().ok()?;
    let sp1 = p;
    while p < slice.len() && matches!(slice[p], b' ' | b'\t' | b'\r' | b'\n') {
        p += 1;
    }
    if p == sp1 {
        return None;
    }
    let g_start = p;
    while p < slice.len() && slice[p].is_ascii_digit() {
        p += 1;
    }
    if p == g_start {
        return None;
    }
    let sp2 = p;
    while p < slice.len() && matches!(slice[p], b' ' | b'\t' | b'\r' | b'\n') {
        p += 1;
    }
    if p == sp2 || p >= slice.len() || slice[p] != b'R' {
        return None;
    }
    let after = p + 1;
    if after < slice.len() && !is_pdf_delimiter(slice[after]) {
        return None;
    }
    Some((num, after))
}

fn decode_stream(dict: &[u8], raw: &[u8]) -> Result<Vec<u8>, &'static str> {
    let Some(val_start) = find_key_value_start(dict, b"/Filter") else {
        if raw.len() > MAX_STREAM_DECOMPRESSED {
            return Err("stream_over_budget");
        }
        return Ok(raw.to_vec());
    };
    let tail = &dict[val_start..];
    let is_flate = if tail.starts_with(b"/FlateDecode") {
        let after = b"/FlateDecode".len();
        after >= tail.len() || is_pdf_delimiter(tail[after])
    } else if tail.starts_with(b"[") {
        if let Some(end) = tail.iter().position(|&b| b == b']') {
            let inner = std::str::from_utf8(&tail[1..end]).unwrap_or("").trim();
            inner == "/FlateDecode"
        } else {
            false
        }
    } else {
        false
    };

    if !is_flate {
        return Err("unsupported_pdf_filter");
    }

    miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(raw, MAX_STREAM_DECOMPRESSED)
        .map_err(|_| "flate_decode_failed")
}

// --- PDF Content Stream Text Extraction (`BT ... ET`) ---

#[derive(Debug)]
enum Token {
    Str(String),
    Array(Vec<ArrayItem>),
    Num(f64),
    Op(String),
}

#[derive(Debug)]
enum ArrayItem {
    Str(String),
    Num(f64),
}

fn extract_text_lines(stream: &[u8]) -> Vec<String> {
    let tokens = tokenize_content_stream(stream);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_text = false;
    let mut operands: Vec<Token> = Vec::new();
    let mut last_tm_y: Option<f64> = None;

    let flush = |cur: &mut String, out: &mut Vec<String>| {
        let trimmed = cur.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
        cur.clear();
    };

    for tok in tokens {
        match tok {
            Token::Op(op) => {
                match op.as_str() {
                    "BT" => {
                        flush(&mut current, &mut lines);
                        in_text = true;
                        last_tm_y = None;
                    }
                    "ET" => {
                        flush(&mut current, &mut lines);
                        in_text = false;
                        last_tm_y = None;
                    }
                    "T*" if in_text => {
                        flush(&mut current, &mut lines);
                    }
                    "Td" | "TD" if in_text => {
                        if let Some(Token::Num(ty)) = operands.last() {
                            if ty.abs() > 0.01 {
                                flush(&mut current, &mut lines);
                            }
                        }
                    }
                    "Tm" if in_text => {
                        if let Some(Token::Num(y)) = operands.last() {
                            if let Some(prev_y) = last_tm_y {
                                if (y - prev_y).abs() > 0.5 {
                                    flush(&mut current, &mut lines);
                                }
                            }
                            last_tm_y = Some(*y);
                        }
                    }
                    "Tj" if in_text => {
                        if let Some(Token::Str(s)) = operands.last() {
                            current.push_str(s);
                        }
                    }
                    "'" | "\"" if in_text => {
                        flush(&mut current, &mut lines);
                        if let Some(Token::Str(s)) = operands.last() {
                            current.push_str(s);
                        }
                    }
                    "TJ" if in_text => {
                        if let Some(Token::Array(items)) = operands.last() {
                            for item in items {
                                match item {
                                    ArrayItem::Str(s) => current.push_str(s),
                                    ArrayItem::Num(offset) => {
                                        if *offset < -180.0
                                            && !current.is_empty()
                                            && !current.ends_with(' ')
                                        {
                                            current.push(' ');
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
                operands.clear();
            }
            other => {
                if operands.len() < 16 {
                    operands.push(other);
                }
            }
        }
    }
    flush(&mut current, &mut lines);
    lines
}

fn tokenize_content_stream(stream: &[u8]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let n = stream.len();

    while i < n {
        let b = stream[i];
        if matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | 0) {
            i += 1;
            continue;
        }
        // Comment
        if b == b'%' {
            while i < n && stream[i] != b'\n' && stream[i] != b'\r' {
                i += 1;
            }
            continue;
        }
        // Literal string (...)
        if b == b'(' {
            let (s, next) = parse_pdf_literal_string(stream, i);
            out.push(Token::Str(s));
            i = next;
            continue;
        }
        // Dictionary <<...>> or hex string <...>
        if b == b'<' {
            if i + 1 < n && stream[i + 1] == b'<' {
                i += 2;
                continue;
            }
            let (s, next) = parse_pdf_hex_string(stream, i);
            out.push(Token::Str(s));
            i = next;
            continue;
        }
        if b == b'>' && i + 1 < n && stream[i + 1] == b'>' {
            i += 2;
            continue;
        }
        // Array [...]
        if b == b'[' {
            let (items, next) = parse_pdf_array(stream, i);
            out.push(Token::Array(items));
            i = next;
            continue;
        }
        // Name /Foo — skip
        if b == b'/' {
            i += 1;
            while i < n && !is_pdf_delimiter(stream[i]) {
                i += 1;
            }
            continue;
        }
        // Single-char text operators ' or "
        if b == b'\'' || b == b'"' {
            out.push(Token::Op((b as char).to_string()));
            i += 1;
            continue;
        }
        // Number or operator word
        let start = i;
        while i < n && !is_pdf_delimiter(stream[i]) {
            i += 1;
        }
        if i == start {
            i += 1;
            continue;
        }
        let word = std::str::from_utf8(&stream[start..i]).unwrap_or("");
        if let Ok(num) = word.parse::<f64>() {
            out.push(Token::Num(num));
        } else if !word.is_empty() {
            out.push(Token::Op(word.to_string()));
        }
    }

    out
}

fn parse_pdf_literal_string(stream: &[u8], start: usize) -> (String, usize) {
    let mut i = start + 1;
    let mut depth = 1usize;
    let mut raw = Vec::new();
    while i < stream.len() && depth > 0 {
        let b = stream[i];
        if b == b'\\' && i + 1 < stream.len() {
            i += 1;
            match stream[i] {
                b'n' => raw.push(b'\n'),
                b'r' => raw.push(b'\r'),
                b't' => raw.push(b'\t'),
                b'b' => raw.push(0x08),
                b'f' => raw.push(0x0c),
                b'(' => raw.push(b'('),
                b')' => raw.push(b')'),
                b'\\' => raw.push(b'\\'),
                b'\r' => {
                    if i + 1 < stream.len() && stream[i + 1] == b'\n' {
                        i += 1;
                    }
                }
                b'\n' => {}
                c @ b'0'..=b'7' => {
                    let mut val = (c - b'0') as u32;
                    for _ in 0..2 {
                        if i + 1 < stream.len() && matches!(stream[i + 1], b'0'..=b'7') {
                            i += 1;
                            val = (val << 3) | ((stream[i] - b'0') as u32);
                        } else {
                            break;
                        }
                    }
                    raw.push((val & 0xff) as u8);
                }
                other => raw.push(other),
            }
            i += 1;
            continue;
        }
        if b == b'(' {
            depth += 1;
            raw.push(b);
        } else if b == b')' {
            depth -= 1;
            if depth > 0 {
                raw.push(b);
            }
        } else {
            raw.push(b);
        }
        i += 1;
    }
    (decode_pdf_bytes(&raw), i)
}

fn parse_pdf_hex_string(stream: &[u8], start: usize) -> (String, usize) {
    let mut i = start + 1;
    let mut nibbles = Vec::new();
    while i < stream.len() && stream[i] != b'>' {
        let b = stream[i];
        if let Some(v) = hex_nibble(b) {
            nibbles.push(v);
        }
        i += 1;
    }
    if i < stream.len() && stream[i] == b'>' {
        i += 1;
    }
    let mut bytes = Vec::with_capacity(nibbles.len().div_ceil(2));
    let mut k = 0;
    while k < nibbles.len() {
        let hi = nibbles[k];
        let lo = if k + 1 < nibbles.len() {
            nibbles[k + 1]
        } else {
            0
        };
        bytes.push((hi << 4) | lo);
        k += 2;
    }
    (decode_pdf_bytes(&bytes), i)
}

fn decode_pdf_bytes(raw: &[u8]) -> String {
    if raw.len() >= 2 && raw[0] == 0xFE && raw[1] == 0xFF {
        let units: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        Err(_) => raw.iter().map(|&b| b as char).collect(),
    }
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn parse_pdf_array(stream: &[u8], start: usize) -> (Vec<ArrayItem>, usize) {
    let mut i = start + 1;
    let mut items = Vec::new();
    while i < stream.len() && stream[i] != b']' {
        let b = stream[i];
        if matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | 0) {
            i += 1;
            continue;
        }
        if b == b'(' {
            let (s, next) = parse_pdf_literal_string(stream, i);
            items.push(ArrayItem::Str(s));
            i = next;
            continue;
        }
        if b == b'<' {
            let (s, next) = parse_pdf_hex_string(stream, i);
            items.push(ArrayItem::Str(s));
            i = next;
            continue;
        }
        let num_start = i;
        while i < stream.len() && !is_pdf_delimiter(stream[i]) {
            i += 1;
        }
        if i == num_start {
            i += 1;
            continue;
        }
        if let Ok(num) = std::str::from_utf8(&stream[num_start..i])
            .unwrap_or("")
            .parse::<f64>()
        {
            items.push(ArrayItem::Num(num));
        }
    }
    if i < stream.len() && stream[i] == b']' {
        i += 1;
    }
    (items, i)
}

// --- Epistemic Line Classification ---

fn classify_lines(lines: &[PdfLine], page_count: usize) -> PdfExtraction {
    let has_credit_label = lines.iter().any(|l| {
        l.text == "This Total is a credit-note total, not an amount paid."
            || l.text.eq_ignore_ascii_case("Document: credit note")
    });
    let has_order_label = lines.iter().any(|l| {
        l.text == "This Total is an order total whose payment nature is not stated."
            || l.text.eq_ignore_ascii_case("Document: order confirmation")
    });
    let has_refund_not_stated = lines
        .iter()
        .any(|l| l.text == "Receipt of the refund is not stated.");
    let total_count = lines
        .iter()
        .filter(|l| parse_usd_prefixed(&l.text, "Total USD ").is_some())
        .count();

    let mut candidates: Vec<PdfCandidate> = Vec::new();
    let mut amount_indices: Vec<(usize, String)> = Vec::new();
    let mut period_indices: Vec<(usize, String, String)> = Vec::new();

    for line in lines {
        if candidates.len() >= MAX_CANDIDATES {
            return unsupported("too_many_candidates", page_count);
        }
        let text = line.text.as_str();

        if let Some(amount) = parse_usd_prefixed(text, "Amount paid USD ") {
            let mut value = base_value("stated_amount_paid", false);
            value.insert("currency".into(), json!("USD"));
            value.insert("amount_text".into(), json!(amount));
            value.insert("payment_verified".into(), json!(false));
            let idx = candidates.len();
            amount_indices.push((idx, amount.to_string()));
            candidates.push(PdfCandidate {
                kind: "stated_amount_paid".into(),
                usable: true,
                excerpt: text.to_string(),
                value,
                locator: make_locator(line),
            });
            continue;
        }

        if let Some((start, end)) = parse_service_period(text) {
            let mut value = base_value("stated_service_period", false);
            value.insert("period_start".into(), json!(start));
            value.insert("period_end".into(), json!(end));
            let idx = candidates.len();
            period_indices.push((idx, start.to_string(), end.to_string()));
            candidates.push(PdfCandidate {
                kind: "stated_service_period".into(),
                usable: true,
                excerpt: text.to_string(),
                value,
                locator: make_locator(line),
            });
            continue;
        }

        if let Some(total) = parse_usd_prefixed(text, "Total USD ") {
            let (kind, semantic) = if total_count == 1 && has_credit_label && !has_order_label {
                ("credit_note_total", "credit_note_total")
            } else if total_count == 1 && has_order_label && !has_credit_label {
                ("unjudged_order_total", "unjudged_order_total")
            } else {
                ("unresolved_total", "unresolved_total")
            };
            let mut value = base_value(kind, false);
            value.insert("currency".into(), json!("USD"));
            value.insert("amount_text".into(), json!(total));
            value.insert("semantic".into(), json!(semantic));
            value.insert("payment_verified".into(), json!(false));
            if semantic == "credit_note_total" {
                value.insert(
                    "refund_received_stated".into(),
                    json!(!has_refund_not_stated),
                );
            }
            candidates.push(PdfCandidate {
                kind: kind.into(),
                usable: false,
                excerpt: text.to_string(),
                value,
                locator: make_locator(line),
            });
            continue;
        }

        if let Some(email) = text
            .strip_prefix("Account email: ")
            .or_else(|| text.strip_prefix("Buyer email: "))
        {
            let email = email.trim();
            if !email.is_empty() {
                let masked = email.contains('*');
                let kind = if masked {
                    "masked_account_email"
                } else {
                    "account_email"
                };
                let mut value = base_value(kind, false);
                value.insert("email".into(), json!(email));
                value.insert("masked".into(), json!(masked));
                value.insert("attributed".into(), json!(false));
                candidates.push(PdfCandidate {
                    kind: kind.into(),
                    usable: false,
                    excerpt: text.to_string(),
                    value,
                    locator: make_locator(line),
                });
                continue;
            }
        }

        if let Some(rest) = text.strip_prefix("The service remains usable through ") {
            if let Some(date) = rest.strip_suffix('.') {
                if is_iso_date(date) {
                    let mut value = base_value("cancellation_usable_through", false);
                    value.insert("usable_through".into(), json!(date));
                    value.insert("auto_renewal_cancelled".into(), json!(true));
                    candidates.push(PdfCandidate {
                        kind: "cancellation_usable_through".into(),
                        usable: false,
                        excerpt: text.to_string(),
                        value,
                        locator: make_locator(line),
                    });
                    continue;
                }
            }
        }

        if let Some(scope) = text.strip_prefix("License scope, original wording: ") {
            let scope = scope.trim();
            if !scope.is_empty() {
                let mut value = base_value("onetime_license_scope", false);
                value.insert("purchase_kind".into(), json!("one-time"));
                value.insert("verbatim_scope".into(), json!(scope));
                candidates.push(PdfCandidate {
                    kind: "onetime_license_scope".into(),
                    usable: false,
                    excerpt: text.to_string(),
                    value,
                    locator: make_locator(line),
                });
                continue;
            }
        }
    }

    // Resolve amount slot (adopt first if unambiguous; conflict if multiple distinct values)
    let mut has_conflict = false;
    let distinct_amounts: BTreeSet<&str> = amount_indices.iter().map(|(_, a)| a.as_str()).collect();
    let adopt_amount_idx = if distinct_amounts.len() == 1 {
        let first = amount_indices[0].0;
        for (idx, _) in amount_indices.iter().skip(1) {
            candidates[*idx].usable = false;
            candidates[*idx]
                .value
                .insert("reason".into(), json!("duplicate_statement"));
        }
        Some(first)
    } else if distinct_amounts.len() > 1 {
        has_conflict = true;
        for (idx, _) in &amount_indices {
            candidates[*idx].usable = false;
            candidates[*idx]
                .value
                .insert("reason".into(), json!("conflicting_statements"));
        }
        None
    } else {
        None
    };

    // Resolve period slot
    let distinct_periods: BTreeSet<(&str, &str)> = period_indices
        .iter()
        .map(|(_, s, e)| (s.as_str(), e.as_str()))
        .collect();
    let adopt_period_idx = if distinct_periods.len() == 1 {
        let first = period_indices[0].0;
        for (idx, _, _) in period_indices.iter().skip(1) {
            candidates[*idx].usable = false;
            candidates[*idx]
                .value
                .insert("reason".into(), json!("duplicate_statement"));
        }
        Some(first)
    } else if distinct_periods.len() > 1 {
        has_conflict = true;
        for (idx, _, _) in &period_indices {
            candidates[*idx].usable = false;
            candidates[*idx]
                .value
                .insert("reason".into(), json!("conflicting_statements"));
        }
        None
    } else {
        None
    };

    let (status, reason) = if has_conflict {
        ("conflict", Some("conflicting_statements"))
    } else if adopt_amount_idx.is_some() || adopt_period_idx.is_some() {
        ("matched", None)
    } else if !candidates.is_empty() {
        ("no_match", Some("no_adoptable_fact"))
    } else {
        ("no_match", Some("no_extractable_text"))
    };

    PdfExtraction {
        status,
        reason,
        page_count,
        candidates,
        adopt_amount_idx,
        adopt_period_idx,
    }
}

fn base_value(kind: &str, adopted: bool) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("type".into(), json!(kind));
    map.insert("statement".into(), json!("source_stated"));
    map.insert("purchase_id".into(), Value::Null);
    map.insert("account_id".into(), Value::Null);
    map.insert("relationship".into(), Value::Null);
    map.insert("observed_at".into(), Value::Null);
    map.insert("time_status".into(), json!("unknown"));
    map.insert("current_status".into(), json!("unknown"));
    map.insert("effective".into(), json!(false));
    map.insert("adopted".into(), json!(adopted));
    map
}

fn make_locator(line: &PdfLine) -> Value {
    json!({
        "offset_unit": OFFSET_UNIT,
        "normalization": NORMALIZATION,
        "join": JOIN,
        "pdf_page": line.page,
        "stream_object": line.stream_object,
        "line_no": line.line_no,
        "byte_start": line.byte_start,
        "byte_end": line.byte_end,
        "unicode_scalar_start": line.byte_start,
        "unicode_scalar_end": line.byte_end,
        "eml": Value::Null,
        "coordinates": Value::Null
    })
}

fn parse_usd_prefixed<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = text.strip_prefix(prefix)?;
    body::usd_amount_text(rest).then_some(rest)
}

fn parse_service_period(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("Service period: ")?;
    let (start, end) = rest.split_once(" through ")?;
    (is_iso_date(start) && is_iso_date(end)).then_some((start, end))
}

fn is_iso_date(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn rfind_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).rposition(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_pdf(stream: &[u8], flate: bool) -> Vec<u8> {
        let (filter_entry, body) = if flate {
            let compressed = miniz_oxide::deflate::compress_to_vec_zlib(stream, 6);
            ("/Filter /FlateDecode ", compressed)
        } else {
            ("", stream.to_vec())
        };
        let mut out = b"%PDF-1.4\n".to_vec();
        out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
        out.extend_from_slice(
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>\nendobj\n",
        );
        out.extend_from_slice(
            format!(
                "4 0 obj\n<< {filter_entry}/Length {} >>\nstream\n",
                body.len()
            )
            .as_bytes(),
        );
        out.extend_from_slice(&body);
        out.extend_from_slice(b"\nendstream\nendobj\n%%EOF\n");
        out
    }

    #[test]
    fn extracts_flate_compressed_tj_and_tj_array_pdf() {
        let content = b"BT\n[(Amount paid) -220 (USD 29.00)] TJ\nT* (Service period: 2026-01-01 through 2026-12-31) Tj\nET\n";
        let pdf = minimal_pdf(content, true);
        let res = extract(&pdf);
        assert_eq!(res.status, "matched");
        assert_eq!(res.page_count, 1);
        assert!(res.adopt_amount_idx.is_some());
        assert!(res.adopt_period_idx.is_some());
        let amt = &res.candidates[res.adopt_amount_idx.unwrap()];
        assert_eq!(amt.value["amount_text"], "29.00");
        assert_eq!(amt.locator["pdf_page"], 1);
    }

    #[test]
    fn detects_encrypted_pdf_without_guessing() {
        let mut pdf = minimal_pdf(b"BT (Amount paid USD 10.00) Tj ET\n", false);
        let trailer = b"trailer\n<< /Root 1 0 R /Encrypt 9 0 R >>\n%%EOF\n";
        pdf.extend_from_slice(trailer);
        let res = extract(&pdf);
        assert_eq!(res.status, "unsupported");
        assert_eq!(res.reason, Some("encrypted_pdf"));
        assert!(res.candidates.is_empty());
    }

    #[test]
    fn keeps_credit_note_total_unadopted() {
        let content = b"BT\n(Document: credit note) Tj\nT* (Total USD 15.99) Tj\nT* (This Total is a credit-note total, not an amount paid.) Tj\nT* (Receipt of the refund is not stated.) Tj\nET\n";
        let pdf = minimal_pdf(content, false);
        let res = extract(&pdf);
        assert_eq!(res.status, "no_match");
        assert!(res.adopt_amount_idx.is_none());
        assert_eq!(res.candidates.len(), 1);
        assert_eq!(res.candidates[0].kind, "credit_note_total");
        assert!(!res.candidates[0].usable);
    }
}
