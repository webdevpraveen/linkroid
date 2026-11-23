// SPDX-License-Identifier: GPL-3.0-or-later
//! `ICredentialProviderCredential` implementation for Linkroid phone biometric unlock.

#![allow(unsafe_code, non_snake_case)]

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use windows::{
    Win32::{
        Foundation::{E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY, E_POINTER, NTSTATUS},
        Graphics::Gdi::HBITMAP,
        Security::Authentication::Identity::{
            LsaConnectUntrusted, LsaDeregisterLogonProcess, LsaLookupAuthenticationPackage,
            LSA_STRING,
        },
        System::Com::CoTaskMemAlloc,
        UI::Shell::{
            CPFS_DISPLAYED, CPFIS_NONE, CPGSR_NO_CREDENTIAL_FINISHED, CPGSR_SUCCESS, CPSI_ERROR,
            CPSI_NONE, CPSI_SUCCESS, CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION,
            CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE, CREDENTIAL_PROVIDER_FIELD_STATE,
            CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE, CREDENTIAL_PROVIDER_STATUS_ICON,
            ICredentialProviderCredential, ICredentialProviderCredentialEvents,
            ICredentialProviderCredential_Impl,
        },
    },
    core::{BOOL, PCWSTR, PWSTR, Ref, implement},
};

use crate::CLSID_LINKROID_CREDENTIAL_PROVIDER;
use crate::pipe_client;
use crate::provider::ObjectGuard;
use linkroid_protocol::auth::ipc::{IpcRequest, IpcResponse, UnlockState};

/// Allocates an null-terminated wide string buffer using `CoTaskMemAlloc`.
/// The caller (LogonUI) is responsible for freeing it via `CoTaskMemFree`.
#[allow(unsafe_code)]
pub unsafe fn alloc_co_task_mem_str(s: &str) -> PWSTR {
    let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * std::mem::size_of::<u16>();
    let ptr = CoTaskMemAlloc(bytes) as *mut u16;
    if !ptr.is_null() {
        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
    }
    PWSTR(ptr)
}

#[cfg(windows)]
unsafe fn get_negotiate_package_id() -> u32 {
    let mut lsa_handle = windows::Win32::Foundation::LSA_HANDLE::default();
    if LsaConnectUntrusted(&mut lsa_handle).is_err() {
        return 0;
    }
    let pkg_name = b"Negotiate\0";
    let lsa_str = LSA_STRING {
        Length: 9,
        MaximumLength: 10,
        Buffer: windows::core::PSTR(pkg_name.as_ptr() as *mut u8),
    };
    let mut package_id = 0u32;
    let status = LsaLookupAuthenticationPackage(lsa_handle, &lsa_str, &mut package_id);
    let _ = LsaDeregisterLogonProcess(lsa_handle);
    if status.0 < 0 {
        0
    } else {
        package_id
    }
}

struct CredentialInner {
    advise_context: usize,
    events: Option<ICredentialProviderCredentialEvents>,
    status_text: String,
    session_id: String,
    unlock_success: bool,
    polling_active: Arc<AtomicBool>,
}

#[implement(ICredentialProviderCredential)]
pub struct LinkroidCredential {
    inner: Arc<Mutex<CredentialInner>>,
    _guard: ObjectGuard,
}

impl LinkroidCredential {
    pub fn new(advise_context: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(CredentialInner {
                advise_context,
                events: None,
                status_text: "Select tile to unlock with your phone".into(),
                session_id: String::new(),
                unlock_success: false,
                polling_active: Arc::new(AtomicBool::new(false)),
            })),
            _guard: ObjectGuard::new(),
        }
    }

    fn spawn_status_poller(&self, session_id: String) {
        let inner_arc = Arc::clone(&self.inner);
        let polling_flag = {
            let inner = inner_arc.lock().unwrap_or_else(|e| e.into_inner());
            inner.polling_active.store(true, Ordering::Relaxed);
            Arc::clone(&inner.polling_active)
        };

        std::thread::spawn(move || {
            let start_time = std::time::Instant::now();
            let timeout = std::time::Duration::from_secs(30);

            while polling_flag.load(Ordering::Relaxed) && start_time.elapsed() < timeout {
                std::thread::sleep(std::time::Duration::from_millis(400));
                if !polling_flag.load(Ordering::Relaxed) {
                    break;
                }

                match pipe_client::send_request(&IpcRequest::QueryStatus) {
                    Ok(IpcResponse::Status {
                        active_session: Some(ref active_id),
                        active_unlock_state: Some(UnlockState::Success),
                        ..
                    }) if active_id == &session_id => {
                        let (events, advise_context) = {
                            let mut inner = inner_arc.lock().unwrap_or_else(|e| e.into_inner());
                            inner.unlock_success = true;
                            inner.status_text = "Fingerprint verified! Logging in...".into();
                            (inner.events.clone(), inner.advise_context)
                        };
                        if let Some(ev) = events {
                            unsafe {
                                let _ = ev.OnCredentialsChanged(advise_context);
                            }
                        }
                        break;
                    }
                    Ok(IpcResponse::Status {
                        active_session: Some(ref active_id),
                        active_unlock_state: Some(UnlockState::Failed | UnlockState::Expired),
                        ..
                    }) if active_id == &session_id => {
                        let (events, advise_context) = {
                            let mut inner = inner_arc.lock().unwrap_or_else(|e| e.into_inner());
                            inner.unlock_success = false;
                            inner.status_text =
                                "Unlock rejected or timed out. Click to retry.".into();
                            (inner.events.clone(), inner.advise_context)
                        };
                        if let Some(ev) = events {
                            unsafe {
                                let _ = ev.OnCredentialsChanged(advise_context);
                            }
                        }
                        break;
                    }
                    _ => {}
                }
            }
            polling_flag.store(false, Ordering::Relaxed);
        });
    }
}

