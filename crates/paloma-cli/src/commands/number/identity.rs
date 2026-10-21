//! Identity changes and contact-book deletion affect other people's ability to find a
//! business, so writes are explicit and confirmed rather than folded into registration.

use crate::app::{App, no_id};
use crate::output::Output;
use clap::Subcommand;
use paloma_core::{Error, Result, validate};
use paloma_graph::api::phone_numbers;

#[derive(Debug, Subcommand)]
pub enum UsernameCommand {
    /// Read the current username and approval status.
    Get,
    /// List usernames reserved for the business portfolio.
    Reserved,
    /// Adopt a username; Meta enforces eligibility and rolling change limits.
    Set {
        username: String,
        /// Move the username from another number in the same portfolio.
        #[arg(long)]
        force_transfer: bool,
    },
    /// Remove this number's username (does not reset the change limit).
    Delete,
}

impl UsernameCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        if let Self::Set { username, .. } = &self {
            validate::business_username(username)?;
        }
        let phone = app.ctx()?.require_phone_number_id()?.to_owned();
        match self {
            Self::Get => {
                app.send_and_emit(&phone_numbers::username(&phone), |v| {
                    v.get("username")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                })
                .await
            }
            Self::Reserved => {
                if let Some(resp) = app.send(&phone_numbers::reserved_usernames(&phone)).await? {
                    app.out
                        .emit(&Output::response(&resp)?.table("/data", &["username_suggestions"]));
                }
                Ok(())
            }
            Self::Set {
                username,
                force_transfer,
            } => {
                let action = if force_transfer {
                    "adopt or transfer"
                } else {
                    "adopt"
                };
                if !app
                    .confirm_or_abort(&format!("{action} username {username} on number {phone}"))?
                {
                    return Ok(());
                }
                app.out.note("username changes are limited to 2 per rolling 14 days (10 when reserved); deleting does not reset the limit");
                app.send_and_emit(
                    &phone_numbers::set_username(&phone, &username, force_transfer),
                    no_id,
                )
                .await
            }
            Self::Delete => {
                if !app.confirm_or_abort(&format!("delete username from number {phone}"))? {
                    return Ok(());
                }
                app.send_and_emit(&phone_numbers::delete_username(&phone), no_id)
                    .await
            }
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum ContactsCommand {
    /// Check one BSUID in Meta's portfolio-scoped contact book.
    Get { bsuid: String },
    /// Delete one BSUID from the portfolio-scoped contact book.
    Delete { bsuid: String },
}

impl ContactsCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        let (bsuid, delete) = match self {
            Self::Get { bsuid } => (bsuid, false),
            Self::Delete { bsuid } => (bsuid, true),
        };
        if validate::bsuid(&bsuid) != Some(validate::Bsuid::Scoped) || bsuid.trim() != bsuid {
            return Err(Error::validation(
                "contact book requires one portfolio-scoped BSUID, such as US.123456",
            ));
        }
        let phone = app.ctx()?.require_phone_number_id()?.to_owned();
        if delete
            && !app
                .confirm_or_abort("delete contact-book entry for the entire business portfolio")?
        {
            return Ok(());
        }
        let req = phone_numbers::contact(&phone, &bsuid, delete);
        app.send_and_emit(&req, no_id).await
    }
}
