//! Template categories. Meta's three values decide the TTL range, the send route and which
//! components a template may have, so they're one type instead of strings matched in each
//! crate.

use std::fmt;
use std::str::FromStr;

use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateCategory {
    Marketing,
    Utility,
    Authentication,
}

impl TemplateCategory {
    pub const ALL: [Self; 3] = [Self::Marketing, Self::Utility, Self::Authentication];

    /// Meta's wire value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Marketing => "MARKETING",
            Self::Utility => "UTILITY",
            Self::Authentication => "AUTHENTICATION",
        }
    }
}

impl fmt::Display for TemplateCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Case-insensitive, since Graph and users write these in either case.
impl FromStr for TemplateCategory {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|c| c.as_str().eq_ignore_ascii_case(s.trim()))
            .ok_or_else(|| {
                Error::validation(format!(
                    "unknown template category '{s}' (MARKETING, UTILITY or AUTHENTICATION)"
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_parse_in_any_case_and_print_as_meta_writes_them() {
        assert_eq!(
            "utility".parse::<TemplateCategory>().unwrap(),
            TemplateCategory::Utility
        );
        assert_eq!(
            TemplateCategory::Authentication.to_string(),
            "AUTHENTICATION"
        );
        assert!("transactional".parse::<TemplateCategory>().is_err());
    }
}
