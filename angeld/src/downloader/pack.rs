use super::chunk::*;
use super::util::*;
use super::*;
use crate::cloud_guard;
use crate::cloud_guard::GuardOperation;
use crate::db;
use crate::db::StorageMode;
use crate::packer::DATA_SHARDS;
use crate::packer::LOCAL_PACK_EXTENSION;
use crate::packer::TOTAL_SHARDS;
use crate::packer::local_pack_path;
use crate::secure_fs::write_ephemeral_bytes;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::sync::Mutex;

impl Downloader {
    pub(super) async fn download_pack(
        &self,
        pack_id: &str,
    ) -> Result<RestoredPackSource, DownloaderError> {
        let pack = db::get_pack(&self.pool, pack_id)
            .await?
            .ok_or_else(|| DownloaderError::PackMissing(pack_id.to_string()))?;
        let storage_mode = StorageMode::from_str(&pack.storage_mode);
        if storage_mode == StorageMode::LocalOnly {
            let local_path = local_pack_path(
                &env_path("OMNIDRIVE_SPOOL_DIR", ".omnidrive/spool"),
                pack_id,
            );
            if !fs::try_exists(&local_path).await? {
                return Err(DownloaderError::InvalidPackRecord(
                    "local-only pack manifest missing",
                ));
            }
            return Ok(RestoredPackSource {
                pack_id: pack_id.to_string(),
                providers: vec!["local-only".to_string()],
                local_path,
            });
        }

        // Deduplicate concurrent downloads of the same remote pack.
        // Only one task downloads from B2 at a time; all other concurrent
        // FetchData callbacks for the same pack_id wait for the lock, then
        // short-circuit via the spool file that the first downloader wrote.
        let local_path = self
            .download_spool_dir
            .join(format!("{pack_id}.{LOCAL_PACK_EXTENSION}"));
        let pack_lock = {
            let mut map = self.pack_download_locks.lock().await;
            map.entry(pack_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _pack_guard = pack_lock.lock().await;
        if fs::try_exists(&local_path).await? {
            return Ok(RestoredPackSource {
                pack_id: pack_id.to_string(),
                providers: vec!["spool-cache".to_string()],
                local_path,
            });
        }

        let shards = db::get_pack_shards(&self.pool, pack_id).await?;
        if shards.is_empty() {
            return Err(DownloaderError::NoPackShards(pack_id.to_string()));
        }

        let mut candidates = Vec::new();
        let providers_snapshot = self
            .providers
            .read()
            .map_err(|_| DownloaderError::RuntimeConfig("provider lock is poisoned".to_string()))?
            .clone();

        for shard in shards {
            let Some(provider) = providers_snapshot.get(&shard.provider) else {
                continue;
            };

            let latency = self.probe_latency(provider, &shard.object_key).await.ok();
            candidates.push((provider, shard, latency));
        }

        if candidates.is_empty() {
            return Err(DownloaderError::ShardDownloadFailed {
                pack_id: pack_id.to_string(),
                errors: vec![format!(
                    "no configured providers available for pack {pack_id}"
                )],
            });
        }

        candidates.sort_by_key(|(_, shard, latency)| {
            (
                latency.unwrap_or(Duration::MAX),
                if shard.status == "COMPLETED" { 0 } else { 1 },
            )
        });

        let required_shards = match storage_mode {
            StorageMode::Ec2_1 => DATA_SHARDS,
            StorageMode::SingleReplica => 1,
            StorageMode::LocalOnly => 0,
        };
        let shard_slots = if storage_mode == StorageMode::SingleReplica {
            1
        } else {
            TOTAL_SHARDS
        };
        let mut shard_bytes: Vec<Option<Vec<u8>>> = vec![None; shard_slots];
        let mut downloaded_from = Vec::new();
        let mut errors = Vec::new();

        for (provider, shard, _) in candidates {
            let shard_index = usize::try_from(shard.shard_index)
                .map_err(|_| DownloaderError::NumericOverflow("shard index"))?;
            if shard_index >= shard_slots || shard_bytes[shard_index].is_some() {
                continue;
            }

            match self
                .download_shard(
                    pack_id,
                    provider,
                    &shard.object_key,
                    shard_index,
                    shard.size,
                )
                .await
            {
                Ok(local_path) => {
                    let bytes = fs::read(&local_path).await?;
                    shard_bytes[shard_index] = Some(bytes);
                    downloaded_from.push(provider.provider_name.to_string());
                    if shard_bytes.iter().flatten().count() >= required_shards {
                        break;
                    }
                }
                Err(err) => {
                    errors.push(format!(
                        "{} shard {}: {}",
                        shard.provider, shard.shard_index, err
                    ));
                }
            }
        }

        if shard_bytes.iter().flatten().count() < required_shards {
            return Err(DownloaderError::ShardDownloadFailed {
                pack_id: pack_id.to_string(),
                errors,
            });
        }

        let ciphertext = match storage_mode {
            StorageMode::Ec2_1 => reconstruct_ciphertext(&pack, &mut shard_bytes)?,
            StorageMode::SingleReplica => shard_bytes.into_iter().next().flatten().ok_or(
                DownloaderError::InvalidPackRecord("single replica missing shard"),
            )?,
            StorageMode::LocalOnly => unreachable!("local-only handled above"),
        };
        let manifest_bytes = build_manifest_bytes(&pack, &ciphertext)?;
        write_ephemeral_bytes(&local_path, &manifest_bytes)
            .await
            .map_err(|err| DownloaderError::Io(std::io::Error::other(err.to_string())))?;

        // G.2: record download traffic for stats chart
        let _ = db::record_traffic(&self.pool, 0, manifest_bytes.len() as i64).await;

        Ok(RestoredPackSource {
            pack_id: pack_id.to_string(),
            providers: downloaded_from,
            local_path,
        })
    }

    async fn download_shard(
        &self,
        pack_id: &str,
        provider: &DownloadProvider,
        object_key: &str,
        shard_index: usize,
        estimated_size: i64,
    ) -> Result<PathBuf, String> {
        match cloud_guard::current_decision(
            &self.pool,
            GuardOperation::Read {
                count: 1,
                estimated_egress_bytes: estimated_size.max(0),
            },
        )
        .await
        {
            Ok(cloud_guard::GuardDecision::Allowed) => {}
            Ok(cloud_guard::GuardDecision::DryRun { .. }) => {
                let estimated_mib = (estimated_size.max(0) as f64) / (1024.0 * 1024.0);
                let monthly_rate = self
                    .app_config
                    .provider_cost_per_gib_month(provider.provider_name);
                let estimated_cost =
                    ((estimated_size.max(0) as f64) / 1_073_741_824.0) * monthly_rate;
                return Err(format!(
                    "[DRY-RUN] Would download shard {} for pack {} from {} (~{:.2} MiB, est. monthly storage delta ${:.5})",
                    shard_index, pack_id, provider.provider_name, estimated_mib, estimated_cost
                ));
            }
            Ok(cloud_guard::GuardDecision::Suspended { reason })
            | Ok(cloud_guard::GuardDecision::QuotaExceeded { reason }) => {
                return Err(reason);
            }
            Err(err) => return Err(format!("cloud guard failed: {err}")),
        }

        let response = tokio::time::timeout(
            self.provider_timeout,
            provider
                .client
                .get_object()
                .bucket(&provider.bucket)
                .key(object_key)
                .send(),
        )
        .await
        .map_err(|_| format!("{} download timed out", provider.provider_name))?
        .map_err(|err| {
            format!(
                "{} get_object failed: {}",
                provider.provider_name,
                format_error_details(&err)
            )
        })?;

        let body = response.body.collect().await.map_err(|err| {
            format!(
                "{} body read failed: {}",
                provider.provider_name,
                format_error_details(&err)
            )
        })?;

        let bytes = body.into_bytes();
        let actual_size = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
        let delta = actual_size.saturating_sub(estimated_size.max(0));
        if delta != 0
            && let Err(err) = cloud_guard::reconcile_read_bytes(&self.pool, delta).await
        {
            tracing::warn!(
                "downloader egress reconcile failed pack={} shard={}: {}",
                pack_id,
                shard_index,
                err
            );
        }

        let local_path = self
            .download_spool_dir
            .join(format!("{pack_id}.download-shard{shard_index}"));
        write_ephemeral_bytes(&local_path, &bytes)
            .await
            .map_err(|err| err.to_string())?;

        Ok(local_path)
    }

    pub(super) async fn try_fetch_chunk_from_peer(
        &self,
        inode_id: i64,
        revision_id: i64,
        inode_path: &str,
        cache_key: &str,
        chunk: &db::FileChunkLocation,
        is_prefetched: bool,
    ) -> Result<Option<Vec<u8>>, DownloaderError> {
        let peer_client = { self.peer_client.lock().await.clone() };
        let Some(peer_client) = peer_client else {
            return Ok(None);
        };

        let Some(bytes) = peer_client
            .fetch_chunk(&chunk.chunk_id)
            .await
            .map_err(|err| {
                DownloaderError::Io(std::io::Error::other(format!("peer fetch failed: {err}")))
            })?
        else {
            return Ok(None);
        };

        self.cache
            .put_chunk(
                inode_id,
                revision_id,
                chunk.chunk_index,
                &chunk.pack_id,
                inode_path,
                &bytes,
                is_prefetched,
            )
            .await?;
        if let Some(cached) = self.cache.get_chunk(cache_key).await? {
            return Ok(Some(cached));
        }
        Ok(Some(bytes))
    }
}
