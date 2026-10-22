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
fn usage_errors_exit_2_and_missing_verify_token_exits_5() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(["send", "image", "15551234567"])
        .assert()
        .code(2);
    paloma(None, &home)
        .args(["webhook", "listen"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("PALOMA_VERIFY_TOKEN"));
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

fn debug_token_body(is_valid: bool) -> Value {
    json!({
        "data": {
            "app_id": "999",
            "type": "SYSTEM_USER",
            "application": "Lucky Shrub",
            "expires_at": 0,
            "is_valid": is_valid,
            "scopes": ["whatsapp_business_management", "whatsapp_business_messaging"]
        }
    })
}

async fn mount_debug_token(graph: &MockServer, response: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path("/v26.0/debug_token"))
        .and(query_param("input_token", TOKEN))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(response)
        .expect(1)
        .mount(graph)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_status_scopes_shows_the_token_without_a_phone_number() {
    let body = debug_token_body(true);
    let graph = MockServer::start().await;
    mount_debug_token(&graph, ResponseTemplate::new(200).set_body_json(&body)).await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .env_remove("PALOMA_WABA_ID")
        .args(["auth", "status", "--scopes", "-o", "json", "-vv"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(stdout_json(&out), body);
    assert!(!String::from_utf8_lossy(&out.stderr).contains(TOKEN));

    for (flags, expected) in [
        (vec!["-q"], "valid"),
        (vec!["-o", "table"], "whatsapp_business_messaging"),
    ] {
        let graph = MockServer::start().await;
        mount_debug_token(&graph, ResponseTemplate::new(200).set_body_json(&body)).await;
        paloma(Some(&graph), &home)
            .args(["auth", "status", "--scopes"])
            .args(&flags)
            .assert()
            .success()
            .stdout(predicate::str::contains(expected).and(predicate::str::contains(TOKEN).not()));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_status_scopes_exits_3_when_the_token_is_bad() {
    let responses = [
        ResponseTemplate::new(200).set_body_json(debug_token_body(false)),
        ResponseTemplate::new(401)
            .set_body_json(json!({ "error": { "message": "expired", "code": 190 } })),
    ];
    for response in responses {
        let graph = MockServer::start().await;
        mount_debug_token(&graph, response).await;
        let home = TempDir::new().unwrap();
        paloma(Some(&graph), &home)
            .args(["auth", "status", "--scopes"])
            .assert()
            .code(3)
            .stdout(predicate::str::is_empty());
    }
}

#[test]
fn auth_status_scopes_dry_run_needs_no_token_and_shows_none() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .env_remove("PALOMA_TOKEN")
        .args(["auth", "status", "--scopes", "--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let request = stdout_json(&out);
    assert_eq!(request["method"], "GET");
    let url = request["url"].as_str().unwrap();
    assert!(url.contains("/v26.0/debug_token?input_token="), "{url}");
    assert!(!url.contains("active-token"), "{url}");
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
            json!({"error": {"message": "too fast", "code": 130_429}}),
            4,
            "rate limited (400)",
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
                    "--via",
                    "cloud",
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
                "--via",
                "cloud",
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
            "--via",
            "cloud",
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

/// A mock that answers `status` once with these headers, then falls through to what's
/// mounted after it.
async fn fail_once(graph: &MockServer, verb: &str, status: u16, retry_after: Option<&str>) {
    // Only a 429 carries a throttling code, which would decide the exit code by itself.
    let code = if status == 429 { 4 } else { 1 };
    let mut response = ResponseTemplate::new(status)
        .set_body_json(json!({"error": {"message": "slow down", "code": code}}));
    if let Some(value) = retry_after {
        response = response.insert_header("Retry-After", value);
    }
    Mock::given(method(verb))
        .respond_with(response)
        .up_to_n_times(1)
        .expect(1)
        .mount(graph)
        .await;
}

async fn mount_sent(graph: &MockServer, times: u64) {
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.R"}]})),
        )
        .expect(times)
        .mount(graph)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn write_retries_a_429_that_says_when_and_sends_only_once_more() {
    let graph = MockServer::start().await;
    fail_once(&graph, "POST", 429, Some("0")).await;
    mount_sent(&graph, 1).await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["send", "text", "15551234567", "hi", "-q"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "wamid.R\n");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("HTTP 429: retrying in 0s (attempt 2 of 3)"),
        "{stderr}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn write_never_retries_without_a_valid_or_short_retry_after() {
    for retry_after in [
        None,
        Some("soon"),
        Some("Wed, 21 Oct 2026 07:28:00 GMT"),
        Some("3600"),
    ] {
        let graph = MockServer::start().await;
        fail_once(&graph, "POST", 429, retry_after).await;
        mount_sent(&graph, 0).await;
        let home = TempDir::new().unwrap();
        paloma(Some(&graph), &home)
            .args(["send", "text", "15551234567", "hi"])
            .assert()
            .code(4)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("retrying").not());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn write_never_retries_a_server_error_even_with_retry_after() {
    let graph = MockServer::start().await;
    fail_once(&graph, "POST", 503, Some("0")).await;
    mount_sent(&graph, 0).await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["send", "text", "15551234567", "hi"])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("retrying").not());
}

#[test]
fn write_never_retries_when_the_server_is_unreachable() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env("PALOMA_GRAPH_URL", format!("http://127.0.0.1:{port}"))
        .args(["send", "text", "15551234567", "hi"])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("retrying").not());
}

#[test]
fn read_retries_when_the_server_is_unreachable_then_fails() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env("PALOMA_GRAPH_URL", format!("http://127.0.0.1:{port}"))
        .args(["number", "get"])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(
            predicate::str::contains("attempt 2 of 3")
                .and(predicate::str::contains("attempt 3 of 3")),
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn read_retries_a_server_error_and_stdout_holds_only_the_final_body() {
    let graph = MockServer::start().await;
    fail_once(&graph, "GET", 503, None).await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"id":"111"}"#))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["number", "get", "-o", "raw"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        r#"{"id":"111"}"#
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("HTTP 503: retrying in 1s"));
}

#[tokio::test(flavor = "multi_thread")]
async fn read_stops_after_three_tries_and_keeps_the_rate_limit_exit_code() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "0")
                .set_body_json(json!({"error": {"message": "slow down", "code": 4}})),
        )
        .expect(3)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["number", "get"])
        .assert()
        .code(4)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("rate limited"));
}

#[tokio::test(flavor = "multi_thread")]
async fn read_does_not_retry_a_client_error() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({"error": {"message": "bad", "code": 100}})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["number", "get"])
        .assert()
        .code(1);
}

#[tokio::test(flavor = "multi_thread")]
async fn write_retry_repeats_the_identical_request() {
    let graph = MockServer::start().await;
    fail_once(&graph, "POST", 429, Some("0")).await;
    mount_sent(&graph, 1).await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["send", "text", "15551234567", "hi"])
        .assert()
        .success();
    let requests = graph.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].url, requests[1].url);
    assert_eq!(requests[0].body, requests[1].body);
    assert!(!requests[1].body.is_empty());
    let bearer = |i: usize| requests[i].headers.get("authorization").cloned();
    assert_eq!(bearer(0), bearer(1));
    assert_eq!(
        bearer(1).unwrap().to_str().unwrap(),
        format!("Bearer {TOKEN}")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn write_retry_resends_the_whole_file_on_a_multipart_upload() {
    let graph = MockServer::start().await;
    fail_once(&graph, "POST", 429, Some("0")).await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/media")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":"m1"})))
        .expect(1)
        .mount(&graph)
        .await;
    mount_sent(&graph, 1).await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("photo.jpg");
    std::fs::write(&file, b"JPEG-BYTES-SENTINEL").unwrap();
    paloma(Some(&graph), &home)
        .args(["send", "image", "15551234567", "--file"])
        .arg(&file)
        .assert()
        .success();
    let requests = graph.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    for upload in &requests[..2] {
        assert!(upload.url.path().ends_with("/media"));
        let body = String::from_utf8_lossy(&upload.body);
        assert!(body.contains("JPEG-BYTES-SENTINEL"), "{body}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn write_stops_after_three_tries_on_repeated_429s() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "0")
                .set_body_json(json!({"error": {"message": "slow down", "code": 4}})),
        )
        .expect(3)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["send", "text", "15551234567", "hi"])
        .assert()
        .code(4)
        .stdout(predicate::str::is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_follows_the_write_rules() {
    let graph = MockServer::start().await;
    fail_once(&graph, "DELETE", 429, Some("0")).await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["block", "remove", "15551234567", "-y"])
        .assert()
        .success();

    let graph = MockServer::start().await;
    fail_once(&graph, "DELETE", 429, None).await;
    paloma(Some(&graph), &home)
        .args(["block", "remove", "15551234567", "-y"])
        .assert()
        .code(4);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_timeout_is_never_retried_for_reads_or_writes() {
    for (verb, args) in [
        ("GET", vec!["number", "get"]),
        ("POST", vec!["send", "text", "15551234567", "hi"]),
    ] {
        let graph = MockServer::start().await;
        Mock::given(method(verb))
            .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(5)))
            .expect(1)
            .mount(&graph)
            .await;
        let home = TempDir::new().unwrap();
        paloma(Some(&graph), &home)
            .args(args)
            .args(["--timeout", "200ms"])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(
                predicate::str::contains("timed out")
                    .and(predicate::str::contains("retrying").not()),
            );
    }
}

/// Mounts the media lookup, pointing at `/cdn/file` on the same server.
async fn mount_media_lookup(graph: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/v26.0/m1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"url": format!("{}/cdn/file", graph.uri())})),
        )
        .mount(graph)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn download_retries_and_writes_only_the_final_body() {
    let graph = MockServer::start().await;
    mount_media_lookup(&graph).await;
    Mock::given(method("GET"))
        .and(path("/cdn/file"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "0")
                .set_body_string("ERROR-BODY"),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path("/cdn/file"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"FILE".to_vec()))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let dest = home.path().join("out.bin");
    paloma(Some(&graph), &home)
        .args(["media", "download", "m1", "-O"])
        .arg(&dest)
        .assert()
        .success();
    assert_eq!(std::fs::read(&dest).unwrap(), b"FILE");
}

#[tokio::test(flavor = "multi_thread")]
async fn download_that_stays_rate_limited_exits_4_and_creates_no_file() {
    let graph = MockServer::start().await;
    mount_media_lookup(&graph).await;
    Mock::given(method("GET"))
        .and(path("/cdn/file"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
        .expect(3)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let dest = home.path().join("out.bin");
    paloma(Some(&graph), &home)
        .args(["media", "download", "m1", "-O"])
        .arg(&dest)
        .assert()
        .code(4)
        .stdout(predicate::str::is_empty());
    assert!(!dest.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_retry_in_the_middle_of_paging_does_not_duplicate_rows() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("after", "c1"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
        .up_to_n_times(1)
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(query_param("after", "c1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data":[{"name":"b"}],
            "paging":{"cursors":{"after":"c2"}}
        })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data":[{"name":"a"}],
            "paging":{"cursors":{"after":"c1"}, "next":"https://example.test/next"}
        })))
        .expect(1)
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
        stdout_json(&out)["data"],
        json!([{"name":"a"}, {"name":"b"}])
    );
}

/// A secret's safe input paths: the value reaches the API, without a warning or an echo.
#[tokio::test(flavor = "multi_thread")]
async fn pin_from_env_or_stdin_reaches_the_api_without_a_warning() {
    for use_stdin in [false, true] {
        let graph = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/register")))
            .and(body_json(
                json!({"messaging_product":"whatsapp", "pin":"825194"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
            .expect(1)
            .mount(&graph)
            .await;
        let home = TempDir::new().unwrap();
        let mut cmd = paloma(Some(&graph), &home);
        cmd.args(["number", "register"]);
        if use_stdin {
            cmd.arg("--pin-stdin").write_stdin("825194\n");
        } else {
            cmd.env("PALOMA_PIN", "825194");
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!stderr.contains("visible to other processes"), "{stderr}");
        assert!(!stderr.contains("825194"), "{stderr}");
        assert!(!String::from_utf8_lossy(&out.stdout).contains("825194"));
    }
}

#[test]
fn secret_flags_still_work_but_warn_without_echoing_the_value() {
    let home = TempDir::new().unwrap();
    for (args, flag, value) in [
        (
            vec!["number", "set-pin", "--pin", "825194"],
            "--pin",
            "825194",
        ),
        (
            vec![
                "webhook",
                "subscribe",
                "--callback-url",
                "https://example.test/hook",
                "--verify-token",
                "VERIFY_SENTINEL",
            ],
            "--verify-token",
            "VERIFY_SENTINEL",
        ),
    ] {
        let out = paloma(None, &home)
            .args(args)
            .args(["--dry-run"])
            .output()
            .unwrap();
        assert!(out.status.success());
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(&format!("{flag} is visible to other processes")),
            "{stderr}"
        );
        assert!(!stderr.contains(value), "{stderr}");
        assert!(!String::from_utf8_lossy(&out.stdout).contains(value));
    }
}

#[test]
fn secret_flags_are_hidden_and_their_stdin_flags_are_shown() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(["number", "register", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("--pin-stdin").and(predicate::str::contains("--pin ").not()),
        );
    paloma(None, &home)
        .args(["webhook", "subscribe", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("--verify-token-stdin")
                .and(predicate::str::contains("--verify-token ").not()),
        );
}

#[test]
fn bad_or_missing_secrets_exit_5_without_a_request() {
    let home = TempDir::new().unwrap();
    let hook = "https://example.test/hook";
    for (args, stdin, needle) in [
        (vec!["number", "register"], None, "PALOMA_PIN"),
        (vec!["number", "set-pin"], None, "--pin-stdin"),
        (
            vec!["number", "register", "--pin-stdin"],
            Some("12"),
            "PIN must be exactly 6 digits",
        ),
        (
            vec!["number", "register", "--pin-stdin"],
            Some(" \n"),
            "empty",
        ),
        (
            vec!["webhook", "subscribe", "--callback-url", hook],
            None,
            "PALOMA_VERIFY_TOKEN",
        ),
        (
            vec![
                "webhook",
                "subscribe",
                "--callback-url",
                hook,
                "--verify-token-stdin",
            ],
            Some(""),
            "empty",
        ),
        (
            vec![
                "webhook",
                "fields",
                "set",
                "messages",
                "--callback-url",
                hook,
                "--app-id",
                "3",
            ],
            None,
            "PALOMA_VERIFY_TOKEN",
        ),
        (
            vec![
                "webhook",
                "fields",
                "set",
                "messages",
                "--verify-token",
                "t",
                "--app-id",
                "3",
            ],
            None,
            "needs --callback-url",
        ),
    ] {
        let mut cmd = paloma(None, &home);
        cmd.args(&args);
        if let Some(input) = stdin {
            cmd.write_stdin(input);
        }
        cmd.assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(needle));
    }
}

#[test]
fn secret_flag_and_its_stdin_flag_conflict() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(["number", "register", "--pin", "825194", "--pin-stdin"])
        .assert()
        .code(2);
}

#[test]
fn verify_token_from_env_or_stdin_is_used_and_masked_in_a_dry_run() {
    let home = TempDir::new().unwrap();
    let args = [
        "webhook",
        "subscribe",
        "--callback-url",
        "https://example.test/hook",
        "--dry-run",
        "-o",
        "raw",
    ];
    let from_env = paloma(None, &home)
        .args(args)
        .env("PALOMA_VERIFY_TOKEN", "VERIFY_SENTINEL")
        .output()
        .unwrap();
    let from_stdin = paloma(None, &home)
        .args(args)
        .arg("--verify-token-stdin")
        .write_stdin("VERIFY_SENTINEL\n")
        .output()
        .unwrap();
    for out in [from_env, from_stdin] {
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let body = stdout_json(&out);
        assert!(body.get("verify_token").is_some(), "{body}");
        assert!(!body.to_string().contains("VERIFY_SENTINEL"));
        assert!(!String::from_utf8_lossy(&out.stderr).contains("visible to other"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn verify_token_from_env_or_stdin_reaches_the_api_exactly() {
    for use_stdin in [false, true] {
        let graph = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{WABA}/subscribed_apps")))
            .and(body_json(json!({
                "override_callback_uri":"https://example.test/hook",
                "verify_token":"VERIFY_SENTINEL"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
            .expect(1)
            .mount(&graph)
            .await;
        Mock::given(method("POST"))
            .and(path("/v26.0/333/subscriptions"))
            .and(query_param("callback_url", "https://example.test/hook"))
            .and(query_param("verify_token", "VERIFY_SENTINEL"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
            .expect(1)
            .mount(&graph)
            .await;
        let home = TempDir::new().unwrap();
        let hook = ["--callback-url", "https://example.test/hook", "--yes"];
        for verb in [
            vec!["webhook", "subscribe"],
            vec!["webhook", "fields", "set", "messages", "--app-id", "333"],
        ] {
            let mut cmd = paloma(Some(&graph), &home);
            cmd.env("PALOMA_APP_SECRET", "s").args(verb).args(hook);
            if use_stdin {
                cmd.arg("--verify-token-stdin")
                    .write_stdin("VERIFY_SENTINEL\n");
            } else {
                cmd.env("PALOMA_VERIFY_TOKEN", "VERIFY_SENTINEL");
            }
            let out = cmd.output().unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn verify_token_stdin_without_yes_refuses_before_spending_a_call() {
    let graph = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[]})))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let hook = ["--callback-url", "https://example.test/hook"];
    for verb in [
        vec!["webhook", "subscribe"],
        vec!["webhook", "fields", "set", "messages"],
    ] {
        paloma(Some(&graph), &home)
            .args(verb)
            .args(hook)
            .arg("--verify-token-stdin")
            .write_stdin("VERIFY_SENTINEL")
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("pass --yes"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fields_set_checks_the_verify_token_before_the_app_id_lookup() {
    let graph = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[]})))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["webhook", "fields", "set", "messages"])
        .args(["--callback-url", "https://example.test/hook", "--yes"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("PALOMA_VERIFY_TOKEN"));
}

#[test]
fn fields_set_ignores_a_verify_token_in_the_env_without_a_callback() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .env("PALOMA_APP_SECRET", "s")
        .env("PALOMA_VERIFY_TOKEN", "VERIFY_SENTINEL")
        .args(["webhook", "fields", "set", "messages", "--app-id", "333"])
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!String::from_utf8_lossy(&out.stdout).contains("verify_token"));
}

#[tokio::test(flavor = "multi_thread")]
async fn template_typed_flags_send_the_exact_components_in_order() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product":"whatsapp", "recipient_type":"individual", "to":"15551234567",
            "type":"template", "template":{
                "name":"order_ready", "language":{"code":"en_US"}, "components":[
                    {"type":"header", "parameters":[{"type":"image", "image":{"id":"MEDIA1"}}]},
                    {"type":"body", "parameters":[
                        {"type":"text", "parameter_name":"first_name", "text":"Ana"}
                    ]},
                    {"type":"button", "sub_type":"url", "index":"0", "parameters":[
                        {"type":"text", "text":"a=b:c"}
                    ]},
                    {"type":"button", "sub_type":"quick_reply", "index":"1", "parameters":[
                        {"type":"payload", "payload":"STOP"}
                    ]},
                    {"type":"button", "sub_type":"copy_code", "index":2, "parameters":[
                        {"type":"coupon_code", "coupon_code":"WINTER25"}
                    ]},
                    {"type":"button", "sub_type":"flow", "index":"3", "parameters":[
                        {"type":"action", "action":{"flow_token":"FLOW_SENTINEL"}}
                    ]},
                    {"type":"button", "sub_type":"CATALOG", "index":4, "parameters":[
                        {"type":"action", "action":{"thumbnail_product_retailer_id":"SKU1"}}
                    ]}
                ]
            }
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.T"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            "--via",
            "cloud",
            "15551234567",
            "order_ready",
        ])
        .args(["--lang", "en_US", "--header-image", "MEDIA1"])
        .args(["--var", "first_name=Ana"])
        .args(["--button", "0:url=a=b:c"])
        .args(["--button", "1:quick_reply=STOP"])
        .args(["--button", "2:copy_code=WINTER25"])
        .args(["--button", "3:flow=FLOW_SENTINEL"])
        .args(["--button", "4:catalog=SKU1", "-q"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "wamid.T\n");
}

#[test]
fn template_media_header_is_an_id_or_a_link_and_a_dry_run_masks_secrets() {
    let home = TempDir::new().unwrap();
    let components = |args: &[&str]| {
        let out = paloma(None, &home)
            .args([
                "send",
                "template",
                "--via",
                "cloud",
                "15551234567",
                "t",
                "--lang",
                "en",
            ])
            .args(args)
            .args(["--dry-run", "-o", "raw"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        stdout_json(&out)["template"]["components"].clone()
    };
    assert_eq!(
        components(&["--header-video", "V1"]),
        json!([{"type":"header", "parameters":[{"type":"video", "video":{"id":"V1"}}]}])
    );
    assert_eq!(
        components(&["--header-document", "https://example.test/a.pdf"]),
        json!([{"type":"header", "parameters":[
            {"type":"document", "document":{"link":"https://example.test/a.pdf"}}
        ]}])
    );
    let flow = components(&["--button", "0:flow=FLOW_SENTINEL"]);
    assert!(!flow.to_string().contains("FLOW_SENTINEL"), "{flow}");
}

#[test]
fn template_header_flags_and_the_components_file_conflict() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("c.json");
    std::fs::write(&file, "[]").unwrap();
    let file = file.to_str().unwrap();
    for args in [
        vec!["--header-image", "A", "--header-video", "B"],
        vec!["--header-video", "A", "--header-document", "B"],
        vec!["--header-text", "A", "--header-image", "B"],
        vec!["--header-image", "B", "--header-text", "A"],
        vec!["--var", "a=b", "--header-text", "A"],
        vec!["--components-file", file, "--header-image", "A"],
        vec!["--components-file", file, "--button", "0:url=a"],
    ] {
        paloma(None, &home)
            .args([
                "send",
                "template",
                "--via",
                "cloud",
                "15551234567",
                "t",
                "--lang",
                "en",
            ])
            .args(args)
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty());
    }
}

#[test]
fn template_bad_typed_flags_exit_5_without_echoing_the_value() {
    let home = TempDir::new().unwrap();
    let long_coupon = format!("0:copy_code={}", "A".repeat(21));
    for (args, needle) in [
        (vec!["--button", "SECRET"], "#1 must use INDEX:TYPE=VALUE"),
        (vec!["--button", "0:flowSECRET"], "#1 must use"),
        (vec!["--button", "0:flow:SECRET"], "#1 must use"),
        (vec!["--button", "0:flow:SECRET=x"], "#1 type must be"),
        (vec!["--button", "SECRET:flow=x"], "#1 index must be 0 to 9"),
        (vec!["--button", "10:url=a"], "#1 index must be 0 to 9"),
        (vec!["--button", "0:url=  "], "#1 value is empty"),
        (
            vec!["--button", "0:url=a", "--button", "0:flow=SECRET"],
            "#2 index 0 is already used",
        ),
        (
            vec!["--button", "0:copy_code=a", "--button", "1:copy_code=b"],
            "#2 is a second copy_code",
        ),
        (vec!["--button", long_coupon.as_str()], "over 20 characters"),
        (vec!["--header-image", "  "], "--header-image"),
        (vec!["--header-image", "ftp://SECRET/a.png"], "media id or"),
        (vec!["--header-video", "./SECRET.mp4"], "media id or"),
        (vec!["--header-document", "a SECRET"], "media id or"),
        (vec!["--header-image", "http://"], "media id or"),
        (vec!["--header-image", "https:///SECRET"], "media id or"),
    ] {
        paloma(None, &home)
            .args([
                "send",
                "template",
                "--via",
                "cloud",
                "15551234567",
                "t",
                "--lang",
                "en",
            ])
            .args(args)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(needle).and(predicate::str::contains("SECRET").not()));
    }
}

#[test]
fn template_uppercase_link_is_a_link_and_stays_masked_everywhere() {
    let home = TempDir::new().unwrap();
    let args = [
        "send",
        "template",
        "--via",
        "cloud",
        "15551234567",
        "t",
        "--lang",
        "en",
        "--header-image",
        "HTTPS://x.test/a.png?access_token=SECRET",
        "--button",
        "0:flow=SECRET",
        "--dry-run",
        "-vv",
    ];
    for format in ["table", "json", "yaml", "raw"] {
        let out = paloma(None, &home)
            .args(args)
            .args(["-o", format])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!String::from_utf8_lossy(&out.stdout).contains("SECRET"));
        assert!(!String::from_utf8_lossy(&out.stderr).contains("SECRET"));
        if format == "raw" {
            let image = &stdout_json(&out)["template"]["components"][0]["parameters"][0]["image"];
            assert!(image.get("link").is_some(), "{image}");
            assert!(image.get("id").is_none(), "{image}");
        }
    }
}

#[test]
fn template_documented_flag_combinations_work() {
    let home = TempDir::new().unwrap();
    for args in [
        vec!["--header-image", "A", "--body-var", "x"],
        vec!["--header-image", "A", "--var", "name=x"],
        vec!["--header-text", "A", "--button", "0:url=b"],
    ] {
        paloma(None, &home)
            .args([
                "send",
                "template",
                "--via",
                "cloud",
                "15551234567",
                "t",
                "--lang",
                "en",
            ])
            .args(args)
            .arg("--dry-run")
            .assert()
            .success();
    }
}

#[test]
fn template_blank_body_var_exits_5_without_a_request() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args([
            "send",
            "template",
            "--via",
            "cloud",
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
            "--via",
            "cloud",
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
                "--via",
                "cloud",
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
                "--via",
                "cloud",
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
            "--via",
            "cloud",
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
                "--via",
                "cloud",
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

#[test]
fn reply_to_quotes_the_message_on_every_send_type_that_allows_it() {
    let home = TempDir::new().unwrap();
    let contacts = home.path().join("contacts.json");
    std::fs::write(&contacts, r#"[{"name":{"formatted_name":"A"}}]"#).unwrap();
    let contacts = contacts.to_str().unwrap();
    let to = "15551234567";
    let cases: Vec<Vec<&str>> = vec![
        vec!["text", to, "hi"],
        vec!["template", "--via", "cloud", to, "order", "--lang", "en_US"],
        vec!["image", to, "--id", "9"],
        vec!["document", to, "--id", "9"],
        vec!["audio", to, "--id", "9"],
        vec!["video", to, "--id", "9"],
        vec!["sticker", to, "--id", "9"],
        vec!["location", to, "--lat", "1", "--long", "2"],
        vec!["contact", to, "--file", contacts],
        vec![
            "interactive",
            "button",
            to,
            "--body",
            "b",
            "--button",
            "y:Yes",
        ],
        vec![
            "interactive",
            "list",
            to,
            "--body",
            "b",
            "--button",
            "Open",
            "--section",
            "S:a:A",
        ],
        vec![
            "interactive",
            "cta-url",
            to,
            "--body",
            "b",
            "--display",
            "D",
            "--url",
            "https://x.test",
        ],
    ];
    for case in cases {
        let out = paloma(None, &home)
            .arg("send")
            .args(&case)
            .args(["--reply-to", "wamid.X", "--dry-run", "-o", "json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{case:?}");
        assert_eq!(
            stdout_json(&out)["body"]["context"],
            json!({ "message_id": "wamid.X" }),
            "{case:?}"
        );

        let out = paloma(None, &home)
            .arg("send")
            .args(&case)
            .args(["--dry-run", "-o", "json"])
            .output()
            .unwrap();
        assert!(
            stdout_json(&out)["body"].get("context").is_none(),
            "{case:?}"
        );
    }
}

#[test]
fn reply_to_is_refused_where_it_cannot_apply_or_is_blank() {
    let home = TempDir::new().unwrap();
    let to = "15551234567";
    // A blank id, such as an unset shell variable, must not send an unquoted message.
    let contacts = home.path().join("contacts.json");
    std::fs::write(&contacts, r#"[{"name":{"formatted_name":"A"}}]"#).unwrap();
    let blank: [&[&str]; 6] = [
        &["text", to, "hi"],
        &["template", "--via", "cloud", to, "order", "--lang", "en_US"],
        &["image", to, "--id", "9"],
        &["location", to, "--lat", "1", "--long", "2"],
        &["contact", to, "--file", contacts.to_str().unwrap()],
        &[
            "interactive",
            "button",
            to,
            "--body",
            "b",
            "--button",
            "y:Yes",
        ],
    ];
    for case in blank {
        paloma(None, &home)
            .arg("send")
            .args(case)
            .args(["--reply-to", " ", "--dry-run"])
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("--reply-to"));
    }
    let unsupported: [&[&str]; 2] = [
        &["reaction", to, "--message-id", "wamid.Y", "--emoji", "👍"],
        &["raw", "--stdin"],
    ];
    for case in unsupported {
        paloma(None, &home)
            .arg("send")
            .args(case)
            .args(["--reply-to", "wamid.X", "--dry-run"])
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("--reply-to"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn blank_reply_to_fails_before_the_upload_and_before_the_context() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/media")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "MEDIA1" })))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    for without_phone in [false, true] {
        let mut cmd = paloma(Some(&graph), &home);
        if without_phone {
            cmd.env_remove("PALOMA_PHONE_NUMBER_ID");
        }
        cmd.args(["send", "image", "15551234567", "--reply-to", " ", "--file"])
            .arg(&file)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("--reply-to"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn reply_to_on_a_template_warns_on_stderr_and_still_sends() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": "15551234567",
            "type": "template",
            "context": { "message_id": "wamid.X" },
            "template": { "name": "order", "language": { "code": "en_US" } }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "messages": [{ "id": "wamid.ABC" }] })),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            "--via",
            "cloud",
            "15551234567",
            "order",
            "--lang",
            "en_US",
        ])
        .args(["--reply-to", "wamid.X", "-q"])
        .assert()
        .success()
        .stdout("wamid.ABC\n")
        .stderr(predicate::str::contains("quoted message"));
}

#[tokio::test(flavor = "multi_thread")]
async fn voice_note_from_a_local_file_uploads_then_sends_with_voice_and_reply() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/media")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "MEDIA1" })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": "15551234567",
            "type": "audio",
            "context": { "message_id": "wamid.X" },
            "audio": { "id": "MEDIA1", "voice": true }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "messages": [{ "id": "wamid.ABC" }] })),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("note.ogg");
    std::fs::write(&file, "OGGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args([
            "send",
            "audio",
            "15551234567",
            "--voice",
            "--reply-to",
            "wamid.X",
            "-q",
        ])
        .arg("--file")
        .arg(&file)
        .assert()
        .success()
        .stdout("wamid.ABC\n");
}

#[test]
fn voice_dry_run_shows_the_flag_and_only_audio_accepts_it() {
    let home = TempDir::new().unwrap();
    let to = "15551234567";
    let out = paloma(None, &home)
        .args([
            "send",
            "audio",
            to,
            "--link",
            "https://x.test/a.ogg",
            "--voice",
        ])
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert_eq!(
        stdout_json(&out)["body"]["audio"],
        json!({ "link": "https://x.test/a.ogg", "voice": true })
    );

    // The local-file path prints its upload too, and the send must still carry both.
    let file = home.path().join("note.ogg");
    std::fs::write(&file, "OGGDATA").unwrap();
    let out = paloma(None, &home)
        .args(["send", "audio", to, "--voice", "--reply-to", "wamid.X"])
        .args(["--dry-run", "-o", "json", "--file"])
        .arg(&file)
        .output()
        .unwrap();
    let requests = stdout_json(&out);
    assert_eq!(requests.as_array().map(Vec::len), Some(2), "{requests}");
    assert!(
        requests[0]["url"].as_str().unwrap().ends_with("/media"),
        "{requests}"
    );
    assert_eq!(
        requests[1]["body"],
        json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": to,
            "type": "audio",
            "context": { "message_id": "wamid.X" },
            "audio": { "id": "<uploaded-at-send-time>", "voice": true }
        })
    );

    let out = paloma(None, &home)
        .args(["send", "audio", to, "--id", "9", "--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert_eq!(stdout_json(&out)["body"]["audio"], json!({ "id": "9" }));

    for kind in ["image", "document", "video", "sticker"] {
        paloma(None, &home)
            .args(["send", kind, to, "--id", "9", "--voice", "--dry-run"])
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("--voice"));
        paloma(None, &home)
            .args(["send", kind, "--help"])
            .assert()
            .success()
            .stdout(predicate::str::contains("--voice").not());
    }
    paloma(None, &home)
        .args(["send", "audio", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--voice"));
}

/// A handle upload's first step: the session id comes back with its own query string.
async fn mount_upload_session(graph: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v26.0/999/uploads"))
        .and(query_param("file_name", "pic.png"))
        .and(query_param("file_length", "7"))
        .and(query_param("file_type", "image/png"))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "id": "upload:S1?sig=abc" })),
        )
        .expect(1)
        .mount(graph)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_opens_a_session_then_sends_the_raw_file() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .and(query_param("sig", "abc"))
        .and(header("authorization", format!("OAuth {TOKEN}").as_str()))
        .and(header("file_offset", "0"))
        .and(header("content-length", "7"))
        .and(header("content-type", "application/octet-stream"))
        .and(wiremock::matchers::body_string("PNGDATA"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    // No phone number id: a handle belongs to the app, not to a number.
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["media", "upload", "--handle", "--app-id", "999", "-q"])
        .arg(&file)
        .assert()
        .success()
        .stdout("4::HANDLE\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_resumes_from_the_offset_meta_reports() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .and(header("file_offset", "0"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path("/v26.0/upload:S1"))
        .and(query_param("sig", "abc"))
        .and(header("authorization", format!("OAuth {TOKEN}").as_str()))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "id": "upload:S1", "file_offset": "3" })),
        )
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .and(header("file_offset", "3"))
        .and(header("content-length", "4"))
        .and(wiremock::matchers::body_string("DATA"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--app-id", "999", "-q"])
        .arg(&file)
        .assert()
        .success()
        .stdout("4::HANDLE\n")
        .stderr(predicate::str::contains("resuming at byte 3"));
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_gives_up_after_three_tries_with_the_server_error() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .respond_with(ResponseTemplate::new(503))
        .expect(3)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path("/v26.0/upload:S1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "file_offset": "0" })))
        .expect(2)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--app-id", "999", "-q"])
        .arg(&file)
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_does_not_resume_after_an_auth_error_or_trust_a_bad_offset() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": { "message": "bad token", "code": 190 }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--app-id", "999"])
        .arg(&file)
        .assert()
        .code(3)
        .stdout(predicate::str::is_empty());

    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path("/v26.0/upload:S1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "file_offset": "8" })))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--app-id", "999"])
        .arg(&file)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("unusable upload offset"));
}

