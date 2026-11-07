//! A named credential/context profile from `config.toml`.

use std::fmt;
use std::str::FromStr;

use paloma_core::Error;
use serde::{Deserialize, Serialize};

/// One `[profiles.<name>]` table. `token_ref`/`app_secret_ref` are indirections:
/// `keyring:<account>` resolves through the OS keyring, anything else is a plaintext
/// value (accepted, but warned about).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waba_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone_number_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_secret_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messaging_account_id: Option<String>,
}

impl Profile {
    pub fn get(&self, key: ProfileKey) -> Option<&str> {
        match key {
            ProfileKey::TokenRef => self.token_ref.as_deref(),
            ProfileKey::WabaId => self.waba_id.as_deref(),
            ProfileKey::PhoneNumberId => self.phone_number_id.as_deref(),
            ProfileKey::ApiVersion => self.api_version.as_deref(),
            ProfileKey::AppSecretRef => self.app_secret_ref.as_deref(),
            ProfileKey::MessagingAccountId => self.messaging_account_id.as_deref(),
        }
    }
}

/// The settable keys of a profile, as written in `config.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileKey {
    TokenRef,
    WabaId,
    PhoneNumberId,
    ApiVersion,
    AppSecretRef,
    MessagingAccountId,
}

impl ProfileKey {
    pub const ALL: [Self; 6] = [
        Self::TokenRef,
        Self::WabaId,
        Self::PhoneNumberId,
        Self::ApiVersion,
        Self::AppSecretRef,
        Self::MessagingAccountId,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TokenRef => "token_ref",
            Self::WabaId => "waba_id",
            Self::PhoneNumberId => "phone_number_id",
            Self::ApiVersion => "api_version",
            Self::AppSecretRef => "app_secret_ref",
            Self::MessagingAccountId => "messaging_account_id",
        }
    }

    /// Whether the value may be a secret (and must be redacted when plaintext).
    pub const fn is_secret_ref(self) -> bool {
        matches!(self, Self::TokenRef | Self::AppSecretRef)
    }
}

impl fmt::Display for ProfileKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProfileKey {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let wanted = s.trim().to_ascii_lowercase().replace('-', "_");
        Self::ALL
            .into_iter()
            .find(|k| k.as_str() == wanted)
            .ok_or_else(|| {
                let keys: Vec<_> = Self::ALL.iter().map(|k| k.as_str()).collect();
                Error::validation(format!(
                    "unknown key '{s}' (expected one of: {})",
                    keys.join(", ")
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_parse_in_any_case_with_dashes_and_round_trip() {
        for key in ProfileKey::ALL {
            assert_eq!(key.as_str().parse::<ProfileKey>().unwrap(), key);
            assert_eq!(key.to_string(), key.as_str());
        }
        assert_eq!("WABA-ID".parse::<ProfileKey>().unwrap(), ProfileKey::WabaId);
        assert!("password".parse::<ProfileKey>().is_err());
    }

    #[test]
    fn only_the_secret_references_are_redacted() {
        let secret: Vec<_> = ProfileKey::ALL
            .into_iter()
            .filter(|k| k.is_secret_ref())
            .collect();
        assert_eq!(secret, [ProfileKey::TokenRef, ProfileKey::AppSecretRef]);
    }
}
