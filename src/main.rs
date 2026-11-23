use std::io::{self, Write};
use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || matches!(args[0].as_str(), "--help" | "-h") {
        let _ = deliver(CLI_USAGE);
        exit(0);
    }
    if matches!(args[0].as_str(), "--version" | "-V") {
        let sha = env!("SUBVAULT_GIT_SHA");
        let line = if sha.is_empty() || sha == "unknown" {
            format!("subvault {}", env!("CARGO_PKG_VERSION"))
        } else {
            let short: String = sha.chars().take(8).collect();
            format!("subvault {} ({short})", env!("CARGO_PKG_VERSION"))
        };
        let _ = deliver(&line);
        exit(0);
    }
    let report = subvault::execute(args);
    let text = serde_json::to_string_pretty(&report.body)
        .unwrap_or_else(|_| "{\"outcome\":\"Usage\",\"reason\":\"report_encoding\"}".to_string());
    if deliver(&text).is_ok() {
        exit(report.code);
    }
    // The receipt did not reach the caller. Do not panic, do not exit 0, and
    // do not exit 15: a failed print is not proof the command was uncommitted.
    let mut err = io::stderr().lock();
    let _ = err.write_all(b"stdout_undelivered; result unknown; retry the same command id\n");
    let _ = err.flush();
    exit(subvault::EXIT_UNCERTAIN);
}

fn deliver(text: &str) -> io::Result<()> {
    // stdout is line-buffered. A pretty JSON write used to fail inside
    // println and panic before the explicit flush. Buffering here keeps a
    // short receipt off the device until flush, so that flush error is
    // returned. A receipt larger than this buffer fails in write_all instead.
    let mut out = io::BufWriter::new(io::stdout().lock());
    out.write_all(text.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()
}

const CLI_USAGE: &str = "\
SubVault — local-first subscription, invoice, license & warranty vault

Usage:
  subvault app [--vault PATH] [--port PORT] [--no-browser|--no-open]
  subvault init --vault PATH --format 6 --protection passphrase|os-disk [AUTH] [--recovery-key-out PATH]
  subvault <command> --vault PATH [options]

Core commands:
  app               Launch the local loopback workbench UI (127.0.0.1)
  init              Create a new vault (Format 6 for real encrypted/os-disk vaults)
  open / verify     Inspect and cryptographically verify vault integrity
  pdf / html / csv  Ingest and extract bills, invoices, or bank/Alipay/WeChat CSVs
  import / read     Store or extract arbitrary original files (e.g., screenshots)
  annotate          Attach vendor, plan, cycle, warranty, or reimbursement metadata
  record-fact       Manually record amount or service period facts
  correct / undo    Record auditable human corrections or compensating undos
  facts / candidates  Query stored facts and extraction candidates
  review            Render a plain-text review sheet outside the vault
  export            Export an offline-exit-1 directory package outside the vault
  verify-package    Independently verify an exported package without the database
  redact            Generate a PII-redacted template from a file or vault source
  rotate-passphrase Re-wrap vault keyslots with a new passphrase or recovery key
  recover / cancel  Reconcile interrupted journals or cancel pending operations

Authentication (Format 6 passphrase vaults; never pass secrets as CLI flags):
  --passphrase-file PATH | --passphrase-stdin | SUBVAULT_PASSPHRASE_FILE
  --recovery-key-file PATH | --recovery-key-stdin | SUBVAULT_RECOVERY_KEY_FILE

Machine-readable legacy JSON metadata:
  subvault help     Emit structured JSON command catalog
  subvault version  Emit structured JSON capability manifest";
