# SubVault v0.1.0 Release Notes

> **Status:** Ready for `v0.1.0` — all five milestones in [`docs/ROADMAP.md`](./docs/ROADMAP.md) (encrypted Format 6 core, bilingual real-world PDF/HTML/CSV extraction, and zero-install local graphical workbench `subvault-app`) are implemented and verified across Windows and Linux.

**Target tag:** `v0.1.0`

---

## Highlights

1. **Zero-Install Single-Binary Local Graphical Workbench (`subvault-app` / `subvault app`)**
   - Double-click `subvault-app.exe` (Windows) or `./subvault-app` (Linux) to open a local workspace (`127.0.0.1` loopback-only, 256-bit `X-SubVault-Token` header auth, strict `Host`/`Origin`/`Sec-Fetch-Site` guards, sandboxed HTML preview, and in-memory `Zeroizing` encryption keys). Note: `subvault-app`'s UI is currently Simplified Chinese; the `subvault` CLI and static Web Demo are English.
   - Drag and drop individual files or entire folders of PDFs, styled HTML emails, Alipay/WeChat CSVs, or receipt screenshots; view per-currency spend totals without fabricated exchange rates or guessed Active/Expired states; inspect original PDFs/images/sandboxed HTML inline; edit or manually record scanned bills with full undo history; export Excel-ready formula-safe UTF-8 BOM `.csv`, verifiable `.zip` offline exit packs (built 100% in memory with zero temp-dir plaintext), or redacted templates in one click.

2. **Verified Rust + SQLite Preservation, Encryption & Bilingual Extraction Core (`subvault` CLI, Formats 1–6)**
   - **Bilingual Real-World Bill Extraction (M5):** Pure-Rust PDF parser supporting `/ObjStm`, `/XRef` streams, `/FlateDecode` PNG predictors, `/ToUnicode` CMaps, `WinAnsiEncoding`/`MacRomanEncoding`, Form XObjects (`/Do`), and coordinate-clustered tables; multi-currency (`CNY`, `USD`, `EUR`, `GBP`, `JPY`, `HKD`, `CAD`, `AUD`, `SGD`, `TWD`, `KRW`, `CHF`); Chinese electronic invoices (数电票); styled/GBK HTML emails (zero JS/network, ignoring `<script>`/`<style>`/hidden preheaders/`<del>`); RFC 4180 multiline UTF-8/UTF-16/GB18030 Alipay & WeChat CSV statements (`subvault csv`); encrypted user annotations (`subvault annotate`); and idempotent auditable manual entry (`subvault record-fact`).
   - **Real-Data Format 6 & At-Rest Protection (M4):** Dedicated non-synthetic Format 6 (`subvault init --format 6 --protection passphrase|os-disk`, `SUBVAULT_VAULT` banner) isolated from disposable Formats 1–5. `passphrase` mode provides pure-Rust `Argon2id` (with bounded KDF parameters) + `HKDF-SHA256` + `XChaCha20-Poly1305` envelope encryption across objects, staging, journals, and sensitive SQLite columns (`PRAGMA secure_delete = ON`), offline `SVRK-...` recovery keys, `subvault rotate-passphrase` (with `VACUUM`), dedicated `EXIT_AUTH = 22`, mandatory `--acknowledge-plaintext yes` on `export`, and zero raw-secret CLI arguments.
   - **Redacted Template Export (`subvault redact`):** Local-only deterministic sanitizer for external files or stored vault sources (`--source` or `--vault --source-id`) with repeatable `--redact-term`, Chinese fapiao tax/bank/ID patterns, SSN/UUID/Stripe/VAT/phone/card patterns, HTML adjacent table-cell & entity-decoded redaction, and PII-free JSON receipts.
   - **Immutable Byte Custody (T1):** Content-addressed `objects/obj_<id>` storage with SHA-256 integrity verification, zero byte normalization, idempotent `CommandId` journaling, exclusive non-blocking locks (`flock` on Linux, Win32 `LockFileEx` on Windows), SQLite 3.46 WAL (`synchronous=FULL`), atomic no-replace directory publication, and honest exit codes (`0`–`22`).
   - **Native Text-PDF & Static HTML Extraction (Formats 2–5):**
     - Format 5 (`subvault init --format 5`) accepts **both** `subvault pdf` (uncompressed and `/FlateDecode` via `miniz_oxide =0.8.8`, bounded memory, 1-based `pdf_page` locators) and `subvault html` (`tl` 0.7.8, refusing `<style>`/stylesheets instead of guessing CSS visibility).
     - Preserves raw bytes verbatim on encrypted PDFs (`encrypted_pdf`), image-only PDFs (`no_extractable_text`, `ocr=not_available`), and CSS-styled HTML; rejects fake renamed `.pdf` text files before preservation (`invalid_pdf_header`).
   - **Append-Only Human Decisions:** `correct` and `undo` append auditable human revisions while keeping original bytes untouched and surviving byte-identical re-imports (`AlreadyPreserved`).
   - **Offline Exit, Standalone Verification & Read-Only Review Sheet (T5):** `subvault export` produces self-contained `offline-exit-1` packages (`records.json`, `facts.csv`, `manifest.json`, `originals/<object_id>`); `subvault verify-package --package PATH` independently verifies the exported package without needing the live vault; `subvault review` writes a lock-consistent plain-text review sheet linked only to hash-verified package originals.

3. **Interactive Pure-Static Web Demo (`web/`) — Real Binary PDF Support + Native CLI Bridge**
   - **Preloaded Synthetic Corpus:** Ships out-of-the-box with 12 realistic billing scenarios covering monthly/annual plans, personal vs. work account separation, SHA-256 duplicate reuse, CSV vs. invoice conflicts (`$99.00` vs. `$107.91`), credit notes, masked cancellation notices, one-time perpetual licenses, and static HTML bills.
   - **Real Binary `.pdf` Drop (`web/pdf.js`) & Full `.zip` Export (`web/zip.js`):** Dropping local `.pdf`/`.html`/`.csv`/`.txt` files hashes exact raw bytes locally; **Download Full Exit Pack (.zip)** exports an archive directly verifiable by `subvault verify-package`; **Open Native Export** loads and SHA-256-verifies real native `subvault export` folders in-browser.

4. **CI, Automated Packaging & Verification**
   - `.github/workflows/ci.yml`, `.github/workflows/pages.yml`, `.github/workflows/release.yml`, `scripts/package-release.sh`, and `scripts/package-release-windows.ps1` with `SHA256SUMS.txt`.

---

## Portable Bundle Assets (`scripts/package-release.sh`)

| Asset | Contents |
| --- | --- |
| `subvault-v0.1.0-linux-x86_64.zip` | Portable Linux x86_64 `subvault-app` local graphical workbench + `bin/subvault` CLI (Formats 1–6), static Web Demo (`web/`), verified T0 synthetic corpus, `run-demo.sh`, and documentation |
| `subvault-v0.1.0-linux-x86_64.tar.gz` | Same Linux portable bundle in `.tar.gz` format |
| `subvault-v0.1.0-windows-x86_64.zip` | Portable Windows x86_64 double-click `subvault-app.exe` local workbench + `bin/subvault.exe` CLI (Formats 1–6), static Web Demo (`web/`), verified T0 synthetic corpus, `run-demo.ps1`, and documentation |
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

## Launch the Local Graphical Workbench

- **Windows:** Unzip `subvault-v0.1.0-windows-x86_64.zip` and double-click `subvault-app.exe`.
- **Linux:** Extract `subvault-v0.1.0-linux-x86_64.zip` and run `./subvault-app`.
