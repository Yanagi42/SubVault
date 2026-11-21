//! Real-world bilingual (English & Chinese) static bill/invoice/CSV extraction for Format 6.
//!
//! Strictly deterministic, bounded-memory, zero JavaScript execution, zero network calls,
//! and epistemically honest:
//! - Credit notes / refunds (`红字` / `退款` / `credit note`) and unpaid orders/quotes are never
//!   adopted as paid amounts.
//! - Multiple distinct paid amounts on the same document produce `status = "conflict"` rather
//!   than guessing.
//! - Supports major currencies (`USD`, `CNY`, `EUR`, `GBP`, `JPY`, `HKD`, `CAD`, `AUD`, `SGD`,
//!   `TWD`, `KRW`, `CHF`).
//! - Supports UTF-8 (with/without BOM) and GB18030/GBK CSVs (Alipay / WeChat / Bank / Store).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use crate::pdf_parse;

pub const REAL_RULES_VERSION: &str = "real-bilingual-1";
pub const REAL_HTML_RULES_VERSION: &str = "real-html-bilingual-1";
pub const REAL_CSV_RULES_VERSION: &str = "real-csv-bilingual-1";
pub const MAX_REAL_HTML_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_CSV_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_CSV_CANDIDATES: usize = 500;
const MAX_REAL_CANDIDATES: usize = 64;

pub const SUPPORTED_CURRENCIES: &[&str] = &[
    "USD", "CNY", "EUR", "GBP", "JPY", "HKD", "CAD", "AUD", "SGD", "TWD", "KRW", "CHF",
];

pub fn normalize_currency(raw: &str) -> Option<&'static str> {
    let up = raw.trim().to_ascii_uppercase();
    match up.as_str() {
        "RMB" => Some("CNY"),
        other => SUPPORTED_CURRENCIES
            .iter()
            .copied()
            .find(|&code| code == other),
    }
}

pub fn normalize_amount_text(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Accept optional comma thousands separators and either `.` or European `,` decimals.
    let mut cleaned = trimmed.replace([' ', '\u{00a0}'], "");
    if let Some(stripped) = cleaned.strip_prefix('+') {
        cleaned = stripped.to_string();
    }
    if cleaned.starts_with('-') || cleaned.is_empty() {
        return None;
    }
    // Determine decimal separator
    let normalized = if let Some(dot_pos) = cleaned.rfind('.') {
        let frac = &cleaned[dot_pos + 1..];
        let whole_raw = &cleaned[..dot_pos];
        if !frac.bytes().all(|b| b.is_ascii_digit()) || frac.is_empty() || frac.len() > 2 {
            return None;
        }
        let whole = strip_grouping_commas(whole_raw)?;
        let frac_padded = if frac.len() == 1 {
            format!("{frac}0")
        } else {
            frac.to_string()
        };
        format!("{whole}.{frac_padded}")
    } else if let Some(comma_pos) = cleaned.rfind(',') {
        let after = &cleaned[comma_pos + 1..];
        let before = &cleaned[..comma_pos];
        if after.len() == 2
            && after.bytes().all(|b| b.is_ascii_digit())
            && !before.contains(',')
            && !before.is_empty()
            && before.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        {
            // European decimal comma like `12,99` or `1.299,50`
            let whole = before.replace('.', "");
            if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let whole = trim_leading_zeros(&whole);
            format!("{whole}.{after}")
        } else {
            // Thousands separator comma like `1,200`
            let whole = strip_grouping_commas(&cleaned)?;
            format!("{whole}.00")
        }
    } else {
        if !cleaned.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let whole = trim_leading_zeros(&cleaned);
        format!("{whole}.00")
    };
    // Sanity bound: reject absurdly long digit sequences (e.g. order IDs mistaken for money)
    let (whole, frac) = normalized.split_once('.')?;
    if whole.len() > 10 || frac.len() != 2 {
        return None;
    }
    Some(normalized)
}

