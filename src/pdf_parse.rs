//! Deterministic, bounded-memory static text extractor for PDF content streams.
//!
//! Preserves original PDF bytes untouched elsewhere; this module only reads
//! already-accepted PDF bytes and emits source-stated candidates with exact
//! 1-based `pdf_page`, stream object number, and extracted-stream byte offsets.
//! No OCR, no JavaScript, no external links, and no password guessing.

use std::borrow::Cow;
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
const MAX_XOBJECT_DEPTH: usize = 8;
const MAX_GS_DEPTH: usize = 128;
const MAX_REF_DEPTH: usize = 8;
const MAX_CMAP_MAPPINGS: usize = 65_536;
const MAX_OBJSTM_OBJECTS: usize = 65_536;
const IDENTITY_MAT: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

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
    dict: Cow<'a, [u8]>,
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

#[derive(Default)]
struct DecodeBudget {
    total: usize,
}

impl DecodeBudget {
    fn account(&mut self, len: usize) -> Result<(), &'static str> {
        self.total = self.total.saturating_add(len);
        if self.total > MAX_TOTAL_DECOMPRESSED {
            return Err("decompressed_budget_exceeded");
        }
        Ok(())
    }
}

struct PageSpec {
    content_streams: Vec<u32>,
    resources_chain: Vec<u32>,
}

struct ExtractedPage {
    page_num: usize,
    stream_object: u32,
    lines: Vec<String>,
}

pub fn has_valid_pdf_envelope(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF-") && find_subslice(bytes, b"%%EOF").is_some()
}

pub fn extract_pages_for_redaction(
    bytes: &[u8],
) -> Result<Vec<(usize, Vec<String>)>, &'static str> {
    let pages = extract_all_pages(bytes).map_err(|(reason, _)| reason)?;
    Ok(pages
        .into_iter()
        .map(|page| (page.page_num, page.lines))
        .collect())
}

pub fn extract(bytes: &[u8]) -> PdfExtraction {
    let pages = match extract_all_pages(bytes) {
        Ok(pages) => pages,
        Err((reason, page_count)) => return unsupported(reason, page_count),
    };
    let page_count = pages.len();
    let mut lines = Vec::new();

    for page in pages {
        let mut page_stream_offset = 0usize;
        for (idx, text) in page.lines.into_iter().enumerate() {
            let byte_start = page_stream_offset;
            let byte_end = byte_start + text.len();
            page_stream_offset = byte_end + 1;
            lines.push(PdfLine {
                page: page.page_num,
                line_no: idx + 1,
                stream_object: page.stream_object,
                text,
                byte_start,
                byte_end,
            });
        }
    }

    classify_lines(&lines, page_count)
}

fn extract_all_pages(bytes: &[u8]) -> Result<Vec<ExtractedPage>, (&'static str, usize)> {
    if !has_valid_pdf_envelope(bytes) {
        return Err(("invalid_pdf_header", 0));
    }

    let (mut objects, objstm_nums) = parse_objects(bytes);
    if has_encrypt_marker(bytes, &objects) {
        return Err(("encrypted_pdf", 0));
    }

    let mut budget = DecodeBudget::default();
    if let Err(reason) = unpack_object_streams(&mut objects, &objstm_nums, &mut budget) {
        return Err((reason, 0));
    }

    let pages = collect_pages(&objects);
    let page_count = pages.len();
    if page_count > MAX_PAGES {
        return Err(("too_many_pages", page_count));
    }

    let mut font_cache: BTreeMap<u32, FontInfo> = BTreeMap::new();
    let mut out = Vec::with_capacity(page_count);

    for (page_idx, page) in pages.iter().enumerate() {
        let mut combined_stream = Vec::new();
        for &stream_num in &page.content_streams {
            let Some(obj) = objects.get(&stream_num) else {
                continue;
            };
            let Some(raw_stream) = obj.stream else {
                continue;
            };
            let decoded = match decode_stream(&objects, &obj.dict, raw_stream, &mut budget) {
                Ok(buf) => buf,
                Err(reason) => return Err((reason, page_count)),
            };
            if !combined_stream.is_empty() && !decoded.is_empty() {
                combined_stream.push(b'\n');
            }
            combined_stream.extend_from_slice(&decoded);
        }

        let mut page_res = PageResources::default();
        for &node_num in &page.resources_chain {
            if let Some(node_obj) = objects.get(&node_num) {
                if let Some(res_val) = dict_lookup(&node_obj.dict, b"/Resources") {
                    if let Err(reason) = load_resources_from_slice(
                        &objects,
                        res_val,
                        &mut font_cache,
                        &mut budget,
                        &mut page_res,
                    ) {
                        return Err((reason, page_count));
                    }
                }
            }
        }

        let raw_lines = match extract_page_lines(
            &objects,
            &combined_stream,
            &page_res,
            &mut font_cache,
            &mut budget,
        ) {
            Ok(lines) => lines,
            Err(reason) => return Err((reason, page_count)),
        };

        let mut page_lines = Vec::new();
        for raw_line in raw_lines {
            let trimmed = raw_line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.len() > MAX_LINE_BYTES {
                return Err(("line_over_budget", page_count));
            }
            page_lines.push(trimmed.to_string());
        }

        out.push(ExtractedPage {
            page_num: page_idx + 1,
            stream_object: page.content_streams.first().copied().unwrap_or(0),
            lines: page_lines,
        });
    }

    Ok(out)
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
    if let Some(pos) = rfind_subslice(bytes, b"trailer") {
        let tail = &bytes[pos + b"trailer".len()..];
        let dict_pos = skip_pdf_ws_and_comments(tail, 0);
        if dict_pos < tail.len() && tail[dict_pos..].starts_with(b"<<") {
            if let Some((start, end)) = find_dict_bounds(tail, dict_pos) {
                if dict_lookup(&tail[start..end], b"/Encrypt").is_some() {
                    return true;
                }
            }
        }
        if has_name_token(&bytes[pos..], b"/Encrypt") {
            return true;
        }
    }
    objects
        .values()
        .any(|obj| dict_lookup(&obj.dict, b"/Encrypt").is_some())
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

fn is_pdf_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | 0)
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

fn after_keyword_ok(bytes: &[u8], after: usize) -> bool {
    after >= bytes.len() || is_pdf_delimiter(bytes[after])
}

fn trim_pdf_ws(mut slice: &[u8]) -> &[u8] {
    while let Some(&first) = slice.first() {
        if is_pdf_ws(first) {
            slice = &slice[1..];
        } else {
            break;
        }
    }
    while let Some(&last) = slice.last() {
        if is_pdf_ws(last) {
            slice = &slice[..slice.len() - 1];
        } else {
            break;
        }
    }
    slice
}

fn skip_pdf_ws_and_comments(bytes: &[u8], mut i: usize) -> usize {
    let n = bytes.len();
    while i < n {
        if is_pdf_ws(bytes[i]) {
            i += 1;
        } else if bytes[i] == b'%' {
            while i < n && bytes[i] != b'\n' && bytes[i] != b'\r' {
                i += 1;
            }
        } else {
            break;
        }
    }
    i
}

fn skip_pdf_literal_string(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    let mut depth = 1usize;
    let n = bytes.len();
    while i < n && depth > 0 {
        let b = bytes[i];
        if b == b'\\' {
            i = (i + 2).min(n);
            continue;
        }
        if b == b'(' {
            depth += 1;
        } else if b == b')' {
            depth -= 1;
        }
        i += 1;
    }
    i
}

