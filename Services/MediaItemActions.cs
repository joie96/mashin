using mashin.Models;
using Microsoft.Extensions.Logging;
using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;

namespace mashin.Services;

#region Interfaces

/// <summary>
/// Interface for common actions on MediaItems that can be used in different views (TableView, RowView, etc.).
/// </summary>
public interface IMediaItemActions
{
    Task PlayMediaAsync(object item, object? startItem = null);
    Task PlayMediaNextAsync(object item, object? startItem = null);
    Task PlayMediaLastAsync(object item, object? startItem = null);
    Task ShufflePlayMediaAsync(IEnumerable<MediaItem> items);
    Task ShufflePlayMediaAsync(MediaItem parentItem, IEnumerable<MediaItem> associatedItems);
    Task AddToPlaylistAsync(object item, Playlist playlist);
    Task RemoveFromPlaylistAsync(object item, Playlist playlist);
    Task AddToFavoritesAsync(object item);
    Task RemoveFromFavoritesAsync(object item);
    Task UpdatePlaylistAsync(Playlist playlist);
    Task RemovePlaylistAsync(Playlist playlist);
    Task ClearQueueAsync(string queueId, bool skipStop = false);
    Task PlayIndexAsync(string queueId, int index);
    Task DeleteQueueItemAsync(string queueId, int itemIndex);
    Task DeleteQueueItemAsync(string queueId, string itemId);
    Task MoveQueueItemAsync(string queueId, string queueItemId, int posShift = 0);
    Task SetDontStopTheMusicAsync(string queueId, bool dontStopTheMusicEnabled);

}

#endregion

/// <summary>
/// Provides common actions for MediaItems that can be used in different views (TableView, RowView, etc.).
/// </summary>
public class MediaItemActions : IMediaItemActions
{
    #region Fields

    private readonly MusicAssistantService _musicAssistant;
    private readonly IUserDataService _userDataService;
    private readonly IPlaybackService _playbackService;
    private readonly ISendspinPlayerService _sendspinPlayerService;
    private readonly ILogger<MediaItemActions> _logger;

    #endregion

    #region Constructor

    public MediaItemActions(
        MusicAssistantService musicAssistant,
        IUserDataService userDataService,
        IPlaybackService playbackService,
        ISendspinPlayerService sendspinPlayerService,
        ILogger<MediaItemActions> logger)
    {
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _playbackService = playbackService;
        _sendspinPlayerService = sendspinPlayerService;
        _logger = logger;
    }

    #endregion

    #region Media Item Actions

    /// <summary>
    /// Plays the specified media item(s), replacing the current queue.
    /// </summary>
    public async Task PlayMediaAsync(object item, object? startItem = null)
    {
        var mediaItems = GetMediaItemsFromParameter(item);
        if (mediaItems.Count == 0)
        {
            _logger.LogWarning("No items selected to play");
            return;
        }

        var activePlayerId = _playbackService.ActivePlayerId;
        if (string.IsNullOrWhiteSpace(activePlayerId))
        {
            _logger.LogWarning("No active player available. Player connection is missing.");
            return;
        }

        if (!await _sendspinPlayerService.EnsureConnectedAsync(activePlayerId))
        {
            _logger.LogWarning("Play aborted: local Sendspin connection is not available");
            return;
        }

        _logger.LogInformation("Playing {Count} item(s)", mediaItems.Count);
        _playbackService.PlaybackState = new PlaybackStateModel(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);

        try
        {
            await _musicAssistant.PlayMediaAsync(
                activePlayerId,
                mediaItems,
                QueueOption.Replace,
                startItem: startItem);
            await _playbackService.RefreshNowAsync();
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to play media");
        }
    }

    /// <summary>
    /// Plays the specified media item(s) next in the queue.
    /// </summary>
    public async Task PlayMediaNextAsync(object item, object? startItem = null)
    {
        var mediaItems = GetMediaItemsFromParameter(item);
        if (mediaItems.Count == 0)
        {
            _logger.LogWarning("No items selected for 'Play Next'");
            return;
        }

        var activePlayerId = _playbackService.ActivePlayerId;
        if (string.IsNullOrWhiteSpace(activePlayerId))
        {
            _logger.LogWarning("No active player available. Player connection is missing.");
            return;
        }

        if (!await _sendspinPlayerService.EnsureConnectedAsync(activePlayerId))
        {
            _logger.LogWarning("Play next aborted: local Sendspin connection is not available");
            return;
        }

        _logger.LogInformation("Playing {Count} item(s) next", mediaItems.Count);

        try
        {
            await _musicAssistant.PlayMediaAsync(
                activePlayerId,
                mediaItems,
                QueueOption.Next,
                startItem: startItem);
            await _playbackService.RefreshNowAsync();
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to play media next");
        }
    }

