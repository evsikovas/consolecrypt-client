package io.consolecrypt.consolecrypt

import android.app.Activity
import android.app.KeyguardManager
import android.hardware.biometrics.BiometricManager
import android.hardware.biometrics.BiometricPrompt
import android.os.CancellationSignal
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Device unlock under OS protection (S5) for the `consolecrypt/local_auth`
 * channel.
 *
 * One Android Keystore AES-256-GCM key per profile that requires user
 * authentication for every use (class-3 biometrics or the screen lock),
 * is invalidated by a new biometric enrollment and lives in StrongBox when
 * the device has one. Its Cipher works only inside a successful
 * BiometricPrompt (CryptoObject): enabling encrypts 32 random bytes of key
 * material, unlocking decrypts them. The material goes to the Rust core,
 * which only checks it against the profile's unlock envelope; nothing here
 * is trusted as "the user authenticated".
 *
 * Handle (stored by the core, not secret):
 * `1 || u8 len(alias) || alias || u8 len(iv) || iv || ciphertext+tag`.
 */
internal class DeviceUnlockKeys(private val activity: Activity) {
    sealed interface Outcome {
        class Created(val material: ByteArray, val handle: ByteArray) : Outcome
        class Released(val material: ByteArray) : Outcome
        object Cancelled : Outcome
        object Invalidated : Outcome
        class Failed(val reason: String) : Outcome
    }

    private val authenticators = BiometricManager.Authenticators.BIOMETRIC_STRONG or
        BiometricManager.Authenticators.DEVICE_CREDENTIAL
    private var pending: CancellationSignal? = null

