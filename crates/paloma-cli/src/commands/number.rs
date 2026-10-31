//! Phone-number lifecycle: registration, PIN, display name and identity. These writes can
//! take a number offline, so they confirm, and PINs and codes never come from argv.

use clap::{Args, Subcommand, ValueEnum};
use paloma_core::limits::MAX_PHONE_NUMBERS_PAGE;
use paloma_core::{Result, validate};
use paloma_graph::api::phone_numbers::{self, DETAIL_FIELDS, ListFilter, QUALITY_FIELDS};
mod admin;
mod identity;
mod settings;

use secrecy::{ExposeSecret, SecretString};

use super::listing::{Pagination, Rows};
use super::util::{SecretSource, read_secret};
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
    /// Show a phone number's details.
    Get {
        /// Phone number id; the active number when left out.
        phone_number_id: Option<String>,
    },
    /// Show quality rating, messaging limit and throughput.
    Quality {
        /// Phone number id; the active number when left out.
        phone_number_id: Option<String>,
    },
    /// Show messaging and calling health for a number.
    Health {
        /// Phone number id; the active number when left out.
        phone_number_id: Option<String>,
    },
    /// Register the number for Cloud API use.
    Register(RegisterArgs),
    /// Deregister the number.
    Deregister,
    /// Prove you own the number with a code sent by SMS or voice call.
    #[command(subcommand)]
    Verification(Verification),
    /// Add a phone number to the active WABA; verification and registration follow.
    Add {
        /// The number in international digits, without the calling code or a plus sign.
        phone_number: String,
        /// Country calling code (not a country), such as 1 or 52.
        #[arg(long)]
        cc: String,
        /// Display name, which Meta reviews.
        #[arg(long)]
        name: String,
    },
    /// Change the display name.
    #[command(subcommand)]
    Name(NameCommand),
    /// Change whether people can find the number in WhatsApp search.
    #[command(subcommand)]
    Visibility(VisibilityCommand),
    /// Inspect or apply for Official Business Account status.
    #[command(subcommand)]
    Oba(admin::ObaCommand),
    /// Manage India business compliance information.
    #[command(subcommand)]
    Compliance(admin::ComplianceCommand),
    /// Manage the business username.
    #[command(subcommand)]
    Username(identity::UsernameCommand),
    /// Inspect or delete Meta contact-book entries.
    #[command(subcommand)]
    Contacts(identity::ContactsCommand),
    /// Configure local storage and identity checks.
    #[command(subcommand)]
    Settings(settings::SettingsCommand),
    /// Change the two-step verification PIN.
    #[command(subcommand)]
    Pin(PinCommand),
}

