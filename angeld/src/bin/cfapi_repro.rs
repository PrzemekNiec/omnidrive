#[cfg(windows)]
mod win {
    use std::ffi::OsStr;
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use windows::core::{GUID, PCWSTR};
    use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK};
    use windows::Win32::Storage::CloudFilters::{
        CfRegisterSyncRoot, CfUnregisterSyncRoot, CF_HARDLINK_POLICY, CF_HARDLINK_POLICY_NONE,
        CF_HYDRATION_POLICY, CF_HYDRATION_POLICY_FULL, CF_HYDRATION_POLICY_MODIFIER,
        CF_HYDRATION_POLICY_MODIFIER_NONE, CF_HYDRATION_POLICY_PRIMARY, CF_INSYNC_POLICY,
        CF_INSYNC_POLICY_NONE, CF_PLACEHOLDER_MANAGEMENT_POLICY,
        CF_PLACEHOLDER_MANAGEMENT_POLICY_CREATE_UNRESTRICTED, CF_POPULATION_POLICY,
        CF_POPULATION_POLICY_FULL, CF_POPULATION_POLICY_MODIFIER,
        CF_POPULATION_POLICY_MODIFIER_NONE, CF_POPULATION_POLICY_PRIMARY, CF_REGISTER_FLAG_NONE,
        CF_SYNC_POLICIES, CF_SYNC_REGISTRATION,
    };
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

    const PROVIDER_NAME: &str = "OmniDrive_SA";
    const PROVIDER_VERSION: &str = "1.0";
    const SYNC_ROOT_IDENTITY: &str = "OmniDrive_SA_Id";
    const PROVIDER_ID: GUID = GUID::from_u128(0xd2bbeb8c_f4ea_4e1a_8f28_bab1b5e42051);

    struct Cleanup {
        sync_root_wide: Vec<u16>,
        should_uninitialize: bool,
        registered: bool,
    }

    impl Drop for Cleanup {
        fn drop(&mut self) {
            if self.registered {
                let result = unsafe { CfUnregisterSyncRoot(PCWSTR(self.sync_root_wide.as_ptr())) };
                match result {
                    Ok(()) => println!("CfUnregisterSyncRoot => SUCCESS"),
                    Err(err) => println!("CfUnregisterSyncRoot => {err}"),
                }
            }

            if self.should_uninitialize {
                unsafe { CoUninitialize() };
            }
        }
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        println!("CoInitializeEx(COINIT_MULTITHREADED)");
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let should_uninitialize = if hr == S_OK || hr == S_FALSE {
            true
        } else if hr == RPC_E_CHANGED_MODE {
            false
        } else {
            return Err(format!("CoInitializeEx failed: {}", windows::core::Error::from(hr)).into());
        };

        let sync_root = PathBuf::from(r"C:\Users\Przemek\AppData\Local\OmniDrive_StandAlone\SyncRoot");
        println!("create_dir_all({})", sync_root.display());
        std::fs::create_dir_all(&sync_root)?;

        let sync_root_wide = wide_path(&sync_root)?;
        let provider_name_wide = wide_str(PROVIDER_NAME);
        let provider_version_wide = wide_str(PROVIDER_VERSION);
        let identity_bytes = SYNC_ROOT_IDENTITY.as_bytes().to_vec();

        let mut cleanup = Cleanup {
            sync_root_wide: sync_root_wide.clone(),
            should_uninitialize,
            registered: false,
        };

        let registration = CF_SYNC_REGISTRATION {
            StructSize: std::mem::size_of::<CF_SYNC_REGISTRATION>() as u32,
            ProviderName: PCWSTR(provider_name_wide.as_ptr()),
            ProviderVersion: PCWSTR(provider_version_wide.as_ptr()),
            SyncRootIdentity: identity_bytes.as_ptr().cast(),
            SyncRootIdentityLength: identity_bytes.len() as u32,
            FileIdentity: std::ptr::null(),
            FileIdentityLength: 0,
            ProviderId: PROVIDER_ID,
        };

        let policies = CF_SYNC_POLICIES {
            StructSize: std::mem::size_of::<CF_SYNC_POLICIES>() as u32,
            Hydration: CF_HYDRATION_POLICY {
                Primary: CF_HYDRATION_POLICY_PRIMARY(CF_HYDRATION_POLICY_FULL.0),
                Modifier: CF_HYDRATION_POLICY_MODIFIER(CF_HYDRATION_POLICY_MODIFIER_NONE.0),
            },
            Population: CF_POPULATION_POLICY {
                Primary: CF_POPULATION_POLICY_PRIMARY(CF_POPULATION_POLICY_FULL.0),
                Modifier: CF_POPULATION_POLICY_MODIFIER(CF_POPULATION_POLICY_MODIFIER_NONE.0),
            },
            InSync: CF_INSYNC_POLICY(CF_INSYNC_POLICY_NONE.0),
            HardLink: CF_HARDLINK_POLICY(CF_HARDLINK_POLICY_NONE.0),
            PlaceholderManagement: CF_PLACEHOLDER_MANAGEMENT_POLICY(
                CF_PLACEHOLDER_MANAGEMENT_POLICY_CREATE_UNRESTRICTED.0,
            ),
        };

        println!("sync_root={}", sync_root.display());
        println!("provider_name={PROVIDER_NAME}");
        println!("provider_version={PROVIDER_VERSION}");
        println!("identity={SYNC_ROOT_IDENTITY}");
        println!("provider_id={PROVIDER_ID:?}");
        println!("CfRegisterSyncRoot(...)");

        match unsafe {
            CfRegisterSyncRoot(
                PCWSTR(sync_root_wide.as_ptr()),
                &registration,
                &policies,
                CF_REGISTER_FLAG_NONE,
            )
        } {
            Ok(()) => {
                println!("CfRegisterSyncRoot => SUCCESS");
                cleanup.registered = true;
                Ok(())
            }
            Err(err) => Err(format!("CfRegisterSyncRoot failed: {err}").into()),
        }
    }

    fn wide_str(value: &str) -> Vec<u16> {
        OsStr::new(value)
            .encode_wide()
            .chain(iter::once(0))
            .collect()
    }

    fn wide_path(path: &std::path::Path) -> Result<Vec<u16>, Box<dyn std::error::Error>> {
        let canonical = path.canonicalize()?;
        Ok(canonical
            .as_os_str()
            .encode_wide()
            .chain(iter::once(0))
            .collect())
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    win::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("cfapi_repro is only available on Windows");
}