#[test]
fn upload_handle_refuses_bad_files_before_any_call_and_dry_run_hides_the_token() {
    let home = TempDir::new().unwrap();
    let write = |name: &str, content: &str| {
        let file = home.path().join(name);
        std::fs::write(&file, content).unwrap();
        file
    };
    // No graph server and no app id: these must fail before looking for either.
    for file in [write("notes.txt", "hello"), write("empty.png", "")] {
        paloma(None, &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["media", "upload", "--handle", "--dry-run"])
            .arg(&file)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty());
    }

    let out = paloma(None, &home)
        .args([
            "media",
            "upload",
            "--handle",
            "--app-id",
            "999",
            "--dry-run",
            "-o",
            "json",
        ])
        .arg(write("pic.png", "PNGDATA"))
        .output()
        .unwrap();
    assert!(out.status.success());
    let requests = stdout_json(&out);
    assert_eq!(requests.as_array().map(Vec::len), Some(2), "{requests}");
    assert!(
        requests[0]["url"]
            .as_str()
            .unwrap()
            .contains("/999/uploads?file_name=pic.png&file_length=7&file_type=image%2Fpng"),
        "{requests}"
    );
    assert_eq!(requests[1]["auth"], "OAuth <access-token>");
    assert_eq!(requests[1]["headers"], json!({ "file_offset": "0" }));
    assert_eq!(requests[1]["file"]["bytes"], 7);
    assert!(!requests.to_string().contains(TOKEN), "{requests}");
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_looks_up_the_app_when_none_is_set() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/subscribed_apps")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "whatsapp_business_api_data": { "id": "999" } }]
        })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--dry-run", "-o", "json"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        stdout_json(&out)[0]["url"]
            .as_str()
            .unwrap()
            .contains("/999/uploads"),
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("using app 999"));
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_warns_when_the_waba_has_several_apps_and_names_app_id_when_it_has_none() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/subscribed_apps")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [
                { "whatsapp_business_api_data": { "id": "999" } },
                { "whatsapp_business_api_data": { "id": "888" } }
            ]
        })))
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--dry-run"])
        .arg(&file)
        .assert()
        .success()
        .stderr(predicate::str::contains("warning").and(predicate::str::contains("--app-id")));

    paloma(Some(&graph), &home)
        .env_remove("PALOMA_WABA_ID")
        .args(["media", "upload", "--handle", "--dry-run"])
        .arg(&file)
        .assert()
        .code(3)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("--app-id"));
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_resumes_after_a_timeout() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .and(header("file_offset", "0"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(4))
                .set_body_json(json!({ "h": "4::TOO_LATE" })),
        )
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path("/v26.0/upload:S1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "file_offset": "3" })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .and(header("file_offset", "3"))
        .and(wiremock::matchers::body_string("DATA"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args([
            "media",
            "upload",
            "--handle",
            "--app-id",
            "999",
            "-q",
            "--timeout",
            "1s",
        ])
        .arg(&file)
        .assert()
        .success()
        .stdout("4::HANDLE\n")
        .stderr(predicate::str::contains("resuming at byte 3"));
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_says_so_when_meta_has_the_file_but_gave_no_handle() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .and(header("file_offset", "0"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path("/v26.0/upload:S1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "file_offset": 7 })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .and(header("file_offset", "7"))
        .and(header("content-length", "0"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": { "message": "bad offset", "code": 100 }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--app-id", "999"])
        .arg(&file)
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("whole file"));
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_keeps_the_transfer_failure_visible_when_the_offset_check_fails() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .respond_with(ResponseTemplate::new(503).set_body_json(json!({
            "error": { "message": "transfer broke", "code": 2 }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path("/v26.0/upload:S1"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": { "message": "session gone", "code": 100 }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--app-id", "999"])
        .arg(&file)
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(
            predicate::str::contains("transfer broke")
                .and(predicate::str::contains("session gone")),
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_output_formats_pass_the_response_through_and_empty_handles_fail() {
    for (format, expected) in [
        ("json", "\"h\""),
        ("raw", "{\"h\":\"4::HANDLE\"}"),
        ("table", "4::HANDLE"),
    ] {
        let graph = MockServer::start().await;
        mount_upload_session(&graph).await;
        Mock::given(method("POST"))
            .and(path("/v26.0/upload:S1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
            .mount(&graph)
            .await;
        let home = TempDir::new().unwrap();
        let file = home.path().join("pic.png");
        std::fs::write(&file, "PNGDATA").unwrap();
        paloma(Some(&graph), &home)
            .args([
                "media", "upload", "--handle", "--app-id", "999", "-o", format,
            ])
            .arg(&file)
            .assert()
            .success()
            .stdout(predicate::str::contains(expected));
    }

    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "" })))
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--app-id", "999", "-q"])
        .arg(&file)
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_stops_when_the_file_changes_after_the_session_opens() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v26.0/999/uploads"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(2))
                .set_body_json(json!({ "id": "upload:S1" })),
        )
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    let out = std::thread::scope(|scope| {
        let running = scope.spawn(|| {
            paloma(Some(&graph), &home)
                .args(["media", "upload", "--handle", "--app-id", "999", "-q"])
                .arg(&file)
                .output()
                .unwrap()
        });
        std::thread::sleep(std::time::Duration::from_millis(700));
        std::fs::write(&file, "CHANGED DATA").unwrap();
        running.join().unwrap()
    });
    assert_eq!(out.status.code(), Some(5));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("changed during the upload"));
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_keeps_the_session_signature_and_token_out_of_verbose_logs() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v26.0/999/uploads"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "id": "upload:S1?sig=SIGSECRET" })),
        )
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .and(query_param("sig", "SIGSECRET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--app-id", "999", "-vv"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("upload:S1"),
        "the log must really run: {stderr}"
    );
    assert!(!stderr.contains("SIGSECRET"), "{stderr}");
    assert!(!stderr.contains(TOKEN), "{stderr}");
}

#[cfg(unix)]
#[test]
fn upload_handle_refuses_an_unreadable_file_before_any_call() {
    use std::os::unix::fs::PermissionsExt;

    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::File::open(&file).is_ok() {
        return; // root reads anything, so there's nothing to check.
    }
    paloma(None, &home)
        .args(["media", "upload", "--handle", "--app-id", "999"])
        .arg(&file)
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_photo_uploads_then_sets_the_handle() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .and(wiremock::matchers::body_string("PNGDATA"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/whatsapp_business_profile")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "profile_picture_handle": "4::HANDLE"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["profile", "photo", "--app-id", "999", "-o", "json"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(stdout_json(&out), json!({ "success": true }));
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_photo_does_not_update_the_profile_when_the_upload_fails() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": { "message": "bad token", "code": 190 }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/whatsapp_business_profile")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["profile", "photo", "--app-id", "999"])
        .arg(&file)
        .assert()
        .code(3)
        .stdout(predicate::str::is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_photo_explains_a_failed_update_after_a_good_upload() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/whatsapp_business_profile")))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": { "message": "handle is invalid", "code": 100 }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["profile", "photo", "--app-id", "999"])
        .arg(&file)
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(
            predicate::str::contains("handle is invalid")
                .and(predicate::str::contains("profile get")),
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_photo_quiet_prints_nothing_and_raw_prints_the_update_response() {
    let cases: [(&[&str], &str); 2] = [(&["-q"], ""), (&["-o", "raw"], "{\"success\":true}")];
    for (flags, expected) in cases {
        let graph = MockServer::start().await;
        mount_upload_session(&graph).await;
        Mock::given(method("POST"))
            .and(path("/v26.0/upload:S1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
            .mount(&graph)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/whatsapp_business_profile")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
            .mount(&graph)
            .await;
        let home = TempDir::new().unwrap();
        let file = home.path().join("pic.png");
        std::fs::write(&file, "PNGDATA").unwrap();
        let out = paloma(Some(&graph), &home)
            .args(["profile", "photo", "--app-id", "999"])
            .args(flags)
            .arg(&file)
            .output()
            .unwrap();
        assert!(out.status.success(), "{flags:?}");
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), expected);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_photo_checks_the_file_then_the_phone_before_any_call() {
    // Any request reaching this server fails the assertion at the end.
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let write = |name: &str| {
        let file = home.path().join(name);
        std::fs::write(&file, "DATA").unwrap();
        file
    };
    for file in [write("menu.pdf"), write("promo.mp4"), write("notes.txt")] {
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["profile", "photo"])
            .arg(&file)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(
                "it takes image/jpeg, image/jpg, image/png
",
            ));
    }
    let png = write("pic.png");
    paloma(Some(&graph), &home)
        .args(["profile", "photo", "--type", "application/pdf"])
        .arg(&png)
        .assert()
        .code(5);
    // The phone check comes before the app lookup, which would otherwise spend a read.
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["profile", "photo"])
        .arg(&png)
        .assert()
        .code(3)
        .stderr(predicate::str::contains("phone number id"));
    assert_eq!(graph.received_requests().await.unwrap().len(), 0);
}

#[test]
fn profile_photo_dry_run_prints_every_step_and_honors_the_type_flag() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("logo.bin");
    std::fs::write(&file, "DATA").unwrap();
    let out = paloma(None, &home)
        .args(["profile", "photo", "--app-id", "999", "--type", "image/png"])
        .args(["--dry-run", "-o", "json"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success());
    let requests = stdout_json(&out);
    assert_eq!(requests.as_array().map(Vec::len), Some(3), "{requests}");
    assert!(
        requests[0]["url"]
            .as_str()
            .unwrap()
            .contains("/999/uploads?file_name=logo.bin&file_length=4&file_type=image%2Fpng"),
        "{requests}"
    );
    assert_eq!(requests[1]["file"]["bytes"], 4);
    assert_eq!(
        requests[2]["body"],
        json!({
            "messaging_product": "whatsapp",
            "profile_picture_handle": "<handle-from-the-upload>"
        })
    );
    assert!(!requests.to_string().contains(TOKEN), "{requests}");
}

#[tokio::test(flavor = "multi_thread")]
async fn number_get_and_quality_request_the_current_limit_field() {
    let graph = MockServer::start().await;
    let p = format!("/v26.0/{PHONE}");
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param(
            "fields",
            "id,display_phone_number,verified_name,quality_rating,code_verification_status,\
             name_status,status,health_status,whatsapp_business_manager_messaging_limit,\
             throughput,platform_type",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": PHONE,
            "status": "CONNECTED",
            "whatsapp_business_manager_messaging_limit": "TIER_250",
            "health_status": {
                "can_send_message": "AVAILABLE",
                "entities": [{"entity_type": "PHONE_NUMBER", "id": PHONE, "can_send_message": "AVAILABLE"}]
            }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param(
            "fields",
            "display_phone_number,quality_rating,whatsapp_business_manager_messaging_limit,throughput",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "quality_rating": "GREEN",
            "whatsapp_business_manager_messaging_limit": "TIER_250"
        })))
        .expect(1)
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["number", "get"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("TIER_250")
                .and(predicate::str::contains("CONNECTED"))
                .and(predicate::str::contains("AVAILABLE")),
        );
    paloma(Some(&graph), &home)
        .args(["number", "quality", "-q"])
        .assert()
        .success()
        .stdout("GREEN\n");
}

#[test]
fn number_reads_no_longer_ask_for_the_deprecated_limit_field() {
    let home = TempDir::new().unwrap();
    for verb in ["get", "quality"] {
        let out = paloma(None, &home)
            .args(["number", verb, "--dry-run", "-o", "json"])
            .output()
            .unwrap();
        assert!(out.status.success());
        let url = stdout_json(&out)["url"].as_str().unwrap().to_owned();
        assert!(!url.contains("messaging_limit_tier"), "{url}");
        assert!(
            url.contains("whatsapp_business_manager_messaging_limit"),
            "{url}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn number_list_all_follows_paging_next_and_keeps_the_filters() {
    let graph = MockServer::start().await;
    let p = format!("/v26.0/{WABA}/phone_numbers");
    let filtering = r#"[{"field":"account_mode","operator":"EQUAL","value":"LIVE"}]"#;
    let sort = "['last_onboarded_time_ascending']";
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param("after", "C1"))
        .and(query_param("filtering", filtering))
        .and(query_param("sort", sort))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "2" }],
            "paging": { "cursors": { "after": "C2" } }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param("filtering", filtering))
        .and(query_param("sort", sort))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "1" }],
            "paging": { "cursors": { "after": "C1" }, "next": "https://graph/next" }
        })))
        .expect(1)
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["number", "list", "--all", "-o", "json", "--limit", "5"])
        .args(["--account-mode", "LIVE", "--sort", "last-onboarded-asc"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        stdout_json(&out),
        json!({ "data": [{ "id": "1" }, { "id": "2" }] })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn number_list_single_page_renders_columns_and_warns_on_stderr() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/phone_numbers")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "1", "verified_name": "Jasper" }],
            "paging": { "cursors": { "after": "C1" }, "next": "https://graph/next" }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["number", "list", "--sort", "last-onboarded-desc"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("VERIFIED NAME")
                .and(predicate::str::contains("Jasper"))
                .and(predicate::str::contains("--after").not()),
        )
        .stderr(predicate::str::contains("--after C1"));
    // Newest first is Meta's default, so asking for it sends no parameters at all.
    let received = graph.received_requests().await.unwrap();
    assert_eq!(received[0].url.query(), None);
}

#[test]
fn number_list_dry_run_prints_the_page_request() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args(["number", "list", "--dry-run", "-o", "json", "--all"])
        .args([
            "--account-mode",
            "SANDBOX",
            "--limit",
            "100",
            "--after",
            "C9",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v = stdout_json(&out);
    let url = v["url"].as_str().unwrap();
    assert_eq!(v["method"], "GET");
    assert!(
        url.contains(&format!("/{WABA}/phone_numbers?filtering=")),
        "{url}"
    );
    assert!(url.contains("SANDBOX"), "{url}");
    assert!(url.contains("limit=100&after=C9"), "{url}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("dry-run: no request sent"));
}

#[tokio::test(flavor = "multi_thread")]
async fn number_list_rejects_bad_flags_before_any_request() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for limit in ["0", "101"] {
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["number", "list", "--limit", limit])
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("between 1 and 100"));
    }
    paloma(Some(&graph), &home)
        .args(["number", "list", "--account-mode", "PAUSED"])
        .assert()
        .code(2);
    paloma(Some(&graph), &home)
        .args(["number", "list", "--sort", "creation-time"])
        .assert()
        .code(2);
    assert_eq!(graph.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn block_list_all_walks_to_an_empty_page_without_paging_next() {
    let graph = MockServer::start().await;
    let p = format!("/v26.0/{PHONE}/block_users");
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param("after", "C2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [],
            "paging": { "cursors": { "after": "C2", "before": "C2" } }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param("after", "C1"))
        .and(query_param("limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "messaging_product": "whatsapp", "wa_id": "2" }],
            "paging": { "cursors": { "after": "C2", "before": "C1" } }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param("limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "messaging_product": "whatsapp", "wa_id": "1" }],
            "paging": { "cursors": { "after": "C1", "before": "C0" } }
        })))
        .expect(1)
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["block", "list", "--all", "--limit", "1", "-o", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        stdout_json(&out),
        json!({ "data": [
            { "messaging_product": "whatsapp", "wa_id": "1" },
            { "messaging_product": "whatsapp", "wa_id": "2" }
        ] })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn block_list_all_refuses_a_repeated_cursor_rather_than_truncating() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}/block_users")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "wa_id": "1" }],
            "paging": { "cursors": { "after": "SAME" } }
        })))
        .expect(2)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["block", "list", "--all"])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("SAME twice"));
}

#[tokio::test(flavor = "multi_thread")]
async fn block_list_single_page_says_what_paging_can_and_cannot_tell() {
    let graph = MockServer::start().await;
    let p = format!("/v26.0/{PHONE}/block_users");
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param("after", "NEXT"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "wa_id": "2" }],
            "paging": { "cursors": { "after": "END" }, "next": "https://graph/next" }
        })))
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param("after", "EMPTY"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [],
            "paging": { "cursors": { "after": "EMPTY" } }
        })))
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(&p))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "messaging_product": "whatsapp", "wa_id": "1" }],
            "paging": { "cursors": { "after": "NEXT" } }
        })))
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["block", "list"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("WA ID")
                .and(predicate::str::contains("USER ID"))
                .and(predicate::str::contains("--after").not()),
        )
        .stderr(
            predicate::str::contains("may exist").and(predicate::str::contains("--after NEXT")),
        );
    paloma(Some(&graph), &home)
        .args(["block", "list", "--after", "NEXT"])
        .assert()
        .success()
        .stderr(
            predicate::str::contains("more results exist")
                .and(predicate::str::contains("--after END")),
        );
    paloma(Some(&graph), &home)
        .args(["block", "list", "--after", "EMPTY"])
        .assert()
        .success()
        .stdout("(none)\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn block_list_dry_run_prints_the_page_request() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args(["block", "list", "--dry-run", "-o", "json", "--all"])
        .args(["--limit", "10", "--after", "C9"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v = stdout_json(&out);
    assert_eq!(v["method"], "GET");
    assert!(
        v["url"]
            .as_str()
            .unwrap()
            .ends_with(&format!("/{PHONE}/block_users?limit=10&after=C9")),
        "{v}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn block_list_rejects_a_zero_limit_before_resolving_the_phone_number() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["block", "list", "--limit", "0"])
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("--limit"));
    assert_eq!(graph.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn number_register_sends_the_data_localization_region() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/register")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "pin": "825194",
            "data_localization_region": "CH"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success": true})))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .env("PALOMA_PIN", "825194")
        .args(["number", "register", "--data-localization-region", "CH"])
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
async fn number_register_without_a_region_leaves_it_out_of_the_body() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/register")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "pin": "825194"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success": true})))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .env("PALOMA_PIN", "825194")
        .args(["number", "register"])
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
async fn number_register_rejects_an_unsupported_region_before_the_pin_or_any_request() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for bad in ["US", "de", "DEU", ""] {
        // No PIN and no phone number id: either would fail differently if read first.
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["number", "register", "--data-localization-region", bad])
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("data localization region"));
    }
    assert_eq!(graph.received_requests().await.unwrap().len(), 0);
}

#[test]
fn number_register_dry_run_shows_the_region_and_masks_the_pin() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .env("PALOMA_PIN", "825194")
        .args(["number", "register", "--data-localization-region", "DE"])
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v = stdout_json(&out);
    assert_eq!(v["body"]["data_localization_region"], "DE");
    assert_eq!(v["body"]["pin"], "<redacted>");
    assert!(!v.to_string().contains("825194"), "{v}");
}

/// The query strings of everything the stub received, in order.
async fn received_queries(graph: &MockServer) -> Vec<String> {
    graph
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.url.query().unwrap_or_default().to_owned())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn template_list_sends_new_filters_and_fields_become_the_columns() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/message_templates")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "name": "promo", "quality_score": { "score": "GREEN" }, "id": "9" }]
        })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["template", "list", "--name", "promo"])
        .args(["--fields", "name,quality_score"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("QUALITY SCORE")
                .and(predicate::str::contains("GREEN"))
                .and(predicate::str::contains("LANGUAGE").not()),
        )
        .stderr(predicate::str::is_empty());
    assert_eq!(
        received_queries(&graph).await,
        ["name=promo&fields=name%2Cquality_score"]
    );
}

#[test]
fn template_list_dry_run_prints_the_filters() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args(["template", "list", "--name", "promo", "--fields", "name"])
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        stdout_json(&out)["url"]
            .as_str()
            .unwrap()
            .ends_with("/message_templates?name=promo&fields=name"),
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn template_list_bad_filters_fail_without_a_request() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (args, code) in [
        // Meta's docs define quality_score as a field, not as a filter.
        (vec!["--quality-score", "GREEN"], 2),
        (vec!["--name", "  "], 5),
        (vec!["--fields", "name,,status"], 5),
    ] {
        paloma(Some(&graph), &home)
            .args(["template", "list"])
            .args(&args)
            .assert()
            .code(code)
            .stdout(predicate::str::is_empty());
    }
    assert_eq!(graph.received_requests().await.unwrap().len(), 0);
}

/// Page 1 holds only a longer name, page 2 the exact one, and page 3 must stay unread.
async fn mock_template_pages(graph: &MockServer) {
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
            "data": [
                { "name": "promo_two", "language": "en", "status": "PENDING", "id": "21" },
                { "name": "promo", "language": "es", "category": "MARKETING",
                  "status": "REJECTED", "rejected_reason": "SCAM", "id": "22" }
            ],
            "paging": { "cursors": { "after": "C2" }, "next": "https://graph/next" }
        })))
        .mount(graph)
        .await;
    Mock::given(method("GET"))
        .and(path(&p))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "name": "promo_one", "language": "en", "status": "APPROVED", "id": "11" }],
            "paging": { "cursors": { "after": "C1" }, "next": "https://graph/next" }
        })))
        .mount(graph)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn template_status_follows_paging_only_until_the_exact_match() {
    let graph = MockServer::start().await;
    mock_template_pages(&graph).await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["template", "status", "promo", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        stdout_json(&out),
        json!({ "name": "promo", "language": "es", "category": "MARKETING",
                "status": "REJECTED", "rejected_reason": "SCAM" })
    );
    assert_eq!(
        received_queries(&graph).await,
        ["name=promo&limit=100", "name=promo&limit=100&after=C1"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn template_get_finds_a_later_page_and_shows_only_exact_matches() {
    let graph = MockServer::start().await;
    mock_template_pages(&graph).await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["template", "get", "promo", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let shown = stdout_json(&out);
    assert_eq!(shown["data"].as_array().map(Vec::len), Some(1), "{shown}");
    assert_eq!(shown["data"][0]["id"], "22");
    // Other translations could be on the unread page, and stderr has to say so.
    assert!(String::from_utf8_lossy(&out.stderr).contains("--language"));

    paloma(Some(&graph), &home)
        .args(["template", "get", "promo", "--language", "es", "-q"])
        .assert()
        .success()
        .stdout("22\n")
        .stderr(predicate::str::is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn template_get_and_status_exit_1_when_no_page_has_the_name() {
    let graph = MockServer::start().await;
    let p = format!("/v26.0/{WABA}/message_templates");
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param("after", "C1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [], "paging": { "cursors": { "after": "C2" } }
        })))
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(&p))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "name": "promo_one", "id": "11" }],
            "paging": { "cursors": { "after": "C1" }, "next": "https://graph/next" }
        })))
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    for verb in ["get", "status"] {
        paloma(Some(&graph), &home)
            .args(["template", verb, "promo"])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("no template named 'promo'"));
    }
    assert_eq!(graph.received_requests().await.unwrap().len(), 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn template_lookup_refuses_a_repeated_cursor() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/message_templates")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [],
            "paging": { "cursors": { "after": "SAME" }, "next": "https://graph/next" }
        })))
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["template", "status", "promo"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("cursor SAME twice"));
}

#[test]
fn template_get_dry_run_prints_the_lookup_without_a_request() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args(["template", "get", "promo", "--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        stdout_json(&out)["url"]
            .as_str()
            .unwrap()
            .ends_with("/message_templates?name=promo&limit=100"),
    );
}

async fn mock_template_delete(graph: &MockServer) {
    Mock::given(method("DELETE"))
        .and(path(format!("/v26.0/{WABA}/message_templates")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(1)
        .mount(graph)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn template_delete_by_id_sends_the_name_and_id_without_a_lookup() {
    let graph = MockServer::start().await;
    mock_template_delete(&graph).await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["template", "delete", "promo", "--id", "1407", "--yes", "-q"])
        .assert()
        .success()
        .stdout("1407\n");
    assert_eq!(received_queries(&graph).await, ["name=promo&hsm_id=1407"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn template_delete_ids_sends_only_the_json_array() {
    let graph = MockServer::start().await;
    mock_template_delete(&graph).await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["template", "delete", "--ids", "1387,1304", "--yes", "-q"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
    let requests = graph.received_requests().await.unwrap();
    let pairs: Vec<(String, String)> = requests[0]
        .url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    assert_eq!(pairs, [("hsm_ids".to_owned(), "[1387,1304]".to_owned())]);
}

#[tokio::test(flavor = "multi_thread")]
async fn template_delete_refuses_bad_targets_without_a_request() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let too_many = vec!["1"; 101].join(",");
    for (args, code) in [
        // Meta's rule: an id needs its name, and hsm_ids can't be combined with either.
        (vec!["--id", "1"], 2),
        (vec!["promo", "--ids", "1"], 2),
        (vec!["--ids", "1", "--id", "2"], 2),
        (vec!["--ids", "1", "--language", "es"], 2),
        (vec!["promo", "--id", "1", "--language", "es"], 2),
        (vec![], 2),
        (vec!["--ids", too_many.as_str()], 5),
        (vec!["--ids", "1,abc"], 5),
        (vec!["promo", "--id", "abc"], 5),
        (vec!["  "], 5),
    ] {
        paloma(Some(&graph), &home)
            .args(["template", "delete", "--yes"])
            .args(&args)
            .assert()
            .code(code)
            .stdout(predicate::str::is_empty());
    }
    assert_eq!(graph.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn template_delete_prompt_names_the_target_and_refuses_without_a_tty() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (args, named) in [
        (
            vec!["--ids", "1387,1304"],
            "2 template(s) by id (1387, 1304)",
        ),
        (vec!["promo", "--id", "1407"], "template 'promo' (id 1407)"),
    ] {
        paloma(Some(&graph), &home)
            .args(["template", "delete"])
            .args(&args)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(named));
    }
    assert_eq!(graph.received_requests().await.unwrap().len(), 0);
}

#[test]
fn template_delete_ids_dry_run_prints_the_request() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args([
            "template",
            "delete",
            "--ids",
            "1,2",
            "--dry-run",
            "-o",
            "json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let request = stdout_json(&out);
    assert_eq!(request["method"], "DELETE");
    assert!(
        request["url"]
            .as_str()
            .unwrap()
            .ends_with("/message_templates?hsm_ids=%5B1%2C2%5D"),
        "{request}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn template_create_inline_sends_every_typed_component() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}/message_templates")))
        .and(body_json(json!({
            "name": "seasonal_promotion",
            "category": "UTILITY",
            "language": "en_US",
            "parameter_format": "positional",
            "allow_category_change": true,
            "message_send_ttl_seconds": 3600,
            "components": [
                { "type": "HEADER", "format": "TEXT", "text": "Our {{1}} is on!",
                  "example": { "header_text": ["Summer Sale"] } },
                { "type": "BODY", "text": "Use {{1}} by {{2}}.",
                  "example": { "body_text": [["25OFF", "August"]] } },
                { "type": "FOOTER", "text": "Reply STOP to opt out" },
                { "type": "BUTTONS", "buttons": [
                    { "type": "QUICK_REPLY", "text": "Unsubscribe" },
                    { "type": "QUICK_REPLY", "text": "Tell me more" },
                    { "type": "URL", "text": "Shop", "url": "https://x.test/?a=1&p={{1}}",
                      "example": ["summer"] },
                    { "type": "URL", "text": "Help", "url": "https://x.test/help" },
                    { "type": "PHONE_NUMBER", "text": "Call", "phone_number": "15550051310" },
                    { "type": "COPY_CODE", "example": "25OFF" }
                ] }
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "572", "status": "PENDING", "category": "UTILITY"
        })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["template", "create", "--name", "seasonal_promotion"])
        .args(["--category", "UTILITY", "--language", "en_US"])
        .args(["--body", "Use {{1}} by {{2}}.", "--example", "25OFF,August"])
        .args(["--header-text", "Our {{1}} is on!"])
        .args(["--header-example", "Summer Sale"])
        .args(["--footer", "Reply STOP to opt out"])
        .args(["--button", "quick_reply:Unsubscribe"])
        .args(["--button", "quick_reply:Tell me more"])
        .args(["--button", "url:Shop=https://x.test/?a=1&p={{1}}"])
        .args(["--url-example", "summer"])
        .args(["--button", "url:Help=https://x.test/help"])
        .args(["--button", "phone_number:Call=15550051310"])
        .args(["--button", "copy_code:25OFF"])
        .args(["--parameter-format", "positional"])
        .args(["--allow-category-change", "--ttl-seconds", "3600", "-q"])
        .assert()
        .success()
        .stdout("572\n");
}

#[test]
fn template_create_named_parameters_use_the_named_example_shape() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args(["template", "create", "--name", "order"])
        .args(["--category", "UTILITY", "--language", "en_US"])
        .args(["--parameter-format", "named"])
        .args(["--body", "Thanks {{first_name}}, order {{order_number}}."])
        .args(["--example", "order_number=86,first_name=Pablo"])
        .args(["--header-text", "For {{first_name}}"])
        .args(["--header-example", "first_name=Pablo"])
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        stdout_json(&out)["body"],
        json!({
            "name": "order", "category": "UTILITY", "language": "en_US",
            "parameter_format": "named",
            "components": [
                { "type": "HEADER", "format": "TEXT", "text": "For {{first_name}}",
                  "example": { "header_text_named_params": [
                      { "param_name": "first_name", "example": "Pablo" } ] } },
                { "type": "BODY", "text": "Thanks {{first_name}}, order {{order_number}}.",
                  "example": { "body_text_named_params": [
                      { "param_name": "order_number", "example": "86" },
                      { "param_name": "first_name", "example": "Pablo" } ] } }
            ]
        })
    );
}

#[test]
fn template_create_media_header_takes_an_upload_handle() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args(["template", "create", "--name", "flyer"])
        .args(["--category", "MARKETING", "--language", "en_US"])
        .args(["--body", "Specials", "--header-media", "IMAGE"])
        .args(["--header-handle", "4::aW", "--ttl-seconds", "43200"])
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        stdout_json(&out)["body"],
        json!({
            "name": "flyer", "category": "MARKETING", "language": "en_US",
            "message_send_ttl_seconds": 43200,
            "components": [
                { "type": "HEADER", "format": "IMAGE",
                  "example": { "header_handle": ["4::aW"] } },
                { "type": "BODY", "text": "Specials" }
            ]
        })
    );
}

#[test]
fn template_create_accepts_the_30_day_ttl_value() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args(["template", "create", "--name", "t", "--category", "UTILITY"])
        .args(["--language", "en", "--body", "Hi", "--ttl-seconds", "-1"])
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(stdout_json(&out)["body"]["message_send_ttl_seconds"], -1);
}

#[tokio::test(flavor = "multi_thread")]
async fn template_create_inline_validates_before_any_request() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let long_61 = "x".repeat(61);
    let long_26 = "x".repeat(26);
    let long_body = "x".repeat(1025);
    let long_url = format!("url:Go=https://x.test/{}", "x".repeat(2000));
    let long_phone = format!("phone_number:Call={}", "1".repeat(21));
    let long_code = format!("copy_code:{}", "x".repeat(21));
    let long_label = format!("quick_reply:{long_26}");
    let button = |spec: &'static str| ["--button", spec];
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (vec!["--footer", &long_61], "--footer is 61 chars"),
        (vec!["--header-text", &long_61], "--header-text is 61 chars"),
        (vec!["--header-text", "{{1}} {{2}}"], "at most 1 variable"),
        (vec!["--header-text", "Hi {{1}}"], "--header-example has 0"),
        (vec!["--ttl-seconds", "43201"], "must be 30 to 43200"),
        (vec!["--ttl-seconds", "0"], "must be 30 to 43200"),
        (vec!["--button", &long_label], "label is 26 chars"),
        (vec!["--button", &long_url], "URL is 2"),
        (vec!["--button", &long_phone], "number is 21 chars"),
        (vec!["--button", &long_code], "example is 21 chars"),
        (vec!["--button", "nope:x"], "must be quick_reply:TEXT"),
        (vec!["--button", "url:No value"], "must be quick_reply:TEXT"),
        (
            vec!["--button", "url:Go=https://x.test/{{1}}"],
            "--url-example",
        ),
        (
            vec![
                "--button",
                "url:Go=https://x.test/{{1}}/end",
                "--url-example",
                "a",
            ],
            "at its end",
        ),
        (
            vec!["--button", "url:Go=https://x.test/", "--url-example", "a"],
            "more --url-example",
        ),
        (
            [
                button("url:A=https://x.test/a"),
                button("url:B=https://x.test/b"),
                button("url:C=https://x.test/c"),
            ]
            .concat(),
            "3 URL buttons",
        ),
        (
            [button("phone_number:A=1"), button("phone_number:B=2")].concat(),
            "2 phone number buttons",
        ),
        (
            [button("copy_code:A"), button("copy_code:B")].concat(),
            "2 copy code buttons",
        ),
        (
            [
                button("quick_reply:A"),
                button("url:B=https://x.test/"),
                button("quick_reply:C"),
            ]
            .concat(),
            "must sit together",
        ),
        (
            vec![
                "--button",
                "quick_reply:1",
                "--button",
                "quick_reply:2",
                "--button",
                "quick_reply:3",
                "--button",
                "quick_reply:4",
                "--button",
                "quick_reply:5",
                "--button",
                "quick_reply:6",
                "--button",
                "quick_reply:7",
                "--button",
                "quick_reply:8",
                "--button",
                "quick_reply:9",
                "--button",
                "quick_reply:10",
                "--button",
                "quick_reply:11",
            ],
            "11 buttons given",
        ),
    ];
    for (flags, needle) in cases {
        paloma(Some(&graph), &home)
            .args(["template", "create", "--name", "t", "--category", "UTILITY"])
            .args(["--language", "en", "--body", "Hi"])
            .args(&flags)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(needle));
    }
    for (name, category, body, extra, needle) in [
        ("Bad Name", "UTILITY", "Hi", vec![], "lowercase"),
        ("t", "UTILITY", long_body.as_str(), vec![], "--body is 1025"),
        ("t", "UTILITY", "Hi {{1}}", vec![], "--example has 0"),
        (
            "t",
            "UTILITY",
            "Hi",
            vec!["--example", "a"],
            "--example has 1",
        ),
        (
            "t",
            "UTILITY",
            "Hi {{2}}",
            vec!["--example", "a"],
            "from {{1}}",
        ),
        (
            "t",
            "UTILITY",
            "Hi {{who}}",
            vec!["--example", "a"],
            "--parameter-format named",
        ),
        (
            "t",
            "UTILITY",
            "Hi {{who}}",
            vec!["--parameter-format", "named", "--example", "a"],
            "name=value",
        ),
        (
            "t",
            "UTILITY",
            "Hi {{who}}",
            vec!["--parameter-format", "named"],
            "no example for {{who}}",
        ),
        ("t", "AUTHENTICATION", "Hi", vec!["--footer", "x"], "--file"),
        (
            "t",
            "MARKETING",
            "Hi",
            vec!["--ttl-seconds", "120"],
            "43200 to 2592000",
        ),
        (
            "t",
            "MARKETING",
            "Hi",
            vec!["--ttl-seconds", "-1"],
            "43200 to 2592000",
        ),
        (
            "t",
            "AUTHENTICATION",
            "Hi",
            vec!["--ttl-seconds", "901"],
            "30 to 900",
        ),
    ] {
        paloma(Some(&graph), &home)
            .args(["template", "create", "--name", name, "--category", category])
            .args(["--language", "en", "--body", body])
            .args(&extra)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(needle));
    }
    assert_eq!(graph.received_requests().await.unwrap().len(), 0);
}

