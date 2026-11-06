//! `export`, `apply` and `diff`: only drift is written, and dry runs never write.

use crate::common::*;

fn calling_manifest(home: &TempDir, status: &str) -> std::path::PathBuf {
    let path = home.path().join("calling.yaml");
    std::fs::write(
        &path,
        format!("apiVersion: paloma/v1\nkind: Calling\nmetadata:\n  phoneNumberId: \"{PHONE}\"\nspec:\n  status: {status}\n  callIconVisibility: ${{ICON:-DEFAULT}}\n"),
    )
    .unwrap();
    path
}

async fn mock_calling_settings(graph: &MockServer) {
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}/settings")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "calling": { "status": "ENABLED", "call_icon_visibility": "DEFAULT", "callback_permission_status": "ENABLED" }
        })))
        .mount(graph)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn diff_exits_1_on_drift_and_0_in_sync() {
    let graph = MockServer::start().await;
    mock_calling_settings(&graph).await;
    let home = TempDir::new().unwrap();

    let in_sync = calling_manifest(&home, "ENABLED");
    paloma(Some(&graph), &home)
        .args(["diff", "-f"])
        .arg(&in_sync)
        .assert()
        .success()
        .stderr(predicate::str::contains("Calling/111 in sync"));

    let drift = calling_manifest(&home, "DISABLED");
    paloma(Some(&graph), &home)
        .args(["diff", "-f"])
        .arg(&drift)
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains(r#"status: "ENABLED" -> "DISABLED""#)
                .and(predicate::str::contains("error").not()),
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn apply_dry_run_reads_but_never_writes() {
    let graph = MockServer::start().await;
    mock_calling_settings(&graph).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = calling_manifest(&home, "DISABLED");
    let out = paloma(Some(&graph), &home)
        .args(["apply", "--dry-run", "-f"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        stdout_json(&out),
        json!({ "messaging_product": "whatsapp", "calling": { "status": "DISABLED", "call_icon_visibility": "DEFAULT" } })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn apply_writes_only_drift() {
    let graph = MockServer::start().await;
    mock_calling_settings(&graph).await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/settings")))
        .and(body_json(json!({ "messaging_product": "whatsapp", "calling": { "status": "DISABLED", "call_icon_visibility": "DEFAULT" } })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = calling_manifest(&home, "DISABLED");
    paloma(Some(&graph), &home)
        .args(["apply", "--yes", "-f"])
        .arg(&file)
        .assert()
        .success()
        .stderr(predicate::str::contains("Calling/111 configured"));
}

/// `apply` asks for the same confirmations and runs the same checks as the commands that
/// make the same change, so a manifest isn't a way around them.
#[tokio::test(flavor = "multi_thread")]
async fn apply_guards_writes_like_the_matching_commands() {
    let graph = MockServer::start().await;
    mock_calling_settings(&graph).await;
    let home = TempDir::new().unwrap();
    let calling = calling_manifest(&home, "DISABLED");
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&calling)
        .assert()
        .code(5)
        .stderr(
            predicate::str::contains("turn calling off").and(predicate::str::contains("--yes")),
        );

    // Turning SIP off confirms, like `calling sip disable`.
    let sip = home.path().join("sip.yaml");
    std::fs::write(
        &sip,
        format!("apiVersion: paloma/v1\nkind: Calling\nmetadata:\n  phoneNumberId: \"{PHONE}\"\nspec:\n  sip:\n    status: DISABLED\n"),
    )
    .unwrap();
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&sip)
        .assert()
        .code(5)
        .stderr(predicate::str::contains("turn SIP off"));
    assert!(
        graph
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.method.as_str() == "GET"),
        "a refused plan writes nothing"
    );

    // A plain-http callback is refused before the kind reads anything.
    let fresh = MockServer::start().await;
    let webhook = home.path().join("webhook.yaml");
    std::fs::write(
        &webhook,
        "apiVersion: paloma/v1\nkind: Webhook\nspec:\n  callbackUrl: http://example.com/hook\n  verifyToken: t\n",
    )
    .unwrap();
    paloma(Some(&fresh), &home)
        .args(["apply", "--yes", "-f"])
        .arg(&webhook)
        .assert()
        .code(5)
        .stderr(predicate::str::contains("callbackUrl must be an https URL"));
    assert!(fresh.received_requests().await.unwrap().is_empty());
}

#[test]
fn export_template_is_offline_and_annotated() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env_remove("PALOMA_TOKEN")
        .args(["export", "calling", "--skeleton"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("apiVersion: paloma/v1")
                .and(predicate::str::contains("phoneNumberId: <phone-number-id>"))
                .and(predicate::str::contains("  # sip:")),
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn manifest_password_is_redacted_from_preview_and_drift_but_sent_unchanged() {
    let graph = MockServer::start().await;
    mock_calling_settings(&graph).await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/settings")))
        .and(body_json(json!({"messaging_product":"whatsapp", "calling":{
            "sip":{"servers":[{"hostname":"sip.example.test", "sip_user_password":"SIP_PASSWORD_SENTINEL"}]}
        }})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1).mount(&graph).await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("calling-secret.yaml");
    std::fs::write(&file, "apiVersion: paloma/v1\nkind: Calling\nspec:\n  sip:\n    servers:\n      - hostname: sip.example.test\n        sipUserPassword: SIP_PASSWORD_SENTINEL\n").unwrap();
    for command in ["diff", "apply"] {
        let mut cmd = paloma(Some(&graph), &home);
        cmd.args([command, "-f"]).arg(&file).arg("-vv");
        if command == "apply" {
            cmd.arg("--dry-run");
        }
        let out = cmd.output().unwrap();
        assert_eq!(out.status.code(), Some(i32::from(command == "diff")));
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!combined.contains("SIP_PASSWORD_SENTINEL"), "{combined}");
        assert!(combined.contains("redacted"));
    }
    let out = paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&file)
        .arg("-vv")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!String::from_utf8_lossy(&out.stderr).contains("SIP_PASSWORD_SENTINEL"));
}

fn template_manifest(home: &TempDir, footer: &str) -> std::path::PathBuf {
    let path = home.path().join("template.yaml");
    std::fs::write(
        &path,
        format!(
            "apiVersion: paloma/v1\nkind: Template\nspec:\n  name: promo\n  language: en_US\n  category: MARKETING\n  components:\n    - type: header\n      format: image\n      example:\n        headerHandle: [\"4::upload-handle\"]\n    - type: BODY\n      text: \"Hi {{{{1}}}}\"\n    - type: FOOTER\n      text: \"{footer}\"\n    - type: BUTTONS\n      buttons:\n        - type: URL\n          text: Shop\n          url: \"https://example.com/shop\"\n"
        ),
    )
    .unwrap();
    path
}

/// The live template as Meta returns it: upper-case types, the header handle rewritten
/// to a CDN URL, and server fields the manifest never declares.
async fn mock_live_template(graph: &MockServer) {
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/message_templates")))
        .and(query_param("name", "promo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [{
            "id": "tpl-1", "name": "promo", "language": "en_US", "status": "APPROVED",
            "category": "MARKETING",
            "components": [
                { "type": "HEADER", "format": "IMAGE", "example": { "header_handle": ["https://scontent.example/h.jpg"] } },
                { "type": "BODY", "text": "Hi {{1}}" },
                { "type": "FOOTER", "text": "Reply STOP to opt out" },
                { "type": "BUTTONS", "buttons": [{ "type": "URL", "text": "Shop", "url": "https://example.com/shop" }] }
            ]
        }] })))
        .mount(graph)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn template_diff_sees_non_body_changes_but_not_server_metadata() {
    let graph = MockServer::start().await;
    mock_live_template(&graph).await;
    let home = TempDir::new().unwrap();

    let same = template_manifest(&home, "Reply STOP to opt out");
    paloma(Some(&graph), &home)
        .args(["diff", "-f"])
        .arg(&same)
        .assert()
        .success()
        .stderr(predicate::str::contains("in sync"));

    let footer_changed = template_manifest(&home, "Reply STOP to unsubscribe");
    paloma(Some(&graph), &home)
        .args(["diff", "-f"])
        .arg(&footer_changed)
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            r#"components.2.text: "Reply STOP to opt out" -> "Reply STOP to unsubscribe""#,
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn template_apply_edits_with_the_declared_components() {
    let graph = MockServer::start().await;
    mock_live_template(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/tpl-1"))
        .and(body_json(json!({
            "components": [
                { "type": "header", "format": "image", "example": { "header_handle": ["4::upload-handle"] } },
                { "type": "BODY", "text": "Hi {{1}}" },
                { "type": "FOOTER", "text": "Reply STOP to unsubscribe" },
                { "type": "BUTTONS", "buttons": [{ "type": "URL", "text": "Shop", "url": "https://example.com/shop" }] }
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = template_manifest(&home, "Reply STOP to unsubscribe");
    paloma(Some(&graph), &home)
        .args(["apply", "--yes", "-f"])
        .arg(&file)
        .assert()
        .success();
}

fn shorthand_manifest(home: &TempDir, category: &str, body: &str) -> std::path::PathBuf {
    let path = home.path().join("notice.yaml");
    std::fs::write(
        &path,
        format!("apiVersion: paloma/v1\nkind: Template\nspec:\n  name: notice\n  language: en_US\n  category: {category}\n  body: \"{body}\"\n"),
    )
    .unwrap();
    path
}

fn live_notice(header_format: &str, status: &str) -> Value {
    json!({ "data": [{
        "id": "tpl-2", "name": "notice", "language": "en_US", "status": status,
        "category": "UTILITY",
        "components": [
            { "type": "HEADER", "format": header_format, "text": "Order update" },
            { "type": "BODY", "text": "Your order shipped." },
            { "type": "FOOTER", "text": "Lucky Shrub" },
            { "type": "BUTTONS", "buttons": [{ "type": "QUICK_REPLY", "text": "Thanks" }] }
        ]
    }] })
}

async fn mock_notice(graph: &MockServer, header_format: &str, status: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/message_templates")))
        .and(query_param("name", "notice"))
        .respond_with(ResponseTemplate::new(200).set_body_json(live_notice(header_format, status)))
        .mount(graph)
        .await;
}

/// Meta's edit replaces every component, so a `body` shorthand edit must resend the
/// live header, footer and buttons unchanged around the new BODY.
#[tokio::test(flavor = "multi_thread")]
async fn template_shorthand_edit_keeps_the_other_live_components() {
    let graph = MockServer::start().await;
    mock_notice(&graph, "TEXT", "APPROVED").await;
    Mock::given(method("POST"))
        .and(path("/v26.0/tpl-2"))
        .and(body_json(json!({
            "components": [
                { "type": "HEADER", "format": "TEXT", "text": "Order update" },
                { "type": "BODY", "text": "Your order is on its way." },
                { "type": "FOOTER", "text": "Lucky Shrub" },
                { "type": "BUTTONS", "buttons": [{ "type": "QUICK_REPLY", "text": "Thanks" }] }
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = shorthand_manifest(&home, "UTILITY", "Your order is on its way.");
    paloma(Some(&graph), &home)
        .args(["apply", "--yes", "-f"])
        .arg(&file)
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
/// Meta only allows category edits on REJECTED or PAUSED templates
/// (`templates/template-management.md`), so the fixture is a rejected one.
async fn template_category_change_sends_only_the_category() {
    let graph = MockServer::start().await;
    // Media would block a shorthand edit of the components, but they aren't being edited.
    mock_notice(&graph, "IMAGE", "REJECTED").await;
    Mock::given(method("POST"))
        .and(path("/v26.0/tpl-2"))
        .and(body_json(json!({ "category": "MARKETING" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = shorthand_manifest(&home, "MARKETING", "Your order shipped.");
    paloma(Some(&graph), &home)
        .args(["apply", "--yes", "-f"])
        .arg(&file)
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
async fn template_shorthand_refuses_to_edit_a_media_header_template() {
    let graph = MockServer::start().await;
    mock_notice(&graph, "IMAGE", "APPROVED").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = shorthand_manifest(&home, "UTILITY", "Your order is on its way.");
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&file)
        .assert()
        .code(5)
        .stderr(predicate::str::contains("media header or carousel"));
}

#[tokio::test(flavor = "multi_thread")]
async fn template_apply_in_sync_writes_nothing() {
    let graph = MockServer::start().await;
    mock_live_template(&graph).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = template_manifest(&home, "Reply STOP to opt out");
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&file)
        .assert()
        .success();
}

/// `promo`/`en_US` as Meta returns it, on page 2 of the plan's lookup; page 3 stays unread.
async fn mock_live_template_on_page_two(graph: &MockServer) {
    let p = format!("/v26.0/{WABA}/message_templates");
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param("after", "C2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [] })))
        .expect(0)
        .mount(graph)
        .await;
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param("after", "C1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{
                "id": "tpl-1", "name": "promo", "language": "en_US", "status": "APPROVED",
                "category": "MARKETING",
                "components": [
                    { "type": "HEADER", "format": "IMAGE", "example": { "header_handle": ["https://scontent.example/h.jpg"] } },
                    { "type": "BODY", "text": "Hi {{1}}" },
                    { "type": "FOOTER", "text": "Reply STOP to opt out" },
                    { "type": "BUTTONS", "buttons": [{ "type": "URL", "text": "Shop", "url": "https://example.com/shop" }] }
                ]
            }],
            "paging": { "cursors": { "after": "C2" }, "next": "https://graph/next" }
        })))
        .mount(graph)
        .await;
    Mock::given(method("GET"))
        .and(path(&p))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "11", "name": "promo_one", "language": "en_US" }],
            "paging": { "cursors": { "after": "C1" }, "next": "https://graph/next" }
        })))
        .mount(graph)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn template_apply_in_sync_on_a_later_page_does_not_create_it() {
    let graph = MockServer::start().await;
    mock_live_template_on_page_two(&graph).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = template_manifest(&home, "Reply STOP to opt out");
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&file)
        .assert()
        .success();
    assert_eq!(
        received_queries(&graph).await,
        [
            "name=promo&language=en_US&limit=100",
            "name=promo&language=en_US&limit=100&after=C1",
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn template_apply_drift_on_a_later_page_edits_instead_of_creating() {
    let graph = MockServer::start().await;
    mock_live_template_on_page_two(&graph).await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}/message_templates")))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path("/v26.0/tpl-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = template_manifest(&home, "Reply STOP to unsubscribe");
    paloma(Some(&graph), &home)
        .args(["apply", "--yes", "-f"])
        .arg(&file)
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
async fn template_apply_still_creates_when_no_page_has_it() {
    let graph = MockServer::start().await;
    mock_three_template_pages(
        &graph,
        json!([{ "name": "promo", "language": "es", "id": "21" }]),
        json!([]),
    )
    .await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}/message_templates")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "new" })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = template_manifest(&home, "Reply STOP to opt out");
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&file)
        .assert()
        .success();
    assert_eq!(graph.received_requests().await.unwrap().len(), 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn webhook_manifest_stays_waba_scoped_when_the_phone_has_an_override() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/subscribed_apps")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"data":[{"override_callback_uri":"https://example.com/waba"}]}),
            ),
        )
        .expect(2)
        .mount(&graph)
        .await;
    Mock::given(method("GET")).and(path(format!("/v26.0/{PHONE}")))
        .and(query_param("fields", "webhook_configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"webhook_configuration":{"phone_number":"https://example.com/number","application":"https://example.com/app"}})))
        .expect(0).mount(&graph).await;
    Mock::given(method("GET"))
        .and(path("/v26.0/333/subscriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"data":[{"object":"whatsapp_business_account","fields":[{"name":"messages"}]}]}),
        ))
        .expect(2)
        .mount(&graph)
        .await;
    let out = paloma(Some(&graph), &home)
        .env("PALOMA_APP_ID", "333")
        .env("PALOMA_APP_SECRET", "secret")
        .args(["export", "Webhook", "-o", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let manifest = stdout_json(&out);
    assert_eq!(manifest["spec"]["callbackUrl"], "https://example.com/waba");
    let file = home.path().join("webhook.json");
    std::fs::write(&file, manifest.to_string()).unwrap();
    paloma(Some(&graph), &home)
        .env("PALOMA_APP_ID", "333")
        .env("PALOMA_APP_SECRET", "secret")
        .args(["apply", "-f"])
        .arg(&file)
        .assert()
        .success()
        .stdout("")
        .stderr(predicate::str::contains("unchanged"));
    assert!(
        graph
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.method.as_str() == "GET")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn webhook_manifest_refuses_a_new_unknown_field_but_keeps_a_subscribed_one() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v26.0/333/subscriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [{
            "object": "whatsapp_business_account",
            "fields": [{"name": "messages"}, {"name": "future_meta_field"}],
        }]})))
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let manifest = |fields: &str| {
        let file = home.path().join("webhook.yaml");
        std::fs::write(
            &file,
            format!("apiVersion: paloma/v1\nkind: Webhook\nspec:\n  fields: [{fields}]\n"),
        )
        .unwrap();
        file
    };
    let run = |file: &std::path::Path, verb: &str| {
        let mut cmd = paloma(Some(&graph), &home);
        cmd.env("PALOMA_APP_SECRET", APP_SECRET)
            .args([verb, "-f"])
            .arg(file)
            .args(["--app-id", "333", "--yes"]);
        cmd
    };
    let exported = manifest("messages, future_meta_field");
    run(&exported, "diff").assert().success();
    let typo = manifest("messages, future_meta_field, mesages");
    run(&typo, "apply")
        .assert()
        .code(5)
        .stderr(predicate::str::contains("unknown webhook field: mesages"));
}

fn account_manifest(home: &TempDir, spec: &str) -> std::path::PathBuf {
    let file = home.path().join("account.yaml");
    std::fs::write(
        &file,
        format!("apiVersion: paloma/v1\nkind: Account\nmetadata: {{}}\nspec:\n{spec}"),
    )
    .unwrap();
    file
}

async fn mock_account_settings(graph: &MockServer) {
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}")))
        .and(query_param("fields", "name,timezone_id"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "Acme",
            "timezone_id": "7",
            "id": WABA,
        })))
        .mount(graph)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn account_manifest_diff_compares_a_bare_timezone_with_metas_string() {
    let graph = MockServer::start().await;
    mock_account_settings(&graph).await;
    let home = TempDir::new().unwrap();
    let file = account_manifest(&home, "  name: Acme\n  timezoneId: 7\n");
    paloma(Some(&graph), &home)
        .args(["diff", "-f"])
        .arg(&file)
        .assert()
        .success();
    let file = account_manifest(&home, "  name: Acme Inc\n  timezoneId: 7\n");
    paloma(Some(&graph), &home)
        .args(["diff", "-f"])
        .arg(&file)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Account/222"));
}

#[tokio::test(flavor = "multi_thread")]
async fn account_manifest_apply_dry_run_reads_but_never_writes() {
    let graph = MockServer::start().await;
    mock_account_settings(&graph).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = account_manifest(&home, "  name: Acme Inc\n");
    paloma(Some(&graph), &home)
        .args(["apply", "--dry-run", "-f"])
        .arg(&file)
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
async fn account_manifest_apply_writes_only_on_drift() {
    let graph = MockServer::start().await;
    mock_account_settings(&graph).await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}")))
        .and(body_json(json!({ "name": "Acme Inc", "timezone_id": "7" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = account_manifest(&home, "  name: Acme Inc\n  timezoneId: \"7\"\n");
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&file)
        .assert()
        .success()
        .stderr(predicate::str::contains("Account/222 configured"));
    let file = account_manifest(&home, "  name: Acme\n");
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&file)
        .assert()
        .success()
        .stderr(predicate::str::contains("Account/222 unchanged"));
}

#[tokio::test(flavor = "multi_thread")]
async fn account_manifest_refuses_the_toggle_insights_and_billing_before_any_call() {
    let graph = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    for spec in [
        "  disableMarketingMessagesOnCloudApi: true\n",
        "  isEnabledForInsights: true\n",
        "  currency: INR\n",
        "  name: Acme\n  disableMarketingMessagesOnCloudApi: false\n",
    ] {
        let file = account_manifest(&home, spec);
        paloma(Some(&graph), &home)
            .args(["apply", "-f"])
            .arg(&file)
            .assert()
            .code(5)
            .stderr(predicate::str::contains("manages name and timezoneId"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn account_manifest_export_keeps_only_the_settings() {
    let graph = MockServer::start().await;
    mock_account_settings(&graph).await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["export", "account", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let manifest = stdout_json(&out);
    assert_eq!(manifest["kind"], "Account");
    assert_eq!(manifest["metadata"], json!({}));
    assert_eq!(
        manifest["spec"],
        json!({ "name": "Acme", "timezoneId": "7" })
    );
    paloma(None, &home)
        .env_remove("PALOMA_TOKEN")
        .args(["export", "account", "--skeleton"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("kind: Account")
                .and(predicate::str::contains("# timezoneId:"))
                .and(predicate::str::contains("phoneNumberId").not())
                .and(predicate::str::contains("disableMarketing").not()),
        );
}

fn qr_manifest(home: &TempDir, spec: &str) -> std::path::PathBuf {
    let file = home.path().join("qr.yaml");
    std::fs::write(
        &file,
        format!("apiVersion: paloma/v1\nkind: QrCode\nspec:\n{spec}\n"),
    )
    .unwrap();
    file
}

async fn mock_qr_get(graph: &MockServer) {
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}/message_qrdls/{QR_CODE}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [json!({"code": QR_CODE, "prefilled_message": "Cyber Monday", "deep_link_url":"https://wa.me/message/example"})] })))
        .mount(graph)
        .await;
}

fn qr_spec(message: &str) -> String {
    format!("  code: {QR_CODE}\n  prefilledMessage: {message}")
}

#[tokio::test(flavor = "multi_thread")]
async fn qr_code_export_emits_only_the_code_and_message_and_round_trips() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}/message_qrdls")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [json!({"code": QR_CODE, "prefilled_message": "Cyber Monday", "deep_link_url":"https://wa.me/message/example"}), { "code": "WOMVT6TJ2BP7A1", "prefilled_message": "Hello" }],
        })))
        .expect(1)
        .mount(&graph)
        .await;
    mock_qr_get(&graph).await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["export", "qrcode"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let exported = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(exported.matches("kind: QrCode").count(), 2, "{exported}");
    assert!(exported.contains(&format!("code: {QR_CODE}")), "{exported}");
    assert!(
        exported.contains("prefilledMessage: Cyber Monday"),
        "{exported}"
    );
    assert!(
        exported.contains(&format!("phoneNumberId: '{PHONE}'")),
        "{exported}"
    );
    assert!(!exported.contains("deepLinkUrl"), "{exported}");

    let first = exported.split("---").next().unwrap();
    let file = home.path().join("exported.yaml");
    std::fs::write(&file, first).unwrap();
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&file)
        .assert()
        .success()
        .stderr(predicate::str::contains("QrCode/111 unchanged"));
}

#[test]
fn qr_code_export_template_is_offline() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(["export", "qrcode", "--skeleton"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("kind: QrCode")
                .and(predicate::str::contains("# code:"))
                .and(predicate::str::contains("# prefilledMessage:")),
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn qr_code_diff_exits_1_on_drift_and_0_in_sync() {
    let graph = MockServer::start().await;
    mock_qr_get(&graph).await;
    let home = TempDir::new().unwrap();
    let file = qr_manifest(&home, &qr_spec("Cyber Monday"));
    paloma(Some(&graph), &home)
        .args(["diff", "-f"])
        .arg(&file)
        .assert()
        .success()
        .stderr(predicate::str::contains("QrCode/111 in sync"));
    let file = qr_manifest(&home, &qr_spec("Cyber Tuesday"));
    paloma(Some(&graph), &home)
        .args(["diff", "-f"])
        .arg(&file)
        .assert()
        .code(1)
        .stderr(predicate::str::contains(format!(
            r#"{QR_CODE} prefilled_message: "Cyber Monday" -> "Cyber Tuesday""#
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn qr_code_apply_dry_run_reads_but_never_writes() {
    let graph = MockServer::start().await;
    mock_qr_get(&graph).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = qr_manifest(&home, &qr_spec("Cyber Tuesday"));
    let out = paloma(Some(&graph), &home)
        .args(["apply", "--dry-run", "-f"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        stdout_json(&out),
        json!({ "code": QR_CODE, "prefilled_message": "Cyber Tuesday" })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn qr_code_apply_writes_only_drift_through_the_update() {
    let graph = MockServer::start().await;
    mock_qr_get(&graph).await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/message_qrdls")))
        .and(body_json(
            json!({ "code": QR_CODE, "prefilled_message": "Cyber Tuesday" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"code": QR_CODE, "prefilled_message": "Cyber Monday", "deep_link_url":"https://wa.me/message/example"})))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = qr_manifest(&home, &qr_spec("Cyber Monday"));
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&file)
        .assert()
        .success()
        .stderr(predicate::str::contains("QrCode/111 unchanged"));
    let file = qr_manifest(&home, &qr_spec("Cyber Tuesday"));
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&file)
        .assert()
        .success()
        .stderr(predicate::str::contains("QrCode/111 configured"));
}

#[tokio::test(flavor = "multi_thread")]
async fn qr_manifest_rejects_invalid_inputs_before_any_request() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for spec in [
        "  prefilledMessage: Hi".to_owned(),
        "  code: bad\n  prefilledMessage: Hi".to_owned(),
        format!("  code: {QR_CODE}\n  prefilledMessage: ''"),
    ] {
        let file = qr_manifest(&home, &spec);
        paloma(Some(&graph), &home)
            .args(["apply", "-f"])
            .arg(file)
            .assert()
            .code(5)
            .stdout("");
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn qr_manifest_missing_live_code_never_creates_one() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}/message_qrdls/{QR_CODE}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[]})))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&graph)
        .await;
    let file = qr_manifest(&home, &qr_spec("Hello"));
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(file)
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("create it with"));
}

#[tokio::test(flavor = "multi_thread")]
async fn qr_manifest_rejects_wrong_ambiguous_or_malformed_live_state() {
    let home = TempDir::new().unwrap();
    let file = qr_manifest(&home, &qr_spec("Hello"));
    let node = json!({"code":QR_CODE,"prefilled_message":"Hello"});
    for body in [
        json!({"code":"4O4YGZEG3RIVE1","prefilled_message":"Hello"}),
        json!({"data":[{"code":"4O4YGZEG3RIVE1","prefilled_message":"Hello"}]}),
        json!({"data":[node.clone(),node.clone()]}),
        json!({"data":[{}]}),
        json!({"data":null}),
        json!({"code":QR_CODE,"data":[node],"prefilled_message":"Hello"}),
        json!({"code":QR_CODE}),
    ] {
        let graph = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/v26.0/{PHONE}/message_qrdls/{QR_CODE}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(2)
            .mount(&graph)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&graph)
            .await;
        for verb in ["apply", "diff"] {
            paloma(Some(&graph), &home)
                .args([verb, "-f"])
                .arg(&file)
                .assert()
                .code(1)
                .stdout("")
                .stderr(predicate::str::contains("invalid QR lookup response"));
        }
    }
}

fn phone_settings_manifest(home: &TempDir, spec: &str) -> std::path::PathBuf {
    let path = home.path().join("settings.yaml");
    std::fs::write(
        &path,
        format!("apiVersion: paloma/v1\nkind: PhoneSettings\nspec:\n{spec}"),
    )
    .unwrap();
    path
}

#[tokio::test(flavor = "multi_thread")]
async fn phone_settings_manifest_exports_only_writable_fields_and_reconciles_each_feature() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let current = json!({"storage_configuration":{"status":"IN_COUNTRY_STORAGE_ENABLED","data_localization_region":"IN"},"user_identity_change":{"enable_identity_key_check":true},"calling":{"sip":{"servers":[{"password":"PRIVATE_SIP_SENTINEL"}]}}});
    Mock::given(method("GET"))
        .and(path("/v26.0/111/settings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(current))
        .mount(&graph)
        .await;
    let out = paloma(Some(&graph), &home)
        .args(["export", "PhoneSettings", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let spec = &stdout_json(&out)["spec"];
    assert_eq!(spec["storageConfiguration"]["dataLocalizationRegion"], "IN");
    assert!(spec.get("calling").is_none());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("PRIVATE_SIP_SENTINEL"));
    let file = phone_settings_manifest(
        &home,
        "  storageConfiguration:\n    status: IN_COUNTRY_STORAGE_ENABLED\n    dataLocalizationRegion: IN\n  userIdentityChange:\n    enableIdentityKeyCheck: true\n",
    );
    paloma(Some(&graph), &home)
        .args(["diff", "-f"])
        .arg(&file)
        .assert()
        .success();
    let file = phone_settings_manifest(
        &home,
        "  storageConfiguration:\n    status: IN_COUNTRY_STORAGE_DISABLED\n  userIdentityChange:\n    enableIdentityKeyCheck: false\n",
    );
    paloma(Some(&graph), &home)
        .args(["diff", "-f"])
        .arg(&file)
        .assert()
        .code(1);
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&file)
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::contains("IN_COUNTRY_STORAGE_DISABLED"));
    assert!(
        graph
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.method.as_str() == "GET")
    );
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&file)
        .assert()
        .failure()
        .stderr(predicate::str::contains("--yes"));
    for body in [
        json!({"storage_configuration":{"status":"IN_COUNTRY_STORAGE_DISABLED"}}),
        json!({"user_identity_change":{"enable_identity_key_check":false}}),
    ] {
        Mock::given(method("POST"))
            .and(path("/v26.0/111/settings"))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
            .expect(1)
            .mount(&graph)
            .await;
    }
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&file)
        .arg("--yes")
        .assert()
        .success()
        .stderr(predicate::str::contains("configured"));
}

