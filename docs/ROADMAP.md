# SubVault Roadmap — Next Iteration (`v0.2.0`)

This document captures the clean handoff boundary at the close of **`v0.1.0`** (verified Linux preservation/extraction core + interactive pure-static Web Demo + bidirectional `offline-exit-1` verification) and defines the concrete engineering milestones for the next iteration (**`v0.2.0`**).

---

## Completed in `v0.1.0`

- [x] **T0–T5 Rust + SQLite Core (Linux `ext4`):** immutable byte preservation (`objects/obj_*`), SHA-256 integrity, `flock`, SQLite 3.46 WAL (`synchronous=FULL`), `renameat2(RENAME_NOREPLACE)` + parent fd `fsync`, Format 1–4 isolation, deterministic UTF-8 fragment & constrained static HTML extraction, append-only `correct`/`undo`, `export`, `review`, and standalone `verify-package`.
- [x] **Pure-Static Web Demo & Native Bridge (`web/`):** preloaded 12-file synthetic corpus, one-click sample re-import & SHA-256 deduplication audit, masked account attribution + undo, local persistence, pure-JS `.zip` offline exit pack (passes native `subvault verify-package`), and `Open Native Export` loader (verifies native `subvault export` folders in-browser).
- [x] **Engineering & Release Infrastructure:** `cargo fmt` + `cargo clippy --lib --bins -D warnings` clean, GitHub Actions CI (`ci.yml`), GitHub Pages deploy (`pages.yml`), automated tag release workflow (`release.yml`), portable packaging script (`scripts/package-release.sh`) with `SHA256SUMS.txt`, and headless UI screenshots.

---

## Next Iteration (`v0.2.0`) — From Synthetic Core to First 100 Real Users

### Milestone 1: Native Windows Support & Verification
- **Goal:** Lift the `target_os = "linux"` compile gate in `src/platform.rs` cleanly with real Windows verification.
- **Concrete tasks:**
  1. Wire `src/windows_lock.rs` (`CreateFileW` + `LockFileEx`) into `VaultLock` under `cfg(windows)`.
  2. Wire `src/windows_file_id.rs` (`GetFileInformationByHandleEx` / `FileIdInfo`) into output-path containment checks (`place_output`), covering Windows case-insensitivity and NTFS junctions/reparse points.
  3. Implement atomic no-replace directory publication for Windows without silent overwrite.
  4. Add a `windows-latest` job to `.github/workflows/ci.yml` and Windows `.zip` output to `release.yml`.

### Milestone 2: Native PDF Text Extraction
- **Goal:** Support real-world text PDFs (starting with the exact 9 text-PDF fixtures already defined in `synthetic/corpus/originals/*.pdf`) while keeping raw PDF bytes untouched and never silently running OCR.
- **Concrete tasks:**
  1. Add a deterministic, bounded-memory PDF text extractor emitting page-scoped locators (`pdf_page`, byte/scalar offsets) into `candidates` + `facts`.
  2. Support multi-document logical expectations from `synthetic/corpus/logical-expectations.json`: same-merchant multi-account separation, credit-note vs. paid separation, order-total non-adoption, one-time perpetual license scope, and CSV-vs-invoice conflict surfacing.
  3. Add a redacted-template export helper so early users can share broken vendor invoice layouts safely without leaking personal data.

### Milestone 3: Real-Data Protection Mode & Removing `SYNTHETIC_ONLY`
- **Goal:** Answer `protection_preference` and `primary_input_preference` cleanly so real personal/work billing receipts can be stored safely.
- **Concrete tasks:**
  1. Finalize local at-rest protection design (e.g., user-passphrase envelope encryption / SQLCipher or OS-keystore-backed object encryption + explicit offline recovery key).
  2. Introduce a non-synthetic production vault format (preserving strict isolation from disposable synthetic Formats 1–4).
  3. Document threat model, backup/recovery procedure, and verified offline exit decryption flow.

### Milestone 4: Thin Native Desktop Shell (Windows-First)
- **Goal:** Replace the CLI-only workflow with a native desktop wrapper over the Rust core (e.g., Tauri or egui sharing the exact UI flows validated in `web/`).
- **Concrete tasks:**
  1. Drag-and-drop & watch-folder local intake (PDF, saved HTML emails, store CSVs) — still zero Plaid and zero inbox OAuth.
  2. Overview ledger, one-click jump to untouched original evidence, inline `correct`/`undo`/attribution, and one-click verifiable exit pack export.
  3. Signed portable Windows installer/zip for the first 100 indie-dev & local-first pilot users.
