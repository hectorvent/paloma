//! `paloma send template`: typed parameters, components files and category routing.

use crate::common::*;

#[tokio::test(flavor = "multi_thread")]
async fn template_components_file_preserves_payload_and_redacts_dry_runs() {
    let graph = MockServer::start().await;
    let components = json!([
        {"type":"header", "parameters":[{"type":"image", "image":{"link":"https://example.test/header.jpg?access_token=MEDIA_SENTINEL"}}]},
        {"type":"body", "parameters":[
            {"type":"text", "parameter_name":"customer_name", "text":"Ana"},
            {"type":"currency", "currency":{"fallback_value":"$10.00", "code":"USD", "amount_1000":10000}},
            {"type":"date_time", "date_time":{"fallback_value":"October 26"}}
        ]},
        {"type":"button", "sub_type":"url", "index":"0", "parameters":[{"type":"text", "text":"order-42"}]}
    ]);
    let expected = json!({"messaging_product":"whatsapp", "recipient_type":"individual", "to":"15551234567", "type":"template", "template":{
        "name":"order_ready", "language":{"code":"en_US"}, "components":components
    }});
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(&expected))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.TEMPLATE"}]})),
        )
        .expect(2)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("components.json");
    for document in [components.clone(), json!({"components":components})] {
        std::fs::write(&file, document.to_string()).unwrap();
        for format in ["json", "yaml", "table", "raw"] {
            let out = paloma(Some(&graph), &home)
                .args([
                    "send",
                    "template",
                    "--via",
                    "cloud",
                    "15551234567",
                    "order_ready",
                    "--language",
                    "en_US",
                    "--components-file",
                ])
                .arg(&file)
                .args(["--dry-run", "-o", format])
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(!stdout.contains("MEDIA_SENTINEL"));
            assert!(stdout.contains("order-42"));
            assert!(String::from_utf8_lossy(&out.stderr).contains("dry-run"));
            if format == "raw" {
                let rendered = stdout_json(&out);
                assert_eq!(
                    rendered["template"]["components"][1],
                    expected["template"]["components"][1]
                );
            }
        }
        // A real send per file shape: the mock's exact body match covers both.
        let out = paloma(Some(&graph), &home)
            .args([
                "send",
                "template",
                "--via",
                "cloud",
                "15551234567",
                "order_ready",
                "--language",
                "en_US",
                "--components-file",
            ])
            .arg(&file)
            .arg("-q")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), "wamid.TEMPLATE\n");
        assert!(out.stderr.is_empty());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn template_text_flags_accept_values_starting_with_a_hyphen() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product":"whatsapp", "recipient_type":"individual", "to":"15551234567",
            "type":"template", "template":{
                "name":"sale", "language":{"code":"en_US"}, "components":[
                    {"type":"header", "parameters":[{"type":"text", "text":"-50% today"}]},
                    {"type":"body", "parameters":[{"type":"text", "text":"-5"}]}
                ]
            }
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.SALE"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            "--via",
            "cloud",
            "15551234567",
            "sale",
            "--language",
            "en_US",
            "--header-text",
            "-50% today",
            "--body-var",
            "-5",
            "-q",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "wamid.SALE\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn template_typed_flags_send_the_exact_components_in_order() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product":"whatsapp", "recipient_type":"individual", "to":"15551234567",
            "type":"template", "template":{
                "name":"order_ready", "language":{"code":"en_US"}, "components":[
                    {"type":"header", "parameters":[{"type":"image", "image":{"id":"MEDIA1"}}]},
                    {"type":"body", "parameters":[
                        {"type":"text", "parameter_name":"first_name", "text":"Ana"}
                    ]},
                    {"type":"button", "sub_type":"url", "index":"0", "parameters":[
                        {"type":"text", "text":"a=b:c"}
                    ]},
                    {"type":"button", "sub_type":"quick_reply", "index":"1", "parameters":[
                        {"type":"payload", "payload":"STOP"}
                    ]},
                    {"type":"button", "sub_type":"copy_code", "index":2, "parameters":[
                        {"type":"coupon_code", "coupon_code":"WINTER25"}
                    ]},
                    {"type":"button", "sub_type":"flow", "index":"3", "parameters":[
                        {"type":"action", "action":{"flow_token":"FLOW_SENTINEL"}}
                    ]},
                    {"type":"button", "sub_type":"CATALOG", "index":4, "parameters":[
                        {"type":"action", "action":{"thumbnail_product_retailer_id":"SKU1"}}
                    ]}
                ]
            }
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.T"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            "--via",
            "cloud",
            "15551234567",
            "order_ready",
        ])
        .args(["--language", "en_US", "--header-image", "MEDIA1"])
        .args(["--var", "first_name=Ana"])
        .args(["--button", "0:url=a=b:c"])
        .args(["--button", "1:quick_reply=STOP"])
        .args(["--button", "2:copy_code=WINTER25"])
        .args(["--button", "3:flow=FLOW_SENTINEL"])
        .args(["--button", "4:catalog=SKU1", "-q"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "wamid.T\n");
}