#[tokio::test(flavor = "multi_thread")]
async fn phone_settings_reject_invalid_specs_before_any_read_and_confirm_identity_only_changes() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for spec in [
        "  payloadEncryption: {}\n",
        "  storageConfiguration:\n    status: IN_COUNTRY_STORAGE_ENABLED\n",
        "  userIdentityChange:\n    enableIdentityKeyCheck: nope\n",
    ] {
        let file = phone_settings_manifest(&home, spec);
        paloma(Some(&graph), &home)
            .args(["apply", "-f"])
            .arg(&file)
            .arg("--yes")
            .assert()
            .code(5)
            .stdout("");
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
    Mock::given(method("GET"))
        .and(path("/v26.0/111/settings"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"user_identity_change":{"enable_identity_key_check":false}})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let file = phone_settings_manifest(
        &home,
        "  userIdentityChange:\n    enableIdentityKeyCheck: true\n",
    );
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&file)
        .assert()
        .failure()
        .stderr(predicate::str::contains("--yes"));
    assert!(
        graph
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.method.as_str() == "GET")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn number_manifest_search_visibility_is_idempotent_and_pin_diagnostics_are_redacted() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("number.yaml");
    Mock::given(method("GET"))
        .and(path("/v26.0/111"))
        .and(query_param("fields", "search_visibility"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"search_visibility":"VISIBLE"})),
        )
        .mount(&graph)
        .await;
    std::fs::write(
        &file,
        "apiVersion: paloma/v1\nkind: Number\nspec:\n  searchVisibility: VISIBLE\n",
    )
    .unwrap();
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&file)
        .assert()
        .success()
        .stderr(predicate::str::contains("unchanged"));
    Mock::given(method("GET"))
        .and(path("/v26.0/111"))
        .and(query_param(
            "fields",
            "search_visibility,verified_name,new_display_name,new_name_status",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"search_visibility":"VISIBLE","verified_name":"Old"})),
        )
        .mount(&graph)
        .await;
    std::fs::write(&file,"apiVersion: paloma/v1\nkind: Number\nspec:\n  pin: '817263'\n  newDisplayName: Acme\n  searchVisibility: HIDDEN\n").unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&file)
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("817263"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("817263"));
    Mock::given(method("POST"))
        .and(path("/v26.0/111"))
        .and(body_json(
            json!({"pin":"817263","new_display_name":"Acme","search_visibility":"HIDDEN"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
        .arg(&file)
        .arg("--yes")
        .assert()
        .success();
}

fn write_manifest(home: &TempDir, text: &str) -> std::path::PathBuf {
    let file = home.path().join("apply.yaml");
    std::fs::write(&file, text).unwrap();
    file
}

async fn mock_phone_settings(graph: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/v26.0/111/settings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "storage_configuration": {"status": "default"},
            "user_identity_change": {"enable_identity_key_check": false},
        })))
        .mount(graph)
        .await;
}

