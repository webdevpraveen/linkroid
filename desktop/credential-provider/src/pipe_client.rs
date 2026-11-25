// SPDX-License-Identifier: GPL-3.0-or-later
//! IPC client connecting the Credential Provider DLL to the Linkroid Windows Service.

#![allow(unsafe_code)]

use linkroid_protocol::auth::ipc::{IpcRequest, IpcResponse};

#[cfg(windows)]
pub fn send_request(req: &IpcRequest) -> Result<IpcResponse, String> {
    use linkroid_protocol::auth::ipc::{
        AUTH_PIPE_NAME, decode_ipc_message, encode_ipc_message,
    };
    use windows::{
        core::{HSTRING, PCWSTR},
        Win32::{
            Foundation::CloseHandle,
            Storage::FileSystem::{
                CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
                FILE_SHARE_NONE, OPEN_EXISTING, ReadFile, WriteFile,
            },
        },
    };

    let pipe_h = HSTRING::from(AUTH_PIPE_NAME);
    let handle = unsafe {
        CreateFileW(
            PCWSTR(pipe_h.as_ptr()),
            FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(|e| format!("Service pipe not available: {e}"))?;

    let encoded = encode_ipc_message(req).map_err(|e| e.to_string())?;
    let mut written = 0u32;
    unsafe {
        WriteFile(
            handle,
            Some(&encoded),
            Some(&mut written),
            None,
        )
    }
    .map_err(|e| format!("Failed to write to service pipe: {e}"))?;

    let mut buf = vec![0u8; 65536];
    let mut read_bytes = 0u32;
    unsafe {
        ReadFile(
            handle,
            Some(&mut buf),
            Some(&mut read_bytes),
            None,
        )
    }
    .map_err(|e| format!("Failed to read from service pipe: {e}"))?;

    let _ = unsafe { CloseHandle(handle) };

    buf.truncate(read_bytes as usize);
    decode_ipc_message(&buf).map_err(|e| e.to_string())
}

#[cfg(not(windows))]
pub fn send_request(_req: &IpcRequest) -> Result<IpcResponse, String> {
    Err("Named Pipe is only supported on Windows".into())
}