#[test]
fn template_create_flag_combinations_are_usage_errors() {
    let home = TempDir::new().unwrap();
    let inline = [
        "--name",
        "t",
        "--category",
        "UTILITY",
        "--language",
        "en",
        "--body",
        "Hi",
    ];
    for flags in [
        vec!["--file", "t.json", "--footer", "x"],
        vec!["--footer", "x"],
        [
            &inline[..],
            &[
                "--header-text",
                "a",
                "--header-media",
                "IMAGE",
                "--header-handle",
                "h",
            ],
        ]
        .concat(),
        [&inline[..], &["--header-media", "IMAGE"]].concat(),
        [&inline[..], &["--header-handle", "h"]].concat(),
        [&inline[..], &["--header-example", "a"]].concat(),
        [&inline[..], &["--url-example", "a"]].concat(),
        [&inline[..], &["--parameter-format", "nope"]].concat(),
    ] {
        paloma(None, &home)
            .args(["template", "create"])
            .args(&flags)
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty());
    }
}

/// Two lookup pages and a last one, for `promo` held in the given languages.
async fn mock_three_template_pages(graph: &MockServer, page_two: Value, page_three: Value) {
    let p = format!("/v26.0/{WABA}/message_templates");
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param("after", "C2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": page_three, "paging": { "cursors": { "after": "C3" } }
        })))
        .mount(graph)
        .await;
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param("after", "C1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": page_two,
            "paging": { "cursors": { "after": "C2" }, "next": "https://graph/next" }
        })))
        .mount(graph)
        .await;
    Mock::given(method("GET"))
        .and(path(&p))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "name": "promo_one", "language": "en", "id": "11" }],
            "paging": { "cursors": { "after": "C1" }, "next": "https://graph/next" }
        })))
        .mount(graph)
        .await;
}

fn template_edit_file(home: &TempDir) -> std::path::PathBuf {
    let file = home.path().join("edit.json");
    std::fs::write(&file, r#"{"category":"UTILITY"}"#).unwrap();
    file
}

#[tokio::test(flavor = "multi_thread")]
async fn template_edit_without_language_sees_a_translation_on_the_last_page() {
    let graph = MockServer::start().await;
    mock_three_template_pages(
        &graph,
        json!([{ "name": "promo", "language": "en", "id": "21" }]),
        json!([{ "name": "promo", "language": "es", "id": "31" }]),
    )
    .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["template", "edit", "promo", "--file"])
        .arg(template_edit_file(&home))
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("pass --language"));
    assert_eq!(graph.received_requests().await.unwrap().len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn template_edit_without_language_reads_every_page_then_edits_the_only_match() {
    let graph = MockServer::start().await;
    mock_three_template_pages(
        &graph,
        json!([{ "name": "promo", "language": "en", "id": "21" }]),
        json!([{ "name": "promo_two", "language": "es", "id": "31" }]),
    )
    .await;
    Mock::given(method("POST"))
        .and(path("/v26.0/21"))
        .and(body_json(json!({ "category": "UTILITY" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["template", "edit", "promo", "-q", "--file"])
        .arg(template_edit_file(&home))
        .assert()
        .success()
        .stdout("21\n");
    assert_eq!(
        received_queries(&graph).await,
        [
            "name=promo&limit=100",
            "name=promo&limit=100&after=C1",
            "name=promo&limit=100&after=C2",
            "",
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn template_edit_without_language_exits_1_when_no_page_has_the_name() {
    let graph = MockServer::start().await;
    mock_three_template_pages(&graph, json!([]), json!([])).await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["template", "edit", "promo", "--file"])
        .arg(template_edit_file(&home))
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("no template named 'promo'"));
    assert_eq!(graph.received_requests().await.unwrap().len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn template_edit_with_language_stops_at_the_match() {
    let graph = MockServer::start().await;
    mock_template_pages(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/22"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args([
            "template",
            "edit",
            "promo",
            "--language",
            "es",
            "-q",
            "--file",
        ])
        .arg(template_edit_file(&home))
        .assert()
        .success()
        .stdout("22\n");
    assert_eq!(graph.received_requests().await.unwrap().len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn template_delete_language_finds_a_later_page_and_stops_there() {
    let graph = MockServer::start().await;
    mock_template_pages(&graph).await;
    mock_template_delete(&graph).await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["template", "delete", "promo", "--language", "es", "--yes"])
        .assert()
        .success();
    let requests = graph.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[2].method.as_str(), "DELETE");
    assert_eq!(requests[2].url.query(), Some("name=promo&hsm_id=22"));
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
        .args(["apply", "-f"])
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

// Interactive media headers.

const TO: &str = "15551234567";

fn dry_run_body(home: &TempDir, args: &[&str]) -> Value {
    let out = paloma(None, home)
        .args(["send", "interactive"])
        .args(args)
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout_json(&out)["body"]["interactive"].clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn interactive_button_sends_a_media_id_header() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": TO,
            "type": "interactive",
            "interactive": {
                "type": "button",
                "header": { "type": "image", "image": { "id": "2762" } },
                "body": { "text": "Pick" },
                "action": { "buttons": [
                    { "type": "reply", "reply": { "id": "y", "title": "Yes" } }
                ] }
            }
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.R"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["send", "interactive", "button", TO, "--body", "Pick"])
        .args(["--button", "y:Yes", "--header-image", "2762", "-q"])
        .assert()
        .success()
        .stdout("wamid.R\n");
}

#[test]
fn interactive_button_takes_each_media_kind_by_id_or_link() {
    let home = TempDir::new().unwrap();
    let base = ["button", TO, "--body", "b", "--button", "y:Yes"];
    for (flag, kind) in [
        ("--header-image", "image"),
        ("--header-video", "video"),
        ("--header-document", "document"),
    ] {
        let mut args = base.to_vec();
        args.extend([flag, "https://x.test/f"]);
        assert_eq!(
            dry_run_body(&home, &args)["header"],
            json!({ "type": kind, kind: { "link": "https://x.test/f" } })
        );
        let mut args = base.to_vec();
        args.extend([flag, "99"]);
        assert_eq!(
            dry_run_body(&home, &args)["header"],
            json!({ "type": kind, kind: { "id": "99" } })
        );
    }
}

#[test]
fn interactive_cta_url_takes_a_media_link_and_refuses_an_id() {
    let home = TempDir::new().unwrap();
    let base = [
        "cta-url",
        TO,
        "--body",
        "b",
        "--display",
        "Open",
        "--url",
        "https://x.test",
    ];
    let mut args = base.to_vec();
    args.extend(["--header-video", "https://x.test/v.mp4"]);
    assert_eq!(
        dry_run_body(&home, &args)["header"],
        json!({ "type": "video", "video": { "link": "https://x.test/v.mp4" } })
    );
    paloma(None, &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["send", "interactive"])
        .args(base)
        .args(["--header-image", "99"])
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("not a media id"));
}

#[test]
fn interactive_media_header_flags_are_exclusive_and_list_has_none() {
    let home = TempDir::new().unwrap();
    let button = ["send", "interactive", "button", TO, "--body", "b"];
    for extra in [
        ["--header", "Text", "--header-image", "1"],
        ["--header-video", "2", "--header-image", "1"],
    ] {
        paloma(None, &home)
            .args(button)
            .args(["--button", "y:Yes"])
            .args(extra)
            .assert()
            .code(2);
    }
    paloma(None, &home)
        .args(["send", "interactive", "list", TO, "--body", "b"])
        .args(["--button", "Open", "--section", "S:a:A"])
        .args(["--header-image", "1"])
        .assert()
        .code(2);
}

#[test]
fn interactive_media_header_refuses_a_bad_reference_without_echoing_it() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["send", "interactive", "button", TO, "--body", "b"])
        .args(["--button", "y:Yes", "--header-image", "ftp://x/secret"])
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty())
        .stderr(
            predicate::str::contains("--header-image")
                .and(predicate::str::contains("secret").not()),
        );
}

// List row descriptions and limits.

fn list_args(section: &str) -> Vec<&str> {
    vec![
        "list",
        TO,
        "--body",
        "b",
        "--button",
        "Open",
        "--section",
        section,
    ]
}

#[test]
fn list_row_description_is_sent_and_the_old_syntax_still_works() {
    let home = TempDir::new().unwrap();
    let section = r"Ship:fast:Express::Next day\, by 5: pm,slow:Time: 10:30";
    assert_eq!(
        dry_run_body(&home, &list_args(section))["action"]["sections"],
        json!([{ "title": "Ship", "rows": [
            { "id": "fast", "title": "Express", "description": "Next day, by 5: pm" },
            { "id": "slow", "title": "Time: 10:30" }
        ] }])
    );
}

#[test]
fn list_row_limits_are_exact_and_checked_before_context() {
    let home = TempDir::new().unwrap();
    let cases = [
        ("a".repeat(200), "T".to_owned(), "D".to_owned(), None),
        (
            "a".repeat(201),
            "T".to_owned(),
            "D".to_owned(),
            Some("row id"),
        ),
        ("a".to_owned(), "T".repeat(24), "D".to_owned(), None),
        (
            "a".to_owned(),
            "T".repeat(25),
            "D".to_owned(),
            Some("row title"),
        ),
        ("a".to_owned(), "T".to_owned(), "D".repeat(72), None),
        (
            "a".to_owned(),
            "T".to_owned(),
            "D".repeat(73),
            Some("row description"),
        ),
        (
            " ".to_owned(),
            "T".to_owned(),
            "D".to_owned(),
            Some("row id"),
        ),
        (
            "a".to_owned(),
            " ".to_owned(),
            "D".to_owned(),
            Some("row title"),
        ),
        (
            "a".to_owned(),
            "T".to_owned(),
            " ".to_owned(),
            Some("row description"),
        ),
    ];
    for (id, title, description, problem) in cases {
        let section = format!("S:{id}:{title}::{description}");
        let mut cmd = paloma(None, &home);
        cmd.args(["send", "interactive"]).args(list_args(&section));
        match problem {
            None => {
                cmd.arg("--dry-run").assert().success();
            }
            Some(problem) => {
                cmd.env_remove("PALOMA_PHONE_NUMBER_ID")
                    .assert()
                    .code(5)
                    .stdout(predicate::str::is_empty())
                    .stderr(predicate::str::contains(problem));
            }
        }
    }
}

// Media type and size checks.

const MIB: u64 = 1024 * 1024;

fn sized_file(home: &TempDir, name: &str, bytes: u64) -> std::path::PathBuf {
    let file = home.path().join(name);
    std::fs::File::create(&file)
        .unwrap()
        .set_len(bytes)
        .unwrap();
    file
}

#[test]
fn send_file_checks_type_and_size_per_kind_before_context() {
    let home = TempDir::new().unwrap();
    let cases = [
        ("image", "a.jpg", 5 * MIB, true),
        ("image", "a.png", 5 * MIB + 1, false),
        ("image", "a.webp", 1, false),
        ("image", "a.pdf", 1, false),
        ("video", "a.mp4", 16 * MIB, true),
        ("video", "a.3gp", 16 * MIB + 1, false),
        ("video", "a.mov", 1, false),
        ("audio", "a.m4a", 16 * MIB, true),
        ("audio", "a.amr", 1, true),
        ("audio", "a.ogg", 16 * MIB + 1, false),
        ("audio", "a.wav", 1, false),
        ("sticker", "a.webp", 500 * 1024, true),
        ("sticker", "a.webp", 500 * 1024 + 1, false),
        ("sticker", "a.png", 1, false),
        ("document", "a.pdf", 100 * MIB, true),
        ("document", "a.docx", 100 * MIB + 1, false),
    ];
    for (kind, name, bytes, fits) in cases {
        let file = sized_file(&home, name, bytes);
        let mut cmd = paloma(None, &home);
        cmd.args(["send", kind, TO, "--file"]).arg(&file);
        if fits {
            cmd.args(["--dry-run", "-o", "json"]).assert().success();
        } else {
            cmd.env_remove("PALOMA_PHONE_NUMBER_ID")
                .assert()
                .code(5)
                .stdout(predicate::str::is_empty())
                .stderr(predicate::str::contains(kind));
        }
    }
}

#[test]
fn send_document_warns_about_a_type_meta_does_not_support_and_sends_it() {
    let home = TempDir::new().unwrap();
    let file = sized_file(&home, "a.zip", 1);
    let out = paloma(None, &home)
        .args(["send", "document", TO, "--dry-run", "-o", "json", "--file"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("warning: application/zip"), "{stderr}");
    assert_eq!(stdout_json(&out).as_array().map(Vec::len), Some(2));
}

#[test]
fn send_upload_guesses_the_type_meta_lists_for_m4a() {
    let home = TempDir::new().unwrap();
    let file = sized_file(&home, "a.m4a", 1);
    let out = paloma(None, &home)
        .args(["send", "audio", TO, "--dry-run", "-o", "json", "--file"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(stdout_json(&out)[0].to_string().contains("audio/mp4"));
}

#[test]
fn media_upload_checks_the_type_and_its_size_before_context() {
    let home = TempDir::new().unwrap();
    let cases: [(&str, u64, Option<&str>, bool); 8] = [
        ("a.jpg", 5 * MIB, None, true),
        ("a.jpg", 5 * MIB + 1, None, false),
        ("a.mp4", 16 * MIB + 1, None, false),
        ("a.webp", 500 * 1024 + 1, None, false),
        ("a.gif", 1, None, false),
        // The flag wins over the extension, both ways.
        ("a.jpg", 5 * MIB + 1, Some("video/mp4"), true),
        ("a.bin", 1, Some("image/png"), true),
        ("a.png", 1, Some("image/gif"), false),
    ];
    for (name, bytes, mime, fits) in cases {
        let file = sized_file(&home, name, bytes);
        let mut cmd = paloma(None, &home);
        cmd.args(["media", "upload"]).arg(&file);
        if let Some(mime) = mime {
            cmd.args(["--type", mime]);
        }
        if fits {
            cmd.arg("--dry-run")
                .assert()
                .success()
                .stderr(predicate::str::contains("warning:").not());
        } else {
            cmd.env_remove("PALOMA_PHONE_NUMBER_ID")
                .assert()
                .code(5)
                .stdout(predicate::str::is_empty());
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn media_upload_warns_about_an_unlisted_type_and_still_uploads() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/media")))
        .and(body_string_contains("application/zip"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "MEDIA1" })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = sized_file(&home, "a.zip", 4);
    paloma(Some(&graph), &home)
        .args(["media", "upload", "-q"])
        .arg(&file)
        .assert()
        .success()
        .stdout("MEDIA1\n")
        .stderr(predicate::str::contains("warning: application/zip"));
}

#[test]
fn media_upload_keeps_type_parameters_in_the_request() {
    let home = TempDir::new().unwrap();
    let file = sized_file(&home, "a.ogg", 1);
    let out = paloma(None, &home)
        .args(["media", "upload", "--dry-run", "-o", "json"])
        .args(["--type", "audio/ogg; codecs=opus"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        stdout_json(&out)
            .to_string()
            .contains("audio/ogg; codecs=opus")
    );
}

#[test]
fn media_upload_handle_keeps_its_own_type_list() {
    let home = TempDir::new().unwrap();
    // Over the 5 MB message limit, which doesn't apply to a resumable upload.
    let file = sized_file(&home, "a.png", 5 * MIB + 1);
    paloma(None, &home)
        .args([
            "media",
            "upload",
            "--handle",
            "--app-id",
            "999",
            "--dry-run",
        ])
        .arg(&file)
        .assert()
        .success();
}

// Business profile checks.

#[tokio::test(flavor = "multi_thread")]
async fn profile_set_sends_fields_at_their_limits() {
    let graph = MockServer::start().await;
    let (about, address) = ("a".repeat(139), "b".repeat(256));
    let (description, email) = ("c".repeat(512), "d".repeat(128));
    let website = "e".repeat(256);
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/whatsapp_business_profile")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "about": about,
            "address": address,
            "description": description,
            "email": email,
            "vertical": "PROF_SERVICES",
            "websites": [website, "https://x.test"]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["profile", "set", "--vertical", "prof_services"])
        .args(["--about", &about, "--address", &address])
        .args(["--description", &description, "--email", &email])
        .args(["--website", &website, "--website", "https://x.test"])
        .assert()
        .success();
}

#[test]
fn profile_set_refuses_values_over_the_limits_before_context() {
    let home = TempDir::new().unwrap();
    let cases = [
        ("--about", String::new()),
        ("--about", "a".repeat(140)),
        ("--address", "a".repeat(257)),
        ("--description", "a".repeat(513)),
        ("--email", "a".repeat(129)),
        ("--website", "a".repeat(257)),
    ];
    for (flag, value) in cases {
        paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["profile", "set", flag, &value])
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(flag));
    }
}

#[test]
fn profile_set_refuses_a_third_website() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["profile", "set", "--website", "a", "--website", "b"])
        .args(["--website", "c"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("at most 2"));
}

#[test]
fn profile_set_vertical_is_a_closed_set() {
    let home = TempDir::new().unwrap();
    for refused in ["FOOD", "UNDEFINED", "NOT_A_BIZ"] {
        paloma(None, &home)
            .args(["profile", "set", "--vertical", refused])
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty());
    }
    let out = paloma(None, &home)
        .args(["profile", "set", "--vertical", "MATRIMONY_SERVICE"])
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        stdout_json(&out)["body"]["vertical"],
        json!("MATRIMONY_SERVICE")
    );
}

// Review fixes across the v0.5 lists, lookups and validation.

#[tokio::test(flavor = "multi_thread")]
async fn block_list_all_follows_next_even_past_an_empty_page() {
    let graph = MockServer::start().await;
    let phone_path = format!("/v26.0/{PHONE}/block_users");
    Mock::given(method("GET"))
        .and(path(phone_path.clone()))
        .and(query_param("after", "C1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "wa_id": "15550001" }],
            "paging": { "cursors": { "after": "C2" } }
        })))
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(phone_path.clone()))
        .and(query_param("after", "C2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [] })))
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(phone_path))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [],
            "paging": { "next": "https://graph/next", "cursors": { "after": "C1" } }
        })))
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["block", "list", "--all", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        stdout_json(&out),
        json!({ "data": [{ "wa_id": "15550001" }] })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_next_page_without_a_cursor_is_an_error_not_the_end() {
    let graph = MockServer::start().await;
    let broken =
        json!({ "data": [{ "name": "other" }], "paging": { "next": "https://graph/next" } });
    for edge in [
        format!("/v26.0/{WABA}/message_templates"),
        format!("/v26.0/{WABA}/phone_numbers"),
        format!("/v26.0/{PHONE}/block_users"),
    ] {
        Mock::given(method("GET"))
            .and(path(edge))
            .respond_with(ResponseTemplate::new(200).set_body_json(&broken))
            .mount(&graph)
            .await;
    }
    let home = TempDir::new().unwrap();
    let cases: [&[&str]; 4] = [
        &["template", "list", "--all"],
        &["number", "list", "--all"],
        &["block", "list", "--all"],
        &["template", "get", "promo"],
    ];
    for args in cases {
        paloma(Some(&graph), &home)
            .args(args)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("no cursor"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_list_page_with_no_data_is_an_error() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}/block_users")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["block", "list", "--all"])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn template_delete_language_refuses_a_match_with_no_id() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/message_templates")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "name": "promo", "language": "es" }]
        })))
        .mount(&graph)
        .await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["template", "delete", "promo", "--language", "es", "--yes"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no id"));
}

#[test]
fn review_validation_fails_before_any_call() {
    let home = TempDir::new().unwrap();
    let list = [
        "send",
        "interactive",
        "list",
        "15551234567",
        "--body",
        "b",
        "--button",
        "o",
    ];
    let long_title = format!("{}:a:A", "t".repeat(25));
    let cases: Vec<Vec<&str>> = vec![
        [&list[..], &["--section", ":a:A"]].concat(),
        [&list[..], &["--section", long_title.as_str()]].concat(),
        [&list[..], &["--section", "S:a::Desc"]].concat(),
        vec!["template", "delete", "--ids", "5,5", "--yes"],
        vec!["template", "list", "--limit", "0"],
        vec![
            "template",
            "create",
            "--name",
            "t",
            "--category",
            "UTILITY",
            "--language",
            "en",
            "--body",
            "Hi {{1}}",
            "--example",
            " ",
        ],
    ];
    for args in cases {
        paloma(None, &home)
            .env("PALOMA_GRAPH_URL", "http://127.0.0.1:9")
            .args(&args)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty());
    }
}

#[test]
fn template_create_example_can_hold_a_comma_and_fields_are_trimmed() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(["template", "create", "--name", "t", "--category", "UTILITY"])
        .args(["--language", "en", "--body", "Hi {{1}}, {{2}}"])
        .args(["--example", "Smith\\, John,#1", "--dry-run", "-o", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Smith, John").and(predicate::str::contains("#1")));
    let out = paloma(None, &home)
        .args(["template", "list", "--fields", "name, status"])
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let url = stdout_json(&out)["url"].as_str().unwrap().to_owned();
    assert!(url.contains("status"), "{url}");
    assert!(!url.contains("%20") && !url.contains("+status"), "{url}");
}

const BSUID: &str = "US.13491208655302741918";
const PARENT_BSUID: &str = "US.ENT.11815799212886844830";

/// A send under `--dry-run`, returning the body it would POST.
fn dry_run_send_body(home: &TempDir, args: &[&str]) -> Value {
    let out = paloma(None, home)
        .arg("send")
        .args(args)
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let printed = stdout_json(&out);
    printed.get("body").cloned().unwrap_or(printed)
}

#[tokio::test(flavor = "multi_thread")]
async fn send_to_a_bsuid_posts_recipient_and_no_to() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "recipient": BSUID,
            "type": "text",
            "text": { "preview_url": false, "body": "hello" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messaging_product": "whatsapp",
            "contacts": [{ "input": BSUID, "user_id": BSUID }],
            "messages": [{ "id": "wamid.BSUID" }]
        })))
        .expect(1)
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["send", "text", BSUID, "hello", "-q"])
        .assert()
        .success()
        .stdout("wamid.BSUID\n")
        .stderr(predicate::str::is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_to_a_parent_bsuid_posts_recipient() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "recipient": PARENT_BSUID,
            "type": "text",
            "text": { "preview_url": false, "body": "hello" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "contacts": [{ "input": PARENT_BSUID, "user_id": PARENT_BSUID }],
            "messages": [{ "id": "wamid.PARENT" }]
        })))
        .expect(1)
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["send", "text", PARENT_BSUID, "hello", "-q"])
        .assert()
        .success()
        .stdout("wamid.PARENT\n");
}

#[test]
fn every_send_verb_addresses_a_bsuid_with_recipient() {
    let home = TempDir::new().unwrap();
    let contacts = home.path().join("contacts.json");
    std::fs::write(&contacts, r#"[{"name":{"formatted_name":"A"}}]"#).unwrap();
    let upload = home.path().join("photo.png");
    std::fs::write(&upload, b"png").unwrap();
    let contacts = contacts.to_str().unwrap();
    let upload = upload.to_str().unwrap();
    let cases: Vec<(&str, Vec<&str>)> = vec![
        ("text", vec!["text", BSUID, "hi"]),
        (
            "template",
            vec!["template", "--via", "cloud", BSUID, "t", "--lang", "en"],
        ),
        ("image", vec!["image", BSUID, "--id", "9"]),
        ("document", vec!["document", BSUID, "--id", "9"]),
        ("audio", vec!["audio", BSUID, "--id", "9"]),
        ("video", vec!["video", BSUID, "--id", "9"]),
        ("sticker", vec!["sticker", BSUID, "--id", "9"]),
        (
            "location",
            vec!["location", BSUID, "--lat", "1", "--long", "2"],
        ),
        ("contacts", vec!["contact", BSUID, "--file", contacts]),
        (
            "reaction",
            vec!["reaction", BSUID, "--message-id", "wamid.X", "--emoji", "y"],
        ),
        (
            "interactive",
            vec![
                "interactive",
                "button",
                BSUID,
                "--body",
                "b",
                "--button",
                "Y",
            ],
        ),
        (
            "interactive",
            vec![
                "interactive",
                "list",
                BSUID,
                "--body",
                "b",
                "--button",
                "B",
                "--section",
                "S:a:A",
            ],
        ),
        (
            "interactive",
            vec![
                "interactive",
                "cta-url",
                BSUID,
                "--body",
                "b",
                "--display",
                "D",
                "--url",
                "https://x.test",
            ],
        ),
    ];
    for (kind, args) in &cases {
        let body = dry_run_send_body(&home, args);
        assert_eq!(body["recipient"], BSUID, "{args:?}: {body}");
        assert_eq!(body["recipient_type"], "individual", "{args:?}: {body}");
        assert_eq!(body["type"], *kind, "{args:?}: {body}");
        assert!(body.get("to").is_none(), "{args:?}: {body}");
    }

    // An upload prints two requests, and only the second addresses anyone.
    let out = paloma(None, &home)
        .args([
            "send",
            "image",
            BSUID,
            "--file",
            upload,
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
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(&format!("\"recipient\": \"{BSUID}\"")),
        "{stdout}"
    );
    assert!(!stdout.contains("\"to\":"), "{stdout}");
}

#[test]
fn a_phone_number_still_addresses_with_to() {
    let home = TempDir::new().unwrap();
    let body = dry_run_send_body(&home, &["text", "+15551234567", "hi"]);
    assert_eq!(body["to"], "+15551234567");
    assert!(body.get("recipient").is_none(), "{body}");
}

#[test]
fn a_malformed_recipient_exits_5_before_resolving_context() {
    let home = TempDir::new().unwrap();
    for bad in ["us.123", "US.", "US.ENT.", "USA.123", "US.12-34", "US123"] {
        // Without a sender, getting as far as the context would be a different error.
        paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .env("PALOMA_GRAPH_URL", "http://127.0.0.1:9")
            .args(["send", "text", bad, "hi"])
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(
                predicate::str::contains("E.164")
                    .and(predicate::str::contains("business-scoped user id")),
            );
    }
    let too_long = format!("US.{}", "1".repeat(129));
    paloma(None, &home)
        .args(["send", "text", &too_long, "hi", "--dry-run"])
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_authentication_template_to_a_bsuid_surfaces_metas_refusal() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {
                "message": "(#131062) Business-scoped User ID (BSUID) recipients are not supported for this message.",
                "type": "OAuthException",
                "code": 131_062
            }
        })))
        .expect(1)
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args([
            "send", "template", "--via", "cloud", BSUID, "otp", "--lang", "en", "-q",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("131062").and(predicate::str::contains("phone number")));
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_keeps_its_own_addressing() {
    let payload = json!({
        "messaging_product": "whatsapp",
        "to": "15551234567",
        "recipient": BSUID,
        "type": "text",
        "text": { "body": "hi" }
    });
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(&payload))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "contacts": [{ "input": "15551234567", "wa_id": "15551234567" }],
            "messages": [{ "id": "wamid.RAW" }]
        })))
        .expect(1)
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    let file = home.path().join("payload.json");
    std::fs::write(&file, payload.to_string()).unwrap();
    paloma(Some(&graph), &home)
        .args(["send", "raw", "--file", file.to_str().unwrap(), "-q"])
        .assert()
        .success()
        .stdout("wamid.RAW\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn block_add_and_remove_write_user_id_for_a_bsuid() {
    let body = json!({
        "messaging_product": "whatsapp",
        "block_users": [{ "user": "15551234567" }, { "user_id": BSUID }]
    });
    let graph = MockServer::start().await;
    for (http_method, listed) in [("POST", "added_users"), ("DELETE", "removed_users")] {
        Mock::given(method(http_method))
            .and(path(format!("/v26.0/{PHONE}/block_users")))
            .and(body_json(&body))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "messaging_product": "whatsapp",
                "block_users": { listed: [
                    { "input": "15551234567", "wa_id": "15551234567" },
                    { "input": BSUID, "user_id": BSUID }
                ] }
            })))
            .expect(1)
            .mount(&graph)
            .await;
    }

    let home = TempDir::new().unwrap();
    for (verb, listed) in [("add", "added_users"), ("remove", "removed_users")] {
        let out = paloma(Some(&graph), &home)
            .args(["block", verb, "15551234567", BSUID, "-o", "json"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{verb}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            stdout_json(&out)["block_users"][listed][1]["user_id"],
            BSUID
        );
    }
}

#[test]
fn block_dry_run_prints_user_id() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args(["block", "add", BSUID, "--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        stdout_json(&out)["body"]["block_users"],
        json!([{ "user_id": BSUID }])
    );
}

