//! Regional compliance and OBA applications are explicit submissions, not automatic
//! registration steps. Validate their required documents before credentials or HTTP.
use crate::{
    app::{App, no_id},
    commands::util::read_json,
};
use clap::Subcommand;
use paloma_core::{Error, Result, limits, validate};
use paloma_graph::{api::phone_numbers, callback};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum ObaCommand {
    /// Show the Official Business Account application status.
    Get,
    /// Submit an application document containing website, country and evidence links.
    Apply {
        /// The application (JSON): website, country and evidence links.
        #[arg(short = 'f', long)]
        file: PathBuf,
    },
}
#[derive(Debug, Subcommand)]
pub enum ComplianceCommand {
    /// Show business compliance information (India).
    Get,
    /// Submit legal entity and grievance/customer-care details from a JSON document.
    Set {
        /// The compliance details (JSON): legal entity, grievance officer and customer care.
        #[arg(short = 'f', long)]
        file: PathBuf,
    },
}
fn text<'a>(body: &'a Value, key: &str) -> Result<&'a str> {
    let value = body
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::validation(format!("{key} must be a nonempty string")))?;
    validate::not_blank(value, key)?;
    Ok(value)
}
fn oba(body: &Value) -> Result<()> {
    if !body.is_object() {
        return Err(Error::validation("OBA application must be a JSON object"));
    }
    callback::parse_url(text(body, "business_website_url")?)?;
    // Meta documents a free-form string here, so its format is Meta's to judge.
    text(body, "primary_country_of_operation")?;
    if let Some(links) = body.get("supporting_links") {
        let links = links
            .as_array()
            .ok_or_else(|| Error::validation("supporting_links must be an array"))?;
        if !(limits::MIN_OBA_SUPPORTING_LINKS..=limits::MAX_OBA_SUPPORTING_LINKS)
            .contains(&links.len())
        {
            return Err(Error::validation("supply 5–10 supporting links"));
        }
        for link in links {
            callback::parse_url(
                link.as_str()
                    .ok_or_else(|| Error::validation("supporting links must be URLs"))?,
            )?;
        }
    }
    Ok(())
}
fn contact_details(value: &Value) -> Result<()> {
    let email = text(value, "email")?;
    validate::max_chars(email, limits::MAX_COMPLIANCE_EMAIL, "email")?;
    let valid = email.split_once('@').is_some_and(|(local, domain)| {
        !local.is_empty()
            && domain.contains('.')
            && !domain.starts_with('.')
            && !domain.ends_with('.')
            && !domain.contains('@')
    });
    if !valid || email.chars().any(char::is_whitespace) {
        return Err(Error::validation(
            "contact email must be a valid email address",
        ));
    }
    for key in ["mobile_number", "landline_number"] {
        if let Some(number) = value.get(key) {
            let number = number
                .as_str()
                .ok_or_else(|| Error::validation("contact phone numbers must be strings"))?;
            validate::recipient(number)?;
            if validate::bsuid(number).is_some() {
                return Err(Error::validation(
                    "contact phone numbers must be international phone numbers, not a BSUID",
                ));
            }
        }
    }
    Ok(())
}

fn compliance(body: &mut Value) -> Result<()> {
    if !body.is_object() {
        return Err(Error::validation(
            "compliance document must be a JSON object",
        ));
    }
    let name = text(body, "entity_name")?;
    if name.chars().count() < limits::MIN_COMPLIANCE_ENTITY_NAME {
        return Err(Error::validation(
            "entity_name must have at least 2 characters",
        ));
    }
    validate::max_chars(name, limits::MAX_COMPLIANCE_ENTITY_NAME, "entity_name")?;
    let kind = text(body, "entity_type")?;
    if !matches!(
        kind,
        "LIMITED_LIABILITY_PARTNERSHIP"
            | "SOLE_PROPRIETORSHIP"
            | "PARTNERSHIP"
            | "PUBLIC_COMPANY"
            | "PRIVATE_COMPANY"
            | "OTHER"
    ) {
        return Err(Error::validation(
            "entity_type must be a documented business entity type",
        ));
    }
    if kind == "OTHER" {
        text(body, "entity_type_custom")?;
    } else if body.get("entity_type_custom").is_some() {
        return Err(Error::validation(
            "entity_type_custom is only valid for OTHER",
        ));
    }
    if let Some(value) = body.get("is_registered")
        && (!value.is_boolean() || !matches!(kind, "OTHER" | "PARTNERSHIP"))
    {
        return Err(Error::validation(
            "is_registered is boolean and only valid for OTHER or PARTNERSHIP",
        ));
    }
    let officer = body
        .get("grievance_officer_details")
        .ok_or_else(|| Error::validation("grievance_officer_details is required"))?;
    text(officer, "name")?;
    contact_details(officer)?;
    let care = body
        .get("customer_care_details")
        .ok_or_else(|| Error::validation("customer_care_details is required"))?;
    contact_details(care)?;
    if body
        .get("messaging_product")
        .is_some_and(|v| v != "whatsapp")
    {
        return Err(Error::validation("messaging_product must be whatsapp"));
    }
    body["messaging_product"] = Value::String("whatsapp".into());
    Ok(())
}
impl ObaCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::Get => {
                // Status is a field on the number: a live read of the edge returns
                // "nonexisting field", and Meta's guide uses this form.
                let phone = app.phone()?;
                app.send_and_emit(
                    &phone_numbers::get(phone, "official_business_account"),
                    |v| phone_numbers::oba_status(v).map(str::to_owned),
                )
                .await
            }
            Self::Apply { file } => {
                let body = read_json(&file)?;
                oba(&body)?;
                submit(app, "official_business_account", Some(body)).await
            }
        }
    }
}
impl ComplianceCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        let body = match self {
            Self::Get => None,
            Self::Set { file } => {
                let mut body = read_json(&file)?;
                compliance(&mut body)?;
                Some(body)
            }
        };
        submit(app, "business_compliance_info", body).await
    }
}
async fn submit(app: &App, edge: &str, body: Option<Value>) -> Result<()> {
    let phone = app.phone()?;
    if body.is_some()
        && !app.confirm_or_abort(&format!("submit {edge} information for number {phone}"))?
    {
        return Ok(());
    }
    app.send_and_emit(&phone_numbers::administration(phone, edge, body), no_id)
        .await
}