    fun availability(): Map<String, Any?> {
        val biometrics = activity.getSystemService(BiometricManager::class.java)
        val secure = activity.getSystemService(KeyguardManager::class.java).isDeviceSecure
        val kind = when {
            biometrics.canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG) ==
                BiometricManager.BIOMETRIC_SUCCESS -> "fingerprint"
            secure -> "device_credential"
            else -> null
        }
        return mapOf("kind" to kind, "not_enrolled" to !secure)
    }

    /** New key [alias]; inside the prompt, encrypt fresh material with it. */
    fun create(alias: String, reason: String, done: (Outcome) -> Unit) {
        if (!validAlias(alias)) return done(Outcome.Failed("invalid alias"))
        val cipher = try {
            delete(alias)
            Cipher.getInstance(TRANSFORMATION).apply { init(Cipher.ENCRYPT_MODE, generate(alias)) }
        } catch (e: Exception) {
            delete(alias)
            return done(Outcome.Failed(e.javaClass.simpleName))
        }
        // A key that never encrypted anything is useless: drop it.
        val finish = { outcome: Outcome ->
            if (outcome !is Outcome.Created) delete(alias)
            done(outcome)
        }
        prompt(cipher, reason, finish) { unlocked ->
            val material = ByteArray(MATERIAL_LEN).also { SecureRandom().nextBytes(it) }
            val sealed = unlocked.doFinal(material)
            Outcome.Created(material, encode(alias, unlocked.iv, sealed))
        }
    }

    /** Inside the prompt, decrypt the material in [handle]. */
    fun release(handle: ByteArray, reason: String, done: (Outcome) -> Unit) {
        val parsed = decode(handle) ?: return done(Outcome.Invalidated)
        val (alias, iv, sealed) = parsed
        val cipher = try {
            val key = keyStore().getKey(alias, null) as? SecretKey ?: return done(Outcome.Invalidated)
            Cipher.getInstance(TRANSFORMATION).apply {
                init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(TAG_BITS, iv))
            }
        } catch (_: KeyPermanentlyInvalidatedException) {
            delete(alias)
            return done(Outcome.Invalidated)
        } catch (e: Exception) {
            return done(Outcome.Failed(e.javaClass.simpleName))
        }
        prompt(cipher, reason, done) { unlocked -> Outcome.Released(unlocked.doFinal(sealed)) }
    }

    fun delete(alias: String) {
        if (!validAlias(alias)) return
        runCatching { keyStore().deleteEntry(alias) }
    }

    fun cancel() {
        pending?.cancel()
        pending = null
    }

    /** Runs [use] with the authenticated Cipher; reports through [done] on the main thread. */
    private fun prompt(
        cipher: Cipher,
        reason: String,
        done: (Outcome) -> Unit,
        use: (Cipher) -> Outcome,
    ) {
        if (pending != null) return done(Outcome.Failed("busy"))
        val signal = CancellationSignal()
        pending = signal
        val finish = { outcome: Outcome ->
            if (pending === signal) pending = null
            done(outcome)
        }
        try {
            BiometricPrompt.Builder(activity)
                .setTitle("ConsoleCrypt")
                .setSubtitle(reason.replaceFirstChar { it.uppercase() })
                .setAllowedAuthenticators(authenticators)
                .build()
                .authenticate(
                    BiometricPrompt.CryptoObject(cipher),
                    signal,
                    activity.mainExecutor,
                    object : BiometricPrompt.AuthenticationCallback() {
                        override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                            val unlocked = result.cryptoObject?.cipher
                            finish(
                                if (unlocked == null) {
                                    Outcome.Failed("no cipher")
                                } else {
                                    try {
                                        use(unlocked)
                                    } catch (_: KeyPermanentlyInvalidatedException) {
                                        Outcome.Invalidated
                                    } catch (e: Exception) {
                                        Outcome.Failed(e.javaClass.simpleName)
                                    }
                                },
                            )
                        }

                        override fun onAuthenticationError(code: Int, message: CharSequence) {
                            finish(Outcome.Cancelled)
                        }
                    },
                )
        } catch (e: Exception) {
            pending = null
            done(Outcome.Failed(e.javaClass.simpleName))
        }
    }

    private fun generate(alias: String): SecretKey {
        fun spec(strongBox: Boolean) = KeyGenParameterSpec.Builder(
            alias,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setUserAuthenticationRequired(true)
            // Every use needs a fresh prompt (no validity window).
            .setUserAuthenticationParameters(
                0,
                KeyProperties.AUTH_BIOMETRIC_STRONG or KeyProperties.AUTH_DEVICE_CREDENTIAL,
            )
            .setInvalidatedByBiometricEnrollment(true)
            .setIsStrongBoxBacked(strongBox)
            .build()
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        return try {
            generator.init(spec(strongBox = true))
            generator.generateKey()
        } catch (_: StrongBoxUnavailableException) {
            generator.init(spec(strongBox = false))
            generator.generateKey()
        }
    }

    private fun keyStore(): KeyStore = KeyStore.getInstance(KEYSTORE).apply { load(null) }

    companion object {
        private const val KEYSTORE = "AndroidKeyStore"
        private const val TRANSFORMATION = "AES/GCM/NoPadding"
        private const val TAG_BITS = 128
        private const val MATERIAL_LEN = 32
        private const val HANDLE_V1: Byte = 1

        fun validAlias(alias: String) =
            alias.startsWith("cc-device-unlock-") && alias.length <= 128 &&
                alias.all { it.isLetterOrDigit() || it == '-' }

        fun encode(alias: String, iv: ByteArray, sealed: ByteArray): ByteArray {
            val a = alias.toByteArray(Charsets.UTF_8)
            return byteArrayOf(HANDLE_V1, a.size.toByte()) + a + byteArrayOf(iv.size.toByte()) + iv + sealed
        }

        fun decode(handle: ByteArray): Triple<String, ByteArray, ByteArray>? {
            if (handle.size < 3 || handle[0] != HANDLE_V1) return null
            val aliasLen = handle[1].toInt() and 0xff
            val ivAt = 2 + aliasLen
            if (handle.size <= ivAt) return null
            val alias = String(handle, 2, aliasLen, Charsets.UTF_8)
            val ivLen = handle[ivAt].toInt() and 0xff
            val sealedAt = ivAt + 1 + ivLen
            if (!validAlias(alias) || ivLen != 12 || handle.size <= sealedAt + 16) return null
            return Triple(alias, handle.copyOfRange(ivAt + 1, sealedAt), handle.copyOfRange(sealedAt, handle.size))
        }
    }
}
