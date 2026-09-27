use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(version, about = "Declaratively manage agent skills for a project")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a skill-lock.toml template in the current directory
    Init,
}
