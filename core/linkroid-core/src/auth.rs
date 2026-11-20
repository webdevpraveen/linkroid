// SPDX-License-Identifier: MPL-2.0
//! Biometric authentication & phone-unlock manager (Phase 1).
//!
//! Enforces non-negotiable security requirements:
//! - Single-use nonces and 30-second expiry (replay protection).
//! - Binding to the PC's `DeviceId` (relay protection).
//! - Hardware-backed EC P-256 ECDSA signature verification via `ring`.
//! - Rate limiting: 5 consecutive failures triggers a 5-minute lockout, doubling on repeated triggers.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use linkroid_protocol::{
    DeviceId,
    auth::{
        CHALLENGE_TTL_SECS, SignedLockRequest, UnlockChallenge, UnlockResponse,
        canonical_lock_digest, canonical_unlock_digest,
    },
};
use ring::signature::{self, UnparsedPublicKey};
use thiserror::Error;

/// Maximum consecutive verification failures before locking out the phone.
pub const MAX_FAILED_ATTEMPTS: u32 = 5;
/// Initial lockout duration after 5 failed attempts (5 minutes).
pub const BASE_LOCKOUT_SECS: u64 = 300;
/// Maximum lockout duration under exponential backoff (24 hours).
pub const MAX_LOCKOUT_SECS: u64 = 86400;
/// Allowed clock skew for timestamp validation (5 seconds).
pub const CLOCK_SKEW_TOLERANCE_SECS: u64 = 5;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("phone {0} is not enrolled for biometric unlock")]
    NotEnrolled(DeviceId),
    #[error("phone {0} is locked out for {1}s due to too many failed attempts")]
    RateLimited(DeviceId, u64),
    #[error("unlock challenge not found or already consumed")]
    InvalidSession,
    #[error("challenge has expired")]
    Expired,
    #[error("challenge belongs to phone {expected}, but response was received from {actual}")]
    DeviceMismatch { expected: DeviceId, actual: DeviceId },
    #[error("PC ID mismatch: expected {expected}, challenge targeted {actual}")]
    PcIdMismatch { expected: DeviceId, actual: DeviceId },
    #[error("timestamp is outside valid window")]
    InvalidTimestamp,
    #[error("cryptographic signature verification failed")]
    InvalidSignature,
    #[error("missing required signature for lock request")]
    MissingSignature,
    #[error("internal cryptographic error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone)]
struct ActiveChallenge {
    phone_id: DeviceId,
    nonce: Vec<u8>,
    session_id: String,
    created_at: u64,
    expiry: u64,
}

#[derive(Debug, Clone)]
struct RateLimitTracker {
    failed_attempts: u32,
    lockout_until: Option<u64>,
    current_lockout_secs: u64,
}

impl Default for RateLimitTracker {
    fn default() -> Self {
        Self {
            failed_attempts: 0,
            lockout_until: None,
            current_lockout_secs: BASE_LOCKOUT_SECS,
        }
    }
}

/// Core authentication manager handling challenges, signature verification, and rate limiting.
#[derive(Debug)]
pub struct AuthManager {
    local_pc_id: DeviceId,
    enrolled_phones: Mutex<HashMap<DeviceId, Vec<u8>>>,
    challenges: Mutex<HashMap<String, ActiveChallenge>>,
    rate_limits: Mutex<HashMap<DeviceId, RateLimitTracker>>,
}

impl AuthManager {
    /// Creates a new `AuthManager` for this PC's `DeviceId`.
    pub fn new(local_pc_id: DeviceId) -> Self {
        Self {
            local_pc_id,
            enrolled_phones: Mutex::new(HashMap::new()),
            challenges: Mutex::new(HashMap::new()),
            rate_limits: Mutex::new(HashMap::new()),
        }
    }

    /// Enrolls or updates an Android phone's biometric EC P-256 public key (SEC1 uncompressed).
    pub fn enroll_phone(&self, phone_id: DeviceId, public_key_sec1: Vec<u8>) {
        let mut phones = self.enrolled_phones.lock().unwrap_or_else(|e| e.into_inner());
        phones.insert(phone_id, public_key_sec1);
    }

