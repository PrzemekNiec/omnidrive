use std::fmt;
use std::path::Path;
use tracing::debug;

#[derive(Debug)]
pub enum AclError {
    Io(std::io::Error),
    Platform(String),
}

impl fmt::Display for AclError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "acl i/o error: {err}"),
            Self::Platform(message) => write!(f, "acl platform error: {message}"),
        }
    }
}

impl std::error::Error for AclError {}

impl From<std::io::Error> for AclError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn secure_directory(path: &Path) -> Result<(), AclError> {
    std::fs::create_dir_all(path)?;

    #[cfg(debug_assertions)]
    {
        debug!(
            "skipping ACL hardening in debug build for {}",
            path.display()
        );
        return Ok(());
    }

    #[cfg(not(debug_assertions))]
    {
        let normalized = normalize_directory_path(path)?;
        secure_directory_inner(&normalized)
    }
}

pub fn prepare_sync_root_directory(path: &Path) -> Result<(), AclError> {
    std::fs::create_dir_all(path)?;

    #[cfg(target_os = "windows")]
    {
        let normalized = normalize_directory_path(path)?;
        apply_sync_root_acl_windows(&normalized)?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
    }

    Ok(())
}

fn normalize_directory_path(path: &Path) -> Result<std::path::PathBuf, AclError> {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return Ok(canonical);
    }

    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    Ok(std::env::current_dir()?.join(path))
}

#[cfg_attr(debug_assertions, allow(dead_code))]
#[cfg(target_os = "windows")]
fn secure_directory_inner(path: &Path) -> Result<(), AclError> {
    let sddl = build_runtime_directory_sddl()?;
    apply_sddl_to_directory(path, &sddl, true)
}

#[cfg(target_os = "windows")]
fn apply_sync_root_acl_windows(path: &Path) -> Result<(), AclError> {
    let sddl = build_sync_root_sddl()?;
    match apply_sddl_to_directory(path, &sddl, false) {
        Ok(()) => Ok(()),
        Err(primary_err) => {
            debug!(
                "sync-root ACL via SetNamedSecurityInfoW failed for {}: {}; falling back to icacls",
                path.display(),
                primary_err
            );
            apply_sync_root_acl_with_icacls(path).map_err(|fallback_err| {
                AclError::Platform(format!(
                    "{primary_err}; icacls fallback failed: {fallback_err}"
                ))
            })
        }
    }
}

#[cfg_attr(debug_assertions, allow(dead_code))]
#[cfg(target_os = "windows")]
fn build_runtime_directory_sddl() -> Result<String, AclError> {
    let current_user_sid = current_user_sid_string()?;
    Ok(format!("D:P(A;;GA;;;SY)(A;;GA;;;{current_user_sid})"))
}

#[cfg(target_os = "windows")]
fn build_sync_root_sddl() -> Result<String, AclError> {
    let current_user_sid = current_user_sid_string()?;
    Ok(format!(
        "D:AI(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)(A;OICI;GA;;;{current_user_sid})(A;OICI;GRGWGX;;;AU)"
    ))
}

#[cfg(target_os = "windows")]
fn current_user_sid_string() -> Result<String, AclError> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows::Win32::Security::{TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::core::PWSTR;

    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(platform_error)?;

        let mut token_info_len = 0u32;
        let _ = windows::Win32::Security::GetTokenInformation(
            token,
            TokenUser,
            None,
            0,
            &mut token_info_len,
        );
        let mut token_buffer = vec![0u8; token_info_len as usize];
        windows::Win32::Security::GetTokenInformation(
            token,
            TokenUser,
            Some(token_buffer.as_mut_ptr() as *mut _),
            token_info_len,
            &mut token_info_len,
        )
        .map_err(platform_error)?;
        let token_user = &*(token_buffer.as_ptr() as *const TOKEN_USER);

        let mut sid_string = PWSTR::null();
        ConvertSidToStringSidW(token_user.User.Sid, &mut sid_string).map_err(platform_error)?;
        let current_user_sid = pwstr_to_string(sid_string)?;
        let _ = LocalFree(Some(HLOCAL(sid_string.0 as *mut _)));
        let _ = CloseHandle(token);
        Ok(current_user_sid)
    }
}

