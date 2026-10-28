//! `paloma profile`, `commerce` and `automation`: business-facing settings.

use crate::common::*;

#[tokio::test(flavor = "multi_thread")]
async fn profile_photo_uploads_then_sets_the_handle() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .and(wiremock::matchers::body_string("PNGDATA"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/whatsapp_business_profile")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "profile_picture_handle": "4::HANDLE"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["profile", "photo", "--app-id", "999", "-o", "json"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(stdout_json(&out), json!({ "success": true }));
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_photo_does_not_update_the_profile_when_the_upload_fails() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": { "message": "bad token", "code": 190 }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/whatsapp_business_profile")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["profile", "photo", "--app-id", "999"])
        .arg(&file)
        .assert()
        .code(3)
        .stdout(predicate::str::is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_photo_explains_a_failed_update_after_a_good_upload() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path("/v26.0/upload:S1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/whatsapp_business_profile")))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": { "message": "handle is invalid", "code": 100 }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["profile", "photo", "--app-id", "999"])
        .arg(&file)
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(
            predicate::str::contains("handle is invalid")
                .and(predicate::str::contains("profile get")),
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_photo_quiet_prints_nothing_and_raw_prints_the_update_response() {
    let cases: [(&[&str], &str); 2] = [(&["-q"], ""), (&["-o", "raw"], "{\"success\":true}")];
    for (flags, expected) in cases {
        let graph = MockServer::start().await;
        mount_upload_session(&graph).await;
        Mock::given(method("POST"))
            .and(path("/v26.0/upload:S1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
            .mount(&graph)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/whatsapp_business_profile")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
            .mount(&graph)
            .await;
        let home = TempDir::new().unwrap();
        let file = home.path().join("pic.png");
        std::fs::write(&file, "PNGDATA").unwrap();
        let out = paloma(Some(&graph), &home)
            .args(["profile", "photo", "--app-id", "999"])
            .args(flags)
            .arg(&file)
            .output()
            .unwrap();
        assert!(out.status.success(), "{flags:?}");
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), expected);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_photo_checks_the_file_then_the_phone_before_any_call() {
    // Any request reaching this server fails the assertion at the end.
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let write = |name: &str| {
        let file = home.path().join(name);
        std::fs::write(&file, "DATA").unwrap();
        file
    };
    for file in [write("menu.pdf"), write("promo.mp4"), write("notes.txt")] {
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_WABA_ID")
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["profile", "photo"])
            .arg(&file)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(
                "it takes image/jpeg, image/jpg, image/png
",
            ));
    }
    let png = write("pic.png");
    paloma(Some(&graph), &home)
        .args(["profile", "photo", "--type", "application/pdf"])
        .arg(&png)
        .assert()
        .code(5);
    // The phone check comes before the app lookup, which would otherwise spend a read.
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["profile", "photo"])
        .arg(&png)
        .assert()
        .code(3)
        .stderr(predicate::str::contains("phone number id"));
    assert_eq!(graph.received_requests().await.unwrap().len(), 0);
}

#[test]
fn profile_photo_dry_run_prints_every_step_and_honors_the_type_flag() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("logo.bin");
    std::fs::write(&file, "DATA").unwrap();
    let out = paloma(None, &home)
        .args(["profile", "photo", "--app-id", "999", "--type", "image/png"])
        .args(["--dry-run", "-o", "json"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success());
    let requests = stdout_json(&out);
    assert_eq!(requests.as_array().map(Vec::len), Some(3), "{requests}");
    assert!(
        requests[0]["url"]
            .as_str()
            .unwrap()
            .contains("/999/uploads?file_name=logo.bin&file_length=4&file_type=image%2Fpng"),
        "{requests}"
    );
    assert_eq!(requests[1]["file"]["bytes"], 4);
    assert_eq!(
        requests[2]["body"],
        json!({
            "messaging_product": "whatsapp",
            "profile_picture_handle": "<handle-from-the-upload>"
        })
    );
    assert!(!requests.to_string().contains(TOKEN), "{requests}");
}

// Business profile checks.

#[tokio::test(flavor = "multi_thread")]
async fn profile_set_sends_fields_at_their_limits() {
    let graph = MockServer::start().await;
    let (about, address) = ("a".repeat(139), "b".repeat(256));
    let (description, email) = ("c".repeat(512), "d".repeat(128));
    let website = "e".repeat(256);
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/whatsapp_business_profile")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "about": about,
            "address": address,
            "description": description,
            "email": email,
            "vertical": "PROF_SERVICES",
            "websites": [website, "https://x.test"]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["profile", "set", "--vertical", "prof_services"])
        .args(["--about", &about, "--address", &address])
        .args(["--description", &description, "--email", &email])
        .args(["--website", &website, "--website", "https://x.test"])
        .assert()
        .success();
}

#[test]
fn profile_set_refuses_values_over_the_limits_before_context() {
    let home = TempDir::new().unwrap();
    let cases = [
        ("--about", String::new()),
        ("--about", "a".repeat(140)),
        ("--address", "a".repeat(257)),
        ("--description", "a".repeat(513)),
        ("--email", "a".repeat(129)),
        ("--website", "a".repeat(257)),
    ];
    for (flag, value) in cases {
        paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["profile", "set", flag, &value])
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(flag));
    }
}

