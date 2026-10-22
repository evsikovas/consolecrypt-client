package io.consolecrypt.consolecrypt

import android.app.Activity
import android.content.Intent
import android.content.ClipData
import android.content.ClipboardManager
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.Settings
import androidx.core.content.FileProvider
import android.os.Bundle
import android.os.PersistableBundle
import android.view.WindowManager
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.ViewTreeObserver
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File

class MainActivity : FlutterActivity() {
    private val kubeAuthBrowser by lazy { AuthSessionBrowser(this, AuthSessionBrowser.KUBERNETES_REDIRECT) }
    private val syncAuthBrowser by lazy { AuthSessionBrowser(this, AuthSessionBrowser.SYNC_SERVER_REDIRECT) }
    private external fun nativeInitialize(context: android.content.Context)
    private var exportResult: MethodChannel.Result? = null
    private var exportSource: File? = null
    private var forwardPermissionResult: MethodChannel.Result? = null
    private var rdpFullscreen = false
    private var rdpStatusVisible = true
    private var rdpNavigationVisible = true
    private var rdpBarsBehavior = WindowInsetsController.BEHAVIOR_DEFAULT
    private var rdpImeVisible = false
    private val rdpLayoutListener = ViewTreeObserver.OnGlobalLayoutListener {
        val visible = window.decorView.rootWindowInsets?.isVisible(WindowInsets.Type.ime()) == true
        if (rdpFullscreen && rdpImeVisible && !visible) {
            window.decorView.postDelayed({ applyRdpFullscreen() }, 1100)
        }
        rdpImeVisible = visible
    }

    private fun applyRdpFullscreen() {
        if (!rdpFullscreen) return
        window.insetsController?.apply {
            systemBarsBehavior = WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            hide(WindowInsets.Type.systemBars())
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus && rdpFullscreen) {
            // Android can reveal bars while dismissing the IME. Reapply only
            // while this session still owns fullscreen, after its guard delay.
            window.decorView.postDelayed({ applyRdpFullscreen() }, 1100)
        }
    }

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

