# SubVault v0.1.0 Release Notes

First public local-first release of SubVault — encrypted Format 6 vault core, bilingual real-world bill extraction, and the single-binary `subvault-app` graphical workbench, verified across Windows and Linux.

## Highlights

1. **Zero-Install Local Graphical Workbench (`subvault-app` / `subvault app`)**
   - Loopback-only (`127.0.0.1`) workspace with 256-bit `X-SubVault-Token` auth, strict `Host`/`Origin`/`Sec-Fetch-Site` and CSP guards, sandboxed HTML preview, and in-memory `Zeroizing` keys. *(UI is currently Simplified Chinese; the `subvault` CLI and static Web Demo are English.)*
   - Drag and drop files or entire folders of PDF invoices, styled HTML emails, Alipay/WeChat CSV statements, or receipt screenshots; view per-currency spend totals without guessed exchange rates or fabricated Active/Expired states; inspect originals inline; resolve scanned bills in Quick Triage with full undo history; export formula-safe Excel `.csv`, verifiable `.zip` offline exit packs (built 100% in memory), or redacted templates.

2. **Format 6 Encrypted Vault & Bilingual Extraction Core (`subvault` CLI)**
   - **At-Rest Encryption:** Format 6 (`subvault init --format 6 --protection passphrase|os-disk`) isolated from disposable synthetic Formats 1–5. `passphrase` mode encrypts objects, staging, journals, and sensitive SQLite columns with `Argon2id` + `HKDF-SHA256` + `XChaCha20-Poly1305` (`PRAGMA secure_delete = ON`), includes offline `SVRK-...` recovery keys and `subvault rotate-passphrase`, forbids raw-secret CLI flags, and requires `--acknowledge-plaintext yes` on encrypted `export`.
   - **Bilingual Static Extraction:** Pure-Rust PDF (`/ObjStm`, PNG predictors, `/ToUnicode` CMaps, Form XObjects, Chinese e-fapiao UTF-16BE), styled/GB18030/UTF-16 HTML (zero JS/network), and RFC 4180 Alipay/WeChat CSV (`subvault csv`) across 12 major currencies (`CNY`, `USD`, `EUR`, `GBP`, `JPY`, `HKD`, `CAD`, `AUD`, `SGD`, `TWD`, `KRW`, `CHF`), plus `subvault annotate` and `subvault record-fact` for manual entries.
   - **Immutable Custody & Verifiable Exit:** Content-addressed `objects/obj_<id>` storage with SHA-256/HMAC dedup, single-writer locks (`flock` / `LockFileEx`), SQLite 3.46 WAL (`synchronous=FULL`), append-only `correct`/`undo`, local PII sanitizer (`subvault redact`), and standalone `subvault export`, `subvault verify-package`, and `subvault review`.

3. **Pure-Static Web Demo (`web/`)**
   - Preloaded 12-file synthetic corpus, real binary `.pdf` hashing and deduplication (`web/pdf.js`), in-browser `.zip` exit packs verifiable by `subvault verify-package`, and `Open Native Export` verification of native CLI exports.

4. **Cross-Platform CI & Portable Bundles**
   - Automated Linux and Windows builds, MSRV check, Web/CLI interop tests, and checksummed portable archives.

---

## Portable Bundle Assets

| Asset | Contents |
| --- | --- |
| `subvault-v0.1.0-linux-x86_64.zip` | Portable Linux x86_64 `subvault-app` workbench + `bin/subvault` CLI, static Web Demo (`web/`), T0 synthetic corpus, `run-demo.sh`, docs & licenses |
| `subvault-v0.1.0-linux-x86_64.tar.gz` | Same Linux portable bundle in `.tar.gz` format |
| `subvault-v0.1.0-windows-x86_64.zip` | Portable Windows x86_64 `subvault-app.exe` workbench + `bin/subvault.exe` CLI, static Web Demo (`web/`), T0 synthetic corpus, `run-demo.ps1`, docs & licenses |
| `subvault-v0.1.0-web-demo.zip` | Pure-static Web Demo files ready for any static host or offline folder |
| `SHA256SUMS.txt` | SHA-256 checksums for all release archives |

### Verify & Launch

**Linux:**
```bash
sha256sum --ignore-missing -c SHA256SUMS.txt
unzip subvault-v0.1.0-linux-x86_64.zip
cd subvault-v0.1.0-linux-x86_64
./subvault-app         # Launch local graphical workbench
./run-demo.sh cli      # End-to-end Format 5 PDF/HTML/export/verify CLI walkthrough
./run-demo.sh web      # Serve pure-static Web Demo at http://127.0.0.1:4173
```

**Windows:**
- Verify checksum against `SHA256SUMS.txt`, unzip `subvault-v0.1.0-windows-x86_64.zip`, and double-click `subvault-app.exe` (or run `.\run-demo.ps1 -Mode cli` / `.\run-demo.ps1 -Mode web`).
