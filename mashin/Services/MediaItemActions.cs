using mashin.Models;
using Microsoft.Extensions.Logging;
using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;

namespace mashin.Services;

/// <summary>
/// Interface for common actions on MediaItems that can be used in different views (TableView, RowView, etc.).
/// </summary>
public interface IMediaItemActions
{
    Task PlayMediaAsync(object item);
    Task PlayMediaNextAsync(object item);
    Task PlayMediaLastAsync(object item);
    Task AddToPlaylistAsync(object item, Playlist playlist);
    Task RemoveFromPlaylistAsync(object item, Playlist playlist);
    Task AddToFavoritesAsync(object item);
    Task RemoveFromFavoritesAsync(object item);

}

/// <summary>
/// Provides common actions for MediaItems that can be used in different views (TableView, RowView, etc.).
/// </summary>
public class MediaItemActions : IMediaItemActions
{
    private readonly MusicAssistantService _musicAssistant;
    private readonly IPlayerService _playerService;
    private readonly IUserDataService _userDataService;
    private readonly ILogger<MediaItemActions> _logger;

    public MediaItemActions(
        MusicAssistantService musicAssistant,
        IPlayerService playerService,
        IUserDataService userDataService,
        ILogger<MediaItemActions> logger)
    {
        _musicAssistant = musicAssistant;
        _playerService = playerService;
        _userDataService = userDataService;
        _logger = logger;
    }

       /// <summary>
    /// Plays the specified media item(s), replacing the current queue.
    /// </summary>
    public async Task PlayMediaAsync(object item)
    {
        var mediaItems = GetMediaItemsFromParameter(item);
        if (mediaItems.Count == 0)
        {
            _logger.LogWarning("No items selected to play");
            return;
        }

        if (string.IsNullOrEmpty(_playerService.ClientId))
        {
            _logger.LogWarning("ClientId is not available. Player connection is missing.");
            return;
        }

        _logger.LogInformation("Playing {Count} item(s)", mediaItems.Count);

        try
        {
            await _musicAssistant.PlayMediaAsync(
                _playerService.ClientId,
                mediaItems,
                QueueOption.Replace);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to play media");
        }
    }

    /// <summary>
    /// Plays the specified media item(s) next in the queue.
    /// </summary>
    public async Task PlayMediaNextAsync(object item)
    {
        var mediaItems = GetMediaItemsFromParameter(item);
        if (mediaItems.Count == 0)
        {
            _logger.LogWarning("No items selected for 'Play Next'");
            return;
        }

        if (string.IsNullOrEmpty(_playerService.ClientId))
        {
            _logger.LogWarning("ClientId is not available. Player connection is missing.");
            return;
        }

        _logger.LogInformation("Playing {Count} item(s) next", mediaItems.Count);

        try
        {
            await _musicAssistant.PlayMediaAsync(
                _playerService.ClientId,
                mediaItems,
                QueueOption.Next);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to play media next");
        }
    }

    /// <summary>
    /// Adds the specified media item(s) to the end of the queue.
    /// </summary>
    public async Task PlayMediaLastAsync(object item)
    {
        var mediaItems = GetMediaItemsFromParameter(item);
        if (mediaItems.Count == 0)
        {
            _logger.LogWarning("No items selected for 'Play Last'");
            return;
        }

        if (string.IsNullOrEmpty(_playerService.ClientId))
        {
            _logger.LogWarning("ClientId is not available. Player connection is missing.");
            return;
        }

        _logger.LogInformation("Playing {Count} item(s) last", mediaItems.Count);

        try
        {
            await _musicAssistant.PlayMediaAsync(
                _playerService.ClientId,
                mediaItems,
                QueueOption.Add);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to add media to queue");
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
}