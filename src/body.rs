use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::ids::hex_encode;

pub const EXTRACTOR: &str = "subvault-body";
pub const EXTRACTOR_VERSION: &str = "1";
pub const NORMALIZATION: &str = "identity-utf8-scalars-1";
pub const JOIN: &str = "single-fragment-1";
pub const RULES_VERSION: &str = "stated-lines-1";
pub const LINE_ENDING: &str = "lf-or-crlf-1";
pub const MEDIA_TYPE: &str = "text/plain;charset=utf-8";
pub const ROLE: &str = "user_provided_fragment";
pub const OFFSET_UNIT: &str = "unicode_scalar_in_versioned_extracted_string";
pub const MAX_FRAGMENT_BYTES: u64 = 65_536;
pub const MAX_LINE_BYTES: usize = 1024;
pub const MAX_CANDIDATES: usize = 64;

pub enum InputError {
    Empty,
    InvalidUtf8 { bytes_read: u64 },
    OverBudget { stat_size: u64 },
}

pub enum ExtractError {
    OverLine,
    TooMany,
}

#[derive(Clone)]
pub struct Draft {
    pub kind: &'static str,
    pub usable: bool,
    pub excerpt: String,
    pub scalar_start: usize,
    pub scalar_end: usize,
    pub byte_start: usize,
    pub byte_end: usize,
    pub value: Map<String, Value>,
}

struct Line {
    text: String,
    scalar_start: usize,
    scalar_end: usize,
    byte_start: usize,
    byte_end: usize,
    class: Class,
}

enum Class {
    Amount(String),
    Total(String),
    CreditLabel,
    OrderLabel,
    RefundNotStated,
    Email(String),
    Period(String, String),
    StatusAbsent(String),
    Other,
}

pub fn config_json() -> Value {
    json!({
        "extractor": EXTRACTOR,
        "extractor_version": EXTRACTOR_VERSION,
        "normalization": NORMALIZATION,
        "join": JOIN,
        "rules_version": RULES_VERSION,
        "line_ending": LINE_ENDING,
        "max_fragment_bytes": MAX_FRAGMENT_BYTES,
        "max_line_bytes": MAX_LINE_BYTES,
        "max_candidates": MAX_CANDIDATES,
        "media_type": MEDIA_TYPE,
        "role": ROLE,
        "offset_unit": OFFSET_UNIT
    })
}

pub fn logical_key(content_sha256: &str) -> String {
    let parts = [
        EXTRACTOR,
        EXTRACTOR_VERSION,
        NORMALIZATION,
        JOIN,
        RULES_VERSION,
        LINE_ENDING,
        &MAX_FRAGMENT_BYTES.to_string(),
        &MAX_LINE_BYTES.to_string(),
        &MAX_CANDIDATES.to_string(),
        MEDIA_TYPE,
        ROLE,
        OFFSET_UNIT,
        content_sha256,
    ];
    let mut hasher = Sha256::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            hasher.update([0xff]);
        }
        hasher.update(part.as_bytes());
    }
    hex_encode(&hasher.finalize())
}

pub fn prepare(bytes: &[u8]) -> Result<&str, InputError> {
    if bytes.is_empty() {
        return Err(InputError::Empty);
    }
    if bytes.len() as u64 > MAX_FRAGMENT_BYTES {
        return Err(InputError::OverBudget {
            stat_size: bytes.len() as u64,
        });
    }
    std::str::from_utf8(bytes).map_err(|_| InputError::InvalidUtf8 {
        bytes_read: bytes.len() as u64,
    })
}

