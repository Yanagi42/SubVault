// SubVault Browser Engine — Pure static ES module mirroring SubVault's
// Rust/SQLite preservation, extraction, append-only decision, and offline-exit contracts.

import { DEMO_BATCH_DOCS, REVIEW_ANCHOR_DATE } from "./demo-data.js";

// --- Deterministic Pure-JS SHA-256 (works on http://, https://, file://, and Node) ---

const K = new Uint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2
]);

export function sha256Bytes(bytes) {
  const len = bytes.length;
  const bitLenHi = Math.floor((len * 8) / 0x100000000);
  const bitLenLo = (len * 8) >>> 0;
  const paddedLen = Math.ceil((len + 9) / 64) * 64;
  const padded = new Uint8Array(paddedLen);
  padded.set(bytes);
  padded[len] = 0x80;
  const view = new DataView(padded.buffer);
  view.setUint32(paddedLen - 8, bitLenHi, false);
  view.setUint32(paddedLen - 4, bitLenLo, false);

  let h0 = 0x6a09e667;
  let h1 = 0xbb67ae85;
  let h2 = 0x3c6ef372;
  let h3 = 0xa54ff53a;
  let h4 = 0x510e527f;
  let h5 = 0x9b05688c;
  let h6 = 0x1f83d9ab;
  let h7 = 0x5be0cd19;

  const w = new Uint32Array(64);
  for (let offset = 0; offset < paddedLen; offset += 64) {
    for (let i = 0; i < 16; i++) {
      w[i] = view.getUint32(offset + i * 4, false);
    }
    for (let i = 16; i < 64; i++) {
      const s0 = ((w[i - 15] >>> 7) | (w[i - 15] << 25)) ^ ((w[i - 15] >>> 18) | (w[i - 15] << 14)) ^ (w[i - 15] >>> 3);
      const s1 = ((w[i - 2] >>> 17) | (w[i - 2] << 15)) ^ ((w[i - 2] >>> 19) | (w[i - 2] << 13)) ^ (w[i - 2] >>> 10);
      w[i] = (w[i - 16] + s0 + w[i - 7] + s1) >>> 0;
    }

    let a = h0, b = h1, c = h2, d = h3, e = h4, f = h5, g = h6, h = h7;
    for (let i = 0; i < 64; i++) {
      const S1 = ((e >>> 6) | (e << 26)) ^ ((e >>> 11) | (e << 21)) ^ ((e >>> 25) | (e << 7));
      const ch = (e & f) ^ (~e & g);
      const temp1 = (h + S1 + ch + K[i] + w[i]) >>> 0;
      const S0 = ((a >>> 2) | (a << 30)) ^ ((a >>> 13) | (a << 19)) ^ ((a >>> 22) | (a << 10));
      const maj = (a & b) ^ (a & c) ^ (b & c);
      const temp2 = (S0 + maj) >>> 0;

      h = g;
      g = f;
      f = e;
      e = (d + temp1) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (temp1 + temp2) >>> 0;
    }

    h0 = (h0 + a) >>> 0;
    h1 = (h1 + b) >>> 0;
    h2 = (h2 + c) >>> 0;
    h3 = (h3 + d) >>> 0;
    h4 = (h4 + e) >>> 0;
    h5 = (h5 + f) >>> 0;
    h6 = (h6 + g) >>> 0;
    h7 = (h7 + h) >>> 0;
  }

  return [h0, h1, h2, h3, h4, h5, h6, h7]
    .map((n) => n.toString(16).padStart(8, "0"))
    .join("");
}

const encoder = new TextEncoder();

export function utf8Bytes(text) {
  return encoder.encode(text);
}

export function sha256Text(text) {
  return sha256Bytes(utf8Bytes(text));
}

function deterministicHex(seed, length = 16) {
  return sha256Text(seed).slice(0, length);
}

export function isValidUsdAmount(text) {
  return /^\d+\.\d{2}$/.test(String(text).trim());
}

// --- Static Extraction (mirrors src/body.rs & src/html.rs rules) ---

function decodeHtmlEntities(str) {
  return str
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&nbsp;/g, "\u00a0");
}

function collapseHtmlWhitespace(str) {
  return str.replace(/[ \t\r\f\n]+/g, " ").trim();
}

function stripTags(html) {
  return decodeHtmlEntities(html.replace(/<br\s*\/?>/gi, "\n").replace(/<[^>]+>/g, ""));
}

function locateExcerpt(sourceText, excerpt) {
  const idx = sourceText.indexOf(excerpt);
  if (idx < 0) return null;
  const before = sourceText.slice(0, idx);
  const byteStart = utf8Bytes(before).length;
  const byteEnd = byteStart + utf8Bytes(excerpt).length;
  const scalarStart = Array.from(before).length;
  const scalarEnd = scalarStart + Array.from(excerpt).length;
  return { byteStart, byteEnd, scalarStart, scalarEnd };
}

