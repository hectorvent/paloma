//! Every kind's plan against a stubbed Graph API: in sync when live matches, the right
//! writes when it doesn't, and the same confirmations and refusals as the matching command.

use paloma_core::{Credentials, Result};
use paloma_graph::api::{calling, commerce, phone_numbers, templates, webhook};
use paloma_graph::{Client, ClientConfig};
use secrecy::SecretString;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::kind::Cx;
use crate::{Kind, KindPlan, Scope};

const PHONE: &str = "111";
const WABA: &str = "222";

#[derive(Debug)]
struct Token;

impl Credentials for Token {
    fn access_token(&self) -> Result<SecretString> {
        Ok("EAAtest".to_owned().into())
    }
    fn app_secret(&self) -> Result<SecretString> {
        Ok("secret".to_owned().into())
    }
}

/// A Graph stub answering one GET with `body`.
async fn graph(get: &str, body: Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{get}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    server
}

async fn plan(server: &MockServer, kind: Kind, spec: Value) -> Result<KindPlan> {
    let client = Client::new(
        ClientConfig::new(server.uri(), "v26.0"),
        std::sync::Arc::new(Token),
    )?;
    let scope = Scope::new(Some(WABA), Some(PHONE), Some("333"));
    kind.plan(
        Cx {
            client: &client,
            scope: &scope,
        },
        &spec,
    )
    .await
}

async fn writes(server: &MockServer) -> usize {
    let requests = server.received_requests().await.unwrap();
    requests
        .iter()
        .filter(|r| r.method.as_str() != "GET")
        .count()
}

#[tokio::test]
async fn calling_is_in_sync_when_live_matches_and_confirms_turning_things_off() {
    let live = json!({ "calling": { "status": "ENABLED", "sip": { "status": "ENABLED" } } });
    let server = graph(&format!("{PHONE}/settings"), live).await;
    let same = plan(&server, Kind::Calling, json!({ "status": "ENABLED" }))
        .await
        .unwrap();
    assert!(same.in_sync() && same.confirm.is_none());

    let off = plan(&server, Kind::Calling, json!({ "status": "DISABLED" }))
        .await
        .unwrap();
    assert_eq!(off.requests.len(), 1);
    assert_eq!(off.confirm, Some(calling::DISABLE_CONFIRM));

    let sip_off = plan(
        &server,
        Kind::Calling,
        json!({ "sip": { "status": "DISABLED" } }),
    )
    .await
    .unwrap();
    assert_eq!(sip_off.confirm, Some(calling::SIP_OFF_CONFIRM));
    assert_eq!(writes(&server).await, 0, "planning never writes");
}

#[tokio::test]
async fn commerce_confirms_only_when_a_feature_turns_off() {
    let live =
        json!({ "data": [{ "id": "c", "is_cart_enabled": true, "is_catalog_visible": true }] });
    let server = graph(&format!("{PHONE}/whatsapp_commerce_settings"), live).await;
    let same = plan(
        &server,
        Kind::Commerce,
        json!({ "is_catalog_visible": true }),
    )
    .await
    .unwrap();
    assert!(same.in_sync());
    let off = plan(&server, Kind::Commerce, json!({ "is_cart_enabled": false }))
        .await
        .unwrap();
    assert_eq!(off.confirm, Some(commerce::DISABLE_CONFIRM));
}

#[tokio::test]
async fn profile_drift_writes_without_a_confirmation() {
    let live = json!({ "data": [{ "about": "Open daily" }] });
    let server = graph(&format!("{PHONE}/whatsapp_business_profile"), live).await;
    assert!(
        plan(&server, Kind::Profile, json!({ "about": "Open daily" }))
            .await
            .unwrap()
            .in_sync()
    );
    let changed = plan(&server, Kind::Profile, json!({ "about": "Closed Sundays" }))
        .await
        .unwrap();
    assert_eq!(changed.requests.len(), 1);
    assert!(changed.confirm.is_none());
}

#[tokio::test]
async fn number_confirms_a_pin_and_refuses_fields_it_does_not_manage() {
    let server = MockServer::start().await;
    let pin = plan(&server, Kind::Number, json!({ "pin": "825194" }))
        .await
        .unwrap();
    assert_eq!(pin.confirm, Some(phone_numbers::PIN_CONFIRM));
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        0,
        "a pin needs no read"
    );
    let err = plan(&server, Kind::Number, json!({ "color": "blue" }))
        .await
        .unwrap_err();
    assert_eq!(err.exit_code().code(), 5);
}

#[tokio::test]
async fn webhook_checks_the_callback_before_reading_and_confirms_a_change() {
    let server = MockServer::start().await;
    let insecure = json!({ "callback_url": "http://example.com/hook", "verify_token": "t" });
    let err = plan(&server, Kind::Webhook, insecure).await.unwrap_err();
    assert_eq!(err.exit_code().code(), 5);
    assert!(server.received_requests().await.unwrap().is_empty());

    let subscribed = json!({ "data": [{ "override_callback_uri": "https://old.example.com" }] });
    let server = graph(&format!("{WABA}/subscribed_apps"), subscribed).await;
    let change = json!({ "callback_url": "https://new.example.com", "verify_token": "t" });
    let moved = plan(&server, Kind::Webhook, change).await.unwrap();
    assert_eq!(moved.requests.len(), 1);
    assert_eq!(moved.confirm, Some(webhook::SUBSCRIBE_CONFIRM));
}

