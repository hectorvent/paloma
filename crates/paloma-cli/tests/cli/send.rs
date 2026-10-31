//! `paloma send`: message bodies, recipients, media uploads and the 24h window.

use crate::common::*;

#[tokio::test(flavor = "multi_thread")]
async fn send_text_posts_with_bearer_and_quiet_prints_wamid() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": "15551234567",
            "type": "text",
            "text": { "preview_url": false, "body": "hello" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messaging_product": "whatsapp",
            "contacts": [{ "input": "15551234567", "wa_id": "15551234567" }],
            "messages": [{ "id": "wamid.ABC" }]
        })))
        .expect(1)
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["send", "text", "15551234567", "hello", "-q"])
        .assert()
        .success()
        .stdout("wamid.ABC\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn messaging_account_flag_is_sent_on_the_message_and_quiet_still_prints_the_id() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product":"whatsapp", "recipient_type":"individual", "to":"15551234567",
            "type":"text", "text":{"preview_url":false, "body":"hi"},
            "messaging_account_id":"777"
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"messages":[{"id":"wamid.BILLED"}]})),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let out = paloma(Some(&graph), &home)
        .args(["send", "text", "15551234567", "hi"])
        .args(["--messaging-account-id", "777", "-q"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "wamid.BILLED\n");
}

#[test]
fn messaging_account_resolves_flag_then_env_then_profile() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .args(["config", "set", "messaging_account_id", "111"])
        .assert()
        .success();
    let account = |env: Option<&str>, flag: Option<&str>| {
        let mut cmd = paloma(None, &home);
        cmd.args([
            "send",
            "text",
            "15551234567",
            "hi",
            "--dry-run",
            "-o",
            "raw",
        ]);
        if let Some(value) = env {
            cmd.env("PALOMA_MESSAGING_ACCOUNT_ID", value);
        }
        if let Some(value) = flag {
            cmd.args(["--messaging-account-id", value]);
        }
        stdout_json(&cmd.output().unwrap())["messaging_account_id"].clone()
    };
    assert_eq!(account(None, None), json!("111"));
    assert_eq!(account(Some("222"), None), json!("222"));
    assert_eq!(account(Some("222"), Some("333")), json!("333"));
}

#[test]
fn messaging_account_is_absent_unless_set_and_only_goes_on_messages() {
    let home = TempDir::new().unwrap();
    let raw_body = |args: &[&str], stdin: Option<&str>| {
        let mut cmd = paloma(None, &home);
        cmd.args(args).args(["--dry-run", "-o", "raw"]);
        if let Some(input) = stdin {
            cmd.write_stdin(input);
        }
        stdout_json(&cmd.output().unwrap())
    };
    let text = raw_body(&["send", "text", "15551234567", "hi"], None);
    assert!(text.get("messaging_account_id").is_none());

    let account = ["--messaging-account-id", "777"];
    let read = raw_body(
        &[&["message", "read", "wamid.X"], &account[..]].concat(),
        None,
    );
    assert!(read.get("messaging_account_id").is_none());

    let raw_args = [&["send", "raw", "--stdin"], &account[..]].concat();
    let own = r#"{"to":"1","type":"text","text":{"body":"x"},"messaging_account_id":"999"}"#;
    assert_eq!(
        raw_body(&raw_args, Some(own))["messaging_account_id"],
        json!("999")
    );
    let unset = r#"{"to":"1","type":"text","text":{"body":"x"}}"#;
    assert_eq!(
        raw_body(&raw_args, Some(unset))["messaging_account_id"],
        json!("777")
    );
    let alias = r#"{"to":"1","type":"text","text":{"body":"x"},"paid_messaging_account_id":"999"}"#;
    let body = raw_body(&raw_args, Some(alias));
    assert!(body.get("messaging_account_id").is_none());
    assert_eq!(body["paid_messaging_account_id"], json!("999"));
    let status = r#"{"messaging_product":"whatsapp","status":"read","message_id":"wamid.X"}"#;
    assert!(
        raw_body(&raw_args, Some(status))
            .get("messaging_account_id")
            .is_none()
    );
}

