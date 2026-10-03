//! The `paloma` command-line interface: the clap tree, per-invocation wiring and the
//! output contract. The binary in `main.rs` is a thin shell over this library, which
//! also lets `xtask` render man pages and completions from the same definitions.

pub mod app;
pub mod cli;
pub mod commands;
pub mod output;