pub fn extract(text: &str) -> Result<Vec<Draft>, ExtractError> {
    let lines = scan(text)?;
    let total_indexes: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| matches!(line.class, Class::Total(_)).then_some(index))
        .collect();
    let credit = lines
        .iter()
        .any(|line| matches!(line.class, Class::CreditLabel));
    let order = lines
        .iter()
        .any(|line| matches!(line.class, Class::OrderLabel));
    let refund_not_stated = lines
        .iter()
        .any(|line| matches!(line.class, Class::RefundNotStated));
    let mut drafts = Vec::new();
    for line in &lines {
        let draft = match &line.class {
            Class::Amount(amount) => Some(amount_draft(line, amount)),
            Class::Total(amount) => Some(total_draft(
                line,
                amount,
                total_indexes.len(),
                credit,
                order,
                refund_not_stated,
            )),
            Class::Email(label) => Some(email_draft(line, label)),
            Class::Period(start, end) => Some(period_draft(line, start, end)),
            Class::StatusAbsent(date) => Some(status_draft(line, date)),
            Class::CreditLabel | Class::OrderLabel | Class::RefundNotStated | Class::Other => None,
        };
        if let Some(draft) = draft {
            drafts.push(draft);
            if drafts.len() > MAX_CANDIDATES {
                return Err(ExtractError::TooMany);
            }
        }
    }
    Ok(drafts)
}

fn scan(text: &str) -> Result<Vec<Line>, ExtractError> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut byte_pos = 0usize;
    let mut scalar_pos = 0usize;
    while byte_pos < bytes.len() {
        let byte_start = byte_pos;
        let scalar_start = scalar_pos;
        let end = bytes[byte_pos..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| byte_pos + offset)
            .unwrap_or(bytes.len());
        // A CR immediately before LF is the other half of the line ending.
        // It is not part of the sentence and it is not a Unicode rewrite.
        let mut content_end = end;
        if end < bytes.len() && content_end > byte_start && bytes[content_end - 1] == b'\r' {
            content_end -= 1;
        }
        if content_end - byte_start > MAX_LINE_BYTES {
            return Err(ExtractError::OverLine);
        }
        let line = &text[byte_start..content_end];
        let scalars = line.chars().count();
        lines.push(Line {
            text: line.to_string(),
            scalar_start,
            scalar_end: scalar_start + scalars,
            byte_start,
            byte_end: content_end,
            class: classify(line),
        });
        scalar_pos += scalars;
        if content_end < end {
            scalar_pos += 1;
        }
        byte_pos = end;
        if byte_pos < bytes.len() && bytes[byte_pos] == b'\n' {
            byte_pos += 1;
            scalar_pos += 1;
        }
    }
    Ok(lines)
}

fn classify(line: &str) -> Class {
    if let Some(rest) = line.strip_prefix("Amount paid USD ") {
        if let Some(amount) = money(rest) {
            return Class::Amount(amount);
        }
    }
    if let Some(rest) = line.strip_prefix("Total USD ") {
        if let Some(amount) = money(rest) {
            return Class::Total(amount);
        }
    }
    if line == "This Total is a credit-note total, not an amount paid." {
        return Class::CreditLabel;
    }
    if line == "This Total is an order total whose payment nature is not stated." {
        return Class::OrderLabel;
    }
    if line == "Receipt of the refund is not stated." {
        return Class::RefundNotStated;
    }
    if let Some(rest) = line.strip_prefix("Account email: ") {
        if email_token(rest) {
            return Class::Email(rest.to_string());
        }
    }
    if let Some(rest) = line.strip_prefix("Service period: ") {
        if let Some((start, end)) = period(rest) {
            return Class::Period(start, end);
        }
    }
    if let Some(rest) =
        line.strip_prefix("This document states the service period above and no status after ")
    {
        if let Some(date) = rest.strip_suffix('.') {
            if date_token(date) {
                return Class::StatusAbsent(date.to_string());
            }
        }
    }
    Class::Other
}

fn money(text: &str) -> Option<String> {
    let (whole, frac) = text.split_once('.')?;
    if whole.is_empty()
        || frac.len() != 2
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !frac.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some(text.to_string())
}

fn email_token(text: &str) -> bool {
    !text.is_empty() && text.len() <= 200 && text.bytes().all(|byte| byte.is_ascii_graphic())
}

