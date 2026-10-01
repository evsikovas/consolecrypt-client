package io.consolecrypt.consolecrypt

import android.app.KeyguardManager
import android.app.Activity
import android.content.Intent
import android.content.ClipData
import android.content.ClipboardManager
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.Settings
import androidx.core.content.FileProvider
import android.hardware.biometrics.BiometricManager
import android.hardware.biometrics.BiometricPrompt
import android.os.CancellationSignal
import android.os.Bundle
import android.os.PersistableBundle
import android.view.WindowManager
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File

class MainActivity : FlutterActivity() {
    private external fun nativeInitialize(context: android.content.Context)
    private var authCancellation: CancellationSignal? = null
    private var exportResult: MethodChannel.Result? = null
    private var exportSource: File? = null
    private var authResult: MethodChannel.Result? = null

    // Device-wide, non-secret preference, read before Flutter's first frame.
    // It must also apply on the lock screen and after Activity recreation.
    private val screenCapturePreferences by lazy {
        getSharedPreferences("screen_capture", MODE_PRIVATE)
    }

    private fun savedScreenCaptureAllowed(): Boolean = runCatching {
        screenCapturePreferences.getBoolean("allowed", false)
    }.getOrDefault(false)

    private fun applyScreenCaptureAllowed(allowed: Boolean) {
        if (allowed) window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        else window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
    }

    private fun screenCaptureAllowed(): Boolean =
        window.attributes.flags and WindowManager.LayoutParams.FLAG_SECURE == 0