#[test]
fn block_refuses_a_parent_bsuid_and_a_malformed_user_before_resolving_context() {
    let home = TempDir::new().unwrap();
    for verb in ["add", "remove"] {
        paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .env("PALOMA_GRAPH_URL", "http://127.0.0.1:9")
            .args(["block", verb, "15551234567", PARENT_BSUID])
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("parent"));
        paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .env("PALOMA_GRAPH_URL", "http://127.0.0.1:9")
            .args(["block", verb, "us.123"])
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("us.123"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn block_list_shows_user_id_and_survives_a_missing_wa_id() {
    let data = json!({
        "data": [
            { "messaging_product": "whatsapp", "wa_id": "15550001", "user_id": "US.111" },
            { "messaging_product": "whatsapp", "user_id": "US.222" }
        ]
    });
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}/block_users")))
        .respond_with(ResponseTemplate::new(200).set_body_json(&data))
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["block", "list"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let rows: Vec<Vec<&str>> = stdout
        .lines()
        .map(|line| line.split_whitespace().collect())
        .collect();
    assert_eq!(
        rows,
        [
            vec!["WA", "ID", "USER", "ID"],
            vec!["15550001", "US.111"],
            vec!["-", "US.222"],
        ],
        "{stdout}"
    );

    let out = paloma(Some(&graph), &home)
        .args(["block", "list", "-o", "json"])
        .output()
        .unwrap();
    assert_eq!(stdout_json(&out), data);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_send_response_without_wa_id_keeps_the_wamid_contract() {
    let responses = [
        json!({
            "messaging_product": "whatsapp",
            "contacts": [{ "input": BSUID, "user_id": BSUID }],
            "messages": [{ "id": "wamid.ONLY" }]
        }),
        json!({
            "messaging_product": "whatsapp",
            "contacts": [{ "user_id": BSUID }],
            "messages": [{ "id": "wamid.ONLY" }]
        }),
        json!({
            "messaging_product": "whatsapp",
            "contacts": [],
            "messages": [{ "id": "wamid.ONLY" }]
        }),
        json!({ "messages": [{ "id": "wamid.ONLY" }] }),
    ];
    for response in responses {
        let graph = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/messages")))
            .respond_with(ResponseTemplate::new(200).set_body_json(&response))
            .mount(&graph)
            .await;

        let home = TempDir::new().unwrap();
        paloma(Some(&graph), &home)
            .args(["send", "text", BSUID, "hi", "-q"])
            .assert()
            .success()
            .stdout("wamid.ONLY\n")
            .stderr(predicate::str::is_empty());
        paloma(Some(&graph), &home)
            .args(["send", "text", BSUID, "hi"])
            .assert()
            .success()
            .stdout(predicate::str::contains("wamid.ONLY"));
        let out = paloma(Some(&graph), &home)
            .args(["send", "text", BSUID, "hi", "-o", "json"])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(stdout_json(&out), response);
    }
}

#[tokio::test]
async fn send_contact_typed_flags_post_the_exact_contact() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": "15551234567",
            "type": "contacts",
            "contacts": [{
                "name": {
                    "formatted_name": "Ana Diaz",
                    "first_name": "Ana",
                    "last_name": "Diaz",
                },
                "phones": [
                    { "phone": "+16505559999", "wa_id": "16505559999" },
                    { "phone": "+16505550000" },
                ],
                "emails": [{ "email": "ana@example.com" }],
            }],
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "messages": [{ "id": "wamid.C" }] })),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["send", "contact", "15551234567", "--name", "Ana Diaz"])
        .args(["--first-name", "Ana", "--last-name", "Diaz"])
        .args(["--phone", "+16505559999", "--wa-id", "16505559999"])
        .args(["--phone", "+16505550000", "--email", "ana@example.com"])
        .args(["-q", "--force"])
        .assert()
        .success()
        .stdout("wamid.C\n");
}

#[test]
fn send_contact_name_needs_a_first_or_last_name() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["send", "contact", "15551234567", "--name", "Ana"])
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("--first-name"));
    let out = paloma(None, &home)
        .args(["send", "contact", "15551234567", "--name", "Ana"])
        .args(["--first-name", "Ana", "--force", "--dry-run", "-o", "raw"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        stdout_json(&out)["contacts"],
        json!([{ "name": { "formatted_name": "Ana", "first_name": "Ana" } }])
    );
}

#[test]
fn send_contact_needs_one_source_and_file_conflicts_with_the_typed_flags() {
    let home = TempDir::new().unwrap();
    let to = "15551234567";
    let cases: Vec<Vec<&str>> = vec![
        vec!["send", "contact", to],
        vec!["send", "contact", to, "--phone", "+1650"],
        vec![
            "send",
            "contact",
            to,
            "--name",
            "A",
            "--first-name",
            "A",
            "--wa-id",
            "1650",
        ],
        vec!["send", "contact", to, "--file", "c.json", "--name", "A"],
        vec!["send", "contact", to, "--file", "c.json", "--phone", "+1"],
        vec!["send", "contact", to, "--file", "c.json", "--email", "a@b"],
    ];
    for args in cases {
        paloma(None, &home)
            .env("PALOMA_GRAPH_URL", "http://127.0.0.1:9")
            .args(&args)
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty());
    }
}

#[test]
fn send_contact_blank_values_and_oversized_files_exit_5_before_any_request() {
    let home = TempDir::new().unwrap();
    let too_many = home.path().join("too-many.json");
    let contacts = vec![json!({ "name": { "formatted_name": "A" } }); 258];
    std::fs::write(&too_many, Value::Array(contacts).to_string()).unwrap();
    let to = "15551234567";
    let cases: Vec<Vec<&str>> = vec![
        vec!["--name", " "],
        vec!["--name", "A", "--first-name", " "],
        vec!["--name", "A", "--last-name", " "],
        vec!["--name", "A", "--phone", " "],
        vec!["--name", "A", "--phone", "+1", "--wa-id", " "],
        vec!["--name", "A", "--email", " "],
        vec!["--file", too_many.to_str().unwrap()],
    ];
    for args in cases {
        paloma(None, &home)
            .env_remove("PALOMA_TOKEN")
            .env("PALOMA_GRAPH_URL", "http://127.0.0.1:9")
            .args(["send", "contact", to, "--force"])
            .args(&args)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty());
    }
}

const HISTORY_WAMID: &str = "wamid.HBgLMTY0NzM3Nzc3NzUV+AgAR/GBI5QTNEQ==";
const HISTORY_FIELDS: &str = "cursor,node{id,delivery_status,error_description,occurrence_timestamp,status_timestamp,application,webhook_update_state,webhook_uri}";

async fn mount_history_lookup(graph: &MockServer) {
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}/message_history")))
        .and(query_param("message_id", HISTORY_WAMID))
        .and(query_param("fields", "id,message_id"))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "777", "message_id": HISTORY_WAMID }],
        })))
        .expect(1)
        .mount(graph)
        .await;
}

fn history_event(id: &str, status: &str) -> Value {
    json!({
        "cursor": id,
        "node": { "id": id, "delivery_status": status, "occurrence_timestamp": 1_640_995_200 },
    })
}

#[tokio::test]
async fn message_history_follows_the_lookup_and_every_page_of_events() {
    let graph = MockServer::start().await;
    mount_history_lookup(&graph).await;
    Mock::given(method("GET"))
        .and(path("/v26.0/777/events"))
        .and(query_param("fields", HISTORY_FIELDS))
        .and(query_param("limit", "100"))
        .and(query_param("after", "C1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [history_event("e2", "DELIVERED")],
            "paging": { "cursors": { "after": "C2" } },
        })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path("/v26.0/777/events"))
        .and(query_param("fields", HISTORY_FIELDS))
        .and(query_param("limit", "100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [history_event("e1", "SENT")],
            "paging": { "cursors": { "after": "C1" }, "next": "https://graph.test/next" },
        })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["message", "history", HISTORY_WAMID, "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        stdout_json(&out),
        json!({ "data": [history_event("e1", "SENT"), history_event("e2", "DELIVERED")] })
    );
    assert!(out.stderr.is_empty());
}

#[tokio::test]
async fn message_history_table_shows_the_status_rows_and_quiet_prints_the_history_id() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}/message_history")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "777", "message_id": HISTORY_WAMID }],
        })))
        .expect(2)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path("/v26.0/777/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{
                "cursor": "e1",
                "node": {
                    "id": "e1",
                    "delivery_status": "ERROR",
                    "occurrence_timestamp": 1_640_995_200,
                    "error_description": "webhook failed",
                },
            }],
        })))
        .expect(2)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["message", "history", HISTORY_WAMID])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("DELIVERY STATUS")
                .and(predicate::str::contains("OCCURRENCE TIMESTAMP"))
                .and(predicate::str::contains("WEBHOOK URI"))
                .and(predicate::str::contains("ERROR"))
                .and(predicate::str::contains("1640995200"))
                .and(predicate::str::contains("webhook failed")),
        );
    paloma(Some(&graph), &home)
        .args(["message", "history", HISTORY_WAMID, "-q"])
        .assert()
        .success()
        .stdout("777\n");
}

#[tokio::test]
async fn message_history_refuses_an_entry_for_another_message_and_an_empty_lookup() {
    for data in [
        json!([]),
        json!([{ "id": "1", "message_id": "wamid.OTHER" }]),
    ] {
        let graph = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/v26.0/{PHONE}/message_history")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": data })))
            .expect(1)
            .mount(&graph)
            .await;
        Mock::given(method("GET"))
            .and(path("/v26.0/1/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [] })))
            .expect(0)
            .mount(&graph)
            .await;
        let home = TempDir::new().unwrap();
        paloma(Some(&graph), &home)
            .args(["message", "history", HISTORY_WAMID])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("no message history"));
    }
}

#[tokio::test]
async fn message_history_a_message_with_no_events_is_an_empty_trail() {
    let graph = MockServer::start().await;
    mount_history_lookup(&graph).await;
    Mock::given(method("GET"))
        .and(path("/v26.0/777/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [] })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["message", "history", HISTORY_WAMID])
        .assert()
        .success()
        .stdout("(none)\n");
}

#[test]
fn message_history_dry_run_prints_both_requests_and_sends_neither() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .env("PALOMA_GRAPH_URL", "http://127.0.0.1:9")
        .args(["message", "history", "wamid.X", "--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let requests = stdout_json(&out);
    let requests = requests.as_array().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["method"], "GET");
    let lookup = requests[0]["url"].as_str().unwrap();
    assert!(
        lookup.contains(&format!("/{PHONE}/message_history?message_id=wamid.X")),
        "{lookup}"
    );
    assert_eq!(requests[1]["method"], "GET");
    let events = requests[1]["url"].as_str().unwrap();
    assert!(events.contains("/<history-id>/events?"), "{events}");
    assert!(events.contains("limit=100"), "{events}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("dry-run"));
}

#[test]
fn message_history_blank_wamid_exits_5_before_any_context() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .env_remove("PALOMA_TOKEN")
        .env("PALOMA_GRAPH_URL", "http://127.0.0.1:9")
        .args(["message", "history", " "])
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty());
}

/// Stub the send endpoint so that only this exact body gets a wamid.
async fn expect_exact_send(graph: &MockServer, body: Value) {
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .and(body_json(body))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.NEW"}]})),
        )
        .expect(1)
        .mount(graph)
        .await;
}

/// The whole message around an interactive object, as sent to a phone number.
fn interactive_message(interactive: &Value) -> Value {
    json!({
        "messaging_product": "whatsapp",
        "recipient_type": "individual",
        "to": TO,
        "type": "interactive",
        "interactive": interactive,
    })
}

/// Run a send with `-q`: stdout must be only the wamid, and stderr must be quiet.
fn assert_quiet_send(graph: &MockServer, args: &[&str]) {
    let home = TempDir::new().unwrap();
    paloma(Some(graph), &home)
        .args(args)
        .arg("-q")
        .assert()
        .success()
        .stdout("wamid.NEW\n")
        .stderr(predicate::str::is_empty());
}

/// A bad value exits 5 with nothing on stdout, before context resolves or a request goes.
fn assert_refused_locally(args: &[&str]) {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .env_remove("PALOMA_TOKEN")
        .env("PALOMA_GRAPH_URL", "http://127.0.0.1:9")
        .args(args)
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty());
}

/// The body a dry run would send.
fn sent_dry_run_body(args: &[&str]) -> Value {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .env("PALOMA_GRAPH_URL", "http://127.0.0.1:9")
        .args(args)
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("dry-run"));
    stdout_json(&out)["body"].clone()
}

#[tokio::test]
async fn send_flow_sends_every_part_and_quiet_prints_the_wamid() {
    let graph = MockServer::start().await;
    let mut body = interactive_message(&json!({
        "type": "flow",
        "header": { "type": "text", "text": "Booking" },
        "body": { "text": "Book a table" },
        "footer": { "text": "Acme" },
        "action": {
            "name": "flow",
            "parameters": {
                "flow_message_version": "3",
                "flow_id": "123456",
                "flow_cta": "Book!",
                "flow_token": "order-7",
                "flow_action": "navigate",
                "mode": "draft",
                "flow_action_payload": {
                    "screen": "START",
                    "data": "{\"plan\":\"gold\"}"
                }
            }
        }
    }));
    body["context"] = json!({ "message_id": "wamid.Q" });
    expect_exact_send(&graph, body).await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "flow",
            TO,
            "--body",
            "Book a table",
            "--header",
            "Booking",
            "--footer",
            "Acme",
            "--flow-id",
            "123456",
            "--cta",
            "Book!",
            "--flow-token",
            "order-7",
            "--action",
            "navigate",
            "--mode",
            "draft",
            "--screen",
            "START",
            "--data",
            r#"{"plan":"gold"}"#,
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[tokio::test]
async fn send_flow_by_name_leaves_the_rest_to_meta() {
    let graph = MockServer::start().await;
    expect_exact_send(
        &graph,
        interactive_message(&json!({
            "type": "flow",
            "body": { "text": "Book" },
            "action": {
                "name": "flow",
                "parameters": {
                    "flow_message_version": "3",
                    "flow_name": "booking_v1",
                    "flow_cta": "Open"
                }
            }
        })),
    )
    .await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "flow",
            TO,
            "--body",
            "Book",
            "--flow-name",
            "booking_v1",
            "--cta",
            "Open",
        ],
    );
}

#[test]
fn send_flow_takes_data_from_a_file_and_a_media_header() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("data.json");
    std::fs::write(&file, r#"{"plan":"gold"}"#).unwrap();
    let body = sent_dry_run_body(&[
        "send",
        "flow",
        TO,
        "--body",
        "Book",
        "--flow-id",
        "1",
        "--cta",
        "Open",
        "--header-image",
        "2762",
        "--data-file",
        file.to_str().unwrap(),
    ]);
    assert_eq!(
        body["interactive"]["header"],
        json!({ "type": "image", "image": { "id": "2762" } })
    );
    assert_eq!(
        body["interactive"]["action"]["parameters"]["flow_action_payload"],
        json!({ "data": "{\"plan\":\"gold\"}" })
    );
}

#[test]
fn send_flow_dry_run_masks_the_flow_token_and_secrets_in_its_data() {
    let body = sent_dry_run_body(&[
        "send",
        "flow",
        TO,
        "--body",
        "Book",
        "--flow-id",
        "1",
        "--cta",
        "Open",
        "--flow-token",
        "TOKEN_SENTINEL",
        "--data",
        r#"{"api_token":"DATA_SENTINEL","u":"https://x.test/?access_token=LINK_SENTINEL","city":"SD"}"#,
    ]);
    let printed = body.to_string();
    for sentinel in ["TOKEN_SENTINEL", "DATA_SENTINEL", "LINK_SENTINEL"] {
        assert!(!printed.contains(sentinel), "{printed}");
    }
    assert!(printed.contains("SD"), "{printed}");
}

#[test]
fn send_interactive_carousel_refuses_a_bad_card_type_url_or_link() {
    let home = TempDir::new().unwrap();
    let card = |kind: &str, link: &str, url: &str| {
        json!({
            "card_index": 0,
            "type": kind,
            "header": { "type": "image", "image": { "link": link } },
            "action": {
                "name": "cta_url",
                "parameters": { "display_text": "Open", "url": url }
            }
        })
    };
    let good = "https://x.test/a.png";
    let bad = [
        card("bogus", good, "https://x.test"),
        card("cta_url", &format!(" {good}"), "https://x.test"),
        card("cta_url", good, "not a url"),
    ];
    for bad_card in bad {
        let file = home.path().join("cards.json");
        std::fs::write(
            &file,
            json!([bad_card, card("cta_url", good, "https://x.test")]).to_string(),
        )
        .unwrap();
        paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args([
                "send",
                "interactive",
                "carousel",
                "15551234567",
                "--body",
                "b",
            ])
            .arg("--card-file")
            .arg(&file)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty());
    }
}

#[test]
fn message_history_trims_the_wamid() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args(["message", "history", " wamid.X ", "--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let requests = stdout_json(&out);
    let lookup = requests[0]["url"].as_str().unwrap();
    assert!(lookup.contains("message_id=wamid.X"), "{lookup}");
    assert!(!lookup.contains("%20"), "{lookup}");
}

#[test]
fn send_flow_refuses_bad_values_before_any_context() {
    let base = [
        "send",
        "flow",
        TO,
        "--body",
        "Book",
        "--flow-id",
        "1",
        "--cta",
        "Open",
    ];
    for extra in [
        vec!["--data", "not json"],
        vec!["--data", "[1]"],
        vec!["--data", "{}"],
        vec!["--action", "data_exchange", "--screen", "START"],
        vec!["--action", "data_exchange", "--data", r#"{"a":1}"#],
        vec!["--flow-token", " "],
        vec!["--screen", " "],
        vec!["--reply-to", " "],
        vec!["--header-image", "ftp://x"],
    ] {
        let mut args = base.to_vec();
        args.extend(extra);
        assert_refused_locally(&args);
    }
    assert_refused_locally(&[
        "send",
        "flow",
        TO,
        "--body",
        " ",
        "--flow-id",
        "1",
        "--cta",
        "Open",
    ]);
    assert_refused_locally(&[
        "send",
        "flow",
        TO,
        "--body",
        "Book",
        "--flow-id",
        " ",
        "--cta",
        "Open",
    ]);
    assert_refused_locally(&[
        "send",
        "flow",
        TO,
        "--body",
        "Book",
        "--flow-id",
        "1",
        "--cta",
        " ",
    ]);
    assert_refused_locally(&[
        "send",
        "flow",
        "abc",
        "--body",
        "Book",
        "--flow-id",
        "1",
        "--cta",
        "Open",
    ]);
}

#[test]
fn send_flow_closed_sets_and_exclusive_flags_are_usage_errors() {
    let home = TempDir::new().unwrap();
    for args in [
        vec!["--body", "B", "--cta", "C"],
        vec![
            "--body",
            "B",
            "--cta",
            "C",
            "--flow-id",
            "1",
            "--flow-name",
            "n",
        ],
        vec![
            "--body",
            "B",
            "--cta",
            "C",
            "--flow-id",
            "1",
            "--action",
            "jump",
        ],
        vec![
            "--body",
            "B",
            "--cta",
            "C",
            "--flow-id",
            "1",
            "--mode",
            "live",
        ],
        vec![
            "--body",
            "B",
            "--cta",
            "C",
            "--flow-id",
            "1",
            "--data",
            "{}",
            "--data-file",
            "x",
        ],
        vec![
            "--body",
            "B",
            "--cta",
            "C",
            "--flow-id",
            "1",
            "--header",
            "H",
            "--header-image",
            "1",
        ],
    ] {
        paloma(None, &home)
            .args(["send", "flow", TO])
            .args(args)
            .assert()
            .code(2);
    }
}

#[test]
fn send_flow_warns_on_a_long_cta_but_sends_it() {
    let home = TempDir::new().unwrap();
    let cta = "x".repeat(31);
    paloma(None, &home)
        .args(["send", "flow", TO, "--body", "B", "--flow-id", "1"])
        .args(["--cta", cta.as_str(), "--dry-run", "-o", "json"])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning: --cta"));
}

#[tokio::test]
async fn location_request_sends_the_send_location_action() {
    let graph = MockServer::start().await;
    let mut body = interactive_message(&json!({
        "type": "location_request_message",
        "body": { "text": "Where to?" },
        "action": { "name": "send_location" }
    }));
    body["context"] = json!({ "message_id": "wamid.Q" });
    expect_exact_send(&graph, body).await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "interactive",
            "location-request",
            TO,
            "--body",
            "Where to?",
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[test]
fn location_request_checks_its_body_and_allows_no_header() {
    let long = "x".repeat(1025);
    let fine = "x".repeat(1024);
    let cmd = ["send", "interactive", "location-request", TO, "--body"];
    let with = |body: &'static str| {
        let mut args = cmd.to_vec();
        args.push(body);
        args
    };
    assert_refused_locally(&with(" "));
    let mut too_long = cmd.to_vec();
    too_long.push(&long);
    assert_refused_locally(&too_long);
    let mut at_limit = cmd.to_vec();
    at_limit.push(&fine);
    sent_dry_run_body(&at_limit);
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(with("Where?"))
        .args(["--header", "H"])
        .assert()
        .code(2);
}

#[tokio::test]
async fn address_sends_a_country_with_optional_text() {
    let graph = MockServer::start().await;
    expect_exact_send(
        &graph,
        interactive_message(&json!({
            "type": "address_message",
            "header": { "type": "text", "text": "Delivery" },
            "body": { "text": "Where should it go?" },
            "footer": { "text": "Acme" },
            "action": { "name": "address_message", "parameters": { "country": "IN" } }
        })),
    )
    .await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "interactive",
            "address",
            TO,
            "--body",
            "Where should it go?",
            "--header",
            "Delivery",
            "--footer",
            "Acme",
            "--country",
            "IN",
        ],
    );
}

#[test]
fn address_doesnt_block_other_regions_and_takes_a_parameters_file() {
    let body = sent_dry_run_body(&[
        "send",
        "interactive",
        "address",
        TO,
        "--body",
        "Where?",
        "--country",
        "SG",
    ]);
    assert_eq!(
        body["interactive"]["action"]["parameters"],
        json!({ "country": "SG" })
    );
    let home = TempDir::new().unwrap();
    let file = home.path().join("address.json");
    let parameters = json!({
        "country": "IN",
        "values": { "name": "Ana", "in_pin_code": "666666" },
        "validation_errors": { "in_pin_code": "We could not locate this pin code." },
        "saved_addresses": [{ "id": "a1", "value": { "city": "Mumbai" } }],
    });
    std::fs::write(&file, parameters.to_string()).unwrap();
    let body = sent_dry_run_body(&[
        "send",
        "interactive",
        "address",
        TO,
        "--body",
        "Where?",
        "--parameters-file",
        file.to_str().unwrap(),
    ]);
    assert_eq!(body["interactive"]["action"]["parameters"], parameters);
}

#[test]
fn address_refuses_bad_parameters_and_needs_a_country() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("address.json");
    let file_str = file.to_str().unwrap();
    for bad in [
        json!([]),
        json!({}),
        json!({ "country": " " }),
        json!({ "country": "IN", "values": [] }),
        json!({ "country": "IN", "saved_addresses": {} }),
        json!({ "country": "IN", "validation_errors": "x" }),
    ] {
        std::fs::write(&file, bad.to_string()).unwrap();
        assert_refused_locally(&[
            "send",
            "interactive",
            "address",
            TO,
            "--body",
            "Where?",
            "--parameters-file",
            file_str,
        ]);
    }
    assert_refused_locally(&[
        "send",
        "interactive",
        "address",
        TO,
        "--body",
        "Where?",
        "--country",
        " ",
    ]);
    paloma(None, &home)
        .args(["send", "interactive", "address", TO, "--body", "Where?"])
        .assert()
        .code(2);
}

fn url_card(name: &str) -> Value {
    json!({
        "header": { "type": "image", "image": { "link": format!("https://x.test/{name}.png") } },
        "body": { "text": name },
        "action": {
            "name": "cta_url",
            "parameters": { "display_text": "Buy", "url": format!("https://x.test/{name}") }
        }
    })
}

#[tokio::test]
async fn carousel_card_file_fills_index_and_type_and_keeps_the_rest() {
    let graph = MockServer::start().await;
    let filled = |index: usize, name: &str| {
        let mut card = url_card(name);
        card["card_index"] = index.into();
        card["type"] = "cta_url".into();
        card
    };
    let mut body = interactive_message(&json!({
        "type": "carousel",
        "body": { "text": "New in" },
        "action": { "cards": [filled(0, "a"), filled(1, "b")] }
    }));
    body["context"] = json!({ "message_id": "wamid.Q" });
    expect_exact_send(&graph, body).await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("cards.json");
    std::fs::write(
        &file,
        json!({ "cards": [url_card("a"), url_card("b")] }).to_string(),
    )
    .unwrap();
    assert_quiet_send(
        &graph,
        &[
            "send",
            "interactive",
            "carousel",
            TO,
            "--body",
            "New in",
            "--card-file",
            file.to_str().unwrap(),
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[test]
fn carousel_card_file_keeps_quick_replies_and_a_bare_array() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("cards.json");
    let replies = |name: &str| {
        json!({
            "card_index": 0,
            "type": "cta_url",
            "header": { "type": "video", "video": { "link": "https://x.test/v.mp4" } },
            "action": { "buttons": [
                { "type": "quick_reply", "quick_reply": { "id": name, "title": "More" } }
            ] }
        })
    };
    let mut second = replies("b");
    second["card_index"] = 1.into();
    let cards = json!([replies("a"), second]);
    std::fs::write(&file, cards.to_string()).unwrap();
    let body = sent_dry_run_body(&[
        "send",
        "interactive",
        "carousel",
        TO,
        "--body",
        "New in",
        "--card-file",
        file.to_str().unwrap(),
    ]);
    assert_eq!(body["interactive"]["action"]["cards"], cards);
}

#[test]
fn carousel_card_file_refuses_what_meta_would_without_echoing_it() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("cards.json");
    let file_str = file.to_str().unwrap().to_owned();
    let refused = |cards: &Value| {
        std::fs::write(&file, cards.to_string()).unwrap();
        let out = paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .env_remove("PALOMA_TOKEN")
            .args(["send", "interactive", "carousel", TO, "--body", "B"])
            .args(["--card-file", file_str.as_str()])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(5), "{cards}");
        assert!(out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("SENTINEL"));
    };
    refused(&json!({ "other": [] }));
    refused(&json!([url_card("a")]));
    refused(&Value::Array(vec![url_card("a"); 11]));
    let mut no_header = url_card("a");
    no_header.as_object_mut().unwrap().remove("header");
    refused(&json!([no_header, url_card("b")]));
    let mut long_body = url_card("a");
    long_body["body"]["text"] = format!("SENTINEL{}", "x".repeat(160)).into();
    refused(&json!([long_body, url_card("b")]));
    let mut product = url_card("a");
    product["type"] = "product".into();
    refused(&json!([product, url_card("b")]));
    let mut replies = url_card("a");
    replies["action"] = json!({ "buttons": [
        { "type": "quick_reply", "quick_reply": { "id": "x", "title": "More" } }
    ] });
    refused(&json!([replies, url_card("b")]));
}

#[tokio::test]
async fn product_carousel_makes_one_card_per_product() {
    let graph = MockServer::start().await;
    let card = |index: usize, sku: &str| {
        json!({
            "card_index": index,
            "type": "product",
            "action": { "product_retailer_id": sku, "catalog_id": "CAT" }
        })
    };
    expect_exact_send(
        &graph,
        interactive_message(&json!({
            "type": "carousel",
            "body": { "text": "Featured" },
            "action": { "cards": [card(0, "abc"), card(1, "def")] }
        })),
    )
    .await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "interactive",
            "carousel",
            TO,
            "--body",
            "Featured",
            "--products",
            "abc,def",
            "--catalog-id",
            "CAT",
        ],
    );
}

#[test]
fn product_carousel_checks_its_count_and_its_flags() {
    let cmd = ["send", "interactive", "carousel", TO, "--body", "B"];
    let many = ["s"; 11].join(",");
    for products in ["a", many.as_str(), "a,,b"] {
        let mut args = cmd.to_vec();
        args.extend(["--products", products, "--catalog-id", "CAT"]);
        assert_refused_locally(&args);
    }
    let mut blank_catalog = cmd.to_vec();
    blank_catalog.extend(["--products", "a,b", "--catalog-id", " "]);
    assert_refused_locally(&blank_catalog);
    let home = TempDir::new().unwrap();
    for extra in [
        vec![],
        vec!["--products", "a,b"],
        vec!["--catalog-id", "CAT"],
        vec!["--products", "a,b", "--catalog-id", "C", "--card-file", "x"],
        vec!["--products", "a,b", "--catalog-id", "C", "--footer", "F"],
    ] {
        paloma(None, &home).args(cmd).args(extra).assert().code(2);
    }
}

