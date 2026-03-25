use crate::db;
use crate::diagnostics::{self, WorkerKind, WorkerStatus};
use crate::secure_fs::secure_delete;
use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use omnidrive_core::crypto::{KeyBytes, RootKdfParams, derive_root_keys};
use rand::RngCore;
use sha2::Sha256;
use sqlx::SqlitePool;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::task::JoinHandle;
use tokio::time::{Duration, MissedTickBehavior, interval};
use crate::uploader::{ProviderConfig, Uploader, UploaderError};
use crate::vault::VaultKeyStore;
use tracing::{info, warn};

pub const METADATA_BACKUP_MAGIC: &[u8; 15] = b"OMNIDRIVE-META1";
pub const METADATA_BACKUP_VERSION: u8 = 0x02;
pub const METADATA_BACKUP_INFO: &[u8] = b"omnidrive-metadata-backup-v1";
pub const METADATA_BACKUP_NONCE_LEN: usize = 12;
pub const METADATA_BACKUP_TAG_LEN: usize = 16;
const METADATA_BACKUP_HEADER_FIXED_LEN: usize = 15 + 1 + 2 + 4 + 4 + 4 + 4 + 12;
const METADATA_BACKUP_WORKER_TICK: Duration = Duration::from_secs(60 * 60);
const METADATA_BACKUP_MIN_INTERVAL: Duration = Duration::from_secs(60 * 60 * 24);

#[allow(dead_code)]
#[derive(Debug)]
pub enum DisasterRecoveryError {
    Io(std::io::Error),
    Db(sqlx::Error),
    InvalidOutputPath(&'static str),
    HkdfExpand(hkdf::InvalidLength),
    Aead(aes_gcm::Error),
    Uploader(UploaderError),
    NoConfiguredProviders,
    NoSuccessfulUploads,
    InvalidBackupFormat(&'static str),
    BackupDecryptFailed,
    DownloadFailed(Vec<String>),
}

impl fmt::Display for DisasterRecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "i/o error: {err}"),
            Self::Db(err) => write!(f, "sqlite error: {err}"),
            Self::InvalidOutputPath(reason) => write!(f, "invalid snapshot output path: {reason}"),
            Self::HkdfExpand(_) => write!(f, "hkdf output length was invalid"),
            Self::Aead(_) => write!(f, "aes-gcm operation failed"),
            Self::Uploader(err) => write!(f, "uploader error: {err}"),
            Self::NoConfiguredProviders => write!(f, "no metadata backup providers configured"),
            Self::NoSuccessfulUploads => write!(f, "metadata backup failed on every provider"),
            Self::InvalidBackupFormat(reason) => write!(f, "invalid metadata backup format: {reason}"),
            Self::BackupDecryptFailed => write!(f, "failed to decrypt metadata backup"),
            Self::DownloadFailed(errors) => {
                write!(f, "failed to download metadata backup: {}", errors.join(" | "))
            }
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

impl From<hkdf::InvalidLength> for DisasterRecoveryError {
    fn from(value: hkdf::InvalidLength) -> Self {
        Self::HkdfExpand(value)
    }
}

impl From<aes_gcm::Error> for DisasterRecoveryError {
    fn from(value: aes_gcm::Error) -> Self {
        Self::Aead(value)
    }
}

impl From<UploaderError> for DisasterRecoveryError {
    fn from(value: UploaderError) -> Self {
        Self::Uploader(value)
    }
}

#[allow(dead_code)]
pub struct MetadataBackupProviderManager {
    uploaders: Vec<Uploader>,
    download_providers: Vec<MetadataBackupDownloadProvider>,
    local_store: Option<LocalMetadataBackupStore>,
}

#[allow(dead_code)]
struct MetadataBackupDownloadProvider {
    provider_name: &'static str,
    bucket: String,
    client: aws_sdk_s3::Client,
}

#[allow(dead_code)]
struct LocalMetadataBackupStore {
    root: PathBuf,
}

impl MetadataBackupProviderManager {
    pub async fn from_env() -> Result<Self, DisasterRecoveryError> {
        let _ = dotenvy::dotenv();
        if let Some(root) = std::env::var_os("OMNIDRIVE_METADATA_BACKUP_DIR") {
            let root = PathBuf::from(root);
            fs::create_dir_all(&root).await?;
            return Ok(Self {
                uploaders: Vec::new(),
                download_providers: Vec::new(),
                local_store: Some(LocalMetadataBackupStore { root }),
            });
        }

        let configs = vec![
            ProviderConfig::from_r2_env()?,
            ProviderConfig::from_b2_env()?,
            ProviderConfig::from_scaleway_env()?,
        ];
        let mut uploaders = Vec::with_capacity(configs.len());
        let mut download_providers = Vec::with_capacity(configs.len());

        for config in configs {
            uploaders.push(Uploader::from_provider_config(config.clone()).await?);
            download_providers.push(
                MetadataBackupDownloadProvider::from_provider_config(config).await?,
            );
        }

        if uploaders.is_empty() {
            return Err(DisasterRecoveryError::NoConfiguredProviders);
        }

        Ok(Self {
            uploaders,
            download_providers,
            local_store: None,
        })
    }
}

pub fn start_metadata_backup_worker(
    db_pool: SqlitePool,
    provider_manager: Arc<MetadataBackupProviderManager>,
    keystore: Arc<VaultKeyStore>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(METADATA_BACKUP_WORKER_TICK);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        diagnostics::set_worker_status(WorkerKind::MetadataBackup, WorkerStatus::Idle);

        loop {
            ticker.tick().await;

            let last_success = match db::get_last_successful_metadata_backup_at(&db_pool).await {
                Ok(value) => value,
                Err(err) => {
                    warn!("metadata backup worker failed to query last backup: {err}");
                    continue;
                }
            };

            let now_ms = match unix_timestamp_ms() {
                Ok(value) => value as i64,
                Err(err) => {
                    warn!("metadata backup worker failed to read clock: {err}");
                    continue;
                }
            };

            let should_backup = match last_success {
                Some(created_at) => {
                    let elapsed_ms = now_ms.saturating_sub(created_at);
                    elapsed_ms >= METADATA_BACKUP_MIN_INTERVAL.as_millis() as i64
                }
                None => true,
            };

            if !should_backup {
                continue;
            }

            let master_key = match keystore.require_master_key().await {
                Ok(key) => key,
                Err(_) => {
                    warn!("metadata backup worker skipped: vault is locked");
                    continue;
                }
            };
            diagnostics::set_worker_status(WorkerKind::MetadataBackup, WorkerStatus::Active);

            if let Err(err) =
                run_metadata_backup_now(&db_pool, provider_manager.as_ref(), &master_key).await
            {
                warn!("metadata backup worker failed: {err}");
            } else {
                info!("metadata backup worker uploaded a fresh recovery snapshot");
            }
            diagnostics::set_worker_status(WorkerKind::MetadataBackup, WorkerStatus::Idle);
        }
    })
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

