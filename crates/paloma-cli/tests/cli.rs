//! Contract tests: run the real `paloma` binary against a stubbed Graph API and assert
//! what users are promised: exit codes, what lands on stdout vs stderr, and the exact
//! request sent (or printed under `--dry-run`).

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{Value, json};
use tempfile::TempDir;
use wiremock::matchers::{body_json, body_string_contains, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PHONE: &str = "111";
const WABA: &str = "222";
const TOKEN: &str = "EAAtest";

/// A `paloma` command isolated from the developer's machine: temp config, env token,
/// no colors, Graph API pointed at `graph`.
fn paloma(graph: Option<&MockServer>, home: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("paloma").expect("binary builds");
    cmd.env_clear()
        .env("PALOMA_CONFIG", home.path().join("config.toml"))
        .env("PALOMA_TOKEN", TOKEN)
        .env("PALOMA_PHONE_NUMBER_ID", PHONE)
        .env("PALOMA_WABA_ID", WABA)
        .env("NO_COLOR", "1")
        .env("HOME", home.path());
    if let Some(g) = graph {
        cmd.env("PALOMA_GRAPH_URL", g.uri());
    }
    cmd
}

fn stdout_json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

#[test]
fn dry_run_prints_exact_request_without_network() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args([
            "send",
            "text",
            "+15551234567",
            "hi",
            "--reply-to",
            "wamid.X",
            "--dry-run",
            "-o",
            "json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("dry-run: no request sent"));
    let v = stdout_json(&out);
    insta::assert_json_snapshot!(v, @r#"
    {
      "method": "POST",
      "url": "https://graph.facebook.com/v26.0/111/messages",
      "body": {
        "messaging_product": "whatsapp",
        "recipient_type": "individual",
        "to": "+15551234567",
        "type": "text",
        "context": {
          "message_id": "wamid.X"
        },
        "text": {
          "preview_url": false,
          "body": "hi"
        }
      }
    }
    "#);
}

#[test]
fn validation_fails_before_any_request() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(["send", "text", "12", "hi"])
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("not a valid E.164"));
    paloma(None, &home)
        .args([
            "send",
            "interactive",
            "button",
            "15551234567",
            "--body",
            "x",
            "--button",
            "a",
            "--button",
            "b",
            "--button",
            "c",
            "--button",
            "d",
        ])
        .assert()
        .code(5);
}

#[test]
fn usage_errors_exit_2_and_stubs_exit_1() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(["send", "image", "15551234567"])
        .assert()
        .code(2);
    paloma(None, &home)
        .args(["webhook", "listen"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("planned for v1.0"));
}

#[test]
fn missing_context_is_an_auth_error_naming_the_fix() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["send", "text", "15551234567", "hi", "--dry-run"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("--phone-number-id"));
}

#[tokio::test(flavor = "multi_thread")]
async fn send_text_posts_with_bearer_and_quiet_prints_wamid() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": "15551234567",
            "type": "text",
            "text": { "preview_url": false, "body": "hello" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messaging_product": "whatsapp",
            "contacts": [{ "input": "15551234567", "wa_id": "15551234567" }],
            "messages": [{ "id": "wamid.ABC" }]
        })))
        .expect(1)
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["send", "text", "15551234567", "hello", "-q"])
        .assert()
        .success()
        .stdout("wamid.ABC\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn graph_errors_map_to_exit_codes() {
    let cases = [
        (
            401,
            json!({"error": {"message": "expired", "code": 190}}),
            3,
            "auth error",
        ),
        (
            429,
            json!({"error": {"message": "slow down", "code": 4}}),
            4,
            "rate limited",
        ),
        (
            400,
            json!({"error": {"message": "re-engage", "code": 131_047}}),
            6,
            "24-hour",
        ),
        (
            400,
            json!({"error": {"message": "bad", "code": 100, "error_data": {"details": "why"}}}),
            1,
            "bad: why",
        ),
    ];
    for (status, body, code, needle) in cases {
        let graph = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&graph)
            .await;
        let home = TempDir::new().unwrap();
        paloma(Some(&graph), &home)
            .args(["send", "text", "15551234567", "hi"])
            .assert()
            .code(code)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(needle));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn template_list_all_follows_paging_next_not_cursor() {
    let graph = MockServer::start().await;
    let p = format!("/v26.0/{WABA}/message_templates");
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param("after", "C1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "name": "b", "status": "PENDING" }],
            "paging": { "cursors": { "after": "C2" } }
        })))
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(&p))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "name": "a", "status": "APPROVED" }],
            "paging": { "cursors": { "after": "C1" }, "next": "https://graph/next" }
        })))
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["template", "list", "--all", "-o", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        stdout_json(&out),
        json!({ "data": [{ "name": "a", "status": "APPROVED" }, { "name": "b", "status": "PENDING" }] })
    );

    // A single page warns on stderr that more exist, and renders a column table.
    paloma(Some(&graph), &home)
        .args(["template", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("NAME").and(predicate::str::contains("APPROVED")))
        .stderr(predicate::str::contains("--after C1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn media_upload_is_multipart() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/media")))
        .and(body_string_contains("name=\"messaging_product\""))
        .and(body_string_contains("image/png"))
        .and(body_string_contains("PNGDATA"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "MEDIA1" })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload"])
        .arg(&file)
        .arg("-q")
        .assert()
        .success()
        .stdout("MEDIA1\n");
}

