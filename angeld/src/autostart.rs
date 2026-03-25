use std::fmt;
use std::path::PathBuf;

pub const AUTOSTART_VALUE_NAME: &str = "OmniDriveAngeld";

#[derive(Debug)]
pub enum AutostartError {
    Io(std::io::Error),
    MissingExecutable(PathBuf),
    Platform(String),
}

impl fmt::Display for AutostartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "autostart i/o error: {err}"),
            Self::MissingExecutable(path) => {
                write!(f, "autostart target is missing: {}", path.display())
            }
            Self::Platform(message) => write!(f, "autostart platform error: {message}"),
        }
    }
}

impl std::error::Error for AutostartError {}

impl From<std::io::Error> for AutostartError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn default_current_user_autostart_command() -> Result<String, AutostartError> {
    let current_exe = std::env::current_exe()?;
    let install_dir = current_exe
        .parent()
        .ok_or_else(|| AutostartError::Platform("current executable has no parent".to_string()))?;
    let daemon_exe = install_dir.join("angeld.exe");
    if !daemon_exe.exists() {
        return Err(AutostartError::MissingExecutable(daemon_exe));
    }

    let launcher_vbs = install_dir.join("angeld-autostart.vbs");
    if launcher_vbs.exists() {
        let wscript = windows_wscript_path();
        return Ok(format!(
            "\"{}\" //B \"{}\"",
            wscript.display(),
            launcher_vbs.display()
        ));
    }

    Ok(format!("\"{}\"", daemon_exe.display()))
}

pub fn register_current_user_autostart(command_line: &str) -> Result<(), AutostartError> {
    #[cfg(target_os = "windows")]
    {
        register_current_user_autostart_windows(command_line)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = command_line;
        Err(AutostartError::Platform(
            "current-user autostart is only implemented on Windows".to_string(),
        ))
    }
}

pub fn unregister_current_user_autostart() -> Result<(), AutostartError> {
    #[cfg(target_os = "windows")]
    {
        unregister_current_user_autostart_windows()
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err(AutostartError::Platform(
            "current-user autostart is only implemented on Windows".to_string(),
        ))
    }
}

fn windows_wscript_path() -> PathBuf {
    std::env::var("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join("wscript.exe")
}

#[cfg(target_os = "windows")]
fn register_current_user_autostart_windows(command_line: &str) -> Result<(), AutostartError> {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, KEY_WOW64_64KEY, REG_OPTION_NON_VOLATILE, REG_SZ,
        RegCloseKey, RegCreateKeyExW, RegSetValueExW,
    };
    use windows::core::PCWSTR;

    let subkey = wide_null("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let value_name = wide_null(AUTOSTART_VALUE_NAME);
    let value_data = wide_null(command_line);

    unsafe {
        let mut key = HKEY::default();
        let result = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_WOW64_64KEY,
            None,
            &mut key,
            None,
        );
        if result != ERROR_SUCCESS {
            return Err(AutostartError::Platform(format!(
                "RegCreateKeyExW failed: {}",
                result.0
            )));
        }

        let bytes = std::slice::from_raw_parts(
            value_data.as_ptr() as *const u8,
            value_data.len() * std::mem::size_of::<u16>(),
        );
        let result = RegSetValueExW(
            key,
            PCWSTR(value_name.as_ptr()),
            Some(0),
            REG_SZ,
            Some(bytes),
        );
        let _ = RegCloseKey(key);
        if result != ERROR_SUCCESS {
            return Err(AutostartError::Platform(format!(
                "RegSetValueExW failed: {}",
                result.0
            )));
        }
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn unregister_current_user_autostart_windows() -> Result<(), AutostartError> {
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, KEY_WOW64_64KEY, RegCloseKey, RegDeleteValueW,
        RegOpenKeyExW,
    };
    use windows::core::PCWSTR;

    let subkey = wide_null("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let value_name = wide_null(AUTOSTART_VALUE_NAME);

    unsafe {
        let mut key = HKEY::default();
        let result = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            KEY_SET_VALUE | KEY_WOW64_64KEY,
            &mut key,
        );
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        if result.0 != 0 {
            return Err(AutostartError::Platform(format!(
                "RegOpenKeyExW failed: {}",
                result.0
            )));
        }

        let result = RegDeleteValueW(key, PCWSTR(value_name.as_ptr()));
        let _ = RegCloseKey(key);
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        if result.0 != 0 {
            return Err(AutostartError::Platform(format!(
                "RegDeleteValueW failed: {}",
                result.0
            )));
        }
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