    /// <summary>
    /// Adds the specified media item(s) to the end of the queue.
    /// </summary>
    public async Task PlayMediaLastAsync(object item, object? startItem = null)
    {
        var mediaItems = GetMediaItemsFromParameter(item);
        if (mediaItems.Count == 0)
        {
            _logger.LogWarning("No items selected for 'Play Last'");
            return;
        }

        var activePlayerId = _playbackService.ActivePlayerId;
        if (string.IsNullOrWhiteSpace(activePlayerId))
        {
            _logger.LogWarning("No active player available. Player connection is missing.");
            return;
        }

        if (!await _sendspinPlayerService.EnsureConnectedAsync(activePlayerId))
        {
            _logger.LogWarning("Play last aborted: local Sendspin connection is not available");
            return;
        }

        _logger.LogInformation("Playing {Count} item(s) last", mediaItems.Count);

        try
        {
            await _musicAssistant.PlayMediaAsync(
                activePlayerId,
                mediaItems,
                QueueOption.Add,
                startItem: startItem);
            await _playbackService.RefreshNowAsync();
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to add media to queue");
        }
    }

    /// <summary>
    /// Shuffles a standalone collection, starts with one random item,
    /// then queues the remaining shuffled items next.
    /// </summary>
    public async Task ShufflePlayMediaAsync(IEnumerable<MediaItem> items)
    {
        var mediaItems = items?
            .Where(mediaItem => mediaItem != null)
            .ToList() ?? new List<MediaItem>();

        if (mediaItems.Count == 0)
        {
            _logger.LogWarning("No items available to shuffle play");
            return;
        }

        for (var i = mediaItems.Count - 1; i > 0; i--)
        {
            var j = Random.Shared.Next(i + 1);
            (mediaItems[i], mediaItems[j]) = (mediaItems[j], mediaItems[i]);
        }

        await PlayMediaAsync(mediaItems[0]);

        var remainingItems = mediaItems.Skip(1).ToList();
        if (remainingItems.Count > 0)
        {
            await PlayMediaNextAsync(remainingItems);
        }
    }