pub async fn create_encrypted_metadata_snapshot(
    source_pool: &SqlitePool,
    output_enc_path: &Path,
    master_key: &[u8],
) -> Result<(), DisasterRecoveryError> {
    let output_enc_path = normalize_encrypted_output_path(output_enc_path)?;
    let temp_snapshot_path = temporary_plaintext_snapshot_path(&output_enc_path);

    if let Some(parent) = output_enc_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    if let Some(parent) = temp_snapshot_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    if fs::try_exists(&output_enc_path).await? {
        fs::remove_file(&output_enc_path).await?;
    }
    if fs::try_exists(&temp_snapshot_path).await? {
        fs::remove_file(&temp_snapshot_path).await?;
    }

    create_metadata_snapshot(source_pool, &temp_snapshot_path).await?;

    let vault_config = db::get_vault_config(source_pool)
        .await?
        .ok_or(DisasterRecoveryError::InvalidBackupFormat(
            "vault_config missing",
        ))?;
    let kdf_params = RootKdfParams::new(
        u32::try_from(vault_config.parameter_set_version)
            .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("parameter_set_version"))?,
        vault_config.salt,
        u32::try_from(vault_config.memory_cost_kib)
            .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("memory_cost_kib"))?,
        u32::try_from(vault_config.time_cost)
            .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("time_cost"))?,
        u32::try_from(vault_config.lanes)
            .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("lanes"))?,
    );

    let encrypt_result = encrypt_metadata_snapshot(
        &temp_snapshot_path,
        &output_enc_path,
        master_key,
        &kdf_params,
    )
    .await;
    let remove_result = secure_delete(&temp_snapshot_path).await;

    encrypt_result?;
    match remove_result {
        Ok(()) => Ok(()),
        Err(err) => Err(DisasterRecoveryError::Io(std::io::Error::other(err.to_string()))),
    }
}

