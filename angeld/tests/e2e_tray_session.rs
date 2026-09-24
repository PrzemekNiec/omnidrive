mod common;

#[tokio::test]
async fn daemon_publishes_tray_session_usable_on_a_gated_route() {
    let harness = common::DaemonHarness::spawn()
        .await
        .expect("daemon harness spawn");

    let session_path = harness
        .temp_root
        .join("localapp")
        .join("OmniDrive")
        .join("tray-session");
    let contents = std::fs::read_to_string(&session_path).unwrap_or_else(|err| {
        panic!(
            "{}",
            harness.failure_message(&format!("tray-session file must exist after spawn: {err}"))
        )
    });
    let token = contents.trim().to_string();
    assert!(!token.is_empty(), "tray-session token must not be empty");
    assert!(
        !token.chars().any(|c| c.is_whitespace()),
        "trimmed token must contain no whitespace: {token:?}"
    );

    let unauthorized = common::http_get_raw(&format!("{}/api/ingest", harness.base_url), None)
        .await
        .expect("unauthorized request");
    assert_eq!(unauthorized.status, 401, "body={}", unauthorized.body);

    let authorized =
        common::http_get_raw(&format!("{}/api/ingest", harness.base_url), Some(&token))
            .await
            .expect("authorized request");
    assert_eq!(authorized.status, 200, "body={}", authorized.body);

    let sddl = angeld::win_acl::dacl_sddl(&session_path).expect("read dacl");
    assert_eq!(sddl.matches("(A;").count(), 2, "sddl={sddl}");
    assert!(sddl.contains(";;;SY)"), "sddl={sddl}");
    let current_user_sid =
        angeld::win_acl::current_user_sid_string_for_tests().expect("current user sid");
    assert!(sddl.contains(&current_user_sid), "sddl={sddl}");
    assert!(!sddl.contains(";;;AU)"), "sddl={sddl}");
    assert!(!sddl.contains(";;;BU)"), "sddl={sddl}");
    assert!(!sddl.contains(";;;WD)"), "sddl={sddl}");
    assert!(!sddl.contains(";;;BA)"), "sddl={sddl}");
}
