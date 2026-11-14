mod cli;
mod engine;
mod fsutil;
mod ids;
mod lockfile;
mod model;
mod store;

pub fn execute(args: Vec<String>) -> model::Report {
    cli::execute(args)
}
