//! `paloma calling` contract tests.

use crate::common::*;

#[tokio::test]
async fn calling_hours_set_sends_the_whole_schedule_and_confirms() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("POST"))
        .and(path("/v26.0/111/settings"))
        .and(body_json(
            json!({"messaging_product":"whatsapp","calling":{"call_hours":{
                "status":"ENABLED","timezone_id":"America/Santo_Domingo",
                "weekly_operating_hours":[
                    {"day_of_week":"MONDAY","open_time":"0900","close_time":"1700"},
                    {"day_of_week":"TUESDAY","open_time":"1000","close_time":"1200"},
                ],
                "holiday_schedule":[{"date":"2026-12-25","start_time":"0000","end_time":"2359"}],
            }}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    let args = [
        "calling",
        "hours",
        "set",
        "--timezone",
        "America/Santo_Domingo",
        "--open",
        "tuesday=1000-1200",
        "--open",
        "MONDAY=0900-1700",
        "--holiday",
        "2026-12-25=0000-2359",
    ];
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(args)
        .assert()
        .code(5)
        .stderr(predicate::str::contains("--yes"));
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(args)
        .arg("--yes")
        .assert()
        .success();
}

#[tokio::test]
async fn calling_hours_disable_keeps_the_saved_schedule() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let schedule = json!({"timezone_id":"Asia/Singapore",
        "weekly_operating_hours":[{"day_of_week":"MONDAY","open_time":"0900","close_time":"1700"}],
        "holiday_schedule":[{"date":"2026-12-25","start_time":"0000","end_time":"2359"}]});
    let mut enabled = schedule.clone();
    enabled["status"] = json!("ENABLED");
    let mut disabled = schedule;
    disabled["status"] = json!("DISABLED");
    Mock::given(method("GET"))
        .and(path("/v26.0/111/settings"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"calling":{"call_hours":enabled}})),
        )
        .mount(&graph)
        .await;
    Mock::given(method("POST"))
        .and(path("/v26.0/111/settings"))
        .and(body_json(
            json!({"messaging_product":"whatsapp","calling":{"call_hours":disabled}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["calling", "hours", "get", "-q"])
        .assert()
        .success()
        .stdout("ENABLED\n");
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["calling", "hours", "disable", "--yes"])
        .assert()
        .success();
}

#[tokio::test]
async fn calling_hours_disable_writes_nothing_without_a_schedule() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path("/v26.0/111/settings"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"calling":{"status":"ENABLED"}})),
        )
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .env("PALOMA_PHONE_NUMBER_ID", "111")
        .args(["calling", "hours", "disable"])
        .assert()
        .success()
        .stderr(predicate::str::contains("aren't set"));
    assert_eq!(
        subscription_writes(&graph.received_requests().await.unwrap()),
        0
    );
}

#[tokio::test]
async fn calling_permission_addresses_phones_and_bsuids() {
    for (user, param, value) in [
        ("+15551234567", "user_wa_id", "15551234567"),
        ("US.1349", "recipient", "US.1349"),
    ] {
        let graph = MockServer::start().await;
        let home = TempDir::new().unwrap();
        Mock::given(method("GET"))
            .and(path("/v26.0/111/call_permissions"))
            .and(query_param(param, value))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "messaging_product":"whatsapp",
                "permission":{"status":"no_permission"},
                "actions":[],
            })))
            .expect(1)
            .mount(&graph)
            .await;
        paloma(Some(&graph), &home)
            .env("PALOMA_PHONE_NUMBER_ID", "111")
            .args(["calling", "permission", user, "-q"])
            .assert()
            .success()
            .stdout("no_permission\n");
    }
}

#[tokio::test]
async fn calling_analytics_builds_the_field_expansion() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path("/v26.0/222"))
        .and(query_param(
            "fields",
            r#"call_analytics.start(1759276800).end(1761955200).granularity(MONTHLY).country_codes(["DO"]).directions(["USER_INITIATED"]).dimensions(["country"]).metric_types(["COUNT","AVERAGE_DURATION"])"#,
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "call_analytics":{"granularity":"MONTHLY","data_points":[
                {"start":1,"end":2,"count":35,"cost":0.47,"average_duration":106}]},
            "id":"222",
        })))
        .expect(1)
        .mount(&graph)
        .await;
    paloma(Some(&graph), &home)
        .args([
            "calling",
            "analytics",
            "--start",
            "2025-10-01",
            "--end",
            "2025-11-01",
            "--granularity",
            "month",
            "--country",
            "DO",
            "--direction",
            "user-initiated",
            "--dimension",
            "country",
            "--metric",
            "count,average-duration",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("AVERAGE DURATION").and(predicate::str::contains("106")));
}

