package com.joie96.mashin.media.session

import android.os.Bundle
import androidx.media3.common.Player
import androidx.media3.session.CommandButton
import androidx.media3.session.SessionCommand
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class PlaybackSessionActionsTest {
    private val mediaItem = """{"media_type":"track","item_id":"track-1","provider":"library","uri":"library://track/track-1","name":"Track"}"""

    // State-aware icons and canonical command arguments.
    @Test
    fun buttonsReflectFavoriteShuffleAndEveryRepeatMode() {
        val snapshot = PlaybackSnapshot.empty().copy(serializedCurrentMediaItem = mediaItem)
        val buttons = PlaybackSessionActions.buttons(snapshot)
        assertEquals(PlaybackSessionActions.commands, buttons.map { it.sessionCommand })
        assertEquals(listOf(CommandButton.ICON_HEART_UNFILLED, CommandButton.ICON_SHUFFLE_OFF, CommandButton.ICON_REPEAT_OFF),
            buttons.map { it.icon })
        assertTrue(buttons.all { it.isEnabled && it.iconResId != 0 && it.slots.contains(CommandButton.SLOT_OVERFLOW) })
        val enabled = PlaybackSessionActions.buttons(snapshot.copy(isCurrentItemFavorite = true,
            isShuffleEnabled = true, repeatMode = Player.REPEAT_MODE_ALL))
        assertEquals(listOf(CommandButton.ICON_HEART_FILLED, CommandButton.ICON_SHUFFLE_ON, CommandButton.ICON_REPEAT_ALL),
            enabled.map { it.icon })
        assertEquals("Favorit entfernen", enabled.first().displayName)
        assertEquals(CommandButton.ICON_REPEAT_ONE,
            PlaybackSessionActions.buttons(snapshot.copy(repeatMode = Player.REPEAT_MODE_ONE)).last().icon)
        assertFalse(PlaybackSessionActions.buttons(PlaybackSnapshot.empty()).first().isEnabled)
    }

    @Test
    fun favoriteAndShuffleRequestsToggleTheCanonicalState() {
        for (isEnabled in listOf(false, true)) {
            val snapshot = PlaybackSnapshot.empty().copy(serializedCurrentMediaItem = mediaItem,
                isCurrentItemFavorite = isEnabled, isShuffleEnabled = isEnabled)
            val favorite = PlaybackSessionActions.request(PlaybackSessionActions.toggleCurrentItemFavorite, snapshot)!!
            assertEquals(PlaybackCommandKind.SetUserDataFavorites, favorite.kind)
            val arguments = JSONObject(favorite.arguments)
            assertEquals(!isEnabled, arguments.getBoolean("favorite"))
            assertEquals("track-1", arguments.getJSONArray("mediaItems").getJSONObject(0).getString("item_id"))
            val shuffle = PlaybackSessionActions.request(PlaybackSessionActions.toggleShuffle, snapshot)!!
            assertEquals(PlaybackCommandKind.TransportShuffle, shuffle.kind)
            assertEquals(!isEnabled, JSONObject(shuffle.arguments).getBoolean("enabled"))
        }
        assertNull(PlaybackSessionActions.request(SessionCommand("unknown", Bundle.EMPTY), PlaybackSnapshot.empty()))
    }

    @Test
    fun pendingFavoriteRequestsToggleImmediatelyAndPreserveTheLatestWish() {
        val snapshot = PlaybackSnapshot.empty().copy(serializedCurrentMediaItem = mediaItem)
        val first = PlaybackSessionActions.request(PlaybackSessionActions.toggleCurrentItemFavorite, snapshot)!!
        val added = PlaybackSessionActions.projectFavoriteRequests(snapshot, listOf(first))
        assertTrue(added.isCurrentItemFavorite)
        assertFalse(snapshot.isCurrentItemFavorite)
        assertEquals(CommandButton.ICON_HEART_FILLED, PlaybackSessionActions.buttons(added).first().icon)
        val second = PlaybackSessionActions.request(PlaybackSessionActions.toggleCurrentItemFavorite, added)!!
        assertFalse(JSONObject(second.arguments).getBoolean("favorite"))
        val removed = PlaybackSessionActions.projectFavoriteRequests(snapshot, listOf(first, second))
        assertFalse(removed.isCurrentItemFavorite)
        assertFalse(PlaybackSessionActions.projectFavoriteRequests(snapshot.copy(isCurrentItemFavorite = true), listOf(second)).isCurrentItemFavorite)
        assertFalse(PlaybackSessionActions.projectFavoriteRequests(snapshot, emptyList()).isCurrentItemFavorite)
        assertEquals(CommandButton.ICON_HEART_UNFILLED, PlaybackSessionActions.buttons(removed).first().icon)
    }

    @Test
    fun pendingFavoriteRequestsFollowAliasesButNeverAnotherTrack() {
        val snapshot = PlaybackSnapshot.empty().copy(serializedCurrentMediaItem = mediaItem)
        val alias = JSONObject(mediaItem).put("provider_mappings", org.json.JSONArray()
            .put(JSONObject().put("provider_instance", "source-a").put("item_id", "remote")))
        val request = PlaybackRequest(PlaybackCommandKind.SetUserDataFavorites,
            """{"mediaItems":[{"media_type":"track","provider":"source-a","item_id":"remote"}],"favorite":true}""")
        assertTrue(PlaybackSessionActions.projectFavoriteRequests(snapshot.copy(serializedCurrentMediaItem = alias.toString()), listOf(request)).isCurrentItemFavorite)
        assertFalse(PlaybackSessionActions.projectFavoriteRequests(snapshot, listOf(request)).isCurrentItemFavorite)
        val invalid = request.copy(arguments = "not-json")
        assertEquals(snapshot, PlaybackSessionActions.projectFavoriteRequests(snapshot, listOf(invalid)))
    }

    @Test
    fun repeatCyclesOffAllOneOff() {
        for ((currentMode, nextMode) in listOf(Player.REPEAT_MODE_OFF to Player.REPEAT_MODE_ALL,
            Player.REPEAT_MODE_ALL to Player.REPEAT_MODE_ONE, Player.REPEAT_MODE_ONE to Player.REPEAT_MODE_OFF)) {
            val request = PlaybackSessionActions.request(PlaybackSessionActions.cycleRepeatMode,
                PlaybackSnapshot.empty().copy(repeatMode = currentMode))!!
            assertEquals(PlaybackCommandKind.TransportRepeatMode, request.kind)
            assertEquals(nextMode, JSONObject(request.arguments).getInt("mode"))
        }
    }

    @Test(expected = IllegalArgumentException::class)
    fun favoriteWithoutCurrentItemIsRejected() {
        PlaybackSessionActions.request(PlaybackSessionActions.toggleCurrentItemFavorite, PlaybackSnapshot.empty())
    }
}