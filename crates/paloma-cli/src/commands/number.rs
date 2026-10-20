use clap::{Args, Subcommand, ValueEnum};
use paloma_core::limits::MAX_PHONE_NUMBERS_PAGE;
use paloma_core::{Result, validate};
use paloma_graph::api::phone_numbers::{self, DETAIL_FIELDS, ListFilter, QUALITY_FIELDS};
use secrecy::{ExposeSecret, SecretString};

use super::{SecretSource, read_secret};
use crate::app::{App, field_id, no_id};

const LIST_COLUMNS: &[&str] = &[
    "id",
    "display_phone_number",
    "verified_name",
    "quality_rating",
    "code_verification_status",
];

#[derive(Debug, Subcommand)]
pub enum NumberCommand {
    /// List the WABA's phone numbers.
    List(ListArgs),
    /// Show a phone number's details [default: the active number].
    Get { phone_number_id: Option<String> },
    /// Show quality rating, messaging limit and throughput.
    Quality { phone_number_id: Option<String> },
    /// Show messaging and calling health for a number.
    Health { phone_number_id: Option<String> },
    /// Register the number for Cloud API use.
    Register(RegisterArgs),
    /// Deregister the number.
    Deregister,
    /// Set the two-step verification PIN.
    SetPin(PinInput),
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Only numbers in this account mode. Meta is still testing this filter, so not every
    /// developer has it.
    #[arg(long, value_enum)]
    account_mode: Option<AccountMode>,
    /// Order by when Embedded Signup onboarding finished. Meta's default is
    /// last-onboarded-desc.
    #[arg(long, value_enum)]
    sort: Option<Sort>,
    /// Page size, at most 100.
    #[arg(long)]
    limit: Option<u32>,
    /// Follow every page.
    #[arg(long)]
    all: bool,
    /// Start after this cursor.
    #[arg(long)]
    after: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "SCREAMING_SNAKE_CASE")]
enum AccountMode {
    Sandbox,
    Live,
}

impl From<AccountMode> for phone_numbers::AccountMode {
    fn from(mode: AccountMode) -> Self {
        match mode {
            AccountMode::Sandbox => Self::Sandbox,
            AccountMode::Live => Self::Live,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Sort {
    LastOnboardedAsc,
    LastOnboardedDesc,
}

#[derive(Debug, Args)]
pub struct RegisterArgs {
    #[command(flatten)]
    pin: PinInput,
    /// Store message data at rest in this country (AE, AU, BH, BR, CA, CH, DE, GB, ID, IN,
    /// JP, KR, SG, ZA). First registration only: a registered number changes this through
    /// the Settings API after deregistering, which Paloma doesn't do yet.
    #[arg(long, value_name = "CC")]
    data_localization_region: Option<String>,
}

/// The six-digit two-step verification PIN: from PALOMA_PIN, stdin or a prompt.
#[derive(Debug, Args)]
pub struct PinInput {
    /// Deprecated: prefer PALOMA_PIN or --pin-stdin.
    #[arg(long, hide = true)]
    pin: Option<String>,
    /// Read the six-digit PIN from stdin. Otherwise uses PALOMA_PIN, or prompts.
    #[arg(long, conflicts_with = "pin")]
    pin_stdin: bool,
}

impl PinInput {
    fn read(&self, app: &App) -> Result<SecretString> {
        let pin = read_secret(
            app,
            &SecretSource {
                what: "PIN",
                flag: "--pin",
                stdin_flag: "--pin-stdin",
                env: "PALOMA_PIN",
            },
            self.pin.as_deref(),
            self.pin_stdin,
        )?;
        validate::pin(pin.expose_secret())?;
        Ok(pin)
    }
}

impl NumberCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::List(a) => {
                validate::page_size(a.limit, Some(MAX_PHONE_NUMBERS_PAGE))?;
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                let filter = ListFilter {
                    account_mode: a.account_mode.map(Into::into),
                    oldest_first: matches!(a.sort, Some(Sort::LastOnboardedAsc)),
                    limit: a.limit,
                };
                super::template::list(
                    app,
                    a.all,
                    a.after,
                    |after| phone_numbers::list(&waba, &filter, after),
                    LIST_COLUMNS,
                )
                .await
            }
            Self::Get { phone_number_id } => {
                let id = target(app, phone_number_id)?;
                app.send_and_emit(&phone_numbers::get(&id, DETAIL_FIELDS), field_id)
                    .await
            }
            Self::Quality { phone_number_id } => {
                let id = target(app, phone_number_id)?;
                app.send_and_emit(&phone_numbers::get(&id, QUALITY_FIELDS), |v| {
                    v.get("quality_rating")
                        .and_then(|q| q.as_str())
                        .map(str::to_owned)
                })
                .await
            }
            Self::Health { phone_number_id } => {
                let id = target(app, phone_number_id)?;
                super::account::show_health(app, &phone_numbers::get(&id, "health_status")).await
            }
            Self::Register(a) => {
                if let Some(region) = &a.data_localization_region {
                    validate::data_localization_region(region)?;
                }
                let pin = a.pin.read(app)?;
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                let req = phone_numbers::register(
                    &phone,
                    pin.expose_secret(),
                    a.data_localization_region.as_deref(),
                );
                app.send_and_emit(&req, no_id).await
            }
            Self::Deregister => {
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                if !app.confirm_or_abort(&format!("deregister number {phone}"))? {
                    return Ok(());
                }
                app.send_and_emit(&phone_numbers::deregister(&phone), no_id)
                    .await
            }
            Self::SetPin(input) => {
                let pin = input.read(app)?;
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                app.send_and_emit(&phone_numbers::set_pin(&phone, pin.expose_secret()), no_id)
                    .await
            }
        }
    }
}

fn target(app: &App, explicit: Option<String>) -> Result<String> {
    match explicit {
        Some(id) => {
            validate::numeric_id(&id, "phone number id")?;
            Ok(id)
        }
        None => Ok(app.ctx()?.require_phone_number_id()?.to_owned()),
    }
}
