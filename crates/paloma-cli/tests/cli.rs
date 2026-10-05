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
