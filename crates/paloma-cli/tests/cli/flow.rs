//! `paloma flow` contract tests.

use crate::common::*;

#[tokio::test(flavor = "multi_thread")]
async fn flow_validation_errors_print_response_and_exit_5() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/{WABA}/flows")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "F1",
            "success": true,
            "validation_errors": [{ "error": "INVALID_PROPERTY_VALUE", "message": "bad", "line_start": 3, "column_start": 7 }]
        })))
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args([
            "flow",
            "create",
            "--name",
            "signup",
            "--category",
            "SIGN_UP",
            "-q",
        ])
        .assert()
        .code(5)
        .stdout("F1\n")
        .stderr(predicate::str::contains(
            "line 3:7 INVALID_PROPERTY_VALUE: bad",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn flow_migration_copies_named_flows() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("POST"))
        .and(path(format!("/{V}/222/migrate_flows")))
        .and(query_param("source_waba_id", "333"))
        .and(query_param("source_flow_names", "[\"booking\",\"survey\"]"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"migrated_flows":[{"source_id":"a","migrated_id":"b"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "flow",
            "migrate",
            "--source-waba-id",
            "333",
            "--name",
            "booking",
            "--name",
            "survey",
            "--yes",
            "-o",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("migrated_id"));
}

#[tokio::test(flavor = "multi_thread")]
async fn flow_key_reads_and_writes_public_pem_but_rejects_private_material_before_context() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("public.pem");
    let public = "-----BEGIN PUBLIC KEY-----\nTEST_PUBLIC_MATERIAL\n-----END PUBLIC KEY-----";
    std::fs::write(&file, public).unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/{V}/111/whatsapp_business_encryption")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"business_public_key_signature_status":"VALID"})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["flow", "key", "get", "-o", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("VALID"));
    Mock::given(method("POST"))
        .and(path(format!("/{V}/111/whatsapp_business_encryption")))
        .and(body_string_contains("name=\"business_public_key\""))
        .and(body_string_contains(public))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["flow", "key", "set", "--public-key"])
        .arg(&file)
        .args(["--yes", "-q"])
        .assert()
        .success()
        .stdout("");
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_TOKEN")
        .args(["flow", "key", "set", "--public-key"])
        .arg(&file)
        .arg("--dry-run")
        .assert()
        .success();
    std::fs::write(
        &file,
        "-----BEGIN PRIVATE KEY-----\nPRIVATE_SENTINEL\n-----END PRIVATE KEY-----",
    )
    .unwrap();
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["flow", "key", "set", "--public-key"])
        .arg(&file)
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("PRIVATE_SENTINEL").not());
}

#[tokio::test(flavor = "multi_thread")]
async fn flow_migration_partial_failure_preserves_results_and_exits_one() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let body = json!({"migrated_flows":[{"source_name":"one","migrated_id":"444"}],"failed_flows":[{"source_name":"two","error_code":"4233041","error_message":"already exists"}]});
    Mock::given(method("POST"))
        .and(path(format!("/{V}/222/migrate_flows")))
        .and(query_param("source_waba_id", "333"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
        .expect(2)
        .mount(&graph)
        .await;
    let out = paloma(Some(&graph), &home)
        .args([
            "flow",
            "migrate",
            "--source-waba-id",
            "333",
            "--yes",
            "-o",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout_json(&out), body);
    assert!(String::from_utf8_lossy(&out.stderr).contains("1 Flow migration(s) failed"));
    paloma(Some(&graph), &home)
        .args(["flow", "migrate", "--source-waba-id", "333", "--yes", "-q"])
        .assert()
        .code(1)
        .stdout("444\n");
}
