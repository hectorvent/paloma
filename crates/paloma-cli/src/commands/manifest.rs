use std::path::PathBuf;

use clap::Args;
use paloma_config::secrets::KEYRING_PREFIX;
use paloma_core::{Error, ExitCode, Result};
use paloma_manifest::interpolate::{Scheme, interpolate};
use paloma_manifest::kind::Cx;
use paloma_manifest::{Kind, KindPlan, Manifest, case, yaml};
use serde_json::Value;

use crate::app::App;
use crate::cli::Format;

#[derive(Debug, Args)]
pub struct Export {
    /// Account, Calling, Commerce, ConversationalAutomation, Number, PhoneSettings, Profile, QrCode, Template or Webhook.
    kind: String,
    /// Emit a skeleton manifest without calling the API.
    #[arg(long)]
    template: bool,
    /// Only emit fields that are set (no commented-out stubs).
    #[arg(long)]
    minimal: bool,
}

#[derive(Debug, Args)]
pub struct Apply {
    /// Manifest YAML file (multi-document supported).
    #[arg(short, long)]
    file: PathBuf,
}

#[derive(Debug, Args)]
pub struct Diff {
    /// Manifest YAML file (multi-document supported).
    #[arg(short, long)]
    file: PathBuf,
}

impl Export {
    pub async fn run(self, app: &App) -> Result<()> {
        let kind = Kind::parse(&self.kind)?;
        let manifests: Vec<Value> = if self.template {
            let phone = kind.phone_scoped().then_some("<phone-number-id>");
            vec![Manifest::document(
                kind.name(),
                phone,
                Value::Object(serde_json::Map::new()),
            )]
        } else {
            let ctx = app.ctx()?;
            let phone = if kind.phone_scoped() {
                Some(ctx.require_phone_number_id()?.to_owned())
            } else {
                None
            };
            let specs = kind
                .export(Cx {
                    client: app.client()?,
                    ctx,
                })
                .await?;
            specs
                .iter()
                .map(|s| Manifest::document(kind.name(), phone.as_deref(), case::to_camel(s)))
                .collect()
        };
        if manifests.is_empty() {
            app.out
                .note(format!("no {} resources to export", kind.name()));
            return Ok(());
        }
        let text = if app.out.format() == Format::Json {
            let v = if manifests.len() == 1 {
                manifests[0].clone()
            } else {
                Value::Array(manifests)
            };
            serde_json::to_string_pretty(&v).unwrap_or_default()
        } else {
            {
                let template = kind.spec_template();
                manifests
                    .iter()
                    .map(|m| {
                        if self.minimal {
                            yaml::to_yaml(m)
                        } else {
                            yaml::to_annotated_yaml(m, &template)
                        }
                    })
                    .map(|d| d.trim_end().to_owned())
                    .collect::<Vec<_>>()
                    .join("\n---\n")
            }
        };
        app.out.stdout(&format!("{text}\n"));
        Ok(())
    }
}

impl Apply {
    pub async fn run(self, app: &App) -> Result<()> {
        for (label, plan) in plans(app, &self.file).await? {
            if plan.in_sync() {
                app.out.info(format!("{label} unchanged"));
                continue;
            }
            if app.dry_run() {
                app.out.info(format!("{label} would configure:"));
                print_changes(app, &plan);
                if let Some(p) = &plan.preview {
                    app.out.stdout(&format!(
                        "{}\n",
                        serde_json::to_string_pretty(&paloma_graph::request::redact_json(p))
                            .unwrap_or_default()
                    ));
                }
                continue;
            }
            let changes_number_settings = plan.requests.iter().any(|req| matches!(&req.body, paloma_graph::request::Body::Json(body) if body.get("storage_configuration").is_some() || body.get("user_identity_change").is_some() || body.get("new_display_name").is_some() || body.get("search_visibility").is_some()));
            if changes_number_settings
                && !app.confirm_or_abort(
                    "change number identity or storage settings; storage changes require deregistration and re-registration",
                )?
            {
                continue;
            }
            for req in &plan.requests {
                app.fetch(req).await?;
            }
            app.out.info(format!("{label} configured:"));
            print_changes(app, &plan);
        }
        Ok(())
    }
}

impl Diff {
    pub async fn run(self, app: &App) -> Result<()> {
        let mut drift = false;
        for (label, plan) in plans(app, &self.file).await? {
            if plan.in_sync() {
                app.out.info(format!("{label} in sync"));
            } else {
                drift = true;
                app.out.info(format!("{label} drift:"));
                print_changes(app, &plan);
            }
        }
        if drift {
            Err(Error::Silent(ExitCode::Generic))
        } else {
            Ok(())
        }
    }
}

fn print_changes(app: &App, plan: &KindPlan) {
    for c in &plan.changes {
        app.out.info(format!("  {c}"));
    }
}

/// Parse, interpolate and plan every document, failing before any write if one is bad.
async fn plans(app: &App, file: &std::path::Path) -> Result<Vec<(String, KindPlan)>> {
    let text = super::read_file(file)?;
    let docs = yaml::read_documents(&text, &file.display().to_string())?;
    let lookup = |scheme: Scheme, name: &str| -> Option<String> {
        match scheme {
            Scheme::Env => std::env::var(name).ok(),
            Scheme::Keyring => app
                .store()
                .get(name.strip_prefix(KEYRING_PREFIX).unwrap_or(name))
                .ok()
                .flatten(),
        }
    };
    let manifests = docs
        .iter()
        .map(|d| {
            Manifest::from_doc(&interpolate(d, &lookup)?)
                .and_then(|m| Ok((Kind::parse(&m.kind)?, m)))
        })
        .collect::<Result<Vec<_>>>()?;

    let base = app.ctx()?;
    let client = app.client()?;
    let mut out = Vec::with_capacity(manifests.len());
    for (kind, m) in manifests {
        let target = app
            .globals
            .phone_number_id
            .clone()
            .or_else(|| m.phone_number_id());
        let ctx = match target {
            Some(id) => base.with_phone_number_id(id),
            None => base.with_phone_number_id(base.phone_number_id.clone().unwrap_or_default()),
        };
        let scope = if kind.phone_scoped() {
            ctx.phone_number_id.as_deref()
        } else {
            ctx.waba_id.as_deref()
        };
        let label = format!(
            "{}/{}",
            kind.name(),
            scope.filter(|s| !s.is_empty()).unwrap_or("?")
        );
        let plan = kind
            .plan(Cx { client, ctx: &ctx }, &case::to_snake(&m.spec))
            .await?;
        out.push((label, plan));
    }
    Ok(out)
}
