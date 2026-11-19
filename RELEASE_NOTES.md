# SubVault v0.1.0 Release Notes

**Release date:** 2026-11-19  
**Tag:** `v0.1.0`  
**Live Demo:** <https://yanagi42.github.io/SubVault/>

---

## Highlights

SubVault `v0.1.0` packages the verified **local-first Rust + SQLite preservation core (T0–T5, Vault Formats 1–4)** alongside a **pure-static interactive Web Demo** so anyone can inspect SubVault's epistemic ledger, SHA-256 byte-identical deduplication, conflict handling, append-only human corrections, and verifiable offline exit flow directly in a browser or from a portable Linux bundle.

### What's New in `v0.1.0`

1. **Interactive Pure-Static Web Demo (`web/`)**
   - **Preloaded Synthetic Corpus:** Ships out-of-the-box with realistic billing scenarios covering monthly/annual subscriptions, personal vs. work account separation at the same vendor, byte-identical duplicate detection across renamed files, CSV vs. tax-invoice amount conflicts (`$99.00` vs. `$107.91`), credit-note totals vs. paid amounts, cancelled-but-usable renewals, masked unassigned account emails, one-time perpetual software licenses, and Format 4 static HTML table-row bills.
   - **One-Click Sample Import & Local File Drop:** Re-import the 12-file sample batch or drop local `.html`, `.txt`, `.csv`, and `.pdf` files directly in the browser to observe real SHA-256 object reuse, occurrence logging, and non-destructive human `correct` / `undo` decision preservation.
   - **Zero Lock-In Browser Exports:** Preview and download `review.txt`, `facts.csv`, `records.json`, and a SHA-256 `manifest.json` (`offline-exit-1`) with zero backend calls.
   - **Subpath-Safe Static Hosting:** All static asset references are strictly relative (`./*`) with `.nojekyll` enabled for zero-error deployment on GitHub Pages (`/SubVault/`).

2. **Verified Rust + SQLite Preservation & Extraction Core (`subvault` CLI)**
   - **Immutable Byte Custody (T1):** Content-addressed `objects/obj_<id>` storage with SHA-256 integrity verification, zero byte normalization, idempotent `CommandId` journaling, exclusive non-blocking `flock`, SQLite 3.46 WAL (`synchronous=FULL`), atomic directory publication via `renameat2(RENAME_NOREPLACE)` + parent fd `fsync`, and 17 distinct honest exit codes.
   - **Source-Stated Candidates & Facts (T2a / T3 / Format 4):** Deterministic extraction for UTF-8 text fragments and constrained static HTML bills (`tl` 0.7.8 + `html-escape` 0.2.13, refusing `<style>` / stylesheets instead of guessing CSS visibility), recording exact Unicode scalar and UTF-8 byte offsets.
   - **Append-Only Human Decisions:** `correct` and `undo` append auditable human revisions while keeping original bytes untouched and surviving re-imports.
   - **Offline Exit & Read-Only Review Sheet (T5):** `subvault export` produces self-contained packages (`records.json`, `facts.csv`, `manifest.json`, `originals/<object_id>`) with explicit `complete` vs. `partial` reporting; `subvault review` writes a lock-consistent plain-text review sheet linked only to hash-verified package originals.

3. **CI, Automated Packaging & Release Verification**
   - Added `.github/workflows/ci.yml` running deterministic corpus verification, full Rust unit and historical-worktree compatibility integration tests, Web Demo checks, and portable bundle packaging with `SHA256SUMS.txt` verification.
   - Added `.github/workflows/pages.yml` for automated GitHub Pages deployment with subpath HTTP 200 checks.
   - Added `scripts/package-release.sh` to generate reproducible portable archives and `SHA256SUMS.txt`.

---

## Portable Release Assets

| Asset | Contents |
| --- | --- |
| `subvault-v0.1.0-linux-x86_64.zip` | Portable Linux x86_64 `subvault` CLI binary, static Web Demo (`web/`), verified T0 synthetic corpus, `run-demo.sh`, and documentation |
| `subvault-v0.1.0-linux-x86_64.tar.gz` | Same portable bundle in `.tar.gz` format |
| `subvault-v0.1.0-web-demo.zip` | Pure-static Web Demo files ready for any static host or offline folder |
| `SHA256SUMS.txt` | SHA-256 checksums for all release archives |

### Verify Checksums

```bash
sha256sum -c SHA256SUMS.txt
```

### Quick Start (Portable Bundle)

```bash
unzip subvault-v0.1.0-linux-x86_64.zip
cd subvault-v0.1.0-linux-x86_64

# Run an end-to-end native CLI Format 4 HTML + export + review walkthrough
./run-demo.sh cli

# Or launch the local pure-static Web Demo at http://127.0.0.1:4173
./run-demo.sh web
```

---

## Known Scope & Platform Boundaries

SubVault prioritizes **epistemic honesty** over premature claims:

- **Synthetic Verification Stage:** The native CLI vault retains the `SYNTHETIC_ONLY` guard (`user_ready=false`, `security_complete=false`) until real-data encryption/protection preferences and PDF text extraction are finalized.
- **Platform Support:** Native binary builds and file-locking tests currently target Linux (`ext4` / `flock` / `renameat2`). Windows lock and file-identity contracts (`src/windows_lock.rs`, `src/windows_file_id.rs`) are staged behind `cfg(windows)` and have not yet been compiled or runtime-verified on Windows.
- **Not Yet Implemented:** Native PDF text extraction, OCR, at-rest encryption selection, full-text search, automatic email/bank syncing, and native Windows desktop UI shell.
