//! `export`, `apply` and `diff`. Every manifest is planned against live state first, so
//! only drift is written, a dry run never writes, and `diff` exits 1 on drift.

use std::path::PathBuf;

use clap::Args;
use paloma_config::secrets::KEYRING_PREFIX;
use paloma_core::{Error, ExitCode, Result};
use paloma_manifest::interpolate::Scheme;
use paloma_manifest::kind::Cx;
use paloma_manifest::{Kind, KindPlan, Manifest, Scope, case, plan, yaml};
use serde_json::Value;

use crate::app::App;
use crate::cli::Format;

/// The kinds `export` takes, in kebab-case like every other CLI value. A manifest's `kind:`
/// stays PascalCase, so the PascalCase name works here too.
fn kind_names() -> clap::builder::PossibleValuesParser {
    use clap::builder::PossibleValue;
    Kind::ALL
        .iter()
        .map(|kind| PossibleValue::new(kind.cli_name()).alias(kind.name()))
        .collect::<Vec<_>>()
        .into()
}

#[derive(Debug, Args)]
pub struct Export {
    /// The kind of resource to export.
    #[arg(value_parser = kind_names(), ignore_case = true)]
    kind: String,
    /// Emit a skeleton manifest without calling the API.
    #[arg(long)]
    skeleton: bool,
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
        let kind: Kind = self.kind.parse()?;
        let manifests: Vec<Value> = if self.skeleton {
            let phone = kind.phone_scoped().then_some("<phone-number-id>");
            vec![Manifest::document(
                kind,
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
            let scope = scope(app)?;
            let specs = kind
                .export(Cx {
                    client: app.client()?,
                    scope: &scope,
                })
                .await?;
            specs
                .iter()
                .map(|s| Manifest::document(kind, phone.as_deref(), case::to_camel(s)))
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
        let plans = plans(app, &self.file).await?;
        if let Some(refusal) = plans.iter().find_map(|(_, plan)| plan.refusal.clone()) {
            return Err(Error::validation(refusal));
        }
        // Ask before the first write, so a refusal can't land halfway through the manifest.
        let mut approved = Vec::with_capacity(plans.len());
        for (_, plan) in &plans {
            approved.push(match plan.confirm {
                Some(what) if !plan.in_sync() => app.confirm_or_abort(what)?,
                _ => true,
            });
        }
        for (index, ((label, plan), approved)) in plans.iter().zip(approved).enumerate() {
            if plan.in_sync() {
                app.out.info(format!("{label} unchanged"));
                continue;
            }
            if app.dry_run() {
                app.out.info(format!("{label} would configure:"));
                print_changes(app, plan);
                if let Some(p) = &plan.preview {
                    app.out.stdout(&format!(
                        "{}\n",
                        serde_json::to_string_pretty(&paloma_graph::request::redact_json(p))
                            .unwrap_or_default()
                    ));
                }
                continue;
            }
            if !approved {
                continue;
            }
            if let Err(error) = write(app, label, plan).await {
                let skipped: Vec<&str> = plans[index + 1..]
                    .iter()
                    .filter(|(_, later)| !later.in_sync())
                    .map(|(label, _)| label.as_str())
                    .collect();
                if !skipped.is_empty() {
                    app.out
                        .warn(format!("not attempted: {}", skipped.join(", ")));
                }
                return Err(error);
            }
            app.out.info(format!("{label} configured:"));
            print_changes(app, plan);
        }
        Ok(())
    }
}

/// Send a plan's writes in order. Graph has no transaction across them, so a failure
/// says which ones already took effect and which never ran.
async fn write(app: &App, label: &str, plan: &KindPlan) -> Result<()> {
    for (index, req) in plan.requests.iter().enumerate() {
        if let Err(error) = app.fetch(req).await {
            if plan.requests.len() > 1 {
                app.out.warn(format!(
                    "{label} partly applied; applied: {}; failed: {}; not attempted: {}",
                    request_names(&plan.requests[..index]),
                    request_names(std::slice::from_ref(req)),
                    request_names(&plan.requests[index + 1..]),
                ));
            }
            return Err(error);
        }
    }
    Ok(())
}

/// Requests named by the top-level fields they write; never their values.
fn request_names(requests: &[paloma_graph::request::Request]) -> String {
    let names: Vec<String> = requests
        .iter()
        .map(|req| {
            req.json_body().and_then(Value::as_object).map_or_else(
                || "request".to_owned(),
                |body| body.keys().cloned().collect::<Vec<_>>().join("+"),
            )
        })
        .collect();
    if names.is_empty() {
        "none".to_owned()
    } else {
        names.join(", ")
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

/// The ids the run targets before any document narrows them.
fn scope(app: &App) -> Result<Scope> {
    let ctx = app.ctx()?;
    Ok(Scope::new(
        ctx.waba_id(),
        ctx.phone_number_id(),
        ctx.app_id(),
    ))
}

/// Parse, interpolate and plan every document, failing before any write if one is bad.
async fn plans(app: &App, file: &std::path::Path) -> Result<Vec<(String, KindPlan)>> {
    let text = super::util::read_file(file)?;
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
    let manifests = plan::load(&text, &file.display().to_string(), &lookup)?;

    let base = scope(app)?;
    let client = app.client()?;
    let mut out = Vec::with_capacity(manifests.len());
    for m in manifests {
        let scope = base.for_document(&m, app.globals.phone_number_id.as_deref());
        let plan = m
            .kind
            .plan(
                Cx {
                    client,
                    scope: &scope,
                },
                &case::to_snake(&m.spec),
            )
            .await?;
        out.push((scope.label(m.kind), plan));
    }
    Ok(out)
}
