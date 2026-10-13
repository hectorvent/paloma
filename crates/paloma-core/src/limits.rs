//! Cloud API limits as named constants: the single source of truth for "what would Meta
//! reject", so pre-flight checks and help text never drift apart.

use std::ops::RangeInclusive;

/// Maximum characters in a text message body.
pub const MAX_TEXT_BODY: usize = 4096;
/// Maximum characters in a media caption.
pub const MAX_CAPTION: usize = 1024;
/// Reply buttons allowed on an interactive button message.
pub const MAX_REPLY_BUTTONS: usize = 3;
/// Most buttons a template can have, so the highest button index is one less.
pub const MAX_TEMPLATE_BUTTONS: u8 = 10;
pub const MAX_COUPON_CODE: usize = 20;
/// Characters in a template name.
pub const MAX_TEMPLATE_NAME: usize = 512;
/// Characters in a template's BODY text.
pub const MAX_TEMPLATE_BODY: usize = 1024;
/// Characters in a template's text header.
pub const MAX_TEMPLATE_HEADER_TEXT: usize = 60;
/// Parameters a template's text header can hold.
pub const MAX_TEMPLATE_HEADER_PARAMS: usize = 1;
/// Characters in a template's footer.
pub const MAX_TEMPLATE_FOOTER: usize = 60;
/// Characters in a template button's label.
pub const MAX_TEMPLATE_BUTTON_TEXT: usize = 25;
/// Characters in a phone number button's number.
pub const MAX_TEMPLATE_BUTTON_PHONE: usize = 20;
/// Characters in a URL button's URL.
pub const MAX_TEMPLATE_BUTTON_URL: usize = 2000;
/// URL buttons allowed on one template.
pub const MAX_TEMPLATE_URL_BUTTONS: usize = 2;
/// Phone number buttons allowed on one template.
pub const MAX_TEMPLATE_PHONE_BUTTONS: usize = 1;
/// Copy code buttons allowed on one template.
pub const MAX_TEMPLATE_COPY_CODE_BUTTONS: usize = 1;
/// Template ids one bulk delete can name.
pub const MAX_TEMPLATE_DELETE_IDS: usize = 100;
/// `message_send_ttl_seconds` an authentication template accepts.
pub const TEMPLATE_TTL_AUTHENTICATION: RangeInclusive<i64> = 30..=900;
/// `message_send_ttl_seconds` a utility template accepts.
pub const TEMPLATE_TTL_UTILITY: RangeInclusive<i64> = 30..=43_200;
/// `message_send_ttl_seconds` a marketing template accepts.
pub const TEMPLATE_TTL_MARKETING: RangeInclusive<i64> = 43_200..=2_592_000;
/// The value that asks for a 30-day TTL on authentication and utility templates.
pub const TEMPLATE_TTL_30_DAYS: i64 = -1;
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
/// Maximum characters in a list section's title.
pub const MAX_LIST_SECTION_TITLE: usize = 24;
/// Maximum characters in a list row's id, title and description.
pub const MAX_LIST_ROW_ID: usize = 200;
pub const MAX_LIST_ROW_TITLE: usize = 24;
pub const MAX_LIST_ROW_DESCRIPTION: usize = 72;
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
