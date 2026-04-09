use crate::config::AppConfig;
use crate::db;
use crate::device_identity::LocalDeviceIdentity;
use crate::diagnostics::{self, WorkerKind, WorkerStatus};
use crate::downloader::Downloader;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, UdpSocket};
use tokio::time::{MissedTickBehavior, interval};
use tracing::{debug, info, warn};

const PEER_CALLER_DEVICE_HEADER: &str = "x-omnidrive-caller-device";
const PEER_VAULT_ID_HEADER: &str = "x-omnidrive-vault-id";
#[derive(Clone)]
pub struct PeerClient {
    pool: SqlitePool,
    caller_device_id: String,
    local_vault_id: String,
    http: Client,
}

#[derive(Clone)]
pub struct PeerService {
    pool: SqlitePool,
    downloader: Arc<Downloader>,
    local_device: LocalDeviceIdentity,
    local_vault_id: String,
    peer_port: u16,
    discovery_port: u16,
    discovery_interval: Duration,
}

#[derive(Clone)]
struct PeerServiceState {
    pool: SqlitePool,
    downloader: Arc<Downloader>,
    local_device: LocalDeviceIdentity,
    local_vault_id: String,
}

#[derive(Debug)]
pub enum PeerError {
    Io(std::io::Error),
    Db(sqlx::Error),
    Http(reqwest::Error),
    Json(serde_json::Error),
}

impl fmt::Display for PeerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "peer i/o error: {err}"),
            Self::Db(err) => write!(f, "peer sqlite error: {err}"),
            Self::Http(err) => write!(f, "peer http error: {err}"),
            Self::Json(err) => write!(f, "peer json error: {err}"),
        }
    }
}

impl std::error::Error for PeerError {}

impl From<std::io::Error> for PeerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<sqlx::Error> for PeerError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value)
    }
}

impl From<reqwest::Error> for PeerError {
    fn from(value: reqwest::Error) -> Self {
        Self::Http(value)
    }
}

impl From<serde_json::Error> for PeerError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PeerAnnouncement {
    device_id: String,
    device_name: String,
    vault_id: String,
    peer_port: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PeerHelloResponse {
    device_id: String,
    device_name: String,
    vault_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PeerSummary {
    pub peer_id: String,
    pub device_name: String,
    pub vault_id: String,
    pub peer_api_base: String,
    pub trusted: bool,
    pub health_score: i64,
    pub stale: bool,
    pub last_seen_at: i64,
    pub last_handshake_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MultiDeviceSnapshot {
    pub local_device_id: String,
    pub local_device_name: String,
    pub vault_id: String,
    pub peer_port: u16,
    pub trusted_peers: Vec<PeerSummary>,
    pub recent_conflicts: Vec<ConflictSummary>,
    pub last_run: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConflictSummary {
    pub conflict_id: i64,
    pub inode_id: i64,
    pub winning_revision_id: i64,
    pub losing_revision_id: i64,
    pub reason: String,
    pub materialized_inode_id: Option<i64>,
    pub materialized_revision_id: Option<i64>,
    pub created_at: i64,
}

#[derive(Deserialize)]
struct HelloQuery {
    vault_id: String,
}

impl PeerClient {
    pub fn new(pool: SqlitePool, caller_device_id: String, local_vault_id: String) -> Self {
        Self {
            pool,
            caller_device_id,
            local_vault_id,
            http: Client::builder().timeout(Duration::from_millis(900)).build().expect("peer client"),
        }
    }

    pub async fn fetch_chunk(&self, chunk_id: &[u8]) -> Result<Option<Vec<u8>>, PeerError> {
        let mut peers = db::list_trusted_peers(&self.pool).await?;
        if peers.is_empty() {
            return Ok(None);
        }

        let config = AppConfig::from_env();
        peers.sort_by_key(|peer| {
            let policy = evaluate_peer_policy(
                peer,
                current_time_ms(),
                config.peer_stale_after_ms,
                config.peer_error_backoff_ms,
            );
            (
                if policy.eligible { 0 } else { 1 },
                -policy.health_score,
                -peer.last_seen_at,
            )
        });

        let chunk_hex = encode_hex(chunk_id);
        for peer in peers {
            let policy = evaluate_peer_policy(
                &peer,
                current_time_ms(),
                config.peer_stale_after_ms,
                config.peer_error_backoff_ms,
            );
            if !policy.eligible {
                continue;
            }

            let url = format!("{}/peer/chunks/{chunk_hex}", peer.peer_api_base.trim_end_matches('/'));
            let response = self
                .http
                .get(&url)
                .header(PEER_CALLER_DEVICE_HEADER, &self.caller_device_id)
                .header(PEER_VAULT_ID_HEADER, &self.local_vault_id)
                .send()
                .await;

            match response {
                Ok(response) if response.status().is_success() => {
                    db::update_peer_error(&self.pool, &peer.peer_id, None).await?;
                    let bytes = response.bytes().await?;
                    return Ok(Some(bytes.to_vec()));
                }
                Ok(response) if response.status() == StatusCode::NOT_FOUND => continue,
                Ok(response) => {
                    let message = format!("peer returned {}", response.status());
                    db::update_peer_error(&self.pool, &peer.peer_id, Some(&message)).await?;
                }
                Err(err) => {
                    db::update_peer_error(&self.pool, &peer.peer_id, Some(&err.to_string())).await?;
                }
            }
        }

        Ok(None)
    }
}

impl PeerService {
    pub fn new(
        pool: SqlitePool,
        downloader: Arc<Downloader>,
        local_device: LocalDeviceIdentity,
        local_vault_id: String,
        peer_port: u16,
        discovery_port: u16,
        discovery_interval: Duration,
    ) -> Self {
        Self {
            pool,
            downloader,
            local_device,
            local_vault_id,
            peer_port,
            discovery_port,
            discovery_interval,
        }
    }

    pub async fn run(self) -> Result<(), PeerError> {
        diagnostics::set_worker_status(WorkerKind::Peer, WorkerStatus::Starting);
        let peer_port = self.peer_port;
        let discovery_port = self.discovery_port;

        let server_state = PeerServiceState {
            pool: self.pool.clone(),
            downloader: self.downloader.clone(),
            local_device: self.local_device.clone(),
            local_vault_id: self.local_vault_id.clone(),
        };
        let app = Router::new()
            .route("/peer/hello", get(get_peer_hello))
            .route("/peer/chunks/{chunk_hex}", get(get_peer_chunk))
            .with_state(server_state);

        let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], self.peer_port))).await?;
        let server = axum::serve(listener, app);
        let discovery = self.run_discovery_loop();

        diagnostics::set_worker_status(WorkerKind::Peer, WorkerStatus::Idle);
        info!(
            "peer service listening on http://0.0.0.0:{} with discovery {}",
            peer_port, discovery_port
        );

        tokio::select! {
            result = server => {
                diagnostics::set_worker_status(WorkerKind::Peer, WorkerStatus::Starting);
                result.map_err(PeerError::Io)
            }
            result = discovery => {
                diagnostics::set_worker_status(WorkerKind::Peer, WorkerStatus::Starting);
                result
            }
        }
    }