export function extractDocument(content, mediaType) {
  const candidates = [];
  let extractionStatus = "ok";
  let extractionRule = "stated-lines-1";
  let extractionNote = null;

  const isHtml = mediaType.includes("html");
  const isCsv = mediaType.includes("csv");

  if (isHtml) {
    extractionRule = "stated-html-blocks-3";
    // Format 4 safety gate: reject CSS / style / stylesheet links without executing HTML
    const hasCss =
      /<style[\s>]/i.test(content) ||
      /\sstyle\s*=/i.test(content) ||
      /<link[^>]+rel\s*=\s*["'][^"']*stylesheet/i.test(content);
    if (hasCss) {
      return {
        extractionStatus: "unsupported",
        extractionRule,
        extractionNote:
          "Format 4 safety boundary: HTML contains <style> or stylesheet rules. Raw bytes are preserved verbatim; extraction is intentionally skipped rather than guessing CSS visibility.",
        candidates: [],
        adoptable: [],
        metadata: extractCommonMetadata(content)
      };
    }

    // 1. Check same-row <tr> explicit label + value cell pairs
    const trRegex = /<tr\b[^>]*>([\s\S]*?)<\/tr>/gi;
    let trMatch;
    while ((trMatch = trRegex.exec(content)) !== null) {
      const rowHtml = trMatch[0];
      const cellMatches = [...rowHtml.matchAll(/<(td|th)\b[^>]*>([\s\S]*?)<\/\1>/gi)];
      const cells = cellMatches.map((m) => ({
        raw: m[0],
        text: collapseHtmlWhitespace(stripTags(m[2]))
      }));

      // Amount paid row pair
      const paidLabels = cells.filter((c) => c.text === "Amount paid" || c.text === "Amount paid:");
      const paidValues = cells.filter((c) => /^USD (\d+\.\d{2})$/.test(c.text));
      if (paidLabels.length === 1 && paidValues.length === 1) {
        const amount = paidValues[0].text.match(/^USD (\d+\.\d{2})$/)[1];
        const loc = locateExcerpt(content, rowHtml);
        candidates.push({
          kind: "stated_amount_paid",
          usable: true,
          excerpt: rowHtml,
          displayExcerpt: `Amount paid USD ${amount}`,
          displayVersion: "html-row-stated-1",
          value: { currency: "USD", amount_text: amount, payment_verified: false },
          locator: loc
        });
      }

      // Service period row pair
      const periodLabels = cells.filter((c) => c.text === "Service period" || c.text === "Service period:");
      const periodValues = cells.filter((c) => /^(\d{4}-\d{2}-\d{2}) through (\d{4}-\d{2}-\d{2})$/.test(c.text));
      if (periodLabels.length === 1 && periodValues.length === 1) {
        const pm = periodValues[0].text.match(/^(\d{4}-\d{2}-\d{2}) through (\d{4}-\d{2}-\d{2})$/);
        const loc = locateExcerpt(content, rowHtml);
        candidates.push({
          kind: "stated_service_period",
          usable: true,
          excerpt: rowHtml,
          displayExcerpt: `Service period: ${pm[1]} through ${pm[2]}`,
          displayVersion: "html-row-stated-1",
          value: { period_start: pm[1], period_end: pm[2], time_status: "unknown", current_status: "unknown" },
          locator: loc
        });
      }
    }

    // 2. Also scan block sentences inside <p>, <div>, <td>, <th>, <li>
    const blockRegex = /<(p|div|li|td|th|h[1-6]|blockquote)\b[^>]*>([\s\S]*?)<\/\1>/gi;
    let bMatch;
    while ((bMatch = blockRegex.exec(content)) !== null) {
      const collapsed = collapseHtmlWhitespace(stripTags(bMatch[2]));
      const paidM = collapsed.match(/^Amount paid USD (\d+\.\d{2})$/);
      if (paidM && !candidates.some((c) => c.kind === "stated_amount_paid" && c.value.amount_text === paidM[1])) {
        candidates.push({
          kind: "stated_amount_paid",
          usable: true,
          excerpt: bMatch[0],
          displayExcerpt: collapsed,
          displayVersion: "html-collapse-1",
          value: { currency: "USD", amount_text: paidM[1], payment_verified: false },
          locator: locateExcerpt(content, bMatch[0])
        });
      }
      const perM = collapsed.match(/^Service period:\s*(\d{4}-\d{2}-\d{2}) through (\d{4}-\d{2}-\d{2})$/);
      if (perM && !candidates.some((c) => c.kind === "stated_service_period" && c.value.period_start === perM[1])) {
        candidates.push({
          kind: "stated_service_period",
          usable: true,
          excerpt: bMatch[0],
          displayExcerpt: collapsed,
          displayVersion: "html-collapse-1",
          value: { period_start: perM[1], period_end: perM[2], time_status: "unknown", current_status: "unknown" },
          locator: locateExcerpt(content, bMatch[0])
        });
      }
    }
  } else if (isCsv) {
    extractionRule = "csv-row-inspect-1";
    const lines = content.split(/\r?\n/).filter((l) => l.trim().length > 0);
    if (lines.length >= 2) {
      const rowText = lines[1];
      const cells = parseCsvLine(rowText);
      const header = parseCsvLine(lines[0]).map((h) => h.trim().toLowerCase());
      const chargedIdx = header.indexOf("charged");
      const currencyIdx = header.indexOf("currency");
      const orderIdx = header.indexOf("order_id");
      if (chargedIdx >= 0 && cells[chargedIdx] && isValidUsdAmount(cells[chargedIdx])) {
        candidates.push({
          kind: "csv_charged_amount",
          usable: false,
          reason: "csv_store_export_row",
          excerpt: rowText,
          displayExcerpt: `CSV charged ${cells[currencyIdx] || "USD"} ${cells[chargedIdx]} (Order ${cells[orderIdx] || "n/a"})`,
          displayVersion: "csv-row-1",
          value: {
            currency: cells[currencyIdx] || "USD",
            amount_text: cells[chargedIdx],
            order_id: cells[orderIdx] || null,
            payment_verified: false
          },
          locator: locateExcerpt(content, rowText)
        });
      }
    }
  } else {
    // Plain-text / text-pdf stated lines (mirrors src/body.rs)
    const lines = content.split(/\r?\n/);
    const hasCreditLabel = lines.some((l) => /credit-note total/i.test(l));
    const hasOrderLabel = lines.some((l) => /order total whose payment nature is not stated/i.test(l));
    const hasRefundNotStated = lines.some((l) => /Receipt of the refund is not stated/i.test(l));

    for (const line of lines) {
      const trimmed = line.trim();
      const paidM = trimmed.match(/^Amount paid USD (\d+\.\d{2})$/);
      if (paidM) {
        candidates.push({
          kind: "stated_amount_paid",
          usable: true,
          excerpt: line,
          displayExcerpt: line,
          displayVersion: "identity-utf8-scalars-1",
          value: { currency: "USD", amount_text: paidM[1], payment_verified: false },
          locator: locateExcerpt(content, line)
        });
        continue;
      }

      const periodM = trimmed.match(/^Service period:\s*(\d{4}-\d{2}-\d{2}) through (\d{4}-\d{2}-\d{2})$/);
      if (periodM) {
        candidates.push({
          kind: "stated_service_period",
          usable: true,
          excerpt: line,
          displayExcerpt: line,
          displayVersion: "identity-utf8-scalars-1",
          value: { period_start: periodM[1], period_end: periodM[2], time_status: "unknown", current_status: "unknown" },
          locator: locateExcerpt(content, line)
        });
        continue;
      }

      const totalM = trimmed.match(/^Total USD (\d+\.\d{2})$/);
      if (totalM) {
        const semantic = hasCreditLabel
          ? "credit_note_total"
          : hasOrderLabel
          ? "unjudged_order_total"
          : "unresolved_total";
        candidates.push({
          kind: semantic,
          usable: false,
          reason: semantic,
          excerpt: line,
          displayExcerpt: line,
          displayVersion: "identity-utf8-scalars-1",
          value: {
            currency: "USD",
            amount_text: totalM[1],
            refund_received_stated: hasRefundNotStated ? false : null,
            payment_verified: false
          },
          locator: locateExcerpt(content, line)
        });
        continue;
      }

      const usableM = trimmed.match(/^The service remains usable through (\d{4}-\d{2}-\d{2})\.$/);
      if (usableM) {
        candidates.push({
          kind: "cancellation_usable_through",
          usable: false,
          reason: "renewal_cancelled_still_usable",
          excerpt: line,
          displayExcerpt: line,
          displayVersion: "identity-utf8-scalars-1",
          value: { usable_through: usableM[1], auto_renewal_cancelled: true },
          locator: locateExcerpt(content, line)
        });
        continue;
      }

      const scopeM = trimmed.match(/^License scope, original wording:\s*(.+)$/);
      if (scopeM) {
        candidates.push({
          kind: "onetime_license_scope",
          usable: false,
          reason: "verbatim_license_scope",
          excerpt: line,
          displayExcerpt: scopeM[1],
          displayVersion: "identity-utf8-scalars-1",
          value: { verbatim_scope: scopeM[1], purchase_kind: "one-time" },
          locator: locateExcerpt(content, line)
        });
        continue;
      }

      const emailM = trimmed.match(/^(?:Account|Buyer) email:\s*(.+)$/);
      if (emailM) {
        const addr = emailM[1].trim();
        const masked = addr.includes("*");
        candidates.push({
          kind: masked ? "masked_account_email" : "account_email",
          usable: false,
          reason: masked ? "masked_unassigned" : "source_stated_email",
          excerpt: line,
          displayExcerpt: line,
          displayVersion: "identity-utf8-scalars-1",
          value: { email: addr, attributed: false, masked },
          locator: locateExcerpt(content, line)
        });
      }
    }
  }

  // Only unambiguous stated_amount_paid & stated_service_period are adopted as facts
  const adoptable = [];
  for (const factType of ["stated_amount_paid", "stated_service_period"]) {
    const matches = candidates.filter((c) => c.kind === factType && c.usable);
    const distinctKeys = new Set(matches.map((m) => JSON.stringify(m.value)));
    if (distinctKeys.size === 1 && matches.length >= 1) {
      adoptable.push(matches[0]);
    } else if (distinctKeys.size > 1) {
      extractionStatus = "conflict";
      extractionNote = `Conflicting ${factType} statements within the same document; neither auto-adopted.`;
    }
  }

  if (candidates.length === 0 && extractionStatus === "ok") {
    extractionStatus = "no_match";
  }

  return {
    extractionStatus,
    extractionRule,
    extractionNote,
    candidates,
    adoptable,
    metadata: extractCommonMetadata(content)
  };
}

function parseCsvLine(line) {
  const cells = [];
  let cur = "";
  let inQuotes = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (inQuotes) {
      if (ch === '"' && line[i + 1] === '"') {
        cur += '"';
        i++;
      } else if (ch === '"') {
        inQuotes = false;
      } else {
        cur += ch;
      }
    } else if (ch === '"') {
      inQuotes = true;
    } else if (ch === ",") {
      cells.push(cur);
      cur = "";
    } else {
      cur += ch;
    }
  }
  cells.push(cur);
  return cells;
}

