// SPDX-License-Identifier: GPL-3.0-or-later
package com.webdevpraveen.linkroid.auth

import android.Manifest
import android.app.Activity
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.hardware.biometrics.BiometricPrompt
import android.os.Build
import android.os.Bundle
import android.os.CancellationSignal
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import com.webdevpraveen.linkroid.LinkroidApplication
import com.webdevpraveen.linkroid.R
import com.webdevpraveen.linkroid.core.authLockDigest
import com.webdevpraveen.linkroid.core.authUnlockDigest
import java.security.KeyPair
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.Signature
import java.security.interfaces.ECPublicKey
import java.security.spec.ECGenParameterSpec

/**
 * Manages hardware-backed Android Keystore EC P-256 keys and biometric authentication
 * for remote Windows login and workstation lock.
 */
object BiometricAuthManager {
    private const val TAG = "BiometricAuth"
    private const val KEYSTORE = "AndroidKeyStore"
    private const val ALIAS_PREFIX = "linkroid.auth.pc."

    fun aliasFor(pcId: String): String = ALIAS_PREFIX + pcId.replace(Regex("[^a-zA-Z0-9._-]"), "_")

    /**
     * Retrieves or generates a hardware-backed EC P-256 key pair bound to user biometric enrollment.
     * StrongBox is used where available; any new fingerprint enrolled permanently invalidates the key.
     */
    fun getOrCreateKeyPair(context: Context, pcId: String): KeyPair = synchronized(this) {
        val alias = aliasFor(pcId)
        val store = KeyStore.getInstance(KEYSTORE).apply { load(null) }

        if (store.containsAlias(alias)) {
            val priv = store.getKey(alias, null) as? java.security.PrivateKey
            val pub = store.getCertificate(alias)?.publicKey
            if (priv != null && pub != null) {
                return KeyPair(pub, priv)
            }
            store.deleteEntry(alias)
        }

        val generator = KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, KEYSTORE)
        val hasStrongBox = Build.VERSION.SDK_INT >= Build.VERSION_CODES.P &&
            context.packageManager.hasSystemFeature(PackageManager.FEATURE_STRONGBOX_KEYSTORE)

        fun spec(strongBox: Boolean): KeyGenParameterSpec {
            val builder = KeyGenParameterSpec.Builder(
                alias,
                KeyProperties.PURPOSE_SIGN or KeyProperties.PURPOSE_VERIFY,
            )
                .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
                .setDigests(KeyProperties.DIGEST_SHA256)
                .setUserAuthenticationRequired(true)

            builder.setInvalidatedByBiometricEnrollment(true)

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P && strongBox) {
                builder.setIsStrongBoxBacked(true)
            }
            return builder.build()
        }

        if (hasStrongBox) {
            try {
                generator.initialize(spec(strongBox = true))
                return generator.generateKeyPair()
            } catch (e: Exception) {
                Log.w(TAG, "StrongBox key generation failed, falling back to TEE Keystore", e)
            }
        }

        generator.initialize(spec(strongBox = false))
        return generator.generateKeyPair()
    }

    /**
     * Extracts the uncompressed 65-byte SEC1 public key point (0x04 || X || Y).
     */
    fun getPublicKeySec1(keyPair: KeyPair): ByteArray {
        val ecPub = keyPair.public as ECPublicKey
        val w = ecPub.w
        val x = w.affineX.toByteArray()
        val y = w.affineY.toByteArray()
        val out = ByteArray(65)
        out[0] = 0x04

        val xStart = if (x.size > 32) x.size - 32 else 0
        val xLen = minOf(x.size, 32)
        System.arraycopy(x, xStart, out, 1 + (32 - xLen), xLen)

        val yStart = if (y.size > 32) y.size - 32 else 0
        val yLen = minOf(y.size, 32)
        System.arraycopy(y, yStart, out, 33 + (32 - yLen), yLen)

        return out
    }

    /**
     * Creates a CryptoObject wrapping an initialized ECDSA-SHA256 signature object.
     */
    fun createCryptoObject(keyPair: KeyPair): BiometricPrompt.CryptoObject {
        val sig = Signature.getInstance("SHA256withECDSA")
        sig.initSign(keyPair.private)
        return BiometricPrompt.CryptoObject(sig)
    }

    /**
     * Signs the unlock challenge using the authenticated Signature object.
     */
    fun signUnlock(
        signature: Signature,
        nonce: ByteArray,
        sessionId: String,
        pcId: String,
        timestamp: Long,
    ): ByteArray {
        val digest = authUnlockDigest(nonce, sessionId, pcId, timestamp.toULong())
        signature.update(digest)
        return signature.sign()
    }

    /**
     * Signs a remote lock request using the authenticated Signature object.
     */
    fun signLock(
        signature: Signature,
        nonce: ByteArray,
        pcId: String,
        timestamp: Long,
    ): ByteArray {
        val digest = authLockDigest(nonce, pcId, timestamp.toULong())
        signature.update(digest)
        return signature.sign()
    }

    /**
     * Removes the key for this PC when unpaired.
     */
    fun deleteKey(pcId: String) {
        runCatching {
            val store = KeyStore.getInstance(KEYSTORE).apply { load(null) }
            store.deleteEntry(aliasFor(pcId))
        }
    }
}

/**
 * Manages heads-up notifications and foreground intent launches for unlock challenges.
 */
