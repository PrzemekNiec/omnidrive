use crate::runtime_paths::tray_session_file_path;
use crate::win_acl::write_user_only_file;
use sqlx::SqlitePool;
use std::path::PathBuf;
use tracing::warn;

pub async fn publish(pool: &SqlitePool) -> Result<PathBuf, String> {
    let session = crate::api::auth::create_session_for_local_device(pool).await?;
    write(&session.token)
}

pub(crate) fn refresh(token: &str) {
    if let Err(err) = write(token) {
        warn!("[TRAY-SESSION] refresh failed: {err}");
    }
}

fn write(token: &str) -> Result<PathBuf, String> {
    let path = tray_session_file_path();
    write_user_only_file(&path, format!("{token}\n").as_bytes()).map_err(|err| err.to_string())?;
    Ok(path)
}
