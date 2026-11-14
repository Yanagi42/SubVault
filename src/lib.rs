mod cli;
mod engine;
mod facts;
mod fsutil;
mod ids;
mod inspect;
mod journal;
mod lockfile;
mod model;
mod store;

pub fn execute(args: Vec<String>) -> model::Report {
    cli::execute(args)
}