#[test]
fn template_media_header_is_an_id_or_a_link_and_a_dry_run_masks_secrets() {
    let home = TempDir::new().unwrap();
    let components = |args: &[&str]| {
        let out = paloma(None, &home)
            .args([
                "send",
                "template",
                "--via",
                "cloud",
                "15551234567",
                "t",
                "--language",
                "en",
            ])
            .args(args)
            .args(["--dry-run", "-o", "raw"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        stdout_json(&out)["template"]["components"].clone()
    };
    assert_eq!(
        components(&["--header-video", "V1"]),
        json!([{"type":"header", "parameters":[{"type":"video", "video":{"id":"V1"}}]}])
    );
    assert_eq!(
        components(&["--header-document", "https://example.test/a.pdf"]),
        json!([{"type":"header", "parameters":[
            {"type":"document", "document":{"link":"https://example.test/a.pdf"}}
        ]}])
    );
    let flow = components(&["--button", "0:flow=FLOW_SENTINEL"]);
    assert!(!flow.to_string().contains("FLOW_SENTINEL"), "{flow}");
}

#[test]
fn template_header_flags_and_the_components_file_conflict() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("c.json");
    std::fs::write(&file, "[]").unwrap();
    let file = file.to_str().unwrap();
    for args in [
        vec!["--header-image", "A", "--header-video", "B"],
        vec!["--header-video", "A", "--header-document", "B"],
        vec!["--header-text", "A", "--header-image", "B"],
        vec!["--header-image", "B", "--header-text", "A"],
        vec!["--var", "a=b", "--header-text", "A"],
        vec!["--components-file", file, "--header-image", "A"],
        vec!["--components-file", file, "--button", "0:url=a"],
    ] {
        paloma(None, &home)
            .args([
                "send",
                "template",
                "--via",
                "cloud",
                "15551234567",
                "t",
                "--language",
                "en",
            ])
            .args(args)
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty());
    }
}

#[test]
fn template_bad_typed_flags_exit_5_without_echoing_the_value() {
    let home = TempDir::new().unwrap();
    let long_coupon = format!("0:copy_code={}", "A".repeat(21));
    for (args, needle) in [
        (vec!["--button", "SECRET"], "#1 must use INDEX:TYPE=VALUE"),
        (vec!["--button", "0:flowSECRET"], "#1 must use"),
        (vec!["--button", "0:flow:SECRET"], "#1 must use"),
        (vec!["--button", "0:flow:SECRET=x"], "#1 type must be"),
        (vec!["--button", "SECRET:flow=x"], "#1 index must be 0 to 9"),
        (vec!["--button", "10:url=a"], "#1 index must be 0 to 9"),
        (vec!["--button", "0:url=  "], "#1 value is empty"),
        (
            vec!["--button", "0:url=a", "--button", "0:flow=SECRET"],
            "#2 index 0 is already used",
        ),
        (
            vec!["--button", "0:copy_code=a", "--button", "1:copy_code=b"],
            "#2 is a second copy_code",
        ),
        (vec!["--button", long_coupon.as_str()], "over 20 characters"),
        (vec!["--header-image", "  "], "--header-image"),
        (vec!["--header-image", "ftp://SECRET/a.png"], "media id or"),
        (vec!["--header-video", "./SECRET.mp4"], "media id or"),
        (vec!["--header-document", "a SECRET"], "media id or"),
        (vec!["--header-image", "http://"], "media id or"),
        (vec!["--header-image", "https:///SECRET"], "media id or"),
    ] {
        paloma(None, &home)
            .args([
                "send",
                "template",
                "--via",
                "cloud",
                "15551234567",
                "t",
                "--language",
                "en",
            ])
            .args(args)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(needle).and(predicate::str::contains("SECRET").not()));
    }
}

