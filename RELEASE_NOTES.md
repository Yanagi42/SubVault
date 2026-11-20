# SubVault v0.1.1 Release Notes

**Release date:** 2026-11-20  
**Tag:** `v0.1.1`  
**Live Demo:** <https://yanagi42.github.io/SubVault/>

---

## Highlights

SubVault `v0.1.1` adds **Format 5 native static text-PDF preservation and extraction (`subvault pdf`)** — allowing a single isolated vault to preserve and verify **both** real text-PDFs (uncompressed and `/FlateDecode`) and static HTML billing emails, while keeping Formats 1–4 strictly unmigrated and bringing real binary `.pdf` hashing and parsing into the pure-static Web Demo.

### What's New in `v0.1.1`

1. **Native Text-PDF Preservation & Extraction — Isolated Format 5 (`subvault pdf`)**
   - **Pure-Rust Bounded-Memory PDF Parser (`src/pdf_parse.rs`, `src/pdf.rs`):** Preserves raw `%PDF-` bytes verbatim in `objects/obj_<id>` (SHA-256 verified, zero normalization) and extracts source-stated lines from uncompressed and `/FlateDecode` (`miniz_oxide` `=0.8.8`, capped at 1 MiB/stream and 4 MiB/PDF) content streams with 1-based `pdf_page` locators.
   - **Verified Against the Full T0 Synthetic PDF Corpus:** Extracts monthly/yearly `stated_amount_paid` and `stated_service_period` facts, keeps credit-note and order-confirmation `Total` lines un-adopted, preserves masked cancellation notices (`j***@...`, usable-through dates) unassigned, preserves one-time perpetual license scope verbatim without inventing renewal dates, and deduplicates renamed byte-identical copies (`AlreadyPreserved`) without overwriting prior human `correct`/`undo` edits.
   - **Honest Safety Boundaries:** Encrypted PDFs (`/Encrypt`) are preserved verbatim and reported as `extraction=unsupported` (`encrypted_pdf`); scanned/image-only PDFs report `no_extractable_text` (`ocr=not_available`); fake renamed non-PDF text files are rejected before preservation (`invalid_pdf_header`).
   - **Mixed PDF + HTML Vault:** Format 5 (`subvault init --format 5`) accepts both `subvault pdf` and `subvault html`, with full support for `facts`, `candidates`, `correct`, `undo`, `export`, `verify-package`, and `review --package`.

2. **Interactive Pure-Static Web Demo (`web/`) — Real Binary PDF Support + Native CLI Bridge**
   - **Real Binary `.pdf` Drop (`web/pdf.js`):** Dropping local `.pdf` files onto the browser hashes exact raw PDF bytes (matching `synthetic/corpus/byte-expectations.json` SHA-256 bit-for-bit), deduplicates renamed copies, and extracts static text lines client-side.
   - **Bidirectional CLI ↔ Web Interop:** Download a full `.zip` offline exit archive (`web/zip.js`) directly verifiable by `subvault verify-package`, or open any native `subvault export` directory (Format 3/4/5) via **Open Native Export** to verify every file's SHA-256 in-browser.
   - **Preloaded Demo & Human Decisions:** Ships preloaded with the 12-file synthetic corpus, inline amount `correct`/`undo`, masked account attribution + undo, and local browser persistence.

3. **CI, Standalone Verifier & Automated Release Verification**
   - Includes `subvault verify-package --package PATH` for standalone offline verification without the live vault.
   - Enforces `cargo fmt --check`, `cargo clippy --lib --bins --locked -- -D warnings`, deterministic T0 corpus verification, full Rust unit/integration suites (including `tests/pdf_bill.rs` and `tests/verify_package.rs`), bidirectional Format 5 Web↔CLI interop checks, and portable bundle packaging with `SHA256SUMS.txt`.

---

## Portable Release Assets

| Asset | Contents |
| --- | --- |
| `subvault-v0.1.1-linux-x86_64.zip` | Portable Linux x86_64 `subvault` CLI binary (Formats 1–5), static Web Demo (`web/`), verified T0 synthetic corpus, `run-demo.sh`, and documentation |
| `subvault-v0.1.1-linux-x86_64.tar.gz` | Same portable bundle in `.tar.gz` format |
| `subvault-v0.1.1-web-demo.zip` | Pure-static Web Demo files ready for any static host or offline folder |
| `SHA256SUMS.txt` | SHA-256 checksums for all release archives |

### Verify Checksums

```bash
sha256sum -c SHA256SUMS.txt
```

### Quick Start (Portable Bundle)

```bash
unzip subvault-v0.1.1-linux-x86_64.zip
cd subvault-v0.1.1-linux-x86_64

# Run an end-to-end Format 5 PDF + duplicate reuse + HTML + export + verify-package + review demo
./run-demo.sh cli

# Or launch the local pure-static Web Demo at http://127.0.0.1:4173
./run-demo.sh web
```

---

## Known Scope & Platform Boundaries

- **Synthetic Verification Stage:** Native CLI vaults retain the `SYNTHETIC_ONLY` guard (`user_ready=false`, `security_complete=false`) until at-rest encryption and key recovery are finalized (`v0.1.3`).
- **Platform Support:** Native binary builds and file-locking tests currently target Linux (`ext4` / `flock` / `renameat2`). Windows lock and file-identity contracts (`src/windows_lock.rs`, `src/windows_file_id.rs`) are staged behind `cfg(windows)` for `v0.1.2`.
- **Not Yet Implemented:** OCR for scanned PDFs, encrypted PDF decryption, at-rest vault encryption, full-text search, and native desktop UI shell.
