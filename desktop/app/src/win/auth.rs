// SPDX-License-Identifier: GPL-3.0-or-later
//! Windows Credential Provider IPC client for desktop settings and enrollment.

#![allow(unsafe_code)]

use std::{
    fs::OpenOptions,
    io::{Read, Write},
};

use linkroid_core::linkroid_protocol::auth::ipc::{
    AUTH_PIPE_NAME, IpcRequest, IpcResponse, decode_ipc_message, encode_ipc_message,
};

pub fn send_ipc_request(req: &IpcRequest) -> Result<IpcResponse, String> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(AUTH_PIPE_NAME)
        .map_err(|e| format!("Could not connect to Linkroid Auth Service pipe: {e}"))?;

    let payload = encode_ipc_message(req)?;
    file.write_all(&payload)
        .map_err(|e| format!("Failed to write to pipe: {e}"))?;
    file.flush()
        .map_err(|e| format!("Failed to flush pipe: {e}"))?;

    let mut len_buf = [0u8; 4];
    file.read_exact(&mut len_buf)
        .map_err(|e| format!("Failed to read response length: {e}"))?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > 65536 {
        return Err("Response message too large".into());
    }

    let mut body = vec![0u8; len];
    file.read_exact(&mut body)
        .map_err(|e| format!("Failed to read response body: {e}"))?;

    let mut full = Vec::with_capacity(4 + len);
    full.extend_from_slice(&len_buf);
    full.extend_from_slice(&body);

    decode_ipc_message::<IpcResponse>(&full)
}

pub fn query_status() -> Result<(bool, bool, Option<String>, bool), String> {
    match send_ipc_request(&IpcRequest::QueryStatus)? {
        IpcResponse::Status {
            enrolled,
            phone_connected,
            phone_name,
            locked_out,
        } => Ok((enrolled, phone_connected, phone_name, locked_out)),
        IpcResponse::Error { message, .. } => Err(message),
        _ => Err("Unexpected response".into()),
    }
}

pub fn enroll_credential(password: &str) -> Result<(), String> {
    let username = std::env::var("USERNAME").unwrap_or_else(|_| "User".into());
    let domain = std::env::var("USERDOMAIN").ok();
    let encrypted_secret = password.as_bytes().to_vec();

    match send_ipc_request(&IpcRequest::EnrollCredential {
        username,
        domain,
        encrypted_secret,
    })? {
        IpcResponse::CredentialEnrolled { success } if success => Ok(()),
        IpcResponse::Error { message, .. } => Err(message),
        _ => Err("Enrollment failed".into()),
    }
}

pub fn revoke_credential() -> Result<bool, String> {
    match send_ipc_request(&IpcRequest::RevokeCredential)? {
        IpcResponse::CredentialRevoked { success } => Ok(success),
        IpcResponse::Error { message, .. } => Err(message),
        _ => Err("Revocation failed".into()),
    }
}

pub fn lock_workstation() -> Result<(), String> {
    linkroid_core::auth::service::lock_workstation()
}