function extractCommonMetadata(content) {
  const emailM = content.match(/(?:Account|Buyer) email:\s*([^\s<]+)/i);
  const orderM = content.match(/Order id:\s*([A-Z0-9-]+)/i);
  const docIdM = content.match(/Document id:\s*([A-Z0-9-]+)/i);
  const usableM = content.match(/usable through (\d{4}-\d{2}-\d{2})/i);
  const scopeM = content.match(/License scope, original wording:\s*(.+)/i);
  return {
    email: emailM ? emailM[1] : null,
    orderId: orderM ? orderM[1] : null,
    docId: docIdM ? docIdM[1] : null,
    usableThrough: usableM ? usableM[1] : null,
    licenseScope: scopeM ? scopeM[1].trim() : null
  };
}

// --- SubVault In-Memory Vault Store ---

export class SubVaultStore {
  constructor() {
    this.reset();
  }

  reset() {
    this.vaultFormat = 4;
    this.reviewAnchorDate = REVIEW_ANCHOR_DATE;
    this.objects = new Map();      // sha256 -> object record
    this.sources = [];             // array of source records (ordered)
    this.occurrences = [];         // array of occurrence records
    this.facts = [];               // array of fact records
    this.decisions = [];           // append-only decision chain
    this.activityLog = [];         // human-readable audit log of recent commands
    this.seq = 1;
  }

