use std::fmt;
use std::path::Path;

#[derive(Debug)]
pub enum VirtualDriveError {
    Io(std::io::Error),
    InvalidDriveLetter,
    InvalidPath,
    CommandFailed(String),
    #[cfg_attr(windows, allow(dead_code))]
    UnsupportedPlatform,
    #[cfg(windows)]
    Windows(windows::core::Error),
}

impl fmt::Display for VirtualDriveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "i/o error: {err}"),
            Self::InvalidDriveLetter => write!(f, "invalid drive letter"),
            Self::InvalidPath => write!(f, "invalid target path"),
            Self::CommandFailed(message) => write!(f, "command failed: {message}"),
            Self::UnsupportedPlatform => {
                write!(f, "virtual drive mapping is only supported on Windows")
            }
            #[cfg(windows)]
            Self::Windows(err) => write!(f, "windows error: {err}"),
        }
    }
}

impl std::error::Error for VirtualDriveError {}

impl From<std::io::Error> for VirtualDriveError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[cfg(windows)]
impl From<windows::core::Error> for VirtualDriveError {
    fn from(value: windows::core::Error) -> Self {
        Self::Windows(value)
    }
}

pub fn mount_virtual_drive(drive_letter: &str, target_path: &Path) -> Result<(), VirtualDriveError> {
    #[cfg(windows)]
    {
        imp::mount_virtual_drive(drive_letter, target_path)
    }

    #[cfg(not(windows))]
    {
        let _ = drive_letter;
        let _ = target_path;
        Err(VirtualDriveError::UnsupportedPlatform)
    }
}

pub fn select_mount_drive_letter(preferred_drive_letter: &str) -> Result<String, VirtualDriveError> {
    #[cfg(windows)]
    {
        imp::select_mount_drive_letter(preferred_drive_letter)
    }

    #[cfg(not(windows))]
    {
        let _ = preferred_drive_letter;
        Err(VirtualDriveError::UnsupportedPlatform)
    }
}

pub fn unmount_virtual_drive(drive_letter: &str) -> Result<(), VirtualDriveError> {
    #[cfg(windows)]
    {
        imp::unmount_virtual_drive(drive_letter)
    }

    #[cfg(not(windows))]
    {
        let _ = drive_letter;
        Err(VirtualDriveError::UnsupportedPlatform)
    }
}

pub fn get_virtual_drive_target(
    drive_letter: &str,
) -> Result<Option<std::path::PathBuf>, VirtualDriveError> {
    #[cfg(windows)]
    {
        imp::get_virtual_drive_target(drive_letter)
    }

    #[cfg(not(windows))]
    {
        let _ = drive_letter;
        Err(VirtualDriveError::UnsupportedPlatform)
    }
}

pub fn list_virtual_drives() -> Result<Vec<(String, std::path::PathBuf)>, VirtualDriveError> {
    #[cfg(windows)]
    {
        imp::list_virtual_drives()
    }

    #[cfg(not(windows))]
    {
        Err(VirtualDriveError::UnsupportedPlatform)
    }
}

pub fn hide_sync_root(target_path: &Path) -> Result<(), VirtualDriveError> {
    #[cfg(windows)]
    {
        imp::hide_sync_root(target_path)
    }

    #[cfg(not(windows))]
    {
        let _ = target_path;
        Err(VirtualDriveError::UnsupportedPlatform)
    }
}

pub fn configure_virtual_drive_appearance(
    drive_letter: &str,
    label: &str,
    icon_path: &Path,
) -> Result<(), VirtualDriveError> {
    #[cfg(windows)]
    {
        imp::configure_virtual_drive_appearance(drive_letter, label, icon_path)
    }

    #[cfg(not(windows))]
    {
        let _ = drive_letter;
        let _ = label;
        let _ = icon_path;
        Err(VirtualDriveError::UnsupportedPlatform)
    }
}

