#!/usr/bin/env node
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { QUICK_SAMPLE_SCENARIOS } from "../web/demo-data.js";
import { SubVaultStore, sha256Text } from "../web/engine.js";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const webDir = path.join(root, "web");

// 1. Verify pure-JS SHA-256 matches Node crypto across ASCII & Unicode
for (const sample of ["", "SubVault", "café\r\nAmount paid USD 15.99\n💰"]) {
  const expected = crypto.createHash("sha256").update(sample, "utf8").digest("hex");
  assert.equal(sha256Text(sample), expected, `SHA-256 mismatch for ${JSON.stringify(sample)}`);
}

// 2. Verify preloaded demo corpus ingestion & duplicate detection
const store = new SubVaultStore();
const firstLoad = store.loadDemoCorpus("b_test_1");
assert.equal(firstLoad.total, 12);
assert.equal(firstLoad.added, 11, "12 files should collapse into 11 unique SHA-256 objects");
assert.equal(firstLoad.reused, 1, "renamed September receipt copy must be deduplicated");

const stats = store.getSummaryStats();
assert.equal(stats.uniqueObjects, 11);
assert.equal(stats.totalOccurrences, 12);
assert.equal(stats.duplicateHits, 1);

const audit = store.getAuditReport();
assert.equal(audit.duplicates.length, 1);
assert.equal(audit.conflicts.length, 1, "must flag $99.00 CSV vs $107.91 invoice conflict");
assert.equal(audit.conflicts[0].orderId, "SYN-C-WORK-2026-031");
assert.equal(audit.maskedUnassigned.length, 1, "must keep masked j*** email unassigned");
assert.equal(audit.separatedAccounts.length, 1, "Paperlantern Cloud personal & work accounts must stay separate");
assert.equal(audit.separatedAccounts[0].merchant, "Paperlantern Cloud");

// 3. Verify Format 4 HTML table extraction & CSS safety boundary
const htmlSource = store.sources.find((s) => s.displayName === "northline-2026-11-email.html");
assert.ok(htmlSource, "HTML bill must be in demo corpus");
const htmlFacts = store.getFactsForSource(htmlSource.sourceId);
assert.equal(htmlFacts.length, 2, "HTML bill must adopt stated_amount_paid and stated_service_period");

const styledSample = QUICK_SAMPLE_SCENARIOS.find((s) => s.id === "html-css-unsupported");
const styledRes = store.ingestDocument({ ...styledSample.doc, commandId: "cmd_styled" });
assert.equal(styledRes.source.preservation, "preserved");
assert.equal(styledRes.source.extractionStatus, "unsupported");
assert.equal(store.getFactsForSource(styledRes.source.sourceId).length, 0);

// 4. Verify human correction & undo survive re-import without duplicating objects
const sepSource = store.sources.find((s) => s.displayName === "v-2026-09-receipt.pdf");
const amtFact = store.getFactsForSource(sepSource.sourceId).find((f) => f.factType === "stated_amount_paid");
assert.equal(amtFact.value.amount_text, "15.99");

store.correctAmount(amtFact.factId, "14.99");
assert.equal(amtFact.value.amount_text, "14.99");
assert.equal(amtFact.originalValue.amount_text, "15.99", "original value must remain untouched");

const secondLoad = store.loadDemoCorpus("b_test_2");
assert.equal(secondLoad.added, 0, "re-import must create zero new objects");
assert.equal(secondLoad.reused, 12, "all 12 files must reuse existing SHA-256 objects");
assert.equal(amtFact.value.amount_text, "14.99", "human correction must survive re-import");

store.undoLastDecision(amtFact.factId);
assert.equal(amtFact.value.amount_text, "15.99", "undo must restore previous value");
assert.equal(store.getDecisionsForFact(amtFact.factId).length, 3, "rule_adopt + human_correct + undo");

// 5. Verify offline exit package integrity
const pkg = store.buildExitPackage();
assert.equal(pkg.manifest.completeness, "complete");
assert.equal(pkg.snapshotId, sha256Text(pkg.recordsJson));
for (const entry of pkg.manifest.files) {
  if (entry.path === "records.json") assert.equal(entry.sha256, sha256Text(pkg.recordsJson));
  if (entry.path === "facts.csv") assert.equal(entry.sha256, sha256Text(pkg.factsCsv));
  if (entry.path === "review.txt") assert.equal(entry.sha256, sha256Text(pkg.reviewTxt));
}
assert.ok(pkg.reviewTxt.includes("CURRENT CONCLUSIONS"));
assert.ok(pkg.factsCsv.startsWith("fact_id,source_id,"));

// 6. Verify static hosting & subpath safety in web/
assert.ok(fs.existsSync(path.join(webDir, ".nojekyll")), "web/.nojekyll must exist for GitHub Pages");
const html = fs.readFileSync(path.join(webDir, "index.html"), "utf8");
assert.ok(!/(?:src|href)\s*=\s*["']\//.test(html), "index.html must not contain root-absolute / paths");
assert.ok(!/(?:src|href)\s*=\s*["']https?:\/\//.test(html), "index.html must not depend on external CDN URLs");

for (const match of html.matchAll(/(?:src|href)\s*=\s*["'](\.\/[^"']+)["']/g)) {
  const relPath = match[1];
  const full = path.join(webDir, relPath);
  assert.ok(fs.existsSync(full), `Missing relative static asset referenced in index.html: ${relPath}`);
}

const css = fs.readFileSync(path.join(webDir, "styles.css"), "utf8");
assert.ok(!/url\(\s*['"]?\//.test(css), "styles.css must not contain root-absolute url(/...)");

console.log("web smoke & subpath tests passed (all assertions green)");
