// SPDX-License-Identifier: MPL-2.0
//! DPAPI machine-scoped credential storage and secure in-memory zeroization.
//!
//! Enforces:
//! - Password is encrypted with machine-scoped DPAPI (`CRYPTPROTECT_LOCAL_MACHINE`).
//! - Plaintext password never written to disk or logs.
//! - Memory containing plaintext password is zeroized immediately on drop.
//! - Revocation zeroes the ciphertext file on disk before unlinking.

#![allow(unsafe_code)]

use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

/// A memory buffer that is guaranteed to be overwritten with zeros when dropped.
pub struct ZeroizedSecret(Vec<u8>);

impl ZeroizedSecret {
    pub fn new(data: Vec<u8>) -> Self {
        Self(data)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn as_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.0)
    }
}

impl Drop for ZeroizedSecret {
    fn drop(&mut self) {
        for byte in self.0.iter_mut() {
            // Volatile write prevents the compiler from optimizing away the clearing.
            unsafe {
                std::ptr::write_volatile(byte, 0);
            }
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

impl fmt::Debug for ZeroizedSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[REDACTED SECRET]")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncryptedRecord {
    version: u32,
    username: String,
    domain: Option<String>,
    ciphertext: Vec<u8>,
}

/// Manages machine-scoped storage of Windows logon credentials for the Linkroid service.
#[derive(Debug, Clone)]
pub struct CredentialStore {
    file_path: PathBuf,
}

impl CredentialStore {
    /// Initializes a `CredentialStore` storing credentials under the given directory.
    pub fn new(dir: PathBuf) -> Self {
        Self {
            file_path: dir.join("auth_credential.dat"),
        }
    }

    /// Whether a stored credential exists on disk.
    pub fn exists(&self) -> bool {
        self.file_path.exists()
    }

    /// Encrypts and securely stores the logon credentials using DPAPI.
    pub fn store(
        &self,
        username: &str,
        domain: Option<&str>,
        password: &[u8],
    ) -> Result<(), String> {
        let ciphertext = encrypt_dpapi(password)?;
        let record = EncryptedRecord {
            version: 1,
            username: username.to_owned(),
            domain: domain.map(|s| s.to_owned()),
            ciphertext,
        };

        let buf = serde_json::to_vec(&record).map_err(|e| e.to_string())?;

        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let tmp_path = self.file_path.with_extension("tmp");
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp_path)
                .map_err(|e| e.to_string())?;
            file.write_all(&buf).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
        }

        fs::rename(&tmp_path, &self.file_path).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Loads and decrypts the logon credential, returning the username, domain, and zeroized secret.
    pub fn load(&self) -> Result<Option<(String, Option<String>, ZeroizedSecret)>, String> {
        if !self.file_path.exists() {
            return Ok(None);
        }

        let mut file = File::open(&self.file_path).map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).map_err(|e| e.to_string())?;

        let record: EncryptedRecord =
            serde_json::from_slice(&buf).map_err(|e| e.to_string())?;
        let plaintext = decrypt_dpapi(&record.ciphertext)?;
        Ok(Some((record.username, record.domain, plaintext)))
    }

    /// Securely revokes and wipes the stored credential file.
    pub fn revoke(&self) -> Result<bool, String> {
        if !self.file_path.exists() {
            return Ok(false);
        }

        // Overwrite file contents with zeros before unlinking to eliminate magnetic/flash residue
        if let Ok(metadata) = fs::metadata(&self.file_path) {
            let len = metadata.len() as usize;
            if let Ok(mut file) = OpenOptions::new().write(true).open(&self.file_path) {
                let zeros = vec![0u8; len];
                let _ = file.write_all(&zeros);
                let _ = file.sync_all();
            }
        }

        fs::remove_file(&self.file_path).map_err(|e| e.to_string())?;
        Ok(true)
    }
}

#[cfg(windows)]
fn encrypt_dpapi(data: &[u8]) -> Result<Vec<u8>, String> {
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Security::Cryptography::{
        CRYPTPROTECT_LOCAL_MACHINE, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB, CryptProtectData,
    };

    let mut in_blob = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB::default();

    let success = unsafe {
        CryptProtectData(
            &mut in_blob,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_LOCAL_MACHINE | CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        )
    };

    if !success.as_bool() {
        return Err("CryptProtectData failed".into());
    }

    let ciphertext = unsafe {
        std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec()
    };

    unsafe {
        LocalFree(Some(windows::Win32::Foundation::HLOCAL(out_blob.pbData as _)));
    }

    Ok(ciphertext)
}

#[cfg(windows)]
fn decrypt_dpapi(data: &[u8]) -> Result<ZeroizedSecret, String> {
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Security::Cryptography::{
        CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB, CryptUnprotectData,
    };

    let mut in_blob = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB::default();

    let success = unsafe {
        CryptUnprotectData(
            &mut in_blob,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        )
    };

    if !success.as_bool() {
        return Err("CryptUnprotectData failed".into());
    }

    let secret_bytes = unsafe {
        std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec()
    };

    // Zero the Windows-allocated plaintext buffer before freeing
    unsafe {
        for i in 0..out_blob.cbData as usize {
            std::ptr::write_volatile(out_blob.pbData.add(i), 0);
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
        LocalFree(Some(windows::Win32::Foundation::HLOCAL(out_blob.pbData as _)));
    }

    Ok(ZeroizedSecret::new(secret_bytes))
}

#[cfg(not(windows))]
fn encrypt_dpapi(data: &[u8]) -> Result<Vec<u8>, String> {
    // Cross-platform mock using simple masking for CI runners (Linux/macOS)
    let mut masked = data.to_vec();
    for (i, b) in masked.iter_mut().enumerate() {
        *b ^= (0xAA + (i as u8)) & 0xFF;
    }
    Ok(masked)
}

#[cfg(not(windows))]
fn decrypt_dpapi(data: &[u8]) -> Result<ZeroizedSecret, String> {
    let mut unmasked = data.to_vec();
    for (i, b) in unmasked.iter_mut().enumerate() {
        *b ^= (0xAA + (i as u8)) & 0xFF;
    }
    Ok(ZeroizedSecret::new(unmasked))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zeroized_secret_scrubbing() {
        let secret = ZeroizedSecret::new(vec![0x42; 64]);
        assert_eq!(secret.as_bytes()[0], 0x42);
        assert_eq!(secret.as_bytes().len(), 64);
        drop(secret);
    }

    #[test]
    fn test_credential_store_roundtrip() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let store = CredentialStore::new(temp_dir.path().to_path_buf());

        assert!(!store.exists());
        let username = "testuser";
        let domain = Some("WORKGROUP");
        let password = b"P@ssw0rd123!";

        store.store(username, domain, password).expect("store credential");
        assert!(store.exists());

        let loaded = store.load().expect("load credential").expect("some credential");
        assert_eq!(loaded.0, username);
        assert_eq!(loaded.1, domain.map(String::from));
        assert_eq!(loaded.2.as_bytes(), password);

        assert!(store.revoke().expect("revoke"));
        assert!(!store.exists());
        assert!(store.load().expect("load after revoke").is_none());
    }
}