#[test]
fn template_uppercase_link_is_a_link_and_stays_masked_everywhere() {
    let home = TempDir::new().unwrap();
    let args = [
        "send",
        "template",
        "--via",
        "cloud",
        "15551234567",
        "t",
        "--language",
        "en",
        "--header-image",
        "HTTPS://x.test/a.png?access_token=SECRET",
        "--button",
        "0:flow=SECRET",
        "--dry-run",
        "-vv",
    ];
    for format in ["table", "json", "yaml", "raw"] {
        let out = paloma(None, &home)
            .args(args)
            .args(["-o", format])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!String::from_utf8_lossy(&out.stdout).contains("SECRET"));
        assert!(!String::from_utf8_lossy(&out.stderr).contains("SECRET"));
        if format == "raw" {
            let image = &stdout_json(&out)["template"]["components"][0]["parameters"][0]["image"];
            assert!(image.get("link").is_some(), "{image}");
            assert!(image.get("id").is_none(), "{image}");
        }
    }
}

#[test]
fn template_documented_flag_combinations_work() {
    let home = TempDir::new().unwrap();
    for args in [
        vec!["--header-image", "A", "--body-var", "x"],
        vec!["--header-image", "A", "--var", "name=x"],
        vec!["--header-text", "A", "--button", "0:url=b"],
    ] {
        paloma(None, &home)
            .args([
                "send",
                "template",
                "--via",
                "cloud",
                "15551234567",
                "t",
                "--language",
                "en",
            ])
            .args(args)
            .arg("--dry-run")
            .assert()
            .success();
    }
}

