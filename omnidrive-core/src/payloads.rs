use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub type Sha256 = [u8; 32];
pub type ObjectId128 = [u8; 16];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExtensionValue {
    UInt(u64),
    Bool(bool),
    String(String),
    Binary(Vec<u8>),
    Array(Vec<ExtensionValue>),
    Map(BTreeMap<String, ExtensionValue>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuperblockPayload {
    pub commit_seq: u64,
    pub fence_token: u64,
    pub format_epoch: u64,
    pub root_manifest_id: Sha256,
    pub pack_catalog_manifest_id: Sha256,
    pub gc_safe_before_commit_seq: u64,
    pub provider_quorum: Vec<u64>,
    pub lease_epoch_ms: u64,
    pub lease_ttl_ms: u64,
    pub repo_flags: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<BTreeMap<String, ExtensionValue>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootManifestPayload {
    pub repo_id: Sha256,
    pub commit_seq: u64,
    pub root_dir_inode_id: ObjectId128,
    pub dir_manifest_id: Sha256,
    pub changed_inode_ids: Vec<ObjectId128>,
    pub deleted_inode_ids: Vec<ObjectId128>,
    pub stats: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_root_manifest_id: Option<Sha256>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryEntryPayload {
    pub name_ciphertext: Vec<u8>,
    pub name_hash: Sha256,
    pub inode_id: ObjectId128,
    pub inode_kind: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirManifestPayload {
    pub inode_id: ObjectId128,
    pub parent_inode_id: ObjectId128,
    pub entries: Vec<DirectoryEntryPayload>,
    pub mtime_ms: u64,
    pub ctime_ms: u64,
    pub mode: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gid: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkEntryPayload {
    pub ordinal: u64,
    pub chunk_id: Sha256,
    pub pack_id: Sha256,
    pub offset_in_pack: u64,
    pub cipher_len: u64,
    pub plain_len: u64,
    pub pack_record_digest: Sha256,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileManifestPayload {
    pub inode_id: ObjectId128,
    pub file_version_id: ObjectId128,
    pub logical_size: u64,
    pub chunking_algo: u64,
    pub chunk_entries: Vec<ChunkEntryPayload>,
    pub mtime_ms: u64,
    pub ctime_ms: u64,
    pub mode: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_file_manifest_id: Option<Sha256>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderLocationPayload {
    pub provider_id: u64,
    pub object_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    pub state: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackEntryPayload {
    pub pack_id: Sha256,
    pub provider_locations: Vec<ProviderLocationPayload>,
    pub pack_size: u64,
    pub chunk_count: u64,
    pub tail_index_offset: u64,
    pub tail_index_len: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackCatalogManifestPayload {
    pub commit_seq: u64,
    pub pack_entries: Vec<PackEntryPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_pack_catalog_manifest_id: Option<Sha256>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcTombstoneManifestPayload {
    pub commit_seq: u64,
    pub candidate_pack_ids: Vec<Sha256>,
    pub candidate_manifest_ids: Vec<Sha256>,
    pub retention_floor_commit_seq: u64,
    pub created_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TailIndexRecordPayload {
    pub chunk_id: Sha256,
    pub offset: u64,
    pub record_len: u64,
    pub plain_len: u64,
    pub cipher_len: u64,
    pub compression_algo: u64,
    pub record_digest: Sha256,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TailIndexPayload {
    pub pack_id: Sha256,
    pub record_count: u64,
    pub records: Vec<TailIndexRecordPayload>,
    pub pack_payload_hash: Sha256,
    pub stats: BTreeMap<String, u64>,
}
