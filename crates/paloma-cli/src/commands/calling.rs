//! Calling settings are partial updates of one `calling` object. Inputs Meta would drop
//! without an error (a timeout trigger with no timeout, a bad SIP port) are refused here.

mod hours;
mod insights;

use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use paloma_core::limits::{DEFAULT_SIP_PORT, MAX_VOICEMAIL_TIMEOUT_SECONDS};
use paloma_core::{Error, Result, validate};
use paloma_graph::api::calling;

use super::util::read_json;
use crate::app::{App, no_id};

#[derive(Debug, Subcommand)]
pub enum CallingCommand {
    /// Show the calling settings.
    Get {
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
    /// Take voicemail for missed or rejected calls.
    #[command(subcommand)]
    Voicemail(Voicemail),
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

/// A CLI enum with the same variants as its `paloma_graph::api::calling` counterpart, which
/// carries Meta's wire value. clap needs its own `ValueEnum`, and the graph crate has no clap.
macro_rules! cli_enum {
    ($(#[$doc:meta])* $vis:vis $name:ident => $graph:ident { $($(#[$vdoc:meta])* $variant:ident),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
        #[value(rename_all = "SCREAMING_SNAKE_CASE")]
        $vis enum $name {
            $($(#[$vdoc])* $variant),+
        }

        impl From<$name> for calling::$graph {
            fn from(value: $name) -> Self {
                match value {
                    $($name::$variant => Self::$variant),+
                }
            }
        }
    };
}

cli_enum!(Toggle => Toggle { Enabled, Disabled });
cli_enum!(CallIcon => CallIcon { Default, DisableAll });
cli_enum!(pub Codec => Codec { Pcma, Pcmu });
cli_enum!(
    /// The guide's values; see `paloma_graph::api::calling::Srtp`.
    Srtp => Srtp { Dtls, Sdes }
);
cli_enum!(VoicemailTrigger => VoicemailTrigger {
    /// You reject the call.
    Reject,
    /// Nobody accepts or rejects the call within --ring-seconds.
    Timeout,
});

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
    /// Show the call button only to people with phone numbers from these countries.
    #[arg(long, value_name = "CC", value_delimiter = ',')]
    icon_countries: Vec<String>,
    /// Show the call button in every country again.
    #[arg(long, conflicts_with = "icon_countries")]
    icon_everywhere: bool,
    /// SRTP key exchange protocol for SIP calls.
    #[arg(long, value_enum, ignore_case = true)]
    srtp: Option<Srtp>,
    /// A full `calling` object (JSON) for settings without a flag (video, ...).
    #[arg(short = 'f', long, conflicts_with_all = [
        "status", "call_icon", "callback_permission", "icon_countries", "icon_everywhere", "srtp",
    ])]
    file: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Voicemail {
    /// Turn voicemail on.
    Set(VoicemailSet),
    /// Turn voicemail off.
    Disable,
}

#[derive(Debug, Args)]
pub struct VoicemailSet {
    /// When voicemail answers; repeatable or comma-separated.
    #[arg(
        long = "trigger",
        value_enum,
        ignore_case = true,
        value_delimiter = ',',
        value_name = "TRIGGER,...",
        required = true
    )]
    triggers: Vec<VoicemailTrigger>,
    /// Media id of the greeting, from `paloma media upload --voicemail-announcement`.
    #[arg(long, value_name = "MEDIA_ID")]
    announcement: String,
    /// Seconds the call rings before voicemail answers (0-30); required with the timeout trigger.
    #[arg(long, value_name = "SECONDS")]
    ring_seconds: Option<u32>,
}

#[derive(Debug, Subcommand)]
pub enum Sip {
    /// Enable SIP and set its servers.
    Set(SipSet),
    /// Turn SIP off.
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
            Self::Get { sip_credentials } => {
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                return app
                    .send_and_emit(&calling::get(&phone, sip_credentials), no_id)
                    .await;
            }
            other => other,
        };
        let phone =
            |app: &App| -> Result<String> { Ok(app.ctx()?.require_phone_number_id()?.to_owned()) };
        // Turning a feature off, or emptying a list, changes what callers get.
        let confirm = match &command {
            Self::Disable => Some(calling::DISABLE_CONFIRM),
            Self::Sip(Sip::Disable) => Some(calling::SIP_OFF_CONFIRM),
            Self::Voicemail(Voicemail::Disable) => Some(calling::VOICEMAIL_OFF_CONFIRM),
            Self::Codecs(Codecs::Clear) => Some(calling::CODECS_CLEAR_CONFIRM),
            _ => None,
        };
        let req = match command {
            Self::Hours(_) | Self::Permission { .. } | Self::Analytics(_) | Self::Get { .. } => {
                unreachable!("handled above")
            }
            Self::Enable => calling::set_status(&phone(app)?, true),
            Self::Disable => calling::set_status(&phone(app)?, false),
            Self::Set(a) => return set(app, a).await,
            Self::Sip(Sip::Set(a)) => {
                let servers = a
                    .servers
                    .iter()
                    .map(|s| parse_server(s))
                    .collect::<Result<Vec<_>>>()?;
                calling::set_sip(
                    &phone(app)?,
                    &servers,
                    a.tgrp.as_deref(),
                    a.context.as_deref(),
                    a.enable_calling,
                )
            }
            Self::Sip(Sip::Disable) => calling::disable_sip(&phone(app)?),
            Self::Codecs(Codecs::Set { codecs }) => {
                let codecs: Vec<calling::Codec> = codecs.into_iter().map(Into::into).collect();
                calling::set_codecs(&phone(app)?, &codecs)
            }
            Self::Codecs(Codecs::Clear) => calling::set_codecs(&phone(app)?, &[]),
            Self::Voicemail(Voicemail::Set(a)) => {
                let (triggers, media_id, timeout) = voicemail(&a)?;
                let req = calling::set_voicemail(&phone(app)?, &triggers, media_id, timeout);
                app.out.note(
                    "Meta advises turning call hours off while voicemail is on, because people \
                     can't call outside them (paloma calling hours disable)",
                );
                req
            }
            Self::Voicemail(Voicemail::Disable) => calling::disable_voicemail(&phone(app)?),
        };
        if let Some(what) = confirm
            && !app.confirm_or_abort(what)?
        {
            return Ok(());
        }
        app.send_and_emit(&req, no_id).await
    }
}

