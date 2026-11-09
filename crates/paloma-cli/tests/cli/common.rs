//! Shared fixtures: an isolated `paloma` command and the mocks several groups reuse.

pub(crate) use assert_cmd::Command;
pub(crate) use predicates::prelude::*;
pub(crate) use serde_json::{Value, json};
pub(crate) use tempfile::TempDir;
pub(crate) use wiremock::matchers::{
    body_json, body_string_contains, header, method, path, query_param,
};
pub(crate) use wiremock::{Mock, MockServer, ResponseTemplate};

pub(crate) const PHONE: &str = "111";

pub(crate) const WABA: &str = "222";

pub(crate) const TOKEN: &str = "EAAtest";

/// A `paloma` command isolated from the developer's machine: temp config, env token,
/// no colors, Graph API pointed at `graph`.
pub(crate) fn paloma(graph: Option<&MockServer>, home: &TempDir) -> Command {
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

pub(crate) fn stdout_json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// A handle upload's first step: the session id comes back with its own query string.
pub(crate) async fn mount_upload_session(graph: &MockServer) {
    Mock::given(method("POST"))
        .and(path(format!("/{V}/999/uploads")))
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

/// The query strings of everything the stub received, in order.
pub(crate) async fn received_queries(graph: &MockServer) -> Vec<String> {
    graph
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.url.query().unwrap_or_default().to_owned())
        .collect()
}

/// Two lookup pages and a last one, for `promo` held in the given languages.
pub(crate) async fn mock_three_template_pages(
    graph: &MockServer,
    page_two: Value,
    page_three: Value,
) {
    let p = format!("/{V}/{WABA}/message_templates");
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

// Interactive media headers.

pub(crate) const TO: &str = "15551234567";

// Media type and size checks.

pub(crate) const MIB: u64 = 1024 * 1024;

pub(crate) fn sized_file(home: &TempDir, name: &str, bytes: u64) -> std::path::PathBuf {
    let file = home.path().join(name);
    std::fs::File::create(&file)
        .unwrap()
        .set_len(bytes)
        .unwrap();
    file
}

pub(crate) const BSUID: &str = "US.13491208655302741918";

pub(crate) const PARENT_BSUID: &str = "US.ENT.11815799212886844830";

/// Stub the send endpoint so that only this exact body gets a wamid.
pub(crate) async fn expect_exact_send(graph: &MockServer, body: Value) {
    Mock::given(method("POST"))
        .and(path(format!("/{V}/{PHONE}/messages")))
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
pub(crate) fn interactive_message(interactive: &Value) -> Value {
    json!({
        "messaging_product": "whatsapp",
        "recipient_type": "individual",
        "to": TO,
        "type": "interactive",
        "interactive": interactive,
    })
}

/// Run a send with `-q`: stdout must be only the wamid, and stderr must be quiet.
pub(crate) fn assert_quiet_send(graph: &MockServer, args: &[&str]) {
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
pub(crate) fn assert_refused_locally(args: &[&str]) {
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
pub(crate) fn sent_dry_run_body(args: &[&str]) -> Value {
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

/// Print a command's request under `--dry-run`, with nothing sent.
pub(crate) fn dry_run_request(home: &TempDir, args: &[&str]) -> Value {
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

/// The Graph API version every mocked path uses: the binary's default, so bumping it is one
/// change in `paloma-graph`, not hundreds here.
pub(crate) const V: &str = paloma_graph::DEFAULT_API_VERSION;
/// [`V`] spelled out, for asserting whole URLs; a test below keeps the two in step.
pub(crate) const GRAPH: &str = "https://graph.facebook.com/v27.0";

#[test]
fn graph_url_uses_the_default_version() {
    assert_eq!(GRAPH, format!("https://graph.facebook.com/{V}"));
}

pub(crate) const APP_SECRET: &str = "APP_SECRET_SENTINEL";

pub(crate) const QR_CODE: &str = "WOMVT6TJ2BP7A1";

pub(crate) fn subscription_writes(requests: &[wiremock::Request]) -> usize {
    requests
        .iter()
        .filter(|r| r.method.as_str() != "GET")
        .count()
}

pub(crate) const GROUP: &str = "Y2FwaV9ncm91cDoxNzA1";
