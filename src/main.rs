mod cli;
mod init;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Init => run_init(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_init() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::current_dir()?;
    init::run(&dir)?;
    println!("Created {}", init::MANIFEST_FILE);
    Ok(())
}
