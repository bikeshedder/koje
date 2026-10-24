//! `koje` – command line client.
//!
//! The binary is a thin layer: parse arguments, call into `koje-core` (and,
//! from milestone M3 on, talk to `kojed`). Unimplemented commands fail with a
//! pointer to the milestone that delivers them (`docs/plan/`).

mod apps;
mod bar;
mod cli;
mod commands;
mod doctor;
mod dotfiles;
mod hostbus;
mod kojed;
mod layout;
mod memory;
mod mounts;
mod prompt;
mod runtime;
mod setup;
mod terminal;

use std::process::ExitCode;

use clap::Parser;
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    let cli = cli::Cli::parse();
    init_tracing(cli.verbose);

    match commands::run(&cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("koje: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn init_tracing(verbose: u8) {
    let default = match verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}