#[tokio::test]
async fn a_template_is_created_when_missing_and_an_edit_confirms() {
    let missing = graph(&format!("{WABA}/message_templates"), json!({ "data": [] })).await;
    let spec =
        json!({ "name": "notice", "language": "en_US", "category": "UTILITY", "body": "Shipped" });
    let created = plan(&missing, Kind::Template, spec.clone()).await.unwrap();
    assert_eq!(created.requests.len(), 1);
    assert!(created.changes[0].starts_with("create notice/en_US"));

    let live = json!({ "data": [{ "id": "9", "name": "notice", "language": "en_US",
        "status": "APPROVED", "category": "UTILITY",
        "components": [{ "type": "BODY", "text": "Shipped" }] }] });
    let existing = graph(&format!("{WABA}/message_templates"), live).await;
    assert!(
        plan(&existing, Kind::Template, spec)
            .await
            .unwrap()
            .in_sync()
    );
    let edited = json!({ "name": "notice", "language": "en_US", "category": "UTILITY", "body": "On its way" });
    let edit = plan(&existing, Kind::Template, edited).await.unwrap();
    assert_eq!(edit.confirm, Some(templates::EDIT_CONFIRM));
}

#[tokio::test]
async fn account_compares_a_numeric_timezone_as_text_and_refuses_unmanaged_fields() {
    let live = json!({ "id": WABA, "name": "Acme", "timezone_id": "7" });
    let server = graph(WABA, live).await;
    let same = plan(
        &server,
        Kind::Account,
        json!({ "name": "Acme", "timezone_id": 7 }),
    )
    .await
    .unwrap();
    assert!(same.in_sync());
    let renamed = plan(&server, Kind::Account, json!({ "name": "Acme Ltd" }))
        .await
        .unwrap();
    assert_eq!(renamed.requests.len(), 1);
    assert!(renamed.confirm.is_none());

    let quiet = MockServer::start().await;
    let insights = json!({ "is_enabled_for_insights": true });
    let err = plan(&quiet, Kind::Account, insights).await.unwrap_err();
    assert_eq!(err.exit_code().code(), 5);
    assert!(quiet.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn conversational_automation_writes_only_what_drifted() {
    let live = json!({ "conversational_automation": { "enable_welcome_message": true } });
    let server = graph(PHONE, live).await;
    let same = json!({ "enable_welcome_message": true });
    assert!(
        plan(&server, Kind::ConversationalAutomation, same)
            .await
            .unwrap()
            .in_sync()
    );
    let off = json!({ "enable_welcome_message": false });
    let changed = plan(&server, Kind::ConversationalAutomation, off)
        .await
        .unwrap();
    assert_eq!(changed.requests.len(), 1);
    assert_eq!(writes(&server).await, 0);
}

#[tokio::test]
async fn phone_settings_sends_identity_before_storage_and_confirms() {
    let live = json!({
        "storage_configuration": { "status": "DEFAULT" },
        "user_identity_change": { "enable_identity_key_check": false },
    });
    let server = graph(&format!("{PHONE}/settings"), live).await;
    let same = json!({ "user_identity_change": { "enable_identity_key_check": false } });
    assert!(
        plan(&server, Kind::PhoneSettings, same)
            .await
            .unwrap()
            .in_sync()
    );

    let both = json!({
        "storage_configuration": { "status": "IN_COUNTRY_STORAGE_ENABLED", "data_localization_region": "IN" },
        "user_identity_change": { "enable_identity_key_check": true },
    });
    let changed = plan(&server, Kind::PhoneSettings, both).await.unwrap();
    let keys: Vec<_> = changed
        .requests
        .iter()
        .map(|r| {
            r.json_body()
                .unwrap()
                .as_object()
                .unwrap()
                .keys()
                .next()
                .unwrap()
                .clone()
        })
        .collect();
    assert_eq!(keys, ["user_identity_change", "storage_configuration"]);
    assert!(changed.confirm.is_some());

    let quiet = MockServer::start().await;
    let no_region = json!({ "storage_configuration": { "status": "IN_COUNTRY_STORAGE_ENABLED" } });
    let err = plan(&quiet, Kind::PhoneSettings, no_region)
        .await
        .unwrap_err();
    assert_eq!(err.exit_code().code(), 5);
    assert!(quiet.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_qr_code_updates_its_message_and_an_unknown_code_is_refused() {
    let code = "WOMVT6TJ2BP7A1";
    let live = json!({ "data": [{ "code": code, "prefilled_message": "Hi" }] });
    let server = graph(&format!("{PHONE}/message_qrdls/{code}"), live).await;
    let same = json!({ "code": code, "prefilled_message": "Hi" });
    assert!(plan(&server, Kind::QrCode, same).await.unwrap().in_sync());
    let changed = plan(
        &server,
        Kind::QrCode,
        json!({ "code": code, "prefilled_message": "Hello" }),
    )
    .await
    .unwrap();
    assert_eq!(changed.requests.len(), 1);
    assert!(
        changed.changes[0].starts_with(code),
        "{:?}",
        changed.changes
    );

    let missing = graph(
        &format!("{PHONE}/message_qrdls/{code}"),
        json!({ "data": [] }),
    )
    .await;
    let err = plan(
        &missing,
        Kind::QrCode,
        json!({ "code": code, "prefilled_message": "Hi" }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.exit_code().code(), 5);
    assert_eq!(writes(&missing).await, 0);
}
