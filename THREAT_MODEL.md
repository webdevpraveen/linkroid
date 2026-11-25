<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
# Linkroid Phone Biometric Unlock: Security Architecture & Threat Model

This document outlines the security architecture, design guarantees, and threat analysis for the phone biometric remote unlock and workstation lock subsystem in Linkroid.

---

## 1. System Architecture

```
  +-----------------------------------------------------------+
  |                        Windows PC                         |
  |                                                           |
  |  [ LogonUI.exe ]                                          |
  |         |                                                 |
  |  [ Credential Provider DLL ]                              |
  |    - Thin native COM DLL                                  |
  |    - No network, no crypto                                |
  |    - Fail-closed (NO_DEFAULT)                             |
  |         |                                                 |
  |         | Length-prefixed IPC over Named Pipe             |
  |         | (\\.\pipe\LinkroidAuthPipe, strict DACL)         |
  |         v                                                 |
  |  [ Linkroid SYSTEM Service ]                              |
  |    - Session 0 isolation                                  |
  |    - Owns pairing state & mTLS transport                  |
  |    - DPAPI machine-scope encrypted credential store       |
  |    - Rate limiting & replay protection                    |
  |    - Memory zeroization on drop                           |
  +-----------------------------------------------------------+
                                ^
                                | Mutual TLS (mTLS) with pinned keys
                                | (Wi-Fi or USB cable fallback)
                                v
  +-----------------------------------------------------------+
  |                      Android Phone                        |
  |                                                           |
  |  [ Linkroid Foreground Service / App ]                    |
  |    - Receives challenge via mTLS                          |
  |    - Raises high-priority notification / prompt           |
  |    - Never stores or receives Windows password            |
  |         |                                                 |
  |  [ Hardware-Backed Android Keystore ]                     |
  |    - EC P-256 key pair per paired PC                      |
  |    - StrongBox where available (TEE fallback)             |
  |    - setUserAuthenticationRequired(true)                  |
  |    - setInvalidatedByBiometricEnrollment(true)            |
  |         |                                                 |
  |  [ BiometricPrompt with CryptoObject ]                    |
  |    - Fingerprint unlocks Signature instance               |
  |    - Signs canonical domain-separated challenge digest    |
  +-----------------------------------------------------------+
```

---

## 2. Threat Analysis & Mitigations

### 2.1 Stolen Phone
- **Threat**: An unauthorized person gains physical possession of the paired Android phone and attempts to unlock the user's PC.
- **Mitigation**:
  - The Windows password is **never sent to or stored on the phone**.
  - The Keystore private key is protected with `setUserAuthenticationRequired(true)` and wrapped in a `BiometricPrompt.CryptoObject`. The private key cannot be accessed or used without a successful live biometric scan (fingerprint).
  - Phone screen lock or PIN bypass does not grant access to the cryptographic key without biometric authorization.

### 2.2 Stolen PC / Disk Extraction
- **Threat**: An adversary steals the PC, boots from an external drive, or extracts the SSD to access stored credentials.
- **Mitigation**:
  - Windows passwords are stored encrypted using DPAPI machine scope (`CryptProtectData`), accessible only to `NT AUTHORITY\SYSTEM` on the local machine.
  - Credential records in memory implement volatile zeroization (`zeroize`) upon drop.
  - Phone unlock at cold-boot logon is disabled by default; the user must log in once with their primary Windows credentials after boot before remote unlock can proceed.

### 2.3 Malicious Device on the Same Wi-Fi (Hostel / College Networks)
- **Threat**: An attacker on the same local network (common in shared apartments, college campuses, and cafes) sniffs traffic or attempts to inject spoofed unlock requests.
- **Mitigation**:
  - All communication uses mutual TLS (mTLS) with pinned certificates exchanged during initial SAS/QR code pairing.
  - Unauthenticated network packets are dropped at the transport layer.
  - USB cable connection (ADB port forwarding) is available as a fallback when Wi-Fi client isolation is active.

### 2.4 Man-in-the-Middle (MITM) & Relay Attacks
- **Threat**: An attacker captures a challenge from PC A and replays or relays it to phone B, or tricks the user into unlocking the wrong computer.
- **Mitigation**:
  - The signed digest covers `canonical_unlock_digest(nonce || session_id || pc_id || timestamp)`.
  - Because `pc_id` is embedded in the cryptographic signature, a signature generated for PC A is invalid for PC B.
  - The prompt displayed on the phone explicitly names the target PC (`Unlock {pc_name}`).

### 2.5 Replay Attacks & Nonce Freshness
- **Threat**: An eavesdropper records a valid `UnlockResponse` and replays it at a later time.
- **Mitigation**:
  - Every challenge uses a 32-byte cryptographically secure random nonce (`rand::random()`).
  - Active challenges expire in 30 seconds (`CHALLENGE_TTL_SECS = 30`).
  - Challenges are strictly single-use: the service deletes the active challenge from memory the moment a response is checked.
  - Clocks outside the skew window (`CLOCK_SKEW_TOLERANCE_SECS = 5`) are denied.

### 2.6 New Fingerprint Enrolled on Phone
- **Threat**: A third party who knows the phone PIN enrolls their own fingerprint in Android Settings to bypass biometric security.
- **Mitigation**:
  - Keystore keys are generated with `setInvalidatedByBiometricEnrollment(true)`.
  - Android OS permanently destroys/invalidates the hardware key when any new biometric is enrolled.
  - Subsequent signature attempts throw `KeyPermanentlyInvalidatedException`, immediately revoking authentication until re-paired by the authorized user.

### 2.7 Malware on the PC (User Session vs Session 0 Isolation)
- **Threat**: A low-privilege process running in the user's desktop session tries to tamper with the Credential Provider or steal the Windows password.
- **Mitigation**:
  - The Credential Provider DLL is a thin COM object loaded by `LogonUI.exe` that contains no network code, no cryptographic keys, and no persistent storage.
  - The service runs isolated in Session 0 as `NT AUTHORITY\SYSTEM`.
  - The Named Pipe IPC enforces a strict DACL: only SYSTEM, Administrators, and Authenticated Users (LogonUI) can access the pipe. Remote network clients are rejected via `PIPE_REJECT_REMOTE_CLIENTS`.
  - Workstation lock operations are safely dispatched to the interactive console session.

### 2.8 Rate Limiting & Brute Force Protection
- **Threat**: An attacker repeatedly triggers unlock attempts to guess credentials or exhaust system resources.
- **Mitigation**:
  - 5 consecutive failed attempts trigger an automatic 5-minute lockout period for the device.
  - The lockout period doubles exponentially on repeated failures.

### 2.9 Fail-Closed Guarantee
- **Guarantee**:
  - The Credential Provider tile never overrides or displaces standard Windows authentication (`*pdwdefault = CREDENTIAL_PROVIDER_NO_DEFAULT`).
  - Normal password, Windows Hello PIN, and smart card logons remain fully functional.
  - Safe Mode and standard uninstaller scripts allow instantaneous clean unregistration without system lockout.