#[cfg(windows)]
mod imp {
    use super::VirtualDriveError;
    use std::ffi::OsStr;
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetFileAttributesW, GetLogicalDrives, SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN,
        FILE_FLAGS_AND_ATTRIBUTES,
    };
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, REG_SZ,
    };
    use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};

    pub fn mount_virtual_drive(
        drive_letter: &str,
        target_path: &Path,
    ) -> Result<(), VirtualDriveError> {
        let device_name = normalize_drive_letter(drive_letter)?;
        let normalized = normalize_path(target_path)?;
        let _ = unmount_virtual_drive(&device_name);
        let output = Command::new("subst")
            .arg(&device_name)
            .arg(&normalized)
            .creation_flags(CREATE_NO_WINDOW)
            .output()?;
        if !output.status.success() {
            return Err(VirtualDriveError::CommandFailed(format!(
                "subst {} {} failed: {}",
                device_name,
                normalized.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }

        let drive_root = format!(r"{}\",
            device_name
        );
        std::fs::read_dir(&drive_root)
            .map_err(|err| VirtualDriveError::CommandFailed(format!("mounted drive {} is not browsable: {}", drive_root, err)))?;

        Ok(())
    }

    pub fn select_mount_drive_letter(
        preferred_drive_letter: &str,
    ) -> Result<String, VirtualDriveError> {
        let preferred = normalize_drive_letter(preferred_drive_letter)?;
        let preferred_letter = preferred
            .chars()
            .next()
            .ok_or(VirtualDriveError::InvalidDriveLetter)?;
        let used_mask = unsafe { GetLogicalDrives() };

        if used_mask == 0 {
            return Ok(preferred);
        }

        if drive_letter_available(preferred_letter, used_mask) {
            return Ok(preferred);
        }

        for letter in ('D'..='Z').filter(|letter| *letter != preferred_letter) {
            if drive_letter_available(letter, used_mask) {
                return Ok(format!("{letter}:"));
            }
        }

        Err(VirtualDriveError::InvalidDriveLetter)
    }

    pub fn unmount_virtual_drive(drive_letter: &str) -> Result<(), VirtualDriveError> {
        let device_name = normalize_drive_letter(drive_letter)?;
        let output = Command::new("subst")
            .arg(&device_name)
            .arg("/D")
            .creation_flags(CREATE_NO_WINDOW)
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr_trimmed = stderr.trim();
            if !stderr_trimmed.is_empty()
                && !stderr_trimmed.contains("The system cannot find the drive specified")
                && !stderr_trimmed.contains("Nie można odnaleźć określonego dysku")
            {
                return Err(VirtualDriveError::CommandFailed(format!(
                    "subst {} /D failed: {}",
                    device_name, stderr_trimmed
                )));
            }
        }

        Ok(())
    }

    pub fn get_virtual_drive_target(
        drive_letter: &str,
    ) -> Result<Option<PathBuf>, VirtualDriveError> {
        let normalized = normalize_drive_letter(drive_letter)?;
        Ok(list_virtual_drives()?
            .into_iter()
            .find_map(|(letter, target)| (letter == normalized).then_some(target)))
    }

    pub fn list_virtual_drives() -> Result<Vec<(String, PathBuf)>, VirtualDriveError> {
        let output = Command::new("subst").creation_flags(CREATE_NO_WINDOW).output()?;
        if !output.status.success() {
            return Err(VirtualDriveError::CommandFailed(format!(
                "subst query failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut mappings = Vec::new();
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Some((drive, target)) = trimmed.split_once("=>") else {
                continue;
            };
            let drive = normalize_drive_letter(drive.trim().trim_end_matches(':'))?;
            let target = PathBuf::from(target.trim());
            mappings.push((drive, target));
        }

        Ok(mappings)
    }

    pub fn hide_sync_root(target_path: &Path) -> Result<(), VirtualDriveError> {
        let normalized = normalize_path(target_path)?;
        let path = wide_null(&normalized.to_string_lossy().replace('/', "\\"));

        let current = unsafe { GetFileAttributesW(PCWSTR(path.as_ptr())) };
        if current == u32::MAX {
            return Err(windows::core::Error::from_thread().into());
        }

        let updated = FILE_FLAGS_AND_ATTRIBUTES(current) | FILE_ATTRIBUTE_HIDDEN;
        unsafe { SetFileAttributesW(PCWSTR(path.as_ptr()), updated)? };
        Ok(())
    }

    pub fn configure_virtual_drive_appearance(
        drive_letter: &str,
        label: &str,
        icon_path: &Path,
    ) -> Result<(), VirtualDriveError> {
        let drive = normalize_drive_letter(drive_letter)?;
        let drive_key = drive
            .chars()
            .next()
            .ok_or(VirtualDriveError::InvalidDriveLetter)?
            .to_ascii_uppercase();
        let icon_path = normalize_path(icon_path)?;
        let icon_resource = format!("{},0", icon_path.to_string_lossy().replace('/', "\\"));
        let base_key = format!(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\DriveIcons\\{drive_key}"
        );
        let icon_key = format!("{base_key}\\DefaultIcon");
        let label_key = format!("{base_key}\\DefaultLabel");

        set_registry_default_value(&icon_key, &icon_resource)?;
        set_registry_default_value(&label_key, label)?;

        unsafe {
            SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
        }

        Ok(())
    }

    fn normalize_drive_letter(drive_letter: &str) -> Result<String, VirtualDriveError> {
        let trimmed = drive_letter.trim().trim_end_matches('\\').trim_end_matches('/');
        let core = trimmed.strip_suffix(':').unwrap_or(trimmed);

        if core.len() != 1 {
            return Err(VirtualDriveError::InvalidDriveLetter);
        }

        let letter = core.chars().next().ok_or(VirtualDriveError::InvalidDriveLetter)?;
        if !letter.is_ascii_alphabetic() {
            return Err(VirtualDriveError::InvalidDriveLetter);
        }

        Ok(format!("{}:", letter.to_ascii_uppercase()))
    }

    fn drive_letter_available(letter: char, used_mask: u32) -> bool {
        let bit = 1u32 << (letter.to_ascii_uppercase() as u8 - b'A');
        used_mask & bit == 0
    }

    fn normalize_path(target_path: &Path) -> Result<PathBuf, VirtualDriveError> {
        let canonical = target_path.canonicalize()?;
        let rendered = canonical.to_string_lossy().replace('/', "\\");
        let trimmed = rendered.strip_prefix(r"\\?\").unwrap_or(&rendered);
        let normalized = PathBuf::from(trimmed);

        if !normalized.is_absolute() {
            return Err(VirtualDriveError::InvalidPath);
        }

        Ok(normalized)
    }
    fn set_registry_default_value(path: &str, value: &str) -> Result<(), VirtualDriveError> {
        let path_w = wide_null(path);
        let value_w = wide_null(value);
        let mut key = HKEY::default();

        unsafe {
            RegCreateKeyW(
                HKEY_CURRENT_USER,
                PCWSTR(path_w.as_ptr()),
                &mut key,
            )
            .ok()?;

            let bytes = std::slice::from_raw_parts(
                value_w.as_ptr().cast::<u8>(),
                value_w.len() * std::mem::size_of::<u16>(),
            );
            let result = RegSetValueExW(key, None, Some(0), REG_SZ, Some(bytes));
            let close_result = RegCloseKey(key);

            result.ok()?;
            close_result.ok()?;
        }

        Ok(())
    }

    fn wide_null(value: &str) -> Vec<u16> {
        OsStr::new(value)
            .encode_wide()
            .chain(iter::once(0))
            .collect()
    }
}
