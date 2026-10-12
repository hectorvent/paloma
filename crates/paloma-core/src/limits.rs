//! Cloud API limits as named constants: the single source of truth for "what would Meta
//! reject", so pre-flight checks and help text never drift apart.

/// Maximum characters in a text message body.
pub const MAX_TEXT_BODY: usize = 4096;
/// Maximum characters in a media caption.
pub const MAX_CAPTION: usize = 1024;
/// Reply buttons allowed on an interactive button message.
pub const MAX_REPLY_BUTTONS: usize = 3;
/// Most buttons a template can have, so the highest button index is one less.
pub const MAX_TEMPLATE_BUTTONS: u8 = 10;
pub const MAX_COUPON_CODE: usize = 20;
/// Rows allowed across all sections of an interactive list message.
pub const MAX_LIST_ROWS: usize = 10;
/// Digits in a two-step verification PIN.
pub const PIN_DIGITS: usize = 6;
/// Default SIP port when `--server` omits one (SIP over TLS).
pub const DEFAULT_SIP_PORT: u16 = 5061;