pub async fn encrypt_metadata_snapshot(
    input_db_path: &Path,
    output_enc_path: &Path,
    master_key: &[u8],
    kdf_params: &RootKdfParams,
) -> Result<(), DisasterRecoveryError> {
    let input_bytes = fs::read(input_db_path).await?;
    let derived_key = derive_metadata_backup_key(master_key)?;

    let mut nonce = [0u8; METADATA_BACKUP_NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce);

    let cipher = Aes256Gcm::new_from_slice(&derived_key).map_err(|_| {
        DisasterRecoveryError::InvalidOutputPath("metadata backup key length was invalid")
    })?;
    let mut ciphertext = input_bytes;
    let tag = cipher.encrypt_in_place_detached(
        Nonce::from_slice(&nonce),
        &[],
        &mut ciphertext,
    )?;

    if let Some(parent) = output_enc_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let mut encoded = Vec::with_capacity(
        METADATA_BACKUP_HEADER_FIXED_LEN
            + kdf_params.salt.len()
            + ciphertext.len()
            + METADATA_BACKUP_TAG_LEN,
    );
    encoded.extend_from_slice(METADATA_BACKUP_MAGIC);
    encoded.push(METADATA_BACKUP_VERSION);
    encoded.extend_from_slice(
        &u16::try_from(kdf_params.salt.len())
            .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("salt too long"))?
            .to_le_bytes(),
    );
    encoded.extend_from_slice(&kdf_params.parameter_set_version.to_le_bytes());
    encoded.extend_from_slice(&kdf_params.memory_cost_kib.to_le_bytes());
    encoded.extend_from_slice(&kdf_params.time_cost.to_le_bytes());
    encoded.extend_from_slice(&kdf_params.lanes.to_le_bytes());
    encoded.extend_from_slice(&nonce);
    encoded.extend_from_slice(&kdf_params.salt);
    encoded.extend_from_slice(&ciphertext);
    encoded.extend_from_slice(tag.as_slice());

    fs::write(output_enc_path, encoded).await?;
    Ok(())
}

pub fn derive_metadata_backup_key(master_key: &[u8]) -> Result<KeyBytes, DisasterRecoveryError> {
    let hkdf = Hkdf::<Sha256>::from_prk(master_key)
        .map_err(|_| DisasterRecoveryError::InvalidOutputPath("master key length"))?;
    let mut key = [0u8; 32];
    hkdf.expand(METADATA_BACKUP_INFO, &mut key)?;
    Ok(key)
}

#[allow(dead_code)]
pub async fn restore_metadata_from_cloud(
    provider_manager: &MetadataBackupProviderManager,
    passphrase: &str,
    output_db_path: &Path,
) -> Result<(), DisasterRecoveryError> {
    let object_key = "_omnidrive/system/metadata/latest.db.enc";
    let mut errors = Vec::new();

    if let Some(local_store) = &provider_manager.local_store {
        let encoded = local_store.download_bytes(object_key).await?;
        let plaintext = decrypt_metadata_backup(&encoded, passphrase)?;
        if let Some(parent) = output_db_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(output_db_path, plaintext).await?;
        return Ok(());
    }

    for provider in &provider_manager.download_providers {
        match provider.download_bytes(object_key).await {
            Ok(encoded) => {
                let plaintext = decrypt_metadata_backup(&encoded, passphrase)?;
                if let Some(parent) = output_db_path.parent() {
                    fs::create_dir_all(parent).await?;
                }
                fs::write(output_db_path, plaintext).await?;
                return Ok(());
            }
            Err(err) => errors.push(format!("{}: {}", provider.provider_name, err)),
        }
    }

    Err(DisasterRecoveryError::DownloadFailed(errors))
}

