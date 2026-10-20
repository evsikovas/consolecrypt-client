package io.consolecrypt.consolecrypt

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Handler
import android.os.IBinder
import android.os.Looper

/** Owns visibility and cancellation; the native core owns loopback sockets. */
class KubeForwardService : Service() {
    private external fun nativeForwardCount(): Int
    private external fun nativeStopForwards()
    private val handler = Handler(Looper.getMainLooper())
    private var lastCount = -1
    private val refresh = object : Runnable {
        override fun run() {
            val count = nativeForwardCount()
            if (count == 0) {
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
                return
            }
            if (count != lastCount) {
                getSystemService(NotificationManager::class.java).notify(NOTIFICATION, notification(count))
                lastCount = count
            }
            handler.postDelayed(this, 1000)
        }
    }

    override fun onCreate() {
        super.onCreate()
        System.loadLibrary("cc_bridge")
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL, getString(R.string.kube_forward_channel), NotificationManager.IMPORTANCE_LOW)
                .apply { setShowBadge(false) }
        )
    }

    private fun notification(count: Int): Notification {
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val stop = PendingIntent.getService(
            this, 1, Intent(this, KubeForwardService::class.java).setAction(STOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        return Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_kube_forward)
            .setContentTitle(getString(R.string.kube_forward_title))
            .setContentText(getString(R.string.kube_forward_count, count))
            .setContentIntent(open)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
            .addAction(Notification.Action.Builder(null, getString(R.string.kube_forward_stop), stop).build())
            .build()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == STOP) {
            nativeStopForwards()
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return START_NOT_STICKY
        }
        val count = nativeForwardCount()
        // Even an expired request satisfies the foreground-start contract before
        // stopping; Android never gets an orphaned startForegroundService call.
        startForeground(NOTIFICATION, notification(count), ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        handler.removeCallbacks(refresh)
        handler.post(refresh)
        return START_NOT_STICKY
    }

    override fun onTimeout(startId: Int, fgsType: Int) {
        nativeStopForwards()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        nativeStopForwards()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onDestroy() {
        handler.removeCallbacks(refresh)
        nativeStopForwards()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    companion object {
        private const val CHANNEL = "kubernetes_forwards"
        private const val NOTIFICATION = 6202
        private const val STOP = "io.consolecrypt.consolecrypt.STOP_KUBE_FORWARDS"
    }
}
