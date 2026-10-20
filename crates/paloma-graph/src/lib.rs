//! A small, typed client for the WhatsApp Cloud API on `graph.facebook.com`.
//!
//! Every endpoint function in [`api`] is pure: it builds a [`Request`] describing the
//! call without performing it. The [`Client`] then either executes that request or the
//! caller renders it for `--dry-run`. Keeping requests as data is what lets dry-run be
//! enforced in one place instead of in every command, and lets manifests plan writes
//! that are applied later through the same path.

pub mod api;
pub mod client;
pub mod error_map;
pub mod paging;
pub mod request;
pub mod retry;

pub use client::{Client, ClientConfig, Response, RetryNotice};
pub use request::{Auth, Body, FormPart, Method, PartValue, Request, Target};

/// The Graph API host.
pub const GRAPH_BASE: &str = "https://graph.facebook.com";

/// The Graph API version used when no profile, env var or flag picks one.
pub const DEFAULT_API_VERSION: &str = "v26.0";

pub mod callback;
pub mod webhook_samples;
