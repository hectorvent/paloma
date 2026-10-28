//! `paloma webhook` contract tests.

use crate::common::*;

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

// ---- webhook receiving contracts ----
const VERIFY: &str = "VERIFY_SENTINEL";

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

fn subscriptions_at(version: &str) -> Value {
    json!({"data":[
        {"object":"page","callback_url":"https://example.com/bot","active":true,
         "fields":[{"name":"feed","version":"v21.0"}]},
        {"object":"whatsapp_business_account","callback_url":"https://example.com/wa","active":true,
         "fields":[{"name":"messages","version":version},{"name":"calls","version":version}]},
    ]})
}

fn webhook_version_cmd(graph: &MockServer, home: &TempDir) -> Command {
    let mut cmd = paloma(Some(graph), home);
    cmd.env("PALOMA_APP_ID", "999")
        .env("PALOMA_APP_SECRET", "APP_SECRET_SENTINEL")
        .env("PALOMA_API_VERSION", "v21.0");
    cmd
}

async fn mock_subscriptions(graph: &MockServer, version: &str) {
    Mock::given(method("GET"))
        .and(path("/v21.0/999/subscriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(subscriptions_at(version)))
        .mount(graph)
        .await;
}

#[tokio::test]
async fn webhook_version_reports_only_the_whatsapp_subscription() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    mock_subscriptions(&graph, "v25.0").await;
    let out = webhook_version_cmd(&graph, &home)
        .args(["webhook", "version", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        stdout_json(&out),
        json!({"object":"whatsapp_business_account","version":"v25.0","fields":[
            {"name":"messages","version":"v25.0"},{"name":"calls","version":"v25.0"},
        ]})
    );
    webhook_version_cmd(&graph, &home)
        .args(["webhook", "version", "-q"])
        .assert()
        .success()
        .stdout("v25.0\n");
}

#[tokio::test]
async fn webhook_version_set_writes_nothing_when_already_there() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    mock_subscriptions(&graph, "v26.0").await;
    webhook_version_cmd(&graph, &home)
        .args(["webhook", "version", "set", "v26.0"])
        .assert()
        .success()
        .stderr(predicate::str::contains("already"));
    assert_eq!(
        subscription_writes(&graph.received_requests().await.unwrap()),
        0
    );
}

#[tokio::test]
async fn webhook_version_set_rejects_bad_input_before_any_call() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    webhook_version_cmd(&graph, &home)
        .args(["webhook", "version", "set", "26", "--yes"])
        .assert()
        .code(5)
        .stdout("");
    webhook_version_cmd(&graph, &home)
        .args([
            "webhook",
            "version",
            "set",
            "v26.0",
            "--yes",
            "--verify-token-stdin",
        ])
        .write_stdin("t")
        .assert()
        .failure();
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn webhook_version_set_refuses_without_confirmation() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    mock_subscriptions(&graph, "v21.0").await;
    webhook_version_cmd(&graph, &home)
        .args(["webhook", "version", "set", "v26.0"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("--yes"));
    assert_eq!(
        subscription_writes(&graph.received_requests().await.unwrap()),
        0
    );
}

#[tokio::test]
async fn webhook_version_set_dry_run_targets_the_new_version_without_writing() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    mock_subscriptions(&graph, "v21.0").await;
    let out = webhook_version_cmd(&graph, &home)
        .args([
            "webhook",
            "version",
            "set",
            "v26.0",
            "--dry-run",
            "-o",
            "json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let url = stdout_json(&out)["url"].as_str().unwrap().to_owned();
    assert!(url.contains("/v26.0/999/subscriptions"), "{url}");
    assert!(url.contains("object=whatsapp_business_account"), "{url}");
    assert_eq!(
        subscription_writes(&graph.received_requests().await.unwrap()),
        0
    );
}

#[tokio::test]
async fn webhook_version_set_resubmits_fields_through_the_target_version() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path("/v21.0/999/subscriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(subscriptions_at("v21.0")))
        .up_to_n_times(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path("/v26.0/999/subscriptions"))
        .and(query_param("object", "whatsapp_business_account"))
        .and(query_param("fields", "messages,calls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    mock_subscriptions(&graph, "v26.0").await;
    let out = webhook_version_cmd(&graph, &home)
        .args(["webhook", "version", "set", "v26.0", "--yes", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(stdout_json(&out)["version"], "v26.0");
    let writes = graph.received_requests().await.unwrap();
    let write = writes.iter().find(|r| r.method.as_str() == "POST").unwrap();
    assert!(!write.url.query().unwrap().contains("verify_token"));
    assert!(!write.url.query().unwrap().contains("callback_url"));
}

#[tokio::test]
async fn webhook_version_set_fails_when_meta_keeps_the_old_version() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    mock_subscriptions(&graph, "v21.0").await;
    Mock::given(method("POST"))
        .and(path("/v26.0/999/subscriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .mount(&graph)
        .await;
    webhook_version_cmd(&graph, &home)
        .args(["webhook", "version", "set", "v26.0", "--yes"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("still reports v21.0"));
}

#[tokio::test]
async fn webhook_version_set_reverify_resends_the_callback_with_a_token() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path("/v21.0/999/subscriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(subscriptions_at("v21.0")))
        .up_to_n_times(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path("/v26.0/999/subscriptions"))
        .and(query_param("callback_url", "https://example.com/wa"))
        .and(query_param("verify_token", "VERIFY_SENTINEL"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    mock_subscriptions(&graph, "v26.0").await;
    webhook_version_cmd(&graph, &home)
        .env("PALOMA_VERIFY_TOKEN", "VERIFY_SENTINEL")
        .args(["webhook", "version", "set", "v26.0", "--reverify", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("VERIFY_SENTINEL").not())
        .stderr(predicate::str::contains("VERIFY_SENTINEL").not());
}
