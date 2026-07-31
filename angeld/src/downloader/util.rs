use super::*;
use std::env;
use std::path::PathBuf;
use std::time::Duration;

pub(super) fn env_path(key: &str, default: &str) -> PathBuf {
    env::var(key)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(default))
}

pub(super) fn duration_from_env(key: &str, default_ms: u64) -> Duration {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_millis(default_ms))
}

pub(super) fn to_usize(value: i64, context: &'static str) -> Result<usize, DownloaderError> {
    usize::try_from(value).map_err(|_| DownloaderError::NumericOverflow(context))
}

pub(super) fn to_u64(value: i64, context: &'static str) -> Result<u64, DownloaderError> {
    u64::try_from(value).map_err(|_| DownloaderError::NumericOverflow(context))
}

pub(super) fn format_error_details(err: &impl std::error::Error) -> String {
    let mut details = vec![format!("display={err}"), format!("debug={err:?}")];
    let mut current = err.source();
    let mut depth = 0usize;
    while let Some(source) = current {
        depth += 1;
        details.push(format!("source[{depth}]={source}"));
        current = source.source();
    }
    details.join(" | ")
}
