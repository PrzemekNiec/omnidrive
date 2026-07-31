use super::util::*;
use super::*;
use crate::db;
use crate::packer::DATA_SHARDS;
use crate::packer::PARITY_SHARDS;
use crate::packer::TOTAL_SHARDS;
use omnidrive_core::crypto::ChunkId;
use omnidrive_core::crypto::GcmTag;
use omnidrive_core::crypto::KeyBytes;
use omnidrive_core::crypto::decrypt_chunk;
use omnidrive_core::crypto::decrypt_chunk_v2_verified;
use omnidrive_core::layout::CHUNK_RECORD_MAGIC;
use omnidrive_core::layout::COMPRESSION_ALGO_NONE;
use omnidrive_core::layout::ChunkRecordPrefix;
use reed_solomon_erasure::galois_8::ReedSolomon;
use zerocopy::AsBytes;
use zerocopy::byteorder::big_endian::U64;

/// Raw encrypted chunk data for zero-knowledge sharing.
/// The browser receives nonce || ciphertext || gcm_tag and decrypts via WebCrypto.
pub struct EncryptedChunkBytes {
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
    pub gcm_tag: [u8; 16],
}

impl EncryptedChunkBytes {
    /// Serialize to wire format: nonce (12) || ciphertext (N) || tag (16).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(12 + self.ciphertext.len() + 16);
        buf.extend_from_slice(&self.nonce);
        buf.extend_from_slice(&self.ciphertext);
        buf.extend_from_slice(&self.gcm_tag);
        buf
    }
}

pub(super) fn reconstruct_ciphertext(
    pack: &db::PackRecord,
    shards: &mut [Option<Vec<u8>>],
) -> Result<Vec<u8>, DownloaderError> {
    if shards.len() != TOTAL_SHARDS {
        return Err(DownloaderError::InvalidPackRecord("shard count mismatch"));
    }

    let shard_len = to_usize(pack.shard_size, "shard size")?;
    for shard in shards.iter_mut() {
        if let Some(bytes) = shard.as_mut()
            && bytes.len() != shard_len
        {
            return Err(DownloaderError::InvalidPackRecord("shard size mismatch"));
        }
    }

    let reed_solomon = ReedSolomon::new(DATA_SHARDS, PARITY_SHARDS)?;
    reed_solomon.reconstruct(shards)?;

    let mut ciphertext = Vec::with_capacity(to_usize(pack.cipher_size, "cipher size")?);
    for shard in shards.iter().take(DATA_SHARDS) {
        let bytes = shard.as_ref().ok_or(DownloaderError::InvalidPackRecord(
            "missing data shard after reconstruct",
        ))?;
        ciphertext.extend_from_slice(bytes);
    }

    let cipher_size = to_usize(pack.cipher_size, "cipher size")?;
    if ciphertext.len() < cipher_size {
        return Err(DownloaderError::InvalidPackRecord(
            "reconstructed ciphertext shorter than expected",
        ));
    }
    ciphertext.truncate(cipher_size);
    Ok(ciphertext)
}

pub(super) fn build_manifest_bytes(
    pack: &db::PackRecord,
    ciphertext: &[u8],
) -> Result<Vec<u8>, DownloaderError> {
    let chunk_id = vec_to_chunk_id(&pack.chunk_id)?;
    let nonce = vec_to_nonce(&pack.nonce)?;
    let gcm_tag = vec_to_gcm_tag(&pack.gcm_tag)?;
    let plain_len = u64::try_from(pack.logical_size)
        .map_err(|_| DownloaderError::NumericOverflow("logical size"))?;

    let prefix = ChunkRecordPrefix {
        record_magic: CHUNK_RECORD_MAGIC,
        record_version: u8::try_from(pack.encryption_version)
            .map_err(|_| DownloaderError::NumericOverflow("encryption version"))?,
        flags: 0,
        compression_algo: COMPRESSION_ALGO_NONE,
        key_wrapping_algo: 0,
        chunk_id,
        plain_len: U64::new(plain_len),
        cipher_len: U64::new(ciphertext.len() as u64),
        nonce,
        dek_id_hint: [0u8; 4],
        reserved_1: [0u8; 8],
    };

    let mut bytes = Vec::with_capacity(ChunkRecordPrefix::SIZE + ciphertext.len() + gcm_tag.len());
    bytes.extend_from_slice(prefix.as_bytes());
    bytes.extend_from_slice(ciphertext);
    bytes.extend_from_slice(&gcm_tag);
    Ok(bytes)
}

