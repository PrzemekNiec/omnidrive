/// Windows Hello / DPAPI credential storage for lock-screen convenience unlock.
///
/// Flow:
///   1. User unlocks with passphrase → `store_passphrase()` encrypts it with DPAPI
///      and saves the ciphertext to Windows Credential Manager.
///   2. On subsequent starts, `POST /api/unlock/windows-hello` calls
///      `retrieve_passphrase()` → DPAPI decrypts (requires active Windows session,
///      which itself was unlocked with fingerprint/PIN/password) → vault unlocks.
///
/// Security boundary: same as Windows user session (DPAPI user-scope).
/// The stored blob is useless without being logged in as the same Windows account.

#[cfg(windows)]
mod inner {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Security::Credentials::{
        CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
    };
    use windows::core::{PCWSTR, PWSTR};

    const CRED_TARGET: &str = "OmniDrive/VaultPassphrase";

    fn to_wide_null(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    fn dpapi_protect(data: &[u8]) -> Result<Vec<u8>, String> {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        unsafe {
            CryptProtectData(
                &mut input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
        .map_err(|e| format!("CryptProtectData: {e}"))?;
        // Copy before the LocalHeap allocation is reclaimed.
        // Tiny blob (~200 bytes), one-time per unlock — acceptable to not explicitly free.
        let result = unsafe {
            std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec()
        };
        Ok(result)
    }

    fn dpapi_unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        unsafe {
            CryptUnprotectData(
                &mut input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
        .map_err(|e| format!("CryptUnprotectData: {e}"))?;
        let result = unsafe {
            std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec()
        };
        Ok(result)
    }

    fn cred_write(blob: &[u8]) -> Result<(), String> {
        let target = to_wide_null(CRED_TARGET);
        let mut cred: CREDENTIALW = unsafe { std::mem::zeroed() };
        cred.Type = CRED_TYPE_GENERIC;
        cred.TargetName = PWSTR(target.as_ptr() as *mut u16);
        cred.CredentialBlob = blob.as_ptr() as *mut u8;
        cred.CredentialBlobSize = blob.len() as u32;
        cred.Persist = CRED_PERSIST_LOCAL_MACHINE;
        unsafe { CredWriteW(&cred, 0) }.map_err(|e| format!("CredWriteW: {e}"))
    }

    fn cred_read_raw() -> Result<Option<Vec<u8>>, String> {
        let target = to_wide_null(CRED_TARGET);
        let mut ptr: *mut CREDENTIALW = std::ptr::null_mut();
        match unsafe { CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None, &mut ptr) } {
            Ok(_) => {
                let blob = unsafe {
                    std::slice::from_raw_parts(
                        (*ptr).CredentialBlob,
                        (*ptr).CredentialBlobSize as usize,
                    )
                    .to_vec()
                };
                unsafe { CredFree(ptr.cast()) };
                Ok(Some(blob))
            }
            // ERROR_NOT_FOUND (0x80070490) — no credential stored yet
            Err(ref e) if e.code().0 as u32 == 0x80070490 => Ok(None),
            Err(e) => Err(format!("CredReadW: {e}")),
        }
    }

    pub fn store_passphrase(passphrase: &str) -> Result<(), String> {
        let encrypted = dpapi_protect(passphrase.as_bytes())?;
        cred_write(&encrypted)
    }

    pub fn retrieve_passphrase() -> Result<Option<String>, String> {
        let Some(encrypted) = cred_read_raw()? else {
            return Ok(None);
        };
        let plain = dpapi_unprotect(&encrypted)?;
        String::from_utf8(plain).map(Some).map_err(|e| e.to_string())
    }

    pub fn has_stored_credential() -> bool {
        matches!(cred_read_raw(), Ok(Some(_)))
    }
}

#[cfg(windows)]
pub use inner::{has_stored_credential, retrieve_passphrase, store_passphrase};

#[cfg(not(windows))]
pub fn store_passphrase(_passphrase: &str) -> Result<(), String> {
    Err("Windows Hello is only available on Windows".to_string())
}

#[cfg(not(windows))]
pub fn retrieve_passphrase() -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(not(windows))]
pub fn has_stored_credential() -> bool {
    false
}
