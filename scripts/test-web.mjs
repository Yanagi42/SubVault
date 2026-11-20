#!/usr/bin/env node
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
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

// 3. Verify masked account human attribution & undo
const maskedSrc = audit.maskedUnassigned[0];
store.assignMaskedAccount(maskedSrc.sourceId, "jordan.hale.example@paperlantern.example", "Work (Attributed)");
assert.equal(store.getAuditReport().maskedUnassigned.length, 0, "attributed notice leaves unassigned queue");
assert.equal(maskedSrc.originalAccountEmail, "j***@paperlantern.example", "original masked email stays untouched");
store.undoAccountAssignment(maskedSrc.sourceId);
assert.equal(store.getAuditReport().maskedUnassigned.length, 1, "undo restores unassigned masked state");

// 4. Verify Format 4 HTML table extraction & CSS safety boundary
const htmlSource = store.sources.find((s) => s.displayName === "northline-2026-11-email.html");
assert.ok(htmlSource, "HTML bill must be in demo corpus");
const htmlFacts = store.getFactsForSource(htmlSource.sourceId);
assert.equal(htmlFacts.length, 2, "HTML bill must adopt stated_amount_paid and stated_service_period");

const styledSample = QUICK_SAMPLE_SCENARIOS.find((s) => s.id === "html-css-unsupported");
const styledRes = store.ingestDocument({ ...styledSample.doc, commandId: "cmd_styled" });
assert.equal(styledRes.source.preservation, "preserved");
assert.equal(styledRes.source.extractionStatus, "unsupported");
assert.equal(store.getFactsForSource(styledRes.source.sourceId).length, 0);

// 5. Verify human correction & undo survive re-import without duplicating objects
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

// 6. Verify snapshot serialization round-trip (localStorage contract)
const cloned = new SubVaultStore();
assert.equal(cloned.fromSnapshot(JSON.parse(JSON.stringify(store.toSnapshot()))), true);
assert.equal(cloned.sources.length, store.sources.length);
assert.equal(cloned.objects.size, store.objects.size);

store.undoLastDecision(amtFact.factId);
assert.equal(amtFact.value.amount_text, "15.99", "undo must restore previous value");
assert.equal(store.getDecisionsForFact(amtFact.factId).length, 3, "rule_adopt + human_correct + undo");

// 7. Verify offline exit package integrity & pure-JS ZIP build
const pkg = store.buildExitPackage();
assert.equal(pkg.manifest.completeness, "complete");
assert.equal(pkg.snapshotId, sha256Text(pkg.recordsJson));
for (const entry of pkg.manifest.files) {
  if (entry.path === "records.json") assert.equal(entry.sha256, sha256Text(pkg.recordsJson));
  if (entry.path === "facts.csv") assert.equal(entry.sha256, sha256Text(pkg.factsCsv));
  if (entry.path === "review.txt") assert.equal(entry.sha256, sha256Text(pkg.reviewTxt));
}
assert.ok(pkg.reviewTxt.includes("CURRENT CONCLUSIONS"));
assert.ok(
  pkg.factsCsv.startsWith(
    "fact_id,source_id,candidate_id,fact_type,revision,statement,currency,amount_text,period_start,period_end,latest_decision_id"
  ),
  "facts.csv header must match native Rust exporter contract"
);

// Verify native export loader round-trip (clean + tamper detection)
const fileMap = new Map([
  ["export-pack/manifest.json", pkg.manifestJson],
  ["export-pack/records.json", pkg.recordsJson],
  ["export-pack/facts.csv", pkg.factsCsv],
  ["export-pack/review.txt", pkg.reviewTxt],
  ...pkg.originals.map((o) => [`export-pack/${o.path}`, o.bytes])
]);
const loadedNative = new SubVaultStore();
const verdict = loadedNative.loadNativeExitPackage(fileMap);
assert.equal(verdict.verified, true, `native export loader must verify clean package: ${JSON.stringify(verdict)}`);
assert.equal(verdict.defects.length, 0);
assert.equal(loadedNative.sources.length, store.sources.length);

// Flip one byte in records.json -> must detect defect
const badMap = new Map(fileMap);
badMap.set("export-pack/records.json", pkg.recordsJson + " ");
const badVerdict = new SubVaultStore().loadNativeExitPackage(badMap);
assert.equal(badVerdict.verified, false, "tampered records.json must fail verification");
assert.ok(badVerdict.defects.length >= 1);

