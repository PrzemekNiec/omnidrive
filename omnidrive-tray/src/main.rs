//! OmniDrive System Tray Companion (Epic 35.3)
//!
//! Thin client that lives in the Windows system tray, polls the angeld daemon
//! via HTTP, and swaps its icon to reflect the current vault/ingest state.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use serde::Deserialize;
use tokio::time::{self, Duration};
use tracing::{error, info, warn};

// ── Daemon API base ────────────────────────────────────────────────────────

const DAEMON_BASE: &str = "http://127.0.0.1:8787";
const POLL_INTERVAL: Duration = Duration::from_secs(3);

// ── Tray state ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayState {
    /// Default/neutral — daemon not yet polled
    Idle,
    /// Vault is locked, waiting for passphrase
    Locked,
    /// Active ingest jobs (PENDING / CHUNKING / UPLOADING)
    Syncing,
    /// Everything healthy, queue empty
    Synced,
    /// Daemon unreachable, provider error, or FAILED ingest jobs
    Error,
}

// ── Icon loader ────────────────────────────────────────────────────────────

struct IconSet {
    base_cloud: Icon,
    locked: Icon,
    syncing: Icon,
    synced: Icon,
    error: Icon,
}

fn load_icon(path: &PathBuf) -> Icon {
    let img = image::open(path)
        .unwrap_or_else(|e| panic!("cannot load icon {}: {e}", path.display()))
        .into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h)
        .unwrap_or_else(|e| panic!("invalid icon data {}: {e}", path.display()))
}

fn load_icons(base: &PathBuf) -> IconSet {
    IconSet {
        base_cloud: load_icon(&base.join("BASE_CLOUD.png")),
        locked: load_icon(&base.join("STATE_LOCKED.png")),
        syncing: load_icon(&base.join("STATE_SYNCING.png")),
        synced: load_icon(&base.join("STATE_SYNCED.png")),
        error: load_icon(&base.join("STATE_ERROR.png")),
    }
}

fn icon_for_state(icons: &IconSet, state: TrayState) -> Icon {
    match state {
        TrayState::Idle => icons.base_cloud.clone(),
        TrayState::Locked => icons.locked.clone(),
        TrayState::Syncing => icons.syncing.clone(),
        TrayState::Synced => icons.synced.clone(),
        TrayState::Error => icons.error.clone(),
    }
}

fn tooltip_for_state(state: TrayState) -> &'static str {
    match state {
        TrayState::Idle => "OmniDrive — uruchamianie...",
        TrayState::Locked => "OmniDrive — Skarbiec zablokowany",
        TrayState::Syncing => "OmniDrive — synchronizacja...",
        TrayState::Synced => "OmniDrive — zsynchronizowany",
        TrayState::Error => "OmniDrive — błąd",
    }
}

// ── API response types ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct VaultStatus {
    unlocked: bool,
}

#[derive(Deserialize)]
struct ProviderHealth {
    connection_status: String,
}

#[derive(Deserialize)]
struct IngestJob {
    state: String,
}

#[derive(Deserialize)]
struct IngestResponse {
    jobs: Vec<IngestJob>,
}

// ── Daemon poller ──────────────────────────────────────────────────────────

