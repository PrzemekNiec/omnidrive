use super::super::SmartSyncError;
use super::placeholder::*;
use super::projection::*;
use super::state::*;
use crate::db;
use crate::downloader::Downloader;
use sqlx::SqlitePool;
use std::mem::size_of;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::ptr;
use std::sync::Arc;
use tokio::runtime::Handle;
use tracing::error;
use tracing::info;
use tracing::trace;
use tracing::warn;
use windows::Win32::Foundation::NTSTATUS;
use windows::Win32::Storage::CloudFilters::CF_CALLBACK_INFO;
use windows::Win32::Storage::CloudFilters::CF_CALLBACK_PARAMETERS;
use windows::Win32::Storage::CloudFilters::CF_OPERATION_INFO;
use windows::Win32::Storage::CloudFilters::CF_OPERATION_PARAMETERS;
use windows::Win32::Storage::CloudFilters::CF_OPERATION_TRANSFER_DATA_FLAGS;
use windows::Win32::Storage::CloudFilters::CF_OPERATION_TRANSFER_PLACEHOLDERS_FLAGS;
use windows::Win32::Storage::CloudFilters::CF_OPERATION_TYPE_TRANSFER_DATA;
use windows::Win32::Storage::CloudFilters::CF_OPERATION_TYPE_TRANSFER_PLACEHOLDERS;
use windows::Win32::Storage::CloudFilters::CfExecute;

pub fn install_hydration_runtime(
    pool: SqlitePool,
    downloader: Arc<Downloader>,
) -> Result<(), SmartSyncError> {
    let context = HydrationContext {
        pool,
        runtime: Handle::current(),
        downloader,
    };

    let _ = HYDRATION_CONTEXT.set(context);
    Ok(())
}

pub(super) unsafe extern "system" fn fetch_data_callback(
    callback_info: *const CF_CALLBACK_INFO,
    callback_parameters: *const CF_CALLBACK_PARAMETERS,
) {
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        fetch_data_callback_inner(callback_info, callback_parameters)
    }));
    if let Err(panic_payload) = result {
        log_callback_panic("FETCH_DATA", panic_payload);
        if !callback_info.is_null() && !callback_parameters.is_null() {
            let callback_info = unsafe { &*callback_info };
            let callback_parameters = unsafe { &*callback_parameters };
            let fetch = unsafe { callback_parameters.Anonymous.FetchData };
            let _ = complete_transfer_failure(
                callback_info,
                fetch.RequiredFileOffset,
                fetch.RequiredLength,
            );
        }
    }
}

