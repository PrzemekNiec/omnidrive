use crate::runtime_paths::RuntimePaths;
use crate::{shell_integration, virtual_drive};
use serde::Serialize;
use std::env;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

const SHELL_MODE_UNKNOWN: u8 = 0;
const SHELL_MODE_LOCAL_ONLY: u8 = 1;
const SHELL_MODE_CLOUD: u8 = 2;

static SHELL_MODE_HINT: AtomicU8 = AtomicU8::new(SHELL_MODE_UNKNOWN);

#[derive(Debug)]
pub enum ShellStateError {
    Io(std::io::Error),
    VirtualDrive(virtual_drive::VirtualDriveError),
    ShellIntegration(shell_integration::ShellIntegrationError),
}

impl fmt::Display for ShellStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "shell state i/o error: {err}"),
            Self::VirtualDrive(err) => write!(f, "virtual drive error: {err}"),
            Self::ShellIntegration(err) => write!(f, "shell integration error: {err}"),
        }
    }
}

impl std::error::Error for ShellStateError {}

impl From<std::io::Error> for ShellStateError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<virtual_drive::VirtualDriveError> for ShellStateError {
    fn from(value: virtual_drive::VirtualDriveError) -> Self {
        Self::VirtualDrive(value)
    }
}

impl From<shell_integration::ShellIntegrationError> for ShellStateError {
    fn from(value: shell_integration::ShellIntegrationError) -> Self {
        Self::ShellIntegration(value)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ShellDriveMapping {
    pub drive_letter: String,
    pub target: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShellStateSnapshot {
    pub mode: String,
    pub preferred_drive_letter: String,
    pub expected_target: String,
    pub current_drive_target: Option<String>,
    pub drive_present: bool,
    pub drive_browsable: bool,
    pub drive_target_matches: bool,
    pub autostart_registered: bool,
    pub drive_icon_registered: bool,
    pub drive_label_registered: bool,
    pub context_menu_registered: bool,
    pub duplicate_drive_mappings: Vec<ShellDriveMapping>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShellRepairReport {
    pub actions: Vec<String>,
    pub shell_state: ShellStateSnapshot,
}

impl ShellStateSnapshot {
    pub fn is_healthy(&self) -> bool {
        self.drive_present
            && self.drive_browsable
            && self.drive_target_matches
            && self.autostart_registered
            && self.drive_icon_registered
            && self.drive_label_registered
            && self.context_menu_registered
            && self.duplicate_drive_mappings.is_empty()
    }
}

pub fn audit_shell_state() -> ShellStateSnapshot {
    let expected_target = expected_drive_target();
    let preferred_drive_letter = preferred_drive_letter();
    let mappings = virtual_drive::list_virtual_drives().unwrap_or_default();
    let current_drive_target = mappings
        .iter()
        .find(|(drive_letter, _)| same_drive_letter(drive_letter, &preferred_drive_letter))
        .map(|(_, target)| target.to_string_lossy().to_string());
    let drive_present = current_drive_target.is_some();
    let drive_browsable = is_drive_browsable(&preferred_drive_letter);
    let drive_target_matches = current_drive_target
        .as_deref()
        .map(|target| normalized_string_path(target) == normalized_path(&expected_target))
        .unwrap_or(false);

    let duplicate_drive_mappings = mappings
        .into_iter()
        .filter(|(drive_letter, target)| {
            !same_drive_letter(drive_letter, &preferred_drive_letter)
                && normalized_path(target) == normalized_path(&expected_target)
        })
        .map(|(drive_letter, target)| ShellDriveMapping {
            drive_letter,
            target: target.to_string_lossy().to_string(),
        })
        .collect();

    let autostart_registered = read_registry_string(
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
        Some("OmniDriveAngeld"),
    )
    .is_some();
    let drive_icon_registered = read_registry_string(
        &format!(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\DriveIcons\{}\DefaultIcon",
            drive_key(&preferred_drive_letter)
        ),
        None,
    )
    .is_some();
    let drive_label_registered = read_registry_string(
        &format!(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\DriveIcons\{}\DefaultLabel",
            drive_key(&preferred_drive_letter)
        ),
        None,
    )
    .is_some();
    let context_menu_registered = read_registry_string(
        r"HKCU\Software\Classes\Directory\shell\OmniDrive",
        Some("MUIVerb"),
    )
    .is_some();

    ShellStateSnapshot {
        mode: if remote_providers_configured() {
            "cloud".to_string()
        } else {
            "local_only".to_string()
        },
        preferred_drive_letter,
        expected_target: expected_target.to_string_lossy().to_string(),
        current_drive_target,
        drive_present,
        drive_browsable,
        drive_target_matches,
        autostart_registered,
        drive_icon_registered,
        drive_label_registered,
        context_menu_registered,
        duplicate_drive_mappings,
    }
}

pub fn repair_virtual_drive() -> Result<ShellRepairReport, ShellStateError> {
    let expected_target = expected_drive_target();
    let preferred_drive_letter = preferred_drive_letter();
    let mut actions = Vec::new();

    for (drive_letter, target) in virtual_drive::list_virtual_drives()? {
        if !same_drive_letter(&drive_letter, &preferred_drive_letter)
            && normalized_path(&target) == normalized_path(&expected_target)
        {
            virtual_drive::unmount_virtual_drive(&drive_letter)?;
            actions.push(format!(
                "removed duplicate virtual drive mapping {} -> {}",
                drive_letter,
                target.display()
            ));
        }
    }

    let current_target = virtual_drive::get_virtual_drive_target(&preferred_drive_letter)?;
    if let Some(target) = &current_target
        && (normalized_path(target) != normalized_path(&expected_target)
            || !is_drive_browsable(&preferred_drive_letter))
        {
            virtual_drive::unmount_virtual_drive(&preferred_drive_letter)?;
            actions.push(format!(
                "removed stale virtual drive mapping {} -> {}",
                preferred_drive_letter,
                target.display()
            ));
        }

    let current_target = virtual_drive::get_virtual_drive_target(&preferred_drive_letter)?;
    if current_target.is_none() || !is_drive_browsable(&preferred_drive_letter) {
        let drive_letter = virtual_drive::select_mount_drive_letter(&preferred_drive_letter)
            .unwrap_or(preferred_drive_letter.clone());
        virtual_drive::mount_virtual_drive(&drive_letter, &expected_target)?;
        actions.push(format!(
            "mounted virtual drive {} -> {}",
            drive_letter,
            expected_target.display()
        ));
    }

    Ok(ShellRepairReport {
        actions,
        shell_state: audit_shell_state(),
    })
}

pub fn repair_explorer_integration() -> Result<ShellRepairReport, ShellStateError> {
    let preferred_drive_letter = preferred_drive_letter();
    let expected_target = expected_drive_target();
    let active_drive_letter = active_drive_letter_for_target(&preferred_drive_letter, &expected_target)
        .unwrap_or(preferred_drive_letter.clone());
    let icon_path = virtual_drive_icon_path();
    let api_base = shell_api_base();
    let mut actions = Vec::new();

    virtual_drive::configure_virtual_drive_appearance(&active_drive_letter, "OmniDrive", &icon_path)?;
    actions.push(format!(
        "configured virtual drive appearance for {}",
        active_drive_letter
    ));

    shell_integration::register_explorer_context_menu(
        &active_drive_letter,
        &api_base,
        &icon_path,
    )?;
    actions.push(format!(
        "registered explorer context menu for {}",
        active_drive_letter
    ));

    Ok(ShellRepairReport {
        actions,
        shell_state: audit_shell_state(),
    })
}

pub fn startup_recover_shell() -> Result<ShellRepairReport, ShellStateError> {
    let initial = audit_shell_state();
    let mut actions = Vec::new();
    let mut current = initial.clone();

    if !initial.drive_present
        || !initial.drive_browsable
        || !initial.drive_target_matches
        || !initial.duplicate_drive_mappings.is_empty()
    {
        let report = repair_virtual_drive()?;
        actions.extend(report.actions);
        current = report.shell_state;
    }

    if !current.drive_icon_registered
        || !current.drive_label_registered
        || !current.context_menu_registered
    {
        let report = repair_explorer_integration()?;
        actions.extend(report.actions);
        current = report.shell_state;
    }

    Ok(ShellRepairReport {
        actions,
        shell_state: current,
    })
}

pub fn set_cloud_mode_hint(cloud_enabled: bool) {
    SHELL_MODE_HINT.store(
        if cloud_enabled {
            SHELL_MODE_CLOUD
        } else {
            SHELL_MODE_LOCAL_ONLY
        },
        Ordering::Relaxed,
    );
}

fn expected_drive_target() -> PathBuf {
    let runtime_paths = RuntimePaths::detect();
    if remote_providers_configured() {
        runtime_paths.sync_root
    } else {
        runtime_paths
            .default_watch_dir
            .unwrap_or(runtime_paths.sync_root)
    }
}

fn preferred_drive_letter() -> String {
    env::var("OMNIDRIVE_DRIVE_LETTER").unwrap_or_else(|_| "O:".to_string())
}

fn remote_providers_configured() -> bool {
    match SHELL_MODE_HINT.load(Ordering::Relaxed) {
        SHELL_MODE_CLOUD => return true,
        SHELL_MODE_LOCAL_ONLY => return false,
        _ => {}
    }

    env::var("OMNIDRIVE_R2_BUCKET").is_ok()
        || env::var("OMNIDRIVE_SCALEWAY_BUCKET").is_ok()
        || env::var("OMNIDRIVE_B2_BUCKET").is_ok()
}

fn active_drive_letter_for_target(preferred: &str, expected_target: &Path) -> Option<String> {
    if virtual_drive::get_virtual_drive_target(preferred)
        .ok()
        .flatten()
        .map(|target| normalized_path(&target) == normalized_path(expected_target))
        .unwrap_or(false)
    {
        return Some(preferred.to_string());
    }

    virtual_drive::list_virtual_drives()
        .ok()
        .and_then(|mappings| {
            mappings.into_iter().find_map(|(drive_letter, target)| {
                (normalized_path(&target) == normalized_path(expected_target)).then_some(drive_letter)
            })
        })
}

fn virtual_drive_icon_path() -> PathBuf {
    if let Ok(path) = env::var("OMNIDRIVE_DRIVE_ICON") {
        return PathBuf::from(path);
    }

    if let Ok(current_exe) = env::current_exe()
        && let Some(exe_dir) = current_exe.parent() {
            let installed_icon = exe_dir.join("icons").join("omnidrive.ico");
            if installed_icon.exists() {
                return installed_icon;
            }
        }

    PathBuf::from("icons").join("omnidrive.ico")
}

fn shell_api_base() -> String {
    let bind = env::var("OMNIDRIVE_API_BIND").unwrap_or_else(|_| "127.0.0.1:8787".to_string());
    let host_port = bind
        .strip_prefix("0.0.0.0:")
        .map(|port| format!("127.0.0.1:{port}"))
        .unwrap_or(bind);
    format!("http://{host_port}")
}

fn drive_key(drive_letter: &str) -> char {
    drive_letter
        .trim()
        .trim_end_matches('\\')
        .trim_end_matches('/')
        .trim_end_matches(':')
        .chars()
        .next()
        .unwrap_or('O')
        .to_ascii_uppercase()
}

fn same_drive_letter(left: &str, right: &str) -> bool {
    drive_key(left) == drive_key(right)
}

fn is_drive_browsable(drive_letter: &str) -> bool {
    let root = format!(r"{}\", drive_letter.trim().trim_end_matches('\\').trim_end_matches('/'));
    std::fs::read_dir(root).is_ok()
}

fn normalized_path(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn normalized_string_path(path: &str) -> String {
    normalized_path(Path::new(path))
}

/// Read a REG_SZ value from HKCU using the Windows Registry API directly.
/// `key` is the subkey path under HKCU (e.g. r"Software\...\Run").
/// `value_name` is the value name, or None for the default value.
/// Never spawns a child process — safe to call in a polling loop.
#[cfg(windows)]
fn read_registry_string(key: &str, value_name: Option<&str>) -> Option<String> {
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WOW64_64KEY, REG_SZ, RegCloseKey,
        RegOpenKeyExW, RegQueryValueExW,
    };
    use windows::core::PCWSTR;

    fn wide_null(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    let subkey_w = wide_null(key);
    let mut hkey = HKEY::default();
    let res = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey_w.as_ptr()),
            Some(0),
            KEY_READ | KEY_WOW64_64KEY,
            &mut hkey,
        )
    };
    if res == ERROR_FILE_NOT_FOUND || res.0 != 0 {
        return None;
    }

    let value_name_w: Vec<u16> = match value_name {
        Some(name) => wide_null(name),
        None => vec![0u16], // empty string = default value
    };

    let mut data_type = REG_SZ;
    let mut size: u32 = 0;
    // First call: get required buffer size
    let res = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(value_name_w.as_ptr()),
            None,
            Some(&mut data_type),
            None,
            Some(&mut size),
        )
    };
    if res.0 != 0 || size == 0 {
        unsafe { let _ = RegCloseKey(hkey); }
        return None;
    }

    let mut buf: Vec<u8> = vec![0u8; size as usize];
    let res = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(value_name_w.as_ptr()),
            None,
            Some(&mut data_type),
            Some(buf.as_mut_ptr()),
            Some(&mut size),
        )
    };
    unsafe { let _ = RegCloseKey(hkey); }
    if res.0 != 0 {
        return None;
    }

    // REG_SZ is UTF-16LE; reinterpret buf as &[u16] and strip null terminator
    let words: Vec<u16> = buf[..size as usize]
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let end = words.iter().position(|&c| c == 0).unwrap_or(words.len());
    Some(String::from_utf16_lossy(&words[..end]))
}

#[cfg(not(windows))]
fn read_registry_string(_key: &str, _value_name: Option<&str>) -> Option<String> {
    None
}
