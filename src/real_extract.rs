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
//! - Supports UTF-8 (with/without BOM), UTF-16LE/BE BOM, and GB18030/GBK CSVs (Alipay / WeChat / Bank / Store).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

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
    // Determine decimal separator (compare last `.` vs last `,` so `1.299,50` works cleanly)
    let dot_pos = cleaned.rfind('.');
    let comma_pos = cleaned.rfind(',');
    let normalized = match (dot_pos, comma_pos) {
        (Some(d), c) if c.is_none_or(|cp| d > cp) => {
            let frac = &cleaned[d + 1..];
            let whole_raw = &cleaned[..d];
            if !frac.bytes().all(|b| b.is_ascii_digit()) || frac.is_empty() || frac.len() > 2 {
                return None;
            }
            let whole = strip_grouping_sep(whole_raw, ',')?;
            let frac_padded = if frac.len() == 1 {
                format!("{frac}0")
            } else {
                frac.to_string()
            };
            format!("{whole}.{frac_padded}")
        }
        (d, Some(c)) if d.is_none_or(|dp| c > dp) => {
            let after = &cleaned[c + 1..];
            let before = &cleaned[..c];
            if after.len() == 2
                && after.bytes().all(|b| b.is_ascii_digit())
                && !before.contains(',')
                && !before.is_empty()
            {
                // European decimal comma like `12,99` or `1.299,50` with strict dot grouping
                let whole = strip_grouping_sep(before, '.')?;
                format!("{whole}.{after}")
            } else {
                // Thousands separator comma like `1,200`
                let whole = strip_grouping_sep(&cleaned, ',')?;
                format!("{whole}.00")
            }
        }
        _ => {
            if !cleaned.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let whole = trim_leading_zeros(&cleaned);
            format!("{whole}.00")
        }
    };
    // Sanity bound: reject absurdly long digit sequences (e.g. order IDs mistaken for money)
    let (whole, frac) = normalized.split_once('.')?;
    if whole.len() > 10 || frac.len() != 2 {
        return None;
    }
    Some(normalized)
}

fn strip_grouping_sep(raw: &str, sep: char) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    if !raw.contains(sep) {
        if !raw.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        return Some(trim_leading_zeros(raw));
    }
    let parts: Vec<&str> = raw.split(sep).collect();
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

