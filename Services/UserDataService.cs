using mashin.Collections;
using mashin.Models;
using Microsoft.Extensions.Logging;
using SixLabors.ImageSharp;
using SixLabors.ImageSharp.Formats.Jpeg;
using SixLabors.ImageSharp.Processing;
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
    private const int PlaylistCollageSizePx = 500;
    private const int PlaylistCollageTileCount = 4;
    private const int PlaylistCollageJpegQuality = 85;

    private readonly MusicAssistantService _musicAssistant;
    private readonly SettingsService _settings;
    private readonly ILogger<UserDataService> _logger;
    private readonly SemaphoreSlim _lock = new(1, 1);
    private readonly object _pushQueueGate = new();
    private static readonly HttpClient _httpClient = new();

    private Dictionary<string, object> _preferences = new(StringComparer.OrdinalIgnoreCase);
    private readonly ObservableRangeCollection<Track> _favoriteTracks = new();
    private readonly ObservableRangeCollection<Album> _favoriteAlbums = new();
    private readonly ObservableRangeCollection<Playlist> _favoritePlaylists = new();
    private readonly ObservableRangeCollection<Artist> _favoriteArtists = new();
    private readonly ObservableRangeCollection<Playlist> _playlists = new();
    private bool _isLoadingPreferences;
    private bool _isPushInProgress;
    // Single-slot queue: newer pending snapshot replaces older pending snapshot.
    private Dictionary<string, object>? _pendingPushPreferences;

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

    public ObservableRangeCollection<Track> FavoriteTracks => _favoriteTracks;

    public ObservableRangeCollection<Album> FavoriteAlbums => _favoriteAlbums;

    public ObservableRangeCollection<Playlist> FavoritePlaylists => _favoritePlaylists;

    public ObservableRangeCollection<Artist> FavoriteArtists => _favoriteArtists;

    public ObservableRangeCollection<Playlist> Playlists => _playlists;

    #endregion

    #region Loading and pushing preferences

    private void QueuePreferencesPush()
    {
        _ = Task.Run(async () =>
        {
            try
            {
                var pushed = await PushPreferencesAsync(CancellationToken.None).ConfigureAwait(false);
                if (!pushed)
                {
                    _logger.LogWarning("Background push of user preferences failed.");
                }
            }
            catch (Exception ex)
            {
                _logger.LogError(ex, "Background push of user preferences crashed.");
            }
        });
    }

    public async Task<Dictionary<string, object>> LoadPreferencesAsync(CancellationToken cancellationToken = default)
    {
        IsLoadingPreferences = true;
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
            IsLoadingPreferences = false;
        }
    }

    public async Task<bool> PushPreferencesAsync(CancellationToken cancellationToken = default)
    {
        var configuredUsername = _settings.Username;
        if (string.IsNullOrWhiteSpace(configuredUsername))
        {
            return false;
        }

        Dictionary<string, object> preferencesToPush;

        await _lock.WaitAsync(cancellationToken);
        try
        {
            ConvertToSnapshot();

            preferencesToPush = CloneDictionary(_preferences);

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
        }
        finally
        {
            _lock.Release();
        }

        lock (_pushQueueGate)
        {
            if (_isPushInProgress)
            {
                // Coalesce pending pushes into the latest snapshot only.
                _pendingPushPreferences = preferencesToPush;
                return true;
            }

            _isPushInProgress = true;
        }

        var lastPushResult = true;

        try
        {
            var currentPreferencesToPush = preferencesToPush;

            while (true)
            {
                bool hasPendingPush;
                lock (_pushQueueGate)
                {
                    hasPendingPush = _pendingPushPreferences != null;
                }

                _logger.LogDebug(
                    "Pushing user preferences (queued after this push: {PendingPushCount}).",
                    hasPendingPush ? 1 : 0);

                try
                {
                    var updatedUser = await _musicAssistant.UpdateUserAsync(
                        username: configuredUsername,
                        preferences: currentPreferencesToPush);

                    lastPushResult = updatedUser != null;
                }
                catch (OperationCanceledException)
                {
                    throw;
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "Failed to save user preferences");
                    lastPushResult = false;
                }

                lock (_pushQueueGate)
                {
                    if (_pendingPushPreferences == null)
                    {
                        _isPushInProgress = false;
                        return lastPushResult;
                    }

                    // Drain the newest queued snapshot after the current push completes.
                    currentPreferencesToPush = _pendingPushPreferences;
                    _pendingPushPreferences = null;
                }
            }
        }
        finally
        {
            lock (_pushQueueGate)
            {
                _isPushInProgress = false;
            }
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

        return mediaItem.MediaType switch
        {
            MediaType.Track => _favoriteTracks.Any(item => string.Equals(item.Uri, mediaItem.Uri, StringComparison.OrdinalIgnoreCase)),
            MediaType.Album => _favoriteAlbums.Any(item => string.Equals(item.Uri, mediaItem.Uri, StringComparison.OrdinalIgnoreCase)),
            MediaType.Artist => _favoriteArtists.Any(item => string.Equals(item.Uri, mediaItem.Uri, StringComparison.OrdinalIgnoreCase)),
            MediaType.Playlist => _favoritePlaylists.Any(item => string.Equals(item.Uri, mediaItem.Uri, StringComparison.OrdinalIgnoreCase)),
            _ => false
        };
    }

    public async Task SetFavoriteAsync(IEnumerable<MediaItem> mediaItems, bool isFavorite, CancellationToken cancellationToken = default)
    {
        if (mediaItems == null)
        {
            _logger.LogWarning("Skipping favorite update because media items list is null.");
            return;
        }

        var skippedInvalid = 0;
        var skippedUnsupported = 0;

        await _lock.WaitAsync(cancellationToken);
        try
        {
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
                    MediaType.Track => UpdateFavoriteList(
                        _favoriteTracks,
                        uri,
                        isFavorite,
                        () => UserDataSnapshotMapper.ToTrack(UserDataSnapshotMapper.ToTrackSnapshot(mediaItem as Track ?? new Track
                        {
                            Uri = mediaItem.Uri ?? string.Empty,
                            ItemId = mediaItem.ItemId,
                            Provider = mediaItem.Provider,
                            Name = mediaItem.Name,
                            DisplayName = mediaItem.DisplayName,
                            Duration = 0
                        }), favorite: true)),
                    MediaType.Album => UpdateFavoriteList(
                        _favoriteAlbums,
                        uri,
                        isFavorite,
                        () => UserDataSnapshotMapper.ToAlbum(UserDataSnapshotMapper.ToAlbumSnapshot(mediaItem as Album ?? new Album
                        {
                            Uri = mediaItem.Uri ?? string.Empty,
                            ItemId = mediaItem.ItemId,
                            Provider = mediaItem.Provider,
                            Name = mediaItem.Name,
                            DisplayName = mediaItem.DisplayName
                        }), favorite: true)),
                    MediaType.Artist => UpdateFavoriteList(
                        _favoriteArtists,
                        uri,
                        isFavorite,
                        () => UserDataSnapshotMapper.ToArtist(UserDataSnapshotMapper.ToArtistSnapshot(mediaItem as Artist ?? new Artist
                        {
                            Uri = mediaItem.Uri ?? string.Empty,
                            ItemId = mediaItem.ItemId,
                            Provider = mediaItem.Provider,
                            Name = mediaItem.Name,
                            DisplayName = mediaItem.DisplayName
                        }), favorite: true)),
                    MediaType.Playlist => UpdateFavoriteList(
                        _favoritePlaylists,
                        uri,
                        isFavorite,
                        () => UserDataSnapshotMapper.ToPlaylist(UserDataSnapshotMapper.ToPlaylistSnapshot(mediaItem as Playlist ?? new Playlist
                        {
                            Uri = mediaItem.Uri ?? string.Empty,
                            ItemId = mediaItem.ItemId,
                            Provider = mediaItem.Provider,
                            Name = mediaItem.Name,
                            DisplayName = mediaItem.DisplayName
                        }, includeItems: false), favorite: true)),
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
        }
        finally
        {
            _lock.Release();
        }

        if (skippedInvalid > 0)
        {
            _logger.LogWarning("Skipped favorite updates for {Count} items because media item or uri was invalid.", skippedInvalid);
        }

        if (skippedUnsupported > 0)
        {
            _logger.LogWarning("Skipped favorite updates for {Count} items because media type was not supported.", skippedUnsupported);
        }

        QueuePreferencesPush();
    }

    #endregion

    #region Playlists

    public async Task<Playlist> AddPlaylistAsync(Playlist playlist, CancellationToken cancellationToken = default)
    {
        if (playlist == null)
        {
            throw new ArgumentNullException(nameof(playlist));
        }

        await _lock.WaitAsync(cancellationToken);
        Playlist storedPlaylist;
        try
        {
            if (string.IsNullOrWhiteSpace(playlist.ItemId))
            {
                playlist.ItemId = GenerateNextLocalPlaylistId(_playlists);
            }

            if (string.IsNullOrWhiteSpace(playlist.Provider))
            {
                playlist.Provider = LocalPlaylistProvider;
            }

            storedPlaylist = UserDataSnapshotMapper.ToPlaylist(UserDataSnapshotMapper.ToPlaylistSnapshot(playlist));
            var insertIndex = 0;
            for (; insertIndex < _playlists.Count; insertIndex++)
            {
                var existing = _playlists[insertIndex];
                var sortNameCompare = string.Compare(
                    storedPlaylist.SortName ?? string.Empty,
                    existing.SortName ?? string.Empty,
                    StringComparison.OrdinalIgnoreCase);

                if (sortNameCompare < 0)
                {
                    break;
                }

                if (sortNameCompare == 0)
                {
                    var nameCompare = string.Compare(
                        storedPlaylist.Name ?? string.Empty,
                        existing.Name ?? string.Empty,
                        StringComparison.OrdinalIgnoreCase);

                    if (nameCompare < 0)
                    {
                        break;
                    }
                }
            }

            _playlists.Insert(insertIndex, storedPlaylist);
        }
        finally
        {
            _lock.Release();
        }

        QueuePreferencesPush();

        return UserDataSnapshotMapper.ToPlaylist(UserDataSnapshotMapper.ToPlaylistSnapshot(storedPlaylist));
    }

    public async Task<bool> UpdatePlaylistAsync(Playlist playlist, CancellationToken cancellationToken = default)
    {
        if (playlist == null || string.IsNullOrWhiteSpace(playlist.ItemId))
        {
            return false;
        }

        await _lock.WaitAsync(cancellationToken);
        try
        {
            var existing = _playlists.FirstOrDefault(candidate =>
                string.Equals(candidate.ItemId, playlist.ItemId, StringComparison.OrdinalIgnoreCase));

            if (existing == null)
            {
                return false;
            }

            var index = _playlists.IndexOf(existing);

            var replacement = UserDataSnapshotMapper.ToPlaylist(UserDataSnapshotMapper.ToPlaylistSnapshot(playlist));
            replacement.Provider = string.IsNullOrWhiteSpace(replacement.Provider)
                ? LocalPlaylistProvider
                : replacement.Provider;

            _playlists[index] = replacement;
            var sortedPlaylists = _playlists
                .OrderBy(candidate => candidate.SortName ?? string.Empty, StringComparer.OrdinalIgnoreCase)
                .ThenBy(candidate => candidate.Name ?? string.Empty, StringComparer.OrdinalIgnoreCase)
                .ToList();
            _playlists.ReplaceRange(sortedPlaylists);
        }
        finally
        {
            _lock.Release();
        }

        QueuePreferencesPush();

        return true;
    }

    public async Task<bool> RemovePlaylistAsync(Playlist playlist, CancellationToken cancellationToken = default)
    {
        if (playlist == null || string.IsNullOrWhiteSpace(playlist.ItemId))
        {
            return false;
        }

        await _lock.WaitAsync(cancellationToken);
        try
        {
            var playlistsToRemove = _playlists.Where(existing =>
                string.Equals(existing.ItemId, playlist.ItemId, StringComparison.OrdinalIgnoreCase))
                .ToList();

            if (playlistsToRemove.Count == 0)
            {
                return false;
            }

            _playlists.RemoveRange(playlistsToRemove);
        }
        finally
        {
            _lock.Release();
        }

        QueuePreferencesPush();

        return true;
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

        await _lock.WaitAsync(cancellationToken);
        try
        {
            var playlistModel = _playlists.FirstOrDefault(existing =>
                string.Equals(existing.ItemId, playlistId, StringComparison.OrdinalIgnoreCase));

            if (playlistModel == null)
            {
                return false;
            }

            var localTracks = playlistModel.Items.ToList();
            foreach (var track in tracksToAdd)
            {
                localTracks.Add(UserDataSnapshotMapper.ToTrack(UserDataSnapshotMapper.ToTrackSnapshot(track), favorite: false));
            }

            playlistModel.Items = localTracks;

            try
            {
                var generatedImageDataUri = await BuildPlaylistCollageDataUriAsync(localTracks, cancellationToken);
                if (!string.IsNullOrWhiteSpace(generatedImageDataUri))
                {
                    playlistModel.Metadata = UserDataSnapshotMapper.BuildMetadata(generatedImageDataUri);
                }
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Failed to generate playlist collage image for playlist {PlaylistId}", playlistId);
            }
        }
        finally
        {
            _lock.Release();
        }

        QueuePreferencesPush();

        return true;
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

        await _lock.WaitAsync(cancellationToken);
        try
        {
            var playlistModel = _playlists.FirstOrDefault(existing =>
                string.Equals(existing.ItemId, playlistId, StringComparison.OrdinalIgnoreCase));

            if (playlistModel == null)
            {
                return false;
            }

            var localTracks = playlistModel.Items.ToList();
            var removed = localTracks.RemoveAll(track => !string.IsNullOrWhiteSpace(track.Uri)
                && urisToRemove.Contains(track.Uri));

            if (removed == 0)
            {
                return false;
            }

            playlistModel.Items = localTracks;

            try
            {
                var generatedImageDataUri = await BuildPlaylistCollageDataUriAsync(localTracks, cancellationToken);
                playlistModel.Metadata = UserDataSnapshotMapper.BuildMetadata(generatedImageDataUri);
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Failed to generate playlist collage image for playlist {PlaylistId}", playlistId);
            }
        }
        finally
        {
            _lock.Release();
        }

        QueuePreferencesPush();

        return true;
    }

    #endregion

    #region Helpers

    private FavoritesSnapshot LoadFavoritesSnapshot()
    {
        if (!_preferences.TryGetValue(FavoritesRootKey, out var favRootObj) || favRootObj is null)
        {
            var created = new FavoritesSnapshot();
            _preferences[FavoritesRootKey] = created;
            _favoriteTracks.ReplaceRange(Array.Empty<Track>());
            _favoriteAlbums.ReplaceRange(Array.Empty<Album>());
            _favoritePlaylists.ReplaceRange(Array.Empty<Playlist>());
            _favoriteArtists.ReplaceRange(Array.Empty<Artist>());
            return created;
        }

        if (favRootObj is FavoritesSnapshot snapshot)
        {
            _favoriteTracks.ReplaceRange(snapshot.Tracks
                .Select(track => UserDataSnapshotMapper.ToTrack(track, favorite: true)));
            _favoriteAlbums.ReplaceRange(snapshot.Albums
                .Select(album => UserDataSnapshotMapper.ToAlbum(album, favorite: true)));
            _favoritePlaylists.ReplaceRange(snapshot.Playlists
                .Select(playlist => UserDataSnapshotMapper.ToPlaylist(playlist, favorite: true)));
            _favoriteArtists.ReplaceRange(snapshot.Artists
                .Select(artist => UserDataSnapshotMapper.ToArtist(artist, favorite: true)));
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
                    _favoriteTracks.ReplaceRange(parsedSnapshot.Tracks
                        .Select(track => UserDataSnapshotMapper.ToTrack(track, favorite: true)));
                    _favoriteAlbums.ReplaceRange(parsedSnapshot.Albums
                        .Select(album => UserDataSnapshotMapper.ToAlbum(album, favorite: true)));
                    _favoritePlaylists.ReplaceRange(parsedSnapshot.Playlists
                        .Select(playlist => UserDataSnapshotMapper.ToPlaylist(playlist, favorite: true)));
                    _favoriteArtists.ReplaceRange(parsedSnapshot.Artists
                        .Select(artist => UserDataSnapshotMapper.ToArtist(artist, favorite: true)));
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
        _favoriteTracks.ReplaceRange(Array.Empty<Track>());
        _favoriteAlbums.ReplaceRange(Array.Empty<Album>());
        _favoritePlaylists.ReplaceRange(Array.Empty<Playlist>());
        _favoriteArtists.ReplaceRange(Array.Empty<Artist>());
        return createdSnapshot;
    }

    private static bool UpdateFavoriteList<T>(ICollection<T> list, string uri, bool isFavorite, Func<T> createItem)
        where T : MediaItem
    {
        var existing = list.FirstOrDefault(item => string.Equals(item.Uri, uri, StringComparison.OrdinalIgnoreCase));

        if (isFavorite)
        {
            if (existing != null)
            {
                return false;
            }

            list.Add(createItem());
            return true;
        }

        if (existing == null)
        {
            return false;
        }

        return list.Remove(existing);
    }

    private PlaylistsSnapshot LoadPlaylistsSnapshot()
    {
        if (!_preferences.TryGetValue(PlaylistsRootKey, out var playlistsRootObj) || playlistsRootObj is null)
        {
            var created = new PlaylistsSnapshot();
            _preferences[PlaylistsRootKey] = created;
            _playlists.ReplaceRange(Array.Empty<Playlist>());
            return created;
        }

        if (playlistsRootObj is PlaylistsSnapshot snapshot)
        {
            _playlists.ReplaceRange(snapshot.Playlists
                .Select(playlist => UserDataSnapshotMapper.ToPlaylist(playlist))
                .OrderBy(playlist => playlist.SortName ?? string.Empty, StringComparer.OrdinalIgnoreCase)
                .ThenBy(playlist => playlist.Name ?? string.Empty, StringComparer.OrdinalIgnoreCase));
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
                    _playlists.ReplaceRange(parsedSnapshot.Playlists
                        .Select(playlist => UserDataSnapshotMapper.ToPlaylist(playlist))
                        .OrderBy(playlist => playlist.SortName ?? string.Empty, StringComparer.OrdinalIgnoreCase)
                        .ThenBy(playlist => playlist.Name ?? string.Empty, StringComparer.OrdinalIgnoreCase));
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
        _playlists.ReplaceRange(Array.Empty<Playlist>());
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

    private static string GenerateNextLocalPlaylistId(IEnumerable<Playlist> playlists)
    {
        var id = playlists
            .Select(playlist => ParseLocalPlaylistId(playlist.ItemId))
            .Where(parsedId => parsedId > 0)
            .DefaultIfEmpty(0)
            .Max() + 1;

        return id.ToString();
    }

    private static string? GetTrackImagePath(Track track)
    {
        if (track == null)
        {
            return null;
        }

        if (!string.IsNullOrWhiteSpace(track.Album?.PrimaryImage?.Path))
        {
            return track.Album.PrimaryImage?.Path;
        }

        if (!string.IsNullOrWhiteSpace(track.PrimaryImage?.Path))
        {
            return track.PrimaryImage?.Path;
        }

        return null;
    }

    private async Task<string?> BuildPlaylistCollageDataUriAsync(
        List<Track> tracks,
        CancellationToken cancellationToken)
    {
        var sourcePaths = tracks
            .Select(GetTrackImagePath)
            .Where(path => !string.IsNullOrWhiteSpace(path))
            .Select(path => path!)
            .Distinct(StringComparer.Ordinal)
            .OrderBy(_ => Random.Shared.Next())
            .Take(PlaylistCollageTileCount)
            .ToList();

        if (sourcePaths.Count == 0)
        {
            return null;
        }

        var decodedImages = new List<SixLabors.ImageSharp.Image<SixLabors.ImageSharp.PixelFormats.Rgba32>>();
        try
        {
            foreach (var sourcePath in sourcePaths)
            {
                cancellationToken.ThrowIfCancellationRequested();

                var imageBytes = await LoadImageBytesAsync(sourcePath, cancellationToken);
                if (imageBytes == null || imageBytes.Length == 0)
                {
                    continue;
                }

                var image = SixLabors.ImageSharp.Image.Load<SixLabors.ImageSharp.PixelFormats.Rgba32>(imageBytes);
                decodedImages.Add(image);
            }

            if (decodedImages.Count == 0)
            {
                return null;
            }

            while (decodedImages.Count < PlaylistCollageTileCount)
            {
                decodedImages.Add(decodedImages[decodedImages.Count % Math.Max(1, decodedImages.Count)].Clone());
            }

            var tileSize = PlaylistCollageSizePx / 2;
            using var collage = new SixLabors.ImageSharp.Image<SixLabors.ImageSharp.PixelFormats.Rgba32>(
                PlaylistCollageSizePx,
                PlaylistCollageSizePx,
                SixLabors.ImageSharp.Color.Black);

            for (var i = 0; i < PlaylistCollageTileCount; i++)
            {
                using var tile = decodedImages[i].Clone(ctx => ctx.Resize(new SixLabors.ImageSharp.Processing.ResizeOptions
                {
                    Size = new SixLabors.ImageSharp.Size(tileSize, tileSize),
                    Mode = SixLabors.ImageSharp.Processing.ResizeMode.Crop,
                    Position = SixLabors.ImageSharp.Processing.AnchorPositionMode.Center
                }));

                var x = (i % 2) * tileSize;
                var y = (i / 2) * tileSize;

                collage.Mutate(ctx => ctx.DrawImage(tile, new SixLabors.ImageSharp.Point(x, y), 1f));
            }

            using var output = new MemoryStream();
            await collage.SaveAsJpegAsync(output, new SixLabors.ImageSharp.Formats.Jpeg.JpegEncoder
            {
                Quality = PlaylistCollageJpegQuality
            }, cancellationToken);

            var base64 = Convert.ToBase64String(output.ToArray());
            return string.Concat("data:image/jpeg;base64,", base64);
        }
        finally
        {
            foreach (var image in decodedImages)
            {
                image.Dispose();
            }
        }
    }

    private void ConvertToSnapshot()
    {
        var favoritesSnapshot = new FavoritesSnapshot
        {
            Tracks = _favoriteTracks
                .Select(track => UserDataSnapshotMapper.ToTrackSnapshot(track))
                .ToList(),
            Albums = _favoriteAlbums
                .Select(album => UserDataSnapshotMapper.ToAlbumSnapshot(album))
                .ToList(),
            Playlists = _favoritePlaylists
                .Select(playlist => UserDataSnapshotMapper.ToPlaylistSnapshot(playlist, includeItems: false))
                .ToList(),
            Artists = _favoriteArtists
                .Select(artist => UserDataSnapshotMapper.ToArtistSnapshot(artist))
                .ToList()
        };

        var playlistsSnapshot = new PlaylistsSnapshot
        {
            Playlists = _playlists
                .Select(playlist => UserDataSnapshotMapper.ToPlaylistSnapshot(playlist))
                .ToList()
        };

        _preferences[FavoritesRootKey] = favoritesSnapshot;
        _preferences[PlaylistsRootKey] = playlistsSnapshot;
    }

    private static async Task<byte[]?> LoadImageBytesAsync(string sourcePath, CancellationToken cancellationToken)
    {
        if (sourcePath.StartsWith("data:", StringComparison.OrdinalIgnoreCase))
        {
            var commaIndex = sourcePath.IndexOf(',', StringComparison.Ordinal);
            if (commaIndex <= 0 || commaIndex >= sourcePath.Length - 1)
            {
                return null;
            }

            var header = sourcePath[..commaIndex];
            var payload = sourcePath[(commaIndex + 1)..];
            if (header.Contains(";base64", StringComparison.OrdinalIgnoreCase))
            {
                return Convert.FromBase64String(payload);
            }

            var text = Uri.UnescapeDataString(payload);
            return System.Text.Encoding.UTF8.GetBytes(text);
        }

        if (!Uri.TryCreate(sourcePath, UriKind.Absolute, out var uri))
        {
            return null;
        }

        if (!string.Equals(uri.Scheme, Uri.UriSchemeHttp, StringComparison.OrdinalIgnoreCase)
            && !string.Equals(uri.Scheme, Uri.UriSchemeHttps, StringComparison.OrdinalIgnoreCase))
        {
            return null;
        }

        return await _httpClient.GetByteArrayAsync(uri, cancellationToken);
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
