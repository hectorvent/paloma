//! `config.toml` edits go through `toml_edit`, so a hand-edited file keeps its comments and
//! order. Secrets never land in it: token keys hold keyring references.

use std::io::IsTerminal;

use clap::{Args, Subcommand};
use paloma_config::ProfileKey;
use paloma_config::secrets::keyring_account;
use paloma_core::{Error, Result};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Map, Value, json};

use super::auth::store_login;
use crate::app::App;
use crate::output::Output;

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// List all profiles (secrets redacted).
    List,
    /// Print one key of the selected profile.
    Get {
        /// token_ref, waba_id, phone_number_id, api_version, app_secret_ref or
        /// messaging_account_id.
        key: ProfileKey,
    },
    /// Set one key of the selected profile (creating it if needed).
    Set {
        /// token_ref, waba_id, phone_number_id, api_version, app_secret_ref or
        /// messaging_account_id.
        key: ProfileKey,
        /// The new value.
        value: String,
    },
    /// Remove one key from the selected profile.
    Unset {
        /// The key to remove, as for `config get`.
        key: ProfileKey,
    },
    /// Make a profile the default.
    Use {
        /// The profile to make the default.
        profile: String,
    },
    /// Print the config file path.
    Path,
}

impl ConfigCommand {
    pub fn run(self, app: &App) -> Result<()> {
        let file = app.config_file()?;
        match self {
            Self::List => {
                let config = file.load()?;
                let profiles: Map<String, Value> = config
                    .profiles
                    .iter()
                    .map(|(name, p)| {
                        let view: Map<String, Value> = ProfileKey::ALL
                            .iter()
                            .map(|k| (k.to_string(), display_value(*k, p.get(*k)).into()))
                            .collect();
                        (name.clone(), Value::Object(view))
                    })
                    .collect();
                app.out.emit(&Output::new(json!({
                    "default_profile": config.default_profile_name(),
                    "profiles": profiles,
                })));
            }
            Self::Get { key } => {
                let config = file.load()?;
                let name = app.profile_name()?;
                let profile = config.profile(&name).ok_or_else(|| {
                    Error::other(format!("no profile '{name}' in {}", file.path().display()))
                })?;
                let value = profile.get(key).map(str::to_owned);
                app.out
                    .emit(&Output::new(json!({ key.as_str(): value })).id(value));
            }
            Self::Set { key, value } => {
                let name = app.profile_name()?;
                file.set_key(&name, key, Some(&value))?;
                app.out.info(format!("set {key} on profile '{name}'"));
                if key.is_secret_ref() && keyring_account(&value).is_none() {
                    app.out.warn("this stores a plaintext secret in the config file; prefer `paloma auth login`");
                }
            }
            Self::Unset { key } => {
                let name = app.profile_name()?;
                file.set_key(&name, key, None)?;
                app.out.info(format!("removed {key} from profile '{name}'"));
            }
            Self::Use { profile } => {
                if file.load()?.profile(&profile).is_none() {
                    return Err(Error::validation(format!(
                        "no profile '{profile}' in {}",
                        file.path().display()
                    )));
                }
                file.set_default_profile(&profile)?;
                app.out.info(format!("default profile is now '{profile}'"));
            }
            Self::Path => {
                let path = file.path().display().to_string();
                app.out
                    .emit(&Output::new(json!({ "path": path })).id(Some(path)));
            }
        }
        Ok(())
    }
}

fn display_value(key: ProfileKey, value: Option<&str>) -> String {
    match value.filter(|v| !v.is_empty()) {
        None => "-".into(),
        Some(v) if key.is_secret_ref() && keyring_account(v).is_none() => {
            "<plaintext:redacted>".into()
        }
        Some(v) => v.to_owned(),
    }
}

/// Interactive first-run setup: token into the keyring, ids into the profile.
#[derive(Debug, Args)]
pub struct Configure;

impl Configure {
    pub fn run(app: &App) -> Result<()> {
        if !std::io::stdin().is_terminal() {
            return Err(Error::validation(
                "`paloma configure` needs a terminal; use `paloma auth login --with-token` and `paloma config set` in scripts",
            ));
        }
        let name = app.profile_name()?;
        app.out.info(format!("Configuring profile '{name}'"));
        let token = dialoguer::Password::new()
            .with_prompt("Access token")
            .interact()
            .map_err(|e| Error::failed(format!("prompt failed: {e}"), e))?;
        let phone = prompt(
            "Sender phone number id",
            app.globals.phone_number_id.as_deref(),
        )?;
        let waba = prompt(
            "WhatsApp Business Account id",
            app.globals.waba_id.as_deref(),
        )?;
        let version = prompt("Graph API version", Some(paloma_graph::DEFAULT_API_VERSION))?;
        let token = SecretString::from(token);
        let result = store_login(
            app,
            &name,
            token.expose_secret(),
            phone.as_deref(),
            waba.as_deref(),
            version.as_deref(),
        )?;
        app.out.info(format!(
            "saved profile '{name}' (token in {})",
            app.store().name()
        ));
        app.out.emit(&result);
        Ok(())
    }
}

fn prompt(label: &str, default: Option<&str>) -> Result<Option<String>> {
    let mut input = dialoguer::Input::<String>::new()
        .with_prompt(label)
        .allow_empty(true);
    if let Some(d) = default.filter(|d| !d.is_empty()) {
        input = input.default(d.to_owned());
    }
    let v = input
        .interact_text()
        .map_err(|e| Error::failed(format!("prompt failed: {e}"), e))?;
    Ok(Some(v.trim().to_owned()).filter(|v| !v.is_empty()))
}