// 7b. Verify real binary %PDF-1.4 ingestion & exact SHA-256 deduplication in Web engine
const pdfStore = new SubVaultStore();
const sepPdfPath = path.join(root, "synthetic/corpus/originals/v-2026-09-receipt.pdf");
const dupPdfPath = path.join(root, "synthetic/corpus/originals/renamed/NLV-september-statement-copy.pdf");
const sepPdfRes = await pdfStore.ingestBinaryFile({
  displayName: "v-2026-09-receipt.pdf",
  externalPath: "originals/v-2026-09-receipt.pdf",
  mediaType: "application/pdf",
  bytes: fs.readFileSync(sepPdfPath),
  batchId: "b_pdf_bin"
});
assert.equal(
  sepPdfRes.source.contentSha256,
  "c69d395d4e1abf23a82ef91c5b50220bdb821c4de53daf03a1f060d49acec8f3",
  "raw binary PDF SHA-256 must match synthetic/corpus/byte-expectations.json bit-for-bit"
);
assert.equal(sepPdfRes.source.byteLength, 941);
assert.equal(pdfStore.getFactsForSource(sepPdfRes.source.sourceId).length, 2);

const dupPdfRes = await pdfStore.ingestBinaryFile({
  displayName: "NLV-september-statement-copy.pdf",
  externalPath: "originals/renamed/NLV-september-statement-copy.pdf",
  mediaType: "application/pdf",
  bytes: fs.readFileSync(dupPdfPath),
  batchId: "b_pdf_bin_2"
});
assert.equal(dupPdfRes.storageReused, true, "renamed binary PDF copy must deduplicate via SHA-256");
assert.equal(pdfStore.objects.size, 1);

// 8. Bidirectional Rust CLI <-> Web Demo interop verification (when native binary exists)
const nativeBin = [
  path.join(root, "target/release/subvault"),
  path.join(root, "target/debug/subvault")
].find((p) => fs.existsSync(p));

if (nativeBin) {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "subvault-interop-"));
  try {
    // A) Web-exported ZIP (including real binary PDF!) -> unzip -> native `subvault verify-package` must pass!
    const { zipBytes } = pdfStore.buildExitZip();
    const zipPath = path.join(tmpDir, "web-exit.zip");
    const unzipDir = path.join(tmpDir, "web-unzipped");
    fs.writeFileSync(zipPath, zipBytes);
    fs.mkdirSync(unzipDir);
    execFileSync("python3", ["-m", "zipfile", "-e", zipPath, unzipDir]);
    const verifyOut = JSON.parse(
      execFileSync(nativeBin, ["verify-package", "--package", unzipDir], { encoding: "utf8" })
    );
    assert.equal(verifyOut.outcome, "Verified", `Web ZIP failed native verify-package: ${JSON.stringify(verifyOut)}`);
    assert.equal(verifyOut.facts_csv_checked, true);

    // B) Native Format 5 `subvault pdf` + `subvault html` -> `subvault export` -> Web `loadNativeExitPackage`!
    const vaultDir = path.join(tmpDir, "vault5");
    const billPath = path.join(tmpDir, "bill.html");
    const nativePack = path.join(tmpDir, "native-export-5");
    fs.writeFileSync(
      billPath,
      "<!doctype html><html><body><table><tr><th>Amount paid</th><td>USD 19.50</td></tr><tr><th>Service period:</th><td>2026-11-01 through 2026-11-30</td></tr></table></body></html>\n"
    );
    execFileSync(nativeBin, ["init", "--vault", vaultDir, "--format", "5"]);
    execFileSync(nativeBin, ["pdf", "--vault", vaultDir, "--command-id", "c-pdf", "--source", sepPdfPath]);
    execFileSync(nativeBin, ["pdf", "--vault", vaultDir, "--command-id", "c-pdf-dup", "--source", dupPdfPath]);
    execFileSync(nativeBin, ["html", "--vault", vaultDir, "--command-id", "c-html", "--source", billPath]);
    execFileSync(nativeBin, ["export", "--vault", vaultDir, "--out", nativePack]);

    const nativeVerify = JSON.parse(
      execFileSync(nativeBin, ["verify-package", "--package", nativePack], { encoding: "utf8" })
    );
    assert.equal(nativeVerify.outcome, "Verified");
    assert.equal(nativeVerify.vault_format, 5);
    assert.equal(nativeVerify.verified_originals, 2);

    const nativeFiles = new Map();
    const walk = (dir) => {
      for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        const full = path.join(dir, entry.name);
        if (entry.isDirectory()) walk(full);
        else nativeFiles.set(path.relative(tmpDir, full).replace(/\\/g, "/"), fs.readFileSync(full));
      }
    };
    walk(nativePack);
    const interopStore = new SubVaultStore();
    const nativeVerdict = interopStore.loadNativeExitPackage(nativeFiles);
    assert.equal(nativeVerdict.verified, true, `Native Format 5 export failed Web verifier: ${JSON.stringify(nativeVerdict)}`);
    assert.equal(interopStore.sources.length, 2);
    assert.equal(interopStore.facts.length, 4);
    assert.equal(interopStore.getAuditReport().duplicates.length, 1);
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
}

// 9. Verify static hosting & subpath safety in web/
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

console.log(
  `web smoke, subpath & ${nativeBin ? "bidirectional Rust-CLI<->Web interop " : ""}tests passed (all assertions green)`
);