#[test]
fn messaging_account_is_shown_on_the_media_message_in_a_file_dry_run() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("photo.jpg");
    std::fs::write(&file, b"jpeg").unwrap();
    let out = paloma(None, &home)
        .args(["send", "image", "15551234567", "--file"])
        .arg(&file)
        .args(["--messaging-account-id", "777", "--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let requests = stdout_json(&out);
    assert_eq!(requests[1]["body"]["messaging_account_id"], json!("777"));
    assert!(requests[0].to_string().contains("/media"));
    assert!(!requests[0].to_string().contains("messaging_account_id"));
}

#[test]
fn reply_to_quotes_the_message_on_every_send_type_that_allows_it() {
    let home = TempDir::new().unwrap();
    let contacts = home.path().join("contacts.json");
    std::fs::write(&contacts, r#"[{"name":{"formatted_name":"A"}}]"#).unwrap();
    let contacts = contacts.to_str().unwrap();
    let to = "15551234567";
    let cases: Vec<Vec<&str>> = vec![
        vec!["text", to, "hi"],
        vec![
            "template",
            "--via",
            "cloud",
            to,
            "order",
            "--language",
            "en_US",
        ],
        vec!["image", to, "--id", "9"],
        vec!["document", to, "--id", "9"],
        vec!["audio", to, "--id", "9"],
        vec!["video", to, "--id", "9"],
        vec!["sticker", to, "--id", "9"],
        vec!["location", to, "--lat", "1", "--long", "2"],
        vec!["contact", to, "--file", contacts],
        vec![
            "interactive",
            "button",
            to,
            "--body",
            "b",
            "--button",
            "y:Yes",
        ],
        vec![
            "interactive",
            "list",
            to,
            "--body",
            "b",
            "--button",
            "Open",
            "--section",
            "S:a:A",
        ],
        vec![
            "interactive",
            "cta-url",
            to,
            "--body",
            "b",
            "--display",
            "D",
            "--url",
            "https://x.test",
        ],
    ];
    for case in cases {
        let out = paloma(None, &home)
            .arg("send")
            .args(&case)
            .args(["--reply-to", "wamid.X", "--dry-run", "-o", "json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{case:?}");
        assert_eq!(
            stdout_json(&out)["body"]["context"],
            json!({ "message_id": "wamid.X" }),
            "{case:?}"
        );

        let out = paloma(None, &home)
            .arg("send")
            .args(&case)
            .args(["--dry-run", "-o", "json"])
            .output()
            .unwrap();
        assert!(
            stdout_json(&out)["body"].get("context").is_none(),
            "{case:?}"
        );
    }
}

#[test]
fn reply_to_is_refused_where_it_cannot_apply_or_is_blank() {
    let home = TempDir::new().unwrap();
    let to = "15551234567";
    // A blank id, such as an unset shell variable, must not send an unquoted message.
    let contacts = home.path().join("contacts.json");
    std::fs::write(&contacts, r#"[{"name":{"formatted_name":"A"}}]"#).unwrap();
    let blank: [&[&str]; 6] = [
        &["text", to, "hi"],
        &[
            "template",
            "--via",
            "cloud",
            to,
            "order",
            "--language",
            "en_US",
        ],
        &["image", to, "--id", "9"],
        &["location", to, "--lat", "1", "--long", "2"],
        &["contact", to, "--file", contacts.to_str().unwrap()],
        &[
            "interactive",
            "button",
            to,
            "--body",
            "b",
            "--button",
            "y:Yes",
        ],
    ];
    for case in blank {
        paloma(None, &home)
            .arg("send")
            .args(case)
            .args(["--reply-to", " ", "--dry-run"])
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("--reply-to"));
    }
    let unsupported: [&[&str]; 2] = [
        &["reaction", to, "--message-id", "wamid.Y", "--emoji", "👍"],
        &["raw", "--stdin"],
    ];
    for case in unsupported {
        paloma(None, &home)
            .arg("send")
            .args(case)
            .args(["--reply-to", "wamid.X", "--dry-run"])
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("--reply-to"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn blank_reply_to_fails_before_the_upload_and_before_the_context() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/media")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "MEDIA1" })))
        .expect(0)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("pic.png");
    std::fs::write(&file, "PNGDATA").unwrap();
    for without_phone in [false, true] {
        let mut cmd = paloma(Some(&graph), &home);
        if without_phone {
            cmd.env_remove("PALOMA_PHONE_NUMBER_ID");
        }
        cmd.args(["send", "image", "15551234567", "--reply-to", " ", "--file"])
            .arg(&file)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("--reply-to"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn reply_to_on_a_template_warns_on_stderr_and_still_sends() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": "15551234567",
            "type": "template",
            "context": { "message_id": "wamid.X" },
            "template": { "name": "order", "language": { "code": "en_US" } }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "messages": [{ "id": "wamid.ABC" }] })),
        )
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
            "15551234567",
            "order",
            "--language",
            "en_US",
        ])
        .args(["--reply-to", "wamid.X", "-q"])
        .assert()
        .success()
        .stdout("wamid.ABC\n")
        .stderr(predicate::str::contains("quoted message"));
}

#[tokio::test(flavor = "multi_thread")]
async fn voice_note_from_a_local_file_uploads_then_sends_with_voice_and_reply() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/media")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "MEDIA1" })))
        .expect(1)
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": "15551234567",
            "type": "audio",
            "context": { "message_id": "wamid.X" },
            "audio": { "id": "MEDIA1", "voice": true }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "messages": [{ "id": "wamid.ABC" }] })),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    let file = home.path().join("note.ogg");
    std::fs::write(&file, "OGGDATA").unwrap();
    paloma(Some(&graph), &home)
        .args([
            "send",
            "audio",
            "15551234567",
            "--voice",
            "--reply-to",
            "wamid.X",
            "-q",
        ])
        .arg("--file")
        .arg(&file)
        .assert()
        .success()
        .stdout("wamid.ABC\n");
}