  nextId(prefix, seed) {
    const id = `${prefix}_${deterministicHex(`${seed}:${this.seq++}`, 12)}`;
    return id;
  }

  ingestDocument(input) {
    const bytes = utf8Bytes(input.content);
    const sha256 = sha256Bytes(bytes);
    const byteLength = bytes.length;
    const commandId = input.commandId || this.nextId("cmd", `${input.displayName}:${Date.now()}`);
    const batchId = input.batchId || "b_interactive";
    const recordedAt = Date.now();

    // Check byte-identical deduplication!
    const existingObj = this.objects.get(sha256);
    if (existingObj) {
      const existingSource = this.sources.find((s) => s.objectId === existingObj.objectId);
      const occ = {
        occurrenceId: this.nextId("occ", `${commandId}:${input.externalPath}`),
        sourceId: existingSource.sourceId,
        objectId: existingObj.objectId,
        commandId,
        batchId,
        displayName: input.displayName,
        externalPath: input.externalPath || input.displayName,
        storageReused: true,
        recordedAt
      };
      this.occurrences.push(occ);
      existingSource.occurrenceIds.push(occ.occurrenceId);

      const entry = {
        type: "duplicate_reused",
        commandId,
        sourceId: existingSource.sourceId,
        objectId: existingObj.objectId,
        sha256,
        displayName: input.displayName,
        originalName: existingSource.displayName,
        message: `Byte-identical duplicate detected: "${input.displayName}" matches "${existingSource.displayName}" (${sha256.slice(0, 12)}…). Reused ${existingObj.objectId}, logged ${occ.occurrenceId}, preserved existing facts & human decisions.`
      };
      this.activityLog.unshift(entry);
      return { outcome: "DuplicateReused", storageReused: true, source: existingSource, occurrence: occ, entry };
    }

    // New unique object bytes
    const objectId = this.nextId("obj", sha256);
    const objectRow = {
      objectId,
      contentSha256: sha256,
      byteLength,
      corpusSha256: input.corpusSha256 || null,
      corpusByteLength: input.corpusByteLength || null,
      relpath: `objects/${objectId}`,
      rawContent: input.content,
      state: "complete"
    };
    this.objects.set(sha256, objectRow);

    const extracted = extractDocument(input.content, input.mediaType || "text/plain;charset=utf-8");
    const inferredMerchant =
      input.merchant || inferMerchant(input.content, input.displayName);
    const inferredEmail =
      input.accountEmail || extracted.metadata.email || "Not stated";
    const inferredAccount =
      input.accountLabel ||
      (inferredEmail.includes("*")
        ? "Unassigned (Masked)"
        : /work/i.test(input.content)
        ? "Work"
        : "Personal");

    const sourceId = this.nextId("src", `${objectId}:${input.displayName}`);
    const occId = this.nextId("occ", `${commandId}:${input.displayName}`);

    const occurrence = {
      occurrenceId: occId,
      sourceId,
      objectId,
      commandId,
      batchId,
      displayName: input.displayName,
      externalPath: input.externalPath || input.displayName,
      storageReused: false,
      recordedAt
    };
    this.occurrences.push(occurrence);

    const source = {
      sourceId,
      objectId,
      contentSha256: sha256,
      byteLength,
      mediaType: input.mediaType || "text/plain;charset=utf-8",
      role: input.role || (input.mediaType?.includes("html") ? "user_provided_html" : "user_provided_fragment"),
      displayName: input.displayName,
      externalPath: input.externalPath || input.displayName,
      merchant: inferredMerchant,
      accountLabel: inferredAccount,
      accountEmail: inferredEmail,
      billingCategory: input.billingCategory || inferCategory(input.content, input.mediaType),
      orderId: input.orderId || extracted.metadata.orderId || extracted.metadata.docId || null,
      scenarioTags: input.scenarioTags || [],
      preservation: "preserved",
      extractionStatus: extracted.extractionStatus,
      extractionRule: extracted.extractionRule,
      extractionNote: extracted.extractionNote,
      candidates: extracted.candidates.map((c, idx) => ({
        candidateId: this.nextId("cand", `${sourceId}:${idx}:${c.kind}`),
        ordinal: idx + 1,
        ...c
      })),
      occurrenceIds: [occId],
      metadata: extracted.metadata
    };
    this.sources.push(source);

    // Adopt unambiguous source-stated facts with initial `rule_adopt` decision
    for (const draft of extracted.adoptable) {
      const matchingCand = source.candidates.find(
        (c) => c.kind === draft.kind && c.displayExcerpt === draft.displayExcerpt
      );
      const factId = this.nextId("fact", `${sourceId}:${draft.kind}`);
      const decisionId = this.nextId("dec", `${factId}:1`);
      const value = {
        ...draft.value,
        statement: "source_stated",
        payment_verified: false,
        current_status: "unknown",
        time_status: "unknown",
        observed_at: null
      };

      const fact = {
        factId,
        sourceId,
        candidateId: matchingCand ? matchingCand.candidateId : null,
        factType: draft.kind,
        revision: 1,
        value,
        originalValue: { ...value }
      };
      this.facts.push(fact);

      this.decisions.push({
        decisionId,
        factId,
        sourceId,
        commandId,
        slot: draft.kind,
        kind: "rule_adopt",
        actorKind: "rule",
        actorId: "act_00000000000000000000000000000001",
        ruleVersion: extracted.extractionRule,
        recordedAt,
        priorValue: null,
        newValue: { ...value },
        targetDecisionId: null
      });
    }

    const entry = {
      type: "committed",
      commandId,
      sourceId,
      objectId,
      sha256,
      displayName: input.displayName,
      message: `Preserved "${input.displayName}" as ${objectId} (${byteLength} B, SHA-256 ${sha256.slice(0, 12)}…) · ${extracted.adoptable.length} stated fact(s) adopted.`
    };
    this.activityLog.unshift(entry);
    return { outcome: "Committed", storageReused: false, source, occurrence, entry };
  }