    async fn run_discovery_loop(self) -> Result<(), PeerError> {
        let socket = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], self.discovery_port))).await?;
        socket.set_broadcast(true)?;

        let mut ticker = interval(self.discovery_interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut receive_buf = vec![0u8; 2048];
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    diagnostics::set_worker_status(WorkerKind::Peer, WorkerStatus::Active);
                    let announcement = PeerAnnouncement {
                        device_id: self.local_device.device_id.clone(),
                        device_name: self.local_device.device_name.clone(),
                        vault_id: self.local_vault_id.clone(),
                        peer_port: self.peer_port,
                    };
                    let payload = serde_json::to_vec(&announcement)?;
                    let target = SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), self.discovery_port);
                    let _ = socket.send_to(&payload, target).await;
                    diagnostics::set_worker_status(WorkerKind::Peer, WorkerStatus::Idle);
                }
                received = socket.recv_from(&mut receive_buf) => {
                    let (size, sender) = received?;
                    let payload = &receive_buf[..size];
                    let Ok(announcement) = serde_json::from_slice::<PeerAnnouncement>(payload) else {
                        continue;
                    };
                    if announcement.device_id == self.local_device.device_id {
                        continue;
                    }
                    if announcement.vault_id != self.local_vault_id {
                        continue;
                    }

                    let peer_api_base = format!("http://{}:{}", sender.ip(), announcement.peer_port);
                    db::note_peer_seen(
                        &self.pool,
                        &announcement.device_id,
                        &announcement.device_name,
                        &announcement.vault_id,
                        &peer_api_base,
                    ).await?;

                    if let Err(err) = handshake_peer(
                        &self.pool,
                        &self.local_vault_id,
                        &announcement.device_id,
                        &peer_api_base,
                    ).await {
                        debug!("peer handshake warning for {}: {}", peer_api_base, err);
                    }
                }
            }
        }
    }
}

pub async fn snapshot_multi_device(
    pool: &SqlitePool,
    local_device: &LocalDeviceIdentity,
    vault_id: &str,
    peer_port: u16,
) -> Result<MultiDeviceSnapshot, sqlx::Error> {
    let config = AppConfig::from_env();
    let now = current_time_ms();
    let peers = db::list_trusted_peers(pool).await?;
    let conflicts = db::list_recent_conflicts(pool, 20).await?;
    Ok(MultiDeviceSnapshot {
        local_device_id: local_device.device_id.clone(),
        local_device_name: local_device.device_name.clone(),
        vault_id: vault_id.to_string(),
        peer_port,
        trusted_peers: peers
            .into_iter()
            .map(|peer| {
                let policy = evaluate_peer_policy(
                    &peer,
                    now,
                    config.peer_stale_after_ms,
                    config.peer_error_backoff_ms,
                );
                PeerSummary {
                    peer_id: peer.peer_id,
                    device_name: peer.device_name,
                    vault_id: peer.vault_id,
                    peer_api_base: peer.peer_api_base,
                    trusted: peer.trusted != 0,
                    health_score: policy.health_score,
                    stale: policy.stale,
                    last_seen_at: peer.last_seen_at,
                    last_handshake_at: peer.last_handshake_at,
                    last_error: peer.last_error,
                }
            })
            .collect(),
        recent_conflicts: conflicts
            .into_iter()
            .map(|conflict| ConflictSummary {
                conflict_id: conflict.conflict_id,
                inode_id: conflict.inode_id,
                winning_revision_id: conflict.winning_revision_id,
                losing_revision_id: conflict.losing_revision_id,
                reason: conflict.reason,
                materialized_inode_id: conflict.materialized_inode_id,
                materialized_revision_id: conflict.materialized_revision_id,
                created_at: conflict.created_at,
            })
            .collect(),
        last_run: current_time_ms(),
    })
}

