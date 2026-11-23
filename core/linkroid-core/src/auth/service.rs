// SPDX-License-Identifier: MPL-2.0
//! Windows Named Pipe IPC server and service dispatch (Phase 2).
//!
//! Exposes `\\.\pipe\LinkroidAuthPipe` with strict DACL:
//! - SYSTEM (`SY`): Full Control
//! - Administrators (`BA`): Full Control
//! - Authenticated Users (`AU`): Read / Write (LogonUI and interactive user app)
//! - Remote network clients rejected (`PIPE_REJECT_REMOTE_CLIENTS`).

#![allow(unsafe_code)]

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[cfg(windows)]
use linkroid_protocol::auth::ipc::{
    AUTH_PIPE_NAME, decode_ipc_message, encode_ipc_message,
};
use linkroid_protocol::auth::ipc::{IpcRequest, IpcResponse};
#[cfg(windows)]
use tracing::info;
use tracing::warn;

use super::{AuthManager, credential::CredentialStore};

/// Service engine managing the authentication named pipe and dispatching requests.
#[derive(Debug)]
pub struct AuthPipeServer {
    auth: Arc<AuthManager>,
    credential_store: CredentialStore,
    running: Arc<AtomicBool>,
}

impl AuthPipeServer {
    /// Initializes a new `AuthPipeServer`.
    pub fn new(auth: Arc<AuthManager>, storage_dir: PathBuf) -> Self {
        Self {
            auth,
            credential_store: CredentialStore::new(storage_dir),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns a reference to the underlying `CredentialStore`.
    pub fn credential_store(&self) -> &CredentialStore {
        &self.credential_store
    }

    /// Handles an incoming IPC request and produces an IPC response.
    pub fn handle_request(&self, req: IpcRequest) -> IpcResponse {
        match req {
            IpcRequest::QueryStatus => {
                let enrolled = self.auth.has_enrolled_devices();
                let phone_connected = self.auth.is_any_phone_connected();
                let phone_name = self.auth.first_enrolled_phone_name();
                let locked_out = self.auth.is_any_enrolled_phone_locked_out();
                IpcResponse::Status {
                    enrolled,
                    phone_connected,
                    phone_name,
                    locked_out,
                }
            }
            IpcRequest::StartUnlock => {
                match self.auth.start_unlock_for_enrolled_phone() {
                    Ok((session_id, expiry)) => IpcResponse::UnlockInitiated { session_id, expiry },
                    Err(err) => IpcResponse::Error {
                        code: 1,
                        message: err.to_string(),
                    },
                }
            }
            IpcRequest::PollUnlock { session_id } => {
                let state = self.auth.session_state(&session_id);
                IpcResponse::UnlockStatus { session_id, state }
            }
            IpcRequest::LockWorkStation => {
                match lock_workstation() {
                    Ok(()) => IpcResponse::LockAcknowledged { success: true },
                    Err(err) => {
                        warn!(reason = %err, "failed to lock workstation");
                        IpcResponse::Error {
                            code: 2,
                            message: err,
                        }
                    }
                }
            }
            IpcRequest::EnrollCredential {
                username,
                domain,
                encrypted_secret,
            } => {
                match self.credential_store.store(&username, domain.as_deref(), &encrypted_secret) {
                    Ok(()) => IpcResponse::CredentialEnrolled { success: true },
                    Err(err) => IpcResponse::Error {
                        code: 3,
                        message: err,
                    },
                }
            }
            IpcRequest::RevokeCredential => {
                let success = self.credential_store.revoke().unwrap_or(false);
                IpcResponse::CredentialRevoked { success }
            }
            IpcRequest::GetLogonSerialization { session_id } => {
                if self.auth.session_state(&session_id)
                    != linkroid_protocol::auth::ipc::UnlockState::Success
                {
                    return IpcResponse::Error {
                        code: 403,
                        message: "session not authenticated or expired".into(),
                    };
                }
                self.auth.set_session_state(
                    &session_id,
                    linkroid_protocol::auth::ipc::UnlockState::Expired,
                );

                match self.credential_store.load() {
                    Ok(Some((username, domain, secret))) => {
                        #[cfg(windows)]
                        let domain_user = match domain {
                            Some(ref d) if !d.is_empty() => format!("{d}\\{username}"),
                            _ => username,
                        };
                        #[cfg(windows)]
                        {
                            match pack_credentials(&domain_user, secret.as_bytes()) {
                                Ok(serialized_buffer) => {
                                    let auth_package_id = lookup_negotiate_package_id();
                                    IpcResponse::LogonSerialization {
                                        auth_package: "Negotiate".into(),
                                        auth_package_id,
                                        serialized_buffer,
                                    }
                                }
                                Err(e) => IpcResponse::Error {
                                    code: 500,
                                    message: e,
                                },
                            }
                        }
                        #[cfg(not(windows))]
                        {
                            let _ = (username, domain, secret);
                            IpcResponse::LogonSerialization {
                                auth_package: "Negotiate".into(),
                                auth_package_id: 1,
                                serialized_buffer: vec![0u8; 128],
                            }
                        }
                    }
                    Ok(None) => IpcResponse::Error {
                        code: 404,
                        message: "no credentials enrolled on this workstation".into(),
                    },
                    Err(e) => IpcResponse::Error {
                        code: 500,
                        message: e,
                    },
                }
            }
        }
    }

    /// Runs the Named Pipe server listening loop until stopped.
    #[cfg(windows)]
    pub fn run(&self) -> Result<(), String> {
        use windows::{
            Win32::{
                Foundation::{CloseHandle, LocalFree},
                Security::{
                    Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
                    PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
                },
                System::Pipes::{
                    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe,
                    PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS,
                    PIPE_TYPE_MESSAGE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
                },
                Storage::FileSystem::{
                    FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
                },
            },
            core::{HSTRING, PCWSTR, w},
        };

        self.running.store(true, Ordering::SeqCst);
        info!(pipe = AUTH_PIPE_NAME, "starting Linkroid auth Named Pipe server");

        // SDDL allowing SYSTEM (SY) Full Control, Admins (BA) Full Control, Authenticated Users (AU) Read/Write
        let sddl = w!("D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;AU)");
        let mut sd = PSECURITY_DESCRIPTOR::default();
        let mut sa = SECURITY_ATTRIBUTES::default();

        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl,
                1, // SDDL_REVISION_1
                &mut sd,
                None,
            )
        };

        if converted.is_ok() {
            sa.nLength = std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32;
            sa.lpSecurityDescriptor = sd.0;
            sa.bInheritHandle = false.into();
        }

        let pipe_name = HSTRING::from(AUTH_PIPE_NAME);

        while self.running.load(Ordering::SeqCst) {
            let pipe = unsafe {
                CreateNamedPipeW(
                    PCWSTR(pipe_name.as_ptr()),
                    PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                    PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    PIPE_UNLIMITED_INSTANCES,
                    65536,
                    65536,
                    5000,
                    if converted.is_ok() { Some(&sa) } else { None },
                )
            };

            if pipe.is_invalid() {
                warn!("failed to create named pipe; retrying in 1s");
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }

            let pipe_handle = pipe;
            let connected = unsafe { ConnectNamedPipe(pipe_handle, None) };
            if connected.is_ok()
                || unsafe { windows::Win32::Foundation::GetLastError() }
                    == windows::Win32::Foundation::ERROR_PIPE_CONNECTED
            {
                let mut read_buf = vec![0u8; 65536];
                let mut bytes_read = 0u32;
                let read_ok = unsafe {
                    ReadFile(
                        pipe_handle,
                        Some(&mut read_buf),
                        Some(&mut bytes_read),
                        None,
                    )
                };

                if read_ok.is_ok() && bytes_read >= 4 {
                    read_buf.truncate(bytes_read as usize);
                    let resp = match decode_ipc_message::<IpcRequest>(&read_buf) {
                        Ok(req) => self.handle_request(req),
                        Err(e) => IpcResponse::Error {
                            code: 400,
                            message: e,
                        },
                    };

                    if let Ok(encoded_resp) = encode_ipc_message(&resp) {
                        let mut bytes_written = 0u32;
                        let _ = unsafe {
                            WriteFile(
                                pipe_handle,
                                Some(&encoded_resp),
                                Some(&mut bytes_written),
                                None,
                            )
                        };
                    }
                }
            }

            let _ = unsafe { DisconnectNamedPipe(pipe_handle) };
            let _ = unsafe { CloseHandle(pipe_handle) };
        }

        if !sd.0.is_null() {
            unsafe {
                LocalFree(Some(windows::Win32::Foundation::HLOCAL(sd.0)));
            }
        }

        Ok(())
    }

