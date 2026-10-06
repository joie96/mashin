package com.example.mashin_flutter.media.session

import android.os.Bundle
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.CommandButton
import androidx.media3.session.SessionCommand
import org.json.JSONArray
import org.json.JSONObject

/** Projects Rust-owned action state for both notification and automotive controllers. */
@UnstableApi
internal object PlaybackSessionActions {
    val toggleCurrentItemFavorite = SessionCommand("mashin.toggleCurrentItemFavorite", Bundle.EMPTY)
    val toggleShuffle = SessionCommand("mashin.toggleShuffle", Bundle.EMPTY)
    val cycleRepeatMode = SessionCommand("mashin.cycleRepeatMode", Bundle.EMPTY)
    val commands = listOf(toggleCurrentItemFavorite, toggleShuffle, cycleRepeatMode)

    // Controller commands are restricted to current-item and transport actions.
    fun request(command: SessionCommand, snapshot: PlaybackSnapshot): PlaybackRequest? = when (command.customAction) {
        toggleCurrentItemFavorite.customAction -> {
            val mediaItem = requireNotNull(snapshot.serializedCurrentMediaItem) { "No current media item to favorite" }
            PlaybackRequest(PlaybackCommandKind.SetUserDataFavorites, JSONObject()
                .put("mediaItems", JSONArray().put(JSONObject(mediaItem)))
                .put("favorite", !snapshot.isCurrentItemFavorite).toString())
        }
        toggleShuffle.customAction -> PlaybackRequest(PlaybackCommandKind.TransportShuffle,
            JSONObject().put("enabled", !snapshot.isShuffleEnabled).toString())
        cycleRepeatMode.customAction -> PlaybackRequest(PlaybackCommandKind.TransportRepeatMode,
            JSONObject().put("mode", when (snapshot.repeatMode) {
                Player.REPEAT_MODE_OFF -> Player.REPEAT_MODE_ALL
                Player.REPEAT_MODE_ALL -> Player.REPEAT_MODE_ONE
                else -> Player.REPEAT_MODE_OFF
            }).toString())
        else -> null
    }

    // Secondary slots preserve previous/next; overflow remains available on smaller surfaces.
    fun buttons(snapshot: PlaybackSnapshot): List<CommandButton> = listOf(
        CommandButton.Builder(if (snapshot.isCurrentItemFavorite) CommandButton.ICON_HEART_FILLED else CommandButton.ICON_HEART_UNFILLED)
            .setSessionCommand(toggleCurrentItemFavorite)
            .setDisplayName(if (snapshot.isCurrentItemFavorite) "Favorit entfernen" else "Favorit hinzuf\u00fcgen")
            .setEnabled(snapshot.serializedCurrentMediaItem != null)
            .setSlots(CommandButton.SLOT_BACK_SECONDARY, CommandButton.SLOT_OVERFLOW)
            .build(),
        CommandButton.Builder(if (snapshot.isShuffleEnabled) CommandButton.ICON_SHUFFLE_ON else CommandButton.ICON_SHUFFLE_OFF)
            .setSessionCommand(toggleShuffle)
            .setDisplayName(if (snapshot.isShuffleEnabled) "Zufallsmodus ausschalten" else "Zufallsmodus einschalten")
            .setSlots(CommandButton.SLOT_FORWARD_SECONDARY, CommandButton.SLOT_OVERFLOW)
            .build(),
        CommandButton.Builder(when (snapshot.repeatMode) {
            Player.REPEAT_MODE_ALL -> CommandButton.ICON_REPEAT_ALL
            Player.REPEAT_MODE_ONE -> CommandButton.ICON_REPEAT_ONE
            else -> CommandButton.ICON_REPEAT_OFF
        })
            .setSessionCommand(cycleRepeatMode)
            .setDisplayName(when (snapshot.repeatMode) {
                Player.REPEAT_MODE_ALL -> "Wiederholung: Alle"
                Player.REPEAT_MODE_ONE -> "Wiederholung: Ein Titel"
                else -> "Wiederholung: Aus"
            })
            .setSlots(CommandButton.SLOT_OVERFLOW)
            .build(),
    )
}