pub async fn upload_metadata_backup(
    db_pool: &SqlitePool,
    provider_manager: &MetadataBackupProviderManager,
    enc_file_path: &Path,
) -> Result<(), DisasterRecoveryError> {
    if provider_manager.uploaders.is_empty() && provider_manager.local_store.is_none() {
        return Err(DisasterRecoveryError::NoConfiguredProviders);
    }

    let metadata = fs::metadata(enc_file_path).await?;
    let encrypted_size = i64::try_from(metadata.len())
        .map_err(|_| DisasterRecoveryError::InvalidOutputPath("encrypted size overflow"))?;
    let created_at = unix_timestamp_ms()? as i64;
    let snapshot_key = format!(
        "_omnidrive/system/metadata/snapshots/{}.db.enc",
        created_at
    );
    let latest_key = "_omnidrive/system/metadata/latest.db.enc";

    let mut successful_uploads = 0usize;

    if let Some(local_store) = &provider_manager.local_store {
        let backup_id = format!("{created_at}-{}", local_store.provider_name());
        db::record_metadata_backup_attempt(
            db_pool,
            &backup_id,
            created_at,
            i64::from(METADATA_BACKUP_VERSION),
            &snapshot_key,
            local_store.provider_name(),
            encrypted_size,
            "UPLOADING",
        )
        .await?;

        match local_store.upload_file(enc_file_path, &snapshot_key).await {
            Ok(()) => {
                successful_uploads += 1;
                db::update_metadata_backup_status(db_pool, &backup_id, "COMPLETED", None).await?;
                if let Err(err) = local_store.upload_file(enc_file_path, latest_key).await {
                    warn!(
                        "metadata backup latest pointer update failed for {}: {}",
                        local_store.provider_name(),
                        err
                    );
                }
            }
            Err(err) => {
                let error_text = err.to_string();
                db::update_metadata_backup_status(
                    db_pool,
                    &backup_id,
                    "FAILED",
                    Some(&error_text),
                )
                .await?;
            }
        }
    }

    for uploader in &provider_manager.uploaders {
        let backup_id = format!("{created_at}-{}", uploader.provider_name());
        db::record_metadata_backup_attempt(
            db_pool,
            &backup_id,
            created_at,
            i64::from(METADATA_BACKUP_VERSION),
            &snapshot_key,
            uploader.provider_name(),
            encrypted_size,
            "UPLOADING",
        )
        .await?;

        match uploader.upload_system_file(enc_file_path, &snapshot_key).await {
            Ok(_) => {
                successful_uploads += 1;
                db::update_metadata_backup_status(db_pool, &backup_id, "COMPLETED", None).await?;

                if let Err(err) = uploader.upload_system_file(enc_file_path, latest_key).await {
                    warn!(
                        "metadata backup latest pointer update failed for {}: {}",
                        uploader.provider_name(),
                        err
                    );
                }
            }
            Err(err) => {
                db::update_metadata_backup_status(
                    db_pool,
                    &backup_id,
                    "FAILED",
                    Some(&err.to_string()),
                )
                .await?;
            }
        }
    }

    if successful_uploads == 0 {
        return Err(DisasterRecoveryError::NoSuccessfulUploads);
    }

    Ok(())
}

pub async fn run_metadata_backup_now(
    db_pool: &SqlitePool,
    provider_manager: &MetadataBackupProviderManager,
    master_key: &[u8],
) -> Result<(), DisasterRecoveryError> {
    let temp_enc_path = temporary_encrypted_backup_path();
    let create_result =
        create_encrypted_metadata_snapshot(db_pool, &temp_enc_path, master_key).await;

    if let Err(err) = create_result {
        let _ = secure_delete(&temp_enc_path).await;
        return Err(err);
    }

    let upload_result = upload_metadata_backup(db_pool, provider_manager, &temp_enc_path).await;
    let cleanup_result = secure_delete(&temp_enc_path).await;

    if let Err(err) = cleanup_result {
        warn!(
            "metadata backup temp cleanup failed for {}: {}",
            temp_enc_path.display(),
            err
        );
    }

    upload_result
}

