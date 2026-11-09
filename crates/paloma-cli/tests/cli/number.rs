//! `paloma number` contract tests.

use crate::common::*;

#[tokio::test(flavor = "multi_thread")]
async fn number_get_and_quality_request_the_current_limit_field() {
    let graph = MockServer::start().await;
    let p = format!("/{V}/{PHONE}");
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param(
            "fields",
            "id,display_phone_number,verified_name,quality_rating,code_verification_status,\
             name_status,status,health_status,whatsapp_business_manager_messaging_limit,\
             throughput,platform_type",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": PHONE,
            "status": "CONNECTED",
            "whatsapp_business_manager_messaging_limit": "TIER_250",
            "health_status": {
                "can_send_message": "AVAILABLE",
                "entities": [{"entity_type": "PHONE_NUMBER", "id": PHONE, "can_send_message": "AVAILABLE"}]
            }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param(
            "fields",
            "display_phone_number,quality_rating,whatsapp_business_manager_messaging_limit,throughput",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "quality_rating": "GREEN",
            "whatsapp_business_manager_messaging_limit": "TIER_250"
        })))
        .expect(1)
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["number", "get"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("TIER_250")
                .and(predicate::str::contains("CONNECTED"))
                .and(predicate::str::contains("AVAILABLE")),
        );
    paloma(Some(&graph), &home)
        .args(["number", "quality", "-q"])
        .assert()
        .success()
        .stdout("GREEN\n");
}

#[test]
fn number_reads_no_longer_ask_for_the_deprecated_limit_field() {
    let home = TempDir::new().unwrap();
    for verb in ["get", "quality"] {
        let out = paloma(None, &home)
            .args(["number", verb, "--dry-run", "-o", "json"])
            .output()
            .unwrap();
        assert!(out.status.success());
        let url = stdout_json(&out)["url"].as_str().unwrap().to_owned();
        assert!(!url.contains("messaging_limit_tier"), "{url}");
        assert!(
            url.contains("whatsapp_business_manager_messaging_limit"),
            "{url}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn number_list_all_follows_paging_next_and_keeps_the_filters() {
    let graph = MockServer::start().await;
    let p = format!("/{V}/{WABA}/phone_numbers");
    let filtering = r#"[{"field":"account_mode","operator":"EQUAL","value":"LIVE"}]"#;
    let sort = "['last_onboarded_time_ascending']";
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param("after", "C1"))
        .and(query_param("filtering", filtering))
        .and(query_param("sort", sort))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "2" }],
            "paging": { "cursors": { "after": "C2" } }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(&p))
        .and(query_param("filtering", filtering))
        .and(query_param("sort", sort))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "1" }],
            "paging": { "cursors": { "after": "C1" }, "next": "https://graph/next" }
        })))
        .expect(1)
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["number", "list", "--all", "-o", "json", "--limit", "5"])
        .args(["--account-mode", "LIVE", "--sort", "last-onboarded-asc"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        stdout_json(&out),
        json!({ "data": [{ "id": "1" }, { "id": "2" }] })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn number_list_single_page_renders_columns_and_warns_on_stderr() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/{V}/{WABA}/phone_numbers")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "1", "verified_name": "Jasper" }],
            "paging": { "cursors": { "after": "C1" }, "next": "https://graph/next" }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["number", "list", "--sort", "last-onboarded-desc"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("VERIFIED NAME")
                .and(predicate::str::contains("Jasper"))
                .and(predicate::str::contains("--after").not()),
        )
        .stderr(predicate::str::contains("--after C1"));
    // Newest first is Meta's default, so asking for it sends no parameters at all.
    let received = graph.received_requests().await.unwrap();
    assert_eq!(received[0].url.query(), None);
}

#[test]
fn number_list_dry_run_prints_the_page_request() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args(["number", "list", "--dry-run", "-o", "json", "--all"])
        .args([
            "--account-mode",
            "SANDBOX",
            "--limit",
            "100",
            "--after",
            "C9",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v = stdout_json(&out);
    let url = v["url"].as_str().unwrap();
    assert_eq!(v["method"], "GET");
    assert!(
        url.contains(&format!("/{WABA}/phone_numbers?filtering=")),
        "{url}"
    );
    assert!(url.contains("SANDBOX"), "{url}");
    assert!(url.contains("limit=100&after=C9"), "{url}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("dry-run: no request sent"));
}

#[tokio::test(flavor = "multi_thread")]
async fn number_list_rejects_bad_flags_before_any_request() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for limit in ["0", "101"] {
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["number", "list", "--limit", limit])
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("between 1 and 100"));
    }
    paloma(Some(&graph), &home)
        .args(["number", "list", "--account-mode", "PAUSED"])
        .assert()
        .code(2);
    paloma(Some(&graph), &home)
        .args(["number", "list", "--sort", "creation-time"])
        .assert()
        .code(2);
    assert_eq!(graph.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn number_register_sends_the_data_localization_region() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/{PHONE}/register")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "pin": "825194",
            "data_localization_region": "CH"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success": true})))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .env("PALOMA_PIN", "825194")
        .args(["number", "register", "--data-localization-region", "CH"])
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
async fn number_register_without_a_region_leaves_it_out_of_the_body() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/{PHONE}/register")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "pin": "825194"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success": true})))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .env("PALOMA_PIN", "825194")
        .args(["number", "register"])
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
async fn number_register_rejects_an_unsupported_region_before_the_pin_or_any_request() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for bad in ["US", "de", "DEU", ""] {
        // No PIN and no phone number id: either would fail differently if read first.
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["number", "register", "--data-localization-region", bad])
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("data localization region"));
    }
    assert_eq!(graph.received_requests().await.unwrap().len(), 0);
}

