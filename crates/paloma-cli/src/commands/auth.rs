use clap::Subcommand;
use paloma_config::ProfileKey;
use paloma_config::secrets::{keyring_ref, token_account};
use paloma_core::{Error, Result};
use paloma_graph::api::{phone_numbers, tokens};
use secrecy::ExposeSecret;
use serde_json::{Value, json};

use super::{SecretSource, read_secret};
use crate::app::App;
use crate::output::Output;

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Store an access token in the OS keyring and make the profile the default.
    Login {
        /// Read the token from stdin (for CI: `echo "$T" | paloma auth login --with-token`).
        #[arg(long)]
        with_token: bool,
    },
    /// Check that the token works against the active phone number.
    Status {
        /// Inspect the token instead: its type, scopes and expiry. Needs no phone number id,
        /// so it still answers when a missing permission is what fails the usual check.
        #[arg(long)]
        scopes: bool,
    },
    /// Show who the active profile acts as.
    Whoami,
    /// Remove the profile's token from the keyring.
    Logout,
}

impl AuthCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::Login { with_token } => login(app, with_token),
            Self::Status { scopes: true } => token_details(app).await,
            Self::Status { scopes: false } => {
                let (ctx, number) = phone_status(app).await?;
                let Some(number) = number else { return Ok(()) };
                app.out.emit(
                    &Output::new(json!({
                        "token": "valid",
                        "profile": ctx.0,
                        "phone_number_id": ctx.1,
                        "display_phone_number": number.get("display_phone_number"),
                        "verified_name": number.get("verified_name"),
                        "quality_rating": number.get("quality_rating"),
                    }))
                    .id(Some("valid".into())),
                );
                Ok(())
            }
            Self::Whoami => {
                let (ctx, number) = phone_status(app).await?;
                let Some(number) = number else { return Ok(()) };
                let waba = app.ctx()?.waba_id.clone();
                app.out.emit(
                    &Output::new(json!({
                        "profile": ctx.0,
                        "waba_id": waba.unwrap_or_else(|| "-".into()),
                        "phone_number_id": ctx.1,
                        "display_phone_number": number.get("display_phone_number"),
                        "business_name": number.get("verified_name"),
                    }))
                    .id(Some(ctx.0)),
                );
                Ok(())
            }
            Self::Logout => {
                let name = app.profile_name()?;
                let removed = app.store().delete(&token_account(&name))?;
                app.out.info(if removed {
                    format!("removed the token for profile '{name}' from the keyring")
                } else {
                    format!("no stored token for profile '{name}'")
                });
                Ok(())
            }
        }
    }
}

fn login(app: &App, with_token: bool) -> Result<()> {
    // clap merges `--token` with PALOMA_TOKEN; only a value that didn't come from the
    // environment was typed on the command line, so only that one warns.
    let env = std::env::var("PALOMA_TOKEN").ok();
    let from_flag = app
        .globals
        .token
        .as_ref()
        .map(ExposeSecret::expose_secret)
        .filter(|t| env.as_deref() != Some(*t));
    let token = read_secret(
        app,
        &SecretSource {
            what: "access token",
            flag: "--token",
            stdin_flag: "--with-token",
            env: "PALOMA_TOKEN",
            missing_is_auth: true,
        },
        from_flag,
        with_token,
    )?;
    let token = token.expose_secret();
    let name = app.profile_name()?;
    let g = &app.globals;
    let out = store_login(
        app,
        &name,
        token,
        g.phone_number_id.as_deref(),
        g.waba_id.as_deref(),
        g.api_version.as_deref(),
    )?;
    app.out.info(format!(
        "logged in as profile '{name}' (token stored in {})",
        app.store().name()
    ));
    app.out.emit(&out);
    Ok(())
}

/// Put the token in the keyring and point the profile at it, keeping any other keys
/// the profile already has.
pub fn store_login(
    app: &App,
    profile: &str,
    token: &str,
    phone_number_id: Option<&str>,
    waba_id: Option<&str>,
    api_version: Option<&str>,
) -> Result<Output> {
    if token.trim().is_empty() {
        return Err(Error::auth("no token provided"));
    }
    let account = token_account(profile);
    app.store().set(&account, token.trim())?;
    let file = app.config_file()?;
    let token_ref = keyring_ref(&account);
    file.set_key(profile, ProfileKey::TokenRef, Some(&token_ref))?;
    for (key, value) in [
        (ProfileKey::PhoneNumberId, phone_number_id),
        (ProfileKey::WabaId, waba_id),
        (ProfileKey::ApiVersion, api_version),
    ] {
        if let Some(v) = value.filter(|v| !v.trim().is_empty()) {
            file.set_key(profile, key, Some(v))?;
        }
    }
    file.set_default_profile(profile)?;
    Ok(Output::new(json!({
        "profile": profile,
        "token_ref": token_ref,
        "config": file.path().display().to_string(),
    }))
    .id(Some(profile.to_owned())))
}

/// Meta's description of the active token. An answer that doesn't call it valid exits 3, so
/// `auth status --scopes -q` can gate a script the way `auth status -q` does.
async fn token_details(app: &App) -> Result<()> {
    let ctx = app.ctx()?;
    if app.dry_run() {
        // A placeholder, so a dry run never reads the keyring.
        return app.print_dry_run(&[tokens::debug("<active-token>")]);
    }
    let token = ctx.require_token()?;
    let resp = app.fetch(&tokens::debug(token.expose_secret())).await?;
    let data = resp
        .json()?
        .get("data")
        .cloned()
        .ok_or_else(|| Error::other("the Graph API returned no token details"))?;
    if data.get("is_valid") != Some(&Value::Bool(true)) {
        return Err(Error::auth("the access token is not valid"));
    }
    app.out.emit(
        &Output {
            data,
            ..Output::response(&resp)?
        }
        .id(Some("valid".into())),
    );
    Ok(())
}

/// `((profile, phone_number_id), number)`; `number` is `None` under `--dry-run`.
async fn phone_status(app: &App) -> Result<((String, String), Option<Value>)> {
    let ctx = app.ctx()?;
    let phone = ctx.require_phone_number_id()?.to_owned();
    let req = phone_numbers::get(&phone, phone_numbers::STATUS_FIELDS);
    let number = match app.send(&req).await? {
        Some(resp) => Some(resp.json()?),
        None => None,
    };
    Ok(((ctx.profile.clone(), phone), number))
}