async fn handshake_peer(
    pool: &SqlitePool,
    vault_id: &str,
    expected_peer_id: &str,
    peer_api_base: &str,
) -> Result<(), PeerError> {
    let http = Client::builder().timeout(Duration::from_millis(900)).build()?;
    let url = format!(
        "{}/peer/hello?vault_id={}",
        peer_api_base.trim_end_matches('/'),
        vault_id
    );
    let response = http.get(url).send().await?;
    if !response.status().is_success() {
        db::update_peer_error(pool, expected_peer_id, Some("hello handshake rejected")).await?;
        return Ok(());
    }

    let hello = response.json::<PeerHelloResponse>().await?;
    if hello.device_id != expected_peer_id || hello.vault_id != vault_id {
        db::update_peer_error(pool, expected_peer_id, Some("hello identity mismatch")).await?;
        return Ok(());
    }

    db::upsert_trusted_peer(
        pool,
        &hello.device_id,
        &hello.device_name,
        &hello.vault_id,
        peer_api_base,
        None,
    )
    .await?;

    Ok(())
}

async fn get_peer_hello(
    State(state): State<PeerServiceState>,
    Query(query): Query<HelloQuery>,
) -> impl IntoResponse {
    if query.vault_id != state.local_vault_id {
        return StatusCode::FORBIDDEN.into_response();
    }

    (
        StatusCode::OK,
        Json(PeerHelloResponse {
            device_id: state.local_device.device_id.clone(),
            device_name: state.local_device.device_name.clone(),
            vault_id: state.local_vault_id.clone(),
        }),
    )
        .into_response()
}

async fn get_peer_chunk(
    State(state): State<PeerServiceState>,
    Path(chunk_hex): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(caller_device_id) = headers
        .get(PEER_CALLER_DEVICE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    let Some(vault_id) = headers
        .get(PEER_VAULT_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    if vault_id != state.local_vault_id {
        return StatusCode::FORBIDDEN.into_response();
    }

    match db::get_trusted_peer_by_id(&state.pool, caller_device_id).await {
        Ok(Some(peer)) if peer.trusted != 0 => {}
        Ok(_) => return StatusCode::FORBIDDEN.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }

    let Ok(chunk_id) = decode_hex(&chunk_hex) else {
        return StatusCode::BAD_REQUEST.into_response();
    };

    match state.downloader.read_plaintext_chunk_by_id(&chunk_id).await {
        Ok(Some(bytes)) => (StatusCode::OK, bytes).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            warn!("peer chunk request failed for {}: {}", chunk_hex, err);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn decode_hex(input: &str) -> Result<Vec<u8>, ()> {
    if !input.len().is_multiple_of(2) {
        return Err(());
    }

    let mut bytes = Vec::with_capacity(input.len() / 2);
    let chars: Vec<char> = input.chars().collect();
    for chunk in chars.chunks(2) {
        let high = chunk[0].to_digit(16).ok_or(())?;
        let low = chunk[1].to_digit(16).ok_or(())?;
        bytes.push(((high << 4) | low) as u8);
    }
    Ok(bytes)
}

fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

struct PeerPolicy {
    health_score: i64,
    stale: bool,
    eligible: bool,
}

fn evaluate_peer_policy(
    peer: &db::TrustedPeerRecord,
    now_ms: i64,
    stale_after_ms: u64,
    error_backoff_ms: u64,
) -> PeerPolicy {
    let stale = now_ms.saturating_sub(peer.last_seen_at) > stale_after_ms as i64;
    let in_error_backoff = peer.last_error.is_some()
        && now_ms.saturating_sub(peer.last_seen_at) < error_backoff_ms as i64;

    let mut health_score = 100i64;
    if peer.trusted == 0 {
        health_score -= 35;
    }
    if stale {
        health_score -= 40;
    }
    if peer.last_error.is_some() {
        health_score -= 25;
    }
    if peer.last_handshake_at.is_none() {
        health_score -= 10;
    }
    health_score = health_score.clamp(0, 100);

    PeerPolicy {
        health_score,
        stale,
        eligible: peer.trusted != 0 && !stale && !in_error_backoff,
    }
}
