//! `paloma send`. Every message type shares the recipient check, `--reply-to` and the 24-hour
//! window guard, so each family module only shapes its own body.

use super::util::{read_file, read_json, read_stdin, split_rows};
use crate::app::App;
use clap::{ArgGroup, Args, Subcommand, ValueEnum};
use paloma_core::compliance::{NoTracking, SendPolicy, enforce_session_window};
use paloma_core::limits::{self, MAX_LIST_ROWS, MAX_REPLY_BUTTONS};
use paloma_core::{Error, Result, validate};
use paloma_graph::api::media as media_api;
use paloma_graph::api::messages::{
    self, Button, InteractiveHeader, InteractiveText, MediaKind, MediaOptions, MediaSource, Row,
    Section,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
mod commerce;
mod contact;
mod flow;
mod interactive;
mod media;
mod template;
mod template_routing;
use commerce::{Catalog, ProductList, ProductMessage};
use contact::Contact;
use flow::{FlowMessage, send_flow};
use interactive::{Interactive, InteractiveCommon, check_common, interactive_media_header};
use media::{Audio, Media, send_media};
use template::{Template, TemplateVia};

#[derive(Debug, Subcommand)]
pub enum SendCommand {
    /// Send a text message.
    Text(Text),
    /// Send an approved template.
    Template(Template),
    /// Send an image.
    #[command(mut_arg("filename", |a| a.hide(true)))]
    Image(Media),
    /// Send a document.
    Document(Media),
    /// Send an audio clip.
    #[command(mut_arg("filename", |a| a.hide(true)), mut_arg("caption", |a| a.hide(true)))]
    Audio(Audio),
    /// Send a video.
    #[command(mut_arg("filename", |a| a.hide(true)))]
    Video(Media),
    /// Send a sticker.
    #[command(mut_arg("filename", |a| a.hide(true)), mut_arg("caption", |a| a.hide(true)))]
    Sticker(Media),
    /// Send a location pin.
    Location(Location),
    /// Send a contact card from flags, or contact cards from a JSON file.
    Contact(Contact),
    /// Send an interactive message (buttons, list, call-to-action URL, carousel and more).
    #[command(subcommand)]
    Interactive(Interactive),
    /// React to a message with an emoji.
    Reaction(Reaction),
    /// Send an arbitrary message object as-is.
    Raw(Raw),
    /// Send a WhatsApp Flow, opened from a button on the message.
    Flow(FlowMessage),
    /// Ask the user for permission to call them. Needs calling enabled on the number.
    CallPermission(CallPermission),
    /// Send one product from your catalog.
    Product(ProductMessage),
    /// Send up to 30 products from your catalog, in sections.
    ProductList(ProductList),
    /// Send a button that opens your catalog.
    Catalog(Catalog),
}

/// The flag on every send type that Meta lets quote an earlier message.
#[derive(Debug, Args)]
pub struct Reply {
    /// Quote-reply to this message id (wamid…).
    #[arg(long)]
    reply_to: Option<String>,
}

impl Reply {
    /// A blank id is a mistake, such as an unset shell variable, and would otherwise send
    /// a message that doesn't quote anything.
    fn id(&self) -> Result<Option<&str>> {
        if let Some(id) = &self.reply_to {
            validate::not_blank(id, "--reply-to")?;
        }
        Ok(self.reply_to.as_deref())
    }
}

/// The 24-hour window guardrail flags.
#[derive(Debug, Clone, Copy, Args)]
pub struct Policy {
    /// Send even if the 24h customer-service window may be closed.
    #[arg(long)]
    force: bool,
    /// Refuse free-form sends unless the window is known to be open.
    #[arg(long, conflicts_with = "force")]
    strict: bool,
}

impl Policy {
    fn enforce(self, to: &str) -> Result<()> {
        enforce_session_window(
            &NoTracking,
            to,
            false,
            SendPolicy {
                force: self.force,
                strict: self.strict,
            },
        )
    }
}

#[derive(Debug, Args)]
pub struct Text {
    /// Recipient (E.164 number or business-scoped user ID).
    to: String,
    /// Treat the recipient as a group ID (see `paloma group`). Meta takes group sends for
    /// text, media and templates only.
    #[arg(long)]
    group: bool,
    /// Message body.
    #[arg(required_unless_present = "body_file", conflicts_with = "body_file")]
    body: Option<String>,
    /// Read the body from a file.
    #[arg(long)]
    body_file: Option<PathBuf>,
    /// Render a link preview for the first URL in the body.
    #[arg(long)]
    preview_url: bool,
    #[command(flatten)]
    reply: Reply,
    #[command(flatten)]
    policy: Policy,
}

#[derive(Debug, Args)]
pub struct Location {
    /// Recipient (E.164 number or business-scoped user ID).
    to: String,
    /// Latitude in decimal degrees.
    #[arg(long, allow_hyphen_values = true)]
    lat: f64,
    /// Longitude in decimal degrees.
    #[arg(long, allow_hyphen_values = true)]
    long: f64,
    /// Place name.
    #[arg(long)]
    name: Option<String>,
    /// Street address.
    #[arg(long)]
    address: Option<String>,
    #[command(flatten)]
    reply: Reply,
    #[command(flatten)]
    policy: Policy,
}

#[derive(Debug, Args)]
pub struct Reaction {
    /// Recipient (E.164 number or business-scoped user ID).
    to: String,
    /// The message to react to (wamid…).
    #[arg(long)]
    message_id: String,
    /// The emoji; an empty string removes the reaction.
    #[arg(long)]
    emoji: String,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("input").required(true).args(["file", "stdin"])))]
