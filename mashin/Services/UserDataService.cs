using mashin.Models;
using Microsoft.Extensions.Logging;
using System.Text.Json;

namespace mashin.Services;

/// <summary>
/// Stores and synchronizes user-specific data (preferences and favorites) via auth/me and auth/user/update in music assistant.
/// </summary>
public interface IUserDataService
{
    bool IsLoaded { get; }
    AuthUser? CurrentUser { get; }

    Task<bool> EnsureLoadedAsync(bool forceRefresh = false, CancellationToken cancellationToken = default);
    Task<Dictionary<string, object>> GetPreferencesAsync(bool forceRefresh = false, CancellationToken cancellationToken = default);
    T? GetPreference<T>(string key);
    Task<bool> SetPreferenceAsync(string key, object? value, CancellationToken cancellationToken = default);

    bool IsFavorite(MediaItem mediaItem);
    Task<bool> SetFavoriteAsync(MediaItem mediaItem, bool isFavorite, CancellationToken cancellationToken = default);
    Task<bool> SetFavoritesAsync(IEnumerable<MediaItem> mediaItems, bool isFavorite, CancellationToken cancellationToken = default);
}

public sealed class UserDataService : IUserDataService
{
    #region Constants and fields

    private const string FavoritesRootKey = "mashin.favorites";

    private readonly MusicAssistantService _musicAssistant;
    private readonly ILogger<UserDataService> _logger;
    private readonly SemaphoreSlim _lock = new(1, 1);

    private Dictionary<string, object> _preferences = new(StringComparer.OrdinalIgnoreCase);

    public bool IsLoaded { get; private set; }
    public AuthUser? CurrentUser { get; private set; }

    #endregion

    #region Construction

    public UserDataService(
        MusicAssistantService musicAssistant,
        ILogger<UserDataService> logger)
    {
        _musicAssistant = musicAssistant;
        _logger = logger;
    }

    #endregion

    #region Load and preferences

    public async Task<bool> EnsureLoadedAsync(bool forceRefresh = false, CancellationToken cancellationToken = default)
    {
        await _lock.WaitAsync(cancellationToken);
        try
        {
            if (IsLoaded && !forceRefresh)
            {
                return true;
            }

            var user = await _musicAssistant.GetCurrentUserAsync();
            if (user == null)
            {
                return false;
            }

            CurrentUser = user;
            _preferences = NormalizeDictionary(user.Preferences);
            IsLoaded = true;
            _logger.LogInformation("Loaded user data for {Username}", user.Username);
            return true;
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load user data");
            return false;
        }
        finally
        {
            _lock.Release();
        }
    }

    public async Task<Dictionary<string, object>> GetPreferencesAsync(bool forceRefresh = false, CancellationToken cancellationToken = default)
    {
        await EnsureLoadedAsync(forceRefresh, cancellationToken);

        await _lock.WaitAsync(cancellationToken);
        try
        {
            return CloneDictionary(_preferences);
        }
        finally
        {
            _lock.Release();
        }
    }

    public T? GetPreference<T>(string key)
    {
        if (string.IsNullOrWhiteSpace(key))
        {
            return default;
        }

        if (!_preferences.TryGetValue(key, out var value) || value is null)
        {
            return default;
        }

        if (value is T typedValue)
        {
            return typedValue;
        }

        try
        {
            var json = JsonSerializer.Serialize(value);
            return JsonSerializer.Deserialize<T>(json);
        }
        catch
        {
            return default;
        }
    }

    public async Task<bool> SetPreferenceAsync(string key, object? value, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(key))
        {
            return false;
        }

        if (!await EnsureLoadedAsync(false, cancellationToken))
        {
            return false;
        }