#[tokio::test]
async fn call_button_sends_its_parameters() {
    let graph = MockServer::start().await;
    let mut body = interactive_message(&json!({
        "type": "voice_call",
        "body": { "text": "Call us" },
        "action": {
            "name": "voice_call",
            "parameters": {
                "display_text": "Call on WhatsApp",
                "ttl_minutes": 100,
                "payload": "campaign-7"
            }
        }
    }));
    body["context"] = json!({ "message_id": "wamid.Q" });
    expect_exact_send(&graph, body).await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "interactive",
            "call",
            TO,
            "--body",
            "Call us",
            "--display",
            "Call on WhatsApp",
            "--ttl-minutes",
            "100",
            "--payload",
            "campaign-7",
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[test]
fn call_button_leaves_parameters_out_and_checks_its_limits() {
    let cmd = ["send", "interactive", "call", TO, "--body", "Call"];
    assert_eq!(
        sent_dry_run_body(&cmd)["interactive"]["action"],
        json!({ "name": "voice_call" })
    );
    let long_label = "x".repeat(21);
    let long_payload = "x".repeat(513);
    for extra in [
        vec!["--ttl-minutes", "0"],
        vec!["--ttl-minutes", "43201"],
        vec!["--display", long_label.as_str()],
        vec!["--display", " "],
        vec!["--payload", long_payload.as_str()],
    ] {
        let mut args = cmd.to_vec();
        args.extend(extra);
        assert_refused_locally(&args);
    }
    let mut at_limit = cmd.to_vec();
    at_limit.extend(["--ttl-minutes", "43200"]);
    assert_eq!(
        sent_dry_run_body(&at_limit)["interactive"]["action"]["parameters"]["ttl_minutes"],
        43200
    );
}

#[tokio::test]
async fn request_contact_sends_the_request_contact_info_action() {
    let graph = MockServer::start().await;
    let mut body = interactive_message(&json!({
        "type": "request_contact_info",
        "body": { "text": "Share your number?" },
        "action": { "name": "request_contact_info" }
    }));
    body["context"] = json!({ "message_id": "wamid.Q" });
    expect_exact_send(&graph, body).await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "interactive",
            "request-contact",
            TO,
            "--body",
            "Share your number?",
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[test]
fn request_contact_goes_to_a_bsuid_and_refuses_a_blank_body() {
    let body = sent_dry_run_body(&[
        "send",
        "interactive",
        "request-contact",
        BSUID,
        "--body",
        "Share?",
    ]);
    assert_eq!(body["recipient"], BSUID);
    assert!(body.get("to").is_none());
    assert_refused_locally(&["send", "interactive", "request-contact", TO, "--body", " "]);
}

#[tokio::test]
async fn call_permission_sends_a_body_to_a_bsuid() {
    let graph = MockServer::start().await;
    expect_exact_send(
        &graph,
        json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "recipient": BSUID,
            "type": "interactive",
            "context": { "message_id": "wamid.Q" },
            "interactive": {
                "type": "call_permission_request",
                "body": { "text": "May we call about your order?" },
                "action": { "name": "call_permission_request" }
            }
        }),
    )
    .await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "call-permission",
            BSUID,
            "--body",
            "May we call about your order?",
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[test]
fn call_permission_body_is_optional_and_it_has_no_header_or_footer() {
    assert_eq!(
        sent_dry_run_body(&["send", "call-permission", TO])["interactive"],
        json!({
            "type": "call_permission_request",
            "action": { "name": "call_permission_request" }
        })
    );
    assert_refused_locally(&["send", "call-permission", TO, "--body", " "]);
    let home = TempDir::new().unwrap();
    for flag in ["--header", "--footer"] {
        paloma(None, &home)
            .args(["send", "call-permission", TO, flag, "x"])
            .assert()
            .code(2);
    }
}

#[tokio::test]
async fn product_sends_the_catalog_and_retailer_ids() {
    let graph = MockServer::start().await;
    let mut body = interactive_message(&json!({
        "type": "product",
        "body": { "text": "This one" },
        "footer": { "text": "Acme" },
        "action": { "catalog_id": "CAT", "product_retailer_id": "SKU1" }
    }));
    body["context"] = json!({ "message_id": "wamid.Q" });
    expect_exact_send(&graph, body).await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "product",
            TO,
            "--catalog-id",
            "CAT",
            "--product",
            "SKU1",
            "--body",
            "This one",
            "--footer",
            "Acme",
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[test]
fn product_body_is_optional_and_a_header_isnt_allowed() {
    let cmd = [
        "send",
        "product",
        TO,
        "--catalog-id",
        "CAT",
        "--product",
        "SKU1",
    ];
    assert_eq!(
        sent_dry_run_body(&cmd)["interactive"],
        json!({
            "type": "product",
            "action": { "catalog_id": "CAT", "product_retailer_id": "SKU1" }
        })
    );
    let long_footer = "x".repeat(61);
    for extra in [vec!["--body", " "], vec!["--footer", long_footer.as_str()]] {
        let mut args = cmd.to_vec();
        args.extend(extra);
        assert_refused_locally(&args);
    }
    assert_refused_locally(&["send", "product", TO, "--catalog-id", " ", "--product", "S"]);
    assert_refused_locally(&["send", "product", TO, "--catalog-id", "C", "--product", " "]);
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(cmd)
        .args(["--header", "H"])
        .assert()
        .code(2);
}

#[tokio::test]
async fn product_list_sends_its_sections() {
    let graph = MockServer::start().await;
    let mut body = interactive_message(&json!({
        "type": "product_list",
        "header": { "type": "text", "text": "Our picks" },
        "body": { "text": "Have a look" },
        "footer": { "text": "Acme" },
        "action": {
            "catalog_id": "CAT",
            "sections": [
                { "title": "Cakes", "product_items": [
                    { "product_retailer_id": "c1" },
                    { "product_retailer_id": "c,2" }
                ] },
                { "title": "Drinks", "product_items": [
                    { "product_retailer_id": "d1" }
                ] }
            ]
        }
    }));
    body["context"] = json!({ "message_id": "wamid.Q" });
    expect_exact_send(&graph, body).await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "product-list",
            TO,
            "--catalog-id",
            "CAT",
            "--header",
            "Our picks",
            "--body",
            "Have a look",
            "--footer",
            "Acme",
            "--section",
            r"Cakes:c1,c\,2",
            "--section",
            "Drinks:d1",
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[test]
fn product_list_checks_its_limits_and_requires_a_header() {
    let cmd = [
        "send",
        "product-list",
        TO,
        "--catalog-id",
        "CAT",
        "--header",
        "H",
        "--body",
        "B",
    ];
    let mut valid = cmd.to_vec();
    valid.extend(["--section", "A:s"]);
    assert_eq!(
        sent_dry_run_body(&valid)["interactive"]["action"]["sections"],
        json!([{ "title": "A", "product_items": [{ "product_retailer_id": "s" }] }])
    );
    let thirty_one = format!("A:{}", ["s"; 31].join(","));
    let long_title = format!("{}:s", "x".repeat(25));
    for section in [
        "NoColon",
        "Empty:",
        ":s",
        thirty_one.as_str(),
        long_title.as_str(),
    ] {
        let mut args = cmd.to_vec();
        args.extend(["--section", section]);
        assert_refused_locally(&args);
    }
    let mut eleven = cmd.to_vec();
    for _ in 0..11 {
        eleven.extend(["--section", "A:s"]);
    }
    assert_refused_locally(&eleven);
    let long_header = "x".repeat(61);
    assert_refused_locally(&[
        "send",
        "product-list",
        TO,
        "--catalog-id",
        "CAT",
        "--header",
        &long_header,
        "--body",
        "B",
        "--section",
        "A:s",
    ]);
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(["send", "product-list", TO, "--catalog-id", "C"])
        .args(["--body", "B", "--section", "A:s"])
        .assert()
        .code(2);
}

#[tokio::test]
async fn catalog_sends_a_thumbnail_and_footer() {
    let graph = MockServer::start().await;
    let mut body = interactive_message(&json!({
        "type": "catalog_message",
        "body": { "text": "Browse" },
        "footer": { "text": "Acme" },
        "action": {
            "name": "catalog_message",
            "parameters": { "thumbnail_product_retailer_id": "SKU1" }
        }
    }));
    body["context"] = json!({ "message_id": "wamid.Q" });
    expect_exact_send(&graph, body).await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "catalog",
            TO,
            "--body",
            "Browse",
            "--footer",
            "Acme",
            "--thumbnail",
            "SKU1",
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[test]
fn catalog_leaves_parameters_out_and_checks_its_limits() {
    assert_eq!(
        sent_dry_run_body(&["send", "catalog", TO, "--body", "Browse"])["interactive"],
        json!({
            "type": "catalog_message",
            "body": { "text": "Browse" },
            "action": { "name": "catalog_message" }
        })
    );
    let long_body = "x".repeat(1025);
    assert_refused_locally(&["send", "catalog", TO, "--body", &long_body]);
    assert_refused_locally(&["send", "catalog", TO, "--body", " "]);
    assert_refused_locally(&["send", "catalog", TO, "--body", "B", "--thumbnail", " "]);
}

#[test]
fn the_new_send_verbs_honor_the_window_guardrail() {
    let home = TempDir::new().unwrap();
    for args in [
        vec!["flow", TO, "--body", "B", "--flow-id", "1", "--cta", "C"],
        vec!["call-permission", TO],
        vec!["product", TO, "--catalog-id", "C", "--product", "S"],
        vec!["catalog", TO, "--body", "B"],
        vec!["interactive", "request-contact", TO, "--body", "B"],
        vec!["interactive", "location-request", TO, "--body", "B"],
        vec!["interactive", "call", TO, "--body", "B"],
        vec![
            "interactive",
            "address",
            TO,
            "--body",
            "B",
            "--country",
            "IN",
        ],
        vec![
            "interactive",
            "carousel",
            TO,
            "--body",
            "B",
            "--products",
            "a,b",
            "--catalog-id",
            "C",
        ],
        vec![
            "product-list",
            TO,
            "--catalog-id",
            "C",
            "--header",
            "H",
            "--body",
            "B",
            "--section",
            "A:s",
        ],
    ] {
        paloma(None, &home)
            .arg("send")
            .args(args)
            .args(["--strict", "--dry-run"])
            .assert()
            .code(6);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn account_get_requests_details_and_preserves_output_contracts() {
    let graph = MockServer::start().await;
    let body = json!({"id": WABA, "name": "Example", "account_review_status": "APPROVED"});
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}")))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .and(query_param("fields", "id,name,timezone_id,message_template_namespace,account_review_status,business_verification_status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(3)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["account", "get"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Example").and(predicate::str::contains("APPROVED")))
        .stderr("");
    paloma(Some(&graph), &home)
        .args(["account", "get", "-q"])
        .assert()
        .success()
        .stdout(format!("{WABA}\n"))
        .stderr("");
    let out = paloma(Some(&graph), &home)
        .args(["account", "get", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    assert_eq!(stdout_json(&out), body);
    assert!(
        graph
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.body.is_empty())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn account_get_custom_fields_and_health_use_the_waba_flag() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (verb, flags, fields, body) in [
        (
            "get",
            vec!["--fields", "id,name,service_pricing_policy"],
            "id,name,service_pricing_policy",
            json!({"id":"999", "name":"Other", "service_pricing_policy":"PAID_EXEMPT"}),
        ),
        (
            "health",
            vec![],
            "health_status",
            json!({"id":"999", "health_status":{"can_send_message":"BLOCKED", "entities":[{"entity_type":"WABA", "id":"999", "can_send_message":"BLOCKED", "errors":[{"error_code":141_006}]}]}}),
        ),
    ] {
        Mock::given(method("GET"))
            .and(path("/v26.0/999"))
            .and(query_param("fields", fields))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .expect(2)
            .mount(&graph)
            .await;
        let out = paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["account", verb, "--waba-id", "999", "-o", "json"])
            .args(&flags)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(out.stderr.is_empty());
        assert_eq!(stdout_json(&out), body);
        paloma(Some(&graph), &home)
            .args(["account", verb, "--waba-id", "999", "-q"])
            .args(&flags)
            .assert()
            .success()
            .stdout("999\n")
            .stderr("");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn account_lists_render_nested_apps_and_limits_without_a_phone_context() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (verb, edge, body, expected) in [
        (
            "limits",
            "phone_numbers",
            json!({"data":[{"id":PHONE, "display_phone_number":"15551234567", "whatsapp_business_manager_messaging_limit":"TIER_250"}]}),
            "TIER_250",
        ),
        (
            "subscribed-apps",
            "subscribed_apps",
            json!({"data":[{"whatsapp_business_api_data":{"id":"333", "name":"Example App"}, "override_callback_uri":"https://example.com/webhook"}]}),
            "Example App",
        ),
    ] {
        let mut mock = Mock::given(method("GET"))
            .and(path(format!("/v26.0/{WABA}/{edge}")))
            .and(header("authorization", format!("Bearer {TOKEN}")));
        if verb == "limits" {
            mock = mock.and(query_param(
                "fields",
                "id,display_phone_number,whatsapp_business_manager_messaging_limit",
            ));
        }
        mock.respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .expect(3)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["account", verb])
            .assert()
            .success()
            .stdout(predicate::str::contains(expected))
            .stderr("");
        let out = paloma(Some(&graph), &home)
            .args(["account", verb, "-o", "json"])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(out.stderr.is_empty());
        assert_eq!(stdout_json(&out), body);
        paloma(Some(&graph), &home)
            .args(["account", verb, "-q"])
            .assert()
            .success()
            .stdout("")
            .stderr("");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn account_lists_warn_and_follow_cursors_with_all() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (verb, edge) in [
        ("limits", "phone_numbers"),
        ("subscribed-apps", "subscribed_apps"),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/v26.0/{WABA}/{edge}")))
            .and(query_param("after", "start"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data":[{"id":"first"}],
                "paging":{"cursors":{"after":"next"}, "next":"https://graph.facebook.com/next"}
            })))
            .expect(2)
            .mount(&graph)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/v26.0/{WABA}/{edge}")))
            .and(query_param("after", "next"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data":[{"id":"second"}]})),
            )
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args(["account", verb, "--after", "start", "-o", "json"])
            .assert()
            .success()
            .stdout(predicate::str::contains("first"))
            .stderr(predicate::str::contains("more results exist"));
        let out = paloma(Some(&graph), &home)
            .args(["account", verb, "--after", "start", "--all", "-o", "json"])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(out.stderr.is_empty());
        assert_eq!(
            stdout_json(&out)["data"],
            json!([{"id":"first"}, {"id":"second"}])
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn account_reads_dry_run_without_network_or_token() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for verb in ["get", "health", "limits", "subscribed-apps"] {
        let out = paloma(Some(&graph), &home)
            .env_remove("PALOMA_TOKEN")
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["account", verb, "--dry-run", "-o", "json"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let request = stdout_json(&out);
        assert_eq!(request["method"], "GET");
        assert!(
            request["url"]
                .as_str()
                .unwrap()
                .contains(&format!("/v26.0/{WABA}"))
        );
        assert!(!request.to_string().contains(TOKEN));
        assert!(String::from_utf8_lossy(&out.stderr).contains("dry-run: no request sent"));
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[test]
fn account_reads_require_waba_and_validate_fields_before_context() {
    let home = TempDir::new().unwrap();
    for verb in ["get", "health", "limits", "subscribed-apps"] {
        paloma(None, &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["account", verb, "--dry-run"])
            .assert()
            .code(3)
            .stdout("")
            .stderr(predicate::str::contains("--waba-id"));
    }
    paloma(None, &home)
        .env_remove("PALOMA_WABA_ID")
        .args(["account", "get", "--fields", "  "])
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("fields"));
    paloma(None, &home)
        .env_remove("PALOMA_WABA_ID")
        .args(["account", "analytics"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains("--start"));
}

#[tokio::test(flavor = "multi_thread")]
async fn billing_currency_start_posts_optional_credit_and_quiet_prints_migration_id() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for credit in [None, Some("12345")] {
        let mut body = json!({"currency":"INR"});
        if let Some(id) = credit {
            body["extended_credit_id"] = json!(id);
        }
        Mock::given(method("POST"))
            .and(path(format!(
                "/v26.0/{WABA}/set_payment_method_migration_intent"
            )))
            .and(header("authorization", format!("Bearer {TOKEN}")))
            .and(body_json(body))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!({"migration_id":"mig_123", "migration_status":"INITIATED"}),
                ),
            )
            .expect(1)
            .mount(&graph)
            .await;
        let mut cmd = paloma(Some(&graph), &home);
        cmd.args([
            "account",
            "billing-currency",
            "start",
            "--currency",
            "INR",
            "--yes",
            "-q",
        ]);
        if let Some(id) = credit {
            cmd.args(["--extended-credit-id", id]);
        }
        cmd.assert().success().stdout("mig_123\n").stderr("");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn billing_currency_status_and_complete_use_migration_context_and_check_readiness() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (id, status, succeeds) in [
        ("mig_ready", "READY_TO_COMPLETE", true),
        ("mig_retry", "FAILED_TO_COMPLETE", true),
        ("mig_busy", "IN_PROGRESS", false),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/v26.0/{id}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!({"id":id, "status":status, "destination_waba":{"id":"999"}}),
                ),
            )
            .expect(2)
            .mount(&graph)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{id}/resume_migration")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"id":id, "status":"COMPLETED"})),
            )
            .expect(u64::from(succeeds))
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["account", "billing-currency", "status", id, "-q"])
            .assert()
            .success()
            .stdout(format!("{id}\n"))
            .stderr("");
        let mut cmd = paloma(Some(&graph), &home);
        cmd.env_remove("PALOMA_WABA_ID")
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["account", "billing-currency", "complete", id, "--yes", "-q"]);
        if succeeds {
            cmd.assert().success().stdout(format!("{id}\n")).stderr("");
        } else {
            cmd.assert()
                .code(1)
                .stdout("")
                .stderr(predicate::str::contains("not ready to complete"));
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn billing_currency_dry_run_and_confirmation_never_send_requests() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for args in [
        vec!["start", "--currency", "BRL"],
        vec!["complete", "mig_123"],
    ] {
        let out = paloma(Some(&graph), &home)
            .env_remove("PALOMA_TOKEN")
            .args(["account", "billing-currency"])
            .args(&args)
            .args(["--dry-run", "-o", "json"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let request = stdout_json(&out);
        if args[0] == "complete" {
            assert_eq!(request[0]["method"], "GET");
            assert_eq!(request[1]["method"], "POST");
        } else {
            assert_eq!(request["method"], "POST");
        }
        assert!(String::from_utf8_lossy(&out.stderr).contains("dry-run: no request sent"));
        paloma(Some(&graph), &home)
            .args(["account", "billing-currency"])
            .args(&args)
            .assert()
            .code(5)
            .stdout("")
            .stderr(predicate::str::contains("--yes"));
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[test]
fn billing_currency_invalid_inputs_fail_before_context() {
    let home = TempDir::new().unwrap();
    for args in [
        vec!["start", "--currency", "inr"],
        vec!["start", "--currency", "INR", "--extended-credit-id", "../x"],
        vec!["status", "../x"],
        vec!["complete", "mig_123?access_token=x"],
    ] {
        paloma(None, &home)
            .env_remove("PALOMA_WABA_ID")
            .env_remove("PALOMA_TOKEN")
            .args(["account", "billing-currency"])
            .args(&args)
            .assert()
            .code(5)
            .stdout("");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn account_set_shapes_settings_including_false_and_supports_file() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}")))
        .and(body_json(json!({"name":"New name", "timezone_id":"1", "disable_marketing_messages_on_cloud_api":false})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1).mount(&graph).await;
    paloma(Some(&graph), &home)
        .args([
            "account",
            "set",
            "--name",
            "New name",
            "--timezone-id",
            "1",
            "--disable-cloud-marketing",
            "false",
            "-q",
        ])
        .assert()
        .success()
        .stdout("")
        .stderr("");
    let file = home.path().join("account.json");
    std::fs::write(&file, r#"{"name":"From file"}"#).unwrap();
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}")))
        .and(body_json(json!({"name":"From file"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["account", "set", "--file"])
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("true"))
        .stderr("");
    paloma(Some(&graph), &home)
        .args(["account", "set", "--disable-cloud-marketing", "true"])
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("--yes"));
    let out = paloma(Some(&graph), &home)
        .args([
            "account",
            "set",
            "--disable-cloud-marketing",
            "true",
            "--dry-run",
            "-o",
            "json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        stdout_json(&out)["body"],
        json!({"disable_marketing_messages_on_cloud_api":true})
    );
}

#[test]
fn account_set_rejects_empty_settings_and_nonobjects_before_context() {
    let home = TempDir::new().unwrap();
    for args in [vec![], vec!["--name", " "], vec!["--timezone-id", ""]] {
        paloma(None, &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["account", "set"])
            .args(&args)
            .assert()
            .code(5)
            .stdout("");
    }
    let file = home.path().join("bad.json");
    for body in [
        "[]",
        "{}",
        "null",
        r#"{"name":" "}"#,
        r#"{"timezone_id":0}"#,
        r#"{"disable_marketing_messages_on_cloud_api":"true"}"#,
    ] {
        std::fs::write(&file, body).unwrap();
        paloma(None, &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["account", "set", "--file"])
            .arg(&file)
            .assert()
            .code(5)
            .stdout("");
    }
    paloma(None, &home)
        .args(["account", "set", "--name", "x", "--file"])
        .arg(&file)
        .assert()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn account_activities_and_number_health_request_the_documented_fields() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/activities")))
        .and(query_param(
            "fields",
            "id,actor_business_id,actor_id,category,event_type,timestamp",
        ))
        .and(query_param("after", "cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"data":[{"event_type":"ACCOUNT_UPDATE", "timestamp":"2026-11-04"}]}),
        ))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["account", "activities", "--after", "cursor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ACCOUNT_UPDATE"))
        .stderr("");
    Mock::given(method("GET"))
        .and(path("/v26.0/999"))
        .and(query_param("fields", "health_status"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"id":"999", "health_status":{"can_send_message":"AVAILABLE"}}),
            ),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_WABA_ID")
        .args(["number", "health", "999", "-q"])
        .assert()
        .success()
        .stdout("999\n")
        .stderr("");
}

#[tokio::test(flavor = "multi_thread")]
async fn qr_lifecycle_uses_collection_posts_and_scoped_get_delete() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let code = "4O4YGZEG3RIVE1";
    for (args, body) in [
        (
            vec!["create", "--message", "Hello"],
            json!({"prefilled_message":"Hello"}),
        ),
        (
            vec!["update", code, "--message", "Updated"],
            json!({"code":code,"prefilled_message":"Updated"}),
        ),
    ] {
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/message_qrdls")))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"code":code,"deep_link_url":"https://wa.me/message/example"}),
            ))
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args(["qr"])
            .args(&args)
            .arg("-q")
            .assert()
            .success()
            .stdout(format!("{code}\n"))
            .stderr("");
    }
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}/message_qrdls/{code}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data":[{"code":code,"prefilled_message":"Hello"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["qr", "get", code, "-q"])
        .assert()
        .success()
        .stdout(format!("{code}\n"))
        .stderr("");
    Mock::given(method("DELETE"))
        .and(path(format!("/v26.0/{PHONE}/message_qrdls/{code}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["qr", "delete", code, "--yes", "-q"])
        .assert()
        .success()
        .stdout(format!("{code}\n"))
        .stderr("");
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}/message_qrdls")))
        .and(query_param("limit", "25"))
        .and(query_param("after", "cursor"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data":[{"code":code,"prefilled_message":"Hello"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["qr", "list", "--limit", "25", "--after", "cursor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Hello"))
        .stderr("");
}

#[tokio::test(flavor = "multi_thread")]
async fn qr_image_download_omits_authorization_and_preserves_graph_output() {
    let graph = MockServer::start().await;
    let cdn = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let code = "4O4YGZEG3RIVE1";
    let body = json!({"code":code,"qr_image_url":format!("{}/qr.svg",cdn.uri())});
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/message_qrdls")))
        .and(body_json(
            json!({"prefilled_message":"Hello","generate_qr_image":"SVG"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path("/qr.svg"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<svg/>"))
        .expect(1)
        .mount(&cdn)
        .await;
    let dest = home.path().join("qr.svg");
    let out = paloma(Some(&graph), &home)
        .args([
            "qr",
            "create",
            "--message",
            "Hello",
            "--image",
            "svg",
            "--out",
        ])
        .arg(&dest)
        .args(["-o", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(stdout_json(&out), body);
    assert!(String::from_utf8_lossy(&out.stderr).contains("wrote 6 bytes"));
    assert_eq!(std::fs::read(&dest).unwrap(), b"<svg/>");
    assert!(
        !cdn.received_requests().await.unwrap()[0]
            .headers
            .contains_key("authorization")
    );
    assert!(
        graph.received_requests().await.unwrap()[0]
            .headers
            .contains_key("authorization")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn qr_dry_run_prints_both_image_steps_and_invalid_input_never_sends() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let dest = home.path().join("qr.png");
    let out = paloma(Some(&graph), &home)
        .env_remove("PALOMA_TOKEN")
        .args(["qr", "get", "4O4YGZEG3RIVE1", "--image", "png", "--out"])
        .arg(&dest)
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let requests = stdout_json(&out);
    assert_eq!(requests.as_array().unwrap().len(), 2);
    assert!(
        requests[0]["url"]
            .as_str()
            .unwrap()
            .contains("qr_image_url.format%28PNG%29")
    );
    assert_eq!(requests[1]["method"], "GET");
    assert!(!dest.exists());
    for args in [
        vec!["get", "../bad"],
        vec!["create", "--message", &"x".repeat(141)],
        vec!["list", "--limit", "26"],
    ] {
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["qr"])
            .args(&args)
            .assert()
            .code(5)
            .stdout("");
    }
    paloma(Some(&graph), &home)
        .args(["qr", "delete", "4O4YGZEG3RIVE1"])
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("--yes"));
    std::fs::write(&dest, b"existing").unwrap();
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["qr", "create", "--message", "x", "--image", "png", "--out"])
        .arg(&dest)
        .assert()
        .code(5)
        .stdout("");
    assert_eq!(std::fs::read(&dest).unwrap(), b"existing");
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn automation_reads_and_writes_typed_and_cleared_components() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}")))
        .and(query_param("fields", "conversational_automation"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"id":PHONE,"conversational_automation":{"prompts":["Support"]}}),
            ),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["automation", "get", "-q"])
        .assert()
        .success()
        .stdout(format!("{PHONE}\n"))
        .stderr("");
    for (args, body) in [
        (
            vec![
                "--welcome",
                "false",
                "--prompt",
                "Support",
                "--command",
                "help:Get help: now",
            ],
            json!({"enable_welcome_message":false,"prompts":["Support"],"commands":[{"command_name":"help","command_description":"Get help: now"}]}),
        ),
        (
            vec!["--clear-prompts", "--clear-commands"],
            json!({"prompts":[],"commands":[]}),
        ),
    ] {
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/conversational_automation")))
            .and(body_json(&body))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args(["automation", "set"])
            .args(&args)
            .arg("-q")
            .assert()
            .success()
            .stdout("")
            .stderr("");
        let out = paloma(Some(&graph), &home)
            .env_remove("PALOMA_TOKEN")
            .args(["automation", "set"])
            .args(&args)
            .args(["--dry-run", "-o", "json"])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(stdout_json(&out)["body"], body);
    }
}

#[test]
fn automation_validates_typed_and_file_settings_before_context() {
    let home = TempDir::new().unwrap();
    for args in [
        vec![],
        vec!["--command", "bad"],
        vec!["--command", "/help:Hint"],
        vec!["--prompt", &"x".repeat(81)],
        vec![
            "--prompt", "a", "--prompt", "b", "--prompt", "c", "--prompt", "d", "--prompt", "e",
        ],
    ] {
        paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["automation", "set"])
            .args(&args)
            .assert()
            .code(5)
            .stdout("");
    }
    let file = home.path().join("automation.json");
    for body in [
        json!([]),
        json!({}),
        json!({"prompts":[1]}),
        json!({"commands":[{"command_name":"x"}]}),
        json!({"enable_welcome_message":"true"}),
    ] {
        std::fs::write(&file, body.to_string()).unwrap();
        paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["automation", "set", "--file"])
            .arg(&file)
            .assert()
            .code(5)
            .stdout("");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn download_timeout_applies_to_the_cdn_request_and_does_not_retry() {
    let graph = MockServer::start().await;
    mount_media_lookup(&graph).await;
    Mock::given(method("GET"))
        .and(path("/cdn/file"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(2))
                .set_body_string("late"),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let dest = home.path().join("late.bin");
    paloma(Some(&graph), &home)
        .args(["media", "download", "m1", "--timeout", "100ms", "-O"])
        .arg(&dest)
        .assert()
        .code(1)
        .stdout("")
        .stderr(
            predicate::str::contains("timed out").and(predicate::str::contains("retrying").not()),
        );
    assert!(!dest.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn webhook_can_use_default_callback_and_set_or_clear_a_number_override() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}/subscribed_apps")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_VERIFY_TOKEN")
        .args(["webhook", "subscribe", "--yes", "-q"])
        .assert()
        .success()
        .stdout("")
        .stderr("");
    assert!(graph.received_requests().await.unwrap()[0].body.is_empty());
    for (args, body) in [
        (
            vec!["--callback-url", "https://example.com/number"],
            json!({"webhook_configuration":{"override_callback_uri":"https://example.com/number","verify_token":"secret"}}),
        ),
        (
            vec![],
            json!({"webhook_configuration":{"override_callback_uri":""}}),
        ),
    ] {
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}")))
            .and(body_json(&body))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .env("PALOMA_VERIFY_TOKEN", "secret")
            .args(["webhook", "subscribe", "--number", "--yes", "-q"])
            .args(&args)
            .assert()
            .success()
            .stdout("")
            .stderr("");
        let out = paloma(Some(&graph), &home)
            .env("PALOMA_VERIFY_TOKEN", "secret")
            .args([
                "webhook",
                "subscribe",
                "--number",
                "--dry-run",
                "-o",
                "json",
            ])
            .args(&args)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(!String::from_utf8_lossy(&out.stdout).contains("secret"));
        assert_eq!(
            stdout_json(&out)["body"]["webhook_configuration"]["override_callback_uri"],
            body["webhook_configuration"]["override_callback_uri"]
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn webhook_get_quiet_uses_number_override_ahead_of_waba_and_application() {
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
        .expect(2).mount(&graph).await;
    paloma(Some(&graph), &home)
        .args(["webhook", "get", "-q"])
        .assert()
        .success()
        .stdout("https://example.com/number\n")
        .stderr("");
    let response = paloma(Some(&graph), &home)
        .args(["webhook", "get", "-o", "json"])
        .output()
        .unwrap();
    assert!(response.status.success());
    assert!(response.stderr.is_empty());
    let body = stdout_json(&response);
    assert_eq!(
        body["number_override_callback_uri"],
        "https://example.com/number"
    );
    assert_eq!(body["override_callback_uri"], "https://example.com/waba");
    assert_eq!(body["effective_callback_url"], "https://example.com/number");
}

#[test]
fn webhook_new_fields_are_offline_and_callback_validation_precedes_context() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env_remove("PALOMA_TOKEN")
        .args(["webhook", "fields", "available", "-q"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("group_participants_update\n")
                .and(predicate::str::contains("business_username_updates\n"))
                .and(predicate::str::contains("account_settings_update\n")),
        );
    for args in [
        vec!["--callback-url", ""],
        vec!["--callback-url", &"x".repeat(201)],
        vec!["--verify-token-stdin"],
    ] {
        paloma(None, &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["webhook", "subscribe"])
            .args(&args)
            .assert()
            .code(5)
            .stdout("");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn template_namespace_reads_the_waba_and_quiet_prints_the_namespace() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}")))
        .and(query_param("fields", "message_template_namespace"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id":WABA,"message_template_namespace":"example_namespace"})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["template", "namespace", "-q"])
        .assert()
        .success()
        .stdout("example_namespace\n")
        .stderr("");
    let out = paloma(Some(&graph), &home)
        .env_remove("PALOMA_TOKEN")
        .args(["template", "namespace", "--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(stdout_json(&out)["method"], "GET");
}

#[tokio::test(flavor = "multi_thread")]
async fn qr_image_refuses_a_file_created_while_the_graph_request_runs() {
    let graph = MockServer::start().await;
    let cdn = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let dest = home.path().join("race.svg");
    let race_dest = dest.clone();
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/message_qrdls")))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"code":"4O4YGZEG3RIVE1","qr_image_url":format!("{}/qr.svg",cdn.uri())}),
        ))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path("/qr.svg"))
        .respond_with(move |_: &wiremock::Request| {
            std::fs::write(&race_dest, b"KEEP").unwrap();
            ResponseTemplate::new(200).set_body_string("REPLACE")
        })
        .expect(1)
        .mount(&cdn)
        .await;
    paloma(Some(&graph), &home)
        .args(["qr", "create", "--message", "x", "--image", "svg", "--out"])
        .arg(&dest)
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("4O4YGZEG3RIVE1 was created"));
    assert_eq!(std::fs::read(dest).unwrap(), b"KEEP");
}

#[tokio::test(flavor = "multi_thread")]
async fn download_timeout_also_bounds_a_stalled_response_body() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 4096];
        let bytes_read = stream.read(&mut request).unwrap();
        assert!(bytes_read > 0);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nx")
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
    });
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v26.0/m1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"url":format!("http://{address}/file")})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let dest = home.path().join("partial.bin");
    paloma(Some(&graph), &home)
        .args(["media", "download", "m1", "--timeout", "100ms", "-O"])
        .arg(&dest)
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("timed out"));
    assert!(std::fs::read(&dest).unwrap().len() < 100);
    server.join().unwrap();
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

/// Each lookup returns the next category so reclassification can be exercised without
/// caching or timing-dependent mocks. Exact query matching also catches account drift.
async fn mock_routing_categories(graph: &MockServer, account: &str, categories: &[&str]) {
    let categories: Vec<String> = categories.iter().map(ToString::to_string).collect();
    let expected = u64::try_from(categories.len()).unwrap();
    let calls = std::sync::atomic::AtomicUsize::new(0);
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{account}/message_templates")))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .and(query_param("name", "order"))
        .and(query_param("language", "en_US"))
        .and(query_param("fields", "id,name,language,category"))
        .and(query_param("limit", "100"))
        .respond_with(move |_: &wiremock::Request| {
            let index = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_json(json!({"data":[{
                "id":"template_1", "name":"order", "language":"en_US",
                "category":categories[index.min(categories.len() - 1)]
            }]}))
        })
        .expect(expected)
        .mount(graph)
        .await;
}

fn routing_template_body() -> Value {
    json!({"messaging_product":"whatsapp", "recipient_type":"individual", "to":"15551234567", "type":"template", "template":{"name":"order", "language":{"code":"en_US"}}})
}

#[tokio::test(flavor = "multi_thread")]
async fn template_auto_routes_each_current_category_and_preserves_response_output() {
    for (category, edge) in [
        ("MARKETING", "marketing_messages"),
        ("UTILITY", "messages"),
        ("AUTHENTICATION", "messages"),
    ] {
        let graph = MockServer::start().await;
        let home = TempDir::new().unwrap();
        mock_routing_categories(&graph, WABA, &[category, category, category]).await;
        let response =
            json!({"messages":[{"id":"wamid.ROUTED"}], "contacts":[{"input":"15551234567"}]});
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/{edge}")))
            .and(header("authorization", format!("Bearer {TOKEN}")))
            .and(body_json(routing_template_body()))
            .respond_with(ResponseTemplate::new(200).set_body_json(&response))
            .expect(3)
            .mount(&graph)
            .await;
        for format in ["json", "raw"] {
            let out = paloma(Some(&graph), &home)
                .args([
                    "send",
                    "template",
                    "15551234567",
                    "order",
                    "--lang",
                    "en_US",
                    "-o",
                    format,
                ])
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(out.stderr.is_empty());
            assert_eq!(stdout_json(&out), response);
        }
        paloma(Some(&graph), &home)
            .args([
                "send",
                "template",
                "15551234567",
                "order",
                "--lang",
                "en_US",
                "-q",
            ])
            .assert()
            .success()
            .stdout("wamid.ROUTED\n")
            .stderr("");
        assert_eq!(graph.received_requests().await.unwrap().len(), 6);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn marketing_route_keeps_bsuid_components_reply_and_the_billed_account() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    mock_routing_categories(&graph, "777", &["MARKETING"]).await;
    let components = json!([
        {"type":"header","parameters":[{"type":"image","image":{"id":"MEDIA_1"}}]},
        {"type":"body","parameters":[{"type":"text","parameter_name":"first_name","text":"Ana"}]},
        {"type":"button","sub_type":"flow","index":"0","parameters":[{"type":"action","action":{"flow_token":"ROUTING_FLOW_SECRET"}}]}
    ]);
    let file = home.path().join("routing-components.json");
    std::fs::write(&file, components.to_string()).unwrap();
    Mock::given(method("POST")).and(path(format!("/v26.0/{PHONE}/marketing_messages")))
        .and(body_json(json!({"messaging_product":"whatsapp", "recipient_type":"individual", "recipient":BSUID, "messaging_account_id":"777", "context":{"message_id":"wamid.OLD"}, "type":"template", "template":{"name":"order", "language":{"code":"en_US"}, "components":components}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.BSUID"}],"contacts":[{"user_id":BSUID}]})))
        .expect(1).mount(&graph).await;
    let out = paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            BSUID,
            "order",
            "--lang",
            "en_US",
            "--messaging-account-id",
            "777",
            "--components-file",
        ])
        .arg(&file)
        .args(["--reply-to", "wamid.OLD", "-q", "-vv"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"wamid.BSUID\n");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("quoted message"));
    assert!(!stderr.contains("ROUTING_FLOW_SECRET"));
    assert!(!stderr.contains(TOKEN));
}

#[tokio::test(flavor = "multi_thread")]
async fn explicit_template_routes_skip_metadata_and_need_no_account_context() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (via, edge) in [("cloud", "messages"), ("marketing", "marketing_messages")] {
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/{edge}")))
            .and(body_json(routing_template_body()))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"messages":[{"id":"wamid.EXPLICIT"}]})),
            )
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .env_remove("PALOMA_MESSAGING_ACCOUNT_ID")
            .args([
                "send",
                "template",
                "15551234567",
                "order",
                "--lang",
                "en_US",
                "--via",
                via,
                "-q",
            ])
            .assert()
            .success()
            .stdout("wamid.EXPLICIT\n")
            .stderr("");
    }
    assert!(
        graph
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.method.as_str() == "POST")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn template_routing_dry_runs_are_offline_and_show_dynamic_or_fixed_endpoints() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for via in ["auto", "cloud", "marketing"] {
        let out = paloma(Some(&graph), &home)
            .env_remove("PALOMA_TOKEN")
            .args([
                "send",
                "template",
                "15551234567",
                "order",
                "--lang",
                "en_US",
                "--via",
                via,
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
        let plan = stdout_json(&out);
        if via == "auto" {
            assert_eq!(plan[0]["method"], "GET");
            assert!(
                plan[0]["url"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("/{WABA}/message_templates?"))
            );
            assert!(
                plan[1]["url"]
                    .as_str()
                    .unwrap()
                    .contains("<messages-or-marketing_messages-from-category>")
            );
            assert_eq!(plan[1]["body"], routing_template_body());
        } else {
            let edge = if via == "cloud" {
                "messages"
            } else {
                "marketing_messages"
            };
            assert!(
                plan["url"]
                    .as_str()
                    .unwrap()
                    .ends_with(&format!("/{PHONE}/{edge}"))
            );
            assert_eq!(plan["body"], routing_template_body());
        }
        assert!(String::from_utf8_lossy(&out.stderr).contains("dry-run: no request sent"));
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_template_routing_matches_exact_name_and_language_across_pages() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET")).and(path(format!("/v26.0/{WABA}/message_templates")))
        .and(query_param("after", "next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"name":"order","language":"en_US","category":"MARKETING","correct_category":"UTILITY"}]})))
        .expect(1).with_priority(1).mount(&graph).await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/message_templates")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[
            {"name":"order_extra","language":"en_US","category":"UTILITY"},
            {"name":"order","language":"es","category":"AUTHENTICATION"}],
            "paging":{"cursors":{"after":"next"},"next":"https://graph.facebook.com/next"}})))
        .expect(1)
        .with_priority(2)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/marketing_messages")))
        .and(body_json(routing_template_body()))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.EXACT"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            "15551234567",
            "order",
            "--lang",
            "en_US",
            "-q",
        ])
        .assert()
        .success()
        .stdout("wamid.EXACT\n")
        .stderr("");
}

#[tokio::test(flavor = "multi_thread")]
async fn template_routing_rejects_missing_unknown_or_ambiguous_metadata_without_sending() {
    for response in [
        json!({"data":[]}),
        json!({"data":[{"name":"order","language":"en_US"}]}),
        json!({"data":[{"name":"order","language":"en_US","category":"FUTURE_CATEGORY"}]}),
        json!({"data":[{"name":"order","language":"en_US","category":"UTILITY"},{"name":"order","language":"en_US","category":"MARKETING"}]}),
        json!({"data":null}),
    ] {
        let graph = MockServer::start().await;
        let home = TempDir::new().unwrap();
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args([
                "send",
                "template",
                "15551234567",
                "order",
                "--lang",
                "en_US",
            ])
            .assert()
            .code(1)
            .stdout("");
        assert_eq!(graph.received_requests().await.unwrap().len(), 1);
    }
}

#[test]
fn template_routing_validates_locally_before_account_resolution() {
    let home = TempDir::new().unwrap();
    for args in [
        vec!["bad", "order", "--lang", "en_US"],
        vec!["15551234567", "order", "--lang", "en_US", "--var", "bad"],
        vec!["15551234567", "order", "--lang", " "],
    ] {
        paloma(None, &home)
            .env_remove("PALOMA_WABA_ID")
            .env_remove("PALOMA_TOKEN")
            .args(["send", "template"])
            .args(args)
            .assert()
            .code(5)
            .stdout("");
    }
    paloma(None, &home)
        .env_remove("PALOMA_WABA_ID")
        .env_remove("PALOMA_MESSAGING_ACCOUNT_ID")
        .args([
            "send",
            "template",
            "15551234567",
            "order",
            "--lang",
            "en_US",
            "--dry-run",
        ])
        .assert()
        .code(3)
        .stdout("")
        .stderr(predicate::str::contains("--via cloud|marketing"));
    paloma(None, &home)
        .args([
            "send",
            "template",
            "15551234567",
            "order",
            "--lang",
            "en_US",
            "--via",
            "unknown",
        ])
        .assert()
        .code(2)
        .stdout("");
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_template_routing_reroutes_once_after_confirmed_category_changes() {
    for (old, new, first_edge, next_edge, code) in [
        (
            "UTILITY",
            "MARKETING",
            "messages",
            "marketing_messages",
            131_063,
        ),
        (
            "MARKETING",
            "UTILITY",
            "marketing_messages",
            "messages",
            100,
        ),
    ] {
        let graph = MockServer::start().await;
        let home = TempDir::new().unwrap();
        mock_routing_categories(&graph, WABA, &[old, new]).await;
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/{first_edge}")))
            .and(body_json(routing_template_body()))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(json!({"error":{"code":code,"message":"category rejected"}})),
            )
            .expect(1)
            .mount(&graph)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/{next_edge}")))
            .and(body_json(routing_template_body()))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"messages":[{"id":"wamid.REROUTED"}]})),
            )
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args([
                "send",
                "template",
                "15551234567",
                "order",
                "--lang",
                "en_US",
                "-q",
            ])
            .assert()
            .success()
            .stdout("wamid.REROUTED\n")
            .stderr(predicate::str::contains(format!(
                "retrying once via /{next_edge}"
            )));
        let requests = graph.received_requests().await.unwrap();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[1].body, requests[3].body);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_template_routing_stops_if_refresh_does_not_change_the_endpoint() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    mock_routing_categories(&graph, WABA, &["UTILITY", "AUTHENTICATION"]).await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({"error":{"code":131_063,"message":"original refusal"}})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            "15551234567",
            "order",
            "--lang",
            "en_US",
            "-q",
        ])
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("original refusal"));
    assert_eq!(graph.received_requests().await.unwrap().len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_template_routing_never_loops_on_a_second_rejection() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    mock_routing_categories(&graph, WABA, &["UTILITY", "MARKETING"]).await;
    for (edge, code) in [("messages", 131_063), ("marketing_messages", 100)] {
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/{edge}")))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(json!({"error":{"code":code,"message":"still rejected"}})),
            )
            .expect(1)
            .mount(&graph)
            .await;
    }
    paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            "15551234567",
            "order",
            "--lang",
            "en_US",
        ])
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("still rejected"));
    assert_eq!(graph.received_requests().await.unwrap().len(), 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_template_routing_never_reroutes_ambiguous_or_noncategory_failures() {
    for (response, exit) in [
        (
            ResponseTemplate::new(500)
                .set_body_json(json!({"error":{"code":100,"message":"server failed"}})),
            1,
        ),
        (
            ResponseTemplate::new(503)
                .set_body_json(json!({"error":{"code":131_063,"message":"unavailable"}})),
            1,
        ),
        (
            ResponseTemplate::new(403)
                .set_body_json(json!({"error":{"code":100,"message":"denied"}})),
            3,
        ),
        (
            ResponseTemplate::new(429)
                .set_body_json(json!({"error":{"code":130_429,"message":"slow"}})),
            4,
        ),
        (
            ResponseTemplate::new(400)
                .set_body_json(json!({"error":{"code":131_049,"message":"policy"}})),
            6,
        ),
        (
            ResponseTemplate::new(400).set_body_string("not a Graph error"),
            1,
        ),
        (ResponseTemplate::new(200).set_body_string("not JSON"), 1),
        (
            ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(2)),
            1,
        ),
    ] {
        let graph = MockServer::start().await;
        let home = TempDir::new().unwrap();
        mock_routing_categories(&graph, WABA, &["UTILITY"]).await;
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/messages")))
            .respond_with(response)
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args([
                "send",
                "template",
                "15551234567",
                "order",
                "--lang",
                "en_US",
                "--timeout",
                "200ms",
                "-q",
            ])
            .assert()
            .code(exit)
            .stdout("")
            .stderr(predicate::str::contains("retrying once").not());
        assert_eq!(graph.received_requests().await.unwrap().len(), 2);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn explicit_template_routing_does_not_refresh_or_override_a_rejected_route() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("POST")).and(path(format!("/v26.0/{PHONE}/messages")))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error":{"code":131_063,"message":"Marketing templates disabled for Cloud API"}})))
        .expect(1).mount(&graph).await;
    paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            "15551234567",
            "order",
            "--lang",
            "en_US",
            "--via",
            "cloud",
        ])
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("--via marketing"));
    assert_eq!(graph.received_requests().await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_template_routing_surfaces_lookup_auth_errors_without_a_send() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403).set_body_json(
            json!({"error":{"code":200,"message":"template management permission missing"}}),
        ))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            "15551234567",
            "order",
            "--lang",
            "en_US",
        ])
        .assert()
        .code(3)
        .stdout("")
        .stderr(predicate::str::contains(
            "template management permission missing",
        ));
    assert_eq!(graph.received_requests().await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn template_routing_never_refreshes_http_400_auth_policy_or_unknown_rejections() {
    // Some existing error mappings retain HTTP-400 auth/policy codes as generic API
    // errors. The reroute allowlist must remain safe independently of that taxonomy.
    for (category, edge) in [("UTILITY", "messages"), ("MARKETING", "marketing_messages")] {
        for code in [
            3, 10, 190, 200, 299, 131_005, 368, 130_497, 131_031, 999_999,
        ] {
            let graph = MockServer::start().await;
            let home = TempDir::new().unwrap();
            mock_routing_categories(&graph, WABA, &[category]).await;
            Mock::given(method("POST"))
                .and(path(format!("/v26.0/{PHONE}/{edge}")))
                .respond_with(
                    ResponseTemplate::new(400)
                        .set_body_json(json!({"error":{"code":code,"message":"do not reroute"}})),
                )
                .expect(1)
                .mount(&graph)
                .await;
            paloma(Some(&graph), &home)
                .args([
                    "send",
                    "template",
                    "15551234567",
                    "order",
                    "--lang",
                    "en_US",
                    "-q",
                ])
                .assert()
                .code(1)
                .stdout("")
                .stderr(predicate::str::contains("retrying once").not());
            assert_eq!(graph.received_requests().await.unwrap().len(), 2);
        }
    }
}

fn messaging_analytics_response() -> Value {
    json!({"id":WABA,"analytics":{"phone_numbers":["15551234567"],"country_codes":["US"],"granularity":"DAY","data_points":[{"start":1_793_491_200_u64,"end":1_793_577_600_u64,"sent":12,"delivered":10}]}})
}

fn pricing_analytics_response() -> Value {
    json!({"id":WABA,"pricing_analytics":{"data":[
        {"data_points":[{"start":1_793_491_200_u64,"end":1_793_577_600_u64,"phone_number":"15551234567","country":"IN","pricing_category":"AUTHENTICATION","pricing_type":"REGULAR","tier":"0:750000","volume":2,"cost":4.6}]},
        {"data_points":[{"start":1_793_491_200_u64,"end":1_793_577_600_u64,"country":"US","pricing_category":"MARKETING_LITE_DYNAMIC","pricing_type":"REGULAR","volume":3}]}
    ]}})
}

#[tokio::test(flavor = "multi_thread")]
async fn account_analytics_sends_the_exact_messaging_field_expansion_and_preserves_formats() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let body = messaging_analytics_response();
    Mock::given(method("GET")).and(path(format!("/v26.0/{WABA}")))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .and(query_param("fields", "analytics.start(1793491200).end(1793577600).granularity(DAY).phone_numbers([\"15551234567\",\"15557654321\"]).product_types([0,2,100]).country_codes([\"US\",\"IN\"])"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(5).mount(&graph).await;
    for format in ["table", "json", "yaml", "raw"] {
        let out = paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args([
                "account",
                "analytics",
                "--start",
                "2026-11-01",
                "--end",
                "2026-11-02",
                "--phone-number",
                "+15551234567,15557654321",
                "--product-type",
                "template,non-template",
                "--product-type",
                "incoming",
                "--country",
                "US,IN",
                "-o",
                format,
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stderr.is_empty());
        match format {
            "table" => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                assert!(
                    stdout.contains("SENT")
                        && stdout.contains("DELIVERED")
                        && stdout.contains("12")
                        && stdout.contains("10")
                );
            }
            "yaml" => assert_eq!(
                serde_norway::from_slice::<Value>(&out.stdout).unwrap(),
                body
            ),
            _ => assert_eq!(stdout_json(&out), body),
        }
    }
    paloma(Some(&graph), &home)
        .args([
            "account",
            "analytics",
            "--start",
            "2026-11-01",
            "--end",
            "2026-11-02",
            "--phone-number",
            "+15551234567,15557654321",
            "--product-type",
            "template,non-template,incoming",
            "--country",
            "US,IN",
            "-q",
        ])
        .assert()
        .success()
        .stdout("")
        .stderr("");
    assert!(
        graph
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.body.is_empty())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn account_pricing_analytics_serializes_all_filters_and_flattens_every_table_group() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let body = pricing_analytics_response();
    Mock::given(method("GET")).and(path(format!("/v26.0/{WABA}")))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .and(query_param("fields", "pricing_analytics.start(1793491200).end(1793577600).granularity(DAILY).phone_numbers([\"15551234567\"]).country_codes([\"US\",\"IN\"]).metric_types([\"COST\",\"VOLUME\"]).pricing_types([\"REGULAR\",\"FREE_ENTRY_POINT\"]).pricing_categories([\"AUTHENTICATION\",\"MARKETING_LITE_DYNAMIC\"]).dimensions([\"COUNTRY\",\"PRICING_CATEGORY\",\"TIER\",\"PHONE\",\"PRICING_TYPE\"])"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(5).mount(&graph).await;
    let args = [
        "account",
        "analytics",
        "--pricing",
        "--start",
        "1793491200",
        "--end",
        "1793577600",
        "--phone-number",
        "15551234567",
        "--country",
        "US,IN",
        "--metric",
        "cost,volume",
        "--pricing-type",
        "regular,free-entry-point",
        "--pricing-category",
        "authentication,marketing-lite-dynamic",
        "--dimension",
        "country,pricing-category,tier,phone,pricing-type",
    ];
    for format in ["table", "json", "yaml", "raw"] {
        let out = paloma(Some(&graph), &home)
            .args(args)
            .args(["-o", format])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stderr.is_empty());
        match format {
            "table" => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                assert!(
                    stdout.contains("AUTHENTICATION") && stdout.contains("MARKETING_LITE_DYNAMIC")
                );
                assert!(stdout.contains("0:750000") && stdout.contains("4.6"));
                let missing_cost_row = stdout
                    .lines()
                    .find(|line| line.contains("MARKETING_LITE_DYNAMIC"))
                    .unwrap();
                assert!(missing_cost_row.ends_with('-'), "{missing_cost_row}");
            }
            "yaml" => assert_eq!(
                serde_norway::from_slice::<Value>(&out.stdout).unwrap(),
                body
            ),
            _ => assert_eq!(stdout_json(&out), body),
        }
    }
    paloma(Some(&graph), &home)
        .args(args)
        .arg("-q")
        .assert()
        .success()
        .stdout("")
        .stderr("");
}

#[tokio::test(flavor = "multi_thread")]
async fn analytics_granularities_translate_by_mode_and_omit_unspecified_filters_offline() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (pricing, granularity, expected) in [
        (false, "half-hour", "HALF_HOUR"),
        (false, "day", "DAY"),
        (false, "month", "MONTH"),
        (true, "half-hour", "HALF_HOUR"),
        (true, "day", "DAILY"),
        (true, "month", "MONTHLY"),
    ] {
        let mut cmd = paloma(Some(&graph), &home);
        cmd.env_remove("PALOMA_TOKEN")
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args([
                "account",
                "analytics",
                "--start",
                "2026-11-01",
                "--end",
                "2026-11-02",
                "--granularity",
                granularity,
                "--dry-run",
                "-o",
                "json",
            ]);
        if pricing {
            cmd.arg("--pricing");
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let request = stdout_json(&out);
        assert_eq!(request["method"], "GET");
        assert!(request.get("body").is_none());
        let field = if pricing {
            "pricing_analytics"
        } else {
            "analytics"
        };
        let query = format!("{field}.start(1793491200).end(1793577600).granularity({expected})");
        let url = percent_encoding::percent_decode_str(request["url"].as_str().unwrap())
            .decode_utf8()
            .unwrap();
        assert_eq!(url, format!("{}/v26.0/{WABA}?fields={query}", graph.uri()));
        assert!(String::from_utf8_lossy(&out.stderr).contains("dry-run: no request sent"));
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn analytics_invalid_dates_ranges_and_filters_fail_before_context_or_a_request() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (start, end, extra) in [
        ("2026-02-29", "2026-03-01", vec![]),
        ("2026-11-31", "2026-12-01", vec![]),
        ("bad", "2026-11-02", vec![]),
        ("2026-11-01", "bad", vec![]),
        ("-1", "10", vec![]),
        ("2026-11-02", "2026-11-01", vec![]),
        ("2026-11-01", "2026-11-01", vec![]),
        ("18446744073709551616", "18446744073709551617", vec![]),
        ("1", "2", vec!["--country", "us"]),
        ("1", "2", vec!["--country", "USA"]),
        ("1", "2", vec!["--country", "US\").end(0)"]),
        ("1", "2", vec!["--phone-number", "111"]),
        ("1", "2", vec!["--phone-number", BSUID]),
    ] {
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .env_remove("PALOMA_TOKEN")
            .args(["account", "analytics", "--start", start, "--end", end])
            .args(extra)
            .assert()
            .code(5)
            .stdout("");
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[test]
fn analytics_closed_sets_and_mode_conflicts_are_usage_errors() {
    let home = TempDir::new().unwrap();
    for extra in [
        vec!["--granularity", "daily"],
        vec!["--product-type", "1"],
        vec!["--pricing", "--product-type", "template"],
        vec!["--metric", "cost"],
        vec!["--dimension", "country"],
        vec!["--pricing-type", "regular"],
        vec!["--pricing-category", "utility"],
        vec!["--pricing", "--metric", "conversations"],
        vec!["--pricing", "--dimension", "unknown"],
        vec!["--pricing", "--pricing-category", "unknown"],
        vec!["--pricing", "--pricing-type", "paid-exempt"],
        vec!["--conversations"],
    ] {
        paloma(None, &home)
            .env_remove("PALOMA_WABA_ID")
            .env_remove("PALOMA_TOKEN")
            .args(["account", "analytics", "--start", "1", "--end", "2"])
            .args(extra)
            .assert()
            .code(2)
            .stdout("");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn analytics_targets_the_selected_waba_and_can_read_pre_migration_history() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path("/v26.0/old_account"))
        .and(query_param(
            "fields",
            "analytics.start(1709164800).end(1709251200).granularity(DAY)",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id":"old_account","analytics":{"data_points":[]}})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env("PALOMA_MESSAGING_ACCOUNT_ID", "new_account")
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args([
            "account",
            "analytics",
            "--start",
            "2024-02-29",
            "--end",
            "2024-03-01",
            "--waba-id",
            "old_account",
        ])
        .assert()
        .success()
        .stdout("(none)\n")
        .stderr("");
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_WABA_ID")
        .args([
            "account",
            "analytics",
            "--start",
            "1",
            "--end",
            "2",
            "--dry-run",
        ])
        .assert()
        .code(3)
        .stdout("")
        .stderr(predicate::str::contains("--waba-id"));
}

#[tokio::test(flavor = "multi_thread")]
async fn analytics_empty_and_malformed_reports_are_distinct() {
    for (pricing, body, valid) in [
        (false, json!({"analytics":{"data_points":[]}}), true),
        (true, json!({"pricing_analytics":{"data":[]}}), true),
        (
            true,
            json!({"pricing_analytics":{"data":[{"data_points":[]}]}}),
            true,
        ),
        (false, json!({"id":WABA}), false),
        (false, json!({"analytics":{"data_points":[1]}}), false),
        (true, json!({"pricing_analytics":{"data":[{}]}}), false),
        (true, json!({"pricing_analytics":{"data":null}}), false),
    ] {
        let graph = MockServer::start().await;
        let home = TempDir::new().unwrap();
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(&graph)
            .await;
        let mut cmd = paloma(Some(&graph), &home);
        cmd.args(["account", "analytics", "--start", "1", "--end", "2"]);
        if pricing {
            cmd.arg("--pricing");
        }
        if valid {
            cmd.assert().success().stdout("(none)\n").stderr("");
        } else {
            cmd.assert()
                .code(1)
                .stdout("")
                .stderr(predicate::str::contains("malformed"));
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn analytics_preserves_api_auth_and_rate_limit_errors_on_stderr() {
    for (status, body, exit) in [
        (
            403,
            json!({"error":{"code":200,"message":"analytics permission denied"}}),
            3,
        ),
        (
            429,
            json!({"error":{"code":130_429,"message":"analytics throttled"}}),
            4,
        ),
    ] {
        let graph = MockServer::start().await;
        let home = TempDir::new().unwrap();
        // Reads retry throttling by design, so allow the three bounded attempts.
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_json(body)
                    .insert_header("Retry-After", "0"),
            )
            .expect(if status == 429 { 3 } else { 1 })
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args(["account", "analytics", "--start", "1", "--end", "2"])
            .assert()
            .code(exit)
            .stdout("")
            .stderr(predicate::str::contains("analytics"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn pricing_analytics_raw_keeps_decimal_text_and_unknown_response_fields() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let body = r#"{ "id":"222", "currency":"EUR", "pricing_analytics":{"data":[{"data_points":[{"start":1,"end":2,"pricing_category":"FUTURE_CATEGORY","cost":0.1234567890123456789012345}]}]}}"#;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "account",
            "analytics",
            "--pricing",
            "--start",
            "1",
            "--end",
            "2",
            "-o",
            "raw",
        ])
        .assert()
        .success()
        .stdout(format!("{body}\n"))
        .stderr("");
}

/// Print a command's request under `--dry-run`, with nothing sent.
fn dry_run_request(home: &TempDir, args: &[&str]) -> Value {
    let out = paloma(None, home)
        .args(args)
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("dry-run: no request sent"));
    stdout_json(&out)
}

const GRAPH: &str = "https://graph.facebook.com/v26.0";

#[tokio::test(flavor = "multi_thread")]
async fn template_get_quality_asks_for_the_quality_fields_and_lists_them() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/message_templates")))
        .and(query_param("name", "sale"))
        .and(query_param(
            "fields",
            "name,language,status,quality_score,health_status",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{
            "name":"sale", "language":"en_US", "status":"APPROVED", "id":"77",
            "quality_score":{"score":"GREEN", "date":1_758_754_645},
            "health_status":{"can_send_message":"AVAILABLE"},
        }]})))
        .expect(2)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["template", "get", "sale", "--quality"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("GREEN")
                .and(predicate::str::contains("AVAILABLE"))
                .and(predicate::str::contains("CAN SEND MESSAGE")),
        );
    paloma(Some(&graph), &home)
        .args(["template", "get", "sale", "--quality", "-q"])
        .assert()
        .success()
        .stdout("77\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn template_archive_confirms_and_posts_the_ids() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}/message_templates/archive")))
        .and(body_json(json!({"hsm_ids":["1","2"]})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"archived_templates":["1","2"], "failed_templates":{}})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["template", "archive", "--ids", "1,2"])
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("--yes"));
    paloma(Some(&graph), &home)
        .args(["template", "archive", "--ids", "1,2", "--yes", "-o", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("archived_templates"));
    let dry = dry_run_request(&home, &["template", "archive", "--ids", "1"]);
    assert_eq!(
        dry["url"],
        format!("{GRAPH}/{WABA}/message_templates/archive")
    );
    assert_eq!(dry["body"], json!({"hsm_ids":["1"]}));
}

#[tokio::test(flavor = "multi_thread")]
async fn template_unarchive_needs_no_confirmation_and_a_partial_failure_exits_1() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}/message_templates/unarchive")))
        .and(body_json(json!({"hsm_ids":["1","2"]})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "unarchived_templates":["1"],
            "failed_templates":{"2":"Template is not archived"},
        })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["template", "unarchive", "--ids", "1,2", "-o", "json"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Template is not archived"))
        .stderr(predicate::str::contains("1 template(s) failed"));
}

#[test]
fn template_archive_validates_ids_before_any_request() {
    let home = TempDir::new().unwrap();
    let too_many: Vec<String> = (0..101).map(|i| i.to_string()).collect();
    for ids in [too_many.join(","), "abc".to_owned(), "1,1".to_owned()] {
        for verb in ["archive", "unarchive"] {
            paloma(None, &home)
                .args(["template", verb, "--ids", &ids, "--yes"])
                .assert()
                .code(5);
        }
    }
    paloma(None, &home)
        .args(["template", "archive"])
        .assert()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn template_unpause_posts_to_the_template_and_quiet_prints_its_id() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v26.0/77/unpause"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["template", "unpause", "77", "-q"])
        .assert()
        .success()
        .stdout("77\n");
    paloma(None, &home)
        .args(["template", "unpause", "sale"])
        .assert()
        .code(5);
}

#[tokio::test(flavor = "multi_thread")]
async fn template_library_list_has_no_waba_in_its_path_and_shows_columns() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v26.0/message_template_library"))
        .and(query_param("search", "payments"))
        .and(query_param("topic", "PAYMENTS"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{
            "name":"low_balance_warning_1", "language":"en_US", "category":"UTILITY",
            "topic":"PAYMENTS", "usecase":"LOW_BALANCE_WARNING", "id":"7147",
        }]})))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_WABA_ID")
        .args([
            "template", "library", "list", "--search", "payments", "--topic", "PAYMENTS",
        ])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("low_balance_warning_1")
                .and(predicate::str::contains("USECASE")),
        );
    assert_eq!(
        dry_run_request(&home, &["template", "library", "list", "--name", "x"])["url"],
        format!("{GRAPH}/message_template_library?name=x")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn template_library_create_sends_the_library_fields() {
    let graph = MockServer::start().await;
    let buttons = json!([{"type":"PHONE_NUMBER", "phone_number":"+16315551010"}]);
    let inputs = json!({"add_contact_number":true});
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}/message_templates")))
        .and(body_json(json!({
            "name":"my_delivery_update",
            "category":"UTILITY",
            "language":"en_US",
            "library_template_name":"delivery_update_1",
            "library_template_button_inputs":buttons,
            "library_template_body_inputs":inputs,
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id":"55", "status":"APPROVED", "category":"UTILITY"})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let button_file = home.path().join("buttons.json");
    std::fs::write(&button_file, buttons.to_string()).unwrap();
    let body_file = home.path().join("body.json");
    std::fs::write(&body_file, inputs.to_string()).unwrap();
    let base = [
        "template",
        "library",
        "create",
        "--name",
        "my_delivery_update",
        "--library-template",
        "delivery_update_1",
        "--language",
        "en_US",
    ];
    paloma(Some(&graph), &home)
        .args(base)
        .arg("--button-inputs-file")
        .arg(&button_file)
        .arg("--body-inputs-file")
        .arg(&body_file)
        .arg("-q")
        .assert()
        .success()
        .stdout("55\n");
    // An array of inputs and an object of them, the wrong way round.
    paloma(None, &home)
        .args(base)
        .arg("--button-inputs-file")
        .arg(&body_file)
        .assert()
        .code(5);
    paloma(None, &home)
        .args(base)
        .arg("--body-inputs-file")
        .arg(&button_file)
        .assert()
        .code(5);
}

#[tokio::test(flavor = "multi_thread")]
async fn template_compare_sends_a_window_in_unix_seconds() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v26.0/1/compare"))
        .and(query_param("template_ids", "[2]"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[
            {"metric":"BLOCK_RATE", "type":"RELATIVE", "order_by_relative_metric":["2","1"]},
        ]})))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["template", "compare", "1", "2", "--days", "30"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BLOCK_RATE").and(predicate::str::contains("2,1")));

    let dry = dry_run_request(&home, &["template", "compare", "1", "2", "--days", "7"]);
    let url = dry["url"].as_str().unwrap();
    assert!(
        url.starts_with(&format!("{GRAPH}/1/compare?template_ids=%5B2%5D&start=")),
        "{url}"
    );
    let param = |key: &str| -> u64 {
        url.split(&format!("{key}="))
            .nth(1)
            .and_then(|rest| rest.split('&').next())
            .and_then(|value| value.parse().ok())
            .unwrap()
    };
    assert_eq!(param("end") - param("start"), 604_800);
    assert!(param("end") < 10_000_000_000, "seconds, not milliseconds");
}

#[test]
fn template_compare_rejects_other_windows_and_ids_before_a_request() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(["template", "compare", "1", "2", "--days", "14"])
        .assert()
        .code(2);
    paloma(None, &home)
        .args(["template", "compare", "1", "x", "--days", "7"])
        .assert()
        .code(5);
}

#[tokio::test(flavor = "multi_thread")]
async fn template_migrate_confirms_then_posts_to_the_destination() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}/migrate_message_templates")))
        .and(body_json(json!({
            "source_waba_id":"333",
            "page_number":0,
            "count":200,
            "template_ids":["1","2"],
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "migrated_templates":["1"],
            "failed_templates":{"2":"Incorrect category"},
        })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let args = [
        "template", "migrate", "--from", "333", "--ids", "1,2", "--page", "0", "--count", "200",
    ];
    paloma(Some(&graph), &home)
        .args(args)
        .assert()
        .code(5)
        .stderr(predicate::str::contains("--yes"));
    paloma(Some(&graph), &home)
        .args(args)
        .args(["--yes", "-o", "json"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Incorrect category"))
        .stderr(predicate::str::contains("1 template(s) failed"));
    let dry = dry_run_request(&home, &["template", "migrate", "--from", "333"]);
    assert_eq!(
        dry["url"],
        format!("{GRAPH}/{WABA}/migrate_message_templates")
    );
    assert_eq!(dry["body"], json!({"source_waba_id":"333"}));
}

#[test]
fn template_migrate_validates_before_a_request() {
    let home = TempDir::new().unwrap();
    let too_many: Vec<String> = (0..501).map(|i| i.to_string()).collect();
    for args in [
        vec!["--from", "abc"],
        vec!["--from", "333", "--count", "501"],
        vec!["--from", "333", "--ids", &too_many.join(",")],
    ] {
        paloma(None, &home)
            .args(["template", "migrate", "--yes"])
            .args(args)
            .assert()
            .code(5);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn template_preview_needs_auth_sends_otp_and_narrows_languages_itself() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/message_template_previews")))
        .and(query_param("category", "AUTHENTICATION"))
        .and(query_param("add_security_recommendation", "true"))
        .and(query_param("code_expiration_minutes", "10"))
        .and(query_param("button_types", "OTP"))
        .and(wiremock::matchers::query_param_is_missing("language"))
        .and(wiremock::matchers::query_param_is_missing("languages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[
            {"language":"en_US", "body":"code", "footer":"10 minutes", "buttons":[{"text":"Copy code"}]},
            {"language":"es_ES", "body":"codigo", "footer":"10 minutos", "buttons":[{"text":"Copiar"}]},
        ]})))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args([
            "template",
            "preview",
            "--auth",
            "--security-recommendation",
            "--code-expiration",
            "10",
            "--language",
            "es_ES,fr",
            "-o",
            "json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v = stdout_json(&out);
    assert_eq!(v["data"].as_array().unwrap().len(), 1);
    assert_eq!(v["data"][0]["language"], "es_ES");
    assert!(String::from_utf8_lossy(&out.stderr).contains("no preview for language fr"));

    assert_eq!(
        dry_run_request(&home, &["template", "preview", "--auth"])["url"],
        format!(
            "{GRAPH}/{WABA}/message_template_previews?category=AUTHENTICATION&button_types=OTP"
        )
    );
    paloma(None, &home)
        .args(["template", "preview"])
        .assert()
        .code(2);
    paloma(None, &home)
        .args(["template", "preview", "--auth", "--code-expiration", "91"])
        .assert()
        .code(5);
}

#[tokio::test(flavor = "multi_thread")]
async fn template_upsert_posts_the_file_and_rejects_a_single_language() {
    let graph = MockServer::start().await;
    let body = json!({
        "name":"auth_code",
        "languages":["en_US","es_ES"],
        "category":"AUTHENTICATION",
        "components":[{"type":"BODY", "add_security_recommendation":true}],
    });
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}/upsert_message_templates")))
        .and(body_json(&body))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[
            {"id":"95", "status":"APPROVED", "language":"en_US"},
            {"id":"96", "status":"APPROVED", "language":"es_ES"},
        ]})))
        .expect(2)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("upsert.json");
    std::fs::write(&file, body.to_string()).unwrap();
    let upsert = |extra: &[&str]| {
        let mut cmd = paloma(Some(&graph), &home);
        cmd.args(["template", "upsert", "--file"])
            .arg(&file)
            .args(extra);
        cmd
    };
    upsert(&["-q"]).assert().success().stdout("95\n");
    upsert(&[])
        .assert()
        .success()
        .stdout(predicate::str::contains("es_ES").and(predicate::str::contains("96")));
    for bad in [
        json!({"name":"a", "language":"en_US", "languages":["en_US"]}),
        json!({"name":"a"}),
        json!({"languages":["en_US"]}),
    ] {
        std::fs::write(&file, bad.to_string()).unwrap();
        upsert(&[]).assert().code(5);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn template_group_create_sends_integer_ids() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}/template_groups")))
        .and(body_json(json!({
            "name":"Black Friday",
            "description":"US sale",
            "whatsapp_business_templates":[278_077_987_957_091_u64, 2],
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":"90"})))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let args = [
        "template",
        "group",
        "create",
        "--name",
        "Black Friday",
        "--description",
        "US sale",
        "--templates",
        "278077987957091,2",
        "-q",
    ];
    paloma(Some(&graph), &home)
        .args(args)
        .assert()
        .success()
        .stdout("90\n");
    let long_name = "x".repeat(257);
    let long_description = "x".repeat(513);
    for bad in [
        vec!["--name", &long_name, "--templates", "1"],
        vec![
            "--name",
            "n",
            "--description",
            &long_description,
            "--templates",
            "1",
        ],
        vec!["--name", "n", "--templates", "abc"],
    ] {
        paloma(None, &home)
            .args(["template", "group", "create"])
            .args(bad)
            .assert()
            .code(5);
    }
    paloma(None, &home)
        .args(["template", "group", "create", "--name", "n"])
        .assert()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn template_group_get_set_and_delete_use_the_group_node() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v26.0/90"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":"90", "name":"g"})))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path("/v26.0/90"))
        .and(body_json(
            json!({"add_templates":[1,2], "remove_templates":[3]}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v26.0/90"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let group = |args: &[&str]| {
        let mut cmd = paloma(Some(&graph), &home);
        cmd.args(["template", "group"]).args(args);
        cmd
    };
    group(&["get", "90", "-q"])
        .assert()
        .success()
        .stdout("90\n");
    group(&["set", "90", "--add", "1,2", "--remove", "3", "-q"])
        .assert()
        .success()
        .stdout("90\n");
    group(&["delete", "90"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("--yes"));
    group(&["delete", "90", "--yes", "-q"])
        .assert()
        .success()
        .stdout("90\n");
    group(&["set", "90"]).assert().code(2);
    group(&["get", "abc"]).assert().code(5);
}

#[tokio::test(flavor = "multi_thread")]
async fn template_analytics_queries_templates_and_lists_the_data_points() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/template_analytics")))
        .and(query_param("granularity", "daily"))
        .and(query_param("start", "1718064000"))
        .and(query_param("end", "1718122745"))
        .and(query_param("template_ids", "[14,26]"))
        .and(query_param("metric_types", "sent,read"))
        .and(query_param("product_type", "MARKETING_MESSAGES_LITE_API"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{
            "granularity":"DAILY",
            "product_type":"marketing_messages_lite_api",
            "data_points":[{"template_id":"14", "start":1, "end":2, "sent":5, "read":3}],
        }, {"data_points":[{"template_id":"26", "start":1, "end":2, "sent":6}]}]})))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args([
            "template",
            "analytics",
            "--ids",
            "14,26",
            "--start",
            "1718064000",
            "--end",
            "1718122745",
            "--metrics",
            "sent,read",
            "--product-type",
            "MARKETING_MESSAGES_LITE_API",
        ])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("TEMPLATE ID")
                .and(predicate::str::contains("14"))
                .and(predicate::str::contains("26")),
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn template_analytics_queries_groups_in_the_accounts_timezone() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/template_group_analytics")))
        .and(query_param("template_group_ids", "[104]"))
        .and(query_param("start", "2026-10-01"))
        .and(query_param("end", "2026-10-02"))
        .and(query_param("use_waba_timezone", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{
            "granularity":"DAILY",
            "data_points":[{"template_group_id":"104", "start":1, "end":2, "sent":9}],
        }]})))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args([
            "template",
            "analytics",
            "--groups",
            "104",
            "--start",
            "2026-10-01",
            "--end",
            "2026-10-02",
            "--waba-timezone",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("TEMPLATE GROUP ID").and(predicate::str::contains("104")));
}

