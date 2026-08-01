use super::chunk::*;
use super::util::*;
use super::*;
use crate::cache::CacheManager;
use crate::db;
use omnidrive_core::crypto::KeyBytes;
use omnidrive_core::layout::CHUNK_RECORD_MAGIC;
use omnidrive_core::layout::ChunkRecordPrefix;
use secrecy::ExposeSecret;
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;
use tokio::fs::File;
use tokio::io::AsyncSeekExt;
use tokio::io::AsyncWriteExt;

impl Downloader {
    /// Resolves the V2 key from the pack that holds the chunk. `None` means the pack
    /// predates envelope encryption and the V1 vault key applies instead.
    async fn pack_dek(&self, pack_id: &str) -> Option<KeyBytes> {
        self.vault_keys
            .dek_for_pack(&self.pool, pack_id)
            .await
            .ok()
            .map(|secret| secret.expose_secret().clone())
    }

    pub async fn restore_file(
        &self,
        inode_id: i64,
        output_path: impl AsRef<Path>,
    ) -> Result<RestoreResult, DownloaderError> {
        let output_path = output_path.as_ref().to_path_buf();
        let vault_key = self.vault_keys.vault_key_for_v1_read(&self.pool).await?;
        let chunk_locations = db::get_file_chunk_locations(&self.pool, inode_id).await?;
        if chunk_locations.is_empty() {
            return Err(DownloaderError::NoChunksForInode(inode_id));
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let mut output = File::create(&output_path).await?;
        let mut current_offset = 0u64;
        let mut downloaded_packs = HashMap::<String, RestoredPackSource>::new();

        for chunk in chunk_locations {
            let source = if let Some(existing) = downloaded_packs.get(&chunk.pack_id) {
                existing.clone()
            } else {
                let downloaded = self.download_pack(&chunk.pack_id).await?;
                downloaded_packs.insert(chunk.pack_id.clone(), downloaded.clone());
                downloaded
            };

            let pack_bytes = fs::read(&source.local_path).await?;
            let dek_option = self.pack_dek(&chunk.pack_id).await;
            let plaintext =
                decrypt_chunk_record(&pack_bytes, &chunk, &vault_key, dek_option.as_ref())?;

            let desired_offset = to_u64(chunk.file_offset, "file offset")?;
            if current_offset != desired_offset {
                output
                    .seek(std::io::SeekFrom::Start(desired_offset))
                    .await?;
                current_offset = desired_offset;
            }

            output.write_all(&plaintext).await?;
            current_offset = current_offset
                .checked_add(plaintext.len() as u64)
                .ok_or(DownloaderError::NumericOverflow("bytes written"))?;
        }

        output.flush().await?;

        Ok(RestoreResult {
            inode_id,
            output_path,
            bytes_written: current_offset,
            pack_sources: downloaded_packs.into_values().collect(),
        })
    }

    pub async fn read_range(
        &self,
        inode_id: i64,
        revision_id: i64,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, DownloaderError> {
        let revision = db::get_file_revision(&self.pool, inode_id, revision_id)
            .await?
            .ok_or(DownloaderError::NoChunksForInode(inode_id))?;

        if length == 0 {
            return Ok(Vec::new());
        }

        let end_offset = offset
            .checked_add(length)
            .ok_or(DownloaderError::NumericOverflow("range end"))?;
        let start_i64 =
            i64::try_from(offset).map_err(|_| DownloaderError::NumericOverflow("range start"))?;
        let end_i64 =
            i64::try_from(end_offset).map_err(|_| DownloaderError::NumericOverflow("range end"))?;

        let chunk_locations = db::get_revision_chunk_locations_in_range(
            &self.pool,
            inode_id,
            revision_id,
            start_i64,
            end_i64,
        )
        .await?;
        if chunk_locations.is_empty() {
            return Err(DownloaderError::NoChunksForInode(inode_id));
        }

        let inode_path = db::get_inode_path(&self.pool, inode_id)
            .await?
            .unwrap_or_else(|| format!("inode/{inode_id}"));
        let vault_key = self.vault_keys.vault_key_for_v1_read(&self.pool).await?;
        let mut downloaded_packs = HashMap::<String, RestoredPackSource>::new();
        let mut result = Vec::with_capacity(
            usize::try_from(length)
                .map_err(|_| DownloaderError::NumericOverflow("range length"))?,
        );
        let first_chunk_index = chunk_locations.first().map(|chunk| chunk.chunk_index);
        let last_chunk_index = chunk_locations.last().map(|chunk| chunk.chunk_index);

        for chunk in chunk_locations {
            let plaintext = self
                .load_plaintext_chunk(
                    inode_id,
                    revision_id,
                    &inode_path,
                    &vault_key,
                    &mut downloaded_packs,
                    &chunk,
                    false,
                )
                .await?;

            let chunk_start = to_u64(chunk.file_offset, "file offset")?;
            let chunk_end = chunk_start
                .checked_add(plaintext.len() as u64)
                .ok_or(DownloaderError::NumericOverflow("chunk end"))?;
            let slice_start = offset.max(chunk_start);
            let slice_end = end_offset.min(chunk_end);

            if slice_start >= slice_end {
                continue;
            }

            let local_start = usize::try_from(slice_start - chunk_start)
                .map_err(|_| DownloaderError::NumericOverflow("slice start"))?;
            let local_end = usize::try_from(slice_end - chunk_start)
                .map_err(|_| DownloaderError::NumericOverflow("slice end"))?;
            result.extend_from_slice(&plaintext[local_start..local_end]);

            if result.len()
                >= usize::try_from(length)
                    .map_err(|_| DownloaderError::NumericOverflow("range length"))?
            {
                break;
            }
        }

        let target_len = usize::try_from(length)
            .map_err(|_| DownloaderError::NumericOverflow("range length"))?;
        if result.len() > target_len {
            result.truncate(target_len);
        }

        self.maybe_schedule_prefetch(
            inode_id,
            revision_id,
            revision.size,
            &inode_path,
            first_chunk_index,
            last_chunk_index,
        )
        .await;

        Ok(result)
    }

    /// Streaming variant of `read_range`.  Instead of collecting all bytes
    /// into a single `Vec<u8>`, this calls `on_chunk(absolute_offset, slice)`
    /// for each decrypted chunk piece.  The caller can feed each piece
    /// straight to Windows via `CfExecute` and the chunk is dropped before
    /// the next one is loaded — peak RAM stays at ~1 chunk (≤ 4 MB).
    pub async fn read_range_streamed<F>(
        &self,
        inode_id: i64,
        revision_id: i64,
        offset: u64,
        length: u64,
        mut on_chunk: F,
    ) -> Result<(), DownloaderError>
    where
        F: FnMut(u64, &[u8]) -> Result<(), DownloaderError>,
    {
        let revision = db::get_file_revision(&self.pool, inode_id, revision_id)
            .await?
            .ok_or(DownloaderError::NoChunksForInode(inode_id))?;

        if length == 0 {
            return Ok(());
        }

        let end_offset = offset
            .checked_add(length)
            .ok_or(DownloaderError::NumericOverflow("range end"))?;
        let start_i64 =
            i64::try_from(offset).map_err(|_| DownloaderError::NumericOverflow("range start"))?;
        let end_i64 =
            i64::try_from(end_offset).map_err(|_| DownloaderError::NumericOverflow("range end"))?;

        let chunk_locations = db::get_revision_chunk_locations_in_range(
            &self.pool,
            inode_id,
            revision_id,
            start_i64,
            end_i64,
        )
        .await?;
        if chunk_locations.is_empty() {
            return Err(DownloaderError::NoChunksForInode(inode_id));
        }

        let inode_path = db::get_inode_path(&self.pool, inode_id)
            .await?
            .unwrap_or_else(|| format!("inode/{inode_id}"));
        let vault_key = self.vault_keys.vault_key_for_v1_read(&self.pool).await?;
        let mut downloaded_packs = HashMap::<String, RestoredPackSource>::new();
        let first_chunk_index = chunk_locations.first().map(|chunk| chunk.chunk_index);
        let last_chunk_index = chunk_locations.last().map(|chunk| chunk.chunk_index);
        let mut bytes_emitted: u64 = 0;

        for chunk in chunk_locations {
            let plaintext = self
                .load_plaintext_chunk(
                    inode_id,
                    revision_id,
                    &inode_path,
                    &vault_key,
                    &mut downloaded_packs,
                    &chunk,
                    false,
                )
                .await?;

            let chunk_start = to_u64(chunk.file_offset, "file offset")?;
            let chunk_end = chunk_start
                .checked_add(plaintext.len() as u64)
                .ok_or(DownloaderError::NumericOverflow("chunk end"))?;
            let slice_start = offset.max(chunk_start);
            let slice_end = end_offset.min(chunk_end);

            if slice_start >= slice_end {
                continue;
            }

            let local_start = usize::try_from(slice_start - chunk_start)
                .map_err(|_| DownloaderError::NumericOverflow("slice start"))?;
            let local_end = usize::try_from(slice_end - chunk_start)
                .map_err(|_| DownloaderError::NumericOverflow("slice end"))?;

            on_chunk(slice_start, &plaintext[local_start..local_end])?;

            bytes_emitted += (local_end - local_start) as u64;
            if bytes_emitted >= length {
                break;
            }
            // `plaintext` is dropped here — RAM freed before next chunk.
        }

        self.maybe_schedule_prefetch(
            inode_id,
            revision_id,
            revision.size,
            &inode_path,
            first_chunk_index,
            last_chunk_index,
        )
        .await;

        Ok(())
    }

    pub async fn read_plaintext_chunk_by_id(
        &self,
        chunk_id: &[u8],
    ) -> Result<Option<Vec<u8>>, DownloaderError> {
        let Some(chunk) = db::get_chunk_lookup_by_chunk_id(&self.pool, chunk_id).await? else {
            return Ok(None);
        };
        let inode_path = db::get_inode_path(&self.pool, chunk.inode_id)
            .await?
            .unwrap_or_else(|| format!("inode/{}", chunk.inode_id));
        let cache_key = CacheManager::cache_key(chunk.revision_id, chunk.chunk_index);
        if let Some(bytes) = self.cache.get_chunk(&cache_key).await? {
            return Ok(Some(bytes));
        }

        let vault_key = self.vault_keys.vault_key_for_v1_read(&self.pool).await?;
        let dek_option = self.pack_dek(&chunk.pack_id).await;
        let mut downloaded_packs = HashMap::<String, RestoredPackSource>::new();
        let file_chunk = db::FileChunkLocation {
            chunk_id: chunk.chunk_id,
            chunk_index: chunk.chunk_index,
            file_offset: chunk.file_offset,
            size: chunk.size,
            pack_id: chunk.pack_id,
            pack_offset: chunk.pack_offset,
            encrypted_size: chunk.encrypted_size,
        };
        let source = if let Some(existing) = downloaded_packs.get(&file_chunk.pack_id) {
            existing.clone()
        } else {
            let downloaded = self.download_pack(&file_chunk.pack_id).await?;
            downloaded_packs.insert(file_chunk.pack_id.clone(), downloaded.clone());
            downloaded
        };
        let pack_bytes = fs::read(&source.local_path).await?;
        let bytes =
            decrypt_chunk_record(&pack_bytes, &file_chunk, &vault_key, dek_option.as_ref())?;
        self.cache
            .put_chunk(
                chunk.inode_id,
                chunk.revision_id,
                file_chunk.chunk_index,
                &file_chunk.pack_id,
                &inode_path,
                &bytes,
                false,
            )
            .await?;
        Ok(Some(bytes))
    }

    /// Fetch a chunk's raw encrypted bytes (nonce + ciphertext + tag) without decryption.
    /// Used by the sharing system to serve encrypted chunks to browser-based decryptors.
    pub async fn get_encrypted_chunk_bytes(
        &self,
        chunk: &db::FileChunkLocation,
    ) -> Result<EncryptedChunkBytes, DownloaderError> {
        let source = self.download_pack(&chunk.pack_id).await?;
        let pack_bytes = tokio::fs::read(&source.local_path).await?;

        let pack_offset = to_usize(chunk.pack_offset, "pack offset")?;
        let encrypted_size = to_usize(chunk.encrypted_size, "encrypted size")?;
        let record_end = pack_offset
            .checked_add(encrypted_size)
            .ok_or(DownloaderError::NumericOverflow("record end"))?;

        if record_end > pack_bytes.len() || encrypted_size < ChunkRecordPrefix::SIZE {
            return Err(DownloaderError::InvalidPackRecord("record bounds"));
        }

        let record = &pack_bytes[pack_offset..record_end];
        if record[..4] != CHUNK_RECORD_MAGIC {
            return Err(DownloaderError::InvalidPackRecord("chunk magic"));
        }

        let cipher_len = u64::from_be_bytes(
            record[48..56]
                .try_into()
                .map_err(|_| DownloaderError::InvalidPackRecord("cipher_len"))?,
        );
        let cipher_len_usize = usize::try_from(cipher_len)
            .map_err(|_| DownloaderError::NumericOverflow("cipher length"))?;

        let nonce: [u8; 12] = record[56..68]
            .try_into()
            .map_err(|_| DownloaderError::InvalidPackRecord("nonce"))?;

        let ciphertext_start = ChunkRecordPrefix::SIZE;
        let ciphertext_end = ciphertext_start + cipher_len_usize;
        let tag_end = ciphertext_end + ChunkRecordPrefix::GCM_TAG_SIZE;

        if tag_end > record.len() {
            return Err(DownloaderError::InvalidPackRecord("record too short"));
        }

        let ciphertext = record[ciphertext_start..ciphertext_end].to_vec();
        let gcm_tag: [u8; 16] = record[ciphertext_end..tag_end]
            .try_into()
            .map_err(|_| DownloaderError::InvalidPackRecord("gcm tag"))?;

        Ok(EncryptedChunkBytes {
            nonce,
            ciphertext,
            gcm_tag,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn load_plaintext_chunk(
        &self,
        inode_id: i64,
        revision_id: i64,
        inode_path: &str,
        vault_key: &KeyBytes,
        downloaded_packs: &mut HashMap<String, RestoredPackSource>,
        chunk: &db::FileChunkLocation,
        is_prefetched: bool,
    ) -> Result<Vec<u8>, DownloaderError> {
        let cache_key = CacheManager::cache_key(revision_id, chunk.chunk_index);
        if let Some(bytes) = self.cache.get_chunk(&cache_key).await? {
            return Ok(bytes);
        }

        if let Some(bytes) = self
            .try_fetch_chunk_from_peer(
                inode_id,
                revision_id,
                inode_path,
                &cache_key,
                chunk,
                is_prefetched,
            )
            .await?
        {
            return Ok(bytes);
        }

        let source = if let Some(existing) = downloaded_packs.get(&chunk.pack_id) {
            existing.clone()
        } else {
            let downloaded = self.download_pack(&chunk.pack_id).await?;
            downloaded_packs.insert(chunk.pack_id.clone(), downloaded.clone());
            downloaded
        };

        let pack_bytes = fs::read(&source.local_path).await?;
        let dek = self.pack_dek(&chunk.pack_id).await;
        let plaintext = decrypt_chunk_record(&pack_bytes, chunk, vault_key, dek.as_ref())?;
        self.cache
            .put_chunk(
                inode_id,
                revision_id,
                chunk.chunk_index,
                &chunk.pack_id,
                inode_path,
                &plaintext,
                is_prefetched,
            )
            .await?;
        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::packer::DEFAULT_CHUNK_SIZE;
    use crate::packer::Packer;
    use crate::packer::PackerConfig;
    use crate::uploader::ProviderConfig;
    use crate::vault::VaultKeyStore;
    use axum::Router;
    use axum::body::Bytes;
    use axum::extract::Path;
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::put;
    use std::collections::HashMap;
    use std::env;
    use std::sync::Arc;
    use std::time::Duration;
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;
    use tokio::fs;
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;

    type MockObjectStore = Arc<Mutex<HashMap<(String, String), Vec<u8>>>>;

    #[derive(Clone)]
    struct MockS3State {
        objects: MockObjectStore,
        head_delay_by_bucket: Arc<HashMap<String, Duration>>,
    }

    #[tokio::test]
    async fn roundtrip_pack_upload_download_restore_file() -> Result<(), Box<dyn std::error::Error>>
    {
        let test_root = env::temp_dir().join(format!(
            "omnidrive-downloader-test-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        let upload_spool_dir = test_root.join("upload-spool");
        let download_spool_dir = test_root.join("download-spool");
        let source_path = test_root.join("source.bin");
        let restored_path = test_root.join("restored.bin");
        let payload = vec![0x5Au8; DEFAULT_CHUNK_SIZE + 777];

        fs::create_dir_all(&upload_spool_dir).await?;
        fs::create_dir_all(&download_spool_dir).await?;
        fs::write(&source_path, &payload).await?;

        let pool = db::init_db("sqlite::memory:").await?;
        let inode_id = db::create_inode(
            &pool,
            None,
            "source.bin",
            "FILE",
            i64::try_from(payload.len())?,
        )
        .await?;

        // Use real unlock to bootstrap V2 envelope key (required for DEK)
        let vault_keys = VaultKeyStore::new();
        vault_keys.unlock(&pool, "test-passphrase").await?;
        let packer = Packer::new(
            pool.clone(),
            vault_keys.clone(),
            PackerConfig::new(&upload_spool_dir),
        )?;
        let pack_result = packer.pack_file(inode_id, &source_path).await?;

        let state = MockS3State {
            objects: Arc::new(Mutex::new(HashMap::new())),
            head_delay_by_bucket: Arc::new(HashMap::from([
                ("bucket-r2".to_string(), Duration::from_millis(30)),
                ("bucket-scaleway".to_string(), Duration::from_millis(200)),
                ("bucket-b2".to_string(), Duration::from_millis(20)),
            ])),
        };
        let app = Router::new()
            .route(
                "/{*path}",
                put(mock_put_object)
                    .get(mock_get_object)
                    .head(mock_head_object),
            )
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let configs = vec![
            ProviderConfig {
                provider_name: "cloudflare-r2",
                endpoint: format!("http://{addr}"),
                region: "auto".to_string(),
                bucket: "bucket-r2".to_string(),
                access_key_id: "test".to_string(),
                secret_access_key: "test".to_string(),
                force_path_style: true,
            },
            ProviderConfig {
                provider_name: "scaleway",
                endpoint: format!("http://{addr}"),
                region: "pl-waw".to_string(),
                bucket: "bucket-scaleway".to_string(),
                access_key_id: "test".to_string(),
                secret_access_key: "test".to_string(),
                force_path_style: true,
            },
            ProviderConfig {
                provider_name: "backblaze-b2",
                endpoint: format!("http://{addr}"),
                region: "eu-central-003".to_string(),
                bucket: "bucket-b2".to_string(),
                access_key_id: "test".to_string(),
                secret_access_key: "test".to_string(),
                force_path_style: true,
            },
        ];

        for pack_id in &pack_result.pack_ids {
            let shards = db::get_pack_shards(&pool, pack_id).await?;
            for shard in shards {
                if shard.provider == "scaleway" {
                    continue;
                }

                let local_path =
                    download_spool_dir.join(format!("seed-{}-{}.bin", pack_id, shard.shard_index));
                let upload_path =
                    upload_spool_dir.join(format!("{pack_id}.download-shard{}", shard.shard_index));
                let packer_shard_path =
                    upload_spool_dir.join(format!("{pack_id}.shard{}", shard.shard_index));
                let bytes = fs::read(&packer_shard_path).await?;
                fs::write(&local_path, &bytes).await?;
                state.objects.lock().await.insert(
                    (
                        provider_bucket(&shard.provider).to_string(),
                        shard.object_key.clone(),
                    ),
                    bytes,
                );
                let _ = fs::remove_file(&upload_path).await;
            }
        }

        let downloader = Downloader::from_provider_configs(
            pool.clone(),
            vault_keys,
            &download_spool_dir,
            Duration::from_secs(30),
            configs,
        )
        .await?;
        let restored = downloader.restore_file(inode_id, &restored_path).await?;
        let restored_bytes = fs::read(&restored_path).await?;

        assert_eq!(restored_bytes, payload);
        assert_eq!(restored.bytes_written, payload.len() as u64);
        assert_eq!(restored.pack_sources.len(), pack_result.pack_ids.len());
        assert!(
            restored
                .pack_sources
                .iter()
                .all(|source| source.providers.len() >= 2)
        );

        let current_revision = db::get_current_file_revision(&pool, inode_id)
            .await?
            .expect("current revision");
        let range_offset = (DEFAULT_CHUNK_SIZE as u64) - 123;
        let range_length = 512u64;
        let range_bytes = downloader
            .read_range(
                inode_id,
                current_revision.revision_id,
                range_offset,
                range_length,
            )
            .await?;
        assert_eq!(
            range_bytes,
            payload[range_offset as usize..(range_offset + range_length) as usize]
        );

        server.abort();
        let _ = fs::remove_dir_all(&test_root).await;
        Ok(())
    }

    async fn mock_put_object(
        State(state): State<MockS3State>,
        Path(path): Path<String>,
        body: Bytes,
    ) -> impl IntoResponse {
        let (bucket, key) = split_bucket_and_key(&path);
        state
            .objects
            .lock()
            .await
            .insert((bucket.to_string(), key.to_string()), body.to_vec());
        StatusCode::OK
    }

    async fn mock_head_object(
        State(state): State<MockS3State>,
        Path(path): Path<String>,
    ) -> impl IntoResponse {
        let (bucket, key) = split_bucket_and_key(&path);
        if let Some(delay) = state.head_delay_by_bucket.get(bucket) {
            tokio::time::sleep(*delay).await;
        }

        let objects = state.objects.lock().await;
        if let Some(bytes) = objects.get(&(bucket.to_string(), key.to_string())) {
            (
                StatusCode::OK,
                [("content-length", bytes.len().to_string())],
            )
                .into_response()
        } else {
            StatusCode::NOT_FOUND.into_response()
        }
    }

    async fn mock_get_object(
        State(state): State<MockS3State>,
        Path(path): Path<String>,
    ) -> impl IntoResponse {
        let (bucket, key) = split_bucket_and_key(&path);
        let objects = state.objects.lock().await;
        if let Some(bytes) = objects.get(&(bucket.to_string(), key.to_string())) {
            (StatusCode::OK, bytes.clone()).into_response()
        } else {
            StatusCode::NOT_FOUND.into_response()
        }
    }

    fn split_bucket_and_key(path: &str) -> (&str, &str) {
        let trimmed = path.trim_start_matches('/');
        let mut segments = trimmed.splitn(2, '/');
        let bucket = segments.next().unwrap_or_default();
        let key = segments.next().unwrap_or_default();
        (bucket, key)
    }

    fn provider_bucket(provider: &str) -> &'static str {
        match provider {
            "cloudflare-r2" => "bucket-r2",
            "scaleway" => "bucket-scaleway",
            "backblaze-b2" => "bucket-b2",
            _ => "bucket-unknown",
        }
    }
}