    /// <summary>
    /// Plays one random associated item immediately, queues the parent item next,
    /// then shuffles queue entries from position 2.
    /// </summary>
    public async Task ShufflePlayMediaAsync(MediaItem parentItem, IEnumerable<MediaItem> associatedItems)
    {
        var associatedMediaItems = associatedItems?
            .Where(mediaItem => mediaItem != null)
            .ToList() ?? new List<MediaItem>();

        if (associatedMediaItems.Count == 0)
        {
            _logger.LogWarning("No associated items available to shuffle play");
            return;
        }

        var activePlayerId = _playbackService.ActivePlayerId;
        if (string.IsNullOrWhiteSpace(activePlayerId))
        {
            _logger.LogWarning("No active player available. Player connection is missing.");
            return;
        }

        if (!await _sendspinPlayerService.EnsureConnectedAsync(activePlayerId))
        {
            _logger.LogWarning("Shuffle play aborted: local Sendspin connection is not available");
            return;
        }

        var queueId = activePlayerId;

        try
        {
            if (parentItem == null)
            {
                _logger.LogWarning("No parent item provided for shuffle play");
                return;
            }

            var randomTrackIndex = Random.Shared.Next(associatedMediaItems.Count);
            var randomTrack = associatedMediaItems[randomTrackIndex];

            // Start immediately with one random track.
            await PlayMediaAsync(randomTrack);

            // Queue the full parent item next (playlist/album/etc.).
            await PlayMediaNextAsync(parentItem);

            // Remove the duplicate track that was re-added by parentItem
            var currentQueueItem = _playbackService.CurrentPlayerQueue?.CurrentItem;
            var currentQueueItemId = currentQueueItem?.QueueItemId;
            var currentTrackItemId = currentQueueItem?.MediaItem?.ItemId;
            var currentTrackUri = currentQueueItem?.MediaItem?.Uri;

            var queueItemsAfterAppend = _playbackService.CurrentQueueItems.ToList();

            var duplicateQueueItemId = queueItemsAfterAppend
                .Where(queueItem => !string.IsNullOrWhiteSpace(queueItem.QueueItemId))
                .Where(queueItem => !string.Equals(queueItem.QueueItemId, currentQueueItemId, StringComparison.Ordinal))
                .FirstOrDefault(queueItem =>
                    (!string.IsNullOrWhiteSpace(currentTrackItemId)
                        && string.Equals(queueItem.MediaItem?.ItemId, currentTrackItemId, StringComparison.Ordinal))
                    || (!string.IsNullOrWhiteSpace(currentTrackUri)
                        && string.Equals(queueItem.MediaItem?.Uri, currentTrackUri, StringComparison.Ordinal)))
                ?.QueueItemId;

            if (!string.IsNullOrWhiteSpace(duplicateQueueItemId))
            {
                await _musicAssistant.DeleteQueueItemAsync(queueId, duplicateQueueItemId);
                await _playbackService.RefreshNowAsync();
            }

            var queueItems = _playbackService.CurrentQueueItems.ToList();
            if (queueItems.Count == 0)
            {
                // Queue refresh succeeded but no items arrived; abort to avoid invalid move operations.
                _logger.LogWarning("Queue is empty after shuffle preparation");
                return;
            }

            var queueItemOrder = queueItems
                .Select(queueItem => queueItem.QueueItemId)
                .Where(queueItemId => !string.IsNullOrWhiteSpace(queueItemId))
                .ToList();

            // Shuffle from queue position 2 (index 1), keep first queued item stable.
            const int firstShuffleIndex = 1;

            if (queueItemOrder.Count > firstShuffleIndex + 1)
            {
                const int moveBatchSize = 10;
                const int moveBatchPauseMs = 250;

                var prefix = queueItemOrder.Take(firstShuffleIndex).ToList();
                var shuffledIds = queueItemOrder.Skip(firstShuffleIndex).ToList();

                for (var i = shuffledIds.Count - 1; i > 0; i--)
                {
                    var j = Random.Shared.Next(i + 1);
                    (shuffledIds[i], shuffledIds[j]) = (shuffledIds[j], shuffledIds[i]);
                }

                shuffledIds = prefix.Concat(shuffledIds).ToList();

                var movesSincePause = 0;

                for (var targetPosition = firstShuffleIndex; targetPosition < shuffledIds.Count; targetPosition++)
                {
                    var queueItemId = shuffledIds[targetPosition];
                    var currentPosition = queueItemOrder.IndexOf(queueItemId);
                    if (currentPosition < 0)
                    {
                        continue;
                    }

                    var positionShift = targetPosition - currentPosition;
                    if (positionShift == 0)
                    {
                        continue;
                    }

                    // Apply delta moves so we preserve order tracking without rebuilding the queue.
                    await _musicAssistant.MoveQueueItemAsync(queueId, queueItemId, positionShift);
                    movesSincePause++;

                    if (movesSincePause >= moveBatchSize)
                    {
                        // Throttle bursts of queue mutations to reduce backend pressure on large lists.
                        await Task.Delay(moveBatchPauseMs);
                        movesSincePause = 0;
                    }

                    queueItemOrder.RemoveAt(currentPosition);
                    queueItemOrder.Insert(targetPosition, queueItemId);
                }

                await _playbackService.RefreshNowAsync();
            }

            // No final play_index call: playback should continue on the already started random track.
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to shuffle play media");
        }
    }

    /// <summary>
    /// Adds the currently selected items to the specified playlist.
    /// </summary>
    public async Task AddToPlaylistAsync(object item, Playlist playlist)
    {
        try
        {
            var mediaItems = GetMediaItemsFromParameter(item);

            if (mediaItems.Count == 0)
            {
                _logger.LogInformation("No items selected to add to playlist");
                return;
            }

            _logger.LogInformation("Adding {Count} items to playlist: {PlaylistName}",
                mediaItems.Count, playlist.Name);

            var uris = mediaItems
                .Where(i => !string.IsNullOrEmpty(i.Uri))
                .Select(i => i.Uri!)
                .ToList();

            if (!uris.Any())
            {
                _logger.LogWarning("No valid URIs found in selected items");
                return;
            }

            await _musicAssistant.AddPlaylistTracksAsync(playlist.ItemId, uris);

            _logger.LogInformation("Successfully added {Count} items to playlist: {PlaylistName}",
                uris.Count, playlist.Name);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to add items to playlist");
        }
    }

