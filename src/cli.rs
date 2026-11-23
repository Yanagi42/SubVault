use std::path::PathBuf;

use serde_json::json;

use crate::crypto::{AuthInput, NewPassphraseInput, ProtectionMode};
use crate::engine::{
    self, CancelRequest, Hooks, ImportRequest, InitRequest, InjectAction, MarkerRequest,
    ReadRequest, RotatePassphraseRequest,
};
use crate::model::{Fail, Report, EXIT_USAGE};

struct Flags {
    vault: Option<PathBuf>,
    command_id: Option<String>,
    batch_id: Option<String>,
    media_type: Option<String>,
    source: Option<PathBuf>,
    display_name: Option<String>,
    source_id: Option<String>,
    value: Option<String>,
    out: Option<PathBuf>,
    generation: Option<i64>,
    kind: Option<String>,
    stop_at: Option<String>,
    checkpoint: Option<PathBuf>,
    inject: Option<String>,
    witness: Option<PathBuf>,
    fragment: Option<PathBuf>,
    format: Option<u8>,
    protection: Option<ProtectionMode>,
    passphrase_file: Option<PathBuf>,
    passphrase_stdin: bool,
    recovery_key_file: Option<PathBuf>,
    recovery_key_stdin: bool,
    recovery_key_out: Option<PathBuf>,
    new_passphrase_file: Option<PathBuf>,
    new_passphrase_stdin: bool,
    rotate_recovery_key: bool,
    acknowledge_plaintext: Option<bool>,
    fact_id: Option<String>,
    decision_id: Option<String>,
    expected_revision: Option<String>,
    amount_text: Option<String>,
    currency: Option<String>,
    period_start: Option<String>,
    period_end: Option<String>,
    vendor: Option<String>,
    plan_name: Option<String>,
    account_label: Option<String>,
    billing_cycle: Option<String>,
    category: Option<String>,
    bill_date: Option<String>,
    notes: Option<String>,
    asset_kind: Option<String>,
    warranty_end: Option<String>,
    license_scope: Option<String>,
    reimburse_status: Option<String>,
    actor_id: Option<String>,
    allow_partial: Option<bool>,
    package: Option<PathBuf>,
    redact_terms: Vec<String>,
    port: Option<u16>,
    no_browser: bool,
    help: bool,
}

impl Flags {
    fn empty() -> Self {
        Self {
            vault: None,
            command_id: None,
            batch_id: None,
            media_type: None,
            source: None,
            display_name: None,
            source_id: None,
            value: None,
            out: None,
            generation: None,
            kind: None,
            stop_at: None,
            checkpoint: None,
            inject: None,
            witness: None,
            fragment: None,
            format: None,
            protection: None,
            passphrase_file: None,
            passphrase_stdin: false,
            recovery_key_file: None,
            recovery_key_stdin: false,
            recovery_key_out: None,
            new_passphrase_file: None,
            new_passphrase_stdin: false,
            rotate_recovery_key: false,
            acknowledge_plaintext: None,
            fact_id: None,
            decision_id: None,
            expected_revision: None,
            amount_text: None,
            currency: None,
            period_start: None,
            period_end: None,
            vendor: None,
            plan_name: None,
            account_label: None,
            billing_cycle: None,
            category: None,
            bill_date: None,
            notes: None,
            asset_kind: None,
            warranty_end: None,
            license_scope: None,
            reimburse_status: None,
            actor_id: None,
            allow_partial: None,
            package: None,
            redact_terms: Vec::new(),
            port: None,
            no_browser: false,
            help: false,
        }
    }

    fn auth(&self) -> AuthInput {
        AuthInput {
            passphrase_file: self.passphrase_file.clone(),
            passphrase_stdin: self.passphrase_stdin,
            recovery_key_file: self.recovery_key_file.clone(),
            recovery_key_stdin: self.recovery_key_stdin,
            session_cipher: None,
            direct_passphrase: None,
            direct_recovery_key: None,
        }
    }
}

