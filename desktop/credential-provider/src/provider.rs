// SPDX-License-Identifier: GPL-3.0-or-later
//! `ICredentialProvider` and `IClassFactory` implementation for Linkroid.

#![allow(unsafe_code, non_snake_case)]

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU32, Ordering},
};

use windows::{
    Win32::{
        Foundation::{
            CLASS_E_NOAGGREGATION, E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY, E_POINTER, S_OK,
        },
        System::Com::{CoTaskMemAlloc, IClassFactory, IClassFactory_Impl},
        UI::Shell::{
            CPFT_COMMAND_LINK, CPFT_LARGE_TEXT, CPFT_SMALL_TEXT, CPFT_TILE_IMAGE,
            CPUS_LOGON, CPUS_UNLOCK_WORKSTATION, CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR,
            CREDENTIAL_PROVIDER_NO_DEFAULT, CREDENTIAL_PROVIDER_USAGE_SCENARIO,
            ICredentialProvider, ICredentialProviderCredential, ICredentialProviderEvents,
            ICredentialProvider_Impl,
        },
    },
    core::{BOOL, GUID, Interface, Ref, implement},
};

use crate::credential::{LinkroidCredential, alloc_co_task_mem_str};
use crate::pipe_client;
use linkroid_protocol::auth::ipc::{IpcRequest, IpcResponse};

/// Global active object counter for `DllCanUnloadNow`.
pub static ACTIVE_OBJECTS: AtomicU32 = AtomicU32::new(0);

#[derive(Debug)]
pub struct ObjectGuard;

impl ObjectGuard {
    pub fn new() -> Self {
        ACTIVE_OBJECTS.fetch_add(1, Ordering::Relaxed);
        Self
    }
}

impl Default for ObjectGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ObjectGuard {
    fn drop(&mut self) {
        ACTIVE_OBJECTS.fetch_sub(1, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct ProviderInner {
    scenario: CREDENTIAL_PROVIDER_USAGE_SCENARIO,
    flags: u32,
    events: Option<ICredentialProviderEvents>,
    advise_context: usize,
}

#[implement(ICredentialProvider)]
pub struct LinkroidCredentialProvider {
    inner: Arc<Mutex<ProviderInner>>,
    _guard: ObjectGuard,
}

impl LinkroidCredentialProvider {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ProviderInner::default())),
            _guard: ObjectGuard::new(),
        }
    }
}

impl Default for LinkroidCredentialProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ICredentialProvider_Impl for LinkroidCredentialProvider_Impl {
    fn SetUsageScenario(
        &self,
        cpus: CREDENTIAL_PROVIDER_USAGE_SCENARIO,
        dwflags: u32,
    ) -> windows::core::Result<()> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.scenario = cpus;
        inner.flags = dwflags;
        if cpus == CPUS_UNLOCK_WORKSTATION || cpus == CPUS_LOGON {
            Ok(())
        } else {
            Err(E_NOTIMPL.into())
        }
    }

    fn SetSerialization(
        &self,
        _pcpcs: *const windows::Win32::UI::Shell::CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn Advise(
        &self,
        pcpe: Ref<'_, ICredentialProviderEvents>,
        upadvisecontext: usize,
    ) -> windows::core::Result<()> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.events = pcpe.ok().ok().cloned();
        inner.advise_context = upadvisecontext;
        Ok(())
    }

    fn UnAdvise(&self) -> windows::core::Result<()> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.events = None;
        inner.advise_context = 0;
        Ok(())
    }

    fn GetFieldDescriptorCount(&self) -> windows::core::Result<u32> {
        Ok(4)
    }

    fn GetFieldDescriptorAt(
        &self,
        dwindex: u32,
    ) -> windows::core::Result<*mut CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR> {
        if dwindex >= 4 {
            return Err(E_INVALIDARG.into());
        }
        unsafe {
            let ptr = CoTaskMemAlloc(std::mem::size_of::<CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR>())
                as *mut CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR;
            if ptr.is_null() {
                return Err(E_OUTOFMEMORY.into());
            }
            let (cpft, label) = match dwindex {
                0 => (CPFT_TILE_IMAGE, ""),
                1 => (CPFT_LARGE_TEXT, "Linkroid Phone Unlock"),
                2 => (CPFT_SMALL_TEXT, "Select to unlock with your phone"),
                3 => (CPFT_COMMAND_LINK, "Unlock"),
                _ => unreachable!(),
            };
            (*ptr).dwFieldID = dwindex;
            (*ptr).cpft = cpft;
            (*ptr).pszLabel = alloc_co_task_mem_str(label);
            (*ptr).guidFieldType = GUID::zeroed();
            Ok(ptr)
        }
    }

    fn GetCredentialCount(
        &self,
        pdwcount: *mut u32,
        pdwdefault: *mut u32,
        pbautologonwithdefault: *mut BOOL,
    ) -> windows::core::Result<()> {
        if pdwcount.is_null() || pdwdefault.is_null() || pbautologonwithdefault.is_null() {
            return Err(E_POINTER.into());
        }
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let supported = inner.scenario == CPUS_UNLOCK_WORKSTATION || inner.scenario == CPUS_LOGON;

        let enrolled = if supported {
            match pipe_client::send_request(&IpcRequest::QueryStatus) {
                Ok(IpcResponse::Status { enrolled, .. }) => enrolled,
                _ => false,
            }
        } else {
            false
        };

        unsafe {
            *pdwcount = if enrolled { 1 } else { 0 };
            // Never hijack default logon tile; keep standard Windows password login primary
            *pdwdefault = CREDENTIAL_PROVIDER_NO_DEFAULT;
            *pbautologonwithdefault = BOOL::from(false);
        }
        Ok(())
    }

    fn GetCredentialAt(
        &self,
        dwindex: u32,
    ) -> windows::core::Result<ICredentialProviderCredential> {
        if dwindex != 0 {
            return Err(E_INVALIDARG.into());
        }
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let cred = LinkroidCredential::new(inner.advise_context, inner.events.clone());
        let cred: ICredentialProviderCredential = cred.into();
        Ok(cred)
    }
}

#[implement(IClassFactory)]
pub struct CredentialProviderClassFactory {
    _guard: ObjectGuard,
}

impl CredentialProviderClassFactory {
    pub fn new() -> Self {
        Self {
            _guard: ObjectGuard::new(),
        }
    }
}

impl Default for CredentialProviderClassFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl IClassFactory_Impl for CredentialProviderClassFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Ref<'_, windows::core::IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut std::ffi::c_void,
    ) -> windows::core::Result<()> {
        if ppvobject.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *ppvobject = std::ptr::null_mut() };
        if !punkouter.is_null() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        if riid.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let provider = LinkroidCredentialProvider::new();
        let provider: ICredentialProvider = provider.into();
        let unk: windows::core::IUnknown = provider.cast()?;
        unsafe { unk.query(riid, ppvobject).ok() }
    }

    fn LockServer(&self, flock: BOOL) -> windows::core::Result<()> {
        if flock.as_bool() {
            ACTIVE_OBJECTS.fetch_add(1, Ordering::Relaxed);
        } else {
            ACTIVE_OBJECTS.fetch_sub(1, Ordering::Relaxed);
        }
        Ok(())
    }
}
