//! `paloma auth`, `config` and the token checks that need no phone number.

use crate::common::*;

fn debug_token_body(is_valid: bool) -> Value {
    json!({
        "data": {
            "app_id": "999",
            "type": "SYSTEM_USER",
            "application": "Lucky Shrub",
            "expires_at": 0,
            "is_valid": is_valid,
            "scopes": ["whatsapp_business_management", "whatsapp_business_messaging"]
        }
    })
}

async fn mount_debug_token(graph: &MockServer, response: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path(format!("/{V}/debug_token")))
        .and(query_param("input_token", TOKEN))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(response)
        .expect(1)
        .mount(graph)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_status_with_scopes_shows_the_token_without_a_phone_number() {
    let body = debug_token_body(true);
    let graph = MockServer::start().await;
    mount_debug_token(&graph, ResponseTemplate::new(200).set_body_json(&body)).await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .env_remove("PALOMA_WABA_ID")
        .args(["auth", "status", "--scopes", "-o", "json", "-vv"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(stdout_json(&out), body);
    assert!(!String::from_utf8_lossy(&out.stderr).contains(TOKEN));

    for (flags, expected) in [
        (vec!["-q"], "valid"),
        (vec!["-o", "table"], "whatsapp_business_messaging"),
    ] {
        let graph = MockServer::start().await;
        mount_debug_token(&graph, ResponseTemplate::new(200).set_body_json(&body)).await;
        paloma(Some(&graph), &home)
            .args(["auth", "status", "--scopes"])
            .args(&flags)
            .assert()
            .success()
            .stdout(predicate::str::contains(expected).and(predicate::str::contains(TOKEN).not()));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_status_with_scopes_exits_3_when_the_token_is_bad() {
    let responses = [
        ResponseTemplate::new(200).set_body_json(debug_token_body(false)),
        ResponseTemplate::new(401)
            .set_body_json(json!({ "error": { "message": "expired", "code": 190 } })),
    ];
    for response in responses {
        let graph = MockServer::start().await;
        mount_debug_token(&graph, response).await;
        let home = TempDir::new().unwrap();
        paloma(Some(&graph), &home)
            .args(["auth", "status", "--scopes"])
            .assert()
            .code(3)
            .stdout(predicate::str::is_empty());
    }
}

#[test]
fn auth_status_with_scopes_dry_run_needs_no_token_and_shows_none() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .env_remove("PALOMA_TOKEN")
        .args(["auth", "status", "--scopes", "--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let request = stdout_json(&out);
    assert_eq!(request["method"], "GET");
    let url = request["url"].as_str().unwrap();
    assert!(
        url.contains(&format!("/{V}/debug_token?input_token=")),
        "{url}"
    );
    assert!(!url.contains("active-token"), "{url}");
}

#[test]
fn config_set_get_use_list_round_trip() {
    let home = TempDir::new().unwrap();
    let run = |args: &[&str]| {
        paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(args)
            .assert()
    };
    run(&["-p", "work", "config", "set", "phone_number_id", "999"]).success();
    run(&["-p", "work", "config", "set", "token_ref", "EAAplain"])
        .success()
        .stderr(predicate::str::contains("plaintext"));
    run(&["config", "use", "work"]).success();
    run(&["config", "get", "phone_number_id", "-q"])
        .success()
        .stdout("999\n");
    run(&["config", "list"])
        .success()
        .stdout(predicate::str::contains("<plaintext:redacted>"));
    run(&["config", "use", "ghost"]).code(5);
    run(&["config", "get", "nonsense"]).code(2);
}
