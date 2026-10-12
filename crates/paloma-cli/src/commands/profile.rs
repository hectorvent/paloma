use std::path::PathBuf;

use clap::{Args, Subcommand};
use paloma_core::{Error, Result, limits};
use paloma_graph::api::profile;
use serde_json::{Map, Value};

use super::{HandleUpload, app_id};
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
    /// Business category, e.g. RETAIL, PROF_SERVICES.
    #[arg(long)]
    vertical: Option<String>,
    #[arg(long)]
    description: Option<String>,
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
                let mut fields = Map::new();
                for (k, v) in [
                    ("about", a.about),
                    ("email", a.email),
                    ("address", a.address),
                    ("vertical", a.vertical),
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