#[test]
fn voice_dry_run_shows_the_flag_and_only_audio_accepts_it() {
    let home = TempDir::new().unwrap();
    let to = "15551234567";
    let out = paloma(None, &home)
        .args([
            "send",
            "audio",
            to,
            "--link",
            "https://x.test/a.ogg",
            "--voice",
        ])
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert_eq!(
        stdout_json(&out)["body"]["audio"],
        json!({ "link": "https://x.test/a.ogg", "voice": true })
    );

    // The local-file path prints its upload too, and the send must still carry both.
    let file = home.path().join("note.ogg");
    std::fs::write(&file, "OGGDATA").unwrap();
    let out = paloma(None, &home)
        .args(["send", "audio", to, "--voice", "--reply-to", "wamid.X"])
        .args(["--dry-run", "-o", "json", "--file"])
        .arg(&file)
        .output()
        .unwrap();
    let requests = stdout_json(&out);
    assert_eq!(requests.as_array().map(Vec::len), Some(2), "{requests}");
    assert!(
        requests[0]["url"].as_str().unwrap().ends_with("/media"),
        "{requests}"
    );
    assert_eq!(
        requests[1]["body"],
        json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": to,
            "type": "audio",
            "context": { "message_id": "wamid.X" },
            "audio": { "id": "<uploaded-at-send-time>", "voice": true }
        })
    );

    let out = paloma(None, &home)
        .args(["send", "audio", to, "--id", "9", "--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert_eq!(stdout_json(&out)["body"]["audio"], json!({ "id": "9" }));

    for kind in ["image", "document", "video", "sticker"] {
        paloma(None, &home)
            .args(["send", kind, to, "--id", "9", "--voice", "--dry-run"])
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("--voice"));
        paloma(None, &home)
            .args(["send", kind, "--help"])
            .assert()
            .success()
            .stdout(predicate::str::contains("--voice").not());
    }
    paloma(None, &home)
        .args(["send", "audio", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--voice"));
}

#[test]
fn send_file_checks_type_and_size_per_kind_before_context() {
    let home = TempDir::new().unwrap();
    let cases = [
        ("image", "a.jpg", 5 * MIB, true),
        ("image", "a.png", 5 * MIB + 1, false),
        ("image", "a.webp", 1, false),
        ("image", "a.pdf", 1, false),
        ("video", "a.mp4", 16 * MIB, true),
        ("video", "a.3gp", 16 * MIB + 1, false),
        ("video", "a.mov", 1, false),
        ("audio", "a.m4a", 16 * MIB, true),
        ("audio", "a.amr", 1, true),
        ("audio", "a.ogg", 16 * MIB + 1, false),
        ("audio", "a.wav", 1, false),
        ("sticker", "a.webp", 500 * 1024, true),
        ("sticker", "a.webp", 500 * 1024 + 1, false),
        ("sticker", "a.png", 1, false),
        ("document", "a.pdf", 100 * MIB, true),
        ("document", "a.docx", 100 * MIB + 1, false),
    ];
    for (kind, name, bytes, fits) in cases {
        let file = sized_file(&home, name, bytes);
        let mut cmd = paloma(None, &home);
        cmd.args(["send", kind, TO, "--file"]).arg(&file);
        if fits {
            cmd.args(["--dry-run", "-o", "json"]).assert().success();
        } else {
            cmd.env_remove("PALOMA_PHONE_NUMBER_ID")
                .assert()
                .code(5)
                .stdout(predicate::str::is_empty())
                .stderr(predicate::str::contains(kind));
        }
    }
}

#[test]
fn send_document_warns_about_a_type_meta_does_not_support_and_sends_it() {
    let home = TempDir::new().unwrap();
    let file = sized_file(&home, "a.zip", 1);
    let out = paloma(None, &home)
        .args(["send", "document", TO, "--dry-run", "-o", "json", "--file"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("warning: application/zip"), "{stderr}");
    assert_eq!(stdout_json(&out).as_array().map(Vec::len), Some(2));
}

#[test]
fn send_upload_guesses_the_type_meta_lists_for_m4a() {
    let home = TempDir::new().unwrap();
    let file = sized_file(&home, "a.m4a", 1);
    let out = paloma(None, &home)
        .args(["send", "audio", TO, "--dry-run", "-o", "json", "--file"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(stdout_json(&out)[0].to_string().contains("audio/mp4"));
}

/// A send under `--dry-run`, returning the body it would POST.
fn dry_run_send_body(home: &TempDir, args: &[&str]) -> Value {
    let out = paloma(None, home)
        .arg("send")
        .args(args)
        .args(["--dry-run", "-o", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let printed = stdout_json(&out);
    printed.get("body").cloned().unwrap_or(printed)
}

#[tokio::test(flavor = "multi_thread")]
async fn send_to_a_bsuid_posts_recipient_and_no_to() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "recipient": BSUID,
            "type": "text",
            "text": { "preview_url": false, "body": "hello" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messaging_product": "whatsapp",
            "contacts": [{ "input": BSUID, "user_id": BSUID }],
            "messages": [{ "id": "wamid.BSUID" }]
        })))
        .expect(1)
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["send", "text", BSUID, "hello", "-q"])
        .assert()
        .success()
        .stdout("wamid.BSUID\n")
        .stderr(predicate::str::is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_to_a_parent_bsuid_posts_recipient() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "recipient": PARENT_BSUID,
            "type": "text",
            "text": { "preview_url": false, "body": "hello" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "contacts": [{ "input": PARENT_BSUID, "user_id": PARENT_BSUID }],
            "messages": [{ "id": "wamid.PARENT" }]
        })))
        .expect(1)
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["send", "text", PARENT_BSUID, "hello", "-q"])
        .assert()
        .success()
        .stdout("wamid.PARENT\n");
}