#[tokio::test]
async fn apply_refuses_a_later_confirmation_before_writing_anything() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    mock_phone_settings(&graph).await;
    Mock::given(method("GET"))
        .and(path("/v26.0/222"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"name": "Old"})))
        .mount(&graph)
        .await;
    let file = write_manifest(
        &home,
        "apiVersion: paloma/v1\nkind: Account\nspec:\n  name: New\n---\n\
         apiVersion: paloma/v1\nkind: PhoneSettings\nspec:\n  userIdentityChange:\n    enableIdentityKeyCheck: true\n",
    );
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["apply", "--file"])
        .arg(&file)
        .assert()
        .code(5)
        .stderr(predicate::str::contains("refusing to"));
    let requests = graph.received_requests().await.unwrap();
    assert!(
        requests.iter().all(|r| r.method.as_str() == "GET"),
        "a write was sent before the refusal"
    );
}

#[tokio::test]
async fn apply_confirms_a_commerce_manifest_that_disables_the_cart() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path("/v26.0/111/whatsapp_commerce_settings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"data":[{"id":"9","is_cart_enabled":true,"is_catalog_visible":true}]}),
        ))
        .mount(&graph)
        .await;
    let file = write_manifest(
        &home,
        "apiVersion: paloma/v1\nkind: Commerce\nspec:\n  isCartEnabled: false\n",
    );
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["apply", "--file"])
        .arg(&file)
        .assert()
        .code(5)
        .stderr(predicate::str::contains("disable shopping cart"));
    let requests = graph.received_requests().await.unwrap();
    assert!(requests.iter().all(|r| r.method.as_str() == "GET"));
}

