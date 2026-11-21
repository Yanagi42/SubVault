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
- **Pure-Static Interactive Web Demo (`web/`):**
  - Preloaded 12-file synthetic corpus, one-click sample batch import, SHA-256 deduplication & conflict audit, masked account attribution + undo, browser `localStorage` persistence, client-side real binary `.pdf` hashing/extraction (`web/pdf.js`), pure-JS `.zip` offline exit download (`web/zip.js`), and native CLI export folder loader (`Open Native Export`) with bidirectional Format 5 CLI↔Web interop verification.
- **CI, Pages, Packaging & Documentation:**
  - `.github/workflows/ci.yml` (`fetch-depth: 0`, `fmt`, `clippy -D warnings`, full Rust suites, Web/CLI interop, portable bundle build), `.github/workflows/pages.yml`, `.github/workflows/release.yml`, `scripts/package-release.sh`, `scripts/test-web.mjs`, `scripts/capture-screenshots.py`, `RELEASE_NOTES.md`, English `README.md`, real headless screenshots in `docs/screenshots/`, `docs/ROADMAP.md`, and preserved full Chinese spec in `docs/SPEC-zh.md`.

### Changed
- Resolved all Clippy lint warnings in library and binary targets (`cargo clippy --lib --bins -- -D warnings`).
- Made `scripts/run-t1-evidence.sh` portable via `SUBVAULT_EVIDENCE_OUT` and `CARGO_HOME`.

### Remaining Before Cutting `0.1.0`
- Local at-rest encryption & key recovery design (lifting the `SYNTHETIC_ONLY` guard for real data).
- Thin native desktop shell over the Rust/SQLite core.
