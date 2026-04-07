//! OmniDrive Shell Extension — IContextMenu for Explorer (Epic 35.2a-b)
//!
//! COM DLL (cdylib) loaded into explorer.exe.
//! ZERO business logic, ZERO async runtime, ZERO heavy dependencies.
//! Every export and COM method wrapped in catch_unwind.

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::Registry::*;
use windows::Win32::System::SystemServices::*;
use windows::Win32::UI::Shell::Common::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

// ── CLSID ──────────────────────────────────────────────────────────────────

const CLSID_OMNIDRIVE: GUID = GUID {
    data1: 0x8D43_7341,
    data2: 0xB89B,
    data3: 0x4D14,
    data4: [0x99, 0x83, 0x5A, 0x50, 0x52, 0x9A, 0x88, 0xB4],
};

const CLSID_STR: &str = "{8D437341-B89B-4D14-9983-5A50529A88B4}";
const EXTENSION_NAME: &str = "OmniDrive";

static OBJECT_COUNT: AtomicUsize = AtomicUsize::new(0);
static DLL_HMODULE: Mutex<usize> = Mutex::new(0);

// ── Logging ────────────────────────────────────────────────────────────────

fn log_to_file(msg: &str) {
    let _ = (|| -> std::io::Result<()> {
        use std::io::Write;
        let tmp = std::env::temp_dir().join("omnidrive_shell_ext.log");
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(tmp)?;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        writeln!(f, "[{ts}] {msg}")?;
        Ok(())
    })();
}

fn wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

// ── DLL entry point ────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
unsafe extern "system" fn DllMain(hinst: HMODULE, reason: u32, _reserved: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        let _ = catch_unwind(|| {
            if let Ok(mut h) = DLL_HMODULE.lock() {
                *h = hinst.0 as usize;
            }
            log_to_file("DllMain: DLL_PROCESS_ATTACH");
        });
    }
    TRUE
}

// ── COM exports ────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    let result = catch_unwind(|| unsafe {
        let rclsid = &*rclsid;
        let riid = &*riid;
        let ppv = &mut *ppv;
        *ppv = std::ptr::null_mut();

        if *rclsid != CLSID_OMNIDRIVE {
            return CLASS_E_CLASSNOTAVAILABLE;
        }

        let factory: IClassFactory = OmniDriveClassFactory.into();
        factory.query(riid, ppv)
    });
    match result {
        Ok(hr) => hr,
        Err(_) => {
            log_to_file("PANIC in DllGetClassObject");
            E_FAIL
        }
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    catch_unwind(|| {
        if OBJECT_COUNT.load(Ordering::SeqCst) == 0 { S_OK } else { S_FALSE }
    })
    .unwrap_or(S_FALSE)
}

#[unsafe(no_mangle)]
unsafe extern "system" fn DllRegisterServer() -> HRESULT {
    catch_unwind(|| {
        if let Err(e) = register_server() {
            log_to_file(&format!("DllRegisterServer FAILED: {e}"));
            return SELFREG_E_CLASS;
        }
        log_to_file("DllRegisterServer OK");
        S_OK
    })
    .unwrap_or(SELFREG_E_CLASS)
}

#[unsafe(no_mangle)]
unsafe extern "system" fn DllUnregisterServer() -> HRESULT {
    catch_unwind(|| {
        if let Err(e) = unregister_server() {
            log_to_file(&format!("DllUnregisterServer FAILED: {e}"));
            return SELFREG_E_CLASS;
        }
        log_to_file("DllUnregisterServer OK");
        S_OK
    })
    .unwrap_or(SELFREG_E_CLASS)
}

// ── Registry ───────────────────────────────────────────────────────────────

fn get_dll_path() -> std::result::Result<String, String> {
    let h = DLL_HMODULE.lock().map_err(|e| format!("lock: {e}"))?;
    let hmod = HMODULE(*h as *mut c_void);
    let mut buf = [0u16; MAX_PATH as usize];
    let len = unsafe { GetModuleFileNameW(Some(hmod), &mut buf) };
    if len == 0 {
        return Err("GetModuleFileNameW returned 0".into());
    }
    Ok(String::from_utf16_lossy(&buf[..len as usize]))
}

fn reg_set_string(key: HKEY, name: Option<&str>, value: &str) -> std::result::Result<(), String> {
    let wide_name = name.map(wide_null);
    let wide_val = wide_null(value);
    let name_ptr = PCWSTR(wide_name.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()));
    let val_bytes = unsafe {
        std::slice::from_raw_parts(wide_val.as_ptr() as *const u8, wide_val.len() * 2)
    };
    let status = unsafe { RegSetValueExW(key, name_ptr, Some(0), REG_SZ, Some(val_bytes)) };
    if status.is_err() {
        return Err(format!("RegSetValueExW: {}", status.0));
    }
    Ok(())
}

fn reg_create_key(parent: HKEY, subkey: &str) -> std::result::Result<HKEY, String> {
    let wide = wide_null(subkey);
    let mut key = HKEY::default();
    let status = unsafe { RegCreateKeyW(parent, PCWSTR(wide.as_ptr()), &mut key) };
    if status.is_err() {
        return Err(format!("RegCreateKeyW({subkey}): {}", status.0));
    }
    Ok(key)
}

