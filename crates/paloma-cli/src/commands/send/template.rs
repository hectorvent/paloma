//! Template sends: typed parameters or a components file, and the route (`/messages` or
//! `/marketing_messages`) each template category needs.

use super::{
    App, Args, Error, MediaKind, PathBuf, Reply, Result, Value, ValueEnum, addressed, check_to,
    limits, messages, read_json, template_routing, validate,
};

#[derive(Debug, Args)]
pub struct Template {
    /// Recipient (E.164 number or business-scoped user ID).
    pub(super) to: String,
    /// Template name.
    pub(super) name: String,
    /// Template language code, e.g. en_US.
    #[arg(long)]
    pub(super) language: String,
    /// Treat the recipient as a group ID (see `paloma group`). Meta takes group sends for
    /// text, media and templates only.
    #[arg(long)]
    pub(super) group: bool,
    /// Select the endpoint: auto looks up the latest category; explicit routes skip lookup.
    #[arg(long, value_enum, default_value = "auto")]
    pub(super) via: TemplateVia,
    /// With --via marketing: whether Meta may fall back to Cloud API if onboarding is incomplete.
    #[arg(long, value_enum)]
    pub(super) product_policy: Option<ProductPolicy>,
    /// With --via marketing: share this message's activity with Meta, overriding the account.
    #[arg(long, overrides_with = "no_message_activity_sharing")]
    pub(super) message_activity_sharing: bool,
    /// With --via marketing: don't share this message's activity with Meta.
    #[arg(long, overrides_with = "message_activity_sharing")]
    pub(super) no_message_activity_sharing: bool,
    /// Body variable values, in order ({{1}}, {{2}}, …); repeatable.
    #[arg(long = "body-var", value_name = "VALUE", allow_hyphen_values = true)]
    pub(super) body_vars: Vec<String>,
    /// Named body text parameter; repeat for each name.
    #[arg(long = "var", value_name = "NAME=VALUE", conflicts_with = "body_vars")]
    pub(super) named_vars: Vec<String>,
    /// Text value for a positional header parameter.
    #[arg(
        long,
        value_name = "TEXT",
        conflicts_with_all = ["named_vars", "header_media"],
        allow_hyphen_values = true
    )]
    pub(super) header_text: Option<String>,
    /// Image for the header: a media id or an http(s) link.
    #[arg(long, value_name = "ID|URL", group = "header_media")]
    pub(super) header_image: Option<String>,
    /// Video for the header: a media id or an http(s) link.
    #[arg(long, value_name = "ID|URL", group = "header_media")]
    pub(super) header_video: Option<String>,
    /// Document for the header: a media id or an http(s) link.
    #[arg(long, value_name = "ID|URL", group = "header_media")]
    pub(super) header_document: Option<String>,
    /// A button's parameter; repeatable. TYPE is url, quick_reply, copy_code, flow or catalog.
    #[arg(long = "button", value_name = "INDEX:TYPE=VALUE")]
    pub(super) buttons: Vec<String>,
    /// JSON send components: an array or {"components": [...]}.
    #[arg(
        long,
        value_name = "PATH",
        conflicts_with_all = ["body_vars", "named_vars", "header_text", "header_media", "buttons"]
    )]
    pub(super) components_file: Option<PathBuf>,
    #[command(flatten)]
    pub(super) reply: Reply,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum ProductPolicy {
    /// Send through Cloud API when marketing onboarding is incomplete.
    CloudApiFallback,
    /// Never fall back.
    Strict,
}

impl ProductPolicy {
    pub(super) const fn api(self) -> &'static str {
        match self {
            Self::CloudApiFallback => "CLOUD_API_FALLBACK",
            Self::Strict => "STRICT",
        }
    }
}