    /// Mock run loop on non-Windows platforms for unit testing and CI.
    #[cfg(not(windows))]
    pub fn run(&self) -> Result<(), String> {
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Stops the server.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// Locks the workstation across all sessions (user session and Session 0 service).
pub fn lock_workstation() -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Shutdown::LockWorkStation;

        // In interactive user sessions, LockWorkStation works directly.
        if unsafe { LockWorkStation() }.is_ok() {
            return Ok(());
        }

        // When running in Session 0 as SYSTEM, LockWorkStation fails with access denied.
        // Fallback: Disconnect active console session, which locks the screen immediately.
        use windows::Win32::System::RemoteDesktop::{
            WTSDisconnectSession, WTSGetActiveConsoleSessionId, WTS_CURRENT_SERVER_HANDLE,
        };

        let active_session = unsafe { WTSGetActiveConsoleSessionId() };
        if active_session != 0xFFFFFFFF {
            let ok = unsafe {
                WTSDisconnectSession(Some(WTS_CURRENT_SERVER_HANDLE), active_session, false)
            };
            if ok.is_ok() {
                return Ok(());
            }
        }

        Err("failed to lock workstation from session".into())
    }

    #[cfg(not(windows))]
    {
        Ok(())
    }
}

#[cfg(windows)]
fn pack_credentials(username: &str, password_utf8: &[u8]) -> Result<Vec<u8>, String> {
    use windows::{
        core::HSTRING,
        Win32::Security::Credentials::{
            CredPackAuthenticationBufferW, CRED_PACK_PROTECTED_CREDENTIALS,
        },
    };

    let user_h = HSTRING::from(username);
    let pass_str = std::str::from_utf8(password_utf8).map_err(|e| e.to_string())?;
    let pass_h = HSTRING::from(pass_str);

    let mut size = 0u32;
    unsafe {
        let _ = CredPackAuthenticationBufferW(
            CRED_PACK_PROTECTED_CREDENTIALS,
            windows::core::PCWSTR(user_h.as_ptr()),
            windows::core::PCWSTR(pass_h.as_ptr()),
            None,
            &mut size,
        );
    }
    if size == 0 {
        return Err("failed to determine packed credential buffer size".into());
    }

    let mut buffer = vec![0u8; size as usize];
    let ok = unsafe {
        CredPackAuthenticationBufferW(
            CRED_PACK_PROTECTED_CREDENTIALS,
            windows::core::PCWSTR(user_h.as_ptr()),
            windows::core::PCWSTR(pass_h.as_ptr()),
            Some(buffer.as_mut_ptr()),
            &mut size,
        )
    };
    if !ok.as_bool() {
        return Err("CredPackAuthenticationBufferW failed to pack credential".into());
    }
    buffer.truncate(size as usize);
    Ok(buffer)
}

