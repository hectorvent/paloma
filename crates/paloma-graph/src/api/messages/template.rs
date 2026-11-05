//! Template sends, their typed components, and which endpoint (`/messages` or
//! `/marketing_messages`) a template's category goes to.

use super::{MediaKind, MediaSource, Request, Value, envelope, json, media_object};

/// Marketing templates use a separate edge; utility and authentication stay on Cloud API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateEndpoint {
    Cloud,
    Marketing,
}

impl TemplateEndpoint {
    pub const fn edge(self) -> &'static str {
        match self {
            Self::Cloud => "messages",
            Self::Marketing => "marketing_messages",
        }
    }

    pub const fn for_category(category: paloma_core::TemplateCategory) -> Self {
        match category {
            paloma_core::TemplateCategory::Marketing => Self::Marketing,
            paloma_core::TemplateCategory::Utility
            | paloma_core::TemplateCategory::Authentication => Self::Cloud,
        }
    }
}

pub fn send_template(phone: &str, payload: Value, endpoint: TemplateEndpoint) -> Request {
    Request::post(format!("{phone}/{}", endpoint.edge())).json(payload)
}

/// Options only `/marketing_messages` takes, at the top level of the message.
pub fn with_marketing_options(
    mut payload: Value,
    product_policy: Option<&str>,
    activity_sharing: Option<bool>,
) -> Value {
    if let Some(policy) = product_policy {
        payload["product_policy"] = policy.into();
    }
    if let Some(sharing) = activity_sharing {
        payload["message_activity_sharing"] = sharing.into();
    }
    payload
}

/// An offline plan cannot know the category. Never execute this placeholder request.
pub fn template_routing_preview(phone: &str, payload: Value) -> Request {
    Request::post(format!(
        "{phone}/<messages-or-marketing_messages-from-category>"
    ))
    .json(payload)
}

/// A template send; `body_vars` fill `{{1}}`, `{{2}}`, … of the BODY component in order.
pub fn template(to: &str, name: &str, language: &str, body_vars: &[String]) -> Value {
    let params = TemplateParams {
        body_vars,
        ..TemplateParams::default()
    };
    template_with_components(to, name, language, template_components(&params), None)
}

/// A template's header parameter at send time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateHeader {
    Text(String),
    Media(MediaKind, MediaSource),
}

/// What a template button takes at send time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateButtonKind {
    /// The variable part of a URL, including a one-time password.
    Url,
    QuickReply,
    CopyCode,
    Flow,
    Catalog,
}

/// One template button's send-time parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateButton {
    pub index: u8,
    pub kind: TemplateButtonKind,
    pub value: String,
}

impl TemplateButton {
    /// Meta's examples write `index` as a string for some kinds and an integer for others,
    /// so each kind follows its own documented shape.
    pub(super) fn to_component(&self) -> Value {
        let (sub_type, index, parameter) = match self.kind {
            TemplateButtonKind::Url => (
                "url",
                self.index.to_string().into(),
                json!({ "type": "text", "text": self.value }),
            ),
            TemplateButtonKind::QuickReply => (
                "quick_reply",
                self.index.to_string().into(),
                json!({ "type": "payload", "payload": self.value }),
            ),
            TemplateButtonKind::CopyCode => (
                "copy_code",
                Value::from(self.index),
                json!({ "type": "coupon_code", "coupon_code": self.value }),
            ),
            TemplateButtonKind::Flow => (
                "flow",
                self.index.to_string().into(),
                json!({ "type": "action", "action": { "flow_token": self.value } }),
            ),
            TemplateButtonKind::Catalog => (
                "CATALOG",
                Value::from(self.index),
                json!({
                    "type": "action",
                    "action": { "thumbnail_product_retailer_id": self.value }
                }),
            ),
        };
        json!({ "type": "button", "sub_type": sub_type, "index": index, "parameters": [parameter] })
    }
}

/// The send-time parameters with typed flags. Anything else goes through
/// [`template_with_components`] as full component objects.
#[derive(Debug, Default)]
pub struct TemplateParams<'a> {
    pub body_vars: &'a [String],
    pub named_vars: &'a [(String, String)],
    pub header: Option<TemplateHeader>,
    pub buttons: &'a [TemplateButton],
}

pub fn template_components(params: &TemplateParams<'_>) -> Vec<Value> {
    let mut components = Vec::new();
    match &params.header {
        Some(TemplateHeader::Text(text)) => components
            .push(json!({ "type": "header", "parameters": [{ "type": "text", "text": text }] })),
        Some(TemplateHeader::Media(kind, source)) => {
            let media = media_object(source);
            let kind = kind.as_str();
            components
                .push(json!({ "type": "header", "parameters": [{ "type": kind, kind: media }] }));
        }
        None => {}
    }
    let parameters: Vec<Value> =
        params
            .body_vars
            .iter()
            .map(|text| json!({ "type": "text", "text": text }))
            .chain(params.named_vars.iter().map(
                |(name, text)| json!({ "type": "text", "parameter_name": name, "text": text }),
            ))
            .collect();
    if !parameters.is_empty() {
        components.push(json!({ "type": "body", "parameters": parameters }));
    }
    components.extend(params.buttons.iter().map(TemplateButton::to_component));
    components
}

/// Preserve caller-supplied send components, omitting the field when none are needed.
pub fn template_with_components(
    to: &str,
    name: &str,
    language: &str,
    components: Vec<Value>,
    reply_to: Option<&str>,
) -> Value {
    let mut content = json!({ "name": name, "language": { "code": language } });
    if !components.is_empty() {
        content["components"] = components.into();
    }
    envelope(to, "template", reply_to, content)
}

#[cfg(test)]
mod template_component_tests {
    use super::*;

    #[test]
    fn header_only_template_omits_body_and_empty_components_are_omitted() {
        let components = template_components(&TemplateParams {
            header: Some(TemplateHeader::Text("Order update".into())),
            ..TemplateParams::default()
        });
        let payload = template_with_components("15551234567", "order", "en_US", components, None);
        assert_eq!(
            payload["template"]["components"],
            json!([
                {"type":"header", "parameters":[{"type":"text", "text":"Order update"}]}
            ])
        );
        let payload = template_with_components("15551234567", "hello", "en_US", vec![], None);
        assert!(payload["template"].get("components").is_none());
    }
}