pub fn execute(args: Vec<String>) -> Report {
    if args.is_empty() || args[0] == "help" || args[0] == "--help" || args[0] == "-h" {
        return engine::help_report();
    }
    if args[0] == "version" || args[0] == "--version" || args[0] == "-V" {
        return engine::version_report();
    }
    let (command, flags) = match parse(&args) {
        Ok(parsed) => parsed,
        Err(fail) => return fail.into_report(),
    };
    if flags.help {
        if !is_known_command(&command) {
            return usage("unknown_command");
        }
        return engine::help_report();
    }
    if command == "review" && (flags.stop_at.is_some() || flags.inject.is_some()) {
        return usage("review_has_no_hook");
    }
    if command == "verify-package" && (flags.stop_at.is_some() || flags.inject.is_some()) {
        return usage("verify_package_has_no_hook");
    }
    if command == "redact" && (flags.stop_at.is_some() || flags.inject.is_some()) {
        return usage("redact_has_no_hook");
    }
    let hooks = match hooks_from(&flags) {
        Ok(hooks) => hooks,
        Err(fail) => return fail.into_report(),
    };
    if !flags.redact_terms.is_empty() && command != "redact" {
        return usage("redact_term_only_on_redact");
    }
    if flags.format.is_some() && command != "init" {
        return usage("format_only_on_init");
    }
    if flags.protection.is_some() && command != "init" {
        return usage("protection_only_on_init");
    }
    if flags.recovery_key_out.is_some() && command != "init" && command != "rotate-passphrase" {
        return usage("recovery_key_out_only_on_init_or_rotate");
    }
    if (flags.new_passphrase_file.is_some()
        || flags.new_passphrase_stdin
        || flags.rotate_recovery_key)
        && command != "rotate-passphrase"
    {
        return usage("rotate_flags_only_on_rotate_passphrase");
    }
    if flags.allow_partial.is_some() && command != "export" {
        return usage("allow_partial_only_on_export");
    }
    if flags.acknowledge_plaintext.is_some() && command != "export" {
        return usage("acknowledge_plaintext_only_on_export");
    }
    if flags.package.is_some() && command != "review" && command != "verify-package" {
        return usage("package_only_on_review");
    }
    let auth = flags.auth();
    match command.as_str() {
        "init" => {
            let Some(vault) = flags.vault else {
                return usage("vault_required");
            };
            engine::init_vault(InitRequest {
                vault,
                format: flags.format.unwrap_or(1),
                protection: flags.protection,
                auth,
                recovery_key_out: flags.recovery_key_out,
                hooks,
            })
        }
        "rotate-passphrase" => {
            let Some(vault) = flags.vault else {
                return usage("vault_required");
            };
            engine::rotate_passphrase(RotatePassphraseRequest {
                vault,
                auth,
                new_passphrase: NewPassphraseInput {
                    file: flags.new_passphrase_file,
                    stdin: flags.new_passphrase_stdin,
                    direct: None,
                },
                rotate_recovery_key: flags.rotate_recovery_key,
                recovery_key_out: flags.recovery_key_out,
            })
        }
        "app" => crate::workbench::run_cli_app(crate::workbench::WorkbenchOptions {
            port: flags
                .port
                .unwrap_or(crate::workbench::DEFAULT_WORKBENCH_PORT),
            open_browser: !flags.no_browser,
            initial_vault: flags.vault,
        }),
        "open" | "verify" | "recover" => {
            let Some(vault) = flags.vault else {
                return usage("vault_required");
            };
            if command == "recover" {
                engine::recover_vault(&vault, auth)
            } else if command == "verify" {
                engine::verify_vault(&vault, auth)
            } else {
                engine::open_vault(&vault, auth)
            }
        }
        "hold" => {
            let Some(vault) = flags.vault else {
                return usage("vault_required");
            };
            engine::hold_vault(&vault, hooks)
        }
        "import" => {
            let (Some(vault), Some(command_id), Some(media_type), Some(source)) = (
                flags.vault,
                flags.command_id,
                flags.media_type,
                flags.source,
            ) else {
                return usage("import_required_flags");
            };
            engine::import_file(ImportRequest {
                vault,
                command_id,
                batch_id: flags.batch_id,
                media_type,
                source,
                display_name: flags.display_name,
                auth,
                hooks,
            })
        }
        "read" => {
            let (Some(vault), Some(source_id), Some(out)) =
                (flags.vault, flags.source_id, flags.out)
            else {
                return usage("read_required_flags");
            };
            engine::read_source(ReadRequest {
                vault,
                source_id,
                out,
                auth,
            })
        }
        "marker" => {
            let (Some(vault), Some(command_id), Some(source_id), Some(value)) =
                (flags.vault, flags.command_id, flags.source_id, flags.value)
            else {
                return usage("marker_required_flags");
            };
            engine::set_marker(MarkerRequest {
                vault,
                command_id,
                source_id,
                value,
                auth,
                hooks,
            })
        }
        "html" => {
            let (Some(vault), Some(command_id), Some(source)) =
                (flags.vault, flags.command_id, flags.source)
            else {
                return usage("html_required_flags");
            };
            if flags.media_type.is_some() {
                return usage("html_media_type_fixed");
            }
            engine::submit_html(engine::HtmlRequest {
                vault,
                command_id,
                source,
                batch_id: flags.batch_id,
                auth,
                hooks,
            })
        }
        "pdf" => {
            let (Some(vault), Some(command_id), Some(source)) =
                (flags.vault, flags.command_id, flags.source)
            else {
                return usage("pdf_required_flags");
            };
            if flags.media_type.is_some() {
                return usage("pdf_media_type_fixed");
            }
            engine::submit_pdf(engine::PdfRequest {
                vault,
                command_id,
                source,
                batch_id: flags.batch_id,
                auth,
                hooks,
            })
        }
        "csv" => {
            let (Some(vault), Some(command_id), Some(source)) =
                (flags.vault, flags.command_id, flags.source)
            else {
                return usage("csv_required_flags");
            };
            if flags.media_type.is_some() {
                return usage("csv_media_type_fixed");
            }
            engine::submit_csv(engine::CsvRequest {
                vault,
                command_id,
                source,
                batch_id: flags.batch_id,
                auth,
                hooks,
            })
        }
        "annotate" => {
            let (Some(vault), Some(source_id)) = (flags.vault, flags.source_id) else {
                return usage("annotate_required_flags");
            };
            engine::annotate_source(engine::AnnotateRequest {
                vault,
                source_id,
                vendor: flags.vendor,
                plan_name: flags.plan_name,
                account_label: flags.account_label,
                billing_cycle: flags.billing_cycle,
                category: flags.category,
                bill_date: flags.bill_date,
                notes: flags.notes,
                asset_kind: flags.asset_kind,
                warranty_end: flags.warranty_end,
                license_key: None,
                license_scope: flags.license_scope,
                reimburse_status: flags.reimburse_status,
                auth,
            })
        }
        "record-fact" => {
            let (Some(vault), Some(command_id), Some(source_id), Some(actor_id)) = (
                flags.vault,
                flags.command_id,
                flags.source_id,
                flags.actor_id,
            ) else {
                return usage("record_fact_required_flags");
            };
            engine::record_manual_fact(engine::ManualFactRequest {
                vault,
                command_id,
                source_id,
                currency: flags.currency,
                amount_text: flags.amount_text,
                period_start: flags.period_start,
                period_end: flags.period_end,
                actor_id,
                auth,
            })
        }
        "fragment" => {
            let (Some(vault), Some(command_id), Some(fragment)) =
                (flags.vault, flags.command_id, flags.fragment)
            else {
                return usage("fragment_required_flags");
            };
            engine::submit_fragment(engine::FragmentRequest {
                vault,
                command_id,
                fragment,
                batch_id: flags.batch_id,
                auth,
                hooks,
            })
        }
        "candidates" => {
            let Some(vault) = flags.vault else {
                return usage("vault_required");
            };
            engine::query_candidates(&vault, flags.source_id, auth)
        }
        "facts" => {
            let Some(vault) = flags.vault else {
                return usage("vault_required");
            };
            engine::query_facts(&vault, flags.source_id, flags.fact_id, auth)
        }
        "review" => {
            let (Some(vault), Some(out)) = (flags.vault, flags.out) else {
                return usage("review_required_flags");
            };
            engine::review_sheet(&vault, &out, flags.package.as_deref(), auth)
        }
        "verify-package" => {
            let Some(package) = flags.package else {
                return usage("verify_package_required_flags");
            };
            crate::verify_package::verify_package(&package)
        }
        "redact" => {
            let Some(out) = flags.out else {
                return usage("redact_required_flags");
            };
            engine::redact_template(engine::RedactRequest {
                source: flags.source,
                vault: flags.vault,
                source_id: flags.source_id,
                out,
                redact_terms: flags.redact_terms,
                auth,
            })
        }
        "export" => {
            let (Some(vault), Some(out)) = (flags.vault, flags.out) else {
                return usage("export_required_flags");
            };
            engine::export_copy(engine::ExportRequest {
                vault,
                out,
                allow_partial: flags.allow_partial.unwrap_or(false),
                acknowledge_plaintext: flags.acknowledge_plaintext.unwrap_or(false),
                auth,
                hooks,
            })
        }
        "correct" => {
            let (
                Some(vault),
                Some(command_id),
                Some(fact_id),
                Some(expected_revision),
                Some(amount_text),
                Some(actor_id),
            ) = (
                flags.vault,
                flags.command_id,
                flags.fact_id,
                flags.expected_revision,
                flags.amount_text,
                flags.actor_id,
            )
            else {
                return usage("correct_required_flags");
            };
            engine::correct_fact(engine::CorrectRequest {
                vault,
                command_id,
                fact_id,
                expected_revision,
                amount_text,
                currency: flags.currency,
                actor_id,
                auth,
                hooks,
            })
        }
        "undo" => {
            let (
                Some(vault),
                Some(command_id),
                Some(decision_id),
                Some(expected_revision),
                Some(actor_id),
            ) = (
                flags.vault,
                flags.command_id,
                flags.decision_id,
                flags.expected_revision,
                flags.actor_id,
            )
            else {
                return usage("undo_required_flags");
            };
            engine::undo_decision(engine::UndoRequest {
                vault,
                command_id,
                decision_id,
                expected_revision,
                actor_id,
                auth,
                hooks,
            })
        }
        "cancel" => {
            let (Some(vault), Some(command_id), Some(generation)) =
                (flags.vault, flags.command_id, flags.generation)
            else {
                return usage("cancel_required_flags");
            };
            engine::cancel_command(CancelRequest {
                vault,
                command_id,
                generation,
                kind: flags.kind.unwrap_or_else(|| "preservation".into()),
                auth,
            })
        }
        _ => usage("unknown_command"),
    }
}

