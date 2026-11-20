# SubVault Roadmap — Path to `v0.1.0`

SubVault will **not** cut premature version bumps during pre-release development. All engineering milestones below work toward shipping **`v0.1.0`** as the **first genuinely usable local-first release**.

---

## Completed Pre-`v0.1.0` Foundations

### Milestone 1: Local Preservation Core (Formats 1–4) & Pure-Static Web Demo
- [x] Linux `ext4` single-writer preservation (`flock`, SQLite 3.46 WAL `synchronous=FULL`, `renameat2(RENAME_NOREPLACE)` + parent fd `fsync`, 17 honest exit codes).
- [x] Isolated Formats 1–4 (`fragment`, constrained static `html`, append-only `correct`/`undo`, `export`, `review`, standalone `verify-package`).
- [x] Pure-static interactive Web Demo (`web/`) with preloaded synthetic corpus, SHA-256 deduplication audit, masked account attribution, full `.zip` offline exit, and `Open Native Export` verifier.
- [x] GitHub Actions CI (`ci.yml`), GitHub Pages (`pages.yml`), tag release workflow (`release.yml`), and portable packaging (`scripts/package-release.sh`).

### Milestone 2: Format 5 Native Text-PDF Preservation & Extraction (`subvault pdf`)
- [x] Pure-Rust bounded-memory PDF content stream parser (`src/pdf_parse.rs`, `miniz_oxide` `FlateDecode` with 1 MiB/stream & 4 MiB/PDF decompression-bomb caps, 1-based `pdf_page` locators).
- [x] Isolated Format 5 vault (`subvault init --format 5`) accepting **both** `subvault pdf` and `subvault html` in one vault, while keeping Formats 1–4 strictly unmigrated.
- [x] Verified against all 9 real `%PDF-1.4` files + renamed byte-identical duplicate in `synthetic/corpus/originals/`, plus compressed `FlateDecode` PDFs, encrypted PDFs (`preservation=preserved`, `extraction=unsupported`), and fake renamed text files (`invalid_pdf_header`).
- [x] Real binary `.pdf` drag-and-drop & exact SHA-256 deduplication in the Web Demo (`web/pdf.js`) plus bidirectional Format 5 Rust CLI ↔ Web interop tests.

---

## Remaining Milestones Before Cutting `v0.1.0`

### Milestone 3: Native Windows Platform Wiring & CI Matrix
- Wire `src/windows_lock.rs` (`CreateFileW` + `LockFileEx`) into `VaultLock` under `cfg(windows)`.
- Wire `src/windows_file_id.rs` (`GetFileInformationByHandleEx` / `FileIdInfo`) into `place_output`, covering case-insensitivity and NTFS junctions/reparse points.
- Implement atomic no-replace directory publication on Windows and add a `windows-latest` job to `.github/workflows/ci.yml` and `release.yml`.

### Milestone 4: At-Rest Protection Mode & Real-Data Vault Format
- Finalize local at-rest encryption and offline recovery-key design (`protection_preference`), plus a redacted-template export helper for sharing broken real-world vendor invoice layouts safely.
- Introduce a dedicated non-synthetic production vault format isolated from disposable synthetic Formats 1–5 (lifting the `SYNTHETIC_ONLY` guard).

### Milestone 5: Thin Native Desktop Shell → Ship `v0.1.0`
- Thin native desktop shell wrapping the verified Rust/SQLite core (drag-and-drop / watch-folder local intake for PDF, HTML, and CSV — zero Plaid and zero inbox OAuth).
- Cut the first official **`v0.1.0`** release once real users can safely drop real bills and run the full workflow end-to-end.
