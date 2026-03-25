use std::env;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeMode {
    Workspace,
    Installed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimePaths {
    pub mode: RuntimeMode,
    pub runtime_base_dir: PathBuf,
    pub db_url: String,
    pub db_file_path: Option<PathBuf>,
    pub db_dir: Option<PathBuf>,
    pub cache_dir: PathBuf,
    pub spool_dir: PathBuf,
    pub download_spool_dir: PathBuf,
    pub log_dir: PathBuf,
    pub sync_root: PathBuf,
}

impl RuntimePaths {
    pub fn detect() -> Self {
        let mode = detect_runtime_mode();
        let local_app_base = local_app_omnidrive_root();
        let workspace_runtime_base = PathBuf::from(".omnidrive");
        let runtime_base_dir = env_path("OMNIDRIVE_RUNTIME_BASE_DIR").unwrap_or_else(|| match mode {
            RuntimeMode::Installed => local_app_base.clone(),
            RuntimeMode::Workspace => workspace_runtime_base,
        });

        let db_url = env::var("OMNIDRIVE_DB_URL").unwrap_or_else(|_| {
            if let Some(path) = env_path("OMNIDRIVE_DB_PATH") {
                sqlite_url_from_path(&path)
            } else if mode == RuntimeMode::Installed {
                sqlite_url_from_path(&runtime_base_dir.join("omnidrive.db"))
            } else {
                "sqlite:omnidrive.db".to_string()
            }
        });
        let db_file_path = sqlite_db_file_path(&db_url);
        let db_dir = sqlite_db_directory(&db_url);

        let cache_dir = env_path("OMNIDRIVE_CACHE_DIR").unwrap_or_else(|| {
            if mode == RuntimeMode::Installed {
                runtime_base_dir.join("Cache")
            } else {
                PathBuf::from(".omnidrive").join("cache")
            }
        });

        let spool_dir = env_path("OMNIDRIVE_SPOOL_DIR").unwrap_or_else(|| {
            if mode == RuntimeMode::Installed {
                runtime_base_dir.join("Spool")
            } else {
                PathBuf::from(".omnidrive").join("spool")
            }
        });

        let download_spool_dir =
            env_path("OMNIDRIVE_DOWNLOAD_SPOOL_DIR").unwrap_or_else(|| {
                if mode == RuntimeMode::Installed {
                    runtime_base_dir.join("download-spool")
                } else {
                    PathBuf::from(".omnidrive").join("download-spool")
                }
            });

        let log_dir = env_path("OMNIDRIVE_LOG_DIR").unwrap_or_else(|| {
            if mode == RuntimeMode::Installed {
                runtime_base_dir.join("logs")
            } else {
                PathBuf::from(".omnidrive").join("logs")
            }
        });

        let sync_root = env_path("OMNIDRIVE_SYNC_ROOT")
            .unwrap_or_else(|| local_app_base.join("SyncRoot"));

        Self {
            mode,
            runtime_base_dir,
            db_url,
            db_file_path,
            db_dir,
            cache_dir,
            spool_dir,
            download_spool_dir,
            log_dir,
            sync_root,
        }
    }

    pub fn is_installed_mode(&self) -> bool {
        self.mode == RuntimeMode::Installed
    }

    pub fn export_env_defaults(&self) {
        set_env_default(
            "OMNIDRIVE_RUNTIME_BASE_DIR",
            self.runtime_base_dir.to_string_lossy().to_string(),
        );
        set_env_default("OMNIDRIVE_DB_URL", self.db_url.clone());
        if let Some(db_path) = &self.db_file_path {
            set_env_default("OMNIDRIVE_DB_PATH", db_path.to_string_lossy().to_string());
        }
        set_env_default(
            "OMNIDRIVE_CACHE_DIR",
            self.cache_dir.to_string_lossy().to_string(),
        );
        set_env_default(
            "OMNIDRIVE_SPOOL_DIR",
            self.spool_dir.to_string_lossy().to_string(),
        );
        set_env_default(
            "OMNIDRIVE_DOWNLOAD_SPOOL_DIR",
            self.download_spool_dir.to_string_lossy().to_string(),
        );
        set_env_default(
            "OMNIDRIVE_LOG_DIR",
            self.log_dir.to_string_lossy().to_string(),
        );
        set_env_default(
            "OMNIDRIVE_SYNC_ROOT",
            self.sync_root.to_string_lossy().to_string(),
        );
    }

    pub async fn bootstrap_directories(&self, include_sync_root: bool) -> io::Result<()> {
        tokio::fs::create_dir_all(&self.runtime_base_dir).await?;
        tokio::fs::create_dir_all(&self.cache_dir).await?;
        tokio::fs::create_dir_all(&self.spool_dir).await?;
        tokio::fs::create_dir_all(&self.download_spool_dir).await?;
        tokio::fs::create_dir_all(&self.log_dir).await?;
        if let Some(db_dir) = &self.db_dir {
            tokio::fs::create_dir_all(db_dir).await?;
        }
        if include_sync_root {
            tokio::fs::create_dir_all(&self.sync_root).await?;
        }
        Ok(())
    }

    pub fn secure_runtime_directories(&self) -> io::Result<()> {
        if !self.is_installed_mode() {
            return Ok(());
        }

        secure_runtime_directory(&self.runtime_base_dir)?;
        secure_runtime_directory(&self.cache_dir)?;
        secure_runtime_directory(&self.spool_dir)?;
        secure_runtime_directory(&self.download_spool_dir)?;
        secure_runtime_directory(&self.log_dir)?;
        if let Some(db_dir) = &self.db_dir {
            secure_runtime_directory(db_dir)?;
        }
        Ok(())
    }
}

fn secure_runtime_directory(path: &Path) -> io::Result<()> {
    crate::win_acl::secure_directory(path)
        .map_err(|err| io::Error::other(format!("failed to secure {}: {err}", path.display())))
}

pub fn sqlite_db_directory(db_url: &str) -> Option<PathBuf> {
    if db_url.contains(":memory:") {
        return None;
    }

    let raw = db_url
        .strip_prefix("sqlite://")
        .or_else(|| db_url.strip_prefix("sqlite:"))
        .unwrap_or(db_url);

    if raw.is_empty() {
        return None;
    }

    let normalized = normalize_sqlite_path(raw);
    let path = PathBuf::from(normalized);
    Some(
        path.parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".")),
    )
}

