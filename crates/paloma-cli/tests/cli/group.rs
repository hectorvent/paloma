//! `paloma group` contract tests.

use crate::common::*;

#[tokio::test]
async fn group_writes_send_exact_bodies() {
    let group_path = format!("/v26.0/{GROUP}");
    let invite = format!("{group_path}/invite_link");
    let members = format!("{group_path}/participants");
    let requests = format!("{group_path}/join_requests");
    for (args, verb, url, body) in [
        (
            vec![
                "group",
                "create",
                " Inquiries ",
                "--description",
                "About cars",
                "--join-approval",
                "approval-required",
            ],
            "POST",
            "/v26.0/111/groups".to_owned(),
            json!({"messaging_product":"whatsapp","subject":"Inquiries","description":"About cars","join_approval_mode":"approval_required"}),
        ),
        (
            vec!["group", "set", GROUP, "--subject", "Watches"],
            "POST",
            group_path.clone(),
            json!({"messaging_product":"whatsapp","subject":"Watches"}),
        ),
        (
            vec!["group", "invite", "reset", GROUP, "--yes"],
            "POST",
            invite.clone(),
            json!({"messaging_product":"whatsapp"}),
        ),
        (
            vec!["group", "members", "remove", GROUP, "15551234567", "--yes"],
            "DELETE",
            members.clone(),
            json!({"messaging_product":"whatsapp","participants":[{"user":"15551234567"}]}),
        ),
        (
            vec!["group", "requests", "approve", GROUP, "req1", "req2"],
            "POST",
            requests.clone(),
            json!({"messaging_product":"whatsapp","join_requests":["req1","req2"]}),
        ),
        (
            vec!["group", "requests", "reject", GROUP, "req1"],
            "DELETE",
            requests.clone(),
            json!({"messaging_product":"whatsapp","join_requests":["req1"]}),
        ),
        (
            vec!["group", "pin", GROUP, "wamid.1", "--days", "7"],
            "POST",
            "/v26.0/111/messages".to_owned(),
            json!({"messaging_product":"whatsapp","recipient_type":"group","to":GROUP,"type":"pin","pin":{"type":"pin","message_id":"wamid.1","expiration_days":7}}),
        ),
        (
            vec!["group", "unpin", GROUP, "wamid.1"],
            "POST",
            "/v26.0/111/messages".to_owned(),
            json!({"messaging_product":"whatsapp","recipient_type":"group","to":GROUP,"type":"pin","pin":{"type":"unpin","message_id":"wamid.1"}}),
        ),
    ] {
        let graph = MockServer::start().await;
        let home = TempDir::new().unwrap();
        Mock::given(method(verb))
            .and(path(url))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .env("PALOMA_PHONE_NUMBER_ID", "111")
            .args(&args)
            .assert()
            .success();
    }
}

#[tokio::test]
async fn group_create_prints_the_request_id_and_explains_the_webhook() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("POST"))
        .and(path("/v26.0/111/groups"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"messaging_product":"whatsapp","request_id":"req-9"})),
        )
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["group", "create", "Inquiries", "-q"])
        .assert()
        .success()
        .stdout("req-9\n");
}

#[tokio::test]
async fn group_list_reads_nested_groups_and_follows_cursors() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path("/v26.0/111/groups"))
        .and(query_param("after", "next-page"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"data":{"groups":[{"id":"b","subject":"Two","created_at":"2"}]}}),
        ))
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path("/v26.0/111/groups"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data":{"groups":[{"id":"a","subject":"One","created_at":"1"}]},
            "paging":{"cursors":{"after":"next-page"},"next":"https://example.com/next"},
        })))
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["group", "list", "--all", "-q"])
        .assert()
        .success()
        .stdout("a\nb\n");
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["group", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("SUBJECT").and(predicate::str::contains("One")))
        .stderr(predicate::str::contains("--after next-page"));
}

#[tokio::test]
async fn group_reads_use_their_documented_nodes() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{GROUP}")))
        .and(query_param("fields", "subject,description,suspended,creation_timestamp,total_participant_count,participants,join_approval_mode"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":GROUP,"subject":"One"})))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{GROUP}/invite_link")))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"messaging_product":"whatsapp","invite_link":"https://chat.whatsapp.com/abc"}),
        ))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{GROUP}/join_requests")))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"data":[{"join_request_id":"r1","wa_id":"1555","creation_timestamp":"1"}]}),
        ))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["group", "get", GROUP, "-q"])
        .assert()
        .success()
        .stdout(format!("{GROUP}\n"));
    paloma(Some(&graph), &home)
        .args(["group", "invite", "get", GROUP, "-q"])
        .assert()
        .success()
        .stdout("https://chat.whatsapp.com/abc\n");
    paloma(Some(&graph), &home)
        .args(["group", "requests", "list", GROUP])
        .assert()
        .success()
        .stdout(predicate::str::contains("JOIN REQUEST ID").and(predicate::str::contains("r1")));
}