  loadDemoCorpus(batchTag = "b_demo_initial") {
    let added = 0;
    let reused = 0;
    const results = [];
    for (const doc of DEMO_BATCH_DOCS) {
      const res = this.ingestDocument({
        ...doc,
        commandId: `${batchTag}_${doc.displayName}`,
        batchId: batchTag
      });
      if (res.storageReused) reused++;
      else added++;
      results.push(res);
    }
    return { added, reused, total: results.length, results };
  }

  correctAmount(factId, newAmountText, actorId = "act_human_reviewer") {
    const clean = String(newAmountText).trim();
    if (!isValidUsdAmount(clean)) {
      throw new Error("Amount must be USD decimal format (e.g. 107.91)");
    }
    const fact = this.facts.find((f) => f.factId === factId && f.factType === "stated_amount_paid");
    if (!fact) throw new Error("Amount fact not found");
    if (fact.value.amount_text === clean) {
      throw new Error(`Amount is already USD ${clean}`);
    }

    const priorValue = { ...fact.value };
    const newValue = { ...fact.value, amount_text: clean, statement: "human_corrected" };
    fact.revision += 1;
    fact.value = newValue;

    const decision = {
      decisionId: this.nextId("dec", `${factId}:rev${fact.revision}`),
      factId,
      sourceId: fact.sourceId,
      commandId: this.nextId("cmd", `correct:${factId}:${fact.revision}`),
      slot: fact.factType,
      kind: "human_correct",
      actorKind: "human",
      actorId,
      ruleVersion: null,
      recordedAt: Date.now(),
      priorValue,
      newValue: { ...newValue },
      targetDecisionId: null
    };
    this.decisions.push(decision);

    this.activityLog.unshift({
      type: "human_correct",
      commandId: decision.commandId,
      sourceId: fact.sourceId,
      message: `Human correction on ${fact.factId}: USD ${priorValue.amount_text} → USD ${clean} (original bytes untouched; still states USD ${fact.originalValue.amount_text}).`
    });
    return { fact, decision };
  }

