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
        .args(["send", "template", "15551234567", "order_ready"])
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
            .args(["send", "template", "15551234567", "t", "--lang", "en"])
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
            .args(["send", "template", "15551234567", "t", "--lang", "en"])
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
            .args(["send", "template", "15551234567", "t", "--lang", "en"])
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
            .args(["send", "template", "15551234567", "t", "--lang", "en"])
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

#[test]
fn reply_to_quotes_the_message_on_every_send_type_that_allows_it() {
    let home = TempDir::new().unwrap();
    let contacts = home.path().join("contacts.json");
    std::fs::write(&contacts, r#"[{"name":{"formatted_name":"A"}}]"#).unwrap();
    let contacts = contacts.to_str().unwrap();
    let to = "15551234567";
    let cases: Vec<Vec<&str>> = vec![
        vec!["text", to, "hi"],
        vec!["template", to, "order", "--lang", "en_US"],
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
        &["template", to, "order", "--lang", "en_US"],
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
