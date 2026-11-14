use std::path::PathBuf;

use serde_json::json;

use crate::engine::{
    self, CancelRequest, Hooks, ImportRequest, InjectAction, MarkerRequest, ReadRequest,
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
    fact_id: Option<String>,
    decision_id: Option<String>,
    expected_revision: Option<String>,
    amount_text: Option<String>,
    actor_id: Option<String>,
    allow_partial: Option<bool>,
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
            fact_id: None,
            decision_id: None,
            expected_revision: None,
            amount_text: None,
            actor_id: None,
            allow_partial: None,
        }
    }
}

pub fn execute(args: Vec<String>) -> Report {
    if args.is_empty() || args[0] == "help" || args[0] == "--help" || args[0] == "-h" {
        return engine::help_report();
    }
    if args[0] == "version" {
        return engine::version_report();
    }
    let (command, flags) = match parse(&args) {
        Ok(parsed) => parsed,
        Err(fail) => return fail.into_report(),
    };
    if command == "review" && (flags.stop_at.is_some() || flags.inject.is_some()) {
        return usage("review_has_no_hook");
    }
    let hooks = match hooks_from(&flags) {
        Ok(hooks) => hooks,
        Err(fail) => return fail.into_report(),
    };
    if flags.format.is_some() && command != "init" {
        return usage("format_only_on_init");
    }
    if flags.allow_partial.is_some() && command != "export" {
        return usage("allow_partial_only_on_export");
    }
    match command.as_str() {
        "init" => {
            let Some(vault) = flags.vault else {
                return usage("vault_required");
            };
            engine::init_vault(&vault, flags.format.unwrap_or(1), hooks)
        }
        "open" | "verify" | "recover" => {
            let Some(vault) = flags.vault else {
                return usage("vault_required");
            };
            if command == "recover" {
                engine::recover_vault(&vault)
            } else if command == "verify" {
                engine::verify_vault(&vault)
            } else {
                engine::open_vault(&vault)
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
                hooks,
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
                hooks,
            })
        }
        "candidates" => {
            let Some(vault) = flags.vault else {
                return usage("vault_required");
            };
            engine::query_candidates(&vault, flags.source_id)
        }
        "facts" => {
            let Some(vault) = flags.vault else {
                return usage("vault_required");
            };
            engine::query_facts(&vault, flags.source_id, flags.fact_id)
        }
        "review" => {
            let (Some(vault), Some(out)) = (flags.vault, flags.out) else {
                return usage("review_required_flags");
            };
            engine::review_sheet(&vault, &out)
        }
        "export" => {
            let (Some(vault), Some(out)) = (flags.vault, flags.out) else {
                return usage("export_required_flags");
            };
            engine::export_copy(engine::ExportRequest {
                vault,
                out,
                allow_partial: flags.allow_partial.unwrap_or(false),
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
                actor_id,
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
            "--actor-id" => flags.actor_id = Some(value),
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

fn usage(reason: &str) -> Report {
    Fail::new(EXIT_USAGE, "Usage", reason).into_report()
}