fn reg_delete_tree(parent: HKEY, subkey: &str) {
    let wide = wide_null(subkey);
    unsafe { let _ = RegDeleteTreeW(parent, PCWSTR(wide.as_ptr())); }
}

fn register_server() -> std::result::Result<(), String> {
    let dll_path = get_dll_path()?;

    // HKCR\CLSID\{...}
    let clsid_key = reg_create_key(HKEY_CLASSES_ROOT, &format!("CLSID\\{CLSID_STR}"))?;
    reg_set_string(clsid_key, None, EXTENSION_NAME)?;
    unsafe { let _ = RegCloseKey(clsid_key); }

    // HKCR\CLSID\{...}\InprocServer32 with ThreadingModel = Apartment
    let inproc_key = reg_create_key(
        HKEY_CLASSES_ROOT,
        &format!("CLSID\\{CLSID_STR}\\InprocServer32"),
    )?;
    reg_set_string(inproc_key, None, &dll_path)?;
    reg_set_string(inproc_key, Some("ThreadingModel"), "Apartment")?;
    unsafe { let _ = RegCloseKey(inproc_key); }

    // HKCR\*\shellex\ContextMenuHandlers\OmniDrive
    let files_key = reg_create_key(
        HKEY_CLASSES_ROOT,
        &format!("*\\shellex\\ContextMenuHandlers\\{EXTENSION_NAME}"),
    )?;
    reg_set_string(files_key, None, CLSID_STR)?;
    unsafe { let _ = RegCloseKey(files_key); }

    // HKCR\Directory\shellex\ContextMenuHandlers\OmniDrive
    let dir_key = reg_create_key(
        HKEY_CLASSES_ROOT,
        &format!("Directory\\shellex\\ContextMenuHandlers\\{EXTENSION_NAME}"),
    )?;
    reg_set_string(dir_key, None, CLSID_STR)?;
    unsafe { let _ = RegCloseKey(dir_key); }

    // Approved list
    let approved_key = reg_create_key(
        HKEY_LOCAL_MACHINE,
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Shell Extensions\\Approved",
    )?;
    reg_set_string(approved_key, Some(CLSID_STR), EXTENSION_NAME)?;
    unsafe { let _ = RegCloseKey(approved_key); }

    Ok(())
}

fn unregister_server() -> std::result::Result<(), String> {
    reg_delete_tree(
        HKEY_CLASSES_ROOT,
        &format!("*\\shellex\\ContextMenuHandlers\\{EXTENSION_NAME}"),
    );
    reg_delete_tree(
        HKEY_CLASSES_ROOT,
        &format!("Directory\\shellex\\ContextMenuHandlers\\{EXTENSION_NAME}"),
    );
    reg_delete_tree(HKEY_CLASSES_ROOT, &format!("CLSID\\{CLSID_STR}"));

    let wide_path = wide_null(
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Shell Extensions\\Approved",
    );
    let wide_clsid = wide_null(CLSID_STR);
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(wide_path.as_ptr()),
            Some(0),
            KEY_WRITE,
            &mut key,
        )
        .is_ok()
        {
            let _ = RegDeleteValueW(key, PCWSTR(wide_clsid.as_ptr()));
            let _ = RegCloseKey(key);
        }
    }

    Ok(())
}

// ── IClassFactory ──────────────────────────────────────────────────────────

#[implement(IClassFactory)]
struct OmniDriveClassFactory;

impl IClassFactory_Impl for OmniDriveClassFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Ref<'_, IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut c_void,
    ) -> Result<()> {
        catch_unwind(AssertUnwindSafe(|| unsafe {
            let ppvobject = &mut *ppvobject;
            *ppvobject = std::ptr::null_mut();

            if punkouter.is_some() {
                return Err(Error::from(CLASS_E_NOAGGREGATION));
            }

            let ext = OmniDriveContextMenu {
                target_path: Mutex::new(None),
            };
            let unknown: IUnknown = ext.into();
            let hr = unknown.query(&*riid, ppvobject);
            if hr.is_err() {
                return Err(Error::from(hr));
            }

            OBJECT_COUNT.fetch_add(1, Ordering::SeqCst);
            log_to_file("ClassFactory::CreateInstance OK");
            Ok(())
        }))
        .unwrap_or_else(|_| {
            log_to_file("PANIC in ClassFactory::CreateInstance");
            Err(Error::from(E_FAIL))
        })
    }

    fn LockServer(&self, flock: BOOL) -> Result<()> {
        let _ = catch_unwind(|| {
            if flock.as_bool() {
                OBJECT_COUNT.fetch_add(1, Ordering::SeqCst);
            } else {
                OBJECT_COUNT.fetch_sub(1, Ordering::SeqCst);
            }
        });
        Ok(())
    }
}

// ── OmniDriveContextMenu ──────────────────────────────────────────────────

