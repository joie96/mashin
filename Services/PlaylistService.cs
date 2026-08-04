using mashin.Collections;
using mashin.Models;
using Microsoft.Extensions.Logging;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Runtime.CompilerServices;

namespace mashin.Services;

public interface IPlaylistService : INotifyPropertyChanged
{
    ObservableRangeCollection<Playlist> Playlists { get; }
    bool IsLoading { get; }

    event EventHandler<PlaylistServiceChangedEventArgs>? Changed;

    Task RefreshAsync();
    Task CreatePlaylistAsync(string name);
    Task UpdatePlaylistAsync(Playlist playlist);
    Task RemovePlaylistAsync(Playlist playlist);
    Task AddTracksAsync(Playlist playlist, IReadOnlyList<Track> tracksToAdd);
    Task RemoveTracksAsync(Playlist playlist, IReadOnlyList<Track> tracksToRemove);
}

public enum PlaylistServiceChangeType
{
    Refreshed,
    Created,
    Updated,
    Deleted,
    MetadataUpdated
}

public sealed class PlaylistServiceChangedEventArgs : EventArgs
{
    public PlaylistServiceChangedEventArgs(PlaylistServiceChangeType changeType, string? playlistItemId = null)
    {
        ChangeType = changeType;
        PlaylistItemId = playlistItemId;
    }

    public PlaylistServiceChangeType ChangeType { get; }

    public string? PlaylistItemId { get; }
}

public sealed class PlaylistService : IPlaylistService
{
    #region Fields

    private readonly MusicAssistantService _musicAssistant;
    private readonly IUserDataService _userDataService;
    private readonly SettingsService _settings;
    private readonly ILogger<PlaylistService> _logger;
    private readonly SemaphoreSlim _refreshLock = new(1, 1);
    private bool _isLoading;

    #endregion

    #region Construction

    public PlaylistService(
        MusicAssistantService musicAssistant,
        IUserDataService userDataService,
        SettingsService settings,
        ILogger<PlaylistService> logger)
    {
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _settings = settings;
        _logger = logger;
    }

    #endregion

    #region Properties and Events

    public ObservableRangeCollection<Playlist> Playlists { get; } = new();

