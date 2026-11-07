//! `paloma media` contract tests.

use crate::common::*;

#[tokio::test(flavor = "multi_thread")]
async fn media_upload_is_multipart() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/{PHONE}/media")))
        .and(body_string_contains("name=\"messaging_product\""))
        .and(body_string_contains("image/png"))
        .and(body_string_contains("PNGDATA"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "MEDIA1" })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload"])
        .arg(&file)
        .arg("-q")
        .assert()
        .success()
        .stdout("MEDIA1\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_opens_a_session_then_sends_the_raw_file() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/upload:S1")))
        .and(query_param("sig", "abc"))
        .and(header("authorization", format!("OAuth {TOKEN}").as_str()))
        .and(header("file_offset", "0"))
        .and(header("content-length", "7"))
        .and(header("content-type", "application/octet-stream"))
        .and(wiremock::matchers::body_string("PNGDATA"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    // No phone number id: a handle belongs to the app, not to a number.
    paloma(Some(&graph), &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["media", "upload", "--handle", "--app-id", "999", "-q"])
        .arg(&file)
        .assert()
        .success()
        .stdout("4::HANDLE\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_resumes_from_the_offset_meta_reports() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/upload:S1")))
        .and(header("file_offset", "0"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{V}/upload:S1")))
        .and(query_param("sig", "abc"))
        .and(header("authorization", format!("OAuth {TOKEN}").as_str()))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "id": "upload:S1", "file_offset": "3" })),
        )
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/upload:S1")))
        .and(header("file_offset", "3"))
        .and(header("content-length", "4"))
        .and(wiremock::matchers::body_string("DATA"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--app-id", "999", "-q"])
        .arg(&file)
        .assert()
        .success()
        .stdout("4::HANDLE\n")
        .stderr(predicate::str::contains("resuming at byte 3"));
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_gives_up_after_three_tries_with_the_server_error() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/upload:S1")))
        .respond_with(ResponseTemplate::new(503))
        .expect(3)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{V}/upload:S1")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "file_offset": "0" })))
        .expect(2)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--app-id", "999", "-q"])
        .arg(&file)
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_does_not_resume_after_an_auth_error_or_trust_a_bad_offset() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/upload:S1")))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": { "message": "bad token", "code": 190 }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--app-id", "999"])
        .arg(&file)
        .assert()
        .code(3)
        .stdout(predicate::str::is_empty());

    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/upload:S1")))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{V}/upload:S1")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "file_offset": "8" })))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--app-id", "999"])
        .arg(&file)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("unusable upload offset"));
}