#[test]
fn number_register_dry_run_shows_the_region_and_masks_the_pin() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .env("PALOMA_PIN", "825194")
        .args(["number", "register", "--data-localization-region", "DE"])
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v = stdout_json(&out);
    assert_eq!(v["body"]["data_localization_region"], "DE");
    assert_eq!(v["body"]["pin"], "<redacted>");
    assert!(!v.to_string().contains("825194"), "{v}");
}

#[test]
fn number_explicit_id_validation_precedes_context() {
    let home = TempDir::new().unwrap();
    for verb in ["get", "quality", "health"] {
        paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["number", verb, "../bad"])
            .assert()
            .code(5)
            .stdout("");
    }
}

// ---- identity, settings and administration contracts ----
#[tokio::test(flavor = "multi_thread")]
async fn number_onboarding_and_identity_writes_use_exact_payloads() {
    let home = TempDir::new().unwrap();
    for (args, node, body, id) in [
        (
            vec![
                "number",
                "verification",
                "request",
                "--method",
                "SMS",
                "--language",
                "es_ES",
            ],
            "111/request_code",
            json!({"code_method":"SMS","language":"es_ES"}),
            false,
        ),
        (
            vec![
                "number",
                "add",
                "15551234567",
                "--cc",
                "1",
                "--name",
                "Acme",
            ],
            "222/phone_numbers",
            json!({"cc":"1","phone_number":"15551234567","verified_name":"Acme"}),
            true,
        ),
        (
            vec!["number", "name", "set", "Acme Support"],
            "111",
            json!({"new_display_name":"Acme Support"}),
            false,
        ),
        (
            vec!["number", "visibility", "set", "HIDDEN"],
            "111",
            json!({"search_visibility":"NON_VISIBLE"}),
            false,
        ),
        (
            vec!["number", "visibility", "set", "NON_VISIBLE"],
            "111",
            json!({"search_visibility":"NON_VISIBLE"}),
            false,
        ),
        (
            vec!["number", "visibility", "set", "VISIBLE"],
            "111",
            json!({"search_visibility":"VISIBLE"}),
            false,
        ),
        (
            vec![
                "number",
                "username",
                "set",
                "Acme_Store",
                "--force-transfer",
            ],
            "111/username",
            json!({"username":"Acme_Store","transfer_action":"force_transfer"}),
            false,
        ),
        (
            vec!["number", "settings", "set", "--storage-region", "IN"],
            "111/settings",
            json!({"storage_configuration":{"status":"IN_COUNTRY_STORAGE_ENABLED","data_localization_region":"IN"}}),
            false,
        ),
        (
            vec!["number", "settings", "set", "--no-storage"],
            "111/settings",
            json!({"storage_configuration":{"status":"IN_COUNTRY_STORAGE_DISABLED"}}),
            false,
        ),
        (
            vec!["number", "settings", "set", "--no-identity-check"],
            "111/settings",
            json!({"user_identity_change":{"enable_identity_key_check":false}}),
            false,
        ),
    ] {
        let graph = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/{V}/{node}")))
            .and(body_json(body.clone()))
            .respond_with(ResponseTemplate::new(200).set_body_json(if id {
                json!({"id":"444"})
            } else {
                json!({"success":true})
            }))
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args(&args)
            .args(["--yes", "-q"])
            .assert()
            .success()
            .stdout(if id { "444\n" } else { "" });
        let preview = dry_run_request(&home, &args);
        assert_eq!(preview["body"], body, "{args:?}");
        assert_eq!(graph.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn usernames_contacts_and_settings_reads_and_deletes_keep_their_contracts() {
    let home = TempDir::new().unwrap();
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/{V}/111/username")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"username":"acme_shop","status":"approved"})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["number", "username", "get", "-q"])
        .assert()
        .success()
        .stdout("acme_shop\n")
        .stderr("");
    Mock::given(method("GET"))
        .and(path(format!("/{V}/111/username_suggestions")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data":[{"username_suggestions":["acme_shop"]}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["number", "username", "list", "-o", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("acme_shop"));
    Mock::given(method("DELETE"))
        .and(path(format!("/{V}/111/username")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["number", "username", "delete", "--yes", "-q"])
        .assert()
        .success()
        .stdout("");
    for (verb, http) in [("get", "GET"), ("delete", "DELETE")] {
        Mock::given(method(http))
            .and(path(format!("/{V}/111/contact_book")))
            .and(query_param("messaging_product", "whatsapp"))
            .and(query_param("bsuid", "US.12345"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"contacts":[{"user_id":"US.12345"}]})),
            )
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args([
                "number", "contacts", verb, "US.12345", "--yes", "-o", "json",
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains("US.12345"));
        assert_eq!(
            dry_run_request(&home, &["number", "contacts", verb, "US.12345"])["method"],
            http
        );
    }
    Mock::given(method("GET")).and(path(format!("/{V}/111/settings"))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"storage_configuration":{"status":"IN_COUNTRY_STORAGE_ENABLED","data_localization_region":"IN"}}))).expect(1).mount(&graph).await;
    paloma(Some(&graph), &home)
        .args(["number", "settings", "get", "-o", "raw"])
        .assert()
        .success()
        .stdout(predicate::str::contains("IN_COUNTRY_STORAGE_ENABLED"));
}

#[tokio::test(flavor = "multi_thread")]
async fn identity_and_settings_reject_bad_inputs_before_context_or_network() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for args in [
        vec!["number", "username", "set", "abc..def"],
        vec!["number", "username", "set", "12345"],
        vec!["number", "username", "set", "Acme.COM"],
        vec!["number", "contacts", "get", "US.ENT.123"],
        vec!["number", "contacts", "delete", "15551234567"],
        vec!["number", "settings", "set", "--storage-region", "XX"],
        vec!["number", "name", "set", " "],
        vec![
            "number",
            "add",
            "+15551234567",
            "--cc",
            "1",
            "--name",
            "Acme",
        ],
        vec![
            "number",
            "add",
            "15551234567",
            "--cc",
            "+1",
            "--name",
            "Acme",
        ],
        vec![
            "number",
            "verification",
            "request",
            "--method",
            "SMS",
            "--language",
            " ",
        ],
        vec!["commerce", "set"],
        vec!["automation", "bot", "get", "../bad"],
        vec!["flow", "migrate", "--source-waba-id", "../bad"],
        vec!["account", "users", "list", "--business-id", "../bad"],
    ] {
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .env_remove("PALOMA_WABA_ID")
            .args(&args)
            .assert()
            .code(5)
            .stdout("");
    }
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .env("PALOMA_VERIFICATION_CODE", "BAD_SECRET")
        .args(["number", "verification", "confirm"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("BAD_SECRET").not());
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["number", "settings", "set", "--yes"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains("--identity-check"));
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args([
            "number",
            "settings",
            "set",
            "--identity-check",
            "--no-storage",
            "--yes",
        ])
        .assert()
        .code(2)
        .stdout("");
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn oba_and_india_compliance_submit_validated_documents_and_confirm() {
    let home = TempDir::new().unwrap();
    let graph = MockServer::start().await;
    let file = home.path().join("document.json");
    for (group, verb, edge, body) in [
        (
            "oba",
            "apply",
            "official_business_account",
            json!({"business_website_url":"https://example.com","primary_country_of_operation":"US"}),
        ),
        (
            "compliance",
            "set",
            "business_compliance_info",
            json!({"messaging_product":"whatsapp","entity_name":"Acme","entity_type":"PRIVATE_COMPANY","grievance_officer_details":{"name":"Officer","email":"officer@example.com"},"customer_care_details":{"email":"care@example.com"}}),
        ),
    ] {
        std::fs::write(&file, body.to_string()).unwrap();
        Mock::given(method("POST"))
            .and(path(format!("/{V}/111/{edge}")))
            .and(body_json(body.clone()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
            .expect(1)
            .mount(&graph)
            .await;
        // OBA status is a field on the number; compliance is read from its edge.
        let (read_path, read_body, read_key) = if group == "oba" {
            (
                format!("/{V}/111"),
                json!({"official_business_account":{"oba_status":"NOT_STARTED"},"id":"111"}),
                "oba_status",
            )
        } else {
            (
                format!("/{V}/111/{edge}"),
                json!({"data":[body.clone()]}),
                "data",
            )
        };
        Mock::given(method("GET"))
            .and(path(read_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(read_body))
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args(["number", group, verb, "--file"])
            .arg(&file)
            .args(["--yes", "-q"])
            .assert()
            .success()
            .stdout("");
        paloma(Some(&graph), &home)
            .args(["number", group, verb, "--file"])
            .arg(&file)
            .assert()
            .failure()
            .stderr(predicate::str::contains("--yes"));
        paloma(Some(&graph), &home)
            .args(["number", group, verb, "--file"])
            .arg(&file)
            .args(["--dry-run", "-o", "json"])
            .assert()
            .success()
            .stdout(predicate::str::contains("POST"));
        paloma(Some(&graph), &home)
            .args(["number", group, "get", "-o", "json"])
            .assert()
            .success()
            .stdout(predicate::str::contains(read_key));
        std::fs::write(&file, "{}").unwrap();
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["number", group, verb, "--file"])
            .arg(&file)
            .assert()
            .code(5)
            .stdout("");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn compliance_validation_rejects_wrong_entity_contacts_and_bounds_before_a_call() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("compliance.json");
    let valid = json!({"entity_name":"Acme","entity_type":"PRIVATE_COMPANY","grievance_officer_details":{"name":"Officer","email":"officer@example.com"},"customer_care_details":{"email":"care@example.com"}});
    for (pointer, value) in [
        ("/entity_name", json!("A")),
        ("/entity_name", json!("a".repeat(129))),
        ("/entity_type_custom", json!("Other")),
        ("/is_registered", json!(true)),
        ("/customer_care_details/email", json!("invalid")),
        (
            "/grievance_officer_details/mobile_number",
            json!("US.12345"),
        ),
    ] {
        let mut body = valid.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        if parent.is_empty() {
            body[key] = value;
        } else {
            body.pointer_mut(parent).unwrap()[key] = value;
        }
        std::fs::write(&file, body.to_string()).unwrap();
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["number", "compliance", "set", "--file"])
            .arg(&file)
            .arg("--yes")
            .assert()
            .code(5)
            .stdout("");
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn contacts_delete_warns_when_meta_deleted_nothing() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("DELETE"))
        .and(path(format!("/{V}/111/contact_book")))
        .and(query_param("bsuid", "US.123"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"messaging_product":"whatsapp","success":true,"deleted":false}),
            ),
        )
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "number",
            "contacts",
            "delete",
            "US.123",
            "--yes",
            "--phone-number-id",
            "111",
            "-o",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"deleted\": false"))
        .stderr(predicate::str::contains(
            "no contact-book entry was deleted",
        ));
}

#[tokio::test]
async fn oba_leaves_the_country_format_to_meta_but_requires_a_value() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("oba.json");
    for (country, code) in [("United States", 0), ("  ", 5)] {
        std::fs::write(
            &file,
            json!({"business_website_url":"https://example.com","primary_country_of_operation":country})
                .to_string(),
        )
        .unwrap();
        paloma(Some(&graph), &home)
            .args(["number", "oba", "apply", "--dry-run", "--file"])
            .arg(&file)
            .assert()
            .code(code);
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn oba_get_reads_the_status_field_on_the_number() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/{V}/111")))
        .and(query_param("fields", "official_business_account"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"official_business_account":{"oba_status":"NOT_STARTED"},"id":"111"}),
        ))
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["number", "oba", "get", "-q"])
        .assert()
        .success()
        .stdout("NOT_STARTED\n");
}
