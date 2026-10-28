//! `paloma qr` contract tests.

use crate::common::*;

#[tokio::test(flavor = "multi_thread")]
async fn qr_lifecycle_uses_collection_posts_and_scoped_get_delete() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let code = "4O4YGZEG3RIVE1";
    for (args, body) in [
        (
            vec!["create", "--message", "Hello"],
            json!({"prefilled_message":"Hello"}),
        ),
        (
            vec!["update", code, "--message", "Updated"],
            json!({"code":code,"prefilled_message":"Updated"}),
        ),
    ] {
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/message_qrdls")))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"code":code,"deep_link_url":"https://wa.me/message/example"}),
            ))
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .args(["qr"])
            .args(&args)
            .arg("-q")
            .assert()
            .success()
            .stdout(format!("{code}\n"))
            .stderr("");
    }
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}/message_qrdls/{code}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data":[{"code":code,"prefilled_message":"Hello"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["qr", "get", code, "-q"])
        .assert()
        .success()
        .stdout(format!("{code}\n"))
        .stderr("");
    Mock::given(method("DELETE"))
        .and(path(format!("/v26.0/{PHONE}/message_qrdls/{code}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["qr", "delete", code, "--yes", "-q"])
        .assert()
        .success()
        .stdout(format!("{code}\n"))
        .stderr("");
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}/message_qrdls")))
        .and(query_param("limit", "25"))
        .and(query_param("after", "cursor"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data":[{"code":code,"prefilled_message":"Hello"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["qr", "list", "--limit", "25", "--after", "cursor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Hello"))
        .stderr("");
}

#[tokio::test(flavor = "multi_thread")]
async fn qr_image_download_omits_authorization_and_preserves_graph_output() {
    let graph = MockServer::start().await;
    let cdn = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let code = "4O4YGZEG3RIVE1";
    let body = json!({"code":code,"qr_image_url":format!("{}/qr.svg",cdn.uri())});
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/message_qrdls")))
        .and(body_json(
            json!({"prefilled_message":"Hello","generate_qr_image":"SVG"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path("/qr.svg"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<svg/>"))
        .expect(1)
        .mount(&cdn)
        .await;
    let dest = home.path().join("qr.svg");
    let out = paloma(Some(&graph), &home)
        .args([
            "qr",
            "create",
            "--message",
            "Hello",
            "--image",
            "svg",
            "--out",
        ])
        .arg(&dest)
        .args(["-o", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(stdout_json(&out), body);
    assert!(String::from_utf8_lossy(&out.stderr).contains("wrote 6 bytes"));
    assert_eq!(std::fs::read(&dest).unwrap(), b"<svg/>");
    assert!(
        !cdn.received_requests().await.unwrap()[0]
            .headers
            .contains_key("authorization")
    );
    assert!(
        graph.received_requests().await.unwrap()[0]
            .headers
            .contains_key("authorization")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn qr_dry_run_prints_both_image_steps_and_invalid_input_never_sends() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let dest = home.path().join("qr.png");
    let out = paloma(Some(&graph), &home)
        .env_remove("PALOMA_TOKEN")
        .args(["qr", "get", "4O4YGZEG3RIVE1", "--image", "png", "--out"])
        .arg(&dest)
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let requests = stdout_json(&out);
    assert_eq!(requests.as_array().unwrap().len(), 2);
    assert!(
        requests[0]["url"]
            .as_str()
            .unwrap()
            .contains("qr_image_url.format%28PNG%29")
    );
    assert_eq!(requests[1]["method"], "GET");
    assert!(!dest.exists());
    for args in [
        vec!["get", "../bad"],
        vec!["create", "--message", &"x".repeat(141)],
        vec!["list", "--limit", "26"],
    ] {
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .args(["qr"])
            .args(&args)
            .assert()
            .code(5)
            .stdout("");
    }
    paloma(Some(&graph), &home)
        .args(["qr", "delete", "4O4YGZEG3RIVE1"])
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("--yes"));
    std::fs::write(&dest, b"existing").unwrap();
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["qr", "create", "--message", "x", "--image", "png", "--out"])
        .arg(&dest)
        .assert()
        .code(5)
        .stdout("");
    assert_eq!(std::fs::read(&dest).unwrap(), b"existing");
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn qr_image_refuses_a_file_created_while_the_graph_request_runs() {
    let graph = MockServer::start().await;
    let cdn = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let dest = home.path().join("race.svg");
    let race_dest = dest.clone();
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/message_qrdls")))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"code":"4O4YGZEG3RIVE1","qr_image_url":format!("{}/qr.svg",cdn.uri())}),
        ))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path("/qr.svg"))
        .respond_with(move |_: &wiremock::Request| {
            std::fs::write(&race_dest, b"KEEP").unwrap();
            ResponseTemplate::new(200).set_body_string("REPLACE")
        })
        .expect(1)
        .mount(&cdn)
        .await;
    paloma(Some(&graph), &home)
        .args(["qr", "create", "--message", "x", "--image", "svg", "--out"])
        .arg(&dest)
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("4O4YGZEG3RIVE1 was created"));
    assert_eq!(std::fs::read(dest).unwrap(), b"KEEP");
}

#[test]
fn qr_positional_message_and_default_image_are_previewed_offline() {
    let home = TempDir::new().unwrap();
    let out = paloma(None, &home)
        .args([
            "qr",
            "create",
            "Hello",
            "--image",
            "SVG",
            "--dry-run",
            "-o",
            "json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let requests = stdout_json(&out);
    assert_eq!(
        requests[0]["body"],
        json!({"prefilled_message":"Hello","generate_qr_image":"SVG"})
    );
    assert_eq!(requests.as_array().unwrap().len(), 2);
    paloma(None, &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["qr", "create", " "])
        .assert()
        .code(5)
        .stdout("");
}

#[tokio::test(flavor = "multi_thread")]
async fn qr_default_image_uses_a_safe_filename_without_sending_the_token() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/v26.0/{PHONE}/message_qrdls/{QR_CODE}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"data":[{"code":QR_CODE,"qr_image_url":format!("{}/image",graph.uri())}]}),
        ))
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path("/image"))
        .and(|request: &wiremock::Request| !request.headers.contains_key("authorization"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"image".to_vec()))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .current_dir(home.path())
        .args(["qr", "get", QR_CODE, "--image", "png", "-q"])
        .assert()
        .success()
        .stdout(format!("{QR_CODE}\n"));
    let saved = home.path().join(format!("{QR_CODE}.png"));
    assert_eq!(std::fs::read(&saved).unwrap(), b"image");
    paloma(Some(&graph), &home)
        .current_dir(home.path())
        .args(["qr", "get", QR_CODE, "--image", "png", "-q"])
        .assert()
        .code(5)
        .stdout("");
    assert_eq!(std::fs::read(saved).unwrap(), b"image");
}

#[tokio::test]
async fn qr_create_dry_run_marks_the_image_url_as_a_placeholder() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args([
            "qr",
            "create",
            "hello",
            "--image",
            "svg",
            "--dry-run",
            "-o",
            "json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let requests: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(requests[1]["url"], "<qr_image_url from the QR response>");
}

#[tokio::test]
async fn qr_create_says_the_code_exists_when_its_response_code_is_unusable() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("POST"))
        .and(path("/v26.0/111/message_qrdls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code":"ABC123","prefilled_message":"hello","qr_image_url":"https://example.com/a.svg"
        })))
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .current_dir(home.path())
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["qr", "create", "hello", "--image", "svg"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("QR code ABC123 was created"));
}
