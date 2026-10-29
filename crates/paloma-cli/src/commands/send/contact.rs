//! Contact cards, from typed flags for one card or a JSON file for several.

#[allow(clippy::wildcard_imports)] // part of `send`: shares its imports and helpers
use super::*;

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("contact_source").required(true).args(["file", "name"])))]
pub struct Contact {
    /// Recipient (E.164 number or business-scoped user ID).
    pub(super) to: String,
    /// JSON file holding the Cloud API `contacts` array (or `{"contacts": [...]}`).
    #[arg(long, conflicts_with_all = ["first_name", "last_name", "phones", "wa_id", "emails"])]
    pub(super) file: Option<PathBuf>,
    /// The contact's full name, shown on the message.
    #[arg(long)]
    pub(super) name: Option<String>,
    /// The contact's first name.
    #[arg(long, requires = "name")]
    pub(super) first_name: Option<String>,
    /// The contact's last name.
    #[arg(long, requires = "name")]
    pub(super) last_name: Option<String>,
    /// A phone number for the contact; repeatable.
    #[arg(long = "phone", value_name = "NUMBER", requires = "name")]
    pub(super) phones: Vec<String>,
    /// WhatsApp id of the first --phone. It adds the Message and Save contact buttons;
    /// without it the message shows an Invite to WhatsApp button.
    #[arg(long, requires = "phones")]
    pub(super) wa_id: Option<String>,
    /// An email address for the contact; repeatable.
    #[arg(long = "email", value_name = "ADDRESS", requires = "name")]
    pub(super) emails: Vec<String>,
    #[command(flatten)]
    pub(super) reply: Reply,
    #[command(flatten)]
    pub(super) policy: Policy,
}

pub(super) async fn send(app: &App, a: Contact) -> Result<()> {
    validate::recipient(&a.to)?;
    let contacts = match &a.file {
        Some(path) => contacts_file(path)?,
        None => typed_contact(&a)?,
    };
    let reply_to = a.reply.id()?;
    a.policy.enforce(&a.to)?;
    dispatch(app, messages::contacts(&a.to, contacts, reply_to)).await
}

/// The contacts array from a `--file`, passed through as authored.
pub(super) fn contacts_file(path: &Path) -> Result<Value> {
    let doc = read_json(path)?;
    let contacts = match doc {
        Value::Array(_) => doc,
        Value::Object(ref m) if m.get("contacts").is_some_and(Value::is_array) => {
            m["contacts"].clone()
        }
        _ => {
            return Err(Error::validation(
                "contacts file must hold a JSON array or {\"contacts\": [...]}",
            ));
        }
    };
    let count = contacts.as_array().map_or(0, Vec::len);
    if count == 0 {
        return Err(Error::validation("contacts file has no contacts"));
    }
    if count > limits::MAX_CONTACTS {
        return Err(Error::validation(format!(
            "contacts file has {count} contacts; max is {}",
            limits::MAX_CONTACTS
        )));
    }
    Ok(contacts)
}

/// The contacts array for the typed flags. clap has already required `--name` here.
pub(super) fn typed_contact(a: &Contact) -> Result<Value> {
    let formatted_name = a.name.as_deref().unwrap_or_default();
    validate::not_blank(formatted_name, "--name")?;
    // Meta's docs call the name's other fields optional, but the API answers 131009 without one.
    if a.first_name.is_none() && a.last_name.is_none() {
        return Err(Error::validation(
            "--name also needs --first-name or --last-name, or Meta rejects the contact",
        ));
    }
    for (value, flag) in [
        (&a.first_name, "--first-name"),
        (&a.last_name, "--last-name"),
        (&a.wa_id, "--wa-id"),
    ] {
        if let Some(value) = value {
            validate::not_blank(value, flag)?;
        }
    }
    for phone in &a.phones {
        validate::not_blank(phone, "--phone")?;
    }
    for email in &a.emails {
        validate::not_blank(email, "--email")?;
    }
    Ok(messages::contact_card(&messages::ContactCard {
        formatted_name,
        first_name: a.first_name.as_deref(),
        last_name: a.last_name.as_deref(),
        phones: &a.phones,
        wa_id: a.wa_id.as_deref(),
        emails: &a.emails,
    }))
}
