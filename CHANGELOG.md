# Changelog

All notable changes to SubVault are documented in this file.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). No public release tag has been cut yet — all work below builds toward the first genuinely usable **`0.1.0`** release (see [`docs/ROADMAP.md`](./docs/ROADMAP.md)).

## [Unreleased] (Target: `0.1.0`)

### Added
- **Rust + SQLite Preservation & Extraction Core (`src/`):**
  - Immutable content-addressed byte custody (`objects/obj_<id>`), SHA-256 verification, idempotent `CommandId` journaling, exclusive `flock`, SQLite 3.46 WAL (`synchronous=FULL`), atomic directory publication via `renameat2(RENAME_NOREPLACE)` + parent fd `fsync`, and honest exit-code separation (`0`–`21`).
  - Isolated vault formats 1–5: deterministic UTF-8 text fragments (`stated-lines-1`), constrained static HTML bills (`tl` 0.7.8, `stated-html-blocks-3`, refusing `<style>`/stylesheets), and native static text-PDF extraction (`subvault pdf`, `src/pdf_parse.rs`, uncompressed + `/FlateDecode` via `miniz_oxide =0.8.8` with decompression caps and 1-based `pdf_page` locators). Format 5 accepts both PDF and HTML in one vault while leaving Formats 1–4 unmigrated.
  - Append-only human decision chain (`correct` / `undo`) preserving original bytes verbatim across re-imports.
  - Self-contained `offline-exit-1` exporter (`subvault export`), lock-consistent read-only plain-text review sheet (`subvault review`), and standalone offline package verifier (`subvault verify-package --package PATH`).
  - **Native Windows Platform Support (`x86_64-pc-windows-msvc`):**
    - Wired `src/windows_lock.rs` (`CreateFileW` + `LockFileEx`, `bInheritHandle = FALSE`) into `VaultLock` and `src/windows_file_id.rs` (`GetFileInformationByHandleEx(FileIdInfo)`, 128-bit NTFS/ReFS identity) into `place_output` (refusing case-insensitive vault aliases, `..` traversal, and NTFS directory junctions).
    - Added Windows no-replace directory publish (`SetFileInformationByHandle(FileRenameInfo)` with `ReplaceIfExists = FALSE` + moved-parent handle tracking), `CREATE_NEW | FILE_FLAG_OPEN_REPARSE_POINT`, `BCryptGenRandom`, `GetDriveTypeW`/UNC network-share refusal, write-access file sync, Windows SQLite `file:///C:/...` URI handling, and `originals/` slash normalization in `export::verify_tree`.
    - Added `tests/windows_native.rs`, cross-platform test support in `pdf_bill`/`verify_package`/`review_sheet`/`review_snapshot`, `scripts/package-release-windows.ps1`, and `windows-latest` CI/release jobs.
  - **Real-Data Format 6, At-Rest Encryption & Redacted Template Export (Milestone 4):**
    - Introduced dedicated Format 6 real-data vaults (`subvault init --format 6 --protection passphrase|os-disk`) isolated from synthetic Formats 1–5 (`SUBVAULT_VAULT` banner instead of `SYNTHETIC_ONLY`, `synthetic_only = false`, `real_data_import = "available"`, accepting PDF, HTML, and text fragments in one vault).
    - Added pure-Rust application-layer envelope encryption (`src/crypto.rs`: `Argon2id` + `HKDF-SHA256` + `XChaCha20-Poly1305`) encrypting `objects/obj_*` (`SVENC001` 64 KiB authenticated chunks bound to `vault_id || object_id || chunk_idx || flags`), `staging/*.partial`, `journal/*.json` (`SVJRN001`), and all sensitive SQLite cells (`enc:v1:...` bound to `vault_id || table || row_id || column`), plus keyed `HMAC-SHA256` dedup keys, offline checksummed recovery keys (`SVRK-...`), `subvault rotate-passphrase`, dedicated `EXIT_AUTH = 22`, and CLI raw-secret flag refusal (`--passphrase` rejected).
    - Enforced explicit `--acknowledge-plaintext yes` on `subvault export` for `passphrase_aead_v1` vaults with honest exit package protection annotations (`plaintext_exit_from_passphrase_aead_v1` / `plaintext_exit_from_os_disk_assumed`), compatible with `verify-package` and `review --package`.
    - Added deterministic local-only `subvault redact` (`src/redact.rs`, `tests/redact.rs`) supporting both external files and vault sources (`--source` or `--vault --source-id`) with repeatable `--redact-term`, PDF text extraction without binary/XMP metadata, HTML script/style/comment stripping with preserved table structure, and PII-free JSON receipts.
- **Pure-Static Interactive Web Demo (`web/`):**
  - Preloaded 12-file synthetic corpus, one-click sample batch import, SHA-256 deduplication & conflict audit, masked account attribution + undo, browser `localStorage` persistence, client-side real binary `.pdf` hashing/extraction (`web/pdf.js`), pure-JS `.zip` offline exit download (`web/zip.js`), and native CLI export folder loader (`Open Native Export`) with bidirectional Format 5 CLI↔Web interop verification.
- **CI, Pages, Packaging & Documentation:**
  - `.github/workflows/ci.yml` (`fetch-depth: 0`, `fmt`, `clippy -D warnings`, full Rust suites on Linux & Windows, Web/CLI interop, portable bundle build), manual-only `.github/workflows/pages.yml`, `.github/workflows/release.yml`, `scripts/package-release.sh`, `scripts/package-release-windows.ps1`, `scripts/test-web.mjs`, `scripts/capture-screenshots.py`, `RELEASE_NOTES.md`, English `README.md`, real headless screenshots in `docs/screenshots/`, `docs/ROADMAP.md`, and preserved full Chinese spec in `docs/SPEC-zh.md`.

### Changed
- Resolved all Clippy lint warnings in library and binary targets (`cargo clippy --lib --bins -- -D warnings`).
- Switched `.github/workflows/pages.yml` to manual `workflow_dispatch` only until the product is ready for public demo deployment.
- Made `scripts/run-t1-evidence.sh` portable via `SUBVAULT_EVIDENCE_OUT` and `CARGO_HOME`.

### Remaining Before Cutting `0.1.0`
- Thin native desktop shell over the Rust/SQLite core.