impl ICredentialProviderCredential_Impl for LinkroidCredential_Impl {
    fn Advise(
        &self,
        pcpce: Ref<'_, ICredentialProviderCredentialEvents>,
    ) -> windows::core::Result<()> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.events = pcpce.ok().cloned();
        Ok(())
    }

    fn UnAdvise(&self) -> windows::core::Result<()> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.events = None;
        inner.polling_active.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn SetSelected(&self) -> windows::core::Result<BOOL> {
        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.status_text = "Sending prompt to your phone...".into();
            inner.unlock_success = false;
            inner.session_id.clear();
        }

        let start_res = pipe_client::send_request(&IpcRequest::StartUnlock);
        let session_id = match start_res {
            Ok(IpcResponse::UnlockStarted { session_id, .. }) => session_id,
            Ok(IpcResponse::RateLimited { retry_after_secs }) => {
                let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                inner.status_text = format!("Rate limited. Wait {retry_after_secs}s.");
                return Ok(BOOL::from(false));
            }
            Err(e) => {
                let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                inner.status_text = format!("Service unavailable: {e}");
                return Ok(BOOL::from(false));
            }
            _ => {
                let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                inner.status_text = "Failed to start unlock.".into();
                return Ok(BOOL::from(false));
            }
        };

        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.session_id = session_id.clone();
            inner.status_text = "Verify fingerprint on your phone...".into();
        }

        self.spawn_status_poller(session_id);
        Ok(BOOL::from(false))
    }

    fn SetDeselected(&self) -> windows::core::Result<()> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.polling_active.store(false, Ordering::Relaxed);
        inner.status_text = "Select tile to unlock with your phone".into();
        inner.unlock_success = false;
        Ok(())
    }

    fn GetFieldState(
        &self,
        dwfieldid: u32,
        pcpfs: *mut CREDENTIAL_PROVIDER_FIELD_STATE,
        pcpfis: *mut CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE,
    ) -> windows::core::Result<()> {
        if pcpfs.is_null() || pcpfis.is_null() {
            return Err(E_POINTER.into());
        }
        if dwfieldid >= 4 {
            return Err(E_INVALIDARG.into());
        }
        unsafe {
            *pcpfs = CPFS_DISPLAYED;
            *pcpfis = CPFIS_NONE;
        }
        Ok(())
    }

    fn GetStringValue(&self, dwfieldid: u32) -> windows::core::Result<PWSTR> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let text = match dwfieldid {
            1 => "Unlock with Phone",
            2 => &inner.status_text,
            3 => "Retry / Unlock",
            _ => return Err(E_INVALIDARG.into()),
        };
        Ok(unsafe { alloc_co_task_mem_str(text) })
    }

    fn GetBitmapValue(&self, dwfieldid: u32) -> windows::core::Result<HBITMAP> {
        if dwfieldid == 0 {
            // Null HBITMAP instructs LogonUI to display default user tile
            Ok(HBITMAP(std::ptr::null_mut()))
        } else {
            Err(E_INVALIDARG.into())
        }
    }

    fn GetCheckboxValue(
        &self,
        _dwfieldid: u32,
        _pbchecked: *mut BOOL,
        _ppszlabel: *mut PWSTR,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn GetSubmitButtonValue(&self, dwfieldid: u32) -> windows::core::Result<u32> {
        if dwfieldid == 3 {
            Ok(3)
        } else {
            Err(E_INVALIDARG.into())
        }
    }

    fn GetComboBoxValueCount(
        &self,
        _dwfieldid: u32,
        _pcitems: *mut u32,
        _pdwselecteditem: *mut u32,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn GetComboBoxValueAt(&self, _dwfieldid: u32, _dwitem: u32) -> windows::core::Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn SetStringValue(&self, _dwfieldid: u32, _psz: PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn SetCheckboxValue(&self, _dwfieldid: u32, _bchecked: BOOL) -> windows::core::Result<()> {
        Ok(())
    }

    fn SetComboBoxSelectedValue(
        &self,
        _dwfieldid: u32,
        _dwselecteditem: u32,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn CommandLinkClicked(&self, dwfieldid: u32) -> windows::core::Result<()> {
        if dwfieldid == 3 {
            let _ = self.SetSelected();
        }
        Ok(())
    }

    fn GetSerialization(
        &self,
        pcpgsr: *mut CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE,
        pcpcs: *mut CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION,
        ppszoptionalstatustext: *mut PWSTR,
        pcpsioptionalstatusicon: *mut CREDENTIAL_PROVIDER_STATUS_ICON,
    ) -> windows::core::Result<()> {
        if pcpgsr.is_null() || pcpcs.is_null() {
            return Err(E_POINTER.into());
        }

        let (ready, session_id) = {
            let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            (inner.unlock_success, inner.session_id.clone())
        };

        if !ready || session_id.is_empty() {
            unsafe {
                *pcpgsr = CPGSR_NO_CREDENTIAL_FINISHED;
                if !ppszoptionalstatustext.is_null() {
                    *ppszoptionalstatustext = PWSTR(std::ptr::null_mut());
                }
                if !pcpsioptionalstatusicon.is_null() {
                    *pcpsioptionalstatusicon = CPSI_NONE;
                }
            }
            return Ok(());
        }

        match pipe_client::send_request(&IpcRequest::GetLogonSerialization { session_id }) {
            Ok(IpcResponse::LogonSerialization {
                auth_package_id,
                serialized_buffer,
                ..
            }) => unsafe {
                let buf_size = serialized_buffer.len();
                let mem = CoTaskMemAlloc(buf_size) as *mut u8;
                if mem.is_null() {
                    *pcpgsr = CPGSR_NO_CREDENTIAL_FINISHED;
                    return Err(E_OUTOFMEMORY.into());
                }
                std::ptr::copy_nonoverlapping(serialized_buffer.as_ptr(), mem, buf_size);

                let pkg_id = if auth_package_id != 0 {
                    auth_package_id
                } else {
                    get_negotiate_package_id()
                };

                (*pcpcs).clsidCredentialProvider = CLSID_LINKROID_CREDENTIAL_PROVIDER;
                (*pcpcs).ulAuthenticationPackage = pkg_id;
                (*pcpcs).cbSerialization = buf_size as u32;
                (*pcpcs).rgbSerialization = mem;

                *pcpgsr = CPGSR_SUCCESS;
                if !ppszoptionalstatustext.is_null() {
                    *ppszoptionalstatustext = alloc_co_task_mem_str("Unlocked!");
                }
                if !pcpsioptionalstatusicon.is_null() {
                    *pcpsioptionalstatusicon = CPSI_SUCCESS;
                }
                Ok(())
            },
            _ => unsafe {
                *pcpgsr = CPGSR_NO_CREDENTIAL_FINISHED;
                if !ppszoptionalstatustext.is_null() {
                    *ppszoptionalstatustext =
                        alloc_co_task_mem_str("Failed to serialize credentials");
                }
                if !pcpsioptionalstatusicon.is_null() {
                    *pcpsioptionalstatusicon = CPSI_ERROR;
                }
                Ok(())
            },
        }
    }

    fn ReportResult(
        &self,
        ntsstatus: NTSTATUS,
        _ntssubstatus: NTSTATUS,
        ppszoptionalstatustext: *mut PWSTR,
        pcpsioptionalstatusicon: *mut CREDENTIAL_PROVIDER_STATUS_ICON,
    ) -> windows::core::Result<()> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.polling_active.store(false, Ordering::Relaxed);
        if ntsstatus.0 != 0 {
            inner.status_text = "Logon failed".into();
            unsafe {
                if !ppszoptionalstatustext.is_null() {
                    *ppszoptionalstatustext = alloc_co_task_mem_str("Logon failed. Try password.");
                }
                if !pcpsioptionalstatusicon.is_null() {
                    *pcpsioptionalstatusicon = CPSI_ERROR;
                }
            }
        }
        Ok(())
    }
}
