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
/// Contacts one contacts message can carry.
pub const MAX_CONTACTS: usize = 257;
/// Most delivery status events `GET /{message-history-id}/events` returns per page.
pub const MAX_MESSAGE_HISTORY_EVENTS_PAGE: u32 = 100;
/// Rows allowed across all sections of an interactive list message.
pub const MAX_LIST_ROWS: usize = 10;
/// Maximum characters in a list section's title.
pub const MAX_LIST_SECTION_TITLE: usize = 24;
/// Maximum characters in a list row's id, title and description.
pub const MAX_LIST_ROW_ID: usize = 200;
pub const MAX_LIST_ROW_TITLE: usize = 24;
pub const MAX_LIST_ROW_DESCRIPTION: usize = 72;
/// Characters in an interactive body. Meta's location request, catalog and carousel pages
/// state this. Other kinds fall back to its OpenAPI schema, which says the same, except a
/// list, whose page allows more.
pub const MAX_INTERACTIVE_BODY: usize = 1024;
/// Characters in an interactive text header and footer.
pub const MAX_INTERACTIVE_HEADER: usize = 60;
pub const MAX_INTERACTIVE_FOOTER: usize = 60;
/// Characters Meta advises for a Flow's button. Its page advises rather than requires
/// this, though its OpenAPI schema says maximum, so going over only warns.
pub const ADVISED_FLOW_CTA: usize = 30;
/// Cards one carousel must have, media or product.
pub const CAROUSEL_CARDS: RangeInclusive<usize> = 2..=10;
/// A carousel card's optional body text.
pub const MAX_CAROUSEL_CARD_BODY: usize = 160;
pub const MAX_CAROUSEL_CARD_BODY_LINE_BREAKS: usize = 2;
/// A carousel card's quick-reply button, and the label of its URL button.
pub const MAX_CAROUSEL_REPLY_ID: usize = 256;
pub const MAX_CAROUSEL_BUTTON_LABEL: usize = 20;
/// Characters in a call button's label.
pub const MAX_CALL_BUTTON_TEXT: usize = 20;
/// Characters in a call button's tracking payload.
pub const MAX_CALL_BUTTON_PAYLOAD: usize = 512;
/// How long a call button lasts, in minutes: up to 30 days.
pub const CALL_BUTTON_TTL_MINUTES: RangeInclusive<u32> = 1..=43_200;
/// Products across all sections of a multi-product message.
pub const MAX_PRODUCT_LIST_PRODUCTS: usize = 30;
/// Sections in a multi-product message, each with a title of at most this many characters.
pub const MAX_PRODUCT_LIST_SECTIONS: usize = 10;
pub const MAX_PRODUCT_LIST_SECTION_TITLE: usize = 24;
/// Maximum characters in the business profile's text fields. Meta's profile pages only
/// state the address limit. The rest come from its OpenAPI spec, except the description:
/// the spec says 256 but the Embedded Signup page says 512, and the page wins.
pub const MAX_PROFILE_ABOUT: usize = 139;
pub const MAX_PROFILE_ADDRESS: usize = 256;
pub const MAX_PROFILE_DESCRIPTION: usize = 512;
pub const MAX_PROFILE_EMAIL: usize = 128;
pub const MAX_PROFILE_WEBSITE: usize = 256;
/// Websites a business profile can list.
pub const MAX_PROFILE_WEBSITES: usize = 2;
/// Meta doesn't say whether its "MB" is decimal or binary, so sizes use the larger reading
/// and never refuse a file Meta would take.
const KB: u64 = 1024;
const MB: u64 = 1024 * KB;

/// What Meta supports for one media type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaLimit {
    pub kind: &'static str,
    pub types: &'static [&'static str],
    pub max_bytes: u64,
    /// Whether Meta may still take a type that isn't listed.
    pub allows_unlisted: bool,
}

pub const AUDIO_MEDIA: MediaLimit = MediaLimit {
    kind: "audio",
    types: &[
        "audio/aac",
        "audio/amr",
        "audio/mpeg",
        "audio/mp4",
        "audio/ogg",
    ],
    max_bytes: 16 * MB,
    allows_unlisted: false,
};
/// Meta's page lists these and adds that other file types "may be sent via the API", but
/// aren't supported and may not display correctly.
pub const DOCUMENT_MEDIA: MediaLimit = MediaLimit {
    kind: "document",
    types: &[
        "text/plain",
        "application/vnd.ms-excel",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "application/msword",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "application/vnd.ms-powerpoint",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "application/pdf",
    ],
    max_bytes: 100 * MB,
    allows_unlisted: true,
};
pub const IMAGE_MEDIA: MediaLimit = MediaLimit {
    kind: "image",
    types: &["image/jpeg", "image/png"],
    max_bytes: 5 * MB,
    allows_unlisted: false,
};
/// Animated stickers can reach 500 KB and static ones 100 KB. Telling them apart means
/// parsing the WebP, so this is the larger limit and Meta enforces the smaller one.
pub const STICKER_MEDIA: MediaLimit = MediaLimit {
    kind: "sticker",
    types: &["image/webp"],
    max_bytes: 500 * KB,
    allows_unlisted: false,
};
pub const VIDEO_MEDIA: MediaLimit = MediaLimit {
    kind: "video",
    types: &["video/3gpp", "video/mp4"],
    max_bytes: 16 * MB,
    allows_unlisted: false,
};
/// Every media type, for an upload that doesn't say which one it's for.
pub const MEDIA_LIMITS: [MediaLimit; 5] = [
    AUDIO_MEDIA,
    DOCUMENT_MEDIA,
    IMAGE_MEDIA,
    STICKER_MEDIA,
    VIDEO_MEDIA,
];
/// Digits in a two-step verification PIN.
pub const PIN_DIGITS: usize = 6;
/// Most phone numbers `GET /{waba-id}/phone_numbers` returns per page.
pub const MAX_PHONE_NUMBERS_PAGE: u32 = 100;
/// Alphanumeric characters in a business-scoped user ID, after its prefix.
pub const MAX_BSUID_ID: usize = 128;
/// Users a business phone number's blocklist can hold.
pub const MAX_BLOCKLIST_USERS: usize = 64_000;
/// Countries `data_localization_region` accepts when registering a number.
pub const DATA_LOCALIZATION_REGIONS: [&str; 14] = [
    "AE", "AU", "BH", "BR", "CA", "CH", "DE", "GB", "ID", "IN", "JP", "KR", "SG", "ZA",
];
/// Default SIP port when `--server` omits one (SIP over TLS).
pub const DEFAULT_SIP_PORT: u16 = 5061;
