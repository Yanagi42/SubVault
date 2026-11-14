mod body;
mod cli;
mod engine;
mod facts;
mod fsutil;
mod ids;
mod inspect;
mod journal;
mod lock_contract;
mod lockfile;
mod model;
mod platform;
mod store;
#[cfg(windows)]
mod windows_lock;

pub use model::EXIT_UNCERTAIN;

pub fn execute(args: Vec<String>) -> model::Report {
    cli::execute(args)
}
