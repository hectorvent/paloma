//! Contract tests: run the real `paloma` binary against a stubbed Graph API and assert
//! what users are promised: exit codes, what lands on stdout vs stderr, and the exact
//! request sent (or printed under `--dry-run`).

// A contract test reads as one scenario, setup to assertions; splitting it hides the story.
#![allow(clippy::too_many_lines)]

mod account;
mod auth_config;
mod block;
mod calling;
mod common;
mod contract;
mod flow;
mod group;
mod manifest;
mod media;
mod message;
mod number;
mod profile;
mod qr;
mod retry;
mod secrets;
mod send;
mod send_interactive;
mod send_template;
mod template;
mod webhook;