#[test]
fn every_send_verb_addresses_a_bsuid_with_recipient() {
    let home = TempDir::new().unwrap();
    let contacts = home.path().join("contacts.json");
    std::fs::write(&contacts, r#"[{"name":{"formatted_name":"A"}}]"#).unwrap();
    let upload = home.path().join("photo.png");
    std::fs::write(&upload, b"png").unwrap();
    let contacts = contacts.to_str().unwrap();
    let upload = upload.to_str().unwrap();
    let cases: Vec<(&str, Vec<&str>)> = vec![
        ("text", vec!["text", BSUID, "hi"]),
        (
            "template",
            vec!["template", "--via", "cloud", BSUID, "t", "--language", "en"],
        ),
        ("image", vec!["image", BSUID, "--id", "9"]),
        ("document", vec!["document", BSUID, "--id", "9"]),
        ("audio", vec!["audio", BSUID, "--id", "9"]),
        ("video", vec!["video", BSUID, "--id", "9"]),
        ("sticker", vec!["sticker", BSUID, "--id", "9"]),
        (
            "location",
            vec!["location", BSUID, "--lat", "1", "--long", "2"],
        ),
        ("contacts", vec!["contact", BSUID, "--file", contacts]),
        (
            "reaction",
            vec!["reaction", BSUID, "--message-id", "wamid.X", "--emoji", "y"],
        ),
        (
            "interactive",
            vec![
                "interactive",
                "button",
                BSUID,
                "--body",
                "b",
                "--button",
                "Y",
            ],
        ),
        (
            "interactive",
            vec![
                "interactive",
                "list",
                BSUID,
                "--body",
                "b",
                "--button",
                "B",
                "--section",
                "S:a:A",
            ],
        ),
        (
            "interactive",
            vec![
                "interactive",
                "cta-url",
                BSUID,
                "--body",
                "b",
                "--display",
                "D",
                "--url",
                "https://x.test",
            ],
        ),
    ];
    for (kind, args) in &cases {
        let body = dry_run_send_body(&home, args);
        assert_eq!(body["recipient"], BSUID, "{args:?}: {body}");
        assert_eq!(body["recipient_type"], "individual", "{args:?}: {body}");
        assert_eq!(body["type"], *kind, "{args:?}: {body}");
        assert!(body.get("to").is_none(), "{args:?}: {body}");
    }

    // An upload prints two requests, and only the second addresses anyone.
    let out = paloma(None, &home)
        .args([
            "send",
            "image",
            BSUID,
            "--file",
            upload,
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
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(&format!("\"recipient\": \"{BSUID}\"")),
        "{stdout}"
    );
    assert!(!stdout.contains("\"to\":"), "{stdout}");
}

#[test]
fn a_phone_number_still_addresses_with_to() {
    let home = TempDir::new().unwrap();
    let body = dry_run_send_body(&home, &["text", "+15551234567", "hi"]);
    assert_eq!(body["to"], "+15551234567");
    assert!(body.get("recipient").is_none(), "{body}");
}

#[test]
fn a_malformed_recipient_exits_5_before_resolving_context() {
    let home = TempDir::new().unwrap();
    for bad in ["us.123", "US.", "US.ENT.", "USA.123", "US.12-34", "US123"] {
        // Without a sender, getting as far as the context would be a different error.
        paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .env("PALOMA_GRAPH_URL", "http://127.0.0.1:9")
            .args(["send", "text", bad, "hi"])
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(
                predicate::str::contains("E.164")
                    .and(predicate::str::contains("business-scoped user id")),
            );
    }
    let too_long = format!("US.{}", "1".repeat(129));
    paloma(None, &home)
        .args(["send", "text", &too_long, "hi", "--dry-run"])
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_keeps_its_own_addressing() {
    let payload = json!({
        "messaging_product": "whatsapp",
        "to": "15551234567",
        "recipient": BSUID,
        "type": "text",
        "text": { "body": "hi" }
    });
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(&payload))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "contacts": [{ "input": "15551234567", "wa_id": "15551234567" }],
            "messages": [{ "id": "wamid.RAW" }]
        })))
        .expect(1)
        .mount(&graph)
        .await;

    let home = TempDir::new().unwrap();
    let file = home.path().join("payload.json");
    std::fs::write(&file, payload.to_string()).unwrap();
    paloma(Some(&graph), &home)
        .args(["send", "raw", "--file", file.to_str().unwrap(), "-q"])
        .assert()
        .success()
        .stdout("wamid.RAW\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_send_response_without_wa_id_keeps_the_wamid_contract() {
    let responses = [
        json!({
            "messaging_product": "whatsapp",
            "contacts": [{ "input": BSUID, "user_id": BSUID }],
            "messages": [{ "id": "wamid.ONLY" }]
        }),
        json!({
            "messaging_product": "whatsapp",
            "contacts": [{ "user_id": BSUID }],
            "messages": [{ "id": "wamid.ONLY" }]
        }),
        json!({
            "messaging_product": "whatsapp",
            "contacts": [],
            "messages": [{ "id": "wamid.ONLY" }]
        }),
        json!({ "messages": [{ "id": "wamid.ONLY" }] }),
    ];
    for response in responses {
        let graph = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/messages")))
            .respond_with(ResponseTemplate::new(200).set_body_json(&response))
            .mount(&graph)
            .await;

        let home = TempDir::new().unwrap();
        paloma(Some(&graph), &home)
            .args(["send", "text", BSUID, "hi", "-q"])
            .assert()
            .success()
            .stdout("wamid.ONLY\n")
            .stderr(predicate::str::is_empty());
        paloma(Some(&graph), &home)
            .args(["send", "text", BSUID, "hi"])
            .assert()
            .success()
            .stdout(predicate::str::contains("wamid.ONLY"));
        let out = paloma(Some(&graph), &home)
            .args(["send", "text", BSUID, "hi", "-o", "json"])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(stdout_json(&out), response);
    }
}

