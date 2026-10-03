//! Developer tasks: `cargo xtask <task>`.
//!
//! - `dist-assets [DIR]`: man pages and shell completions for packaging (default
//!   `target/dist-assets`), generated from the same clap definitions as the binary.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::CommandFactory;
use clap_complete::Shell;
use paloma_cli::cli::Cli;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("dist-assets") => dist_assets(
            &args
                .get(1)
                .map_or_else(|| PathBuf::from("target/dist-assets"), PathBuf::from),
        ),
        _ => {
            eprintln!("usage: cargo xtask dist-assets [DIR]");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

fn dist_assets(dir: &Path) -> std::io::Result<()> {
    let man = dir.join("man");
    let completions = dir.join("completions");
    std::fs::create_dir_all(&man)?;
    std::fs::create_dir_all(&completions)?;

    let cmd = Cli::command();
    clap_mangen::generate_to(cmd.clone(), &man)?;
    for shell in [
        Shell::Bash,
        Shell::Zsh,
        Shell::Fish,
        Shell::PowerShell,
        Shell::Elvish,
    ] {
        clap_complete::generate_to(shell, &mut cmd.clone(), "paloma", &completions)?;
    }
    println!(
        "wrote man pages to {} and completions to {}",
        man.display(),
        completions.display()
    );
    Ok(())
}
