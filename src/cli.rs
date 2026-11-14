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
    let hooks = match hooks_from(&flags) {
        Ok(hooks) => hooks,
        Err(fail) => return fail.into_report(),
    };
    match command.as_str() {
        "init" => {
            let Some(vault) = flags.vault else {
                return usage("vault_required");
            };
            engine::init_vault(&vault)
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