#[test]
fn template_analytics_validates_before_a_request() {
    let home = TempDir::new().unwrap();
    let window = ["--start", "1718064000", "--end", "1718122745"];
    let eleven = (0..11).map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    for bad in [
        vec!["--ids", &eleven],
        vec!["--groups", &eleven],
        vec!["--groups", "abc"],
        vec!["--ids", "abc"],
    ] {
        paloma(None, &home)
            .args(["template", "analytics"])
            .args(window)
            .args(bad)
            .assert()
            .code(5);
    }
    // --waba-timezone takes dates only.
    paloma(None, &home)
        .args(["template", "analytics", "--ids", "1", "--waba-timezone"])
        .args(window)
        .assert()
        .code(5);
    paloma(None, &home)
        .args([
            "template",
            "analytics",
            "--ids",
            "1",
            "--start",
            "soon",
            "--end",
            "1",
        ])
        .assert()
        .code(5);
    // Needs a subject, and Meta documents product_type only for templates.
    paloma(None, &home)
        .args(["template", "analytics"])
        .args(window)
        .assert()
        .code(2);
    paloma(None, &home)
        .args([
            "template",
            "analytics",
            "--groups",
            "1",
            "--product-type",
            "CLOUD_API",
        ])
        .args(window)
        .assert()
        .code(2);
}

