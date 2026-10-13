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
/// File types the Resumable Upload API accepts.
pub const RESUMABLE_UPLOAD_TYPES: [&str; 5] = [
    "application/pdf",
    "image/jpeg",
    "image/jpg",
    "image/png",
    "video/mp4",
];
/// The image types among those, which is what a profile photo can be.
pub const PROFILE_PHOTO_TYPES: [&str; 3] = ["image/jpeg", "image/jpg", "image/png"];
/// Rows allowed across all sections of an interactive list message.
pub const MAX_LIST_ROWS: usize = 10;
/// Digits in a two-step verification PIN.
pub const PIN_DIGITS: usize = 6;
/// Most phone numbers `GET /{waba-id}/phone_numbers` returns per page.
pub const MAX_PHONE_NUMBERS_PAGE: u32 = 100;
/// Users a business phone number's blocklist can hold.
pub const MAX_BLOCKLIST_USERS: usize = 64_000;
/// Countries `data_localization_region` accepts when registering a number.
pub const DATA_LOCALIZATION_REGIONS: [&str; 14] = [
    "AE", "AU", "BH", "BR", "CA", "CH", "DE", "GB", "ID", "IN", "JP", "KR", "SG", "ZA",
];
/// Default SIP port when `--server` omits one (SIP over TLS).
pub const DEFAULT_SIP_PORT: u16 = 5061;
