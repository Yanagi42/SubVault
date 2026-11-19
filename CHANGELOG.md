# Changelog

All notable changes to SubVault are documented in this file.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/).

## [Unreleased]

Planned for the next iteration (`v0.2.0` — see [`docs/ROADMAP.md`](./docs/ROADMAP.md)):
- Windows native runtime wiring (`src/windows_lock.rs`, `src/windows_file_id.rs`) and `windows-latest` CI verification.
- Native PDF text extraction with exact page/offset locators.
- At-rest encryption & key-recovery decision enabling removal of the `SYNTHETIC_ONLY` gate for real personal data.
- Native desktop shell wrapping the Rust/SQLite engine.

## [0.1.0] - 2026-11-19

### Added
- **Rust + SQLite Preservation & Extraction Core (`src/`):**
  - Immutable content-addressed byte custody (`objects/obj_<id>`), SHA-256 verification, idempotent `CommandId` journaling, exclusive `flock`, SQLite 3.46 WAL (`synchronous=FULL`), atomic directory publication via `renameat2(RENAME_NOREPLACE)` + parent fd `fsync`, and honest exit-code separation (`0`–`21`).
  - Isolated vault formats 1–4 (`T0`–`T5`), deterministic UTF-8 text fragment extraction (`stated-lines-1`) and constrained static HTML extraction (`tl` 0.7.8, `stated-html-blocks-3`, refusing `<style>`/stylesheets).
  - Append-only human decision chain (`correct` / `undo`) preserving original bytes verbatim across re-imports.
  - Self-contained `offline-exit-1` exporter (`subvault export`), lock-consistent read-only plain-text review sheet (`subvault review`), and standalone offline package verifier (`subvault verify-package --package PATH`).
- **Pure-Static Interactive Web Demo (`web/`):**
  - Preloaded 12-file synthetic corpus covering monthly/annual bills, same-merchant personal vs. work isolation, SHA-256 byte-identical deduplication, CSV vs. invoice amount conflicts, credit notes, masked cancellation notices, perpetual licenses, and Format 4 HTML bills.
  - One-click sample batch import, local file drag-and-drop, inline amount `correct`/`undo`, masked account attribution + undo, browser `localStorage` persistence, one-click full `.zip` offline exit download (`web/zip.js`), and native CLI export folder loader (`Open Native Export`) with SHA-256 verification.
  - Bidirectional interoperability verified between `subvault export` / `subvault verify-package` and the Web Demo.
- **CI, Pages, Release Automation & Docs:**
  - `.github/workflows/ci.yml` (`fetch-depth: 0`, `fmt`, `clippy -D warnings`, full Rust test suite, Web/interop tests, portable packaging).
  - `.github/workflows/pages.yml` (subpath `/SubVault/` verification + GitHub Pages deploy).
  - `.github/workflows/release.yml` (automated `v*` tag release publishing with `SHA256SUMS.txt`).
  - `scripts/package-release.sh`, `scripts/test-web.mjs`, `scripts/capture-screenshots.py`, `RELEASE_NOTES.md`, English `README.md`, real headless screenshots in `docs/screenshots/`, and preserved full Chinese spec in `docs/SPEC-zh.md`.

### Changed
- Cleaned all Clippy lint warnings in library and binary targets (`cargo clippy --lib --bins -- -D warnings`).
- Made `scripts/run-t1-evidence.sh` portable across local Cargo registries and writable output directories via `SUBVAULT_EVIDENCE_OUT` and `CARGO_HOME`.
