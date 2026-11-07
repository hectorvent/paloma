//! `paloma send interactive`: each interactive type's exact body and limits.

use crate::common::*;

fn dry_run_body(home: &TempDir, args: &[&str]) -> Value {
    let out = paloma(None, home)
        .args(["send", "interactive"])
        .args(args)
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout_json(&out)["body"]["interactive"].clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn interactive_button_sends_a_media_id_header() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": TO,
            "type": "interactive",
            "interactive": {
                "type": "button",
                "header": { "type": "image", "image": { "id": "2762" } },
                "body": { "text": "Pick" },
                "action": { "buttons": [
                    { "type": "reply", "reply": { "id": "y", "title": "Yes" } }
                ] }
            }
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.R"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["send", "interactive", "button", TO, "--body", "Pick"])
        .args(["--button", "y:Yes", "--header-image", "2762", "-q"])
        .assert()
        .success()
        .stdout("wamid.R\n");
}

#[test]
fn interactive_button_takes_each_media_kind_by_id_or_link() {
    let home = TempDir::new().unwrap();
    let base = ["button", TO, "--body", "b", "--button", "y:Yes"];
    for (flag, kind) in [
        ("--header-image", "image"),
        ("--header-video", "video"),
        ("--header-document", "document"),
    ] {
        let mut args = base.to_vec();
        args.extend([flag, "https://x.test/f"]);
        assert_eq!(
            dry_run_body(&home, &args)["header"],
            json!({ "type": kind, kind: { "link": "https://x.test/f" } })
        );
        let mut args = base.to_vec();
        args.extend([flag, "99"]);
        assert_eq!(
            dry_run_body(&home, &args)["header"],
            json!({ "type": kind, kind: { "id": "99" } })
        );
    }
}

#[test]
fn interactive_cta_url_takes_a_media_link_and_refuses_an_id() {
    let home = TempDir::new().unwrap();
    let base = [
        "cta-url",
        TO,
        "--body",
        "b",
        "--display",
        "Open",
        "--url",
        "https://x.test",
    ];
    let mut args = base.to_vec();
    args.extend(["--header-video", "https://x.test/v.mp4"]);
    assert_eq!(
        dry_run_body(&home, &args)["header"],
        json!({ "type": "video", "video": { "link": "https://x.test/v.mp4" } })
    );
    paloma(None, &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["send", "interactive"])
        .args(base)
        .args(["--header-image", "99"])
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("not a media id"));
}

#[test]
fn interactive_media_header_flags_are_exclusive_and_list_has_none() {
    let home = TempDir::new().unwrap();
    let button = ["send", "interactive", "button", TO, "--body", "b"];
    for extra in [
        ["--header", "Text", "--header-image", "1"],
        ["--header-video", "2", "--header-image", "1"],
    ] {
        paloma(None, &home)
            .args(button)
            .args(["--button", "y:Yes"])
            .args(extra)
            .assert()
            .code(2);
    }
    paloma(None, &home)
        .args(["send", "interactive", "list", TO, "--body", "b"])
        .args(["--button", "Open", "--section", "S:a:A"])
        .args(["--header-image", "1"])
        .assert()
        .code(2);
}

#[test]
fn interactive_media_header_refuses_a_bad_reference_without_echoing_it() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["send", "interactive", "button", TO, "--body", "b"])
        .args(["--button", "y:Yes", "--header-image", "ftp://x/secret"])
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty())
        .stderr(
            predicate::str::contains("--header-image")
                .and(predicate::str::contains("secret").not()),
        );
}

// List row descriptions and limits.

fn list_args(section: &str) -> Vec<&str> {
    vec![
        "list",
        TO,
        "--body",
        "b",
        "--button",
        "Open",
        "--section",
        section,
    ]
}

#[test]
fn list_row_description_is_sent_and_the_old_syntax_still_works() {
    let home = TempDir::new().unwrap();
    let section = r"Ship:fast:Express::Next day\, by 5: pm,slow:Time: 10:30";
    assert_eq!(
        dry_run_body(&home, &list_args(section))["action"]["sections"],
        json!([{ "title": "Ship", "rows": [
            { "id": "fast", "title": "Express", "description": "Next day, by 5: pm" },
            { "id": "slow", "title": "Time: 10:30" }
        ] }])
    );
}