#[test]
fn template_blank_body_var_exits_5_without_a_request() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args([
            "send",
            "template",
            "--via",
            "cloud",
            "15551234567",
            "sale",
            "--language",
            "en_US",
            "--body-var",
            "   ",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(5));
    assert!(out.stdout.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn template_text_flags_send_named_or_positional_body_with_header() {
    let graph = MockServer::start().await;
    for (args, parameters) in [
        (
            vec!["--var", "customer_name=Ana=VIP", "--var", "order_number=42"],
            json!([
                {"type":"text", "parameter_name":"customer_name", "text":"Ana=VIP"},
                {"type":"text", "parameter_name":"order_number", "text":"42"}
            ]),
        ),
        (
            vec!["--body-var", "Ana", "--body-var", "42"],
            json!([
                {"type":"text", "text":"Ana"}, {"type":"text", "text":"42"}
            ]),
        ),
    ] {
        let positional = args[0] == "--body-var";
        let mut components = Vec::new();
        if positional {
            components.push(
                json!({"type":"header", "parameters":[{"type":"text", "text":"Order update"}]}),
            );
        }
        components.push(json!({"type":"body", "parameters":parameters}));
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/messages")))
            .and(body_json(json!({"messaging_product":"whatsapp", "recipient_type":"individual", "to":"15551234567", "type":"template", "template":{
                "name":"order_ready", "language":{"code":"en_US"}, "components":components
            }})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.FLAGS"}]})))
            .expect(1).mount(&graph).await;
        let home = TempDir::new().unwrap();
        let mut command = paloma(Some(&graph), &home);
        command.args([
            "send",
            "template",
            "--via",
            "cloud",
            "15551234567",
            "order_ready",
            "--language",
            "en_US",
        ]);
        if positional {
            command.args(["--header-text", "Order update"]);
        }
        command
            .args(args)
            .arg("-q")
            .assert()
            .success()
            .stdout("wamid.FLAGS\n")
            .stderr("");
    }
}

#[test]
fn template_parameter_errors_fail_before_context_resolution() {
    let home = TempDir::new().unwrap();
    // Any attempt to resolve context would fail with a config error instead of exit 5.
    std::fs::write(home.path().join("config.toml"), "broken = [").unwrap();
    for args in [
        vec!["--var", "missing_equals"],
        vec!["--var", "=Ana"],
        vec!["--var", "Name=Ana"],
        vec!["--var", "first-name=Ana"],
        vec!["--var", "name=   "],
        vec!["--var", "name=Ana", "--var", "name=Bob"],
        vec!["--header-text", "   "],
    ] {
        paloma(None, &home)
            .args([
                "send",
                "template",
                "--via",
                "cloud",
                "15551234567",
                "order_ready",
                "--language",
                "en_US",
            ])
            .args(args)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty());
    }
    let file = home.path().join("bad-components.json");
    for document in [
        "not json",
        "{}",
        "null",
        "[1]",
        "[{\"type\":\"\"}]",
        "[{\"type\":\"body\",\"parameters\":{}}]",
        "[{\"type\":\"body\",\"parameters\":[null]}]",
        "{\"components\":[],\"to\":\"other\"}",
    ] {
        std::fs::write(&file, document).unwrap();
        paloma(None, &home)
            .args([
                "send",
                "template",
                "--via",
                "cloud",
                "15551234567",
                "order_ready",
                "--language",
                "en_US",
                "--components-file",
            ])
            .arg(&file)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty());
    }
    paloma(None, &home)
        .args([
            "send",
            "template",
            "--via",
            "cloud",
            "15551234567",
            "order_ready",
            "--language",
            "en_US",
            "--components-file",
        ])
        .arg(home.path().join("missing.json"))
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty());
}

#[test]
fn template_parameter_sources_conflict_as_usage_errors() {
    let home = TempDir::new().unwrap();
    for args in [
        vec!["--components-file", "unused.json", "--body-var", "Ana"],
        vec!["--components-file", "unused.json", "--var", "name=Ana"],
        vec!["--components-file", "unused.json", "--header-text", "Hi"],
        vec!["--body-var", "Ana", "--var", "name=Ana"],
        vec!["--var", "name=Ana", "--header-text", "Hi"],
    ] {
        paloma(None, &home)
            .args([
                "send",
                "template",
                "--via",
                "cloud",
                "15551234567",
                "order_ready",
                "--language",
                "en_US",
            ])
            .args(args)
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_authentication_template_to_a_bsuid_surfaces_metas_refusal() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {
                "message": "(#131062) Business-scoped User ID (BSUID) recipients are not supported for this message.",
                "type": "OAuthException",
                "code": 131_062
            }
        })))
        .expect(1)
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            "--via",
            "cloud",
            BSUID,
            "otp",
            "--language",
            "en",
            "-q",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("131062").and(predicate::str::contains("phone number")));
}

/// Each lookup returns the next category so reclassification can be exercised without
/// caching or timing-dependent mocks. Exact query matching also catches account drift.
async fn mock_routing_categories(graph: &MockServer, account: &str, categories: &[&str]) {
    let categories: Vec<String> = categories.iter().map(ToString::to_string).collect();
    let expected = u64::try_from(categories.len()).unwrap();
    let calls = std::sync::atomic::AtomicUsize::new(0);
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{account}/message_templates")))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .and(query_param("name", "order"))
        .and(query_param("language", "en_US"))
        .and(query_param("fields", "id,name,language,category"))
        .and(query_param("limit", "100"))
        .respond_with(move |_: &wiremock::Request| {
            let index = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_json(json!({"data":[{
                "id":"template_1", "name":"order", "language":"en_US",
                "category":categories[index.min(categories.len() - 1)]
            }]}))
        })
        .expect(expected)
        .mount(graph)
        .await;
}

