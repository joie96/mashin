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

    Task<Dictionary<string, object>> GetPreferencesAsync(bool forceRefresh = false, CancellationToken cancellationToken = default);
    T? GetPreference<T>(string key);
    Task<bool> SetPreferenceAsync(string key, object? value, CancellationToken cancellationToken = default);

    bool IsFavorite(MediaItem mediaItem);
    Task<FavoritesSnapshot?> GetFavoritesSnapshotAsync(CancellationToken cancellationToken = default);
    Task<bool> SetFavoriteAsync(MediaItem mediaItem, bool isFavorite, CancellationToken cancellationToken = default);
    Task<bool> SetFavoritesAsync(IEnumerable<MediaItem> mediaItems, bool isFavorite, CancellationToken cancellationToken = default);
}

public sealed class UserDataService : IUserDataService
{
    #region Constants and fields

    private const string FavoritesRootKey = "mashin.favorites";

    private readonly MusicAssistantService _musicAssistant;
    private readonly SettingsService _settings;
    private readonly ILogger<UserDataService> _logger;
    private readonly SemaphoreSlim _lock = new(1, 1);

    private Dictionary<string, object> _preferences = new(StringComparer.OrdinalIgnoreCase);

    public bool IsLoaded { get; private set; }
    public AuthUser? CurrentUser { get; private set; }

    #endregion

    #region Construction

    public UserDataService(
        MusicAssistantService musicAssistant,
        SettingsService settings,
        ILogger<UserDataService> logger)
    {
        _musicAssistant = musicAssistant;
        _settings = settings;
        _logger = logger;
    }

    #endregion

    #region Load and preferences

    private async Task<bool> EnsureLoadedInternalAsync(bool forceRefresh = false, CancellationToken cancellationToken = default)
    {
        await _lock.WaitAsync(cancellationToken);
        try
        {
            var configuredUsername = _settings.Username;

            if (IsLoaded && !forceRefresh)
            {
                if (string.IsNullOrWhiteSpace(configuredUsername)
                    || string.Equals(CurrentUser?.Username, configuredUsername, StringComparison.OrdinalIgnoreCase))
                {
                    return true;
                }

                _logger.LogInformation(
                    "Detected user switch from {CurrentUser} to {ConfiguredUser}, reloading user data",
                    CurrentUser?.Username,
                    configuredUsername);
            }

            var user = await _musicAssistant.GetCurrentUserAsync();
            if (user == null)
            {
                CurrentUser = null;
                _preferences = new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase);
                IsLoaded = false;
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
            CurrentUser = null;
            _preferences = new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase);
            IsLoaded = false;
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
        await EnsureLoadedInternalAsync(forceRefresh, cancellationToken);

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

        if (!IsLoaded && !EnsureLoadedInternalSync())
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

        if (!await EnsureLoadedInternalAsync(false, cancellationToken))
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
        if (mediaItem == null || string.IsNullOrWhiteSpace(mediaItem.Uri))
        {
            return false;
        }

        if (!IsLoaded && !EnsureLoadedInternalSync())
        {
            return false;
        }

        var snapshot = LoadFavoritesSnapshot(createIfMissing: false);
        if (snapshot == null)
        {
            return false;
        }

        return IsSnapshotFavorite(snapshot, mediaItem.MediaType, mediaItem.Uri!);
    }