#[tokio::test]
async fn apply_reports_what_a_failed_multi_request_plan_left_behind() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    mock_phone_settings(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/111/settings"))
        .and(body_string_contains("user_identity_change"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path("/v26.0/111/settings"))
        .and(body_string_contains("storage_configuration"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({"error":{"message":"number is registered","code":100}})),
        )
        .mount(&graph)
        .await;
    let file = write_manifest(
        &home,
        "apiVersion: paloma/v1\nkind: PhoneSettings\nspec:\n  userIdentityChange:\n    enableIdentityKeyCheck: true\n  storageConfiguration:\n    status: IN_COUNTRY_STORAGE_ENABLED\n    dataLocalizationRegion: IN\n",
    );
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["apply", "--yes", "--file"])
        .arg(&file)
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "partly applied; applied: user_identity_change; failed: storage_configuration; not attempted: none",
        ));
}

#[tokio::test]
async fn phone_settings_previews_each_request_and_flags_an_unreadable_identity_check() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path("/v26.0/111/settings"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"storage_configuration":{"status":"default"}})),
        )
        .mount(&graph)
        .await;
    let file = write_manifest(
        &home,
        "apiVersion: paloma/v1\nkind: PhoneSettings\nspec:\n  userIdentityChange:\n    enableIdentityKeyCheck: true\n  storageConfiguration:\n    status: IN_COUNTRY_STORAGE_DISABLED\n",
    );
    let out = paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["apply", "--dry-run", "--file"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success());
    // Storage already matches, so only the identity request is planned and previewed.
    let preview: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        preview,
        json!({"user_identity_change":{"enable_identity_key_check":true}})
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("not returned by Graph"));
}

