using mashin.Models;
using Microsoft.Extensions.Logging;
using System.Text.Json;

namespace mashin.Services;

/// <summary>
/// Stores and synchronizes user-specific data (preferences and favorites) via auth/me and auth/user/update in music assistant.
/// </summary>
public sealed class UserDataService
{
    #region Constants and fields

    private const string FavoritesRootKey = "mashin.favorites";

    private readonly MusicAssistantService _musicAssistant;
    private readonly SettingsService _settings;
    private readonly ILogger<UserDataService> _logger;
    private readonly SemaphoreSlim _lock = new(1, 1);

    private Dictionary<string, object> _preferences = new(StringComparer.OrdinalIgnoreCase);

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

    #region Loadading and Pushing preferences

    public async Task<Dictionary<string, object>> LoadPreferencesAsync(CancellationToken cancellationToken = default)
    {
        await _lock.WaitAsync(cancellationToken);
        try
        {
            var user = await _musicAssistant.GetCurrentUserAsync();
            if (user == null)
            {
                _preferences = new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase);
                _ = LoadFavoritesSnapshot();
                return CloneDictionary(_preferences);
            }

            _preferences = NormalizeDictionary(user.Preferences);
            _ = LoadFavoritesSnapshot();
            _logger.LogInformation("Loaded user data for {Username}", user.Username);
            return CloneDictionary(_preferences);
        }
        catch (Exception ex)
        {
            _preferences = new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase);
            _ = LoadFavoritesSnapshot();
            _logger.LogWarning(ex, "Failed to load user data");
            return CloneDictionary(_preferences);
        }
        finally
        {
            _lock.Release();
        }
    }

    public async Task<bool> PushPreferencesAsync(CancellationToken cancellationToken = default)
    {
        await _lock.WaitAsync(cancellationToken);
        try
        {
            var configuredUsername = _settings.Username;
            if (string.IsNullOrWhiteSpace(configuredUsername))
            {
                return false;
            }

            var preferencesToPush = CloneDictionary(_preferences);
            if (preferencesToPush.TryGetValue(FavoritesRootKey, out var favoritesRootObj)
                && favoritesRootObj is FavoritesSnapshot favoritesSnapshot)
            {
                try
                {
                    var favoritesJson = JsonSerializer.Serialize(favoritesSnapshot);
                    var favoritesDictionary = JsonSerializer.Deserialize<Dictionary<string, object>>(favoritesJson);
                    preferencesToPush[FavoritesRootKey] = NormalizeDictionary(favoritesDictionary);
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "Failed to prepare favorites snapshot for push");
                    return false;
                }
            }

            try
            {
                var updatedUser = await _musicAssistant.UpdateUserAsync(
                    username: configuredUsername,
                    preferences: preferencesToPush);

                if (updatedUser == null)
                {
                    return false;
                }

                _preferences = NormalizeDictionary(updatedUser.Preferences);
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
        finally
        {
            _lock.Release();
        }
    }

    #endregion

    #region Favorites

    public async Task<bool> IsFavoriteAsync(MediaItem mediaItem, CancellationToken cancellationToken = default)
    {
        if (mediaItem == null || string.IsNullOrWhiteSpace(mediaItem.Uri))
        {
            return false;
        }

        var snapshot = LoadFavoritesSnapshot();

        return mediaItem.MediaType switch
        {
            MediaType.Track => snapshot.Tracks.Any(item => string.Equals(item.Uri, mediaItem.Uri, StringComparison.OrdinalIgnoreCase)),
            MediaType.Album => snapshot.Albums.Any(item => string.Equals(item.Uri, mediaItem.Uri, StringComparison.OrdinalIgnoreCase)),
            MediaType.Artist => snapshot.Artists.Any(item => string.Equals(item.Uri, mediaItem.Uri, StringComparison.OrdinalIgnoreCase)),
            MediaType.Playlist => snapshot.Playlists.Any(item => string.Equals(item.Uri, mediaItem.Uri, StringComparison.OrdinalIgnoreCase)),
            _ => false
        };
    }

    public async Task<FavoritesSnapshot> GetFavoritesAsync(CancellationToken cancellationToken = default)
    {
        return LoadFavoritesSnapshot();
    }

    public async Task SetFavoriteAsync(IEnumerable<MediaItem> mediaItems, bool isFavorite, CancellationToken cancellationToken = default)
    {
        if (mediaItems == null)
        {
            _logger.LogWarning("Skipping favorite update because media items list is null.");
            return;
        }

        var snapshot = LoadFavoritesSnapshot();
        var skippedInvalid = 0;
        var skippedUnsupported = 0;

        foreach (var mediaItem in mediaItems)
        {
            cancellationToken.ThrowIfCancellationRequested();

            if (mediaItem == null || string.IsNullOrWhiteSpace(mediaItem.Uri))
            {
                skippedInvalid++;
                continue;
            }

            var uri = mediaItem.Uri;

            var updated = mediaItem.MediaType switch
            {
                MediaType.Track => UpdateSnapshotList(snapshot.Tracks, uri, isFavorite, () => CreateTrackSnapshot(mediaItem as Track ?? new Track
                {
                    Uri = mediaItem.Uri ?? string.Empty,
                    ItemId = mediaItem.ItemId,
                    Provider = mediaItem.Provider,
                    Name = mediaItem.Name,
                    DisplayName = mediaItem.DisplayName,
                    Duration = 0
                })),
                MediaType.Album => UpdateSnapshotList(snapshot.Albums, uri, isFavorite, () => CreateAlbumSnapshot(mediaItem as Album ?? new Album
                {
                    Uri = mediaItem.Uri ?? string.Empty,
                    ItemId = mediaItem.ItemId,
                    Provider = mediaItem.Provider,
                    Name = mediaItem.Name,
                    DisplayName = mediaItem.DisplayName
                })),
                MediaType.Artist => UpdateSnapshotList(snapshot.Artists, uri, isFavorite, () => CreateArtistSnapshot(mediaItem as Artist ?? new Artist
                {
                    Uri = mediaItem.Uri ?? string.Empty,
                    ItemId = mediaItem.ItemId,
                    Provider = mediaItem.Provider,
                    Name = mediaItem.Name,
                    DisplayName = mediaItem.DisplayName
                })),
                MediaType.Playlist => UpdateSnapshotList(snapshot.Playlists, uri, isFavorite, () => CreatePlaylistSnapshot(mediaItem as Playlist ?? new Playlist
                {
                    Uri = mediaItem.Uri ?? string.Empty,
                    ItemId = mediaItem.ItemId,
                    Provider = mediaItem.Provider,
                    Name = mediaItem.Name,
                    DisplayName = mediaItem.DisplayName
                })),
                _ => (bool?)null
            };

            if (!updated.HasValue)
            {
                skippedUnsupported++;
                continue;
            }

            if (!updated.Value)
            {
                _logger.LogDebug(
                    "Favorite snapshot unchanged for media item {MediaUri} with target state {IsFavorite}.",
                    uri,
                    isFavorite);
            }

            mediaItem.Favorite = isFavorite;
        }

        if (skippedInvalid > 0)
        {
            _logger.LogWarning("Skipped favorite updates for {Count} items because media item or uri was invalid.", skippedInvalid);
        }

        if (skippedUnsupported > 0)
        {
            _logger.LogWarning("Skipped favorite updates for {Count} items because media type was not supported.", skippedUnsupported);
        }

        var pushed = await PushPreferencesAsync(cancellationToken);
        if (!pushed)
        {
            _logger.LogWarning("Failed to push updated favorites preferences to server.");
        }
    }

    #endregion

    #region Favorites snapshot helpers

    private FavoritesSnapshot LoadFavoritesSnapshot()
    {
        if (!_preferences.TryGetValue(FavoritesRootKey, out var favRootObj) || favRootObj is null)
        {
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

        var createdSnapshot = new FavoritesSnapshot();
        _preferences[FavoritesRootKey] = createdSnapshot;
        return createdSnapshot;
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
            ImagePath = track.PrimaryImage?.Path,
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
                ImagePath = track.Album.PrimaryImage?.Path,
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
            ImagePath = album.PrimaryImage?.Path,
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
            ImagePath = artist.PrimaryImage?.Path,
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
            ImagePath = playlist.PrimaryImage?.Path,
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

}