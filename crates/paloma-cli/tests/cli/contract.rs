//! Cross-cutting contracts: exit codes, dry-run, usage errors and paging failures.

use crate::common::*;

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

#[test]
fn destructive_actions_refuse_without_tty_or_yes() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(["number", "deregister"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("pass --yes"));
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
async fn new_disruptive_commands_refuse_without_confirmation_and_preview_offline() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for args in [
        vec!["number", "verification", "request", "--method", "VOICE"],
        vec!["number", "name", "set", "Acme"],
        vec!["number", "username", "set", "acme_shop"],
        vec!["number", "username", "delete"],
        vec!["number", "contacts", "delete", "US.123"],
        vec!["number", "settings", "set", "--no-storage"],
        vec!["number", "settings", "set", "--identity-check"],
        vec!["number", "visibility", "set", "VISIBLE"],
        vec!["number", "pin", "set"],
        vec!["calling", "disable"],
        vec!["calling", "sip", "disable"],
        vec!["calling", "voicemail", "disable"],
        vec!["calling", "codecs", "clear"],
        vec!["block", "add", "15551234567"],
        vec!["commerce", "set", "--no-cart"],
        vec!["flow", "migrate", "--source-waba-id", "333"],
    ] {
        paloma(Some(&graph), &home)
            .env("PALOMA_PIN", "825194")
            .args(&args)
            .assert()
            .failure()
            .stdout("")
            .stderr(predicate::str::contains("--yes"));
        paloma(Some(&graph), &home)
            .env("PALOMA_PIN", "825194")
            .env_remove("PALOMA_TOKEN")
            .args(&args)
            .args(["--dry-run", "-o", "json"])
            .assert()
            .success()
            .stdout(predicate::str::contains("method"));
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

/// Every list prints one row id per line under `-q`, so its output can feed another
/// command; a blocked user without a shared number is listed by BSUID.
#[tokio::test(flavor = "multi_thread")]
async fn lists_print_one_row_id_per_line_under_quiet() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (args, node, body, expected) in [
        (
            vec!["template", "list"],
            format!("/v26.0/{WABA}/message_templates"),
            json!({"data":[{"id":"7","name":"a"},{"id":"8","name":"b"}]}),
            "7\n8\n",
        ),
        (
            vec!["flow", "list"],
            format!("/v26.0/{WABA}/flows"),
            json!({"data":[{"id":"9","name":"f"}]}),
            "9\n",
        ),
        (
            vec!["block", "list"],
            format!("/v26.0/{PHONE}/block_users"),
            json!({"data":[{"wa_id":"15551234567"},{"user_id":"US.13491208655302741918"}]}),
            "15551234567\nUS.13491208655302741918\n",
        ),
    ] {
        Mock::given(method("GET"))
            .and(path(node))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args(&args)
            .arg("-q")
            .assert()
            .success()
            .stdout(expected);
    }
}
