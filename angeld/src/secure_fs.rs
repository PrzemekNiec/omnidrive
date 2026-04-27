use std::fmt;
use std::future::Future;
use std::path::Path;
use std::time::Duration;
use tokio::fs;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::time::sleep;

#[derive(Debug)]
pub enum SecureFsError {
    Io(std::io::Error),
    NumericOverflow(&'static str),
}

impl fmt::Display for SecureFsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "secure fs i/o error: {err}"),
            Self::NumericOverflow(ctx) => write!(f, "numeric overflow while handling {ctx}"),
        }
    }
}

impl std::error::Error for SecureFsError {}

impl From<std::io::Error> for SecureFsError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Retries an `io::Result`-returning async operation on Windows file-lock errors.
///
/// Defender, Explorer, and cfapi hold file handles briefly after another process
/// closes them. `ERROR_SHARING_VIOLATION (32)` and `ERROR_LOCK_VIOLATION (33)` are
/// transient — retrying with linear backoff is the correct response.
///
/// Any other error, including `NotFound`, is returned immediately without retry.
pub async fn retry_io<F, Fut, T>(
    op_name: &'static str,
    path: &Path,
    attempts: usize,
    backoff: Duration,
    op: F,
) -> std::io::Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = std::io::Result<T>>,
{
    let mut last_err = None;
    for attempt in 1..=attempts {
        match op().await {
            Ok(val) => return Ok(val),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(err),
            Err(err) if is_lock_error(&err) => {
                tracing::debug!(
                    op = op_name,
                    path = %path.display(),
                    attempt,
                    total = attempts,
                    error = %err,
                    "file locked — will retry"
                );
                last_err = Some(err);
                if attempt < attempts {
                    sleep(backoff).await;
                }
            }
            Err(err) => return Err(err),
        }
    }
    let err = last_err.expect("retry loop must capture last error");
    tracing::warn!(
        op = op_name,
        path = %path.display(),
        attempts,
        error = %err,
        "file still locked after all retries"
    );
    Err(err)
}

/// Returns true for Windows error codes indicating a transient file lock held by another process.
#[cfg(windows)]
fn is_lock_error(err: &std::io::Error) -> bool {
    // ERROR_SHARING_VIOLATION = 32, ERROR_LOCK_VIOLATION = 33
    matches!(err.raw_os_error(), Some(32) | Some(33))
}

#[cfg(not(windows))]
fn is_lock_error(_err: &std::io::Error) -> bool {
    false
}

pub async fn secure_delete(path: impl AsRef<Path>) -> Result<(), SecureFsError> {
    let path = path.as_ref();
    let metadata = match fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(SecureFsError::Io(err)),
    };

    if !metadata.is_file() {
        match fs::remove_file(path).await {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(SecureFsError::Io(err)),
        }
    }

    let mut file = fs::OpenOptions::new().write(true).open(path).await?;
    let file_len = metadata.len();
    let zero_chunk = vec![0u8; 1024 * 1024];
    let mut remaining = file_len;

    file.seek(std::io::SeekFrom::Start(0)).await?;
    while remaining > 0 {
        let to_write = usize::try_from(remaining.min(zero_chunk.len() as u64))
            .map_err(|_| SecureFsError::NumericOverflow("secure delete chunk size"))?;
        file.write_all(&zero_chunk[..to_write]).await?;
        remaining -= u64::try_from(to_write)
            .map_err(|_| SecureFsError::NumericOverflow("secure delete remaining size"))?;
    }
    file.flush().await?;
    file.sync_all().await?;
    drop(file);

    retry_io(
        "secure_delete",
        path,
        5,
        Duration::from_millis(500),
        || fs::remove_file(path),
    )
    .await
    .or_else(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            Ok(())
        } else {
            Err(SecureFsError::Io(err))
        }
    })
}

pub async fn write_ephemeral_bytes(
    path: impl AsRef<Path>,
    bytes: &[u8],
) -> Result<(), SecureFsError> {
    let path = path.as_ref().to_path_buf();
    let payload = bytes.to_vec();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    tokio::task::spawn_blocking(move || write_ephemeral_bytes_blocking(&path, &payload))
        .await
        .map_err(|_| SecureFsError::Io(std::io::Error::other("ephemeral write task failed")))??;

    Ok(())
}

#[cfg(windows)]
fn write_ephemeral_bytes_blocking(path: &Path, bytes: &[u8]) -> Result<(), SecureFsError> {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_ATTRIBUTE_TEMPORARY: u32 = 0x0000_0100;

    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .attributes(FILE_ATTRIBUTE_TEMPORARY)
        .open(path)?;
    file.write_all(bytes)?;
    file.flush()?;
    Ok(())
}

#[cfg(not(windows))]
fn write_ephemeral_bytes_blocking(path: &Path, bytes: &[u8]) -> Result<(), SecureFsError> {
    std::fs::write(path, bytes)?;
    Ok(())
}
