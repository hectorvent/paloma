mod hours;
mod insights;

use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use paloma_core::limits::DEFAULT_SIP_PORT;
use paloma_core::{Error, Result};
use paloma_graph::api::calling;
use serde_json::{Map, Value, json};

use super::read_json;
use crate::app::{App, no_id};

#[derive(Debug, Subcommand)]
pub enum CallingCommand {
    /// Show the calling settings.
    Status {
        /// Include SIP credentials in the response.
        #[arg(long)]
        sip_credentials: bool,
    },
    /// Turn calling on.
    Enable,
    /// Turn calling off.
    Disable,
    /// Update calling settings.
    Set(Set),
    /// Configure SIP signaling.
    #[command(subcommand)]
    Sip(Sip),
    /// Configure additional audio codecs.
    #[command(subcommand)]
    Codecs(Codecs),
    /// Show or change the hours when people can call you.
    #[command(subcommand)]
    Hours(hours::Hours),
    /// Show whether a person can be called, and the remaining limits.
    Permission {
        /// E.164 number or business-scoped user ID.
        user: String,
    },
    /// Report call counts, costs and durations.
    Analytics(insights::Analytics),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "SCREAMING_SNAKE_CASE")]
enum Toggle {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "SCREAMING_SNAKE_CASE")]
enum CallIcon {
    Default,
    DisableAll,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Codec {
    Pcma,
    Pcmu,
}

fn enum_str(v: &impl ValueEnum) -> String {
    v.to_possible_value()
        .expect("no skipped variants")
        .get_name()
        .to_owned()
}

#[derive(Debug, Args)]
pub struct Set {
    /// Calling status.
    #[arg(long, value_enum, ignore_case = true)]
    status: Option<Toggle>,
    /// Call button visibility.
    #[arg(long, value_enum, ignore_case = true)]
    call_icon: Option<CallIcon>,
    /// Whether users can grant callback permission.
    #[arg(long, value_enum, ignore_case = true)]
    callback_permission: Option<Toggle>,
    /// A full `calling` object (JSON) for settings without a flag (hours, ...).
    #[arg(long, conflicts_with_all = ["status", "call_icon", "callback_permission"])]
    file: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Sip {
    /// Enable SIP and set its servers.
    Set(SipSet),
    /// Disable SIP.
    Disable,
}

#[derive(Debug, Args)]
pub struct SipSet {
    /// SIP server as host[:port] (default port 5061); repeatable.
    #[arg(long = "server", value_name = "HOST[:PORT]", required = true)]
    servers: Vec<String>,
    /// `tgrp` request-URI user parameter.
    #[arg(long)]
    tgrp: Option<String>,
    /// `context` request-URI user parameter.
    #[arg(long)]
    context: Option<String>,
    /// Also set calling status to ENABLED.
    #[arg(long)]
    enable_calling: bool,
}

#[derive(Debug, Subcommand)]
pub enum Codecs {
    /// Set the additional codecs.
    Set {
        #[arg(required = true, value_enum, ignore_case = true)]
        codecs: Vec<Codec>,
    },
    /// Remove all additional codecs.
    Clear,
}

impl CallingCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        // These validate their own input before touching context.
        let command = match self {
            Self::Hours(command) => return command.run(app).await,
            Self::Permission { user } => return insights::permission(app, &user).await,
            Self::Analytics(args) => return args.run(app).await,
            other => other,
        };
        let phone = app.ctx()?.require_phone_number_id()?.to_owned();
        let calling = match command {
            Self::Hours(_) | Self::Permission { .. } | Self::Analytics(_) => {
                unreachable!("handled before context resolution")
            }
            Self::Status { sip_credentials } => {
                return app
                    .send_and_emit(&calling::get(&phone, sip_credentials), no_id)
                    .await;
            }
            Self::Enable => json!({ "status": "ENABLED" }),
            Self::Disable => json!({ "status": "DISABLED" }),
            Self::Set(a) => match a.file {
                Some(file) => {
                    let v = read_json(&file)?;
                    if !v.is_object() {
                        return Err(Error::validation(
                            "--file must hold a JSON object (the `calling` settings)",
                        ));
                    }
                    v
                }
                None => {
                    let mut m = Map::new();
                    if let Some(s) = a.status {
                        m.insert("status".into(), enum_str(&s).into());
                    }
                    if let Some(i) = a.call_icon {
                        m.insert("call_icon_visibility".into(), enum_str(&i).into());
                    }
                    if let Some(c) = a.callback_permission {
                        m.insert("callback_permission_status".into(), enum_str(&c).into());
                    }
                    if m.is_empty() {
                        return Err(Error::validation(
                            "pass at least one of --status, --call-icon, --callback-permission, --file",
                        ));
                    }
                    Value::Object(m)
                }
            },
            Self::Sip(Sip::Set(a)) => sip_set(&a)?,
            Self::Sip(Sip::Disable) => json!({ "sip": { "status": "DISABLED" } }),
            Self::Codecs(Codecs::Set { codecs }) => {
                let names: Vec<String> = codecs.iter().map(enum_str).collect();
                json!({ "audio": { "additional_codecs": names } })
            }
            Self::Codecs(Codecs::Clear) => json!({ "audio": { "additional_codecs": [] } }),
        };
        app.send_and_emit(&calling::set(&phone, calling), no_id)
            .await
    }
}

fn sip_set(a: &SipSet) -> Result<Value> {
    let mut params = Map::new();
    if let Some(t) = a.tgrp.as_deref().filter(|s| !s.is_empty()) {
        params.insert("tgrp".into(), t.into());
    }
    if let Some(c) = a.context.as_deref().filter(|s| !s.is_empty()) {
        params.insert("context".into(), c.into());
    }
    let servers = a
        .servers
        .iter()
        .map(|s| {
            let (host, port) = parse_server(s)?;
            let mut server = json!({ "hostname": host, "port": port });
            if !params.is_empty() {
                server["request_uri_user_params"] = Value::Object(params.clone());
            }
            Ok(server)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut calling = Map::new();
    if a.enable_calling {
        calling.insert("status".into(), "ENABLED".into());
    }
    calling.insert(
        "sip".into(),
        json!({ "status": "ENABLED", "servers": servers }),
    );
    Ok(Value::Object(calling))
}

/// `host[:port]`, split on the last colon.
fn parse_server(spec: &str) -> Result<(String, u16)> {
    let spec = spec.trim();
    let (host, port) = match spec.rsplit_once(':') {
        Some((h, p)) => {
            let port: u16 = p.parse().ok().filter(|p| *p > 0).ok_or_else(|| {
                Error::validation(format!("invalid port in --server '{spec}' (1-65535)"))
            })?;
            (h, port)
        }
        None => (spec, DEFAULT_SIP_PORT),
    };
    if host.is_empty() {
        return Err(Error::validation(format!("--server '{spec}' has no host")));
    }
    Ok((host.to_owned(), port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_parsing() {
        assert_eq!(
            parse_server("sip.example.com").unwrap(),
            ("sip.example.com".into(), 5061)
        );
        assert_eq!(
            parse_server("sip.example.com:5060").unwrap(),
            ("sip.example.com".into(), 5060)
        );
        assert!(parse_server("sip.example.com:0").is_err());
        assert!(parse_server("sip.example.com:99999").is_err());
        assert!(parse_server(":5060").is_err());
    }
}