#[allow(dead_code)]
fn decrypt_metadata_backup(
    encoded: &[u8],
    passphrase: &str,
) -> Result<Vec<u8>, DisasterRecoveryError> {
    if encoded.len() < METADATA_BACKUP_HEADER_FIXED_LEN + METADATA_BACKUP_TAG_LEN {
        return Err(DisasterRecoveryError::InvalidBackupFormat("file too short"));
    }

    let magic_end = METADATA_BACKUP_MAGIC.len();
    if &encoded[..magic_end] != METADATA_BACKUP_MAGIC {
        return Err(DisasterRecoveryError::InvalidBackupFormat("magic mismatch"));
    }

    let version = encoded[magic_end];
    if version != METADATA_BACKUP_VERSION {
        return Err(DisasterRecoveryError::InvalidBackupFormat(
            "unsupported backup version",
        ));
    }

    let mut cursor = magic_end + 1;
    let salt_len = u16::from_le_bytes(
        encoded[cursor..cursor + 2]
            .try_into()
            .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("salt_len"))?,
    ) as usize;
    cursor += 2;

    let parameter_set_version = u32::from_le_bytes(
        encoded[cursor..cursor + 4]
            .try_into()
            .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("parameter_set_version"))?,
    );
    cursor += 4;
    let memory_cost_kib = u32::from_le_bytes(
        encoded[cursor..cursor + 4]
            .try_into()
            .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("memory_cost_kib"))?,
    );
    cursor += 4;
    let time_cost = u32::from_le_bytes(
        encoded[cursor..cursor + 4]
            .try_into()
            .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("time_cost"))?,
    );
    cursor += 4;
    let lanes = u32::from_le_bytes(
        encoded[cursor..cursor + 4]
            .try_into()
            .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("lanes"))?,
    );
    cursor += 4;

    let nonce: [u8; METADATA_BACKUP_NONCE_LEN] = encoded
        .get(cursor..cursor + METADATA_BACKUP_NONCE_LEN)
        .ok_or(DisasterRecoveryError::InvalidBackupFormat("nonce"))?
        .try_into()
        .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("nonce"))?;
    cursor += METADATA_BACKUP_NONCE_LEN;

    let salt = encoded
        .get(cursor..cursor + salt_len)
        .ok_or(DisasterRecoveryError::InvalidBackupFormat("salt"))?
        .to_vec();
    cursor += salt_len;

    if encoded.len() < cursor + METADATA_BACKUP_TAG_LEN {
        return Err(DisasterRecoveryError::InvalidBackupFormat(
            "ciphertext missing",
        ));
    }

    let ciphertext_end = encoded.len() - METADATA_BACKUP_TAG_LEN;
    let mut plaintext = encoded[cursor..ciphertext_end].to_vec();
    let tag: [u8; METADATA_BACKUP_TAG_LEN] = encoded[ciphertext_end..]
        .try_into()
        .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("tag"))?;

    let root_keys = derive_root_keys(
        passphrase.as_bytes(),
        &RootKdfParams::new(
            parameter_set_version,
            salt,
            memory_cost_kib,
            time_cost,
            lanes,
        ),
    )
    .map_err(|_| DisasterRecoveryError::BackupDecryptFailed)?;
    let metadata_backup_key = derive_metadata_backup_key(&root_keys.master_key)?;
    let cipher = Aes256Gcm::new_from_slice(&metadata_backup_key)
        .map_err(|_| DisasterRecoveryError::BackupDecryptFailed)?;

    cipher
        .decrypt_in_place_detached(
            Nonce::from_slice(&nonce),
            &[],
            &mut plaintext,
            aes_gcm::Tag::from_slice(&tag),
        )
        .map_err(|_| DisasterRecoveryError::BackupDecryptFailed)?;

    Ok(plaintext)
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

fn normalize_encrypted_output_path(output_path: &Path) -> Result<PathBuf, DisasterRecoveryError> {
    if output_path.as_os_str().is_empty() {
        return Err(DisasterRecoveryError::InvalidOutputPath("empty path"));
    }
    if output_path.is_dir() {
        return Err(DisasterRecoveryError::InvalidOutputPath(
            "path points to a directory",
        ));
    }

    let output_string = output_path.to_string_lossy();
    if output_string.to_ascii_lowercase().ends_with(".enc") {
        Ok(output_path.to_path_buf())
    } else {
        Ok(PathBuf::from(format!("{output_string}.enc")))
    }
}

fn temporary_plaintext_snapshot_path(output_enc_path: &Path) -> PathBuf {
    let mut temp = output_enc_path.to_path_buf();
    let file_name = output_enc_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("metadata-snapshot.db.enc");
    temp.set_file_name(format!("{file_name}.tmp.db"));
    temp
}

pub fn temporary_encrypted_backup_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "omnidrive-metadata-backup-{}.db.enc",
        unix_timestamp_ms().unwrap_or(0)
    ))
}