#[test]
fn list_row_limits_are_exact_and_checked_before_context() {
    let home = TempDir::new().unwrap();
    let cases = [
        ("a".repeat(200), "T".to_owned(), "D".to_owned(), None),
        (
            "a".repeat(201),
            "T".to_owned(),
            "D".to_owned(),
            Some("row id"),
        ),
        ("a".to_owned(), "T".repeat(24), "D".to_owned(), None),
        (
            "a".to_owned(),
            "T".repeat(25),
            "D".to_owned(),
            Some("row title"),
        ),
        ("a".to_owned(), "T".to_owned(), "D".repeat(72), None),
        (
            "a".to_owned(),
            "T".to_owned(),
            "D".repeat(73),
            Some("row description"),
        ),
        (
            " ".to_owned(),
            "T".to_owned(),
            "D".to_owned(),
            Some("row id"),
        ),
        (
            "a".to_owned(),
            " ".to_owned(),
            "D".to_owned(),
            Some("row title"),
        ),
        (
            "a".to_owned(),
            "T".to_owned(),
            " ".to_owned(),
            Some("row description"),
        ),
    ];
    for (id, title, description, problem) in cases {
        let section = format!("S:{id}:{title}::{description}");
        let mut cmd = paloma(None, &home);
        cmd.args(["send", "interactive"]).args(list_args(&section));
        match problem {
            None => {
                cmd.arg("--dry-run").assert().success();
            }
            Some(problem) => {
                cmd.env_remove("PALOMA_PHONE_NUMBER_ID")
                    .assert()
                    .code(5)
                    .stdout(predicate::str::is_empty())
                    .stderr(predicate::str::contains(problem));
            }
        }
    }
}

#[test]
fn send_interactive_carousel_refuses_a_bad_card_type_url_or_link() {
    let home = TempDir::new().unwrap();
    let card = |kind: &str, link: &str, url: &str| {
        json!({
            "card_index": 0,
            "type": kind,
            "header": { "type": "image", "image": { "link": link } },
            "action": {
                "name": "cta_url",
                "parameters": { "display_text": "Open", "url": url }
            }
        })
    };
    let good = "https://x.test/a.png";
    let bad = [
        card("bogus", good, "https://x.test"),
        card("cta_url", &format!(" {good}"), "https://x.test"),
        card("cta_url", good, "not a url"),
    ];
    for bad_card in bad {
        let file = home.path().join("cards.json");
        std::fs::write(
            &file,
            json!([bad_card, card("cta_url", good, "https://x.test")]).to_string(),
        )
        .unwrap();
        paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args([
                "send",
                "interactive",
                "carousel",
                "15551234567",
                "--body",
                "b",
            ])
            .arg("--card-file")
            .arg(&file)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty());
    }
}

#[tokio::test]
async fn location_request_sends_the_send_location_action() {
    let graph = MockServer::start().await;
    let mut body = interactive_message(&json!({
        "type": "location_request_message",
        "body": { "text": "Where to?" },
        "action": { "name": "send_location" }
    }));
    body["context"] = json!({ "message_id": "wamid.Q" });
    expect_exact_send(&graph, body).await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "interactive",
            "location-request",
            TO,
            "--body",
            "Where to?",
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[test]
fn location_request_checks_its_body_and_allows_no_header() {
    let long = "x".repeat(1025);
    let fine = "x".repeat(1024);
    let cmd = ["send", "interactive", "location-request", TO, "--body"];
    let with = |body: &'static str| {
        let mut args = cmd.to_vec();
        args.push(body);
        args
    };
    assert_refused_locally(&with(" "));
    let mut too_long = cmd.to_vec();
    too_long.push(&long);
    assert_refused_locally(&too_long);
    let mut at_limit = cmd.to_vec();
    at_limit.push(&fine);
    sent_dry_run_body(&at_limit);
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(with("Where?"))
        .args(["--header", "H"])
        .assert()
        .code(2);
}