#[tokio::test]
async fn number_manifest_does_not_resubmit_a_pending_display_name() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path("/v26.0/111"))
        .and(query_param(
            "fields",
            "verified_name,new_display_name,new_name_status",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "verified_name":"Old","new_display_name":"Acme","new_name_status":"PENDING_REVIEW"
        })))
        .mount(&graph)
        .await;
    let file = write_manifest(
        &home,
        "apiVersion: paloma/v1\nkind: Number\nspec:\n  newDisplayName: Acme\n",
    );
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["apply", "--file"])
        .arg(&file)
        .assert()
        .success()
        .stderr(predicate::str::contains("unchanged"));
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["diff", "--file"])
        .arg(&file)
        .assert()
        .success();
    let requests = graph.received_requests().await.unwrap();
    assert!(requests.iter().all(|r| r.method.as_str() == "GET"));
}

#[tokio::test]
async fn qr_export_keeps_rows_that_would_fail_input_validation() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path("/v26.0/111/message_qrdls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[
            {"code":"SHORT","prefilled_message":"Hi"},
            {"code":"WOMVT6TJ2BP7A1","prefilled_message":"Hello"},
        ]})))
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["export", "qrcode", "--minimal"])
        .assert()
        .success()
        .stdout(predicate::str::contains("SHORT").and(predicate::str::contains("WOMVT6TJ2BP7A1")));
}

