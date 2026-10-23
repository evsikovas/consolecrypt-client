package io.consolecrypt.consolecrypt

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.PersistableBundle
import android.os.SystemClock
import java.security.SecureRandom

/**
 * Protected clipboard for secrets (S8): `consolecrypt/clipboard`.
 *
 * * Every clip we write is marked IS_SENSITIVE (no preview in the system
 *   copy notification or keyboard suggestions) and carries a random 128-bit
 *   `io.consolecrypt.clip_id` in its description extras — the ownership
 *   token. Neither the text nor a hash of it is stored anywhere.
 * * A secret's pending clear is `{clip_id, deadline}` in private
 *   SharedPreferences plus an inexact `setAndAllowWhileIdle` alarm (no exact
 *   alarm permission; a Doze delay is acceptable) to [ClipboardClearReceiver].
 * * Android 10+ lets only the focused app read the clipboard, so ownership is
 *   checked when the window has focus. In the background the clip is cleared
 *   without a check when "clear in the background" is on (default), or left
 *   pending until the window regains focus.
 */
object SecretClipboard {
    private const val PREFS = "clipboard"
    private const val KEY_ID = "clip_id"
    private const val KEY_DEADLINE = "deadline_ms"
    private const val KEY_UNVERIFIED = "clear_unverified"
    private const val EXTRA_CLIP_ID = "io.consolecrypt.clip_id"

    // The API 33 constant's value; the literal also works on our API 30 minimum.
    private const val EXTRA_IS_SENSITIVE_LITERAL = "android.content.extra.IS_SENSITIVE"

    /** Whether our window has input focus (and so may read the clipboard). */
    @Volatile
    var focused = false
        private set

    private val random = SecureRandom()

    private fun prefs(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    private fun manager(context: Context) = context.getSystemService(ClipboardManager::class.java)

    private fun newId(): String {
        val bytes = ByteArray(16)
        random.nextBytes(bytes)
        return bytes.joinToString("") { "%02x".format(it) }
    }

    fun backgroundClearEnabled(context: Context): Boolean =
        runCatching { prefs(context).getBoolean(KEY_UNVERIFIED, true) }.getOrDefault(true)

    fun setBackgroundClearEnabled(context: Context, enabled: Boolean): Boolean =
        prefs(context).edit().putBoolean(KEY_UNVERIFIED, enabled).commit()

    /**
     * Writes [text] as a sensitive clip. With [clearAfterMs], schedules its
     * clear and returns the clip id (token); without, it is a private copy
     * that is never cleared. Any earlier pending clear is cancelled: the
     * clipboard now holds this clip.
     */
    fun copy(context: Context, text: String, clearAfterMs: Long?): String {
        val id = newId()
        val clip = ClipData.newPlainText("", text)
        clip.description.extras = PersistableBundle().apply {
            val sensitive = if (Build.VERSION.SDK_INT >= 33) ClipDescription.EXTRA_IS_SENSITIVE
            else EXTRA_IS_SENSITIVE_LITERAL
            putBoolean(sensitive, true)
            putString(EXTRA_CLIP_ID, id)
        }
        forget(context)
        manager(context).setPrimaryClip(clip)
        if (clearAfterMs != null) {
            prefs(context).edit()
                .putString(KEY_ID, id)
                .putLong(KEY_DEADLINE, System.currentTimeMillis() + clearAfterMs)
                .commit()
            schedule(context, clearAfterMs)
        }
        return id
    }

    /** Drops the pending clear (the clipboard got other content of ours). */
    fun forget(context: Context) {
        cancelAlarm(context)
        prefs(context).edit().remove(KEY_ID).remove(KEY_DEADLINE).commit()
    }

    private fun drop(context: Context, id: String) {
        if (prefs(context).getString(KEY_ID, null) == id) forget(context)
    }

    /**
     * Clears the clip [token] if the clipboard still holds it. Returns
     * `cleared`, `foreign`, `gone` or `deferred` (background, unverifiable,
     * clearing without a check off: still pending until focus returns).
     */
    fun clearIfOwned(context: Context, token: String): String {
        val manager = manager(context)
        // Null when the clipboard is empty — or, without focus on Android 10+,
        // when the system hides it from us.
        val description = runCatching { manager.primaryClipDescription }.getOrNull()
        if (description != null) {
            val owned = description.extras?.getString(EXTRA_CLIP_ID) == token
            if (owned) manager.clearPrimaryClip()
            drop(context, token)
            return if (owned) "cleared" else "foreign"
        }
        if (focused) {
            drop(context, token)
            return "gone"
        }
        if (prefs(context).getString(KEY_ID, null) != token) {
            // A newer copy of ours replaced this clip (or it was handled).
            return "gone"
        }
        if (!backgroundClearEnabled(context)) return "deferred"
        // Background, unverifiable: on Android 10+ only the app on screen can
        // write the clipboard, so whatever is there was copied while we were
        // away. Clearing it is the lesser evil (user setting, on by default).
        manager.clearPrimaryClip()
        drop(context, token)
        return "cleared"
    }

    /** Alarm: the deadline of the pending clear has (nearly) passed. */
    fun onAlarm(context: Context) {
        val prefs = prefs(context)
        val id = prefs.getString(KEY_ID, null) ?: return
        val left = prefs.getLong(KEY_DEADLINE, 0) - System.currentTimeMillis()
        if (left > 1_000) {
            schedule(context, left)
            return
        }
        runCatching { clearIfOwned(context, id) }
    }

    /**
     * Window focus changed. On focus (including a cold start or the return
     * after a reboot, when the alarm is lost) an expired pending clear is
     * finished with an ownership check; a future one gets its alarm back.
     */
    fun onFocusChanged(context: Context, hasFocus: Boolean) {
        focused = hasFocus
        if (!hasFocus) return
        val prefs = prefs(context)
        val id = prefs.getString(KEY_ID, null) ?: return
        val left = prefs.getLong(KEY_DEADLINE, 0) - System.currentTimeMillis()
        if (left > 0) schedule(context, left) else runCatching { clearIfOwned(context, id) }
    }

    private fun alarmIntent(context: Context): PendingIntent = PendingIntent.getBroadcast(
        context,
        0,
        Intent(context, ClipboardClearReceiver::class.java),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    private fun schedule(context: Context, delayMs: Long) {
        runCatching {
            context.getSystemService(AlarmManager::class.java).setAndAllowWhileIdle(
                AlarmManager.ELAPSED_REALTIME_WAKEUP,
                SystemClock.elapsedRealtime() + delayMs.coerceAtLeast(0),
                alarmIntent(context),
            )
        }
    }

    private fun cancelAlarm(context: Context) {
        runCatching { context.getSystemService(AlarmManager::class.java).cancel(alarmIntent(context)) }
    }
}

/** Not exported: only our own alarm can trigger it. */
class ClipboardClearReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        SecretClipboard.onAlarm(context.applicationContext)
    }
}
