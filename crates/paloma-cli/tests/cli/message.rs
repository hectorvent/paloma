//! `paloma message` contract tests.

use crate::common::*;

const HISTORY_WAMID: &str = "wamid.HBgLMTY0NzM3Nzc3NzUV+AgAR/GBI5QTNEQ==";

const HISTORY_FIELDS: &str = "cursor,node{id,delivery_status,error_description,occurrence_timestamp,status_timestamp,application,webhook_update_state,webhook_uri}";

async fn mount_history_lookup(graph: &MockServer) {
    Mock::given(method("GET"))
        .and(path(format!("/{V}/{PHONE}/message_history")))
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
        .and(path(format!("/{V}/777/events")))
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
        .and(path(format!("/{V}/777/events")))
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
        .and(path(format!("/{V}/{PHONE}/message_history")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "777", "message_id": HISTORY_WAMID }],
        })))
        .expect(2)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{V}/777/events")))
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
            .and(path(format!("/{V}/{PHONE}/message_history")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": data })))
            .expect(1)
            .mount(&graph)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/{V}/1/events")))
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
        .and(path(format!("/{V}/777/events")))
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

#[tokio::test(flavor = "multi_thread")]
async fn message_history_with_no_entry_says_a_delivered_message_can_have_none() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/{V}/{PHONE}/message_history")))
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
