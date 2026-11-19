# SubVault

**Local-first, SHA-256 verifiable subscription, invoice, and software-license vault — zero bank logins, zero inbox OAuth, and zero fabricated "Active/Expired" guesses.**

[![CI](https://github.com/Yanagi42/SubVault/actions/workflows/ci.yml/badge.svg)](https://github.com/Yanagi42/SubVault/actions/workflows/ci.yml)
[![Deploy Demo](https://github.com/Yanagi42/SubVault/actions/workflows/pages.yml/badge.svg)](https://github.com/Yanagi42/SubVault/actions/workflows/pages.yml)
![Rust 1.83+](https://img.shields.io/badge/rust-1.83%2B-orange)
![Local-First](https://img.shields.io/badge/storage-SQLite%20WAL%20%2B%20SHA--256-047857)

### Quick Links

- **Live Interactive Demo (GitHub Pages):** **[https://yanagi42.github.io/SubVault/](https://yanagi42.github.io/SubVault/)** — preloaded synthetic corpus, one-click sample import, real SHA-256 duplicate detection, inline `correct`/`undo` & masked-account attribution, native CLI export verifier (`Open Native Export`), and one-click `.zip` offline exit download
- **Portable Releases & Checksums:** **[Download Latest Release](https://github.com/Yanagi42/SubVault/releases/latest)** · [Release Notes (`RELEASE_NOTES.md`)](./RELEASE_NOTES.md) · [Changelog (`CHANGELOG.md`)](./CHANGELOG.md)
- **Next Iteration Plan (`v0.2.0`) & Full Technical Spec:** [Roadmap (`docs/ROADMAP.md`)](./docs/ROADMAP.md) · [Full Technical Verification Spec (中文)](./docs/SPEC-zh.md)

---

## Screenshots

### Overview Ledger & Evidence Inspector
*Preloaded synthetic corpus showing separated personal/work accounts, byte-identical duplicate reuse (`Deduped ×2`), CSV-vs-invoice conflict flags, untouched static source preview, and append-only human decision history.*

![SubVault Overview Ledger and Evidence Inspector](./docs/screenshots/ledger-overview.png)

### Duplicate Detection & Conflict Audit
*SHA-256 content-hash deduplication across renamed copies, side-by-side `$107.91` invoice vs. `$99.00` store CSV conflict isolation, same-merchant multi-account separation, and masked cancellation handling.*

![SubVault Duplicate Detection and Conflict Audit](./docs/screenshots/dedup-and-audit.png)

---

## Why SubVault?

Existing subscription trackers force a bad trade-off:
- **Bank/email scrapers** demand Plaid credentials or inbox OAuth, lose the original PDF/HTML receipts, merge personal and work accounts at the same merchant, and guess "Active / Expired" states.
- **Manual spreadsheets & receipt folders** duplicate entries whenever you re-import a folder and clobber manual fixes.

SubVault takes a strict **evidence-first** design:
1. **Immutable Original Custody:** Raw document bytes are stored untouched under `objects/obj_<id>` and verified by SHA-256. Path and filename are never identity.
2. **Automatic Byte-Identical Deduplication:** Re-importing renamed or duplicate files reuses the stored object and records a new `occ_*` occurrence — never duplicating spend and never overwriting prior human edits.
3. **Epistemic Honesty (No Guessing):** Source-stated paid amounts are explicitly marked `payment_verified=false`; ended service periods keep `current_status=unknown`; cancelled auto-renewals stay usable through the stated date; masked emails stay unassigned; credit notes and order totals never masquerade as paid invoices.
4. **Append-Only Human Decisions:** `correct` and `undo` append auditable revisions while preserving the untouched original bytes verbatim.
5. **Verifiable Offline Exit:** One command writes a self-contained exit package (`records.json`, `facts.csv`, `manifest.json`, `originals/`) with SHA-256 hashes and explicit `complete` / `partial` reporting — zero lock-in.

---

## Architecture

```mermaid
flowchart LR
    subgraph Inputs["Local Evidence Inputs (No Plaid / No Inbox OAuth)"]
        I1["Plain-Text Fragments / Receipts"]
        I2["Static HTML Bills (Format 4)"]
        I3["Store CSVs & Original Files"]
    end

    subgraph Core["SubVault Rust + SQLite Core (src/)"]
        L["Single-Writer Lock<br/>flock + CommandId Journal"]
        O["Content-Addressed Object Store<br/>objects/obj_* (SHA-256, Raw Bytes)"]
        E["Deterministic Static Extractor<br/>Exact UTF-8 Byte & Scalar Offsets"]
        D["SQLite 3.46 WAL Ledger<br/>sources · occurrences · candidates · facts · decisions"]
    end

    subgraph Outputs["Verifiable Human & Machine Surfaces"]
        R["Read-Only Review Sheet<br/>subvault review"]
        X["Offline Exit Package<br/>records.json · facts.csv · manifest.json · originals/"]
        W["Pure-Static Web Demo (web/)<br/>Preloaded Corpus · Dedup Audit · Browser Export"]
    end

    Inputs --> L
    L --> O
    L --> D
    O --> E --> D
    D --> R
    D --> X
    Inputs -. "In-browser SHA-256 mirror" .-> W
```

---

## Quick Start

### 1. Try the Live Demo (Zero Install)
Open **[https://yanagi42.github.io/SubVault/](https://yanagi42.github.io/SubVault/)**:
- Browse the preloaded 12-file synthetic corpus immediately.
- Click **`+ Import Sample Batch`** or **`+ Renamed Duplicate Receipt`** to watch SHA-256 deduplication reuse objects without duplicating spend or losing human edits.
- Click any row to inspect raw bytes, run **`Correct Amount`** / **`Undo`**, or switch to **`Review Sheet & Offline Exit`** to download `facts.csv`, `records.json`, and `manifest.json`.

### 2. Run the Portable Release Bundle (Linux x86_64)
Download `subvault-v0.1.0-linux-x86_64.zip` and `SHA256SUMS.txt` from [GitHub Releases](https://github.com/Yanagi42/SubVault/releases/latest):

```bash
sha256sum -c SHA256SUMS.txt
unzip subvault-v0.1.0-linux-x86_64.zip
cd subvault-v0.1.0-linux-x86_64

# Run an end-to-end native CLI Format 4 HTML + dedup + export + review walkthrough
./run-demo.sh cli

# Or serve the offline pure-static Web Demo locally at http://127.0.0.1:4173
./run-demo.sh web
```

### 3. Build & Test from Source

**Prerequisites (Linux):** Rust `1.83+`, `python3`, `sqlite3`, `gcc`, and `node` (for Web Demo tests).

```bash
# 1. Verify the deterministic synthetic T0 corpus
python3 synthetic/generate_corpus.py --check

# 2. Build the native Rust CLI binary
SUBVAULT_GIT_SHA=$(git rev-parse HEAD) cargo build --release --locked

# 3. Run the full Rust unit & integration suite (all 19 behavior & compatibility suites)
SUBVAULT_GIT_SHA=$(git rev-parse HEAD) cargo test --locked -- --test-threads=1

# 4. Run Web Demo SHA-256, dedup, and subpath asset tests
node scripts/test-web.mjs

# 5. Build portable release bundles + SHA256SUMS.txt into dist/
bash scripts/package-release.sh
```

### 4. Native CLI Walkthrough (Format 4 HTML + Verifiable Exit)

```bash
# Initialize an isolated Format 4 vault
./target/release/subvault init --vault /tmp/my-vault --format 4

# Preserve & extract a static HTML bill
./target/release/subvault html \
  --vault /tmp/my-vault \
  --command-id cmd-bill-1 \
  --source ./bill.html

# Export a SHA-256 verifiable offline exit package outside the vault
./target/release/subvault export \
  --vault /tmp/my-vault \
  --out /tmp/my-export-pack

# Independently verify the exported package without needing the live vault
./target/release/subvault verify-package \
  --package /tmp/my-export-pack

# Generate a lock-consistent plain-text review sheet linked to verified originals
./target/release/subvault review \
  --vault /tmp/my-vault \
  --out /tmp/review.txt \
  --package /tmp/my-export-pack
```

---

## Repository Layout

| Path | Purpose |
| --- | --- |
| `src/` | Rust preservation engine, SQLite WAL store, static text & HTML extractors, append-only decision chain, offline exporter, and review sheet renderer |
| `tests/` | 19 integration & backward-compatibility test suites (crash/fault hooks, WAL/header consistency, directory `renameat2(RENAME_NOREPLACE)`, parent fd `fsync`) |
| `web/` | Zero-dependency, subpath-safe pure-static interactive Web Demo (`index.html`, `app.js`, `engine.js`, `demo-data.js`, `zip.js`, `styles.css`) |
| `synthetic/` | Deterministic T0 corpus generator and independent byte/logical expectations |
| `scripts/` | Release packaging (`package-release.sh`), Web Demo & CLI-interop checks (`test-web.mjs`), screenshot capture (`capture-screenshots.py`), and T1 evidence script |
| `.github/workflows/` | Automated CI (`ci.yml`), GitHub Pages deployment (`pages.yml`), and tag-driven release publishing (`release.yml`) |
| `docs/` | Real UI screenshots (`docs/screenshots/`), next-iteration roadmap (`docs/ROADMAP.md`), and the complete Chinese technical specification (`docs/SPEC-zh.md`) |

---

## Current Status & Honest Boundaries

SubVault intentionally separates **verified guarantees** from **unbuilt milestones**:

- **Verified today:** Linux `ext4` single-writer preservation (`flock`, SQLite 3.46 WAL `synchronous=FULL`, `renameat2(RENAME_NOREPLACE)`), Format 1–4 isolation, deterministic UTF-8 fragment & constrained static HTML extraction (`tl` 0.7.8, CSS/stylesheet safely rejected), append-only human `correct`/`undo`, verifiable `offline-exit-1` export packages, read-only `review` sheets, and the pure-static interactive Web Demo.
- **Not yet completed:** Native PDF text extraction, OCR, at-rest encryption selection (CLI vaults still carry the `SYNTHETIC_ONLY` guard), full-text search, native Windows runtime verification (`src/windows_lock.rs` and `src/windows_file_id.rs` are staged behind `cfg(windows)`), and native desktop UI shell.

For the exhaustive contract of exit codes (0–21), crash/recovery semantics, and Format 1–4 invariants, see [`docs/SPEC-zh.md`](./docs/SPEC-zh.md).
