use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "carl")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Serve,
    Auth,
    Pair,
    Doctor,
    Sessions,
}