    /// Removes an enrolled phone's public key and clears its active sessions.
    pub fn unenroll_phone(&self, phone_id: &DeviceId) -> bool {
        let mut phones = self.enrolled_phones.lock().unwrap_or_else(|e| e.into_inner());
        let removed = phones.remove(phone_id).is_some();
        if removed {
            let mut challenges = self.challenges.lock().unwrap_or_else(|e| e.into_inner());
            challenges.retain(|_, c| c.phone_id != *phone_id);
            let mut rate_limits = self.rate_limits.lock().unwrap_or_else(|e| e.into_inner());
            rate_limits.remove(phone_id);
        }
        removed
    }

    /// Returns whether the specified phone is currently enrolled.
    pub fn is_enrolled(&self, phone_id: &DeviceId) -> bool {
        let phones = self.enrolled_phones.lock().unwrap_or_else(|e| e.into_inner());
        phones.contains_key(phone_id)
    }

    /// Checks if a phone is currently locked out by rate limiting.
    pub fn check_rate_limit(&self, phone_id: &DeviceId, now: u64) -> Result<(), AuthError> {
        let rate_limits = self.rate_limits.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(lockout_until) = rate_limits
            .get(phone_id)
            .and_then(|t| t.lockout_until)
            .filter(|&until| now < until)
        {
            return Err(AuthError::RateLimited(*phone_id, lockout_until - now));
        }
        Ok(())
    }

    /// Generates a fresh single-use 32-byte challenge for the enrolled phone.
    pub fn create_unlock_challenge(&self, phone_id: DeviceId) -> Result<UnlockChallenge, AuthError> {
        if !self.is_enrolled(&phone_id) {
            return Err(AuthError::NotEnrolled(phone_id));
        }

        let now = now_secs();
        self.check_rate_limit(&phone_id, now)?;

        let nonce_bytes: [u8; 32] = rand::random();
        let nonce = nonce_bytes.to_vec();

        let session_bytes: [u8; 16] = rand::random();
        let session_id = data_encoding::HEXLOWER.encode(&session_bytes);

        let expiry = now + CHALLENGE_TTL_SECS;

        let challenge = UnlockChallenge {
            nonce: nonce.clone(),
            session_id: session_id.clone(),
            pc_id: self.local_pc_id,
            expiry,
        };

        let mut challenges = self.challenges.lock().unwrap_or_else(|e| e.into_inner());
        // Clean up expired challenges
        challenges.retain(|_, c| c.expiry > now);

        challenges.insert(
            session_id.clone(),
            ActiveChallenge {
                phone_id,
                nonce,
                session_id,
                created_at: now,
                expiry,
            },
        );

        Ok(challenge)
    }

    /// Verifies the phone's biometric response against the active challenge and enrolled public key.
    pub fn verify_unlock_response(
        &self,
        phone_id: &DeviceId,
        response: &UnlockResponse,
    ) -> Result<(), AuthError> {
        let now = now_secs();
        self.check_rate_limit(phone_id, now)?;

        let enrolled_pubkey = {
            let phones = self.enrolled_phones.lock().unwrap_or_else(|e| e.into_inner());
            phones.get(phone_id).cloned().ok_or(AuthError::NotEnrolled(*phone_id))?
        };

        // CRITICAL: remove challenge immediately to enforce single-use nonce
        let challenge = {
            let mut challenges = self.challenges.lock().unwrap_or_else(|e| e.into_inner());
            challenges.remove(&response.session_id)
        }
        .ok_or(AuthError::InvalidSession)?;

        if challenge.phone_id != *phone_id {
            self.record_failure(phone_id, now);
            return Err(AuthError::DeviceMismatch { expected: challenge.phone_id, actual: *phone_id });
        }

        if now > challenge.expiry {
            self.record_failure(phone_id, now);
            return Err(AuthError::Expired);
        }

        if response.timestamp > now + CLOCK_SKEW_TOLERANCE_SECS
            || response.timestamp < challenge.created_at.saturating_sub(CLOCK_SKEW_TOLERANCE_SECS)
        {
            self.record_failure(phone_id, now);
            return Err(AuthError::InvalidTimestamp);
        }

        let digest = canonical_unlock_digest(
            &challenge.nonce,
            &challenge.session_id,
            &self.local_pc_id,
            response.timestamp,
        );

        let public_key = UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_ASN1, &enrolled_pubkey);
        if public_key.verify(&digest, &response.signature).is_err() {
            self.record_failure(phone_id, now);
            return Err(AuthError::InvalidSignature);
        }