#[test]
fn upload_handle_refuses_bad_files_before_any_call_and_dry_run_hides_the_token() {
    let home = TempDir::new().unwrap();
    let write = |name: &str, content: &str| {
        let file = home.path().join(name);
        std::fs::write(&file, content).unwrap();
        file
    };
    // No graph server and no app id: these must fail before looking for either.
    for file in [write("notes.txt", "hello"), write("empty.png", "")] {
        paloma(None, &home)
            .env_remove("PALOMA_WABA_ID")
            .args(["media", "upload", "--handle", "--dry-run"])
            .arg(&file)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty());
    }

    let out = paloma(None, &home)
        .args([
            "media",
            "upload",
            "--handle",
            "--app-id",
            "999",
            "--dry-run",
            "-o",
            "json",
        ])
        .arg(write("pic.png", "PNGDATA"))
        .output()
        .unwrap();
    assert!(out.status.success());
    let requests = stdout_json(&out);
    assert_eq!(requests.as_array().map(Vec::len), Some(2), "{requests}");
    assert!(
        requests[0]["url"]
            .as_str()
            .unwrap()
            .contains("/999/uploads?file_name=pic.png&file_length=7&file_type=image%2Fpng"),
        "{requests}"
    );
    assert_eq!(requests[1]["auth"], "OAuth <access-token>");
    assert_eq!(requests[1]["headers"], json!({ "file_offset": "0" }));
    assert_eq!(requests[1]["file"]["bytes"], 7);
    assert!(!requests.to_string().contains(TOKEN), "{requests}");
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_looks_up_the_app_when_none_is_set() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/{V}/{WABA}/subscribed_apps")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "whatsapp_business_api_data": { "id": "999" } }]
        })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--dry-run", "-o", "json"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        stdout_json(&out)[0]["url"]
            .as_str()
            .unwrap()
            .contains("/999/uploads"),
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("using app 999"));
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_warns_when_the_waba_has_several_apps_and_names_app_id_when_it_has_none() {
    let graph = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/{V}/{WABA}/subscribed_apps")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [
                { "whatsapp_business_api_data": { "id": "999" } },
                { "whatsapp_business_api_data": { "id": "888" } }
            ]
        })))
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--dry-run"])
        .arg(&file)
        .assert()
        .success()
        .stderr(predicate::str::contains("warning").and(predicate::str::contains("--app-id")));

    paloma(Some(&graph), &home)
        .env_remove("PALOMA_WABA_ID")
        .args(["media", "upload", "--handle", "--dry-run"])
        .arg(&file)
        .assert()
        // Missing input the user can supply, like any other missing flag.
        .code(5)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("--app-id"));
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_resumes_after_a_timeout() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/upload:S1")))
        .and(header("file_offset", "0"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(4))
                .set_body_json(json!({ "h": "4::TOO_LATE" })),
        )
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{V}/upload:S1")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "file_offset": "3" })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/upload:S1")))
        .and(header("file_offset", "3"))
        .and(wiremock::matchers::body_string("DATA"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args([
            "media",
            "upload",
            "--handle",
            "--app-id",
            "999",
            "-q",
            "--timeout",
            "1s",
        ])
        .arg(&file)
        .assert()
        .success()
        .stdout("4::HANDLE\n")
        .stderr(predicate::str::contains("resuming at byte 3"));
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_says_so_when_meta_has_the_file_but_gave_no_handle() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/upload:S1")))
        .and(header("file_offset", "0"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{V}/upload:S1")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "file_offset": 7 })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/upload:S1")))
        .and(header("file_offset", "7"))
        .and(header("content-length", "0"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": { "message": "bad offset", "code": 100 }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--app-id", "999"])
        .arg(&file)
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("whole file"));
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_keeps_the_transfer_failure_visible_when_the_offset_check_fails() {
    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/upload:S1")))
        .respond_with(ResponseTemplate::new(503).set_body_json(json!({
            "error": { "message": "transfer broke", "code": 2 }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{V}/upload:S1")))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": { "message": "session gone", "code": 100 }
        })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--app-id", "999"])
        .arg(&file)
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(
            predicate::str::contains("transfer broke")
                .and(predicate::str::contains("session gone")),
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_output_formats_pass_the_response_through_and_empty_handles_fail() {
    for (format, expected) in [
        ("json", "\"h\""),
        ("raw", "{\"h\":\"4::HANDLE\"}"),
        ("table", "4::HANDLE"),
    ] {
        let graph = MockServer::start().await;
        mount_upload_session(&graph).await;
        Mock::given(method("POST"))
            .and(path(format!("/{V}/upload:S1")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
            .mount(&graph)
            .await;
        let home = TempDir::new().unwrap();
        let file = home.path().join("pic.png");
        std::fs::write(&file, "PNGDATA").unwrap();
        paloma(Some(&graph), &home)
            .args([
                "media", "upload", "--handle", "--app-id", "999", "-o", format,
            ])
            .arg(&file)
            .assert()
            .success()
            .stdout(predicate::str::contains(expected));
    }

    let graph = MockServer::start().await;
    mount_upload_session(&graph).await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/upload:S1")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "" })))
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--app-id", "999", "-q"])
        .arg(&file)
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_stops_when_the_file_changes_after_the_session_opens() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/999/uploads")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(2))
                .set_body_json(json!({ "id": "upload:S1" })),
        )
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/upload:S1")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    let out = std::thread::scope(|scope| {
        let running = scope.spawn(|| {
            paloma(Some(&graph), &home)
                .args(["media", "upload", "--handle", "--app-id", "999", "-q"])
                .arg(&file)
                .output()
                .unwrap()
        });
        std::thread::sleep(std::time::Duration::from_millis(700));
        std::fs::write(&file, "CHANGED DATA").unwrap();
        running.join().unwrap()
    });
    assert_eq!(out.status.code(), Some(5));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("changed during the upload"));
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_handle_keeps_the_session_signature_and_token_out_of_verbose_logs() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/999/uploads")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "id": "upload:S1?sig=SIGSECRET" })),
        )
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/upload:S1")))
        .and(query_param("sig", "SIGSECRET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "h": "4::HANDLE" })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["media", "upload", "--handle", "--app-id", "999", "-vv"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("upload:S1"),
        "the log must really run: {stderr}"
    );
    assert!(!stderr.contains("SIGSECRET"), "{stderr}");
    assert!(!stderr.contains(TOKEN), "{stderr}");
}

#[cfg(unix)]
#[test]
fn upload_handle_refuses_an_unreadable_file_before_any_call() {
    use std::os::unix::fs::PermissionsExt;

    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::File::open(&file).is_ok() {
        return; // root reads anything, so there's nothing to check.
    }
    paloma(None, &home)
        .args(["media", "upload", "--handle", "--app-id", "999"])
        .arg(&file)
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty());
}

#[test]
fn media_upload_checks_the_type_and_its_size_before_context() {
    let home = TempDir::new().unwrap();
    let cases: [(&str, u64, Option<&str>, bool); 8] = [
        ("a.jpg", 5 * MIB, None, true),
        ("a.jpg", 5 * MIB + 1, None, false),
        ("a.mp4", 16 * MIB + 1, None, false),
        ("a.webp", 500 * 1024 + 1, None, false),
        ("a.gif", 1, None, false),
        // The flag wins over the extension, both ways.
        ("a.jpg", 5 * MIB + 1, Some("video/mp4"), true),
        ("a.bin", 1, Some("image/png"), true),
        ("a.png", 1, Some("image/gif"), false),
    ];
    for (name, bytes, mime, fits) in cases {
        let file = sized_file(&home, name, bytes);
        let mut cmd = paloma(None, &home);
        cmd.args(["media", "upload"]).arg(&file);
        if let Some(mime) = mime {
            cmd.args(["--type", mime]);
        }
        if fits {
            cmd.arg("--dry-run")
                .assert()
                .success()
                .stderr(predicate::str::contains("warning:").not());
        } else {
            cmd.env_remove("PALOMA_PHONE_NUMBER_ID")
                .assert()
                .code(5)
                .stdout(predicate::str::is_empty());
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn media_upload_warns_about_an_unlisted_type_and_still_uploads() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/{PHONE}/media")))
        .and(body_string_contains("application/zip"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "MEDIA1" })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = sized_file(&home, "a.zip", 4);
    paloma(Some(&graph), &home)
        .args(["media", "upload", "-q"])
        .arg(&file)
        .assert()
        .success()
        .stdout("MEDIA1\n")
        .stderr(predicate::str::contains("warning: application/zip"));
}

#[test]
fn media_upload_keeps_type_parameters_in_the_request() {
    let home = TempDir::new().unwrap();
    let file = sized_file(&home, "a.ogg", 1);
    let out = paloma(None, &home)
        .args(["media", "upload", "--dry-run", "-o", "json"])
        .args(["--type", "audio/ogg; codecs=opus"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        stdout_json(&out)
            .to_string()
            .contains("audio/ogg; codecs=opus")
    );
}

#[test]
fn media_upload_handle_keeps_its_own_type_list() {
    let home = TempDir::new().unwrap();
    // Over the 5 MB message limit, which doesn't apply to a resumable upload.
    let file = sized_file(&home, "a.png", 5 * MIB + 1);
    paloma(None, &home)
        .args([
            "media",
            "upload",
            "--handle",
            "--app-id",
            "999",
            "--dry-run",
        ])
        .arg(&file)
        .assert()
        .success();
}

#[tokio::test]
async fn media_upload_marks_a_voicemail_announcement() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/{V}/{PHONE}/media")))
        .and(body_string_contains("name=\"use_case\""))
        .and(body_string_contains("call_voicemail_announcement"))
        .and(body_string_contains("OGGDATA"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "MEDIA1" })))
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let greeting = home.path().join("greeting.ogg");
    std::fs::write(&greeting, "OGGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--voicemail-announcement", "-q"])
        .arg(&greeting)
        .assert()
        .success()
        .stdout("MEDIA1\n");
    let mp3 = home.path().join("greeting.mp3");
    std::fs::write(&mp3, "MP3DATA").unwrap();
    paloma(Some(&graph), &home)
        .args(["media", "upload", "--voicemail-announcement"])
        .arg(&mp3)
        .assert()
        .code(5)
        .stderr(predicate::str::contains("audio/ogg"));
}