fn date_token(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

fn period(text: &str) -> Option<(String, String)> {
    let (start, end) = text.split_once(" through ")?;
    if date_token(start) && date_token(end) {
        Some((start.to_string(), end.to_string()))
    } else {
        None
    }
}

fn located(line: &Line, kind: &'static str, semantic: &str, usable: bool) -> Draft {
    let mut value = Map::new();
    value.insert("kind".into(), json!(kind));
    value.insert("semantic".into(), json!(semantic));
    value.insert("usable".into(), json!(usable));
    value.insert("effective".into(), json!(false));
    value.insert("adopted".into(), json!(false));
    value.insert("purchase_id".into(), Value::Null);
    value.insert("account_id".into(), Value::Null);
    value.insert("relationship".into(), Value::Null);
    value.insert("observed_at".into(), Value::Null);
    value.insert("time_status".into(), json!("unknown"));
    value.insert("payment_authorization".into(), Value::Null);
    value.insert("current_status".into(), json!("unknown"));
    Draft {
        kind,
        usable,
        excerpt: line.text.clone(),
        scalar_start: line.scalar_start,
        scalar_end: line.scalar_end,
        byte_start: line.byte_start,
        byte_end: line.byte_end,
        value,
    }
}

fn amount_draft(line: &Line, amount: &str) -> Draft {
    let mut draft = located(line, "stated_amount_paid", "amount_paid", true);
    draft.value.insert("amount_text".into(), json!(amount));
    draft.value.insert("currency".into(), json!("USD"));
    draft
}

fn total_draft(
    line: &Line,
    amount: &str,
    total_count: usize,
    credit: bool,
    order: bool,
    refund_not_stated: bool,
) -> Draft {
    let single = total_count == 1;
    let (kind, semantic, usable, reason, payment_nature, refund_receipt) = if !single {
        (
            "unresolved_total",
            "unresolved",
            false,
            Some("multiple_totals_not_paired"),
            Value::Null,
            Value::Null,
        )
    } else if credit && order {
        (
            "unresolved_total",
            "unresolved",
            false,
            Some("conflicting_total_labels"),
            Value::Null,
            Value::Null,
        )
    } else if credit {
        (
            "stated_credit_note_total",
            "credit_note_total",
            true,
            None,
            json!("not_amount_paid"),
            if refund_not_stated {
                json!("not_stated")
            } else {
                Value::Null
            },
        )
    } else if order {
        (
            "stated_order_total",
            "order_total",
            true,
            None,
            json!("not_stated"),
            Value::Null,
        )
    } else {
        (
            "unresolved_total",
            "unresolved",
            false,
            Some("payment_nature_not_stated"),
            Value::Null,
            Value::Null,
        )
    };
    let mut draft = located(line, kind, semantic, usable);
    draft.value.insert("amount_text".into(), json!(amount));
    draft.value.insert("currency".into(), json!("USD"));
    draft.value.insert("payment_nature".into(), payment_nature);
    draft.value.insert("refund_receipt".into(), refund_receipt);
    draft.value.insert("reason".into(), json!(reason));
    draft
}

fn email_draft(line: &Line, label: &str) -> Draft {
    let mut draft = located(line, "stated_account_label", "account_label", true);
    draft.value.insert("label".into(), json!(label));
    draft
        .value
        .insert("masked".into(), json!(label.contains('*')));
    draft.value.insert("attributed".into(), json!(false));
    draft
}

fn period_draft(line: &Line, start: &str, end: &str) -> Draft {
    let mut draft = located(line, "stated_service_period", "service_period", true);
    draft.value.insert("period_start".into(), json!(start));
    draft.value.insert("period_end".into(), json!(end));
    draft
}

fn status_draft(line: &Line, date: &str) -> Draft {
    let mut draft = located(
        line,
        "stated_status_absent",
        "status_after_not_stated",
        true,
    );
    draft.value.insert("after".into(), json!(date));
    draft
}
