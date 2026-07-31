use serde::Serialize;
use sqlx::FromRow;
use sqlx::SqlitePool;

// ── Stats (Epic 36 G-BE) ────────────────────────────────────────────

#[derive(Debug, Clone, FromRow)]
pub struct StatsOverview {
    pub files_count: i64,
    pub logical_size_bytes: i64,
}

pub async fn get_stats_overview(pool: &SqlitePool) -> Result<StatsOverview, sqlx::Error> {
    sqlx::query_as::<_, StatsOverview>(
        r#"
        SELECT
            COALESCE(COUNT(*), 0) AS files_count,
            COALESCE(SUM(size), 0) AS logical_size_bytes
        FROM inodes
        WHERE kind = 'file'
          AND deleted_at IS NULL
        "#,
    )
    .fetch_one(pool)
    .await
}

pub async fn count_active_devices(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM devices WHERE revoked_at IS NULL")
        .fetch_one(pool)
        .await
}

/// Record upload or download bytes into a 2-hour bucket.
pub async fn record_traffic(
    pool: &SqlitePool,
    upload_bytes: i64,
    download_bytes: i64,
) -> Result<(), sqlx::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let bucket_epoch = now - (now % 7200); // 2-hour bucket

    sqlx::query(
        r#"
        INSERT INTO traffic_stats (bucket_epoch, upload_bytes, download_bytes)
        VALUES (?, ?, ?)
        ON CONFLICT(bucket_epoch) DO UPDATE SET
            upload_bytes = upload_bytes + excluded.upload_bytes,
            download_bytes = download_bytes + excluded.download_bytes
        "#,
    )
    .bind(bucket_epoch)
    .bind(upload_bytes)
    .bind(download_bytes)
    .execute(pool)
    .await?;

    Ok(())
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct TrafficBucket {
    pub bucket_epoch: i64,
    pub upload_bytes: i64,
    pub download_bytes: i64,
}

/// Return traffic buckets for the last N hours (default 24).
pub async fn get_traffic_buckets(
    pool: &SqlitePool,
    hours: u32,
) -> Result<Vec<TrafficBucket>, sqlx::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let cutoff = now - (hours as i64 * 3600);
    let cutoff_bucket = cutoff - (cutoff % 7200);

    sqlx::query_as::<_, TrafficBucket>(
        r#"
        SELECT bucket_epoch, upload_bytes, download_bytes
        FROM traffic_stats
        WHERE bucket_epoch >= ?
        ORDER BY bucket_epoch ASC
        "#,
    )
    .bind(cutoff_bucket)
    .fetch_all(pool)
    .await
}
