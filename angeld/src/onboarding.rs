#![allow(dead_code)]

use crate::uploader::ProviderConfig;
use std::env;
use std::fmt;

pub const SYSTEM_CONFIG_ONBOARDING_STATE: &str = "onboarding_state";
pub const SYSTEM_CONFIG_ONBOARDING_MODE: &str = "onboarding_mode";
pub const SYSTEM_CONFIG_LAST_ONBOARDING_STEP: &str = "last_onboarding_step";
pub const SYSTEM_CONFIG_DRAFT_ENV_DETECTED: &str = "draft_env_detected";
pub const SYSTEM_CONFIG_CLOUD_ENABLED: &str = "cloud_enabled";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OnboardingState {
    Initial,
    InProgress,
    Completed,
}

impl OnboardingState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Initial => "INITIAL",
            Self::InProgress => "IN_PROGRESS",
            Self::Completed => "COMPLETED",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "IN_PROGRESS" => Self::InProgress,
            "COMPLETED" => Self::Completed,
            _ => Self::Initial,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OnboardingMode {
    LocalOnly,
    CloudEnabled,
    JoinExisting,
}

impl OnboardingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalOnly => "LOCAL_ONLY",
            Self::CloudEnabled => "CLOUD_ENABLED",
            Self::JoinExisting => "JOIN_EXISTING",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "CLOUD_ENABLED" => Self::CloudEnabled,
            "JOIN_EXISTING" => Self::JoinExisting,
            _ => Self::LocalOnly,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderDraft {
    pub provider_name: String,
    pub endpoint: Option<String>,
    pub region: Option<String>,
    pub bucket: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub force_path_style: bool,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSecretMaterial {
    pub access_key_id: String,
    pub secret_access_key: String,
}

#[derive(Debug)]
pub enum OnboardingSecretError {
    EmptySecret(&'static str),
    Platform(String),
}

impl fmt::Display for OnboardingSecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySecret(field) => write!(f, "secret field {field} cannot be empty"),
            Self::Platform(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for OnboardingSecretError {}

impl ProviderDraft {
    pub fn is_complete(&self) -> bool {
        self.endpoint.as_ref().is_some_and(|value| !value.is_empty())
            && self.region.as_ref().is_some_and(|value| !value.is_empty())
            && self.bucket.as_ref().is_some_and(|value| !value.is_empty())
            && self
                .access_key_id
                .as_ref()
                .is_some_and(|value| !value.is_empty())
            && self
                .secret_access_key
                .as_ref()
                .is_some_and(|value| !value.is_empty())
    }
}

pub fn detect_env_provider_drafts() -> Vec<ProviderDraft> {
    let _ = dotenvy::dotenv();

    [
        (
            "cloudflare-r2",
            "OMNIDRIVE_R2_ENDPOINT",
            "OMNIDRIVE_R2_REGION",
            "auto",
            "OMNIDRIVE_R2_BUCKET",
            "OMNIDRIVE_R2_ACCESS_KEY_ID",
            "OMNIDRIVE_R2_SECRET_ACCESS_KEY",
            "OMNIDRIVE_R2_FORCE_PATH_STYLE",
        ),
        (
            "backblaze-b2",
            "OMNIDRIVE_B2_ENDPOINT",
            "OMNIDRIVE_B2_REGION",
            "eu-central-003",
            "OMNIDRIVE_B2_BUCKET",
            "OMNIDRIVE_B2_ACCESS_KEY_ID",
            "OMNIDRIVE_B2_SECRET_ACCESS_KEY",
            "OMNIDRIVE_B2_FORCE_PATH_STYLE",
        ),
        (
            "scaleway",
            "OMNIDRIVE_SCALEWAY_ENDPOINT",
            "OMNIDRIVE_SCALEWAY_REGION",
            "pl-waw",
            "OMNIDRIVE_SCALEWAY_BUCKET",
            "OMNIDRIVE_SCALEWAY_ACCESS_KEY_ID",
            "OMNIDRIVE_SCALEWAY_SECRET_ACCESS_KEY",
            "OMNIDRIVE_SCALEWAY_FORCE_PATH_STYLE",
        ),
    ]
    .into_iter()
    .filter_map(
        |(
            provider_name,
            endpoint_key,
            region_key,
            default_region,
            bucket_key,
            access_key_key,
            secret_key_key,
            force_path_style_key,
        )| {
            let endpoint = env_value(endpoint_key);
            let bucket = env_value(bucket_key);
            let access_key_id = env_value(access_key_key);
            let secret_access_key = env_value(secret_key_key);
            let region = env_value(region_key).or_else(|| Some(default_region.to_string()));
            let force_path_style = env_flag(force_path_style_key);

            let any_present = endpoint.is_some()
                || bucket.is_some()
                || access_key_id.is_some()
                || secret_access_key.is_some();
            if !any_present {
                return None;
            }

            Some(ProviderDraft {
                provider_name: provider_name.to_string(),
                endpoint,
                region,
                bucket,
                access_key_id,
                secret_access_key,
                force_path_style,
                source: ".env".to_string(),
            })
        },
    )
    .collect()
}

pub(crate) fn provider_config_from_env(provider_name: &str) -> Option<ProviderConfig> {
    let _ = dotenvy::dotenv();
    match provider_name {
        "cloudflare-r2" => ProviderConfig::from_r2_env().ok(),
        "backblaze-b2" => ProviderConfig::from_b2_env().ok(),
        "scaleway" => ProviderConfig::from_scaleway_env().ok(),
        _ => None,
    }
}

pub fn seal_provider_secrets(
    access_key_id: &str,
    secret_access_key: &str,
) -> Result<(Vec<u8>, Vec<u8>), OnboardingSecretError> {
    if access_key_id.trim().is_empty() {
        return Err(OnboardingSecretError::EmptySecret("access_key_id"));
    }
    if secret_access_key.trim().is_empty() {
        return Err(OnboardingSecretError::EmptySecret("secret_access_key"));
    }

    Ok((
        protect_for_current_user(access_key_id.as_bytes())?,
        protect_for_current_user(secret_access_key.as_bytes())?,
    ))
}

pub fn unseal_provider_secrets(
    access_key_id_ciphertext: &[u8],
    secret_access_key_ciphertext: &[u8],
) -> Result<ProviderSecretMaterial, OnboardingSecretError> {
    let access_key_id = String::from_utf8(unprotect_for_current_user(access_key_id_ciphertext)?)
        .map_err(|err| OnboardingSecretError::Platform(format!("invalid UTF-8 in access key material: {err}")))?;
    let secret_access_key =
        String::from_utf8(unprotect_for_current_user(secret_access_key_ciphertext)?).map_err(
            |err| {
                OnboardingSecretError::Platform(format!(
                    "invalid UTF-8 in secret key material: {err}"
                ))
            },
        )?;

    Ok(ProviderSecretMaterial {
        access_key_id,
        secret_access_key,
    })
}

fn env_value(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_flag(key: &str) -> bool {
    matches!(
        env::var(key)
            .ok()
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("1" | "true" | "yes")
    )
}

#[cfg(windows)]
fn protect_for_current_user(plaintext: &[u8]) -> Result<Vec<u8>, OnboardingSecretError> {
    use windows::Win32::Security::Cryptography::{
        CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB, CryptProtectData,
    };
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::core::PCWSTR;

    let input = CRYPT_INTEGER_BLOB {
        cbData: plaintext.len() as u32,
        pbData: plaintext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();

    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|err| {
            OnboardingSecretError::Platform(format!("CryptProtectData failed: {err}"))
        })?;

        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData as _)));
        Ok(bytes)
    }
}

