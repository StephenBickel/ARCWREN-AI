use std::{error::Error, fmt};

use carl::cli::{Cli, Command};
use clap::Parser;

#[derive(Debug)]
struct NotImplemented(&'static str);

impl fmt::Display for NotImplemented {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} is not implemented", self.0)
    }
}

impl Error for NotImplemented {}

fn run(command: Command) -> Result<(), NotImplemented> {
    let command = match command {
        Command::Serve => "serve",
        Command::Auth => "auth",
        Command::Pair => "pair",
        Command::Doctor => "doctor",
        Command::Sessions => "sessions",
    };

    Err(NotImplemented(command))
}

fn main() -> Result<(), NotImplemented> {
    run(Cli::parse().command)
}
