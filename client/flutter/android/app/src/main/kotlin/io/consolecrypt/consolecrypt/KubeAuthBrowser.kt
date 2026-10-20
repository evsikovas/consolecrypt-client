package io.consolecrypt.consolecrypt

import android.app.Activity
import android.net.Uri
import androidx.browser.customtabs.CustomTabsIntent
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel

/** Browser-only OIDC transport. Rust owns PKCE, state verification and tokens. */
class KubeAuthBrowser(private val activity: Activity) {
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
                    val redirect = call.argument<String>("redirectUri")
                    val requestTicket = call.argument<String>("ticket")
                    val state = url.getQueryParameters("state")
                    require(url.scheme == "https" && !url.host.isNullOrEmpty() &&
                        url.userInfo == null && url.fragment == null)
                    require(redirect == "consolecrypt://kubernetes/oauth/callback")
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

    fun onCallback(uri: Uri?) {
        if (result == null || uri == null || uri.toString().length > 32768) return
        if (uri.scheme != "consolecrypt" || uri.host != "kubernetes" ||
            uri.path != "/oauth/callback" || uri.fragment != null || uri.userInfo != null) return
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
}