unsafe fn fetch_data_callback_inner(
    callback_info: *const CF_CALLBACK_INFO,
    callback_parameters: *const CF_CALLBACK_PARAMETERS,
) {
    if callback_info.is_null() || callback_parameters.is_null() {
        return;
    }

    let callback_info = unsafe { &*callback_info };
    let callback_parameters = unsafe { &*callback_parameters };
    let fetch = unsafe { callback_parameters.Anonymous.FetchData };

    let Some(identity) =
        decode_file_identity(callback_info.FileIdentity, callback_info.FileIdentityLength)
    else {
        warn!(
            "smart-sync: hydration requested with invalid identity, request_key={}",
            callback_info.RequestKey
        );
        let _ = complete_transfer_failure(
            callback_info,
            fetch.RequiredFileOffset,
            fetch.RequiredLength,
        );
        return;
    };

    crate::auto_lock::touch(crate::auto_lock::TouchSource::CfApi);

    let request = HydrationRequest {
        connection_key: callback_info.ConnectionKey,
        transfer_key: callback_info.TransferKey,
        request_key: callback_info.RequestKey,
        inode_id: identity.inode_id,
        revision_id: identity.revision_id,
        offset: fetch.RequiredFileOffset,
        length: fetch.RequiredLength,
    };

    info!(
        "Hydration requested for inode: {}, revision: {}, offset: {}, length: {}",
        request.inode_id, request.revision_id, request.offset, request.length
    );

    let Some(context) = HYDRATION_CONTEXT.get().cloned() else {
        warn!(
            "smart-sync: hydration runtime missing, request_key={}",
            request.request_key
        );
        flush_smart_sync_logs();
        let _ = complete_transfer_failure_from_request(&request);
        return;
    };

    if !context.downloader.has_remote_providers() {
        warn!(
            "smart-sync: no remote providers configured for request_key={}, inode={}, revision={}; returning empty hydration result in setup/local-only mode",
            request.request_key, request.inode_id, request.revision_id
        );
        flush_smart_sync_logs();
        let _ = complete_transfer_success(&request, &[]);
        return;
    }

    context.runtime.spawn(async move {
        let offset = match u64::try_from(request.offset) {
            Ok(value) => value,
            Err(_) => {
                warn!(
                    "smart-sync: invalid negative offset for inode={}, revision={}",
                    request.inode_id, request.revision_id
                );
                flush_smart_sync_logs();
                let _ = complete_transfer_failure_from_request(&request);
                return;
            }
        };
        let length = match u64::try_from(request.length) {
            Ok(value) => value,
            Err(_) => {
                warn!(
                    "smart-sync: invalid negative length for inode={}, revision={}",
                    request.inode_id, request.revision_id
                );
                flush_smart_sync_logs();
                let _ = complete_transfer_failure_from_request(&request);
                return;
            }
        };

        // Streamed hydration: download + decrypt one chunk at a time,
        // feed each slice to Windows immediately via CfExecute, then
        // drop the chunk before loading the next.  Peak RAM ≤ 1 chunk.
        let stream_result = context
            .downloader
            .read_range_streamed(
                request.inode_id,
                request.revision_id,
                offset,
                length,
                |chunk_offset, chunk_bytes| {
                    let file_offset = i64::try_from(chunk_offset).map_err(|_| {
                        crate::downloader::DownloaderError::NumericOverflow("chunk offset")
                    })?;
                    complete_transfer_chunk(&request, file_offset, chunk_bytes).map_err(
                        |err| {
                            crate::downloader::DownloaderError::Io(std::io::Error::other(
                                format!("CfExecute transfer failed: {err}"),
                            ))
                        },
                    )
                },
            )
            .await;

        match stream_result {
            Ok(()) => {
                if let Err(err) = db::set_hydration_state(&context.pool, request.inode_id, 1).await {
                    warn!(
                        "smart-sync: failed to persist hydration state for inode={}: {}",
                        request.inode_id, err
                    );
                }
                if let Ok(path) = projection_path_for_inode(&context.pool, request.inode_id).await {
                    if let Err(err) = mark_in_sync(&path, true) {
                        warn!("smart-sync: mark_in_sync after hydration failed for inode={}: {}", request.inode_id, err);
                    }
                    notify_shell_path_changed(&path);
                }
            }
            Err(err) => {
                if !context.downloader.has_remote_providers() {
                    warn!(
                        "smart-sync: local-only setup mode could not hydrate inode={}, revision={} without configured remote providers: {}",
                        request.inode_id, request.revision_id, err
                    );
                }
                warn!(
                    "smart-sync: streamed hydration failed for inode={}, revision={}, offset={}, length={}: {}",
                    request.inode_id, request.revision_id, request.offset, request.length, err
                );
                flush_smart_sync_logs();
                let _ = complete_transfer_failure_from_request(&request);
            }
        }
    });
}

/// Callback for FETCH_PLACEHOLDERS.  We pre-create all placeholders at
/// startup, so there is nothing new to return.  However, we MUST call
/// CfExecute with CF_OPERATION_TYPE_TRANSFER_PLACEHOLDERS (zero entries)
/// to complete the request — otherwise the minifilter blocks directory
/// enumeration indefinitely, causing Explorer timeouts.
pub(super) unsafe extern "system" fn fetch_placeholders_callback(
    callback_info: *const CF_CALLBACK_INFO,
    _callback_parameters: *const CF_CALLBACK_PARAMETERS,
) {
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        fetch_placeholders_callback_inner(callback_info)
    }));
    if let Err(panic_payload) = result {
        log_callback_panic("FETCH_PLACEHOLDERS", panic_payload);
    }
}