#[cfg(target_os = "windows")]
fn apply_sddl_to_directory(
    path: &Path,
    sddl: &str,
    protected: bool,
) -> Result<(), AclError> {
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;
    use windows::Win32::Foundation::{ERROR_SUCCESS, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT,
        SetNamedSecurityInfoW,
    };
    use windows::Win32::Security::{
        DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PSECURITY_DESCRIPTOR,
        PROTECTED_DACL_SECURITY_INFORMATION, UNPROTECTED_DACL_SECURITY_INFORMATION,
    };
    use windows::core::PCWSTR;

    unsafe {
        let sddl_w: Vec<u16> = OsStr::new(&sddl).encode_wide().chain(once(0)).collect();
        let mut security_descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl_w.as_ptr()),
            SDDL_REVISION_1 as u32,
            &mut security_descriptor,
            Some(null_mut()),
        )
        .map_err(platform_error)?;

        let mut dacl_present = false.into();
        let mut dacl_defaulted = false.into();
        let mut dacl = null_mut();
        GetSecurityDescriptorDacl(
            security_descriptor,
            &mut dacl_present,
            &mut dacl,
            &mut dacl_defaulted,
        )
        .map_err(platform_error)?;
        let path_w: Vec<u16> = path.as_os_str().encode_wide().chain(once(0)).collect();
        let security_flags = if protected {
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION
        } else {
            DACL_SECURITY_INFORMATION | UNPROTECTED_DACL_SECURITY_INFORMATION
        };

        let result = SetNamedSecurityInfoW(
            PCWSTR(path_w.as_ptr()),
            SE_FILE_OBJECT,
            security_flags,
            None,
            None,
            Some(dacl),
            None,
        );
        let _ = LocalFree(Some(HLOCAL(security_descriptor.0 as *mut _)));

        if result != ERROR_SUCCESS {
            return Err(AclError::Platform(format!(
                "SetNamedSecurityInfoW failed: {}",
                result.0
            )));
        }
    }

    Ok(())
}

#[cfg(unix)]
fn secure_directory_inner(path: &Path) -> Result<(), AclError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(all(not(target_os = "windows"), not(unix)))]
fn secure_directory_inner(_path: &Path) -> Result<(), AclError> {
    Ok(())
}

#[cfg(target_os = "windows")]
fn apply_sync_root_acl_with_icacls(path: &Path) -> Result<(), String> {
    let current_user_sid =
        current_user_sid_string().map_err(|err| err.to_string())?;
    let path_str = path.as_os_str().to_string_lossy().to_string();

    let output = std::process::Command::new("icacls")
        .arg(&path_str)
        .arg("/inheritance:e")
        .arg("/grant:r")
        .arg("*S-1-5-18:(OI)(CI)F")
        .arg("*S-1-5-32-544:(OI)(CI)F")
        .arg(format!("*{}:(OI)(CI)F", current_user_sid))
        .arg("*S-1-5-11:(OI)(CI)RX")
        .arg("/C")
        .output()
        .map_err(|err| err.to_string())?;

    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "status={:?}, stdout={}, stderr={}",
        output.status.code(),
        stdout.trim(),
        stderr.trim()
    ))
}

#[cfg(target_os = "windows")]
unsafe fn pwstr_to_string(value: windows::core::PWSTR) -> Result<String, AclError> {
    if value.is_null() {
        return Err(AclError::Platform("null PWSTR".to_string()));
    }

    let mut len = 0usize;
    while unsafe { *value.0.add(len) } != 0 {
        len += 1;
    }

    Ok(String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(value.0, len)
    }))
}

#[cfg(target_os = "windows")]
fn platform_error(err: windows::core::Error) -> AclError {
    AclError::Platform(err.to_string())
}
