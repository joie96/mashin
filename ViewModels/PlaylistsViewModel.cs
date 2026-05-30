using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
using MauiIcons.Fluent;
using MauiIcons.Fluent.Filled;
using Microsoft.Extensions.Logging;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Windows.Input;

namespace mashin.ViewModels;

public sealed class PlaylistsViewModel : INotifyPropertyChanged, INavigationAware, IDisposable
{
    #region Fields

    private readonly MusicAssistantService _musicAssistantService;
    private readonly IUserDataService _userDataService;
    private readonly INavigationService _navigationService;
    private readonly IMediaItemActions _mediaActions;
    private readonly IContextMenuService _contextMenuService;
    private readonly ILogger<PlaylistsViewModel> _logger;
    private readonly ObservableCollection<ContextMenuItem> _playlistContextMenuItems = new();
    private readonly ObservableCollection<Playlist> _playlists = new();
    private readonly IReadOnlyList<TableViewSkeleton> _playlistSkeletons = Enumerable.Range(0, 12)
        .Select(_ => new TableViewSkeleton())
        .ToList();

    private bool _isLoading;
    private bool _disposed;
    private Playlist? _contextPlaylist;

    #endregion

    #region Construction

    public PlaylistsViewModel(
        MusicAssistantService musicAssistantService,
        IUserDataService userDataService,
        INavigationService navigationService,
        IMediaItemActions mediaActions,
        IContextMenuService contextMenuService,
        ILogger<PlaylistsViewModel> logger)
    {
        _musicAssistantService = musicAssistantService;
        _userDataService = userDataService;
        _navigationService = navigationService;
        _mediaActions = mediaActions;
        _contextMenuService = contextMenuService;
        _logger = logger;

        // Navigation command
        PlaylistTappedCommand = new Command<Playlist>(async playlist =>
            await _navigationService.NavigateToAsync<PlaylistDetailPage>(playlist));

        // Long-press selection command
        PlaylistLongPressedCommand = new Command<Playlist>(playlist =>
        {
            if (playlist == null)
            {
                return;
            }

            playlist.IsSelected = !playlist.IsSelected;
        });

        // Context menu command
        ShowPlaylistContextMenuAtAnchorCommand = new Command<View>(async anchor =>
        {
            if (anchor == null)
            {
                return;
            }

            _contextPlaylist = anchor.BindingContext as Playlist;
            if (_contextPlaylist == null)
            {
                return;
            }

            if (_playlistContextMenuItems.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(_playlistContextMenuItems, anchor);
            }
        });

        BuildPlaylistContextMenuItems();
    }

    #endregion

    #region Properties

    public bool IsLoading
    {
        get => _isLoading;
        private set
        {
            if (SetProperty(ref _isLoading, value))
            {
                OnPropertyChanged(nameof(ShowPlaylistListView));
                OnPropertyChanged(nameof(ShowNoPlaylistsMessage));
                OnPropertyChanged(nameof(PlaylistItems));
            }
        }
    }

    public bool HasPlaylists => _playlists.Count > 0;

    public bool ShowPlaylistListView => IsLoading || HasPlaylists;

    public bool ShowNoPlaylistsMessage => !IsLoading && !HasPlaylists;

    public IEnumerable<object> PlaylistItems => IsLoading
        ? _playlistSkeletons
        : _playlists;

    #endregion

    #region Commands

    public ICommand PlaylistTappedCommand { get; }

    public ICommand PlaylistLongPressedCommand { get; }

    public ICommand ShowPlaylistContextMenuAtAnchorCommand { get; }

    #endregion

    #region Lifecycle

    public async Task OnNavigatedToAsync(object? parameter)
    {
        await LoadPlaylistsAsync();
    }

    public Task OnNavigatedFromAsync()
    {
        return Task.CompletedTask;
    }

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }

        _disposed = true;
    }

    #endregion

    #region Loading

    private async Task LoadPlaylistsAsync()
    {
        IsLoading = true;

        try
        {
            await _userDataService.GetPreferencesAsync();

            var username = _userDataService.CurrentUser?.Username;
            var prefix = string.IsNullOrWhiteSpace(username)
                ? null
                : string.Concat(username, "--");

            var playlists = await _musicAssistantService.GetLibraryPlaylistsAsync(
                search: string.IsNullOrWhiteSpace(prefix) ? null : prefix,
                orderBy: "sort_name");

            await LoadPlaylistsMetadataAsync(playlists);

            _playlists.Clear();

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

                _playlists.Add(playlist);
            }

            OnPropertyChanged(nameof(HasPlaylists));
            OnPropertyChanged(nameof(ShowPlaylistListView));
            OnPropertyChanged(nameof(ShowNoPlaylistsMessage));
            OnPropertyChanged(nameof(PlaylistItems));
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to load playlists page.");
        }
        finally
        {
            IsLoading = false;
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
                var tracks = await _musicAssistantService.GetPlaylistTracksAsync(playlist.ItemId, playlist.Provider);
                var tracksCount = tracks.Count;
                var totalDurationSeconds = tracks.Sum(track => Math.Max(0, track.Duration));

                playlist.TracksCount = tracksCount;
                playlist.TotalDurationSeconds = totalDurationSeconds;
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

    #endregion

    #region Context Menu

    private void BuildPlaylistContextMenuItems()
    {
        _playlistContextMenuItems.Clear();

        _playlistContextMenuItems.Add(new ContextMenuItem
        {
            Text = "Öffnen",
            Icon = FluentIcons.TextBulletListLtr16,
            Command = new Command(async () =>
            {
                var target = GetPlaylistsForAction().FirstOrDefault();
                if (target != null)
                {
                    await _navigationService.NavigateToAsync<PlaylistDetailPage>(target);
                }
            })
        });

        _playlistContextMenuItems.Add(new ContextMenuItem
        {
            Text = "Abspielen",
            Icon = FluentIcons.Play12,
            Command = new Command(async () => await _mediaActions.PlayMediaAsync(GetPlaylistsForAction()))
        });

        _playlistContextMenuItems.Add(new ContextMenuItem
        {
            Text = "Als Nächstes spielen",
            Icon = FluentIcons.ArrowForward16,
            Command = new Command(async () => await _mediaActions.PlayMediaNextAsync(GetPlaylistsForAction()))
        });

        _playlistContextMenuItems.Add(new ContextMenuItem
        {
            Text = "Als Letztes spielen",
            Icon = FluentIcons.ArrowNext12,
            Command = new Command(async () => await _mediaActions.PlayMediaLastAsync(GetPlaylistsForAction()))
        });

        _playlistContextMenuItems.Add(new ContextMenuItem { IsSeparator = true });

        _playlistContextMenuItems.Add(new ContextMenuItem
        {
            Text = "Zu Favoriten hinzufügen",
            Icon = FluentIcons.Heart12,
            Command = new Command(async () => await _mediaActions.AddToFavoritesAsync(GetPlaylistsForAction()))
        });

        _playlistContextMenuItems.Add(new ContextMenuItem
        {
            Text = "Aus Favoriten entfernen",
            Icon = FluentFilledIcons.Heart12Filled,
            IconIsFilled = true,
            Command = new Command(async () => await _mediaActions.RemoveFromFavoritesAsync(GetPlaylistsForAction()))
        });
    }

    private IReadOnlyList<Playlist> GetPlaylistsForAction()
    {
        var selected = _playlists.Where(playlist => playlist.IsSelected).ToList();
        if (selected.Count > 0)
        {
            return selected;
        }

        return _contextPlaylist == null ? Array.Empty<Playlist>() : new[] { _contextPlaylist };
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
