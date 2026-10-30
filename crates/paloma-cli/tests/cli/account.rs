//! `paloma account` contract tests.

use crate::common::*;

#[tokio::test(flavor = "multi_thread")]
async fn account_get_requests_details_and_preserves_output_contracts() {
    let graph = MockServer::start().await;
    let body = json!({"id": WABA, "name": "Example", "account_review_status": "APPROVED"});
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}")))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .and(query_param("fields", "id,name,timezone_id,message_template_namespace,account_review_status,business_verification_status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(3)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["account", "get"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Example").and(predicate::str::contains("APPROVED")))
        .stderr("");
    paloma(Some(&graph), &home)
        .args(["account", "get", "-q"])
        .assert()
        .success()
        .stdout(format!("{WABA}\n"))
        .stderr("");
    let out = paloma(Some(&graph), &home)
        .args(["account", "get", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    assert_eq!(stdout_json(&out), body);
    assert!(
        graph
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.body.is_empty())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn account_get_custom_fields_and_health_use_the_waba_flag() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (verb, flags, fields, body) in [
        (
            "get",
            vec!["--fields", "id,name,service_pricing_policy"],
            "id,name,service_pricing_policy",
            json!({"id":"999", "name":"Other", "service_pricing_policy":"PAID_EXEMPT"}),
        ),
        (
            "health",
            vec![],
            "health_status",
            json!({"id":"999", "health_status":{"can_send_message":"BLOCKED", "entities":[{"entity_type":"WABA", "id":"999", "can_send_message":"BLOCKED", "errors":[{"error_code":141_006}]}]}}),
        ),
    ] {
        Mock::given(method("GET"))
            .and(path("/v26.0/999"))
            .and(query_param("fields", fields))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .expect(2)
            .mount(&graph)
            .await;
        let out = paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["account", verb, "--waba-id", "999", "-o", "json"])
            .args(&flags)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(out.stderr.is_empty());
        assert_eq!(stdout_json(&out), body);
        paloma(Some(&graph), &home)
            .args(["account", verb, "--waba-id", "999", "-q"])
            .args(&flags)
            .assert()
            .success()
            .stdout("999\n")
            .stderr("");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn account_lists_render_nested_apps_and_limits_without_a_phone_context() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (verb, edge, body, expected) in [
        (
            "limits",
            "phone_numbers",
            json!({"data":[{"id":PHONE, "display_phone_number":"15551234567", "whatsapp_business_manager_messaging_limit":"TIER_250"}]}),
            "TIER_250",
        ),
        (
            "subscribed-apps",
            "subscribed_apps",
            json!({"data":[{"whatsapp_business_api_data":{"id":"333", "name":"Example App"}, "override_callback_uri":"https://example.com/webhook"}]}),
            "Example App",
        ),
    ] {
        let mut mock = Mock::given(method("GET"))
            .and(path(format!("/v26.0/{WABA}/{edge}")))
            .and(header("authorization", format!("Bearer {TOKEN}")));
        if verb == "limits" {
            mock = mock.and(query_param(
                "fields",
                "id,display_phone_number,whatsapp_business_manager_messaging_limit",
            ));
        }
        mock.respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .expect(3)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["account", verb])
            .assert()
            .success()
            .stdout(predicate::str::contains(expected))
            .stderr("");
        let out = paloma(Some(&graph), &home)
            .args(["account", verb, "-o", "json"])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(out.stderr.is_empty());
        assert_eq!(stdout_json(&out), body);
        // A list prints one row id per line under -q.
        let id = if verb == "limits" { PHONE } else { "333" };
        paloma(Some(&graph), &home)
            .args(["account", verb, "-q"])
            .assert()
            .success()
            .stdout(format!("{id}\n"))
            .stderr("");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn account_lists_warn_and_follow_cursors_with_all() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (verb, edge) in [
        ("limits", "phone_numbers"),
        ("subscribed-apps", "subscribed_apps"),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/v26.0/{WABA}/{edge}")))
            .and(query_param("after", "start"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data":[{"id":"first"}],
                "paging":{"cursors":{"after":"next"}, "next":"https://graph.facebook.com/next"}
            })))
            .expect(2)
            .mount(&graph)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/v26.0/{WABA}/{edge}")))
            .and(query_param("after", "next"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data":[{"id":"second"}]})),
            )
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args(["account", verb, "--after", "start", "-o", "json"])
            .assert()
            .success()
            .stdout(predicate::str::contains("first"))
            .stderr(predicate::str::contains("more results exist"));
        let out = paloma(Some(&graph), &home)
            .args(["account", verb, "--after", "start", "--all", "-o", "json"])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(out.stderr.is_empty());
        assert_eq!(
            stdout_json(&out)["data"],
            json!([{"id":"first"}, {"id":"second"}])
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn account_reads_dry_run_without_network_or_token() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for verb in ["get", "health", "limits", "subscribed-apps"] {
        let out = paloma(Some(&graph), &home)
            .env_remove("PALOMA_TOKEN")
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["account", verb, "--dry-run", "-o", "json"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let request = stdout_json(&out);
        assert_eq!(request["method"], "GET");
        assert!(
            request["url"]
                .as_str()
                .unwrap()
                .contains(&format!("/v26.0/{WABA}"))
        );
        assert!(!request.to_string().contains(TOKEN));
        assert!(String::from_utf8_lossy(&out.stderr).contains("dry-run: no request sent"));
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[test]
fn account_reads_require_waba_and_validate_fields_before_context() {
    let home = TempDir::new().unwrap();
    for verb in ["get", "health", "limits", "subscribed-apps"] {
        paloma(None, &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["account", verb, "--dry-run"])
            .assert()
            .code(3)
            .stdout("")
            .stderr(predicate::str::contains("--waba-id"));
    }
    paloma(None, &home)
        .env_remove("PALOMA_WABA_ID")
        .args(["account", "get", "--fields", "  "])
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("fields"));
    paloma(None, &home)
        .env_remove("PALOMA_WABA_ID")
        .args(["account", "analytics"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains("--start"));
}

#[tokio::test(flavor = "multi_thread")]
async fn billing_currency_start_posts_optional_credit_and_quiet_prints_migration_id() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for credit in [None, Some("12345")] {
        let mut body = json!({"currency":"INR"});
        if let Some(id) = credit {
            body["extended_credit_id"] = json!(id);
        }
        Mock::given(method("POST"))
            .and(path(format!(
                "/v26.0/{WABA}/set_payment_method_migration_intent"
            )))
            .and(header("authorization", format!("Bearer {TOKEN}")))
            .and(body_json(body))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!({"migration_id":"mig_123", "migration_status":"INITIATED"}),
                ),
            )
            .expect(1)
            .mount(&graph)
            .await;
        let mut cmd = paloma(Some(&graph), &home);
        cmd.args([
            "account",
            "billing-currency",
            "start",
            "--currency",
            "INR",
            "--yes",
            "-q",
        ]);
        if let Some(id) = credit {
            cmd.args(["--extended-credit-id", id]);
        }
        cmd.assert().success().stdout("mig_123\n").stderr("");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn billing_currency_status_and_complete_use_migration_context_and_check_readiness() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (id, status, succeeds) in [
        ("mig_ready", "READY_TO_COMPLETE", true),
        ("mig_retry", "FAILED_TO_COMPLETE", true),
        ("mig_busy", "IN_PROGRESS", false),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/v26.0/{id}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!({"id":id, "status":status, "destination_waba":{"id":"999"}}),
                ),
            )
            .expect(2)
            .mount(&graph)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{id}/resume_migration")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"id":id, "status":"COMPLETED"})),
            )
            .expect(u64::from(succeeds))
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["account", "billing-currency", "status", id, "-q"])
            .assert()
            .success()
            .stdout(format!("{id}\n"))
            .stderr("");
        let mut cmd = paloma(Some(&graph), &home);
        cmd.env_remove("PALOMA_WABA_ID")
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["account", "billing-currency", "complete", id, "--yes", "-q"]);
        if succeeds {
            cmd.assert().success().stdout(format!("{id}\n")).stderr("");
        } else {
            cmd.assert()
                .code(1)
                .stdout("")
                .stderr(predicate::str::contains("not ready to complete"));
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn billing_currency_dry_run_and_confirmation_never_send_requests() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for args in [
        vec!["start", "--currency", "BRL"],
        vec!["complete", "mig_123"],
    ] {
        let out = paloma(Some(&graph), &home)
            .env_remove("PALOMA_TOKEN")
            .args(["account", "billing-currency"])
            .args(&args)
            .args(["--dry-run", "-o", "json"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let request = stdout_json(&out);
        if args[0] == "complete" {
            assert_eq!(request[0]["method"], "GET");
            assert_eq!(request[1]["method"], "POST");
        } else {
            assert_eq!(request["method"], "POST");
        }
        assert!(String::from_utf8_lossy(&out.stderr).contains("dry-run: no request sent"));
        paloma(Some(&graph), &home)
            .args(["account", "billing-currency"])
            .args(&args)
            .assert()
            .code(5)
            .stdout("")
            .stderr(predicate::str::contains("--yes"));
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[test]
fn billing_currency_invalid_inputs_fail_before_context() {
    let home = TempDir::new().unwrap();
    for args in [
        vec!["start", "--currency", "inr"],
        vec!["start", "--currency", "INR", "--extended-credit-id", "../x"],
        vec!["status", "../x"],
        vec!["complete", "mig_123?access_token=x"],
    ] {
        paloma(None, &home)
            .env_remove("PALOMA_WABA_ID")
            .env_remove("PALOMA_TOKEN")
            .args(["account", "billing-currency"])
            .args(&args)
            .assert()
            .code(5)
            .stdout("");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn account_set_shapes_settings_including_false_and_supports_file() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}")))
        .and(body_json(json!({"name":"New name", "timezone_id":"1", "disable_marketing_messages_on_cloud_api":false})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1).mount(&graph).await;
    paloma(Some(&graph), &home)
        .args([
            "account",
            "set",
            "--name",
            "New name",
            "--timezone-id",
            "1",
            "--disable-cloud-marketing",
            "false",
            "-q",
        ])
        .assert()
        .success()
        .stdout("")
        .stderr("");
    let file = home.path().join("account.json");
    std::fs::write(&file, r#"{"name":"From file"}"#).unwrap();
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}")))
        .and(body_json(json!({"name":"From file"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["account", "set", "--file"])
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("true"))
        .stderr("");
    paloma(Some(&graph), &home)
        .args(["account", "set", "--disable-cloud-marketing", "true"])
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("--yes"));
    let out = paloma(Some(&graph), &home)
        .args([
            "account",
            "set",
            "--disable-cloud-marketing",
            "true",
            "--dry-run",
            "-o",
            "json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        stdout_json(&out)["body"],
        json!({"disable_marketing_messages_on_cloud_api":true})
    );
}

#[test]
fn account_set_rejects_empty_settings_and_nonobjects_before_context() {
    let home = TempDir::new().unwrap();
    for args in [vec![], vec!["--name", " "], vec!["--timezone-id", ""]] {
        paloma(None, &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["account", "set"])
            .args(&args)
            .assert()
            .code(5)
            .stdout("");
    }
    let file = home.path().join("bad.json");
    for body in [
        "[]",
        "{}",
        "null",
        r#"{"name":" "}"#,
        r#"{"timezone_id":0}"#,
        r#"{"disable_marketing_messages_on_cloud_api":"true"}"#,
    ] {
        std::fs::write(&file, body).unwrap();
        paloma(None, &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["account", "set", "--file"])
            .arg(&file)
            .assert()
            .code(5)
            .stdout("");
    }
    paloma(None, &home)
        .args(["account", "set", "--name", "x", "--file"])
        .arg(&file)
        .assert()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn account_activities_and_number_health_request_the_documented_fields() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/activities")))
        .and(query_param(
            "fields",
            "id,actor_business_id,actor_id,category,event_type,timestamp",
        ))
        .and(query_param("after", "cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"data":[{"event_type":"ACCOUNT_UPDATE", "timestamp":"2026-11-04"}]}),
        ))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["account", "activities", "--after", "cursor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ACCOUNT_UPDATE"))
        .stderr("");
    Mock::given(method("GET"))
        .and(path("/v26.0/999"))
        .and(query_param("fields", "health_status"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"id":"999", "health_status":{"can_send_message":"AVAILABLE"}}),
            ),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_WABA_ID")
        .args(["number", "health", "999", "-q"])
        .assert()
        .success()
        .stdout("999\n")
        .stderr("");
}

fn messaging_analytics_response() -> Value {
    json!({"id":WABA,"analytics":{"phone_numbers":["15551234567"],"country_codes":["US"],"granularity":"DAY","data_points":[{"start":1_793_491_200_u64,"end":1_793_577_600_u64,"sent":12,"delivered":10}]}})
}

fn pricing_analytics_response() -> Value {
    json!({"id":WABA,"pricing_analytics":{"data":[
        {"data_points":[{"start":1_793_491_200_u64,"end":1_793_577_600_u64,"phone_number":"15551234567","country":"IN","pricing_category":"AUTHENTICATION","pricing_type":"REGULAR","tier":"0:750000","volume":2,"cost":4.6}]},
        {"data_points":[{"start":1_793_491_200_u64,"end":1_793_577_600_u64,"country":"US","pricing_category":"MARKETING_LITE_DYNAMIC","pricing_type":"REGULAR","volume":3}]}
    ]}})
}

#[tokio::test(flavor = "multi_thread")]
async fn account_analytics_sends_the_exact_messaging_field_expansion_and_preserves_formats() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let body = messaging_analytics_response();
    Mock::given(method("GET")).and(path(format!("/v26.0/{WABA}")))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .and(query_param("fields", "analytics.start(1793491200).end(1793577600).granularity(DAY).phone_numbers([\"15551234567\",\"15557654321\"]).product_types([0,2,100]).country_codes([\"US\",\"IN\"])"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(5).mount(&graph).await;
    for format in ["table", "json", "yaml", "raw"] {
        let out = paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args([
                "account",
                "analytics",
                "--start",
                "2026-11-01",
                "--end",
                "2026-11-02",
                "--phone-number",
                "+15551234567,15557654321",
                "--product-type",
                "template,non-template",
                "--product-type",
                "incoming",
                "--country",
                "US,IN",
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
        match format {
            "table" => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                assert!(
                    stdout.contains("SENT")
                        && stdout.contains("DELIVERED")
                        && stdout.contains("12")
                        && stdout.contains("10")
                );
            }
            "yaml" => assert_eq!(
                serde_norway::from_slice::<Value>(&out.stdout).unwrap(),
                body
            ),
            _ => assert_eq!(stdout_json(&out), body),
        }
    }
    paloma(Some(&graph), &home)
        .args([
            "account",
            "analytics",
            "--start",
            "2026-11-01",
            "--end",
            "2026-11-02",
            "--phone-number",
            "+15551234567,15557654321",
            "--product-type",
            "template,non-template,incoming",
            "--country",
            "US,IN",
            "-q",
        ])
        .assert()
        .success()
        .stdout("")
        .stderr("");
    assert!(
        graph
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.body.is_empty())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn account_pricing_analytics_serializes_all_filters_and_flattens_every_table_group() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let body = pricing_analytics_response();
    Mock::given(method("GET")).and(path(format!("/v26.0/{WABA}")))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .and(query_param("fields", "pricing_analytics.start(1793491200).end(1793577600).granularity(DAILY).phone_numbers([\"15551234567\"]).country_codes([\"US\",\"IN\"]).metric_types([\"COST\",\"VOLUME\"]).pricing_types([\"REGULAR\",\"FREE_ENTRY_POINT\"]).pricing_categories([\"AUTHENTICATION\",\"MARKETING_LITE_DYNAMIC\"]).dimensions([\"COUNTRY\",\"PRICING_CATEGORY\",\"TIER\",\"PHONE\",\"PRICING_TYPE\"])"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(5).mount(&graph).await;
    let args = [
        "account",
        "analytics",
        "--pricing",
        "--start",
        "1793491200",
        "--end",
        "1793577600",
        "--phone-number",
        "15551234567",
        "--country",
        "US,IN",
        "--metric",
        "cost,volume",
        "--pricing-type",
        "regular,free-entry-point",
        "--pricing-category",
        "authentication,marketing-lite-dynamic",
        "--dimension",
        "country,pricing-category,tier,phone,pricing-type",
    ];
    for format in ["table", "json", "yaml", "raw"] {
        let out = paloma(Some(&graph), &home)
            .args(args)
            .args(["-o", format])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stderr.is_empty());
        match format {
            "table" => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                assert!(
                    stdout.contains("AUTHENTICATION") && stdout.contains("MARKETING_LITE_DYNAMIC")
                );
                assert!(stdout.contains("0:750000") && stdout.contains("4.6"));
                let missing_cost_row = stdout
                    .lines()
                    .find(|line| line.contains("MARKETING_LITE_DYNAMIC"))
                    .unwrap();
                assert!(missing_cost_row.ends_with('-'), "{missing_cost_row}");
            }
            "yaml" => assert_eq!(
                serde_norway::from_slice::<Value>(&out.stdout).unwrap(),
                body
            ),
            _ => assert_eq!(stdout_json(&out), body),
        }
    }
    paloma(Some(&graph), &home)
        .args(args)
        .arg("-q")
        .assert()
        .success()
        .stdout("")
        .stderr("");
}

#[tokio::test(flavor = "multi_thread")]
async fn analytics_granularities_translate_by_mode_and_omit_unspecified_filters_offline() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (pricing, granularity, expected) in [
        (false, "half-hour", "HALF_HOUR"),
        (false, "day", "DAY"),
        (false, "month", "MONTH"),
        (true, "half-hour", "HALF_HOUR"),
        (true, "day", "DAILY"),
        (true, "month", "MONTHLY"),
    ] {
        let mut cmd = paloma(Some(&graph), &home);
        cmd.env_remove("PALOMA_TOKEN")
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args([
                "account",
                "analytics",
                "--start",
                "2026-11-01",
                "--end",
                "2026-11-02",
                "--granularity",
                granularity,
                "--dry-run",
                "-o",
                "json",
            ]);
        if pricing {
            cmd.arg("--pricing");
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let request = stdout_json(&out);
        assert_eq!(request["method"], "GET");
        assert!(request.get("body").is_none());
        let field = if pricing {
            "pricing_analytics"
        } else {
            "analytics"
        };
        let query = format!("{field}.start(1793491200).end(1793577600).granularity({expected})");
        let url = percent_encoding::percent_decode_str(request["url"].as_str().unwrap())
            .decode_utf8()
            .unwrap();
        assert_eq!(url, format!("{}/v26.0/{WABA}?fields={query}", graph.uri()));
        assert!(String::from_utf8_lossy(&out.stderr).contains("dry-run: no request sent"));
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn analytics_invalid_dates_ranges_and_filters_fail_before_context_or_a_request() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (start, end, extra) in [
        ("2026-02-29", "2026-03-01", vec![]),
        ("2026-11-31", "2026-12-01", vec![]),
        ("bad", "2026-11-02", vec![]),
        ("2026-11-01", "bad", vec![]),
        ("-1", "10", vec![]),
        ("2026-11-02", "2026-11-01", vec![]),
        ("2026-11-01", "2026-11-01", vec![]),
        ("18446744073709551616", "18446744073709551617", vec![]),
        ("1", "2", vec!["--country", "us"]),
        ("1", "2", vec!["--country", "USA"]),
        ("1", "2", vec!["--country", "US\").end(0)"]),
        ("1", "2", vec!["--phone-number", "111"]),
        ("1", "2", vec!["--phone-number", BSUID]),
    ] {
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .env_remove("PALOMA_TOKEN")
            .args(["account", "analytics", "--start", start, "--end", end])
            .args(extra)
            .assert()
            .code(5)
            .stdout("");
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[test]
fn analytics_closed_sets_and_mode_conflicts_are_usage_errors() {
    let home = TempDir::new().unwrap();
    for extra in [
        vec!["--granularity", "daily"],
        vec!["--product-type", "1"],
        vec!["--pricing", "--product-type", "template"],
        vec!["--metric", "cost"],
        vec!["--dimension", "country"],
        vec!["--pricing-type", "regular"],
        vec!["--pricing-category", "utility"],
        vec!["--pricing", "--metric", "conversations"],
        vec!["--pricing", "--dimension", "unknown"],
        vec!["--pricing", "--pricing-category", "unknown"],
        vec!["--pricing", "--pricing-type", "paid-exempt"],
        vec!["--conversations"],
    ] {
        paloma(None, &home)
            .env_remove("PALOMA_WABA_ID")
            .env_remove("PALOMA_TOKEN")
            .args(["account", "analytics", "--start", "1", "--end", "2"])
            .args(extra)
            .assert()
            .code(2)
            .stdout("");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn analytics_targets_the_selected_waba_and_can_read_pre_migration_history() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path("/v26.0/old_account"))
        .and(query_param(
            "fields",
            "analytics.start(1709164800).end(1709251200).granularity(DAY)",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id":"old_account","analytics":{"data_points":[]}})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env("PALOMA_MESSAGING_ACCOUNT_ID", "new_account")
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args([
            "account",
            "analytics",
            "--start",
            "2024-02-29",
            "--end",
            "2024-03-01",
            "--waba-id",
            "old_account",
        ])
        .assert()
        .success()
        .stdout("(none)\n")
        .stderr("");
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_WABA_ID")
        .args([
            "account",
            "analytics",
            "--start",
            "1",
            "--end",
            "2",
            "--dry-run",
        ])
        .assert()
        .code(3)
        .stdout("")
        .stderr(predicate::str::contains("--waba-id"));
}

#[tokio::test(flavor = "multi_thread")]
async fn analytics_empty_and_malformed_reports_are_distinct() {
    for (pricing, body, valid) in [
        (false, json!({"analytics":{"data_points":[]}}), true),
        (true, json!({"pricing_analytics":{"data":[]}}), true),
        (
            true,
            json!({"pricing_analytics":{"data":[{"data_points":[]}]}}),
            true,
        ),
        (false, json!({"id":WABA}), false),
        (false, json!({"analytics":{"data_points":[1]}}), false),
        (true, json!({"pricing_analytics":{"data":[{}]}}), false),
        (true, json!({"pricing_analytics":{"data":null}}), false),
    ] {
        let graph = MockServer::start().await;
        let home = TempDir::new().unwrap();
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(&graph)
            .await;
        let mut cmd = paloma(Some(&graph), &home);
        cmd.args(["account", "analytics", "--start", "1", "--end", "2"]);
        if pricing {
            cmd.arg("--pricing");
        }
        if valid {
            cmd.assert().success().stdout("(none)\n").stderr("");
        } else {
            cmd.assert()
                .code(1)
                .stdout("")
                .stderr(predicate::str::contains("malformed"));
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn analytics_preserves_api_auth_and_rate_limit_errors_on_stderr() {
    for (status, body, exit) in [
        (
            403,
            json!({"error":{"code":200,"message":"analytics permission denied"}}),
            3,
        ),
        (
            429,
            json!({"error":{"code":130_429,"message":"analytics throttled"}}),
            4,
        ),
    ] {
        let graph = MockServer::start().await;
        let home = TempDir::new().unwrap();
        // Reads retry throttling by design, so allow the three bounded attempts.
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_json(body)
                    .insert_header("Retry-After", "0"),
            )
            .expect(if status == 429 { 3 } else { 1 })
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args(["account", "analytics", "--start", "1", "--end", "2"])
            .assert()
            .code(exit)
            .stdout("")
            .stderr(predicate::str::contains("analytics"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn pricing_analytics_raw_keeps_decimal_text_and_unknown_response_fields() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let body = r#"{ "id":"222", "currency":"EUR", "pricing_analytics":{"data":[{"data_points":[{"start":1,"end":2,"pricing_category":"FUTURE_CATEGORY","cost":0.1234567890123456789012345}]}]}}"#;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "account",
            "analytics",
            "--pricing",
            "--start",
            "1",
            "--end",
            "2",
            "-o",
            "raw",
        ])
        .assert()
        .success()
        .stdout(format!("{body}\n"))
        .stderr("");
}

#[tokio::test(flavor = "multi_thread")]
async fn account_insights_is_confirmed_one_way_and_uses_query() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{WABA}")))
        .and(query_param("is_enabled_for_insights", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(3)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["account", "set", "--enable-insights"])
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("--yes"));
    for args in [
        vec!["--enable-insights", "--yes"],
        vec!["--insights", "true", "--yes"],
    ] {
        paloma(Some(&graph), &home)
            .args(["account", "set"])
            .args(args)
            .assert()
            .success()
            .stderr("");
    }
    for args in [
        vec!["--insights", "false"],
        vec!["--insights", "true", "--name", "Acme"],
    ] {
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["account", "set"])
            .args(args)
            .assert()
            .code(5)
            .stdout("");
    }
    let file = home.path().join("insights.json");
    for body in [
        r#"{"is_enabled_for_insights":false}"#,
        r#"{"is_enabled_for_insights":true,"name":"Acme"}"#,
    ] {
        std::fs::write(&file, body).unwrap();
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["account", "set", "--file"])
            .arg(&file)
            .assert()
            .code(5)
            .stdout("");
    }
    std::fs::write(&file, r#"{"is_enabled_for_insights":true}"#).unwrap();
    paloma(Some(&graph), &home)
        .args(["account", "set", "--file"])
        .arg(&file)
        .arg("--yes")
        .assert()
        .success();
    let requests = graph.received_requests().await.unwrap();
    assert!(requests.iter().all(|request| request.body.is_empty()));
    let out = paloma(Some(&graph), &home)
        .args([
            "account",
            "set",
            "--enable-insights",
            "--dry-run",
            "-o",
            "json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        stdout_json(&out)["url"]
            .as_str()
            .unwrap()
            .contains("is_enabled_for_insights=true")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn account_limits_portfolio_and_activities_limit_are_validated() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path("/v26.0/333"))
        .and(query_param(
            "fields",
            "whatsapp_business_manager_messaging_limit",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"whatsapp_business_manager_messaging_limit":"TIER_250"})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_WABA_ID")
        .args(["account", "limits", "--business-id", "333"])
        .assert()
        .success()
        .stdout(predicate::str::contains("TIER_250"));
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{WABA}/activities")))
        .and(query_param("limit", "100"))
        .and(query_param("after", "cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[]})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "account",
            "activities",
            "--limit",
            "100",
            "--after",
            "cursor",
        ])
        .assert()
        .success();
    for args in [
        vec!["limits", "--business-id", "x/y"],
        vec!["activities", "--limit", "101"],
    ] {
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["account"])
            .args(args)
            .assert()
            .code(5)
            .stdout("");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn health_tables_show_entity_diagnostics_but_quiet_keeps_the_id() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET")).and(path(format!("/v26.0/{WABA}")))
        .and(query_param("fields", "health_status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":WABA,"health_status":{"can_send_message":"LIMITED","entities":[{"entity_type":"WABA","id":WABA,"can_send_message":"BLOCKED","errors":[{"error_code":42,"error_description":"Payment missing","possible_solution":"Add payment"}]}]}})))
        .mount(&graph).await;
    paloma(Some(&graph), &home)
        .args(["account", "health"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Payment missing")
                .and(predicate::str::contains("Add payment")),
        );
    paloma(Some(&graph), &home)
        .args(["account", "health", "-q"])
        .assert()
        .success()
        .stdout(format!("{WABA}\n"))
        .stderr("");
}

#[tokio::test(flavor = "multi_thread")]
async fn account_users_and_automation_bot_use_the_documented_nodes() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for (args, node) in [
        (
            vec!["account", "users", "--business-id", "999", "--limit", "10"],
            "222/assigned_users",
        ),
        (vec!["account", "users", "--system"], "222/system_users"),
    ] {
        let mut mock = Mock::given(method("GET")).and(path(format!("/v26.0/{node}")));
        if node.ends_with("assigned_users") {
            mock = mock
                .and(query_param("business", "999"))
                .and(query_param("limit", "10"));
        }
        mock.respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data":[{"id":"777","name":"A"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
        paloma(Some(&graph), &home)
            .args(&args)
            .arg("-q")
            .assert()
            .success()
            .stdout("777\n");
        assert_eq!(dry_run_request(&home, &args)["method"], "GET");
    }
    Mock::given(method("GET"))
        .and(path("/v26.0/444"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id":"444","prompts":["Hi"]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["automation", "bot", "444", "-q"])
        .assert()
        .success()
        .stdout("444\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn welcome_sequences_create_update_read_and_delete_waba_ad_assets() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("sequence.json");
    let sequence = json!({"text":"Welcome","autofill_message":{"content":"Hi"},"ice_breakers":[{"title":"Help"}]});
    std::fs::write(&file, sequence.to_string()).unwrap();
    let graph = MockServer::start().await;
    for (args, body, result, quiet) in [
        (
            vec![
                "account",
                "welcome-sequence",
                "create",
                "--name",
                "Support",
                "--file",
            ],
            json!({"name":"Support","welcome_message_sequence":sequence}),
            json!({"sequence_id":"444"}),
            "444\n",
        ),
        (
            vec![
                "account",
                "welcome-sequence",
                "set",
                "444",
                "--name",
                "New",
                "--file",
            ],
            json!({"name":"New","welcome_message_sequence":sequence,"sequence_id":"444"}),
            json!({"success":true}),
            "",
        ),
    ] {
        Mock::given(method("POST"))
            .and(path("/v26.0/222/welcome_message_sequences"))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(200).set_body_json(result))
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args(&args)
            .arg(&file)
            .args(["--yes", "-q"])
            .assert()
            .success()
            .stdout(quiet);
        paloma(Some(&graph), &home)
            .args(&args)
            .arg(&file)
            .arg("--dry-run")
            .assert()
            .success()
            .stdout(predicate::str::contains("welcome_message_sequence"));
    }
    Mock::given(method("GET"))
        .and(path("/v26.0/222/welcome_message_sequences"))
        .and(query_param("sequence_id", "444"))
        .and(query_param("limit", "5"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([{"sequence_id":"444","is_used_in_ad":false}])),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "account",
            "welcome-sequence",
            "list",
            "--id",
            "444",
            "--limit",
            "5",
            "-o",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("is_used_in_ad"));
    Mock::given(method("DELETE"))
        .and(path("/v26.0/222/welcome_message_sequences"))
        .and(query_param("sequence_id", "444"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "account",
            "welcome-sequence",
            "delete",
            "444",
            "--yes",
            "-q",
        ])
        .assert()
        .success()
        .stdout("");
    paloma(Some(&graph), &home)
        .args(["account", "welcome-sequence", "delete", "444"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--yes"));
    std::fs::write(&file, "{\"ice_breakers\":[42]}").unwrap();
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_WABA_ID")
        .args([
            "account",
            "welcome-sequence",
            "create",
            "--name",
            "Support",
            "--file",
        ])
        .arg(&file)
        .assert()
        .code(5)
        .stdout("");
}

#[tokio::test(flavor = "multi_thread")]
async fn account_users_follow_cursors_and_quiet_prints_every_id() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET")).and(path("/v26.0/222/assigned_users")).and(query_param("business","999")).and(query_param("after","start")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"id":"444"}],"paging":{"next":"https://untrusted.invalid/?access_token=LEAK_SENTINEL","cursors":{"after":"next"}}}))).expect(1).mount(&graph).await;
    Mock::given(method("GET"))
        .and(path("/v26.0/222/assigned_users"))
        .and(query_param("business", "999"))
        .and(query_param("after", "next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"id":"555"}]})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "account",
            "users",
            "--business-id",
            "999",
            "--after",
            "start",
            "--all",
            "-q",
        ])
        .assert()
        .success()
        .stdout("444\n555\n")
        .stderr("");
}

#[tokio::test]
async fn welcome_sequence_list_renders_meta_s_bare_array() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path("/v26.0/222/welcome_message_sequences"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"sequence_id":"8716291","name":"Driver Sign up","is_used_in_ad":true},
            {"sequence_id":"4362","name":"Basic Triage","is_used_in_ad":false},
        ])))
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["account", "welcome-sequence", "list"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Driver Sign up").and(predicate::str::contains("[0]").not()),
        );
    paloma(Some(&graph), &home)
        .args(["account", "welcome-sequence", "list", "-q"])
        .assert()
        .success()
        .stdout("8716291\n4362\n");
}

#[tokio::test]
async fn account_reach_serializes_the_targeting_and_lists_estimates() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path("/v26.0/222/reachestimate"))
        .and(query_param(
            "targeting_spec",
            r#"{"geo_locations":{"countries":["IN","BR"]}}"#,
        ))
        .and(query_param("date_interval", "L7D"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "waba_currency":"USD",
            "estimates":[{"bid_amount":400,"users":1000,"deliveries_lower_bound":500,
                "deliveries_upper_bound":570,"cost_lower_bound":389.74,"cost_upper_bound":390.74}],
        })))
        .mount(&graph)
        .await;
    let args = [
        "account",
        "reach",
        "--country",
        "IN",
        "--country",
        "BR",
        "--interval",
        "L7D",
    ];
    paloma(Some(&graph), &home)
        .args(args)
        .assert()
        .success()
        .stdout(predicate::str::contains("BID AMOUNT").and(predicate::str::contains("389.74")));
    paloma(Some(&graph), &home)
        .args(args)
        .args(["-o", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("waba_currency"));
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_WABA_ID")
        .args([
            "account",
            "reach",
            "--country",
            "india",
            "--interval",
            "L7D",
        ])
        .assert()
        .code(5);
}
