//! The seam between "where secrets come from" (`paloma-config`) and "who sends them"
//! (`paloma-graph`), so neither crate depends on the other.

pub use secrecy::SecretString;

use crate::Result;

/// Supplies secrets on demand. Implementations may hit the OS keyring, so callers ask
/// only when a request is really about to be sent.
pub trait Credentials: Send + Sync + std::fmt::Debug {
    /// The user/system-user access token used for almost every Graph call.
    fn access_token(&self) -> Result<SecretString>;
    /// The Meta app secret, combined with an app id into an app access token.
    fn app_secret(&self) -> Result<SecretString>;
}