fn parse(args: &[String]) -> Result<(String, Flags), Fail> {
    let command = args[0].clone();
    let mut flags = Flags::empty();
    let mut index = 1;
    while index < args.len() {
        let flag = args[index].as_str();
        match flag {
            "--passphrase-stdin" => {
                flags.passphrase_stdin = true;
                index += 1;
                continue;
            }
            "--recovery-key-stdin" => {
                flags.recovery_key_stdin = true;
                index += 1;
                continue;
            }
            "--new-passphrase-stdin" => {
                flags.new_passphrase_stdin = true;
                index += 1;
                continue;
            }
            "--rotate-recovery-key" => {
                flags.rotate_recovery_key = true;
                index += 1;
                continue;
            }
            "--no-browser" | "--no-open" => {
                flags.no_browser = true;
                index += 1;
                continue;
            }
            "--help" | "-h" => {
                flags.help = true;
                index += 1;
                continue;
            }
            "--passphrase" | "--recovery-key" | "--new-passphrase" => {
                return Err(
                    Fail::new(EXIT_USAGE, "Usage", "passphrase_cli_arg_forbidden")
                        .extra("flag", json!(flag)),
                );
            }
            _ if flag.starts_with("--passphrase=")
                || flag.starts_with("--recovery-key=")
                || flag.starts_with("--new-passphrase=") =>
            {
                let name = flag.split('=').next().unwrap_or(flag);
                return Err(
                    Fail::new(EXIT_USAGE, "Usage", "passphrase_cli_arg_forbidden")
                        .extra("flag", json!(name)),
                );
            }
            _ => {}
        }
        let value = args.get(index + 1).cloned().ok_or_else(|| {
            Fail::new(EXIT_USAGE, "Usage", "missing_flag_value").extra("flag", json!(flag))
        })?;
        match flag {
            "--vault" => flags.vault = Some(PathBuf::from(value)),
            "--command-id" => flags.command_id = Some(value),
            "--batch-id" => flags.batch_id = Some(value),
            "--media-type" => flags.media_type = Some(value),
            "--source" => flags.source = Some(PathBuf::from(value)),
            "--display-name" => flags.display_name = Some(value),
            "--source-id" => flags.source_id = Some(value),
            "--value" => flags.value = Some(value),
            "--out" => flags.out = Some(PathBuf::from(value)),
            "--generation" => {
                flags.generation = Some(
                    value
                        .parse::<i64>()
                        .map_err(|_| Fail::new(EXIT_USAGE, "Usage", "bad_generation"))?,
                );
            }
            "--kind" => flags.kind = Some(value),
            "--stop-at" => flags.stop_at = Some(value),
            "--checkpoint" => flags.checkpoint = Some(PathBuf::from(value)),
            "--inject" => flags.inject = Some(value),
            "--witness" => flags.witness = Some(PathBuf::from(value)),
            "--fragment" => flags.fragment = Some(PathBuf::from(value)),
            "--fact-id" => flags.fact_id = Some(value),
            "--decision-id" => flags.decision_id = Some(value),
            "--expected-revision" => flags.expected_revision = Some(value),
            "--amount-text" => flags.amount_text = Some(value),
            "--currency" => flags.currency = Some(value),
            "--period-start" => flags.period_start = Some(value),
            "--period-end" => flags.period_end = Some(value),
            "--vendor" => flags.vendor = Some(value),
            "--plan" => flags.plan_name = Some(value),
            "--account" => flags.account_label = Some(value),
            "--cycle" => flags.billing_cycle = Some(value),
            "--category" => flags.category = Some(value),
            "--bill-date" => flags.bill_date = Some(value),
            "--notes" => flags.notes = Some(value),
            "--asset-kind" => flags.asset_kind = Some(value),
            "--warranty-end" => flags.warranty_end = Some(value),
            "--license-scope" => flags.license_scope = Some(value),
            "--reimburse-status" => flags.reimburse_status = Some(value),
            "--actor-id" => flags.actor_id = Some(value),
            "--package" => flags.package = Some(PathBuf::from(value)),
            "--redact-term" => flags.redact_terms.push(value),
            "--port" => {
                flags.port = Some(
                    value
                        .parse::<u16>()
                        .map_err(|_| Fail::new(EXIT_USAGE, "Usage", "bad_port"))?,
                );
            }
            "--passphrase-file" => flags.passphrase_file = Some(PathBuf::from(value)),
            "--recovery-key-file" => flags.recovery_key_file = Some(PathBuf::from(value)),
            "--recovery-key-out" => flags.recovery_key_out = Some(PathBuf::from(value)),
            "--new-passphrase-file" => flags.new_passphrase_file = Some(PathBuf::from(value)),
            "--protection" => {
                flags.protection = Some(
                    ProtectionMode::from_flag(&value)
                        .ok_or_else(|| Fail::new(EXIT_USAGE, "Usage", "bad_protection"))?,
                );
            }
            "--acknowledge-plaintext" => {
                if value != "yes" {
                    return Err(Fail::new(EXIT_USAGE, "Usage", "bad_acknowledge_plaintext"));
                }
                flags.acknowledge_plaintext = Some(true);
            }
            "--allow-partial" => {
                if value != "yes" {
                    return Err(Fail::new(EXIT_USAGE, "Usage", "bad_allow_partial"));
                }
                flags.allow_partial = Some(true);
            }
            "--format" => {
                flags.format = Some(match value.as_str() {
                    "1" => 1,
                    "2" => 2,
                    "3" => 3,
                    "4" => 4,
                    "5" => 5,
                    "6" => 6,
                    _ => return Err(Fail::new(EXIT_USAGE, "Usage", "bad_format")),
                });
            }
            _ => {
                return Err(
                    Fail::new(EXIT_USAGE, "Usage", "unknown_flag").extra("flag", json!(flag))
                );
            }
        }
        index += 2;
    }
    Ok((command, flags))
}