#[tokio::test]
async fn number_manifest_round_trips_a_non_visible_number() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path("/v26.0/111"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"id":"111","verified_name":"Acme","search_visibility":"NON_VISIBLE"}),
        ))
        .mount(&graph)
        .await;
    let out = paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["export", "number", "--minimal"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("searchVisibility: HIDDEN"), "{text}");
    let file = write_manifest(&home, &text);
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["diff", "--file"])
        .arg(&file)
        .assert()
        .success();
    let requests = graph.received_requests().await.unwrap();
    assert!(requests.iter().all(|r| r.method.as_str() == "GET"));
}

#[tokio::test]
async fn apply_refuses_edits_meta_forbids_while_diff_still_reports_them() {
    for (status, category, body, reason) in [
        (
            "APPROVED",
            "MARKETING",
            "Your order shipped.",
            "won't change its category",
        ),
        ("PENDING", "UTILITY", "A new body.", "can be edited"),
    ] {
        let graph = MockServer::start().await;
        mock_notice(&graph, "TEXT", status).await;
        let home = TempDir::new().unwrap();
        let file = shorthand_manifest(&home, category, body);
        for extra in [&[][..], &["--dry-run"][..]] {
            paloma(Some(&graph), &home)
                .args(["apply", "--yes", "-f"])
                .arg(&file)
                .args(extra)
                .assert()
                .code(5)
                .stdout("")
                .stderr(predicate::str::contains(reason));
        }
        paloma(Some(&graph), &home)
            .args(["diff", "-f"])
            .arg(&file)
            .assert()
            .code(1)
            .stderr(predicate::str::contains("drift"));
        assert!(
            graph
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|r| r.method.as_str() == "GET"),
            "{status}"
        );
    }
}