        // On success, reset rate-limiting failures
        self.record_success(phone_id);
        Ok(())
    }

    /// Verifies an authenticated lock request from a paired phone.
    pub fn verify_lock_request(
        &self,
        phone_id: &DeviceId,
        request: &SignedLockRequest,
    ) -> Result<(), AuthError> {
        let now = now_secs();

        if request.pc_id != self.local_pc_id {
            return Err(AuthError::PcIdMismatch { expected: self.local_pc_id, actual: request.pc_id });
        }

        if request.timestamp > now + CLOCK_SKEW_TOLERANCE_SECS
            || request.timestamp < now.saturating_sub(CHALLENGE_TTL_SECS)
        {
            return Err(AuthError::InvalidTimestamp);
        }

        if let Some(signature_bytes) = &request.signature {
            let enrolled_pubkey = {
                let phones = self.enrolled_phones.lock().unwrap_or_else(|e| e.into_inner());
                phones.get(phone_id).cloned().ok_or(AuthError::NotEnrolled(*phone_id))?
            };

            let digest = canonical_lock_digest(&request.nonce, &self.local_pc_id, request.timestamp);
            let public_key =
                UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_ASN1, &enrolled_pubkey);
            if public_key.verify(&digest, signature_bytes).is_err() {
                self.record_failure(phone_id, now);
                return Err(AuthError::InvalidSignature);
            }
        }

        Ok(())
    }

    fn record_failure(&self, phone_id: &DeviceId, now: u64) {
        let mut rate_limits = self.rate_limits.lock().unwrap_or_else(|e| e.into_inner());
        let tracker = rate_limits.entry(*phone_id).or_default();
        tracker.failed_attempts += 1;
        if tracker.failed_attempts >= MAX_FAILED_ATTEMPTS {
            tracker.lockout_until = Some(now + tracker.current_lockout_secs);
            tracker.current_lockout_secs =
                (tracker.current_lockout_secs * 2).min(MAX_LOCKOUT_SECS);
        }
    }

    fn record_success(&self, phone_id: &DeviceId) {
        let mut rate_limits = self.rate_limits.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tracker) = rate_limits.get_mut(phone_id) {
            tracker.failed_attempts = 0;
            tracker.lockout_until = None;
            tracker.current_lockout_secs = BASE_LOCKOUT_SECS;
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{EcdsaKeyPair, KeyPair};

    fn generate_test_p256_key() -> (Vec<u8>, EcdsaKeyPair) {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&signature::ECDSA_P256_SHA256_ASN1_SIGNING, &rng)
            .expect("generate pkcs8");
        let key_pair = EcdsaKeyPair::from_pkcs8(
            &signature::ECDSA_P256_SHA256_ASN1_SIGNING,
            pkcs8.as_ref(),
            &rng,
        )
        .expect("keypair from pkcs8");
        let pub_sec1 = key_pair.public_key().as_ref().to_vec();
        (pub_sec1, key_pair)
    }

    #[test]
    fn test_valid_unlock_flow() {
        let pc_id = DeviceId([1u8; 32]);
        let phone_id = DeviceId([2u8; 32]);
        let auth = AuthManager::new(pc_id);

        let (pubkey, keypair) = generate_test_p256_key();
        auth.enroll_phone(phone_id, pubkey);

        let challenge = auth.create_unlock_challenge(phone_id).expect("create challenge");
        assert_eq!(challenge.pc_id, pc_id);
        assert_eq!(challenge.nonce.len(), 32);

        let timestamp = now_secs();
        let digest = canonical_unlock_digest(
            &challenge.nonce,
            &challenge.session_id,
            &pc_id,
            timestamp,
        );

        let rng = SystemRandom::new();
        let sig = keypair.sign(&rng, &digest).expect("sign");

        let response = UnlockResponse {
            session_id: challenge.session_id,
            timestamp,
            signature: sig.as_ref().to_vec(),
        };

        assert!(auth.verify_unlock_response(&phone_id, &response).is_ok());
    }

    #[test]
    fn test_replay_attack_rejected() {
        let pc_id = DeviceId([1u8; 32]);
        let phone_id = DeviceId([2u8; 32]);
        let auth = AuthManager::new(pc_id);

        let (pubkey, keypair) = generate_test_p256_key();
        auth.enroll_phone(phone_id, pubkey);

        let challenge = auth.create_unlock_challenge(phone_id).unwrap();
        let timestamp = now_secs();
        let digest = canonical_unlock_digest(
            &challenge.nonce,
            &challenge.session_id,
            &pc_id,
            timestamp,
        );
        let rng = SystemRandom::new();
        let sig = keypair.sign(&rng, &digest).unwrap();

        let response = UnlockResponse {
            session_id: challenge.session_id,
            timestamp,
            signature: sig.as_ref().to_vec(),
        };

        // First verification succeeds
        assert!(auth.verify_unlock_response(&phone_id, &response).is_ok());

        // Replay of the exact same response must fail with InvalidSession
        assert_eq!(auth.verify_unlock_response(&phone_id, &response), Err(AuthError::InvalidSession));
    }

    #[test]
    fn test_expired_challenge_rejected() {
        let pc_id = DeviceId([1u8; 32]);
        let phone_id = DeviceId([2u8; 32]);
        let auth = AuthManager::new(pc_id);

        let (pubkey, keypair) = generate_test_p256_key();
        auth.enroll_phone(phone_id, pubkey);

        let challenge = auth.create_unlock_challenge(phone_id).unwrap();
        // Manipulate challenge expiry to the past
        {
            let mut challenges = auth.challenges.lock().unwrap();
            challenges.get_mut(&challenge.session_id).unwrap().expiry = now_secs() - 1;
        }

        let timestamp = now_secs();
        let digest = canonical_unlock_digest(
            &challenge.nonce,
            &challenge.session_id,
            &pc_id,
            timestamp,
        );
        let rng = SystemRandom::new();
        let sig = keypair.sign(&rng, &digest).unwrap();

        let response = UnlockResponse {
            session_id: challenge.session_id,
            timestamp,
            signature: sig.as_ref().to_vec(),
        };

        assert_eq!(auth.verify_unlock_response(&phone_id, &response), Err(AuthError::Expired));
    }

    #[test]
    fn test_wrong_key_rejected() {
        let pc_id = DeviceId([1u8; 32]);
        let phone_id = DeviceId([2u8; 32]);
        let auth = AuthManager::new(pc_id);

        let (pubkey, _keypair) = generate_test_p256_key();
        let (_wrong_pub, wrong_keypair) = generate_test_p256_key();

        auth.enroll_phone(phone_id, pubkey);

        let challenge = auth.create_unlock_challenge(phone_id).unwrap();
        let timestamp = now_secs();
        let digest = canonical_unlock_digest(
            &challenge.nonce,
            &challenge.session_id,
            &pc_id,
            timestamp,
        );
        let rng = SystemRandom::new();
        let wrong_sig = wrong_keypair.sign(&rng, &digest).unwrap();

        let response = UnlockResponse {
            session_id: challenge.session_id,
            timestamp,
            signature: wrong_sig.as_ref().to_vec(),
        };

        assert_eq!(auth.verify_unlock_response(&phone_id, &response), Err(AuthError::InvalidSignature));
    }

    #[test]
    fn test_wrong_pc_id_rejected() {
        let pc_id = DeviceId([1u8; 32]);
        let other_pc_id = DeviceId([99u8; 32]);
        let phone_id = DeviceId([2u8; 32]);
        let auth = AuthManager::new(pc_id);

        let (pubkey, keypair) = generate_test_p256_key();
        auth.enroll_phone(phone_id, pubkey);

        let challenge = auth.create_unlock_challenge(phone_id).unwrap();
        let timestamp = now_secs();
        // Attacker attempts relay by having phone sign for other_pc_id
        let digest = canonical_unlock_digest(
            &challenge.nonce,
            &challenge.session_id,
            &other_pc_id,
            timestamp,
        );
        let rng = SystemRandom::new();
        let sig = keypair.sign(&rng, &digest).unwrap();

        let response = UnlockResponse {
            session_id: challenge.session_id,
            timestamp,
            signature: sig.as_ref().to_vec(),
        };

        assert_eq!(auth.verify_unlock_response(&phone_id, &response), Err(AuthError::InvalidSignature));
    }

    #[test]
    fn test_rate_limiting_lockout() {
        let pc_id = DeviceId([1u8; 32]);
        let phone_id = DeviceId([2u8; 32]);
        let auth = AuthManager::new(pc_id);

        let (pubkey, _keypair) = generate_test_p256_key();
        let (_, wrong_keypair) = generate_test_p256_key();
        auth.enroll_phone(phone_id, pubkey);

        let rng = SystemRandom::new();
        // Fail 4 times
        for _ in 0..4 {
            let challenge = auth.create_unlock_challenge(phone_id).unwrap();
            let timestamp = now_secs();
            let digest = canonical_unlock_digest(&challenge.nonce, &challenge.session_id, &pc_id, timestamp);
            let sig = wrong_keypair.sign(&rng, &digest).unwrap();
            let resp = UnlockResponse {
                session_id: challenge.session_id,
                timestamp,
                signature: sig.as_ref().to_vec(),
            };
            assert_eq!(auth.verify_unlock_response(&phone_id, &resp), Err(AuthError::InvalidSignature));
        }

        // 5th failure triggers lockout
        let challenge = auth.create_unlock_challenge(phone_id).unwrap();
        let timestamp = now_secs();
        let digest = canonical_unlock_digest(&challenge.nonce, &challenge.session_id, &pc_id, timestamp);
        let sig = wrong_keypair.sign(&rng, &digest).unwrap();
        let resp = UnlockResponse {
            session_id: challenge.session_id,
            timestamp,
            signature: sig.as_ref().to_vec(),
        };
        assert_eq!(auth.verify_unlock_response(&phone_id, &resp), Err(AuthError::InvalidSignature));

        // Now attempts are blocked by RateLimited
        match auth.create_unlock_challenge(phone_id) {
            Err(AuthError::RateLimited(dev, secs)) => {
                assert_eq!(dev, phone_id);
                assert!(secs > 0 && secs <= 300);
            }
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }

    #[test]
    fn test_signed_lock_command() {
        let pc_id = DeviceId([1u8; 32]);
        let phone_id = DeviceId([2u8; 32]);
        let auth = AuthManager::new(pc_id);

        let (pubkey, keypair) = generate_test_p256_key();
        auth.enroll_phone(phone_id, pubkey);

        let timestamp = now_secs();
        let nonce = vec![88u8; 32];
        let digest = canonical_lock_digest(&nonce, &pc_id, timestamp);
        let rng = SystemRandom::new();
        let sig = keypair.sign(&rng, &digest).unwrap();

        let req = SignedLockRequest {
            nonce,
            pc_id,
            timestamp,
            signature: Some(sig.as_ref().to_vec()),
        };

        assert!(auth.verify_lock_request(&phone_id, &req).is_ok());

        // Lock with wrong pc_id must fail
        let mut bad_pc = req.clone();
        bad_pc.pc_id = DeviceId([99u8; 32]);
        assert!(matches!(auth.verify_lock_request(&phone_id, &bad_pc), Err(AuthError::PcIdMismatch { .. })));
    }
}
