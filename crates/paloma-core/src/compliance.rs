//! The 24-hour customer-service window guardrail.
//!
//! Real window *tracking* (derived from inbound events seen by `paloma webhook listen`)
//! is a v1.0 feature. Until then nothing can prove the window is open, so the policy is
//! conservative but unobtrusive: template sends are always allowed, and free-form sends
//! are allowed unless `--strict` is set without `--force`. The [`WindowTracker`] seam is
//! where event-derived tracking plugs in later without touching the send commands.

use crate::{Error, Result};

/// What we know about a recipient's customer-service window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    Open,
    Closed,
    Unknown,
}

/// Source of window knowledge for a recipient.
pub trait WindowTracker {
    fn state(&self, recipient: &str) -> WindowState;
}

/// The v0.x tracker: knows nothing.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoTracking;

impl WindowTracker for NoTracking {
    fn state(&self, _recipient: &str) -> WindowState {
        WindowState::Unknown
    }
}

/// How a send asked to be policed.
#[derive(Debug, Default, Clone, Copy)]
pub struct SendPolicy {
    /// Bypass the guardrail entirely.
    pub force: bool,
    /// Refuse free-form sends unless the window is provably open.
    pub strict: bool,
}

/// Decide whether a send may proceed.
pub fn enforce_session_window(
    tracker: &impl WindowTracker,
    recipient: &str,
    is_template: bool,
    policy: SendPolicy,
) -> Result<()> {
    if is_template || policy.force {
        return Ok(());
    }
    match (tracker.state(recipient), policy.strict) {
        (WindowState::Open, _) | (WindowState::Unknown, false) => Ok(()),
        (WindowState::Closed, _) => Err(Error::policy(
            "the 24h customer-service window is closed for this recipient: \
             send an approved template, or pass --force",
        )),
        (WindowState::Unknown, true) => Err(Error::policy(
            "outside the 24h customer-service window (strict mode): \
             send an approved template, or pass --force",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_and_force_always_pass() {
        let strict = SendPolicy {
            force: false,
            strict: true,
        };
        assert!(enforce_session_window(&NoTracking, "1", true, strict).is_ok());
        let forced = SendPolicy {
            force: true,
            strict: true,
        };
        assert!(enforce_session_window(&NoTracking, "1", false, forced).is_ok());
    }

    #[test]
    fn strict_blocks_unknown_window_with_policy_exit() {
        let strict = SendPolicy {
            force: false,
            strict: true,
        };
        let err = enforce_session_window(&NoTracking, "1", false, strict).unwrap_err();
        assert_eq!(err.exit_code().code(), 6);
        assert!(enforce_session_window(&NoTracking, "1", false, SendPolicy::default()).is_ok());
    }
}