  undoLastDecision(factId, actorId = "act_human_reviewer") {
    const fact = this.facts.find((f) => f.factId === factId);
    if (!fact) throw new Error("Fact not found");
    const factDecisions = this.getDecisionsForFact(factId);
    const latest = factDecisions[factDecisions.length - 1];
    if (!latest || latest.kind === "rule_adopt") {
      throw new Error("No human decision to undo");
    }
    const priorValue = { ...fact.value };
    const restored = { ...latest.priorValue };
    fact.revision += 1;
    fact.value = restored;

    const decision = {
      decisionId: this.nextId("dec", `${factId}:undo:${fact.revision}`),
      factId,
      sourceId: fact.sourceId,
      commandId: this.nextId("cmd", `undo:${factId}:${fact.revision}`),
      slot: fact.factType,
      kind: "undo",
      actorKind: "human",
      actorId,
      ruleVersion: null,
      recordedAt: Date.now(),
      priorValue,
      newValue: { ...restored },
      targetDecisionId: latest.decisionId
    };
    this.decisions.push(decision);

    this.activityLog.unshift({
      type: "undo",
      commandId: decision.commandId,
      sourceId: fact.sourceId,
      message: `Undid ${latest.decisionId} on ${fact.factId}: restored USD ${restored.amount_text} via compensating decision ${decision.decisionId}.`
    });
    return { fact, decision };
  }

  getFactsForSource(sourceId) {
    return this.facts.filter((f) => f.sourceId === sourceId);
  }

  getDecisionsForFact(factId) {
    return this.decisions.filter((d) => d.factId === factId);
  }

  getOccurrencesForSource(sourceId) {
    return this.occurrences.filter((o) => o.sourceId === sourceId);
  }

  getObjectForSource(source) {
    return this.objects.get(source.contentSha256);
  }

  // --- Forensic Audit & Conflict Detection ---

  getAuditReport() {
    const duplicates = this.sources
      .map((source) => ({
        source,
        occurrences: this.getOccurrencesForSource(source.sourceId)
      }))
      .filter((item) => item.occurrences.length > 1);

    // Cross-source amount conflicts (same orderId, differing CSV charged vs Invoice paid)
    const conflicts = [];
    const byOrder = new Map();
    for (const s of this.sources) {
      if (!s.orderId) continue;
      if (!byOrder.has(s.orderId)) byOrder.set(s.orderId, []);
      byOrder.get(s.orderId).push(s);
    }
    for (const [orderId, group] of byOrder.entries()) {
      const paidSources = group
        .map((s) => ({
          source: s,
          fact: this.getFactsForSource(s.sourceId).find((f) => f.factType === "stated_amount_paid")
        }))
        .filter((x) => x.fact);
      const csvSources = group
        .map((s) => ({
          source: s,
          cand: s.candidates.find((c) => c.kind === "csv_charged_amount")
        }))
        .filter((x) => x.cand);

      for (const p of paidSources) {
        for (const c of csvSources) {
          if (p.fact.value.amount_text !== c.cand.value.amount_text) {
            conflicts.push({
              id: `conflict_${orderId}`,
              orderId,
              merchant: p.source.merchant,
              accountEmail: p.source.accountEmail,
              invoiceSource: p.source,
              invoiceAmount: p.fact.value.amount_text,
              csvSource: c.source,
              csvAmount: c.cand.value.amount_text,
              note: `Invoice states Amount paid USD ${p.fact.value.amount_text} while store CSV charged cell says USD ${c.cand.value.amount_text}. SubVault refuses to pick a winner automatically.`
            });
          }
        }
      }
    }

    // Masked unassigned accounts
    const maskedUnassigned = this.sources.filter((s) =>
      s.candidates.some((c) => c.kind === "masked_account_email")
    );

    // Non-payment totals (credit notes, order confirmations)
    const nonPaymentTotals = this.sources.filter((s) =>
      s.candidates.some((c) => c.kind === "credit_note_total" || c.kind === "unjudged_order_total")
    );

    // Multi-account same merchant separation proof
    const vendorAccounts = new Map();
    for (const s of this.sources) {
      if (!s.accountEmail || s.accountEmail.includes("*") || s.accountEmail === "Not stated") continue;
      if (!vendorAccounts.has(s.merchant)) vendorAccounts.set(s.merchant, new Set());
      vendorAccounts.get(s.merchant).add(s.accountEmail);
    }
    const separatedAccounts = [...vendorAccounts.entries()]
      .filter(([, emails]) => emails.size > 1)
      .map(([merchant, emails]) => ({ merchant, emails: [...emails] }));

    return {
      duplicates,
      conflicts,
      maskedUnassigned,
      nonPaymentTotals,
      separatedAccounts
    };
  }