fn unix_timestamp_ms() -> Result<u64, DisasterRecoveryError> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| DisasterRecoveryError::InvalidOutputPath("system clock before unix epoch"))?
        .as_millis() as u64)
}

#[allow(dead_code)]
impl MetadataBackupDownloadProvider {
    async fn from_provider_config(
        config: ProviderConfig,
    ) -> Result<Self, DisasterRecoveryError> {
        let timeout_config = aws_config::timeout::TimeoutConfig::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(90))
            .operation_attempt_timeout(Duration::from_secs(90))
            .operation_timeout(Duration::from_secs(120))
            .build();

        let shared_config = crate::aws_http::load_shared_config(
            aws_sdk_s3::config::Region::new(config.region.clone()),
            timeout_config.clone(),
        )
        .await;

        let s3_config = aws_sdk_s3::config::Builder::from(&shared_config)
            .credentials_provider(aws_sdk_s3::config::Credentials::new(
                config.access_key_id,
                config.secret_access_key,
                None,
                None,
                config.provider_name,
            ))
            .endpoint_url(config.endpoint)
            .region(aws_sdk_s3::config::Region::new(config.region))
            .timeout_config(timeout_config)
            .force_path_style(config.force_path_style)
            .build();

        Ok(Self {
            provider_name: config.provider_name,
            bucket: config.bucket,
            client: aws_sdk_s3::Client::from_conf(s3_config),
        })
    }

    async fn download_bytes(&self, object_key: &str) -> Result<Vec<u8>, DisasterRecoveryError> {
        let response = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(object_key)
            .send()
            .await
            .map_err(|err| {
                DisasterRecoveryError::Uploader(UploaderError::Upload {
                    provider: self.provider_name,
                    operation: "get_object",
                    details: format!("{err}"),
                })
            })?;

        let body = response
            .body
            .collect()
            .await
            .map_err(|err| DisasterRecoveryError::Uploader(UploaderError::Upload {
                provider: self.provider_name,
                operation: "get_object",
                details: format!("{err}"),
            }))?;

        Ok(body.into_bytes().to_vec())
    }
}

impl LocalMetadataBackupStore {
    fn provider_name(&self) -> &'static str {
        "local-metadata-store"
    }

    async fn upload_file(
        &self,
        source_path: &Path,
        object_key: &str,
    ) -> Result<(), DisasterRecoveryError> {
        let target_path = self.root.join(object_key.replace('/', "\\"));
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::copy(source_path, &target_path).await?;
        Ok(())
    }

    async fn download_bytes(&self, object_key: &str) -> Result<Vec<u8>, DisasterRecoveryError> {
        let source_path = self.root.join(object_key.replace('/', "\\"));
        Ok(fs::read(source_path).await?)
    }
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

    #[tokio::test]
    async fn encrypts_snapshot_into_expected_binary_format() -> Result<(), Box<dyn std::error::Error>>
    {
        let test_root = env::temp_dir().join(format!(
            "omnidrive-dr-encrypt-test-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        let input_path = test_root.join("snapshot.db");
        let output_path = test_root.join("snapshot.db.enc");
        let passphrase = "restore-passphrase";
        let kdf_params = RootKdfParams::new(1, vec![0x22; 16], 65_536, 3, 1);
        let root_keys = derive_root_keys(passphrase.as_bytes(), &kdf_params)?;
        let master_key = root_keys.master_key;

        fs::create_dir_all(&test_root).await?;
        fs::write(&input_path, b"sqlite-snapshot-payload").await?;

        encrypt_metadata_snapshot(&input_path, &output_path, &master_key, &kdf_params).await?;

        let encoded = fs::read(&output_path).await?;
        assert!(encoded.len() > METADATA_BACKUP_MAGIC.len() + 1 + METADATA_BACKUP_NONCE_LEN);
        assert_eq!(&encoded[..METADATA_BACKUP_MAGIC.len()], METADATA_BACKUP_MAGIC);
        assert_eq!(encoded[METADATA_BACKUP_MAGIC.len()], METADATA_BACKUP_VERSION);
        let decrypted = decrypt_metadata_backup(&encoded, passphrase)?;
        assert_eq!(decrypted, b"sqlite-snapshot-payload");

        let _ = fs::remove_dir_all(&test_root).await;
        Ok(())
    }
}