#[implement(IShellExtInit, IContextMenu)]
struct OmniDriveContextMenu {
    target_path: Mutex<Option<String>>,
}

impl Drop for OmniDriveContextMenu {
    fn drop(&mut self) {
        OBJECT_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

// ── IShellExtInit ──────────────────────────────────────────────────────────

impl IShellExtInit_Impl for OmniDriveContextMenu_Impl {
    fn Initialize(
        &self,
        _pidlfolder: *const ITEMIDLIST,
        pdtobj: Ref<'_, IDataObject>,
        _hkeyprogid: HKEY,
    ) -> Result<()> {
        let this = AssertUnwindSafe(self);
        catch_unwind(move || {
            let dataobj: &IDataObject = pdtobj.ok()?;
            let path = extract_first_path(dataobj)?;

            // Early bail: only O:\ (our virtual drive)
            if !path.starts_with("O:\\") && !path.starts_with("o:\\") {
                return Err(Error::from(E_FAIL));
            }

            log_to_file(&format!("Initialize — target: {path}"));
            if let Ok(mut p) = this.target_path.lock() {
                *p = Some(path);
            }
            Ok(())
        })
        .unwrap_or_else(|_| {
            log_to_file("PANIC in Initialize");
            Err(Error::from(E_FAIL))
        })
    }
}

/// Extract first file path using modern Shell Item API (avoids STGMEDIUM).
fn extract_first_path(dataobj: &IDataObject) -> Result<String> {
    unsafe {
        let items: IShellItemArray =
            SHCreateShellItemArrayFromDataObject(dataobj)?;
        let item: IShellItem = items.GetItemAt(0)?;
        let display_name = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = display_name.to_string()?;
        Ok(path)
    }
}

// ── IContextMenu ───────────────────────────────────────────────────────────

const CMD_FREE_SPACE: u32 = 0;
const CMD_DOWNLOAD: u32 = 1;
const CMD_COUNT: u32 = 2;

impl IContextMenu_Impl for OmniDriveContextMenu_Impl {
    fn QueryContextMenu(
        &self,
        hmenu: HMENU,
        indexmenu: u32,
        idcmdfirst: u32,
        _idcmdlast: u32,
        _uflags: u32,
    ) -> HRESULT {
        let result = catch_unwind(AssertUnwindSafe(|| -> std::result::Result<HRESULT, Error> {
            unsafe {
                let popup = CreatePopupMenu()?;

                let text_free = wide_null("Zwolnij miejsce");
                AppendMenuW(
                    popup,
                    MF_STRING,
                    (idcmdfirst + CMD_FREE_SPACE) as usize,
                    PCWSTR(text_free.as_ptr()),
                )?;

                let text_dl = wide_null("Pobierz na to urz\u{0105}dzenie");
                AppendMenuW(
                    popup,
                    MF_STRING,
                    (idcmdfirst + CMD_DOWNLOAD) as usize,
                    PCWSTR(text_dl.as_ptr()),
                )?;

                let text_omni = wide_null("OmniDrive");
                InsertMenuW(
                    hmenu,
                    indexmenu,
                    MF_BYPOSITION | MF_POPUP,
                    popup.0 as usize,
                    PCWSTR(text_omni.as_ptr()),
                )?;

                log_to_file("QueryContextMenu — submenu inserted");
                Ok(HRESULT(CMD_COUNT as i32))
            }
        }));

        match result {
            Ok(Ok(hr)) => hr,
            Ok(Err(e)) => {
                log_to_file(&format!("QueryContextMenu error: {e}"));
                E_FAIL
            }
            Err(_) => {
                log_to_file("PANIC in QueryContextMenu");
                E_FAIL
            }
        }
    }

    fn InvokeCommand(&self, pici: *const CMINVOKECOMMANDINFO) -> Result<()> {
        let this = AssertUnwindSafe(self);
        catch_unwind(move || {
            let pici = unsafe { &*pici };
            let cmd_id = pici.lpVerb.0 as usize;

            // High bits set = string verb, not our command
            if cmd_id > 0xFFFF {
                return Err(Error::from(E_FAIL));
            }

            let path = this
                .target_path
                .lock()
                .ok()
                .and_then(|p| p.clone())
                .unwrap_or_else(|| "<unknown>".to_string());

            let action = match cmd_id as u32 {
                CMD_FREE_SPACE => "Zwolnij miejsce",
                CMD_DOWNLOAD => "Pobierz na to urządzenie",
                _ => return Err(Error::from(E_INVALIDARG)),
            };

            log_to_file(&format!("InvokeCommand: action=\"{action}\", path=\"{path}\""));
            Ok(())
        })
        .unwrap_or_else(|_| {
            log_to_file("PANIC in InvokeCommand");
            Err(Error::from(E_FAIL))
        })
    }

    fn GetCommandString(
        &self,
        _idcmd: usize,
        _utype: u32,
        _preserved: *const u32,
        _pszname: PSTR,
        _cchmax: u32,
    ) -> Result<()> {
        Err(Error::from(E_NOTIMPL))
    }
}
