# Security Policy

## Scope & Threat Model

SubVault is a local-first, offline subscription and receipt vault. Security-sensitive components include:

- **Format 6 At-Rest Encryption (`src/crypto.rs`)**: `Argon2id` + `HKDF-SHA256` + `XChaCha20-Poly1305` envelope encryption for `objects/`, `staging/`, `journal/`, and sensitive SQLite columns, plus offline `SVRK-...` recovery keys.
- **Local Graphical Workbench (`src/workbench.rs`, `app/`)**: Loopback-only (`127.0.0.1`) HTTP server protected by a 256-bit `X-SubVault-Token` header, strict `Host`/`Origin`/`Sec-Fetch-Site` validation, sandboxed HTML preview, and in-memory `Zeroizing` key material.
- **Static Extraction & Redaction (`src/pdf_parse.rs`, `src/real_extract.rs`, `src/redact.rs`)**: Bounded-memory parsing of untrusted local PDF, HTML, and CSV files without JavaScript execution or network access, plus PII sanitization for issue sharing.
- **Filesystem & Export Integrity (`src/engine.rs`, `src/export.rs`, `src/verify_package.rs`)**: Path-boundary checks (`place_output`), symlink/junction refusal, atomic directory publication, and SHA-256 package verification.

## Reporting a Vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities.

Instead, report vulnerabilities privately through [GitHub Security Advisories](https://github.com/Yanagi42/SubVault/security/advisories/new) with:

1. Affected version, OS, and vault format (`1`–`6`).
2. Steps to reproduce or a minimal proof of concept.
3. Expected vs. actual security boundary behavior.

Do **not** include real personal bills, passphrases, or recovery keys in reports. If a parsing or redaction bug involves a real invoice, sanitize it locally first or construct a minimal synthetic sample.

## Supported Versions

| Version | Supported |
| --- | --- |
| `0.1.x` / `main` | Yes |

## Disclosure Expectations

- We aim to acknowledge private reports within 72 hours.
- Confirmed issues will be patched with regression tests before public disclosure.
