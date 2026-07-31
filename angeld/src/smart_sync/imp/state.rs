use super::super::SmartSyncError;
use crate::downloader::Downloader;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use tokio::runtime::Handle;
use tracing::trace;
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::Foundation::S_FALSE;
use windows::Win32::Foundation::S_OK;
use windows::Win32::Storage::CloudFilters::CF_CONNECTION_KEY;
use windows::Win32::System::Com::COINIT_MULTITHREADED;
use windows::Win32::System::Com::CoInitializeEx;
use windows::Win32::System::Com::CoUninitialize;
use windows::core::GUID;

pub(super) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(super) const PROVIDER_NAME: &str = "OmniDrive";

pub(super) const PROVIDER_VERSION: &str = "1.0";

pub(super) const ACCOUNT_NAME: &str = "UserVault";

pub(super) const PROVIDER_ID: GUID = GUID::from_u128(0xb7a42c2a_4af1_4f4a_a650_0b1308b8f019);

pub(super) const STATUS_UNSUCCESSFUL: i32 = 0xC0000001u32 as i32;

pub(super) const STATUS_SUCCESS: i32 = 0;

// OnceLock replaced by Mutex so the connection can be cleared on lock and
// re-established on unlock (lock ↔ unlock cycle support).
pub(super) static CONNECTION_KEY: Mutex<Option<CF_CONNECTION_KEY>> = Mutex::new(None);

pub(super) static HYDRATION_CONTEXT: OnceLock<HydrationContext> = OnceLock::new();

#[derive(Clone)]
pub(super) struct HydrationContext {
    pub(super) pool: SqlitePool,
    pub(super) runtime: Handle,
    pub(super) downloader: Arc<Downloader>,
}

#[derive(Clone, Copy)]
pub(super) struct HydrationRequest {
    pub(super) connection_key: CF_CONNECTION_KEY,
    pub(super) transfer_key: i64,
    pub(super) request_key: i64,
    pub(super) inode_id: i64,
    pub(super) revision_id: i64,
    pub(super) offset: i64,
    pub(super) length: i64,
}

pub(super) struct ComApartmentGuard {
    pub(super) should_uninitialize: bool,
}

pub(super) fn initialize_com_apartment() -> Result<ComApartmentGuard, SmartSyncError> {
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr == S_OK || hr == S_FALSE {
        Ok(ComApartmentGuard {
            should_uninitialize: true,
        })
    } else if hr == RPC_E_CHANGED_MODE {
        trace!("smart-sync: COM apartment already initialized in a different mode");
        Ok(ComApartmentGuard {
            should_uninitialize: false,
        })
    } else {
        Err(SmartSyncError::Windows(hr.into()))
    }
}

impl Drop for ComApartmentGuard {
    fn drop(&mut self) {
        if self.should_uninitialize {
            unsafe { CoUninitialize() };
        }
    }
}

pub(super) fn flush_smart_sync_logs() {
    crate::logging::flush_logs_best_effort();
}
