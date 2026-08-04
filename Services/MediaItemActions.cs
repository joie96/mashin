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
    Task AddToFavoritesAsync(object item);
    Task RemoveFromFavoritesAsync(object item);
}

#endregion

/// <summary>
/// Provides common actions for MediaItems that can be used in different views (TableView, RowView, etc.).
/// </summary>
public class MediaItemActions : IMediaItemActions
{
    #region Fields

    private readonly IUserDataService _userDataService;
    private readonly ILogger<MediaItemActions> _logger;

    #endregion

    #region Constructor

    public MediaItemActions(
        IUserDataService userDataService,
        ILogger<MediaItemActions> logger)
    {
        _userDataService = userDataService;
        _logger = logger;
    }

    #endregion

    #region Media Item Actions

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