    private fun startKubeForwardService(result: MethodChannel.Result) {
        try {
            startForegroundService(Intent(this, KubeForwardService::class.java))
            result.success(null)
        } catch (_: Exception) {
            result.error("foreground_unavailable", "Unable to show the tunnel notification", null)
        }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != 6202) return
        val result = forwardPermissionResult ?: return
        forwardPermissionResult = null
        if (grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) result.success(null)
        else result.error("notification_denied", "Tunnel notification permission is required", null)
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "consolecrypt/kube_forwards")
            .setMethodCallHandler { call, result ->
                if (call.method != "start" && call.method != "prepare") {
                    result.notImplemented()
                } else if (forwardPermissionResult != null) {
                    result.error("busy", "Notification permission is already pending", null)
                } else if (android.os.Build.VERSION.SDK_INT >= 33 &&
                    checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
                    if (call.method == "prepare") {
                        forwardPermissionResult = result
                        requestPermissions(arrayOf(android.Manifest.permission.POST_NOTIFICATIONS), 6202)
                    } else result.error("notification_denied", "Tunnel notification permission is required", null)
                } else if (call.method == "prepare") {
                    result.success(null)
                } else {
                    startKubeForwardService(result)
                }
            }
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "consolecrypt/kube_auth")
            .setMethodCallHandler { call, result -> kubeAuthBrowser.handle(call, result) }
        // Enterprise sign-in to the sync server (protocol 1.8).
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "consolecrypt/auth_session")
            .setMethodCallHandler { call, result -> syncAuthBrowser.handle(call, result) }
        applyScreenCaptureAllowed(savedScreenCaptureAllowed())
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "consolecrypt/window").setMethodCallHandler { call, result ->
            when (call.method) {
                "beginRdpFullscreen" -> {
                    if (!rdpFullscreen) {
                        val insets = window.decorView.rootWindowInsets
                        rdpStatusVisible = insets?.isVisible(WindowInsets.Type.statusBars()) ?: true
                        rdpNavigationVisible = insets?.isVisible(WindowInsets.Type.navigationBars()) ?: true
                        rdpBarsBehavior = window.insetsController?.systemBarsBehavior ?: WindowInsetsController.BEHAVIOR_DEFAULT
                        rdpFullscreen = true
                        rdpImeVisible = insets?.isVisible(WindowInsets.Type.ime()) == true
                        window.decorView.viewTreeObserver.addOnGlobalLayoutListener(rdpLayoutListener)
                    }
                    applyRdpFullscreen()
                    result.success(true)
                }
                "endRdpFullscreen" -> {
                    if (rdpFullscreen) {
                        rdpFullscreen = false
                        window.decorView.viewTreeObserver.removeOnGlobalLayoutListener(rdpLayoutListener)
                        window.insetsController?.apply {
                            systemBarsBehavior = rdpBarsBehavior
                            if (rdpStatusVisible) show(WindowInsets.Type.statusBars()) else hide(WindowInsets.Type.statusBars())
                            if (rdpNavigationVisible) show(WindowInsets.Type.navigationBars()) else hide(WindowInsets.Type.navigationBars())
                        }
                    }
                    result.success(null)
                }
                "isFullscreen" -> result.success(rdpFullscreen)
                else -> result.notImplemented()
            }
        }
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
        // Device unlock (S5): Keystore key + BiometricPrompt CryptoObject.
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "consolecrypt/local_auth").setMethodCallHandler { call, result ->
            val reply = { outcome: DeviceUnlockKeys.Outcome ->
                when (outcome) {
                    is DeviceUnlockKeys.Outcome.Created ->
                        result.success(mapOf("material" to outcome.material, "handle" to outcome.handle))
                    is DeviceUnlockKeys.Outcome.Released -> result.success(outcome.material)
                    DeviceUnlockKeys.Outcome.Cancelled -> result.error("cancelled", "Authentication cancelled", null)
                    DeviceUnlockKeys.Outcome.Invalidated -> result.error("invalidated", "Key invalidated", null)
                    is DeviceUnlockKeys.Outcome.Failed -> result.error("failed", outcome.reason, null)
                }
            }
            val reason = call.argument<String>("reason") ?: "unlock your ConsoleCrypt vault"
            when (call.method) {
                "availability" -> result.success(deviceUnlockKeys.availability())
                "createKey" -> deviceUnlockKeys.create(call.argument<String>("alias") ?: "", reason, reply)
                "releaseKey" -> deviceUnlockKeys.release(call.argument<ByteArray>("handle") ?: ByteArray(0), reason, reply)
                "deleteKey" -> {
                    deviceUnlockKeys.delete(call.argument<String>("alias") ?: "")
                    result.success(null)
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

    override fun onDestroy() {
        kubeAuthBrowser.cancel()
        syncAuthBrowser.cancel()
        forwardPermissionResult?.error("cancelled", "Notification request was cancelled", null)
        forwardPermissionResult = null
        exportResult?.success(false)
        exportResult = null; exportSource = null
        deviceUnlockKeys.cancel()
        super.onDestroy()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        val data = intent.data
        if (AuthSessionBrowser.isCallback(data)) {
            if (kubeAuthBrowser.accepts(data)) kubeAuthBrowser.onCallback(data)
            if (syncAuthBrowser.accepts(data)) syncAuthBrowser.onCallback(data)
            intent.data = null
        }
    }

    override fun onPause() {
        kubeAuthBrowser.onPause()
        syncAuthBrowser.onPause()
        super.onPause()
    }

    override fun onResume() {
        super.onResume()
        kubeAuthBrowser.onResume()
        syncAuthBrowser.onResume()
    }

    private val deviceUnlockKeys by lazy { DeviceUnlockKeys(this) }
}
