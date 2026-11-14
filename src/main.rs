use std::io::{self, Write};
use std::process::exit;

fn main() {
    let report = subvault::execute(std::env::args().skip(1).collect());
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