async fn poll_daemon_state(client: &reqwest::Client) -> TrayState {
    // 1. Check if daemon is reachable + vault lock status
    let vault_status = match client
        .get(format!("{DAEMON_BASE}/api/vault/status"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
    {
        Ok(resp) => match resp.json::<VaultStatus>().await {
            Ok(s) => s,
            Err(e) => {
                warn!("vault status parse error: {e}");
                return TrayState::Error;
            }
        },
        Err(_) => return TrayState::Error, // Daemon unreachable
    };

    if !vault_status.unlocked {
        return TrayState::Locked;
    }

    // 2. Check provider health
    if let Ok(resp) = client
        .get(format!("{DAEMON_BASE}/api/health"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
    {
        if let Ok(providers) = resp.json::<Vec<ProviderHealth>>().await {
            let any_failed = providers
                .iter()
                .any(|p| p.connection_status == "FAILED");
            if any_failed {
                return TrayState::Error;
            }
        }
    }

    // 3. Check ingest queue
    if let Ok(resp) = client
        .get(format!("{DAEMON_BASE}/api/ingest"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
    {
        if let Ok(ingest) = resp.json::<IngestResponse>().await {
            let has_failed = ingest.jobs.iter().any(|j| j.state == "FAILED");
            if has_failed {
                return TrayState::Error;
            }

            let has_active = ingest.jobs.iter().any(|j| {
                matches!(j.state.as_str(), "PENDING" | "CHUNKING" | "UPLOADING")
            });
            if has_active {
                return TrayState::Syncing;
            }
        }
    }

    TrayState::Synced
}

// ── Shell actions ──────────────────────────────────────────────────────────

fn open_vault_drive() {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer.exe")
            .arg("O:\\")
            .spawn();
    }
}

fn open_dashboard() {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "http://localhost:8787"])
            .spawn();
    }
}

// ── Custom user event ──────────────────────────────────────────────────────

#[derive(Debug)]
enum UserEvent {
    StateChanged(TrayState),
}

// ── Entry point ────────────────────────────────────────────────────────────

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("OmniDrive Tray Companion starting");

    // Resolve icon directory — relative to executable or workspace root
    let icons_dir = resolve_icons_dir();
    info!("loading icons from {}", icons_dir.display());
    let icons = load_icons(&icons_dir);

    // Build tao event loop with custom user events
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event()
        .build();

    // Context menu
    let menu = Menu::new();
    let item_open_vault = MenuItem::new("Otwórz Skarbiec (O:\\)", true, None);
    let item_dashboard = MenuItem::new("Panel sterowania", true, None);
    let item_exit = MenuItem::new("Wyjdź", true, None);
    menu.append(&item_open_vault).unwrap();
    menu.append(&item_dashboard).unwrap();
    menu.append(&item_exit).unwrap();

    let open_vault_id = item_open_vault.id().clone();
    let dashboard_id = item_dashboard.id().clone();
    let exit_id = item_exit.id().clone();

    // Build tray icon
    let tray = TrayIconBuilder::new()
        .with_icon(icon_for_state(&icons, TrayState::Idle))
        .with_tooltip(tooltip_for_state(TrayState::Idle))
        .with_menu(Box::new(menu))
        .build()
        .expect("failed to create tray icon");

    // Menu event receiver
    let menu_rx = MenuEvent::receiver();

    // Spawn polling thread
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");

        rt.block_on(async {
            let client = reqwest::Client::new();
            let mut last_state = TrayState::Idle;

            loop {
                let state = poll_daemon_state(&client).await;
                if state != last_state {
                    info!("tray state: {:?} -> {:?}", last_state, state);
                    let _ = proxy.send_event(UserEvent::StateChanged(state));
                    last_state = state;
                }
                time::sleep(POLL_INTERVAL).await;
            }
        });
    });

    // Main event loop
    let mut current_state = TrayState::Idle;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(
            std::time::Instant::now() + Duration::from_millis(100),
        );

        // Handle menu events
        if let Ok(event) = menu_rx.try_recv() {
            if event.id == open_vault_id {
                open_vault_drive();
            } else if event.id == dashboard_id {
                open_dashboard();
            } else if event.id == exit_id {
                info!("user requested exit");
                *control_flow = ControlFlow::Exit;
                return;
            }
        }

        // Handle state changes from poller
        if let Event::UserEvent(UserEvent::StateChanged(new_state)) = event {
            if new_state != current_state {
                current_state = new_state;
                tray.set_icon(Some(icon_for_state(&icons, current_state)))
                    .unwrap_or_else(|e| error!("set_icon failed: {e}"));
                tray.set_tooltip(Some(tooltip_for_state(current_state)))
                    .unwrap_or_else(|e| error!("set_tooltip failed: {e}"));
            }
        }
    });
}

fn resolve_icons_dir() -> PathBuf {
    // Try 1: next to executable — icons/tray_icons/
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    if let Some(ref dir) = exe_dir {
        let candidate = dir.join("icons").join("tray_icons");
        if candidate.exists() {
            return candidate;
        }
        // Also check parent (for target/release layout)
        if let Some(parent) = dir.parent() {
            let candidate = parent.join("icons").join("tray_icons");
            if candidate.exists() {
                return candidate;
            }
            // Two levels up (target/release -> workspace root)
            if let Some(grandparent) = parent.parent() {
                let candidate = grandparent.join("icons").join("tray_icons");
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }

    // Try 2: OMNIDRIVE_ICONS env var
    if let Ok(dir) = std::env::var("OMNIDRIVE_ICONS") {
        let p = PathBuf::from(dir);
        if p.exists() {
            return p;
        }
    }

    // Fallback: current working directory
    let cwd = std::env::current_dir().expect("cannot get cwd");
    cwd.join("icons").join("tray_icons")
}
