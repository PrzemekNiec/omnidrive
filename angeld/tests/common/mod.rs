use angeld::db;
use angeld::onboarding::seal_provider_secrets;
use angeld::vault::bootstrap_local_vault;
use sqlx::SqlitePool;
use std::net::SocketAddr;
use std::path::Path;

/// Seeds mock S3 provider configs into the test DB so the daemon starts in full
/// cloud mode (with repair/scrubber/gc workers). Must be called after `db::init_db`
/// and before spawning the daemon.
///
/// Also bootstraps vault params and sets the sync policy for `watch_root` to
/// PARANOIA (EC_2_1) so ingested files get uploaded with erasure coding.
#[allow(dead_code)]
pub async fn seed_mock_providers(
    pool: &SqlitePool,
    mock_addr: SocketAddr,
    watch_root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // Bootstrap vault config + params so the daemon sees a consistent vault_id
    // for the multi-user migration (avoids vault_id mismatch between
    // vault_members and vault_state).
    bootstrap_local_vault(pool).await?;

    // Set sync policy to PARANOIA (EC_2_1) for the watch root so ingested files
    // are uploaded with erasure coding instead of defaulting to LOCAL_ONLY.
    let policy_path = watch_root
        .to_string_lossy()
        .replace('\\', "/");
    db::set_sync_policy_type_for_path(pool, &policy_path, "PARANOIA").await?;

    let endpoint = format!("http://{mock_addr}");

    let providers = [
        ("cloudflare-r2", "auto", "bucket-r2"),
        ("scaleway", "pl-waw", "bucket-scaleway"),
        ("backblaze-b2", "eu-central-003", "bucket-b2"),
    ];

    for (name, region, bucket) in &providers {
        db::upsert_provider_config(
            pool,
            name,
            &endpoint,
            region,
            bucket,
            true,  // force_path_style
            true,  // enabled
            None,
            Some("VALID"),
            None,
            None,
        )
        .await?;

        let (sealed_key_id, sealed_secret) = seal_provider_secrets("test", "test")?;
        db::upsert_provider_secret(pool, name, &sealed_key_id, &sealed_secret).await?;
    }

    // Mark onboarding as completed with cloud enabled
    db::set_system_config_value(pool, "onboarding_state", "COMPLETED").await?;
    db::set_system_config_value(pool, "onboarding_mode", "CLOUD_ENABLED").await?;
    db::set_system_config_value(pool, "cloud_enabled", "1").await?;
    db::set_system_config_value(pool, "last_onboarding_step", "completed").await?;

    Ok(())
}
