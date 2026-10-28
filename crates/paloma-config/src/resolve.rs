//! Resolves the effective [`Context`] for one invocation.
//!
//! Precedence, implemented only here:
//!
//! ```text
//! flag  >  PALOMA_* env  >  profile in config.toml (keyring refs resolved lazily)
//! ```
//!
//! clap merges flag and env into [`Overrides`] before we get here, so this module only
//! layers those over the profile. Secrets are *not* read during resolution: the keyring
//! is consulted the first time [`Context::token`] is called, so `--dry-run` and offline
//! commands never trigger a keychain prompt.

use std::fmt;
use std::sync::{Arc, OnceLock};

use paloma_core::{Error, Result};
use secrecy::{ExposeSecret, SecretString};

use crate::profile::Profile;
use crate::secrets::{SecretStore, keyring_account};
use crate::store::Config;

/// Flag/env values that override the profile. `None` means "not given".
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub profile: Option<String>,
    pub token: Option<SecretString>,
    pub phone_number_id: Option<String>,
    pub waba_id: Option<String>,
    pub app_id: Option<String>,
    pub app_secret: Option<SecretString>,
    pub api_version: Option<String>,
    pub messaging_account_id: Option<String>,
}

/// Where a secret will come from, decided at resolution time and read on first use.
#[derive(Clone)]
enum SecretSource {
    None,
    Literal(SecretString),
    Keyring(String),
}

struct LazySecret {
    source: SecretSource,
    cell: OnceLock<Option<SecretString>>,
}

impl LazySecret {
    fn new(source: SecretSource) -> Self {
        Self {
            source,
            cell: OnceLock::new(),
        }
    }

    fn get(&self, store: &dyn SecretStore) -> Result<Option<SecretString>> {
        if let Some(v) = self.cell.get() {
            return Ok(v.clone());
        }
        let value = match &self.source {
            SecretSource::None => None,
            SecretSource::Literal(s) => Some(s.clone()),
            SecretSource::Keyring(account) => store.get(account)?.map(SecretString::from),
        };
        Ok(self.cell.get_or_init(|| value).clone())
    }
}

/// The fully-resolved, immutable execution context for one invocation.
pub struct Context {
    pub profile: String,
    pub phone_number_id: Option<String>,
    pub waba_id: Option<String>,
    pub app_id: Option<String>,
    pub api_version: String,
    /// The account to bill, for tokens that reach several on one phone number. Optional,
    /// because Meta resolves it from the token when there is only one.
    pub messaging_account_id: Option<String>,
    /// Diagnostics produced during resolution (e.g. a plaintext token in config); the
    /// CLI prints them to stderr.
    pub warnings: Vec<String>,
    token: Arc<LazySecret>,
    app_secret: Arc<LazySecret>,
    store: Arc<dyn SecretStore>,
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("profile", &self.profile)
            .field("phone_number_id", &self.phone_number_id)
            .field("waba_id", &self.waba_id)
            .field("app_id", &self.app_id)
            .field("api_version", &self.api_version)
            .field("messaging_account_id", &self.messaging_account_id)
            .finish_non_exhaustive()
    }
}

impl Context {
    /// Layer `overrides` over the selected profile of `config`.
    pub fn resolve(
        config: &Config,
        overrides: &Overrides,
        default_api_version: &str,
        store: Arc<dyn SecretStore>,
    ) -> Self {
        let profile_name = config
            .selected_profile_name(overrides.profile.as_deref())
            .to_owned();
        let empty = Profile::default();
        let profile = config.profile(&profile_name).unwrap_or(&empty);
        let mut warnings = Vec::new();

        let token = match non_blank_secret(overrides.token.as_ref()) {
            Some(t) => SecretSource::Literal(t.clone()),
            None => reference_source(profile.token_ref.as_deref(), || {
                warnings.push(format!(
                    "profile '{profile_name}' stores a plaintext token in config; prefer the keyring (`paloma auth login`)"
                ));
            }),
        };
        let app_secret = match non_blank_secret(overrides.app_secret.as_ref()) {
            Some(s) => SecretSource::Literal(s.clone()),
            None => reference_source(profile.app_secret_ref.as_deref(), || {}),
        };

        let pick = |flag: &Option<String>, prof: &Option<String>| {
            non_blank(flag.as_deref())
                .or_else(|| non_blank(prof.as_deref()))
                .map(str::to_owned)
        };

        Self {
            phone_number_id: pick(&overrides.phone_number_id, &profile.phone_number_id),
            waba_id: pick(&overrides.waba_id, &profile.waba_id),
            app_id: non_blank(overrides.app_id.as_deref()).map(str::to_owned),
            api_version: pick(&overrides.api_version, &profile.api_version)
                .unwrap_or_else(|| default_api_version.to_owned()),
            messaging_account_id: pick(
                &overrides.messaging_account_id,
                &profile.messaging_account_id,
            ),
            profile: profile_name,
            warnings,
            token: Arc::new(LazySecret::new(token)),
            app_secret: Arc::new(LazySecret::new(app_secret)),
            store,
        }
    }

