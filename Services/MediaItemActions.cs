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
    Task AddToPlaylistAsync(object item, Playlist playlist);
    Task RemoveFromPlaylistAsync(object item, Playlist playlist);
    Task AddToFavoritesAsync(object item);
    Task RemoveFromFavoritesAsync(object item);
    Task UpdatePlaylistAsync(Playlist playlist);
    Task RemovePlaylistAsync(Playlist playlist);
}

#endregion

/// <summary>
/// Provides common actions for MediaItems that can be used in different views (TableView, RowView, etc.).
/// </summary>
public class MediaItemActions : IMediaItemActions
{
    #region Fields

    private readonly IPlaylistService _playlistService;
    private readonly IUserDataService _userDataService;
    private readonly ILogger<MediaItemActions> _logger;

    #endregion

    #region Constructor

    public MediaItemActions(
        IPlaylistService playlistService,
        IUserDataService userDataService,
        ILogger<MediaItemActions> logger)
    {
        _playlistService = playlistService;
        _userDataService = userDataService;
        _logger = logger;
    }

    #endregion

    #region Media Item Actions

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
                _logger.LogDebug("No items selected to add to playlist");
                return;
            }

            _logger.LogDebug("Adding {Count} items to playlist: {PlaylistName}",
                mediaItems.Count, playlist.Name);

            var uris = mediaItems
                .Where(i => !string.IsNullOrEmpty(i.Uri))
                .Select(i => i.Uri!)
                .ToList();

            if (!uris.Any())
            {
                _logger.LogDebug("No valid URIs found in selected items");
                return;
            }

            await _playlistService.AddTracksAsync(playlist, uris);

            _logger.LogDebug("Successfully added {Count} items to playlist: {PlaylistName}",
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
                _logger.LogDebug("No items selected to remove from playlist");
                return;
            }

            _logger.LogDebug("Removing {Count} items from playlist: {PlaylistName}",
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
                _logger.LogDebug("No valid positions found in selected items");
                return;
            }

            await _playlistService.RemoveTracksAsync(playlist, positions);

            _logger.LogDebug("Successfully removed {Count} items from playlist: {PlaylistName}",
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
            _logger.LogDebug("No items selected for favorites");
            return;
        }

        _logger.LogDebug("Adding {Count} item(s) to favorites", mediaItems.Count);

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
            _logger.LogDebug("No items selected to remove from favorites");
            return;
        }

        _logger.LogDebug("Removing {Count} item(s) from favorites", mediaItems.Count);

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
            _logger.LogDebug("Updating playlist: {PlaylistName}", playlist.Name);
            await _playlistService.UpdatePlaylistAsync(playlist);
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
            _logger.LogDebug("Removing playlist: {PlaylistName}", playlist.Name);
            await _playlistService.RemovePlaylistAsync(playlist);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to remove playlist: {PlaylistName}", playlist.Name);
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