pub struct Raw {
    /// JSON file with a complete message object.
    #[arg(short = 'f', long)]
    file: Option<PathBuf>,
    /// Read the message object from stdin.
    #[arg(long)]
    stdin: bool,
}

#[derive(Debug, Args)]
pub struct CallPermission {
    /// Recipient (E.164 number or business-scoped user ID).
    to: String,
    /// Why you want to call. Optional, but Meta recommends giving the user context.
    #[arg(long)]
    body: Option<String>,
    #[command(flatten)]
    reply: Reply,
    #[command(flatten)]
    policy: Policy,
}

impl SendCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::Text(a) => {
                check_to(&a.to, a.group)?;
                let body = match &a.body_file {
                    Some(path) => read_file(path)?,
                    None => a.body.clone().unwrap_or_default(),
                };
                validate::text_body(&body)?;
                let reply_to = a.reply.id()?;
                if !a.group {
                    a.policy.enforce(&a.to)?;
                }
                let payload = messages::text(&a.to, &body, a.preview_url, reply_to);
                dispatch(app, addressed(payload, a.group)).await
            }
            Self::Template(a) => template::send(app, a).await,
            Self::Image(a) => send_media(app, MediaKind::Image, a, false).await,
            Self::Document(a) => send_media(app, MediaKind::Document, a, false).await,
            Self::Audio(a) => send_media(app, MediaKind::Audio, a.media, a.voice).await,
            Self::Video(a) => send_media(app, MediaKind::Video, a, false).await,
            Self::Sticker(a) => send_media(app, MediaKind::Sticker, a, false).await,
            Self::Location(a) => {
                validate::recipient(&a.to)?;
                if !(-90.0..=90.0).contains(&a.lat) || !(-180.0..=180.0).contains(&a.long) {
                    return Err(Error::validation(
                        "--lat must be within ±90 and --long within ±180",
                    ));
                }
                let reply_to = a.reply.id()?;
                a.policy.enforce(&a.to)?;
                let payload = messages::location(
                    &a.to,
                    a.lat,
                    a.long,
                    a.name.as_deref(),
                    a.address.as_deref(),
                    reply_to,
                );
                dispatch(app, payload).await
            }
            Self::Contact(a) => contact::send(app, a).await,
            Self::Interactive(i) => i.run(app).await,
            Self::Reaction(a) => {
                validate::recipient(&a.to)?;
                validate::not_blank(&a.message_id, "--message-id")?;
                dispatch(app, messages::reaction(&a.to, &a.message_id, &a.emoji)).await
            }
            Self::Raw(a) => {
                let text = match &a.file {
                    Some(path) => read_file(path)?,
                    None => read_stdin("the message")?,
                };
                let payload: Value = serde_json::from_str(&text)
                    .map_err(|e| Error::validation(format!("message is not valid JSON: {e}")))?;
                dispatch(app, payload).await
            }
            Self::Flow(a) => send_flow(app, &a).await,
            Self::CallPermission(a) => {
                validate::recipient(&a.to)?;
                if let Some(body) = &a.body {
                    check_body(body)?;
                }
                let reply_to = a.reply.id()?;
                a.policy.enforce(&a.to)?;
                let payload = messages::call_permission_request(&a.to, a.body.as_deref(), reply_to);
                dispatch(app, payload).await
            }
            Self::Product(a) => commerce::send_product(app, a).await,
            Self::ProductList(a) => commerce::send_product_list(app, a).await,
            Self::Catalog(a) => commerce::send_catalog(app, a).await,
        }
    }
}

