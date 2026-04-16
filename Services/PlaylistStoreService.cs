using mashin.Collections;
using mashin.Models;
using Microsoft.Extensions.Logging;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Threading;

namespace mashin.Services;

/// <summary>
/// Provides a centralized, observable source of truth for user playlists,
/// including loading, creation, rename, and delete operations.
/// </summary>
public interface IPlaylistStoreService : INotifyPropertyChanged
{
    ObservableRangeCollection<Playlist> Playlists { get; }
    bool IsLoading { get; }

    Task RefreshAsync();
    Task<bool> CreateAsync(string name);
    Task<bool> RenameAsync(Playlist playlist, string name);
    Task<bool> RemoveAsync(Playlist playlist);
}

/// <summary>
/// Coordinates playlist state between API/actions and UI consumers,
/// exposing one shared in-memory playlist collection for the app.
/// </summary>
public sealed class PlaylistStoreService : IPlaylistStoreService
{
    #region Fields

    private readonly MusicAssistantService _musicAssistant;
    private readonly IUserDataService _userDataService;
    private readonly IMediaItemActions _mediaItemActions;
    private readonly ILogger<PlaylistStoreService> _logger;
    private readonly SemaphoreSlim _sync = new(1, 1);

    private readonly ObservableRangeCollection<Playlist> _playlists = new();
    private bool _isLoading;

    #endregion

    #region Construction

    public PlaylistStoreService(
        MusicAssistantService musicAssistant,
        IUserDataService userDataService,
        IMediaItemActions mediaItemActions,
        ILogger<PlaylistStoreService> logger)
    {
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _mediaItemActions = mediaItemActions;
        _logger = logger;
    }

    #endregion

    #region Properties

    public ObservableRangeCollection<Playlist> Playlists => _playlists;

    public bool IsLoading
    {
        get => _isLoading;
        private set => SetProperty(ref _isLoading, value);
    }

    #endregion

    #region Public API

    public async Task RefreshAsync()
    {
        await _sync.WaitAsync();

        try
        {
            if (IsLoading)
            {
                return;
            }

            IsLoading = true;
            await _userDataService.GetPreferencesAsync();

            var prefix = GetUserPlaylistPrefix();
            var playlists = await _musicAssistant.GetLibraryPlaylistsAsync(
                search: string.IsNullOrWhiteSpace(prefix) ? null : prefix,
                orderBy: "sort_name");

            foreach (var playlist in playlists)
            {
                if (!string.IsNullOrWhiteSpace(prefix)
                    && !string.IsNullOrWhiteSpace(playlist.Name)
                    && playlist.Name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
                {
                    playlist.DisplayName = playlist.Name[prefix.Length..];
                }
                else
                {
                    playlist.DisplayName = playlist.Name;
                }
            }

            _playlists.ReplaceRange(playlists);
            _logger.LogInformation("Loaded {Count} playlists in store", _playlists.Count);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to refresh playlist store");
        }
        finally
        {
            IsLoading = false;
            _sync.Release();
        }
    }

    public async Task<bool> CreateAsync(string name)
    {
        name = name?.Trim() ?? string.Empty;
        if (string.IsNullOrWhiteSpace(name))
        {
            return false;
        }

        await _userDataService.GetPreferencesAsync();

        var prefix = GetUserPlaylistPrefix();
        if (string.IsNullOrWhiteSpace(prefix))
        {
            _logger.LogWarning("Cannot create playlist without a user name prefix.");
            return false;
        }

        if (!name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
        {
            name = string.Concat(prefix, name);
        }

        try
        {
            var playlist = await _musicAssistant.CreatePlaylistAsync(name);
            if (playlist is null)
            {
                return false;
            }

            await RefreshAsync();
            return true;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to create playlist: {Name}", name);
            return false;
        }
    }

    public async Task<bool> RenameAsync(Playlist playlist, string name)
    {
        if (playlist is null)
        {
            return false;
        }

        name = name?.Trim() ?? string.Empty;
        if (string.IsNullOrWhiteSpace(name))
        {
            return false;
        }

        await _userDataService.GetPreferencesAsync();

        var prefix = GetUserPlaylistPrefix();
        if (string.IsNullOrWhiteSpace(prefix))
        {
            _logger.LogWarning("Cannot update playlist without a user name prefix.");
            return false;
        }

        if (!name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
        {
            name = string.Concat(prefix, name);
        }

        var originalName = playlist.Name;
        var originalDisplayName = playlist.DisplayName;

        try
        {
            playlist.Name = name;
            playlist.DisplayName = name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)
                ? name[prefix.Length..]
                : name;

            await _mediaItemActions.UpdatePlaylistAsync(playlist);
            await RefreshAsync();
            return true;
        }
        catch (Exception ex)
        {
            playlist.Name = originalName;
            playlist.DisplayName = originalDisplayName;
            _logger.LogError(ex, "Failed to update playlist: {Name}", name);
            return false;
        }
    }

    public async Task<bool> RemoveAsync(Playlist playlist)
    {
        if (playlist is null)
        {
            return false;
        }

        try
        {
            await _mediaItemActions.RemovePlaylistAsync(playlist);
            await RefreshAsync();
            return true;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to remove playlist: {Name}", playlist.Name);
            return false;
        }
    }

    #endregion

    #region Helpers

    private string? GetUserPlaylistPrefix()
    {
        var username = _userDataService.CurrentUser?.Username;
        if (string.IsNullOrWhiteSpace(username))
        {
            return null;
        }

        return string.Concat(username, "--");
    }

    #endregion

    #region INotifyPropertyChanged

    public event PropertyChangedEventHandler? PropertyChanged;

    private void OnPropertyChanged([CallerMemberName] string? propertyName = null)
        => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));

    private bool SetProperty<T>(ref T field, T value, [CallerMemberName] string? propertyName = null)
    {
        if (EqualityComparer<T>.Default.Equals(field, value))
        {
            return false;
        }

        field = value;
        OnPropertyChanged(propertyName);
        return true;
    }

    #endregion
}
