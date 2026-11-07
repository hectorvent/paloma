//! Retry rules: reads retry transient failures, writes only a 429 that says when.

use crate::common::*;

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
        .and(path(format!("/{V}/{PHONE}/media")))
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
        .and(path(format!("/{V}/m1")))
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
        .and(path(format!("/{V}/m1")))
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