#[cfg(windows)]
fn lookup_negotiate_package_id() -> u32 {
    use windows::{
        core::PSTR,
        Win32::Security::Authentication::Identity::{
            LsaConnectUntrusted, LsaDeregisterLogonProcess, LsaLookupAuthenticationPackage,
            LSA_HANDLE, LSA_STRING,
        },
    };

    let mut lsa_handle = LSA_HANDLE::default();
    if unsafe { LsaConnectUntrusted(&mut lsa_handle) }.is_err() {
        return 0;
    }

    let pkg_name = b"Negotiate\0";
    let lsa_str = LSA_STRING {
        Length: 9,
        MaximumLength: 10,
        Buffer: PSTR(pkg_name.as_ptr() as *mut u8),
    };

    let mut package_id = 0u32;
    let status = unsafe { LsaLookupAuthenticationPackage(lsa_handle, &lsa_str, &mut package_id) };
    let _ = unsafe { LsaDeregisterLogonProcess(lsa_handle) };

    if status.0 < 0 { 0 } else { package_id }
}

#[cfg(test)]
mod tests {
    use super::*;
    use linkroid_protocol::DeviceId;

    #[test]
    fn test_auth_pipe_server_lock_request() {
        let auth = Arc::new(AuthManager::new(DeviceId([1u8; 32])));
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let server = AuthPipeServer::new(auth, temp_dir.path().to_path_buf());

        let resp = server.handle_request(IpcRequest::LockWorkStation);
        assert_eq!(resp, IpcResponse::LockAcknowledged { success: true });
    }

    #[test]
    fn test_auth_pipe_server_status_query() {
        let auth = Arc::new(AuthManager::new(DeviceId([1u8; 32])));
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let server = AuthPipeServer::new(auth, temp_dir.path().to_path_buf());

        let resp = server.handle_request(IpcRequest::QueryStatus);
        assert_eq!(
            resp,
            IpcResponse::Status {
                enrolled: false,
                phone_connected: false,
                phone_name: None,
                locked_out: false,
            }
        );
    }
}