#[test]
fn config_set_get_use_list_round_trip() {
    let home = TempDir::new().unwrap();
    let run = |args: &[&str]| {
        paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(args)
            .assert()
    };
    run(&["-p", "work", "config", "set", "phone_number_id", "999"]).success();
    run(&["-p", "work", "config", "set", "token_ref", "EAAplain"])
        .success()
        .stderr(predicate::str::contains("plaintext"));
    run(&["config", "use", "work"]).success();
    run(&["config", "get", "phone_number_id", "-q"])
        .success()
        .stdout("999\n");
    run(&["config", "list"])
        .success()
        .stdout(predicate::str::contains("<plaintext:redacted>"));
    run(&["config", "use", "ghost"]).code(5);
    run(&["config", "get", "nonsense"]).code(2);
}

#[test]
fn webhook_fields_available_is_offline() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env_remove("PALOMA_TOKEN")
        .args(["webhook", "fields", "available", "-q"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("messages\ncalls\n"));
}

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
        .args(["apply", "-f"])
        .arg(&file)
        .assert()
        .success()
        .stderr(predicate::str::contains("Calling/111 configured"));
}

#[tokio::test(flavor = "multi_thread")]
async fn flow_validation_errors_print_response_and_exit_5() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}/flows")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "F1",
            "success": true,
            "validation_errors": [{ "error": "INVALID_PROPERTY_VALUE", "message": "bad", "line_start": 3, "column_start": 7 }]
        })))
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args([
            "flow",
            "create",
            "--name",
            "signup",
            "--category",
            "SIGN_UP",
            "-q",
        ])
        .assert()
        .code(5)
        .stdout("F1\n")
        .stderr(predicate::str::contains(
            "line 3:7 INVALID_PROPERTY_VALUE: bad",
        ));
}

#[test]
fn destructive_actions_refuse_without_tty_or_yes() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(["number", "deregister"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("pass --yes"));
}

#[test]
fn export_template_is_offline_and_annotated() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env_remove("PALOMA_TOKEN")
        .args(["export", "calling", "--template"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("apiVersion: paloma/v1")
                .and(predicate::str::contains("phoneNumberId: <phone-number-id>"))
                .and(predicate::str::contains("  # sip:")),
        );
}