fn routing_template_body() -> Value {
    json!({"messaging_product":"whatsapp", "recipient_type":"individual", "to":"15551234567", "type":"template", "template":{"name":"order", "language":{"code":"en_US"}}})
}

#[tokio::test(flavor = "multi_thread")]
async fn template_auto_routes_each_current_category_and_preserves_response_output() {
    for (category, edge) in [
        ("MARKETING", "marketing_messages"),
        ("UTILITY", "messages"),
        ("AUTHENTICATION", "messages"),
    ] {
        let graph = MockServer::start().await;
        let home = TempDir::new().unwrap();
        mock_routing_categories(&graph, WABA, &[category, category, category]).await;
        let response =
            json!({"messages":[{"id":"wamid.ROUTED"}], "contacts":[{"input":"15551234567"}]});
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/{edge}")))
            .and(header("authorization", format!("Bearer {TOKEN}")))
            .and(body_json(routing_template_body()))
            .respond_with(ResponseTemplate::new(200).set_body_json(&response))
            .expect(3)
            .mount(&graph)
            .await;
        for format in ["json", "raw"] {
            let out = paloma(Some(&graph), &home)
                .args([
                    "send",
                    "template",
                    "15551234567",
                    "order",
                    "--language",
                    "en_US",
                    "-o",
                    format,
                ])
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(out.stderr.is_empty());
            assert_eq!(stdout_json(&out), response);
        }
        paloma(Some(&graph), &home)
            .args([
                "send",
                "template",
                "15551234567",
                "order",
                "--language",
                "en_US",
                "-q",
            ])
            .assert()
            .success()
            .stdout("wamid.ROUTED\n")
            .stderr("");
        assert_eq!(graph.received_requests().await.unwrap().len(), 6);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn marketing_route_keeps_bsuid_components_reply_and_the_billed_account() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    mock_routing_categories(&graph, "777", &["MARKETING"]).await;
    let components = json!([
        {"type":"header","parameters":[{"type":"image","image":{"id":"MEDIA_1"}}]},
        {"type":"body","parameters":[{"type":"text","parameter_name":"first_name","text":"Ana"}]},
        {"type":"button","sub_type":"flow","index":"0","parameters":[{"type":"action","action":{"flow_token":"ROUTING_FLOW_SECRET"}}]}
    ]);
    let file = home.path().join("routing-components.json");
    std::fs::write(&file, components.to_string()).unwrap();
    Mock::given(method("POST")).and(path(format!("/v26.0/{PHONE}/marketing_messages")))
        .and(body_json(json!({"messaging_product":"whatsapp", "recipient_type":"individual", "recipient":BSUID, "messaging_account_id":"777", "context":{"message_id":"wamid.OLD"}, "type":"template", "template":{"name":"order", "language":{"code":"en_US"}, "components":components}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.BSUID"}],"contacts":[{"user_id":BSUID}]})))
        .expect(1).mount(&graph).await;
    let out = paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            BSUID,
            "order",
            "--language",
            "en_US",
            "--messaging-account-id",
            "777",
            "--components-file",
        ])
        .arg(&file)
        .args(["--reply-to", "wamid.OLD", "-q", "-vv"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"wamid.BSUID\n");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("quoted message"));
    assert!(!stderr.contains("ROUTING_FLOW_SECRET"));
    assert!(!stderr.contains(TOKEN));
}

#[tokio::test(flavor = "multi_thread")]
async fn explicit_template_routes_skip_metadata_and_need_no_account_context() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (via, edge) in [("cloud", "messages"), ("marketing", "marketing_messages")] {
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/{edge}")))
            .and(body_json(routing_template_body()))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"messages":[{"id":"wamid.EXPLICIT"}]})),
            )
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .env_remove("PALOMA_MESSAGING_ACCOUNT_ID")
            .args([
                "send",
                "template",
                "15551234567",
                "order",
                "--language",
                "en_US",
                "--via",
                via,
                "-q",
            ])
            .assert()
            .success()
            .stdout("wamid.EXPLICIT\n")
            .stderr("");
    }
    assert!(
        graph
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.method.as_str() == "POST")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn template_routing_dry_runs_are_offline_and_show_dynamic_or_fixed_endpoints() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for via in ["auto", "cloud", "marketing"] {
        let out = paloma(Some(&graph), &home)
            .env_remove("PALOMA_TOKEN")
            .args([
                "send",
                "template",
                "15551234567",
                "order",
                "--language",
                "en_US",
                "--via",
                via,
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
        let plan = stdout_json(&out);
        if via == "auto" {
            assert_eq!(plan[0]["method"], "GET");
            assert!(
                plan[0]["url"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("/{WABA}/message_templates?"))
            );
            assert!(
                plan[1]["url"]
                    .as_str()
                    .unwrap()
                    .contains("<messages-or-marketing_messages-from-category>")
            );
            assert_eq!(plan[1]["body"], routing_template_body());
        } else {
            let edge = if via == "cloud" {
                "messages"
            } else {
                "marketing_messages"
            };
            assert!(
                plan["url"]
                    .as_str()
                    .unwrap()
                    .ends_with(&format!("/{PHONE}/{edge}"))
            );
            assert_eq!(plan["body"], routing_template_body());
        }
        assert!(String::from_utf8_lossy(&out.stderr).contains("dry-run: no request sent"));
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_template_routing_matches_exact_name_and_language_across_pages() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET")).and(path(format!("/v26.0/{WABA}/message_templates")))
        .and(query_param("after", "next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"name":"order","language":"en_US","category":"MARKETING","correct_category":"UTILITY"}]})))
        .expect(1).with_priority(1).mount(&graph).await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/message_templates")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[
            {"name":"order_extra","language":"en_US","category":"UTILITY"},
            {"name":"order","language":"es","category":"AUTHENTICATION"}],
            "paging":{"cursors":{"after":"next"},"next":"https://graph.facebook.com/next"}})))
        .expect(1)
        .with_priority(2)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/marketing_messages")))
        .and(body_json(routing_template_body()))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.EXACT"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            "15551234567",
            "order",
            "--language",
            "en_US",
            "-q",
        ])
        .assert()
        .success()
        .stdout("wamid.EXACT\n")
        .stderr("");
}

