# Changelog

All notable changes to SubVault are documented in this file.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). All work below builds toward the initial **`0.1.0`** public release (see [`docs/ROADMAP.md`](./docs/ROADMAP.md)).

## [Unreleased] (Target: `0.1.0`)

### Added
- **Immutable Preservation & Verification Core (`src/`):**
  - Content-addressed byte custody (`objects/obj_<id>`), SHA-256 verification, idempotent `CommandId` journaling, SQLite 3.46 WAL (`synchronous=FULL`, `PRAGMA secure_delete = ON`, `temp_store = MEMORY`), atomic no-replace directory publishing, and honest exit codes (`0`–`22`).
  - Cross-platform locking and filesystem safety: Linux `flock` + `renameat2(RENAME_NOREPLACE)` + parent fd `fsync`, and native Windows `x86_64-pc-windows-msvc` (`CreateFileW`/`LockFileEx`, 128-bit `FileIdInfo`, `FileRenameInfo` no-replace publish, `BCryptGenRandom`, junction and UNC/network-share refusal).
  - Isolated synthetic test Formats 1–5 (`fragment`, constrained static `html`, bounded-memory `pdf`, append-only `correct`/`undo`), plus self-contained `offline-exit-1` exporter (`subvault export`), lock-consistent plain-text review sheet (`subvault review`), and bounded streaming standalone verifier (`subvault verify-package --package PATH`).
- **Format 6 Real-Data Vault, At-Rest Encryption & PII Redaction:**
  - Dedicated non-synthetic Format 6 (`subvault init --format 6 --protection passphrase|os-disk`, `SUBVAULT_VAULT` banner) isolated from disposable Formats 1–5.
  - Pure-Rust envelope encryption (`src/crypto.rs`: bounded `Argon2id` + `HKDF-SHA256` + `XChaCha20-Poly1305`) across `objects/obj_*` (`SVENC001`), `staging/`, `journal/` (`SVJRN001`), and sensitive SQLite columns (`enc:v1:...`), plus keyed `HMAC-SHA256` dedup, offline `SVRK-...` recovery keys, `subvault rotate-passphrase` (with `VACUUM`), `EXIT_AUTH = 22`, raw-secret CLI flag refusal, and mandatory `--acknowledge-plaintext yes` on encrypted `export`.
  - Deterministic local-only PII sanitizer (`subvault redact`, `src/redact.rs`) for external files and stored vault sources with repeatable `--redact-term`, Chinese fapiao/tax/ID, SSN/EIN/VAT/UUID/Stripe/phone/card patterns, HTML entity/table/URL scrubbing, and PII-free JSON receipts.
- **Bilingual Real-World Bill Extraction & Manual Entry (`src/pdf_parse.rs`, `src/real_extract.rs`, `src/csv_ingest.rs`):**
  - Pure-Rust PDF extraction supporting `/ObjStm`, `/XRef` streams, `/FlateDecode` PNG/TIFF predictors, `/ASCIIHexDecode`/`/ASCII85Decode`, `/ToUnicode` Chinese CMaps, `WinAnsi`/`MacRoman`/`/Differences`, Form XObjects, inline image skipping, and `(y, x)` visual row clustering.
  - Bilingual styled HTML (UTF-8/GB18030/UTF-16, zero JS/network, ignoring `<script>`/`<style>`/hidden preheaders/`<del>`) and RFC 4180 Alipay/WeChat CSV ingestion (`subvault csv`) across 12 currencies (`CNY`, `USD`, `EUR`, `GBP`, `JPY`, `HKD`, `CAD`, `AUD`, `SGD`, `TWD`, `KRW`, `CHF`), plus encrypted user annotations (`subvault annotate`) and idempotent auditable manual facts (`subvault record-fact`).
- **Single-Binary Local Graphical Workbench (`subvault-app` / `subvault app`, `src/workbench.rs`, `app/`):**
  - Hardened loopback `127.0.0.1` server with 256-bit `X-SubVault-Token` auth, strict `Host`/`Origin`/`Sec-Fetch-Site` and CSP guards, socket timeouts & concurrency caps, serialized per-vault `op_lock`, in-memory `Zeroizing` DEK, and 100% in-memory verifiable `.zip` export (zero temp-dir plaintext).
  - Batch/folder drag-and-drop import, built-in synthetic Demo Pack with one-click cleanup, per-currency dashboard without clock-guessed subscription states, native OS folder picker, inline PDF/image/sandboxed-HTML evidence drawer, CSV row promoter, inline PII redaction, formula-safe Excel UTF-8 BOM CSV export, WCAG AA light/paper/night themes, and keyboard navigation.
- **Pure-Static Interactive Web Demo (`web/`):**
  - Preloaded 12-file synthetic corpus, SHA-256 dedup & conflict audit, masked account attribution + undo, real binary `.pdf` hashing/extraction (`web/pdf.js`), pure-JS `.zip` offline exit (`web/zip.js`), and `Open Native Export` CLI↔Web bidirectional interop verification.
- **CI, Packaging & OSS Documentation:**
  - Dual `MIT OR Apache-2.0` licensing, Linux + Windows CI + MSRV check (`.github/workflows/ci.yml`), manual `workflow_dispatch` GitHub Pages deploy (`.github/workflows/pages.yml`), tag release workflow (`.github/workflows/release.yml`), portable packagers (`scripts/package-release.sh`, `scripts/package-release-windows.ps1`), Web/interop tests (`scripts/test-web.mjs`), `README.md`, `RELEASE_NOTES.md`, `SECURITY.md`, `CONTRIBUTING.md`, issue templates, high-DPI screenshots (`docs/screenshots/`), `docs/ROADMAP.md`, and frozen Format 1–4 spec (`docs/SPEC-zh.md`).