    override fun onCreate(savedInstanceState: Bundle?) {
        applyScreenCaptureAllowed(savedScreenCaptureAllowed())
        super.onCreate(savedInstanceState)
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        applyScreenCaptureAllowed(savedScreenCaptureAllowed())
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "consolecrypt/clipboard").setMethodCallHandler { call, result ->
            if (call.method != "copySecret") {
                result.notImplemented()
                return@setMethodCallHandler
            }
            val text = call.argument<Any>("text") as? String
            val milliseconds = when (val value = call.argument<Any>("clearAfterMilliseconds")) {
                is Int -> value.toLong()
                is Long -> value
                else -> null
            }
            if (text == null || milliseconds == null || milliseconds !in 1..86_400_000L) {
                result.error("bad_args", "Invalid secret clipboard request", null)
                return@setMethodCallHandler
            }
            try {
                val clip = ClipData.newPlainText("", text)
                // The API 33 constant's literal also works on our API 30 minimum.
                // Android's preview must never show the copied secret. The Dart
                // owner still clears it on timeout/disposal; Android has no TTL API.
                clip.description.extras = PersistableBundle().apply {
                    putBoolean("android.content.extra.IS_SENSITIVE", true)
                }
                getSystemService(ClipboardManager::class.java).setPrimaryClip(clip)
                result.success(null)
            } catch (_: Exception) {
                result.error("clipboard", "Secret clipboard unavailable", null)
            }
        }
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "consolecrypt/updates").setMethodCallHandler { call, result ->
            val cache = File(cacheDir, "updates")
            when (call.method) {
                "cacheDirectory" -> if (cache.isDirectory || cache.mkdirs()) result.success(cache.absolutePath)
                    else result.error("storage", "Update cache unavailable", null)
                "install" -> try {
                    val source = File(call.argument<String>("path") ?: "").canonicalFile
                    require(source.isFile && source.extension == "apk" && source.path.startsWith(cache.canonicalPath + File.separator))
                    val archive = packageManager.getPackageArchiveInfo(source.path, PackageManager.GET_SIGNING_CERTIFICATES)
                        ?: throw IllegalArgumentException("Invalid APK")
                    val current = packageManager.getPackageInfo(packageName, PackageManager.GET_SIGNING_CERTIFICATES)
                    require(archive.packageName == packageName && archive.versionName == call.argument<String>("version"))
                    require(archive.longVersionCode > current.longVersionCode)
                    val incoming = archive.signingInfo?.apkContentsSigners ?: throw IllegalArgumentException("Missing signer")
                    val existing = current.signingInfo?.apkContentsSigners ?: throw IllegalArgumentException("Missing signer")
                    require(incoming.size == existing.size && incoming.all { signer -> existing.any { it == signer } })
                    if (!packageManager.canRequestPackageInstalls()) {
                        startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:$packageName")))
                        result.success("permission")
                    } else {
                        val uri = FileProvider.getUriForFile(this, "$packageName.updates", source)
                        startActivity(Intent(Intent.ACTION_VIEW).apply {
                            setDataAndType(uri, "application/vnd.android.package-archive")
                            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                        })
                        result.success("opened")
                    }
                } catch (_: Exception) { result.error("installer", "Cannot install this update", null) }
                else -> result.notImplemented()
            }
        }
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "consolecrypt/screen_capture").setMethodCallHandler { call, result ->
            when (call.method) {
                "getAllowed" -> result.success(screenCaptureAllowed())
                "setAllowed" -> {
                    val allowed = call.argument<Any>("allowed") as? Boolean
                    if (allowed == null) {
                        result.error("invalid_argument", "Expected a boolean capture preference", null)
                    } else {
                        val previous = savedScreenCaptureAllowed()
                        // Acknowledge only after persistence, then update the live window.
                        if (screenCapturePreferences.edit().putBoolean("allowed", allowed).commit()) {
                            applyScreenCaptureAllowed(allowed)
                            result.success(screenCaptureAllowed())
                        } else {
                            screenCapturePreferences.edit().putBoolean("allowed", previous).commit()
                            result.error("storage", "Could not save capture preference", null)
                        }
                    }
                }
                else -> result.notImplemented()
            }
        }
        val initialized = runCatching {
            System.loadLibrary("cc_bridge")
            nativeInitialize(applicationContext)
        }.isSuccess
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "consolecrypt/android").setMethodCallHandler { call, result ->
            when (call.method) {
                "dataDirectory" -> if (initialized) {
                    // noBackupFilesDir prevents transfer of DBs without their Keystore keys.
                    val dir = File(noBackupFilesDir, "consolecrypt")
                    if (dir.isDirectory || dir.mkdirs()) result.success(dir.absolutePath)
                    else result.error("storage", "Cannot create private app storage", null)
                } else result.error("native_init", "Android security initialization failed", null)
                "exportFile" -> {
                    val path = call.argument<String>("path")
                    val source = path?.let { File(it).canonicalFile }
                    if (source == null || !source.isFile || !source.path.startsWith(noBackupFilesDir.canonicalPath + File.separator)) {
                        result.error("export_path", "Invalid export file", null)
                    } else if (exportResult != null) {
                        result.error("busy", "An export is already open", null)
                    } else {
                        exportResult = result
                        exportSource = source
                        try {
                            startActivityForResult(Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                                addCategory(Intent.CATEGORY_OPENABLE)
                                type = "application/octet-stream"
                                putExtra(Intent.EXTRA_TITLE, source.name)
                            }, 7401)
                        } catch (_: Exception) {
                            exportResult = null; exportSource = null
                            result.error("export_unavailable", "Document picker unavailable", null)
                        }
                    }
                }
                else -> result.notImplemented()
            }
        }
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "consolecrypt/local_auth").setMethodCallHandler { call, result ->
            when (call.method) {
                "availability" -> {
                    val manager = getSystemService(BiometricManager::class.java)
                    val enrolled = manager.canAuthenticate(authenticators) == BiometricManager.BIOMETRIC_SUCCESS
                    result.success(mapOf("kind" to if (enrolled) "device_credential" else null,
                        "not_enrolled" to !getSystemService(KeyguardManager::class.java).isDeviceSecure))
                }
                "authenticate" -> {
                    if (authResult != null) { result.success(false); return@setMethodCallHandler }
                    authResult = result
                    val signal = CancellationSignal()
                    authCancellation = signal
                    try {
                        BiometricPrompt.Builder(this)
                            .setTitle("ConsoleCrypt")
                            .setSubtitle(call.argument<String>("reason") ?: "Unlock vault")
                            .setAllowedAuthenticators(authenticators)
                            .build().authenticate(signal, mainExecutor, object : BiometricPrompt.AuthenticationCallback() {
                                override fun onAuthenticationSucceeded(value: BiometricPrompt.AuthenticationResult) = finishAuth(true)
                                override fun onAuthenticationError(code: Int, message: CharSequence) = finishAuth(false)
                            })
                    } catch (_: Exception) { finishAuth(false) }
                }
                else -> result.notImplemented()
            }
        }
    }

    @Deprecated("Activity result API used by FlutterActivity")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        if (requestCode != 7401) { super.onActivityResult(requestCode, resultCode, data); return }
        val result = exportResult ?: return
        val source = exportSource
        exportResult = null; exportSource = null
        val uri = data?.data
        if (resultCode != Activity.RESULT_OK || uri == null || source == null) { result.success(false); return }
        Thread {
            val ok = runCatching {
                require(uri.scheme == "content")
                contentResolver.openOutputStream(uri, "wt")!!.use { output ->
                    source.inputStream().use { it.copyTo(output) }
                }
            }.isSuccess
            runOnUiThread {
                if (ok) result.success(true) else result.error("export_failed", "Could not save document", null)
            }
        }.start()
    }

    private fun finishAuth(success: Boolean) {
        val result = authResult
        authResult = null
        authCancellation = null
        result?.success(success)
    }

    override fun onDestroy() {
        exportResult?.success(false)
        exportResult = null; exportSource = null
        authCancellation?.cancel()
        finishAuth(false)
        super.onDestroy()
    }

    private val authenticators = BiometricManager.Authenticators.BIOMETRIC_STRONG or
        BiometricManager.Authenticators.DEVICE_CREDENTIAL
}