#[tokio::test]
async fn apply_writes_nothing_for_any_document_when_one_edit_is_refused() {
    let graph = MockServer::start().await;
    mock_notice(&graph, "TEXT", "APPROVED").await;
    Mock::given(method("GET"))
        .and(path("/v26.0/222"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"name":"Old"})))
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = write_manifest(
        &home,
        "apiVersion: paloma/v1\nkind: Account\nspec:\n  name: New\n---\napiVersion: paloma/v1\nkind: Template\nspec:\n  name: notice\n  language: en_US\n  category: MARKETING\n  body: \"Your order shipped.\"\n",
    );
    paloma(Some(&graph), &home)
        .args(["apply", "--yes", "-f"])
        .arg(&file)
        .assert()
        .code(5);
    assert!(
        graph
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.method.as_str() == "GET")
    );
}

/// A shorthand template is checked like `template create` only when it would be sent for
/// review: an in-sync template is never rechecked, so a manifest that worked keeps working.
#[tokio::test(flavor = "multi_thread")]
async fn template_shorthand_is_checked_only_when_it_would_be_submitted() {
    let home = TempDir::new().unwrap();
    let live = |body: &str| {
        json!({ "data": [{ "id": "tpl-9", "name": "notice", "language": "en_US",
            "status": "APPROVED", "category": "UTILITY",
            "components": [{ "type": "BODY", "text": body }] }] })
    };
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/message_templates")))
        .respond_with(ResponseTemplate::new(200).set_body_json(live("Hi {{1}}")))
        .mount(&graph)
        .await;
    // In sync, with no example: fine, as before.
    let same = shorthand_manifest(&home, "UTILITY", "Hi {{1}}");
    paloma(Some(&graph), &home)
        .args(["diff", "-f"])
        .arg(&same)
        .assert()
        .code(0);
    // A changed body would go to review without an example for {{1}}: refused up front.
    let changed = shorthand_manifest(&home, "UTILITY", "Bye {{1}}");
    paloma(Some(&graph), &home)
        .args(["diff", "-f"])
        .arg(&changed)
        .assert()
        .code(5)
        .stderr(predicate::str::contains(
            "example has 0 value(s) for 1 variable(s)",
        ));

    // A new template is checked too, and the hint names what a manifest can do.
    let empty = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/message_templates")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [] })))
        .mount(&empty)
        .await;
    let named = shorthand_manifest(&home, "UTILITY", "Hi {{first_name}}");
    paloma(Some(&empty), &home)
        .args(["diff", "-f"])
        .arg(&named)
        .assert()
        .code(5)
        .stderr(
            predicate::str::contains("isn't positional")
                .and(predicate::str::contains("--parameter-format").not()),
        );
}

/// A phone-scoped document with no phone number id anywhere fails before any request,
/// naming where to set one, instead of reading an empty-id path.
#[tokio::test(flavor = "multi_thread")]
async fn a_phone_scoped_document_without_a_phone_number_id_fails_before_reading() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("calling.yaml");
    std::fs::write(
        &file,
        "apiVersion: paloma/v1\nkind: Calling\nspec:\n  status: ENABLED\n",
    )
    .unwrap();
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["diff", "-f"])
        .arg(&file)
        .assert()
        .code(3)
        .stderr(predicate::str::contains("no phone number id"));
    assert!(graph.received_requests().await.unwrap().is_empty());
}