/// Decrypt a chunk record, auto-detecting V1 vs V2 from the record_version byte.
///
/// - `vault_key`: V1 deterministic key (always available after unlock)
/// - `dek`: V2 per-file DEK (None if inode has no DEK yet → must be V1 chunk)
pub(super) fn decrypt_chunk_record(
    pack_bytes: &[u8],
    chunk: &db::FileChunkLocation,
    vault_key: &KeyBytes,
    dek: Option<&KeyBytes>,
) -> Result<Vec<u8>, DownloaderError> {
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

    let record_version = record[4];

    let expected_chunk_id = vec_to_chunk_id(&chunk.chunk_id)?;
    let actual_chunk_id = vec_to_chunk_id(&record[8..40])?;
    if actual_chunk_id != expected_chunk_id {
        return Err(DownloaderError::InvalidPackRecord("chunk_id mismatch"));
    }

    let plain_len = u64::from_be_bytes(
        record[40..48]
            .try_into()
            .map_err(|_| DownloaderError::InvalidPackRecord("plain_len"))?,
    );
    let cipher_len = u64::from_be_bytes(
        record[48..56]
            .try_into()
            .map_err(|_| DownloaderError::InvalidPackRecord("cipher_len"))?,
    );
    let cipher_len_usize = usize::try_from(cipher_len)
        .map_err(|_| DownloaderError::NumericOverflow("cipher length"))?;
    let expected_record_size = ChunkRecordPrefix::SIZE
        .checked_add(cipher_len_usize)
        .and_then(|value| value.checked_add(ChunkRecordPrefix::GCM_TAG_SIZE))
        .ok_or(DownloaderError::NumericOverflow("record size"))?;
    if expected_record_size != encrypted_size {
        return Err(DownloaderError::InvalidPackRecord(
            "encrypted size mismatch",
        ));
    }

    let nonce: [u8; 12] = record[56..68]
        .try_into()
        .map_err(|_| DownloaderError::InvalidPackRecord("nonce"))?;

    let ciphertext_start = ChunkRecordPrefix::SIZE;
    let ciphertext_end = ciphertext_start + cipher_len_usize;
    let tag_end = ciphertext_end + ChunkRecordPrefix::GCM_TAG_SIZE;
    let ciphertext = &record[ciphertext_start..ciphertext_end];
    let gcm_tag: GcmTag = record[ciphertext_end..tag_end]
        .try_into()
        .map_err(|_| DownloaderError::InvalidPackRecord("gcm tag"))?;

    let plaintext = match record_version {
        2 => {
            // V2: decrypt with per-file DEK and the nonce from the prefix
            let dek = dek.ok_or(DownloaderError::InvalidPackRecord(
                "V2 chunk but no DEK available for this inode",
            ))?;
            decrypt_chunk_v2_verified(dek, &expected_chunk_id, &nonce, &[], ciphertext, &gcm_tag)?
        }
        _ => {
            // V1 (or unknown — treat as V1 for backward compat)
            decrypt_chunk(vault_key, &expected_chunk_id, &[], ciphertext, &gcm_tag)?
        }
    };

    if plaintext.len() as i64 != chunk.size || plaintext.len() as u64 != plain_len {
        return Err(DownloaderError::InvalidPackRecord("plain size mismatch"));
    }

    Ok(plaintext)
}

fn vec_to_chunk_id(bytes: &[u8]) -> Result<ChunkId, DownloaderError> {
    bytes
        .try_into()
        .map_err(|_| DownloaderError::InvalidPackRecord("chunk id length"))
}

fn vec_to_nonce(bytes: &[u8]) -> Result<[u8; 12], DownloaderError> {
    bytes
        .try_into()
        .map_err(|_| DownloaderError::InvalidPackRecord("nonce length"))
}

fn vec_to_gcm_tag(bytes: &[u8]) -> Result<GcmTag, DownloaderError> {
    bytes
        .try_into()
        .map_err(|_| DownloaderError::InvalidPackRecord("gcm tag length"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_chunk_bytes_to_bytes_wire_format() {
        let nonce = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let ciphertext = vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE];
        let gcm_tag = [
            0xF0, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFB, 0xFC, 0xFD,
            0xFE, 0xFF,
        ];

        let ecb = EncryptedChunkBytes {
            nonce,
            ciphertext: ciphertext.clone(),
            gcm_tag,
        };

        let wire = ecb.to_bytes();

        // Total length: 12 + 5 + 16 = 33
        assert_eq!(wire.len(), 12 + ciphertext.len() + 16);

        // Verify slice boundaries match WebCrypto expectations
        assert_eq!(&wire[..12], &nonce);
        assert_eq!(&wire[12..12 + ciphertext.len()], ciphertext.as_slice());
        assert_eq!(&wire[wire.len() - 16..], &gcm_tag);

        // Simulate WebCrypto split: iv = wire[..12], data = wire[12..]
        // WebCrypto treats data as ciphertext||tag (last tagLength/8 bytes = tag)
        let browser_iv = &wire[..12];
        let browser_data = &wire[12..];
        assert_eq!(browser_iv, &nonce);
        assert_eq!(browser_data.len(), ciphertext.len() + 16);
    }

    #[test]
    fn encrypted_chunk_bytes_empty_ciphertext() {
        let ecb = EncryptedChunkBytes {
            nonce: [0u8; 12],
            ciphertext: vec![],
            gcm_tag: [0u8; 16],
        };
        let wire = ecb.to_bytes();
        // 12 + 0 + 16 = 28
        assert_eq!(wire.len(), 28);
        assert_eq!(&wire[..12], &[0u8; 12]);
        assert_eq!(&wire[12..], &[0u8; 16]);
    }
}
