# SubVault Roadmap — Path to `v0.1.0`

SubVault avoids premature version bumps during pre-release development. All five milestones below complete the **`v0.1.0`** local-first release.

---

## Completed `v0.1.0` Milestones

### Milestone 1: Local Preservation Core (Formats 1–4) & Pure-Static Web Demo
- [x] Linux `ext4` single-writer preservation (`flock`, SQLite 3.46 WAL `synchronous=FULL`, `renameat2(RENAME_NOREPLACE)` + parent fd `fsync`, 15 honest exit codes `0`–`21`).
- [x] Isolated Formats 1–4 (`fragment`, constrained static `html`, append-only `correct`/`undo`, `export`, `review`, standalone `verify-package`).
- [x] Pure-static Web Demo (`web/`) with preloaded synthetic corpus, SHA-256 deduplication audit, masked account attribution, `.zip` offline exit, and `Open Native Export` verifier.
- [x] GitHub Actions CI (`ci.yml`), GitHub Pages (`pages.yml`), tag release (`release.yml`), and Linux portable packager (`scripts/package-release.sh`).

### Milestone 2: Format 5 Native Text-PDF Preservation & Extraction (`subvault pdf`)
- [x] Pure-Rust bounded-memory PDF content-stream parser (`src/pdf_parse.rs`, `/FlateDecode` decompression-bomb caps, 1-based `pdf_page` locators).
- [x] Isolated Format 5 vault accepting both `subvault pdf` and `subvault html` while keeping Formats 1–4 unmigrated, verified across synthetic PDFs, encrypted PDFs, and fake renamed files.
- [x] Real binary `.pdf` hashing/dedup in Web Demo (`web/pdf.js`) and bidirectional Format 5 Rust CLI ↔ Web interop checks.

### Milestone 3: Native Windows Platform Wiring & CI Matrix
- [x] Native Windows locking (`CreateFileW` + `LockFileEx`), 128-bit `FileIdInfo` path identity (case-insensitive aliases, `..`, NTFS junctions), atomic no-replace directory publishing (`FileRenameInfo`), `BCryptGenRandom`, UNC/network-share refusal, and Windows SQLite URI normalization.
- [x] Added `windows-latest` CI/release jobs and Windows portable packager (`scripts/package-release-windows.ps1`).

### Milestone 4: At-Rest Encryption, Real-Data Format 6 & PII Redaction
- [x] Dedicated non-synthetic Format 6 (`subvault init --format 6 --protection passphrase|os-disk`, `SUBVAULT_VAULT` banner) isolated from disposable Formats 1–5.
- [x] Pure-Rust envelope encryption (`src/crypto.rs`: `Argon2id` + `HKDF-SHA256` + `XChaCha20-Poly1305`) for objects, staging, journals, and sensitive SQLite columns, plus keyed `HMAC-SHA256` dedup, offline `SVRK-...` recovery keys, `subvault rotate-passphrase`, `EXIT_AUTH = 22`, raw-secret CLI refusal, and mandatory `--acknowledge-plaintext yes` on encrypted `export`.
- [x] Local-only `subvault redact` (`src/redact.rs`) for external files and vault sources with zero-plaintext disk scans and AEAD tamper/swap proofs (`tests/format6_crypto.rs`, `tests/redact.rs`).

### Milestone 5: Single-Binary Local Workbench & Bilingual Bill Engine (`v0.1.0` Ready)
- [x] Bilingual Format 6 static extraction (`src/pdf_parse.rs`, `src/real_extract.rs`, `src/csv_ingest.rs`): `/ObjStm` + PNG predictors + `/ToUnicode` CMaps + Form XObjects + row-clustered PDFs, 12 currencies, Chinese e-fapiao, styled/GB18030/UTF-16 HTML, and RFC 4180 Alipay/WeChat CSV statements.
- [x] Encrypted user annotations (`subvault annotate`) and auditable manual entry (`subvault record-fact`, multi-currency `correct`/`undo`) for scanned bills, screenshots, and promoted CSV rows.
- [x] Single-binary local workbench (`subvault-app` / `subvault app`, `src/workbench.rs`, embedded `app/`): loopback `127.0.0.1` + 256-bit `X-SubVault-Token` + Host/Origin/CSP guards + in-memory `Zeroizing` DEK & `.zip` export, folder drag-and-drop, inline evidence preview, Quick Triage, formula-safe Excel CSV export, WCAG AA themes, and keyboard navigation.
- [x] Verified natively on Windows 11 NTFS (`x86_64-pc-windows-msvc`, 85 tests) and Linux (`x86_64-unknown-linux-gnu`, full 26 integration suites in CI).
