//! Deterministic, local-only redacted template exporter (`subvault redact`).
//!
//! Produces a human-inspectable UTF-8 template with PII, credentials, URLs,
//! emails, IDs, dates, monetary values, and user-specified `--redact-term`
//! strings replaced by stable numbered placeholders while preserving layout
//! structure (HTML tables/tags, CSV columns, billing labels, currency symbols).
//!
//! PDFs are converted through static text extraction (`pdf_parse`), so raw PDF
//! binary dictionaries, XMP metadata, and embedded streams are never emitted.
//! Stdout JSON reports counts and output line numbers only — never raw PII.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::crypto::{AuthInput, CryptoError, ProtectionMode};
use crate::facts;
use crate::ids::{hex_encode, is_prefixed_id};
use crate::model::{
    apply_format_honesty, cipher_tamper_fail, honesty, io_fail, unseen_db, Fail, Report,
    EXIT_INTEGRITY, EXIT_OK, EXIT_RECOVERY, EXIT_REJECTED, EXIT_USAGE,
};
use crate::pdf_parse;
use crate::store;

const REDACTION_RULES_VERSION: &str = "redact-template-1";
const MAX_REDACT_BYTES: u64 = 8 * 1024 * 1024;

pub struct RedactRequest {
    pub source: Option<PathBuf>,
    pub vault: Option<PathBuf>,
    pub source_id: Option<String>,
    pub out: PathBuf,
    pub redact_terms: Vec<String>,
    pub auth: AuthInput,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputKind {
    Pdf,
    Html,
    Text,
}

impl InputKind {
    fn as_str(self) -> &'static str {
        match self {
            InputKind::Pdf => "pdf",
            InputKind::Html => "html",
            InputKind::Text => "text",
        }
    }
}

struct LoadedInput {
    bytes: Vec<u8>,
    media_type: Option<String>,
    input_mode: &'static str,
    source_id: Option<String>,
    vault_format: Option<i64>,
    protection: Option<ProtectionMode>,
}

#[derive(Default)]
struct RedactionCounts {
    custom_terms: usize,
    header_fields: usize,
    emails: usize,
    urls_and_ips: usize,
    identifiers: usize,
    amounts: usize,
    dates: usize,
    stripped_scripts: usize,
    stripped_styles: usize,
    stripped_comments: usize,
}

impl RedactionCounts {
    fn total_redactions(&self) -> usize {
        self.custom_terms
            + self.header_fields
            + self.emails
            + self.urls_and_ips
            + self.identifiers
            + self.amounts
            + self.dates
            + self.stripped_scripts
            + self.stripped_styles
            + self.stripped_comments
    }
}

struct Redactor {
    terms_sorted: Vec<String>,
    term_map: BTreeMap<String, usize>,
    field_map: BTreeMap<String, usize>,
    email_map: BTreeMap<String, usize>,
    url_map: BTreeMap<String, usize>,
    id_map: BTreeMap<String, usize>,
    amount_map: BTreeMap<String, usize>,
    date_map: BTreeMap<String, usize>,
    counts: RedactionCounts,
}

impl Redactor {
    fn new(mut terms: Vec<String>) -> Self {
        terms.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        terms.dedup();
        Self {
            terms_sorted: terms,
            term_map: BTreeMap::new(),
            field_map: BTreeMap::new(),
            email_map: BTreeMap::new(),
            url_map: BTreeMap::new(),
            id_map: BTreeMap::new(),
            amount_map: BTreeMap::new(),
            date_map: BTreeMap::new(),
            counts: RedactionCounts::default(),
        }
    }

    fn slot(map: &mut BTreeMap<String, usize>, key: &str) -> usize {
        let norm = key.trim().to_ascii_lowercase();
        let next = map.len() + 1;
        *map.entry(norm).or_insert(next)
    }

    fn term_placeholder(&mut self, matched: &str) -> String {
        self.counts.custom_terms += 1;
        let idx = Self::slot(&mut self.term_map, matched);
        format!("[REDACTED_TERM_{idx}]")
    }

    fn field_placeholder(&mut self, value: &str) -> String {
        self.counts.header_fields += 1;
        let idx = Self::slot(&mut self.field_map, value);
        format!("[REDACTED_FIELD_{idx}]")
    }

    fn email_placeholder(&mut self, value: &str) -> String {
        self.counts.emails += 1;
        let idx = Self::slot(&mut self.email_map, value);
        format!("[REDACTED_EMAIL_{idx}]")
    }

    fn url_placeholder(&mut self, value: &str) -> String {
        self.counts.urls_and_ips += 1;
        let idx = Self::slot(&mut self.url_map, value);
        format!("[REDACTED_URL_{idx}]")
    }

    fn id_placeholder(&mut self, value: &str) -> String {
        self.counts.identifiers += 1;
        let idx = Self::slot(&mut self.id_map, value);
        format!("[REDACTED_ID_{idx}]")
    }

    fn amount_placeholder(&mut self, value: &str) -> String {
        self.counts.amounts += 1;
        let idx = Self::slot(&mut self.amount_map, value);
        format!("[AMOUNT_{idx}]")
    }

    fn date_placeholder(&mut self, shape: &str, value: &str) -> String {
        self.counts.dates += 1;
        let idx = Self::slot(&mut self.date_map, value);
        format!("[DATE_{shape}_{idx}]")
    }

    fn redact_line(&mut self, line: &str) -> (String, bool) {
        let before = self.counts.total_redactions();
        let mut s = self.redact_custom_terms(line);
        s = self.redact_header_field(&s);
        s = self.redact_inline_patterns(&s);
        let changed = self.counts.total_redactions() > before;
        (s, changed)
    }

    fn redact_text_node(&mut self, text: &str) -> (String, bool) {
        let before = self.counts.total_redactions();
        let mut lines_out = Vec::new();
        for part in text.split('\n') {
            let mut s = self.redact_custom_terms(part);
            s = self.redact_header_field(&s);
            s = self.redact_inline_patterns(&s);
            lines_out.push(s);
        }
        let changed = self.counts.total_redactions() > before;
        (lines_out.join("\n"), changed)
    }

    fn redact_inline_only(&mut self, text: &str) -> String {
        let s = self.redact_custom_terms(text);
        self.redact_inline_patterns(&s)
    }

    fn redact_custom_terms(&mut self, input: &str) -> String {
        if self.terms_sorted.is_empty() {
            return input.to_string();
        }
        let terms = self.terms_sorted.clone();
        let mut current = input.to_string();
        for term in &terms {
            if term.is_empty() {
                continue;
            }
            current =
                replace_case_insensitive(&current, term, |matched| self.term_placeholder(matched));
        }
        current
    }

    fn redact_header_field(&mut self, line: &str) -> String {
        let trimmed_start = line.trim_start();
        let indent_len = line.len() - trimmed_start.len();
        let indent = &line[..indent_len];

        // Special no-colon prefix: `Card ending in 4242`
        const NO_COLON_PREFIX: &str = "card ending in ";
        if trimmed_start.len() >= NO_COLON_PREFIX.len()
            && trimmed_start.is_char_boundary(NO_COLON_PREFIX.len())
            && trimmed_start[..NO_COLON_PREFIX.len()].eq_ignore_ascii_case(NO_COLON_PREFIX)
        {
            let label_slice = &trimmed_start[..NO_COLON_PREFIX.len()];
            let rest = &trimmed_start[NO_COLON_PREFIX.len()..];
            let val = rest.trim();
            if val.is_empty() || is_only_placeholders(val) {
                return line.to_string();
            }
            let placeholder = self.field_placeholder(val);
            return format!("{indent}{label_slice}{placeholder}");
        }

        // General `Label:` / `标签：` / spaced CJK fapiao `名 称：` matcher
        if let Some((colon_idx, colon_ch)) = trimmed_start
            .char_indices()
            .take_while(|(idx, _)| *idx < 64)
            .find(|(_, ch)| matches!(ch, ':' | '：'))
        {
            let raw_label = &trimmed_start[..colon_idx];
            if is_sensitive_label_raw(raw_label) {
                let prefix_len = colon_idx + colon_ch.len_utf8();
                let label_slice = &trimmed_start[..prefix_len];
                let rest = &trimmed_start[prefix_len..];
                let val = rest.trim();
                if val.is_empty() || is_only_placeholders(val) {
                    return line.to_string();
                }
                let sep = if label_slice.ends_with('：') {
                    ""
                } else {
                    " "
                };
                let placeholder = self.field_placeholder(val);
                return format!("{indent}{label_slice}{sep}{placeholder}");
            }
        }
        line.to_string()
    }

    fn redact_inline_patterns(&mut self, input: &str) -> String {
        let s = self.redact_urls_and_ips(input);
        let s = self.redact_emails(&s);
        let s = self.redact_dates(&s);
        let s = self.redact_currency_amounts(&s);
        self.redact_identifiers(&s)
    }