#[test]
fn profile_set_refuses_a_third_website() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["profile", "set", "--website", "a", "--website", "b"])
        .args(["--website", "c"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("at most 2"));
}

#[test]
fn profile_set_vertical_is_a_closed_set() {
    let home = TempDir::new().unwrap();
    for refused in ["FOOD", "UNDEFINED", "NOT_A_BIZ"] {
        paloma(None, &home)
            .args(["profile", "set", "--vertical", refused])
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty());
    }
    let out = paloma(None, &home)
        .args(["profile", "set", "--vertical", "MATRIMONY_SERVICE"])
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        stdout_json(&out)["body"]["vertical"],
        json!("MATRIMONY_SERVICE")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn automation_reads_and_writes_typed_and_cleared_components() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}")))
        .and(query_param("fields", "conversational_automation"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"id":PHONE,"conversational_automation":{"prompts":["Support"]}}),
            ),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["automation", "get", "-q"])
        .assert()
        .success()
        .stdout(format!("{PHONE}\n"))
        .stderr("");
    for (args, body) in [
        (
            vec![
                "--welcome",
                "false",
                "--prompt",
                "Support",
                "--command",
                "help:Get help: now",
            ],
            json!({"enable_welcome_message":false,"prompts":["Support"],"commands":[{"command_name":"help","command_description":"Get help: now"}]}),
        ),
        (
            vec!["--clear-prompts", "--clear-commands"],
            json!({"prompts":[],"commands":[]}),
        ),
    ] {
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/conversational_automation")))
            .and(body_json(&body))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args(["automation", "set"])
            .args(&args)
            .arg("-q")
            .assert()
            .success()
            .stdout("")
            .stderr("");
        let out = paloma(Some(&graph), &home)
            .env_remove("PALOMA_TOKEN")
            .args(["automation", "set"])
            .args(&args)
            .args(["--dry-run", "-o", "json"])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(stdout_json(&out)["body"], body);
    }
}

#[test]
fn automation_validates_typed_and_file_settings_before_context() {
    let home = TempDir::new().unwrap();
    for args in [
        vec![],
        vec!["--command", "bad"],
        vec!["--command", "/help:Hint"],
        vec!["--prompt", &"x".repeat(81)],
        vec![
            "--prompt", "a", "--prompt", "b", "--prompt", "c", "--prompt", "d", "--prompt", "e",
        ],
    ] {
        paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["automation", "set"])
            .args(&args)
            .assert()
            .code(5)
            .stdout("");
    }
    let file = home.path().join("automation.json");
    for body in [
        json!([]),
        json!({}),
        json!({"prompts":[1]}),
        json!({"commands":[{"command_name":"x"}]}),
        json!({"enable_welcome_message":"true"}),
    ] {
        std::fs::write(&file, body.to_string()).unwrap();
        paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["automation", "set", "--file"])
            .arg(&file)
            .assert()
            .code(5)
            .stdout("");
    }
}

#[test]
fn automation_rejects_duplicate_commands_and_accepts_source_aliases() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args([
            "automation",
            "set",
            "--command",
            "help:First",
            "--command",
            "help=Second",
        ])
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("duplicate command"));
    let out = paloma(None, &home)
        .args([
            "automation",
            "set",
            "--welcome-message",
            "false",
            "--ice-breaker",
            "Hello",
            "--command",
            "help=Talk: support",
            "--dry-run",
            "-o",
            "json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        stdout_json(&out)["body"],
        json!({"enable_welcome_message":false,"prompts":["Hello"],"commands":[{"command_name":"help","command_description":"Talk: support"}]})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn commerce_uses_query_flags_including_false_and_preserves_list_output() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let body = json!({"data":[{"id":"333","is_cart_enabled":true,"is_catalog_visible":false}]});
    Mock::given(method("GET"))
        .and(path("/v26.0/111/whatsapp_commerce_settings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
        .expect(2)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["commerce", "get", "-q"])
        .assert()
        .success()
        .stdout("333\n");
    let out = paloma(Some(&graph), &home)
        .args(["commerce", "get", "-o", "json"])
        .output()
        .unwrap();
    assert_eq!(stdout_json(&out), body);
    Mock::given(method("POST"))
        .and(path("/v26.0/111/whatsapp_commerce_settings"))
        .and(query_param("is_cart_enabled", "false"))
        .and(query_param("is_catalog_visible", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "commerce",
            "set",
            "--cart-enabled",
            "false",
            "--catalog-visible",
            "true",
            "--yes",
            "-q",
        ])
        .assert()
        .success()
        .stdout("");
    assert_eq!(
        dry_run_request(&home, &["commerce", "set", "--cart-enabled", "false"])["url"],
        format!("{GRAPH}/{PHONE}/whatsapp_commerce_settings?is_cart_enabled=false")
    );
}
