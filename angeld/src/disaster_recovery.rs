use sqlx::SqlitePool;
use std::fmt;
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Debug)]
pub enum DisasterRecoveryError {
    Io(std::io::Error),
    Db(sqlx::Error),
    InvalidOutputPath(&'static str),
}

impl fmt::Display for DisasterRecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "i/o error: {err}"),
            Self::Db(err) => write!(f, "sqlite error: {err}"),
            Self::InvalidOutputPath(reason) => write!(f, "invalid snapshot output path: {reason}"),
        }
    }
}

impl std::error::Error for DisasterRecoveryError {}

impl From<std::io::Error> for DisasterRecoveryError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<sqlx::Error> for DisasterRecoveryError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value)
    }
}

pub async fn create_metadata_snapshot(
    source_pool: &SqlitePool,
    output_path: &Path,
) -> Result<(), DisasterRecoveryError> {
    if output_path.as_os_str().is_empty() {
        return Err(DisasterRecoveryError::InvalidOutputPath("empty path"));
    }

    let output_path = normalize_snapshot_path(output_path)?;
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    if fs::try_exists(&output_path).await? {
        fs::remove_file(&output_path).await?;
    }

    let sql = format!("VACUUM INTO '{}'", sqlite_string_literal(&output_path));
    sqlx::query(&sql).execute(source_pool).await?;

    if !fs::try_exists(&output_path).await? {
        return Err(DisasterRecoveryError::InvalidOutputPath(
            "snapshot file was not created",
        ));
    }

    Ok(())
}

fn normalize_snapshot_path(output_path: &Path) -> Result<PathBuf, DisasterRecoveryError> {
    if output_path.is_dir() {
        return Err(DisasterRecoveryError::InvalidOutputPath(
            "path points to a directory",
        ));
    }

    Ok(output_path.to_path_buf())
}

fn sqlite_string_literal(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn creates_live_sqlite_snapshot_copy() -> Result<(), Box<dyn std::error::Error>> {
        let test_root = env::temp_dir().join(format!(
            "omnidrive-dr-snapshot-test-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        let source_path = test_root.join("source.db");
        let snapshot_path = test_root.join("snapshot.db");

        fs::create_dir_all(&test_root).await?;

        let source_url = format!("sqlite://{}", source_path.to_string_lossy().replace('\\', "/"));
        let pool = db::init_db(&source_url).await?;
        let inode_id = db::create_inode(&pool, None, "snapshot-test.txt", "FILE", 123).await?;
        assert!(inode_id > 0);

        create_metadata_snapshot(&pool, &snapshot_path).await?;

        assert!(fs::try_exists(&snapshot_path).await?);
        assert!(fs::metadata(&snapshot_path).await?.len() > 0);

        let snapshot_url = format!(
            "sqlite://{}",
            snapshot_path.to_string_lossy().replace('\\', "/")
        );
        let snapshot_pool = db::init_db(&snapshot_url).await?;
        let inode = db::get_inode_by_id(&snapshot_pool, inode_id).await?;
        assert!(inode.is_some());

        let _ = fs::remove_dir_all(&test_root).await;
        Ok(())
    }
}
