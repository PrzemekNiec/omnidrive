use super::*;
use crate::db;
use secrecy::ExposeSecret;
use std::collections::HashMap;
use std::time::Duration;

impl Downloader {
    pub(super) async fn maybe_schedule_prefetch(
        &self,
        inode_id: i64,
        revision_id: i64,
        revision_size: i64,
        inode_path: &str,
        first_chunk_index: Option<i64>,
        last_chunk_index: Option<i64>,
    ) {
        let Some(first_chunk_index) = first_chunk_index else {
            return;
        };
        let Some(last_chunk_index) = last_chunk_index else {
            return;
        };

        let previous_chunk_index = {
            let mut state = self.prefetch_state.lock().await;

            state.insert(revision_id, last_chunk_index)
        };

        let mut targets = Vec::new();
        if previous_chunk_index.is_some_and(|prev| prev + 1 == first_chunk_index) {
            targets.push(last_chunk_index + 1);
            targets.push(last_chunk_index + 2);
        }

        let small_file_threshold = 8_i64 * 1024 * 1024;
        if revision_size > 0 && revision_size <= small_file_threshold && first_chunk_index == 0 {
            let total_chunks = ((revision_size - 1) / crate::packer::DEFAULT_CHUNK_SIZE as i64) + 1;
            for chunk_index in (last_chunk_index + 1)..total_chunks {
                targets.push(chunk_index);
            }
        }

        targets.sort_unstable();
        targets.dedup();
        if targets.is_empty() {
            return;
        }

        let downloader = self.clone();
        let inode_path = inode_path.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(75)).await;
            let _ = downloader
                .prefetch_chunks(inode_id, revision_id, &inode_path, targets)
                .await;
        });
    }

    async fn prefetch_chunks(
        &self,
        inode_id: i64,
        revision_id: i64,
        inode_path: &str,
        chunk_indexes: Vec<i64>,
    ) -> Result<(), DownloaderError> {
        if chunk_indexes.is_empty() {
            return Ok(());
        }

        let revision = db::get_file_revision(&self.pool, inode_id, revision_id)
            .await?
            .ok_or(DownloaderError::NoChunksForInode(inode_id))?;
        let chunk_locations = db::get_revision_chunk_locations_in_range(
            &self.pool,
            inode_id,
            revision_id,
            0,
            revision.size,
        )
        .await?;
        if chunk_locations.is_empty() {
            return Ok(());
        }

        let vault_key = self.vault_keys.vault_key_for_v1_read(&self.pool).await?;
        let dek_option = self
            .vault_keys
            .get_or_create_dek(&self.pool, inode_id)
            .await
            .ok()
            .map(|(_, secret)| secret.expose_secret().clone());
        let mut downloaded_packs = HashMap::<String, RestoredPackSource>::new();
        for chunk in chunk_locations
            .into_iter()
            .filter(|chunk| chunk_indexes.contains(&chunk.chunk_index))
        {
            let _ = self
                .load_plaintext_chunk(
                    inode_id,
                    revision_id,
                    inode_path,
                    &vault_key,
                    dek_option.as_ref(),
                    &mut downloaded_packs,
                    &chunk,
                    true,
                )
                .await?;
        }

        Ok(())
    }
}
