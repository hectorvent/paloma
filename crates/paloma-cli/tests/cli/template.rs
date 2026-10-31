//! `paloma template` contract tests.

use crate::common::*;

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
        .args(["template", "get", "promo", "--status", "-o", "json"])
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
    for extra in [None, Some("--status")] {
        paloma(Some(&graph), &home)
            .args(["template", "get", "promo"])
            .args(extra)
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
        .args(["template", "get", "promo", "--status"])
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
        .args(["template", "delete", "--id", "1387,1304", "--yes", "-q"])
        .assert()
        .success()
        .stdout("1387\n1304\n");
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
        // Meta's rule: hsm_ids can't be combined with a name, so a name takes one id.
        (vec!["promo", "--id", "1", "--id", "2"], 5),
        (vec!["--id", "1", "--language", "es"], 2),
        (vec!["promo", "--id", "1", "--language", "es"], 2),
        (vec![], 2),
        (vec!["--id", too_many.as_str()], 5),
        (vec!["--id", "1,abc"], 5),
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
            vec!["--id", "1387,1304"],
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
            "--id",
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
        .args(["--allow-category-change", "--ttl", "1h", "-q"])
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
        .args(["--header-handle", "4::aW", "--ttl", "12h"])
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
        .args(["--language", "en", "--body", "Hi", "--ttl", "30d"])
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
        (vec!["--ttl", "43201s"], "must be 30s to 43200s"),
        (vec!["--ttl", "0s"], "must be 30s to 43200s"),
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
            vec!["--ttl", "2m"],
            "43200s to 2592000s",
        ),
        (
            "t",
            "MARKETING",
            "Hi",
            vec!["--ttl", "1s"],
            "43200s to 2592000s",
        ),
        (
            "t",
            "AUTHENTICATION",
            "Hi",
            vec!["--ttl", "901s"],
            "30s to 900s",
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
        .args(["template", "update", "promo", "--yes", "--file"])
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
        .args(["template", "update", "promo", "-q", "--yes", "--file"])
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
        .args(["template", "update", "promo", "--yes", "--file"])
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
            "update",
            "promo",
            "--language",
            "es",
            "-q",
            "--yes",
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
        vec!["template", "delete", "--id", "5,5", "--yes"],
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
        .args(["template", "archive", "--id", "1,2"])
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("--yes"));
    paloma(Some(&graph), &home)
        .args(["template", "archive", "--id", "1,2", "--yes", "-o", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("archived_templates"));
    let dry = dry_run_request(&home, &["template", "archive", "--id", "1"]);
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
        .args(["template", "unarchive", "--id", "1,2", "-o", "json"])
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
                .args(["template", verb, "--id", &ids, "--yes"])
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
        "template",
        "migrate",
        "--source-waba-id",
        "333",
        "--id",
        "1,2",
        "--page",
        "0",
        "--count",
        "200",
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
    let dry = dry_run_request(&home, &["template", "migrate", "--source-waba-id", "333"]);
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
        vec!["--source-waba-id", "abc"],
        vec!["--source-waba-id", "333", "--count", "501"],
        vec!["--source-waba-id", "333", "--id", &too_many.join(",")],
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
        cmd.args(["template", "upsert", "--yes", "--file"])
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
        "--template",
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
        vec!["--name", &long_name, "--template", "1"],
        vec![
            "--name",
            "n",
            "--description",
            &long_description,
            "--template",
            "1",
        ],
        vec!["--name", "n", "--template", "abc"],
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
    group(&["update", "90", "--add", "1,2", "--remove", "3", "-q"])
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
            "--id",
            "14,26",
            "--start",
            "1718064000",
            "--end",
            "1718122745",
            "--metric",
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
            "--group",
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
        vec!["--id", &eleven],
        vec!["--group", &eleven],
        vec!["--group", "abc"],
        vec!["--id", "abc"],
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
        .args(["template", "analytics", "--id", "1", "--waba-timezone"])
        .args(window)
        .assert()
        .code(5);
    paloma(None, &home)
        .args([
            "template",
            "analytics",
            "--id",
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
            "--group",
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
            "--id",
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
