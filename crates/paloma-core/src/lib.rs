//! Domain primitives shared by every paloma crate.
//!
//! This crate is deliberately dependency-light and synchronous: it holds the pieces of
//! the CLI contract (exit codes, error taxonomy) and the local rules that must run
//! *before* any network call (pre-flight validation, the 24-hour window policy), so
//! they can be unit-tested without a runtime or a Graph API stub.

pub mod compliance;
pub mod credentials;
pub mod error;
pub mod limits;
pub mod template;
pub mod validate;

pub use credentials::Credentials;
pub use error::{Error, ExitCode, Result};
pub use template::TemplateCategory;