fn check_body(body: &str) -> Result<()> {
    validate::not_blank(body, "--body")?;
    validate::max_chars(body, limits::MAX_INTERACTIVE_BODY, "--body")
}

fn check_footer(footer: Option<&str>) -> Result<()> {
    footer.map_or(Ok(()), |footer| {
        validate::max_chars(footer, limits::MAX_INTERACTIVE_FOOTER, "--footer")
    })
}

/// A group id takes the place of a recipient. The 24-hour window is per customer, so sends to
/// a group skip that check.
fn check_to(to: &str, group: bool) -> Result<()> {
    if group {
        validate::group_id(to)
    } else {
        validate::recipient(to)
    }
}

fn addressed(payload: Value, group: bool) -> Value {
    if group {
        messages::for_group(payload)
    } else {
        payload
    }
}

/// POST the message and print the response with its wamid as the primary id.
async fn dispatch(app: &App, payload: Value) -> Result<()> {
    let ctx = app.ctx()?;
    let phone = ctx.require_phone_number_id()?.to_owned();
    let payload = messages::with_messaging_account(payload, ctx.messaging_account_id());
    if let Some(out) = app.send_output(&messages::send(&phone, payload)).await? {
        let id = messages::message_id(&out.data);
        app.out.emit(&out.id(id));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::commerce::parse_product_sections;
    use super::interactive::{parse_buttons, parse_sections};
    use super::*;

    #[test]
    fn buttons_parse_id_and_title() {
        let b = parse_buttons(&["yes:Yes please".into(), "No".into()]).unwrap();
        assert_eq!(
            b[0],
            Button {
                id: "yes".into(),
                title: "Yes please".into()
            }
        );
        assert_eq!(
            b[1],
            Button {
                id: "No".into(),
                title: "No".into()
            }
        );
        assert!(parse_buttons(&vec!["a".into(); 4]).is_err());
    }

    #[test]
    fn sections_parse_and_cap_rows() {
        let s = parse_sections(&["Fruit:a:Apple,b:Banana".into()]).unwrap();
        assert_eq!(s[0].title, "Fruit");
        assert_eq!(s[0].rows[1].title, "Banana");
        assert!(parse_sections(&["Empty:".into()]).is_err());
        let many: String = (0..11)
            .map(|i| format!("{i}:R{i}"))
            .collect::<Vec<_>>()
            .join(",");
        assert!(parse_sections(&[format!("Big:{many}")]).is_err());
    }

    #[test]
    fn a_row_keeps_its_colons_and_takes_an_optional_description() {
        let s = parse_sections(&["Times:a:Today: 10:30,b:Soon::Next day: by 5".into()]).unwrap();
        assert_eq!(s[0].rows[0].title, "Today: 10:30");
        assert_eq!(s[0].rows[0].description, None);
        assert_eq!(s[0].rows[1].title, "Soon");
        assert_eq!(s[0].rows[1].description.as_deref(), Some("Next day: by 5"));
    }

    #[test]
    fn product_sections_parse_and_cap_their_totals() {
        let s = parse_product_sections(&["Top:a,b".into(), r"Sale:c\,d:1".into()]).unwrap();
        assert_eq!(s[0].products, ["a", "b"]);
        assert_eq!(s[1].products, ["c,d:1"]);
        assert!(parse_product_sections(&["Empty:".into()]).is_err());
        assert!(parse_product_sections(&["NoProducts".into()]).is_err());
        let thirty = ["s"; 30].join(",");
        assert!(parse_product_sections(&[format!("A:{thirty}")]).is_ok());
        assert!(parse_product_sections(&[format!("A:{thirty},s")]).is_err());
        let eleven = vec!["A:s".to_owned(); 11];
        assert!(parse_product_sections(&eleven).is_err());
    }

    #[test]
    fn an_escaped_comma_stays_inside_the_row() {
        let s = parse_sections(&[r"S:a:A::One\, two,b:B".into()]).unwrap();
        assert_eq!(s[0].rows.len(), 2);
        assert_eq!(s[0].rows[0].description.as_deref(), Some("One, two"));
    }
}
