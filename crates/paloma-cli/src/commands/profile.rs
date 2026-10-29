//! Meta rejects a whole profile update for one field over its limit, so each field is checked
//! locally. A photo goes up through a resumable upload before the profile points at it.

use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use paloma_core::{Error, Result, limits, validate};
use paloma_graph::api::profile;
use serde_json::{Map, Value};

use super::upload::{HandleUpload, app_id};
use crate::app::{App, no_id};
use crate::output::Output;

#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    /// Show the business profile.
    Get,
    /// Update profile fields.
    Set(Set),
    /// Upload an image and make it the profile photo.
    Photo(Photo),
}

#[derive(Debug, Args)]
pub struct Set {
    /// Short "about" text.
    #[arg(long)]
    about: Option<String>,
    #[arg(long)]
    email: Option<String>,
    /// Website URL; repeat for up to two.
    #[arg(long = "website", value_name = "URL")]
    websites: Vec<String>,
    #[arg(long)]
    address: Option<String>,
    /// Business category.
    #[arg(long, ignore_case = true)]
    vertical: Option<Vertical>,
    #[arg(long)]
    description: Option<String>,
}

impl Set {
    fn validate(&self) -> Result<()> {
        if self.about.as_deref() == Some("") {
            return Err(Error::validation("--about must not be empty"));
        }
        for (value, max, flag) in [
            (&self.about, limits::MAX_PROFILE_ABOUT, "--about"),
            (&self.email, limits::MAX_PROFILE_EMAIL, "--email"),
            (&self.address, limits::MAX_PROFILE_ADDRESS, "--address"),
            (
                &self.description,
                limits::MAX_PROFILE_DESCRIPTION,
                "--description",
            ),
        ] {
            if let Some(value) = value {
                validate::max_chars(value, max, flag)?;
            }
        }
        if self.websites.len() > limits::MAX_PROFILE_WEBSITES {
            return Err(Error::validation(format!(
                "pass at most {} --website values",
                limits::MAX_PROFILE_WEBSITES
            )));
        }
        for website in &self.websites {
            validate::max_chars(website, limits::MAX_PROFILE_WEBSITE, "--website")?;
        }
        Ok(())
    }
}

/// The values Meta documents. `UNDEFINED` and `NOT_A_BIZ` can't be set. Only one of Meta's
/// two reference pages lists `MATRIMONY_SERVICE`, and it's kept so a valid value still works.
#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "SCREAMING_SNAKE_CASE")]
enum Vertical {
    Alcohol,
    Apparel,
    Auto,
    Beauty,
    Edu,
    Entertain,
    EventPlan,
    Finance,
    Govt,
    Grocery,
    Health,
    Hotel,
    MatrimonyService,
    Nonprofit,
    OnlineGambling,
    Other,
    OtcDrugs,
    PhysicalGambling,
    ProfServices,
    Restaurant,
    Retail,
    Travel,
}

impl Vertical {
    fn as_api_value(self) -> String {
        super::util::enum_name(&self)
    }
}

#[derive(Debug, Args)]
pub struct Photo {
    /// JPEG or PNG file.
    file: PathBuf,
    /// MIME type [default: guessed from the extension].
    #[arg(long = "type", value_name = "MIME")]
    mime: Option<String>,
}

impl ProfileCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::Get => {
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                if let Some(resp) = app.send(&profile::get(&phone)).await? {
                    let out = Output::response(&resp)?;
                    // Graph wraps the single profile in `data[0]`; unwrap it for table/yaml.
                    let data = out
                        .data
                        .pointer("/data/0")
                        .cloned()
                        .unwrap_or_else(|| out.data.clone());
                    app.out.emit(&Output { data, ..out });
                }
                Ok(())
            }
            Self::Set(a) => {
                a.validate()?;
                let mut fields = Map::new();
                for (k, v) in [
                    ("about", a.about),
                    ("email", a.email),
                    ("address", a.address),
                    ("vertical", a.vertical.map(Vertical::as_api_value)),
                    ("description", a.description),
                ] {
                    if let Some(v) = v {
                        fields.insert(k.into(), Value::String(v));
                    }
                }
                if !a.websites.is_empty() {
                    fields.insert(
                        "websites".into(),
                        a.websites.into_iter().map(Value::String).collect(),
                    );
                }
                if fields.is_empty() {
                    return Err(Error::validation(
                        "pass at least one field to set (see --help)",
                    ));
                }
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                app.send_and_emit(&profile::set(&phone, fields), no_id)
                    .await
            }
            Self::Photo(a) => {
                let upload = HandleUpload::new(&a.file, a.mime, &limits::PROFILE_PHOTO_TYPES)?;
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                let app_id = app_id(app).await?;
                if app.dry_run() {
                    let [start, transfer] = upload.requests(&app_id);
                    let set = profile::set_photo(&phone, "<handle-from-the-upload>");
                    return app.print_dry_run(&[start, transfer, set]);
                }
                let (handle, _) = upload.run(app, &app_id).await?;
                let updated = app
                    .send_and_emit(&profile::set_photo(&phone, &handle), no_id)
                    .await;
                if updated.is_err() {
                    app.out.warn(
                        "the file uploaded, but the profile update failed. It may still have \
                         applied, so check with `paloma profile get` before you repeat it",
                    );
                }
                updated
            }
        }
    }
}