  getSummaryStats() {
    const audit = this.getAuditReport();
    const uniqueObjects = this.objects.size;
    const totalOccurrences = this.occurrences.length;
    const duplicateHits = this.occurrences.filter((o) => o.storageReused).length;
    const paidFacts = this.facts.filter((f) => f.factType === "stated_amount_paid");
    const statedUsdTotal = paidFacts.reduce((sum, f) => sum + parseFloat(f.value.amount_text || "0"), 0);
    const humanCorrections = this.decisions.filter((d) => d.kind === "human_correct").length;
    const needsHumanCall = audit.conflicts.length + audit.maskedUnassigned.length;

    return {
      uniqueObjects,
      totalSources: this.sources.length,
      totalOccurrences,
      duplicateHits,
      paidFactCount: paidFacts.length,
      statedUsdTotal: statedUsdTotal.toFixed(2),
      humanCorrections,
      needsHumanCall,
      conflictCount: audit.conflicts.length,
      maskedCount: audit.maskedUnassigned.length
    };
  }

  // --- Read-Only Review Sheet (mirrors src/review.rs) ---

  renderReviewSheet() {
    const lines = [
      "SUBVAULT READ-ONLY REVIEW SHEET (DEMO SNAPSHOT)",
      `Review anchor date: ${this.reviewAnchorDate}`,
      "Epistemic contract: Source-stated figures are NOT verified payments; ended periods do NOT imply current expiration.",
      "Original HTML bytes are shown as text only — never rendered or fetched.",
      "=".repeat(78),
      "CURRENT CONCLUSIONS (GROUPED BY SOURCE)",
      ""
    ];

    if (this.sources.length === 0) {
      lines.push("No sources in vault. Import a sample batch or drop local files to begin.");
    } else {
      for (const s of this.sources) {
        const facts = this.getFactsForSource(s.sourceId);
        const occs = this.getOccurrencesForSource(s.sourceId);
        lines.push(`[${s.sourceId}] ${s.merchant} — ${s.accountLabel} (${s.accountEmail})`);
        lines.push(`  Object: originals/${s.objectId} | SHA-256: ${s.contentSha256} | ${s.byteLength} bytes`);
        lines.push(`  Occurrences (${occs.length}): ${occs.map((o) => o.externalPath + (o.storageReused ? " [REUSED]" : "")).join(" ; ")}`);

        const amtFact = facts.find((f) => f.factType === "stated_amount_paid");
        const perFact = facts.find((f) => f.factType === "stated_service_period");

        if (amtFact) {
          const orig = amtFact.originalValue.amount_text;
          const curr = amtFact.value.amount_text;
          if (orig !== curr) {
            lines.push(`  Current amount: USD ${curr} (HUMAN CORRECTED — original document still states USD ${orig}; rev ${amtFact.revision})`);
          } else {
            lines.push(`  Current amount: USD ${curr} (matches original document; payment_verified=false)`);
          }
        } else {
          lines.push("  Current amount: (none adopted — see un-adopted candidates below)");
        }

        if (perFact) {
          lines.push(`  Service period: ${perFact.value.period_start} through ${perFact.value.period_end} (current_status=unknown)`);
        } else if (s.metadata?.usableThrough) {
          lines.push(`  Cancellation note: Auto-renewal cancelled; usable through ${s.metadata.usableThrough}`);
        } else if (s.metadata?.licenseScope) {
          lines.push(`  One-time license scope: ${s.metadata.licenseScope}`);
        }

        const nonAdopted = s.candidates.filter(
          (c) => !facts.some((f) => f.candidateId === c.candidateId)
        );
        if (nonAdopted.length > 0) {
          lines.push(`  Non-fact candidates (${nonAdopted.length}): ${nonAdopted.map((c) => `${c.kind}("${c.displayExcerpt}")`).join(" | ")}`);
        }
        lines.push("");
      }
    }

    lines.push("-".repeat(78));
    lines.push("VERIFICATION NOTES");
    lines.push("* Payment verification: unverified (source statements only).");
    lines.push("* Distinct sources are never silently merged into one payment or purchase.");
    lines.push("* Package originals verified by SHA-256 inside offline exit manifest.");
    return lines.join("\n");
  }

  // --- Verifiable Offline Exit Package (mirrors src/export.rs) ---

