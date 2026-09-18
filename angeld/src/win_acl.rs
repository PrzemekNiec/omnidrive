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

#[allow(dead_code)]
pub fn secure_directory(path: &Path) -> Result<(), AclError> {
    std::fs::create_dir_all(path)?;

    #[cfg(debug_assertions)]
    {
        debug!(
            "skipping ACL hardening in debug build for {}",
            path.display()
        );
        Ok(())
    }

    #[cfg(not(debug_assertions))]
    {
        let normalized = normalize_directory_path(path)?;
        secure_directory_inner(&normalized)
    }
}

pub fn prepare_sync_root_directory(path: &Path) -> Result<(), AclError> {
    std::fs::create_dir_all(path)?;

    #[cfg(debug_assertions)]
    {
        debug!(
            "skipping sync-root ACL hardening in debug build for {}",
            path.display()
        );
    }

    #[cfg(all(target_os = "windows", not(debug_assertions)))]
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

#[cfg_attr(debug_assertions, allow(dead_code))]
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
#[cfg_attr(debug_assertions, allow(dead_code))]
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
    Ok(format!(
        "D:PAI(A;;FA;;;SY)(A;OICIIO;FA;;;SY)(A;;FA;;;{current_user_sid})(A;OICIIO;FA;;;{current_user_sid})"
    ))
}

#[cfg(target_os = "windows")]
#[cfg_attr(debug_assertions, allow(dead_code))]
fn build_sync_root_sddl() -> Result<String, AclError> {
    let current_user_sid = current_user_sid_string()?;
    Ok(format!(
        "D:AI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{current_user_sid})(A;OICI;GRGWGX;;;AU)"
    ))
}

#[cfg(target_os = "windows")]
pub(crate) fn current_user_sid_string() -> Result<String, AclError> {
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
fn apply_sddl_to_directory(path: &Path, sddl: &str, protected: bool) -> Result<(), AclError> {
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
        DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, UNPROTECTED_DACL_SECURITY_INFORMATION,
    };
    use windows::core::PCWSTR;

    unsafe {
        let sddl_w: Vec<u16> = OsStr::new(&sddl).encode_wide().chain(once(0)).collect();
        let mut security_descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl_w.as_ptr()),
            SDDL_REVISION_1,
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

#[cfg(target_os = "windows")]
pub fn write_user_only_file(path: &Path, contents: &[u8]) -> Result<(), AclError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::File::create(path)?;
    let current_user_sid = current_user_sid_string()?;
    let sddl = format!("D:PAI(A;;FA;;;SY)(A;;FA;;;{current_user_sid})");
    apply_sddl_to_directory(path, &sddl, true)?;
    std::fs::write(path, contents)?;
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
#[cfg_attr(debug_assertions, allow(dead_code))]
fn apply_sync_root_acl_with_icacls(path: &Path) -> Result<(), String> {
    let current_user_sid = current_user_sid_string().map_err(|err| err.to_string())?;
    let path_str = path.as_os_str().to_string_lossy().to_string();

    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("icacls")
        .arg(&path_str)
        .arg("/inheritance:e")
        .arg("/grant:r")
        .arg("*S-1-5-18:(OI)(CI)F")
        .arg("*S-1-5-32-544:(OI)(CI)F")
        .arg(format!("*{}:(OI)(CI)F", current_user_sid))
        .arg("*S-1-5-11:(OI)(CI)RX")
        .arg("/C")
        .creation_flags(CREATE_NO_WINDOW)
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
pub(crate) unsafe fn pwstr_to_string(value: windows::core::PWSTR) -> Result<String, AclError> {
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

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_test_file() -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "omnidrive_win_acl_test_{}_{}",
            std::process::id(),
            n
        ))
    }

    #[test]
    fn user_only_file_dacl_has_only_system_and_current_user() {
        use std::ffi::OsStr;
        use std::iter::once;
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Foundation::{HLOCAL, LocalFree};
        use windows::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
            SDDL_REVISION_1, SE_FILE_OBJECT,
        };
        use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
        use windows::core::{PCWSTR, PWSTR};

        let path = unique_test_file();
        let contents = b"session-token-bytes";
        write_user_only_file(&path, contents).expect("write_user_only_file");

        let path_w: Vec<u16> = OsStr::new(&path).encode_wide().chain(once(0)).collect();
        let mut sd = PSECURITY_DESCRIPTOR::default();
        let sddl = unsafe {
            let err = GetNamedSecurityInfoW(
                PCWSTR(path_w.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                None,
                None,
                &mut sd,
            );
            assert_eq!(err.0, 0, "GetNamedSecurityInfoW failed: {}", err.0);

            let mut sddl_ptr = PWSTR::null();
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                sd,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut sddl_ptr,
                None,
            )
            .expect("stringify security descriptor");
            let sddl = pwstr_to_string(sddl_ptr).unwrap();
            let _ = LocalFree(Some(HLOCAL(sddl_ptr.0 as *mut _)));
            let _ = LocalFree(Some(HLOCAL(sd.0 as *mut _)));
            sddl
        };

        let current_user_sid = current_user_sid_string().unwrap();
        assert_eq!(sddl.matches("(A;").count(), 2, "sddl={sddl}");
        assert!(sddl.contains(";;;SY)"), "sddl={sddl}");
        assert!(sddl.contains(&current_user_sid), "sddl={sddl}");
        assert!(!sddl.contains(";;;AU)"), "sddl={sddl}");
        assert!(!sddl.contains(";;;BU)"), "sddl={sddl}");
        assert!(!sddl.contains(";;;WD)"), "sddl={sddl}");
        assert!(!sddl.contains(";;;BA)"), "sddl={sddl}");

        let read_back = std::fs::read(&path).expect("read back file contents");
        assert_eq!(read_back, contents);

        let _ = std::fs::remove_file(&path);
    }
}