object BiometricAuthRequests {
    private const val CHANNEL = "auth_unlock"
    private const val NOTIF_TAG = "auth_unlock"
    private const val TIMEOUT_MS = 30_000L

    fun createChannel(context: Context) {
        val channel = NotificationChannel(
            CHANNEL,
            context.getString(R.string.channel_auth_unlock),
            NotificationManager.IMPORTANCE_HIGH,
        ).apply {
            description = "Prompts to unlock your PC using your phone fingerprint"
            lockscreenVisibility = android.app.Notification.VISIBILITY_PUBLIC
        }
        context.getSystemService(NotificationManager::class.java)?.createNotificationChannel(channel)
    }

    fun show(
        context: Context,
        pcId: String,
        pcName: String,
        sessionId: String,
        nonce: ByteArray,
        expiry: Long,
    ): Boolean {
        createChannel(context)

        val activityIntent = Intent(context, BiometricPromptActivity::class.java).apply {
            putExtra("pc_id", pcId)
            putExtra("pc_name", pcName)
            putExtra("session_id", sessionId)
            putExtra("nonce", nonce)
            putExtra("expiry", expiry)
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        }

        val pendingIntent = PendingIntent.getActivity(
            context,
            sessionId.hashCode(),
            activityIntent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

        val notification = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(context.getString(R.string.auth_unlock_notif_title, pcName))
            .setContentText(context.getString(R.string.auth_unlock_notif_text))
            .setContentIntent(pendingIntent)
            .setFullScreenIntent(pendingIntent, true)
            .setAutoCancel(true)
            .setTimeoutAfter(TIMEOUT_MS)
            .setCategory(NotificationCompat.CATEGORY_ALARM)
            .setPriority(NotificationCompat.PRIORITY_MAX)
            .build()

        val posted = runCatching {
            NotificationManagerCompat.from(context).notify(NOTIF_TAG, sessionId.hashCode(), notification)
        }.isSuccess

        val am = context.getSystemService(android.app.ActivityManager::class.java)
        val inForeground = am?.runningAppProcesses?.any {
            it.pid == android.os.Process.myPid() &&
                it.importance == android.app.ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND
        } == true

        if (inForeground) {
            runCatching { context.startActivity(activityIntent) }
        }

        return posted
    }

    fun dismiss(context: Context, sessionId: String) {
        NotificationManagerCompat.from(context).cancel(NOTIF_TAG, sessionId.hashCode())
    }
}

/**
 * Transparent floating Activity that presents the system BiometricPrompt
 * with hardware-backed CryptoObject authentication.
 */
class BiometricPromptActivity : ComponentActivity() {
    private var cancellationSignal: CancellationSignal? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val pcId = intent.getStringExtra("pc_id") ?: return finish()
        val pcName = intent.getStringExtra("pc_name") ?: getString(R.string.your_pc)
        val sessionId = intent.getStringExtra("session_id") ?: return finish()
        val nonce = intent.getByteArrayExtra("nonce") ?: return finish()
        val expiry = intent.getLongExtra("expiry", 0L)

        val nowSecs = System.currentTimeMillis() / 1000
        if (expiry in 1..nowSecs) {
            finish()
            return
        }

        BiometricAuthRequests.dismiss(this, sessionId)

        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.P) {
            finish()
            return
        }

        val keyPair = try {
            BiometricAuthManager.getOrCreateKeyPair(this, pcId)
        } catch (e: Exception) {
            Log.e("BiometricPrompt", "Failed to retrieve or generate biometric key pair", e)
            finish()
            return
        }

        val cryptoObject = try {
            BiometricAuthManager.createCryptoObject(keyPair)
        } catch (e: Exception) {
            Log.e("BiometricPrompt", "Failed to initialize CryptoObject", e)
            finish()
            return
        }

        cancellationSignal = CancellationSignal()

        val prompt = BiometricPrompt.Builder(this)
            .setTitle(getString(R.string.auth_unlock_title, pcName))
            .setDescription(getString(R.string.auth_unlock_desc, pcName))
            .setNegativeButton(getString(android.R.string.cancel), mainExecutor) { _, _ ->
                finish()
            }
            .build()

        prompt.authenticate(
            cryptoObject,
            cancellationSignal!!,
            mainExecutor,
            object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult?) {
                    val signature = result?.cryptoObject?.signature
                    if (signature != null) {
                        try {
                            val timestamp = System.currentTimeMillis() / 1000
                            val sigBytes = BiometricAuthManager.signUnlock(
                                signature,
                                nonce,
                                sessionId,
                                pcId,
                                timestamp,
                            )
                            val core = (application as? LinkroidApplication)?.core
                            core?.node?.sendUnlockResponse(pcId, sessionId, timestamp.toULong(), sigBytes)
                        } catch (e: Exception) {
                            Log.e("BiometricPrompt", "Failed to sign unlock challenge or send response", e)
                        }
                    }
                    finish()
                }

                override fun onAuthenticationError(errorCode: Int, errString: CharSequence?) {
                    super.onAuthenticationError(errorCode, errString)
                    finish()
                }

                override fun onAuthenticationFailed() {
                    super.onAuthenticationFailed()
                }
            },
        )
    }

    override fun onDestroy() {
        cancellationSignal?.cancel()
        super.onDestroy()
    }
}
