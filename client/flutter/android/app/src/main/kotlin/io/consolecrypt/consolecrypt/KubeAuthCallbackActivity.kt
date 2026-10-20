package io.consolecrypt.consolecrypt

import android.app.Activity
import android.content.Intent
import android.os.Bundle

/** Brings the existing Flutter activity forward without creating a new engine. */
class KubeAuthCallbackActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val uri = intent?.data
        if (uri != null && uri.scheme == "consolecrypt" && uri.host == "kubernetes" &&
            uri.path == "/oauth/callback" && uri.toString().length <= 32768) {
            startActivity(Intent(this, MainActivity::class.java).apply {
                data = uri
                addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            })
        }
        intent?.data = null
        finish()
    }
}