    fn redact_urls_and_ips(&mut self, input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let bytes = input.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(end) = skip_placeholder(input, i) {
                out.push_str(&input[i..end]);
                i = end;
                continue;
            }
            if let Some(end) = match_url(input, i) {
                let matched = &input[i..end];
                out.push_str(&self.url_placeholder(matched));
                i = end;
                continue;
            }
            if let Some(end) = match_ipv4(input, i) {
                let matched = &input[i..end];
                out.push_str(&self.url_placeholder(matched));
                i = end;
                continue;
            }
            let ch = input[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    fn redact_emails(&mut self, input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let bytes = input.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(end) = skip_placeholder(input, i) {
                out.push_str(&input[i..end]);
                i = end;
                continue;
            }
            if let Some(end) = match_email(input, i) {
                let matched = &input[i..end];
                out.push_str(&self.email_placeholder(matched));
                i = end;
                continue;
            }
            let ch = input[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    fn redact_dates(&mut self, input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let bytes = input.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(end) = skip_placeholder(input, i) {
                out.push_str(&input[i..end]);
                i = end;
                continue;
            }
            if let Some((end, shape)) = match_date(input, i) {
                let matched = &input[i..end];
                out.push_str(&self.date_placeholder(shape, matched));
                i = end;
                continue;
            }
            let ch = input[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    fn redact_currency_amounts(&mut self, input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let bytes = input.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(end) = skip_placeholder(input, i) {
                out.push_str(&input[i..end]);
                i = end;
                continue;
            }
            if let Some((prefix, num_val, suffix, end)) = match_currency_amount(input, i) {
                let ph = self.amount_placeholder(num_val);
                out.push_str(prefix);
                out.push_str(&ph);
                out.push_str(suffix);
                i = end;
                continue;
            }
            let ch = input[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    fn redact_identifiers(&mut self, input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let bytes = input.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(end) = skip_placeholder(input, i) {
                out.push_str(&input[i..end]);
                i = end;
                continue;
            }
            if let Some(end) = match_uuid(input, i) {
                let matched = &input[i..end];
                out.push_str(&self.id_placeholder(matched));
                i = end;
                continue;
            }
            if let Some(end) = match_payment_processor_id(input, i) {
                let matched = &input[i..end];
                out.push_str(&self.id_placeholder(matched));
                i = end;
                continue;
            }
            if let Some(end) = match_cn_resident_id(input, i) {
                let matched = &input[i..end];
                out.push_str(&self.id_placeholder(matched));
                i = end;
                continue;
            }
            if let Some(end) = match_cn_uscc(input, i) {
                let matched = &input[i..end];
                out.push_str(&self.id_placeholder(matched));
                i = end;
                continue;
            }
            if let Some(end) = match_eu_vat(input, i) {
                let matched = &input[i..end];
                out.push_str(&self.id_placeholder(matched));
                i = end;
                continue;
            }
            if let Some(end) = match_us_ssn(input, i) {
                let matched = &input[i..end];
                out.push_str(&self.id_placeholder(matched));
                i = end;
                continue;
            }
            if let Some(end) = match_us_ein(input, i) {
                let matched = &input[i..end];
                out.push_str(&self.id_placeholder(matched));
                i = end;
                continue;
            }
            if let Some((prefix, val, end)) = match_last4_card_ref(input, i) {
                let ph = self.id_placeholder(val);
                out.push_str(prefix);
                out.push_str(&ph);
                i = end;
                continue;
            }
            if let Some(end) = match_prefixed_code(input, i) {
                let matched = &input[i..end];
                out.push_str(&self.id_placeholder(matched));
                i = end;
                continue;
            }
            if let Some(end) = match_masked_card(input, i) {
                let matched = &input[i..end];
                out.push_str(&self.id_placeholder(matched));
                i = end;
                continue;
            }
            if let Some(end) = match_mid_masked_card_or_phone(input, i) {
                let matched = &input[i..end];
                out.push_str(&self.id_placeholder(matched));
                i = end;
                continue;
            }
            if let Some(end) = match_parenthesized_phone(input, i) {
                let matched = &input[i..end];
                out.push_str(&self.id_placeholder(matched));
                i = end;
                continue;
            }
            if let Some(end) = match_dotted_phone(input, i) {
                let matched = &input[i..end];
                out.push_str(&self.id_placeholder(matched));
                i = end;
                continue;
            }
            if let Some(end) = match_long_number_or_phone(input, i) {
                let matched = &input[i..end];
                out.push_str(&self.id_placeholder(matched));
                i = end;
                continue;
            }
            let ch = input[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }
}

pub fn redact_template(req: RedactRequest) -> Report {
    match redact_inner(req) {
        Ok(report) => report,
        Err(fail) => fail.into_report(),
    }
}

/// Redacts a vaulted source entirely in memory without creating plaintext files in `%TEMP%`/`/tmp`.
#[allow(dead_code)]
pub(crate) fn redact_vault_source_in_memory(
    vault: &Path,
    source_id: &str,
    redact_terms: Vec<String>,
    auth: &AuthInput,
) -> Report {
    match redact_vault_source_in_memory_inner(vault, source_id, redact_terms, auth) {
        Ok(report) => report,
        Err(fail) => fail.into_report(),
    }
}

fn redact_vault_source_in_memory_inner(
    vault: &Path,
    source_id: &str,
    redact_terms: Vec<String>,
    auth: &AuthInput,
) -> Result<Report, Fail> {
    for term in &redact_terms {
        if term.trim().is_empty() {
            return Err(Fail::new(EXIT_USAGE, "Usage", "empty_redact_term"));
        }
    }
    if !is_prefixed_id(source_id, "src_") {
        return Err(Fail::new(EXIT_USAGE, "Usage", "bad_id"));
    }
    let loaded = load_vault_source(vault, source_id, auth)?;
    let kind = detect_kind(&loaded.bytes, loaded.media_type.as_deref());
    let mut redactor = Redactor::new(redact_terms);
    let (body_text, redacted_lines) = match kind {
        InputKind::Pdf => redact_pdf(&loaded.bytes, &mut redactor)?,
        InputKind::Html => {
            let allow_non_utf8 = loaded.vault_format.is_none_or(|v| v >= 6);
            let (text, _) = super::html::decode_html_bytes(&loaded.bytes, allow_non_utf8)
                .ok_or_else(|| {
                    Fail::new(EXIT_REJECTED, "Rejected", "invalid_utf8")
                        .extra("read_only", json!(true))
                        .extra("output_written", json!(false))
                })?;
            redact_html(&text, &mut redactor)
        }
        InputKind::Text => {
            let text = std::str::from_utf8(&loaded.bytes).map_err(|_| {
                Fail::new(EXIT_REJECTED, "Rejected", "invalid_utf8")
                    .extra("read_only", json!(true))
                    .extra("output_written", json!(false))
            })?;
            redact_plain_text(text, &mut redactor)
        }
    };
    let (header, header_lines) = format_header(kind);
    let final_output = format!("{header}{body_text}");
    let shifted_lines: Vec<usize> = redacted_lines
        .into_iter()
        .map(|ln| ln + header_lines)
        .collect();

    let total_redactions = redactor.counts.total_redactions();
    let mut body = honesty("Redacted");
    body.insert("slice".into(), json!("M4-real"));
    body.insert("read_only".into(), json!(true));
    body.insert("new_effect".into(), json!(false));
    body.insert("input_mode".into(), json!(loaded.input_mode));
    body.insert("input_kind".into(), json!(kind.as_str()));
    body.insert("redacted_text".into(), json!(final_output));
    body.insert("redaction_rules".into(), json!(REDACTION_RULES_VERSION));
    body.insert("manual_review_required".into(), json!(true));
    body.insert("total_redactions".into(), json!(total_redactions));
    body.insert(
        "counts".into(),
        json!({
            "custom_terms": redactor.counts.custom_terms,
            "header_fields": redactor.counts.header_fields,
            "emails": redactor.counts.emails,
            "urls_and_ips": redactor.counts.urls_and_ips,
            "identifiers": redactor.counts.identifiers,
            "amounts": redactor.counts.amounts,
            "dates": redactor.counts.dates,
            "stripped_scripts": redactor.counts.stripped_scripts,
            "stripped_styles": redactor.counts.stripped_styles,
            "stripped_comments": redactor.counts.stripped_comments,
            "total_redactions": total_redactions,
        }),
    );
    body.insert("redacted_lines".into(), json!(shifted_lines));
    if let Some(sid) = loaded.source_id {
        body.insert("source_id".into(), json!(sid));
    }
    if let Some(version) = loaded.vault_format {
        body.insert("vault_format".into(), json!(version));
        apply_format_honesty(&mut body, version, loaded.protection);
    } else {
        body.insert("synthetic_only".into(), json!(false));
    }
    Ok(Report {
        code: EXIT_OK,
        body: Value::Object(body),
    })
}

fn redact_inner(req: RedactRequest) -> Result<Report, Fail> {
    for term in &req.redact_terms {
        if term.trim().is_empty() {
            return Err(Fail::new(EXIT_USAGE, "Usage", "empty_redact_term"));
        }
    }
    let loaded = match (&req.source, &req.vault, &req.source_id) {
        (Some(source), None, None) => {
            check_external_output(&req.out)?;
            load_external_source(source)?
        }
        (None, Some(vault), Some(source_id)) => {
            if !is_prefixed_id(source_id, "src_") {
                return Err(Fail::new(EXIT_USAGE, "Usage", "bad_id"));
            }
            check_vault_output(vault, &req.out)?;
            load_vault_source(vault, source_id, &req.auth)?
        }
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) => {
            return Err(Fail::new(EXIT_USAGE, "Usage", "redact_conflicting_inputs"));
        }
        _ => {
            return Err(Fail::new(EXIT_USAGE, "Usage", "redact_required_flags"));
        }
    };

    let kind = detect_kind(&loaded.bytes, loaded.media_type.as_deref());
    let mut redactor = Redactor::new(req.redact_terms);
    let (body_text, redacted_lines) = match kind {
        InputKind::Pdf => redact_pdf(&loaded.bytes, &mut redactor)?,
        InputKind::Html => {
            let allow_non_utf8 = loaded.vault_format.is_none_or(|v| v >= 6);
            let (text, _) = super::html::decode_html_bytes(&loaded.bytes, allow_non_utf8)
                .ok_or_else(|| {
                    Fail::new(EXIT_REJECTED, "Rejected", "invalid_utf8")
                        .extra("read_only", json!(true))
                        .extra("output_written", json!(false))
                })?;
            redact_html(&text, &mut redactor)
        }
        InputKind::Text => {
            let text = std::str::from_utf8(&loaded.bytes).map_err(|_| {
                Fail::new(EXIT_REJECTED, "Rejected", "invalid_utf8")
                    .extra("read_only", json!(true))
                    .extra("output_written", json!(false))
            })?;
            redact_plain_text(text, &mut redactor)
        }
    };

    let (header, header_lines) = format_header(kind);
    let final_output = format!("{header}{body_text}");
    let shifted_lines: Vec<usize> = redacted_lines
        .into_iter()
        .map(|ln| ln + header_lines)
        .collect();

    write_new(&req.out, final_output.as_bytes())?;

    let mut body = honesty("Redacted");
    body.insert("slice".into(), json!("M4-real"));
    body.insert("read_only".into(), json!(true));
    body.insert("new_effect".into(), json!(false));
    body.insert("input_mode".into(), json!(loaded.input_mode));
    body.insert("input_kind".into(), json!(kind.as_str()));
    body.insert("output".into(), json!(req.out.display().to_string()));
    body.insert("output_written".into(), json!(true));
    body.insert("redaction_rules".into(), json!(REDACTION_RULES_VERSION));
    body.insert("manual_review_required".into(), json!(true));
    body.insert(
        "counts".into(),
        json!({
            "custom_terms": redactor.counts.custom_terms,
            "header_fields": redactor.counts.header_fields,
            "emails": redactor.counts.emails,
            "urls_and_ips": redactor.counts.urls_and_ips,
            "identifiers": redactor.counts.identifiers,
            "amounts": redactor.counts.amounts,
            "dates": redactor.counts.dates,
            "stripped_scripts": redactor.counts.stripped_scripts,
            "stripped_styles": redactor.counts.stripped_styles,
            "stripped_comments": redactor.counts.stripped_comments,
            "total_redactions": redactor.counts.total_redactions(),
        }),
    );
    body.insert("redacted_lines".into(), json!(shifted_lines));
    if let Some(sid) = loaded.source_id {
        body.insert("source_id".into(), json!(sid));
    }
    if let Some(version) = loaded.vault_format {
        body.insert("vault_format".into(), json!(version));
        apply_format_honesty(&mut body, version, loaded.protection);
    } else {
        body.insert("synthetic_only".into(), json!(false));
    }

    Ok(Report {
        code: EXIT_OK,
        body: Value::Object(body),
    })
}

fn format_header(kind: InputKind) -> (String, usize) {
    match kind {
        InputKind::Html => (
            "<!--\n=== SUBVAULT REDACTED TEMPLATE — REVIEW MANUALLY BEFORE SHARING ===\ninput_kind: html\nredaction_rules: redact-template-1\nwarning: Automated redaction is best-effort. Manually inspect every line before sharing.\n=== END HEADER ===\n-->\n".to_string(),
            7,
        ),
        InputKind::Pdf | InputKind::Text => (
            format!(
                "=== SUBVAULT REDACTED TEMPLATE — REVIEW MANUALLY BEFORE SHARING ===\n# input_kind: {}\n# redaction_rules: {REDACTION_RULES_VERSION}\n# warning: Automated redaction is best-effort. Manually inspect every line before sharing.\n=== END HEADER ===\n",
                kind.as_str()
            ),
            5,
        ),
    }
}

fn check_vault_output(vault: &Path, out: &Path) -> Result<(), Fail> {
    match super::place_output(vault, out) {
        super::Place::Create => Ok(()),
        super::Place::Inside => Err(reject("output_inside_vault")),
        super::Place::Unresolved(msg) => {
            Err(reject("output_unresolved").extra("os_error", json!(msg)))
        }
        super::Place::MissingParent => Err(reject("output_parent_missing")),
        super::Place::Exists => Err(reject("output_exists")),
    }
}

fn check_external_output(out: &Path) -> Result<(), Fail> {
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(reject("output_parent_missing"));
    }
    match fs::symlink_metadata(out) {
        Ok(_) => Err(reject("output_exists")),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(reject("output_unresolved").extra("os_error", json!(err.to_string()))),
    }
}

fn load_external_source(path: &Path) -> Result<LoadedInput, Fail> {
    let meta = fs::metadata(path).map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound {
            reject("source_missing")
        } else {
            io_fail(err, "redact_stat")
        }
    })?;
    if !meta.is_file() {
        return Err(reject("source_not_a_file"));
    }
    if meta.len() == 0 {
        return Err(reject("empty_source"));
    }
    if meta.len() > MAX_REDACT_BYTES {
        return Err(reject("over_input_budget").extra("limit", json!(MAX_REDACT_BYTES)));
    }
    let mut file = File::open(path).map_err(|err| io_fail(err, "redact_read"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|err| io_fail(err, "redact_read"))?;
    Ok(LoadedInput {
        bytes,
        media_type: None,
        input_mode: "external_file",
        source_id: None,
        vault_format: None,
        protection: None,
    })
}

fn load_vault_source(vault: &Path, source_id: &str, auth: &AuthInput) -> Result<LoadedInput, Fail> {
    let (conn, version, protection, cipher) = super::open_readonly_unlocked(vault, auth)?;
    let source = store::source_by_id_with_cipher(&conn, source_id, &cipher)
        .map_err(|err| unseen_db(err, "source_lookup"))?
        .ok_or_else(|| reject("unknown_source"))?;
    let object = store::object_by_id_with_cipher(&conn, &source.object_id, &cipher)
        .map_err(|err| unseen_db(err, "object_lookup"))?
        .ok_or_else(|| {
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_row_missing")
                .extra("new_effect", json!(false))
        })?;
    let obj_facts = facts::object_facts_with_cipher(
        vault,
        &object.object_id,
        Some(&object.relpath),
        &source.content_sha256,
        source.byte_length,
        &cipher,
    );
    super::hosted_facts_ok(&obj_facts)?;
    let Some(relpath) = obj_facts.canonical_relpath else {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_id_rejected")
                .extra("new_effect", json!(false)),
        );
    };
    let bytes = match cipher.decrypt_object_to_vec(&object.object_id, &vault.join(relpath)) {
        Ok(bytes) => bytes,
        Err(CryptoError::Io(err)) => {
            return Err(
                Fail::new(EXIT_RECOVERY, "RecoveryRequired", "object_unreadable")
                    .extra("os_error", json!(err.to_string()))
                    .extra("new_effect", json!(false)),
            );
        }
        Err(err) => return Err(cipher_tamper_fail(err, "redact_read_object")),
    };
    if sha256_bytes(&bytes) != source.content_sha256 || bytes.len() as i64 != source.byte_length {
        return Err(
            Fail::new(EXIT_RECOVERY, "RecoveryRequired", "hash_mismatch")
                .extra("new_effect", json!(false)),
        );
    }
    Ok(LoadedInput {
        bytes,
        media_type: Some(source.media_type),
        input_mode: "vault_source",
        source_id: Some(source.source_id),
        vault_format: Some(version),
        protection,
    })
}

fn detect_kind(bytes: &[u8], media_type: Option<&str>) -> InputKind {
    if bytes.starts_with(b"%PDF-") || media_type == Some("application/pdf") {
        return InputKind::Pdf;
    }
    if media_type.is_some_and(|mt| mt.to_ascii_lowercase().starts_with("text/html")) {
        return InputKind::Html;
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        let probe: String = text.trim_start().chars().take(256).collect();
        let lower = probe.to_ascii_lowercase();
        if lower.starts_with("<!doctype html")
            || lower.starts_with("<html")
            || lower.starts_with("<table")
            || lower.starts_with("<body")
            || lower.starts_with("<div")
            || lower.starts_with("<tr")
            || lower.starts_with("<p>")
            || lower.contains("<table")
            || lower.contains("<html")
        {
            return InputKind::Html;
        }
    }
    InputKind::Text
}

fn redact_pdf(bytes: &[u8], redactor: &mut Redactor) -> Result<(String, Vec<usize>), Fail> {
    let pages = pdf_parse::extract_pages_for_redaction(bytes)
        .map_err(|reason| reject("pdf_redaction_unsupported").extra("pdf_reason", json!(reason)))?;
    let mut out = String::new();
    let mut redacted_lines = Vec::new();
    let mut line_no = 0usize;
    for (page_num, lines) in pages {
        line_no += 1;
        out.push_str(&format!("--- PAGE {page_num} ---\n"));
        for line in lines {
            line_no += 1;
            let (redacted, changed) = redactor.redact_line(&line);
            if changed {
                redacted_lines.push(line_no);
            }
            out.push_str(&redacted);
            out.push('\n');
        }
    }
    Ok((out, redacted_lines))
}

fn redact_plain_text(text: &str, redactor: &mut Redactor) -> (String, Vec<usize>) {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(normalized.len());
    let mut redacted_lines = Vec::new();
    for (idx, line) in normalized.split('\n').enumerate() {
        let (redacted, changed) = redactor.redact_line(line);
        if changed {
            redacted_lines.push(idx + 1);
        }
        out.push_str(&redacted);
        out.push('\n');
    }
    if !normalized.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    (out, redacted_lines)
}

#[derive(Default)]
struct HtmlTableState {
    next_td_sensitive: bool,
    cell_mode: HtmlCellMode,
}

#[derive(Default)]
enum HtmlCellMode {
    #[default]
    None,
    CapturingLabel(String),
    SensitiveTd,
}

fn redact_html(html: &str, redactor: &mut Redactor) -> (String, Vec<usize>) {
    let normalized = html.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(normalized.len());
    let mut i = 0usize;
    let bytes = normalized.as_bytes();
    let mut table_stack = vec![HtmlTableState::default()];
    while i < bytes.len() {
        if normalized[i..].starts_with("<!--") {
            if let Some(rel_end) = normalized[i + 4..].find("-->") {
                redactor.counts.stripped_comments += 1;
                i = i + 4 + rel_end + 3;
                continue;
            } else {
                redactor.counts.stripped_comments += 1;
                break;
            }
        }
        if bytes[i] == b'<' {
            if let Some(end_tag) = find_tag_end(&normalized, i) {
                let tag = &normalized[i..end_tag];
                let (is_closing, is_self_closing, lower_name) = html_tag_info(tag);
                if !is_closing && lower_name == "script" {
                    redactor.counts.stripped_scripts += 1;
                    out.push_str("<!-- [STRIPPED_SCRIPT] -->");
                    i = skip_until_closing_tag(&normalized, end_tag, "script");
                    continue;
                }
                if !is_closing && lower_name == "style" {
                    redactor.counts.stripped_styles += 1;
                    out.push_str("<!-- [STRIPPED_STYLE] -->");
                    i = skip_until_closing_tag(&normalized, end_tag, "style");
                    continue;
                }
                update_html_table_state(&mut table_stack, is_closing, is_self_closing, &lower_name);
                out.push_str(&sanitize_html_tag(tag, redactor));
                i = end_tag;
                continue;
            } else {
                out.push_str("&lt;");
                i += 1;
                continue;
            }
        }
        // Text node until next `<`
        let next_lt = normalized[i..]
            .find('<')
            .map(|rel| i + rel)
            .unwrap_or(normalized.len());
        let text_seg = &normalized[i..next_lt];
        let decoded = html_escape::decode_html_entities(text_seg);
        let state = table_stack.last_mut().expect("table_stack is non-empty");
        match &mut state.cell_mode {
            HtmlCellMode::SensitiveTd => {
                let redacted = if decoded.trim().is_empty() {
                    decoded.into_owned()
                } else {
                    let trimmed = decoded.trim();
                    let after_terms = redactor.redact_custom_terms(trimmed);
                    let lead_len = decoded.len() - decoded.trim_start().len();
                    let trail_start = decoded.trim_end().len();
                    let lead = &decoded[..lead_len];
                    let trail = &decoded[trail_start..];
                    if is_only_placeholders(&after_terms) {
                        format!("{lead}{after_terms}{trail}")
                    } else {
                        let ph = redactor.field_placeholder(trimmed);
                        format!("{lead}{ph}{trail}")
                    }
                };
                out.push_str(&escape_html_text(&redacted));
            }
            HtmlCellMode::CapturingLabel(buf) => {
                if buf.len() < 256 {
                    buf.push_str(&decoded);
                }
                let (redacted_seg, _) = redactor.redact_text_node(&decoded);
                out.push_str(&escape_html_text(&redacted_seg));
            }
            HtmlCellMode::None => {
                let (redacted_seg, _) = redactor.redact_text_node(&decoded);
                out.push_str(&escape_html_text(&redacted_seg));
            }
        }
        i = next_lt;
    }
    let redacted_lines: Vec<usize> = out
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            line.contains("[REDACTED_")
                || line.contains("[AMOUNT_")
                || line.contains("[DATE_")
                || line.contains("[STRIPPED_")
        })
        .map(|(idx, _)| idx + 1)
        .collect();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    (out, redacted_lines)
}

fn update_html_table_state(
    table_stack: &mut Vec<HtmlTableState>,
    is_closing: bool,
    is_self_closing: bool,
    lower_name: &str,
) {
    match (is_closing, lower_name) {
        (false, "table") if !is_self_closing => {
            if let Some(top) = table_stack.last_mut() {
                if matches!(top.cell_mode, HtmlCellMode::CapturingLabel(_)) {
                    top.cell_mode = HtmlCellMode::None;
                }
            }
            table_stack.push(HtmlTableState::default());
        }
        (true, "table") => {
            if table_stack.len() > 1 {
                table_stack.pop();
            } else if let Some(top) = table_stack.last_mut() {
                *top = HtmlTableState::default();
            }
        }
        (_, "tr") => {
            if let Some(top) = table_stack.last_mut() {
                top.cell_mode = HtmlCellMode::None;
                top.next_td_sensitive = false;
            }
        }
        (false, "th") => {
            if let Some(top) = table_stack.last_mut() {
                top.next_td_sensitive = false;
                top.cell_mode = if is_self_closing {
                    HtmlCellMode::None
                } else {
                    HtmlCellMode::CapturingLabel(String::new())
                };
            }
        }
        (false, "td") => {
            if let Some(top) = table_stack.last_mut() {
                if let HtmlCellMode::CapturingLabel(buf) = std::mem::take(&mut top.cell_mode) {
                    if is_sensitive_label(&buf) {
                        top.next_td_sensitive = true;
                    }
                }
                if is_self_closing {
                    top.next_td_sensitive = false;
                    top.cell_mode = HtmlCellMode::None;
                } else if top.next_td_sensitive {
                    top.next_td_sensitive = false;
                    top.cell_mode = HtmlCellMode::SensitiveTd;
                } else {
                    top.cell_mode = HtmlCellMode::CapturingLabel(String::new());
                }
            }
        }
        (true, "th" | "td") => {
            if let Some(top) = table_stack.last_mut() {
                top.next_td_sensitive = match std::mem::take(&mut top.cell_mode) {
                    HtmlCellMode::CapturingLabel(buf) => is_sensitive_label(&buf),
                    HtmlCellMode::SensitiveTd | HtmlCellMode::None => false,
                };
            }
        }
        _ => {}
    }
}

fn escape_html_text(s: &str) -> String {
    if !s.contains(['&', '<', '>']) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

fn escape_html_attr(s: &str, quote_char: char) -> String {
    if !s.contains(['&', '<', '>', quote_char]) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if quote_char == '"' => out.push_str("&quot;"),
            '\'' if quote_char == '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn find_tag_end(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = start + 1;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
        } else if b == b'"' || b == b'\'' {
            quote = Some(b);
        } else if b == b'>' {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

fn html_tag_info(tag: &str) -> (bool, bool, String) {
    let inner = tag.strip_prefix('<').unwrap_or(tag);
    let inner = inner.strip_suffix('>').unwrap_or(inner).trim();
    if inner.starts_with('!') || inner.starts_with('?') {
        return (false, false, String::new());
    }
    let (is_closing, rest) = if let Some(after_slash) = inner.strip_prefix('/') {
        (true, after_slash.trim_start())
    } else {
        (false, inner)
    };
    let is_self_closing = !is_closing && rest.ends_with('/');
    let name = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == ':')
        .collect::<String>()
        .to_ascii_lowercase();
    (is_closing, is_self_closing, name)
}

fn skip_until_closing_tag(s: &str, from: usize, tag_name: &str) -> usize {
    let lower = s[from..].to_ascii_lowercase();
    let needle = format!("</{tag_name}");
    if let Some(rel) = lower.find(&needle) {
        let abs = from + rel;
        if let Some(end) = s[abs..].find('>') {
            return abs + end + 1;
        }
    }
    s.len()
}

fn sanitize_html_tag(tag: &str, redactor: &mut Redactor) -> String {
    let inner = &tag[1..tag.len() - 1];
    let trimmed = inner.trim();
    if trimmed.starts_with('/') || trimmed.starts_with('!') || trimmed.starts_with('?') {
        return tag.to_string();
    }
    let self_closing = trimmed.ends_with('/');
    let body = if self_closing {
        trimmed[..trimmed.len() - 1].trim_end()
    } else {
        trimmed
    };
    let mut chars = body.char_indices();
    let mut name_end = body.len();
    for (idx, ch) in &mut chars {
        if ch.is_ascii_whitespace() {
            name_end = idx;
            break;
        }
    }
    let tag_name = &body[..name_end];
    let attr_str = body[name_end..].trim();
    if attr_str.is_empty() {
        return if self_closing {
            format!("<{tag_name} />")
        } else {
            format!("<{tag_name}>")
        };
    }

    let mut out = format!("<{tag_name}");
    let bytes = attr_str.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let name_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' {
            i += 1;
        }
        let attr_name = &attr_str[name_start..i];
        let lower_attr = attr_name.to_ascii_lowercase();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let (raw_val, quote_char) =
                if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                    let q = bytes[i];
                    i += 1;
                    let v_start = i;
                    while i < bytes.len() && bytes[i] != q {
                        i += 1;
                    }
                    let val = &attr_str[v_start..i];
                    if i < bytes.len() {
                        i += 1;
                    }
                    (val, q as char)
                } else {
                    let v_start = i;
                    while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    (&attr_str[v_start..i], '"')
                };

            const SAFE_STRUCT_ATTRS: &[&str] = &[
                "colspan",
                "rowspan",
                "align",
                "valign",
                "scope",
                "role",
                "dir",
                "lang",
                "border",
                "cellpadding",
                "cellspacing",
            ];
            if SAFE_STRUCT_ATTRS.contains(&lower_attr.as_str()) {
                out.push_str(&format!(" {attr_name}={quote_char}{raw_val}{quote_char}"));
            } else if matches!(
                lower_attr.as_str(),
                "href"
                    | "src"
                    | "action"
                    | "poster"
                    | "cite"
                    | "srcset"
                    | "formaction"
                    | "xlink:href"
            ) {
                let ph = redactor.url_placeholder(raw_val);
                out.push_str(&format!(" {attr_name}={quote_char}{ph}{quote_char}"));
            } else {
                let decoded_attr = html_escape::decode_html_entities(raw_val);
                let cleaned = redactor.redact_inline_only(&decoded_attr);
                let escaped = escape_html_attr(&cleaned, quote_char);
                out.push_str(&format!(" {attr_name}={quote_char}{escaped}{quote_char}"));
            }
        } else if !attr_name.is_empty() {
            out.push(' ');
            out.push_str(attr_name);
        }
    }
    if self_closing {
        out.push_str(" />");
    } else {
        out.push('>');
    }
    out
}

