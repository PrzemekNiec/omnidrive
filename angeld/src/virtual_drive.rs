use std::fmt;
use std::path::Path;

#[derive(Debug)]
pub enum VirtualDriveError {
    Io(std::io::Error),
    InvalidDriveLetter,
    InvalidPath,
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

#[cfg(windows)]
mod imp {
    use super::VirtualDriveError;
    use std::ffi::OsStr;
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        DefineDosDeviceW, GetFileAttributesW, SetFileAttributesW, DDD_REMOVE_DEFINITION,
        DDD_RAW_TARGET_PATH, FILE_ATTRIBUTE_HIDDEN, FILE_FLAGS_AND_ATTRIBUTES,
    };

    pub fn mount_virtual_drive(
        drive_letter: &str,
        target_path: &Path,
    ) -> Result<(), VirtualDriveError> {
        let device_name = normalize_drive_letter(drive_letter)?;
        let normalized = normalize_path(target_path)?;
        let raw_target = raw_target_path(&normalized);
        let device_name_w = wide_null(&device_name);
        let raw_target_w = wide_null(&raw_target);

        unsafe {
            DefineDosDeviceW(
                DDD_RAW_TARGET_PATH,
                PCWSTR(device_name_w.as_ptr()),
                PCWSTR(raw_target_w.as_ptr()),
            )?;
        }

        Ok(())
    }

    pub fn unmount_virtual_drive(drive_letter: &str) -> Result<(), VirtualDriveError> {
        let device_name = normalize_drive_letter(drive_letter)?;
        let device_name_w = wide_null(&device_name);

        unsafe {
            DefineDosDeviceW(
                DDD_REMOVE_DEFINITION,
                PCWSTR(device_name_w.as_ptr()),
                PCWSTR::null(),
            )?;
        }

        Ok(())
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

    fn raw_target_path(path: &Path) -> String {
        let rendered = path.to_string_lossy().replace('/', "\\");
        format!(r"\??\{rendered}")
    }

    fn wide_null(value: &str) -> Vec<u16> {
        OsStr::new(value)
            .encode_wide()
            .chain(iter::once(0))
            .collect()
    }
}