#[test]
fn template_upsert_refuses_what_metas_bulk_page_rules_out_before_the_context() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("upsert.json");
    let otp = |button: Value| {
        json!({
            "name": "auth_code",
            "languages": ["en_US"],
            "category": "AUTHENTICATION",
            "components": [{ "type": "BUTTONS", "buttons": [button] }],
        })
    };
    for (bad, needle) in [
        (
            json!({"name":"Auth Code", "languages":["en_US"], "category":"AUTHENTICATION"}),
            "template names",
        ),
        (
            json!({"name":"auth_code", "languages":["en_US"], "category":"UTILITY"}),
            "AUTHENTICATION",
        ),
        (
            json!({"name":"auth_code", "languages":["en_US"]}),
            "AUTHENTICATION",
        ),
        (
            otp(json!({"type":"OTP", "otp_type":"COPY_CODE", "text":"Copy"})),
            "`text`",
        ),
        (
            otp(json!({"type":"OTP", "otp_type":"ONE_TAP", "autofill_text":"Fill"})),
            "`autofill_text`",
        ),
    ] {
        std::fs::write(&file, bad.to_string()).unwrap();
        paloma(None, &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["template", "upsert", "--file"])
            .arg(&file)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(needle));
    }
    std::fs::write(
        &file,
        otp(json!({"type":"OTP", "otp_type":"COPY_CODE"})).to_string(),
    )
    .unwrap();
    // No --yes: like `template edit`, upsert doesn't ask.
    let file = file.to_str().unwrap();
    let request = dry_run_request(&home, &["template", "upsert", "--file", file]);
    assert_eq!(request["body"]["name"], "auth_code");
}

#[test]
fn template_library_create_always_sends_utility() {
    let home = TempDir::new().unwrap();
    let args = [
        "template",
        "library",
        "create",
        "--name",
        "my_update",
        "--library-template",
        "delivery_update_1",
        "--language",
        "en_US",
    ];
    assert_eq!(dry_run_request(&home, &args)["body"]["category"], "UTILITY");
    paloma(None, &home)
        .args(args)
        .args(["--category", "MARKETING"])
        .assert()
        .code(2);
}

#[test]
fn template_analytics_refuses_impossible_dates_and_a_backwards_range() {
    let home = TempDir::new().unwrap();
    let range = |start: &'static str, end: &'static str| {
        vec![
            "template",
            "analytics",
            "--ids",
            "1",
            "--start",
            start,
            "--end",
            end,
        ]
    };
    for (args, needle) in [
        (range("2026-02-30", "2026-03-01"), "--start"),
        (range("2026-02-01", "2026-13-01"), "--end"),
        (range("2026-10-08", "2026-10-01"), "must not be after"),
        (range("1718122745", "1718064000"), "must not be after"),
    ] {
        paloma(None, &home)
            .args(args)
            .assert()
            .code(5)
            .stderr(predicate::str::contains(needle));
    }
    paloma(None, &home)
        .args(range("1718064000", "2026-10-01"))
        .arg("--waba-timezone")
        .assert()
        .code(5)
        .stderr(predicate::str::contains("with --waba-timezone"));
    // One day is a real range: Meta reports whole days.
    let request = dry_run_request(&home, &range("2026-10-01", "2026-10-01"));
    assert!(
        request["url"]
            .as_str()
            .unwrap()
            .contains("start=2026-10-01")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn template_upsert_rejects_non_string_and_blank_languages_before_a_request() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("upsert.json");
    for languages in [json!([42]), json!([""]), json!([" "])] {
        std::fs::write(
            &file,
            json!({
                "name": "auth_code", "category": "AUTHENTICATION", "languages": languages,
            })
            .to_string(),
        )
        .unwrap();
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["template", "upsert", "--file"])
            .arg(&file)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("language"));
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

// ---- webhook receiving contracts ----
const VERIFY: &str = "VERIFY_SENTINEL";
const APP_SECRET: &str = "APP_SECRET_SENTINEL";

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

#[tokio::test(flavor = "multi_thread")]
async fn webhook_verify_url_passes_when_the_challenge_comes_back() {
    let endpoint = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/hook"))
        .and(query_param("keep", "1"))
        .and(query_param("hub.mode", "subscribe"))
        .and(query_param("hub.verify_token", VERIFY))
        .respond_with(|req: &wiremock::Request| {
            let challenge = req
                .url
                .query_pairs()
                .find(|(k, _)| k == "hub.challenge")
                .map(|(_, v)| v.into_owned())
                .unwrap_or_default();
            ResponseTemplate::new(200).set_body_string(challenge)
        })
        .expect(1)
        .mount(&endpoint)
        .await;
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .env_remove("PALOMA_TOKEN")
        .env("PALOMA_VERIFY_TOKEN", VERIFY)
        .args(["webhook", "verify-url", "-o", "json", "-vv"])
        .arg(format!("{}/hook?keep=1", endpoint.uri()))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = stdout_json(&out);
    assert_eq!(v["passed"], true);
    assert_eq!(v["status"], 200);
    assert!(!String::from_utf8_lossy(&out.stdout).contains(VERIFY));
    assert!(!String::from_utf8_lossy(&out.stderr).contains(VERIFY));
}

#[tokio::test(flavor = "multi_thread")]
async fn webhook_verify_url_fails_with_exit_1_on_anything_else() {
    let cases = [
        (ResponseTemplate::new(403), 403, "answered 403"),
        (
            ResponseTemplate::new(200).set_body_string("wrong"),
            200,
            "isn't exactly",
        ),
        (
            ResponseTemplate::new(302).insert_header("location", "https://other.test/"),
            302,
            "redirects",
        ),
    ];
    for (response, status, needle) in cases {
        let endpoint = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(response)
            .expect(1)
            .mount(&endpoint)
            .await;
        let home = TempDir::new().unwrap();
        let out = paloma(None, &home)
            .env("PALOMA_VERIFY_TOKEN", VERIFY)
            .args(["webhook", "verify-url", "-o", "json"])
            .arg(endpoint.uri())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        let v = stdout_json(&out);
        assert_eq!(v["passed"], false);
        assert_eq!(v["status"], status);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(needle), "{stderr}");
        assert!(!stderr.contains(VERIFY));
    }
}

#[test]
fn webhook_verify_url_unreachable_is_exit_1_without_leaking_the_token() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .env("PALOMA_VERIFY_TOKEN", VERIFY)
        .args(["webhook", "verify-url"])
        .arg(format!("http://127.0.0.1:{port}/hook"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("cannot reach the endpoint"), "{stderr}");
    assert!(!stderr.contains(VERIFY), "{stderr}");
}

#[test]
fn webhook_verify_url_validates_first_and_previews_with_the_token_masked() {
    let home = TempDir::new().unwrap();
    // A bad URL fails before the token is even looked for.
    paloma(None, &home)
        .args(["webhook", "verify-url", "ftp://example.test"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("http or https"));
    paloma(None, &home)
        .args(["webhook", "verify-url", "https://example.test/hook"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("PALOMA_VERIFY_TOKEN"));
    paloma(None, &home)
        .args(["webhook", "verify-url"])
        .assert()
        .code(2);
    let out = paloma(None, &home)
        .env("PALOMA_VERIFY_TOKEN", VERIFY)
        .args(["webhook", "verify-url", "https://example.test/hook"])
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v = stdout_json(&out);
    assert_eq!(v["method"], "GET");
    assert_eq!(v["url"], "https://example.test/hook");
    assert_eq!(v["query"]["hub.mode"], "subscribe");
    assert_eq!(v["query"]["hub.verify_token"], "<redacted>");
    assert!(!String::from_utf8_lossy(&out.stdout).contains(VERIFY));
    assert!(String::from_utf8_lossy(&out.stderr).contains("dry-run: no request sent"));
}

#[test]
fn webhook_verify_url_warns_on_plain_http_and_reads_the_token_from_stdin() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args(["webhook", "verify-url", "http://example.test/hook"])
        .args(["--verify-token-stdin", "--dry-run"])
        .write_stdin(VERIFY)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("isn't HTTPS"), "{stderr}");
    assert!(!stderr.contains(VERIFY));
}

