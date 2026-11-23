# Contributing to SubVault

Thank you for helping improve SubVault. Every contribution must preserve SubVault's core principles:

1. **Immutable Original Custody**: Raw imported bytes are never modified or normalized.
2. **Epistemic Honesty**: Never guess verified payment status, active/expired subscription state, exchange rates, or unattributed account ownership. When an invoice has conflicting or ambiguous values, surface candidates honestly and let the user decide.
3. **Zero Silent Network / Zero Credential Scraping**: No telemetry, no remote fetches, no bank logins, and no inbox OAuth.
4. **Backward Compatibility**: Formats 1–5 are isolated synthetic test formats and must keep their existing contracts intact. Real-world features target Format 6.

## Local Development Setup

### Prerequisites
- Rust `1.83+` (with `rustfmt` and `clippy`)
- Python 3 (for verifying the deterministic T0 synthetic corpus)
- Node.js 18+ (for Web Demo and CLI↔Web interoperability tests)

### Running Checks

```bash
# 1. Verify synthetic corpus byte expectations
python3 synthetic/generate_corpus.py --check

# 2. Formatting & lints (CI enforces -D warnings)
cargo fmt --check
cargo clippy --lib --bins --locked -- -D warnings

# 3. Full Rust unit & integration test suite
cargo test --locked -- --test-threads=1

# 4. Web Demo & Rust CLI <-> Web interoperability checks
node scripts/test-web.mjs
```

On Windows PowerShell, use `python` instead of `python3`.

## Licensing

SubVault is dual-licensed under `MIT OR Apache-2.0`. Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this repository by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

## Reporting Broken Bill Layouts Safely

Never attach raw personal PDFs, HTML emails, or bank/Alipay/WeChat CSV files to public issues. Generate a redacted template locally first:

```bash
subvault redact --source ./my-invoice.pdf --out ./redacted.txt --redact-term "MyName"
```

Inspect `./redacted.txt` before uploading to verify no personal names, emails, addresses, phone numbers, card numbers, or tax IDs remain.

## Pull Request Guidelines

- Keep PRs focused and include unit or integration tests covering the behavior you added or fixed.
- Document user-visible changes concisely in `CHANGELOG.md` under `[Unreleased]`.
- For UI changes in `app/` or `web/`, verify both light and dark themes (`gallery`, `paper`, `night` for `app/`) and keyboard navigation.
