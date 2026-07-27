using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
using MauiIcons.Fluent;
using MauiIcons.Fluent.Filled;
using Microsoft.Extensions.Logging;
using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Windows.Input;

namespace mashin.ViewModels;

public sealed class PlaylistsViewModel : INotifyPropertyChanged, INavigationAware, IDisposable
{
    #region Fields

    private readonly IPlaylistService _playlistService;
    private readonly SettingsService _settings;
    private readonly IOverlayService _overlayService;
    private readonly INavigationService _navigationService;
    private readonly IMediaItemActions _mediaActions;
    private readonly PlaybackService _playbackService;
    private readonly IContextMenuService _contextMenuService;
    private readonly ILogger<PlaylistsViewModel> _logger;
    private readonly ObservableCollection<ContextMenuItem> _playlistContextMenuItems = new();
    private readonly IReadOnlyList<TableViewSkeleton> _playlistSkeletons = Enumerable.Range(0, 12)
        .Select(_ => new TableViewSkeleton())
        .ToList();

    private bool _isLoading;
    private bool _disposed;
    private Playlist? _contextPlaylist;

    #endregion

    #region Construction

    public PlaylistsViewModel(
        IPlaylistService playlistService,
        SettingsService settings,
        IOverlayService overlayService,
        INavigationService navigationService,
        IMediaItemActions mediaActions,
        PlaybackService playbackService,
        IContextMenuService contextMenuService,
        ILogger<PlaylistsViewModel> logger)
    {
        _playlistService = playlistService;
        _settings = settings;
        _overlayService = overlayService;
        _navigationService = navigationService;
        _mediaActions = mediaActions;
        _playbackService = playbackService;
        _contextMenuService = contextMenuService;
        _logger = logger;
        _playlistService.PropertyChanged += OnPlaylistServicePropertyChanged;
        _playlistService.Playlists.CollectionChanged += OnPlaylistsCollectionChanged;
        IsLoading = _playlistService.IsLoading;

        // Navigation command
        PlaylistTappedCommand = new Command<Playlist>(async playlist =>
            await _navigationService.NavigateToAsync<PlaylistDetailPage>(playlist));

        CreatePlaylistCommand = new Command(async () => await CreatePlaylistAsync());

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
            var hasSelection = _playlistService.Playlists.Any(playlist => playlist.IsSelected);
            if (_contextPlaylist == null && !hasSelection)
            {
                return;
            }

            BuildPlaylistContextMenuItems();

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

    public bool HasPlaylists => _playlistService.Playlists.Count > 0;

    public bool ShowPlaylistListView => IsLoading || HasPlaylists;

    public bool ShowNoPlaylistsMessage => !IsLoading && !HasPlaylists;

    public IEnumerable<object> PlaylistItems => IsLoading
        ? _playlistSkeletons
        : _playlistService.Playlists;

    #endregion

    #region Commands

    public ICommand PlaylistTappedCommand { get; }

    public ICommand CreatePlaylistCommand { get; }

    public ICommand PlaylistLongPressedCommand { get; }

    public ICommand ShowPlaylistContextMenuAtAnchorCommand { get; }

    #endregion

    #region Lifecycle

    public async Task OnNavigatedToAsync(object? parameter)
    {
        await _playlistService.RefreshAsync();
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

        _playlistService.PropertyChanged -= OnPlaylistServicePropertyChanged;
        _playlistService.Playlists.CollectionChanged -= OnPlaylistsCollectionChanged;
        _disposed = true;
    }

    #endregion

    #region Loading

    #endregion

    #region Playlist Creation

    private async Task CreatePlaylistAsync()
    {
        var name = await _overlayService.ShowCreatePlaylistAsync();
        if (string.IsNullOrWhiteSpace(name))
        {
            return;
        }

        var username = _settings.Username;
        var prefix = string.IsNullOrWhiteSpace(username)
            ? null
            : string.Concat(username, "--");

        if (!string.IsNullOrWhiteSpace(prefix)
            && !name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
        {
            name = string.Concat(prefix, name);
        }

        try
        {
            await _playlistService.CreatePlaylistAsync(name);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to create playlist: {PlaylistName}", name);
        }
    }

    #endregion

    #region Context Menu

    private void BuildPlaylistContextMenuItems()
    {
        _playlistContextMenuItems.Clear();

        _playlistContextMenuItems.Add(new ContextMenuItem
        {
            Text = "Abspielen",
            Icon = FluentIcons.Play12,
            Command = new Command(async () => await _playbackService.PlayMediaAsync(GetPlaylistsForAction().Cast<MediaItem>().ToList()))
        });

        _playlistContextMenuItems.Add(new ContextMenuItem
        {
            Text = "Als Nächstes spielen",
            Icon = FluentIcons.ArrowForward16,
            Command = new Command(async () => await _playbackService.PlayMediaNextAsync(GetPlaylistsForAction().Cast<MediaItem>().ToList()))
        });

        _playlistContextMenuItems.Add(new ContextMenuItem
        {
            Text = "Als Letztes spielen",
            Icon = FluentIcons.ArrowNext12,
            Command = new Command(async () => await _playbackService.PlayMediaLastAsync(GetPlaylistsForAction().Cast<MediaItem>().ToList()))
        });

        _playlistContextMenuItems.Add(new ContextMenuItem { IsSeparator = true });

        var targets = GetPlaylistsForAction();
        var shouldShowRemoveFromFavorites = targets.Count > 0 && targets.All(playlist => playlist.Favorite);

        if (shouldShowRemoveFromFavorites)
        {
            _playlistContextMenuItems.Add(new ContextMenuItem
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () => await _mediaActions.RemoveFromFavoritesAsync(GetPlaylistsForAction()))
            });
            return;
        }

        _playlistContextMenuItems.Add(new ContextMenuItem
        {
            Text = "Zu Favoriten hinzufügen",
            Icon = FluentIcons.Heart12,
            Command = new Command(async () => await _mediaActions.AddToFavoritesAsync(GetPlaylistsForAction()))
        });
    }

    private IReadOnlyList<Playlist> GetPlaylistsForAction()
    {
        var selected = _playlistService.Playlists.Where(playlist => playlist.IsSelected).ToList();
        if (selected.Count > 0)
        {
            return selected;
        }

        return _contextPlaylist == null ? Array.Empty<Playlist>() : new[] { _contextPlaylist };
    }

    private void OnPlaylistServicePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(IPlaylistService.IsLoading))
        {
            IsLoading = _playlistService.IsLoading;
        }
    }

    private void OnPlaylistsCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        OnPropertyChanged(nameof(HasPlaylists));
        OnPropertyChanged(nameof(ShowPlaylistListView));
        OnPropertyChanged(nameof(ShowNoPlaylistsMessage));
        OnPropertyChanged(nameof(PlaylistItems));
    }

    #endregion

    #region INotifyPropertyChanged

    public event PropertyChangedEventHandler? PropertyChanged;

    private void OnPropertyChanged([CallerMemberName] string? propertyName = null)
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));

        if (propertyName == nameof(IsLoading) && !IsLoading)
        {
            _navigationService.IsNavigating = false;
        }
    }

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

