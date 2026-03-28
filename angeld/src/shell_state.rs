use crate::runtime_paths::RuntimePaths;
use crate::{shell_integration, virtual_drive};
use serde::Serialize;
use std::env;
use std::fmt;
use std::path::{Path, PathBuf};

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
    if let Some(target) = &current_target {
        if normalized_path(target) != normalized_path(&expected_target)
            || !is_drive_browsable(&preferred_drive_letter)
        {
            virtual_drive::unmount_virtual_drive(&preferred_drive_letter)?;
            actions.push(format!(
                "removed stale virtual drive mapping {} -> {}",
                preferred_drive_letter,
                target.display()
            ));
        }
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

    if let Ok(current_exe) = env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            let installed_icon = exe_dir.join("icons").join("omnidrive.ico");
            if installed_icon.exists() {
                return installed_icon;
            }
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

fn read_registry_string(key: &str, value_name: Option<&str>) -> Option<String> {
    let mut command = std::process::Command::new("reg");
    command.arg("query").arg(key);
    if let Some(value_name) = value_name {
        command.arg("/v").arg(value_name);
    } else {
        command.arg("/ve");
    }

    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("HKEY_") {
            return None;
        }

        if value_name.is_none() && trimmed.starts_with("(Default)") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            return parts.last().map(|value| value.to_string());
        }

        value_name.and_then(|name| {
            if trimmed.starts_with(name) {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                parts.last().map(|value| value.to_string())
            } else {
                None
            }
        })
    })
}
