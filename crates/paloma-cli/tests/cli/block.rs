//! `paloma block` contract tests.

use crate::common::*;

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
            .args(["block", verb, "15551234567", BSUID, "--yes", "-o", "json"])
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
