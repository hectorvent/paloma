//! The manifest kinds and their reconcile plans.
//!
//! Kinds use enum dispatch rather than trait objects: async methods stay plain `async fn`,
//! the full set is visible in one place, and adding a kind means adding a variant here
//! and a module under `kinds/`.

use paloma_config::Context;
use paloma_core::{Error, Result};
use paloma_graph::{Client, Request};
use serde_json::Value;

use crate::kinds;

/// The outcome of reconciling one manifest against live config.
#[derive(Debug, Clone, Default)]
pub struct KindPlan {
    /// Human-readable changes, e.g. `status: "DISABLED" -> "ENABLED"`.
    pub changes: Vec<String>,
    /// The body to show under `--dry-run`.
    pub preview: Option<Value>,
    /// The writes that bring live config in line, in order.
    pub requests: Vec<Request>,
}

impl KindPlan {
    pub fn synced() -> Self {
        Self::default()
    }

    pub fn in_sync(&self) -> bool {
        self.requests.is_empty()
    }
}

/// What a kind needs to read live state: the client and the (per-manifest) context.
#[derive(Debug, Clone, Copy)]
pub struct Cx<'a> {
    pub client: &'a Client,
    pub ctx: &'a Context,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Calling,
    Commerce,
    ConversationalAutomation,
    Number,
    Profile,
    Template,
    Webhook,
}

impl Kind {
    pub const ALL: [Self; 7] = [
        Self::Calling,
        Self::Commerce,
        Self::ConversationalAutomation,
        Self::Number,
        Self::Profile,
        Self::Template,
        Self::Webhook,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Calling => "Calling",
            Self::Commerce => "Commerce",
            Self::ConversationalAutomation => "ConversationalAutomation",
            Self::Number => "Number",
            Self::Profile => "Profile",
            Self::Template => "Template",
            Self::Webhook => "Webhook",
        }
    }

    /// Resolve a `kind` name, case-insensitively.
    pub fn parse(name: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|k| k.name().eq_ignore_ascii_case(name.trim()))
            .ok_or_else(|| {
                let known: Vec<_> = Self::ALL.iter().map(|k| k.name()).collect();
                Error::validation(format!(
                    "unknown kind '{name}' (known: {})",
                    known.join(", ")
                ))
            })
    }

    /// Whether the kind lives on a phone number (else on the WABA/app).
    pub const fn phone_scoped(self) -> bool {
        !matches!(self, Self::Template | Self::Webhook)
    }

    /// Every writable spec field (camelCase) with a typed stub value; unset ones are
    /// rendered as comments on export.
    pub fn spec_template(self) -> Value {
        match self {
            Self::Calling => kinds::calling::template(),
            Self::Commerce => kinds::commerce::template(),
            Self::ConversationalAutomation => kinds::conversational_automation::template(),
            Self::Number => kinds::number::template(),
            Self::Profile => kinds::profile::template(),
            Self::Template => kinds::template::template(),
            Self::Webhook => kinds::webhook::template(),
        }
    }

    /// Current state as 0..n snake_case specs.
    pub async fn export(self, cx: Cx<'_>) -> Result<Vec<Value>> {
        match self {
            Self::Calling => kinds::calling::export(cx).await,
            Self::Commerce => kinds::commerce::export(cx).await,
            Self::ConversationalAutomation => kinds::conversational_automation::export(cx).await,
            Self::Number => kinds::number::export(cx).await,
            Self::Profile => kinds::profile::export(cx).await,
            Self::Template => kinds::template::export(cx).await,
            Self::Webhook => kinds::webhook::export(cx).await,
        }
    }

    /// Reconcile a desired snake_case spec against live config.
    pub async fn plan(self, cx: Cx<'_>, spec: &Value) -> Result<KindPlan> {
        match self {
            Self::Calling => kinds::calling::plan(cx, spec).await,
            Self::Commerce => kinds::commerce::plan(cx, spec).await,
            Self::ConversationalAutomation => {
                kinds::conversational_automation::plan(cx, spec).await
            }
            Self::Number => kinds::number::plan(cx, spec),
            Self::Profile => kinds::profile::plan(cx, spec).await,
            Self::Template => kinds::template::plan(cx, spec).await,
            Self::Webhook => kinds::webhook::plan(cx, spec).await,
        }
    }
}

/// The idempotent subset-diff reconcile shared by singleton kinds (one config object
/// per phone number): in sync when every desired field already matches.
pub(crate) fn singleton_plan(
    desired: &Value,
    current: &Value,
    mut preview: Value,
    request: Request,
) -> KindPlan {
    let empty = desired.as_object().is_some_and(serde_json::Map::is_empty);
    if empty || crate::diff::in_sync(desired, Some(current)) {
        return KindPlan::synced();
    }
    let changes = crate::diff::changes(desired, Some(current));
    preview = paloma_graph::request::redact_json(&preview);
    KindPlan {
        changes,
        preview: Some(preview),
        requests: vec![request],
    }
}

pub(crate) async fn read(cx: Cx<'_>, req: &Request) -> Result<Value> {
    cx.client.execute(req).await?.json()
}
