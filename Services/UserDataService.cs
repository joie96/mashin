using mashin.Models;
using Microsoft.Extensions.Logging;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Text.Json;

namespace mashin.Services;

/// <summary>
/// Stores and synchronizes user-specific data (favorites and playlists)
/// via auth/me
/// </summary>
public sealed class UserDataService : INotifyPropertyChanged
{
    #region Constants and fields

    private const string FavoritesRootKey = "mashin.favorites";
    private const string PlaylistsRootKey = "mashin.playlists";
    private const string LocalPlaylistProvider = "mashin";

    private readonly MusicAssistantService _musicAssistant;
    private readonly SettingsService _settings;
    private readonly ILogger<UserDataService> _logger;
    private readonly SemaphoreSlim _lock = new(1, 1);

    private Dictionary<string, object> _preferences = new(StringComparer.OrdinalIgnoreCase);
    private bool _isLoadingPreferences;

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

    #region Public state

    public event PropertyChangedEventHandler? PropertyChanged;

    public bool IsLoadingPreferences
    {
        get => _isLoadingPreferences;
        private set => SetProperty(ref _isLoadingPreferences, value);
    }

    #endregion

    #region Loading and pushing preferences

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
                _ = LoadPlaylistsSnapshot();
                return CloneDictionary(_preferences);
            }

            _preferences = NormalizeDictionary(user.Preferences);
            _ = LoadFavoritesSnapshot();
            _ = LoadPlaylistsSnapshot();
            _logger.LogInformation("Loaded user data for {Username}", user.Username);
            return CloneDictionary(_preferences);
        }
        catch (Exception ex)
        {
            _preferences = new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase);
            _ = LoadFavoritesSnapshot();
            _ = LoadPlaylistsSnapshot();
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

            if (preferencesToPush.TryGetValue(PlaylistsRootKey, out var playlistsRootObj)
                && playlistsRootObj is PlaylistsSnapshot playlistsSnapshot)
            {
                try
                {
                    var playlistsJson = JsonSerializer.Serialize(playlistsSnapshot);
                    var playlistsDictionary = JsonSerializer.Deserialize<Dictionary<string, object>>(playlistsJson);
                    preferencesToPush[PlaylistsRootKey] = NormalizeDictionary(playlistsDictionary);
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "Failed to prepare playlists snapshot for push");
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
                _ = LoadFavoritesSnapshot();
                _ = LoadPlaylistsSnapshot();
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
                MediaType.Track => UpdateSnapshotList(snapshot.Tracks, uri, isFavorite, () => UserDataSnapshotMapper.ToTrackSnapshot(mediaItem as Track ?? new Track
                {
                    Uri = mediaItem.Uri ?? string.Empty,
                    ItemId = mediaItem.ItemId,
                    Provider = mediaItem.Provider,
                    Name = mediaItem.Name,
                    DisplayName = mediaItem.DisplayName,
                    Duration = 0
                })),
                MediaType.Album => UpdateSnapshotList(snapshot.Albums, uri, isFavorite, () => UserDataSnapshotMapper.ToAlbumSnapshot(mediaItem as Album ?? new Album
                {
                    Uri = mediaItem.Uri ?? string.Empty,
                    ItemId = mediaItem.ItemId,
                    Provider = mediaItem.Provider,
                    Name = mediaItem.Name,
                    DisplayName = mediaItem.DisplayName
                })),
                MediaType.Artist => UpdateSnapshotList(snapshot.Artists, uri, isFavorite, () => UserDataSnapshotMapper.ToArtistSnapshot(mediaItem as Artist ?? new Artist
                {
                    Uri = mediaItem.Uri ?? string.Empty,
                    ItemId = mediaItem.ItemId,
                    Provider = mediaItem.Provider,
                    Name = mediaItem.Name,
                    DisplayName = mediaItem.DisplayName
                })),
                MediaType.Playlist => UpdateSnapshotList(snapshot.Playlists, uri, isFavorite, () => UserDataSnapshotMapper.ToPlaylistSnapshot(mediaItem as Playlist ?? new Playlist
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

    #region Playlists

    public Task<PlaylistsSnapshot> GetPlaylistsAsync(CancellationToken cancellationToken = default)
    {
        return Task.FromResult(LoadPlaylistsSnapshot());
    }

    public async Task<Playlist> AddPlaylistAsync(Playlist playlist, CancellationToken cancellationToken = default)
    {
        if (playlist == null)
        {
            throw new ArgumentNullException(nameof(playlist));
        }

        var snapshot = LoadPlaylistsSnapshot();

        if (string.IsNullOrWhiteSpace(playlist.ItemId))
        {
            playlist.ItemId = GenerateNextLocalPlaylistId(snapshot);
        }

        if (string.IsNullOrWhiteSpace(playlist.Provider))
        {
            playlist.Provider = LocalPlaylistProvider;
        }

        var playlistSnapshot = UserDataSnapshotMapper.ToPlaylistSnapshot(playlist);
        snapshot.Playlists.Add(playlistSnapshot);

        var pushed = await PushPreferencesAsync(cancellationToken);
        if (!pushed)
        {
            _logger.LogWarning("Failed to push playlist add operation for {PlaylistName}.", playlist.Name);
        }

        return UserDataSnapshotMapper.ToPlaylist(playlistSnapshot);
    }

    public async Task<bool> UpdatePlaylistAsync(Playlist playlist, CancellationToken cancellationToken = default)
    {
        if (playlist == null || string.IsNullOrWhiteSpace(playlist.ItemId))
        {
            return false;
        }

        var snapshot = LoadPlaylistsSnapshot();

        var index = snapshot.Playlists.FindIndex(existing =>
            string.Equals(existing.ItemId, playlist.ItemId, StringComparison.OrdinalIgnoreCase));

        if (index < 0)
        {
            return false;
        }

        var replacement = UserDataSnapshotMapper.ToPlaylistSnapshot(playlist);
        replacement.Provider = string.IsNullOrWhiteSpace(replacement.Provider)
            ? LocalPlaylistProvider
            : replacement.Provider;

        snapshot.Playlists[index] = replacement;

        var pushed = await PushPreferencesAsync(cancellationToken);
        if (!pushed)
        {
            _logger.LogWarning("Failed to push playlist update for {PlaylistId}.", playlist.ItemId);
        }

        return pushed;
    }

    public async Task<bool> RemovePlaylistAsync(Playlist playlist, CancellationToken cancellationToken = default)
    {
        if (playlist == null || string.IsNullOrWhiteSpace(playlist.ItemId))
        {
            return false;
        }

        var snapshot = LoadPlaylistsSnapshot();

        var removed = snapshot.Playlists.RemoveAll(existing =>
            string.Equals(existing.ItemId, playlist.ItemId, StringComparison.OrdinalIgnoreCase));

        if (removed == 0)
        {
            return false;
        }

        var pushed = await PushPreferencesAsync(cancellationToken);
        if (!pushed)
        {
            _logger.LogWarning("Failed to push playlist removal for {PlaylistId}.", playlist.ItemId);
        }

        return pushed;
    }

    public async Task<bool> AddPlaylistTracksAsync(string playlistId, IEnumerable<Track> tracks, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(playlistId) || tracks == null)
        {
            return false;
        }

        var tracksToAdd = tracks
            .Where(track => track != null && !string.IsNullOrWhiteSpace(track.Uri))
            .ToList();

        if (tracksToAdd.Count == 0)
        {
            return false;
        }

        var snapshot = LoadPlaylistsSnapshot();
        var playlistSnapshot = snapshot.Playlists.FirstOrDefault(existing =>
            string.Equals(existing.ItemId, playlistId, StringComparison.OrdinalIgnoreCase));

        if (playlistSnapshot == null)
        {
            return false;
        }

        var localTracks = playlistSnapshot.Items?.ToList() ?? new List<TrackSnapshot>();
        foreach (var track in tracksToAdd)
        {
            localTracks.Add(UserDataSnapshotMapper.ToTrackSnapshot(track));
        }

        playlistSnapshot.Items = localTracks;

        var pushed = await PushPreferencesAsync(cancellationToken);
        if (!pushed)
        {
            _logger.LogWarning("Failed to push track add operation for playlist {PlaylistId}.", playlistId);
        }

        return pushed;
    }

    public async Task<bool> RemovePlaylistTracksAsync(string playlistId, IEnumerable<Track> tracks, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(playlistId) || tracks == null)
        {
            return false;
        }

        var urisToRemove = tracks
            .Select(track => track?.Uri)
            .Where(uri => !string.IsNullOrWhiteSpace(uri))
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        if (urisToRemove.Count == 0)
        {
            return false;
        }

        var snapshot = LoadPlaylistsSnapshot();
        var playlistSnapshot = snapshot.Playlists.FirstOrDefault(existing =>
            string.Equals(existing.ItemId, playlistId, StringComparison.OrdinalIgnoreCase));

        if (playlistSnapshot == null)
        {
            return false;
        }

        var localTracks = playlistSnapshot.Items?.ToList() ?? new List<TrackSnapshot>();
        var removed = localTracks.RemoveAll(track => !string.IsNullOrWhiteSpace(track.Uri)
            && urisToRemove.Contains(track.Uri));

        if (removed == 0)
        {
            return false;
        }

        playlistSnapshot.Items = localTracks;

        var pushed = await PushPreferencesAsync(cancellationToken);
        if (!pushed)
        {
            _logger.LogWarning("Failed to push track remove operation for playlist {PlaylistId}.", playlistId);
        }

        return pushed;
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
                TrackSnapshot track => track.Uri,
                AlbumSnapshot album => album.Uri,
                ArtistSnapshot artist => artist.Uri,
                PlaylistSnapshot playlist => playlist.Uri,
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

    #endregion

    #region Playlists snapshot helpers

    private PlaylistsSnapshot LoadPlaylistsSnapshot()
    {
        if (!_preferences.TryGetValue(PlaylistsRootKey, out var playlistsRootObj) || playlistsRootObj is null)
        {
            var created = new PlaylistsSnapshot();
            _preferences[PlaylistsRootKey] = created;
            return created;
        }

        if (playlistsRootObj is PlaylistsSnapshot snapshot)
        {
            return snapshot;
        }

        if (playlistsRootObj is Dictionary<string, object> dictionary)
        {
            try
            {
                var json = JsonSerializer.Serialize(dictionary);
                PlaylistsSnapshot? deserializedSnapshot = JsonSerializer.Deserialize<PlaylistsSnapshot>(json);
                if (deserializedSnapshot is PlaylistsSnapshot parsedSnapshot)
                {
                    _preferences[PlaylistsRootKey] = parsedSnapshot;
                    return parsedSnapshot;
                }
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Failed to parse playlists snapshot");
            }
        }

        var createdSnapshot = new PlaylistsSnapshot();
        _preferences[PlaylistsRootKey] = createdSnapshot;
        return createdSnapshot;
    }

    private static int ParseLocalPlaylistId(string? itemId)
    {
        if (string.IsNullOrWhiteSpace(itemId))
        {
            return 0;
        }

        return int.TryParse(itemId, out var parsedId)
            ? parsedId
            : 0;
    }

    private static string GenerateNextLocalPlaylistId(PlaylistsSnapshot snapshot)
    {
        var id = snapshot.Playlists
            .Select(playlist => ParseLocalPlaylistId(playlist.ItemId))
            .Where(parsedId => parsedId > 0)
            .DefaultIfEmpty(0)
            .Max() + 1;

        return id.ToString();
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

    #region Utility

    private bool SetProperty<T>(ref T field, T value, [CallerMemberName] string? propertyName = null)
    {
        if (EqualityComparer<T>.Default.Equals(field, value))
        {
            return false;
        }

        field = value;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
        return true;
    }

    #endregion
}
