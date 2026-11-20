mod body;
mod cli;
mod engine;
mod facts;
mod file_identity;
mod fsutil;
mod ids;
mod inspect;
mod journal;
mod lock_contract;
mod lockfile;
mod model;
mod pdf_parse;
mod platform;
mod store;
mod verify_package;
#[cfg(windows)]
mod windows_file_id;
#[cfg(windows)]
mod windows_lock;

pub use model::EXIT_UNCERTAIN;

pub fn execute(args: Vec<String>) -> model::Report {
    cli::execute(args)
}