fn skip_placeholder(s: &str, start: usize) -> Option<usize> {
    if !s.as_bytes().get(start).is_some_and(|&b| b == b'[') {
        return None;
    }
    let tail = &s[start..];
    if !(tail.starts_with("[REDACTED_")
        || tail.starts_with("[AMOUNT_")
        || tail.starts_with("[DATE_")
        || tail.starts_with("[STRIPPED_"))
    {
        return None;
    }
    let end_rel = tail.find(']')?;
    Some(start + end_rel + 1)
}

fn is_only_placeholders(s: &str) -> bool {
    let mut i = 0;
    let bytes = s.as_bytes();
    let mut saw_any = false;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if let Some(end) = skip_placeholder(s, i) {
            saw_any = true;
            i = end;
            continue;
        }
        return false;
    }
    saw_any
}

fn replace_case_insensitive(
    input: &str,
    needle: &str,
    mut make_replacement: impl FnMut(&str) -> String,
) -> String {
    let lower_in = input.to_lowercase();
    let lower_needle = needle.to_lowercase();
    if lower_needle.is_empty() {
        return input.to_string();
    }
    // Map lowercase char indices back to original char boundaries safely for Unicode
    let in_chars: Vec<(usize, char)> = input.char_indices().collect();
    let needle_chars: Vec<char> = lower_needle.chars().collect();
    if needle_chars.is_empty() || in_chars.len() < needle_chars.len() {
        return input.to_string();
    }
    // Fast ASCII path when both are ASCII
    if input.is_ascii() && needle.is_ascii() {
        let mut out = String::with_capacity(input.len());
        let mut pos = 0usize;
        while let Some(rel) = lower_in[pos..].find(&lower_needle) {
            let abs = pos + rel;
            out.push_str(&input[pos..abs]);
            let end = abs + lower_needle.len();
            out.push_str(&make_replacement(&input[abs..end]));
            pos = end;
        }
        out.push_str(&input[pos..]);
        return out;
    }
    // General Unicode-safe case-insensitive scan
    let lower_chars: Vec<char> = input
        .chars()
        .map(|c| c.to_lowercase().next().unwrap_or(c))
        .collect();
    let mut out = String::with_capacity(input.len());
    let mut ci = 0usize;
    let n_len = needle_chars.len();
    while ci < in_chars.len() {
        if ci + n_len <= lower_chars.len() && lower_chars[ci..ci + n_len] == needle_chars[..] {
            let byte_start = in_chars[ci].0;
            let byte_end = if ci + n_len < in_chars.len() {
                in_chars[ci + n_len].0
            } else {
                input.len()
            };
            out.push_str(&make_replacement(&input[byte_start..byte_end]));
            ci += n_len;
        } else {
            out.push(in_chars[ci].1);
            ci += 1;
        }
    }
    out
}

