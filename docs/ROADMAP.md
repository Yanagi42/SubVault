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

### Milestone 3: Native Windows Platform Wiring & CI Matrix
- [x] Wired `src/windows_lock.rs` (`CreateFileW` + `LockFileEx`, `bInheritHandle = FALSE`, mandatory byte-range lock) into `VaultLock` under `cfg(windows)`, with verified two-process `writer_busy` contention (`EXIT_LOCKED = 11`) and lock release recovery.
- [x] Wired `src/windows_file_id.rs` (`GetFileInformationByHandleEx(FileIdInfo)`, 8-byte volume serial + 16-byte `FILE_ID_128`) into `place_output`, covering Windows drive prefixes, verbatim `\\?\` paths, case-insensitive aliases, `..` traversals, and NTFS directory junctions (`mklink /J`).
- [x] Implemented Windows atomic no-replace directory publishing (`SetFileInformationByHandle(FileRenameInfo)` with `ReplaceIfExists = FALSE` and `GetFinalPathNameByHandleW` moved-parent tracking), `CREATE_NEW | FILE_FLAG_OPEN_REPARSE_POINT`, `BCryptGenRandom`, `GetDriveTypeW`/UNC network-filesystem refusal, and SQLite `file:///C:/...` URI normalization.
- [x] Verified natively on Windows 11 NTFS (`x86_64-pc-windows-msvc`) across unit and integration suites (`windows_native`, `verify_package`, `pdf_bill`, `review_sheet`, and concurrent `review_snapshot`), plus added `windows-latest` jobs to `.github/workflows/ci.yml` and `.github/workflows/release.yml` and portable Windows packaging (`scripts/package-release-windows.ps1`).

### Milestone 4: At-Rest Protection Mode, Real-Data Format 6 & Redacted Template Export
- [x] Dedicated non-synthetic Format 6 (`subvault init --format 6 --protection passphrase|os-disk`) isolated from disposable Formats 1–5 (`SUBVAULT_VAULT` banner, `synthetic_only = false`, `real_data_import = "available"`, accepting PDF, HTML, and fragments in one vault).
- [x] Pure-Rust application-layer envelope encryption (`src/crypto.rs`: `Argon2id` + `HKDF-SHA256` + `XChaCha20-Poly1305`) across `objects/obj_*` (`SVENC001`), `staging/*.partial`, `journal/*.json` (`SVJRN001`), and sensitive SQLite columns (`enc:v1:...` bound to `vault_id || table || row_id || column`), plus keyed `HMAC-SHA256` dedup keys, checksummed `SVRK-...` offline recovery keys, `subvault rotate-passphrase`, dedicated `EXIT_AUTH = 22`, CLI secret-arg refusal, and mandatory `--acknowledge-plaintext yes` on encrypted `export`.
- [x] Deterministic local-only `subvault redact` (`src/redact.rs`) supporting external files and stored vault sources (`--source` or `--vault --source-id`), repeatable `--redact-term`, PDF static text extraction without binary/XMP leaks, HTML script/style/comment stripping with preserved table structure, and PII-free JSON receipts.
- [x] Verified on both Windows 11 NTFS (`x86_64-pc-windows-msvc`) and Linux (`x86_64-unknown-linux-gnu`) with zero-plaintext disk scans and AEAD tamper/swap proofs (`tests/format6_crypto.rs`, `tests/redact.rs`).

---

### Milestone 5: Single-Binary Local Workbench & Bilingual Real-World Bill Engine (`v0.1.0` Ready)
- [x] Bilingual real-world static extraction (`src/pdf_parse.rs`, `src/real_extract.rs`, `src/csv_ingest.rs`) for Format 6: pure-Rust `/ObjStm` + PNG predictor + `/ToUnicode` CMap + Form XObject + row-clustered PDF parsing, multi-currency (`CNY`, `USD`, `EUR`, `GBP`, `JPY`, `HKD`, `CAD`, `AUD`, `SGD`, `TWD`, `KRW`, `CHF`), Chinese electronic invoices (UTF-16BE PDFs), styled email HTML (UTF-8/GB18030/UTF-16, zero JS/network, safely ignoring `<script>`/`<style>`/hidden preheaders), and RFC 4180 Alipay GB18030/GBK & WeChat UTF-8/UTF-16 CSV statements.
- [x] Encrypted user annotations (`source_annotations`) and auditable manual entry (`subvault annotate`, `subvault record-fact`, `decisions.kind = 'human_record'`, multi-currency `correct` & `undo`) for scanned PDFs, receipt screenshots, and promoted CSV transactions — with zero plaintext on disk.
- [x] Single-binary zero-install local graphical workbench (`subvault-app` / `subvault app`, `src/workbench.rs`, embedded `app/`): loopback-only `127.0.0.1` + 256-bit `X-SubVault-Token` + strict Host/Origin/CSP guards + in-memory `Zeroizing` DEK + in-memory `.zip` export, folder & batch drag-and-drop import, per-currency subscription dashboard, native OS folder picker, inline PDF/image/sandboxed-HTML evidence viewer, Alipay/WeChat CSV row picker, inline redaction, one-click Excel UTF-8 BOM CSV & verifiable offline exit packs, WCAG AA themes, and keyboard accessibility.
- [x] Ready for `v0.1.0` real-world use on both Windows 11 NTFS (`x86_64-pc-windows-msvc`, 83 native tests passing) and Linux (`x86_64-unknown-linux-gnu`, full 26 integration suites in CI).