        await _lock.WaitAsync(cancellationToken);
        try
        {
            if (value is null)
            {
                _preferences.Remove(key);
            }
            else
            {
                _preferences[key] = NormalizeValue(value)!;
            }

            return await SaveCoreAsync(cancellationToken);
        }
        finally
        {
            _lock.Release();
        }
    }

    #endregion

    #region Favorites

    public bool IsFavorite(MediaItem mediaItem)
    {
        if (!IsLoaded || mediaItem == null || string.IsNullOrWhiteSpace(mediaItem.Uri))
        {
            return false;
        }

        var favorites = GetFavoriteList(mediaItem.MediaType, createIfMissing: false);
        return favorites != null && favorites.Contains(mediaItem.Uri, StringComparer.OrdinalIgnoreCase);
    }

    public async Task<bool> SetFavoriteAsync(MediaItem mediaItem, bool isFavorite, CancellationToken cancellationToken = default)
    {
        if (mediaItem == null)
        {
            return false;
        }

        return await SetFavoritesAsync(new[] { mediaItem }, isFavorite, cancellationToken);
    }

    public async Task<bool> SetFavoritesAsync(IEnumerable<MediaItem> mediaItems, bool isFavorite, CancellationToken cancellationToken = default)
    {
        if (!await EnsureLoadedAsync(false, cancellationToken))
        {
            return false;
        }

        await _lock.WaitAsync(cancellationToken);
        try
        {
            var changed = false;

            foreach (var item in mediaItems.Where(i => i != null && !string.IsNullOrWhiteSpace(i.Uri)))
            {
                var favorites = GetFavoriteList(item.MediaType, createIfMissing: true)!;
                var uri = item.Uri!;

                if (isFavorite)
                {
                    if (!favorites.Contains(uri, StringComparer.OrdinalIgnoreCase))
                    {
                        favorites.Add(uri);
                        changed = true;
                    }
                }
                else
                {
                    var removed = favorites.RemoveAll(existing => string.Equals(existing, uri, StringComparison.OrdinalIgnoreCase));
                    if (removed > 0)
                    {
                        changed = true;
                    }
                }

                item.Favorite = isFavorite;
            }

            if (!changed)
            {
                return true;
            }

            return await SaveCoreAsync(cancellationToken);
        }
        finally
        {
            _lock.Release();
        }
    }

    #endregion

    #region Internal sync

    private async Task<bool> SaveCoreAsync(CancellationToken cancellationToken)
    {
        if (CurrentUser?.UserId == null)
        {
            return false;
        }

        try
        {
            var updatedUser = await _musicAssistant.UpdateUserAsync(
                userId: CurrentUser.UserId,
                preferences: CloneDictionary(_preferences));

            if (updatedUser == null)
            {
                return false;
            }

            CurrentUser = updatedUser;
            _preferences = NormalizeDictionary(updatedUser.Preferences);
            IsLoaded = true;
            return true;
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to save user preferences");
            return false;
        }
    }

    #endregion

    #region Internal mapping

    private List<string>? GetFavoriteList(MediaType mediaType, bool createIfMissing)
    {
        if (!_preferences.TryGetValue(FavoritesRootKey, out var favRootObj) || favRootObj is not Dictionary<string, object> favRoot)
        {
            if (!createIfMissing)
            {
                return null;
            }

            favRoot = new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase);
            _preferences[FavoritesRootKey] = favRoot;
        }

        var typeKey = GetFavoritesTypeKey(mediaType);
        if (!favRoot.TryGetValue(typeKey, out var listObj))
        {
            if (!createIfMissing)
            {
                return null;
            }

            var newList = new List<string>();
            favRoot[typeKey] = newList;
            return newList;
        }

        var normalized = listObj switch
        {
            List<string> s => s,
            IEnumerable<object> objects => objects
                .Select(item => item?.ToString())
                .Where(value => !string.IsNullOrWhiteSpace(value))
                .Select(value => value!)
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .ToList(),
            _ => new List<string>()
        };

        favRoot[typeKey] = normalized;
        return normalized;
    }

    private static string GetFavoritesTypeKey(MediaType mediaType)
    {
        return mediaType switch
        {
            MediaType.Track => "tracks",
            MediaType.Album => "albums",
            MediaType.Artist => "artists",
            MediaType.Playlist => "playlists",
            MediaType.Radio => "radios",
            MediaType.Podcast => "podcasts",
            MediaType.PodcastEpisode => "podcast_episodes",
            MediaType.Audiobook => "audiobooks",
            MediaType.Genre => "genres",
            MediaType.Folder => "folders",
            _ => "items"
        };
    }

    private static Dictionary<string, object> NormalizeDictionary(Dictionary<string, object>? source)
    {
        var normalized = new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase);

        if (source == null)
        {
            return normalized;
        }

        foreach (var (key, value) in source)
        {
            if (string.IsNullOrWhiteSpace(key))
            {
                continue;
            }

            var normalizedValue = NormalizeValue(value);
            if (normalizedValue != null)
            {
                normalized[key] = normalizedValue;
            }
        }

        return normalized;
    }

    private static object? NormalizeValue(object? value)
    {
        return value switch
        {
            null => null,
            JsonElement element => ConvertJsonElement(element),
            Dictionary<string, object> dictionary => NormalizeDictionary(dictionary),
            IDictionary<string, object> dictionary => NormalizeDictionary(dictionary.ToDictionary(kvp => kvp.Key, kvp => kvp.Value)),
            IEnumerable<object> list => list.Select(NormalizeValue).Where(item => item != null).Cast<object>().ToList(),
            string or bool or int or long or float or double or decimal => value,
            _ => value
        };
    }

    private static object? ConvertJsonElement(JsonElement element)
    {
        return element.ValueKind switch
        {
            JsonValueKind.Object => element
                .EnumerateObject()
                .ToDictionary(
                    property => property.Name,
                    property => ConvertJsonElement(property.Value) ?? string.Empty,
                    StringComparer.OrdinalIgnoreCase),
            JsonValueKind.Array => element
                .EnumerateArray()
                .Select(ConvertJsonElement)
                .Where(item => item != null)
                .Cast<object>()
                .ToList(),
            JsonValueKind.String => element.GetString(),
            JsonValueKind.Number => element.TryGetInt64(out var longValue)
                ? longValue
                : element.TryGetDouble(out var doubleValue)
                    ? doubleValue
                    : element.GetDecimal(),
            JsonValueKind.True => true,
            JsonValueKind.False => false,
            _ => null
        };
    }

    #endregion

    #region Clone helpers

    private static Dictionary<string, object> CloneDictionary(Dictionary<string, object> source)
    {
        var clone = new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase);

        foreach (var (key, value) in source)
        {
            var clonedValue = CloneValue(value);
            if (clonedValue != null)
            {
                clone[key] = clonedValue;
            }
        }

        return clone;
    }

    private static object? CloneValue(object? value)
    {
        return value switch
        {
            null => null,
            Dictionary<string, object> dictionary => CloneDictionary(dictionary),
            IEnumerable<object> list => list.Select(CloneValue).Where(item => item != null).Cast<object>().ToList(),
            _ => value
        };
    }

    #endregion
}