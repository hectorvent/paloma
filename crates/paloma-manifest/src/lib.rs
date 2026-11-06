//! K8s-style YAML manifests layered over the Graph API: `export` renders live config as
//! manifests, `diff` reports drift, `apply` writes only what drifted.
//!
//! Specs are **camelCase** at the manifest boundary and **snake_case** everywhere inside
//! a kind; [`case`] converts losslessly with no per-field mapping. `${...}` placeholders
//! are expanded by [`interpolate`] *before* case conversion, so kinds never see one.
//!
//! The pure pieces ([`case`], [`diff`], [`interpolate`], [`yaml`], [`manifest`], [`plan`])
//! are synchronous and need no client, so they unit-test without a runtime.

pub mod case;
pub mod diff;
pub mod interpolate;
pub mod kind;
mod kinds;
pub mod manifest;
pub mod plan;
pub mod yaml;

pub use kind::{Kind, KindPlan};
pub use manifest::{API_VERSION, Manifest};
pub use plan::Scope;