const ASCII_SENSITIVE_LABELS: &[&str] = &[
    "billing address",
    "billing name",
    "account holder",
    "account email",
    "account name",
    "account id",
    "customer email",
    "customer name",
    "payment method",
    "card ending in",
    "card ending",
    "cardholder",
    "billed to",
    "bill to",
    "ship to",
    "full name",
    "subscriber",
    "recipient",
    "telephone",
    "customer",
    "username",
    "member id",
    "user id",
    "account",
    "address",
    "street",
    "mobile",
    "tax id",
    "vat id",
    "email",
    "phone",
    "name",
    "city",
    "vat",
    "gst",
    "tin",
];

const CJK_SENSITIVE_LABELS: &[&str] = &[
    "购买方名称",
    "购方名称",
    "买方名称",
    "发票抬头",
    "抬头",
    "名称",
    "纳税人识别号",
    "税号",
    "统一社会信用代码",
    "地址、电话",
    "地址电话",
    "开户行及账号",
    "开户银行",
    "银行账号",
    "开票人",
    "收款人",
    "复核",
    "复核人",
    "身份证号",
    "身份证号码",
    "发票代码",
    "发票号码",
    "校验码",
    "机器编号",
    "订单号",
    "订单编号",
    "交易单号",
    "商户单号",
    "流水号",
    "客户姓名",
    "账单地址",
    "付款方式",
    "收件人",
    "姓名",
    "账户",
    "账号",
    "地址",
    "电话",
    "手机",
    "邮箱",
    "卡号",
];

fn is_sensitive_label_raw(raw_label: &str) -> bool {
    let trimmed = raw_label.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.chars().all(|c| c.is_ascii() || c.is_whitespace()) {
        let mut norm = String::with_capacity(trimmed.len());
        let mut prev_ws = false;
        for ch in trimmed.chars() {
            if ch.is_whitespace() {
                if !prev_ws {
                    norm.push(' ');
                    prev_ws = true;
                }
            } else {
                norm.push(ch.to_ascii_lowercase());
                prev_ws = false;
            }
        }
        return ASCII_SENSITIVE_LABELS.contains(&norm.as_str());
    }
    let compact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    CJK_SENSITIVE_LABELS.contains(&compact.as_str())
}

fn is_sensitive_label(cell_text: &str) -> bool {
    let stripped = cell_text
        .trim()
        .trim_end_matches(|c: char| matches!(c, ':' | '：') || c.is_whitespace());
    is_sensitive_label_raw(stripped)
}

fn skip_inline_spaces(s: &str, mut pos: usize) -> usize {
    while let Some(ch) = s[pos..].chars().next() {
        if matches!(ch, ' ' | '\t' | '\u{00A0}' | '\u{3000}') {
            pos += ch.len_utf8();
        } else {
            break;
        }
    }
    pos
}

fn match_url(s: &str, start: usize) -> Option<usize> {
    let tail = &s[start..];
    let lower = tail.to_ascii_lowercase();
    let matched_scheme = lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:")
        || (lower.starts_with("www.")
            && (start == 0 || !s.as_bytes()[start - 1].is_ascii_alphanumeric()));
    if !matched_scheme {
        return None;
    }
    let mut end = start;
    for (rel, ch) in tail.char_indices() {
        if ch.is_whitespace() || matches!(ch, '"' | '\'' | '<' | '>' | '(' | ')' | '[' | ']') {
            break;
        }
        end = start + rel + ch.len_utf8();
    }
    while end > start && matches!(s.as_bytes()[end - 1], b'.' | b',' | b';' | b':') {
        end -= 1;
    }
    if end > start + 4 {
        Some(end)
    } else {
        None
    }
}

fn match_ipv4(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'.') {
        return None;
    }
    let mut i = start;
    for part in 0..4 {
        let d_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == d_start || i - d_start > 3 {
            return None;
        }
        let val: u16 = s[d_start..i].parse().ok()?;
        if val > 255 {
            return None;
        }
        if part < 3 {
            if bytes.get(i) != Some(&b'.') {
                return None;
            }
            i += 1;
        }
    }
    if i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
        return None;
    }
    Some(i)
}

fn match_email(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if start > 0 && is_email_local(bytes[start - 1]) {
        return None;
    }
    if !is_email_local_start(bytes[start]) {
        return None;
    }
    let mut i = start;
    while i < bytes.len() && is_email_local(bytes[i]) {
        i += 1;
    }
    if i == start || bytes.get(i) != Some(&b'@') {
        return None;
    }
    i += 1;
    let dom_start = i;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'.' | b'-')) {
        i += 1;
    }
    while i > dom_start && bytes[i - 1] == b'.' {
        i -= 1;
    }
    let domain = &s[dom_start..i];
    let dot = domain.rfind('.')?;
    let tld = &domain[dot + 1..];
    if dot == 0 || tld.len() < 2 || !tld.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    Some(i)
}