async fn set(app: &App, a: Set) -> Result<()> {
    if let Some(file) = a.file {
        let v = read_json(&file)?;
        if !v.is_object() {
            return Err(Error::validation(
                "--file must hold a JSON object (the `calling` settings)",
            ));
        }
        let phone = app.ctx()?.require_phone_number_id()?.to_owned();
        if let Some(what) = calling::confirm_for(&v)
            && !app.confirm_or_abort(what)?
        {
            return Ok(());
        }
        return app.send_and_emit(&calling::set(&phone, v), no_id).await;
    }
    let icon_countries = if a.icon_countries.is_empty() && !a.icon_everywhere {
        None
    } else {
        Some(
            a.icon_countries
                .iter()
                .map(|c| {
                    let c = c.trim().to_ascii_uppercase();
                    validate::country_code(&c).map(|()| c)
                })
                .collect::<Result<Vec<_>>>()?,
        )
    };
    let settings = calling::Settings {
        status: a.status.map(Into::into),
        call_icon_visibility: a.call_icon.map(Into::into),
        callback_permission_status: a.callback_permission.map(Into::into),
        icon_countries,
        srtp_key_exchange_protocol: a.srtp.map(Into::into),
    };
    if settings.is_empty() {
        return Err(Error::validation(
            "pass at least one of --status, --call-icon, --callback-permission, --icon-countries, \
             --icon-everywhere, --srtp, --file",
        ));
    }
    let phone = app.ctx()?.require_phone_number_id()?.to_owned();
    if settings.turns_off() && !app.confirm_or_abort(calling::DISABLE_CONFIRM)? {
        return Ok(());
    }
    app.send_and_emit(&calling::set_settings(&phone, &settings), no_id)
        .await
}

/// Triggers (deduplicated, in order), the greeting's media id and the ring timeout. Meta
/// silently drops a `TIMEOUT` trigger without a timeout, so that is refused here.
fn voicemail(a: &VoicemailSet) -> Result<(Vec<calling::VoicemailTrigger>, u64, Option<u32>)> {
    let announcement = a.announcement.trim();
    validate::numeric_id(announcement, "--announcement")?;
    let media_id: u64 = announcement
        .parse()
        .map_err(|_| Error::validation("--announcement is too long for a media id"))?;
    let on_timeout = a.triggers.contains(&VoicemailTrigger::Timeout);
    let timeout = match (on_timeout, a.ring_seconds) {
        (true, None) => {
            return Err(Error::validation(
                "--trigger timeout needs --ring-seconds, or Meta ignores the trigger",
            ));
        }
        (false, Some(_)) => {
            return Err(Error::validation(
                "--ring-seconds only applies with --trigger timeout",
            ));
        }
        (true, Some(seconds)) if seconds > MAX_VOICEMAIL_TIMEOUT_SECONDS => {
            return Err(Error::validation(format!(
                "--ring-seconds must be 0-{MAX_VOICEMAIL_TIMEOUT_SECONDS}"
            )));
        }
        (true, Some(seconds)) => Some(seconds),
        (false, None) => None,
    };
    let mut triggers: Vec<calling::VoicemailTrigger> = Vec::new();
    for trigger in &a.triggers {
        let trigger = (*trigger).into();
        if !triggers.contains(&trigger) {
            triggers.push(trigger);
        }
    }
    Ok((triggers, media_id, timeout))
}

/// `host[:port]`, split on the last colon.
fn parse_server(spec: &str) -> Result<calling::SipServer> {
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
    Ok(calling::SipServer {
        hostname: host.to_owned(),
        port,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_parsing() {
        assert_eq!(
            parse_server("sip.example.com").unwrap(),
            calling::SipServer {
                hostname: "sip.example.com".into(),
                port: 5061
            }
        );
        assert_eq!(
            parse_server("sip.example.com:5060").unwrap(),
            calling::SipServer {
                hostname: "sip.example.com".into(),
                port: 5060
            }
        );
        assert!(parse_server("sip.example.com:0").is_err());
        assert!(parse_server("sip.example.com:99999").is_err());
        assert!(parse_server(":5060").is_err());
    }
}