#[test]
fn secret_dry_runs_are_redacted_in_every_output_format() {
    let home = TempDir::new().unwrap();
    for format in ["table", "json", "yaml", "raw"] {
        for args in [
            vec!["number", "register", "--pin", "825194"],
            vec!["number", "set-pin", "--pin", "825194"],
            vec![
                "webhook",
                "subscribe",
                "--callback-url",
                "https://example.test/hook",
                "--verify-token",
                "VERIFY_SENTINEL",
            ],
            vec![
                "webhook",
                "fields",
                "set",
                "messages",
                "--callback-url",
                "https://example.test/hook",
                "--verify-token",
                "VERIFY_SENTINEL",
            ],
        ] {
            let out = paloma(None, &home)
                .env("PALOMA_APP_SECRET", "APP_SECRET_SENTINEL")
                .args(args)
                .args(["--app-id", "333", "--dry-run", "-vv", "-o", format])
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            for secret in ["825194", "VERIFY_SENTINEL", "APP_SECRET_SENTINEL", TOKEN] {
                assert!(
                    !combined.contains(secret),
                    "secret leaked in {format}: {combined}"
                );
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn pin_reaches_api_while_verbose_diagnostics_are_redacted() {
    let graph = MockServer::start().await;
    let response = json!({"success":true, "access_token":"RESPONSE_SENTINEL", "paging":{"next":"https://graph.test/next?access_token=PAGING_SENTINEL"}});
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}")))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .and(body_json(json!({"pin":"825194"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["number", "set-pin", "--pin", "825194", "-vv", "-o", "raw"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(stdout_json(&out), response);
    let stderr = String::from_utf8_lossy(&out.stderr);
    for secret in ["825194", TOKEN, "RESPONSE_SENTINEL", "PAGING_SENTINEL"] {
        assert!(!stderr.contains(secret), "secret in stderr: {stderr}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn verification_query_reaches_api_but_not_verbose_logs() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v26.0/333/subscriptions"))
        .and(query_param("verify_token", "VERIFY_SENTINEL"))
        .and(header("authorization", "Bearer 333|APP_SECRET_SENTINEL"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .env("PALOMA_APP_SECRET", "APP_SECRET_SENTINEL")
        .args([
            "webhook",
            "fields",
            "set",
            "messages",
            "--callback-url",
            "https://example.test/hook",
            "--verify-token",
            "VERIFY_SENTINEL",
            "--app-id",
            "333",
            "--yes",
            "-vv",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    for secret in ["VERIFY_SENTINEL", "APP_SECRET_SENTINEL", TOKEN] {
        assert!(!stderr.contains(secret), "secret in stderr: {stderr}");
    }
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
            "category": "MARKETING",
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
        .args(["apply", "-f"])
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
            "category": "UTILITY",
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
        .args(["apply", "-f"])
        .arg(&file)
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
/// Meta only allows category edits on REJECTED or PAUSED templates
/// (`templates/template-management.md`), so the fixture is a rejected one.
async fn template_shorthand_category_change_keeps_every_live_component() {
    let graph = MockServer::start().await;
    mock_notice(&graph, "TEXT", "REJECTED").await;
    let live = live_notice("TEXT", "REJECTED");
    Mock::given(method("POST"))
        .and(path("/v26.0/tpl-2"))
        .and(body_json(json!({
            "category": "MARKETING",
            "components": live["data"][0]["components"]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = shorthand_manifest(&home, "MARKETING", "Your order shipped.");
    paloma(Some(&graph), &home)
        .args(["apply", "-f"])
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

#[tokio::test(flavor = "multi_thread")]
async fn template_components_file_preserves_payload_and_redacts_dry_runs() {
    let graph = MockServer::start().await;
    let components = json!([
        {"type":"header", "parameters":[{"type":"image", "image":{"link":"https://example.test/header.jpg?access_token=MEDIA_SENTINEL"}}]},
        {"type":"body", "parameters":[
            {"type":"text", "parameter_name":"customer_name", "text":"Ana"},
            {"type":"currency", "currency":{"fallback_value":"$10.00", "code":"USD", "amount_1000":10000}},
            {"type":"date_time", "date_time":{"fallback_value":"October 26"}}
        ]},
        {"type":"button", "sub_type":"url", "index":"0", "parameters":[{"type":"text", "text":"order-42"}]}
    ]);
    let expected = json!({"messaging_product":"whatsapp", "recipient_type":"individual", "to":"15551234567", "type":"template", "template":{
        "name":"order_ready", "language":{"code":"en_US"}, "components":components
    }});
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(&expected))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.TEMPLATE"}]})),
        )
        .expect(2)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("components.json");
    for document in [components.clone(), json!({"components":components})] {
        std::fs::write(&file, document.to_string()).unwrap();
        for format in ["json", "yaml", "table", "raw"] {
            let out = paloma(Some(&graph), &home)
                .args([
                    "send",
                    "template",
                    "15551234567",
                    "order_ready",
                    "--lang",
                    "en_US",
                    "--components-file",
                ])
                .arg(&file)
                .args(["--dry-run", "-o", format])
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(!stdout.contains("MEDIA_SENTINEL"));
            assert!(stdout.contains("order-42"));
            assert!(String::from_utf8_lossy(&out.stderr).contains("dry-run"));
            if format == "raw" {
                let rendered = stdout_json(&out);
                assert_eq!(
                    rendered["template"]["components"][1],
                    expected["template"]["components"][1]
                );
            }
        }
        // A real send per file shape: the mock's exact body match covers both.
        let out = paloma(Some(&graph), &home)
            .args([
                "send",
                "template",
                "15551234567",
                "order_ready",
                "--lang",
                "en_US",
                "--components-file",
            ])
            .arg(&file)
            .arg("-q")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), "wamid.TEMPLATE\n");
        assert!(out.stderr.is_empty());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn template_text_flags_accept_values_starting_with_a_hyphen() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product":"whatsapp", "recipient_type":"individual", "to":"15551234567",
            "type":"template", "template":{
                "name":"sale", "language":{"code":"en_US"}, "components":[
                    {"type":"header", "parameters":[{"type":"text", "text":"-50% today"}]},
                    {"type":"body", "parameters":[{"type":"text", "text":"-5"}]}
                ]
            }
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.SALE"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            "15551234567",
            "sale",
            "--lang",
            "en_US",
            "--header-text",
            "-50% today",
            "--body-var",
            "-5",
            "-q",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "wamid.SALE\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn messaging_account_flag_is_sent_on_the_message_and_quiet_still_prints_the_id() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product":"whatsapp", "recipient_type":"individual", "to":"15551234567",
            "type":"text", "text":{"preview_url":false, "body":"hi"},
            "messaging_account_id":"777"
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.BILLED"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["send", "text", "15551234567", "hi"])
        .args(["--messaging-account-id", "777", "-q"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "wamid.BILLED\n");
}

#[test]
fn messaging_account_resolves_flag_then_env_then_profile() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(["config", "set", "messaging_account_id", "111"])
        .assert()
        .success();
    let account = |env: Option<&str>, flag: Option<&str>| {
        let mut cmd = paloma(None, &home);
        cmd.args([
            "send",
            "text",
            "15551234567",
            "hi",
            "--dry-run",
            "-o",
            "raw",
        ]);
        if let Some(value) = env {
            cmd.env("PALOMA_MESSAGING_ACCOUNT_ID", value);
        }
        if let Some(value) = flag {
            cmd.args(["--messaging-account-id", value]);
        }
        stdout_json(&cmd.output().unwrap())["messaging_account_id"].clone()
    };
    assert_eq!(account(None, None), json!("111"));
    assert_eq!(account(Some("222"), None), json!("222"));
    assert_eq!(account(Some("222"), Some("333")), json!("333"));
}

#[test]
fn messaging_account_is_absent_unless_set_and_only_goes_on_messages() {
    let home = TempDir::new().unwrap();
    let raw_body = |args: &[&str], stdin: Option<&str>| {
        let mut cmd = paloma(None, &home);
        cmd.args(args).args(["--dry-run", "-o", "raw"]);
        if let Some(input) = stdin {
            cmd.write_stdin(input);
        }
        stdout_json(&cmd.output().unwrap())
    };
    let text = raw_body(&["send", "text", "15551234567", "hi"], None);
    assert!(text.get("messaging_account_id").is_none());

    let account = ["--messaging-account-id", "777"];
    let read = raw_body(
        &[&["message", "read", "wamid.X"], &account[..]].concat(),
        None,
    );
    assert!(read.get("messaging_account_id").is_none());

    let raw_args = [&["send", "raw", "--stdin"], &account[..]].concat();
    let own = r#"{"to":"1","type":"text","text":{"body":"x"},"messaging_account_id":"999"}"#;
    assert_eq!(
        raw_body(&raw_args, Some(own))["messaging_account_id"],
        json!("999")
    );
    let unset = r#"{"to":"1","type":"text","text":{"body":"x"}}"#;
    assert_eq!(
        raw_body(&raw_args, Some(unset))["messaging_account_id"],
        json!("777")
    );
    let alias = r#"{"to":"1","type":"text","text":{"body":"x"},"paid_messaging_account_id":"999"}"#;
    let body = raw_body(&raw_args, Some(alias));
    assert!(body.get("messaging_account_id").is_none());
    assert_eq!(body["paid_messaging_account_id"], json!("999"));
    let status = r#"{"messaging_product":"whatsapp","status":"read","message_id":"wamid.X"}"#;
    assert!(
        raw_body(&raw_args, Some(status))
            .get("messaging_account_id")
            .is_none()
    );
}

#[test]
fn messaging_account_is_shown_on_the_media_message_in_a_file_dry_run() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("photo.jpg");
    std::fs::write(&file, b"jpeg").unwrap();
    let out = paloma(None, &home)
        .args(["send", "image", "15551234567", "--file"])
        .arg(&file)
        .args(["--messaging-account-id", "777", "--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let requests = stdout_json(&out);
    assert_eq!(requests[1]["body"]["messaging_account_id"], json!("777"));
    assert!(requests[0].to_string().contains("/media"));
    assert!(!requests[0].to_string().contains("messaging_account_id"));
}

#[test]
fn template_blank_body_var_exits_5_without_a_request() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args([
            "send",
            "template",
            "15551234567",
            "sale",
            "--lang",
            "en_US",
            "--body-var",
            "   ",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(5));
    assert!(out.stdout.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn template_text_flags_send_named_or_positional_body_with_header() {
    let graph = MockServer::start().await;
    for (args, parameters) in [
        (
            vec!["--var", "customer_name=Ana=VIP", "--var", "order_number=42"],
            json!([
                {"type":"text", "parameter_name":"customer_name", "text":"Ana=VIP"},
                {"type":"text", "parameter_name":"order_number", "text":"42"}
            ]),
        ),
        (
            vec!["--body-var", "Ana", "--body-var", "42"],
            json!([
                {"type":"text", "text":"Ana"}, {"type":"text", "text":"42"}
            ]),
        ),
    ] {
        let positional = args[0] == "--body-var";
        let mut components = Vec::new();
        if positional {
            components.push(
                json!({"type":"header", "parameters":[{"type":"text", "text":"Order update"}]}),
            );
        }
        components.push(json!({"type":"body", "parameters":parameters}));
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/messages")))
            .and(body_json(json!({"messaging_product":"whatsapp", "recipient_type":"individual", "to":"15551234567", "type":"template", "template":{
                "name":"order_ready", "language":{"code":"en_US"}, "components":components
            }})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.FLAGS"}]})))
            .expect(1).mount(&graph).await;
        let home = TempDir::new().unwrap();
        let mut command = paloma(Some(&graph), &home);
        command.args([
            "send",
            "template",
            "15551234567",
            "order_ready",
            "--lang",
            "en_US",
        ]);
        if positional {
            command.args(["--header-text", "Order update"]);
        }
        command
            .args(args)
            .arg("-q")
            .assert()
            .success()
            .stdout("wamid.FLAGS\n")
            .stderr("");
    }
}

#[test]
fn template_parameter_errors_fail_before_context_resolution() {
    let home = TempDir::new().unwrap();
    // Any attempt to resolve context would fail with a config error instead of exit 5.
    std::fs::write(home.path().join("config.toml"), "broken = [").unwrap();
    for args in [
        vec!["--var", "missing_equals"],
        vec!["--var", "=Ana"],
        vec!["--var", "Name=Ana"],
        vec!["--var", "first-name=Ana"],
        vec!["--var", "name=   "],
        vec!["--var", "name=Ana", "--var", "name=Bob"],
        vec!["--header-text", "   "],
    ] {
        paloma(None, &home)
            .args([
                "send",
                "template",
                "15551234567",
                "order_ready",
                "--lang",
                "en_US",
            ])
            .args(args)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty());
    }
    let file = home.path().join("bad-components.json");
    for document in [
        "not json",
        "{}",
        "null",
        "[1]",
        "[{\"type\":\"\"}]",
        "[{\"type\":\"body\",\"parameters\":{}}]",
        "[{\"type\":\"body\",\"parameters\":[null]}]",
        "{\"components\":[],\"to\":\"other\"}",
    ] {
        std::fs::write(&file, document).unwrap();
        paloma(None, &home)
            .args([
                "send",
                "template",
                "15551234567",
                "order_ready",
                "--lang",
                "en_US",
                "--components-file",
            ])
            .arg(&file)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty());
    }
    paloma(None, &home)
        .args([
            "send",
            "template",
            "15551234567",
            "order_ready",
            "--lang",
            "en_US",
            "--components-file",
        ])
        .arg(home.path().join("missing.json"))
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty());
}

#[test]
fn template_parameter_sources_conflict_as_usage_errors() {
    let home = TempDir::new().unwrap();
    for args in [
        vec!["--components-file", "unused.json", "--body-var", "Ana"],
        vec!["--components-file", "unused.json", "--var", "name=Ana"],
        vec!["--components-file", "unused.json", "--header-text", "Hi"],
        vec!["--body-var", "Ana", "--var", "name=Ana"],
        vec!["--var", "name=Ana", "--header-text", "Hi"],
    ] {
        paloma(None, &home)
            .args([
                "send",
                "template",
                "15551234567",
                "order_ready",
                "--lang",
                "en_US",
            ])
            .args(args)
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty());
    }
}