unsafe fn fetch_placeholders_callback_inner(callback_info: *const CF_CALLBACK_INFO) {
    if callback_info.is_null() {
        return;
    }
    let info = unsafe { &*callback_info };

    crate::auto_lock::touch(crate::auto_lock::TouchSource::CfApi);

    trace!("smart-sync: FETCH_PLACEHOLDERS callback invoked, completing with zero entries");

    let operation_info = CF_OPERATION_INFO {
        StructSize: size_of::<CF_OPERATION_INFO>() as u32,
        Type: CF_OPERATION_TYPE_TRANSFER_PLACEHOLDERS,
        ConnectionKey: info.ConnectionKey,
        TransferKey: info.TransferKey,
        CorrelationVector: ptr::null(),
        SyncStatus: ptr::null(),
        RequestKey: info.RequestKey,
    };

    let mut operation_parameters = CF_OPERATION_PARAMETERS {
        ParamSize: size_of::<CF_OPERATION_PARAMETERS>() as u32,
        ..Default::default()
    };
    operation_parameters.Anonymous.TransferPlaceholders =
        windows::Win32::Storage::CloudFilters::CF_OPERATION_PARAMETERS_0_4 {
            // DISABLE_ON_DEMAND_POPULATION (= 1): tells cldflt.sys this directory is
            // fully populated — all placeholders were pre-created at startup.
            // Without this flag, CF_POPULATION_POLICY_FULL never marks the directory
            // as "populated" and blocks all file-creation operations with
            // ERROR_CANT_RESOLVE_FILENAME (0x80070781).
            // DISABLE_ON_DEMAND_POPULATION = 0x2: marks directory as fully populated.
            // STOP_ON_ERROR               = 0x1: unrelated, do not set.
            Flags: CF_OPERATION_TRANSFER_PLACEHOLDERS_FLAGS(2),
            CompletionStatus: NTSTATUS(STATUS_SUCCESS),
            PlaceholderTotalCount: 0,
            PlaceholderArray: ptr::null_mut(),
            PlaceholderCount: 0,
            EntriesProcessed: 0,
        };

    if let Err(err) = unsafe { CfExecute(&operation_info, &mut operation_parameters) } {
        warn!("smart-sync: FETCH_PLACEHOLDERS CfExecute failed: {}", err);
    }
}

pub(super) unsafe extern "system" fn cancel_fetch_data_callback(
    callback_info: *const CF_CALLBACK_INFO,
    callback_parameters: *const CF_CALLBACK_PARAMETERS,
) {
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        cancel_fetch_data_callback_inner(callback_info, callback_parameters)
    }));
    if let Err(panic_payload) = result {
        log_callback_panic("CANCEL_FETCH_DATA", panic_payload);
    }
}

unsafe fn cancel_fetch_data_callback_inner(
    callback_info: *const CF_CALLBACK_INFO,
    callback_parameters: *const CF_CALLBACK_PARAMETERS,
) {
    if callback_info.is_null() || callback_parameters.is_null() {
        return;
    }

    let callback_info = unsafe { &*callback_info };
    let callback_parameters = unsafe { &*callback_parameters };
    let cancel = unsafe { callback_parameters.Anonymous.Cancel };
    let fetch = unsafe { cancel.Anonymous.FetchData };

    let identity =
        decode_file_identity(callback_info.FileIdentity, callback_info.FileIdentityLength);

    match identity {
        Some(identity) => {
            warn!(
                "smart-sync: hydration canceled for inode={}, revision={}, offset={}, length={}",
                identity.inode_id, identity.revision_id, fetch.FileOffset, fetch.Length
            );
        }
        None => {
            warn!(
                "smart-sync: hydration canceled for unknown identity, offset={}, length={}",
                fetch.FileOffset, fetch.Length
            );
        }
    }
}

fn log_callback_panic(callback_name: &str, panic_payload: Box<dyn std::any::Any + Send>) {
    let message = if let Some(text) = panic_payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = panic_payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "non-string panic payload".to_string()
    };

    error!(
        "smart-sync: {} callback panicked: {}",
        callback_name, message
    );
    eprintln!(
        "smart-sync: {} callback panicked: {}",
        callback_name, message
    );
    crate::logging::flush_logs_best_effort();
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct PlaceholderIdentity {
    pub(super) inode_id: i64,
    pub(super) revision_id: i64,
}

fn decode_file_identity(
    identity_ptr: *const core::ffi::c_void,
    identity_len: u32,
) -> Option<PlaceholderIdentity> {
    if identity_ptr.is_null() || identity_len as usize != size_of::<PlaceholderIdentity>() {
        return None;
    }

    let identity = unsafe { ptr::read_unaligned(identity_ptr.cast::<PlaceholderIdentity>()) };
    Some(identity)
}

