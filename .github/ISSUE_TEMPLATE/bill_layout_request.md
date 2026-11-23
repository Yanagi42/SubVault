---
name: Bill / Invoice Layout Request
about: Request extraction support for a PDF, HTML, or CSV bill layout using a safely redacted sample
title: "[Layout]: "
labels: enhancement
---

## Privacy Notice (Read First)

**Never upload raw personal invoices, email receipts, or Alipay/WeChat/bank CSV files.**

Before submitting, run `subvault redact` locally (or use the **脱敏 / Redact** tab inside `subvault-app`):

```bash
subvault redact \
  --source ./your-bill.pdf \
  --out ./redacted-sample.txt \
  --redact-term "Your Name" \
  --redact-term "Your Company"
```

Open `redacted-sample.txt` and visually double-check that no emails, phone numbers, card numbers, addresses, tax IDs, or account IDs remain.

## Document Metadata

- **File Type:** PDF / HTML email / CSV statement
- **Language / Region:** (e.g., English US / Chinese 数电票 / EUR German)
- **Vendor / Platform:** (e.g., Stripe, Apple, JetBrains, Alipay, WeChat Pay)
- **Expected Extracted Fields:**
  - Currency & Amount Paid:
  - Service Period or Bill Date:
  - Vendor / Plan / Account hint:
  - Should this be excluded as a refund / credit note / unpaid order? (Yes / No)

## Redacted Sample Output

```text
Paste the contents of redacted-sample.txt here
```

## Privacy Check

- [ ] I ran `subvault redact` and manually verified the pasted text contains zero personal or financial PII.