pub fn extract_real_pdf_from_pages(pages: Vec<(usize, Vec<String>)>) -> RealBillExtraction {
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

const MAX_HTML_DEPTH: usize = 128;

pub fn html_to_lines(html: &str) -> Vec<RealLine> {
    let Ok(dom) = tl::parse(html, tl::ParserOptions::default()) else {
        return Vec::new();
    };
    let parser = dom.parser();
    let mut raw_lines: Vec<String> = Vec::new();
    collect_html_lines(dom.children(), parser, &mut raw_lines, 0);
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

fn flush_inline_buf(buf: &mut String, out: &mut Vec<String>) {
    let folded = collapse_ws(buf);
    buf.clear();
    if !folded.is_empty() {
        out.push(folded);
    }
}

fn is_skipped_html_tag(name: &str) -> bool {
    matches!(
        name,
        "script"
            | "style"
            | "noscript"
            | "template"
            | "svg"
            | "head"
            | "meta"
            | "link"
            | "del"
            | "s"
    )
}

fn is_block_html_tag(name: &str) -> bool {
    matches!(
        name,
        "div"
            | "p"
            | "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "td"
            | "th"
            | "ul"
            | "ol"
            | "li"
            | "dl"
            | "dt"
            | "dd"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "main"
            | "aside"
            | "nav"
            | "blockquote"
            | "pre"
            | "form"
            | "fieldset"
            | "figure"
            | "figcaption"
            | "address"
            | "hr"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
    )
}

fn collect_html_lines(
    handles: &[tl::NodeHandle],
    parser: &tl::Parser,
    out: &mut Vec<String>,
    depth: usize,
) {
    if depth >= MAX_HTML_DEPTH {
        return;
    }
    let mut inline_buf = String::new();
    for handle in handles {
        let Some(node) = handle.get(parser) else {
            continue;
        };
        match node {
            tl::Node::Comment(_) => {}
            tl::Node::Raw(raw) => {
                let raw_text = raw.as_utf8_str();
                let decoded = html_escape::decode_html_entities(&raw_text);
                inline_buf.push_str(&decoded);
            }
            tl::Node::Tag(tag) => {
                let name = tag.name().as_utf8_str().to_ascii_lowercase();
                if is_skipped_html_tag(&name) || is_html_hidden(tag) {
                    continue;
                }
                if name == "br" {
                    flush_inline_buf(&mut inline_buf, out);
                    continue;
                }
                let children: Vec<tl::NodeHandle> = tag.children().top().iter().copied().collect();
                if name == "tr" {
                    flush_inline_buf(&mut inline_buf, out);
                    if let Some(cells) = collect_leaf_tr_cells(tag, parser, depth + 1) {
                        if cells.len() == 2 && cells[0].len() == 1 && cells[1].len() == 1 {
                            let sep = if cells[0][0].ends_with(':') || cells[0][0].ends_with('：')
                            {
                                " "
                            } else {
                                ": "
                            };
                            out.push(format!("{}{sep}{}", cells[0][0], cells[1][0]));
                        } else {
                            for cell in cells {
                                out.extend(cell);
                            }
                        }
                        continue;
                    }
                    collect_html_lines(&children, parser, out, depth + 1);
                    continue;
                }
                if is_block_html_tag(&name) {
                    flush_inline_buf(&mut inline_buf, out);
                    if name == "hr" {
                        continue;
                    }
                    if matches!(
                        name.as_str(),
                        "table" | "thead" | "tbody" | "tfoot" | "ul" | "ol" | "dl"
                    ) || has_nested_block(tag, parser, depth + 1)
                    {
                        collect_html_lines(&children, parser, out, depth + 1);
                    } else {
                        extract_visible_lines(tag, parser, out, depth + 1);
                    }
                    continue;
                }
                // Inline wrapper (or layout tag like <center>/<a>) that wraps block elements
                if has_nested_block(tag, parser, depth + 1) {
                    flush_inline_buf(&mut inline_buf, out);
                    collect_html_lines(&children, parser, out, depth + 1);
                } else {
                    append_inline_lines(&children, parser, &mut inline_buf, out, depth + 1);
                }
            }
        }
    }
    flush_inline_buf(&mut inline_buf, out);
}

fn is_html_hidden(tag: &tl::HTMLTag) -> bool {
    for (attr_name, attr_val) in tag.attributes().iter() {
        if attr_name.eq_ignore_ascii_case("hidden") {
            return true;
        }
        if attr_name.eq_ignore_ascii_case("aria-hidden")
            && attr_val
                .as_deref()
                .is_some_and(|val| val.trim().eq_ignore_ascii_case("true"))
        {
            return true;
        }
        if attr_name.eq_ignore_ascii_case("style")
            && attr_val.as_deref().is_some_and(style_hides_element)
        {
            return true;
        }
    }
    false
}

fn style_hides_element(style: &str) -> bool {
    for decl in style.split(';') {
        let Some((raw_prop, raw_val)) = decl.split_once(':') else {
            continue;
        };
        let prop = raw_prop.trim().to_ascii_lowercase();
        let val_low = raw_val.trim().to_ascii_lowercase();
        let val = val_low
            .strip_suffix("!important")
            .map_or(val_low.as_str(), str::trim);
        match prop.as_str() {
            "display" if val == "none" => return true,
            "visibility" if val == "hidden" => return true,
            "mso-hide" if val == "all" => return true,
            "text-decoration" | "text-decoration-line" => {
                if val.split_whitespace().any(|tok| tok == "line-through") {
                    return true;
                }
            }
            "opacity" => {
                if is_css_exact_zero(val, &["%"]) {
                    return true;
                }
            }
            "max-height" | "font-size"
                if is_css_exact_zero(val, &["rem", "px", "pt", "em", "%"]) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn is_css_exact_zero(val: &str, units: &[&str]) -> bool {
    let mut num = val.trim();
    if num.is_empty() {
        return false;
    }
    for unit in units {
        if let Some(before) = num.strip_suffix(unit) {
            num = before.trim();
            break;
        }
    }
    if num.is_empty() {
        return false;
    }
    let mut seen_digit = false;
    let mut seen_dot = false;
    for ch in num.chars() {
        if ch == '0' {
            seen_digit = true;
        } else if ch == '.' && !seen_dot {
            seen_dot = true;
        } else {
            return false;
        }
    }
    seen_digit
}

fn has_nested_block(tag: &tl::HTMLTag, parser: &tl::Parser, depth: usize) -> bool {
    if depth >= MAX_HTML_DEPTH {
        return false;
    }
    for handle in tag.children().top().iter() {
        if let Some(tl::Node::Tag(child)) = handle.get(parser) {
            let name = child.name().as_utf8_str().to_ascii_lowercase();
            if is_skipped_html_tag(&name) || is_html_hidden(child) {
                continue;
            }
            if is_block_html_tag(&name) {
                return true;
            }
            if has_nested_block(child, parser, depth + 1) {
                return true;
            }
        }
    }
    false
}

fn collect_leaf_tr_cells(
    tr: &tl::HTMLTag,
    parser: &tl::Parser,
    depth: usize,
) -> Option<Vec<Vec<String>>> {
    let mut cells: Vec<Vec<String>> = Vec::new();
    let mut saw_cell = false;
    for handle in tr.children().top().iter() {
        let Some(tl::Node::Tag(child)) = handle.get(parser) else {
            continue;
        };
        let name = child.name().as_utf8_str().to_ascii_lowercase();
        if is_skipped_html_tag(&name) || is_html_hidden(child) {
            continue;
        }
        if name == "td" || name == "th" {
            saw_cell = true;
            if has_nested_block(child, parser, depth + 1) {
                return None;
            }
            let mut lines = Vec::new();
            extract_visible_lines(child, parser, &mut lines, depth + 1);
            if !lines.is_empty() {
                cells.push(lines);
            }
        } else if is_block_html_tag(&name) {
            return None;
        }
    }
    if saw_cell {
        Some(cells)
    } else {
        None
    }
}

fn extract_visible_lines(
    tag: &tl::HTMLTag,
    parser: &tl::Parser,
    out: &mut Vec<String>,
    depth: usize,
) {
    let mut buf = String::new();
    let kids: Vec<tl::NodeHandle> = tag.children().top().iter().copied().collect();
    append_inline_lines(&kids, parser, &mut buf, out, depth);
    flush_inline_buf(&mut buf, out);
}

fn append_inline_lines(
    handles: &[tl::NodeHandle],
    parser: &tl::Parser,
    buf: &mut String,
    out: &mut Vec<String>,
    depth: usize,
) {
    if depth >= MAX_HTML_DEPTH {
        return;
    }
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
                if is_skipped_html_tag(&name) || is_html_hidden(child) {
                    continue;
                }
                if name == "br" {
                    flush_inline_buf(buf, out);
                    continue;
                }
                let kids: Vec<tl::NodeHandle> = child.children().top().iter().copied().collect();
                append_inline_lines(&kids, parser, buf, out, depth + 1);
            }
        }
    }
}

fn is_zero_width_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}' | '\u{00AD}'
    )
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true;
    for ch in s.chars() {
        if is_zero_width_char(ch) {
            continue;
        }
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
        || joined_lower.contains("refund confirmation")
        || joined_lower.contains("amount refunded")
        || joined_lower.contains("total refunded")
        || joined_lower.contains("refund issued")
        || joined_lower.contains("this total is a credit-note total")
        || joined_raw.contains("红字发票")
        || joined_raw.contains("红字")
        || joined_raw.contains("负数发票")
        || joined_raw.contains("销项负数")
        || joined_raw.contains("已退款")
        || joined_raw.contains("退款成功")
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
        || joined_raw.contains("数电票")
        || joined_raw.contains("电子发票")
        || joined_raw.contains("电子普通发票")
        || joined_raw.contains("增值税普通发票")
        || joined_raw.contains("增值税专用发票")
        || joined_raw.contains("普通发票")
        || joined_raw.contains("支付成功")
        || joined_raw.contains("付款成功")
        || joined_raw.contains("交易成功")
        || joined_raw.contains("收据");

    let yen_currency = infer_yen_currency(&joined_lower, &joined_raw);

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
        if let Some((cur, amt, used_next)) = match_explicit_paid_amount(lines, i, yen_currency) {
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
            } else {
                let mut value = Map::new();
                value.insert("kind".into(), json!("stated_amount_paid"));
                value.insert("currency".into(), json!(cur));
                value.insert("amount_text".into(), json!(amt));
                value.insert("payment_verified".into(), json!(false));
                value.insert("rules_version".into(), json!(rules_version));
                let idx = candidates.len();
                explicit_paid.push((idx, cur.to_string(), amt));
                candidates.push(RealCandidate {
                    kind: "stated_amount_paid".into(),
                    usable: true,
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
        if let Some((cur, amt, used_next)) = match_total_amount(lines, i, yen_currency) {
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

fn contains_ascii_word(haystack_lower: &str, word_lower: &str) -> bool {
    let mut start = 0usize;
    while let Some(pos) = haystack_lower[start..].find(word_lower) {
        let abs = start + pos;
        let end = abs + word_lower.len();
        let left_ok = abs == 0
            || haystack_lower[..abs]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphabetic());
        let right_ok = end == haystack_lower.len()
            || haystack_lower[end..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_ascii_alphabetic());
        if left_ok && right_ok {
            return true;
        }
        start = abs + word_lower.len();
    }
    false
}

fn infer_yen_currency(joined_lower: &str, joined_raw: &str) -> &'static str {
    let has_cny_signal = contains_ascii_word(joined_lower, "cny")
        || contains_ascii_word(joined_lower, "rmb")
        || joined_raw.contains("人民币")
        || joined_raw.contains('元');
    if has_cny_signal {
        return "CNY";
    }
    let has_jpy_signal = contains_ascii_word(joined_lower, "jpy")
        || joined_raw.contains('円')
        || joined_raw.contains("日本")
        || contains_ascii_word(joined_lower, "tokyo")
        || joined_raw.contains("消費税");
    if has_jpy_signal {
        "JPY"
    } else {
        "CNY"
    }
}

fn match_explicit_paid_amount(
    lines: &[RealLine],
    idx: usize,
    yen_currency: &'static str,
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
        "（小写）",
        "(小写)",
        "实际支付",
        "实付金额",
        "实付款",
        "支付金额",
        "已付金额",
        "扣款金额",
        "已扣款",
        "付款金额",
        "小写",
    ];

    let line = lines[idx].text.trim();
    let lower = line.to_ascii_lowercase();

    // Avoid false positives like "Paid on 2026-10-01", "Unpaid", or "Amount due: $0.00"
    if lower.contains("unpaid")
        || lower.contains("not paid")
        || lower.contains("amount due")
        || lower.contains("balance due")
        || lower.contains("total due")
    {
        return None;
    }

    for &label in EN_LABELS {
        if let Some(rest) = strip_label_ascii(line, label) {
            if let Some((cur, amt)) = find_money_in_str_with_options(rest, None, yen_currency) {
                return Some((cur, amt, false));
            }
            if rest.is_empty() && idx + 1 < lines.len() {
                if let Some((cur, amt)) =
                    find_money_in_str_with_options(lines[idx + 1].text.trim(), None, yen_currency)
                {
                    return Some((cur, amt, true));
                }
            }
        }
    }

    for &label in ZH_LABELS {
        if let Some(rest) = strip_label_zh(line, label) {
            if let Some((cur, amt)) =
                find_money_in_str_with_options(rest, Some("CNY"), yen_currency)
            {
                return Some((cur, amt, false));
            }
            if rest.is_empty() && idx + 1 < lines.len() {
                if let Some((cur, amt)) = find_money_in_str_with_options(
                    lines[idx + 1].text.trim(),
                    Some("CNY"),
                    yen_currency,
                ) {
                    return Some((cur, amt, true));
                }
            }
        }
    }

    // Inline Chinese invoice pattern: `（小写）¥98.00`, `（小写）98.00`, `(小写)￥128.00` anywhere in line
    for marker in ["（小写）", "(小写)", "实付", "支付金额", "价税合计"] {
        if let Some(pos) = line.find(marker) {
            let tail = &line[pos + marker.len()..];
            if let Some((cur, amt)) =
                find_money_in_str_with_options(tail, Some("CNY"), yen_currency)
            {
                return Some((cur, amt, false));
            }
        }
    }

    None
}

fn match_total_amount(
    lines: &[RealLine],
    idx: usize,
    yen_currency: &'static str,
) -> Option<(&'static str, String, bool)> {
    const EN_TOTALS: &[&str] = &[
        "grand total",
        "invoice total",
        "order total",
        "total amount",
        "total",
    ];
    const ZH_TOTALS: &[&str] = &["合计金额", "总计金额", "总计", "合计", "总额"];
    const EXCLUDE_LOWER: &[&str] = &[
        "subtotal",
        "sub-total",
        "total excluding tax",
        "total excl",
        "excluding tax",
        "excl. tax",
        "excl tax",
        "total tax",
        "total savings",
        "total discount",
        "pre-tax",
        "before tax",
    ];
    const EXCLUDE_ZH: &[&str] = &["小计", "税额", "不含税"];

    let line = lines[idx].text.trim();
    let lower = line.to_ascii_lowercase();
    if EXCLUDE_LOWER.iter().any(|k| lower.contains(k))
        || EXCLUDE_ZH.iter().any(|k| line.contains(k))
    {
        return None;
    }
    let is_due_line = lower.contains("total due")
        || lower.contains("amount due")
        || lower.contains("balance due");

    for &label in EN_TOTALS {
        if let Some(rest) = strip_label_ascii(line, label) {
            if let Some((cur, amt)) = find_money_in_str_with_options(rest, None, yen_currency) {
                if is_due_line && amt == "0.00" {
                    return None;
                }
                return Some((cur, amt, false));
            }
            if rest.is_empty() && idx + 1 < lines.len() {
                if let Some((cur, amt)) =
                    find_money_in_str_with_options(lines[idx + 1].text.trim(), None, yen_currency)
                {
                    if is_due_line && amt == "0.00" {
                        return None;
                    }
                    return Some((cur, amt, true));
                }
            }
        }
    }

    for &label in ZH_TOTALS {
        if let Some(rest) = strip_label_zh(line, label) {
            if let Some((cur, amt)) =
                find_money_in_str_with_options(rest, Some("CNY"), yen_currency)
            {
                return Some((cur, amt, false));
            }
            if rest.is_empty() && idx + 1 < lines.len() {
                if let Some((cur, amt)) = find_money_in_str_with_options(
                    lines[idx + 1].text.trim(),
                    Some("CNY"),
                    yen_currency,
                ) {
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
    find_money_in_str_with_options(s, None, "CNY")
}

fn find_money_in_str_with_options(
    s: &str,
    default_currency: Option<&'static str>,
    yen_currency: &'static str,
) -> Option<(&'static str, String)> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if !s.is_char_boundary(i) {
            i += 1;
            continue;
        }
        if let Some((cur, amt)) = try_parse_money_at(s, i, yen_currency) {
            return Some((cur, amt));
        }
        i += s[i..].chars().next().map_or(1, char::len_utf8);
    }
    if let Some(cur) = default_currency {
        // Fallback: bare decimal number like `98.00` right after a Chinese label
        let trimmed = s
            .trim_start_matches([':', '：', ' ', '\t'])
            .trim_end_matches(['元', '圆', ' ']);
        if let Some((num_raw, _)) = take_number_prefix(trimmed) {
            if num_raw.contains('.') {
                if let Some(amt) = normalize_amount_text(num_raw) {
                    return Some((cur, amt));
                }
            }
        }
    }
    None
}

const CURRENCY_SYMBOLS: &[(&str, &str)] = &[
    ("US$", "USD"),
    ("HK$", "HKD"),
    ("CA$", "CAD"),
    ("AU$", "AUD"),
    ("NT$", "TWD"),
    ("JP¥", "JPY"),
    ("JP￥", "JPY"),
    ("A$", "AUD"),
    ("C$", "CAD"),
    ("S$", "SGD"),
    ("$", "USD"),
    ("€", "EUR"),
    ("£", "GBP"),
    ("¥", "CNY"),
    ("￥", "CNY"),
    ("₩", "KRW"),
];

fn try_parse_money_at(
    s: &str,
    start: usize,
    yen_currency: &'static str,
) -> Option<(&'static str, String)> {
    let tail = &s[start..];
    let prev_char = s[..start].chars().next_back();
    let prev_ok = prev_char.is_none_or(|c| !c.is_ascii_alphanumeric());
    let prev_is_minus = matches!(prev_char, Some('-' | '\u{2212}'));

    // 1. Multi-char & single-char prefix currency symbols (ordered longest first!)
    if !prev_is_minus {
        for &(sym, default_code) in CURRENCY_SYMBOLS {
            if sym.as_bytes()[0].is_ascii_alphabetic() && !prev_ok {
                continue;
            }
            if let Some(after) = tail.strip_prefix(sym) {
                let after_ws = after.trim_start();
                if let Some((num_raw, rest)) = take_number_prefix(after_ws) {
                    if let Some(amt) = normalize_amount_text(num_raw) {
                        let base_code = if sym == "¥" || sym == "￥" {
                            yen_currency
                        } else {
                            default_code
                        };
                        // Override code if explicit trailing ISO code exists, e.g., `$20.00 CAD`
                        let final_cur =
                            leading_iso_currency(rest.trim_start()).unwrap_or(base_code);
                        return Some((final_cur, amt));
                    }
                }
            }
        }
    }

    // 2. Prefix ISO currency code (`USD 19.99`, `CNY 98.00`, `RMB 98.00`)
    if prev_ok && !prev_is_minus {
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

    // 3. Number + suffix currency (`19.99 USD`, `19,99 €`, `20.00 $`, `15.00 £`, `98.00元`, `98.00 元`, `1200円`)
    let num_prev_ok = prev_char.is_none_or(|c| {
        !c.is_ascii_alphanumeric() && !matches!(c, '.' | ',' | '-' | '\u{2212}' | '/')
    });
    if num_prev_ok {
        if let Some((num_raw, after_num)) = take_number_prefix(tail) {
            let had_ws = after_num.starts_with(|c: char| c.is_whitespace() || c == '\u{00a0}');
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
            for &(sym, default_code) in CURRENCY_SYMBOLS {
                if let Some(after_sym) = after_ws.strip_prefix(sym) {
                    // Do not steal a prefix symbol belonging to a following number (e.g. `Qty 1 $20.00`)
                    if after_sym.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                        continue;
                    }
                    if had_ws
                        && !num_raw.contains('.')
                        && !num_raw.contains(',')
                        && after_sym
                            .trim_start()
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_ascii_digit())
                    {
                        continue;
                    }
                    if let Some(amt) = normalize_amount_text(num_raw) {
                        let base_code = if sym == "¥" || sym == "￥" {
                            yen_currency
                        } else {
                            default_code
                        };
                        return Some((base_code, amt));
                    }
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

fn is_billing_suffix(rest: &str) -> bool {
    let Some(after_slash) = rest.strip_prefix('/') else {
        return false;
    };
    for zh in ["年", "月", "季", "周"] {
        if after_slash.starts_with(zh) {
            return true;
        }
    }
    for en in [
        "monthly", "month", "mo", "yearly", "year", "yr", "annually", "weekly", "week", "wk",
    ] {
        if after_slash.len() >= en.len()
            && after_slash.is_char_boundary(en.len())
            && after_slash[..en.len()].eq_ignore_ascii_case(en)
        {
            let next = after_slash[en.len()..].chars().next();
            if next.is_none_or(|c| !c.is_ascii_alphabetic()) {
                return true;
            }
        }
    }
    false
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
    let rest = &s[end..];
    // Reject dates (`2026-10-01`, `2026/10/01`) and times (`09:30`), while allowing
    // billing suffixes (`/mo`, `/month`, `/yr`, `/year`, `/年`, `/月`) and punctuation (`:`, `)`, `）`).
    if rest.starts_with('-') {
        return None;
    }
    if rest.starts_with('/') && !is_billing_suffix(rest) {
        return None;
    }
    if rest.starts_with(':') && rest.as_bytes().get(1).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    Some((&s[..end], rest))
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
                    // Handle `Jan 15 - Feb 15, 2026` and year-crossing `Dec 15 - Jan 15, 2026`
                    if let Some(start) = parse_month_day_with_year(left.trim(), year) {
                        if start <= end {
                            return Some((start, end));
                        }
                    }
                    if let Some(start) =
                        parse_month_day_with_year(left.trim(), year.saturating_sub(1))
                    {
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
        "from",
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

    // 1. ISO `YYYY-MM-DD`, `YYYY/MM/DD`, or `YYYY.MM.DD` (with optional trailing timestamp)
    let bytes = s.as_bytes();
    if bytes.len() >= 10
        && bytes[0..4].iter().all(|b| b.is_ascii_digit())
        && matches!(bytes[4], b'-' | b'/' | b'.')
        && bytes[5..7].iter().all(|b| b.is_ascii_digit())
        && bytes[7] == bytes[4]
        && bytes[8..10].iter().all(|b| b.is_ascii_digit())
        && bytes
            .get(10)
            .is_none_or(|&b| !b.is_ascii_digit() && b != b'.')
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

    // 3. `YYYY-M-D`, `YYYY/M/D`, or `YYYY.M.D` with single-digit month/day
    let head = s.split_whitespace().next().unwrap_or(s);
    for sep in ['-', '/', '.'] {
        let parts: Vec<&str> = head.split(sep).collect();
        if parts.len() == 3
            && parts[0].len() == 4
            && !parts[1].is_empty()
            && parts[1].len() <= 2
            && !parts[2].is_empty()
            && parts[2].len() <= 2
        {
            if let (Ok(y), Ok(m), Ok(d)) = (
                parts[0].parse::<u32>(),
                parts[1].parse::<u32>(),
                parts[2].parse::<u32>(),
            ) {
                return format_ymd(y, m, d);
            }
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
    if !(1990..=2100).contains(&y) || !(1..=12).contains(&m) || d == 0 {
        return None;
    }
    let is_leap = (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400);
    let max_day = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap {
                29
            } else {
                28
            }
        }
        _ => return None,
    };
    if d > max_day {
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
    let mut processor_vendor_fallback: Option<String> = None;
    let mut email_account: Option<String> = None;
    let mut fallback_account: Option<String> = None;

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
        "销方名称",
        "销售方",
        "销方",
        "商家",
        "服务商",
    ];
    const PLAN_LABELS_EN: &[&str] = &["plan", "subscription", "product", "item", "description"];
    const PLAN_LABELS_ZH: &[&str] = &[
        "套餐",
        "订阅项目",
        "货物或应税劳务、服务名称",
        "项目名称",
        "商品名称",
        "服务名称",
        "商品说明",
    ];
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
                    if is_plausible_vendor(&v) {
                        if is_payment_processor_name(&v) {
                            processor_vendor_fallback.get_or_insert(v);
                        } else {
                            hints.vendor = Some(v);
                            break;
                        }
                    }
                }
            }
        }
        if hints.vendor.is_none() {
            for &lbl in VENDOR_LABELS_ZH {
                if let Some(rest) = strip_label_zh(t, lbl) {
                    let v = clean_hint_value(rest);
                    if is_plausible_vendor(&v) {
                        if is_payment_processor_name(&v) {
                            processor_vendor_fallback.get_or_insert(v);
                        } else {
                            hints.vendor = Some(v);
                            break;
                        }
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
        if email_account.is_none() {
            if let Some(email) = match_email_line(t) {
                if !email.contains('*') {
                    email_account = Some(email);
                }
            }
        }
        if fallback_account.is_none() {
            if let Some(lbl) = match_account_label_line(t) {
                fallback_account = Some(lbl);
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
    hints.account_label = email_account.or(fallback_account);

    // Known vendor & category fallback from document text
    let all_text: String = lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let all_lower = all_text.to_ascii_lowercase();
    let all_lower_no_powered = all_lower.replace("powered by stripe", "");

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
        let matched = if needle == "stripe" {
            all_lower_no_powered.contains(needle)
        } else {
            all_lower.contains(needle) || all_text.contains(needle)
        };
        if matched {
            if hints.vendor.is_none() {
                if needle == "stripe" {
                    if let Some(proc_vendor) = processor_vendor_fallback.take() {
                        hints.vendor = Some(proc_vendor);
                    } else {
                        hints.vendor = Some(display.to_string());
                    }
                } else {
                    hints.vendor = Some(display.to_string());
                }
            }
            if hints.category.is_none() {
                hints.category = Some(cat.to_string());
            }
            break;
        }
    }
    if hints.vendor.is_none() {
        hints.vendor = processor_vendor_fallback;
    }

    // Infer billing cycle from explicit keywords or period length
    hints.billing_cycle = infer_billing_cycle(&all_lower, &all_text, period);
    hints
}

fn is_plausible_vendor(v: &str) -> bool {
    let trimmed = v.trim();
    if trimmed.is_empty() || trimmed.contains('@') {
        return false;
    }
    if match_period_line(trimmed).is_some() || parse_date_token(trimmed).is_some() {
        return false;
    }
    // Reject truncated or partial date ranges like `2026-10-01 to ...`
    for sep in [" to ", " through ", " - ", " – ", " — ", " 至 ", "至", "~"] {
        if let Some((left, _)) = trimmed.split_once(sep) {
            if parse_date_token(left.trim()).is_some() {
                return false;
            }
        }
    }
    if find_money_in_str(trimmed).is_some() || normalize_amount_text(trimmed).is_some() {
        return false;
    }
    let low = trimmed.to_ascii_lowercase();
    if low.starts_with("powered by ")
        || low.starts_with("your card")
        || low.starts_with("your bank")
        || low.starts_with("your balance")
        || low.starts_with("your account")
        || low.starts_with("your payment")
        || low.starts_with("your subscription")
        || low.starts_with("invoice #")
        || low.starts_with("receipt #")
    {
        return false;
    }
    const BOILERPLATE_LOWER: &[&str] = &[
        "invoice",
        "receipt",
        "tax invoice",
        "payment receipt",
        "billing statement",
        "order confirmation",
        "credit note",
        "summary",
        "billing",
        "customer",
        "subscriber",
        "n/a",
        "none",
        "unknown",
        "-",
    ];
    if BOILERPLATE_LOWER.contains(&low.as_str()) {
        return false;
    }
    const BOILERPLATE_ZH: &[&str] = &[
        "电子发票",
        "增值税电子普通发票",
        "增值税普通发票",
        "增值税专用发票",
        "发票详情",
        "订单详情",
        "账单详情",
        "付款回执",
        "收据",
        "无",
    ];
    !BOILERPLATE_ZH.contains(&trimmed)
}

fn is_payment_processor_name(v: &str) -> bool {
    let low = v.trim().to_ascii_lowercase();
    matches!(
        low.as_str(),
        "stripe"
            | "stripe, inc."
            | "stripe, inc"
            | "stripe inc."
            | "stripe inc"
            | "stripe payments"
            | "paddle"
            | "paddle.com"
            | "paypal"
    )
}

fn match_account_label_line(text: &str) -> Option<String> {
    const EN: &[&str] = &[
        "account id",
        "account name",
        "account label",
        "account",
        "workspace",
        "organization",
        "team",
    ];
    const ZH: &[&str] = &[
        "账号名称",
        "账户名称",
        "购买账号",
        "登录账号",
        "账号",
        "账户",
    ];
    for &lbl in EN {
        if let Some(rest) = strip_label_ascii(text, lbl) {
            let low = rest.to_ascii_lowercase();
            if low.starts_with("email")
                || low.starts_with("balance")
                || low.starts_with("summary")
                || low.starts_with("status")
            {
                continue;
            }
            let v = clean_hint_value(rest);
            if is_valid_account_label(&v) {
                return Some(v);
            }
        }
    }
    for &lbl in ZH {
        if let Some(rest) = strip_label_zh(text, lbl) {
            if rest.starts_with("邮箱") || rest.starts_with("余额") || rest.starts_with("状态")
            {
                continue;
            }
            let v = clean_hint_value(rest);
            if is_valid_account_label(&v) {
                return Some(v);
            }
        }
    }
    None
}

fn is_valid_account_label(v: &str) -> bool {
    !v.is_empty()
        && !v.contains('*')
        && find_money_in_str(v).is_none()
        && parse_date_token(v).is_none()
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
    pub direction: String, // "expense" | "income" | "refund" | "neutral" | "unknown"
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

const MAX_CSV_RECORDS: usize = 50_000;
const MAX_CSV_FIELDS_PER_ROW: usize = 128;
const MAX_CSV_FIELD_BYTES: usize = 65_536;

pub fn decode_csv_bytes(raw: &[u8]) -> (String, &'static str) {
    // 1. UTF-8 BOM
    if let Some(stripped) = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        if let Ok(s) = std::str::from_utf8(stripped) {
            return (s.to_string(), "utf-8-bom");
        }
    }
    // 2. UTF-16LE BOM
    if raw.starts_with(&[0xFF, 0xFE]) {
        let (decoded, _, _) = encoding_rs::UTF_16LE.decode(raw);
        return (
            decoded.trim_start_matches('\u{feff}').to_string(),
            "utf-16le",
        );
    }
    // 3. UTF-16BE BOM
    if raw.starts_with(&[0xFE, 0xFF]) {
        let (decoded, _, _) = encoding_rs::UTF_16BE.decode(raw);
        return (
            decoded.trim_start_matches('\u{feff}').to_string(),
            "utf-16be",
        );
    }
    // 4. Clean UTF-8
    if let Ok(s) = std::str::from_utf8(raw) {
        return (s.to_string(), "utf-8");
    }
    // 5. GB18030 / GBK (Alipay & Chinese bank CSV exports on Windows)
    let (decoded, _, _) = encoding_rs::GB18030.decode(raw);
    (decoded.into_owned(), "gb18030")
}

struct CsvRecord {
    start_line: usize,
    fields: Vec<String>,
    raw_line: String,
}

fn parse_csv_records(text: &str) -> (Vec<CsvRecord>, bool) {
    let mut records = Vec::new();
    let mut hit_limit = false;
    let mut fields: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut raw_line = String::new();
    let mut in_quotes = false;
    let mut field_quoted = false;
    let mut cur_line: usize = 1;
    let mut record_start_line: usize = 1;
    let mut has_data = false;

    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if !has_data {
            record_start_line = cur_line;
        }
        if in_quotes {
            has_data = true;
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    if field.len() < MAX_CSV_FIELD_BYTES {
                        field.push('"');
                    }
                    if raw_line.len() < MAX_CSV_FIELD_BYTES {
                        raw_line.push_str("\"\"");
                    }
                } else {
                    in_quotes = false;
                    if raw_line.len() < MAX_CSV_FIELD_BYTES {
                        raw_line.push('"');
                    }
                }
            } else if ch == '\r' || ch == '\n' {
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                cur_line += 1;
                if field.len() < MAX_CSV_FIELD_BYTES {
                    field.push('\n');
                }
                if raw_line.len() < MAX_CSV_FIELD_BYTES {
                    raw_line.push(' ');
                }
            } else {
                if field.len() < MAX_CSV_FIELD_BYTES {
                    field.push(ch);
                }
                if raw_line.len() < MAX_CSV_FIELD_BYTES {
                    raw_line.push(ch);
                }
            }
        } else if ch == '"' && !field_quoted && field.trim().is_empty() {
            has_data = true;
            field.clear();
            in_quotes = true;
            field_quoted = true;
            if raw_line.len() < MAX_CSV_FIELD_BYTES {
                raw_line.push('"');
            }
        } else if ch == ',' {
            has_data = true;
            if fields.len() < MAX_CSV_FIELDS_PER_ROW {
                fields.push(field.trim().to_string());
            }
            field.clear();
            field_quoted = false;
            if raw_line.len() < MAX_CSV_FIELD_BYTES {
                raw_line.push(',');
            }
        } else if ch == '\r' || ch == '\n' {
            if ch == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            cur_line += 1;
            if has_data {
                if fields.len() < MAX_CSV_FIELDS_PER_ROW {
                    fields.push(field.trim().to_string());
                }
                if records.len() < MAX_CSV_RECORDS {
                    records.push(CsvRecord {
                        start_line: record_start_line,
                        fields: std::mem::take(&mut fields),
                        raw_line: std::mem::take(&mut raw_line),
                    });
                } else {
                    hit_limit = true;
                    break;
                }
                field.clear();
                field_quoted = false;
                has_data = false;
            }
        } else {
            has_data = true;
            if field.len() < MAX_CSV_FIELD_BYTES {
                field.push(ch);
            }
            if raw_line.len() < MAX_CSV_FIELD_BYTES {
                raw_line.push(ch);
            }
        }
    }

    if has_data && !hit_limit {
        if fields.len() < MAX_CSV_FIELDS_PER_ROW {
            fields.push(field.trim().to_string());
        }
        if records.len() < MAX_CSV_RECORDS {
            records.push(CsvRecord {
                start_line: record_start_line,
                fields,
                raw_line,
            });
        } else {
            hit_limit = true;
        }
    }

    (records, hit_limit)
}

fn extract_preamble_account(rec: &CsvRecord) -> Option<String> {
    let first = rec.fields.first().map_or("", String::as_str).trim();
    const PREFIXES: &[&str] = &[
        "账号:[",
        "支付宝账户：",
        "支付宝账户:",
        "支付宝账号：",
        "支付宝账号:",
        "微信昵称：[",
        "微信昵称:[",
        "微信昵称：",
        "微信昵称:",
        "微信号：",
        "微信号:",
        "昵称：",
        "昵称:",
        "账户名：",
        "账户名:",
        "导出账户：",
        "导出账号：",
    ];
    for &prefix in PREFIXES {
        if let Some(rest) = first.strip_prefix(prefix) {
            let cleaned = rest
                .trim()
                .trim_matches(['[', ']', '【', '】', ' ', ','])
                .trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
            // Handle split-column preamble like `支付宝账户：,alice@example.com`
            if let Some(second) = rec.fields.get(1) {
                let val = second
                    .trim()
                    .trim_matches(['[', ']', '【', '】', ' ', ','])
                    .trim();
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

pub fn parse_csv_statement(raw: &[u8]) -> CsvExtraction {
    let (text, encoding) = decode_csv_bytes(raw);
    let (records, hit_record_limit) = parse_csv_records(&text);

    let mut provider = "generic".to_string();
    let mut saw_export_info = false;
    let mut preamble_account: Option<String> = None;
    for rec in records.iter().take(30) {
        let t = rec.raw_line.trim();
        if t.contains("支付宝") || t.contains("支付宝账户") || t.contains("支付宝账号")
        {
            provider = "alipay".into();
        } else if t.contains("微信支付")
            || t.contains("微信号：")
            || t.contains("微信号:")
            || t.contains("微信昵称")
        {
            provider = "wechat".into();
        }
        if t.contains("导出信息") {
            saw_export_info = true;
        }
        if preamble_account.is_none() {
            preamble_account = extract_preamble_account(rec);
        }
    }

    // Find the real CSV header record (skipping Alipay/WeChat preamble headers)
    let mut header_idx: Option<usize> = None;
    let mut headers: Vec<String> = Vec::new();
    for (idx, rec) in records.iter().enumerate().take(50) {
        if rec.fields.len() >= 3 && looks_like_csv_header(&rec.fields) {
            header_idx = Some(idx);
            headers = rec.fields.clone();
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
            truncated: hit_record_limit,
            transactions: Vec::new(),
            candidates: Vec::new(),
            adopt_amount_idx: None,
            hints: ExtractedHints::default(),
        };
    };

    if provider == "generic" {
        if saw_export_info
            && headers.iter().any(|h| {
                h.contains("交易分类") || h.contains("收/付款方式") || h.contains("商家订单号")
            })
        {
            provider = "alipay".into();
        } else if headers.iter().any(|h| h.contains("当前状态"))
            && headers
                .iter()
                .any(|h| h.contains("金额(元)") || h.contains("金额（元）"))
        {
            provider = "wechat".into();
        }
    }

    let col_map = map_csv_columns(&headers);
    if col_map.amount.is_none() {
        return CsvExtraction {
            status: "no_match",
            reason: Some("csv_amount_column_missing"),
            encoding,
            provider,
            total_rows: 0,
            truncated: hit_record_limit,
            transactions: Vec::new(),
            candidates: Vec::new(),
            adopt_amount_idx: None,
            hints: ExtractedHints::default(),
        };
    }

    let provider_default_cur = if provider == "alipay" || provider == "wechat" {
        Some("CNY")
    } else {
        None
    };

    let mut transactions = Vec::new();
    let mut explicit_currencies: Vec<bool> = Vec::new();
    let mut total_rows = 0usize;
    let mut truncated = hit_record_limit;

    for rec in records.iter().skip(h_idx + 1) {
        let trimmed = rec.raw_line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("---")
            || trimmed.starts_with("共")
            || trimmed.starts_with("导出时间")
            || trimmed.starts_with("特别提示")
        {
            continue;
        }
        let cols = &rec.fields;
        if cols.len() < 2 {
            continue;
        }
        let Some(amt_idx) = col_map.amount else {
            continue;
        };
        let Some(raw_amt) = cols.get(amt_idx) else {
            continue;
        };
        let explicit_currency = col_map
            .currency
            .and_then(|i| cols.get(i))
            .and_then(|c| normalize_currency(c));

        let Some(parsed_amt) = parse_csv_amount(
            raw_amt,
            explicit_currency,
            col_map.amount_currency,
            provider_default_cur,
        ) else {
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
        let direction = classify_csv_direction(
            dir_raw,
            status.as_deref(),
            parsed_amt.is_negative,
            col_map.amount_implies_expense,
        );

        total_rows += 1;
        if transactions.len() >= MAX_CSV_CANDIDATES {
            truncated = true;
            continue;
        }
        explicit_currencies.push(parsed_amt.has_explicit_currency);
        transactions.push(CsvTransaction {
            row_number: rec.start_line,
            date,
            merchant,
            description,
            currency: parsed_amt.currency,
            amount_text: parsed_amt.amount_text,
            direction,
            account_hint,
            payment_method,
            status,
            raw_line: rec.raw_line.clone(),
        });
    }

    let mut candidates = Vec::new();
    let mut adopt_amount_idx = None;

    // Only auto-adopt a 1-row CSV when it has BOTH an explicit expense/paid signal
    // AND an explicit (or provider-defined) currency.
    let can_adopt_single = transactions.len() == 1
        && transactions[0].direction == "expense"
        && explicit_currencies.first().copied().unwrap_or(false)
        && !transactions[0].currency.is_empty();
    if can_adopt_single {
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
        if let Some(iso_date) = parse_date_token(&tx.date) {
            hints.bill_date = Some(iso_date);
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

struct ParsedCsvAmount {
    currency: String,
    amount_text: String,
    is_negative: bool,
    has_explicit_currency: bool,
}

fn parse_csv_amount(
    raw: &str,
    explicit_currency: Option<&'static str>,
    header_currency: Option<&'static str>,
    provider_default: Option<&'static str>,
) -> Option<ParsedCsvAmount> {
    let mut s = raw.trim().trim_matches('\u{00a0}').trim();
    if s.is_empty() {
        return None;
    }
    let mut is_negative = false;

    // Accounting parentheses: `(20.00)`, `($20.00)`, `（¥15.00）`
    if let Some(inner) = s
        .strip_prefix('(')
        .and_then(|v| v.strip_suffix(')'))
        .or_else(|| s.strip_prefix('（').and_then(|v| v.strip_suffix('）')))
    {
        is_negative = true;
        s = inner.trim();
    }

    // Leading sign before currency or number: `-¥15.00`, `-$20.00`, `-20.00`
    if let Some(rest) = s.strip_prefix('-').or_else(|| s.strip_prefix('\u{2212}')) {
        is_negative = true;
        s = rest.trim_start();
    } else if let Some(rest) = s.strip_prefix('+') {
        s = rest.trim_start();
    }

    // Trailing minus: `20.00-`
    if !is_negative {
        if let Some(rest) = s.strip_suffix('-').or_else(|| s.strip_suffix('\u{2212}')) {
            let trimmed = rest.trim_end();
            if !trimmed.is_empty() {
                is_negative = true;
                s = trimmed;
            }
        }
    }

    // Sign AFTER prefix currency symbol: `¥-15.00`, `￥-15.00`, `$-20.00`, `US$-20.00`
    for &(sym, code) in CURRENCY_SYMBOLS {
        if let Some(after) = s.strip_prefix(sym) {
            let after_ws = after.trim_start();
            if let Some(rest) = after_ws
                .strip_prefix('-')
                .or_else(|| after_ws.strip_prefix('\u{2212}'))
            {
                is_negative = true;
                let unsigned = rest.trim_start();
                if let Some((num_raw, trailing)) = take_number_prefix(unsigned) {
                    if let Some(amt) = normalize_amount_text(num_raw) {
                        let base = explicit_currency
                            .or_else(|| leading_iso_currency(trailing.trim_start()))
                            .unwrap_or(code);
                        return Some(ParsedCsvAmount {
                            currency: base.to_string(),
                            amount_text: amt,
                            is_negative,
                            has_explicit_currency: true,
                        });
                    }
                }
            } else if let Some(rest) = after_ws.strip_prefix('+') {
                let unsigned = rest.trim_start();
                if let Some((num_raw, trailing)) = take_number_prefix(unsigned) {
                    if let Some(amt) = normalize_amount_text(num_raw) {
                        let base = explicit_currency
                            .or_else(|| leading_iso_currency(trailing.trim_start()))
                            .unwrap_or(code);
                        return Some(ParsedCsvAmount {
                            currency: base.to_string(),
                            amount_text: amt,
                            is_negative,
                            has_explicit_currency: true,
                        });
                    }
                }
            }
            break;
        }
    }

    // Sign AFTER prefix ISO code: `USD -20.00`, `CNY -15.00`
    if let Some((code, after_code)) = strip_leading_iso_currency(s) {
        let after_ws = after_code.trim_start_matches([' ', ':', '：', '$', '¥', '￥', '€', '£']);
        if let Some(rest) = after_ws
            .strip_prefix('-')
            .or_else(|| after_ws.strip_prefix('\u{2212}'))
        {
            is_negative = true;
            let unsigned = rest.trim_start();
            if let Some((num_raw, _)) = take_number_prefix(unsigned) {
                if let Some(amt) = normalize_amount_text(num_raw) {
                    let base = explicit_currency.unwrap_or(code);
                    return Some(ParsedCsvAmount {
                        currency: base.to_string(),
                        amount_text: amt,
                        is_negative,
                        has_explicit_currency: true,
                    });
                }
            }
        }
    }

    // Standard unsigned money token with symbol or ISO code
    if let Some((c, a)) = find_money_in_str(s) {
        let final_cur = explicit_currency.unwrap_or(c);
        return Some(ParsedCsvAmount {
            currency: final_cur.to_string(),
            amount_text: a,
            is_negative,
            has_explicit_currency: true,
        });
    }

    // Bare numeric amount
    if let Some(a) = normalize_amount_text(s) {
        let explicit = explicit_currency.or(header_currency);
        let has_explicit_currency = explicit.is_some() || provider_default.is_some();
        let final_cur = explicit.or(provider_default).unwrap_or("");
        return Some(ParsedCsvAmount {
            currency: final_cur.to_string(),
            amount_text: a,
            is_negative,
            has_explicit_currency,
        });
    }

    None
}

#[derive(Default)]
struct CsvColumnMap {
    date: Option<usize>,
    merchant: Option<usize>,
    description: Option<usize>,
    amount: Option<usize>,
    amount_currency: Option<&'static str>,
    amount_implies_expense: bool,
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

fn extract_header_currency(raw: &str) -> Option<&'static str> {
    for (open, close) in [('(', ')'), ('（', '）'), ('[', ']')] {
        if let Some(start) = raw.find(open) {
            let tail = &raw[start + open.len_utf8()..];
            if let Some(end) = tail.find(close) {
                let inner = tail[..end].trim();
                if let Some(code) = normalize_currency(inner) {
                    return Some(code);
                }
                match inner {
                    "元" | "人民币" | "¥" | "￥" => return Some("CNY"),
                    "$" => return Some("USD"),
                    "€" => return Some("EUR"),
                    "£" => return Some("GBP"),
                    "円" => return Some("JPY"),
                    _ => {}
                }
            }
        }
    }
    None
}

fn score_csv_amount_column(raw: &str, low: &str) -> (u8, bool) {
    // Strictly reject discount, original price, fee, tax, refund, and running-balance columns
    const EXCLUDED_ZH: &[&str] = &[
        "优惠金额",
        "折扣金额",
        "立减金额",
        "红包金额",
        "抵扣金额",
        "原订单金额",
        "商品原价",
        "原价",
        "退款金额",
        "手续费",
        "税额",
        "余额",
    ];
    const EXCLUDED_EN: &[&str] = &[
        "discount",
        "original price",
        "original amount",
        "subtotal",
        "tax",
        "fee",
        "refunded",
        "balance",
    ];
    if EXCLUDED_ZH.iter().any(|k| raw.contains(k)) || EXCLUDED_EN.iter().any(|k| low.contains(k)) {
        return (0, false);
    }

    let base_low = low.split(['(', '[']).next().unwrap_or(low).trim();

    if raw.contains("实付金额") || raw == "实付" || raw == "实付款" {
        return (100, true);
    }
    if raw.contains("支付金额")
        || raw.contains("付款金额")
        || raw.contains("扣款金额")
        || raw.contains("已付金额")
        || matches!(
            base_low,
            "amount paid" | "amount_paid" | "paid amount" | "charge amount"
        )
    {
        return (95, true);
    }
    if matches!(base_low, "debit" | "withdrawal" | "outflow") || raw.contains("支出金额") {
        return (90, true);
    }
    if raw == "金额(元)"
        || raw == "金额（元）"
        || raw == "金额"
        || raw.contains("交易金额")
        || raw == "订单金额"
    {
        return (85, false);
    }
    if matches!(
        base_low,
        "total amount" | "total_amount" | "amount" | "total"
    ) {
        return (80, false);
    }
    if matches!(base_low, "gross" | "gross amount") {
        return (72, false);
    }
    if matches!(base_low, "net" | "net amount") {
        return (70, false);
    }
    if base_low == "charge" {
        return (68, true);
    }
    if base_low == "price" {
        return (65, false);
    }
    if raw.contains("金额") {
        return (50, false);
    }
    (0, false)
}

fn map_csv_columns(headers: &[String]) -> CsvColumnMap {
    let mut map = CsvColumnMap::default();
    let mut best_amount_score = 0u8;
    let mut normalized: BTreeMap<usize, String> = BTreeMap::new();
    for (idx, h) in headers.iter().enumerate() {
        normalized.insert(idx, h.trim().trim_matches('\u{feff}').to_ascii_lowercase());
    }
    for (idx, h) in &normalized {
        let raw = headers[*idx].trim().trim_matches('\u{feff}');
        if map.date.is_none()
            && (raw.contains("交易时间")
                || raw.contains("付款时间")
                || raw.contains("支付时间")
                || raw.contains("创建时间")
                || raw.contains("记账日期")
                || raw.contains("入账日期")
                || raw.contains("日期")
                || h == "date"
                || h == "timestamp"
                || h.contains("transaction date")
                || h.contains("billing date")
                || h.contains("payment date")
                || h.contains("posting date")
                || h.contains("created"))
        {
            map.date = Some(*idx);
        }
        if map.merchant.is_none()
            && (raw.contains("交易对方")
                || raw.contains("商户名称")
                || raw.contains("交易商户")
                || raw.contains("商家")
                || raw.contains("收款方")
                || h == "merchant"
                || h == "vendor"
                || h == "payee"
                || h == "seller"
                || h == "service"
                || h == "counterparty"
                || h == "store"
                || h == "company")
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
        let (amt_score, implies_expense) = score_csv_amount_column(raw, h);
        if amt_score > best_amount_score {
            best_amount_score = amt_score;
            map.amount = Some(*idx);
            map.amount_currency = extract_header_currency(raw);
            map.amount_implies_expense = implies_expense;
        }
        if map.currency.is_none() && (raw.contains("币种") || h == "currency") {
            map.currency = Some(*idx);
        }
        if map.direction.is_none()
            && (raw == "收/支"
                || raw.contains("收支")
                || raw.contains("借贷")
                || h == "direction"
                || h == "type"
                || h == "flow"
                || h == "dr/cr")
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
                || raw.contains("账户")
                || raw.contains("邮箱")
                || h == "account"
                || h == "account_id"
                || h == "email"
                || h == "account_email"
                || h.contains("customer email"))
        {
            map.account = Some(*idx);
        }
    }
    map
}

fn classify_csv_direction(
    dir_raw: &str,
    status: Option<&str>,
    is_negative: bool,
    amount_implies_expense: bool,
) -> String {
    let dir_low = dir_raw.to_ascii_lowercase();
    let status_raw = status.unwrap_or("");
    let status_low = status_raw.to_ascii_lowercase();

    if status_raw.contains("退款")
        || status_low.contains("refund")
        || status_raw.contains("交易关闭")
        || status_low.contains("closed")
        || status_low.contains("failed")
        || status_low.contains("void")
        || status_low.contains("canceled")
        || status_low.contains("cancelled")
        || dir_raw.contains("退款")
        || dir_low.contains("refund")
    {
        return "refund".into();
    }
    if dir_raw.contains("中性交易")
        || dir_raw.contains("不计收支")
        || dir_raw.contains("其他")
        || dir_raw == "/"
        || dir_low == "neutral"
        || dir_low == "transfer"
    {
        return "neutral".into();
    }
    if dir_raw.contains("支出")
        || dir_low == "expense"
        || dir_low == "debit"
        || dir_low == "out"
        || dir_low == "charge"
        || dir_low == "payment"
        || dir_low == "purchase"
    {
        return "expense".into();
    }
    if dir_raw.contains("收入") || dir_low == "income" || dir_low == "credit" || dir_low == "in" {
        return "income".into();
    }
    if is_negative {
        return "refund".into();
    }
    if amount_implies_expense
        || status_low == "paid"
        || status_low == "succeeded"
        || status_low == "charged"
        || status_raw.contains("支付成功")
        || status_raw.contains("付款成功")
    {
        return "expense".into();
    }
    "unknown".into()
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
        assert_eq!(find_money_in_str("98.00 元"), Some(("CNY", "98.00".into())));
        assert_eq!(find_money_in_str("€12,99"), Some(("EUR", "12.99".into())));
        assert_eq!(find_money_in_str("19,99 €"), Some(("EUR", "19.99".into())));
        assert_eq!(
            find_money_in_str("1.299,50 €"),
            Some(("EUR", "1299.50".into()))
        );
        assert_eq!(find_money_in_str("20.00 $"), Some(("USD", "20.00".into())));
        assert_eq!(find_money_in_str("15.00 £"), Some(("GBP", "15.00".into())));
        assert_eq!(
            find_money_in_str("$20.00/mo"),
            Some(("USD", "20.00".into()))
        );
        assert_eq!(find_money_in_str("¥98/年"), Some(("CNY", "98.00".into())));
        assert_eq!(
            find_money_in_str("(USD 19.99)"),
            Some(("USD", "19.99".into()))
        );
        assert_eq!(find_money_in_str("A$29.00"), Some(("AUD", "29.00".into())));
        assert_eq!(find_money_in_str("C$35.00"), Some(("CAD", "35.00".into())));
        assert_eq!(find_money_in_str("S$12.50"), Some(("SGD", "12.50".into())));
        assert_eq!(
            find_money_in_str("VISA$20.00"),
            Some(("USD", "20.00".into()))
        );
        assert_eq!(
            find_money_in_str("Qty 1 $20.00"),
            Some(("USD", "20.00".into()))
        );
        assert_eq!(find_money_in_str("GBP 9.99"), Some(("GBP", "9.99".into())));
        assert_eq!(
            find_money_in_str("JPY 1,200"),
            Some(("JPY", "1200.00".into()))
        );
        assert_eq!(
            find_money_in_str("$25.00 CAD"),
            Some(("CAD", "25.00".into()))
        );
        // Reject malformed European grouping and date-like commas
        assert_eq!(normalize_amount_text("1.299,50"), Some("1299.50".into()));
        assert_eq!(normalize_amount_text("1.2.3,45"), None);
        assert_eq!(normalize_amount_text("2026.01,00"), None);
        assert_eq!(find_money_in_str("1.2.3,45 EUR"), None);
    }

    #[test]
    fn parses_bilingual_service_periods_and_dot_dates() {
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
        assert_eq!(
            parse_date_token("2026.05.01"),
            Some("2026-05-01".to_string())
        );
        assert_eq!(
            parse_date_token("2024.02.29"),
            Some("2024-02-29".to_string())
        );
        assert_eq!(parse_date_token("2026.02.29"), None);
        assert_eq!(parse_date_token("2026-04-31"), None);
        assert_eq!(parse_date_token("2026-13-01"), None);
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
    fn handles_nested_html_tables_br_splitting_preheaders_del_and_zwsp() {
        let html = concat!(
            "<html><body>",
            "<div style=\"opacity:0; max-height:0px; font-size:0; mso-hide:all\">Amount paid: $777.00</div>",
            "<div style=\"opacity: 0.5\">From: Cursor</div>",
            "<table><tr><td>",
            "<table>",
            "<tr><th>Plan</th><td>Cursor Pro<br>2026.05.01 - 2026.06.01</td></tr>",
            "<tr><th>A\u{200B}mount p\u{200C}aid</th><td><del>$40.00</del> <s>$35.00</s> ",
            "<span style=\"text-decoration: line-through\">$30.00</span> $\u{FEFF}20.00\u{00AD}/mo</td></tr>",
            "</table>",
            "</td></tr></table>",
            "</body></html>",
        );
        let res = extract_real_html(html);
        assert_eq!(res.status, "matched");
        let amt = &res.candidates[res.adopt_amount_idx.unwrap()];
        assert_eq!(amt.value["currency"], "USD");
        assert_eq!(amt.value["amount_text"], "20.00");
        let per = &res.candidates[res.adopt_period_idx.unwrap()];
        assert_eq!(per.value["period_start"], "2026-05-01");
        assert_eq!(per.value["period_end"], "2026-06-01");
        assert_eq!(res.hints.vendor.as_deref(), Some("Cursor"));
    }

    #[test]
    fn infers_jpy_vs_cny_for_yen_symbol_from_document_context() {
        let jpy_html = r#"<html><body>
            <h2>Tokyo Cloud Receipt</h2>
            <p>消費税 10% included (日本)</p>
            <p>Amount paid: ¥1,500</p>
        </body></html>"#;
        let res_jpy = extract_real_html(jpy_html);
        assert_eq!(res_jpy.status, "matched");
        let amt = &res_jpy.candidates[res_jpy.adopt_amount_idx.unwrap()];
        assert_eq!(amt.value["currency"], "JPY");
        assert_eq!(amt.value["amount_text"], "1500.00");

        let cny_html = r#"<html><body>
            <h2>人民币收据 (日本旅游纪念品)</h2>
            <p>实付金额：¥98.00</p>
        </body></html>"#;
        let res_cny = extract_real_html(cny_html);
        assert_eq!(res_cny.status, "matched");
        let amt_cny = &res_cny.candidates[res_cny.adopt_amount_idx.unwrap()];
        assert_eq!(amt_cny.value["currency"], "CNY");
        assert_eq!(amt_cny.value["amount_text"], "98.00");
    }

    #[test]
    fn extracts_chinese_shudianpiao_preferring_jiasuiheji_and_metadata() {
        let fapiao_html = r#"<html><body>
            <h2>电子发票（普通发票）数电票</h2>
            <p>销方名称：腾讯云计算（北京）有限责任公司</p>
            <p>项目名称：*信息技术服务*云服务器包年</p>
            <p>开票日期：2026.05.01</p>
            <p>账号：个人</p>
            <table>
              <tr><th>合计</th><td>¥92.45</td></tr>
              <tr><th>税额</th><td>¥5.55</td></tr>
              <tr><th>价税合计（大写）玖拾捌元整</th><td>（小写）98.00</td></tr>
            </table>
        </body></html>"#;
        let res = extract_real_html(fapiao_html);
        assert_eq!(res.status, "matched");
        let amt = &res.candidates[res.adopt_amount_idx.unwrap()];
        assert_eq!(amt.value["currency"], "CNY");
        assert_eq!(amt.value["amount_text"], "98.00");
        assert_eq!(
            res.hints.vendor.as_deref(),
            Some("腾讯云计算（北京）有限责任公司")
        );
        assert_eq!(
            res.hints.plan_name.as_deref(),
            Some("*信息技术服务*云服务器包年")
        );
        assert_eq!(res.hints.account_label.as_deref(), Some("个人"));
        assert_eq!(res.hints.bill_date.as_deref(), Some("2026-05-01"));
    }

    #[test]
    fn avoids_false_vendor_and_zero_due_conflicts() {
        let html = r#"<html><body>
            <h2>Receipt</h2>
            <p>Powered by Stripe</p>
            <p>From: Stripe, Inc.</p>
            <p>From 2026-10-01 to 2026-11-01</p>
            <p>Merchant: Notion Labs</p>
            <p>Subtotal: $18.00</p>
            <p>Total excluding tax: $18.00</p>
            <p>Total tax: $2.00</p>
            <p>Total: $20.00</p>
            <p>Amount paid: $20.00</p>
            <p>Total due: $0.00</p>
            <p>Balance due: $0.00</p>
        </body></html>"#;
        let res = extract_real_html(html);
        assert_eq!(res.status, "matched");
        let amt = &res.candidates[res.adopt_amount_idx.unwrap()];
        assert_eq!(amt.value["currency"], "USD");
        assert_eq!(amt.value["amount_text"], "20.00");
        let per = &res.candidates[res.adopt_period_idx.unwrap()];
        assert_eq!(per.value["period_start"], "2026-10-01");
        assert_eq!(per.value["period_end"], "2026-11-01");
        assert_eq!(res.hints.vendor.as_deref(), Some("Notion Labs"));

        // Paid receipt with Total + Total due $0.00 (no explicit Amount paid) must not conflict
        let total_only = r#"<html><body>
            <h2>Invoice Paid</h2>
            <p>From 2026-10-01 to 2026-11-01</p>
            <p>Total: $20.00</p>
            <p>Total due: $0.00</p>
        </body></html>"#;
        let res2 = extract_real_html(total_only);
        assert_eq!(res2.status, "matched");
        let amt2 = &res2.candidates[res2.adopt_amount_idx.unwrap()];
        assert_eq!(amt2.value["amount_text"], "20.00");
        assert!(res2.hints.vendor.is_none());
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
    fn parses_rfc4180_multiline_utf16_negatives_neutral_and_honest_unknown_csv() {
        // 1. Multiline RFC 4180 CSV with embedded newline, escaped quotes, and negative formats
        let multiline = "Date,Merchant,Description,Amount\r\n\
2026-10-01,OpenAI,\"ChatGPT Plus\r\n\"\"Team\"\" renewal\",$-20.00\r\n\
2026-10-02,WeChat,\"Note\nline 2\",¥-15.00\r\n\
2026-10-03,Bank,Credit adjustment,(20.00)\r\n\
2026-10-04,Stripe,Refund,($35.50)\r\n";
        let res = parse_csv_statement(multiline.as_bytes());
        assert_eq!(res.status, "statement");
        assert_eq!(res.transactions.len(), 4);
        assert_eq!(
            res.transactions[0].description,
            "ChatGPT Plus\n\"Team\" renewal"
        );
        assert_eq!(res.transactions[0].currency, "USD");
        assert_eq!(res.transactions[0].amount_text, "20.00");
        assert_eq!(res.transactions[0].direction, "refund");
        assert_eq!(res.transactions[1].currency, "CNY");
        assert_eq!(res.transactions[1].amount_text, "15.00");
        assert_eq!(res.transactions[1].direction, "refund");
        assert_eq!(res.transactions[2].amount_text, "20.00");
        assert_eq!(res.transactions[2].direction, "refund");
        assert_eq!(res.transactions[3].currency, "USD");
        assert_eq!(res.transactions[3].amount_text, "35.50");
        assert_eq!(res.transactions[3].direction, "refund");

        // 2. UTF-16LE BOM CSV
        let utf16_text =
            "Date,Merchant,Amount (USD),Direction\r\n2026-10-01,Cursor,20.00,expense\r\n";
        let mut utf16_bytes = vec![0xFFu8, 0xFEu8];
        for unit in utf16_text.encode_utf16() {
            utf16_bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let res16 = parse_csv_statement(&utf16_bytes);
        assert_eq!(res16.encoding, "utf-16le");
        assert_eq!(res16.status, "matched");
        assert_eq!(res16.adopt_amount_idx, Some(0));
        assert_eq!(res16.transactions[0].currency, "USD");
        assert_eq!(res16.transactions[0].amount_text, "20.00");

        // 3. 2024-2026 Alipay preamble, neutral transaction, and paid-amount column priority
        let alipay_new = "导出信息：\n支付宝账户：alice@example.com\n交易时间,交易分类,交易对方,商品说明,收/支,优惠金额,实付金额,交易状态\n2026-10-01 09:00:00,转账,余额宝,转入余额宝,中性交易,0.00,500.00,交易成功\n2026-10-02 10:00:00,软件,金山办公,WPS会员,支出,10.00,88.00,交易成功\n";
        let res_ali = parse_csv_statement(alipay_new.as_bytes());
        assert_eq!(res_ali.provider, "alipay");
        assert_eq!(
            res_ali.hints.account_label.as_deref(),
            Some("alice@example.com")
        );
        assert_eq!(res_ali.transactions.len(), 2);
        assert_eq!(res_ali.transactions[0].direction, "neutral");
        assert_eq!(res_ali.transactions[1].direction, "expense");
        assert_eq!(res_ali.transactions[1].amount_text, "88.00");

        // 4. Generic 1-row CSV without explicit direction/currency MUST NOT auto-adopt blindly
        let generic_one = "Date,Merchant,Amount\n2026-10-01,Acme,20.00\n";
        let res_gen = parse_csv_statement(generic_one.as_bytes());
        assert_eq!(res_gen.status, "statement");
        assert!(res_gen.adopt_amount_idx.is_none());
        assert_eq!(res_gen.transactions[0].direction, "unknown");
    }

    #[test]
    fn refuses_credit_note_and_conflicting_amounts() {
        let credit = r#"<html><body><p>Document: Credit Note</p><p>Total $20.00</p></body></html>"#;
        let res = extract_real_html(credit);
        assert!(res.adopt_amount_idx.is_none());

        let refund_zh =
            r#"<html><body><p>负数发票（红字发票）</p><p>价税合计（小写）¥98.00</p></body></html>"#;
        let res_zh = extract_real_html(refund_zh);
        assert!(res_zh.adopt_amount_idx.is_none());

        let refund_en = r#"<html><body><p>Refund Confirmation</p><p>Amount refunded: $20.00</p><p>Amount paid: $20.00</p></body></html>"#;
        let res_en = extract_real_html(refund_en);
        assert!(res_en.adopt_amount_idx.is_none());

        let conflict =
            r#"<html><body><p>Amount paid: $20.00</p><p>Amount paid: $35.00</p></body></html>"#;
        let res2 = extract_real_html(conflict);
        assert_eq!(res2.status, "conflict");
        assert!(res2.adopt_amount_idx.is_none());
    }
}