#[derive(Debug, Subcommand)]
pub enum Verification {
    /// Send an ownership verification code by SMS or voice call.
    Request {
        /// How Meta delivers the code.
        #[arg(long, value_enum)]
        method: CodeMethod,
        /// Language of the message or call.
        #[arg(long, default_value = "en_US")]
        language: String,
    },
    /// Confirm ownership with the code from PALOMA_VERIFICATION_CODE, stdin or a prompt.
    Confirm {
        /// Read the code from stdin.
        #[arg(long)]
        code_stdin: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum NameCommand {
    /// Submit a new display name for review.
    Set {
        /// The new display name.
        name: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum VisibilityCommand {
    /// Set whether the number shows up in WhatsApp search.
    Set {
        #[arg(value_enum)]
        visibility: SearchVisibility,
    },
}

#[derive(Debug, Subcommand)]
pub enum PinCommand {
    /// Replace the two-step verification PIN. Numbers Meta made eligible for its PIN
    /// deprecation have none to replace.
    Set(PinInput),
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
    #[command(flatten)]
    page: Pagination,
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
    /// `number settings set --storage-region` after deregistering.
    #[arg(long, value_name = "CC")]
    data_localization_region: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CodeMethod {
    Sms,
    Voice,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SearchVisibility {
    Visible,
    Hidden,
}

/// The six-digit two-step verification PIN: from PALOMA_PIN, stdin or a prompt.
#[derive(Debug, Args)]
pub struct PinInput {
    /// Read the six-digit PIN from stdin. Otherwise uses PALOMA_PIN, or prompts.
    #[arg(long)]
    pin_stdin: bool,
}

impl PinInput {
    fn read(&self, app: &App) -> Result<SecretString> {
        let pin = read_secret(
            app,
            &SecretSource {
                what: "PIN",
                flag: None,
                stdin_flag: "--pin-stdin",
                env: "PALOMA_PIN",
                missing_is_auth: false,
            },
            None,
            self.pin_stdin,
        )?;
        validate::pin(pin.expose_secret())?;
        Ok(pin)
    }
}

impl NumberCommand {
    #[allow(clippy::too_many_lines)] // one short arm per verb; each arm calls into its module
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::List(a) => {
                validate::page_size(a.page.limit, Some(MAX_PHONE_NUMBERS_PAGE))?;
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                let filter = ListFilter {
                    account_mode: a.account_mode.map(Into::into),
                    oldest_first: matches!(a.sort, Some(Sort::LastOnboardedAsc)),
                    limit: a.page.limit,
                };
                super::listing::list(
                    app,
                    &a.page,
                    |after| phone_numbers::list(&waba, &filter, after),
                    Rows {
                        columns: LIST_COLUMNS,
                        id: "id",
                    },
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
            Self::Oba(command) => command.run(app).await,
            Self::Compliance(command) => command.run(app).await,
            Self::Username(command) => command.run(app).await,
            Self::Contacts(command) => command.run(app).await,
            Self::Settings(command) => command.run(app).await,
            Self::Verification(Verification::Request { method, language }) => {
                validate::not_blank(&language, "language")?;
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                if !app.confirm_or_abort(&format!(
                    "send a verification SMS or voice call to number {phone}"
                ))? {
                    return Ok(());
                }
                let method = match method {
                    CodeMethod::Sms => "SMS",
                    CodeMethod::Voice => "VOICE",
                };
                app.send_and_emit(
                    &phone_numbers::request_code(&phone, method, &language),
                    no_id,
                )
                .await
            }
            Self::Verification(Verification::Confirm { code_stdin }) => {
                let code = read_secret(
                    app,
                    &SecretSource {
                        what: "verification code",
                        flag: None,
                        stdin_flag: "--code-stdin",
                        env: "PALOMA_VERIFICATION_CODE",
                        missing_is_auth: false,
                    },
                    None,
                    code_stdin,
                )?;
                validate::verification_code(code.expose_secret())?;
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                app.send_and_emit(
                    &phone_numbers::verify_code(&phone, code.expose_secret()),
                    no_id,
                )
                .await
            }
            Self::Add {
                phone_number,
                cc,
                name,
            } => {
                validate::calling_code(&cc)?;
                validate::recipient(&phone_number)?;
                if !phone_number.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(paloma_core::Error::validation(
                        "phone number must be international digits without a plus sign",
                    ));
                }
                validate::not_blank(&name, "display name")?;
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                app.send_and_emit(
                    &phone_numbers::add(&waba, &cc, &phone_number, &name),
                    field_id,
                )
                .await
            }
            Self::Name(NameCommand::Set { name }) => {
                validate::not_blank(&name, "display name")?;
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                if !app
                    .confirm_or_abort(&format!("submit a display-name change for number {phone}"))?
                {
                    return Ok(());
                }
                app.send_and_emit(&phone_numbers::set_name(&phone, &name), no_id)
                    .await
            }
            Self::Visibility(VisibilityCommand::Set { visibility }) => {
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                if !app.confirm_or_abort(&format!("change search visibility for number {phone}"))? {
                    return Ok(());
                }
                let value = match visibility {
                    SearchVisibility::Visible => "VISIBLE",
                    SearchVisibility::Hidden => "HIDDEN",
                };
                app.send_and_emit(&phone_numbers::search_visibility(&phone, value), no_id)
                    .await
            }
            Self::Pin(PinCommand::Set(input)) => {
                let pin = input.read(app)?;
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                if !app.confirm_or_abort(&format!(
                    "replace the two-step verification PIN of number {phone}; the old PIN stops working"
                ))? {
                    return Ok(());
                }
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