fn strip_grouping_commas(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    if !raw.contains(',') {
        if !raw.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        return Some(trim_leading_zeros(raw));
    }
    let parts: Vec<&str> = raw.split(',').collect();
    if parts[0].is_empty() || parts[0].len() > 3 || !parts[0].bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    for part in &parts[1..] {
        if part.len() != 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    let joined: String = parts.concat();
    Some(trim_leading_zeros(&joined))
}

fn trim_leading_zeros(digits: &str) -> String {
    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ExtractedHints {
    pub vendor: Option<String>,
    pub plan_name: Option<String>,
    pub account_label: Option<String>,
    pub billing_cycle: Option<String>,
    pub category: Option<String>,
    pub bill_date: Option<String>,
}

impl ExtractedHints {
    pub fn is_empty(&self) -> bool {
        self.vendor.is_none()
            && self.plan_name.is_none()
            && self.account_label.is_none()
            && self.billing_cycle.is_none()
            && self.category.is_none()
            && self.bill_date.is_none()
    }
}

#[derive(Clone, Debug)]
pub struct RealCandidate {
    pub kind: String,
    pub usable: bool,
    pub excerpt: String,
    pub value: Map<String, Value>,
    pub locator: Value,
}

#[derive(Clone, Debug)]
pub struct RealBillExtraction {
    pub status: &'static str, // "matched" | "no_match" | "conflict" | "unsupported"
    pub reason: Option<&'static str>,
    pub page_count: usize,
    pub candidates: Vec<RealCandidate>,
    pub adopt_amount_idx: Option<usize>,
    pub adopt_period_idx: Option<usize>,
    pub hints: ExtractedHints,
}

#[derive(Clone, Debug)]
pub struct RealLine {
    pub text: String,
    pub locator: Value,
}

pub fn extract_real_pdf(bytes: &[u8]) -> RealBillExtraction {
    let pages = match pdf_parse::extract_pages_for_redaction(bytes) {
        Ok(pages) => pages,
        Err(reason) => {
            return RealBillExtraction {
                status: "unsupported",
                reason: Some(reason),
                page_count: 0,
                candidates: Vec::new(),
                adopt_amount_idx: None,
                adopt_period_idx: None,
                hints: ExtractedHints::default(),
            };
        }
    };
    let page_count = pages.len();
    let mut lines = Vec::new();
    for (page_num, page_lines) in pages {
        for (idx, text) in page_lines.into_iter().enumerate() {
            let page_line = idx + 1;
            lines.push(RealLine {
                locator: json!({
                    "kind": "pdf_real_line",
                    "rules_version": REAL_RULES_VERSION,
                    "pdf_page": page_num,
                    "line_no": page_line,
                    "excerpt": text,
                }),
                text,
            });
        }
    }
    extract_from_lines(&lines, page_count, REAL_RULES_VERSION)
}

pub fn extract_real_html(html: &str) -> RealBillExtraction {
    let lines = html_to_lines(html);
    extract_from_lines(&lines, 1, REAL_HTML_RULES_VERSION)
}

pub fn html_to_lines(html: &str) -> Vec<RealLine> {
    let Ok(dom) = tl::parse(html, tl::ParserOptions::default()) else {
        return Vec::new();
    };
    let parser = dom.parser();
    let mut raw_lines: Vec<String> = Vec::new();
    collect_html_lines(dom.children(), parser, &mut raw_lines);
    let mut out = Vec::new();
    for (idx, text) in raw_lines.into_iter().enumerate() {
        let trimmed = collapse_ws(&text);
        if trimmed.is_empty() {
            continue;
        }
        let line_no = idx + 1;
        out.push(RealLine {
            locator: json!({
                "kind": "html_real_line",
                "rules_version": REAL_HTML_RULES_VERSION,
                "line_no": line_no,
                "excerpt": trimmed,
            }),
            text: trimmed,
        });
    }
    out
}

fn collect_html_lines(handles: &[tl::NodeHandle], parser: &tl::Parser, out: &mut Vec<String>) {
    for handle in handles {
        let Some(node) = handle.get(parser) else {
            continue;
        };
        match node {
            tl::Node::Comment(_) => {}
            tl::Node::Raw(raw) => {
                let decoded = html_escape::decode_html_entities(&raw.as_utf8_str()).into_owned();
                let folded = collapse_ws(&decoded);
                if !folded.is_empty() {
                    out.push(folded);
                }
            }
            tl::Node::Tag(tag) => {
                let name = tag.name().as_utf8_str().to_ascii_lowercase();
                if matches!(
                    name.as_str(),
                    "script" | "style" | "noscript" | "template" | "svg" | "head" | "meta" | "link"
                ) {
                    continue;
                }
                if is_html_hidden(tag) {
                    continue;
                }
                if name == "tr" {
                    let cells = collect_tr_cells(tag, parser);
                    if cells.len() == 2 && !cells[0].is_empty() && !cells[1].is_empty() {
                        let sep = if cells[0].ends_with(':') || cells[0].ends_with('：') {
                            " "
                        } else {
                            ": "
                        };
                        out.push(format!("{}{sep}{}", cells[0], cells[1]));
                        continue;
                    } else if !cells.is_empty() {
                        for cell in cells {
                            if !cell.is_empty() {
                                out.push(cell);
                            }
                        }
                        continue;
                    }
                }
                if matches!(
                    name.as_str(),
                    "p" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "dt" | "dd"
                ) {
                    let text = extract_visible_text(tag, parser);
                    if !text.is_empty() {
                        out.push(text);
                    }
                    continue;
                }
                if name == "div" && !has_block_child(tag, parser) {
                    let text = extract_visible_text(tag, parser);
                    if !text.is_empty() {
                        out.push(text);
                    }
                    continue;
                }
                let children: Vec<tl::NodeHandle> = tag.children().top().iter().copied().collect();
                collect_html_lines(&children, parser, out);
            }
        }
    }
}

fn is_html_hidden(tag: &tl::HTMLTag) -> bool {
    let attrs = tag.attributes();
    if attrs.get("hidden").is_some() {
        return true;
    }
    if let Some(Some(aria)) = attrs.get("aria-hidden") {
        if aria.as_utf8_str().trim().eq_ignore_ascii_case("true") {
            return true;
        }
    }
    if let Some(Some(style)) = attrs.get("style") {
        let compact: String = style
            .as_utf8_str()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        if compact.contains("display:none") || compact.contains("visibility:hidden") {
            return true;
        }
    }
    false
}

fn has_block_child(tag: &tl::HTMLTag, parser: &tl::Parser) -> bool {
    for handle in tag.children().top().iter() {
        if let Some(tl::Node::Tag(child)) = handle.get(parser) {
            let name = child.name().as_utf8_str().to_ascii_lowercase();
            if matches!(
                name.as_str(),
                "div"
                    | "p"
                    | "table"
                    | "thead"
                    | "tbody"
                    | "tfoot"
                    | "tr"
                    | "ul"
                    | "ol"
                    | "li"
                    | "section"
                    | "article"
                    | "header"
                    | "footer"
                    | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
            ) {
                return true;
            }
        }
    }
    false
}

fn collect_tr_cells(tr: &tl::HTMLTag, parser: &tl::Parser) -> Vec<String> {
    let mut cells = Vec::new();
    for handle in tr.children().top().iter() {
        if let Some(tl::Node::Tag(child)) = handle.get(parser) {
            let name = child.name().as_utf8_str().to_ascii_lowercase();
            if (name == "td" || name == "th") && !is_html_hidden(child) {
                let text = extract_visible_text(child, parser);
                cells.push(text);
            }
        }
    }
    cells
}

fn extract_visible_text(tag: &tl::HTMLTag, parser: &tl::Parser) -> String {
    let mut buf = String::new();
    fn visit(handles: &[tl::NodeHandle], parser: &tl::Parser, buf: &mut String) {
        for handle in handles {
            let Some(node) = handle.get(parser) else {
                continue;
            };
            match node {
                tl::Node::Comment(_) => {}
                tl::Node::Raw(raw) => {
                    let raw_str = raw.as_utf8_str();
                    let decoded = html_escape::decode_html_entities(&raw_str);
                    buf.push_str(&decoded);
                }
                tl::Node::Tag(child) => {
                    let name = child.name().as_utf8_str().to_ascii_lowercase();
                    if matches!(
                        name.as_str(),
                        "script" | "style" | "noscript" | "template" | "svg"
                    ) || is_html_hidden(child)
                    {
                        continue;
                    }
                    if name == "br" {
                        buf.push(' ');
                        continue;
                    }
                    let kids: Vec<tl::NodeHandle> =
                        child.children().top().iter().copied().collect();
                    visit(&kids, parser, buf);
                }
            }
        }
    }
    let kids: Vec<tl::NodeHandle> = tag.children().top().iter().copied().collect();
    visit(&kids, parser, &mut buf);
    collapse_ws(&buf)
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true;
    for ch in s.chars() {
        if ch.is_whitespace() || ch == '\u{00a0}' {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

// --- Core Bilingual Extraction Logic ---

pub fn extract_from_lines(
    lines: &[RealLine],
    page_count: usize,
    rules_version: &'static str,
) -> RealBillExtraction {
    let joined_lower: String = lines
        .iter()
        .map(|l| l.text.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    let joined_raw: String = lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    let is_credit_or_refund = joined_lower.contains("credit note")
        || joined_lower.contains("credit memo")
        || joined_lower.contains("refund receipt")
        || joined_lower.contains("this total is a credit-note total")
        || joined_raw.contains("红字")
        || joined_raw.contains("已退款")
        || joined_raw.contains("退款金额")
        || joined_raw.contains("退款凭证");

    let is_unpaid_order = joined_lower.contains("order confirmation")
        || joined_lower.contains("payment nature is not stated")
        || joined_lower.contains("proforma")
        || joined_lower.contains("payment failed")
        || joined_lower.contains("unpaid invoice")
        || joined_raw.contains("待付款")
        || joined_raw.contains("未支付")
        || joined_raw.contains("报价单");

    let has_paid_doc_signal = joined_lower.contains("receipt")
        || joined_lower.contains("payment confirmation")
        || joined_lower.contains("invoice paid")
        || joined_lower.contains("status: paid")
        || joined_lower.contains("thank you for your payment")
        || joined_lower.contains("paid on")
        || joined_raw.contains("电子发票")
        || joined_raw.contains("增值税普通发票")
        || joined_raw.contains("增值税专用发票")
        || joined_raw.contains("支付成功")
        || joined_raw.contains("付款成功")
        || joined_raw.contains("交易成功")
        || joined_raw.contains("收据");

    let mut candidates: Vec<RealCandidate> = Vec::new();
    let mut explicit_paid: Vec<(usize, String, String)> = Vec::new();
    let mut fallback_totals: Vec<(usize, String, String)> = Vec::new();
    let mut periods: Vec<(usize, String, String)> = Vec::new();

    let mut skip_next = false;
    for i in 0..lines.len() {
        if skip_next {
            skip_next = false;
            continue;
        }
        if candidates.len() >= MAX_REAL_CANDIDATES {
            break;
        }
        let line = &lines[i];
        let text = line.text.trim();

        // 1. Explicit paid amount (same line OR label on line i + money on line i+1)
        if let Some((cur, amt, used_next)) = match_explicit_paid_amount(lines, i) {
            if !is_credit_or_refund {
                let mut value = Map::new();
                value.insert("kind".into(), json!("stated_amount_paid"));
                value.insert("currency".into(), json!(cur));
                value.insert("amount_text".into(), json!(amt));
                value.insert("payment_verified".into(), json!(false));
                value.insert("rules_version".into(), json!(rules_version));
                let excerpt = if used_next {
                    format!("{} {}", text, lines[i + 1].text.trim())
                } else {
                    text.to_string()
                };
                let idx = candidates.len();
                explicit_paid.push((idx, cur.to_string(), amt));
                candidates.push(RealCandidate {
                    kind: "stated_amount_paid".into(),
                    usable: true,
                    excerpt,
                    value,
                    locator: line.locator.clone(),
                });
                if used_next {
                    skip_next = true;
                }
                continue;
            }
        }

        // 2. Service / billing period
        if let Some((start, end)) = match_period_line(text) {
            let mut value = Map::new();
            value.insert("kind".into(), json!("stated_service_period"));
            value.insert("period_start".into(), json!(start));
            value.insert("period_end".into(), json!(end));
            value.insert("rules_version".into(), json!(rules_version));
            let idx = candidates.len();
            periods.push((idx, start, end));
            candidates.push(RealCandidate {
                kind: "stated_service_period".into(),
                usable: true,
                excerpt: text.to_string(),
                value,
                locator: line.locator.clone(),
            });
            continue;
        }

        // 3. Generic Total line (honestly classified!)
        if let Some((cur, amt, used_next)) = match_total_amount(lines, i) {
            let excerpt = if used_next {
                format!("{} {}", text, lines[i + 1].text.trim())
            } else {
                text.to_string()
            };
            if is_credit_or_refund {
                let mut value = Map::new();
                value.insert("kind".into(), json!("credit_note_total"));
                value.insert("currency".into(), json!(cur));
                value.insert("amount_text".into(), json!(amt));
                value.insert("payment_verified".into(), json!(false));
                candidates.push(RealCandidate {
                    kind: "credit_note_total".into(),
                    usable: false,
                    excerpt,
                    value,
                    locator: line.locator.clone(),
                });
            } else if is_unpaid_order {
                let mut value = Map::new();
                value.insert("kind".into(), json!("unjudged_order_total"));
                value.insert("currency".into(), json!(cur));
                value.insert("amount_text".into(), json!(amt));
                value.insert("payment_verified".into(), json!(false));
                candidates.push(RealCandidate {
                    kind: "unjudged_order_total".into(),
                    usable: false,
                    excerpt,
                    value,
                    locator: line.locator.clone(),
                });
            } else if has_paid_doc_signal {
                let mut value = Map::new();
                value.insert("kind".into(), json!("stated_amount_paid"));
                value.insert("currency".into(), json!(cur));
                value.insert("amount_text".into(), json!(amt));
                value.insert("payment_verified".into(), json!(false));
                value.insert("derived_from".into(), json!("paid_document_total"));
                value.insert("rules_version".into(), json!(rules_version));
                let idx = candidates.len();
                fallback_totals.push((idx, cur.to_string(), amt));
                candidates.push(RealCandidate {
                    kind: "stated_amount_paid".into(),
                    usable: true,
                    excerpt,
                    value,
                    locator: line.locator.clone(),
                });
            } else {
                let mut value = Map::new();
                value.insert("kind".into(), json!("unresolved_total"));
                value.insert("currency".into(), json!(cur));
                value.insert("amount_text".into(), json!(amt));
                value.insert("payment_verified".into(), json!(false));
                candidates.push(RealCandidate {
                    kind: "unresolved_total".into(),
                    usable: false,
                    excerpt,
                    value,
                    locator: line.locator.clone(),
                });
            }
            if used_next {
                skip_next = true;
            }
            continue;
        }

        // 4. Account email candidate
        if let Some(email) = match_email_line(text) {
            let masked = email.contains('*');
            let kind = if masked {
                "masked_account_email"
            } else {
                "account_email"
            };
            let mut value = Map::new();
            value.insert("kind".into(), json!(kind));
            value.insert("email".into(), json!(email));
            value.insert("masked".into(), json!(masked));
            value.insert("attributed".into(), json!(false));
            candidates.push(RealCandidate {
                kind: kind.into(),
                usable: false,
                excerpt: text.to_string(),
                value,
                locator: line.locator.clone(),
            });
        }
    }

    // Resolve amount: prefer explicit paid labels over fallback invoice totals
    let active_amounts = if !explicit_paid.is_empty() {
        for (idx, _, _) in &fallback_totals {
            candidates[*idx].usable = false;
            candidates[*idx]
                .value
                .insert("reason".into(), json!("superseded_by_explicit_paid"));
        }
        &explicit_paid
    } else {
        &fallback_totals
    };

    let mut has_conflict = false;
    let distinct_amounts: BTreeSet<(&str, &str)> = active_amounts
        .iter()
        .map(|(_, c, a)| (c.as_str(), a.as_str()))
        .collect();
    let adopt_amount_idx = if distinct_amounts.len() == 1 {
        let first = active_amounts[0].0;
        for (idx, _, _) in active_amounts.iter().skip(1) {
            candidates[*idx].usable = false;
            candidates[*idx]
                .value
                .insert("reason".into(), json!("duplicate_statement"));
        }
        Some(first)
    } else if distinct_amounts.len() > 1 {
        has_conflict = true;
        for (idx, _, _) in active_amounts {
            candidates[*idx].usable = false;
            candidates[*idx]
                .value
                .insert("reason".into(), json!("conflicting_amounts"));
        }
        None
    } else {
        None
    };

    // Resolve period
    let distinct_periods: BTreeSet<(&str, &str)> = periods
        .iter()
        .map(|(_, s, e)| (s.as_str(), e.as_str()))
        .collect();
    let adopt_period_idx = if distinct_periods.len() == 1 {
        let first = periods[0].0;
        for (idx, _, _) in periods.iter().skip(1) {
            candidates[*idx].usable = false;
            candidates[*idx]
                .value
                .insert("reason".into(), json!("duplicate_statement"));
        }
        Some(first)
    } else if distinct_periods.len() > 1 {
        has_conflict = true;
        for (idx, _, _) in &periods {
            candidates[*idx].usable = false;
            candidates[*idx]
                .value
                .insert("reason".into(), json!("conflicting_periods"));
        }
        None
    } else {
        None
    };

    let period_pair = adopt_period_idx.and_then(|idx| {
        let v = &candidates[idx].value;
        Some((
            v.get("period_start")?.as_str()?.to_string(),
            v.get("period_end")?.as_str()?.to_string(),
        ))
    });
    let hints = extract_hints(lines, period_pair.as_ref());

    let (status, reason) = if has_conflict {
        ("conflict", Some("conflicting_candidates"))
    } else if adopt_amount_idx.is_some() || adopt_period_idx.is_some() {
        ("matched", None)
    } else if !candidates.is_empty() {
        ("no_match", Some("no_adoptable_statement"))
    } else {
        ("no_match", Some("no_matching_lines"))
    };

    RealBillExtraction {
        status,
        reason,
        page_count,
        candidates,
        adopt_amount_idx,
        adopt_period_idx,
        hints,
    }
}

fn match_explicit_paid_amount(
    lines: &[RealLine],
    idx: usize,
) -> Option<(&'static str, String, bool)> {
    const EN_LABELS: &[&str] = &[
        "amount paid",
        "total paid",
        "payment amount",
        "amount charged",
        "total charged",
        "balance paid",
        "payment received",
        "billed amount",
        "total billed",
        "you paid",
        "charged",
        "paid",
    ];
    const ZH_LABELS: &[&str] = &[
        "价税合计（小写）",
        "价税合计(小写)",
        "价税合计",
        "实际支付",
        "实付金额",
        "实付款",
        "支付金额",
        "已付金额",
        "扣款金额",
        "已扣款",
        "付款金额",
        "合计金额",
        "总计金额",
        "小写",
    ];

    let line = lines[idx].text.trim();
    let lower = line.to_ascii_lowercase();

    // Avoid false positives like "Paid on 2026-10-01" or "Unpaid" or "Amount unpaid"
    if lower.contains("unpaid") || lower.contains("not paid") {
        return None;
    }

    for &label in EN_LABELS {
        if let Some(rest) = strip_label_ascii(line, label) {
            if let Some((cur, amt)) = find_money_in_str(rest) {
                return Some((cur, amt, false));
            }
            if rest.is_empty() && idx + 1 < lines.len() {
                if let Some((cur, amt)) = find_money_in_str(lines[idx + 1].text.trim()) {
                    return Some((cur, amt, true));
                }
            }
        }
    }

    for &label in ZH_LABELS {
        if let Some(rest) = strip_label_zh(line, label) {
            if let Some((cur, amt)) = find_money_in_str_with_default(rest, Some("CNY")) {
                return Some((cur, amt, false));
            }
            if rest.is_empty() && idx + 1 < lines.len() {
                if let Some((cur, amt)) =
                    find_money_in_str_with_default(lines[idx + 1].text.trim(), Some("CNY"))
                {
                    return Some((cur, amt, true));
                }
            }
        }
    }

    // Inline Chinese invoice pattern: `（小写）¥98.00` or `(小写)￥128.00` anywhere in line
    for marker in ["（小写）", "(小写)", "实付", "支付金额", "价税合计"] {
        if let Some(pos) = line.find(marker) {
            let tail = &line[pos + marker.len()..];
            if let Some((cur, amt)) = find_money_in_str(tail) {
                return Some((cur, amt, false));
            }
        }
    }

    None
}

fn match_total_amount(lines: &[RealLine], idx: usize) -> Option<(&'static str, String, bool)> {
    const EN_TOTALS: &[&str] = &["grand total", "invoice total", "order total", "total"];
    const ZH_TOTALS: &[&str] = &["总计", "合计", "总额"];

    let line = lines[idx].text.trim();
    let lower = line.to_ascii_lowercase();
    if lower.starts_with("subtotal") || lower.starts_with("sub-total") || line.starts_with("小计")
    {
        return None;
    }

    for &label in EN_TOTALS {
        if let Some(rest) = strip_label_ascii(line, label) {
            if let Some((cur, amt)) = find_money_in_str(rest) {
                return Some((cur, amt, false));
            }
            if rest.is_empty() && idx + 1 < lines.len() {
                if let Some((cur, amt)) = find_money_in_str(lines[idx + 1].text.trim()) {
                    return Some((cur, amt, true));
                }
            }
        }
    }

    for &label in ZH_TOTALS {
        if let Some(rest) = strip_label_zh(line, label) {
            if let Some((cur, amt)) = find_money_in_str_with_default(rest, Some("CNY")) {
                return Some((cur, amt, false));
            }
            if rest.is_empty() && idx + 1 < lines.len() {
                if let Some((cur, amt)) =
                    find_money_in_str_with_default(lines[idx + 1].text.trim(), Some("CNY"))
                {
                    return Some((cur, amt, true));
                }
            }
        }
    }

    None
}

fn strip_label_ascii<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    if line.len() < label.len() {
        return None;
    }
    if !line.is_char_boundary(label.len()) {
        return None;
    }
    if !line[..label.len()].eq_ignore_ascii_case(label) {
        return None;
    }
    let rest = &line[label.len()..];
    if rest.is_empty() {
        return Some("");
    }
    let first = rest.chars().next()?;
    if matches!(first, ':' | '：' | ' ' | '\t' | '-' | '–') {
        Some(rest.trim_start_matches([':', '：', ' ', '\t', '-', '–']))
    } else {
        None
    }
}

fn strip_label_zh<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(label)?;
    Some(rest.trim_start_matches([':', '：', ' ', '\t', '（', '(', '）', ')']))
}

pub fn find_money_in_str(s: &str) -> Option<(&'static str, String)> {
    find_money_in_str_with_default(s, None)
}

fn find_money_in_str_with_default(
    s: &str,
    default_currency: Option<&'static str>,
) -> Option<(&'static str, String)> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if !s.is_char_boundary(i) {
            i += 1;
            continue;
        }
        if let Some((cur, amt)) = try_parse_money_at(s, i) {
            return Some((cur, amt));
        }
        i += s[i..].chars().next().map_or(1, char::len_utf8);
    }
    if let Some(cur) = default_currency {
        // Fallback: bare decimal number like `98.00` right after a Chinese label
        let trimmed = s.trim().trim_end_matches(['元', '圆', ' ']);
        if trimmed.contains('.') {
            if let Some(amt) = normalize_amount_text(trimmed) {
                return Some((cur, amt));
            }
        }
    }
    None
}

fn try_parse_money_at(s: &str, start: usize) -> Option<(&'static str, String)> {
    let tail = &s[start..];

    // 1. Multi-char & single-char currency symbols (ordered longest first!)
    const SYMBOLS: &[(&str, &str)] = &[
        ("US$", "USD"),
        ("HK$", "HKD"),
        ("CA$", "CAD"),
        ("AU$", "AUD"),
        ("NT$", "TWD"),
        ("JP¥", "JPY"),
        ("S$", "SGD"),
        ("$", "USD"),
        ("€", "EUR"),
        ("£", "GBP"),
        ("¥", "CNY"),
        ("￥", "CNY"),
        ("₩", "KRW"),
    ];
    for &(sym, code) in SYMBOLS {
        if let Some(after) = tail.strip_prefix(sym) {
            let after_ws = after.trim_start();
            if let Some((num_raw, rest)) = take_number_prefix(after_ws) {
                if let Some(amt) = normalize_amount_text(num_raw) {
                    // Override code if explicit trailing ISO code exists, e.g., `$20.00 CAD`
                    let final_cur = leading_iso_currency(rest.trim_start()).unwrap_or(code);
                    return Some((final_cur, amt));
                }
            }
        }
    }

    // 2. Prefix ISO currency code (`USD 19.99`, `CNY 98.00`, `RMB 98.00`)
    let prev_ok = start == 0
        || s[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric());
    if prev_ok {
        if let Some((code, after_code)) = strip_leading_iso_currency(tail) {
            let after_ws =
                after_code.trim_start_matches([' ', ':', '：', '$', '¥', '￥', '€', '£']);
            if let Some((num_raw, _)) = take_number_prefix(after_ws) {
                if let Some(amt) = normalize_amount_text(num_raw) {
                    return Some((code, amt));
                }
            }
        }
    }

    // 3. Number + suffix currency (`19.99 USD`, `98.00元`, `98.00 元`, `1200円`)
    if prev_ok {
        if let Some((num_raw, after_num)) = take_number_prefix(tail) {
            let after_ws = after_num.trim_start();
            if let Some((code, _)) = strip_leading_iso_currency(after_ws) {
                if let Some(amt) = normalize_amount_text(num_raw) {
                    return Some((code, amt));
                }
            }
            if after_ws.starts_with('元') || after_ws.starts_with('圆') {
                if let Some(amt) = normalize_amount_text(num_raw) {
                    return Some(("CNY", amt));
                }
            }
            if after_ws.starts_with('円') {
                if let Some(amt) = normalize_amount_text(num_raw) {
                    return Some(("JPY", amt));
                }
            }
        }
    }

    None
}

fn leading_iso_currency(s: &str) -> Option<&'static str> {
    strip_leading_iso_currency(s).map(|(c, _)| c)
}

fn strip_leading_iso_currency(s: &str) -> Option<(&'static str, &str)> {
    const CODES: &[(&str, &str)] = &[
        ("USD", "USD"),
        ("CNY", "CNY"),
        ("RMB", "CNY"),
        ("EUR", "EUR"),
        ("GBP", "GBP"),
        ("JPY", "JPY"),
        ("HKD", "HKD"),
        ("CAD", "CAD"),
        ("AUD", "AUD"),
        ("SGD", "SGD"),
        ("TWD", "TWD"),
        ("KRW", "KRW"),
        ("CHF", "CHF"),
    ];
    for &(token, code) in CODES {
        if s.len() >= token.len()
            && s.is_char_boundary(token.len())
            && s[..token.len()].eq_ignore_ascii_case(token)
        {
            let rest = &s[token.len()..];
            let boundary = rest
                .chars()
                .next()
                .is_none_or(|c| !c.is_ascii_alphanumeric());
            if boundary {
                return Some((code, rest));
            }
        }
    }
    None
}

fn take_number_prefix(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_digit() {
        return None;
    }
    let mut end = 0usize;
    while end < bytes.len()
        && (bytes[end].is_ascii_digit() || bytes[end] == b',' || bytes[end] == b'.')
    {
        end += 1;
    }
    while end > 0 && (bytes[end - 1] == b',' || bytes[end - 1] == b'.') {
        end -= 1;
    }
    if end == 0 {
        return None;
    }
    // Reject dates like `2026-10-01` or `2026/10/01`
    if end < bytes.len() && matches!(bytes[end], b'-' | b'/' | b':') {
        return None;
    }
    Some((&s[..end], &s[end..]))
}

// --- Bilingual Date & Period Parsing ---

pub fn match_period_line(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim();
    // Strip optional prefix label
    let body = strip_period_prefix(trimmed).unwrap_or(trimmed);

    // Split on range connectors
    for sep in [
        " through ",
        " to ",
        " - ",
        " – ",
        " — ",
        " ~ ",
        " 至 ",
        "至",
        "–",
        "—",
        "~",
    ] {
        if let Some((left, right)) = body.split_once(sep) {
            if let Some(end) = parse_date_token(right.trim()) {
                if let Some(start) = parse_date_token(left.trim()) {
                    if start <= end {
                        return Some((start, end));
                    }
                } else if let Some(year) = end.get(0..4).and_then(|y| y.parse::<u32>().ok()) {
                    // Handle `Jan 15 - Feb 15, 2026` where start omits year
                    if let Some(start) = parse_month_day_with_year(left.trim(), year) {
                        if start <= end {
                            return Some((start, end));
                        }
                    }
                }
            }
        }
    }
    // Tight hyphen between ISO/slash dates `2026-01-01-2026-01-31` or `2026/01/01-2026/01/31`
    if body.len() >= 21
        && body.is_char_boundary(10)
        && body.is_char_boundary(11)
        && body.as_bytes()[10] == b'-'
    {
        if let (Some(s), Some(e)) = (parse_date_token(&body[..10]), parse_date_token(&body[11..])) {
            if s <= e {
                return Some((s, e));
            }
        }
    }
    None
}

fn strip_period_prefix(line: &str) -> Option<&str> {
    const EN: &[&str] = &[
        "service period",
        "billing period",
        "subscription period",
        "coverage period",
        "period",
        "term",
    ];
    const ZH: &[&str] = &["服务周期", "账单周期", "计费周期", "有效期", "订阅周期"];
    for &label in EN {
        if let Some(rest) = strip_label_ascii(line, label) {
            return Some(rest);
        }
    }
    for &label in ZH {
        if let Some(rest) = strip_label_zh(line, label) {
            return Some(rest);
        }
    }
    None
}

pub fn parse_date_token(raw: &str) -> Option<String> {
    let s = raw
        .trim()
        .trim_matches(|c: char| matches!(c, '(' | ')' | '[' | ']' | '.' | ',' | ';'));
    if s.is_empty() {
        return None;
    }

    // 1. ISO `YYYY-MM-DD` or `YYYY/MM/DD` (with optional trailing timestamp)
    let bytes = s.as_bytes();
    if bytes.len() >= 10
        && bytes[0..4].iter().all(|b| b.is_ascii_digit())
        && (bytes[4] == b'-' || bytes[4] == b'/')
        && bytes[5..7].iter().all(|b| b.is_ascii_digit())
        && bytes[7] == bytes[4]
        && bytes[8..10].iter().all(|b| b.is_ascii_digit())
    {
        let y: u32 = s[0..4].parse().ok()?;
        let m: u32 = s[5..7].parse().ok()?;
        let d: u32 = s[8..10].parse().ok()?;
        return format_ymd(y, m, d);
    }

    // 2. Chinese `YYYY年M月D日`
    if let Some((y_str, rest)) = s.split_once('年') {
        if let Some((m_str, rest2)) = rest.split_once('月') {
            let d_str = rest2.split('日').next().unwrap_or(rest2).trim();
            if let (Ok(y), Ok(m), Ok(d)) = (
                y_str.trim().parse::<u32>(),
                m_str.trim().parse::<u32>(),
                d_str.parse::<u32>(),
            ) {
                return format_ymd(y, m, d);
            }
        }
    }

    // 3. `YYYY/M/D` or `YYYY-M-D` with single-digit month/day
    let parts_dash: Vec<&str> = s.split(['-', '/']).collect();
    if parts_dash.len() == 3 && parts_dash[0].len() == 4 {
        if let (Ok(y), Ok(m), Ok(d)) = (
            parts_dash[0].parse::<u32>(),
            parts_dash[1].parse::<u32>(),
            parts_dash[2].split_whitespace().next()?.parse::<u32>(),
        ) {
            return format_ymd(y, m, d);
        }
    }

    // 4. English `Oct 1, 2026` / `October 1, 2026` / `1 Oct 2026`
    let tokens: Vec<&str> = s
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.len() >= 3 {
        // Month Day Year
        if let Some(m) = parse_english_month(tokens[0]) {
            if let (Ok(d), Ok(y)) = (
                tokens[1]
                    .trim_end_matches(['s', 't', 'n', 'd', 'r', 'h'])
                    .parse::<u32>(),
                tokens[2].parse::<u32>(),
            ) {
                return format_ymd(y, m, d);
            }
        }
        // Day Month Year
        if let Some(m) = parse_english_month(tokens[1]) {
            if let (Ok(d), Ok(y)) = (
                tokens[0]
                    .trim_end_matches(['s', 't', 'n', 'd', 'r', 'h'])
                    .parse::<u32>(),
                tokens[2].parse::<u32>(),
            ) {
                return format_ymd(y, m, d);
            }
        }
    }

    None
}

fn parse_month_day_with_year(raw: &str, year: u32) -> Option<String> {
    let tokens: Vec<&str> = raw
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.len() == 2 {
        if let Some(m) = parse_english_month(tokens[0]) {
            if let Ok(d) = tokens[1].parse::<u32>() {
                return format_ymd(year, m, d);
            }
        }
        if let Some(m) = parse_english_month(tokens[1]) {
            if let Ok(d) = tokens[0].parse::<u32>() {
                return format_ymd(year, m, d);
            }
        }
    }
    None
}

fn parse_english_month(tok: &str) -> Option<u32> {
    match tok.trim_end_matches('.').to_ascii_lowercase().as_str() {
        "jan" | "january" => Some(1),
        "feb" | "february" => Some(2),
        "mar" | "march" => Some(3),
        "apr" | "april" => Some(4),
        "may" => Some(5),
        "jun" | "june" => Some(6),
        "jul" | "july" => Some(7),
        "aug" | "august" => Some(8),
        "sep" | "sept" | "september" => Some(9),
        "oct" | "october" => Some(10),
        "nov" | "november" => Some(11),
        "dec" | "december" => Some(12),
        _ => None,
    }
}

fn format_ymd(y: u32, m: u32, d: u32) -> Option<String> {
    if !(1990..=2100).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

fn match_email_line(text: &str) -> Option<String> {
    const EN: &[&str] = &[
        "account email",
        "buyer email",
        "customer email",
        "billed to",
        "bill to",
        "email",
    ];
    const ZH: &[&str] = &["账号邮箱", "绑定邮箱", "邮箱", "购买账号", "账号"];
    for &label in EN {
        if let Some(rest) = strip_label_ascii(text, label) {
            if let Some(email) = first_email_token(rest) {
                return Some(email);
            }
        }
    }
    for &label in ZH {
        if let Some(rest) = strip_label_zh(text, label) {
            if let Some(email) = first_email_token(rest) {
                return Some(email);
            }
        }
    }
    None
}

fn first_email_token(s: &str) -> Option<String> {
    for tok in s.split_whitespace() {
        let cleaned = tok.trim_matches(|c: char| matches!(c, '<' | '>' | '(' | ')' | ',' | ';'));
        if let Some((local, domain)) = cleaned.split_once('@') {
            if !local.is_empty() && domain.contains('.') && !domain.ends_with('.') {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}

// --- Metadata Hint Extraction ---

fn extract_hints(lines: &[RealLine], period: Option<&(String, String)>) -> ExtractedHints {
    let mut hints = ExtractedHints::default();

    const VENDOR_LABELS_EN: &[&str] = &[
        "merchant",
        "vendor",
        "billed by",
        "issued by",
        "seller",
        "from",
        "provider",
        "company",
    ];
    const VENDOR_LABELS_ZH: &[&str] = &[
        "收款方",
        "商户名称",
        "销售方名称",
        "销售方",
        "商家",
        "服务商",
    ];
    const PLAN_LABELS_EN: &[&str] = &["plan", "subscription", "product", "item", "description"];
    const PLAN_LABELS_ZH: &[&str] = &["套餐", "订阅项目", "商品名称", "服务名称", "商品说明"];
    const DATE_LABELS_EN: &[&str] = &[
        "date paid",
        "payment date",
        "invoice date",
        "receipt date",
        "billed on",
        "date",
    ];
    const DATE_LABELS_ZH: &[&str] = &[
        "开票日期",
        "支付时间",
        "交易时间",
        "账单日期",
        "扣款时间",
        "日期",
    ];

    for line in lines {
        let t = line.text.trim();
        if hints.vendor.is_none() {
            for &lbl in VENDOR_LABELS_EN {
                if let Some(rest) = strip_label_ascii(t, lbl) {
                    let v = clean_hint_value(rest);
                    if !v.is_empty() && !v.contains('@') {
                        hints.vendor = Some(v);
                        break;
                    }
                }
            }
        }
        if hints.vendor.is_none() {
            for &lbl in VENDOR_LABELS_ZH {
                if let Some(rest) = strip_label_zh(t, lbl) {
                    let v = clean_hint_value(rest);
                    if !v.is_empty() {
                        hints.vendor = Some(v);
                        break;
                    }
                }
            }
        }
        if hints.plan_name.is_none() {
            for &lbl in PLAN_LABELS_EN {
                if let Some(rest) = strip_label_ascii(t, lbl) {
                    let v = clean_hint_value(rest);
                    if !v.is_empty() {
                        hints.plan_name = Some(v);
                        break;
                    }
                }
            }
        }
        if hints.plan_name.is_none() {
            for &lbl in PLAN_LABELS_ZH {
                if let Some(rest) = strip_label_zh(t, lbl) {
                    let v = clean_hint_value(rest);
                    if !v.is_empty() {
                        hints.plan_name = Some(v);
                        break;
                    }
                }
            }
        }
        if hints.account_label.is_none() {
            if let Some(email) = match_email_line(t) {
                if !email.contains('*') {
                    hints.account_label = Some(email);
                }
            }
        }
        if hints.bill_date.is_none() {
            for &lbl in DATE_LABELS_EN {
                if let Some(rest) = strip_label_ascii(t, lbl) {
                    if let Some(d) = parse_date_token(rest) {
                        hints.bill_date = Some(d);
                        break;
                    }
                }
            }
        }
        if hints.bill_date.is_none() {
            for &lbl in DATE_LABELS_ZH {
                if let Some(rest) = strip_label_zh(t, lbl) {
                    if let Some(d) = parse_date_token(rest) {
                        hints.bill_date = Some(d);
                        break;
                    }
                }
            }
        }
    }

    // Known vendor & category fallback from document text
    let all_text: String = lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let all_lower = all_text.to_ascii_lowercase();

    const KNOWN_VENDORS: &[(&str, &str, &str)] = &[
        ("openai", "OpenAI", "AI 工具"),
        ("chatgpt", "OpenAI", "AI 工具"),
        ("anthropic", "Anthropic", "AI 工具"),
        ("claude", "Anthropic", "AI 工具"),
        ("cursor", "Cursor", "开发工具"),
        ("github", "GitHub", "开发工具"),
        ("jetbrains", "JetBrains", "开发工具"),
        ("vercel", "Vercel", "云服务"),
        ("cloudflare", "Cloudflare", "云服务"),
        ("amazon web services", "AWS", "云服务"),
        ("google cloud", "Google Cloud", "云服务"),
        ("google one", "Google One", "云存储"),
        ("icloud", "Apple iCloud", "云存储"),
        ("apple", "Apple", "数字服务"),
        ("netflix", "Netflix", "影音娱乐"),
        ("spotify", "Spotify", "影音娱乐"),
        ("youtube", "YouTube", "影音娱乐"),
        ("notion", "Notion", "办公效率"),
        ("figma", "Figma", "设计办公"),
        ("adobe", "Adobe", "设计办公"),
        ("microsoft 365", "Microsoft 365", "办公效率"),
        ("dropbox", "Dropbox", "云存储"),
        ("1password", "1Password", "安全工具"),
        ("阿里云", "阿里云", "云服务"),
        ("腾讯云", "腾讯云", "云服务"),
        ("华为云", "华为云", "云服务"),
        ("百度网盘", "百度网盘", "云存储"),
        ("金山办公", "WPS", "办公效率"),
        ("wps", "WPS", "办公效率"),
        ("哔哩哔哩", "哔哩哔哩", "影音娱乐"),
        ("腾讯视频", "腾讯视频", "影音娱乐"),
        ("爱奇艺", "爱奇艺", "影音娱乐"),
        ("网易云音乐", "网易云音乐", "影音娱乐"),
        ("qq音乐", "QQ音乐", "影音娱乐"),
        ("京东", "京东", "购物会员"),
        ("stripe", "Stripe", "软件服务"),
    ];
    for &(needle, display, cat) in KNOWN_VENDORS {
        if all_lower.contains(needle) || all_text.contains(needle) {
            if hints.vendor.is_none() {
                hints.vendor = Some(display.to_string());
            }
            if hints.category.is_none() {
                hints.category = Some(cat.to_string());
            }
            break;
        }
    }

    // Infer billing cycle from explicit keywords or period length
    hints.billing_cycle = infer_billing_cycle(&all_lower, &all_text, period);
    hints
}

fn clean_hint_value(s: &str) -> String {
    let trimmed = s.trim();
    let max_chars = 80;
    let out: String = trimmed.chars().take(max_chars).collect();
    out.trim().to_string()
}

fn infer_billing_cycle(
    lower: &str,
    raw: &str,
    period: Option<&(String, String)>,
) -> Option<String> {
    if let Some((start, end)) = period {
        if let Some(days) = approx_days_between(start, end) {
            if (26..=35).contains(&days) {
                return Some("monthly".into());
            }
            if (84..=96).contains(&days) {
                return Some("quarterly".into());
            }
            if (350..=380).contains(&days) {
                return Some("yearly".into());
            }
            if (6..=8).contains(&days) {
                return Some("weekly".into());
            }
        }
    }
    if lower.contains("annual")
        || lower.contains("yearly")
        || lower.contains("/yr")
        || lower.contains("per year")
        || raw.contains("包年")
        || raw.contains("年卡")
        || raw.contains("年度会员")
    {
        return Some("yearly".into());
    }
    if lower.contains("quarterly") || raw.contains("季卡") || raw.contains("季度") {
        return Some("quarterly".into());
    }
    if lower.contains("monthly")
        || lower.contains("/mo")
        || lower.contains("per month")
        || raw.contains("包月")
        || raw.contains("月卡")
        || raw.contains("月度会员")
    {
        return Some("monthly".into());
    }
    if lower.contains("weekly") || raw.contains("周卡") {
        return Some("weekly".into());
    }
    if lower.contains("one-time")
        || lower.contains("perpetual")
        || raw.contains("一次性")
        || raw.contains("买断")
    {
        return Some("one_time".into());
    }
    None
}

fn approx_days_between(start: &str, end: &str) -> Option<i64> {
    let s = ymd_to_days(start)?;
    let e = ymd_to_days(end)?;
    if e >= s {
        Some(e - s)
    } else {
        None
    }
}

fn ymd_to_days(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let y: i64 = parts[0].parse().ok()?;
    let m: i64 = parts[1].parse().ok()?;
    let d: i64 = parts[2].parse().ok()?;
    // Howard Hinnant civil-from-days formula
    let y_adj = y - if m <= 2 { 1 } else { 0 };
    let era = if y_adj >= 0 { y_adj } else { y_adj - 399 } / 400;
    let yoe = y_adj - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

// --- Real-World CSV Statement Parser (Alipay / WeChat / Bank / Generic) ---

#[derive(Clone, Debug)]
pub struct CsvTransaction {
    pub row_number: usize,
    pub date: String,
    pub merchant: String,
    pub description: String,
    pub currency: String,
    pub amount_text: String,
    pub direction: String, // "expense" | "income" | "refund" | "neutral"
    pub account_hint: Option<String>,
    pub payment_method: Option<String>,
    pub status: Option<String>,
    pub raw_line: String,
}

#[derive(Clone, Debug)]
pub struct CsvExtraction {
    pub status: &'static str, // "matched" | "statement" | "no_match"
    pub reason: Option<&'static str>,
    pub encoding: &'static str,
    pub provider: String, // "alipay" | "wechat" | "generic"
    pub total_rows: usize,
    pub truncated: bool,
    #[cfg_attr(not(test), allow(dead_code))]
    pub transactions: Vec<CsvTransaction>,
    pub candidates: Vec<RealCandidate>,
    pub adopt_amount_idx: Option<usize>,
    pub hints: ExtractedHints,
}

pub fn decode_csv_bytes(raw: &[u8]) -> (String, &'static str) {
    // 1. UTF-8 BOM
    if let Some(stripped) = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        if let Ok(s) = std::str::from_utf8(stripped) {
            return (s.to_string(), "utf-8-bom");
        }
    }
    // 2. Clean UTF-8
    if let Ok(s) = std::str::from_utf8(raw) {
        return (s.to_string(), "utf-8");
    }
    // 3. GB18030 / GBK (Alipay & Chinese bank CSV exports on Windows)
    let (decoded, _, _) = encoding_rs::GB18030.decode(raw);
    (decoded.into_owned(), "gb18030")
}

pub fn parse_csv_statement(raw: &[u8]) -> CsvExtraction {
    let (text, encoding) = decode_csv_bytes(raw);
    let raw_lines: Vec<&str> = text.split('\n').map(|l| l.trim_end_matches('\r')).collect();

    let mut provider = "generic".to_string();
    let mut preamble_account: Option<String> = None;
    for line in raw_lines.iter().take(20) {
        let t = line.trim();
        if t.contains("支付宝") {
            provider = "alipay".into();
        } else if t.contains("微信支付") {
            provider = "wechat".into();
        }
        if preamble_account.is_none() {
            if let Some(rest) = t
                .strip_prefix("账号:[")
                .or_else(|| t.strip_prefix("微信昵称：["))
                .or_else(|| t.strip_prefix("账户名："))
            {
                let cleaned = rest.trim_end_matches([']', ' ', ',']).trim();
                if !cleaned.is_empty() {
                    preamble_account = Some(cleaned.to_string());
                }
            }
        }
    }

    // Find the real CSV header line (skipping Alipay/WeChat preamble headers)
    let mut header_idx: Option<usize> = None;
    let mut headers: Vec<String> = Vec::new();
    for (idx, line) in raw_lines.iter().enumerate().take(40) {
        let cols = parse_csv_row(line);
        if cols.len() >= 3 && looks_like_csv_header(&cols) {
            header_idx = Some(idx);
            headers = cols;
            break;
        }
    }

    let Some(h_idx) = header_idx else {
        return CsvExtraction {
            status: "no_match",
            reason: Some("csv_header_not_found"),
            encoding,
            provider,
            total_rows: 0,
            truncated: false,
            transactions: Vec::new(),
            candidates: Vec::new(),
            adopt_amount_idx: None,
            hints: ExtractedHints::default(),
        };
    };

    let col_map = map_csv_columns(&headers);
    if col_map.amount.is_none() {
        return CsvExtraction {
            status: "no_match",
            reason: Some("csv_amount_column_missing"),
            encoding,
            provider,
            total_rows: 0,
            truncated: false,
            transactions: Vec::new(),
            candidates: Vec::new(),
            adopt_amount_idx: None,
            hints: ExtractedHints::default(),
        };
    }

    let mut transactions = Vec::new();
    let mut total_rows = 0usize;
    let mut truncated = false;

    for (offset, line) in raw_lines.iter().enumerate().skip(h_idx + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("---") || trimmed.starts_with("共") {
            continue;
        }
        let cols = parse_csv_row(line);
        if cols.len() < 2 {
            continue;
        }
        let Some(amt_idx) = col_map.amount else {
            continue;
        };
        let Some(raw_amt) = cols.get(amt_idx) else {
            continue;
        };
        let raw_amt_clean = raw_amt.trim();
        if raw_amt_clean.is_empty() {
            continue;
        }
        let is_negative = raw_amt_clean.starts_with('-');
        let unsigned_amt = raw_amt_clean.trim_start_matches(['-', '+', ' ']);

        let explicit_currency = col_map
            .currency
            .and_then(|i| cols.get(i))
            .and_then(|c| normalize_currency(c));
        let default_cur = if provider == "alipay" || provider == "wechat" {
            Some("CNY")
        } else {
            Some("USD")
        };

        let (currency, amount_text) = if let Some((c, a)) = find_money_in_str(unsigned_amt) {
            (explicit_currency.unwrap_or(c).to_string(), a)
        } else if let Some(a) = normalize_amount_text(unsigned_amt) {
            let cur = explicit_currency.or(default_cur).unwrap_or("USD");
            (cur.to_string(), a)
        } else {
            continue;
        };

        let date_raw = col_map
            .date
            .and_then(|i| cols.get(i))
            .map(|s| s.trim())
            .unwrap_or("");
        let date = parse_date_token(date_raw).unwrap_or_else(|| date_raw.to_string());

        let merchant = col_map
            .merchant
            .and_then(|i| cols.get(i))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let description = col_map
            .description
            .and_then(|i| cols.get(i))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let status = col_map
            .status
            .and_then(|i| cols.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let payment_method = col_map
            .payment_method
            .and_then(|i| cols.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let account_hint = col_map
            .account
            .and_then(|i| cols.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| preamble_account.clone());

        let dir_raw = col_map
            .direction
            .and_then(|i| cols.get(i))
            .map(|s| s.trim())
            .unwrap_or("");
        let direction = classify_csv_direction(dir_raw, status.as_deref(), is_negative);

        total_rows += 1;
        if transactions.len() >= MAX_CSV_CANDIDATES {
            truncated = true;
            continue;
        }
        transactions.push(CsvTransaction {
            row_number: offset + 1,
            date,
            merchant,
            description,
            currency,
            amount_text,
            direction,
            account_hint,
            payment_method,
            status,
            raw_line: line.to_string(),
        });
    }

    let mut candidates = Vec::new();
    let mut adopt_amount_idx = None;

    // Single clear expense CSV (e.g. a 1-row store export) adopts its paid amount directly
    let expenses: Vec<&CsvTransaction> = transactions
        .iter()
        .filter(|t| t.direction == "expense")
        .collect();
    if transactions.len() == 1 && expenses.len() == 1 {
        let tx = &transactions[0];
        let mut value = Map::new();
        value.insert("kind".into(), json!("stated_amount_paid"));
        value.insert("currency".into(), json!(tx.currency));
        value.insert("amount_text".into(), json!(tx.amount_text));
        value.insert("payment_verified".into(), json!(false));
        value.insert("merchant".into(), json!(tx.merchant));
        value.insert("description".into(), json!(tx.description));
        value.insert("date".into(), json!(tx.date));
        value.insert("row_number".into(), json!(tx.row_number));
        value.insert("provider".into(), json!(provider));
        candidates.push(RealCandidate {
            kind: "stated_amount_paid".into(),
            usable: true,
            excerpt: tx.raw_line.clone(),
            value,
            locator: json!({
                "kind": "csv_row",
                "rules_version": REAL_CSV_RULES_VERSION,
                "row_number": tx.row_number,
                "provider": provider,
            }),
        });
        adopt_amount_idx = Some(0);
    } else {
        for tx in &transactions {
            let mut value = Map::new();
            value.insert("kind".into(), json!("csv_transaction"));
            value.insert("row_number".into(), json!(tx.row_number));
            value.insert("date".into(), json!(tx.date));
            value.insert("merchant".into(), json!(tx.merchant));
            value.insert("description".into(), json!(tx.description));
            value.insert("currency".into(), json!(tx.currency));
            value.insert("amount_text".into(), json!(tx.amount_text));
            value.insert("direction".into(), json!(tx.direction));
            value.insert("account_hint".into(), json!(tx.account_hint));
            value.insert("payment_method".into(), json!(tx.payment_method));
            value.insert("status".into(), json!(tx.status));
            value.insert("provider".into(), json!(provider));
            candidates.push(RealCandidate {
                kind: "csv_transaction".into(),
                usable: false,
                excerpt: tx.raw_line.clone(),
                value,
                locator: json!({
                    "kind": "csv_row",
                    "rules_version": REAL_CSV_RULES_VERSION,
                    "row_number": tx.row_number,
                    "provider": provider,
                }),
            });
        }
    }

    let mut hints = ExtractedHints::default();
    if transactions.len() == 1 {
        let tx = &transactions[0];
        if !tx.merchant.is_empty() {
            hints.vendor = Some(tx.merchant.clone());
        }
        if !tx.description.is_empty() {
            hints.plan_name = Some(tx.description.clone());
        }
        if !tx.date.is_empty() {
            hints.bill_date = Some(tx.date.clone());
        }
        hints.account_label = tx.account_hint.clone();
    } else {
        hints.vendor = Some(match provider.as_str() {
            "alipay" => "支付宝账单明细".into(),
            "wechat" => "微信支付账单明细".into(),
            _ => "CSV 账单流水".into(),
        });
        hints.category = Some("流水明细".into());
        hints.account_label = preamble_account;
    }

    let (status, reason) = if adopt_amount_idx.is_some() {
        ("matched", None)
    } else if !transactions.is_empty() {
        ("statement", None)
    } else {
        ("no_match", Some("csv_no_transactions"))
    };

    CsvExtraction {
        status,
        reason,
        encoding,
        provider,
        total_rows,
        truncated,
        transactions,
        candidates,
        adopt_amount_idx,
        hints,
    }
}

#[derive(Default)]
struct CsvColumnMap {
    date: Option<usize>,
    merchant: Option<usize>,
    description: Option<usize>,
    amount: Option<usize>,
    currency: Option<usize>,
    direction: Option<usize>,
    status: Option<usize>,
    payment_method: Option<usize>,
    account: Option<usize>,
}

fn looks_like_csv_header(cols: &[String]) -> bool {
    let m = map_csv_columns(cols);
    let score = usize::from(m.amount.is_some())
        + usize::from(m.date.is_some())
        + usize::from(m.merchant.is_some() || m.description.is_some())
        + usize::from(m.direction.is_some() || m.status.is_some());
    m.amount.is_some() && score >= 2
}

fn map_csv_columns(headers: &[String]) -> CsvColumnMap {
    let mut map = CsvColumnMap::default();
    let mut normalized: BTreeMap<usize, String> = BTreeMap::new();
    for (idx, h) in headers.iter().enumerate() {
        normalized.insert(idx, h.trim().trim_matches('\u{feff}').to_ascii_lowercase());
    }
    for (idx, h) in &normalized {
        let raw = headers[*idx].trim();
        if map.date.is_none()
            && (raw.contains("交易时间")
                || raw.contains("付款时间")
                || raw.contains("创建时间")
                || raw.contains("记账日期")
                || raw.contains("日期")
                || h == "date"
                || h.contains("transaction date")
                || h.contains("billing date")
                || h.contains("created"))
        {
            map.date = Some(*idx);
        }
        if map.merchant.is_none()
            && (raw.contains("交易对方")
                || raw.contains("商户名称")
                || raw.contains("商家")
                || raw.contains("收款方")
                || h == "merchant"
                || h == "vendor"
                || h == "payee"
                || h == "seller"
                || h == "service")
        {
            map.merchant = Some(*idx);
        }
        if map.description.is_none()
            && (raw.contains("商品说明")
                || raw.contains("商品名称")
                || raw == "商品"
                || raw.contains("交易类型")
                || raw.contains("摘要")
                || h == "description"
                || h == "item"
                || h == "plan"
                || h == "memo"
                || h == "product")
        {
            map.description = Some(*idx);
        }
        if map.amount.is_none()
            && (raw.contains("金额")
                || h == "amount"
                || h == "amount_paid"
                || h == "total"
                || h == "price"
                || h == "charge"
                || h == "debit")
        {
            map.amount = Some(*idx);
        }
        if map.currency.is_none() && (raw.contains("币种") || h == "currency") {
            map.currency = Some(*idx);
        }
        if map.direction.is_none()
            && (raw == "收/支"
                || raw.contains("收支")
                || h == "direction"
                || h == "type"
                || h == "flow")
        {
            map.direction = Some(*idx);
        }
        if map.status.is_none()
            && (raw.contains("交易状态")
                || raw.contains("当前状态")
                || raw == "状态"
                || h == "status"
                || h == "state")
        {
            map.status = Some(*idx);
        }
        if map.payment_method.is_none()
            && (raw.contains("支付方式")
                || raw.contains("收/付款方式")
                || h.contains("payment")
                || h == "card")
        {
            map.payment_method = Some(*idx);
        }
        if map.account.is_none()
            && (raw.contains("账号")
                || raw.contains("邮箱")
                || h == "account"
                || h == "email"
                || h == "account_email")
        {
            map.account = Some(*idx);
        }
    }
    map
}

fn classify_csv_direction(dir_raw: &str, status: Option<&str>, is_negative: bool) -> String {
    let dir_low = dir_raw.to_ascii_lowercase();
    let status_raw = status.unwrap_or("");
    let status_low = status_raw.to_ascii_lowercase();

    if status_raw.contains("退款")
        || status_low.contains("refund")
        || status_raw.contains("交易关闭")
        || status_low.contains("closed")
        || status_low.contains("failed")
    {
        return "refund".into();
    }
    if dir_raw.contains("支出") || dir_low == "expense" || dir_low == "debit" || dir_low == "out"
    {
        return "expense".into();
    }
    if dir_raw.contains("收入") || dir_low == "income" || dir_low == "credit" || dir_low == "in" {
        return "income".into();
    }
    if dir_raw.contains("不计收支") || dir_raw == "/" || dir_low == "transfer" {
        return "neutral".into();
    }
    if is_negative {
        return "expense".into();
    }
    "expense".into()
}

pub fn parse_csv_row(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    current.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(ch);
            }
        } else if ch == '"' && current.trim().is_empty() {
            current.clear();
            in_quotes = true;
        } else if ch == ',' {
            fields.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    fields.push(current.trim().to_string());
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_international_and_chinese_money_tokens() {
        assert_eq!(find_money_in_str("$19.99"), Some(("USD", "19.99".into())));
        assert_eq!(
            find_money_in_str("US$ 1,299.50"),
            Some(("USD", "1299.50".into()))
        );
        assert_eq!(find_money_in_str("¥98.00"), Some(("CNY", "98.00".into())));
        assert_eq!(find_money_in_str("￥128"), Some(("CNY", "128.00".into())));
        assert_eq!(find_money_in_str("68.50元"), Some(("CNY", "68.50".into())));
        assert_eq!(find_money_in_str("€12,99"), Some(("EUR", "12.99".into())));
        assert_eq!(find_money_in_str("GBP 9.99"), Some(("GBP", "9.99".into())));
        assert_eq!(
            find_money_in_str("JPY 1,200"),
            Some(("JPY", "1200.00".into()))
        );
        assert_eq!(
            find_money_in_str("$25.00 CAD"),
            Some(("CAD", "25.00".into()))
        );
    }

    #[test]
    fn parses_bilingual_service_periods() {
        assert_eq!(
            match_period_line("Billing period: Oct 1, 2026 - Nov 1, 2026"),
            Some(("2026-10-01".into(), "2026-11-01".into()))
        );
        assert_eq!(
            match_period_line("服务周期：2026年1月1日 至 2026年12月31日"),
            Some(("2026-01-01".into(), "2026-12-31".into()))
        );
        assert_eq!(
            match_period_line("Jan 15 - Feb 15, 2026"),
            Some(("2026-01-15".into(), "2026-02-15".into()))
        );
    }

    #[test]
    fn extracts_styled_html_email_safely_ignoring_hidden_and_scripts() {
        let html = r#"<!doctype html>
<html>
<head><style>.price { color: green; }</style><script>alert("Amount paid $999.00");</script></head>
<body>
  <div style="display: none">Amount paid $888.00</div>
  <h1>OpenAI Receipt</h1>
  <p>From: OpenAI, LLC</p>
  <p>Account email: alice@example.com</p>
  <p>Date paid: October 5, 2026</p>
  <table style="border:1px solid #ccc">
    <tr><th>Plan</th><td>ChatGPT Plus (Monthly)</td></tr>
    <tr><th>Billing period</th><td>Oct 5, 2026 - Nov 5, 2026</td></tr>
    <tr><th>Amount paid</th><td class="price">$20.00</td></tr>
  </table>
</body>
</html>"#;
        let res = extract_real_html(html);
        assert_eq!(res.status, "matched");
        let amt = &res.candidates[res.adopt_amount_idx.unwrap()];
        assert_eq!(amt.value["currency"], "USD");
        assert_eq!(amt.value["amount_text"], "20.00");
        let per = &res.candidates[res.adopt_period_idx.unwrap()];
        assert_eq!(per.value["period_start"], "2026-10-05");
        assert_eq!(per.value["period_end"], "2026-11-05");
        assert_eq!(res.hints.vendor.as_deref(), Some("OpenAI, LLC"));
        assert_eq!(
            res.hints.account_label.as_deref(),
            Some("alice@example.com")
        );
        assert_eq!(res.hints.billing_cycle.as_deref(), Some("monthly"));
        assert_eq!(res.hints.bill_date.as_deref(), Some("2026-10-05"));
    }

    #[test]
    fn parses_alipay_gbk_and_wechat_utf8_csv_statements() {
        let alipay_utf8 = "支付宝交易记录明细查询\n账号:[alice@example.com]\n------------------------------------\n交易时间,交易分类,交易对方,商品说明,收/支,金额,收/付款方式,交易状态\n2026-10-01 09:00:00,数字娱乐,Apple,iCloud+ 2TB包月,支出,68.00,招商银行,交易成功\n2026-10-03 12:30:00,办公效率,金山办公,WPS超级会员年卡,支出,148.00,余额宝,交易成功\n2026-10-05 18:00:00,退款,某商家,退款测试,收入,15.00,余额,退款成功\n";
        let (gbk_bytes, _, _) = encoding_rs::GB18030.encode(alipay_utf8);
        let res = parse_csv_statement(&gbk_bytes);
        assert_eq!(res.encoding, "gb18030");
        assert_eq!(res.provider, "alipay");
        assert_eq!(res.status, "statement");
        assert_eq!(res.transactions.len(), 3);
        assert_eq!(res.transactions[0].merchant, "Apple");
        assert_eq!(res.transactions[0].currency, "CNY");
        assert_eq!(res.transactions[0].amount_text, "68.00");
        assert_eq!(res.transactions[0].direction, "expense");
        assert_eq!(res.transactions[2].direction, "refund");
    }

    #[test]
    fn refuses_credit_note_and_conflicting_amounts() {
        let credit = r#"<html><body><p>Document: Credit Note</p><p>Total $20.00</p></body></html>"#;
        let res = extract_real_html(credit);
        assert!(res.adopt_amount_idx.is_none());

        let conflict =
            r#"<html><body><p>Amount paid: $20.00</p><p>Amount paid: $35.00</p></body></html>"#;
        let res2 = extract_real_html(conflict);
        assert_eq!(res2.status, "conflict");
        assert!(res2.adopt_amount_idx.is_none());
    }
}