fn is_email_local_start(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'*')
}

fn is_email_local(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'%' | b'+' | b'-' | b'*')
}

fn match_date(s: &str, start: usize) -> Option<(usize, &'static str)> {
    const MONTHS: &[&str] = &[
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
        "jan",
        "feb",
        "mar",
        "apr",
        "jun",
        "jul",
        "aug",
        "sep",
        "sept",
        "oct",
        "nov",
        "dec",
    ];

    let bytes = s.as_bytes();
    let prev_alnum = start > 0 && bytes[start - 1].is_ascii_alphanumeric();
    if prev_alnum {
        return None;
    }

    // 1. ISO / dotted: YYYY-MM-DD, YYYY/MM/DD, YYYY.MM.DD
    if start + 10 <= bytes.len()
        && bytes[start..start + 4].iter().all(u8::is_ascii_digit)
        && matches!(bytes[start + 4], b'-' | b'/' | b'.')
        && bytes[start + 5..start + 7].iter().all(u8::is_ascii_digit)
        && bytes[start + 7] == bytes[start + 4]
        && bytes[start + 8..start + 10].iter().all(u8::is_ascii_digit)
        && !bytes.get(start + 10).is_some_and(u8::is_ascii_alphanumeric)
    {
        let sep = bytes[start + 4];
        let dot_boundary_bad = sep == b'.'
            && ((start > 0 && bytes[start - 1] == b'.')
                || (bytes.get(start + 10) == Some(&b'.')
                    && bytes.get(start + 11).is_some_and(u8::is_ascii_digit)));
        if !dot_boundary_bad {
            if let (Ok(year), Ok(month), Ok(day)) = (
                s[start..start + 4].parse::<u16>(),
                s[start + 5..start + 7].parse::<u8>(),
                s[start + 8..start + 10].parse::<u8>(),
            ) {
                if (1900..=2100).contains(&year)
                    && (1..=12).contains(&month)
                    && (1..=31).contains(&day)
                {
                    return Some((start + 10, "ISO"));
                }
            }
        }
    }

    // 2. Chinese YYYY年MM月DD日
    if start + 4 <= bytes.len() && bytes[start..start + 4].iter().all(u8::is_ascii_digit) {
        let rest = &s[start + 4..];
        if let Some(after_y) = rest.strip_prefix('年') {
            let m_len = after_y.bytes().take_while(u8::is_ascii_digit).count();
            if (1..=2).contains(&m_len) {
                if let Some(after_m) = after_y[m_len..].strip_prefix('月') {
                    let d_len = after_m.bytes().take_while(u8::is_ascii_digit).count();
                    if (1..=2).contains(&d_len) {
                        if let Some(after_d) = after_m[d_len..].strip_prefix('日') {
                            if let (Ok(year), Ok(month), Ok(day)) = (
                                s[start..start + 4].parse::<u16>(),
                                after_y[..m_len].parse::<u8>(),
                                after_m[..d_len].parse::<u8>(),
                            ) {
                                if (1900..=2100).contains(&year)
                                    && (1..=12).contains(&month)
                                    && (1..=31).contains(&day)
                                {
                                    let consumed = s.len() - start - after_d.len();
                                    return Some((start + consumed, "CJK"));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 3. Slash MM/DD/YYYY or DD/MM/YYYY
    if start + 8 <= bytes.len() && bytes[start].is_ascii_digit() {
        let mut p = start;
        let a_start = p;
        while p < bytes.len() && bytes[p].is_ascii_digit() {
            p += 1;
        }
        if (1..=2).contains(&(p - a_start)) && bytes.get(p) == Some(&b'/') {
            p += 1;
            let b_start = p;
            while p < bytes.len() && bytes[p].is_ascii_digit() {
                p += 1;
            }
            if (1..=2).contains(&(p - b_start)) && bytes.get(p) == Some(&b'/') {
                p += 1;
                let y_start = p;
                while p < bytes.len() && bytes[p].is_ascii_digit() {
                    p += 1;
                }
                if p - y_start == 4 && !bytes.get(p).is_some_and(u8::is_ascii_alphanumeric) {
                    if let (Ok(a), Ok(b), Ok(year)) = (
                        s[a_start..b_start - 1].parse::<u8>(),
                        s[b_start..y_start - 1].parse::<u8>(),
                        s[y_start..p].parse::<u16>(),
                    ) {
                        if (1900..=2100).contains(&year)
                            && (1..=31).contains(&a)
                            && (1..=31).contains(&b)
                            && (a <= 12 || b <= 12)
                        {
                            return Some((p, "SLASH"));
                        }
                    }
                }
            }
        }
    }

    let tail = &s[start..];

    // 4. English text date: `DD Mon YYYY` / `DD Month YYYY` (e.g., `15 Oct 2026`, `3 November 2026`)
    if tail.as_bytes()[0].is_ascii_digit() {
        let mut p = 0usize;
        while tail.as_bytes().get(p).is_some_and(u8::is_ascii_digit) {
            p += 1;
        }
        if (1..=2).contains(&p) {
            if let Ok(day) = tail[..p].parse::<u8>() {
                if (1..=31).contains(&day) {
                    if tail.len() >= p + 2 {
                        let ord = &tail.as_bytes()[p..p + 2];
                        if ord.eq_ignore_ascii_case(b"st")
                            || ord.eq_ignore_ascii_case(b"nd")
                            || ord.eq_ignore_ascii_case(b"rd")
                            || ord.eq_ignore_ascii_case(b"th")
                        {
                            p += 2;
                        }
                    }
                    let ws1 = p;
                    p = skip_inline_spaces(tail, p);
                    if p > ws1 {
                        let m_start = p;
                        while tail.as_bytes().get(p).is_some_and(u8::is_ascii_alphabetic) {
                            p += 1;
                        }
                        if p > m_start {
                            let word = tail[m_start..p].to_ascii_lowercase();
                            if MONTHS.contains(&word.as_str()) {
                                if matches!(tail.as_bytes().get(p), Some(b'.' | b',')) {
                                    p += 1;
                                }
                                let ws2 = p;
                                p = skip_inline_spaces(tail, p);
                                if p > ws2 {
                                    let y_start = p;
                                    while tail.as_bytes().get(p).is_some_and(u8::is_ascii_digit) {
                                        p += 1;
                                    }
                                    if p - y_start == 4
                                        && !tail
                                            .as_bytes()
                                            .get(p)
                                            .is_some_and(u8::is_ascii_alphanumeric)
                                    {
                                        if let Ok(year) = tail[y_start..p].parse::<u16>() {
                                            if (1900..=2100).contains(&year) {
                                                return Some((start + p, "TEXT"));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 5. English text date: `Sep 01, 2026` / `September 1, 2026`
    if tail.as_bytes()[0].is_ascii_alphabetic() {
        let word_len = tail.bytes().take_while(u8::is_ascii_alphabetic).count();
        let word = tail[..word_len].to_ascii_lowercase();
        if MONTHS.contains(&word.as_str()) {
            let mut p = word_len;
            if tail.as_bytes().get(p) == Some(&b'.') {
                p += 1;
            }
            let ws1 = p;
            p = skip_inline_spaces(tail, p);
            if p > ws1 {
                let d_start = p;
                while tail.as_bytes().get(p).is_some_and(u8::is_ascii_digit) {
                    p += 1;
                }
                if (1..=2).contains(&(p - d_start)) {
                    if let Ok(day) = tail[d_start..p].parse::<u8>() {
                        if (1..=31).contains(&day) {
                            if tail.len() >= p + 2 {
                                let ord = &tail.as_bytes()[p..p + 2];
                                if ord.eq_ignore_ascii_case(b"st")
                                    || ord.eq_ignore_ascii_case(b"nd")
                                    || ord.eq_ignore_ascii_case(b"rd")
                                    || ord.eq_ignore_ascii_case(b"th")
                                {
                                    p += 2;
                                }
                            }
                            if tail.as_bytes().get(p) == Some(&b',') {
                                p += 1;
                            }
                            let ws2 = p;
                            p = skip_inline_spaces(tail, p);
                            if p > ws2 {
                                let y_start = p;
                                while tail.as_bytes().get(p).is_some_and(u8::is_ascii_digit) {
                                    p += 1;
                                }
                                if p - y_start == 4
                                    && !tail
                                        .as_bytes()
                                        .get(p)
                                        .is_some_and(u8::is_ascii_alphanumeric)
                                {
                                    if let Ok(year) = tail[y_start..p].parse::<u16>() {
                                        if (1900..=2100).contains(&year) {
                                            return Some((start + p, "TEXT"));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn match_currency_amount(s: &str, start: usize) -> Option<(&str, &str, &str, usize)> {
    const CODES: &[&str] = &[
        "USD", "EUR", "GBP", "JPY", "CNY", "RMB", "HKD", "CAD", "AUD", "SGD", "CHF", "TWD", "KRW",
    ];
    const SYMBOLS: &[&str] = &["US$", "HK$", "CA$", "AU$", "NT$", "$", "€", "£", "¥", "￥"];
    const SUFFIX_SYMBOLS: &[&str] = &["元", "€", "$", "£", "¥", "￥"];

    let bytes = s.as_bytes();
    let prev_alnum = start > 0 && bytes[start - 1].is_ascii_alphanumeric();
    let tail = &s[start..];

    // 1. Currency code prefix: `USD 15.99` / `EUR -10.00`
    if !prev_alnum {
        for code in CODES {
            if tail.starts_with(code)
                && tail[code.len()..]
                    .chars()
                    .next()
                    .is_some_and(|c| matches!(c, ' ' | '\t' | '\u{00A0}'))
            {
                let p = skip_inline_spaces(tail, code.len());
                if let Some((num_end, num_slice)) = scan_money_number(&tail[p..]) {
                    let prefix = &tail[..p];
                    return Some((prefix, num_slice, "", start + p + num_end));
                }
            }
        }
    }

    // 2. Currency symbol prefix: `$15.99`, `US$ 15.99`, `€10,00`
    if !prev_alnum {
        for sym in SYMBOLS {
            if tail.starts_with(sym) {
                let p = skip_inline_spaces(tail, sym.len());
                if let Some((num_end, num_slice)) = scan_money_number(&tail[p..]) {
                    let prefix = &tail[..p];
                    return Some((prefix, num_slice, "", start + p + num_end));
                }
            }
        }
    }

    // 3. Number + currency code/symbol suffix: `15.99 USD`, `98.00元`, `19,99 €`, `20.00 $`, `15.00 £`
    let valid_num_start = bytes[start].is_ascii_digit()
        || (matches!(bytes[start], b'+' | b'-')
            && bytes.get(start + 1).is_some_and(u8::is_ascii_digit));
    if !prev_alnum && valid_num_start {
        if let Some((num_end, num_slice)) = scan_money_number(tail) {
            let p = skip_inline_spaces(tail, num_end);
            if p > num_end {
                for code in CODES {
                    if tail[p..].starts_with(code)
                        && !tail
                            .as_bytes()
                            .get(p + code.len())
                            .is_some_and(u8::is_ascii_alphanumeric)
                    {
                        let suffix = &tail[num_end..p + code.len()];
                        return Some(("", num_slice, suffix, start + p + code.len()));
                    }
                }
            }
            for sym in SUFFIX_SYMBOLS {
                if tail[p..].starts_with(sym) {
                    let after_sym = p + sym.len();
                    // Avoid mistaking a prefix symbol of the next amount (e.g., `Item 1 $9.99`)
                    // for a suffix of the preceding number.
                    if *sym != "元"
                        && tail
                            .as_bytes()
                            .get(after_sym)
                            .is_some_and(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-'))
                    {
                        continue;
                    }
                    let suffix = &tail[num_end..after_sym];
                    return Some(("", num_slice, suffix, start + after_sym));
                }
            }
        }
    }

    None
}

fn scan_money_number(tail: &str) -> Option<(usize, &str)> {
    let bytes = tail.as_bytes();
    let mut p = 0usize;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        p += 1;
    }
    let d_start = p;
    if !bytes.get(p).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let mut had_comma = false;
    while p < bytes.len() && (bytes[p].is_ascii_digit() || bytes[p] == b',') {
        if bytes[p] == b',' {
            had_comma = true;
        }
        p += 1;
    }
    while p > d_start && bytes[p - 1] == b',' {
        p -= 1;
    }
    if bytes.get(p) == Some(&b'.') && bytes.get(p + 1).is_some_and(u8::is_ascii_digit) {
        p += 1;
        let frac_start = p;
        while p < bytes.len() && bytes[p].is_ascii_digit() {
            p += 1;
        }
        let frac_len = p - frac_start;
        // European thousands + decimal comma: e.g. `1.234,56`
        if !had_comma
            && frac_len == 3
            && bytes.get(p) == Some(&b',')
            && bytes.get(p + 1).is_some_and(u8::is_ascii_digit)
        {
            let dec_start = p + 1;
            let mut q = dec_start;
            while q < bytes.len() && bytes[q].is_ascii_digit() {
                q += 1;
            }
            if (1..=2).contains(&(q - dec_start))
                && !bytes.get(q).is_some_and(u8::is_ascii_alphanumeric)
            {
                p = q;
            }
        }
    }
    if bytes.get(p).is_some_and(u8::is_ascii_alphanumeric) {
        return None;
    }
    Some((p, &tail[..p]))
}

fn match_uuid(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
        return None;
    }
    if start + 36 > bytes.len() {
        return None;
    }
    let seg = &bytes[start..start + 36];
    let valid = seg[0..8].iter().all(u8::is_ascii_hexdigit)
        && seg[8] == b'-'
        && seg[9..13].iter().all(u8::is_ascii_hexdigit)
        && seg[13] == b'-'
        && seg[14..18].iter().all(u8::is_ascii_hexdigit)
        && seg[18] == b'-'
        && seg[19..23].iter().all(u8::is_ascii_hexdigit)
        && seg[23] == b'-'
        && seg[24..36].iter().all(u8::is_ascii_hexdigit);
    if !valid {
        return None;
    }
    if bytes.get(start + 36).is_some_and(u8::is_ascii_alphanumeric) {
        return None;
    }
    Some(start + 36)
}

fn match_payment_processor_id(s: &str, start: usize) -> Option<usize> {
    const PREFIXES: &[&str] = &[
        "in_", "cus_", "pi_", "pm_", "ch_", "sub_", "cs_", "py_", "re_",
    ];
    let bytes = s.as_bytes();
    if start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        return None;
    }
    let tail = &s[start..];
    for prefix in PREFIXES {
        if let Some(after_prefix) = tail.strip_prefix(prefix) {
            let mut offset = prefix.len();
            if *prefix == "cs_"
                && (after_prefix.starts_with("test_") || after_prefix.starts_with("live_"))
            {
                offset += 5;
            }
            let body_start = offset;
            let mut p = offset;
            let mut has_digit = false;
            let mut has_upper = false;
            let mut has_lower = false;
            while p < tail.len() && tail.as_bytes()[p].is_ascii_alphanumeric() {
                let b = tail.as_bytes()[p];
                if b.is_ascii_digit() {
                    has_digit = true;
                } else if b.is_ascii_uppercase() {
                    has_upper = true;
                } else if b.is_ascii_lowercase() {
                    has_lower = true;
                }
                p += 1;
            }
            let body_len = p - body_start;
            let is_mixed_case = has_upper && has_lower;
            let trailing_underscore = tail.as_bytes().get(p) == Some(&b'_');
            if body_len >= 8
                && (!trailing_underscore || has_digit)
                && (has_digit || (is_mixed_case && body_len >= 12))
            {
                return Some(start + p);
            }
        }
    }
    None
}

fn match_cn_resident_id(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
        return None;
    }
    if start + 18 > bytes.len() {
        return None;
    }
    let seg = &bytes[start..start + 18];
    if !seg[..17].iter().all(u8::is_ascii_digit) {
        return None;
    }
    if !(seg[17].is_ascii_digit() || matches!(seg[17], b'X' | b'x')) {
        return None;
    }
    if bytes.get(start + 18).is_some_and(u8::is_ascii_alphanumeric) {
        return None;
    }
    Some(start + 18)
}

fn is_uscc_char(b: u8) -> bool {
    b.is_ascii_digit()
        || matches!(
            b,
            b'A'..=b'H' | b'J'..=b'N' | b'P'..=b'R' | b'T' | b'U' | b'W'..=b'Y'
        )
}

fn match_cn_uscc(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
        return None;
    }
    if start + 18 > bytes.len() {
        return None;
    }
    let seg = &bytes[start..start + 18];
    if !(is_uscc_char(seg[0])
        && is_uscc_char(seg[1])
        && seg[2..8].iter().all(u8::is_ascii_digit)
        && seg[8..18].iter().copied().all(is_uscc_char))
    {
        return None;
    }
    if bytes.get(start + 18).is_some_and(u8::is_ascii_alphanumeric) {
        return None;
    }
    Some(start + 18)
}

fn match_eu_vat(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
        return None;
    }
    let tail = bytes.get(start..)?;
    if tail.len() < 10 {
        return None;
    }
    let is_up_num = |b: u8| b.is_ascii_uppercase() || b.is_ascii_digit();
    let len = match [tail[0], tail[1]] {
        // DE\d{9}
        [b'D', b'E'] if tail.len() >= 11 && tail[2..11].iter().all(u8::is_ascii_digit) => Some(11),
        // FR[A-Z0-9]{2}\d{9}
        [b'F', b'R']
            if tail.len() >= 13
                && is_up_num(tail[2])
                && is_up_num(tail[3])
                && tail[4..13].iter().all(u8::is_ascii_digit) =>
        {
            Some(13)
        }
        // GB\d{12} or GB\d{9}
        [b'G', b'B'] if tail.len() >= 14 && tail[2..14].iter().all(u8::is_ascii_digit) => Some(14),
        [b'G', b'B'] if tail.len() >= 11 && tail[2..11].iter().all(u8::is_ascii_digit) => Some(11),
        // IE\d[A-Z0-9]\d{5}[A-Z]{1,2}
        [b'I', b'E']
            if tail.len() >= 10
                && tail[2].is_ascii_digit()
                && is_up_num(tail[3])
                && tail[4..9].iter().all(u8::is_ascii_digit)
                && tail[9].is_ascii_uppercase() =>
        {
            if tail.get(10).is_some_and(u8::is_ascii_uppercase) {
                Some(11)
            } else {
                Some(10)
            }
        }
        // NL\d{9}B\d{2}
        [b'N', b'L']
            if tail.len() >= 14
                && tail[2..11].iter().all(u8::is_ascii_digit)
                && tail[11] == b'B'
                && tail[12..14].iter().all(u8::is_ascii_digit) =>
        {
            Some(14)
        }
        // ES[A-Z0-9]\d{7}[A-Z0-9]
        [b'E', b'S']
            if tail.len() >= 11
                && is_up_num(tail[2])
                && tail[3..10].iter().all(u8::is_ascii_digit)
                && is_up_num(tail[10]) =>
        {
            Some(11)
        }
        // IT\d{11}
        [b'I', b'T'] if tail.len() >= 13 && tail[2..13].iter().all(u8::is_ascii_digit) => Some(13),
        _ => None,
    }?;
    let end = start + len;
    if bytes.get(end).is_some_and(u8::is_ascii_alphanumeric) {
        return None;
    }
    Some(end)
}

fn match_us_ssn(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if start > 0
        && (bytes[start - 1].is_ascii_alphanumeric() || matches!(bytes[start - 1], b'-' | b'.'))
    {
        return None;
    }
    if start + 11 > bytes.len() {
        return None;
    }
    let seg = &bytes[start..start + 11];
    if !(seg[0..3].iter().all(u8::is_ascii_digit)
        && seg[3] == b'-'
        && seg[4..6].iter().all(u8::is_ascii_digit)
        && seg[6] == b'-'
        && seg[7..11].iter().all(u8::is_ascii_digit))
    {
        return None;
    }
    let end = start + 11;
    if end < bytes.len()
        && (bytes[end].is_ascii_alphanumeric()
            || bytes[end] == b'-'
            || (bytes[end] == b'.' && bytes.get(end + 1).is_some_and(u8::is_ascii_digit)))
    {
        return None;
    }
    Some(end)
}

fn match_us_ein(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if start > 0
        && (bytes[start - 1].is_ascii_alphanumeric() || matches!(bytes[start - 1], b'-' | b'.'))
    {
        return None;
    }
    if start + 10 > bytes.len() {
        return None;
    }
    let seg = &bytes[start..start + 10];
    if !(seg[0..2].iter().all(u8::is_ascii_digit)
        && seg[2] == b'-'
        && seg[3..10].iter().all(u8::is_ascii_digit))
    {
        return None;
    }
    let end = start + 10;
    if end < bytes.len()
        && (bytes[end].is_ascii_alphanumeric()
            || bytes[end] == b'-'
            || (bytes[end] == b'.' && bytes.get(end + 1).is_some_and(u8::is_ascii_digit)))
    {
        return None;
    }
    if !has_ein_context(s, start, end) {
        return None;
    }
    Some(end)
}

fn has_ein_context(s: &str, start: usize, end: usize) -> bool {
    let mut win_start = start.saturating_sub(48);
    while win_start > 0 && !s.is_char_boundary(win_start) {
        win_start -= 1;
    }
    let mut win_end = (end + 24).min(s.len());
    while win_end < s.len() && !s.is_char_boundary(win_end) {
        win_end += 1;
    }
    let window = s[win_start..win_end].to_ascii_lowercase();
    window.contains("tax")
        || window.contains("fein")
        || window.contains("employer")
        || window.contains("federal")
        || window.contains("税")
        || contains_ascii_word(&window, "ein")
        || contains_ascii_word(&window, "tin")
        || contains_ascii_word(&window, "irs")
}

fn contains_ascii_word(haystack: &str, word: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut pos = 0usize;
    while let Some(rel) = haystack[pos..].find(word) {
        let abs = pos + rel;
        let end = abs + word.len();
        let before_ok = abs == 0 || !bytes[abs - 1].is_ascii_alphabetic();
        let after_ok = end == bytes.len() || !bytes[end].is_ascii_alphabetic();
        if before_ok && after_ok {
            return true;
        }
        pos = end;
    }
    false
}

fn match_last4_card_ref(s: &str, start: usize) -> Option<(&str, &str, usize)> {
    let bytes = s.as_bytes();
    let tail = &s[start..];

    // 1. English last-4 references (word boundary before start)
    if start == 0 || !bytes[start - 1].is_ascii_alphanumeric() {
        const EN_PREFIXES: &[&str] = &["ending in", "ends in", "last 4 digits", "last 4 digit"];
        for prefix in EN_PREFIXES {
            if tail.len() > prefix.len() + 4
                && tail.is_char_boundary(prefix.len())
                && tail[..prefix.len()].eq_ignore_ascii_case(prefix)
            {
                let mut p = prefix.len();
                let mut had_colon = false;
                if tail.as_bytes().get(p) == Some(&b':') {
                    p += 1;
                    had_colon = true;
                }
                let ws_start = p;
                p = skip_inline_spaces(tail, p);
                if (p > ws_start || had_colon)
                    && p + 4 <= tail.len()
                    && tail.as_bytes()[p..p + 4].iter().all(u8::is_ascii_digit)
                    && !tail
                        .as_bytes()
                        .get(p + 4)
                        .is_some_and(u8::is_ascii_alphanumeric)
                {
                    return Some((&tail[..p], &tail[p..p + 4], start + p + 4));
                }
            }
        }
    }

    // 2. Chinese last-4 references (`卡号尾号`, `尾号`)
    const CJK_PREFIXES: &[&str] = &["卡号尾号", "尾号"];
    for prefix in CJK_PREFIXES {
        if tail.starts_with(prefix) {
            let mut p = prefix.len();
            if let Some(ch @ (':' | '：')) = tail[p..].chars().next() {
                p += ch.len_utf8();
            }
            p = skip_inline_spaces(tail, p);
            if p + 4 <= tail.len()
                && tail.as_bytes()[p..p + 4].iter().all(u8::is_ascii_digit)
                && !tail
                    .as_bytes()
                    .get(p + 4)
                    .is_some_and(u8::is_ascii_alphanumeric)
            {
                return Some((&tail[..p], &tail[p..p + 4], start + p + 4));
            }
        }
    }

    None
}

fn match_prefixed_code(s: &str, start: usize) -> Option<usize> {
    const PREFIXES: &[&str] = &[
        "INV", "ORD", "SUB", "TXN", "REF", "REC", "SYN", "ACCT", "CUST", "PO", "BILL", "CH",
        "INVOICE",
    ];
    let bytes = s.as_bytes();
    if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
        return None;
    }
    let tail = &s[start..];
    for prefix in PREFIXES {
        if tail.len() > prefix.len() + 2
            && tail.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
            && matches!(tail.as_bytes()[prefix.len()], b'-' | b'_' | b'#')
        {
            let mut p = prefix.len() + 1;
            let body_start = p;
            let mut has_digit = false;
            while p < tail.len()
                && (tail.as_bytes()[p].is_ascii_alphanumeric()
                    || matches!(tail.as_bytes()[p], b'-' | b'_'))
            {
                if tail.as_bytes()[p].is_ascii_digit() {
                    has_digit = true;
                }
                p += 1;
            }
            if p - body_start >= 3 && has_digit {
                return Some(start + p);
            }
        }
    }
    None
}

fn match_masked_card(s: &str, start: usize) -> Option<usize> {
    let tail = &s[start..];
    if !(tail.starts_with("****")
        || tail.starts_with("••••")
        || tail.starts_with("＊＊＊＊")
        || tail.starts_with("XXXX"))
    {
        return None;
    }
    let mut p = 0usize;
    let mut digits = 0usize;
    for (idx, ch) in tail.char_indices() {
        if matches!(ch, '*' | '•' | '＊' | 'X' | 'x' | '-' | ' ') {
            p = idx + ch.len_utf8();
        } else if ch.is_ascii_digit() {
            digits += 1;
            p = idx + ch.len_utf8();
        } else {
            break;
        }
    }
    while p > 0 && matches!(tail.as_bytes()[p - 1], b' ' | b'-') {
        p -= 1;
    }
    if digits >= 4 {
        Some(start + p)
    } else {
        None
    }
}

fn match_mid_masked_card_or_phone(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'*') {
        return None;
    }
    let tail = &s[start..];
    let mut p = 0usize;

    // Optional international prefix like `+86 ` before `138****8000`
    if bytes[start] == b'+' {
        p += 1;
        let cc_start = p;
        while p < tail.len() && tail.as_bytes()[p].is_ascii_digit() {
            p += 1;
        }
        if !(1..=3).contains(&(p - cc_start)) {
            return None;
        }
        if matches!(tail.as_bytes().get(p), Some(b' ' | b'-')) {
            p += 1;
        } else {
            return None;
        }
    }

    // Leading digits (e.g., `138`, `6222`, `4111 11`)
    let mut lead_digits = 0usize;
    while p < tail.len() {
        let b = tail.as_bytes()[p];
        if b.is_ascii_digit() {
            lead_digits += 1;
            p += 1;
        } else if matches!(b, b' ' | b'-')
            && lead_digits >= 2
            && tail.as_bytes().get(p + 1).is_some_and(u8::is_ascii_digit)
        {
            p += 1;
        } else {
            break;
        }
    }
    if !(2..=8).contains(&lead_digits) {
        return None;
    }

    // Optional single space or hyphen between leading digits and mask
    if matches!(tail.as_bytes().get(p), Some(b' ' | b'-')) {
        p += 1;
    }

    // Mask run: >= 3 mask chars (`*`, `•`, `＊`, `X`, `x`) with optional internal spaces/hyphens
    let mut mask_chars = 0usize;
    let mut mask_end = p;
    while p < tail.len() {
        let ch = tail[p..].chars().next()?;
        if matches!(ch, '*' | '•' | '＊' | 'X' | 'x') {
            mask_chars += 1;
            p += ch.len_utf8();
            mask_end = p;
        } else if matches!(ch, ' ' | '-') && mask_chars > 0 {
            p += 1;
        } else {
            break;
        }
    }
    if mask_chars < 3 || p > mask_end + 1 {
        return None;
    }

    // Trailing digits (e.g., `1234`, `8000`)
    let mut trail_digits = 0usize;
    while p < tail.len() && tail.as_bytes()[p].is_ascii_digit() {
        trail_digits += 1;
        p += 1;
    }
    if !(4..=6).contains(&trail_digits) {
        return None;
    }

    if p < tail.len() && (tail.as_bytes()[p].is_ascii_alphanumeric() || tail.as_bytes()[p] == b'*')
    {
        return None;
    }
    Some(start + p)
}

fn match_parenthesized_phone(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'.') {
        return None;
    }
    let mut p = start;
    let mut total_digits = 0usize;

    // Optional `+CC ` or `+CC-` country code prefix before `(`
    if bytes[p] == b'+' {
        p += 1;
        let cc_start = p;
        while p < bytes.len() && bytes[p].is_ascii_digit() {
            p += 1;
        }
        let cc_len = p - cc_start;
        if !(1..=3).contains(&cc_len) {
            return None;
        }
        total_digits += cc_len;
        if matches!(bytes.get(p), Some(b' ' | b'-')) {
            p += 1;
        }
    }

    if bytes.get(p) != Some(&b'(') {
        return None;
    }
    p += 1;
    let area_start = p;
    while p < bytes.len() && bytes[p].is_ascii_digit() {
        p += 1;
    }
    let area_len = p - area_start;
    if !(2..=4).contains(&area_len) || bytes.get(p) != Some(&b')') {
        return None;
    }
    total_digits += area_len;
    p += 1;

    if matches!(bytes.get(p), Some(b' ' | b'-' | b'.')) {
        p += 1;
    }
    if !bytes.get(p).is_some_and(u8::is_ascii_digit) {
        return None;
    }

    let mut sub_digits = 0usize;
    let mut last_digit_end = p;
    while p < bytes.len() {
        if bytes[p].is_ascii_digit() {
            sub_digits += 1;
            total_digits += 1;
            p += 1;
            last_digit_end = p;
        } else if matches!(bytes[p], b'-' | b' ' | b'.')
            && bytes.get(p + 1).is_some_and(u8::is_ascii_digit)
        {
            p += 1;
        } else {
            break;
        }
    }

    if last_digit_end < bytes.len()
        && (bytes[last_digit_end].is_ascii_alphanumeric()
            || (bytes[last_digit_end] == b'.'
                && bytes
                    .get(last_digit_end + 1)
                    .is_some_and(u8::is_ascii_digit)))
    {
        return None;
    }
    if total_digits >= 10 && sub_digits >= 6 {
        Some(last_digit_end)
    } else {
        None
    }
}

fn match_dotted_phone(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'.') {
        return None;
    }
    let mut p = start;
    if bytes[p] == b'+' {
        p += 1;
        let cc_start = p;
        while p < bytes.len() && bytes[p].is_ascii_digit() {
            p += 1;
        }
        if !(1..=3).contains(&(p - cc_start)) || bytes.get(p) != Some(&b'.') {
            return None;
        }
        p += 1;
    } else if bytes[p] == b'1' && bytes.get(p + 1) == Some(&b'.') {
        p += 2;
    }

    if p + 12 > bytes.len() {
        return None;
    }
    if !(bytes[p..p + 3].iter().all(u8::is_ascii_digit)
        && bytes[p + 3] == b'.'
        && bytes[p + 4..p + 7].iter().all(u8::is_ascii_digit)
        && bytes[p + 7] == b'.'
        && bytes[p + 8..p + 12].iter().all(u8::is_ascii_digit))
    {
        return None;
    }
    let end = p + 12;
    if end < bytes.len()
        && (bytes[end].is_ascii_alphanumeric()
            || (bytes[end] == b'.' && bytes.get(end + 1).is_some_and(u8::is_ascii_digit)))
    {
        return None;
    }
    Some(end)
}

fn match_long_number_or_phone(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if start > 0
        && (bytes[start - 1].is_ascii_alphanumeric()
            || bytes[start - 1] == b'.'
            || (bytes[start - 1] == b'-' && start >= 2 && bytes[start - 2].is_ascii_digit()))
    {
        return None;
    }
    let has_plus = bytes[start] == b'+';
    if !has_plus && !bytes[start].is_ascii_digit() {
        return None;
    }
    let mut p = if has_plus { start + 1 } else { start };
    if !bytes.get(p).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let mut digits = 0usize;
    let mut last_digit_end = p;
    let mut sep_count = 0usize;
    while p < bytes.len() {
        if bytes[p].is_ascii_digit() {
            digits += 1;
            p += 1;
            last_digit_end = p;
        } else if matches!(bytes[p], b'-' | b' ')
            && bytes.get(p + 1).is_some_and(u8::is_ascii_digit)
        {
            sep_count += 1;
            p += 1;
        } else {
            break;
        }
    }
    if last_digit_end < bytes.len()
        && (bytes[last_digit_end].is_ascii_alphanumeric()
            || (bytes[last_digit_end] == b'.'
                && bytes
                    .get(last_digit_end + 1)
                    .is_some_and(u8::is_ascii_digit)))
    {
        return None;
    }
    if (sep_count == 0 && digits >= 6)
        || (sep_count > 0 && digits >= 10)
        || (has_plus && digits >= 8)
    {
        Some(last_digit_end)
    } else {
        None
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Fail> {
    let mut file = match crate::platform::open_new_nofollow(path) {
        Ok(file) => file,
        Err(err)
            if err.kind() == io::ErrorKind::AlreadyExists
                || crate::platform::exclusive_create_refused(&err) =>
        {
            return Err(reject("output_exists"));
        }
        Err(err) => {
            return Err(Fail::new(EXIT_INTEGRITY, "IntegrityAlert", "io_error")
                .extra("during", json!("redact_write"))
                .extra("os_error", json!(err.to_string()))
                .extra("output_written", json!(false))
                .extra("read_only", json!(true)));
        }
    };
    if let Err(err) = file
        .write_all(bytes)
        .and_then(|_| file.flush())
        .and_then(|_| file.sync_all())
    {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(Fail::new(EXIT_INTEGRITY, "IntegrityAlert", "io_error")
            .extra("during", json!("redact_write"))
            .extra("os_error", json!(err.to_string()))
            .extra("output_written", json!(false))
            .extra("read_only", json!(true)));
    }
    Ok(())
}

fn reject(reason: &'static str) -> Fail {
    Fail::new(EXIT_REJECTED, "Rejected", reason)
        .extra("read_only", json!(true))
        .extra("new_effect", json!(false))
        .extra("output_written", json!(false))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_chinese_fapiao_fields_uscc_and_yuan_amounts() {
        let input = concat!(
            "名 称：北京星云科技有限公司\n",
            "名\u{3000}称：上海测试网络有限公司\n",
            "购买方名称：深圳示例科技股份有限公司\n",
            "纳税人识别号：91110108MA01ABCD12\n",
            "统一社会信用代码 91310000775785552L 与 91110108MA01ABCD12\n",
            "地址、电话：北京市海淀区中关村大街1号 010-12345678\n",
            "开户行及账号：中国工商银行 6222021001123456789\n",
            "发票代码：011002200111\n",
            "发票号码：24112000000123456789\n",
            "校验码：12345 67890 12345 67890\n",
            "机器编号：661619901234\n",
            "订单号：ORD202610150001\n",
            "开票人：王五\n",
            "收款人：张三\n",
            "复 核：李四\n",
            "价税合计：98.00元\n",
            "优惠：98元\n",
            "标签：ABCDEFGHJKLMNPQRTU\n",
        );

        let mut redactor = Redactor::new(Vec::new());
        let (out, _) = redact_plain_text(input, &mut redactor);

        assert!(out.contains("名 称：[REDACTED_FIELD_"), "{out}");
        assert!(out.contains("名\u{3000}称：[REDACTED_FIELD_"), "{out}");
        assert!(out.contains("购买方名称：[REDACTED_FIELD_"), "{out}");
        assert!(out.contains("纳税人识别号：[REDACTED_FIELD_"), "{out}");
        assert!(
            out.contains("统一社会信用代码 [REDACTED_ID_1] 与 [REDACTED_ID_2]"),
            "{out}"
        );
        assert!(out.contains("地址、电话：[REDACTED_FIELD_"), "{out}");
        assert!(out.contains("开户行及账号：[REDACTED_FIELD_"), "{out}");
        assert!(out.contains("发票号码：[REDACTED_FIELD_"), "{out}");
        assert!(out.contains("价税合计：[AMOUNT_"), "{out}");
        assert!(out.contains("]元"), "{out}");
        assert!(out.contains("标签：ABCDEFGHJKLMNPQRTU"), "{out}");

        for secret in [
            "北京星云科技有限公司",
            "上海测试网络有限公司",
            "深圳示例科技股份有限公司",
            "91110108MA01ABCD12",
            "91310000775785552L",
            "中关村大街1号",
            "010-12345678",
            "6222021001123456789",
            "011002200111",
            "24112000000123456789",
            "661619901234",
            "ORD202610150001",
            "王五",
            "张三",
            "李四",
            "98.00",
            "98元",
        ] {
            assert!(!out.contains(secret), "leaked {secret:?} in:\n{out}");
        }
    }

    #[test]
    fn redacts_national_ids_phones_masked_cards_and_last4_refs() {
        let input = concat!(
            "Resident ID 11010119900307123X and 31010119851215002x\n",
            "US SSN 123-45-6789\n",
            "Federal Tax EIN 12-3456789\n",
            "FEIN: 98-7654321\n",
            "Catalog code 12-3456789\n",
            "Phones: (415) 555-0199, +1 (415) 555-0199, 415.555.0199\n",
            "Landlines: 010-12345678 and 0755-88886666.\n",
            "Cards: 6222 **** **** 1234 and **** **** **** 4242\n",
            "Cell numbers: 138****8000 and 138 **** 8000\n",
            "Paid with Visa ending in 4242, card ends in 6184, last 4 digits: 9012\n",
            "银行卡尾号8899 / 卡号尾号 7766\n",
            "Resolution: 1920x1080\n",
        );

        let mut redactor = Redactor::new(Vec::new());
        let (out, _) = redact_plain_text(input, &mut redactor);

        assert_eq!(redactor.counts.header_fields, 0, "{out}");
        assert!(out.contains("Catalog code 12-3456789"), "{out}");
        assert!(out.contains("Resolution: 1920x1080"), "{out}");
        assert!(out.contains("Cell numbers: [REDACTED_ID_"), "{out}");
        assert!(out.contains("Visa ending in [REDACTED_ID_"), "{out}");
        assert!(out.contains("card ends in [REDACTED_ID_"), "{out}");
        assert!(out.contains("last 4 digits: [REDACTED_ID_"), "{out}");
        assert!(out.contains("银行卡尾号[REDACTED_ID_"), "{out}");
        assert!(out.contains("卡号尾号 [REDACTED_ID_"), "{out}");

        for secret in [
            "11010119900307123X",
            "31010119851215002x",
            "123-45-6789",
            "98-7654321",
            "(415) 555-0199",
            "+1 (415) 555-0199",
            "555-0199",
            "415.555.0199",
            "010-12345678",
            "0755-88886666",
            "6222 **** **** 1234",
            "**** **** **** 4242",
            "138****8000",
            "138 **** 8000",
            "4242",
            "6184",
            "9012",
            "8899",
            "7766",
        ] {
            assert!(!out.contains(secret), "leaked {secret:?} in:\n{out}");
        }
        assert!(!out.contains("Federal Tax EIN 12-3456789"), "{out}");
    }

    #[test]
    fn redacts_uuids_payment_ids_eu_vat_dates_and_suffix_amounts() {
        let input = concat!(
            "Trace: 550e8400-e29b-41d4-a716-446655440000\n",
            "Stripe: in_1Ab2Cd3Ef4Gh5Ij6 cus_NffrFeUfNV2Hib pi_3MtwBwLkdIwHu7ix pm_1CardToken99 ch_3Abcdefgh sub_12345678 cs_test_a1b2c3d4e5f6 py_1A2B3C4D re_9Z8Y7X6W\n",
            "Labels: in_memory sub_total re_authentication\n",
            "Registration: DE123456789 FRXX123456789 GB123456789 IE1A23456B NL123456789B01 ESA1234567B IT12345678901\n",
            "Dates: 15 Oct 2026, 3 November 2026, 2026.05.01, 2026.10.15, 1985-04-20\n",
            "Prices: 10,00 €, 19,99 €, 20.00 $, 15.00 £, 98.00元, 98元\n",
            "Qty: Item 1 $9.99\n",
        );

        let mut redactor = Redactor::new(Vec::new());
        let (out, _) = redact_plain_text(input, &mut redactor);

        assert_eq!(redactor.counts.header_fields, 0, "{out}");
        assert!(
            out.contains("Labels: in_memory sub_total re_authentication"),
            "{out}"
        );
        assert!(out.contains("Registration: [REDACTED_ID_"), "{out}");
        assert!(out.contains("Qty: Item 1 $[AMOUNT_"), "{out}");
        assert!(out.contains("[DATE_TEXT_"), "{out}");
        assert!(out.contains("[DATE_ISO_"), "{out}");
        assert!(out.contains("] €"), "{out}");
        assert!(out.contains("] $"), "{out}");
        assert!(out.contains("] £"), "{out}");
        assert!(out.contains("]元"), "{out}");

        for secret in [
            "550e8400-e29b-41d4-a716-446655440000",
            "in_1Ab2Cd3Ef4Gh5Ij6",
            "cus_NffrFeUfNV2Hib",
            "pi_3MtwBwLkdIwHu7ix",
            "pm_1CardToken99",
            "ch_3Abcdefgh",
            "sub_12345678",
            "cs_test_a1b2c3d4e5f6",
            "py_1A2B3C4D",
            "re_9Z8Y7X6W",
            "DE123456789",
            "FRXX123456789",
            "GB123456789",
            "IE1A23456B",
            "NL123456789B01",
            "ESA1234567B",
            "IT12345678901",
            "15 Oct 2026",
            "3 November 2026",
            "2026.05.01",
            "2026.10.15",
            "1985-04-20",
            "10,00",
            "19,99",
            "20.00",
            "15.00",
            "98.00",
            "9.99",
        ] {
            assert!(!out.contains(secret), "leaked {secret:?} in:\n{out}");
        }
    }

    #[test]
    fn redacts_html_adjacent_cells_entities_and_expanded_url_attrs() {
        let html = concat!(
            "<!doctype html><html><body>\n",
            "<table>\n",
            "<tr><th>纳税人识别号</th><td>91110108MA01ABCD12</td></tr>\n",
            "<tr><th>名 称：</th><td>星云科技有限公司</td></tr>\n",
            "<tr><td>Billing Name</td><td>Alice Secret</td><td>订单号</td><td>ORD-889900</td></tr>\n",
            "<tr><th>Amount</th><td>19,99 €</td></tr>\n",
            "</table>\n",
            "<p>Contact: alice&#64;example&#x2E;com for AT&amp;T &lt;Pro&gt;</p>\n",
            "<img srcset=\"https://cdn.example.com/u1-1x.png 1x, https://cdn.example.com/u1-2x.png 2x\" />\n",
            "<button formaction=\"https://pay.example.com/checkout?acct=secret99\">Pay</button>\n",
            "<svg><use xlink:href=\"https://tracker.example.com/icon.svg#user42\" /></svg>\n",
            "</body></html>\n",
        );

        let mut redactor = Redactor::new(Vec::new());
        let (out, lines) = redact_html(html, &mut redactor);

        assert!(!lines.is_empty());
        assert_eq!(redactor.counts.header_fields, 4, "{out}");
        assert_eq!(redactor.counts.amounts, 1, "{out}");
        assert_eq!(redactor.counts.emails, 1, "{out}");
        assert_eq!(redactor.counts.urls_and_ips, 3, "{out}");
        assert!(
            out.contains("<tr><th>纳税人识别号</th><td>[REDACTED_FIELD_"),
            "{out}"
        );
        assert!(
            out.contains("<tr><th>名 称：</th><td>[REDACTED_FIELD_"),
            "{out}"
        );
        assert!(
            out.contains("<td>Billing Name</td><td>[REDACTED_FIELD_"),
            "{out}"
        );
        assert!(out.contains("<td>订单号</td><td>[REDACTED_FIELD_"), "{out}");
        assert!(out.contains("<tr><th>Amount</th><td>[AMOUNT_"), "{out}");
        assert!(
            out.contains("Contact: [REDACTED_EMAIL_1] for AT&amp;T &lt;Pro&gt;"),
            "{out}"
        );
        assert!(out.contains("srcset=\"[REDACTED_URL_"), "{out}");
        assert!(out.contains("formaction=\"[REDACTED_URL_"), "{out}");
        assert!(out.contains("xlink:href=\"[REDACTED_URL_"), "{out}");

        for secret in [
            "91110108MA01ABCD12",
            "星云科技有限公司",
            "Alice Secret",
            "ORD-889900",
            "19,99",
            "alice",
            "example.com",
            "cdn.example.com",
            "pay.example.com",
            "tracker.example.com",
            "secret99",
            "user42",
        ] {
            assert!(!out.contains(secret), "leaked {secret:?} in:\n{out}");
        }
    }
}