  buildExitPackage() {
    const recordsObj = {
      package_version: "offline-exit-1",
      vault_format: this.vaultFormat,
      completeness: "complete",
      review_anchor_date: this.reviewAnchorDate,
      field_notes: {
        payment_verified: "false on all rule-adopted facts; receipts are source statements, not bank settlement proof.",
        html_original: "HTML originals are stored as static text bytes; external references are never fetched."
      },
      sources: this.sources.map((s) => ({
        source_id: s.sourceId,
        object_id: s.objectId,
        content_sha256: s.contentSha256,
        byte_length: s.byteLength,
        media_type: s.mediaType,
        role: s.role,
        merchant: s.merchant,
        account_email: s.accountEmail,
        preservation: s.preservation,
        occurrences: this.getOccurrencesForSource(s.sourceId).map((o) => ({
          occurrence_id: o.occurrenceId,
          display_name: o.displayName,
          external_path: o.externalPath,
          storage_reused: o.storageReused
        }))
      })),
      facts: this.facts.map((f) => ({
        fact_id: f.factId,
        source_id: f.sourceId,
        candidate_id: f.candidateId,
        fact_type: f.factType,
        revision: f.revision,
        value: f.value,
        decision_ids: this.getDecisionsForFact(f.factId).map((d) => d.decisionId)
      })),
      decisions: this.decisions.map((d) => ({
        decision_id: d.decisionId,
        fact_id: d.factId,
        kind: d.kind,
        actor_kind: d.actorKind,
        prior_value: d.priorValue,
        new_value: d.newValue,
        target_decision_id: d.targetDecisionId
      }))
    };

    const recordsJson = JSON.stringify(recordsObj, null, 2);
    const snapshotId = sha256Text(recordsJson);

    const csvHeader = [
      "fact_id",
      "source_id",
      "merchant",
      "account_email",
      "fact_type",
      "revision",
      "currency",
      "amount_text",
      "period_start",
      "period_end",
      "payment_verified",
      "current_status",
      "latest_decision_id"
    ].join(",");

    const csvRows = this.facts.map((f) => {
      const s = this.sources.find((src) => src.sourceId === f.sourceId);
      const decs = this.getDecisionsForFact(f.factId);
      const latestDec = decs.length ? decs[decs.length - 1].decisionId : "";
      const cols = [
        f.factId,
        f.sourceId,
        s?.merchant || "",
        s?.accountEmail || "",
        f.factType,
        String(f.revision),
        f.value.currency || "",
        f.value.amount_text || "",
        f.value.period_start || "",
        f.value.period_end || "",
        String(Boolean(f.value.payment_verified)),
        f.value.current_status || "unknown",
        latestDec
      ];
      return cols.map(csvEscape).join(",");
    });
    const factsCsv = [csvHeader, ...csvRows].join("\n") + "\n";

    const reviewTxt = this.renderReviewSheet();

    const files = [
      { path: "records.json", sha256: snapshotId, byte_length: utf8Bytes(recordsJson).length },
      { path: "facts.csv", sha256: sha256Text(factsCsv), byte_length: utf8Bytes(factsCsv).length },
      { path: "review.txt", sha256: sha256Text(reviewTxt), byte_length: utf8Bytes(reviewTxt).length },
      ...[...this.objects.values()].map((o) => ({
        path: `originals/${o.objectId}`,
        sha256: o.contentSha256,
        byte_length: o.byteLength
      }))
    ].sort((a, b) => a.path.localeCompare(b.path));

    const manifestObj = {
      package_version: "offline-exit-1",
      completeness: "complete",
      snapshot_id: snapshotId,
      vault_format: this.vaultFormat,
      counts: {
        sources: this.sources.length,
        objects: this.objects.size,
        occurrences: this.occurrences.length,
        facts: this.facts.length,
        decisions: this.decisions.length
      },
      files
    };
    const manifestJson = JSON.stringify(manifestObj, null, 2);

    return {
      snapshotId,
      recordsJson,
      factsCsv,
      manifestJson,
      reviewTxt,
      manifest: manifestObj
    };
  }
}

function csvEscape(val) {
  const s = String(val ?? "");
  if (/[",\n\r]/.test(s)) {
    return `"${s.replace(/"/g, '""')}"`;
  }
  return s;
}

function inferMerchant(content, filename) {
  const merchantM = content.match(/^Merchant:\s*(.+)$/m);
  if (merchantM) return merchantM[1].trim();
  const h2M = content.match(/<h[1-3][^>]*>([^<]+)<\/h[1-3]>/i);
  if (h2M) return h2M[1].replace(/\s*(Billing Statement|Renewal Receipt|Invoice).*/i, "").trim();
  const firstLine = content.split(/\r?\n/).find((l) => l.trim().length > 0 && !l.trim().startsWith("<"));
  if (firstLine && firstLine.length < 48 && !firstLine.includes(",")) return firstLine.trim();
  return filename.replace(/\.[^.]+$/, "");
}

function inferCategory(content, mediaType) {
  if (/credit note/i.test(content)) return "credit_note";
  if (/cancellation notice/i.test(content)) return "cancellation";
  if (/order confirmation/i.test(content)) return "order_confirm";
  if (/one-time/i.test(content)) return "one_time";
  if (mediaType?.includes("csv")) return "csv_export";
  return "monthly";
}
