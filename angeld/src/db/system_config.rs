use sqlx::FromRow;
use sqlx::SqlitePool;

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct SystemConfigRecord {
    pub config_key: String,
    pub config_value: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct CloudUsageDailyRecord {
    pub day_epoch: i64,
    pub read_ops: i64,
    pub write_ops: i64,
    pub egress_bytes: i64,
    pub updated_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CloudUsageDelta {
    pub read_ops: i64,
    pub write_ops: i64,
    pub egress_bytes: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloudUsageApplyResult {
    pub day_epoch: i64,
    pub read_ops: i64,
    pub write_ops: i64,
    pub egress_bytes: i64,
    pub allowed: bool,
}

#[allow(dead_code)]
pub async fn get_system_config_value(
    pool: &SqlitePool,
    config_key: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT config_value
        FROM system_config
        WHERE config_key = ?
        "#,
    )
    .bind(config_key)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn list_system_config(pool: &SqlitePool) -> Result<Vec<SystemConfigRecord>, sqlx::Error> {
    sqlx::query_as::<_, SystemConfigRecord>(
        r#"
        SELECT config_key, config_value, created_at, updated_at
        FROM system_config
        ORDER BY config_key ASC
        "#,
    )
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn set_system_config_value(
    pool: &SqlitePool,
    config_key: &str,
    config_value: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO system_config (
            config_key,
            config_value,
            created_at,
            updated_at
        )
        VALUES (
            ?,
            ?,
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        )
        ON CONFLICT(config_key) DO UPDATE SET
            config_value = excluded.config_value,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(config_key)
    .bind(config_value)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_last_applied_roster_snapshot_at(
    pool: &SqlitePool,
) -> Result<Option<i64>, sqlx::Error> {
    Ok(
        get_system_config_value(pool, "last_applied_roster_snapshot_at")
            .await?
            .and_then(|v| v.parse::<i64>().ok()),
    )
}

#[allow(dead_code)]
pub async fn set_last_applied_roster_snapshot_at(
    pool: &SqlitePool,
    created_at: i64,
) -> Result<(), sqlx::Error> {
    set_system_config_value(
        pool,
        "last_applied_roster_snapshot_at",
        &created_at.to_string(),
    )
    .await
}

#[allow(dead_code)]
pub async fn get_cloud_usage_for_day(
    pool: &SqlitePool,
    day_epoch: i64,
) -> Result<Option<CloudUsageDailyRecord>, sqlx::Error> {
    sqlx::query_as::<_, CloudUsageDailyRecord>(
        r#"
        SELECT day_epoch, read_ops, write_ops, egress_bytes, updated_at
        FROM cloud_usage_daily
        WHERE day_epoch = ?
        "#,
    )
    .bind(day_epoch)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn apply_cloud_usage_delta_with_limits(
    pool: &SqlitePool,
    day_epoch: i64,
    delta: CloudUsageDelta,
    read_limit: i64,
    write_limit: i64,
    egress_limit: i64,
) -> Result<CloudUsageApplyResult, sqlx::Error> {
    let mut conn = pool.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE TRANSACTION")
        .execute(&mut *conn)
        .await?;

    let apply_result = async {
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO cloud_usage_daily (
                day_epoch, read_ops, write_ops, egress_bytes, updated_at
            )
            VALUES (
                ?, 0, 0, 0,
                CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
            )
            "#,
        )
        .bind(day_epoch)
        .execute(&mut *conn)
        .await?;

        let existing = sqlx::query_as::<_, CloudUsageDailyRecord>(
            r#"
            SELECT day_epoch, read_ops, write_ops, egress_bytes, updated_at
            FROM cloud_usage_daily
            WHERE day_epoch = ?
            "#,
        )
        .bind(day_epoch)
        .fetch_one(&mut *conn)
        .await?;

        let next_read_ops = existing.read_ops.saturating_add(delta.read_ops);
        let next_write_ops = existing.write_ops.saturating_add(delta.write_ops);
        let next_egress_bytes = existing.egress_bytes.saturating_add(delta.egress_bytes);
        let allowed = next_read_ops <= read_limit
            && next_write_ops <= write_limit
            && next_egress_bytes <= egress_limit;

        if allowed {
            sqlx::query(
                r#"
                UPDATE cloud_usage_daily
                SET
                    read_ops = ?,
                    write_ops = ?,
                    egress_bytes = ?,
                    updated_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
                WHERE day_epoch = ?
                "#,
            )
            .bind(next_read_ops)
            .bind(next_write_ops)
            .bind(next_egress_bytes)
            .bind(day_epoch)
            .execute(&mut *conn)
            .await?;
        }

        Ok::<_, sqlx::Error>(CloudUsageApplyResult {
            day_epoch,
            read_ops: if allowed {
                next_read_ops
            } else {
                existing.read_ops
            },
            write_ops: if allowed {
                next_write_ops
            } else {
                existing.write_ops
            },
            egress_bytes: if allowed {
                next_egress_bytes
            } else {
                existing.egress_bytes
            },
            allowed,
        })
    }
    .await;

    match apply_result {
        Ok(result) => {
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok(result)
        }
        Err(err) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::*;

    #[tokio::test]
    async fn roster_snapshot_marker_round_trips() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        assert_eq!(
            get_last_applied_roster_snapshot_at(&pool).await.unwrap(),
            None
        );
        set_last_applied_roster_snapshot_at(&pool, 1234)
            .await
            .unwrap();
        assert_eq!(
            get_last_applied_roster_snapshot_at(&pool).await.unwrap(),
            Some(1234)
        );
        set_last_applied_roster_snapshot_at(&pool, 5678)
            .await
            .unwrap();
        assert_eq!(
            get_last_applied_roster_snapshot_at(&pool).await.unwrap(),
            Some(5678)
        );
    }
}
