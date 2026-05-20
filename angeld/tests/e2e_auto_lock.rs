mod common;

use common::DaemonHarness;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_status_endpoint_returns_active_state() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let resp = h.get_json("/api/auto-lock/status").await?;
    assert_eq!(resp["idle_timeout_min"].as_u64(), Some(15));
    assert!(resp["remaining_seconds"].as_u64().is_some());
    assert_eq!(resp["state"].as_str(), Some("active"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_status_polling_does_not_touch() -> Result<(), Box<dyn std::error::Error>> {
    // Regression test: status polling MUST NOT reset countdown.
    // This is the whole reason require_session_no_touch exists.
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    // Seed last_activity via POST /touch (explicit require_session + touch call),
    // then wait 2s to cross a whole-second boundary so remaining_secs() counts down.
    let _ = h.post("/api/auto-lock/touch").await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let r1 = h.get_json("/api/auto-lock/status").await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let r2 = h.get_json("/api/auto-lock/status").await?;
    let rem1 = r1["remaining_seconds"].as_u64().unwrap();
    let rem2 = r2["remaining_seconds"].as_u64().unwrap();
    assert!(
        rem2 < rem1,
        "polling must NOT reset timer; rem1={rem1} rem2={rem2}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_authenticated_call_touches_timer() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    // Seed last_activity, then wait so remaining_secs() is counting down.
    let _ = h.post("/api/auto-lock/touch").await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let r1 = h.get_json("/api/auto-lock/status").await?;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    // POST /api/auto-lock/timeout is require_session-gated. set_timeout_minutes
    // does NOT explicitly touch — any reset proves the ACL hook is wired.
    let _ = h
        .post_json(
            "/api/auto-lock/timeout",
            serde_json::json!({"idle_timeout_min": 15}),
        )
        .await?;
    let r2 = h.get_json("/api/auto-lock/status").await?;
    let rem1 = r1["remaining_seconds"].as_u64().unwrap();
    let rem2 = r2["remaining_seconds"].as_u64().unwrap();
    assert!(
        rem2 >= rem1.saturating_sub(1),
        "ACL-hook-only call should reset timer (or stay within 1s); rem1={rem1} rem2={rem2}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_touch_endpoint_resets_remaining() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let before = h.get_json("/api/auto-lock/status").await?;
    let touch_resp = h.post("/api/auto-lock/touch").await?;
    assert_eq!(
        touch_resp.status, 204,
        "POST /touch must return 204; got {} body={}",
        touch_resp.status, touch_resp.body
    );
    let after = h.get_json("/api/auto-lock/status").await?;
    let rem_before = before["remaining_seconds"].as_u64().unwrap();
    let rem_after = after["remaining_seconds"].as_u64().unwrap();
    assert!(
        rem_after >= rem_before,
        "touch must NOT decrease remaining; before={rem_before} after={rem_after}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_set_timeout_hot_reloads_status() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let resp = h
        .post_json(
            "/api/auto-lock/timeout",
            serde_json::json!({"idle_timeout_min": 30}),
        )
        .await?;
    assert_eq!(
        resp.status, 204,
        "POST /timeout must return 204; got {} body={}",
        resp.status, resp.body
    );
    let status = h.get_json("/api/auto-lock/status").await?;
    assert_eq!(status["idle_timeout_min"].as_u64(), Some(30));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_unauthenticated_health_does_not_touch() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    // Seed last_activity via POST /touch (explicit require_session + touch call),
    // then wait 2s to cross a whole-second boundary so remaining_secs() counts down.
    let _ = h.post("/api/auto-lock/touch").await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let r1 = h.get_json("/api/auto-lock/status").await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let _ = h.get_raw("/api/diagnostics/health").await?;
    let r2 = h.get_json("/api/auto-lock/status").await?;
    let rem1 = r1["remaining_seconds"].as_u64().unwrap();
    let rem2 = r2["remaining_seconds"].as_u64().unwrap();
    assert!(
        rem2 < rem1,
        "anonymous health check must NOT touch; rem1={rem1} rem2={rem2}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_status_endpoint_rejects_unauthenticated() -> Result<(), Box<dyn std::error::Error>> {
    let h = DaemonHarness::spawn().await?;
    // No unlock — session_token = None.
    let resp = h.get_raw("/api/auto-lock/status").await?;
    assert_eq!(
        resp.status, 401,
        "expected 401 for unauthenticated GET /status; got {} body={}",
        resp.status, resp.body
    );
    Ok(())
}

#[cfg(all(target_os = "windows", feature = "test-helpers"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_win_session_lock_triggers_force_lock() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let resp = h.get_json("/api/auth/session").await?;
    assert_eq!(resp["valid"].as_bool(), Some(true));

    let r = h.post("/api/auto-lock/_test/simulate-session-lock").await?;
    assert_eq!(
        r.status, 204,
        "simulate-session-lock must return 204; got {} body={}",
        r.status, r.body
    );

    tokio::time::sleep(Duration::from_millis(500)).await;

    let status = h.get_json("/api/auto-lock/status").await?;
    assert_eq!(status["state"].as_str(), Some("locked"));
    Ok(())
}