#[tokio::test]
async fn send_contact_typed_flags_post_the_exact_contact() {
    let graph = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v26.0/{PHONE}/messages")))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": "15551234567",
            "type": "contacts",
            "contacts": [{
                "name": {
                    "formatted_name": "Ana Diaz",
                    "first_name": "Ana",
                    "last_name": "Diaz",
                },
                "phones": [
                    { "phone": "+16505559999", "wa_id": "16505559999" },
                    { "phone": "+16505550000" },
                ],
                "emails": [{ "email": "ana@example.com" }],
            }],
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "messages": [{ "id": "wamid.C" }] })),
        )
        .expect(1)
        .mount(&graph)
        .await;
    let home = TempDir::new().unwrap();
    paloma(Some(&graph), &home)
        .args(["send", "contact", "15551234567", "--name", "Ana Diaz"])
        .args(["--first-name", "Ana", "--last-name", "Diaz"])
        .args(["--phone", "+16505559999", "--wa-id", "16505559999"])
        .args(["--phone", "+16505550000", "--email", "ana@example.com"])
        .args(["-q", "--force"])
        .assert()
        .success()
        .stdout("wamid.C\n");
}

#[test]
fn send_contact_name_needs_a_first_or_last_name() {
    let home = TempDir::new().unwrap();
    paloma(None, &home)
        .env_remove("PALOMA_PHONE_NUMBER_ID")
        .args(["send", "contact", "15551234567", "--name", "Ana"])
        .assert()
        .code(5)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("--first-name"));
    let out = paloma(None, &home)
        .args(["send", "contact", "15551234567", "--name", "Ana"])
        .args(["--first-name", "Ana", "--force", "--dry-run", "-o", "raw"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        stdout_json(&out)["contacts"],
        json!([{ "name": { "formatted_name": "Ana", "first_name": "Ana" } }])
    );
}

