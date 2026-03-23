use std::fmt;
use std::path::Path;

#[derive(Debug)]
pub enum ShellIntegrationError {
    Io(std::io::Error),
    #[cfg_attr(windows, allow(dead_code))]
    UnsupportedPlatform,
    #[cfg(windows)]
    Windows(windows::core::Error),
}

impl fmt::Display for ShellIntegrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "shell integration i/o error: {err}"),
            Self::UnsupportedPlatform => write!(f, "shell integration is only supported on Windows"),
            #[cfg(windows)]
            Self::Windows(err) => write!(f, "shell integration windows error: {err}"),
        }
    }
}

impl std::error::Error for ShellIntegrationError {}

impl From<std::io::Error> for ShellIntegrationError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[cfg(windows)]
impl From<windows::core::Error> for ShellIntegrationError {
    fn from(value: windows::core::Error) -> Self {
        Self::Windows(value)
    }
}

pub fn register_explorer_context_menu(
    drive_letter: &str,
    api_base: &str,
    icon_path: &Path,
) -> Result<(), ShellIntegrationError> {
    #[cfg(windows)]
    {
        imp::register_explorer_context_menu(drive_letter, api_base, icon_path)
    }

    #[cfg(not(windows))]
    {
        let _ = drive_letter;
        let _ = api_base;
        let _ = icon_path;
        Err(ShellIntegrationError::UnsupportedPlatform)
    }
}

#[cfg(windows)]
mod imp {
    use super::ShellIntegrationError;
    use std::ffi::OsStr;
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, REG_DWORD, REG_SZ,
    };

    pub fn register_explorer_context_menu(
        drive_letter: &str,
        api_base: &str,
        icon_path: &Path,
    ) -> Result<(), ShellIntegrationError> {
        let drive_root = normalize_drive_root(drive_letter);
        let applies_to = format!(r#"System.ItemPathDisplay:~="{drive_root}\""#);
        let icon_value = format!("{},0", icon_path.canonicalize()?.to_string_lossy().replace('/', "\\"));

        for class_key in ["*", "Directory"] {
            let base = format!(r"Software\Classes\{class_key}\shell\OmniDrive");
            set_string_value(&base, "MUIVerb", "OmniDrive")?;
            set_string_value(&base, "Icon", &icon_value)?;
            set_string_value(&base, "AppliesTo", &applies_to)?;
            set_string_value(&base, "Position", "Top")?;

            write_subcommand(
                &base,
                "policy_paranoia",
                "Protection: Paranoia (3 Clouds)",
                &icon_value,
                &policy_command(api_base, "PARANOIA"),
                false,
            )?;
            write_subcommand(
                &base,
                "policy_standard",
                "Protection: Standard (1 Cloud)",
                &icon_value,
                &policy_command(api_base, "STANDARD"),
                false,
            )?;
            write_subcommand(
                &base,
                "policy_local",
                "Protection: Local Only",
                &icon_value,
                &policy_command(api_base, "LOCAL"),
                false,
            )?;
            write_subcommand(
                &base,
                "free_up_space",
                "Free up space",
                &icon_value,
                &path_command(api_base, "unpin"),
                true,
            )?;
            write_subcommand(
                &base,
                "always_keep",
                "Always keep on this device",
                &icon_value,
                &path_command(api_base, "pin"),
                false,
            )?;
        }

        Ok(())
    }

    fn write_subcommand(
        base: &str,
        verb: &str,
        label: &str,
        icon: &str,
        command: &str,
        separator_before: bool,
    ) -> Result<(), ShellIntegrationError> {
        let key = format!(r"{base}\shell\{verb}");
        set_string_value(&key, "MUIVerb", label)?;
        set_string_value(&key, "Icon", icon)?;
        if separator_before {
            set_dword_value(&key, "CommandFlags", 0x20)?;
        }
        let command_key = format!(r"{key}\command");
        set_default_value(&command_key, command)?;
        Ok(())
    }

    fn policy_command(api_base: &str, policy_type: &str) -> String {
        format!(
            "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -Command \"$p = $args[0]; Invoke-RestMethod -Method Post -Uri '{api_base}/api/filesystem/set-policy' -ContentType 'application/json' -Body (@{{ path = $p; policy_type = '{policy_type}' }} | ConvertTo-Json -Compress) | Out-Null\" \"%1\""
        )
    }

    fn path_command(api_base: &str, action: &str) -> String {
        format!(
            "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -Command \"$p = $args[0]; Invoke-RestMethod -Method Post -Uri '{api_base}/api/filesystem/{action}' -ContentType 'application/json' -Body (@{{ path = $p }} | ConvertTo-Json -Compress) | Out-Null\" \"%1\""
        )
    }

    fn normalize_drive_root(drive_letter: &str) -> String {
        let trimmed = drive_letter.trim().trim_end_matches('\\').trim_end_matches('/');
        let core = trimmed.strip_suffix(':').unwrap_or(trimmed);
        format!("{}:", core.to_ascii_uppercase())
    }

    fn set_default_value(path: &str, value: &str) -> Result<(), ShellIntegrationError> {
        let key = create_key(path)?;
        set_value_internal(key, None, REG_SZ, wide_null(value))?;
        close_key(key)?;
        Ok(())
    }

    fn set_string_value(path: &str, name: &str, value: &str) -> Result<(), ShellIntegrationError> {
        let key = create_key(path)?;
        set_value_internal(key, Some(name), REG_SZ, wide_null(value))?;
        close_key(key)?;
        Ok(())
    }

    fn set_dword_value(path: &str, name: &str, value: u32) -> Result<(), ShellIntegrationError> {
        let key = create_key(path)?;
        let bytes = value.to_le_bytes();
        unsafe {
            RegSetValueExW(
                key,
                PCWSTR(wide_null(name).as_ptr()),
                Some(0),
                REG_DWORD,
                Some(&bytes),
            )
            .ok()?;
        }
        close_key(key)?;
        Ok(())
    }

    fn create_key(path: &str) -> Result<HKEY, ShellIntegrationError> {
        let mut key = HKEY::default();
        let path_w = wide_null(path);
        unsafe {
            RegCreateKeyW(HKEY_CURRENT_USER, PCWSTR(path_w.as_ptr()), &mut key).ok()?;
        }
        Ok(key)
    }

    fn close_key(key: HKEY) -> Result<(), ShellIntegrationError> {
        unsafe {
            RegCloseKey(key).ok()?;
        }
        Ok(())
    }

    fn set_value_internal(
        key: HKEY,
        name: Option<&str>,
        value_type: windows::Win32::System::Registry::REG_VALUE_TYPE,
        wide: Vec<u16>,
    ) -> Result<(), ShellIntegrationError> {
        let name_w = name.map(wide_null);
        let bytes = unsafe {
            std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * std::mem::size_of::<u16>())
        };
        unsafe {
            RegSetValueExW(
                key,
                name_w
                    .as_ref()
                    .map(|value| PCWSTR(value.as_ptr()))
                    .unwrap_or(PCWSTR::null()),
                Some(0),
                value_type,
                Some(bytes),
            )
            .ok()?;
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
