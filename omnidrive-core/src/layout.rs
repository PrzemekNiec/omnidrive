use zerocopy::byteorder::big_endian::{U16, U64};
use zerocopy::{AsBytes, FromBytes, FromZeroes, Unaligned};

pub const SUPERBLOCK_MAGIC: [u8; 4] = *b"ODSB";
pub const MANIFEST_ENVELOPE_MAGIC: [u8; 4] = *b"ODMF";
pub const PACK_HEADER_MAGIC: [u8; 4] = *b"ODPK";
pub const CHUNK_RECORD_MAGIC: [u8; 4] = *b"CHNK";
pub const PACK_FOOTER_MAGIC: [u8; 4] = *b"ODFT";

pub const FORMAT_MAJOR_V1: u16 = 1;
pub const FORMAT_MINOR_V1: u16 = 0;

pub const HASH_ALGO_SHA256: u8 = 1;
pub const SER_ALGO_CANONICAL_MSGPACK: u8 = 1;
pub const CIPHER_ALGO_AES_256_GCM: u8 = 1;

pub const MANIFEST_KIND_ROOT: u8 = 1;
pub const MANIFEST_KIND_DIR: u8 = 2;
pub const MANIFEST_KIND_FILE: u8 = 3;
pub const MANIFEST_KIND_PACK_CATALOG: u8 = 4;
pub const MANIFEST_KIND_GC_TOMBSTONE: u8 = 5;

pub const CHUNKING_ALGO_FIXED_SIZE: u64 = 1;
pub const CHUNKING_ALGO_CDC: u64 = 2;

pub const COMPRESSION_ALGO_NONE: u8 = 0;
pub const COMPRESSION_ALGO_ZSTD: u8 = 1;

pub type Sha256 = [u8; 32];
pub type WriterId = [u8; 16];
pub type Nonce = [u8; 12];

#[repr(C, packed)]
#[derive(AsBytes, FromBytes, FromZeroes, Unaligned, Clone, Copy, Debug, Eq, PartialEq)]
pub struct SuperblockFixed {
    pub magic: [u8; 4],
    pub major_version: U16,
    pub minor_version: U16,
    pub flags: u8,
    pub hash_algo: u8,
    pub ser_algo: u8,
    pub reserved_0: u8,
    pub commit_seq: U64,
    pub fence_token: U64,
    pub created_at_ms: U64,
    pub writer_id: WriterId,
    pub previous_superblock_id: Sha256,
    pub root_manifest_id: Sha256,
    pub pack_catalog_manifest_id: Sha256,
    pub payload_len: U64,
}

impl SuperblockFixed {
    pub const SIZE: usize = 156;
}

const _: [(); SuperblockFixed::SIZE] = [(); core::mem::size_of::<SuperblockFixed>()];

#[repr(C, packed)]
#[derive(AsBytes, FromBytes, FromZeroes, Unaligned, Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManifestEnvelopeFixed {
    pub magic: [u8; 4],
    pub major_version: U16,
    pub minor_version: U16,
    pub manifest_kind: u8,
    pub flags: u8,
    pub hash_algo: u8,
    pub ser_algo: u8,
    pub parent_manifest_id: Sha256,
    pub logical_root_id: Sha256,
    pub created_at_ms: U64,
    pub payload_len: U64,
}

impl ManifestEnvelopeFixed {
    pub const SIZE: usize = 92;
}

const _: [(); ManifestEnvelopeFixed::SIZE] = [(); core::mem::size_of::<ManifestEnvelopeFixed>()];

#[repr(C, packed)]
#[derive(AsBytes, FromBytes, FromZeroes, Unaligned, Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackHeader {
    pub magic: [u8; 4],
    pub major_version: U16,
    pub minor_version: U16,
    pub flags: u8,
    pub hash_algo: u8,
    pub cipher_algo: u8,
    pub reserved_0: u8,
    pub pack_id: Sha256,
    pub created_at_ms: U64,
    pub record_count_hint: U64,
    pub header_len: U64,
    pub reserved_1: U64,
    pub reserved_2: [u8; 20],
}

impl PackHeader {
    pub const SIZE: usize = 96;
}

const _: [(); PackHeader::SIZE] = [(); core::mem::size_of::<PackHeader>()];

pub const KEY_WRAPPING_ALGO_LEGACY: u8 = 0;
pub const KEY_WRAPPING_ALGO_AES_KW: u8 = 1;

#[repr(C, packed)]
#[derive(AsBytes, FromBytes, FromZeroes, Unaligned, Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChunkRecordPrefix {
    pub record_magic: [u8; 4],    // [0..4]
    pub record_version: u8,       // [4]     1=V1, 2=V2
    pub flags: u8,                // [5]
    pub compression_algo: u8,     // [6]
    pub key_wrapping_algo: u8,    // [7]     0=legacy, 1=AES-KW  (was reserved_0)
    pub chunk_id: Sha256,         // [8..40]
    pub plain_len: U64,           // [40..48]
    pub cipher_len: U64,          // [48..56]
    pub nonce: Nonce,             // [56..68]
    pub dek_id_hint: [u8; 4],    // [68..72] lower 32 bits of dek_id (was reserved_1[0..4])
    pub reserved_1: [u8; 8],     // [72..80] (was reserved_1[4..12])
}

impl ChunkRecordPrefix {
    pub const SIZE: usize = 80;
    pub const GCM_TAG_SIZE: usize = 16;
}

const _: [(); ChunkRecordPrefix::SIZE] = [(); core::mem::size_of::<ChunkRecordPrefix>()];

#[repr(C, packed)]
#[derive(AsBytes, FromBytes, FromZeroes, Unaligned, Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackFooter {
    pub footer_magic: [u8; 4],
    pub major_version: U16,
    pub minor_version: U16,
    pub tail_index_offset: U64,
    pub tail_index_len: U64,
    pub pack_payload_end: U64,
    pub pack_id: Sha256,
    pub footer_mac_or_reserved: [u8; 16],
}

impl PackFooter {
    pub const SIZE: usize = 80;
}

const _: [(); PackFooter::SIZE] = [(); core::mem::size_of::<PackFooter>()];
