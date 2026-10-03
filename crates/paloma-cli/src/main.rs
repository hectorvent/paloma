//! `paloma`: drive the real WhatsApp Cloud API from your terminal and your CI.

use std::process::ExitCode;

use clap::Parser;
use paloma_cli::app::App;
use paloma_cli::cli::Cli;
use paloma_core::ExitCode as Code;

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        // Help and --version exit 0; every parse failure exits 2 (the usage contract).
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(u8::try_from(e.exit_code()).unwrap_or(Code::Usage.code()));
        }
    };
    init_tracing(cli.globals.verbose);

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: cannot start async runtime: {e}");
            return Code::Generic.into();
        }
    };

    let app = App::new(cli.globals);
    match runtime.block_on(cli.command.run(&app)) {
        Ok(()) => Code::Ok.into(),
        Err(e) => {
            app.out.error(&e);
            e.exit_code().into()
        }
    }
}

/// `-v` logs each request line, `-vv` adds request/response bodies. `PALOMA_LOG`
/// takes a full `tracing` filter for anything finer.
fn init_tracing(verbosity: u8) {
    use tracing_subscriber::EnvFilter;
    let default = match verbosity {
        0 => "warn",
        1 => "warn,paloma_graph=debug,paloma=debug",
        _ => "warn,paloma_graph=trace,paloma=trace",
    };
    let filter = EnvFilter::try_from_env("PALOMA_LOG").unwrap_or_else(|_| EnvFilter::new(default));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .without_time()
        .with_target(false)
        .init();
}
