# SubVault v0.1.0 Release Notes (Draft — Pre-Release)

> **Status:** Draft notes for the upcoming first usable **`v0.1.0`** release. SubVault stays on `0.1.0` (unreleased) while remaining real-data milestones in [`docs/ROADMAP.md`](./docs/ROADMAP.md) (Windows native wiring, at-rest encryption, desktop shell) are completed.

**Target tag:** `v0.1.0`  
**Live Demo:** <https://yanagi42.github.io/SubVault/>

---

## Highlights Included So Far

1. **Verified Rust + SQLite Preservation & Extraction Core (`subvault` CLI, Formats 1–5)**
   - **Immutable Byte Custody (T1):** Content-addressed `objects/obj_<id>` storage with SHA-256 integrity verification, zero byte normalization, idempotent `CommandId` journaling, exclusive non-blocking `flock`, SQLite 3.46 WAL (`synchronous=FULL`), atomic directory publication via `renameat2(RENAME_NOREPLACE)` + parent fd `fsync`, and 17 distinct honest exit codes.
   - **Native Text-PDF & Static HTML Extraction (Formats 2–5):**
     - Format 5 (`subvault init --format 5`) accepts **both** `subvault pdf` (uncompressed and `/FlateDecode` via `miniz_oxide =0.8.8`, bounded memory, 1-based `pdf_page` locators) and `subvault html` (`tl` 0.7.8, refusing `<style>`/stylesheets instead of guessing CSS visibility).
     - Preserves raw bytes verbatim on encrypted PDFs (`encrypted_pdf`), image-only PDFs (`no_extractable_text`, `ocr=not_available`), and CSS-styled HTML; rejects fake renamed `.pdf` text files before preservation (`invalid_pdf_header`).
   - **Append-Only Human Decisions:** `correct` and `undo` append auditable human revisions while keeping original bytes untouched and surviving byte-identical re-imports (`AlreadyPreserved`).
   - **Offline Exit, Standalone Verification & Read-Only Review Sheet (T5):** `subvault export` produces self-contained `offline-exit-1` packages (`records.json`, `facts.csv`, `manifest.json`, `originals/<object_id>`); `subvault verify-package --package PATH` independently verifies the exported package without needing the live vault; `subvault review` writes a lock-consistent plain-text review sheet linked only to hash-verified package originals.

2. **Interactive Pure-Static Web Demo (`web/`) — Real Binary PDF Support + Native CLI Bridge**
   - **Preloaded Synthetic Corpus:** Ships out-of-the-box with 12 realistic billing scenarios covering monthly/annual plans, personal vs. work account separation, SHA-256 duplicate reuse, CSV vs. invoice conflicts (`$99.00` vs. `$107.91`), credit notes, masked cancellation notices, one-time perpetual licenses, and static HTML bills.
   - **Real Binary `.pdf` Drop (`web/pdf.js`) & Full `.zip` Export (`web/zip.js`):** Dropping local `.pdf`/`.html`/`.csv`/`.txt` files hashes exact raw bytes locally; **Download Full Exit Pack (.zip)** exports an archive directly verifiable by `subvault verify-package`; **Open Native Export** loads and SHA-256-verifies real native `subvault export` folders in-browser.

3. **CI, Automated Packaging & Verification**
   - `.github/workflows/ci.yml`, `.github/workflows/pages.yml`, `.github/workflows/release.yml`, and `scripts/package-release.sh` with `SHA256SUMS.txt`.

---

## Portable Bundle Assets (`scripts/package-release.sh`)

| Asset | Contents |
| --- | --- |
| `subvault-v0.1.0-linux-x86_64.zip` | Portable Linux x86_64 `subvault` CLI binary (Formats 1–5), static Web Demo (`web/`), verified T0 synthetic corpus, `run-demo.sh`, and documentation |
| `subvault-v0.1.0-linux-x86_64.tar.gz` | Same portable bundle in `.tar.gz` format |
| `subvault-v0.1.0-web-demo.zip` | Pure-static Web Demo files ready for any static host or offline folder |
| `SHA256SUMS.txt` | SHA-256 checksums for all archives |

### Verify & Run Portable Bundle

```bash
sha256sum -c SHA256SUMS.txt
unzip subvault-v0.1.0-linux-x86_64.zip
cd subvault-v0.1.0-linux-x86_64

# Run Format 5 PDF + duplicate reuse + HTML + export + verify-package + review demo
./run-demo.sh cli

# Or launch the local pure-static Web Demo at http://127.0.0.1:4173
./run-demo.sh web
```

---

## Remaining Scope Before Publishing `v0.1.0`

- **Windows Native Runtime & CI:** Wire `src/windows_lock.rs` and `src/windows_file_id.rs` and verify on `windows-latest`.
- **At-Rest Protection & Removing `SYNTHETIC_ONLY`:** Finalize encryption and key recovery for real personal billing data.
- **Thin Desktop Shell:** Wrap the verified Rust/SQLite core for everyday local use.