fn complete_transfer_success(
    request: &HydrationRequest,
    bytes: &[u8],
) -> Result<(), SmartSyncError> {
    let operation_info = CF_OPERATION_INFO {
        StructSize: size_of::<CF_OPERATION_INFO>() as u32,
        Type: CF_OPERATION_TYPE_TRANSFER_DATA,
        ConnectionKey: request.connection_key,
        TransferKey: request.transfer_key,
        CorrelationVector: ptr::null(),
        SyncStatus: ptr::null(),
        RequestKey: request.request_key,
    };

    let mut operation_parameters = CF_OPERATION_PARAMETERS {
        ParamSize: size_of::<CF_OPERATION_PARAMETERS>() as u32,
        ..Default::default()
    };
    operation_parameters.Anonymous.TransferData =
        windows::Win32::Storage::CloudFilters::CF_OPERATION_PARAMETERS_0_0 {
            Flags: CF_OPERATION_TRANSFER_DATA_FLAGS(0),
            CompletionStatus: NTSTATUS(STATUS_SUCCESS),
            Buffer: bytes.as_ptr().cast(),
            Offset: request.offset,
            Length: i64::try_from(bytes.len())
                .map_err(|_| SmartSyncError::InvalidPath("range length overflow"))?,
        };

    unsafe {
        CfExecute(&operation_info, &mut operation_parameters)?;
    }

    Ok(())
}

/// Transfer a single chunk slice to Windows at an explicit file offset.
/// Called once per chunk during streamed hydration — peak RAM ≤ 1 chunk.
fn complete_transfer_chunk(
    request: &HydrationRequest,
    file_offset: i64,
    bytes: &[u8],
) -> Result<(), SmartSyncError> {
    let operation_info = CF_OPERATION_INFO {
        StructSize: size_of::<CF_OPERATION_INFO>() as u32,
        Type: CF_OPERATION_TYPE_TRANSFER_DATA,
        ConnectionKey: request.connection_key,
        TransferKey: request.transfer_key,
        CorrelationVector: ptr::null(),
        SyncStatus: ptr::null(),
        RequestKey: request.request_key,
    };

    let mut operation_parameters = CF_OPERATION_PARAMETERS {
        ParamSize: size_of::<CF_OPERATION_PARAMETERS>() as u32,
        ..Default::default()
    };
    operation_parameters.Anonymous.TransferData =
        windows::Win32::Storage::CloudFilters::CF_OPERATION_PARAMETERS_0_0 {
            Flags: CF_OPERATION_TRANSFER_DATA_FLAGS(0),
            CompletionStatus: NTSTATUS(STATUS_SUCCESS),
            Buffer: bytes.as_ptr().cast(),
            Offset: file_offset,
            Length: i64::try_from(bytes.len())
                .map_err(|_| SmartSyncError::InvalidPath("chunk length overflow"))?,
        };

    unsafe {
        CfExecute(&operation_info, &mut operation_parameters)?;
    }

    Ok(())
}

fn complete_transfer_failure(
    callback_info: &CF_CALLBACK_INFO,
    offset: i64,
    length: i64,
) -> Result<(), SmartSyncError> {
    let request = HydrationRequest {
        connection_key: callback_info.ConnectionKey,
        transfer_key: callback_info.TransferKey,
        request_key: callback_info.RequestKey,
        inode_id: 0,
        revision_id: 0,
        offset,
        length,
    };
    complete_transfer_failure_from_request(&request)
}

fn complete_transfer_failure_from_request(
    request: &HydrationRequest,
) -> Result<(), SmartSyncError> {
    let operation_info = CF_OPERATION_INFO {
        StructSize: size_of::<CF_OPERATION_INFO>() as u32,
        Type: CF_OPERATION_TYPE_TRANSFER_DATA,
        ConnectionKey: request.connection_key,
        TransferKey: request.transfer_key,
        CorrelationVector: ptr::null(),
        SyncStatus: ptr::null(),
        RequestKey: request.request_key,
    };

    let mut operation_parameters = CF_OPERATION_PARAMETERS {
        ParamSize: size_of::<CF_OPERATION_PARAMETERS>() as u32,
        ..Default::default()
    };
    operation_parameters.Anonymous.TransferData =
        windows::Win32::Storage::CloudFilters::CF_OPERATION_PARAMETERS_0_0 {
            Flags: CF_OPERATION_TRANSFER_DATA_FLAGS(0),
            CompletionStatus: NTSTATUS(STATUS_UNSUCCESSFUL),
            Buffer: ptr::null(),
            Offset: request.offset,
            Length: request.length.max(0),
        };

    unsafe {
        CfExecute(&operation_info, &mut operation_parameters)?;
    }

    Ok(())
}