#[test]
fn send_contact_needs_one_source_and_file_conflicts_with_the_typed_flags() {
    let home = TempDir::new().unwrap();
    let to = "15551234567";
    let cases: Vec<Vec<&str>> = vec![
        vec!["send", "contact", to],
        vec!["send", "contact", to, "--phone", "+1650"],
        vec![
            "send",
            "contact",
            to,
            "--name",
            "A",
            "--first-name",
            "A",
            "--wa-id",
            "1650",
        ],
        vec!["send", "contact", to, "--file", "c.json", "--name", "A"],
        vec!["send", "contact", to, "--file", "c.json", "--phone", "+1"],
        vec!["send", "contact", to, "--file", "c.json", "--email", "a@b"],
    ];
    for args in cases {
        paloma(None, &home)
            .env("PALOMA_GRAPH_URL", "http://127.0.0.1:9")
            .args(&args)
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty());
    }
}

#[test]
fn send_contact_blank_values_and_oversized_files_exit_5_before_any_request() {
    let home = TempDir::new().unwrap();
    let too_many = home.path().join("too-many.json");
    let contacts = vec![json!({ "name": { "formatted_name": "A" } }); 258];
    std::fs::write(&too_many, Value::Array(contacts).to_string()).unwrap();
    let to = "15551234567";
    let cases: Vec<Vec<&str>> = vec![
        vec!["--name", " "],
        vec!["--name", "A", "--first-name", " "],
        vec!["--name", "A", "--last-name", " "],
        vec!["--name", "A", "--phone", " "],
        vec!["--name", "A", "--phone", "+1", "--wa-id", " "],
        vec!["--name", "A", "--email", " "],
        vec!["--file", too_many.to_str().unwrap()],
    ];
    for args in cases {
        paloma(None, &home)
            .env_remove("PALOMA_TOKEN")
            .env("PALOMA_GRAPH_URL", "http://127.0.0.1:9")
            .args(["send", "contact", to, "--force"])
            .args(&args)
            .assert()
            .code(5)
            .stdout(predicate::str::is_empty());
    }
}

