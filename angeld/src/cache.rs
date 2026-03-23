use crate::config::AppConfig;
use crate::db;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::env;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::fs;

#[derive(Clone)]
pub struct CacheManager {
    pool: SqlitePool,
    root_dir: PathBuf,
    max_bytes: u64,
    metrics: Arc<CacheMetrics>,
}

#[derive(Default)]
struct CacheMetrics {
    hit_count: AtomicU64,
    miss_count: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CacheRuntimeStats {
    pub hit_count: u64,
    pub miss_count: u64,
}

static GLOBAL_CACHE_METRICS: OnceLock<Arc<CacheMetrics>> = OnceLock::new();

#[derive(Debug)]
pub enum CacheError {
    Io(std::io::Error),
    Db(sqlx::Error),
    NumericOverflow(&'static str),
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "cache i/o error: {err}"),
            Self::Db(err) => write!(f, "cache sqlite error: {err}"),
            Self::NumericOverflow(ctx) => write!(f, "numeric overflow while handling {ctx}"),
        }
    }
}

impl std::error::Error for CacheError {}

impl From<std::io::Error> for CacheError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<sqlx::Error> for CacheError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value)
    }
}

impl CacheManager {
    pub async fn from_env(pool: SqlitePool) -> Result<Self, CacheError> {
        let config = AppConfig::from_env();
        let root_dir = env::var("OMNIDRIVE_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| default_cache_root());

        fs::create_dir_all(&root_dir).await?;

        Ok(Self {
            pool,
            root_dir,
            max_bytes: config.max_cache_bytes,
            metrics: GLOBAL_CACHE_METRICS
                .get_or_init(|| Arc::new(CacheMetrics::default()))
                .clone(),
        })
    }

    pub fn cache_key(revision_id: i64, chunk_index: i64) -> String {
        format!("{revision_id}:{chunk_index}")
    }

    pub async fn get_chunk(&self, cache_key: &str) -> Result<Option<Vec<u8>>, CacheError> {
        let Some(entry) = db::get_cache_entry(&self.pool, cache_key).await? else {
            self.metrics.miss_count.fetch_add(1, Ordering::Relaxed);
            return Ok(None);
        };

        match fs::read(&entry.cache_path).await {
            Ok(bytes) => {
                let byte_len = i64::try_from(bytes.len())
                    .map_err(|_| CacheError::NumericOverflow("cache read length"))?;
                if byte_len != entry.size {
                    self.delete_entry(&entry.cache_key, Path::new(&entry.cache_path))
                        .await?;
                    self.metrics.miss_count.fetch_add(1, Ordering::Relaxed);
                    return Ok(None);
                }

                db::touch_cache_entry(&self.pool, cache_key).await?;
                self.metrics.hit_count.fetch_add(1, Ordering::Relaxed);
                Ok(Some(bytes))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                db::delete_cache_entry(&self.pool, cache_key).await?;
                self.metrics.miss_count.fetch_add(1, Ordering::Relaxed);
                Ok(None)
            }
            Err(err) => Err(CacheError::Io(err)),
        }
    }

    pub async fn put_chunk(
        &self,
        inode_id: i64,
        revision_id: i64,
        chunk_index: i64,
        pack_id: &str,
        file_path: &str,
        bytes: &[u8],
        is_prefetched: bool,
    ) -> Result<(), CacheError> {
        let cache_key = Self::cache_key(revision_id, chunk_index);
        let cache_path = self.cache_path_for_key(&cache_key);
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        fs::write(&cache_path, bytes).await?;

        let size = i64::try_from(bytes.len())
            .map_err(|_| CacheError::NumericOverflow("cache write length"))?;
        db::upsert_cache_entry(
            &self.pool,
            &cache_key,
            inode_id,
            revision_id,
            chunk_index,
            pack_id,
            file_path,
            &cache_path.to_string_lossy(),
            size,
            is_prefetched,
        )
        .await?;

        self.evict_if_needed(Some(&cache_key)).await
    }

    async fn evict_if_needed(&self, protected_key: Option<&str>) -> Result<(), CacheError> {
        let max_bytes = i64::try_from(self.max_bytes)
            .map_err(|_| CacheError::NumericOverflow("cache byte budget"))?;
        let mut total_size = db::get_total_cache_size(&self.pool).await?;
        if total_size <= max_bytes {
            return Ok(());
        }

        while total_size > max_bytes {
            let candidates = db::list_cache_entries_by_lru(&self.pool, 64).await?;
            if candidates.is_empty() {
                break;
            }

            let mut removed_any = false;
            for entry in candidates {
                if protected_key.is_some_and(|key| key == entry.cache_key) {
                    continue;
                }

                self.delete_entry(&entry.cache_key, Path::new(&entry.cache_path))
                    .await?;
                total_size -= entry.size;
                removed_any = true;
                if total_size <= max_bytes {
                    break;
                }
            }

            if !removed_any {
                break;
            }
        }

        Ok(())
    }

    async fn delete_entry(&self, cache_key: &str, cache_path: &Path) -> Result<(), CacheError> {
        match fs::remove_file(cache_path).await {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(CacheError::Io(err)),
        }

        db::delete_cache_entry(&self.pool, cache_key).await?;
        Ok(())
    }

    fn cache_path_for_key(&self, cache_key: &str) -> PathBuf {
        let digest = Sha256::digest(cache_key.as_bytes());
        let digest_hex = hex_lower(&digest);
        let dir_a = &digest_hex[..2];
        let dir_b = &digest_hex[2..4];
        self.root_dir.join(dir_a).join(dir_b).join(format!("{cache_key}.bin"))
    }
}

pub fn cache_runtime_stats() -> CacheRuntimeStats {
    let Some(metrics) = GLOBAL_CACHE_METRICS.get() else {
        return CacheRuntimeStats::default();
    };

    CacheRuntimeStats {
        hit_count: metrics.hit_count.load(Ordering::Relaxed),
        miss_count: metrics.miss_count.load(Ordering::Relaxed),
    }
}

fn default_cache_root() -> PathBuf {
    env::var("LOCALAPPDATA")
        .map(|root| PathBuf::from(root).join("OmniDrive").join("Cache"))
        .unwrap_or_else(|_| PathBuf::from(".omnidrive").join("cache"))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}
