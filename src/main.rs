fn main() {
    let report = subvault::execute(std::env::args().skip(1).collect());
    println!(
        "{}",
        serde_json::to_string_pretty(&report.body).unwrap_or_else(|_| {
            "{\"outcome\":\"Usage\",\"reason\":\"report_encoding\"}".to_string()
        })
    );
    let _ = std::io::Write::flush(&mut std::io::stdout());
    std::process::exit(report.code);
}
