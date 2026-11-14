#!/usr/bin/env python3
"""Rebuild the disposable T0 corpus. Stdlib only. Deterministic bytes.

The committed files under synthetic/corpus/ are the fixture. --check rebuilds
them in a temp directory and compares. This script does not parse subscriptions
and does not talk to the network.
"""

from __future__ import annotations

import hashlib
import json
import shutil
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
CORPUS = ROOT / "corpus"
ORIGINALS = CORPUS / "originals"

BANNER = """SYNTHETIC-ONLY DISPOSABLE CORPUS
This package is fictional SubVault T0 fixture data.
It is not user data and it is not an approved store for real credentials.
Protection and recovery preferences are unanswered.
Primary input preference is unanswered.
Do not import it into a real-data product.
Preserving these bytes is not arrangement, search, or a completed product.
"""


def pdf_escape(text: str) -> str:
    if not text.isascii():
        raise SystemExit(f"PDF fixture text must stay ASCII: {text!r}")
    return text.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


def build_pdf(lines: list[str]) -> bytes:
    commands = ["BT", "/F1 10 Tf", "54 760 Td", "13 TL"]
    for index, line in enumerate(lines):
        escaped = pdf_escape(line)
        if index == 0:
            commands.append(f"({escaped}) Tj")
        else:
            commands.append(f"T* ({escaped}) Tj")
    commands.append("ET")
    stream = ("\n".join(commands) + "\n").encode("ascii")
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
        b"<< /Length %d >>\nstream\n" % len(stream) + stream + b"endstream",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]
    chunks = [b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n"]
    offsets = [0]
    for number, body in enumerate(objects, start=1):
        offsets.append(sum(len(part) for part in chunks))
        chunks.append(f"{number} 0 obj\n".encode("ascii"))
        chunks.append(body)
        chunks.append(b"\nendobj\n")
    xref_at = sum(len(part) for part in chunks)
    xref = ["xref", f"0 {len(objects) + 1}", "0000000000 65535 f "]
    for offset in offsets[1:]:
        xref.append(f"{offset:010d} 00000 n ")
    trailer = (
        "\n".join(xref)
        + "\ntrailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n"
        % (len(objects) + 1, xref_at)
    )
    data = b"".join(chunks) + trailer.encode("ascii")
    check_pdf(data)
    for line in lines:
        if line.encode("ascii") not in data:
            raise SystemExit(f"PDF dropped a source line: {line}")
    return data


def check_pdf(data: bytes) -> None:
    if not data.startswith(b"%PDF-1.4\n"):
        raise SystemExit("not a PDF header")
    if b"\n%%EOF\n" not in data and not data.endswith(b"%%EOF\n"):
        raise SystemExit("PDF missing EOF marker")
    marker = b"\nstartxref\n"
    at = data.rfind(marker)
    if at < 0:
        raise SystemExit("PDF missing startxref")
    line_end = data.find(b"\n", at + len(marker))
    xref_at = int(data[at + len(marker) : line_end])
    if data[xref_at : xref_at + 4] != b"xref":
        raise SystemExit("startxref does not point at xref")
    if b"/Type /Catalog" not in data or b"endobj" not in data:
        raise SystemExit("PDF objects incomplete")


DOCUMENTS: dict[str, list[str]] = {
    "originals/v-2026-09-receipt.pdf": [
        "Northline Video",
        "Document: receipt",
        "Document id: SYN-V-2026-09-001",
        "Account: personal",
        "Account email: mira.ellison.example@northline.example",
        "Amount paid USD 15.99",
        "Currency: USD",
        "Service period: 2026-09-01 through 2026-09-30",
        "This document states a paid amount for the stated service period only.",
    ],
    "originals/v-2026-10-receipt.pdf": [
        "Northline Video",
        "Document: receipt",
        "Document id: SYN-V-2026-10-001",
        "Account: personal",
        "Account email: mira.ellison.example@northline.example",
        "Amount paid USD 15.99",
        "Currency: USD",
        "Service period: 2026-10-01 through 2026-10-31",
        "This document states a paid amount for the stated service period only.",
    ],
    "originals/v-2026-10-credit-note.pdf": [
        "Northline Video",
        "Document: credit note",
        "Document id: SYN-V-2026-10-CN-001",
        "Account: personal",
        "Account email: mira.ellison.example@northline.example",
        "Reference document id: SYN-V-2026-10-001",
        "Total USD 15.99",
        "Currency: USD",
        "This Total is a credit-note total, not an amount paid.",
        "Receipt of the refund is not stated.",
    ],
    "originals/c-personal-2026-invoice.pdf": [
        "Paperlantern Cloud",
        "Document: invoice",
        "Document id: SYN-C-PERSONAL-2026-001",
        "Account: personal",
        "Account email: mira.ellison.example@paperlantern.example",
        "Amount paid USD 107.91",
        "Currency: USD",
        "Service period: 2026-03-01 through 2027-02-28",
        "Issued: 2026-03-01",
        "Merchant: Paperlantern Cloud",
    ],
    "originals/c-work-2026-invoice.pdf": [
        "Paperlantern Cloud",
        "Document: invoice",
        "Document id: SYN-C-WORK-2026-001",
        "Order id: SYN-C-WORK-2026-031",
        "Account: work",
        "Account email: jordan.hale.example@paperlantern.example",
        "Amount paid USD 107.91",
        "Currency: USD",
        "Service period: 2026-03-01 through 2027-02-28",
        "Issued: 2026-03-01",
        "Merchant: Paperlantern Cloud",
    ],
    "originals/c-work-order-confirm.pdf": [
        "Paperlantern Cloud",
        "Document: order confirmation",
        "Document id: SYN-C-WORK-2026-OC-001",
        "Order id: SYN-C-WORK-2026-031",
        "Account: work",
        "Account email: jordan.hale.example@paperlantern.example",
        "Total USD 99.00",
        "Currency: USD",
        "Tax inclusion is not stated.",
        "This Total is an order total whose payment nature is not stated.",
    ],
    "originals/c-cancel-notice-masked.pdf": [
        "Paperlantern Cloud",
        "Document: renewal cancellation notice",
        "Document id: SYN-C-CANCEL-2026-001",
        "Notice date: 2026-10-20",
        "Account email: j***@paperlantern.example",
        "Automatic renewal has been cancelled.",
        "The service remains usable through 2027-02-28.",
        "This notice does not say which account owns the masked email.",
        "This notice does not say the service ended on the notice date.",
    ],
    "originals/harbor-notes-2023-invoice.pdf": [
        "Harbor Notes",
        "Document: invoice",
        "Document id: SYN-HARBOR-2023-001",
        "Account: personal",
        "Account email: mira.ellison.example@harbor-notes.example",
        "Amount paid USD 48.00",
        "Currency: USD",
        "Service period: 2023-01-01 through 2023-12-31",
        "This document states the service period above and no status after 2023-12-31.",
    ],
    "originals/lumen-draft-onetime.pdf": [
        "Lumen Draft",
        "Document: purchase receipt",
        "Document id: SYN-LUMEN-ONCE-001",
        "Buyer email: mira.ellison.example@lumen-draft.example",
        "Amount paid USD 79.00",
        "Currency: USD",
        "Purchase kind: one-time",
        "License scope, original wording: perpetual seat for one person to use Lumen Draft 4 on up to two devices they personally operate. Not transferable. No subscription term.",
        "This payment does not widen the license scope written above.",
    ],
}

CSV = (
    "order_id,account_email,account_role,merchant,description,charged,currency,note\n"
    "SYN-C-WORK-2026-031,jordan.hale.example@paperlantern.example,work,Paperlantern Cloud,"
    "\"annual plan, store export row\",99.00,USD,"
    "\"The charged cell is 99.00 and conflicts with invoice amount paid 107.91 for the same order. "
    "This export does not decide which figure is paid.\"\n"
).encode("utf-8")

CANARY = (
    bytes([0xEF, 0xBB, 0xBF])
    + b"line1\r\nline2 \n"
    + "caf\u00e9".encode("utf-8")
    + bytes([0x00, 0xFF])
    + b"SENTINEL_BODY_9f3c2a"
)

ROLES = {
    "originals/v-2026-09-receipt.pdf": ("text-pdf", "application/pdf", ["monthly-yearly", "human-edit-reimport"]),
    "originals/v-2026-10-receipt.pdf": ("text-pdf", "application/pdf", ["monthly-yearly", "total-refund"]),
    "originals/v-2026-10-credit-note.pdf": ("text-pdf", "application/pdf", ["total-refund"]),
    "originals/c-personal-2026-invoice.pdf": (
        "text-pdf",
        "application/pdf",
        ["monthly-yearly", "same-merchant-two-accounts"],
    ),
    "originals/c-work-2026-invoice.pdf": (
        "text-pdf",
        "application/pdf",
        ["monthly-yearly", "same-merchant-two-accounts", "amount-conflict"],
    ),
    "originals/c-work-order-confirm.pdf": (
        "text-pdf",
        "application/pdf",
        ["unjudged-order-total", "amount-conflict"],
    ),
    "originals/c-cancel-notice-masked.pdf": (
        "text-pdf",
        "application/pdf",
        ["cancel-renewal-still-usable", "masked-unassigned"],
    ),
    "originals/harbor-notes-2023-invoice.pdf": (
        "text-pdf",
        "application/pdf",
        ["period-ended-current-unknown"],
    ),
    "originals/lumen-draft-onetime.pdf": ("text-pdf", "application/pdf", ["onetime-original-scope"]),
    "originals/store-export-work.csv": ("utf-8-csv", "text/csv", ["amount-conflict"]),
    "originals/canary/no-normalize.bin": ("byte-canary", "application/octet-stream", ["human-edit-reimport"]),
    "originals/renamed/NLV-september-statement-copy.pdf": (
        "text-pdf",
        "application/pdf",
        ["human-edit-reimport"],
    ),
}


def file_bytes() -> dict[str, bytes]:
    files = {path: build_pdf(lines) for path, lines in DOCUMENTS.items()}
    files["originals/store-export-work.csv"] = CSV
    files["originals/canary/no-normalize.bin"] = CANARY
    files["originals/renamed/NLV-september-statement-copy.pdf"] = files[
        "originals/v-2026-09-receipt.pdf"
    ]
    if files["originals/renamed/NLV-september-statement-copy.pdf"] != files[
        "originals/v-2026-09-receipt.pdf"
    ]:
        raise SystemExit("renamed copy diverged")
    return files


def byte_manifest(files: dict[str, bytes]) -> dict:
    entries = []
    for path in sorted(files):
        data = files[path]
        role, media, scenarios = ROLES[path]
        identical = None
        if path == "originals/renamed/NLV-september-statement-copy.pdf":
            identical = "originals/v-2026-09-receipt.pdf"
        entries.append(
            {
                "path": path,
                "format_role": role,
                "media_type": media,
                "sha256": hashlib.sha256(data).hexdigest(),
                "byte_length": len(data),
                "scenario_ids": scenarios,
                "byte_identical_to": identical,
                "normalization": "raw bytes; not a decoded or re-encoded document",
            }
        )
    return {
        "corpus_id": "subvault-synthetic-t0-v0",
        "synthetic_only": True,
        "disposable": True,
        "protection_preference": "unanswered",
        "primary_input_preference": "unanswered",
        "t1_must_not_interpret_documents": True,
        "files": entries,
    }


def check_quotes(base: Path, files: dict[str, bytes]) -> None:
    expectations = json.loads((base / "logical-expectations.json").read_text(encoding="utf-8"))
    if expectations.get("t1_implements_these_judgments") is not False:
        raise SystemExit("logical expectations must stay unimplemented in T1")
    seen = set()
    for case in expectations["cases"]:
        seen.add(case["id"])
        for quote in case["source_quotes"]:
            data = files[quote["path"]]
            needle = quote["text"].encode("utf-8")
            if needle not in data:
                raise SystemExit(f"quote missing from {quote['path']}: {quote['text']}")
    required = {
        "monthly-yearly",
        "same-merchant-two-accounts",
        "onetime-original-scope",
        "total-refund",
        "unjudged-order-total",
        "period-ended-current-unknown",
        "cancel-renewal-still-usable",
        "masked-unassigned",
        "amount-conflict",
        "human-edit-reimport",
    }
    if seen != required:
        raise SystemExit(f"scenario ids drifted: {sorted(seen)}")
    if b"SENTINEL_BODY_9f3c2a" not in files["originals/canary/no-normalize.bin"]:
        raise SystemExit("canary sentinel missing")
    if files["originals/store-export-work.csv"].count(b"\r\n") != 0:
        raise SystemExit("CSV fixture must keep LF line endings")


def write_tree(base: Path, files: dict[str, bytes]) -> None:
    if base.exists():
        shutil.rmtree(base)
    for path, data in files.items():
        target = base / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    (base / "SYNTHETIC_ONLY.txt").write_text(BANNER, encoding="utf-8")
    manifest = byte_manifest(files)
    (base / "byte-expectations.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=True) + "\n",
        encoding="utf-8",
    )


def compare(expected: Path, actual: Path) -> list[str]:
    errors = []
    expected_files = sorted(path.relative_to(expected).as_posix() for path in expected.rglob("*") if path.is_file())
    actual_files = sorted(path.relative_to(actual).as_posix() for path in actual.rglob("*") if path.is_file())
    if expected_files != actual_files:
        errors.append(f"file list differs\n expected={expected_files}\n actual={actual_files}")
    for rel in expected_files:
        if rel == "logical-expectations.json":
            continue
        left = (expected / rel).read_bytes()
        right = (actual / rel).read_bytes()
        if left != right:
            errors.append(f"bytes differ: {rel}")
    return errors


def main(argv: list[str]) -> int:
    files = file_bytes()
    check_quotes(CORPUS, files)
    mode = argv[1] if len(argv) > 1 else "write"
    if mode == "write":
        logical = (CORPUS / "logical-expectations.json").read_bytes()
        write_tree(CORPUS, files)
        (CORPUS / "logical-expectations.json").write_bytes(logical)
        print(f"wrote {CORPUS}")
        return 0
    if mode == "--check":
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp) / "corpus"
            write_tree(base, files)
            # Quote check uses the committed logical file; copy it for completeness.
            shutil.copyfile(CORPUS / "logical-expectations.json", base / "logical-expectations.json")
            errors = compare(CORPUS, base)
        if errors:
            print("\n".join(errors), file=sys.stderr)
            return 1
        print("synthetic corpus matches the generator")
        return 0
    print("usage: generate_corpus.py [write|--check]", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