#[tokio::test]
async fn address_sends_a_country_with_optional_text() {
    let graph = MockServer::start().await;
    expect_exact_send(
        &graph,
        interactive_message(&json!({
            "type": "address_message",
            "header": { "type": "text", "text": "Delivery" },
            "body": { "text": "Where should it go?" },
            "footer": { "text": "Acme" },
            "action": { "name": "address_message", "parameters": { "country": "IN" } }
        })),
    )
    .await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "interactive",
            "address",
            TO,
            "--body",
            "Where should it go?",
            "--header",
            "Delivery",
            "--footer",
            "Acme",
            "--country",
            "IN",
        ],
    );
}

#[test]
fn address_doesnt_block_other_regions_and_takes_a_parameters_file() {
    let body = sent_dry_run_body(&[
        "send",
        "interactive",
        "address",
        TO,
        "--body",
        "Where?",
        "--country",
        "SG",
    ]);
    assert_eq!(
        body["interactive"]["action"]["parameters"],
        json!({ "country": "SG" })
    );
    let home = TempDir::new().unwrap();
    let file = home.path().join("address.json");
    let parameters = json!({
        "country": "IN",
        "values": { "name": "Ana", "in_pin_code": "666666" },
        "validation_errors": { "in_pin_code": "We could not locate this pin code." },
        "saved_addresses": [{ "id": "a1", "value": { "city": "Mumbai" } }],
    });
    std::fs::write(&file, parameters.to_string()).unwrap();
    let body = sent_dry_run_body(&[
        "send",
        "interactive",
        "address",
        TO,
        "--body",
        "Where?",
        "--parameters-file",
        file.to_str().unwrap(),
    ]);
    assert_eq!(body["interactive"]["action"]["parameters"], parameters);
}

#[test]
fn address_refuses_bad_parameters_and_needs_a_country() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("address.json");
    let file_str = file.to_str().unwrap();
    for bad in [
        json!([]),
        json!({}),
        json!({ "country": " " }),
        json!({ "country": "IN", "values": [] }),
        json!({ "country": "IN", "saved_addresses": {} }),
        json!({ "country": "IN", "validation_errors": "x" }),
    ] {
        std::fs::write(&file, bad.to_string()).unwrap();
        assert_refused_locally(&[
            "send",
            "interactive",
            "address",
            TO,
            "--body",
            "Where?",
            "--parameters-file",
            file_str,
        ]);
    }
    assert_refused_locally(&[
        "send",
        "interactive",
        "address",
        TO,
        "--body",
        "Where?",
        "--country",
        " ",
    ]);
    paloma(None, &home)
        .args(["send", "interactive", "address", TO, "--body", "Where?"])
        .assert()
        .code(2);
}

fn url_card(name: &str) -> Value {
    json!({
        "header": { "type": "image", "image": { "link": format!("https://x.test/{name}.png") } },
        "body": { "text": name },
        "action": {
            "name": "cta_url",
            "parameters": { "display_text": "Buy", "url": format!("https://x.test/{name}") }
        }
    })
}

