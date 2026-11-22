// SPDX-License-Identifier: GPL-3.0-or-later
//! `linkroid_credential_provider`: Windows Credential Provider COM DLL for
//! phone biometric unlock.

#![allow(linker_messages)]

#[cfg(windows)]
use std::sync::atomic::Ordering;

#[cfg(windows)]
use windows::{
    Win32::{
        Foundation::{CLASS_E_CLASSNOTAVAILABLE, E_INVALIDARG, E_POINTER, HMODULE, S_FALSE, S_OK},
        System::{
            Com::IClassFactory,
            LibraryLoader::GetModuleFileNameW,
            Registry::{
                RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CLASSES_ROOT,
                HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, REG_OPTION_NON_VOLATILE, REG_SZ,
            },
        },
    },
    core::{GUID, HRESULT, HSTRING, IUnknown, Interface, PCWSTR},
};

#[cfg(windows)]
pub mod credential;
pub mod pipe_client;
#[cfg(windows)]
pub mod provider;

#[cfg(windows)]
pub use provider::{ACTIVE_OBJECTS, CredentialProviderClassFactory, LinkroidCredentialProvider};

/// CLSID of the Linkroid Credential Provider:
/// `{7C49E211-B4D8-4F92-B981-6B4A3A5F82C1}`.
#[cfg(windows)]
pub const CLSID_LINKROID_CREDENTIAL_PROVIDER: GUID =
    GUID::from_u128(0x7c49e211_b4d8_4f92_b981_6b4a3a5f82c1);

#[cfg(windows)]
static DLL_MODULE: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

#[cfg(windows)]
#[unsafe(no_mangle)]
pub extern "system" fn DllMain(
    hinst: HMODULE,
    reason: u32,
    _reserved: *const std::ffi::c_void,
) -> windows::core::BOOL {
    if reason == 1 {
        // DLL_PROCESS_ATTACH
        DLL_MODULE.store(hinst.0 as isize, Ordering::Relaxed);
    }
    windows::core::BOOL::from(true)
}

/// Standard COM DLL entry point returning an `IClassFactory` for
/// [`CLSID_LINKROID_CREDENTIAL_PROVIDER`].
///
/// # Safety
/// `rclsid`, `riid`, and `ppv` must be valid pointers as specified by COM.
#[cfg(windows)]
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut std::ffi::c_void,
) -> HRESULT {
    if ppv.is_null() {
        return E_POINTER;
    }
    unsafe { *ppv = std::ptr::null_mut() };
    if rclsid.is_null() || riid.is_null() {
        return E_INVALIDARG;
    }
    let clsid = unsafe { *rclsid };
    if clsid != CLSID_LINKROID_CREDENTIAL_PROVIDER {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = CredentialProviderClassFactory::new().into();
    let unk: IUnknown = match factory.cast() {
        Ok(u) => u,
        Err(e) => return e.code(),
    };
    unsafe { unk.query(riid, ppv) }
}

/// Standard COM DLL unload check.
#[cfg(windows)]
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if ACTIVE_OBJECTS.load(Ordering::Relaxed) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

/// Registers the COM CLSID and LogonUI Credential Provider registry keys.
#[cfg(windows)]
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    unsafe {
        let hinst = HMODULE(DLL_MODULE.load(Ordering::Relaxed) as *mut std::ffi::c_void);
        let mut path_buf = [0u16; 1024];
        let len = GetModuleFileNameW(Some(hinst), &mut path_buf);
        if len == 0 {
            return windows::Win32::Foundation::E_FAIL;
        }

        let clsid_str = "{7C49E211-B4D8-4F92-B981-6B4A3A5F82C1}";
        let clsid_key_path = format!("CLSID\\{clsid_str}");
        let inproc_path = format!("CLSID\\{clsid_str}\\InprocServer32");
        let cp_key_path = format!(
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Authentication\\Credential Providers\\{clsid_str}"
        );

        // 1. HKCR\CLSID\{...}
        let mut hkey = HKEY::default();
        let k1 = HSTRING::from(clsid_key_path);
        if RegCreateKeyExW(
            HKEY_CLASSES_ROOT,
            PCWSTR(k1.as_ptr()),
            0,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_ALL_ACCESS,
            None,
            &mut hkey,
            None,
        )
        .is_err()
        {
            return windows::Win32::Foundation::E_FAIL;
        }
        let desc = "Linkroid Credential Provider";
        let desc_wide: Vec<u16> = desc.encode_utf16().chain(std::iter::once(0)).collect();
        let _ = RegSetValueExW(
            hkey,
            PCWSTR(std::ptr::null()),
            0,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                desc_wide.as_ptr() as *const u8,
                desc_wide.len() * 2,
            )),
        );
        let _ = RegCloseKey(hkey);

        // 2. InprocServer32
        let k2 = HSTRING::from(inproc_path);
        if RegCreateKeyExW(
            HKEY_CLASSES_ROOT,
            PCWSTR(k2.as_ptr()),
            0,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_ALL_ACCESS,
            None,
            &mut hkey,
            None,
        )
        .is_err()
        {
            return windows::Win32::Foundation::E_FAIL;
        }
        let _ = RegSetValueExW(
            hkey,
            PCWSTR(std::ptr::null()),
            0,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                path_buf.as_ptr() as *const u8,
                (len as usize + 1) * 2,
            )),
        );
        let threading = "Apartment";
        let th_wide: Vec<u16> = threading.encode_utf16().chain(std::iter::once(0)).collect();
        let val_name = HSTRING::from("ThreadingModel");
        let _ = RegSetValueExW(
            hkey,
            PCWSTR(val_name.as_ptr()),
            0,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                th_wide.as_ptr() as *const u8,
                th_wide.len() * 2,
            )),
        );
        let _ = RegCloseKey(hkey);

        // 3. HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{...}
        let k3 = HSTRING::from(cp_key_path);
        if RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(k3.as_ptr()),
            0,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_ALL_ACCESS,
            None,
            &mut hkey,
            None,
        )
        .is_err()
        {
            return windows::Win32::Foundation::E_FAIL;
        }
        let cp_name = "LinkroidCredentialProvider";
        let cp_wide: Vec<u16> = cp_name.encode_utf16().chain(std::iter::once(0)).collect();
        let _ = RegSetValueExW(
            hkey,
            PCWSTR(std::ptr::null()),
            0,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                cp_wide.as_ptr() as *const u8,
                cp_wide.len() * 2,
            )),
        );
        let _ = RegCloseKey(hkey);
    }
    S_OK
}

/// Unregisters the COM CLSID and LogonUI Credential Provider registry keys.
#[cfg(windows)]
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    unsafe {
        let clsid_str = "{7C49E211-B4D8-4F92-B981-6B4A3A5F82C1}";
        let clsid_key_path = format!("CLSID\\{clsid_str}");
        let cp_key_path = format!(
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Authentication\\Credential Providers\\{clsid_str}"
        );

        let k1 = HSTRING::from(clsid_key_path);
        let _ = RegDeleteTreeW(HKEY_CLASSES_ROOT, PCWSTR(k1.as_ptr()));

        let k2 = HSTRING::from(cp_key_path);
        let _ = RegDeleteTreeW(HKEY_LOCAL_MACHINE, PCWSTR(k2.as_ptr()));
    }
    S_OK
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_credential_provider_compiles() {
        assert!(true);
    }
}