    public bool IsLoading
    {
        get => _isLoading;
        private set => SetProperty(ref _isLoading, value);
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    public event EventHandler<PlaylistServiceChangedEventArgs>? Changed;

    #endregion

    #region Public API

    public async Task RefreshAsync()
    {
        await _refreshLock.WaitAsync();
        try
        {
            IsLoading = true;

            var playlists = await _musicAssistant.GetLibraryPlaylistsAsync(
                orderBy: "sort_name",
                userPrefix: string.Concat(_settings.Username, "--"));

            const int maxConcurrentRequests = 8;
            using var throttler = new SemaphoreSlim(maxConcurrentRequests);

            var loadTracksTasks = playlists.Select(async playlist =>
            {
                if (string.IsNullOrWhiteSpace(playlist.ItemId)
                    || string.IsNullOrWhiteSpace(playlist.Provider))
                {
                    playlist.Items = Array.Empty<Track>();
                    return;
                }

                await throttler.WaitAsync();
                try
                {
                    var tracks = await _musicAssistant.GetPlaylistTracksAsync(playlist.ItemId, playlist.Provider);
                    for (var i = 0; i < tracks.Count; i++)
                    {
                        tracks[i].Favorite = await _userDataService.IsFavoriteAsync(tracks[i]);
                    }

                    playlist.Items = tracks;
                    playlist.Favorite = await _userDataService.IsFavoriteAsync(playlist);
                }
                catch (Exception ex)
                {
                    _logger.LogWarning(ex, "Failed to load tracks for playlist {PlaylistId}.", playlist.ItemId);
                    playlist.Items = Array.Empty<Track>();
                }
                finally
                {
                    throttler.Release();
                }
            });

            await Task.WhenAll(loadTracksTasks);
            Playlists.ReplaceRange(playlists);

            Changed?.Invoke(this, new PlaylistServiceChangedEventArgs(PlaylistServiceChangeType.Refreshed));
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to refresh playlists.");
        }
        finally
        {
            IsLoading = false;
            _refreshLock.Release();
        }
    }

    public async Task CreatePlaylistAsync(string name)
    {
        var created = await _musicAssistant.CreatePlaylistAsync(name);
        if (created != null)
        {
            ApplyPlaylistDisplayName(created);
            created.Items = Array.Empty<Track>();
            created.Favorite = await _userDataService.IsFavoriteAsync(created);
            AddLocalPlaylist(created);
        }

        Changed?.Invoke(this, new PlaylistServiceChangedEventArgs(PlaylistServiceChangeType.Created, created?.ItemId));
    }

    public async Task UpdatePlaylistAsync(Playlist playlist)
    {
        await _musicAssistant.UpdatePlaylistAsync(playlist.ItemId, playlist, true);
        Changed?.Invoke(this, new PlaylistServiceChangedEventArgs(PlaylistServiceChangeType.Updated, playlist.ItemId));
    }

    public async Task RemovePlaylistAsync(Playlist playlist)
    {
        var removedIndex = FindPlaylistIndexByItemId(playlist.ItemId);
        var removedPlaylist = removedIndex >= 0 ? Playlists[removedIndex] : null;

        if (removedIndex >= 0)
        {
            Playlists.RemoveAt(removedIndex);
        }

        try
        {
            await _musicAssistant.RemovePlaylistAsync(playlist.ItemId);
        }
        catch
        {
            if (removedPlaylist != null)
            {
                Playlists.Insert(Math.Clamp(removedIndex, 0, Playlists.Count), removedPlaylist);
            }

            throw;
        }

        Changed?.Invoke(this, new PlaylistServiceChangedEventArgs(PlaylistServiceChangeType.Deleted, playlist.ItemId));
    }

    public async Task AddTracksAsync(Playlist playlist, IReadOnlyList<Track> tracksToAdd)
    {
        if (tracksToAdd.Count == 0)
        {
            return;
        }

        var uris = tracksToAdd
            .Where(track => !string.IsNullOrWhiteSpace(track.Uri))
            .Select(track => track.Uri!)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToList();

        if (uris.Count == 0)
        {
            _logger.LogDebug("No valid URIs found in selected tracks for playlist {PlaylistId}.", playlist.ItemId);
            return;
        }

        await _musicAssistant.AddPlaylistTracksAsync(playlist.ItemId, uris.ToList());

        var localTracks = playlist.Items.ToList();
        foreach (var track in tracksToAdd)
        {
            if (string.IsNullOrWhiteSpace(track.Uri))
            {
                continue;
            }

            track.Index = localTracks.Count;
            localTracks.Add(track);
        }

        playlist.Items = localTracks;
        ReplaceLocalPlaylist(playlist);
        Changed?.Invoke(this, new PlaylistServiceChangedEventArgs(PlaylistServiceChangeType.MetadataUpdated, playlist.ItemId));
    }

    public async Task RemoveTracksAsync(Playlist playlist, IReadOnlyList<Track> tracksToRemove)
    {
        if (tracksToRemove.Count == 0)
        {
            return;
        }

        var positionsToRemove = tracksToRemove
            .Where(track => track.Index >= 0)
            .Select(track => track.Index)
            .Distinct()
            .OrderByDescending(position => position)
            .ToList();

        if (positionsToRemove.Count == 0)
        {
            _logger.LogDebug("No valid positions found in selected tracks for playlist {PlaylistId}.", playlist.ItemId);
            return;
        }

        await _musicAssistant.RemovePlaylistTracksAsync(playlist.ItemId, positionsToRemove.ToList());

        var localTracks = playlist.Items.ToList();
        foreach (var position in positionsToRemove)
        {
            if (position >= 0 && position < localTracks.Count)
            {
                localTracks.RemoveAt(position);
            }
        }

        for (var i = 0; i < localTracks.Count; i++)
        {
            localTracks[i].Index = i;
        }

        playlist.Items = localTracks;
        ReplaceLocalPlaylist(playlist);
        Changed?.Invoke(this, new PlaylistServiceChangedEventArgs(PlaylistServiceChangeType.MetadataUpdated, playlist.ItemId));
    }

    #endregion

    #region Local Playlist Helpers

    private int FindPlaylistIndexByItemId(string playlistItemId)
    {
        for (var i = 0; i < Playlists.Count; i++)
        {
            if (string.Equals(Playlists[i].ItemId, playlistItemId, StringComparison.OrdinalIgnoreCase))
            {
                return i;
            }
        }

        return -1;
    }

    private void AddLocalPlaylist(Playlist playlist)
    {
        var insertIndex = FindInsertIndex(playlist);
        Playlists.Insert(insertIndex, playlist);
    }

    private void ReplaceLocalPlaylist(Playlist playlist)
    {
        var existingIndex = FindPlaylistIndexByItemId(playlist.ItemId);
        if (existingIndex < 0)
        {
            AddLocalPlaylist(playlist);
            return;
        }

        Playlists[existingIndex] = playlist;
    }

    private int FindInsertIndex(Playlist playlist)
    {
        for (var i = 0; i < Playlists.Count; i++)
        {
            var current = Playlists[i];
            var sortNameComparison = string.Compare(
                playlist.SortName ?? string.Empty,
                current.SortName ?? string.Empty,
                StringComparison.OrdinalIgnoreCase);

            if (sortNameComparison < 0)
            {
                return i;
            }

            if (sortNameComparison == 0
                && string.Compare(playlist.Name, current.Name, StringComparison.OrdinalIgnoreCase) < 0)
            {
                return i;
            }
        }

        return Playlists.Count;
    }

    private void ApplyPlaylistDisplayName(Playlist playlist)
    {
        var prefix = string.Concat(_settings.Username, "--");

        if (!string.IsNullOrWhiteSpace(prefix)
            && !string.IsNullOrWhiteSpace(playlist.Name)
            && playlist.Name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
        {
            playlist.DisplayName = playlist.Name[prefix.Length..];
            return;
        }

        playlist.DisplayName = playlist.Name;
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
