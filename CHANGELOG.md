# Changelog

All notable changes to SubVault are documented in this file.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and conservative `0.1.x` [Semantic Versioning](https://semver.org/).

## [Unreleased]

Planned next incremental steps (see [`docs/ROADMAP.md`](./docs/ROADMAP.md)):
- `v0.1.2`: Windows native runtime wiring (`src/windows_lock.rs`, `src/windows_file_id.rs`) and `windows-latest` CI matrix.
- `v0.1.3`: Local at-rest encryption & key-recovery design for a non-synthetic production format.
- `v0.2.0`: First user-ready Windows desktop shell over the Rust/SQLite core.

## [0.1.1] - 2026-11-20

### Added
- **Isolated Format 5 Vault & Native Text-PDF Extraction (`src/pdf_parse.rs`, `src/pdf.rs`):**
  - New `subvault pdf --vault PATH --command-id ID --source PATH [--batch-id ID]` command preserving raw `%PDF-` bytes untouched in `objects/obj_*` and extracting source-stated lines (`Amount paid`, `Service period`, `Total` credit-note/order-total semantics, masked/plain emails, cancellation `usable through`, and verbatim one-time license scopes) with 1-based `pdf_page` locators.
  - Bounded-memory pure-Rust content stream parser supporting uncompressed streams and `/FlateDecode` (`miniz_oxide` `=0.8.8`, capped at 1 MiB/stream and 4 MiB/PDF); detects encrypted PDFs (`encrypted_pdf`) and refuses fake non-PDF headers (`invalid_pdf_header`) without running OCR.
  - Format 5 (`init --format 5`) accepts both `subvault pdf` and `subvault html` in the same vault while leaving Formats 1–4 untouched and unmigrated.
  - Integration test suite `tests/pdf_bill.rs` verifying all 9 synthetic `%PDF-1.4` fixtures + renamed duplicate reuse, human edit preservation across re-imports, `FlateDecode`, encrypted PDFs, mixed PDF+HTML `export` / `verify-package` / `review`, and Format 1–4 isolation gates.
- **Real Binary PDF Ingestion in Web Demo (`web/pdf.js`):**
  - Dropping real `.pdf` files onto the Web Demo hashes exact raw binary bytes (matching `synthetic/corpus/byte-expectations.json` SHA-256 bit-for-bit) and extracts static text lines directly in-browser.
  - Extended `scripts/test-web.mjs` with real binary PDF SHA-256 deduplication tests and bidirectional Format 5 (`subvault pdf` + `subvault html` → `export` → `verify-package` → Web `Open Native Export`) interop verification.

### Changed
- Updated portable `run-demo.sh cli` inside `scripts/package-release.sh` to demonstrate Format 5 with a real synthetic PDF, renamed duplicate reuse, a static HTML bill, `export`, `verify-package`, and `review`.
- Adjusted `docs/ROADMAP.md` to conservative `0.1.x` milestone increments before `0.2.0`.

## [0.1.0] - 2026-11-19

### Added
- **Rust + SQLite Preservation & Extraction Core (`src/`):**
  - Immutable content-addressed byte custody (`objects/obj_<id>`), SHA-256 verification, idempotent `CommandId` journaling, exclusive `flock`, SQLite 3.46 WAL (`synchronous=FULL`), atomic directory publication via `renameat2(RENAME_NOREPLACE)` + parent fd `fsync`, and honest exit-code separation (`0`–`21`).
  - Isolated vault formats 1–4 (`T0`–`T5`), deterministic UTF-8 text fragment extraction (`stated-lines-1`) and constrained static HTML extraction (`tl` 0.7.8, `stated-html-blocks-3`, refusing `<style>`/stylesheets).
  - Append-only human decision chain (`correct` / `undo`) preserving original bytes verbatim across re-imports.
  - Self-contained `offline-exit-1` exporter (`subvault export`), lock-consistent read-only plain-text review sheet (`subvault review`), and standalone offline package verifier (`subvault verify-package --package PATH`).
- **Pure-Static Interactive Web Demo (`web/`):**
  - Preloaded 12-file synthetic corpus, one-click sample batch import, SHA-256 deduplication & conflict audit, masked account attribution + undo, browser `localStorage` persistence, pure-JS `.zip` offline exit download (`web/zip.js`), and native CLI export folder loader (`Open Native Export`).
- **CI, Pages, Release Automation & Docs:**
  - `.github/workflows/ci.yml`, `.github/workflows/pages.yml`, `.github/workflows/release.yml`, `scripts/package-release.sh`, `scripts/test-web.mjs`, `scripts/capture-screenshots.py`, `RELEASE_NOTES.md`, English `README.md`, real headless screenshots in `docs/screenshots/`, and preserved full Chinese spec in `docs/SPEC-zh.md`.

### Changed
- Resolved all Clippy lint warnings in library and binary targets (`cargo clippy --lib --bins -- -D warnings`).
- Made `scripts/run-t1-evidence.sh` portable via `SUBVAULT_EVIDENCE_OUT` and `CARGO_HOME`.