pub(super) async fn send(app: &App, a: Template) -> Result<()> {
    let sharing =
        crate::commands::util::toggle(a.message_activity_sharing, a.no_message_activity_sharing);
    check_to(&a.to, a.group)?;
    // Auto might route to /messages, which doesn't take these, so ask for the route.
    if (a.product_policy.is_some() || sharing.is_some()) && !matches!(a.via, TemplateVia::Marketing)
    {
        return Err(Error::validation(
            "--product-policy and --message-activity-sharing need --via marketing",
        ));
    }
    // Meta documents group sends only on /messages.
    let via = match (a.group, a.via) {
        (false, via) => via,
        (true, TemplateVia::Marketing) => {
            return Err(Error::validation(
                "--group sends through /messages, so it can't use --via marketing",
            ));
        }
        (true, _) => TemplateVia::Cloud,
    };
    validate::not_blank(&a.name, "template name")?;
    validate::not_blank(&a.language, "--language")?;
    let reply_to = a.reply.id()?;
    let components = if let Some(path) = &a.components_file {
        template_components_file(path)?
    } else {
        let mut named = Vec::new();
        for entry in &a.named_vars {
            let (name, value) = entry
                .split_once('=')
                .ok_or_else(|| Error::validation("--var must use NAME=VALUE"))?;
            validate::template_parameter_name(name)?;
            validate::not_blank(value, "--var value")?;
            if named.iter().any(|(existing, _)| existing == name) {
                return Err(Error::validation("--var parameter names must be unique"));
            }
            named.push((name.to_owned(), value.to_owned()));
        }
        for value in &a.body_vars {
            validate::not_blank(value, "--body-var")?;
        }
        let header = template_header(&a)?;
        let buttons = parse_template_buttons(&a.buttons)?;
        messages::template_components(&messages::TemplateParams {
            body_vars: &a.body_vars,
            named_vars: &named,
            header,
            buttons: &buttons,
        })
    };
    if reply_to.is_some() {
        app.out
            .warn("WhatsApp doesn't show the quoted message on a template");
    }
    template_routing::send(
        app,
        messages::with_marketing_options(
            addressed(
                messages::template_with_components(
                    &a.to,
                    &a.name,
                    &a.language,
                    components,
                    reply_to,
                ),
                a.group,
            ),
            a.product_policy.map(ProductPolicy::api),
            sharing,
        ),
        &a.name,
        &a.language,
        via,
    )
    .await
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum TemplateVia {
    Auto,
    Cloud,
    Marketing,
}

impl TemplateVia {
    pub(super) const fn endpoint(self) -> Option<messages::TemplateEndpoint> {
        match self {
            Self::Auto => None,
            Self::Cloud => Some(messages::TemplateEndpoint::Cloud),
            Self::Marketing => Some(messages::TemplateEndpoint::Marketing),
        }
    }
}

pub(super) fn template_header(a: &Template) -> Result<Option<messages::TemplateHeader>> {
    if let Some(text) = &a.header_text {
        validate::not_blank(text, "--header-text")?;
        return Ok(Some(messages::TemplateHeader::Text(text.clone())));
    }
    let media = [
        (&a.header_image, MediaKind::Image, "--header-image"),
        (&a.header_video, MediaKind::Video, "--header-video"),
        (&a.header_document, MediaKind::Document, "--header-document"),
    ];
    for (value, kind, flag) in media {
        if let Some(value) = value {
            let source = messages::media_reference(value, flag)?;
            return Ok(Some(messages::TemplateHeader::Media(kind, source)));
        }
    }
    Ok(None)
}

/// `INDEX:TYPE=VALUE`, split at the first `:` and `=`, so a value may contain both. Errors
/// name the flag by position and never echo the text, which a typo can fill with a token.
pub(super) fn parse_template_buttons(specs: &[String]) -> Result<Vec<messages::TemplateButton>> {
    use messages::{TemplateButton, TemplateButtonKind as Kind};

    let mut buttons: Vec<TemplateButton> = Vec::new();
    for (position, spec) in (1..).zip(specs) {
        let invalid =
            |problem: String| Error::validation(format!("--button #{position} {problem}"));
        let parsed = spec.split_once(':').and_then(|(index, rest)| {
            rest.split_once('=')
                .map(|(kind, value)| (index, kind, value))
        });
        let Some((index, kind, value)) = parsed else {
            return Err(invalid("must use INDEX:TYPE=VALUE".into()));
        };
        let index = index
            .parse::<u8>()
            .ok()
            .filter(|index| *index < limits::MAX_TEMPLATE_BUTTONS)
            .ok_or_else(|| {
                invalid(format!(
                    "index must be 0 to {}",
                    limits::MAX_TEMPLATE_BUTTONS - 1
                ))
            })?;
        let kind = match kind {
            "url" => Kind::Url,
            "quick_reply" => Kind::QuickReply,
            "copy_code" => Kind::CopyCode,
            "flow" => Kind::Flow,
            "catalog" => Kind::Catalog,
            _ => {
                return Err(invalid(
                    "type must be url, quick_reply, copy_code, flow or catalog".into(),
                ));
            }
        };
        if value.trim().is_empty() {
            return Err(invalid("value is empty".into()));
        }
        if kind == Kind::CopyCode {
            if value.chars().count() > limits::MAX_COUPON_CODE {
                return Err(invalid(format!(
                    "coupon code is over {} characters",
                    limits::MAX_COUPON_CODE
                )));
            }
            if buttons.iter().any(|button| button.kind == Kind::CopyCode) {
                return Err(invalid(
                    "is a second copy_code; a template can have one".into(),
                ));
            }
        }
        if buttons.iter().any(|button| button.index == index) {
            return Err(invalid(format!("index {index} is already used")));
        }
        buttons.push(TemplateButton {
            index,
            kind,
            value: value.to_owned(),
        });
    }
    Ok(buttons)
}

/// Accept the component envelope, not a full message that could override the target.
pub(super) fn template_components_file(path: &std::path::Path) -> Result<Vec<Value>> {
    let document = read_json(path)?;
    let components = match document {
        Value::Array(items) => items,
        Value::Object(mut fields) if fields.len() == 1 => match fields.remove("components") {
            Some(Value::Array(items)) => items,
            _ => {
                return Err(Error::validation(
                    "components file must hold a JSON array or {\"components\": [...]}",
                ));
            }
        },
        _ => {
            return Err(Error::validation(
                "components file must hold a JSON array or {\"components\": [...]}",
            ));
        }
    };
    for (index, component) in components.iter().enumerate() {
        if !component.is_object()
            || component
                .get("type")
                .and_then(Value::as_str)
                .is_none_or(|s| s.trim().is_empty())
        {
            return Err(Error::validation(format!(
                "components[{index}] needs a non-empty 'type' string"
            )));
        }
        if let Some(parameters) = component.get("parameters") {
            let Some(parameters) = parameters.as_array() else {
                return Err(Error::validation(format!(
                    "components[{index}].parameters must be an array"
                )));
            };
            if parameters.iter().any(|parameter| {
                !parameter.is_object()
                    || parameter
                        .get("type")
                        .and_then(Value::as_str)
                        .is_none_or(|s| s.trim().is_empty())
            }) {
                return Err(Error::validation(format!(
                    "components[{index}].parameters entries need a non-empty 'type' string"
                )));
            }
        }
    }
    Ok(components)
}
