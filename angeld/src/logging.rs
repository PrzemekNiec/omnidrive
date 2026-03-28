use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

static LOG_GUARDS: OnceLock<Vec<WorkerGuard>> = OnceLock::new();
const LOG_BASENAME: &str = "angeld.log";

pub fn init_logging() -> io::Result<PathBuf> {
    let log_dir = default_log_dir();
    fs::create_dir_all(&log_dir)?;
    prune_old_logs(&log_dir, Duration::from_secs(60 * 60 * 24 * 7))?;

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,hyper=warn,h2=warn,aws_config=warn"));

    let file_appender = std::panic::catch_unwind(|| open_log_file(&log_dir));
    match file_appender {
        Ok(Ok(file_appender)) => {
            let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
            let stdout_writer = std::io::stdout.with_max_level(tracing::Level::TRACE);
            let combined_writer = stdout_writer.and(file_writer);

            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(combined_writer)
                        .with_target(true)
                        .with_ansi(false),
                )
                .try_init()
                .map_err(|err| io::Error::other(format!("failed to initialize tracing subscriber: {err}")))?;

            let _ = LOG_GUARDS.set(vec![guard]);
        }
        Ok(Err(err)) => return Err(err),
        Err(_) => {
            #[cfg(debug_assertions)]
            {
                eprintln!(
                    "warning: file logger initialization failed for {}; falling back to stdout-only logging in debug",
                    log_dir.display()
                );
                tracing_subscriber::registry()
                    .with(env_filter)
                    .with(
                        tracing_subscriber::fmt::layer()
                            .with_writer(std::io::stdout)
                            .with_target(true)
                            .with_ansi(false),
                    )
                    .try_init()
                    .map_err(|err| io::Error::other(format!("failed to initialize tracing subscriber: {err}")))?;
            }

            #[cfg(not(debug_assertions))]
            {
                return Err(io::Error::other(format!(
                    "failed to initialize rotating file logger in {}",
                    log_dir.display()
                )));
            }
        }
    }
    Ok(log_dir)
}

pub fn default_log_dir() -> PathBuf {
    crate::runtime_paths::RuntimePaths::detect().log_dir
}

pub fn flush_logs_best_effort() {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::thread::sleep(Duration::from_millis(150));
}

fn open_log_file(log_dir: &Path) -> io::Result<std::fs::File> {
    let log_path = log_dir.join(LOG_BASENAME);

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::{
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };

        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
            .open(log_path)
    }

    #[cfg(not(windows))]
    {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(log_path)
    }
}

fn prune_old_logs(log_dir: &Path, max_age: Duration) -> io::Result<()> {
    let now = SystemTime::now();
    for entry in fs::read_dir(log_dir)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !file_name.starts_with(LOG_BASENAME) {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let modified = match metadata.modified() {
            Ok(modified) => modified,
            Err(_) => continue,
        };
        let age = match now.duration_since(modified) {
            Ok(age) => age,
            Err(_) => continue,
        };
        if age > max_age {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(())
}