#[cfg(windows)]
fn unprotect_for_current_user(ciphertext: &[u8]) -> Result<Vec<u8>, OnboardingSecretError> {
    use windows::Win32::Security::Cryptography::{
        CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB, CryptUnprotectData,
    };
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::core::PWSTR;

    let input = CRYPT_INTEGER_BLOB {
        cbData: ciphertext.len() as u32,
        pbData: ciphertext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let mut description = PWSTR::null();

    unsafe {
        CryptUnprotectData(
            &input,
            Some(&mut description),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|err| {
            OnboardingSecretError::Platform(format!("CryptUnprotectData failed: {err}"))
        })?;

        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData as _)));
        if !description.is_null() {
            let _ = LocalFree(Some(HLOCAL(description.0 as _)));
        }
        Ok(bytes)
    }
}

#[cfg(not(windows))]
fn protect_for_current_user(_plaintext: &[u8]) -> Result<Vec<u8>, OnboardingSecretError> {
    Err(OnboardingSecretError::Platform(
        "provider secret sealing is only implemented on Windows".to_string(),
    ))
}

#[cfg(not(windows))]
fn unprotect_for_current_user(_ciphertext: &[u8]) -> Result<Vec<u8>, OnboardingSecretError> {
    Err(OnboardingSecretError::Platform(
        "provider secret unsealing is only implemented on Windows".to_string(),
    ))
}
