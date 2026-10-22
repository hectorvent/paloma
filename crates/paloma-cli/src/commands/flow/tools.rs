//! Metrics preserve Meta's response, migration copies rather than moves, and key
//! installation accepts only a public PEM file, keeping private material out of requests.
use crate::app::{App, no_id};
use crate::commands::read_file;
use crate::output::Output;
use clap::{Args, Subcommand, ValueEnum};
use paloma_core::{Error, Result, limits, validate};
use paloma_graph::api::flows;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "SCREAMING_SNAKE_CASE")]
#[allow(clippy::enum_variant_names)] // These are Meta's public metric names, also exposed by clap.
pub enum MetricName {
    EndpointRequestCount,
    EndpointRequestError,
    EndpointRequestErrorRate,
    EndpointRequestLatencySecondsCeil,
    EndpointAvailability,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Granularity {
    Day,
    Hour,
    Lifetime,
}

#[derive(Debug, Args)]
pub struct Metrics {
    flow_id: String,
    #[arg(long, value_enum)]
    name: MetricName,
    #[arg(long, value_enum, default_value = "DAY")]
    granularity: Granularity,
    #[arg(long)]
    since: Option<String>,
    #[arg(long)]
    until: Option<String>,
}
impl Metrics {
    pub async fn run(self, app: &App) -> Result<()> {
        validate::numeric_id(&self.flow_id, "flow id")?;
        let start = self
            .since
            .as_deref()
            .map(|v| validate::template_analytics_time(v, "since", true))
            .transpose()?;
        let end = self
            .until
            .as_deref()
            .map(|v| validate::template_analytics_time(v, "until", true))
            .transpose()?;
        if matches!((start,end),(Some(a),Some(b)) if a>b) {
            return Err(Error::validation("since must not follow until"));
        }
        let name = self.name.to_possible_value().expect("metric enum");
        let granularity = self
            .granularity
            .to_possible_value()
            .expect("granularity enum");
        let req = flows::metrics(
            &self.flow_id,
            name.get_name(),
            granularity.get_name(),
            self.since.as_deref(),
            self.until.as_deref(),
        );
        app.send_and_emit(&req, no_id).await
    }
}
#[derive(Debug, Args)]
pub struct Migrate {
    #[arg(long)]
    source_waba_id: String,
    /// Names to copy; omit to copy all, up to Meta's per-request limit.
    #[arg(long = "name")]
    names: Vec<String>,
}
impl Migrate {
    pub async fn run(self, app: &App) -> Result<()> {
        validate::numeric_id(&self.source_waba_id, "source WABA id")?;
        if self.names.len() > limits::MAX_FLOW_MIGRATION_NAMES {
            return Err(Error::validation(
                "at most 100 Flow names may be migrated per request",
            ));
        }
        for name in &self.names {
            validate::not_blank(name, "Flow name")?;
        }
        let destination = app.ctx()?.require_waba_id()?.to_owned();
        if destination == self.source_waba_id {
            return Err(Error::validation(
                "source and destination WABAs must differ",
            ));
        }
        if !app.confirm_or_abort(&format!(
            "copy Flows from {} into WABA {destination}; published Flows remain published",
            self.source_waba_id
        ))? {
            return Ok(());
        }
        let request = flows::migrate(&destination, &self.source_waba_id, &self.names);
        let Some(response) = app.send(&request).await? else {
            return Ok(());
        };
        let mut output = Output::response(&response)?;
        let ids = output
            .data
            .get("migrated_flows")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|v| v.get("migrated_id").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>()
            .join("\n");
        output.primary_id = (!ids.is_empty()).then_some(ids);
        let failed = output
            .data
            .get("failed_flows")
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len);
        app.out.emit(&output);
        if failed > 0 {
            app.out.warn(format!(
                "{failed} Flow migration(s) failed; inspect failed_flows in the response"
            ));
            return Err(Error::Silent(paloma_core::error::ExitCode::Generic));
        }
        Ok(())
    }
}
#[derive(Debug, Subcommand)]
pub enum KeyCommand {
    /// Read the installed public key and signature status for this phone number.
    Get,
    /// Install a 2048-bit RSA public key; never pass the private key.
    Set {
        #[arg(long)]
        public_key: PathBuf,
    },
}
impl KeyCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        let key = match self {
            Self::Get => None,
            Self::Set { public_key } => {
                let value = read_file(&public_key)?;
                let value = value.trim();
                if !value.starts_with("-----BEGIN PUBLIC KEY-----")
                    || !value.ends_with("-----END PUBLIC KEY-----")
                    || value.contains("PRIVATE KEY")
                {
                    return Err(Error::validation(
                        "--public-key must contain a PEM public key, never a private key",
                    ));
                }
                Some(value.to_owned())
            }
        };
        let phone = app.ctx()?.require_phone_number_id()?.to_owned();
        if let Some(key) = key {
            if !app.confirm_or_abort(&format!(
                "replace the Flow endpoint public key for number {phone}"
            ))? {
                return Ok(());
            }
            app.send_and_emit(&flows::set_key(&phone, &key), no_id)
                .await
        } else {
            app.send_and_emit(&flows::get_key(&phone), no_id).await
        }
    }
}
