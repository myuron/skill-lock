mod cli;
mod git;
mod hash;
mod init;
mod lock;
mod lockfile;
mod manifest;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Init => run_init(),
        Command::Lock => run_lock(),
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

fn run_lock() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::current_dir()?;
    let report = lock::run(&dir, &git::GitFetcher::github())?;
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    for skill in &report.skills {
        let status = match skill.status {
            lock::Status::Unchanged => "Unchanged",
            lock::Status::Locked => "Locked",
        };
        println!(
            "{status} {} ({}@{})",
            skill.name,
            skill.github,
            &skill.commit[..7.min(skill.commit.len())]
        );
    }
    for name in &report.removed {
        println!("Removed {name}");
    }
    println!("Wrote {}", lockfile::LOCK_FILE);
    Ok(())
}
