//! Z4-01 e2e: dwa pliki o wspolnej tresci przechodza pelna droge
//! packer -> Downloader::restore_file i oba wracaja bajt w bajt.
//!
//! Testy jednostkowe packera ida skrotem (czytaja manifest ze spoola i wolaja
//! decrypt wprost). Ten test przechodzi produkcyjna sciezka odczytu, wiec zlapie
//! regresje w rozwiazywaniu klucza, ktorych tamte nie zobacza.
//!
//! Jedyny test w tym pliku — ustawia zmienne srodowiskowe procesu, wiec nie moze
//! dzielic binarki z innymi testami.

use angeld::db;
use angeld::downloader::Downloader;
use angeld::packer::{Packer, PackerConfig};
use angeld::vault::VaultKeyStore;
use std::env;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs;

#[tokio::test]
async fn shared_content_restores_for_both_files() -> Result<(), Box<dyn std::error::Error>> {
    let root = env::temp_dir().join(format!(
        "omnidrive-e2e-packkey-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    let spool_dir = root.join("spool");
    let cache_dir = root.join("cache");
    let download_spool = root.join("download-spool");
    let out_dir = root.join("out");
    for dir in [&spool_dir, &cache_dir, &download_spool, &out_dir] {
        fs::create_dir_all(dir).await?;
    }

    // SAFETY: single-threaded setup, before any worker task is spawned, and this
    // binary holds exactly one test.
    unsafe {
        env::set_var("OMNIDRIVE_SPOOL_DIR", &spool_dir);
        env::set_var("OMNIDRIVE_CACHE_DIR", &cache_dir);
    }

    let pool = db::init_db("sqlite::memory:").await?;
    let vault_keys = VaultKeyStore::new();
    vault_keys.unlock(&pool, "e2e-passphrase").await?;

    // LOCAL policy keeps packs in the spool, so the whole round-trip runs offline.
    db::set_sync_policy_type_for_path(&pool, "/", "LOCAL").await?;

    let payload = vec![0x9Eu8; 8192];
    let src_a = root.join("a.bin");
    let src_b = root.join("b.bin");
    fs::write(&src_a, &payload).await?;
    fs::write(&src_b, &payload).await?;

    let packer = Packer::new(
        pool.clone(),
        vault_keys.clone(),
        PackerConfig::new(&spool_dir),
    )?;
    let inode_a = db::create_inode(&pool, None, "a.bin", "FILE", payload.len() as i64).await?;
    let inode_b = db::create_inode(&pool, None, "b.bin", "FILE", payload.len() as i64).await?;
    let packed_a = packer.pack_file(inode_a, &src_a).await?;
    let packed_b = packer.pack_file(inode_b, &src_b).await?;
    assert_eq!(
        packed_a.pack_id, packed_b.pack_id,
        "identyczna tresc musi trafic w ten sam pack"
    );

    let downloader = Downloader::from_provider_configs(
        pool.clone(),
        vault_keys.clone(),
        &download_spool,
        Duration::from_millis(5_000),
        Vec::new(),
    )
    .await?;

    let out_a = out_dir.join("a.out");
    let out_b = out_dir.join("b.out");
    downloader.restore_file(inode_a, &out_a).await?;
    downloader
        .restore_file(inode_b, &out_b)
        .await
        .expect("drugi plik dzieli pack z pierwszym i tez musi sie odtworzyc");

    assert_eq!(fs::read(&out_a).await?, payload, "plik A odtworzony");
    assert_eq!(fs::read(&out_b).await?, payload, "plik B odtworzony");

    let _ = fs::remove_dir_all(&root).await;
    Ok(())
}
