use super::super::SmartSyncError;
use super::registration::*;
use std::ffi::OsStr;
use std::iter;
use std::os::windows::ffi::OsStrExt;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::UNIX_EPOCH;

pub(super) fn normalize_sync_root_path(path: &Path) -> Result<PathBuf, SmartSyncError> {
    prepare_sync_root_directory(path)?;
    let canonical = path.canonicalize().map_err(SmartSyncError::Io)?;
    let normalized = normalized_windows_path_string(&canonical)?;
    let normalized = PathBuf::from(normalized);
    if !normalized.is_absolute() {
        return Err(SmartSyncError::InvalidPath(
            "normalized sync root must be absolute",
        ));
    }
    ensure_path_inside_user_profile(&normalized)?;
    Ok(normalized)
}

fn ensure_path_inside_user_profile(path: &Path) -> Result<(), SmartSyncError> {
    let user_profile = std::env::var("USERPROFILE")
        .map_err(|_| SmartSyncError::InvalidPath("USERPROFILE is not set"))?;
    let user_profile = PathBuf::from(
        normalized_windows_path_string(Path::new(&user_profile))
            .map_err(|_| SmartSyncError::InvalidPath("USERPROFILE is not a valid Windows path"))?,
    );

    if !starts_with_case_insensitive(path, &user_profile) {
        return Err(SmartSyncError::InvalidPath(
            "sync root must be inside the current user profile",
        ));
    }

    Ok(())
}

pub(super) fn normalize_relative_placeholder_path(path: &str) -> Result<String, SmartSyncError> {
    let normalized = path
        .replace('\\', "/")
        .trim_start_matches('/')
        .trim()
        .to_string();

    if normalized.is_empty() {
        return Err(SmartSyncError::InvalidPath(
            "placeholder path cannot be empty",
        ));
    }

    if normalized.split('/').any(|segment| {
        segment.is_empty() || segment == "." || segment == ".." || segment.contains(':')
    }) {
        return Err(SmartSyncError::InvalidPath(
            "placeholder path contains invalid segments",
        ));
    }

    Ok(normalized.replace('/', "\\"))
}

fn normalized_windows_path_string(path: &Path) -> Result<String, SmartSyncError> {
    let raw = path.as_os_str().to_string_lossy().replace('/', "\\");
    let without_verbatim = raw.strip_prefix(r"\\?\").unwrap_or(&raw);
    let without_leading = if without_verbatim.starts_with('\\')
        && without_verbatim.len() >= 4
        && without_verbatim.as_bytes()[2] == b':'
        && without_verbatim.as_bytes()[3] == b'\\'
    {
        &without_verbatim[1..]
    } else {
        without_verbatim
    };

    if without_leading.len() < 3
        || without_leading.as_bytes()[1] != b':'
        || without_leading.as_bytes()[2] != b'\\'
    {
        return Err(SmartSyncError::InvalidPath(
            "path must resolve to a drive-qualified Windows path",
        ));
    }

    Ok(without_leading.to_string())
}

pub(super) fn file_time_from_unix_millis(unix_millis: i64) -> Result<i64, SmartSyncError> {
    if unix_millis < 0 {
        return Err(SmartSyncError::InvalidPath("negative unix timestamp"));
    }

    const WINDOWS_EPOCH_OFFSET_SECS: u64 = 11_644_473_600;
    let duration = Duration::from_millis(
        u64::try_from(unix_millis)
            .map_err(|_| SmartSyncError::InvalidPath("negative unix timestamp"))?,
    );
    let system_time = UNIX_EPOCH
        .checked_add(duration)
        .ok_or(SmartSyncError::InvalidPath("timestamp overflow"))?;
    let duration = system_time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SmartSyncError::InvalidPath("system time before unix epoch"))?;
    let ticks = (duration.as_secs() + WINDOWS_EPOCH_OFFSET_SECS)
        .saturating_mul(10_000_000)
        .saturating_add(u64::from(duration.subsec_nanos() / 100));
    i64::try_from(ticks).map_err(|_| SmartSyncError::InvalidPath("timestamp overflow"))
}

pub(super) fn wide_path(path: &Path) -> Result<Vec<u16>, SmartSyncError> {
    if !path.is_absolute() {
        return Err(SmartSyncError::InvalidPath("path must be absolute"));
    }
    Ok(wide_str(path.as_os_str()))
}

pub(super) fn wide_str(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(iter::once(0)).collect()
}

fn starts_with_case_insensitive(path: &Path, prefix: &Path) -> bool {
    let path_parts: Vec<String> = path.components().filter_map(normalized_component).collect();
    let prefix_parts: Vec<String> = prefix
        .components()
        .filter_map(normalized_component)
        .collect();

    path_parts.len() >= prefix_parts.len()
        && path_parts
            .iter()
            .zip(prefix_parts.iter())
            .all(|(left, right)| left == right)
}

fn normalized_component(component: Component<'_>) -> Option<String> {
    match component {
        Component::Prefix(prefix) => {
            Some(prefix.as_os_str().to_string_lossy().to_ascii_lowercase())
        }
        Component::RootDir => Some("\\".to_string()),
        Component::Normal(value) => Some(value.to_string_lossy().to_ascii_lowercase()),
        _ => None,
    }
}
