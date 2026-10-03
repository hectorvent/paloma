use clap::{Args, CommandFactory};
use clap_complete::Shell;
use serde_json::json;

use crate::app::App;
use crate::cli::Cli;
use crate::output::Output;

pub fn version(app: &App) {
    app.out.emit(
        &Output::new(json!({
            "paloma": env!("CARGO_PKG_VERSION"),
            "api_version_default": paloma_graph::DEFAULT_API_VERSION,
        }))
        .id(Some(env!("CARGO_PKG_VERSION").to_owned())),
    );
}

#[derive(Debug, Args)]
pub struct Completion {
    /// Target shell.
    #[arg(value_enum)]
    shell: Shell,
}

impl Completion {
    pub fn run(&self, app: &App) {
        let mut buf = Vec::new();
        clap_complete::generate(self.shell, &mut Cli::command(), "paloma", &mut buf);
        app.out.stdout(&String::from_utf8_lossy(&buf));
    }
}