#[tokio::test]
async fn carousel_card_file_fills_index_and_type_and_keeps_the_rest() {
    let graph = MockServer::start().await;
    let filled = |index: usize, name: &str| {
        let mut card = url_card(name);
        card["card_index"] = index.into();
        card["type"] = "cta_url".into();
        card
    };
    let mut body = interactive_message(&json!({
        "type": "carousel",
        "body": { "text": "New in" },
        "action": { "cards": [filled(0, "a"), filled(1, "b")] }
    }));
    body["context"] = json!({ "message_id": "wamid.Q" });
    expect_exact_send(&graph, body).await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("cards.json");
    std::fs::write(
        &file,
        json!({ "cards": [url_card("a"), url_card("b")] }).to_string(),
    )
    .unwrap();
    assert_quiet_send(
        &graph,
        &[
            "send",
            "interactive",
            "carousel",
            TO,
            "--body",
            "New in",
            "--card-file",
            file.to_str().unwrap(),
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[test]
fn carousel_card_file_keeps_quick_replies_and_a_bare_array() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("cards.json");
    let replies = |name: &str| {
        json!({
            "card_index": 0,
            "type": "cta_url",
            "header": { "type": "video", "video": { "link": "https://x.test/v.mp4" } },
            "action": { "buttons": [
                { "type": "quick_reply", "quick_reply": { "id": name, "title": "More" } }
            ] }
        })
    };
    let mut second = replies("b");
    second["card_index"] = 1.into();
    let cards = json!([replies("a"), second]);
    std::fs::write(&file, cards.to_string()).unwrap();
    let body = sent_dry_run_body(&[
        "send",
        "interactive",
        "carousel",
        TO,
        "--body",
        "New in",
        "--card-file",
        file.to_str().unwrap(),
    ]);
    assert_eq!(body["interactive"]["action"]["cards"], cards);
}

#[test]
fn carousel_card_file_refuses_what_meta_would_without_echoing_it() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("cards.json");
    let file_str = file.to_str().unwrap().to_owned();
    let refused = |cards: &Value| {
        std::fs::write(&file, cards.to_string()).unwrap();
        let out = paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .env_remove("PALOMA_TOKEN")
            .args(["send", "interactive", "carousel", TO, "--body", "B"])
            .args(["--card-file", file_str.as_str()])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(5), "{cards}");
        assert!(out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("SENTINEL"));
    };
    refused(&json!({ "other": [] }));
    refused(&json!([url_card("a")]));
    refused(&Value::Array(vec![url_card("a"); 11]));
    let mut no_header = url_card("a");
    no_header.as_object_mut().unwrap().remove("header");
    refused(&json!([no_header, url_card("b")]));
    let mut long_body = url_card("a");
    long_body["body"]["text"] = format!("SENTINEL{}", "x".repeat(160)).into();
    refused(&json!([long_body, url_card("b")]));
    let mut product = url_card("a");
    product["type"] = "product".into();
    refused(&json!([product, url_card("b")]));
    let mut replies = url_card("a");
    replies["action"] = json!({ "buttons": [
        { "type": "quick_reply", "quick_reply": { "id": "x", "title": "More" } }
    ] });
    refused(&json!([replies, url_card("b")]));
}

#[tokio::test]
async fn product_carousel_makes_one_card_per_product() {
    let graph = MockServer::start().await;
    let card = |index: usize, sku: &str| {
        json!({
            "card_index": index,
            "type": "product",
            "action": { "product_retailer_id": sku, "catalog_id": "CAT" }
        })
    };
    expect_exact_send(
        &graph,
        interactive_message(&json!({
            "type": "carousel",
            "body": { "text": "Featured" },
            "action": { "cards": [card(0, "abc"), card(1, "def")] }
        })),
    )
    .await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "interactive",
            "carousel",
            TO,
            "--body",
            "Featured",
            "--products",
            "abc,def",
            "--catalog-id",
            "CAT",
        ],
    );
}

#[test]
fn product_carousel_checks_its_count_and_its_flags() {
    let cmd = ["send", "interactive", "carousel", TO, "--body", "B"];
    let many = ["s"; 11].join(",");
    for products in ["a", many.as_str(), "a,,b"] {
        let mut args = cmd.to_vec();
        args.extend(["--products", products, "--catalog-id", "CAT"]);
        assert_refused_locally(&args);
    }
    let mut blank_catalog = cmd.to_vec();
    blank_catalog.extend(["--products", "a,b", "--catalog-id", " "]);
    assert_refused_locally(&blank_catalog);
    let home = TempDir::new().unwrap();
    for extra in [
        vec![],
        vec!["--products", "a,b"],
        vec!["--catalog-id", "CAT"],
        vec!["--products", "a,b", "--catalog-id", "C", "--card-file", "x"],
        vec!["--products", "a,b", "--catalog-id", "C", "--footer", "F"],
    ] {
        paloma(None, &home).args(cmd).args(extra).assert().code(2);
    }
}

