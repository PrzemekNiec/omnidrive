use std::fmt;
use std::path::Path;
use tokio::fs;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

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

    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(SecureFsError::Io(err)),
    }
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