/// The endpoint's view of one `simulate`: signature header and exact body.
async fn simulate_against(
    status: u16,
    args: &[&str],
) -> (std::process::Output, Option<String>, Vec<u8>) {
    let endpoint = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/hook"))
        .and(header("content-type", "application/json"))
        .respond_with(ResponseTemplate::new(status))
        .mount(&endpoint)
        .await;
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .env("PALOMA_APP_SECRET", APP_SECRET)
        .args(["webhook", "simulate", "-o", "json", "-vv"])
        .arg(format!("{}/hook", endpoint.uri()))
        .args(args)
        .output()
        .unwrap();
    let requests = endpoint.received_requests().await.unwrap();
    assert_eq!(
        requests.len(),
        1,
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let signature = requests[0]
        .headers
        .get("x-hub-signature-256")
        .map(|v| v.to_str().unwrap().to_owned());
    (out, signature, requests[0].body.clone())
}

fn assert_no_secrets(out: &std::process::Output) {
    for stream in [&out.stdout, &out.stderr] {
        let text = String::from_utf8_lossy(stream);
        assert!(!text.contains(APP_SECRET), "{text}");
        assert!(!text.contains(VERIFY), "{text}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn webhook_simulate_signs_the_exact_body_with_the_app_secret() {
    let (out, signature, body) = simulate_against(200, &[]).await;
    assert!(out.status.success());
    assert_eq!(
        signature.unwrap(),
        paloma_graph::callback::sign(APP_SECRET, &body)
    );
    let v = stdout_json(&out);
    assert_eq!(v["passed"], true);
    assert_eq!(v["event"], "text");
    assert_eq!(v["signature"], "valid");
    assert_no_secrets(&out);
    let sent: Value = serde_json::from_slice(&body).unwrap();
    let value = &sent["entry"][0]["changes"][0]["value"];
    assert_eq!(sent["entry"][0]["id"], WABA);
    assert_eq!(value["metadata"]["phone_number_id"], PHONE);
    assert_eq!(
        value["messages"][0]["from_user_id"],
        value["contacts"][0]["user_id"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn webhook_simulate_events_carry_the_bsuid_and_pricing_shapes() {
    let (_, _, body) = simulate_against(200, &["--event", "text-username"]).await;
    let sent: Value = serde_json::from_slice(&body).unwrap();
    let value = &sent["entry"][0]["changes"][0]["value"];
    assert!(value["contacts"][0].get("wa_id").is_none());
    assert!(value["messages"][0].get("from").is_none());
    assert!(value["contacts"][0]["user_id"].is_string());

    let (_, _, body) = simulate_against(200, &["--event", "user-changed-user-id"]).await;
    let sent: Value = serde_json::from_slice(&body).unwrap();
    let message = &sent["entry"][0]["changes"][0]["value"]["messages"][0];
    assert_eq!(message["system"]["type"], "user_changed_user_id");
    assert!(message.get("from").is_none());

    let (_, _, body) = simulate_against(200, &["--event", "status-paid-exempt"]).await;
    let sent: Value = serde_json::from_slice(&body).unwrap();
    let status = &sent["entry"][0]["changes"][0]["value"]["statuses"][0];
    assert_eq!(status["pricing"]["subtype"], "paid_exempt");

    let (_, _, body) = simulate_against(200, &["--event", "status-marketing-lite-dynamic"]).await;
    let sent: Value = serde_json::from_slice(&body).unwrap();
    let status = &sent["entry"][0]["changes"][0]["value"]["statuses"][0];
    assert_eq!(status["pricing"]["category"], "marketing_lite_dynamic");

    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args([
            "webhook",
            "simulate",
            "https://example.test",
            "--event",
            "nope",
        ])
        .assert()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn webhook_simulate_only_counts_200_as_delivered() {
    for status in [204, 500] {
        let (out, _, _) = simulate_against(status, &[]).await;
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(stdout_json(&out)["passed"], false);
        assert!(String::from_utf8_lossy(&out.stderr).contains("anything but 200"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn webhook_simulate_bad_signatures_pass_only_if_the_endpoint_rejects_them() {
    let (out, signature, body) = simulate_against(403, &["--signature", "wrong"]).await;
    assert!(out.status.success());
    let signature = signature.unwrap();
    assert!(signature.starts_with("sha256="));
    assert_eq!(signature.len(), "sha256=".len() + 64);
    assert_ne!(signature, paloma_graph::callback::sign(APP_SECRET, &body));
    assert_eq!(stdout_json(&out)["passed"], true);

    let (out, signature, _) = simulate_against(200, &["--signature", "wrong"]).await;
    assert_eq!(out.status.code(), Some(1));
    assert!(signature.is_some());
    assert!(String::from_utf8_lossy(&out.stderr).contains("isn't checking"));

    let (out, signature, _) = simulate_against(401, &["--signature", "missing"]).await;
    assert!(out.status.success());
    assert!(signature.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn webhook_simulate_needs_no_app_secret_to_sign_wrong() {
    let endpoint = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&endpoint)
        .await;
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(["webhook", "simulate", "--signature", "wrong"])
        .arg(endpoint.uri())
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
async fn webhook_simulate_file_sends_and_signs_the_bytes_unchanged() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("event.json");
    let original = b"{ \"object\" : \"custom\",\n  \"n\": 1.10 }\n";
    std::fs::write(&file, original).unwrap();
    let (out, signature, body) = simulate_against(200, &["--file", file.to_str().unwrap()]).await;
    assert!(out.status.success());
    assert_eq!(body, original);
    assert_eq!(
        signature.unwrap(),
        paloma_graph::callback::sign(APP_SECRET, original)
    );
    assert_eq!(stdout_json(&out)["event"], "file");
}

#[test]
fn webhook_simulate_validates_before_needing_the_app_secret() {
    let home = TempDir::new().unwrap();
    let bad = home.path().join("bad.json");
    std::fs::write(&bad, "not json").unwrap();
    let big = home.path().join("big.json");
    std::fs::write(&big, format!("\"{}\"", "a".repeat(3 * 1024 * 1024))).unwrap();
    let url = "https://example.test/hook";
    for (args, needle) in [
        (vec!["ftp://example.test"], "http or https"),
        (vec![url, "--file", bad.to_str().unwrap()], "not valid JSON"),
        (vec![url, "--file", big.to_str().unwrap()], "at most"),
        (vec![url, "--file", "/no/such/file"], "not found"),
    ] {
        paloma(None, &home)
            .args(["webhook", "simulate"])
            .args(&args)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(needle));
    }
    paloma(None, &home)
        .args(["webhook", "simulate", url, "--event", "text", "--file", "x"])
        .assert()
        .code(2);
}

#[test]
fn webhook_simulate_without_an_app_secret_is_an_auth_error_and_sends_nothing() {
    let home = TempDir::new().unwrap();
    // The URL refuses connections, so reaching it would be exit 1, not 3.
    paloma(None, &home)
        .args(["webhook", "simulate", "http://127.0.0.1:9/hook"])
        .assert()
        .code(3)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("no app secret"));
}

#[test]
fn webhook_simulate_dry_run_shows_the_request_without_the_secret() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .env("PALOMA_APP_SECRET", APP_SECRET)
        .args([
            "webhook",
            "simulate",
            "http://127.0.0.1:9/hook",
            "--dry-run",
            "-o",
            "json",
        ])
        .args(["--event", "user-changed-user-id"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v = stdout_json(&out);
    assert_eq!(v["method"], "POST");
    assert_eq!(
        v["headers"]["X-Hub-Signature-256"],
        "sha256=<computed when sent>"
    );
    assert_eq!(
        v["body"]["entry"][0]["changes"][0]["value"]["messages"][0]["system"]["type"],
        "user_changed_user_id"
    );
    assert_no_secrets(&out);
    let raw = paloma(None, &home)
        .args([
            "webhook",
            "simulate",
            "http://127.0.0.1:9/",
            "--dry-run",
            "-o",
            "raw",
        ])
        .args(["--signature", "missing"])
        .output()
        .unwrap();
    assert!(raw.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&raw.stdout).unwrap()["object"],
        "whatsapp_business_account"
    );
}

/// A running `webhook listen`, on an ephemeral port.
struct RunningListener {
    child: std::process::Child,
    port: u16,
    stderr: std::sync::Arc<std::sync::Mutex<String>>,
    lines: std::sync::mpsc::Receiver<String>,
}

impl RunningListener {
    fn start(home: &TempDir, app_secret: Option<&str>, extra: &[&str]) -> Self {
        use std::io::BufRead;
        use std::process::Stdio;

        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_paloma"));
        cmd.env_clear()
            .env("PALOMA_CONFIG", home.path().join("config.toml"))
            .env("PALOMA_VERIFY_TOKEN", VERIFY)
            .env("NO_COLOR", "1")
            .env("HOME", home.path())
            .args(["webhook", "listen", "--port", "0"])
            .args(extra)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(secret) = app_secret {
            cmd.env("PALOMA_APP_SECRET", secret);
        }
        let mut child = cmd.spawn().unwrap();

        let stderr = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let (port_tx, port_rx) = std::sync::mpsc::channel();
        let pipe = std::io::BufReader::new(child.stderr.take().unwrap());
        let sink = stderr.clone();
        std::thread::spawn(move || {
            for line in pipe.lines().map_while(std::result::Result::ok) {
                if let Some(rest) = line.strip_prefix("listening on http://") {
                    let addr = rest.split_whitespace().next().unwrap();
                    let port: u16 = addr.rsplit(':').next().unwrap().parse().unwrap();
                    let _ = port_tx.send(port);
                }
                let mut text = sink.lock().unwrap();
                text.push_str(&line);
                text.push('\n');
            }
        });
        let (line_tx, lines) = std::sync::mpsc::channel();
        let pipe = std::io::BufReader::new(child.stdout.take().unwrap());
        std::thread::spawn(move || {
            for line in pipe.lines().map_while(std::result::Result::ok) {
                let _ = line_tx.send(line);
            }
        });
        let port = port_rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("the listener announces its port");
        Self {
            child,
            port,
            stderr,
            lines,
        }
    }

    /// One HTTP/1.1 exchange; `(status, body)`.
    fn http(&self, request_line: &str, headers: &[(&str, &str)], body: &[u8]) -> (u16, String) {
        http_exchange(self.port, request_line, headers, body)
    }

    fn post(&self, signature: Option<&str>, body: &[u8]) -> u16 {
        let headers: Vec<(&str, &str)> = signature
            .map(|s| ("X-Hub-Signature-256", s))
            .into_iter()
            .collect();
        self.http("POST /webhook HTTP/1.1", &headers, body).0
    }

    fn next_line(&self) -> String {
        self.lines
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("an event on stdout")
    }

    fn stderr(&self) -> String {
        self.stderr.lock().unwrap().clone()
    }

    /// The listener answers before it logs, so a warning can arrive just after the response.
    fn stderr_with(&self, needle: &str, count: usize) -> String {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let stderr = self.stderr();
            if stderr.matches(needle).count() >= count || std::time::Instant::now() > deadline {
                return stderr;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

impl Drop for RunningListener {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn http_exchange(
    port: u16,
    request_line: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> (u16, String) {
    use std::io::{Read, Write};

    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let mut head = format!(
        "{request_line}\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        head += name;
        head += ": ";
        head += value;
        head += "\r\n";
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).unwrap();
    // The server may answer and close before an oversized body is all written.
    let _ = stream.write_all(body);
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    let response = String::from_utf8_lossy(&response).into_owned();
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("no status in {response:?}"));
    let body = response
        .split_once("\r\n\r\n")
        .map_or("", |(_, b)| b)
        .to_owned();
    (status, body)
}

#[test]
fn webhook_listen_answers_the_handshake_only_with_the_right_token() {
    let home = TempDir::new().unwrap();
    let listener = RunningListener::start(&home, Some(APP_SECRET), &[]);
    let (status, body) = listener.http(
        &format!("GET /webhook?hub.mode=subscribe&hub.challenge=1158201444&hub.verify_token={VERIFY} HTTP/1.1"),
        &[],
        b"",
    );
    assert_eq!((status, body.as_str()), (200, "1158201444"));
    for query in [
        "hub.mode=subscribe&hub.challenge=1&hub.verify_token=nope",
        "hub.mode=subscribe&hub.challenge=1",
        &format!("hub.mode=unsubscribe&hub.challenge=1&hub.verify_token={VERIFY}"),
        &format!("hub.mode=subscribe&hub.verify_token={VERIFY}"),
        "",
    ] {
        let (status, body) = listener.http(&format!("GET /webhook?{query} HTTP/1.1"), &[], b"");
        assert_eq!(status, 403, "{query}");
        assert!(body.is_empty());
    }
    let stderr = listener.stderr_with("rejected handshake", 5);
    assert!(stderr.contains("verified"), "{stderr}");
    assert!(stderr.contains("rejected handshake"), "{stderr}");
    assert!(!stderr.contains(VERIFY), "{stderr}");
    assert!(!stderr.contains(APP_SECRET), "{stderr}");
}

#[test]
fn webhook_listen_prints_signed_events_as_lines_and_rejects_the_rest() {
    let home = TempDir::new().unwrap();
    let listener = RunningListener::start(&home, Some(APP_SECRET), &[]);
    let body: &[u8] = br#"{"bad": true}"#;
    let wrong = paloma_graph::callback::sign("not-the-secret", body);
    assert_eq!(listener.post(Some(&wrong), body), 403);
    assert_eq!(listener.post(None, body), 403);
    assert_eq!(listener.post(Some("sha256=zz"), body), 403);
    let stderr = listener.stderr_with("answered 403", 3);
    assert!(
        stderr.contains("a bad X-Hub-Signature-256 signature"),
        "{stderr}"
    );
    assert!(
        stderr.contains("no X-Hub-Signature-256 signature"),
        "{stderr}"
    );
    assert!(stderr.contains("answered 403"), "{stderr}");

    // Unknown fields, new enums and numbers serde_json can't hold all pass through; the
    // whitespace goes, so one delivery stays one line.
    let event = br#"{
      "object": "whatsapp_business_account",
      "brand_new_field": {"x": 123456789012345678901234567890, "price": 1.10},
      "entry": [{"id": "1", "changes": [{"field": "messages", "value": {
        "contacts": [{"profile": {"name": "A B"}, "user_id": "US.1"}],
        "messages": [{"from_user_id": "US.1", "id": "wamid.1", "type": "system",
          "system": {"type": "user_changed_user_id", "user_id": "US.1", "previous_user_id": "US.0"}}],
        "statuses": [{"id": "wamid.2", "pricing": {"category": "marketing_lite_dynamic", "subtype": "paid_exempt"}}]
      }}]}]
    }"#;
    let signature = paloma_graph::callback::sign(APP_SECRET, event);
    assert_eq!(listener.post(Some(&signature), event), 200);
    // The rejected ones above never reached stdout, so this is the first line.
    let line = listener.next_line();
    let parsed: Value = serde_json::from_str(&line).unwrap();
    assert!(line.contains("123456789012345678901234567890"), "{line}");
    assert!(line.contains(r#""price":1.10"#), "{line}");
    assert!(line.contains("marketing_lite_dynamic"), "{line}");
    assert!(line.contains("paid_exempt"), "{line}");
    let value = &parsed["entry"][0]["changes"][0]["value"];
    assert_eq!(value["messages"][0]["system"]["previous_user_id"], "US.0");
    assert!(value["contacts"][0].get("wa_id").is_none());

    // A signed body that isn't JSON is refused, not printed.
    let signature = paloma_graph::callback::sign(APP_SECRET, b"nope");
    assert_eq!(listener.post(Some(&signature), b"nope"), 400);
    assert!(listener.stderr_with("isn't JSON", 1).contains("isn't JSON"));
    assert_eq!(listener.http("PUT /webhook HTTP/1.1", &[], b"").0, 405);
    assert!(!listener.stderr().contains(APP_SECRET));
}

#[test]
fn webhook_listen_without_an_app_secret_accepts_events_and_says_so() {
    let home = TempDir::new().unwrap();
    let listener = RunningListener::start(&home, None, &[]);
    assert!(
        listener
            .stderr_with("signatures are NOT checked", 1)
            .contains("signatures are NOT checked")
    );
    assert_eq!(listener.post(None, br#"{"object":"x"}"#), 200);
    assert_eq!(listener.next_line(), r#"{"object":"x"}"#);
}

#[test]
fn webhook_listen_takes_a_full_size_payload_and_refuses_a_larger_one() {
    let home = TempDir::new().unwrap();
    let listener = RunningListener::start(&home, None, &[]);
    let max = 3 * 1024 * 1024;
    let fits = format!("\"{}\"", "a".repeat(max - 2));
    assert_eq!(listener.post(None, fits.as_bytes()), 200);
    assert_eq!(listener.next_line().len(), max);
    let over = format!("\"{}\"", "a".repeat(max));
    assert_eq!(listener.post(None, over.as_bytes()), 413);
    assert!(
        listener
            .stderr_with("rejected POST", 1)
            .contains("rejected POST")
    );
}

#[test]
fn webhook_listen_quiet_prints_only_the_ids() {
    let home = TempDir::new().unwrap();
    let listener = RunningListener::start(&home, None, &["-q"]);
    let event = br#"{"entry":[{"changes":[{"value":{"messages":[{"id":"wamid.1"}],"statuses":[{"id":"wamid.2"}]}}]}]}"#;
    assert_eq!(listener.post(None, event), 200);
    assert_eq!(listener.next_line(), "wamid.1");
    assert_eq!(listener.next_line(), "wamid.2");
}

#[test]
fn webhook_listen_receives_what_simulate_sends() {
    let home = TempDir::new().unwrap();
    let listener = RunningListener::start(&home, Some(APP_SECRET), &[]);
    let url = format!("http://127.0.0.1:{}/webhook", listener.port);
    for (event, needle) in [
        ("text-username", "from_user_id"),
        ("user-changed-user-id", "user_changed_user_id"),
        ("status-paid-exempt", "paid_exempt"),
        ("status-marketing-lite-dynamic", "marketing_lite_dynamic"),
    ] {
        paloma(None, &home)
            .env("PALOMA_APP_SECRET", APP_SECRET)
            .args(["webhook", "simulate", &url, "--event", event])
            .assert()
            .success();
        assert!(listener.next_line().contains(needle), "{event}");
    }
    paloma(None, &home)
        .env("PALOMA_APP_SECRET", APP_SECRET)
        .args(["webhook", "simulate", &url, "--signature", "wrong"])
        .assert()
        .success();
    let verify = paloma(None, &home)
        .env("PALOMA_VERIFY_TOKEN", VERIFY)
        .args(["webhook", "verify-url", &url])
        .output()
        .unwrap();
    assert!(verify.status.success());
}

#[cfg(unix)]
#[test]
fn webhook_listen_exits_0_on_ctrl_c() {
    let home = TempDir::new().unwrap();
    let mut listener = RunningListener::start(&home, None, &[]);
    let pid = listener.child.id().to_string();
    assert!(
        std::process::Command::new("kill")
            .args(["-INT", &pid])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(listener.child.wait().unwrap().code(), Some(0));
}

#[test]
fn webhook_listen_stops_when_nobody_reads_stdout() {
    use std::io::BufRead;
    use std::process::Stdio;

    let home = TempDir::new().unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_paloma"))
        .env_clear()
        .env("PALOMA_CONFIG", home.path().join("config.toml"))
        .env("PALOMA_VERIFY_TOKEN", VERIFY)
        .env("HOME", home.path())
        .args(["webhook", "listen", "--port", "0"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let mut stderr = std::io::BufReader::new(child.stderr.take().unwrap());
    let port = loop {
        let mut line = String::new();
        assert!(
            stderr.read_line(&mut line).unwrap() > 0,
            "no listening line"
        );
        if let Some(rest) = line.strip_prefix("listening on http://127.0.0.1:") {
            break rest.split_whitespace().next().unwrap().parse().unwrap();
        }
    };
    let (status, _) = http_exchange(port, "POST / HTTP/1.1", &[], br#"{"a":1}"#);
    assert_eq!(status, 503);
    assert_eq!(child.wait().unwrap().code(), Some(0));
}

#[test]
fn webhook_listen_dry_run_binds_nothing_and_bad_input_fails_early() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env_remove("PALOMA_TOKEN")
        .args(["webhook", "listen", "--dry-run", "--port", "4242"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(
            predicate::str::contains("nothing was bound")
                .and(predicate::str::contains("http://127.0.0.1:4242")),
        );
    paloma(None, &home)
        .args(["webhook", "listen", "--port", "0"])
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("PALOMA_VERIFY_TOKEN"));
    paloma(None, &home)
        .args(["webhook", "listen", "--host", "not-an-ip"])
        .assert()
        .code(2);
    paloma(None, &home)
        .args(["webhook", "listen", "--port", "70000"])
        .assert()
        .code(2);
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port().to_string();
    paloma(None, &home)
        .env("PALOMA_VERIFY_TOKEN", VERIFY)
        .args(["webhook", "listen", "--port", &port])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("cannot listen on"));
}

#[test]
fn webhook_listen_warns_when_bound_beyond_loopback() {
    let home = TempDir::new().unwrap();
    let listener = RunningListener::start(&home, None, &["--host", "0.0.0.0"]);
    assert!(
        listener
            .stderr_with("reachable from your network", 1)
            .contains("reachable from your network")
    );
}

#[test]
fn webhook_subscribe_refuses_a_blank_or_non_https_callback_before_the_token() {
    let home = TempDir::new().unwrap();
    for form in [&[][..], &["--number"][..]] {
        for (url, needle) in [
            ("", "--callback-url"),
            ("   ", "--callback-url"),
            ("not a url", "not a valid URL"),
            ("ftp://example.test/hook", "not an http or https URL"),
            ("http://example.test/hook", "must be an https URL"),
        ] {
            paloma(None, &home)
                .args(["webhook", "subscribe", "--callback-url", url, "--yes"])
                .args(form)
                .assert()
                .code(5)
                .stdout(predicate::str::is_empty())
                .stderr(
                    predicate::str::contains(needle)
                        .and(predicate::str::contains("PALOMA_VERIFY_TOKEN").not()),
                );
        }
    }
}

/// An endpoint that declares a large body, sends a little of it, then stalls. Reading it to
/// the end would run into `--timeout`, and one byte past the challenge is already an answer.
#[test]
fn webhook_verify_url_reads_no_more_of_the_body_than_it_needs() {
    use std::io::{Read, Write};

    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    let (release, held) = std::sync::mpsc::channel::<()>();
    let endpoint = std::thread::spawn(move || {
        let (mut stream, _) = server.accept().unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\n\r\nxxxxxxxxxxx")
            .unwrap();
        let _ = held.recv();
    });
    let home = TempDir::new().unwrap();
    let url = format!("http://127.0.0.1:{port}/hook");
    paloma(None, &home)
        .env("PALOMA_VERIFY_TOKEN", VERIFY)
        .args(["webhook", "verify-url", &url, "--timeout", "10s"])
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("isn't exactly the hub.challenge")
                .and(predicate::str::contains("timed out").not()),
        );
    drop(release);
    endpoint.join().unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn webhook_listen_reports_a_failed_write_and_exits_1() {
    use std::io::{BufRead, Read};
    use std::process::Stdio;

    let home = TempDir::new().unwrap();
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_paloma"))
        .env_clear()
        .env("PALOMA_CONFIG", home.path().join("config.toml"))
        .env("PALOMA_VERIFY_TOKEN", VERIFY)
        .env("NO_COLOR", "1")
        .env("HOME", home.path())
        .args(["webhook", "listen", "--port", "0"])
        .stdin(Stdio::null())
        .stdout(Stdio::from(full))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = std::io::BufReader::new(child.stderr.take().unwrap());
    let port = loop {
        let mut line = String::new();
        assert!(
            stderr.read_line(&mut line).unwrap() > 0,
            "no listening line"
        );
        if let Some(rest) = line.strip_prefix("listening on http://127.0.0.1:") {
            break rest.split_whitespace().next().unwrap().parse().unwrap();
        }
    };
    let (status, _) = http_exchange(port, "POST / HTTP/1.1", &[], br#"{"a":1}"#);
    assert_eq!(status, 503);
    assert_eq!(child.wait().unwrap().code(), Some(1));
    let mut rest = String::new();
    stderr.read_to_string(&mut rest).unwrap();
    assert!(rest.contains("cannot write the event to stdout"), "{rest}");
    assert!(!rest.contains("stopped"), "{rest}");
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
        .args(["export", "account", "--template"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("kind: Account")
                .and(predicate::str::contains("# timezoneId:"))
                .and(predicate::str::contains("phoneNumberId").not())
                .and(predicate::str::contains("disableMarketing").not()),
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn message_history_with_no_entry_says_a_delivered_message_can_have_none() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}/message_history")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [] })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["message", "history", "wamid.DELIVERED"])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(
            predicate::str::contains("Meta returned no message history for wamid.DELIVERED")
                .and(predicate::str::contains("a message it delivered"))
                .and(predicate::str::contains(
                    "doesn't mean the message doesn't exist",
                )),
        );
}

const QR_CODE: &str = "WOMVT6TJ2BP7A1";
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
        .args(["export", "qrcode", "--template"])
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
async fn account_insights_is_confirmed_one_way_and_uses_query() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}")))
        .and(query_param("is_enabled_for_insights", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(3)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["account", "set", "--enable-insights"])
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("--yes"));
    for args in [
        vec!["--enable-insights", "--yes"],
        vec!["--insights", "true", "--yes"],
    ] {
        paloma(Some(&graph), &home)
            .args(["account", "set"])
            .args(args)
            .assert()
            .success()
            .stderr("");
    }
    for args in [
        vec!["--insights", "false"],
        vec!["--insights", "true", "--name", "Acme"],
    ] {
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["account", "set"])
            .args(args)
            .assert()
            .code(5)
            .stdout("");
    }
    let file = home.path().join("insights.json");
    for body in [
        r#"{"is_enabled_for_insights":false}"#,
        r#"{"is_enabled_for_insights":true,"name":"Acme"}"#,
    ] {
        std::fs::write(&file, body).unwrap();
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["account", "set", "--file"])
            .arg(&file)
            .assert()
            .code(5)
            .stdout("");
    }
    std::fs::write(&file, r#"{"is_enabled_for_insights":true}"#).unwrap();
    paloma(Some(&graph), &home)
        .args(["account", "set", "--file"])
        .arg(&file)
        .arg("--yes")
        .assert()
        .success();
    let requests = graph.received_requests().await.unwrap();
    assert!(requests.iter().all(|request| request.body.is_empty()));
    let out = paloma(Some(&graph), &home)
        .args([
            "account",
            "set",
            "--enable-insights",
            "--dry-run",
            "-o",
            "json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        stdout_json(&out)["url"]
            .as_str()
            .unwrap()
            .contains("is_enabled_for_insights=true")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn account_limits_portfolio_and_activities_limit_are_validated() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path("/v26.0/333"))
        .and(query_param(
            "fields",
            "whatsapp_business_manager_messaging_limit",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"whatsapp_business_manager_messaging_limit":"TIER_250"})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_WABA_ID")
        .args(["account", "limits", "--business-id", "333"])
        .assert()
        .success()
        .stdout(predicate::str::contains("TIER_250"));
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/activities")))
        .and(query_param("limit", "100"))
        .and(query_param("after", "cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[]})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "account",
            "activities",
            "--limit",
            "100",
            "--after",
            "cursor",
        ])
        .assert()
        .success();
    for args in [
        vec!["limits", "--business-id", "x/y"],
        vec!["activities", "--limit", "101"],
    ] {
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["account"])
            .args(args)
            .assert()
            .code(5)
            .stdout("");
    }
}

#[test]
fn automation_rejects_duplicate_commands_and_accepts_source_aliases() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args([
            "automation",
            "set",
            "--command",
            "help:First",
            "--command",
            "help=Second",
        ])
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("duplicate command"));
    let out = paloma(None, &home)
        .args([
            "automation",
            "set",
            "--welcome-message",
            "false",
            "--ice-breaker",
            "Hello",
            "--command",
            "help=Talk: support",
            "--dry-run",
            "-o",
            "json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        stdout_json(&out)["body"],
        json!({"enable_welcome_message":false,"prompts":["Hello"],"commands":[{"command_name":"help","command_description":"Talk: support"}]})
    );
}

#[test]
fn qr_positional_message_and_default_image_are_previewed_offline() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args([
            "qr",
            "create",
            "Hello",
            "--image",
            "SVG",
            "--dry-run",
            "-o",
            "json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let requests = stdout_json(&out);
    assert_eq!(
        requests[0]["body"],
        json!({"prefilled_message":"Hello","generate_qr_image":"SVG"})
    );
    assert_eq!(requests.as_array().unwrap().len(), 2);
    paloma(None, &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["qr", "create", " "])
        .assert()
        .code(5)
        .stdout("");
}

#[tokio::test(flavor = "multi_thread")]
async fn qr_default_image_uses_a_safe_filename_without_sending_the_token() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}/message_qrdls/{QR_CODE}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"data":[{"code":QR_CODE,"qr_image_url":format!("{}/image",graph.uri())}]}),
        ))
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path("/image"))
        .and(|request: &wiremock::Request| !request.headers.contains_key("authorization"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"image".to_vec()))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .current_dir(home.path())
        .args(["qr", "get", QR_CODE, "--image", "png", "-q"])
        .assert()
        .success()
        .stdout(format!("{QR_CODE}\n"));
    let saved = home.path().join(format!("{QR_CODE}.png"));
    assert_eq!(std::fs::read(&saved).unwrap(), b"image");
    paloma(Some(&graph), &home)
        .current_dir(home.path())
        .args(["qr", "get", QR_CODE, "--image", "png", "-q"])
        .assert()
        .code(5)
        .stdout("");
    assert_eq!(std::fs::read(saved).unwrap(), b"image");
}

#[tokio::test(flavor = "multi_thread")]
async fn health_tables_show_entity_diagnostics_but_quiet_keeps_the_id() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET")).and(path(format!("/v26.0/{WABA}")))
        .and(query_param("fields", "health_status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":WABA,"health_status":{"can_send_message":"LIMITED","entities":[{"entity_type":"WABA","id":WABA,"can_send_message":"BLOCKED","errors":[{"error_code":42,"error_description":"Payment missing","possible_solution":"Add payment"}]}]}})))
        .mount(&graph).await;
    paloma(Some(&graph), &home)
        .args(["account", "health"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Payment missing")
                .and(predicate::str::contains("Add payment")),
        );
    paloma(Some(&graph), &home)
        .args(["account", "health", "-q"])
        .assert()
        .success()
        .stdout(format!("{WABA}\n"))
        .stderr("");
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

#[test]
fn number_explicit_id_validation_precedes_context() {
    let home = TempDir::new().unwrap();
    for verb in ["get", "quality", "health"] {
        paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["number", verb, "../bad"])
            .assert()
            .code(5)
            .stdout("");
    }
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

// ---- v1.5 identity, settings and administration contracts ----
#[tokio::test(flavor = "multi_thread")]
async fn number_onboarding_and_identity_writes_use_exact_payloads() {
    let home = TempDir::new().unwrap();
    for (args, node, body, id) in [
        (
            vec![
                "number",
                "request-code",
                "--method",
                "SMS",
                "--language",
                "es_ES",
            ],
            "111/request_code",
            json!({"code_method":"SMS","language":"es_ES"}),
            false,
        ),
        (
            vec!["number", "add", "15551234567", "--name", "Acme"],
            "222/phone_numbers",
            json!({"phone_number":"15551234567","verified_name":"Acme"}),
            true,
        ),
        (
            vec!["number", "set-name", "Acme Support"],
            "111",
            json!({"new_display_name":"Acme Support"}),
            false,
        ),
        (
            vec!["number", "search-visibility", "HIDDEN"],
            "111",
            json!({"search_visibility":"HIDDEN"}),
            false,
        ),
        (
            vec![
                "number",
                "username",
                "set",
                "Acme_Store",
                "--force-transfer",
            ],
            "111/username",
            json!({"username":"Acme_Store","transfer_action":"force_transfer"}),
            false,
        ),
        (
            vec!["number", "settings", "storage", "--region", "IN"],
            "111/settings",
            json!({"storage_configuration":{"status":"IN_COUNTRY_STORAGE_ENABLED","data_localization_region":"IN"}}),
            false,
        ),
        (
            vec!["number", "settings", "storage", "--disable"],
            "111/settings",
            json!({"storage_configuration":{"status":"IN_COUNTRY_STORAGE_DISABLED"}}),
            false,
        ),
        (
            vec!["number", "settings", "identity-check", "--enabled", "false"],
            "111/settings",
            json!({"user_identity_change":{"enable_identity_key_check":false}}),
            false,
        ),
    ] {
        let graph = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{node}")))
            .and(body_json(body.clone()))
            .respond_with(ResponseTemplate::new(200).set_body_json(if id {
                json!({"id":"444"})
            } else {
                json!({"success":true})
            }))
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args(&args)
            .args(["--yes", "-q"])
            .assert()
            .success()
            .stdout(if id { "444\n" } else { "" });
        let preview = dry_run_request(&home, &args);
        assert_eq!(preview["body"], body, "{args:?}");
        assert_eq!(graph.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn verification_code_uses_secret_sources_and_never_appears_in_diagnostics() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("POST"))
        .and(path("/v26.0/111/verify_code"))
        .and(body_json(json!({"code":"817263"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(2)
        .mount(&graph)
        .await;
    let out = paloma(Some(&graph), &home)
        .env("PALOMA_VERIFICATION_CODE", "817263")
        .args(["number", "verify-code", "-vv", "-q"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("817263"));
    paloma(Some(&graph), &home)
        .args(["number", "verify-code", "--code-stdin", "-q"])
        .write_stdin("817263\n")
        .assert()
        .success()
        .stdout("");
    let out = paloma(Some(&graph), &home)
        .env("PALOMA_VERIFICATION_CODE", "817263")
        .args(["number", "verify-code", "--dry-run", "-o", "json", "-vv"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(stdout_json(&out)["body"]["code"], "<redacted>");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("817263"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("817263"));
    assert_eq!(graph.received_requests().await.unwrap().len(), 2);
    // A template's language code has a different meaning and stays visible.
    assert_eq!(
        dry_run_request(
            &home,
            &[
                "send",
                "template",
                "15551234567",
                "hello",
                "--lang",
                "en_US",
                "--via",
                "cloud"
            ]
        )["body"]["template"]["language"]["code"],
        "en_US"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn usernames_contacts_and_settings_reads_and_deletes_keep_their_contracts() {
    let home = TempDir::new().unwrap();
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v26.0/111/username"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"username":"acme_shop","status":"approved"})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["number", "username", "get", "-q"])
        .assert()
        .success()
        .stdout("acme_shop\n")
        .stderr("");
    Mock::given(method("GET"))
        .and(path("/v26.0/111/username_suggestions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data":[{"username_suggestions":["acme_shop"]}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["number", "username", "reserved", "-o", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("acme_shop"));
    Mock::given(method("DELETE"))
        .and(path("/v26.0/111/username"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["number", "username", "delete", "--yes", "-q"])
        .assert()
        .success()
        .stdout("");
    for (verb, http) in [("get", "GET"), ("delete", "DELETE")] {
        Mock::given(method(http))
            .and(path("/v26.0/111/contact_book"))
            .and(query_param("messaging_product", "whatsapp"))
            .and(query_param("bsuid", "US.12345"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"contacts":[{"user_id":"US.12345"}]})),
            )
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args([
                "number", "contacts", verb, "US.12345", "--yes", "-o", "json",
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains("US.12345"));
        assert_eq!(
            dry_run_request(&home, &["number", "contacts", verb, "US.12345"])["method"],
            http
        );
    }
    Mock::given(method("GET")).and(path("/v26.0/111/settings")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"storage_configuration":{"status":"IN_COUNTRY_STORAGE_ENABLED","data_localization_region":"IN"}}))).expect(1).mount(&graph).await;
    paloma(Some(&graph), &home)
        .args(["number", "settings", "get", "-o", "raw"])
        .assert()
        .success()
        .stdout(predicate::str::contains("IN_COUNTRY_STORAGE_ENABLED"));
}

#[tokio::test(flavor = "multi_thread")]
async fn identity_and_settings_reject_bad_inputs_before_context_or_network() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for args in [
        vec!["number", "username", "set", "abc..def"],
        vec!["number", "username", "set", "12345"],
        vec!["number", "username", "set", "Acme.COM"],
        vec!["number", "contacts", "get", "US.ENT.123"],
        vec!["number", "contacts", "delete", "15551234567"],
        vec!["number", "settings", "storage", "--region", "XX"],
        vec!["number", "set-name", " "],
        vec!["number", "add", "+15551234567", "--name", "Acme"],
        vec![
            "number",
            "request-code",
            "--method",
            "SMS",
            "--language",
            " ",
        ],
        vec!["commerce", "set"],
        vec!["automation", "bot", "../bad"],
        vec![
            "flow",
            "metrics",
            "123",
            "--name",
            "ENDPOINT_REQUEST_COUNT",
            "--since",
            "2026-02-30",
        ],
        vec![
            "flow",
            "metrics",
            "123",
            "--name",
            "ENDPOINT_REQUEST_COUNT",
            "--since",
            "2026-11-04",
            "--until",
            "2026-11-03",
        ],
        vec!["flow", "migrate", "--source-waba-id", "../bad"],
        vec!["account", "users", "--business-id", "../bad"],
    ] {
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .env_remove("PALOMA_WABA_ID")
            .args(&args)
            .assert()
            .code(5)
            .stdout("");
    }
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .env("PALOMA_VERIFICATION_CODE", "BAD_SECRET")
        .args(["number", "verify-code"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("BAD_SECRET").not());
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn new_disruptive_commands_refuse_without_confirmation_and_preview_offline() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for args in [
        vec!["number", "request-code", "--method", "VOICE"],
        vec!["number", "set-name", "Acme"],
        vec!["number", "username", "set", "acme_shop"],
        vec!["number", "username", "delete"],
        vec!["number", "contacts", "delete", "US.123"],
        vec!["number", "settings", "storage", "--disable"],
        vec!["number", "settings", "identity-check", "--enabled", "true"],
        vec!["number", "search-visibility", "VISIBLE"],
        vec!["commerce", "set", "--cart-enabled", "false"],
        vec!["flow", "migrate", "--source-waba-id", "333"],
    ] {
        paloma(Some(&graph), &home)
            .args(&args)
            .assert()
            .failure()
            .stdout("")
            .stderr(predicate::str::contains("--yes"));
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_TOKEN")
            .args(&args)
            .args(["--dry-run", "-o", "json"])
            .assert()
            .success()
            .stdout(predicate::str::contains("method"));
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn commerce_uses_query_flags_including_false_and_preserves_list_output() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let body = json!({"data":[{"id":"333","is_cart_enabled":true,"is_catalog_visible":false}]});
    Mock::given(method("GET"))
        .and(path("/v26.0/111/whatsapp_commerce_settings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
        .expect(2)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["commerce", "get", "-q"])
        .assert()
        .success()
        .stdout("333\n");
    let out = paloma(Some(&graph), &home)
        .args(["commerce", "get", "-o", "json"])
        .output()
        .unwrap();
    assert_eq!(stdout_json(&out), body);
    Mock::given(method("POST"))
        .and(path("/v26.0/111/whatsapp_commerce_settings"))
        .and(query_param("is_cart_enabled", "false"))
        .and(query_param("is_catalog_visible", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "commerce",
            "set",
            "--cart-enabled",
            "false",
            "--catalog-visible",
            "true",
            "--yes",
            "-q",
        ])
        .assert()
        .success()
        .stdout("");
    assert_eq!(
        dry_run_request(&home, &["commerce", "set", "--cart-enabled", "false"])["url"],
        format!("{GRAPH}/{PHONE}/whatsapp_commerce_settings?is_cart_enabled=false")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn account_users_and_automation_bot_use_the_documented_nodes() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (args, node) in [
        (
            vec!["account", "users", "--business-id", "999", "--limit", "10"],
            "222/assigned_users",
        ),
        (vec!["account", "users", "--system"], "222/system_users"),
    ] {
        let mut mock = Mock::given(method("GET")).and(path(format!("/v26.0/{node}")));
        if node.ends_with("assigned_users") {
            mock = mock
                .and(query_param("business", "999"))
                .and(query_param("limit", "10"));
        }
        mock.respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data":[{"id":"777","name":"A"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
        paloma(Some(&graph), &home)
            .args(&args)
            .arg("-q")
            .assert()
            .success()
            .stdout("777\n");
        assert_eq!(dry_run_request(&home, &args)["method"], "GET");
    }
    Mock::given(method("GET"))
        .and(path("/v26.0/444"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id":"444","prompts":["Hi"]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["automation", "bot", "444", "-q"])
        .assert()
        .success()
        .stdout("444\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn flow_metrics_preserve_all_points_and_migration_copies_named_flows() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let body = json!({"id":"444","metric":{"name":"ENDPOINT_REQUEST_COUNT","data_points":[{"timestamp":"2026-11-01","data":[{"key":"value","value":5}]},{"timestamp":"2026-11-02","data":[{"key":"value","value":7}]}]}});
    let args = [
        "flow",
        "metrics",
        "444",
        "--name",
        "ENDPOINT_REQUEST_COUNT",
        "--granularity",
        "DAY",
        "--since",
        "2026-11-01",
        "--until",
        "2026-11-02",
    ];
    Mock::given(method("GET")).and(path("/v26.0/444")).and(query_param("fields","metric.name(ENDPOINT_REQUEST_COUNT).granularity(DAY).since(2026-11-01).until(2026-11-02)"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body.clone())).expect(1).mount(&graph).await;
    let out = paloma(Some(&graph), &home)
        .args(args)
        .args(["-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(stdout_json(&out), body);
    assert!(
        dry_run_request(&home, &args)["url"]
            .as_str()
            .unwrap()
            .contains("metric.name")
    );
    Mock::given(method("POST"))
        .and(path("/v26.0/222/migrate_flows"))
        .and(query_param("source_waba_id", "333"))
        .and(query_param("source_flow_names", "[\"booking\",\"survey\"]"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"migrated_flows":[{"source_id":"a","migrated_id":"b"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "flow",
            "migrate",
            "--source-waba-id",
            "333",
            "--name",
            "booking",
            "--name",
            "survey",
            "--yes",
            "-o",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("migrated_id"));
}

#[tokio::test(flavor = "multi_thread")]
async fn flow_key_reads_and_writes_public_pem_but_rejects_private_material_before_context() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("public.pem");
    let public = "-----BEGIN PUBLIC KEY-----\nTEST_PUBLIC_MATERIAL\n-----END PUBLIC KEY-----";
    std::fs::write(&file, public).unwrap();
    Mock::given(method("GET"))
        .and(path("/v26.0/111/whatsapp_business_encryption"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"business_public_key_signature_status":"VALID"})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["flow", "key", "get", "-o", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("VALID"));
    Mock::given(method("POST"))
        .and(path("/v26.0/111/whatsapp_business_encryption"))
        .and(body_string_contains("name=\"business_public_key\""))
        .and(body_string_contains(public))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["flow", "key", "set", "--public-key"])
        .arg(&file)
        .args(["--yes", "-q"])
        .assert()
        .success()
        .stdout("");
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_TOKEN")
        .args(["flow", "key", "set", "--public-key"])
        .arg(&file)
        .arg("--dry-run")
        .assert()
        .success();
    std::fs::write(
        &file,
        "-----BEGIN PRIVATE KEY-----\nPRIVATE_SENTINEL\n-----END PRIVATE KEY-----",
    )
    .unwrap();
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["flow", "key", "set", "--public-key"])
        .arg(&file)
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("PRIVATE_SENTINEL").not());
}

#[tokio::test(flavor = "multi_thread")]
async fn welcome_sequences_create_update_read_and_delete_waba_ad_assets() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("sequence.json");
    let sequence = json!({"text":"Welcome","autofill_message":{"content":"Hi"},"ice_breakers":[{"title":"Help"}]});
    std::fs::write(&file, sequence.to_string()).unwrap();
    let graph = MockServer::start().await;
    for (args, body, result, quiet) in [
        (
            vec![
                "account",
                "welcome-sequence",
                "create",
                "--name",
                "Support",
                "--file",
            ],
            json!({"name":"Support","welcome_message_sequence":sequence}),
            json!({"sequence_id":"444"}),
            "444\n",
        ),
        (
            vec![
                "account",
                "welcome-sequence",
                "set",
                "444",
                "--name",
                "New",
                "--file",
            ],
            json!({"name":"New","welcome_message_sequence":sequence,"sequence_id":"444"}),
            json!({"success":true}),
            "",
        ),
    ] {
        Mock::given(method("POST"))
            .and(path("/v26.0/222/welcome_message_sequences"))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(200).set_body_json(result))
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args(&args)
            .arg(&file)
            .args(["--yes", "-q"])
            .assert()
            .success()
            .stdout(quiet);
        paloma(Some(&graph), &home)
            .args(&args)
            .arg(&file)
            .arg("--dry-run")
            .assert()
            .success()
            .stdout(predicate::str::contains("welcome_message_sequence"));
    }
    Mock::given(method("GET"))
        .and(path("/v26.0/222/welcome_message_sequences"))
        .and(query_param("sequence_id", "444"))
        .and(query_param("limit", "5"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([{"sequence_id":"444","is_used_in_ad":false}])),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "account",
            "welcome-sequence",
            "list",
            "--id",
            "444",
            "--limit",
            "5",
            "-o",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("is_used_in_ad"));
    Mock::given(method("DELETE"))
        .and(path("/v26.0/222/welcome_message_sequences"))
        .and(query_param("sequence_id", "444"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "account",
            "welcome-sequence",
            "delete",
            "444",
            "--yes",
            "-q",
        ])
        .assert()
        .success()
        .stdout("");
    paloma(Some(&graph), &home)
        .args(["account", "welcome-sequence", "delete", "444"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--yes"));
    std::fs::write(&file, "{\"ice_breakers\":[42]}").unwrap();
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_WABA_ID")
        .args([
            "account",
            "welcome-sequence",
            "create",
            "--name",
            "Support",
            "--file",
        ])
        .arg(&file)
        .assert()
        .code(5)
        .stdout("");
}

#[tokio::test(flavor = "multi_thread")]
async fn oba_and_india_compliance_submit_validated_documents_and_confirm() {
    let home = TempDir::new().unwrap();
    let graph = MockServer::start().await;
    let file = home.path().join("document.json");
    for (group, verb, edge, body) in [
        (
            "oba",
            "apply",
            "official_business_account",
            json!({"business_website_url":"https://example.com","primary_country_of_operation":"US"}),
        ),
        (
            "compliance",
            "set",
            "business_compliance_info",
            json!({"messaging_product":"whatsapp","entity_name":"Acme","entity_type":"PRIVATE_COMPANY","grievance_officer_details":{"name":"Officer","email":"officer@example.com"},"customer_care_details":{"email":"care@example.com"}}),
        ),
    ] {
        std::fs::write(&file, body.to_string()).unwrap();
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/111/{edge}")))
            .and(body_json(body.clone()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
            .expect(1)
            .mount(&graph)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/v26.0/111/{edge}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[body.clone()]})))
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args(["number", group, verb, "--file"])
            .arg(&file)
            .args(["--yes", "-q"])
            .assert()
            .success()
            .stdout("");
        paloma(Some(&graph), &home)
            .args(["number", group, verb, "--file"])
            .arg(&file)
            .assert()
            .failure()
            .stderr(predicate::str::contains("--yes"));
        paloma(Some(&graph), &home)
            .args(["number", group, verb, "--file"])
            .arg(&file)
            .args(["--dry-run", "-o", "json"])
            .assert()
            .success()
            .stdout(predicate::str::contains("POST"));
        paloma(Some(&graph), &home)
            .args(["number", group, "get", "-o", "json"])
            .assert()
            .success()
            .stdout(predicate::str::contains("data"));
        std::fs::write(&file, "{}").unwrap();
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["number", group, verb, "--file"])
            .arg(&file)
            .assert()
            .code(5)
            .stdout("");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn flow_migration_partial_failure_preserves_results_and_exits_one() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let body = json!({"migrated_flows":[{"source_name":"one","migrated_id":"444"}],"failed_flows":[{"source_name":"two","error_code":"4233041","error_message":"already exists"}]});
    Mock::given(method("POST"))
        .and(path("/v26.0/222/migrate_flows"))
        .and(query_param("source_waba_id", "333"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
        .expect(2)
        .mount(&graph)
        .await;
    let out = paloma(Some(&graph), &home)
        .args([
            "flow",
            "migrate",
            "--source-waba-id",
            "333",
            "--yes",
            "-o",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout_json(&out), body);
    assert!(String::from_utf8_lossy(&out.stderr).contains("1 Flow migration(s) failed"));
    paloma(Some(&graph), &home)
        .args(["flow", "migrate", "--source-waba-id", "333", "--yes", "-q"])
        .assert()
        .code(1)
        .stdout("444\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn compliance_validation_rejects_wrong_entity_contacts_and_bounds_before_a_call() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("compliance.json");
    let valid = json!({"entity_name":"Acme","entity_type":"PRIVATE_COMPANY","grievance_officer_details":{"name":"Officer","email":"officer@example.com"},"customer_care_details":{"email":"care@example.com"}});
    for (pointer, value) in [
        ("/entity_name", json!("A")),
        ("/entity_name", json!("a".repeat(129))),
        ("/entity_type_custom", json!("Other")),
        ("/is_registered", json!(true)),
        ("/customer_care_details/email", json!("invalid")),
        (
            "/grievance_officer_details/mobile_number",
            json!("US.12345"),
        ),
    ] {
        let mut body = valid.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        if parent.is_empty() {
            body[key] = value;
        } else {
            body.pointer_mut(parent).unwrap()[key] = value;
        }
        std::fs::write(&file, body.to_string()).unwrap();
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["number", "compliance", "set", "--file"])
            .arg(&file)
            .arg("--yes")
            .assert()
            .code(5)
            .stdout("");
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn account_users_follow_cursors_and_quiet_prints_every_id() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET")).and(path("/v26.0/222/assigned_users")).and(query_param("business","999")).and(query_param("after","start")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"id":"444"}],"paging":{"next":"https://untrusted.invalid/?access_token=LEAK_SENTINEL","cursors":{"after":"next"}}}))).expect(1).mount(&graph).await;
    Mock::given(method("GET"))
        .and(path("/v26.0/222/assigned_users"))
        .and(query_param("business", "999"))
        .and(query_param("after", "next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"id":"555"}]})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "account",
            "users",
            "--business-id",
            "999",
            "--after",
            "start",
            "--all",
            "-q",
        ])
        .assert()
        .success()
        .stdout("444\n555\n")
        .stderr("");
}
