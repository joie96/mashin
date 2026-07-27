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
    Task AddTracksAsync(Playlist playlist, IReadOnlyList<string> uris);
    Task RemoveTracksAsync(Playlist playlist, IReadOnlyList<int> positionsToRemove);
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
    private readonly MusicAssistantService _musicAssistant;
    private readonly SettingsService _settings;
    private readonly ILogger<PlaylistService> _logger;
    private readonly SemaphoreSlim _refreshLock = new(1, 1);
    private bool _isLoading;

    public PlaylistService(
        MusicAssistantService musicAssistant,
        SettingsService settings,
        ILogger<PlaylistService> logger)
    {
        _musicAssistant = musicAssistant;
        _settings = settings;
        _logger = logger;
    }

    public ObservableRangeCollection<Playlist> Playlists { get; } = new();

    public bool IsLoading
    {
        get => _isLoading;
        private set => SetProperty(ref _isLoading, value);
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    public event EventHandler<PlaylistServiceChangedEventArgs>? Changed;

    public async Task RefreshAsync()
    {
        await _refreshLock.WaitAsync();
        try
        {
            IsLoading = true;

            var playlists = await _musicAssistant.GetLibraryPlaylistsAsync(
                orderBy: "sort_name",
                userPrefix: string.Concat(_settings.Username, "--"));

            await LoadPlaylistsMetadataAsync(playlists);
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
        await RefreshAsync();
        Changed?.Invoke(this, new PlaylistServiceChangedEventArgs(PlaylistServiceChangeType.Created, created?.ItemId));
    }

    public async Task UpdatePlaylistAsync(Playlist playlist)
    {
        await _musicAssistant.UpdatePlaylistAsync(playlist.ItemId, playlist, true);
        await RefreshAsync();
        Changed?.Invoke(this, new PlaylistServiceChangedEventArgs(PlaylistServiceChangeType.Updated, playlist.ItemId));
    }

    public async Task RemovePlaylistAsync(Playlist playlist)
    {
        await _musicAssistant.RemovePlaylistAsync(playlist.ItemId);
        await RefreshAsync();
        Changed?.Invoke(this, new PlaylistServiceChangedEventArgs(PlaylistServiceChangeType.Deleted, playlist.ItemId));
    }

    public async Task AddTracksAsync(Playlist playlist, IReadOnlyList<string> uris)
    {
        if (uris.Count == 0)
        {
            return;
        }

        await _musicAssistant.AddPlaylistTracksAsync(playlist.ItemId, uris.ToList());
        await RefreshSinglePlaylistMetadataAsync(playlist.ItemId);
    }

    public async Task RemoveTracksAsync(Playlist playlist, IReadOnlyList<int> positionsToRemove)
    {
        if (positionsToRemove.Count == 0)
        {
            return;
        }

        await _musicAssistant.RemovePlaylistTracksAsync(playlist.ItemId, positionsToRemove.ToList());
        await RefreshSinglePlaylistMetadataAsync(playlist.ItemId);
    }

    private async Task RefreshSinglePlaylistMetadataAsync(string? playlistItemId)
    {
        if (string.IsNullOrWhiteSpace(playlistItemId))
        {
            return;
        }

        await _refreshLock.WaitAsync();
        try
        {
            var playlistIndex = FindPlaylistIndexByItemId(playlistItemId);
            if (playlistIndex < 0)
            {
                return;
            }

            var playlist = Playlists[playlistIndex];
            if (string.IsNullOrWhiteSpace(playlist.Provider))
            {
                return;
            }

            var tracks = await _musicAssistant.GetPlaylistTracksAsync(
                playlist.ItemId,
                playlist.Provider,
                forceRefresh: true);

            playlist.TracksCount = tracks.Count;
            playlist.TotalDurationSeconds = tracks.Sum(track => Math.Max(0, track.Duration));

            // Emit a replace notification for the updated item so bound list rows refresh reliably.
            Playlists[playlistIndex] = playlist;

            Changed?.Invoke(this, new PlaylistServiceChangedEventArgs(PlaylistServiceChangeType.MetadataUpdated, playlist.ItemId));
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to refresh playlist metadata for {PlaylistId}.", playlistItemId);
        }
        finally
        {
            _refreshLock.Release();
        }
    }

    private async Task LoadPlaylistsMetadataAsync(IReadOnlyCollection<Playlist> playlists)
    {
        if (playlists.Count == 0)
        {
            return;
        }

        const int maxConcurrentRequests = 8;
        using var throttler = new SemaphoreSlim(maxConcurrentRequests);

        var metadataTasks = playlists.Select(async playlist =>
        {
            if (string.IsNullOrWhiteSpace(playlist.ItemId)
                || string.IsNullOrWhiteSpace(playlist.Provider))
            {
                playlist.TracksCount = 0;
                playlist.TotalDurationSeconds = 0;
                return;
            }

            await throttler.WaitAsync();
            try
            {
                var tracks = await _musicAssistant.GetPlaylistTracksAsync(playlist.ItemId, playlist.Provider);
                playlist.TracksCount = tracks.Count;
                playlist.TotalDurationSeconds = tracks.Sum(track => Math.Max(0, track.Duration));
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Failed to load metadata for playlist {PlaylistId}.", playlist.ItemId);
                playlist.TracksCount = 0;
                playlist.TotalDurationSeconds = 0;
            }
            finally
            {
                throttler.Release();
            }
        });

        await Task.WhenAll(metadataTasks);
    }

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
}