    /// <summary>
    /// Removes the specified media item(s) from a playlist.
    /// </summary>
    public async Task RemoveFromPlaylistAsync(object item, Playlist playlist)
    {
        try
        {
            var mediaItems = GetMediaItemsFromParameter(item);

            if (mediaItems.Count == 0)
            {
                _logger.LogInformation("No items selected to remove from playlist");
                return;
            }

            _logger.LogInformation("Removing {Count} items from playlist: {PlaylistName}",
                mediaItems.Count, playlist.Name);

            // Positionen extrahieren
            var positions = mediaItems
                .Where(i => i is Track track && track.Index > 0)
                .Cast<Track>()
                .Select(t => t.Index)
                .OrderByDescending(p => p)
                .ToList();

            if (!positions.Any())
            {
                _logger.LogWarning("No valid positions found in selected items");
                return;
            }

            await _musicAssistant.RemovePlaylistTracksAsync(playlist.ItemId, positions);

            _logger.LogInformation("Successfully removed {Count} items from playlist: {PlaylistName}",
                positions.Count, playlist.Name);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to remove items from playlist");
        }
    }

    /// <summary>
    /// Adds the specified media item(s) to favorites.
    /// </summary>
    public async Task AddToFavoritesAsync(object item)
    {
        var mediaItems = GetMediaItemsFromParameter(item);
        if (mediaItems.Count == 0)
        {
            _logger.LogWarning("No items selected for favorites");
            return;
        }

        _logger.LogInformation("Adding {Count} item(s) to favorites", mediaItems.Count);

        foreach (var mediaItem in mediaItems)
        {
            if (!string.IsNullOrEmpty(mediaItem.Uri))
            {
                mediaItem.Favorite = true;
            }
        }

        try
        {
            var success = await _userDataService.SetFavoritesAsync(mediaItems, true);
            if (!success)
            {
                _logger.LogWarning("Failed to persist favorite changes to user preferences");
            }
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to add items to favorites");
        }
    }

    /// <summary>
    /// Removes the specified media item(s) from favorites.
    /// </summary>
    public async Task RemoveFromFavoritesAsync(object item)
    {
        var mediaItems = GetMediaItemsFromParameter(item);
        if (mediaItems.Count == 0)
        {
            _logger.LogWarning("No items selected to remove from favorites");
            return;
        }

        _logger.LogInformation("Removing {Count} item(s) from favorites", mediaItems.Count);

        foreach (var mediaItem in mediaItems)
        {
            mediaItem.Favorite = false;
        }

        try
        {
            var success = await _userDataService.SetFavoritesAsync(mediaItems, false);
            if (!success)
            {
                _logger.LogWarning("Failed to persist favorite removals to user preferences");
            }
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to remove items from favorites");
        }
    }

    /// <summary>
    /// Updates a playlist in the library.
    /// </summary>
    public async Task UpdatePlaylistAsync(Playlist playlist)
    {
        if (playlist == null)
        {
            _logger.LogWarning("No playlist provided for update");
            return;
        }

        try
        {
            _logger.LogInformation("Updating playlist: {PlaylistName}", playlist.Name);
            await _musicAssistant.UpdatePlaylistAsync(playlist.ItemId, playlist, true);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to update playlist: {PlaylistName}", playlist.Name);
        }
    }

    /// <summary>
    /// Removes a playlist from the library.
    /// </summary>
    public async Task RemovePlaylistAsync(Playlist playlist)
    {
        if (playlist == null)
        {
            _logger.LogWarning("No playlist provided for removal");
            return;
        }

        try
        {
            _logger.LogInformation("Removing playlist: {PlaylistName}", playlist.Name);
            await _musicAssistant.RemovePlaylistAsync(playlist.ItemId);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to remove playlist: {PlaylistName}", playlist.Name);
        }
    }

    #endregion

    #region Queue Item Actions

    /// <summary>
    /// Clears all items in the queue.
    /// </summary>
    public async Task ClearQueueAsync(string queueId, bool skipStop = false)
    {
        if (string.IsNullOrWhiteSpace(queueId))
        {
            _logger.LogWarning("QueueId is required to clear queue");
            return;
        }

        try
        {
            await _musicAssistant.ClearQueueAsync(queueId, skipStop);
            await _playbackService.RefreshNowAsync();
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to clear queue: {QueueId}", queueId);
        }
    }