#[tokio::test]
async fn group_requests_exit_1_when_meta_processes_only_some() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{GROUP}/join_requests")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messaging_product":"whatsapp",
            "approved_join_requests":["r1"],
            "failed_join_requests":[{"join_request_id":"r2","errors":[{"code":131_203}]}],
        })))
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "group", "requests", "approve", GROUP, "r1", "r2", "-o", "json",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("failed_join_requests"))
        .stderr(predicate::str::contains("couldn't complete every item"));
}

#[tokio::test]
async fn group_destructive_verbs_refuse_without_confirmation() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for args in [
        vec!["group", "delete", GROUP],
        vec!["group", "invite", "reset", GROUP],
        vec!["group", "members", "remove", GROUP, "15551234567"],
    ] {
        paloma(Some(&graph), &home)
            .args(&args)
            .assert()
            .code(5)
            .stdout("")
            .stderr(predicate::str::contains("--yes"));
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn group_rejects_bad_input_before_context_or_network() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let long_subject = "a".repeat(129);
    let not_a_jpeg = home.path().join("photo.png");
    std::fs::write(&not_a_jpeg, "x").unwrap();
    let not_a_jpeg = not_a_jpeg.to_str().unwrap().to_owned();
    for args in [
        vec!["group", "create", " "],
        vec!["group", "create", long_subject.as_str()],
        vec!["group", "get", "../x"],
        vec!["group", "set", GROUP],
        vec!["group", "set", GROUP, "--photo", not_a_jpeg.as_str()],
        vec!["group", "list", "--limit", "1025"],
        vec!["group", "pin", GROUP, "wamid.1", "--days", "31"],
        vec!["group", "pin", GROUP, "wamid.1", "--days", "0"],
        vec![
            "group", "members", "remove", GROUP, "1", "2", "3", "4", "5", "6", "7", "8", "9",
            "--yes",
        ],
    ] {
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(&args)
            .assert()
            .code(5)
            .stdout("");
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn group_set_uploads_a_photo_as_multipart() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let photo = home.path().join("square.jpg");
    std::fs::write(&photo, "jpeg-bytes").unwrap();
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{GROUP}")))
        .and(body_string_contains("name=\"file\""))
        .and(body_string_contains("jpeg-bytes"))
        .and(body_string_contains("name=\"messaging_product\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["group", "set", GROUP, "--photo"])
        .arg(&photo)
        .assert()
        .success();
}

#[tokio::test]
async fn sends_to_a_group_address_the_group() {
    for (args, content) in [
        (
            vec!["send", "text", GROUP, "Hello", "--group", "--strict"],
            json!({"type":"text","text":{"preview_url":false,"body":"Hello"}}),
        ),
        (
            vec!["send", "image", GROUP, "--id", "77", "--group"],
            json!({"type":"image","image":{"id":"77"}}),
        ),
        (
            vec![
                "send",
                "template",
                GROUP,
                "order",
                "--language",
                "en_US",
                "--group",
            ],
            json!({"type":"template","template":{"name":"order","language":{"code":"en_US"}}}),
        ),
    ] {
        let graph = MockServer::start().await;
        let home = TempDir::new().unwrap();
        let mut expected =
            json!({"messaging_product":"whatsapp","recipient_type":"group","to":GROUP});
        for (key, value) in content.as_object().unwrap() {
            expected[key] = value.clone();
        }
        Mock::given(method("POST"))
            .and(path("/v26.0/111/messages"))
            .and(body_json(expected))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.g"}]})),
            )
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .env("PALOMA_PHONE_NUMBER_ID", "111")
            .args(&args)
            .args(["-q"])
            .assert()
            .success()
            .stdout("wamid.g\n");
    }
}

#[tokio::test]
async fn group_sends_reject_bad_ids_and_the_marketing_route() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for args in [
        vec!["send", "text", "../x", "Hello", "--group"],
        vec!["send", "image", "../x", "--id", "77", "--group"],
        vec![
            "send",
            "template",
            GROUP,
            "order",
            "--language",
            "en_US",
            "--group",
            "--via",
            "marketing",
        ],
    ] {
        paloma(Some(&graph), &home)
            .args(&args)
            .assert()
            .code(5)
            .stdout("");
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn group_list_shows_none_for_an_empty_data_array() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path("/v26.0/111/groups"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[]})))
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["group", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(none)"));
}
