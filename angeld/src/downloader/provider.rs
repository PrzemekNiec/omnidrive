use super::util::*;
use super::*;
use crate::cache::CacheManager;
use crate::cloud_guard;
use crate::cloud_guard::GuardOperation;
use crate::config::AppConfig;
use crate::onboarding;
use crate::peer::PeerClient;
use crate::uploader::ProviderConfig;
use crate::vault::VaultKeyStore;
use aws_config::timeout::TimeoutConfig;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::Credentials;
use aws_sdk_s3::config::Region;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;
use std::time::Instant;
use tokio::fs;
use tokio::sync::Mutex;

impl Downloader {
    pub fn has_remote_providers(&self) -> bool {
        self.providers
            .read()
            .map(|providers| !providers.is_empty())
            .unwrap_or(false)
    }

    pub async fn from_env(
        pool: SqlitePool,
        vault_keys: VaultKeyStore,
    ) -> Result<Self, DownloaderError> {
        let _ = dotenvy::dotenv();

        let download_spool_dir =
            env_path("OMNIDRIVE_DOWNLOAD_SPOOL_DIR", ".omnidrive/download-spool");
        let provider_timeout = duration_from_env("OMNIDRIVE_DOWNLOAD_TIMEOUT_MS", 120_000);

        let mut configs = Vec::new();
        if let Ok(config) = ProviderConfig::from_r2_env() {
            configs.push(config);
        }
        if let Ok(config) = ProviderConfig::from_scaleway_env() {
            configs.push(config);
        }
        if let Ok(config) = ProviderConfig::from_b2_env() {
            configs.push(config);
        }

        Self::from_provider_configs(
            pool,
            vault_keys,
            download_spool_dir,
            provider_timeout,
            configs,
        )
        .await
    }

    pub async fn from_provider_configs(
        pool: SqlitePool,
        vault_keys: VaultKeyStore,
        download_spool_dir: impl Into<PathBuf>,
        provider_timeout: Duration,
        configs: Vec<ProviderConfig>,
    ) -> Result<Self, DownloaderError> {
        let download_spool_dir = download_spool_dir.into();
        fs::create_dir_all(&download_spool_dir).await?;
        let cache = CacheManager::from_env(pool.clone(), vault_keys.clone()).await?;
        let app_config = AppConfig::from_env();

        let mut providers = HashMap::new();
        for config in configs {
            let provider = DownloadProvider::from_provider_config(config).await?;
            providers.insert(provider.provider_name.to_string(), provider);
        }

        Ok(Self {
            pool,
            vault_keys,
            download_spool_dir,
            cache,
            providers: Arc::new(RwLock::new(providers)),
            provider_timeout,
            app_config,
            prefetch_state: Arc::new(Mutex::new(HashMap::new())),
            peer_client: Arc::new(Mutex::new(None)),
            pack_download_locks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn reload_active_providers_from_db(&self) -> Result<Vec<String>, DownloaderError> {
        let configs = onboarding::get_active_provider_configs(&self.pool)
            .await
            .map_err(|err| DownloaderError::RuntimeConfig(err.to_string()))?;

        let provider_names: Vec<String> = configs
            .iter()
            .map(|config| config.provider_name.to_string())
            .collect();

        let mut providers = HashMap::new();
        for config in configs {
            let provider = DownloadProvider::from_provider_config(config).await?;
            providers.insert(provider.provider_name.to_string(), provider);
        }

        let mut lock = self
            .providers
            .write()
            .map_err(|_| DownloaderError::RuntimeConfig("provider lock is poisoned".to_string()))?;
        *lock = providers;

        Ok(provider_names)
    }

    pub async fn set_peer_client(&self, peer_client: PeerClient) {
        let mut slot = self.peer_client.lock().await;
        *slot = Some(peer_client);
    }

    pub(super) async fn probe_latency(
        &self,
        provider: &DownloadProvider,
        object_key: &str,
    ) -> Result<Duration, DownloaderError> {
        match cloud_guard::current_decision(
            &self.pool,
            GuardOperation::Read {
                count: 1,
                estimated_egress_bytes: 0,
            },
        )
        .await
        {
            Ok(cloud_guard::GuardDecision::Allowed) => {}
            Ok(cloud_guard::GuardDecision::DryRun { message }) => {
                return Err(DownloaderError::CloudGuard(message));
            }
            Ok(cloud_guard::GuardDecision::Suspended { reason })
            | Ok(cloud_guard::GuardDecision::QuotaExceeded { reason }) => {
                return Err(DownloaderError::CloudGuard(reason));
            }
            Err(err) => return Err(DownloaderError::CloudGuard(err.to_string())),
        }
        let start = Instant::now();
        tokio::time::timeout(
            self.provider_timeout,
            provider
                .client
                .head_object()
                .bucket(&provider.bucket)
                .key(object_key)
                .send(),
        )
        .await
        .map_err(|_| DownloaderError::InvalidPackRecord("provider probe timed out"))?
        .map_err(|_| DownloaderError::InvalidPackRecord("provider probe failed"))?;
        Ok(start.elapsed())
    }
}

impl DownloadProvider {
    async fn from_provider_config(config: ProviderConfig) -> Result<Self, DownloaderError> {
        let provider_name = config.provider_name;
        let operation_timeout = duration_from_env("OMNIDRIVE_DOWNLOAD_TIMEOUT_MS", 120_000);
        let operation_attempt_timeout =
            duration_from_env("OMNIDRIVE_DOWNLOAD_ATTEMPT_TIMEOUT_MS", 90_000);
        let connect_timeout = duration_from_env("OMNIDRIVE_DOWNLOAD_CONNECT_TIMEOUT_MS", 10_000);
        let read_timeout = duration_from_env("OMNIDRIVE_DOWNLOAD_READ_TIMEOUT_MS", 90_000);
        let timeout_config = TimeoutConfig::builder()
            .connect_timeout(connect_timeout)
            .read_timeout(read_timeout)
            .operation_attempt_timeout(operation_attempt_timeout)
            .operation_timeout(operation_timeout)
            .build();

        let shared_config = crate::aws_http::load_shared_config(
            Region::new(config.region.clone()),
            timeout_config.clone(),
            config.endpoint.starts_with("http://"),
        )
        .await;

        let s3_config = aws_sdk_s3::config::Builder::from(&shared_config)
            .credentials_provider(Credentials::new(
                config.access_key_id,
                config.secret_access_key,
                None,
                None,
                provider_name,
            ))
            .endpoint_url(config.endpoint)
            .region(Region::new(config.region))
            .timeout_config(timeout_config)
            .force_path_style(config.force_path_style)
            .build();

        Ok(Self {
            provider_name,
            bucket: config.bucket,
            client: Client::from_conf(s3_config),
        })
    }
}