#[tokio::test]
async fn call_button_sends_its_parameters() {
    let graph = MockServer::start().await;
    let mut body = interactive_message(&json!({
        "type": "voice_call",
        "body": { "text": "Call us" },
        "action": {
            "name": "voice_call",
            "parameters": {
                "display_text": "Call on WhatsApp",
                "ttl_minutes": 100,
                "payload": "campaign-7"
            }
        }
    }));
    body["context"] = json!({ "message_id": "wamid.Q" });
    expect_exact_send(&graph, body).await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "interactive",
            "call",
            TO,
            "--body",
            "Call us",
            "--display",
            "Call on WhatsApp",
            "--ttl",
            "100m",
            "--payload",
            "campaign-7",
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[test]
fn call_button_leaves_parameters_out_and_checks_its_limits() {
    let cmd = ["send", "interactive", "call", TO, "--body", "Call"];
    assert_eq!(
        sent_dry_run_body(&cmd)["interactive"]["action"],
        json!({ "name": "voice_call" })
    );
    let long_label = "x".repeat(21);
    let long_payload = "x".repeat(513);
    for extra in [
        vec!["--ttl", "0s"],
        vec!["--ttl", "43201m"],
        vec!["--display", long_label.as_str()],
        vec!["--display", " "],
        vec!["--payload", long_payload.as_str()],
    ] {
        let mut args = cmd.to_vec();
        args.extend(extra);
        assert_refused_locally(&args);
    }
    let mut at_limit = cmd.to_vec();
    at_limit.extend(["--ttl", "30d"]);
    assert_eq!(
        sent_dry_run_body(&at_limit)["interactive"]["action"]["parameters"]["ttl_minutes"],
        43200
    );
}

#[tokio::test]
async fn request_contact_sends_the_request_contact_info_action() {
    let graph = MockServer::start().await;
    let mut body = interactive_message(&json!({
        "type": "request_contact_info",
        "body": { "text": "Share your number?" },
        "action": { "name": "request_contact_info" }
    }));
    body["context"] = json!({ "message_id": "wamid.Q" });
    expect_exact_send(&graph, body).await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "interactive",
            "request-contact",
            TO,
            "--body",
            "Share your number?",
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[test]
fn request_contact_goes_to_a_bsuid_and_refuses_a_blank_body() {
    let body = sent_dry_run_body(&[
        "send",
        "interactive",
        "request-contact",
        BSUID,
        "--body",
        "Share?",
    ]);
    assert_eq!(body["recipient"], BSUID);
    assert!(body.get("to").is_none());
    assert_refused_locally(&["send", "interactive", "request-contact", TO, "--body", " "]);
}

#[tokio::test]
async fn call_permission_sends_a_body_to_a_bsuid() {
    let graph = MockServer::start().await;
    expect_exact_send(
        &graph,
        json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "recipient": BSUID,
            "type": "interactive",
            "context": { "message_id": "wamid.Q" },
            "interactive": {
                "type": "call_permission_request",
                "body": { "text": "May we call about your order?" },
                "action": { "name": "call_permission_request" }
            }
        }),
    )
    .await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "call-permission",
            BSUID,
            "--body",
            "May we call about your order?",
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[test]
fn call_permission_body_is_optional_and_it_has_no_header_or_footer() {
    assert_eq!(
        sent_dry_run_body(&["send", "call-permission", TO])["interactive"],
        json!({
            "type": "call_permission_request",
            "action": { "name": "call_permission_request" }
        })
    );
    assert_refused_locally(&["send", "call-permission", TO, "--body", " "]);
    let home = TempDir::new().unwrap();
    for flag in ["--header", "--footer"] {
        paloma(None, &home)
            .args(["send", "call-permission", TO, flag, "x"])
            .assert()
            .code(2);
    }
}

#[tokio::test]
async fn product_sends_the_catalog_and_retailer_ids() {
    let graph = MockServer::start().await;
    let mut body = interactive_message(&json!({
        "type": "product",
        "body": { "text": "This one" },
        "footer": { "text": "Acme" },
        "action": { "catalog_id": "CAT", "product_retailer_id": "SKU1" }
    }));
    body["context"] = json!({ "message_id": "wamid.Q" });
    expect_exact_send(&graph, body).await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "product",
            TO,
            "--catalog-id",
            "CAT",
            "--product",
            "SKU1",
            "--body",
            "This one",
            "--footer",
            "Acme",
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[test]
fn product_body_is_optional_and_a_header_isnt_allowed() {
    let cmd = [
        "send",
        "product",
        TO,
        "--catalog-id",
        "CAT",
        "--product",
        "SKU1",
    ];
    assert_eq!(
        sent_dry_run_body(&cmd)["interactive"],
        json!({
            "type": "product",
            "action": { "catalog_id": "CAT", "product_retailer_id": "SKU1" }
        })
    );
    let long_footer = "x".repeat(61);
    for extra in [vec!["--body", " "], vec!["--footer", long_footer.as_str()]] {
        let mut args = cmd.to_vec();
        args.extend(extra);
        assert_refused_locally(&args);
    }
    assert_refused_locally(&["send", "product", TO, "--catalog-id", " ", "--product", "S"]);
    assert_refused_locally(&["send", "product", TO, "--catalog-id", "C", "--product", " "]);
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(cmd)
        .args(["--header", "H"])
        .assert()
        .code(2);
}

#[tokio::test]
async fn product_list_sends_its_sections() {
    let graph = MockServer::start().await;
    let mut body = interactive_message(&json!({
        "type": "product_list",
        "header": { "type": "text", "text": "Our picks" },
        "body": { "text": "Have a look" },
        "footer": { "text": "Acme" },
        "action": {
            "catalog_id": "CAT",
            "sections": [
                { "title": "Cakes", "product_items": [
                    { "product_retailer_id": "c1" },
                    { "product_retailer_id": "c,2" }
                ] },
                { "title": "Drinks", "product_items": [
                    { "product_retailer_id": "d1" }
                ] }
            ]
        }
    }));
    body["context"] = json!({ "message_id": "wamid.Q" });
    expect_exact_send(&graph, body).await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "product-list",
            TO,
            "--catalog-id",
            "CAT",
            "--header",
            "Our picks",
            "--body",
            "Have a look",
            "--footer",
            "Acme",
            "--section",
            r"Cakes:c1,c\,2",
            "--section",
            "Drinks:d1",
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[test]
fn product_list_checks_its_limits_and_requires_a_header() {
    let cmd = [
        "send",
        "product-list",
        TO,
        "--catalog-id",
        "CAT",
        "--header",
        "H",
        "--body",
        "B",
    ];
    let mut valid = cmd.to_vec();
    valid.extend(["--section", "A:s"]);
    assert_eq!(
        sent_dry_run_body(&valid)["interactive"]["action"]["sections"],
        json!([{ "title": "A", "product_items": [{ "product_retailer_id": "s" }] }])
    );
    let thirty_one = format!("A:{}", ["s"; 31].join(","));
    let long_title = format!("{}:s", "x".repeat(25));
    for section in [
        "NoColon",
        "Empty:",
        ":s",
        thirty_one.as_str(),
        long_title.as_str(),
    ] {
        let mut args = cmd.to_vec();
        args.extend(["--section", section]);
        assert_refused_locally(&args);
    }
    let mut eleven = cmd.to_vec();
    for _ in 0..11 {
        eleven.extend(["--section", "A:s"]);
    }
    assert_refused_locally(&eleven);
    let long_header = "x".repeat(61);
    assert_refused_locally(&[
        "send",
        "product-list",
        TO,
        "--catalog-id",
        "CAT",
        "--header",
        &long_header,
        "--body",
        "B",
        "--section",
        "A:s",
    ]);
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(["send", "product-list", TO, "--catalog-id", "C"])
        .args(["--body", "B", "--section", "A:s"])
        .assert()
        .code(2);
}

#[tokio::test]
async fn catalog_sends_a_thumbnail_and_footer() {
    let graph = MockServer::start().await;
    let mut body = interactive_message(&json!({
        "type": "catalog_message",
        "body": { "text": "Browse" },
        "footer": { "text": "Acme" },
        "action": {
            "name": "catalog_message",
            "parameters": { "thumbnail_product_retailer_id": "SKU1" }
        }
    }));
    body["context"] = json!({ "message_id": "wamid.Q" });
    expect_exact_send(&graph, body).await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "catalog",
            TO,
            "--body",
            "Browse",
            "--footer",
            "Acme",
            "--thumbnail",
            "SKU1",
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[test]
fn catalog_leaves_parameters_out_and_checks_its_limits() {
    assert_eq!(
        sent_dry_run_body(&["send", "catalog", TO, "--body", "Browse"])["interactive"],
        json!({
            "type": "catalog_message",
            "body": { "text": "Browse" },
            "action": { "name": "catalog_message" }
        })
    );
    let long_body = "x".repeat(1025);
    assert_refused_locally(&["send", "catalog", TO, "--body", &long_body]);
    assert_refused_locally(&["send", "catalog", TO, "--body", " "]);
    assert_refused_locally(&["send", "catalog", TO, "--body", "B", "--thumbnail", " "]);
}