    /// <summary>
    /// Plays item at index in the queue.
    /// </summary>
    public async Task PlayIndexAsync(string queueId, int index)
    {
        if (string.IsNullOrWhiteSpace(queueId))
        {
            _logger.LogWarning("QueueId is required to play queue index");
            return;
        }

        try
        {
            var activePlayerId = _playbackService.ActivePlayerId;
            if (!await _sendspinPlayerService.EnsureConnectedAsync(activePlayerId))
            {
                _logger.LogWarning("Play index aborted: local Sendspin connection is not available");
                return;
            }

            _playbackService.PlaybackState = new PlaybackStateModel(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);
            await _musicAssistant.PlayIndexAsync(queueId, index);
            await _playbackService.RefreshNowAsync();
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to play index {Index} on queue: {QueueId}", index, queueId);
        }
    }

    /// <summary>
    /// Deletes an item by index from the queue.
    /// </summary>
    public async Task DeleteQueueItemAsync(string queueId, int itemIndex)
    {
        if (string.IsNullOrWhiteSpace(queueId))
        {
            _logger.LogWarning("QueueId is required to delete queue item");
            return;
        }

        if (itemIndex < 0)
        {
            _logger.LogWarning("Queue item index must be >= 0 to delete queue item");
            return;
        }

        try
        {
            await _musicAssistant.DeleteQueueItemAsync(queueId, itemIndex);
            await _playbackService.RefreshNowAsync();
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to delete queue item index {QueueItemIndex} from queue: {QueueId}", itemIndex, queueId);
        }
    }

    /// <summary>
    /// Deletes an item by queue item id from the queue.
    /// </summary>
    public async Task DeleteQueueItemAsync(string queueId, string itemId)
    {
        if (string.IsNullOrWhiteSpace(queueId))
        {
            _logger.LogWarning("QueueId is required to delete queue item");
            return;
        }

        if (string.IsNullOrWhiteSpace(itemId))
        {
            _logger.LogWarning("Item id is required to delete queue item");
            return;
        }

        try
        {
            await _musicAssistant.DeleteQueueItemAsync(queueId, itemId);
            await _playbackService.RefreshNowAsync();
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to delete queue item {QueueItem} from queue: {QueueId}", itemId, queueId);
        }
    }

    /// <summary>
    /// Moves a queue item up/down by a position shift.
    /// </summary>
    public async Task MoveQueueItemAsync(string queueId, string queueItemId, int posShift = 0)
    {
        if (string.IsNullOrWhiteSpace(queueId))
        {
            _logger.LogWarning("QueueId is required to move queue item");
            return;
        }

        if (string.IsNullOrWhiteSpace(queueItemId))
        {
            _logger.LogWarning("Queue item id is required to move queue item");
            return;
        }

        try
        {
            await _musicAssistant.MoveQueueItemAsync(queueId, queueItemId, posShift);
            await _playbackService.RefreshNowAsync();
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to move queue item {QueueItemId} on queue: {QueueId}", queueItemId, queueId);
        }
    }

    /// <summary>
    /// Configures "Don't stop the music" setting on the queue.
    /// </summary>
    public async Task SetDontStopTheMusicAsync(string queueId, bool dontStopTheMusicEnabled)
    {
        if (string.IsNullOrWhiteSpace(queueId))
        {
            _logger.LogWarning("QueueId is required to configure don't stop the music");
            return;
        }

        try
        {
            await _musicAssistant.SetDontStopTheMusicAsync(queueId, dontStopTheMusicEnabled);
            await _playbackService.RefreshNowAsync();
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to set don't stop the music on queue: {QueueId}", queueId);
        }
    }

    #endregion

    #region Helpers

    /// <summary>
    /// Converts a parameter to a list of MediaItems.
    /// Supports single MediaItem or IList of MediaItems.
    /// </summary>
    private List<MediaItem> GetMediaItemsFromParameter(object? param)
    {
        if (param is System.Collections.IList list)
        {
            return list.OfType<MediaItem>().ToList();
        }
        else if (param is IEnumerable<MediaItem> enumerable)
        {
            return enumerable.ToList();
        }
        else if (param is MediaItem item)
        {
            return new List<MediaItem> { item };
        }

        return new List<MediaItem>();
    }

    #endregion
}