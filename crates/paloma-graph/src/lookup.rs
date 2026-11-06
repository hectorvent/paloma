//! Reads that other requests depend on (which app, which callback, which template). They
//! execute through a [`Client`], so they live here rather than in the pure [`crate::api`]
//! builders, and run even under `--dry-run` so the printed request is the real one.

use paloma_core::{Error, Result};
use serde_json::Value;

use crate::api::{templates, webhook};
use crate::{Client, paging};

/// Which app a request acts for, and how it was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppChoice {
    /// The user gave it (`--app-id` or `PALOMA_APP_ID`).
    Given(String),
    /// The first of `subscribed` apps on the WABA. A WABA can have several, and the first
    /// may not be the token's, so callers say which one they picked.
    Discovered { id: String, subscribed: usize },
}

impl AppChoice {
    pub fn id(&self) -> &str {
        match self {
            Self::Given(id) | Self::Discovered { id, .. } => id,
        }
    }
}

/// The app id: the explicit one, else the first app subscribed to the WABA. With neither,
/// it's input the user can supply, so it's a validation error (exit 5).
pub async fn find_app(
    client: &Client,
    explicit: Option<&str>,
    waba_id: Option<&str>,
) -> Result<AppChoice> {
    if let Some(id) = explicit.filter(|s| !s.trim().is_empty()) {
        return Ok(AppChoice::Given(id.to_owned()));
    }
    let Some(waba_id) = waba_id.filter(|s| !s.trim().is_empty()) else {
        return Err(Error::validation(
            "no app id: pass --app-id, set PALOMA_APP_ID, or set a WABA id to look it up",
        ));
    };
    let subscribed = client
        .execute(&webhook::subscribed_apps(waba_id))
        .await?
        .json()?;
    let id = webhook::subscribed_app_id(&subscribed).ok_or_else(|| {
        Error::validation(
            "no app id: pass --app-id, set PALOMA_APP_ID, or subscribe an app to the WABA first",
        )
    })?;
    let count = subscribed["data"].as_array().map_or(0, Vec::len);
    Ok(AppChoice::Discovered {
        id,
        subscribed: count,
    })
}

/// The WABA-scoped manifest callback: WABA override, then app-level configuration.
/// Number overrides must stay out of WABA exports and reconciliation.
pub async fn current_callback_url(
    client: &Client,
    waba_id: &str,
    phone_number_id: Option<&str>,
) -> Result<Option<String>> {
    let subscribed = client
        .execute(&webhook::subscribed_apps(waba_id))
        .await?
        .json()?;
    if let Some(url) = webhook::override_callback(&subscribed) {
        return Ok(Some(url));
    }
    if let Some(phone) = phone_number_id {
        let cfg = client
            .execute(&webhook::configuration(phone))
            .await?
            .json()?;
        return Ok(webhook::application_callback(&cfg));
    }
    Ok(None)
}

/// The first lookup page holding `name` exactly (and `language`, if given), following the
/// cursor only as far as needed. Graph's `name` filter also returns other names, so a
/// page can be full without holding the one asked for.
pub async fn find_page(
    client: &Client,
    waba_id: &str,
    name: &str,
    language: Option<&str>,
) -> Result<Option<Value>> {
    paging::find_page(
        client,
        |after| templates::lookup_page(waba_id, name, language, after),
        |page| !templates::exact_matches(page, name, language).is_empty(),
    )
    .await
}

/// Find the single template matching `name` (and `language`, if given) by executing a
/// lookup. With a language, the first match ends the search. Without one, every page is
/// read first, because a translation on a later page makes the name ambiguous. Several
/// languages without `--language` is a validation error, because acting on an arbitrary
/// one would be a surprise.
pub async fn find_one(
    client: &Client,
    waba_id: &str,
    name: &str,
    language: Option<&str>,
) -> Result<Value> {
    let page = match language {
        Some(_) => find_page(client, waba_id, name, language).await?,
        None => Some(
            paging::collect_all(client, None, |after| {
                templates::lookup_page(waba_id, name, None, after)
            })
            .await?,
        ),
    }
    .unwrap_or_default();
    let matches = templates::exact_matches(&page, name, language);
    match matches.as_slice() {
        [] => Err(Error::other(format!(
            "no template named '{name}'{} on this WABA",
            language.map(|l| format!(" ({l})")).unwrap_or_default()
        ))),
        [one] => Ok((*one).clone()),
        _ if language.is_none() => Err(Error::validation(format!(
            "template '{name}' exists in multiple languages; pass --language"
        ))),
        [first, ..] => Ok((*first).clone()),
    }
}