fn hooks_from(flags: &Flags) -> Result<Hooks, Fail> {
    if flags.stop_at.is_some() && flags.checkpoint.is_none() {
        return Err(Fail::new(EXIT_USAGE, "Usage", "checkpoint_required"));
    }
    if flags.inject.is_some() && flags.witness.is_none() {
        return Err(Fail::new(EXIT_USAGE, "Usage", "witness_required"));
    }
    let (inject_action, inject_point) = match &flags.inject {
        None => (None, None),
        Some(raw) => {
            let Some((action, point)) = raw.split_once(':') else {
                return Err(Fail::new(EXIT_USAGE, "Usage", "bad_inject"));
            };
            let action = match action {
                "enospc" => InjectAction::Enospc,
                "io_error" => InjectAction::IoError,
                _ => return Err(Fail::new(EXIT_USAGE, "Usage", "bad_inject_action")),
            };
            if point.is_empty() {
                return Err(Fail::new(EXIT_USAGE, "Usage", "bad_inject"));
            }
            (Some(action), Some(point.to_string()))
        }
    };
    Ok(Hooks {
        stop_at: flags.stop_at.clone(),
        checkpoint: flags.checkpoint.clone(),
        inject_action,
        inject_point,
        witness: flags.witness.clone(),
    })
}

fn is_known_command(command: &str) -> bool {
    matches!(
        command,
        "help"
            | "version"
            | "init"
            | "rotate-passphrase"
            | "app"
            | "open"
            | "verify"
            | "recover"
            | "hold"
            | "import"
            | "read"
            | "marker"
            | "html"
            | "pdf"
            | "csv"
            | "annotate"
            | "record-fact"
            | "fragment"
            | "candidates"
            | "facts"
            | "review"
            | "verify-package"
            | "redact"
            | "export"
            | "correct"
            | "undo"
            | "cancel"
    )
}

fn usage(reason: &str) -> Report {
    Fail::new(EXIT_USAGE, "Usage", reason).into_report()
}
