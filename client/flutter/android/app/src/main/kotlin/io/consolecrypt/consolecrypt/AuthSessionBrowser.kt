package io.consolecrypt.consolecrypt

import android.app.Activity
import android.net.Uri
import androidx.browser.customtabs.CustomTabsIntent
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel

/**
 * Browser-only OIDC transport (Custom Tabs) for one fixed redirect URI:
 * Kubernetes sign-in (`consolecrypt://kubernetes/oauth/callback`) or the
 * sync server's enterprise sign-in (`consolecrypt://auth/callback`). Rust
 * owns PKCE, state verification and tokens; this only hands back the
 * callback whose `state` matches the one in the opened URL.
 */
class AuthSessionBrowser(private val activity: Activity, private val redirectUri: String) {
    private val redirect = Uri.parse(redirectUri)
    private var result: MethodChannel.Result? = null
    private var ticket: String? = null
    private var expectedState: String? = null
    private var leftActivity = false

    fun handle(call: MethodCall, completion: MethodChannel.Result) {
        when (call.method) {
            "openBrowser" -> {
                if (result != null) {
                    completion.error("busy", "An authentication browser is already open", null)
                    return
                }
                try {
                    val url = Uri.parse(call.argument<String>("url") ?: "")
                    val redirectArg = call.argument<String>("redirectUri")
                    val requestTicket = call.argument<String>("ticket")
                    val state = url.getQueryParameters("state")
                    val loopbackDev = url.scheme == "http" && url.host in setOf("localhost", "127.0.0.1", "::1")
                    require((url.scheme == "https" || loopbackDev) && !url.host.isNullOrEmpty() &&
                        url.userInfo == null && url.fragment == null)
                    require(redirectArg == redirectUri)
                    require(state.size == 1 && state[0].length in 32..256)
                    require(!requestTicket.isNullOrEmpty() && requestTicket.length <= 64)
                    result = completion
                    ticket = requestTicket
                    expectedState = state[0]
                    leftActivity = false
                    CustomTabsIntent.Builder().build().launchUrl(activity, url)
                } catch (_: Exception) {
                    (clear() ?: completion).error("browser", "Authentication browser unavailable", null)
                }
            }
            "cancelBrowser" -> {
                if (ticket == call.argument<String>("ticket")) cancel()
                completion.success(null)
            }
            else -> completion.notImplemented()
        }
    }

    /** Whether [uri] is addressed to this browser's redirect URI. */
    fun accepts(uri: Uri?): Boolean = uri != null && uri.scheme == redirect.scheme &&
        uri.host == redirect.host && uri.path == redirect.path

    fun onCallback(uri: Uri?) {
        if (result == null || uri == null || uri.toString().length > 32768) return
        if (!accepts(uri) || uri.fragment != null || uri.userInfo != null) return
        val states = uri.getQueryParameters("state")
        if (states.size != 1 || states[0] != expectedState) return
        // The code is handed directly to the active native flow. It is not
        // persisted in preferences, logs, restored state or app navigation.
        clear()?.success(uri.toString())
    }

    fun onPause() {
        if (result != null) leftActivity = true
    }

    fun onResume() {
        val currentTicket = ticket
        if (!leftActivity || currentTicket == null) return
        activity.window.decorView.postDelayed({
            if (ticket == currentTicket && leftActivity) cancel()
        }, 500)
    }

    fun cancel() { clear()?.success(null) }

    private fun clear(): MethodChannel.Result? {
        val completion = result
        result = null
        ticket = null
        expectedState = null
        leftActivity = false
        return completion
    }

    companion object {
        const val KUBERNETES_REDIRECT = "consolecrypt://kubernetes/oauth/callback"
        const val SYNC_SERVER_REDIRECT = "consolecrypt://auth/callback"

        /** The redirect URIs the callback activity forwards. */
        fun isCallback(uri: Uri?): Boolean = uri != null && uri.scheme == "consolecrypt" &&
            ((uri.host == "kubernetes" && uri.path == "/oauth/callback") ||
                (uri.host == "auth" && uri.path == "/callback")) &&
            uri.toString().length <= 32768
    }
}