pub fn sqlite_db_file_path(db_url: &str) -> Option<PathBuf> {
    if db_url.contains(":memory:") {
        return None;
    }

    let raw = db_url
        .strip_prefix("sqlite://")
        .or_else(|| db_url.strip_prefix("sqlite:"))
        .unwrap_or(db_url);

    if raw.is_empty() {
        return None;
    }

    Some(PathBuf::from(normalize_sqlite_path(raw)))
}

fn normalize_sqlite_path(raw: &str) -> &str {
    if raw.len() >= 4 && raw.starts_with('/') && raw.as_bytes().get(2) == Some(&b':') {
        &raw[1..]
    } else {
        raw
    }
}

fn sqlite_url_from_path(path: &Path) -> String {
    format!("sqlite:///{}", path.to_string_lossy().replace('\\', "/"))
}

fn env_path(key: &str) -> Option<PathBuf> {
    env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

fn set_env_default(key: &str, value: String) {
    if env::var_os(key).is_none() {
        unsafe {
            env::set_var(key, value);
        }
    }
}

fn detect_runtime_mode() -> RuntimeMode {
    if let Ok(value) = env::var("OMNIDRIVE_RUNTIME_MODE") {
        match value.to_ascii_lowercase().as_str() {
            "installed" => return RuntimeMode::Installed,
            "workspace" | "dev" | "development" => return RuntimeMode::Workspace,
            _ => {}
        }
    }

    if let Ok(exe_path) = env::current_exe() {
        if is_under_program_files(&exe_path) {
            return RuntimeMode::Installed;
        }
    }

    RuntimeMode::Workspace
}

fn is_under_program_files(path: &Path) -> bool {
    let candidates = [
        env_path("ProgramFiles"),
        env_path("ProgramFiles(x86)"),
    ];

    for candidate in candidates.into_iter().flatten() {
        if path.starts_with(&candidate) {
            return true;
        }
    }

    false
}

fn local_app_omnidrive_root() -> PathBuf {
    env_path("LOCALAPPDATA")
        .or_else(|| env_path("USERPROFILE").map(|path| path.join("AppData").join("Local")))
        .unwrap_or_else(|| PathBuf::from(".omnidrive"))
        .join("OmniDrive")
}