#[tokio::test]
async fn calling_reads_and_hours_reject_bad_input_before_context_or_network() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for args in [
        vec!["calling", "permission", "not-a-number"],
        vec!["calling", "analytics", "--start", "nope", "--end", "2"],
        vec!["calling", "analytics", "--start", "5", "--end", "2"],
        vec![
            "calling",
            "analytics",
            "--start",
            "1",
            "--end",
            "2",
            "--country",
            "do",
        ],
        vec![
            "calling",
            "hours",
            "set",
            "--timezone",
            " ",
            "--open",
            "MONDAY=0900-1700",
            "--yes",
        ],
        vec![
            "calling",
            "hours",
            "set",
            "--timezone",
            "UTC",
            "--open",
            "MONDAY=1700-0900",
            "--yes",
        ],
        vec![
            "calling",
            "hours",
            "set",
            "--timezone",
            "UTC",
            "--open",
            "MONDAY=0900-1700",
            "--holiday",
            "2026-13-01=0000-2359",
            "--yes",
        ],
    ] {
        paloma(Some(&graph), &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .env_remove("PALOMA_WABA_ID")
            .args(&args)
            .assert()
            .code(5)
            .stdout("");
    }
    assert!(graph.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn calling_set_restricts_icons_and_sets_srtp() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for calling in [
        json!({"call_icons":{"restrict_to_user_countries":["DO","US"]},"srtp_key_exchange_protocol":"SDES"}),
        json!({"call_icons":{"restrict_to_user_countries":[]}}),
    ] {
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/settings")))
            .and(body_json(
                json!({"messaging_product":"whatsapp","calling":calling}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
            .expect(1)
            .mount(&graph)
            .await;
    }
    paloma(Some(&graph), &home)
        .args([
            "calling",
            "set",
            "--icon-countries",
            "do, US",
            "--srtp",
            "sdes",
        ])
        .assert()
        .success();
    paloma(Some(&graph), &home)
        .args(["calling", "set", "--icon-everywhere"])
        .assert()
        .success();
}

#[test]
fn calling_settings_refuse_bad_input_before_context() {
    let home = TempDir::new().unwrap();
    let cases: [&[&str]; 5] = [
        &["set", "--icon-countries", "USA"],
        &[
            "voicemail",
            "set",
            "--trigger",
            "timeout",
            "--announcement",
            "1",
        ],
        &[
            "voicemail",
            "set",
            "--trigger",
            "reject",
            "--ring-seconds",
            "5",
            "--announcement",
            "1",
        ],
        &[
            "voicemail",
            "set",
            "--trigger",
            "timeout",
            "--ring-seconds",
            "31",
            "--announcement",
            "1",
        ],
        &[
            "voicemail",
            "set",
            "--trigger",
            "reject",
            "--announcement",
            "media-1",
        ],
    ];
    for args in cases {
        paloma(None, &home)
            .env_remove("PALOMA_PHONE_NUMBER_ID")
            .env_remove("PALOMA_TOKEN")
            .arg("calling")
            .args(args)
            .assert()
            .code(5);
    }
}

#[tokio::test]
async fn calling_voicemail_set_and_disable() {
    let graph = MockServer::start().await;
    let home = TempDir::new().unwrap();
    for voicemail in [
        json!({"status":"ENABLED","triggers":["REJECT","TIMEOUT"],
               "audio":{"default":{"announcement_media_id":938_884_519_013_664_u64,"timeout_seconds":20}}}),
        json!({"status":"DISABLED"}),
    ] {
        Mock::given(method("POST"))
            .and(path(format!("/v26.0/{PHONE}/settings")))
            .and(body_json(
                json!({"messaging_product":"whatsapp","calling":{"voicemail":voicemail}}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true})))
            .expect(1)
            .mount(&graph)
            .await;
    }
    paloma(Some(&graph), &home)
        .args([
            "calling",
            "voicemail",
            "set",
            "--trigger",
            "reject,timeout",
            "--trigger",
            "REJECT",
            "--ring-seconds",
            "20",
            "--announcement",
            "938884519013664",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("call hours"));
    paloma(Some(&graph), &home)
        .args(["calling", "voicemail", "disable"])
        .assert()
        .success();
}