#[tokio::test(flavor = "multi_thread")]
async fn template_routing_rejects_missing_unknown_or_ambiguous_metadata_without_sending() {
    for response in [
        json!({"data":[]}),
        json!({"data":[{"name":"order","language":"en_US"}]}),
        json!({"data":[{"name":"order","language":"en_US","category":"FUTURE_CATEGORY"}]}),
        json!({"data":[{"name":"order","language":"en_US","category":"UTILITY"},{"name":"order","language":"en_US","category":"MARKETING"}]}),
        json!({"data":null}),
    ] {
        let graph = MockServer::start().await;
        let home = TempDir::new().unwrap();
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args([
                "send",
                "template",
                "15551234567",
                "order",
                "--language",
                "en_US",
            ])
            .assert()
            .code(1)
            .stdout("");
        assert_eq!(graph.received_requests().await.unwrap().len(), 1);
    }
}

#[test]
fn template_routing_validates_locally_before_account_resolution() {
    let home = TempDir::new().unwrap();
    for args in [
        vec!["bad", "order", "--language", "en_US"],
        vec![
            "15551234567",
            "order",
            "--language",
            "en_US",
            "--var",
            "bad",
        ],
        vec!["15551234567", "order", "--language", " "],
    ] {
        paloma(None, &home)
            .env_remove("PALOMA_WABA_ID")
            .env_remove("PALOMA_TOKEN")
            .args(["send", "template"])
            .args(args)
            .assert()
            .code(5)
            .stdout("");
    }
    paloma(None, &home)
        .env_remove("PALOMA_WABA_ID")
        .env_remove("PALOMA_MESSAGING_ACCOUNT_ID")
        .args([
            "send",
            "template",
            "15551234567",
            "order",
            "--language",
            "en_US",
            "--dry-run",
        ])
        .assert()
        .code(3)
        .stdout("")
        .stderr(predicate::str::contains("--via cloud|marketing"));
    paloma(None, &home)
        .args([
            "send",
            "template",
            "15551234567",
            "order",
            "--language",
            "en_US",
            "--via",
            "unknown",
        ])
        .assert()
        .code(2)
        .stdout("");
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_template_routing_reroutes_once_after_confirmed_category_changes() {
    for (old, new, first_edge, next_edge, code) in [
        (
            "UTILITY",
            "MARKETING",
            "messages",
            "marketing_messages",
            131_063,
        ),
        (
            "MARKETING",
            "UTILITY",
            "marketing_messages",
            "messages",
            100,
        ),
    ] {
        let graph = MockServer::start().await;
        let home = TempDir::new().unwrap();
        mock_routing_categories(&graph, WABA, &[old, new]).await;
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/{first_edge}")))
            .and(body_json(routing_template_body()))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(json!({"error":{"code":code,"message":"category rejected"}})),
            )
            .expect(1)
            .mount(&graph)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/{next_edge}")))
            .and(body_json(routing_template_body()))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"messages":[{"id":"wamid.REROUTED"}]})),
            )
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args([
                "send",
                "template",
                "15551234567",
                "order",
                "--language",
                "en_US",
                "-q",
            ])
            .assert()
            .success()
            .stdout("wamid.REROUTED\n")
            .stderr(predicate::str::contains(format!(
                "retrying once via /{next_edge}"
            )));
        let requests = graph.received_requests().await.unwrap();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[1].body, requests[3].body);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_template_routing_stops_if_refresh_does_not_change_the_endpoint() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    mock_routing_categories(&graph, WABA, &["UTILITY", "AUTHENTICATION"]).await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({"error":{"code":131_063,"message":"original refusal"}})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            "15551234567",
            "order",
            "--language",
            "en_US",
            "-q",
        ])
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("original refusal"));
    assert_eq!(graph.received_requests().await.unwrap().len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_template_routing_never_loops_on_a_second_rejection() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    mock_routing_categories(&graph, WABA, &["UTILITY", "MARKETING"]).await;
    for (edge, code) in [("messages", 131_063), ("marketing_messages", 100)] {
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/{edge}")))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(json!({"error":{"code":code,"message":"still rejected"}})),
            )
            .expect(1)
            .mount(&graph)
            .await;
    }
    paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            "15551234567",
            "order",
            "--language",
            "en_US",
        ])
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("still rejected"));
    assert_eq!(graph.received_requests().await.unwrap().len(), 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_template_routing_never_reroutes_ambiguous_or_noncategory_failures() {
    for (response, exit) in [
        (
            ResponseTemplate::new(500)
                .set_body_json(json!({"error":{"code":100,"message":"server failed"}})),
            1,
        ),
        (
            ResponseTemplate::new(503)
                .set_body_json(json!({"error":{"code":131_063,"message":"unavailable"}})),
            1,
        ),
        (
            ResponseTemplate::new(403)
                .set_body_json(json!({"error":{"code":100,"message":"denied"}})),
            3,
        ),
        (
            ResponseTemplate::new(429)
                .set_body_json(json!({"error":{"code":130_429,"message":"slow"}})),
            4,
        ),
        (
            ResponseTemplate::new(400)
                .set_body_json(json!({"error":{"code":131_049,"message":"policy"}})),
            6,
        ),
        (
            ResponseTemplate::new(400).set_body_string("not a Graph error"),
            1,
        ),
        (ResponseTemplate::new(200).set_body_string("not JSON"), 1),
        (
            ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(2)),
            1,
        ),
    ] {
        let graph = MockServer::start().await;
        let home = TempDir::new().unwrap();
        mock_routing_categories(&graph, WABA, &["UTILITY"]).await;
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/messages")))
            .respond_with(response)
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args([
                "send",
                "template",
                "15551234567",
                "order",
                "--language",
                "en_US",
                "--timeout",
                "200ms",
                "-q",
            ])
            .assert()
            .code(exit)
            .stdout("")
            .stderr(predicate::str::contains("retrying once").not());
        assert_eq!(graph.received_requests().await.unwrap().len(), 2);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn explicit_template_routing_does_not_refresh_or_override_a_rejected_route() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("POST")).and(path(format!("/v26.0/{PHONE}/messages")))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error":{"code":131_063,"message":"Marketing templates disabled for Cloud API"}})))
        .expect(1).mount(&graph).await;
    paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            "15551234567",
            "order",
            "--language",
            "en_US",
            "--via",
            "cloud",
        ])
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("--via marketing"));
    assert_eq!(graph.received_requests().await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_template_routing_surfaces_lookup_auth_errors_without_a_send() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403).set_body_json(
            json!({"error":{"code":200,"message":"template management permission missing"}}),
        ))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "send",
            "template",
            "15551234567",
            "order",
            "--language",
            "en_US",
        ])
        .assert()
        .code(3)
        .stdout("")
        .stderr(predicate::str::contains(
            "template management permission missing",
        ));
    assert_eq!(graph.received_requests().await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn template_routing_never_refreshes_http_400_auth_policy_or_unknown_rejections() {
    // HTTP-400 auth and policy codes keep their own exit codes, and none may reroute.
    for (category, edge) in [("UTILITY", "messages"), ("MARKETING", "marketing_messages")] {
        for (code, exit) in [
            (3, 3),
            (10, 3),
            (190, 3),
            (200, 3),
            (299, 3),
            (131_005, 3),
            (368, 6),
            (130_497, 6),
            (131_031, 6),
            (999_999, 1),
        ] {
            let graph = MockServer::start().await;
            let home = TempDir::new().unwrap();
            mock_routing_categories(&graph, WABA, &[category]).await;
            Mock::given(method("POST"))
                .and(path(format!("/v26.0/{PHONE}/{edge}")))
                .respond_with(
                    ResponseTemplate::new(400)
                        .set_body_json(json!({"error":{"code":code,"message":"do not reroute"}})),
                )
                .expect(1)
                .mount(&graph)
                .await;
            paloma(Some(&graph), &home)
                .args([
                    "send",
                    "template",
                    "15551234567",
                    "order",
                    "--language",
                    "en_US",
                    "-q",
                ])
                .assert()
                .code(exit)
                .stdout("")
                .stderr(predicate::str::contains("retrying once").not());
            assert_eq!(graph.received_requests().await.unwrap().len(), 2);
        }
    }
}

#[tokio::test]
async fn marketing_options_go_on_the_marketing_route_only() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("POST"))
        .and(path("/v26.0/111/marketing_messages"))
        .and(body_json(json!({
            "messaging_product":"whatsapp","recipient_type":"individual","to":"15551234567",
            "type":"template","template":{"name":"sale","language":{"code":"en_US"}},
            "product_policy":"STRICT","message_activity_sharing":false,
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.m"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let base = [
        "send",
        "template",
        "15551234567",
        "sale",
        "--language",
        "en_US",
    ];
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(base)
        .args([
            "--via",
            "marketing",
            "--product-policy",
            "strict",
            "--no-message-activity-sharing",
            "-q",
        ])
        .assert()
        .success()
        .stdout("wamid.m\n");
    for route in [vec![], vec!["--via", "cloud"]] {
        paloma(Some(&graph), &home)
            .args(base)
            .args(route)
            .args(["--product-policy", "strict"])
            .assert()
            .code(5)
            .stderr(predicate::str::contains("--via marketing"));
    }
}

#[test]
fn template_files_can_carry_a_max_price() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("sale.json");
    std::fs::write(
        &file,
        json!({"name":"sale","category":"MARKETING","language":"en",
            "components":[{"type":"BODY","text":"Sale"}],
            "optimization_spec":{"bid_amount":400,"bid_strategy":"LOWEST_COST_WITH_BID_CAP"}})
        .to_string(),
    )
    .unwrap();
    let out = paloma(None, &home)
        .args(["template", "create", "--dry-run", "-o", "json", "--file"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        stdout_json(&out)["body"]["optimization_spec"]["bid_amount"],
        400
    );
}