    public async Task<FavoritesSnapshot?> GetFavoritesSnapshotAsync(CancellationToken cancellationToken = default)
    {
        if (!await EnsureLoadedInternalAsync(false, cancellationToken))
        {
            return null;
        }

        await _lock.WaitAsync(cancellationToken);
        try
        {
            return LoadFavoritesSnapshot(createIfMissing: false);
        }
        finally
        {
            _lock.Release();
        }
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
        if (!await EnsureLoadedInternalAsync(false, cancellationToken))
        {
            return false;
        }

        await _lock.WaitAsync(cancellationToken);
        try
        {
            var changed = false;

            var snapshot = LoadFavoritesSnapshot(createIfMissing: true);
            if (snapshot == null)
            {
                return false;
            }

            foreach (var item in mediaItems.Where(i => i != null && !string.IsNullOrWhiteSpace(i.Uri)))
            {
                var updated = UpdateSnapshot(snapshot, item, isFavorite);
                if (updated)
                {
                    changed = true;
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

    #region Favorites snapshot helpers

    private FavoritesSnapshot? LoadFavoritesSnapshot(bool createIfMissing)
    {
        if (!_preferences.TryGetValue(FavoritesRootKey, out var favRootObj) || favRootObj is null)
        {
            if (!createIfMissing)
            {
                return null;
            }

            var created = new FavoritesSnapshot();
            _preferences[FavoritesRootKey] = created;
            return created;
        }

        if (favRootObj is FavoritesSnapshot snapshot)
        {
            return snapshot;
        }

        if (favRootObj is Dictionary<string, object> dictionary)
        {
            try
            {
                var json = JsonSerializer.Serialize(dictionary);
                FavoritesSnapshot? deserializedSnapshot = JsonSerializer.Deserialize<FavoritesSnapshot>(json);
                if (deserializedSnapshot is FavoritesSnapshot parsedSnapshot)
                {
                    _preferences[FavoritesRootKey] = parsedSnapshot;
                    return parsedSnapshot;
                }
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Failed to parse favorites snapshot");
            }
        }

        if (!createIfMissing)
        {
            return null;
        }

        var createdSnapshot = new FavoritesSnapshot();
        _preferences[FavoritesRootKey] = createdSnapshot;
        return createdSnapshot;
    }

    private static bool IsSnapshotFavorite(FavoritesSnapshot snapshot, MediaType mediaType, string uri)
    {
        return mediaType switch
        {
            MediaType.Track => snapshot.Tracks.Any(item => string.Equals(item.Uri, uri, StringComparison.OrdinalIgnoreCase)),
            MediaType.Album => snapshot.Albums.Any(item => string.Equals(item.Uri, uri, StringComparison.OrdinalIgnoreCase)),
            MediaType.Artist => snapshot.Artists.Any(item => string.Equals(item.Uri, uri, StringComparison.OrdinalIgnoreCase)),
            MediaType.Playlist => snapshot.Playlists.Any(item => string.Equals(item.Uri, uri, StringComparison.OrdinalIgnoreCase)),
            _ => false
        };
    }

    private static bool UpdateSnapshot(FavoritesSnapshot snapshot, MediaItem item, bool isFavorite)
    {
        var uri = item.Uri!;

        return item.MediaType switch
        {
            MediaType.Track => UpdateSnapshotList(snapshot.Tracks, uri, isFavorite, () => CreateTrackSnapshot(item as Track ?? new Track
            {
                Uri = item.Uri ?? string.Empty,
                ItemId = item.ItemId,
                Provider = item.Provider,
                Name = item.Name,
                DisplayName = item.DisplayName,
                Duration = 0
            })),
            MediaType.Album => UpdateSnapshotList(snapshot.Albums, uri, isFavorite, () => CreateAlbumSnapshot(item as Album ?? new Album
            {
                Uri = item.Uri ?? string.Empty,
                ItemId = item.ItemId,
                Provider = item.Provider,
                Name = item.Name,
                DisplayName = item.DisplayName
            })),
            MediaType.Artist => UpdateSnapshotList(snapshot.Artists, uri, isFavorite, () => CreateArtistSnapshot(item as Artist ?? new Artist
            {
                Uri = item.Uri ?? string.Empty,
                ItemId = item.ItemId,
                Provider = item.Provider,
                Name = item.Name,
                DisplayName = item.DisplayName
            })),
            MediaType.Playlist => UpdateSnapshotList(snapshot.Playlists, uri, isFavorite, () => CreatePlaylistSnapshot(item as Playlist ?? new Playlist
            {
                Uri = item.Uri ?? string.Empty,
                ItemId = item.ItemId,
                Provider = item.Provider,
                Name = item.Name,
                DisplayName = item.DisplayName
            })),
            _ => false
        };
    }

    private static bool UpdateSnapshotList<T>(ICollection<T> list, string uri, bool isFavorite, Func<T> createSnapshot)
        where T : class
    {
        var existing = list.FirstOrDefault(item => string.Equals(
            item switch
            {
                FavoriteTrackSnapshot track => track.Uri,
                FavoriteAlbumSnapshot album => album.Uri,
                FavoriteArtistSnapshot artist => artist.Uri,
                FavoritePlaylistSnapshot playlist => playlist.Uri,
                _ => null
            },
            uri,
            StringComparison.OrdinalIgnoreCase));

        if (isFavorite)
        {
            if (existing != null)
            {
                return false;
            }

            list.Add(createSnapshot());
            return true;
        }

        if (existing == null)
        {
            return false;
        }

        return list.Remove(existing);
    }

    private static FavoriteTrackSnapshot CreateTrackSnapshot(Track track)
    {
        var snapshot = new FavoriteTrackSnapshot
        {
            Uri = track.Uri ?? string.Empty,
            ItemId = track.ItemId,
            Provider = track.Provider,
            Name = track.Name,
            DisplayName = track.DisplayName,
            Duration = track.Duration,
            ImageUrl = track.ImageUrl,
            ProviderMappings = track.ProviderMappings
                .Select(CloneProviderMapping)
                .ToList()
        };

        if (track.Album != null)
        {
            snapshot.Album = new FavoriteAlbumRef
            {
                ItemId = track.Album.ItemId,
                Provider = track.Album.Provider,
                Name = track.Album.Name,
                Year = track.Album.Year,
                ImageUrl = track.Album.ImageUrl,
                ProviderMappings = track.Album.ProviderMappings
                    .Select(CloneProviderMapping)
                    .ToList()
            };
        }

        if (track.Artists != null)
        {
            foreach (var artist in track.Artists)
            {
                snapshot.Artists.Add(new FavoriteArtistRef
                {
                    ItemId = artist.ItemId,
                    Provider = artist.Provider,
                    Name = artist.Name,
                    ProviderMappings = artist.ProviderMappings
                        .Select(CloneProviderMapping)
                        .ToList()
                });
            }
        }

        return snapshot;
    }

    private static FavoriteAlbumSnapshot CreateAlbumSnapshot(Album album)
    {
        var snapshot = new FavoriteAlbumSnapshot
        {
            Uri = album.Uri ?? string.Empty,
            ItemId = album.ItemId,
            Provider = album.Provider,
            Name = album.Name,
            DisplayName = album.DisplayName,
            Year = album.Year,
            ImageUrl = album.ImageUrl,
            ProviderMappings = album.ProviderMappings
                .Select(CloneProviderMapping)
                .ToList()
        };

        if (album.Artists != null)
        {
            foreach (var artist in album.Artists)
            {
                snapshot.Artists.Add(new FavoriteArtistRef
                {
                    ItemId = artist.ItemId,
                    Provider = artist.Provider,
                    Name = artist.Name,
                    ProviderMappings = artist.ProviderMappings
                        .Select(CloneProviderMapping)
                        .ToList()
                });
            }
        }

        return snapshot;
    }

    private static FavoriteArtistSnapshot CreateArtistSnapshot(Artist artist)
    {
        return new FavoriteArtistSnapshot
        {
            Uri = artist.Uri ?? string.Empty,
            ItemId = artist.ItemId,
            Provider = artist.Provider,
            Name = artist.Name,
            DisplayName = artist.DisplayName,
            ImageUrl = artist.ImageUrl,
            ProviderMappings = artist.ProviderMappings
                .Select(CloneProviderMapping)
                .ToList()
        };
    }

    private static FavoritePlaylistSnapshot CreatePlaylistSnapshot(Playlist playlist)
    {
        return new FavoritePlaylistSnapshot
        {
            Uri = playlist.Uri ?? string.Empty,
            ItemId = playlist.ItemId,
            Provider = playlist.Provider,
            Name = playlist.Name,
            DisplayName = playlist.DisplayName,
            ImageUrl = playlist.ImageUrl,
            ProviderMappings = playlist.ProviderMappings
                .Select(CloneProviderMapping)
                .ToList()
        };
    }

    private static ProviderMapping CloneProviderMapping(ProviderMapping mapping)
    {
        return new ProviderMapping
        {
            ItemId = mapping.ItemId,
            ProviderDomain = mapping.ProviderDomain,
            ProviderInstance = mapping.ProviderInstance,
            Available = mapping.Available,
            Url = mapping.Url
        };
    }

    #endregion

    #region Preferences normalization

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

    #region Internal sync

    private bool EnsureLoadedInternalSync(bool forceRefresh = false)
    {
        try
        {
            return EnsureLoadedInternalAsync(forceRefresh).ConfigureAwait(false).GetAwaiter().GetResult();
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to synchronously ensure user data is loaded");
            return false;
        }
    }

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
}