fn skip_pdf_hex_string(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'>' {
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

fn find_dict_bounds(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    if !bytes.get(start..)?.starts_with(b"<<") {
        return None;
    }
    let mut i = start;
    let mut depth = 0usize;
    let n = bytes.len();
    while i < n {
        if bytes[i..].starts_with(b"<<") {
            depth += 1;
            i += 2;
        } else if bytes[i..].starts_with(b">>") {
            depth = depth.saturating_sub(1);
            i += 2;
            if depth == 0 {
                return Some((start, i));
            }
        } else if bytes[i] == b'(' {
            i = skip_pdf_literal_string(bytes, i);
        } else if bytes[i] == b'<' {
            i = skip_pdf_hex_string(bytes, i);
        } else if bytes[i] == b'%' {
            while i < n && bytes[i] != b'\n' && bytes[i] != b'\r' {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    None
}

fn find_array_bounds(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start).copied() != Some(b'[') {
        return None;
    }
    let mut i = start;
    let mut depth = 0usize;
    let n = bytes.len();
    while i < n {
        match bytes[i] {
            b'[' => {
                depth += 1;
                i += 1;
            }
            b']' => {
                depth = depth.saturating_sub(1);
                i += 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b'(' => i = skip_pdf_literal_string(bytes, i),
            b'<' if bytes[i..].starts_with(b"<<") => i += 2,
            b'<' => i = skip_pdf_hex_string(bytes, i),
            b'%' => {
                while i < n && bytes[i] != b'\n' && bytes[i] != b'\r' {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    None
}

fn skip_single_value_token(bytes: &[u8], p: usize) -> usize {
    if p >= bytes.len() {
        return p;
    }
    if bytes[p..].starts_with(b"<<") {
        find_dict_bounds(bytes, p).map_or(bytes.len(), |(_, end)| end)
    } else if bytes[p] == b'[' {
        find_array_bounds(bytes, p).unwrap_or(bytes.len())
    } else if bytes[p] == b'(' {
        skip_pdf_literal_string(bytes, p)
    } else if bytes[p] == b'<' {
        skip_pdf_hex_string(bytes, p)
    } else if bytes[p] == b'/' {
        let mut q = p + 1;
        while q < bytes.len() && !is_pdf_delimiter(bytes[q]) {
            q += 1;
        }
        q
    } else {
        let mut q = p;
        while q < bytes.len() && !is_pdf_delimiter(bytes[q]) {
            q += 1;
        }
        if q == p {
            p + 1
        } else {
            q
        }
    }
}

fn dict_entries(dict: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut entries = Vec::new();
    let start = skip_pdf_ws_and_comments(dict, 0);
    if !dict.get(start..).is_some_and(|s| s.starts_with(b"<<")) {
        return entries;
    }
    let mut p = start + 2;
    let n = dict.len();

    while p < n {
        p = skip_pdf_ws_and_comments(dict, p);
        if p >= n || dict[p..].starts_with(b">>") {
            break;
        }
        if dict[p] != b'/' {
            p = skip_single_value_token(dict, p);
            continue;
        }

        let key_start = p;
        p += 1;
        while p < n && !is_pdf_delimiter(dict[p]) {
            p += 1;
        }
        let key = &dict[key_start..p];

        p = skip_pdf_ws_and_comments(dict, p);
        if p >= n || dict[p..].starts_with(b">>") {
            break;
        }
        let val_start = p;
        let val_end = if dict[p..].starts_with(b"<<") {
            let end = find_dict_bounds(dict, p).map_or(n, |(_, e)| e);
            p = end;
            end
        } else if dict[p] == b'[' {
            let end = find_array_bounds(dict, p).unwrap_or(n);
            p = end;
            end
        } else if dict[p] == b'(' {
            let end = skip_pdf_literal_string(dict, p);
            p = end;
            end
        } else if dict[p] == b'<' {
            let end = skip_pdf_hex_string(dict, p);
            p = end;
            end
        } else if dict[p] == b'/' {
            let mut q = p + 1;
            while q < n && !is_pdf_delimiter(dict[q]) {
                q += 1;
            }
            p = q;
            q
        } else {
            let mut q = p;
            let mut last_end = p;
            loop {
                q = skip_pdf_ws_and_comments(dict, q);
                if q >= n
                    || dict[q..].starts_with(b">>")
                    || matches!(dict[q], b'/' | b'[' | b']' | b'(' | b'<')
                {
                    break;
                }
                let tok_start = q;
                while q < n && !is_pdf_delimiter(dict[q]) {
                    q += 1;
                }
                if q == tok_start {
                    q += 1;
                }
                last_end = q;
            }
            p = q;
            last_end
        };

        entries.push((key, &dict[val_start..val_end]));
    }

    entries
}

fn dict_lookup<'a>(dict: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    dict_entries(dict)
        .into_iter()
        .find_map(|(k, v)| (k == key).then_some(v))
}

fn parse_objects(bytes: &[u8]) -> (BTreeMap<u32, PdfObject<'_>>, Vec<u32>) {
    let mut map = BTreeMap::new();
    let mut objstm_nums = Vec::new();
    let mut i = 0usize;
    let n = bytes.len();

    while i < n {
        if bytes[i].is_ascii_digit() && (i == 0 || is_pdf_delimiter(bytes[i - 1])) {
            if let Some((num, body_start)) = parse_obj_header(bytes, i) {
                let p = skip_pdf_ws_and_comments(bytes, body_start);
                if p < n && bytes[p..].starts_with(b"<<") {
                    if let Some((d_start, d_end)) = find_dict_bounds(bytes, p) {
                        let dict_slice = &bytes[d_start..d_end];
                        let after_dict = skip_pdf_ws_and_comments(bytes, d_end);
                        if bytes[after_dict..].starts_with(b"stream") {
                            let s_after = after_dict + 6;
                            let stream_start = if bytes[s_after..].starts_with(b"\r\n") {
                                Some(s_after + 2)
                            } else if s_after < n && bytes[s_after] == b'\n' {
                                Some(s_after + 1)
                            } else {
                                None
                            };
                            if let Some(s_start) = stream_start {
                                if let Some((stream_slice, next_i)) =
                                    extract_stream_slice(bytes, dict_slice, s_start)
                                {
                                    map.insert(
                                        num,
                                        PdfObject {
                                            num,
                                            dict: Cow::Borrowed(dict_slice),
                                            stream: Some(stream_slice),
                                        },
                                    );
                                    if is_dict_type(dict_slice, b"/ObjStm") {
                                        objstm_nums.push(num);
                                    }
                                    i = next_i;
                                    continue;
                                }
                            }
                        }

                        if let Some(end_rel) = find_endobj(&bytes[d_end..]) {
                            map.insert(
                                num,
                                PdfObject {
                                    num,
                                    dict: Cow::Borrowed(dict_slice),
                                    stream: None,
                                },
                            );
                            i = d_end + end_rel + 6;
                            continue;
                        }
                    }
                }

                if let Some(end_rel) = find_endobj_safe(&bytes[body_start..]) {
                    let body = trim_pdf_ws(&bytes[body_start..body_start + end_rel]);
                    map.insert(
                        num,
                        PdfObject {
                            num,
                            dict: Cow::Borrowed(body),
                            stream: None,
                        },
                    );
                    i = body_start + end_rel + 6;
                    continue;
                }
            }
        }
        i += 1;
    }

    (map, objstm_nums)
}

fn extract_stream_slice<'a>(
    bytes: &'a [u8],
    dict_slice: &[u8],
    s_start: usize,
) -> Option<(&'a [u8], usize)> {
    let n = bytes.len();
    if let Some(len) = dict_lookup(dict_slice, b"/Length").and_then(parse_nonneg_usize) {
        if let Some(s_end) = s_start.checked_add(len) {
            if s_end <= n {
                let mut es = s_end;
                while es < n && matches!(bytes[es], b' ' | b'\t' | b'\r' | b'\n') {
                    es += 1;
                }
                if bytes[es..].starts_with(b"endstream") && after_keyword_ok(bytes, es + 9) {
                    let after_es = es + 9;
                    let next_i =
                        find_endobj(&bytes[after_es..]).map_or(after_es, |rel| after_es + rel + 6);
                    return Some((&bytes[s_start..s_end], next_i));
                }
            }
        }
    }

    let (raw_len, after_endobj) = find_endstream_and_endobj(&bytes[s_start..])?;
    let mut raw = &bytes[s_start..s_start + raw_len];
    if raw.ends_with(b"\r\n") {
        raw = &raw[..raw.len() - 2];
    } else if raw.ends_with(b"\n") || raw.ends_with(b"\r") {
        raw = &raw[..raw.len() - 1];
    }
    Some((raw, s_start + after_endobj))
}

fn find_endstream_and_endobj(slice: &[u8]) -> Option<(usize, usize)> {
    let mut start = 0usize;
    let mut first_es: Option<usize> = None;

    while let Some(pos) = find_subslice(&slice[start..], b"endstream") {
        let abs = start + pos;
        let before_ok = abs == 0 || is_pdf_delimiter(slice[abs - 1]);
        let after = abs + 9;
        if before_ok && after_keyword_ok(slice, after) {
            if first_es.is_none() {
                first_es = Some(abs);
            }
            let ws_end = skip_pdf_ws_and_comments(slice, after);
            if slice[ws_end..].starts_with(b"endobj") && after_keyword_ok(slice, ws_end + 6) {
                return Some((abs, ws_end + 6));
            }
        }
        start = abs + 1;
    }

    if let Some(es) = first_es {
        let next_i = find_endobj(&slice[es + 9..]).map_or(es + 9, |rel| es + 9 + rel + 6);
        return Some((es, next_i));
    }
    None
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
    while p < bytes.len() && is_pdf_ws(bytes[p]) {
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
    while p < bytes.len() && is_pdf_ws(bytes[p]) {
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
        if before_ok && after_keyword_ok(slice, after) {
            return Some(abs);
        }
        start = abs + 1;
    }
    None
}

fn find_endobj_safe(slice: &[u8]) -> Option<usize> {
    let mut i = 0usize;
    let n = slice.len();
    while i < n {
        if slice[i] == b'(' {
            i = skip_pdf_literal_string(slice, i);
        } else if slice[i] == b'<' {
            if slice[i..].starts_with(b"<<") {
                i = find_dict_bounds(slice, i).map_or(i + 2, |(_, end)| end);
            } else {
                i = skip_pdf_hex_string(slice, i);
            }
        } else if slice[i] == b'[' {
            i = find_array_bounds(slice, i).unwrap_or(i + 1);
        } else if slice[i] == b'%' {
            while i < n && slice[i] != b'\n' && slice[i] != b'\r' {
                i += 1;
            }
        } else if slice[i..].starts_with(b"endobj")
            && (i == 0 || is_pdf_delimiter(slice[i - 1]))
            && after_keyword_ok(slice, i + 6)
        {
            return Some(i);
        } else {
            i += 1;
        }
    }
    None
}

fn unpack_object_streams(
    objects: &mut BTreeMap<u32, PdfObject<'_>>,
    objstm_nums: &[u32],
    budget: &mut DecodeBudget,
) -> Result<(), &'static str> {
    if objstm_nums.is_empty() {
        return Ok(());
    }

    let mut extracted: Vec<(u32, Vec<u8>)> = Vec::new();
    for &stm_num in objstm_nums.iter().rev() {
        let Some(obj) = objects.get(&stm_num) else {
            continue;
        };
        let Some(raw) = obj.stream else {
            continue;
        };
        let Some(n_objs) = dict_lookup(&obj.dict, b"/N").and_then(|v| resolve_usize(objects, v))
        else {
            continue;
        };
        let Some(first) = dict_lookup(&obj.dict, b"/First").and_then(|v| resolve_usize(objects, v))
        else {
            continue;
        };
        if n_objs == 0 || n_objs > MAX_OBJSTM_OBJECTS {
            continue;
        }

        let decoded = decode_stream(objects, &obj.dict, raw, budget)?;
        if first > decoded.len() {
            continue;
        }
        let pairs = parse_objstm_header(&decoded[..first], n_objs);
        if pairs.is_empty() {
            continue;
        }

        let mut sorted_offsets: Vec<usize> = pairs
            .iter()
            .filter_map(|&(_, rel)| first.checked_add(rel).filter(|&abs| abs <= decoded.len()))
            .collect();
        sorted_offsets.sort_unstable();
        sorted_offsets.dedup();

        for (obj_num, rel_off) in pairs {
            let Some(start) = first.checked_add(rel_off) else {
                continue;
            };
            if start >= decoded.len() {
                continue;
            }
            let end = match sorted_offsets.binary_search(&start) {
                Ok(idx) if idx + 1 < sorted_offsets.len() => sorted_offsets[idx + 1],
                _ => decoded.len(),
            };
            let slice = trim_pdf_ws(&decoded[start..end]);
            if !slice.is_empty() {
                extracted.push((obj_num, slice.to_vec()));
            }
        }
    }

    for (num, data) in extracted {
        objects.entry(num).or_insert_with(|| PdfObject {
            num,
            dict: Cow::Owned(data),
            stream: None,
        });
    }
    Ok(())
}

fn parse_objstm_header(header: &[u8], expected_count: usize) -> Vec<(u32, usize)> {
    let mut pairs = Vec::with_capacity(expected_count.min(1024));
    let mut p = 0usize;
    let n = header.len();

    for _ in 0..expected_count {
        p = skip_pdf_ws_and_comments(header, p);
        let obj_start = p;
        while p < n && header[p].is_ascii_digit() {
            p += 1;
        }
        if p == obj_start {
            break;
        }
        let Ok(obj_num) = std::str::from_utf8(&header[obj_start..p])
            .unwrap_or("")
            .parse::<u32>()
        else {
            break;
        };

        p = skip_pdf_ws_and_comments(header, p);
        let off_start = p;
        while p < n && header[p].is_ascii_digit() {
            p += 1;
        }
        if p == off_start {
            break;
        }
        let Ok(rel_off) = std::str::from_utf8(&header[off_start..p])
            .unwrap_or("")
            .parse::<usize>()
        else {
            break;
        };
        pairs.push((obj_num, rel_off));
    }

    pairs
}

fn resolve_ref_slice<'a, 'b>(
    objects: &'a BTreeMap<u32, PdfObject<'b>>,
    mut slice: &'a [u8],
) -> &'a [u8] {
    for _ in 0..MAX_REF_DEPTH {
        let trimmed = trim_pdf_ws(slice);
        if let Some((num, consumed)) = parse_indirect_ref(trimmed) {
            if trim_pdf_ws(&trimmed[consumed..]).is_empty() {
                if let Some(obj) = objects.get(&num) {
                    slice = &obj.dict;
                    continue;
                }
            }
        }
        return trimmed;
    }
    trim_pdf_ws(slice)
}

fn parse_nonneg_usize(slice: &[u8]) -> Option<usize> {
    let s = std::str::from_utf8(trim_pdf_ws(slice)).ok()?;
    s.parse::<usize>().ok()
}

fn parse_i64(slice: &[u8]) -> Option<i64> {
    let s = std::str::from_utf8(trim_pdf_ws(slice)).ok()?;
    s.parse::<i64>().ok()
}

fn resolve_usize(objects: &BTreeMap<u32, PdfObject<'_>>, slice: &[u8]) -> Option<usize> {
    parse_nonneg_usize(resolve_ref_slice(objects, slice))
}

fn parse_name_token(slice: &[u8]) -> Option<&[u8]> {
    let p = skip_pdf_ws_and_comments(slice, 0);
    if p >= slice.len() || slice[p] != b'/' {
        return None;
    }
    let mut end = p + 1;
    while end < slice.len() && !is_pdf_delimiter(slice[end]) {
        end += 1;
    }
    Some(&slice[p..end])
}

fn decode_pdf_name(raw_name: &[u8]) -> Option<String> {
    let name = raw_name.strip_prefix(b"/")?;
    let mut out = String::with_capacity(name.len());
    let mut i = 0usize;
    while i < name.len() {
        if name[i] == b'#' && i + 2 < name.len() {
            if let (Some(hi), Some(lo)) = (hex_nibble(name[i + 1]), hex_nibble(name[i + 2])) {
                out.push(char::from((hi << 4) | lo));
                i += 3;
                continue;
            }
        }
        out.push(char::from(name[i]));
        i += 1;
    }
    Some(out)
}

fn is_dict_type(dict: &[u8], expected: &[u8]) -> bool {
    dict_has_name(dict, b"/Type", expected)
}

fn dict_has_name(dict: &[u8], key: &[u8], expected: &[u8]) -> bool {
    dict_lookup(dict, key)
        .and_then(parse_name_token)
        .is_some_and(|name| name == expected)
}

fn parse_array_elements(slice: &[u8]) -> Vec<&[u8]> {
    let mut items = Vec::new();
    let start = skip_pdf_ws_and_comments(slice, 0);
    if slice.get(start).copied() != Some(b'[') {
        return items;
    }
    let mut p = start + 1;
    let n = slice.len();

    while p < n {
        p = skip_pdf_ws_and_comments(slice, p);
        if p >= n || slice[p] == b']' {
            break;
        }
        let elem_start = p;
        if slice[p..].starts_with(b"<<") {
            p = find_dict_bounds(slice, p).map_or(n, |(_, e)| e);
        } else if slice[p] == b'[' {
            p = find_array_bounds(slice, p).unwrap_or(n);
        } else if slice[p] == b'(' {
            p = skip_pdf_literal_string(slice, p);
        } else if slice[p] == b'<' {
            p = skip_pdf_hex_string(slice, p);
        } else if slice[p] == b'/' {
            p += 1;
            while p < n && !is_pdf_delimiter(slice[p]) {
                p += 1;
            }
        } else if let Some((_, consumed)) = parse_indirect_ref(&slice[p..]) {
            p += consumed;
        } else {
            while p < n && !is_pdf_delimiter(slice[p]) {
                p += 1;
            }
            if p == elem_start {
                p += 1;
                continue;
            }
        }
        items.push(&slice[elem_start..p]);
    }
    items
}

fn collect_pages(objects: &BTreeMap<u32, PdfObject<'_>>) -> Vec<PageSpec> {
    let mut ordered = Vec::new();
    let mut visited = BTreeSet::new();

    if let Some(catalog) = objects
        .values()
        .find(|o| is_dict_type(&o.dict, b"/Catalog"))
    {
        if let Some(pages_ref) = first_indirect_ref_after(&catalog.dict, b"/Pages") {
            walk_page_tree(objects, pages_ref, &[], &mut visited, &mut ordered);
        }
    }

    if ordered.is_empty() {
        for obj in objects.values() {
            if is_dict_type(&obj.dict, b"/Page") {
                let mut chain = Vec::new();
                let mut seen = BTreeSet::new();
                let mut cur = obj.num;
                while seen.insert(cur) && chain.len() < MAX_REF_DEPTH {
                    chain.push(cur);
                    let Some(cur_obj) = objects.get(&cur) else {
                        break;
                    };
                    let Some(parent) = first_indirect_ref_after(&cur_obj.dict, b"/Parent") else {
                        break;
                    };
                    cur = parent;
                }
                chain.reverse();
                ordered.push(PageSpec {
                    content_streams: content_refs(objects, &obj.dict),
                    resources_chain: chain,
                });
            }
        }
    }

    ordered
}

fn walk_page_tree(
    objects: &BTreeMap<u32, PdfObject<'_>>,
    node_num: u32,
    ancestors: &[u32],
    visited: &mut BTreeSet<u32>,
    out: &mut Vec<PageSpec>,
) {
    if !visited.insert(node_num) || out.len() > MAX_PAGES {
        return;
    }
    let Some(obj) = objects.get(&node_num) else {
        return;
    };
    let mut chain = ancestors.to_vec();
    chain.push(node_num);

    if is_dict_type(&obj.dict, b"/Page") {
        out.push(PageSpec {
            content_streams: content_refs(objects, &obj.dict),
            resources_chain: chain,
        });
        return;
    }
    for kid in indirect_refs_array_after(objects, &obj.dict, b"/Kids") {
        walk_page_tree(objects, kid, &chain, visited, out);
    }
}

fn first_indirect_ref_after(dict: &[u8], key: &[u8]) -> Option<u32> {
    let val = dict_lookup(dict, key)?;
    parse_indirect_ref(trim_pdf_ws(val)).map(|(num, _)| num)
}

fn content_refs(objects: &BTreeMap<u32, PdfObject<'_>>, dict: &[u8]) -> Vec<u32> {
    let Some(val) = dict_lookup(dict, b"/Contents") else {
        return Vec::new();
    };
    let trimmed = trim_pdf_ws(val);
    if trimmed.starts_with(b"[") {
        return parse_ref_list(trimmed);
    }
    if let Some((num, _)) = parse_indirect_ref(trimmed) {
        if let Some(obj) = objects.get(&num) {
            if obj.stream.is_none() && trim_pdf_ws(&obj.dict).starts_with(b"[") {
                return parse_ref_list(&obj.dict);
            }
        }
        return vec![num];
    }
    Vec::new()
}

fn indirect_refs_array_after(
    objects: &BTreeMap<u32, PdfObject<'_>>,
    dict: &[u8],
    key: &[u8],
) -> Vec<u32> {
    let Some(val) = dict_lookup(dict, key) else {
        return Vec::new();
    };
    let resolved = resolve_ref_slice(objects, val);
    if resolved.starts_with(b"[") {
        parse_ref_list(resolved)
    } else {
        Vec::new()
    }
}

fn parse_ref_list(slice: &[u8]) -> Vec<u32> {
    parse_array_elements(slice)
        .into_iter()
        .filter_map(|elem| parse_indirect_ref(trim_pdf_ws(elem)).map(|(num, _)| num))
        .collect()
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
    while p < slice.len() && is_pdf_ws(slice[p]) {
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
    while p < slice.len() && is_pdf_ws(slice[p]) {
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

// --- Stream Decompression & Predictors ---

fn decode_stream(
    objects: &BTreeMap<u32, PdfObject<'_>>,
    dict: &[u8],
    raw: &[u8],
    budget: &mut DecodeBudget,
) -> Result<Vec<u8>, &'static str> {
    let Some(filter_val) = dict_lookup(dict, b"/Filter") else {
        if raw.len() > MAX_STREAM_DECOMPRESSED {
            return Err("stream_over_budget");
        }
        budget.account(raw.len())?;
        return Ok(raw.to_vec());
    };

    let resolved_filter = resolve_ref_slice(objects, filter_val);
    let filter_names: Vec<&[u8]> = if resolved_filter.starts_with(b"[") {
        parse_array_elements(resolved_filter)
            .into_iter()
            .filter_map(parse_name_token)
            .collect()
    } else if let Some(name) = parse_name_token(resolved_filter) {
        vec![name]
    } else {
        Vec::new()
    };

    if filter_names.is_empty() {
        return Err("unsupported_pdf_filter");
    }

    let parms_list: Vec<Option<&[u8]>> = match dict_lookup(dict, b"/DecodeParms") {
        Some(parms_val) => {
            let resolved_parms = resolve_ref_slice(objects, parms_val);
            if resolved_parms.starts_with(b"[") {
                parse_array_elements(resolved_parms)
                    .into_iter()
                    .map(|elem| {
                        let d = resolve_ref_slice(objects, elem);
                        d.starts_with(b"<<").then_some(d)
                    })
                    .collect()
            } else if resolved_parms.starts_with(b"<<") {
                vec![Some(resolved_parms); filter_names.len()]
            } else {
                vec![None; filter_names.len()]
            }
        }
        None => vec![None; filter_names.len()],
    };

    let mut current: Cow<'_, [u8]> = Cow::Borrowed(raw);
    for (idx, &filter) in filter_names.iter().enumerate() {
        let parms = parms_list.get(idx).copied().flatten();
        let next = match filter {
            b"/FlateDecode" | b"/Fl" => {
                let mut inflated = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(
                    &current,
                    MAX_STREAM_DECOMPRESSED,
                )
                .map_err(|_| "flate_decode_failed")?;
                if let Some(p_dict) = parms {
                    inflated = apply_predictor(objects, p_dict, &inflated)?;
                }
                inflated
            }
            b"/ASCIIHexDecode" | b"/AHx" => decode_ascii_hex(&current)?,
            b"/ASCII85Decode" | b"/A85" => decode_ascii85(&current)?,
            _ => return Err("unsupported_pdf_filter"),
        };
        current = Cow::Owned(next);
    }

    let out = current.into_owned();
    budget.account(out.len())?;
    Ok(out)
}

fn apply_predictor(
    objects: &BTreeMap<u32, PdfObject<'_>>,
    parms_dict: &[u8],
    data: &[u8],
) -> Result<Vec<u8>, &'static str> {
    let predictor = dict_lookup(parms_dict, b"/Predictor")
        .and_then(|v| parse_i64(resolve_ref_slice(objects, v)))
        .unwrap_or(1);
    if predictor <= 1 {
        return Ok(data.to_vec());
    }

    let columns = dict_lookup(parms_dict, b"/Columns")
        .and_then(|v| resolve_usize(objects, v))
        .unwrap_or(1);
    let colors = dict_lookup(parms_dict, b"/Colors")
        .and_then(|v| resolve_usize(objects, v))
        .unwrap_or(1);
    let bpc = dict_lookup(parms_dict, b"/BitsPerComponent")
        .and_then(|v| resolve_usize(objects, v))
        .unwrap_or(8);

    if columns == 0 || colors == 0 || colors > 64 || !matches!(bpc, 1 | 2 | 4 | 8 | 16) {
        return Err("flate_decode_failed");
    }

    if predictor == 2 {
        if bpc != 8 {
            return Err("flate_decode_failed");
        }
        let row_bytes = colors.checked_mul(columns).ok_or("flate_decode_failed")?;
        if row_bytes == 0 || data.len() % row_bytes != 0 {
            return Err("flate_decode_failed");
        }
        let mut out = data.to_vec();
        for row in out.chunks_exact_mut(row_bytes) {
            for i in colors..row_bytes {
                row[i] = row[i].wrapping_add(row[i - colors]);
            }
        }
        return Ok(out);
    }

    if !(10..=15).contains(&predictor) {
        return Err("flate_decode_failed");
    }

    let bits_per_pixel = colors.checked_mul(bpc).ok_or("flate_decode_failed")?;
    let bpp = bits_per_pixel.div_ceil(8).max(1);
    let bits_per_row = bits_per_pixel
        .checked_mul(columns)
        .ok_or("flate_decode_failed")?;
    let row_bytes = bits_per_row.div_ceil(8);
    if row_bytes == 0 || row_bytes > MAX_STREAM_DECOMPRESSED {
        return Err("stream_over_budget");
    }
    let stride = row_bytes.checked_add(1).ok_or("flate_decode_failed")?;
    if data.is_empty() {
        return Ok(Vec::new());
    }
    if data.len() % stride != 0 {
        return Err("flate_decode_failed");
    }

    let rows = data.len() / stride;
    let mut out = Vec::with_capacity(rows * row_bytes);
    let mut prev = vec![0u8; row_bytes];
    let mut cur = vec![0u8; row_bytes];

    for chunk in data.chunks_exact(stride) {
        let tag = chunk[0];
        let src = &chunk[1..];
        match tag {
            0 => cur.copy_from_slice(src),
            1 => {
                for (i, &s) in src.iter().enumerate() {
                    let left = if i >= bpp { cur[i - bpp] } else { 0 };
                    cur[i] = s.wrapping_add(left);
                }
            }
            2 => {
                for ((c, &s), &p) in cur.iter_mut().zip(src).zip(&prev) {
                    *c = s.wrapping_add(p);
                }
            }
            3 => {
                for (i, &s) in src.iter().enumerate() {
                    let left = if i >= bpp { u16::from(cur[i - bpp]) } else { 0 };
                    let up = u16::from(prev[i]);
                    cur[i] = s.wrapping_add((((left + up) >> 1) & 0xff) as u8);
                }
            }
            4 => {
                for (i, &s) in src.iter().enumerate() {
                    let a = if i >= bpp { cur[i - bpp] } else { 0 };
                    let b = prev[i];
                    let c = if i >= bpp { prev[i - bpp] } else { 0 };
                    cur[i] = s.wrapping_add(paeth_predictor(a, b, c));
                }
            }
            _ => return Err("flate_decode_failed"),
        }
        out.extend_from_slice(&cur);
        std::mem::swap(&mut prev, &mut cur);
    }

    Ok(out)
}

fn paeth_predictor(a: u8, b: u8, c: u8) -> u8 {
    let a_i = i32::from(a);
    let b_i = i32::from(b);
    let c_i = i32::from(c);
    let p = a_i + b_i - c_i;
    let pa = (p - a_i).abs();
    let pb = (p - b_i).abs();
    let pc = (p - c_i).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn decode_ascii_hex(raw: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut nibbles = Vec::new();
    for &b in raw {
        if b == b'>' {
            break;
        }
        if is_pdf_ws(b) {
            continue;
        }
        let v = hex_nibble(b).ok_or("stream_decode_failed")?;
        nibbles.push(v);
        if nibbles.len().div_ceil(2) > MAX_STREAM_DECOMPRESSED {
            return Err("stream_over_budget");
        }
    }
    let mut out = Vec::with_capacity(nibbles.len().div_ceil(2));
    for chunk in nibbles.chunks(2) {
        let hi = chunk[0];
        let lo = chunk.get(1).copied().unwrap_or(0);
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn decode_ascii85(raw: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut slice = trim_pdf_ws(raw);
    if let Some(rest) = slice.strip_prefix(b"<~") {
        slice = rest;
    }

    let mut out = Vec::new();
    let mut group = [0u32; 5];
    let mut count = 0usize;
    let mut i = 0usize;

    while i < slice.len() {
        let b = slice[i];
        if is_pdf_ws(b) {
            i += 1;
            continue;
        }
        if b == b'~' {
            if slice.get(i + 1) == Some(&b'>') {
                break;
            }
            return Err("stream_decode_failed");
        }
        if b == b'z' {
            if count != 0 {
                return Err("stream_decode_failed");
            }
            if out.len() + 4 > MAX_STREAM_DECOMPRESSED {
                return Err("stream_over_budget");
            }
            out.extend_from_slice(&[0u8; 4]);
            i += 1;
            continue;
        }
        if !(b'!'..=b'u').contains(&b) {
            return Err("stream_decode_failed");
        }
        group[count] = u32::from(b - b'!');
        count += 1;
        if count == 5 {
            let val: u64 = u64::from(group[0]) * 52_200_625
                + u64::from(group[1]) * 614_125
                + u64::from(group[2]) * 7_225
                + u64::from(group[3]) * 85
                + u64::from(group[4]);
            let val_u32 = u32::try_from(val).map_err(|_| "stream_decode_failed")?;
            if out.len() + 4 > MAX_STREAM_DECOMPRESSED {
                return Err("stream_over_budget");
            }
            out.extend_from_slice(&val_u32.to_be_bytes());
            count = 0;
        }
        i += 1;
    }

    if count == 1 {
        return Err("stream_decode_failed");
    }
    if count > 1 {
        for slot in &mut group[count..5] {
            *slot = 84;
        }
        let val: u64 = u64::from(group[0]) * 52_200_625
            + u64::from(group[1]) * 614_125
            + u64::from(group[2]) * 7_225
            + u64::from(group[3]) * 85
            + u64::from(group[4]);
        let val_u32 = u32::try_from(val).map_err(|_| "stream_decode_failed")?;
        let bytes = val_u32.to_be_bytes();
        let emit = count - 1;
        if out.len() + emit > MAX_STREAM_DECOMPRESSED {
            return Err("stream_over_budget");
        }
        out.extend_from_slice(&bytes[..emit]);
    }

    Ok(out)
}

// --- Font Resources, ToUnicode CMaps & Encodings ---

#[derive(Default)]
struct PageResources {
    fonts: BTreeMap<String, FontSlot>,
    xobjects: BTreeMap<String, u32>,
}

impl PageResources {
    fn resolve_font<'a>(
        &'a self,
        name: Option<&str>,
        cache: &'a BTreeMap<u32, FontInfo>,
    ) -> Option<&'a FontInfo> {
        let slot = self.fonts.get(name?)?;
        match slot {
            FontSlot::Cached(num) => cache.get(num),
            FontSlot::Inline(info) => Some(info),
        }
    }
}

#[derive(Clone, Debug)]
enum FontSlot {
    Cached(u32),
    Inline(FontInfo),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BaseEncoding {
    Default,
    WinAnsi,
    MacRoman,
    Standard,
    Identity,
}

#[derive(Clone, Debug)]
struct FontInfo {
    to_unicode: Option<ToUnicodeCmap>,
    base_encoding: BaseEncoding,
    differences: BTreeMap<u8, String>,
}

#[derive(Clone, Debug, Default)]
struct ToUnicodeCmap {
    map_1: BTreeMap<u8, String>,
    map_2: BTreeMap<u16, String>,
    ranges_1: Vec<(u8, u8, Vec<u16>)>,
    ranges_2: Vec<(u16, u16, Vec<u16>)>,
    cs_1: Vec<(u8, u8)>,
    cs_2: Vec<(u16, u16)>,
}

impl ToUnicodeCmap {
    fn lookup_1(&self, code: u8) -> Option<String> {
        if let Some(s) = self.map_1.get(&code) {
            return Some(s.clone());
        }
        for (lo, hi, base_units) in &self.ranges_1 {
            if *lo <= code && code <= *hi {
                return Some(increment_utf16_units(base_units, u16::from(code - *lo)));
            }
        }
        None
    }

    fn lookup_2(&self, code: u16) -> Option<String> {
        if let Some(s) = self.map_2.get(&code) {
            return Some(s.clone());
        }
        for (lo, hi, base_units) in &self.ranges_2 {
            if *lo <= code && code <= *hi {
                return Some(increment_utf16_units(base_units, code - *lo));
            }
        }
        None
    }

    fn in_cs_1(&self, b: u8) -> bool {
        self.cs_1.iter().any(|&(lo, hi)| lo <= b && b <= hi)
    }

    fn has_1byte(&self) -> bool {
        !self.map_1.is_empty() || !self.ranges_1.is_empty() || !self.cs_1.is_empty()
    }

    fn has_2byte(&self) -> bool {
        !self.map_2.is_empty() || !self.ranges_2.is_empty() || !self.cs_2.is_empty()
    }

    fn decode(&self, raw: &[u8], fallback: Option<&FontInfo>) -> String {
        let has_1 = self.has_1byte();
        let has_2 = self.has_2byte();
        let mut out = String::new();

        if has_2 && !has_1 {
            for chunk in raw.chunks_exact(2) {
                let code = u16::from_be_bytes([chunk[0], chunk[1]]);
                if let Some(s) = self.lookup_2(code) {
                    out.push_str(&s);
                }
            }
            return out;
        }

        if has_1 && !has_2 {
            for &b in raw {
                if let Some(s) = self.lookup_1(b) {
                    out.push_str(&s);
                } else if let Some(font) = fallback {
                    out.push_str(&font.decode_single_byte(b));
                }
            }
            return out;
        }

        let mut i = 0usize;
        while i < raw.len() {
            if !self.cs_1.is_empty() && self.in_cs_1(raw[i]) {
                if let Some(s) = self.lookup_1(raw[i]) {
                    out.push_str(&s);
                } else if let Some(font) = fallback {
                    out.push_str(&font.decode_single_byte(raw[i]));
                }
                i += 1;
                continue;
            }
            if i + 1 < raw.len() {
                let code2 = u16::from_be_bytes([raw[i], raw[i + 1]]);
                if let Some(s) = self.lookup_2(code2) {
                    out.push_str(&s);
                    i += 2;
                    continue;
                }
            }
            if let Some(s) = self.lookup_1(raw[i]) {
                out.push_str(&s);
            } else if let Some(font) = fallback {
                if font.base_encoding != BaseEncoding::Identity {
                    out.push_str(&font.decode_single_byte(raw[i]));
                }
            }
            i += 1;
        }
        out
    }
}

fn increment_utf16_units(base_units: &[u16], delta: u16) -> String {
    if base_units.is_empty() {
        return String::new();
    }
    let mut units = base_units.to_vec();
    if let Some(last) = units.last_mut() {
        *last = last.wrapping_add(delta);
    }
    String::from_utf16_lossy(&units)
}

fn parse_utf16be_units(bytes: &[u8]) -> Vec<u16> {
    if bytes.len() == 1 {
        return vec![u16::from(bytes[0])];
    }
    bytes
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect()
}

fn load_resources_from_slice(
    objects: &BTreeMap<u32, PdfObject<'_>>,
    res_val: &[u8],
    font_cache: &mut BTreeMap<u32, FontInfo>,
    budget: &mut DecodeBudget,
    out: &mut PageResources,
) -> Result<(), &'static str> {
    let res_dict = resolve_ref_slice(objects, res_val);
    if let Some(font_val) = dict_lookup(res_dict, b"/Font") {
        let font_dict = resolve_ref_slice(objects, font_val);
        for (k, v) in dict_entries(font_dict) {
            if let Some(name) = decode_pdf_name(k) {
                if let Some((font_obj_num, _)) = parse_indirect_ref(trim_pdf_ws(v)) {
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        font_cache.entry(font_obj_num)
                    {
                        if let Some(font_obj) = objects.get(&font_obj_num) {
                            let resolved_font = resolve_ref_slice(objects, &font_obj.dict);
                            let info = parse_font_info(objects, resolved_font, budget)?;
                            entry.insert(info);
                        }
                    }
                    if font_cache.contains_key(&font_obj_num) {
                        out.fonts.insert(name, FontSlot::Cached(font_obj_num));
                    }
                } else if trim_pdf_ws(v).starts_with(b"<<") {
                    let info = parse_font_info(objects, v, budget)?;
                    out.fonts.insert(name, FontSlot::Inline(info));
                }
            }
        }
    }

    if let Some(xobj_val) = dict_lookup(res_dict, b"/XObject") {
        let xobj_dict = resolve_ref_slice(objects, xobj_val);
        for (k, v) in dict_entries(xobj_dict) {
            if let Some(name) = decode_pdf_name(k) {
                if let Some((xobj_num, _)) = parse_indirect_ref(trim_pdf_ws(v)) {
                    out.xobjects.insert(name, xobj_num);
                }
            }
        }
    }
    Ok(())
}

fn parse_font_info(
    objects: &BTreeMap<u32, PdfObject<'_>>,
    font_dict: &[u8],
    budget: &mut DecodeBudget,
) -> Result<FontInfo, &'static str> {
    let mut to_unicode = None;
    if let Some(tu_val) = dict_lookup(font_dict, b"/ToUnicode") {
        if let Some((tu_num, _)) = parse_indirect_ref(trim_pdf_ws(tu_val)) {
            if let Some(tu_obj) = objects.get(&tu_num) {
                if let Some(raw) = tu_obj.stream {
                    match decode_stream(objects, &tu_obj.dict, raw, budget) {
                        Ok(cmap_bytes) => {
                            to_unicode = Some(parse_cmap(&cmap_bytes));
                        }
                        Err(reason @ ("decompressed_budget_exceeded" | "stream_over_budget")) => {
                            return Err(reason);
                        }
                        Err(_) => {}
                    }
                }
            }
        }
    }

    let mut base_encoding = if dict_has_name(font_dict, b"/Subtype", b"/Type0") {
        BaseEncoding::Identity
    } else {
        BaseEncoding::Default
    };
    let mut differences = BTreeMap::new();

    if let Some(enc_val) = dict_lookup(font_dict, b"/Encoding") {
        let enc_slice = resolve_ref_slice(objects, enc_val);
        if let Some(name) = parse_name_token(enc_slice) {
            base_encoding = match name {
                b"/WinAnsiEncoding" => BaseEncoding::WinAnsi,
                b"/MacRomanEncoding" => BaseEncoding::MacRoman,
                b"/StandardEncoding" => BaseEncoding::Standard,
                b"/Identity-H" | b"/Identity-V" => BaseEncoding::Identity,
                _ => BaseEncoding::WinAnsi,
            };
        } else if enc_slice.starts_with(b"<<") {
            base_encoding =
                match dict_lookup(enc_slice, b"/BaseEncoding").and_then(parse_name_token) {
                    Some(b"/MacRomanEncoding") => BaseEncoding::MacRoman,
                    Some(b"/StandardEncoding") => BaseEncoding::Standard,
                    _ => BaseEncoding::WinAnsi,
                };
            if let Some(diff_val) = dict_lookup(enc_slice, b"/Differences") {
                let diff_slice = resolve_ref_slice(objects, diff_val);
                parse_encoding_differences(diff_slice, &mut differences);
            }
        }
    }

    Ok(FontInfo {
        to_unicode,
        base_encoding,
        differences,
    })
}

fn parse_encoding_differences(slice: &[u8], out: &mut BTreeMap<u8, String>) {
    let mut cur_code = 0u32;
    for elem in parse_array_elements(slice) {
        let trimmed = trim_pdf_ws(elem);
        if let Some(name_raw) = parse_name_token(trimmed) {
            if let Some(glyph_name) = decode_pdf_name(name_raw) {
                if let Ok(code_u8) = u8::try_from(cur_code) {
                    if let Some(mapped) = glyph_name_to_str(&glyph_name) {
                        out.insert(code_u8, mapped);
                    }
                }
                cur_code = cur_code.saturating_add(1);
            }
        } else if let Some(code) = parse_nonneg_usize(trimmed) {
            cur_code = u32::try_from(code).unwrap_or(u32::MAX);
        }
    }
}

enum CmapToken<'a> {
    Hex(Vec<u8>),
    LBracket,
    RBracket,
    Word(&'a str),
}

fn tokenize_cmap(bytes: &[u8]) -> Vec<CmapToken<'_>> {
    let mut tokens = Vec::new();
    let mut i = 0usize;
    let n = bytes.len();

    while i < n {
        let b = bytes[i];
        if is_pdf_ws(b) {
            i += 1;
        } else if b == b'%' {
            while i < n && bytes[i] != b'\n' && bytes[i] != b'\r' {
                i += 1;
            }
        } else if b == b'<' {
            if bytes[i..].starts_with(b"<<") {
                i += 2;
            } else {
                let (hex_bytes, next) = parse_pdf_hex_string(bytes, i);
                tokens.push(CmapToken::Hex(hex_bytes));
                i = next;
            }
        } else if b == b'>' {
            i += 1;
        } else if b == b'[' {
            tokens.push(CmapToken::LBracket);
            i += 1;
        } else if b == b']' {
            tokens.push(CmapToken::RBracket);
            i += 1;
        } else if b == b'(' {
            i = skip_pdf_literal_string(bytes, i);
        } else {
            let start = i;
            while i < n && !is_pdf_delimiter(bytes[i]) {
                i += 1;
            }
            if i == start {
                i += 1;
                continue;
            }
            if let Ok(word) = std::str::from_utf8(&bytes[start..i]) {
                tokens.push(CmapToken::Word(word));
            }
        }
    }
    tokens
}

fn parse_cmap(bytes: &[u8]) -> ToUnicodeCmap {
    #[derive(Clone, Copy)]
    enum Section {
        None,
        Codespace,
        BfChar,
        BfRange,
    }

    let tokens = tokenize_cmap(bytes);
    let mut cmap = ToUnicodeCmap::default();
    let mut section = Section::None;
    let mut i = 0usize;
    let mut total_inserted = 0usize;

    while i < tokens.len() {
        if let CmapToken::Word(w) = tokens[i] {
            match w {
                "begincodespacerange" => {
                    section = Section::Codespace;
                    i += 1;
                    continue;
                }
                "endcodespacerange" | "endbfchar" | "endbfrange" => {
                    section = Section::None;
                    i += 1;
                    continue;
                }
                "beginbfchar" => {
                    section = Section::BfChar;
                    i += 1;
                    continue;
                }
                "beginbfrange" => {
                    section = Section::BfRange;
                    i += 1;
                    continue;
                }
                _ => {}
            }
        }

        match section {
            Section::None => i += 1,
            Section::Codespace => {
                if let (Some(CmapToken::Hex(lo)), Some(CmapToken::Hex(hi))) =
                    (tokens.get(i), tokens.get(i + 1))
                {
                    if lo.len() == 1 && hi.len() == 1 {
                        cmap.cs_1.push((lo[0], hi[0]));
                    } else if lo.len() == 2 && hi.len() == 2 {
                        cmap.cs_2.push((
                            u16::from_be_bytes([lo[0], lo[1]]),
                            u16::from_be_bytes([hi[0], hi[1]]),
                        ));
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Section::BfChar => {
                if let (Some(CmapToken::Hex(src)), Some(CmapToken::Hex(dst))) =
                    (tokens.get(i), tokens.get(i + 1))
                {
                    if total_inserted < MAX_CMAP_MAPPINGS {
                        let units = parse_utf16be_units(dst);
                        let mapped = String::from_utf16_lossy(&units);
                        if src.len() == 1 {
                            cmap.map_1.insert(src[0], mapped);
                            total_inserted += 1;
                        } else if src.len() == 2 {
                            cmap.map_2
                                .insert(u16::from_be_bytes([src[0], src[1]]), mapped);
                            total_inserted += 1;
                        }
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Section::BfRange => {
                if let (Some(CmapToken::Hex(lo)), Some(CmapToken::Hex(hi))) =
                    (tokens.get(i), tokens.get(i + 1))
                {
                    match tokens.get(i + 2) {
                        Some(CmapToken::Hex(dst)) => {
                            let units = parse_utf16be_units(dst);
                            if lo.len() == 1 && hi.len() == 1 && lo[0] <= hi[0] {
                                cmap.ranges_1.push((lo[0], hi[0], units));
                            } else if lo.len() == 2 && hi.len() == 2 {
                                let s = u16::from_be_bytes([lo[0], lo[1]]);
                                let e = u16::from_be_bytes([hi[0], hi[1]]);
                                if s <= e {
                                    cmap.ranges_2.push((s, e, units));
                                }
                            }
                            i += 3;
                        }
                        Some(CmapToken::LBracket) => {
                            let mut j = i + 3;
                            let mut offset = 0u32;
                            while j < tokens.len() {
                                match &tokens[j] {
                                    CmapToken::RBracket => {
                                        j += 1;
                                        break;
                                    }
                                    CmapToken::Hex(dst) => {
                                        if total_inserted < MAX_CMAP_MAPPINGS {
                                            let mapped =
                                                String::from_utf16_lossy(&parse_utf16be_units(dst));
                                            if lo.len() == 1 && hi.len() == 1 {
                                                let code = u32::from(lo[0]) + offset;
                                                if code <= u32::from(hi[0]) {
                                                    if let Ok(c8) = u8::try_from(code) {
                                                        cmap.map_1.insert(c8, mapped);
                                                        total_inserted += 1;
                                                    }
                                                }
                                            } else if lo.len() == 2 && hi.len() == 2 {
                                                let start =
                                                    u32::from(u16::from_be_bytes([lo[0], lo[1]]));
                                                let end =
                                                    u32::from(u16::from_be_bytes([hi[0], hi[1]]));
                                                let code = start + offset;
                                                if code <= end {
                                                    if let Ok(c16) = u16::try_from(code) {
                                                        cmap.map_2.insert(c16, mapped);
                                                        total_inserted += 1;
                                                    }
                                                }
                                            }
                                        }
                                        offset = offset.saturating_add(1);
                                        j += 1;
                                    }
                                    _ => j += 1,
                                }
                            }
                            i = j;
                        }
                        _ => i += 1,
                    }
                } else {
                    i += 1;
                }
            }
        }
    }

    cmap
}

impl FontInfo {
    fn decode_single_byte(&self, b: u8) -> String {
        if let Some(mapped) = self.differences.get(&b) {
            return mapped.clone();
        }
        let ch = match self.base_encoding {
            BaseEncoding::MacRoman => macroman_char(b),
            BaseEncoding::Standard => standard_encoding_char(b),
            BaseEncoding::Default | BaseEncoding::WinAnsi | BaseEncoding::Identity => {
                win1252_char(b)
            }
        };
        ch.to_string()
    }

    fn decode_bytes(&self, raw: &[u8]) -> String {
        if let Some(cmap) = &self.to_unicode {
            return cmap.decode(raw, Some(self));
        }
        if raw.is_ascii() && self.differences.is_empty() {
            return std::str::from_utf8(raw).unwrap_or("").to_string();
        }
        if raw.len() >= 2 && raw[0] == 0xFE && raw[1] == 0xFF {
            let units: Vec<u16> = raw[2..]
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            return String::from_utf16_lossy(&units);
        }
        if self.base_encoding == BaseEncoding::Default && self.differences.is_empty() {
            if let Ok(utf8) = std::str::from_utf8(raw) {
                return utf8.to_string();
            }
        }
        let mut out = String::with_capacity(raw.len());
        for &b in raw {
            out.push_str(&self.decode_single_byte(b));
        }
        out
    }
}

fn decode_text_with_font(raw: &[u8], font: Option<&FontInfo>) -> String {
    if let Some(info) = font {
        return info.decode_bytes(raw);
    }
    if raw.len() >= 2 && raw[0] == 0xFE && raw[1] == 0xFF {
        let units: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if let Ok(utf8) = std::str::from_utf8(raw) {
        return utf8.to_string();
    }
    raw.iter().map(|&b| win1252_char(b)).collect()
}

fn win1252_char(b: u8) -> char {
    const TABLE_80_9F: [char; 32] = [
        '\u{20AC}', '\u{FFFD}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}',
        '\u{2021}', '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{FFFD}',
        '\u{017D}', '\u{FFFD}', '\u{FFFD}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}',
        '\u{2022}', '\u{2013}', '\u{2014}', '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}',
        '\u{0153}', '\u{FFFD}', '\u{017E}', '\u{0178}',
    ];
    if (0x80..=0x9F).contains(&b) {
        TABLE_80_9F[usize::from(b - 0x80)]
    } else {
        char::from(b)
    }
}

fn macroman_char(b: u8) -> char {
    const TABLE_80_FF: [char; 128] = [
        'Ä', 'Å', 'Ç', 'É', 'Ñ', 'Ö', 'Ü', 'á', 'à', 'â', 'ä', 'ã', 'å', 'ç', 'é', 'è', 'ê', 'ë',
        'í', 'ì', 'î', 'ï', 'ñ', 'ó', 'ò', 'ô', 'ö', 'õ', 'ú', 'ù', 'û', 'ü', '†', '°', '¢', '£',
        '§', '•', '¶', 'ß', '®', '©', '™', '´', '¨', '≠', 'Æ', 'Ø', '∞', '±', '≤', '≥', '¥', 'µ',
        '∂', '∑', '∏', 'π', '∫', 'ª', 'º', 'Ω', 'æ', 'ø', '¿', '¡', '¬', '√', 'ƒ', '≈', '∆', '«',
        '»', '…', '\u{00A0}', 'À', 'Ã', 'Õ', 'Œ', 'œ', '–', '—', '“', '”', '‘', '’', '÷', '◊', 'ÿ',
        'Ÿ', '⁄', '€', '‹', '›', 'ﬁ', 'ﬂ', '‡', '·', '‚', '„', '‰', 'Â', 'Ê', 'Á', 'Ë', 'È', 'Í',
        'Î', 'Ï', 'Ì', 'Ó', 'Ô', '\u{F8FF}', 'Ò', 'Ú', 'Û', 'Ù', 'ı', 'ˆ', '˜', '¯', '˘', '˙', '˚',
        '¸', '˝', '˛', 'ˇ',
    ];
    if b < 0x80 {
        char::from(b)
    } else {
        TABLE_80_FF[usize::from(b - 0x80)]
    }
}

fn standard_encoding_char(b: u8) -> char {
    match b {
        0x27 => '’',
        0x60 => '‘',
        0xA1 => '¡',
        0xA2 => '¢',
        0xA3 => '£',
        0xA4 => '⁄',
        0xA5 => '¥',
        0xA6 => 'ƒ',
        0xA7 => '§',
        0xA8 => '¤',
        0xA9 => '\'',
        0xAA => '“',
        0xAB => '«',
        0xAC => '‹',
        0xAD => '›',
        0xAE => 'ﬁ',
        0xAF => 'ﬂ',
        0xB1 => '–',
        0xB2 => '†',
        0xB3 => '‡',
        0xB4 => '·',
        0xB6 => '¶',
        0xB7 => '•',
        0xB8 => '‚',
        0xB9 => '„',
        0xBA => '”',
        0xBB => '»',
        0xBC => '…',
        0xBD => '‰',
        0xBF => '¿',
        0xD0 => '—',
        0xE1 => 'Æ',
        0xE3 => 'ª',
        0xE8 => 'Ł',
        0xE9 => 'Ø',
        0xEA => 'Œ',
        0xEB => 'º',
        0xF1 => 'æ',
        0xF5 => 'ı',
        0xF8 => 'ł',
        0xF9 => 'ø',
        0xFA => 'œ',
        0xFB => 'ß',
        _ => win1252_char(b),
    }
}

fn glyph_name_to_str(name: &str) -> Option<String> {
    if name.len() == 1 && name.as_bytes()[0].is_ascii_alphabetic() {
        return Some(name.to_string());
    }
    let static_hit = match name {
        "space" | "nonbreakingspace" | "nbspace" => Some(" "),
        "exclam" => Some("!"),
        "quotedbl" => Some("\""),
        "numbersign" => Some("#"),
        "dollar" => Some("$"),
        "percent" => Some("%"),
        "ampersand" => Some("&"),
        "quotesingle" => Some("'"),
        "parenleft" => Some("("),
        "parenright" => Some(")"),
        "asterisk" => Some("*"),
        "plus" => Some("+"),
        "comma" => Some(","),
        "hyphen" | "minus" => Some("-"),
        "period" => Some("."),
        "slash" => Some("/"),
        "zero" => Some("0"),
        "one" => Some("1"),
        "two" => Some("2"),
        "three" => Some("3"),
        "four" => Some("4"),
        "five" => Some("5"),
        "six" => Some("6"),
        "seven" => Some("7"),
        "eight" => Some("8"),
        "nine" => Some("9"),
        "colon" => Some(":"),
        "semicolon" => Some(";"),
        "less" => Some("<"),
        "equal" => Some("="),
        "greater" => Some(">"),
        "question" => Some("?"),
        "at" => Some("@"),
        "bracketleft" => Some("["),
        "backslash" => Some("\\"),
        "bracketright" => Some("]"),
        "asciicircum" => Some("^"),
        "underscore" => Some("_"),
        "grave" => Some("`"),
        "braceleft" => Some("{"),
        "bar" => Some("|"),
        "braceright" => Some("}"),
        "asciitilde" => Some("~"),
        "Euro" | "euro" => Some("€"),
        "sterling" => Some("£"),
        "yen" => Some("¥"),
        "cent" => Some("¢"),
        "currency" => Some("¤"),
        "endash" => Some("–"),
        "emdash" => Some("—"),
        "bullet" => Some("•"),
        "quoteleft" => Some("‘"),
        "quoteright" => Some("’"),
        "quotedblleft" => Some("“"),
        "quotedblright" => Some("”"),
        "quotesinglbase" => Some("‚"),
        "quotedblbase" => Some("„"),
        "guilsinglleft" => Some("‹"),
        "guilsinglright" => Some("›"),
        "guillemotleft" => Some("«"),
        "guillemotright" => Some("»"),
        "ellipsis" => Some("…"),
        "trademark" => Some("™"),
        "copyright" => Some("©"),
        "registered" => Some("®"),
        "degree" => Some("°"),
        "plusminus" => Some("±"),
        "multiply" => Some("×"),
        "divide" => Some("÷"),
        "section" => Some("§"),
        "paragraph" => Some("¶"),
        "dagger" => Some("†"),
        "daggerdbl" => Some("‡"),
        "perthousand" => Some("‰"),
        "periodcentered" => Some("·"),
        "fi" => Some("fi"),
        "fl" => Some("fl"),
        "ff" => Some("ff"),
        "ffi" => Some("ffi"),
        "ffl" => Some("ffl"),
        "germandbls" => Some("ß"),
        "Agrave" => Some("À"),
        "Aacute" => Some("Á"),
        "Acircumflex" => Some("Â"),
        "Atilde" => Some("Ã"),
        "Adieresis" => Some("Ä"),
        "Aring" => Some("Å"),
        "AE" => Some("Æ"),
        "Ccedilla" => Some("Ç"),
        "Egrave" => Some("È"),
        "Eacute" => Some("É"),
        "Ecircumflex" => Some("Ê"),
        "Edieresis" => Some("Ë"),
        "Igrave" => Some("Ì"),
        "Iacute" => Some("Í"),
        "Icircumflex" => Some("Î"),
        "Idieresis" => Some("Ï"),
        "Ntilde" => Some("Ñ"),
        "Ograve" => Some("Ò"),
        "Oacute" => Some("Ó"),
        "Ocircumflex" => Some("Ô"),
        "Otilde" => Some("Õ"),
        "Odieresis" => Some("Ö"),
        "Oslash" => Some("Ø"),
        "Ugrave" => Some("Ù"),
        "Uacute" => Some("Ú"),
        "Ucircumflex" => Some("Û"),
        "Udieresis" => Some("Ü"),
        "agrave" => Some("à"),
        "aacute" => Some("á"),
        "acircumflex" => Some("â"),
        "atilde" => Some("ã"),
        "adieresis" => Some("ä"),
        "aring" => Some("å"),
        "ae" => Some("æ"),
        "ccedilla" => Some("ç"),
        "egrave" => Some("è"),
        "eacute" => Some("é"),
        "ecircumflex" => Some("ê"),
        "edieresis" => Some("ë"),
        "igrave" => Some("ì"),
        "iacute" => Some("í"),
        "icircumflex" => Some("î"),
        "idieresis" => Some("ï"),
        "ntilde" => Some("ñ"),
        "ograve" => Some("ò"),
        "oacute" => Some("ó"),
        "ocircumflex" => Some("ô"),
        "otilde" => Some("õ"),
        "odieresis" => Some("ö"),
        "oslash" => Some("ø"),
        "ugrave" => Some("ù"),
        "uacute" => Some("ú"),
        "ucircumflex" => Some("û"),
        "udieresis" => Some("ü"),
        "ydieresis" => Some("ÿ"),
        _ => None,
    };
    if let Some(s) = static_hit {
        return Some(s.to_string());
    }
    if let Some(hex) = name.strip_prefix("uni") {
        if hex.len() == 4 {
            if let Ok(cp) = u32::from_str_radix(hex, 16) {
                if let Some(ch) = char::from_u32(cp) {
                    return Some(ch.to_string());
                }
            }
        }
    }
    if let Some(hex) = name.strip_prefix('u') {
        if (4..=6).contains(&hex.len()) {
            if let Ok(cp) = u32::from_str_radix(hex, 16) {
                if let Some(ch) = char::from_u32(cp) {
                    return Some(ch.to_string());
                }
            }
        }
    }
    None
}

// --- PDF Content Stream Tokenizer & Layout Execution ---

#[derive(Debug)]
enum Token {
    Str(Vec<u8>),
    Name(String),
    Array(Vec<ArrayItem>),
    Num(f64),
    Op(String),
}

#[derive(Debug)]
enum ArrayItem {
    Str(Vec<u8>),
    Num(f64),
}

#[derive(Clone, Debug)]
struct GraphicsState {
    ctm: [f64; 6],
    font_name: Option<String>,
    font_size: f64,
    leading: f64,
    horiz_scale: f64,
    char_space: f64,
    word_space: f64,
}

impl Default for GraphicsState {
    fn default() -> Self {
        Self {
            ctm: IDENTITY_MAT,
            font_name: None,
            font_size: 0.0,
            leading: 0.0,
            horiz_scale: 100.0,
            char_space: 0.0,
            word_space: 0.0,
        }
    }
}

#[derive(Debug)]
struct PositionedSpan {
    x: f64,
    y: f64,
    end_x: f64,
    font_size: f64,
    seq: usize,
    text: String,
}

struct ExecContext<'a, 'b> {
    objects: &'a BTreeMap<u32, PdfObject<'b>>,
    font_cache: &'a mut BTreeMap<u32, FontInfo>,
    budget: &'a mut DecodeBudget,
    active_xobjects: Vec<u32>,
    positioned_spans: Vec<PositionedSpan>,
    unpositioned_lines: Vec<String>,
    unpositioned_buf: String,
    next_seq: usize,
}

fn extract_page_lines(
    objects: &BTreeMap<u32, PdfObject<'_>>,
    stream: &[u8],
    page_res: &PageResources,
    font_cache: &mut BTreeMap<u32, FontInfo>,
    budget: &mut DecodeBudget,
) -> Result<Vec<String>, &'static str> {
    if stream.is_empty() {
        return Ok(Vec::new());
    }
    let mut ctx = ExecContext {
        objects,
        font_cache,
        budget,
        active_xobjects: Vec::new(),
        positioned_spans: Vec::new(),
        unpositioned_lines: Vec::new(),
        unpositioned_buf: String::new(),
        next_seq: 0,
    };
    execute_content_stream(stream, page_res, GraphicsState::default(), 0, &mut ctx)?;

    let mut out = cluster_positioned_spans(ctx.positioned_spans);
    out.extend(ctx.unpositioned_lines);
    Ok(out)
}

fn execute_content_stream(
    stream: &[u8],
    res: &PageResources,
    mut gs: GraphicsState,
    depth: usize,
    ctx: &mut ExecContext<'_, '_>,
) -> Result<(), &'static str> {
    let tokens = tokenize_content_stream(stream);
    let mut gs_stack: Vec<GraphicsState> = Vec::new();
    let mut operands: Vec<Token> = Vec::new();
    let mut in_text = false;
    let mut tm = IDENTITY_MAT;
    let mut tlm = IDENTITY_MAT;
    let mut has_bt_pos = false;

    for tok in tokens {
        match tok {
            Token::Op(op) => {
                match op.as_str() {
                    "q" => {
                        if gs_stack.len() < MAX_GS_DEPTH {
                            gs_stack.push(gs.clone());
                        }
                    }
                    "Q" => {
                        if let Some(prev) = gs_stack.pop() {
                            gs = prev;
                        }
                    }
                    "cm" => {
                        if operands.len() >= 6 {
                            let base = operands.len() - 6;
                            if let (
                                Token::Num(a),
                                Token::Num(b),
                                Token::Num(c),
                                Token::Num(d),
                                Token::Num(e),
                                Token::Num(f),
                            ) = (
                                &operands[base],
                                &operands[base + 1],
                                &operands[base + 2],
                                &operands[base + 3],
                                &operands[base + 4],
                                &operands[base + 5],
                            ) {
                                gs.ctm = mat_mul([*a, *b, *c, *d, *e, *f], gs.ctm);
                            }
                        }
                    }
                    "Tf" => {
                        if operands.len() >= 2 {
                            let base = operands.len() - 2;
                            if let (Token::Name(name), Token::Num(size)) =
                                (&operands[base], &operands[base + 1])
                            {
                                gs.font_name = Some(name.clone());
                                gs.font_size = *size;
                            }
                        }
                    }
                    "TL" => {
                        if let Some(Token::Num(leading)) = operands.last() {
                            gs.leading = *leading;
                        }
                    }
                    "Tc" => {
                        if let Some(Token::Num(cs)) = operands.last() {
                            gs.char_space = *cs;
                        }
                    }
                    "Tw" => {
                        if let Some(Token::Num(ws)) = operands.last() {
                            gs.word_space = *ws;
                        }
                    }
                    "Tz" => {
                        if let Some(Token::Num(scale)) = operands.last() {
                            gs.horiz_scale = *scale;
                        }
                    }
                    "BT" => {
                        flush_unpositioned(ctx);
                        in_text = true;
                        tm = IDENTITY_MAT;
                        tlm = IDENTITY_MAT;
                        has_bt_pos = false;
                    }
                    "ET" => {
                        flush_unpositioned(ctx);
                        in_text = false;
                    }
                    "Td" if in_text => {
                        if operands.len() >= 2 {
                            let base = operands.len() - 2;
                            if let (Token::Num(tx), Token::Num(ty)) =
                                (&operands[base], &operands[base + 1])
                            {
                                tlm = mat_mul([1.0, 0.0, 0.0, 1.0, *tx, *ty], tlm);
                                tm = tlm;
                                has_bt_pos = true;
                            }
                        }
                    }
                    "TD" if in_text => {
                        if operands.len() >= 2 {
                            let base = operands.len() - 2;
                            if let (Token::Num(tx), Token::Num(ty)) =
                                (&operands[base], &operands[base + 1])
                            {
                                gs.leading = -*ty;
                                tlm = mat_mul([1.0, 0.0, 0.0, 1.0, *tx, *ty], tlm);
                                tm = tlm;
                                has_bt_pos = true;
                            }
                        }
                    }
                    "Tm" if in_text => {
                        if operands.len() >= 6 {
                            let base = operands.len() - 6;
                            if let (
                                Token::Num(a),
                                Token::Num(b),
                                Token::Num(c),
                                Token::Num(d),
                                Token::Num(e),
                                Token::Num(f),
                            ) = (
                                &operands[base],
                                &operands[base + 1],
                                &operands[base + 2],
                                &operands[base + 3],
                                &operands[base + 4],
                                &operands[base + 5],
                            ) {
                                tlm = [*a, *b, *c, *d, *e, *f];
                                tm = tlm;
                                has_bt_pos = true;
                            }
                        }
                    }
                    "T*" if in_text => {
                        advance_next_line(&gs, has_bt_pos, &mut tlm, &mut tm, ctx);
                    }
                    "Tj" if in_text => {
                        if let Some(Token::Str(raw)) = operands.last() {
                            let font = res.resolve_font(gs.font_name.as_deref(), ctx.font_cache);
                            let text = decode_text_with_font(raw, font);
                            record_text_run(&text, 0.0, &gs, has_bt_pos, &mut tm, ctx);
                        }
                    }
                    "'" if in_text => {
                        advance_next_line(&gs, has_bt_pos, &mut tlm, &mut tm, ctx);
                        if let Some(Token::Str(raw)) = operands.last() {
                            let font = res.resolve_font(gs.font_name.as_deref(), ctx.font_cache);
                            let text = decode_text_with_font(raw, font);
                            record_text_run(&text, 0.0, &gs, has_bt_pos, &mut tm, ctx);
                        }
                    }
                    "\"" if in_text => {
                        if operands.len() >= 3 {
                            let base = operands.len() - 3;
                            if let (Token::Num(aw), Token::Num(ac), Token::Str(raw)) =
                                (&operands[base], &operands[base + 1], &operands[base + 2])
                            {
                                gs.word_space = *aw;
                                gs.char_space = *ac;
                                advance_next_line(&gs, has_bt_pos, &mut tlm, &mut tm, ctx);
                                let font =
                                    res.resolve_font(gs.font_name.as_deref(), ctx.font_cache);
                                let text = decode_text_with_font(raw, font);
                                record_text_run(&text, 0.0, &gs, has_bt_pos, &mut tm, ctx);
                            }
                        }
                    }
                    "TJ" if in_text => {
                        if let Some(Token::Array(items)) = operands.last() {
                            let font = res.resolve_font(gs.font_name.as_deref(), ctx.font_cache);
                            let mut combined = String::new();
                            let mut pending_space = false;
                            let mut kern_units = 0.0;
                            for item in items {
                                match item {
                                    ArrayItem::Str(raw) => {
                                        let piece = decode_text_with_font(raw, font);
                                        if pending_space
                                            && !combined.is_empty()
                                            && !combined.ends_with(' ')
                                            && !piece.starts_with(' ')
                                        {
                                            combined.push(' ');
                                        }
                                        pending_space = false;
                                        combined.push_str(&piece);
                                    }
                                    ArrayItem::Num(offset) => {
                                        kern_units += -*offset / 1000.0;
                                        if *offset < -180.0 {
                                            pending_space = true;
                                        }
                                    }
                                }
                            }
                            record_text_run(&combined, kern_units, &gs, has_bt_pos, &mut tm, ctx);
                        }
                    }
                    "Do" => {
                        if let Some(Token::Name(xobj_name)) = operands.last() {
                            if let Some(&xobj_num) = res.xobjects.get(xobj_name) {
                                flush_unpositioned(ctx);
                                invoke_form_xobject(xobj_num, res, &gs, depth, ctx)?;
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

    flush_unpositioned(ctx);
    Ok(())
}

fn invoke_form_xobject(
    xobj_num: u32,
    parent_res: &PageResources,
    gs: &GraphicsState,
    depth: usize,
    ctx: &mut ExecContext<'_, '_>,
) -> Result<(), &'static str> {
    if depth >= MAX_XOBJECT_DEPTH || ctx.active_xobjects.contains(&xobj_num) {
        return Ok(());
    }
    let Some(xobj) = ctx.objects.get(&xobj_num) else {
        return Ok(());
    };
    if !dict_has_name(&xobj.dict, b"/Subtype", b"/Form") {
        return Ok(());
    }
    let Some(raw) = xobj.stream else {
        return Ok(());
    };
    let decoded = decode_stream(ctx.objects, &xobj.dict, raw, ctx.budget)?;
    if decoded.is_empty() {
        return Ok(());
    }

    let form_matrix = dict_lookup(&xobj.dict, b"/Matrix")
        .map(|m| parse_matrix(ctx.objects, m))
        .unwrap_or(IDENTITY_MAT);
    let mut child_gs = gs.clone();
    child_gs.ctm = mat_mul(form_matrix, gs.ctm);

    let mut owned_res = None;
    if let Some(res_val) = dict_lookup(&xobj.dict, b"/Resources") {
        let mut child_res = PageResources {
            fonts: parent_res.fonts.clone(),
            xobjects: parent_res.xobjects.clone(),
        };
        load_resources_from_slice(
            ctx.objects,
            res_val,
            ctx.font_cache,
            ctx.budget,
            &mut child_res,
        )?;
        owned_res = Some(child_res);
    }
    let active_res = owned_res.as_ref().unwrap_or(parent_res);

    ctx.active_xobjects.push(xobj_num);
    let res_exec = execute_content_stream(&decoded, active_res, child_gs, depth + 1, ctx);
    ctx.active_xobjects.pop();
    res_exec
}

fn parse_matrix(objects: &BTreeMap<u32, PdfObject<'_>>, slice: &[u8]) -> [f64; 6] {
    let resolved = resolve_ref_slice(objects, slice);
    let elems = parse_array_elements(resolved);
    if elems.len() != 6 {
        return IDENTITY_MAT;
    }
    let mut m = IDENTITY_MAT;
    for (idx, elem) in elems.iter().enumerate() {
        let Ok(v) = std::str::from_utf8(trim_pdf_ws(elem))
            .unwrap_or("")
            .parse::<f64>()
        else {
            return IDENTITY_MAT;
        };
        if !v.is_finite() {
            return IDENTITY_MAT;
        }
        m[idx] = v;
    }
    m
}

fn mat_mul(m1: [f64; 6], m2: [f64; 6]) -> [f64; 6] {
    [
        m1[0] * m2[0] + m1[1] * m2[2],
        m1[0] * m2[1] + m1[1] * m2[3],
        m1[2] * m2[0] + m1[3] * m2[2],
        m1[2] * m2[1] + m1[3] * m2[3],
        m1[4] * m2[0] + m1[5] * m2[2] + m2[4],
        m1[4] * m2[1] + m1[5] * m2[3] + m2[5],
    ]
}

fn default_line_step(font_size: f64) -> f64 {
    let abs = font_size.abs();
    if abs > 1e-6 {
        abs * 1.2
    } else {
        12.0
    }
}

fn advance_next_line(
    gs: &GraphicsState,
    has_bt_pos: bool,
    tlm: &mut [f64; 6],
    tm: &mut [f64; 6],
    ctx: &mut ExecContext<'_, '_>,
) {
    if gs.leading.abs() > 1e-6 {
        *tlm = mat_mul([1.0, 0.0, 0.0, 1.0, 0.0, -gs.leading], *tlm);
        *tm = *tlm;
    } else if has_bt_pos {
        let step = default_line_step(gs.font_size);
        *tlm = mat_mul([1.0, 0.0, 0.0, 1.0, 0.0, -step], *tlm);
        *tm = *tlm;
    } else {
        flush_unpositioned(ctx);
    }
}

fn flush_unpositioned(ctx: &mut ExecContext<'_, '_>) {
    let trimmed = ctx.unpositioned_buf.trim();
    if !trimmed.is_empty() {
        ctx.unpositioned_lines.push(trimmed.to_string());
    }
    ctx.unpositioned_buf.clear();
}

fn approx_char_em(c: char) -> f64 {
    match c {
        ' ' => 0.28,
        'i' | 'l' | 'I' | 'j' | '!' | '|' | '.' | ',' | ':' | ';' | '\'' | '`' => 0.28,
        'f' | 'r' | 't' | '(' | ')' | '[' | ']' | '-' | '/' | '\\' => 0.34,
        'm' | 'w' => 0.78,
        'M' | 'W' | '@' | '%' => 0.90,
        'J' => 0.36,
        'C' | 'D' | 'G' | 'H' | 'N' | 'O' | 'Q' | 'R' | 'U' => 0.74,
        c if c.is_ascii_uppercase() => 0.67,
        c if c.is_ascii_digit() => 0.56,
        c if u32::from(c) >= 0x2E80 => 1.0,
        _ => 0.54,
    }
}

fn record_text_run(
    text: &str,
    kern_em: f64,
    gs: &GraphicsState,
    has_bt_pos: bool,
    tm: &mut [f64; 6],
    ctx: &mut ExecContext<'_, '_>,
) {
    if text.is_empty() {
        return;
    }
    let is_positioned = has_bt_pos || gs.ctm != IDENTITY_MAT;
    if !is_positioned {
        ctx.unpositioned_buf.push_str(text);
        return;
    }

    let trm_start = mat_mul(*tm, gs.ctm);
    let fs = if gs.font_size.abs() > 1e-6 {
        gs.font_size.abs()
    } else {
        10.0
    };
    let h_scale = if gs.horiz_scale.abs() > 1e-6 {
        gs.horiz_scale.abs() / 100.0
    } else {
        1.0
    };

    let mut local_w = kern_em * fs;
    for ch in text.chars() {
        local_w += approx_char_em(ch) * fs + gs.char_space;
        if ch == ' ' {
            local_w += gs.word_space;
        }
    }
    local_w *= h_scale;

    tm[4] += local_w * tm[0];
    tm[5] += local_w * tm[1];
    let trm_end = mat_mul(*tm, gs.ctm);

    let x = trm_start[4];
    let y = trm_start[5];
    let end_x = trm_end[4];
    let page_scale = trm_start[0].hypot(trm_start[1]);
    let page_font_size = fs * h_scale * if page_scale > 1e-6 { page_scale } else { 1.0 };

    if !x.is_finite() || !y.is_finite() || !end_x.is_finite() || !page_font_size.is_finite() {
        return;
    }
    if text.trim().is_empty() {
        return;
    }

    let seq = ctx.next_seq;
    ctx.next_seq += 1;
    ctx.positioned_spans.push(PositionedSpan {
        x,
        y,
        end_x: end_x.max(x),
        font_size: page_font_size,
        seq,
        text: text.to_string(),
    });
}

fn cluster_positioned_spans(mut spans: Vec<PositionedSpan>) -> Vec<String> {
    if spans.is_empty() {
        return Vec::new();
    }
    spans.sort_by(|a, b| b.y.total_cmp(&a.y).then_with(|| a.seq.cmp(&b.seq)));

    let mut rows: Vec<Vec<PositionedSpan>> = Vec::new();
    let mut row_anchor_y: Vec<f64> = Vec::new();

    for span in spans {
        if let Some(&anchor) = row_anchor_y.last() {
            if (anchor - span.y).abs() <= 2.0 {
                if let Some(last_row) = rows.last_mut() {
                    last_row.push(span);
                }
                continue;
            }
        }
        row_anchor_y.push(span.y);
        rows.push(vec![span]);
    }

    let mut lines = Vec::with_capacity(rows.len());
    for mut row in rows {
        row.sort_by(|a, b| a.x.total_cmp(&b.x).then_with(|| a.seq.cmp(&b.seq)));
        let mut line = String::new();
        let mut prev_end_x: Option<f64> = None;
        let mut prev_fs: f64 = 10.0;

        for span in row {
            let trimmed = span.text.trim_matches('\0');
            if trimmed.trim().is_empty() {
                continue;
            }
            if let Some(end_x) = prev_end_x {
                let gap = span.x - end_x;
                let threshold = prev_fs.max(span.font_size) * 0.16;
                if gap > threshold && !line.ends_with(' ') && !trimmed.starts_with(' ') {
                    line.push(' ');
                }
            }
            line.push_str(trimmed);
            prev_end_x = Some(span.end_x);
            prev_fs = span.font_size;
        }

        let trimmed_line = line.trim();
        if !trimmed_line.is_empty() {
            lines.push(trimmed_line.to_string());
        }
    }
    lines
}

fn tokenize_content_stream(stream: &[u8]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let n = stream.len();

    while i < n {
        let b = stream[i];
        if is_pdf_ws(b) {
            i += 1;
            continue;
        }
        if b == b'%' {
            while i < n && stream[i] != b'\n' && stream[i] != b'\r' {
                i += 1;
            }
            continue;
        }
        if b == b'(' {
            let (raw, next) = parse_pdf_literal_string(stream, i);
            out.push(Token::Str(raw));
            i = next;
            continue;
        }
        if b == b'<' {
            if i + 1 < n && stream[i + 1] == b'<' {
                i += 2;
                continue;
            }
            let (raw, next) = parse_pdf_hex_string(stream, i);
            out.push(Token::Str(raw));
            i = next;
            continue;
        }
        if b == b'>' && i + 1 < n && stream[i + 1] == b'>' {
            i += 2;
            continue;
        }
        if b == b'[' {
            let (items, next) = parse_pdf_array(stream, i);
            out.push(Token::Array(items));
            i = next;
            continue;
        }
        if b == b'/' {
            let name_start = i;
            i += 1;
            while i < n && !is_pdf_delimiter(stream[i]) {
                i += 1;
            }
            if let Some(name) = decode_pdf_name(&stream[name_start..i]) {
                out.push(Token::Name(name));
            }
            continue;
        }
        if b == b'\'' || b == b'"' {
            out.push(Token::Op(char::from(b).to_string()));
            i += 1;
            continue;
        }
        let start = i;
        while i < n && !is_pdf_delimiter(stream[i]) {
            i += 1;
        }
        if i == start {
            i += 1;
            continue;
        }
        let word = std::str::from_utf8(&stream[start..i]).unwrap_or("");
        if word == "BI" {
            i = skip_inline_image(stream, i);
            out.push(Token::Op("BI_SKIP".to_string()));
            continue;
        }
        if let Ok(num) = word.parse::<f64>() {
            out.push(Token::Num(num));
        } else if !word.is_empty() {
            out.push(Token::Op(word.to_string()));
        }
    }

    out
}

fn skip_inline_image(stream: &[u8], start: usize) -> usize {
    let mut i = start;
    let n = stream.len();
    let mut data_start = None;

    while i < n {
        let b = stream[i];
        if b == b'(' {
            i = skip_pdf_literal_string(stream, i);
            continue;
        }
        if b == b'<' && !stream[i..].starts_with(b"<<") {
            i = skip_pdf_hex_string(stream, i);
            continue;
        }
        if b == b'/' {
            i += 1;
            while i < n && !is_pdf_delimiter(stream[i]) {
                i += 1;
            }
            continue;
        }
        if b == b'%' {
            while i < n && stream[i] != b'\n' && stream[i] != b'\r' {
                i += 1;
            }
            continue;
        }
        if stream[i..].starts_with(b"ID")
            && (i == 0 || is_pdf_delimiter(stream[i - 1]))
            && i + 2 < n
            && is_pdf_ws(stream[i + 2])
        {
            data_start = Some(i + 3);
            break;
        }
        i += 1;
    }

    let Some(mut p) = data_start else {
        return n;
    };
    while p + 1 < n {
        if stream[p..].starts_with(b"EI") {
            let before_ok = p > start && (is_pdf_ws(stream[p - 1]) || stream[p - 1] == b'>');
            let after_ok = p + 2 >= n || is_pdf_delimiter(stream[p + 2]);
            if before_ok && after_ok {
                return p + 2;
            }
        }
        p += 1;
    }
    n
}

fn parse_pdf_literal_string(stream: &[u8], start: usize) -> (Vec<u8>, usize) {
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
                    let mut val = u32::from(c - b'0');
                    for _ in 0..2 {
                        if i + 1 < stream.len() && matches!(stream[i + 1], b'0'..=b'7') {
                            i += 1;
                            val = (val << 3) | u32::from(stream[i] - b'0');
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
    (raw, i)
}

fn parse_pdf_hex_string(stream: &[u8], start: usize) -> (Vec<u8>, usize) {
    let mut i = start + 1;
    let mut nibbles = Vec::new();
    while i < stream.len() && stream[i] != b'>' {
        if let Some(v) = hex_nibble(stream[i]) {
            nibbles.push(v);
        }
        i += 1;
    }
    if i < stream.len() && stream[i] == b'>' {
        i += 1;
    }
    let mut bytes = Vec::with_capacity(nibbles.len().div_ceil(2));
    for chunk in nibbles.chunks(2) {
        let hi = chunk[0];
        let lo = chunk.get(1).copied().unwrap_or(0);
        bytes.push((hi << 4) | lo);
    }
    (bytes, i)
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
        if is_pdf_ws(b) {
            i += 1;
            continue;
        }
        if b == b'%' {
            while i < stream.len() && stream[i] != b'\n' && stream[i] != b'\r' {
                i += 1;
            }
            continue;
        }
        if b == b'(' {
            let (raw, next) = parse_pdf_literal_string(stream, i);
            items.push(ArrayItem::Str(raw));
            i = next;
            continue;
        }
        if b == b'<' {
            let (raw, next) = parse_pdf_hex_string(stream, i);
            items.push(ArrayItem::Str(raw));
            i = next;
            continue;
        }
        if b == b'[' {
            i = find_array_bounds(stream, i).unwrap_or(stream.len());
            continue;
        }
        if b == b'/' {
            i += 1;
            while i < stream.len() && !is_pdf_delimiter(stream[i]) {
                i += 1;
            }
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

    fn build_pdf_from_objects(objs: &[(u32, Vec<u8>)], trailer_extra: &str) -> Vec<u8> {
        let mut out = b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n".to_vec();
        for (num, body) in objs {
            out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        if !trailer_extra.is_empty() {
            out.extend_from_slice(trailer_extra.as_bytes());
        }
        out.extend_from_slice(b"%%EOF\n");
        out
    }

    fn stream_obj(dict_inner: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!("<< {dict_inner} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendstream");
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

    #[test]
    fn extracts_objstm_compressed_catalog_pages_and_page_objects() {
        let obj1 = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
        let obj2 =
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /Resources << /Font << /F1 5 0 R >> >> >>"
                .to_vec();
        let obj3 = b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec();
        let obj5 = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec();

        let mut body = Vec::new();
        let mut offsets = Vec::new();
        for (num, data) in [(1u32, &obj1), (2, &obj2), (3, &obj3), (5, &obj5)] {
            offsets.push((num, body.len()));
            body.extend_from_slice(data);
            body.push(b'\n');
        }
        let header: String = offsets
            .iter()
            .map(|(n, off)| format!("{n} {off} "))
            .collect();
        let first = header.len();
        let mut objstm_raw = header.into_bytes();
        objstm_raw.extend_from_slice(&body);
        let objstm_compressed = miniz_oxide::deflate::compress_to_vec_zlib(&objstm_raw, 6);

        let content = b"BT /F1 11 Tf 54 740 Td (Amount paid USD 42.00) Tj ET\n";
        let content_compressed = miniz_oxide::deflate::compress_to_vec_zlib(content, 6);

        let pdf = build_pdf_from_objects(
            &[
                (
                    10,
                    stream_obj(
                        &format!("/Type /ObjStm /N 4 /First {first} /Filter /FlateDecode"),
                        &objstm_compressed,
                    ),
                ),
                (4, stream_obj("/Filter /FlateDecode", &content_compressed)),
            ],
            "",
        );

        let pages = extract_pages_for_redaction(&pdf).expect("objstm pdf");
        assert_eq!(pages, vec![(1, vec!["Amount paid USD 42.00".to_string()])]);
        let res = extract(&pdf);
        assert_eq!(res.status, "matched");
        assert_eq!(
            res.candidates[res.adopt_amount_idx.unwrap()].value["amount_text"],
            "42.00"
        );
    }

    #[test]
    fn decodes_flate_stream_with_png_predictors_and_respects_numeric_length() {
        let cols = 5usize;
        let payload: Vec<u8> = b"BT\n(Amount paid USD 18.25)\nTj\nET\n".to_vec();
        let mut padded = payload.clone();
        while padded.len() % cols != 0 {
            padded.push(b' ');
        }
        let rows: Vec<&[u8]> = padded.chunks(cols).collect();
        let mut encoded = Vec::new();
        let mut prev = vec![0u8; cols];
        for (idx, row) in rows.iter().enumerate() {
            let filter = u8::try_from(idx % 5).unwrap();
            encoded.push(filter);
            for c in 0..cols {
                let pred = match filter {
                    0 => 0,
                    1 => {
                        if c >= 1 {
                            row[c - 1]
                        } else {
                            0
                        }
                    }
                    2 => prev[c],
                    3 => {
                        let left = if c >= 1 { u16::from(row[c - 1]) } else { 0 };
                        let up = u16::from(prev[c]);
                        u8::try_from((left + up) / 2).unwrap()
                    }
                    4 => {
                        let a = if c >= 1 { row[c - 1] } else { 0 };
                        let b = prev[c];
                        let cc = if c >= 1 { prev[c - 1] } else { 0 };
                        paeth_predictor(a, b, cc)
                    }
                    _ => unreachable!(),
                };
                encoded.push(row[c].wrapping_sub(pred));
            }
            prev = row.to_vec();
        }
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&encoded, 6);

        let pdf = build_pdf_from_objects(
            &[
                (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
                (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
                (
                    3,
                    b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
                ),
                (
                    4,
                    stream_obj(
                        "/Filter [/FlateDecode] /DecodeParms [<< /Predictor 12 /Columns 5 >>]",
                        &compressed,
                    ),
                ),
            ],
            "",
        );

        let res = extract(&pdf);
        assert_eq!(res.status, "matched");
        assert_eq!(
            res.candidates[res.adopt_amount_idx.unwrap()].value["amount_text"],
            "18.25"
        );

        // Also ensure numeric /Length handles stream payload containing literal "endstream endobj"
        let decoy_stream = b"BT (endstream endobj Amount paid USD 21.00) Tj ET";
        let pdf_decoy = build_pdf_from_objects(
            &[
                (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
                (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
                (
                    3,
                    b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
                ),
                (4, stream_obj("", decoy_stream)),
            ],
            "",
        );
        let pages = extract_pages_for_redaction(&pdf_decoy).expect("numeric length");
        assert_eq!(
            pages,
            vec![(
                1,
                vec!["endstream endobj Amount paid USD 21.00".to_string()]
            )]
        );
    }

    #[test]
    fn decodes_tounicode_cmap_chinese_bfchar_and_bfrange() {
        let cmap = br#"begincodespacerange
<0000> <FFFF>
endcodespacerange
4 beginbfchar
<0001> <4EF7>
<0002> <7A0E>
<0003> <5408>
<0004> <8BA1>
endbfchar
2 beginbfrange
<0010> <0012> [<4EBA> <6C11> <5E01>]
<0020> <0029> <0030>
endbfrange
2 beginbfchar
<0030> <002E>
<0031> <00A5>
endbfchar"#;
        let cmap_z = miniz_oxide::deflate::compress_to_vec_zlib(cmap, 6);
        // <0001><0002><0003><0004> -> 价税合计 ; <0010><0011><0012><0031><0029><0028><0030><0020><0020> -> 人民币¥98.00
        let content = b"BT /C2_0 10 Tf 1 0 0 1 50 700 Tm <0001000200030004> Tj 1 0 0 1 130 700 Tm <001000110012003100290028003000200020> Tj ET\n";

        let pdf = build_pdf_from_objects(
            &[
                (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
                (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
                (
                    3,
                    b"<< /Type /Page /Parent 2 0 R /Resources << /Font << /C2_0 5 0 R >> >> /Contents 4 0 R >>"
                        .to_vec(),
                ),
                (4, stream_obj("", content)),
                (
                    5,
                    b"<< /Type /Font /Subtype /Type0 /Encoding /Identity-H /ToUnicode 6 0 R >>"
                        .to_vec(),
                ),
                (6, stream_obj("/Filter /FlateDecode", &cmap_z)),
            ],
            "",
        );

        let pages = extract_pages_for_redaction(&pdf).expect("tounicode chinese");
        assert_eq!(pages, vec![(1, vec!["价税合计 人民币¥98.00".to_string()])]);
    }

    #[test]
    fn decodes_winansi_euro_endash_and_differences() {
        let content = b"BT /F1 11 Tf 50 700 Td (Plan \x96 \x8019.00) Tj /F2 11 Tf 0 -15 Td (Fee \x814.50) Tj ET\n";
        let pdf = build_pdf_from_objects(
            &[
                (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
                (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
                (
                    3,
                    b"<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> /Contents 4 0 R >>"
                        .to_vec(),
                ),
                (4, stream_obj("", content)),
                (
                    5,
                    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
                        .to_vec(),
                ),
                (
                    6,
                    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding << /Type /Encoding /BaseEncoding /WinAnsiEncoding /Differences [129 /sterling] >> >>"
                        .to_vec(),
                ),
            ],
            "",
        );

        let pages = extract_pages_for_redaction(&pdf).expect("winansi");
        assert_eq!(
            pages,
            vec![(
                1,
                vec!["Plan – €19.00".to_string(), "Fee £4.50".to_string()]
            )]
        );
    }

    #[test]
    fn concatenates_multi_contents_and_skips_inline_images() {
        let part1 = b"BT /F1 12 Tf 1 0 0 1 50 720 Tm (Amount paid ) Tj\nBI /W 2 /H 2 /BPC 8 /CS /G ID \x00(BT (DECOY) Tj ET)\xff EI\n";
        let part2 = b"(USD 77.00) Tj ET\n";

        let pdf = build_pdf_from_objects(
            &[
                (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
                (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
                (
                    3,
                    b"<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 6 0 R >> >> /Contents [4 0 R 5 0 R] >>"
                        .to_vec(),
                ),
                (4, stream_obj("", part1)),
                (5, stream_obj("", part2)),
                (
                    6,
                    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
                ),
            ],
            "",
        );

        let res = extract(&pdf);
        assert_eq!(res.status, "matched");
        assert_eq!(
            res.candidates[res.adopt_amount_idx.unwrap()].value["amount_text"],
            "77.00"
        );
        let pages = extract_pages_for_redaction(&pdf).expect("multi contents");
        assert_eq!(pages, vec![(1, vec!["Amount paid USD 77.00".to_string()])]);
    }

    #[test]
    fn expands_form_xobjects_with_cycle_guard_and_merges_same_row_columns() {
        let page_stream = b"q 1 0 0 1 0 0 cm /Fm0 Do Q\nBT /F1 10 Tf 1 0 0 1 50 700 Tm (Total Paid) Tj ET\nBT /F1 10 Tf 1 0 0 1 400 700 Tm ($49.00) Tj ET\n";
        let form_stream =
            b"BT /F1 10 Tf 1 0 0 1 50 740 Tm (Amount paid USD 49.00) Tj ET\n/Fm0 Do\n";

        let pdf = build_pdf_from_objects(
            &[
                (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
                (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
                (
                    3,
                    b"<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 6 0 R >> /XObject << /Fm0 5 0 R >> >> /Contents 4 0 R >>"
                        .to_vec(),
                ),
                (4, stream_obj("", page_stream)),
                (
                    5,
                    stream_obj(
                        "/Type /XObject /Subtype /Form /BBox [0 0 600 800] /Resources << /XObject << /Fm0 5 0 R >> >>",
                        form_stream,
                    ),
                ),
                (
                    6,
                    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
                ),
            ],
            "",
        );

        let pages = extract_pages_for_redaction(&pdf).expect("xobject");
        assert_eq!(
            pages,
            vec![(
                1,
                vec![
                    "Amount paid USD 49.00".to_string(),
                    "Total Paid $49.00".to_string(),
                ]
            )]
        );
        let res = extract(&pdf);
        assert_eq!(res.status, "matched");
        assert_eq!(
            res.candidates[res.adopt_amount_idx.unwrap()].value["amount_text"],
            "49.00"
        );
    }

    #[test]
    fn detects_encrypt_inside_xref_stream_dict() {
        let pdf = build_pdf_from_objects(
            &[
                (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
                (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
                (
                    3,
                    b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
                ),
                (4, stream_obj("", b"BT (Amount paid USD 10.00) Tj ET")),
                (
                    9,
                    stream_obj("/Type /XRef /Size 10 /Root 1 0 R /Encrypt 10 0 R", b""),
                ),
            ],
            "",
        );
        let res = extract(&pdf);
        assert_eq!(res.status, "unsupported");
        assert_eq!(res.reason, Some("encrypted_pdf"));
    }
}