#[tokio::test]
async fn send_flow_sends_every_part_and_quiet_prints_the_wamid() {
    let graph = MockServer::start().await;
    let mut body = interactive_message(&json!({
        "type": "flow",
        "header": { "type": "text", "text": "Booking" },
        "body": { "text": "Book a table" },
        "footer": { "text": "Acme" },
        "action": {
            "name": "flow",
            "parameters": {
                "flow_message_version": "3",
                "flow_id": "123456",
                "flow_cta": "Book!",
                "flow_token": "order-7",
                "flow_action": "navigate",
                "mode": "draft",
                "flow_action_payload": {
                    "screen": "START",
                    "data": "{\"plan\":\"gold\"}"
                }
            }
        }
    }));
    body["context"] = json!({ "message_id": "wamid.Q" });
    expect_exact_send(&graph, body).await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "flow",
            TO,
            "--body",
            "Book a table",
            "--header",
            "Booking",
            "--footer",
            "Acme",
            "--flow-id",
            "123456",
            "--cta",
            "Book!",
            "--flow-token",
            "order-7",
            "--action",
            "navigate",
            "--mode",
            "draft",
            "--screen",
            "START",
            "--data",
            r#"{"plan":"gold"}"#,
            "--reply-to",
            "wamid.Q",
        ],
    );
}

#[tokio::test]
async fn send_flow_by_name_leaves_the_rest_to_meta() {
    let graph = MockServer::start().await;
    expect_exact_send(
        &graph,
        interactive_message(&json!({
            "type": "flow",
            "body": { "text": "Book" },
            "action": {
                "name": "flow",
                "parameters": {
                    "flow_message_version": "3",
                    "flow_name": "booking_v1",
                    "flow_cta": "Open"
                }
            }
        })),
    )
    .await;
    assert_quiet_send(
        &graph,
        &[
            "send",
            "flow",
            TO,
            "--body",
            "Book",
            "--flow-name",
            "booking_v1",
            "--cta",
            "Open",
        ],
    );
}

#[test]
fn send_flow_takes_data_from_a_file_and_a_media_header() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("data.json");
    std::fs::write(&file, r#"{"plan":"gold"}"#).unwrap();
    let body = sent_dry_run_body(&[
        "send",
        "flow",
        TO,
        "--body",
        "Book",
        "--flow-id",
        "1",
        "--cta",
        "Open",
        "--header-image",
        "2762",
        "--data-file",
        file.to_str().unwrap(),
    ]);
    assert_eq!(
        body["interactive"]["header"],
        json!({ "type": "image", "image": { "id": "2762" } })
    );
    assert_eq!(
        body["interactive"]["action"]["parameters"]["flow_action_payload"],
        json!({ "data": "{\"plan\":\"gold\"}" })
    );
}

#[test]
fn send_flow_dry_run_masks_the_flow_token_and_secrets_in_its_data() {
    let body = sent_dry_run_body(&[
        "send",
        "flow",
        TO,
        "--body",
        "Book",
        "--flow-id",
        "1",
        "--cta",
        "Open",
        "--flow-token",
        "TOKEN_SENTINEL",
        "--data",
        r#"{"api_token":"DATA_SENTINEL","u":"https://x.test/?access_token=LINK_SENTINEL","city":"SD"}"#,
    ]);
    let printed = body.to_string();
    for sentinel in ["TOKEN_SENTINEL", "DATA_SENTINEL", "LINK_SENTINEL"] {
        assert!(!printed.contains(sentinel), "{printed}");
    }
    assert!(printed.contains("SD"), "{printed}");
}

