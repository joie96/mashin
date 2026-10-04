package com.example.mashin_flutter.media.notification

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import androidx.core.app.NotificationCompat
import androidx.media3.common.C
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.CommandButton
import androidx.media3.session.DefaultMediaNotificationProvider
import androidx.media3.session.MediaNotification
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaStyleNotificationHelper
import com.example.mashin_flutter.R
import com.google.common.collect.ImmutableList

// Media notification presentation.
@UnstableApi
internal class PlaybackNotificationProvider(private val context: Context) : DefaultMediaNotificationProvider(context) {
    // Notification channel configuration.
    init {
        context.getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(DEFAULT_CHANNEL_ID,
                context.getString(R.string.media_playback_notification_channel_name),
                NotificationManager.IMPORTANCE_LOW),
        )
        setSmallIcon(R.drawable.ic_media_notification)
    }

    // Establish foreground eligibility before asynchronous Rust startup and audio focus.
    fun createStartupNotification(session: MediaSession): Notification = NotificationCompat.Builder(context, DEFAULT_CHANNEL_ID)
        .setSmallIcon(R.drawable.ic_media_notification)
        .setContentTitle(session.player.mediaMetadata.title ?: "Mashin")
        .setStyle(MediaStyleNotificationHelper.MediaStyle(session))
        .setOngoing(true)
        .setOnlyAlertOnce(true)
        .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
        .build()

    // Media notification presentation.
    override fun addNotificationActions(
        session: MediaSession,
        mediaButtons: ImmutableList<CommandButton>,
        builder: NotificationCompat.Builder,
        actionFactory: MediaNotification.ActionFactory,
    ): IntArray {
        val compactViewActionIndices = super.addNotificationActions(
            session,
            mediaButtons,
            builder,
            actionFactory,
        )
        val durationMillis = session.player.duration
        val positionMillis = session.player.currentPosition
        if (durationMillis > 0 && durationMillis != C.TIME_UNSET && positionMillis != C.TIME_UNSET) {
            builder.setProgress(
                durationMillis.coerceAtMost(Int.MAX_VALUE.toLong()).toInt(),
                positionMillis.coerceIn(0, durationMillis).toInt(),
                false,
            )
        }
        return compactViewActionIndices
    }
}

