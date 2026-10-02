//! Where paloma's settings come from.
//!
//! - [`store`]: the `config.toml` file — profiles and the default profile — read with
//!   `toml` and written with `toml_edit` so user comments and ordering survive.
//! - [`secrets`]: tokens live in the OS keyring behind `keyring:<account>` references,
//!   never in plaintext config or argv.
//! - [`resolve`]: the one place that layers flag/env overrides over the profile and
//!   produces an immutable [`Context`], with credential lookups deferred until a command
//!   actually needs them.

pub mod profile;
pub mod resolve;
pub mod secrets;
pub mod store;

pub use profile::{Profile, ProfileKey};
pub use resolve::{Context, Overrides};
pub use secrets::{KeyringStore, MemoryStore, SecretStore};
pub use store::{Config, ConfigFile};
