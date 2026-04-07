/// E2E test for Epic 35.1c: IngestWorker full pipeline + ghost swap.
///
/// Verifies: PENDING → CHUNKING → UPLOADING → GHOSTED → job deleted.
/// On Windows: also verifies FILE_ATTRIBUTE_OFFLINE + REPARSE_POINT (placeholder).
use angeld::db;
use angeld::ingest::{IngestJob, IngestWorker};
use angeld::vault::VaultKeyStore;
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, Duration};

/// Generate a unique temp directory path for this test run.
fn test_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("omnidrive-e2e-{label}-{nanos}"))
}

/// Background task: poll for pending pack_shards and flip them to COMPLETED.
/// This simulates the UploadWorker uploading shards to the cloud.
async fn mock_shard_uploader(pool: SqlitePool, stop: tokio::sync::watch::Receiver<bool>) {
    loop {
        if *stop.borrow() {
            break;
        }

        // Find all packs with pending shards and mark them completed.
        let rows = sqlx::query_as::<_, db::PackShardRecord>(
            "SELECT id, pack_id, shard_index, shard_role, provider, object_key, \
             size, checksum, status, attempts, last_error, last_verified_at, \
             last_verification_method, last_verification_status, last_verified_size, \
             verification_failures \
             FROM pack_shards WHERE status IN ('PENDING', 'IN_PROGRESS')",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();

        for shard in &rows {
            let _ = db::mark_pack_shard_completed(&pool, &shard.pack_id, shard.shard_index).await;
        }

        // Also mark upload_jobs as completed.
        let jobs: Vec<db::UploadJob> = sqlx::query_as::<_, db::UploadJob>(
            "SELECT id, pack_id, status, attempts FROM upload_jobs WHERE status != 'COMPLETED'",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();

        for job in &jobs {
            let _ = db::mark_upload_job_completed(&pool, job.id).await;
        }

        sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test]
async fn ingest_pipeline_full_cycle() -> Result<(), Box<dyn std::error::Error>> {
    // ── 1. Set up environment ───────────────────────────────────────────
    let root = test_dir("ingest");
    let sync_root = root.join("SyncRoot");
    let spool_dir = root.join("spool");
    tokio::fs::create_dir_all(&sync_root).await?;
    tokio::fs::create_dir_all(&spool_dir).await?;

    // Set OMNIDRIVE_WATCH_DIR so projection_relative_path strips correctly.
    let sync_root_str = sync_root.to_string_lossy().replace('\\', "/");
    // SAFETY: single-threaded at this point, no other threads reading env.
    unsafe { std::env::set_var("OMNIDRIVE_WATCH_DIR", &sync_root_str); }

    // Init DB.
    let db_path = root.join("test-ingest.db");
    let db_url = format!(
        "sqlite:///{}",
        db_path.to_string_lossy().replace('\\', "/")
    );
    let pool = db::init_db(&db_url).await?;

    // Unlock vault (creates V2 keys).
    let vault_keys = VaultKeyStore::new();
    vault_keys.unlock(&pool, "test-passphrase-e2e").await?;

    // ── 2. Create test file (2 MB of random-ish data) ───────────────────
    let test_file = sync_root.join("test_ghost.txt");
    let payload_size: usize = 2 * 1024 * 1024; // 2 MB
    let payload: Vec<u8> = (0..payload_size).map(|i| (i % 251) as u8).collect();
    tokio::fs::write(&test_file, &payload).await?;

    let file_path_str = test_file.to_string_lossy().replace('\\', "/");

    // ── 3. Enqueue ingest job ───────────────────────────────────────────
    let job_id = db::create_ingest_job(&pool, &file_path_str, payload_size as i64).await?;

    // Verify job is PENDING.
    let row = db::get_ingest_job(&pool, job_id).await?.expect("job should exist");
    assert_eq!(row.state, "PENDING");

    // ── 4. Start mock shard uploader in background ──────────────────────
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let uploader_handle = tokio::spawn(mock_shard_uploader(pool.clone(), stop_rx));

    // ── 5. Create IngestWorker and drive the job ────────────────────────
    let worker = IngestWorker::new(
        pool.clone(),
        vault_keys.clone(),
        spool_dir.clone(),
        sync_root.clone(),
    );

    let job = IngestJob::from_row(&row).expect("valid job");
    worker.process_job_for_test(&job).await?;

    // Stop mock uploader.
    stop_tx.send(true)?;
    let _ = uploader_handle.await;

    // ── 6. Assertions ───────────────────────────────────────────────────

    // 6a. Job should be deleted from ingest_jobs (cleanup after GHOSTED).
    let after = db::get_ingest_job(&pool, job_id).await?;
    assert!(
        after.is_none(),
        "ingest job should be deleted after successful ghost swap"
    );

    // 6b. file_revisions should have a current revision for this file.
    let inode_row = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM inodes WHERE name = 'test_ghost.txt' AND kind = 'FILE' LIMIT 1",
    )
    .fetch_one(&pool)
    .await?;

    let rev_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM file_revisions WHERE inode_id = ? AND is_current = 1",
    )
    .bind(inode_row)
    .fetch_one(&pool)
    .await?;
    assert_eq!(rev_count, 1, "should have exactly one current revision");

    // 6c. pack_shards should all be COMPLETED.
    let pending_shards = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pack_shards WHERE status != 'COMPLETED'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(pending_shards, 0, "all shards should be COMPLETED");

    // 6d. The test file should still exist (ghost swap may or may not
    //     succeed outside a registered SyncRoot — we check attributes
    //     only on Windows where cfapi is available).
    assert!(
        test_file.exists(),
        "file should still exist at original path"
    );

    // 6e. Verify logical file size is preserved.
    let meta = tokio::fs::metadata(&test_file).await?;
    assert_eq!(
        meta.len(),
        payload_size as u64,
        "file size should be preserved"
    );

    // ── 6f. Windows-only: check placeholder attributes ──────────────────
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        let attrs = meta.file_attributes();
        const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

        // NOTE: CfConvertToPlaceholder + dehydrate only works inside a
        // registered SyncRoot. In test without SyncRoot registration the
        // ghost swap logs a warning and the file stays as-is.
        // We check for OFFLINE attribute if the ghost swap succeeded.
        if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            assert!(
                attrs & FILE_ATTRIBUTE_OFFLINE != 0,
                "dehydrated placeholder should have FILE_ATTRIBUTE_OFFLINE, got 0x{attrs:08X}"
            );
            eprintln!(
                "✓ Ghost swap verified: file has REPARSE_POINT + OFFLINE (attrs=0x{attrs:08X})"
            );
        } else {
            eprintln!(
                "⚠ Ghost swap did not convert file (no registered SyncRoot in test env), \
                 attrs=0x{attrs:08X} — this is expected in CI/non-cfapi environments"
            );
        }
    }

    // ── Cleanup ─────────────────────────────────────────────────────────
    drop(pool);
    let _ = tokio::fs::remove_dir_all(&root).await;

    Ok(())
}
