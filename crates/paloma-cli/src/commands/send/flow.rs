//! Flow sends: the button that opens a WhatsApp Flow, with its token and starting data.

use super::{
    App, ArgGroup, Args, Error, InteractiveCommon, PathBuf, Result, Value, ValueEnum, check_common,
    dispatch, interactive_media_header, limits, messages, read_json, validate,
};

/// Where a Flow starts: `data_exchange` asks your endpoint for the first screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FlowAction {
    Navigate,
    #[value(name = "data_exchange")]
    DataExchange,
}

impl FlowAction {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Navigate => "navigate",
            Self::DataExchange => "data_exchange",
        }
    }
}

/// Which version of a Flow to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FlowMode {
    Draft,
    Published,
}

impl FlowMode {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Published => "published",
        }
    }
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("flow_ref").required(true).args(["flow_id", "flow_name"])))]
pub struct FlowMessage {
    #[command(flatten)]
    pub(super) common: InteractiveCommon,
    /// The Flow's id.
    #[arg(long)]
    pub(super) flow_id: Option<String>,
    /// The Flow's name.
    #[arg(long)]
    pub(super) flow_name: Option<String>,
    /// Text on the button that opens the Flow. Meta advises 30 characters or less, no emoji.
    #[arg(long)]
    pub(super) cta: String,
    /// Your own id for this send, returned when the Flow completes. Meta defaults it to
    /// "unused".
    #[arg(long, value_name = "ID")]
    pub(super) flow_token: Option<String>,
    /// How the Flow starts. Meta defaults to navigate.
    #[arg(long, value_enum)]
    pub(super) action: Option<FlowAction>,
    /// Send the draft or the published version. Meta defaults to published.
    #[arg(long, value_enum)]
    pub(super) mode: Option<FlowMode>,
    /// Id of the first screen (navigate only). Meta defaults to FIRST_ENTRY_SCREEN.
    #[arg(long, value_name = "ID")]
    pub(super) screen: Option<String>,
    /// Input for the first screen, as a non-empty JSON object (navigate only).
    #[arg(long, value_name = "JSON", conflicts_with = "data_file")]
    pub(super) data: Option<String>,
    /// Read the first screen's input from a JSON file: a non-empty object (navigate only).
    #[arg(long, value_name = "PATH")]
    pub(super) data_file: Option<PathBuf>,
    /// Image for the header: a media id or an http(s) link.
    #[arg(
        long,
        value_name = "ID|URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    pub(super) header_image: Option<String>,
    /// Video for the header: a media id or an http(s) link.
    #[arg(
        long,
        value_name = "ID|URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    pub(super) header_video: Option<String>,
    /// Document for the header: a media id or an http(s) link.
    #[arg(
        long,
        value_name = "ID|URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    pub(super) header_document: Option<String>,
}

pub(super) async fn send_flow(app: &App, a: &FlowMessage) -> Result<()> {
    let c = &a.common;
    validate::recipient(&c.to)?;
    check_common(c)?;
    let reply_to = c.reply.id()?;
    let flow = match (&a.flow_id, &a.flow_name) {
        (Some(id), _) => {
            validate::not_blank(id, "--flow-id")?;
            messages::FlowRef::Id(id)
        }
        (None, Some(name)) => {
            validate::not_blank(name, "--flow-name")?;
            messages::FlowRef::Name(name)
        }
        (None, None) => unreachable!("clap enforces one flow reference"),
    };
    validate::not_blank(&a.cta, "--cta")?;
    for (value, flag) in [(&a.flow_token, "--flow-token"), (&a.screen, "--screen")] {
        if let Some(value) = value {
            validate::not_blank(value, flag)?;
        }
    }
    let data = flow_data(a)?;
    if a.action == Some(FlowAction::DataExchange) && (a.screen.is_some() || data.is_some()) {
        return Err(Error::validation(
            "--screen and --data only apply when --action is navigate",
        ));
    }
    let mut text = c.text();
    if let Some(header) = interactive_media_header(
        a.header_image.as_deref(),
        a.header_video.as_deref(),
        a.header_document.as_deref(),
    )? {
        text.header = Some(header);
    }
    if a.cta.chars().count() > limits::ADVISED_FLOW_CTA {
        app.out.warn(format!(
            "--cta is over the {} characters Meta advises",
            limits::ADVISED_FLOW_CTA
        ));
    }
    c.policy.enforce(&c.to)?;
    let params = messages::FlowParams {
        flow,
        cta: &a.cta,
        token: a.flow_token.as_deref(),
        action: a.action.map(FlowAction::as_str),
        mode: a.mode.map(FlowMode::as_str),
        screen: a.screen.as_deref(),
        data: data.as_ref(),
    };
    dispatch(
        app,
        messages::interactive_flow(&c.to, &text, &params, reply_to),
    )
    .await
}

/// The first screen's input. Meta requires a non-empty object when there is one. Errors
/// never echo the input, which is often personal data.
pub(super) fn flow_data(a: &FlowMessage) -> Result<Option<Value>> {
    let (data, what) = match (&a.data, &a.data_file) {
        (Some(text), _) => (
            serde_json::from_str(text)
                .map_err(|e| Error::validation(format!("--data is not valid JSON: {e}")))?,
            "--data",
        ),
        (None, Some(path)) => (read_json(path)?, "--data-file"),
        (None, None) => return Ok(None),
    };
    match data {
        Value::Object(fields) if !fields.is_empty() => Ok(Some(Value::Object(fields))),
        _ => Err(Error::validation(format!(
            "{what} must hold a non-empty JSON object"
        ))),
    }
}
