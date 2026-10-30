//! Secrets never reach argv, stdout or diagnostics, yet reach the API unchanged.

use crate::common::*;

#[test]
fn secret_dry_runs_are_redacted_in_every_output_format() {
    let home = TempDir::new().unwrap();
    for format in ["table", "json", "yaml", "raw"] {
        for args in [
            vec!["number", "register"],
            vec!["number", "set-pin"],
            vec![
                "webhook",
                "subscribe",
                "--callback-url",
                "https://example.test/hook",
            ],
            vec![
                "webhook",
                "fields",
                "set",
                "messages",
                "--callback-url",
                "https://example.test/hook",
            ],
        ] {
            let out = paloma(None, &home)
                .env("PALOMA_APP_SECRET", "APP_SECRET_SENTINEL")
                .env("PALOMA_PIN", "825194")
                .env("PALOMA_VERIFY_TOKEN", "VERIFY_SENTINEL")
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
        .env("PALOMA_PIN", "825194")
        .args(["number", "set-pin", "-vv", "-o", "raw"])
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
        .env("PALOMA_VERIFY_TOKEN", "VERIFY_SENTINEL")
        .args([
            "webhook",
            "fields",
            "set",
            "messages",
            "--callback-url",
            "https://example.test/hook",
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

/// A secret typed on the command line would be visible to other processes, so those
/// flags don't exist, and the usage error doesn't echo the value.
#[test]
fn secret_value_flags_are_usage_errors_that_do_not_echo_the_value() {
    let home = TempDir::new().unwrap();
    for (args, value) in [
        (vec!["number", "set-pin", "--pin", "825194"], "825194"),
        (
            vec![
                "webhook",
                "subscribe",
                "--callback-url",
                "https://example.test/hook",
                "--verify-token",
                "VERIFY_SENTINEL",
            ],
            "VERIFY_SENTINEL",
        ),
    ] {
        let out = paloma(None, &home)
            .args(args)
            .args(["--dry-run"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!stderr.contains(value), "{stderr}");
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
                "--verify-token-stdin",
                "--app-id",
                "3",
            ],
            Some("t"),
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

#[tokio::test]
async fn verification_code_is_redacted_from_every_dry_run_format() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for format in ["json", "yaml", "table", "raw"] {
        paloma(Some(&graph), &home)
            .env("PALOMA_VERIFICATION_CODE", "987654")
            .args(["number", "verify-code", "--dry-run", "-o", format])
            .assert()
            .success()
            .stdout(predicate::str::contains("987654").not())
            .stderr(predicate::str::contains("987654").not());
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}
