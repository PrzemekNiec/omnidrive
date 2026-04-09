use crate::config::AppConfig;
use crate::db;
use rand::RngCore;
use sqlx::SqlitePool;
use std::env;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalDeviceIdentity {
    pub device_id: String,
    pub device_name: String,
    pub peer_token: String,
    pub created_at: i64,
    pub updated_at: i64,
}

pub async fn ensure_local_device_identity(
    pool: &SqlitePool,
    config: &AppConfig,
) -> Result<LocalDeviceIdentity, sqlx::Error> {
    if let Some(existing) = db::get_local_device_identity(pool).await? {
        if let Some(device_name) = preferred_device_name(config)
            && device_name != existing.device_name {
                db::update_local_device_name(pool, &device_name).await?;
                let refreshed = db::get_local_device_identity(pool)
                    .await?
                    .unwrap_or(existing);
                return Ok(LocalDeviceIdentity {
                    device_id: refreshed.device_id,
                    device_name: refreshed.device_name,
                    peer_token: refreshed.peer_token,
                    created_at: refreshed.created_at,
                    updated_at: refreshed.updated_at,
                });
            }

        return Ok(LocalDeviceIdentity {
            device_id: existing.device_id,
            device_name: existing.device_name,
            peer_token: existing.peer_token,
            created_at: existing.created_at,
            updated_at: existing.updated_at,
        });
    }

    let device_id = format!("dev-{}", random_hex(8));
    let device_name = preferred_device_name(config).unwrap_or_else(default_device_name);
    let peer_token = random_hex(16);
    db::upsert_local_device_identity(pool, &device_id, &device_name, &peer_token).await?;
    let created = db::get_local_device_identity(pool)
        .await?
        .expect("local device identity must exist after upsert");
    Ok(LocalDeviceIdentity {
        device_id: created.device_id,
        device_name: created.device_name,
        peer_token: created.peer_token,
        created_at: created.created_at,
        updated_at: created.updated_at,
    })
}

fn preferred_device_name(config: &AppConfig) -> Option<String> {
    config
        .device_name_override
        .clone()
        .or_else(|| env::var("COMPUTERNAME").ok())
        .or_else(|| env::var("HOSTNAME").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn default_device_name() -> String {
    "OmniDrive Device".to_string()
}

fn random_hex(bytes: usize) -> String {
    let mut data = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut data);
    let mut output = String::with_capacity(bytes * 2);
    for byte in data {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}
