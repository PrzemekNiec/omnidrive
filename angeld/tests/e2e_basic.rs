mod common;
use common::DaemonHarness;
use sqlx::SqlitePool;
use std::time::{Duration, Instant};
use tokio::time::sleep;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn happy_path_upload_queue_clears_and_uploader_returns_idle()
-> Result<(), Box<dyn std::error::Error>> {
    let mut harness = DaemonHarness::spawn().await?;
    let initial = harness.health().await?;
    assert_eq!(initial.pending_uploads_queue_size, 0);
    assert_eq!(initial.worker_statuses.uploader, "idle");

    let pool = harness.connect_db().await?;
    let pack_id = "e2e-local-only-pack";
    sqlx::query(
        r#"
        INSERT INTO packs (
            pack_id,
            chunk_id,
            plaintext_hash,
            storage_mode,
            encryption_version,
            ec_scheme,
            logical_size,
            cipher_size,
            shard_size,
            nonce,
            gcm_tag,
            status
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(pack_id)
    .bind(vec![1u8, 2, 3, 4])
    .bind("e2e-local-only-hash")
    .bind("LOCAL_ONLY")
    .bind(1_i64)
    .bind("local_only")
    .bind(0_i64)
    .bind(0_i64)
    .bind(0_i64)
    .bind(Vec::<u8>::new())
    .bind(Vec::<u8>::new())
    .bind("UPLOADING")
    .execute(&pool)
    .await?;
    angeld::db::queue_pack_for_upload(&pool, pack_id).await?;

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut saw_active = false;
    let mut saw_idle_after_active = false;

    loop {
        if harness.child.try_wait()?.is_some() {
            return Err(harness
                .failure_message("daemon exited before upload queue test completed")
                .into());
        }

        let health = harness.health().await?;
        if health.worker_statuses.uploader == "active" {
            saw_active = true;
        }
        if saw_active && health.worker_statuses.uploader == "idle" {
            saw_idle_after_active = true;
        }

        let job = angeld::db::get_upload_job_by_pack_id(&pool, pack_id).await?;
        let completed = matches!(
            job.as_ref().map(|job| job.status.as_str()),
            Some("COMPLETED")
        );

        if health.pending_uploads_queue_size == 0 && completed && saw_idle_after_active {
            assert!(health.uptime_seconds <= 30);
            assert!(health.last_upload_error.is_none());
            assert_eq!(health.worker_statuses.api, "idle");
            assert_eq!(health.worker_statuses.repair, "idle");
            assert_eq!(health.worker_statuses.scrubber, "idle");
            assert_eq!(health.worker_statuses.gc, "idle");
            assert_eq!(health.worker_statuses.watcher, "idle");
            assert_eq!(health.worker_statuses.metadata_backup, "idle");
            break;
        }

        if Instant::now() >= deadline {
            return Err(harness
                .failure_message("upload queue did not clear in time")
                .into());
        }

        sleep(Duration::from_millis(100)).await;
    }

    harness.shutdown().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_lock_timeout_endpoint_accepts_preset() -> Result<(), Box<dyn std::error::Error>> {
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
        "expected 204 but got {}; body: {}",
        resp.status, resp.body
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_lock_timeout_endpoint_rejects_invalid() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let resp = h
        .post_json(
            "/api/auto-lock/timeout",
            serde_json::json!({"idle_timeout_min": 7}),
        )
        .await?;
    assert_eq!(
        resp.status, 400,
        "expected 400 but got {}; body: {}",
        resp.status, resp.body
    );
    assert!(
        resp.body.contains("invalid_preset"),
        "body missing 'invalid_preset': {}",
        resp.body
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_lock_timeout_endpoint_rejects_unauthenticated()
-> Result<(), Box<dyn std::error::Error>> {
    let h = DaemonHarness::spawn().await?;
    let resp = h
        .post_json(
            "/api/auto-lock/timeout",
            serde_json::json!({"idle_timeout_min": 30}),
        )
        .await?;
    assert_eq!(
        resp.status, 401,
        "expected 401 unauthenticated; got {}; body: {}",
        resp.status, resp.body
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logout_emits_logout_audit_not_auto_lock() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let resp = h
        .post_json("/api/auth/logout", serde_json::Value::Null)
        .await?;
    assert_eq!(
        resp.status, 200,
        "expected 200 but got {}; body: {}",
        resp.status, resp.body
    );
    let pool: SqlitePool = h.connect_db().await?;
    let row: (String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT action, actor_user_id, actor_device_id FROM audit_logs ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(row.0, "logout");
    assert!(
        row.1.is_some(),
        "actor_user_id must be populated on logout audit"
    );
    assert!(
        row.2.is_some(),
        "actor_device_id must be populated on logout audit"
    );
    Ok(())
}
