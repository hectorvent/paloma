//! Secret storage behind `keyring:<account>` references.
//!
//! Secrets never live in plaintext config or argv. The [`SecretStore`] trait is the
//! single seam: the real implementation talks to the platform keyring (macOS Keychain,
//! Windows Credential Manager, Linux Secret Service), and `MemoryStore` backs this crate's tests.

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;
use std::sync::OnceLock;

use paloma_core::{Error, Result};

/// Prefix marking a config value as a keyring indirection.
pub const KEYRING_PREFIX: &str = "keyring:";
/// The keyring service every paloma secret is filed under.
pub const SERVICE: &str = "paloma";

/// The keyring account used for a profile's access token.
pub fn token_account(profile: &str) -> String {
    format!("paloma/{profile}")
}

/// The config reference that points at a keyring account.
pub fn keyring_ref(account: &str) -> String {
    format!("{KEYRING_PREFIX}{account}")
}

/// The keyring account named by a reference, or `None` for a plaintext value.
pub fn keyring_account(reference: &str) -> Option<&str> {
    reference.strip_prefix(KEYRING_PREFIX)
}

pub trait SecretStore: Send + Sync + std::fmt::Debug {
    /// The secret stored under `account`, or `None` when there is none.
    fn get(&self, account: &str) -> Result<Option<String>>;
    fn set(&self, account: &str, secret: &str) -> Result<()>;
    /// Remove the secret; `Ok(false)` when there was nothing to remove.
    fn delete(&self, account: &str) -> Result<bool>;
    /// Short backend name for diagnostics.
    fn name(&self) -> &'static str;
}

/// The platform keyring.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringStore;

static BACKEND: OnceLock<Result<(), String>> = OnceLock::new();

impl KeyringStore {
    /// Install the platform credential store once per process. The error names the
    /// env-var fallback, because "no keyring" is common in containers and CI.
    fn ensure_backend() -> Result<()> {
        BACKEND
            .get_or_init(|| init_platform_store().map_err(|e| e.to_string()))
            .clone()
            .map_err(|e| {
                Error::other(format!(
                    "no OS keyring available ({e}); use PALOMA_TOKEN instead"
                ))
            })
    }

    fn entry(account: &str) -> Result<keyring_core::Entry> {
        Self::ensure_backend()?;
        keyring_core::Entry::new(SERVICE, account).map_err(keyring_err)
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<String>> {
        match Self::entry(account)?.get_password() {
            Ok(v) if v.trim().is_empty() => Ok(None),
            Ok(v) => Ok(Some(v.trim().to_owned())),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(keyring_err(e)),
        }
    }

    fn set(&self, account: &str, secret: &str) -> Result<()> {
        Self::entry(account)?
            .set_password(secret)
            .map_err(keyring_err)
    }

    fn delete(&self, account: &str) -> Result<bool> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring_core::Error::NoEntry) => Ok(false),
            Err(e) => Err(keyring_err(e)),
        }
    }

    fn name(&self) -> &'static str {
        PLATFORM_NAME
    }
}

#[allow(clippy::needless_pass_by_value)] // used as `map_err(keyring_err)`
fn keyring_err(e: keyring_core::Error) -> Error {
    Error::other(format!("keyring access failed: {e}"))
}

#[cfg(target_os = "macos")]
const PLATFORM_NAME: &str = "macos-keychain";
#[cfg(target_os = "windows")]
const PLATFORM_NAME: &str = "windows-credential-manager";
#[cfg(target_os = "linux")]
const PLATFORM_NAME: &str = "secret-service";
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
const PLATFORM_NAME: &str = "none";

#[cfg(target_os = "macos")]
fn init_platform_store() -> keyring_core::Result<()> {
    keyring_core::set_default_store(apple_native_keyring_store::keychain::Store::new()?);
    Ok(())
}

#[cfg(target_os = "windows")]
fn init_platform_store() -> keyring_core::Result<()> {
    keyring_core::set_default_store(windows_native_keyring_store::Store::new()?);
    Ok(())
}

#[cfg(target_os = "linux")]
fn init_platform_store() -> keyring_core::Result<()> {
    keyring_core::set_default_store(zbus_secret_service_keyring_store::Store::new()?);
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn init_platform_store() -> keyring_core::Result<()> {
    Err(keyring_core::Error::NotSupportedByStore(
        "no credential store for this platform".into(),
    ))
}

/// An in-process store for tests.
#[derive(Debug, Default)]
#[cfg(test)]
pub struct MemoryStore {
    secrets: Mutex<HashMap<String, String>>,
}

#[cfg(test)]
impl MemoryStore {
    pub fn with(entries: &[(&str, &str)]) -> Self {
        let map = entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Self {
            secrets: Mutex::new(map),
        }
    }
}

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<String>> {
        Ok(self.secrets.lock().expect("poisoned").get(account).cloned())
    }

    fn set(&self, account: &str, secret: &str) -> Result<()> {
        self.secrets
            .lock()
            .expect("poisoned")
            .insert(account.to_owned(), secret.to_owned());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<bool> {
        Ok(self
            .secrets
            .lock()
            .expect("poisoned")
            .remove(account)
            .is_some())
    }

    fn name(&self) -> &'static str {
        "memory"
    }
}
