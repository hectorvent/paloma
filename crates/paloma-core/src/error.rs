//! The error taxonomy and the exit-code contract.
//!
//! Scripts branch on `paloma`'s exit status, so the mapping from failure kind to number
//! lives in exactly one place: [`Error::exit_code`]. Every expected failure is a variant
//! here; commands signal failure by returning one, never by printing and picking a code.

use std::fmt;

/// Process exit codes. The numeric value is the only source of truth for what `paloma`
/// returns to the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCode {
    Ok = 0,
    Generic = 1,
    Usage = 2,
    Auth = 3,
    RateLimited = 4,
    Validation = 5,
    Policy = 6,
}

impl ExitCode {
    pub const fn code(self) -> u8 {
        self as u8
    }
}

impl From<ExitCode> for std::process::ExitCode {
    fn from(code: ExitCode) -> Self {
        Self::from(code.code())
    }
}

/// An expected, user-facing failure. The `Display` text is what the user sees after
/// `error: `, so it should name the fix, not the stack.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Pre-flight validation failed locally, before any API call (exit 5).
    #[error("{0}")]
    Validation(String),

    /// Missing/invalid/expired credentials or insufficient scopes (exit 3).
    #[error("{0}")]
    Auth(String),

    /// The Graph API answered 429 or a throttling code (exit 4).
    #[error("{0}")]
    RateLimited(String),

    /// Blocked by a messaging-policy guardrail, local or Meta-side (exit 6).
    #[error("{0}")]
    Policy(String),

    /// The command line was well-formed for the parser but semantically wrong (exit 2).
    #[error("{0}")]
    Usage(String),

    /// Any other non-2xx Graph API response (exit 1).
    #[error("Graph API error ({status}{}): {message}", code.map(|c| format!(", #{c}")).unwrap_or_default())]
    Api {
        status: u16,
        code: Option<i64>,
        message: String,
    },

    /// A verb wired into the help tree but not built yet (exit 1).
    #[error("{command}: not yet implemented (planned for {phase})")]
    NotImplemented {
        command: String,
        phase: &'static str,
    },

    /// Local I/O failure, with the operation that failed.
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },

    /// Anything else that is expected but has no dedicated exit code (exit 1).
    #[error("{0}")]
    Other(String),

    /// Exit with a code without an error message: the command already reported the
    /// outcome (e.g. `diff` found drift).
    #[error("")]
    Silent(ExitCode),
}

impl Error {
    pub fn validation(msg: impl fmt::Display) -> Self {
        Self::Validation(msg.to_string())
    }

    pub fn auth(msg: impl fmt::Display) -> Self {
        Self::Auth(msg.to_string())
    }

    pub fn policy(msg: impl fmt::Display) -> Self {
        Self::Policy(msg.to_string())
    }

    pub fn usage(msg: impl fmt::Display) -> Self {
        Self::Usage(msg.to_string())
    }

    pub fn other(msg: impl fmt::Display) -> Self {
        Self::Other(msg.to_string())
    }

    pub fn io(context: impl fmt::Display, source: std::io::Error) -> Self {
        Self::Io {
            context: context.to_string(),
            source,
        }
    }

    pub const fn exit_code(&self) -> ExitCode {
        match self {
            Self::Silent(code) => *code,
            Self::Validation(_) => ExitCode::Validation,
            Self::Auth(_) => ExitCode::Auth,
            Self::RateLimited(_) => ExitCode::RateLimited,
            Self::Policy(_) => ExitCode::Policy,
            Self::Usage(_) => ExitCode::Usage,
            Self::Api { .. } | Self::NotImplemented { .. } | Self::Io { .. } | Self::Other(_) => {
                ExitCode::Generic
            }
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_are_the_contract() {
        assert_eq!(Error::validation("x").exit_code().code(), 5);
        assert_eq!(Error::auth("x").exit_code().code(), 3);
        assert_eq!(Error::RateLimited("x".into()).exit_code().code(), 4);
        assert_eq!(Error::policy("x").exit_code().code(), 6);
        assert_eq!(Error::usage("x").exit_code().code(), 2);
        assert_eq!(Error::other("x").exit_code().code(), 1);
    }

    #[test]
    fn api_error_message_includes_meta_code_when_known() {
        let e = Error::Api {
            status: 400,
            code: Some(100),
            message: "bad".into(),
        };
        assert_eq!(e.to_string(), "Graph API error (400, #100): bad");
        let e = Error::Api {
            status: 500,
            code: None,
            message: "boom".into(),
        };
        assert_eq!(e.to_string(), "Graph API error (500): boom");
    }
}