#[test]
fn send_flow_refuses_bad_values_before_any_context() {
    let base = [
        "send",
        "flow",
        TO,
        "--body",
        "Book",
        "--flow-id",
        "1",
        "--cta",
        "Open",
    ];
    for extra in [
        vec!["--data", "not json"],
        vec!["--data", "[1]"],
        vec!["--data", "{}"],
        vec!["--action", "data_exchange", "--screen", "START"],
        vec!["--action", "data_exchange", "--data", r#"{"a":1}"#],
        vec!["--flow-token", " "],
        vec!["--screen", " "],
        vec!["--reply-to", " "],
        vec!["--header-image", "ftp://x"],
    ] {
        let mut args = base.to_vec();
        args.extend(extra);
        assert_refused_locally(&args);
    }
    assert_refused_locally(&[
        "send",
        "flow",
        TO,
        "--body",
        " ",
        "--flow-id",
        "1",
        "--cta",
        "Open",
    ]);
    assert_refused_locally(&[
        "send",
        "flow",
        TO,
        "--body",
        "Book",
        "--flow-id",
        " ",
        "--cta",
        "Open",
    ]);
    assert_refused_locally(&[
        "send",
        "flow",
        TO,
        "--body",
        "Book",
        "--flow-id",
        "1",
        "--cta",
        " ",
    ]);
    assert_refused_locally(&[
        "send",
        "flow",
        "abc",
        "--body",
        "Book",
        "--flow-id",
        "1",
        "--cta",
        "Open",
    ]);
}

#[test]
fn send_flow_closed_sets_and_exclusive_flags_are_usage_errors() {
    let home = TempDir::new().unwrap();
    for args in [
        vec!["--body", "B", "--cta", "C"],
        vec![
            "--body",
            "B",
            "--cta",
            "C",
            "--flow-id",
            "1",
            "--flow-name",
            "n",
        ],
        vec![
            "--body",
            "B",
            "--cta",
            "C",
            "--flow-id",
            "1",
            "--action",
            "jump",
        ],
        vec![
            "--body",
            "B",
            "--cta",
            "C",
            "--flow-id",
            "1",
            "--mode",
            "live",
        ],
        vec![
            "--body",
            "B",
            "--cta",
            "C",
            "--flow-id",
            "1",
            "--data",
            "{}",
            "--data-file",
            "x",
        ],
        vec![
            "--body",
            "B",
            "--cta",
            "C",
            "--flow-id",
            "1",
            "--header",
            "H",
            "--header-image",
            "1",
        ],
    ] {
        paloma(None, &home)
            .args(["send", "flow", TO])
            .args(args)
            .assert()
            .code(2);
    }
}

#[test]
fn send_flow_warns_on_a_long_cta_but_sends_it() {
    let home = TempDir::new().unwrap();
    let cta = "x".repeat(31);
    paloma(None, &home)
        .args(["send", "flow", TO, "--body", "B", "--flow-id", "1"])
        .args(["--cta", cta.as_str(), "--dry-run", "-o", "json"])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning: --cta"));
}

#[test]
fn the_new_send_verbs_honor_the_window_guardrail() {
    let home = TempDir::new().unwrap();
    for args in [
        vec!["flow", TO, "--body", "B", "--flow-id", "1", "--cta", "C"],
        vec!["call-permission", TO],
        vec!["product", TO, "--catalog-id", "C", "--product", "S"],
        vec!["catalog", TO, "--body", "B"],
        vec!["interactive", "request-contact", TO, "--body", "B"],
        vec!["interactive", "location-request", TO, "--body", "B"],
        vec!["interactive", "call", TO, "--body", "B"],
        vec![
            "interactive",
            "address",
            TO,
            "--body",
            "B",
            "--country",
            "IN",
        ],
        vec![
            "interactive",
            "carousel",
            TO,
            "--body",
            "B",
            "--products",
            "a,b",
            "--catalog-id",
            "C",
        ],
        vec![
            "product-list",
            TO,
            "--catalog-id",
            "C",
            "--header",
            "H",
            "--body",
            "B",
            "--section",
            "A:s",
        ],
    ] {
        paloma(None, &home)
            .arg("send")
            .args(args)
            .args(["--strict", "--dry-run"])
            .assert()
            .code(6);
    }
}

#[test]
fn send_help_only_offers_flags_each_media_type_accepts() {
    let home = TempDir::new().unwrap();
    for (verb, filename, caption) in [
        ("document", true, true),
        ("image", false, true),
        ("video", false, true),
        ("audio", false, false),
        ("sticker", false, false),
    ] {
        let out = paloma(None, &home)
            .args(["send", verb, "--help"])
            .output()
            .unwrap();
        let help = String::from_utf8_lossy(&out.stdout);
        assert_eq!(help.contains("--filename"), filename, "{verb}");
        assert_eq!(help.contains("--caption"), caption, "{verb}");
    }
}