    /// A copy targeting another phone number (per-manifest `metadata.phoneNumberId`).
    #[must_use]
    pub fn with_phone_number_id(&self, id: impl Into<String>) -> Self {
        Self {
            profile: self.profile.clone(),
            phone_number_id: Some(id.into()),
            waba_id: self.waba_id.clone(),
            app_id: self.app_id.clone(),
            api_version: self.api_version.clone(),
            messaging_account_id: self.messaging_account_id.clone(),
            warnings: Vec::new(),
            token: Arc::clone(&self.token),
            app_secret: Arc::clone(&self.app_secret),
            store: Arc::clone(&self.store),
        }
    }

    /// The access token if one is configured (reads the keyring on first call).
    pub fn token(&self) -> Result<Option<SecretString>> {
        self.token.get(self.store.as_ref())
    }

    pub fn require_token(&self) -> Result<SecretString> {
        self.token()?.ok_or_else(|| {
            Error::auth("no access token: run `paloma auth login`, or set PALOMA_TOKEN")
        })
    }

    pub fn app_secret(&self) -> Result<Option<SecretString>> {
        self.app_secret.get(self.store.as_ref())
    }

    pub fn require_app_secret(&self) -> Result<SecretString> {
        self.app_secret()?.ok_or_else(|| {
            Error::auth(
                "no app secret: set PALOMA_APP_SECRET or an app_secret_ref in your profile \
                 (needed for app-level webhook field subscriptions and webhook signatures)",
            )
        })
    }

    pub fn require_phone_number_id(&self) -> Result<&str> {
        self.phone_number_id.as_deref().ok_or_else(|| {
            Error::auth("no phone number id: set it in your profile or pass --phone-number-id")
        })
    }

    pub fn require_waba_id(&self) -> Result<&str> {
        self.waba_id
            .as_deref()
            .ok_or_else(|| Error::auth("no WABA id: set it in your profile or pass --waba-id"))
    }

    pub fn store(&self) -> &dyn SecretStore {
        self.store.as_ref()
    }
}

impl paloma_core::Credentials for Context {
    fn access_token(&self) -> Result<SecretString> {
        self.require_token()
    }

    fn app_secret(&self) -> Result<SecretString> {
        self.require_app_secret()
    }
}

fn reference_source(reference: Option<&str>, on_plaintext: impl FnOnce()) -> SecretSource {
    match non_blank(reference) {
        None => SecretSource::None,
        Some(r) => match keyring_account(r) {
            Some(account) => SecretSource::Keyring(account.to_owned()),
            None => {
                on_plaintext();
                SecretSource::Literal(r.to_owned().into())
            }
        },
    }
}

fn non_blank(s: Option<&str>) -> Option<&str> {
    s.filter(|v| !v.trim().is_empty())
}

fn non_blank_secret(s: Option<&SecretString>) -> Option<&SecretString> {
    s.filter(|v| !v.expose_secret().trim().is_empty())
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;
    use crate::secrets::MemoryStore;

    fn config() -> Config {
        toml::from_str(
            r#"
            default_profile = "work"
            [profiles.work]
            token_ref = "keyring:paloma/work"
            phone_number_id = "111"
            waba_id = "222"
            api_version = "v20.0"
            [profiles.plain]
            token_ref = "EAAplain"
            "#,
        )
        .unwrap()
    }

    fn store() -> Arc<dyn SecretStore> {
        Arc::new(MemoryStore::with(&[("paloma/work", "EAAsecret")]))
    }

    #[test]
    fn profile_values_and_keyring_token() {
        let ctx = Context::resolve(&config(), &Overrides::default(), "v26.0", store());
        assert_eq!(ctx.profile, "work");
        assert_eq!(ctx.require_phone_number_id().unwrap(), "111");
        assert_eq!(ctx.api_version, "v20.0");
        assert_eq!(ctx.require_token().unwrap().expose_secret(), "EAAsecret");
        assert!(ctx.warnings.is_empty());
    }

    #[test]
    fn overrides_win() {
        let o = Overrides {
            token: Some("EAAflag".into()),
            phone_number_id: Some("999".into()),
            api_version: Some("v26.0".into()),
            ..Overrides::default()
        };
        let ctx = Context::resolve(&config(), &o, "v26.0", store());
        assert_eq!(ctx.require_phone_number_id().unwrap(), "999");
        assert_eq!(ctx.api_version, "v26.0");
        assert_eq!(ctx.require_token().unwrap().expose_secret(), "EAAflag");
    }

    #[test]
    fn plaintext_token_warns() {
        let o = Overrides {
            profile: Some("plain".into()),
            ..Overrides::default()
        };
        let ctx = Context::resolve(&config(), &o, "v26.0", store());
        assert_eq!(ctx.warnings.len(), 1);
        assert_eq!(ctx.require_token().unwrap().expose_secret(), "EAAplain");
        assert_eq!(ctx.api_version, "v26.0");
    }

    #[test]
    fn missing_values_are_auth_errors_naming_the_fix() {
        let o = Overrides {
            profile: Some("ghost".into()),
            ..Overrides::default()
        };
        let ctx = Context::resolve(&config(), &o, "v26.0", store());
        let err = ctx.require_token().unwrap_err();
        assert_eq!(err.exit_code().code(), 3);
        assert!(err.to_string().contains("auth login"));
        assert!(ctx.require_waba_id().is_err());
    }
}
