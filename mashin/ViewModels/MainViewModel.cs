using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
using MauiIcons.Fluent;
using Microsoft.Extensions.Logging;
using Microsoft.Maui.Controls;
using Microsoft.Maui.Dispatching;
using Sendspin.SDK.Connection;
using Sendspin.SDK.Models;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Windows.Input;
using mashin.Collections;

namespace mashin.ViewModels;

public sealed class MainViewModel : INotifyPropertyChanged, IAsyncDisposable
{
    #region Fields

    private readonly SettingsService _settings;
    private readonly MusicAssistantService _musicAssistant;
    private readonly IUserDataService _userDataService;
    private readonly IPlayerService _playerService;
    private readonly INavigationService _navigationService;
    private readonly IOverlayService _overlayService;
    private readonly IPlaylistStoreService _playlistStore;
    private readonly IContextMenuService _contextMenuService;
    private readonly ILogger<MainViewModel> _logger;

    private readonly IDispatcherTimer _positionTimer;

    private bool _isPlaying;
    private bool _isBuffering;
    private bool _isNavigating;

    private double _duration;
    private double _position;
    private bool _isSeeking;

    private int _volume = 50;
    private bool _suppressVolumeCommand;

    private string _searchQuery = string.Empty;
    private bool _isSearching;

    private Track? _currentTrack;
    private IReadOnlyList<Track> _currentQueueTracks = Array.Empty<Track>();

    private ObservableCollection<ContextMenuItem> _userMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _queueContextMenuItems = new();

    private NavigationSection _currentSection = NavigationSection.Home;

    public event EventHandler<Track?>? CurrentTrackChanged;

    #endregion

    #region Types

    public enum NavigationSection
    {
        None,
        Home,
        Explore,
        Favorites
    }

    #endregion

    #region Construction

    public MainViewModel(
        SettingsService settings,
        MusicAssistantService musicAssistant,
        IUserDataService userDataService,
        IPlayerService playerService,
        INavigationService navigationService,
        IOverlayService overlayService,
        IPlaylistStoreService playlistStore,
        IMediaItemActions mediaActions,
        IContextMenuService contextMenuService,
        ILogger<MainViewModel> logger)
    {
        _settings = settings;
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _playerService = playerService;
        _navigationService = navigationService;
        _overlayService = overlayService;
        _playlistStore = playlistStore;
        _contextMenuService = contextMenuService;
        _logger = logger;
        MediaActions = mediaActions;

        BuildUserMenuItems();
        BuildQueueContextMenuItems();

        // Navigation Commands
        NavigateToHomeCommand = new Command(async () => await navigationService.NavigateToAsync<HomePage>());
        NavigateToExploreCommand = new Command(async () => await navigationService.NavigateToAsync<ExplorePage>());
        NavigateToFavoritesCommand = new Command(async () => await navigationService.NavigateToAsync<FavoritesPage>());
        NavigateToPlaylistCommand = new Command<Playlist>(async (playlist) => await navigationService.NavigateToAsync<PlaylistDetailPage>(playlist));

        SearchCommand = new Command(async () => await ExecuteSearchAsync());
        
        // Overlay Commands
        ShowCreatePlaylistOverlayCommand = new Command(async () => await ExecuteShowCreatePlaylistOverlayAsync());

        // User Icon Menu Command
        ShowUserMenuCommand = new Command<View>(async (anchor) => {
        if (_userMenuItems?.Count > 0 && anchor != null)
            {
                await _contextMenuService.ShowContextMenuAsync(_userMenuItems, anchor);
            }
        });

        // Queue Context Menu Commands
        ShowQueueContextMenuAtAnchorCommand = new Command<View>(async (anchor) =>
        {
            if (anchor == null)
            {
                return;
            }

            BuildQueueContextMenuItems();

            if (_queueContextMenuItems.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(_queueContextMenuItems, anchor);
            }
        });

        ShowQueueContextMenuAtPositionCommand = new Command<Point>(async (position) =>
        {
            BuildQueueContextMenuItems();

            if (_queueContextMenuItems.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(_queueContextMenuItems, position);
            }
        });

        AlbumTappedCommand = new Command<object>(async parameter =>
            await _navigationService.NavigateToAsync<AlbumDetailPage>(parameter));

        ArtistTappedCommand = new Command<object>(async parameter =>
            await _navigationService.NavigateToAsync<ArtistDetailPage>(parameter));

        // Playback Commands
        PreviousTrackCommand = new Command(async () => await PreviousTrackAsync());
        NextTrackCommand = new Command(async () => await NextTrackAsync());
        TogglePlayPauseCommand = new Command(async () => await TogglePlayPauseAsync());
        SeekCommand = new Command<double>(async seconds => await SeekAsync(seconds));
        PlayPlaylistCommand = new Command<Playlist>(async playlist => await PlayPlaylistAsync(playlist));

        // Timer used as fallback / smoothing when the server does not push position updates frequently enough.
        _positionTimer = (Application.Current?.Dispatcher ?? Dispatcher.GetForCurrentThread() ?? throw new InvalidOperationException("No dispatcher available for MainViewModel.")).CreateTimer();
        _positionTimer.Interval = TimeSpan.FromMilliseconds(500);
        _positionTimer.Tick += OnPositionTimerTick;
        _positionTimer.Start();

        _navigationService.PropertyChanged += OnNavigationServicePropertyChanged;
        IsNavigating = _navigationService.IsNavigating;

        _musicAssistant.LoginRequired += OnLoginRequired;
        _playlistStore.PropertyChanged += OnPlaylistStorePropertyChanged;

        // Subscribe to player state; these events drive most UI state.
        _playerService.ConnectionStateChanged += OnConnectionStateChanged;
        _playerService.GroupStateChanged += OnGroupStateChanged;
    }

    #endregion

    #region Bindable Properties (Navigation)

    public NavigationSection CurrentSection
    {
        get => _currentSection;
        set => SetProperty(ref _currentSection, value);
    }

    public bool IsNavigating
    {
        get => _isNavigating;
        private set => SetProperty(ref _isNavigating, value);
    }

    #endregion

    #region Bindable Properties (Playlists)

    public ObservableRangeCollection<Playlist> Playlists
    {
        get => _playlistStore.Playlists;
    }

    public bool IsLoadingPlaylists
    {
        get => _playlistStore.IsLoading;
    }

    #endregion

    #region Bindable Properties (Playback)

    public bool IsPlaying
    {
        get => _isPlaying;
        private set => SetProperty(ref _isPlaying, value);
    }

    public bool IsBuffering
    {
        get => _isBuffering;
        private set => SetProperty(ref _isBuffering, value);
    }

    public double Duration
    {
        get => _duration;
        private set
        {
            if (SetProperty(ref _duration, value))
            {
                OnPropertyChanged(nameof(DurationText));
            }
        }
    }

    public double Position
    {
        get => _position;
        private set
        {
            if (SetProperty(ref _position, value))
            {
                OnPropertyChanged(nameof(PositionText));
            }
        }
    }

    public string DurationText => FormatTime(Duration);

    public string PositionText => FormatTime(Position);

    public int Volume
    {
        get => _volume;
        set
        {
            if (!SetProperty(ref _volume, value))
            {
                return;
            }

            // Prevent volume echo: update from server should not send a "set_volume" back.
            if (_suppressVolumeCommand)
            {
                return;
            }

            _ = SetVolumeAsync(value);
        }
    }

    public Track? CurrentTrack
    {
        get => _currentTrack;
        private set
        {
            var oldTrack = _currentTrack;

            if (SetProperty(ref _currentTrack, value))
            {
                if (oldTrack?.Uri != value?.Uri)
                {
                    CurrentTrackChanged?.Invoke(this, value);
                }
            }
        }
    }

    public IReadOnlyList<Track> CurrentQueueTracks
    {
        get => _currentQueueTracks;
        private set => SetProperty(ref _currentQueueTracks, value);
    }

    #endregion

    #region Bindable Properties (Search)

    public string SearchQuery
    {
        get => _searchQuery;
        set => SetProperty(ref _searchQuery, value);
    }

    #endregion

    #region Commands

    // Navigaton Commands
    public ICommand NavigateToHomeCommand { get; }
    public ICommand NavigateToExploreCommand { get; }
    public ICommand NavigateToFavoritesCommand { get; }
    public ICommand NavigateToPlaylistCommand { get; }

    // Playback Commands
    public ICommand PreviousTrackCommand { get; }
    public ICommand NextTrackCommand { get; }
    public ICommand TogglePlayPauseCommand { get; }
    public ICommand SeekCommand { get; }

    // Others
    public ICommand PlayPlaylistCommand { get; }
    public ICommand SearchCommand { get; }
    public ICommand ShowCreatePlaylistOverlayCommand { get; }
    public ICommand ShowUserMenuCommand { get; }
    public ICommand ShowQueueContextMenuAtAnchorCommand { get; }
    public ICommand ShowQueueContextMenuAtPositionCommand { get; }
    public ICommand AlbumTappedCommand { get; }
    public ICommand ArtistTappedCommand { get; }
    public IMediaItemActions MediaActions { get; }

    #endregion

    #region Search

    private async Task ExecuteSearchAsync()
    {
        if (_isSearching)
        {
            return;
        }

        var query = SearchQuery?.Trim();
        if (string.IsNullOrWhiteSpace(query))
        {
            return;
        }

        try
        {
            _isSearching = true;
            var request = new SearchRequest(
                query,
                new[] { MediaType.Track, MediaType.Album, MediaType.Playlist, MediaType.Artist });

            await _navigationService.NavigateToAsync<SearchPage>(request);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Search failed for query: {Query}", SearchQuery);
        }
        finally
        {
            _isSearching = false;
        }
    }

    #endregion

    #region Lifecycle

    public async Task InitializeAsync()
    {
        // Establish connection to Sendspin server (if configured).
        if (!string.IsNullOrWhiteSpace(_settings.SendspinUrl))
        {
            var uri = new Uri(_settings.SendspinUrl);
            await _playerService.ConnectAsync(uri);
        }

        await _playlistStore.RefreshAsync();
    }

    public async ValueTask DisposeAsync()
    {
        _positionTimer.Tick -= OnPositionTimerTick;
        _positionTimer.Stop();

        _musicAssistant.LoginRequired -= OnLoginRequired;

        _playerService.ConnectionStateChanged -= OnConnectionStateChanged;
        _playerService.GroupStateChanged -= OnGroupStateChanged;
        _playlistStore.PropertyChanged -= OnPlaylistStorePropertyChanged;

        _navigationService.PropertyChanged -= OnNavigationServicePropertyChanged;

        await Task.CompletedTask;
    }

    #endregion

    #region Login Overlay

    private async void OnLoginRequired(object? sender, EventArgs e)
    {
        await _overlayService.ShowLoginAsync(
            _settings.Username,
            async (username, password) =>
            {
                try
                {
                    _logger.LogInformation("Attempting login for user: {Username}", username);
                    var success = await _musicAssistant.LoginAsync(username, password);

                    if (success)
                    {
                        _logger.LogInformation("Login successful for user: {Username}", username);
                        return (true, null);
                    }

                    _logger.LogWarning("Login failed for user: {Username}", username);
                    return (false, "Anmeldung fehlgeschlagen. Bitte überprüfen Sie Ihre Anmeldedaten.");
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "Login error for user: {Username}", username);
                    return (false, $"Verbindungsfehler: {ex.Message}");
                }
            });
    }

    #endregion

    #region Create Playlist Overlay

    private async Task ExecuteShowCreatePlaylistOverlayAsync()
    {
        var name = await _overlayService.ShowCreatePlaylistAsync();
        if (string.IsNullOrWhiteSpace(name))
        {
            return;
        }

        await _playlistStore.CreateAsync(name);
    }

    #endregion

    #region Playlist Logic

    private async Task PlayPlaylistAsync(Playlist playlist)
    {
        if (playlist is null)
        {
            return;
        }

        // Without a client id, we cannot send playback commands.
        if (string.IsNullOrEmpty(_playerService.ClientId))
        {
            _logger.LogWarning("ClientId is not available. Player connection is missing.");
            return;
        }

        try
        {
            _logger.LogInformation("Play playlist: {Name}", playlist.Name);

            await _musicAssistant.PlayMediaAsync(
                _playerService.ClientId,
                new List<MediaItem> { playlist },
                QueueOption.Play);

            IsBuffering = true;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to play playlist: {Name}", playlist.Name);
        }
    }

    #endregion

    #region Player Control Logic

    private async Task TogglePlayPauseAsync()
    {
        try
        {
            var command = IsPlaying ? "pause" : "play";
            await _playerService.SendCommandAsync(command);
            IsBuffering = true;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to toggle play/pause");
        }
    }

    private async Task NextTrackAsync()
    {
        try
        {
            await _playerService.SendCommandAsync("next");
            IsBuffering = true;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to go next");
        }
    }

    private async Task PreviousTrackAsync()
    {
        try
        {
            await _playerService.SendCommandAsync("previous");
            IsBuffering = true;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to go previous");
        }
    }

    private async Task SeekAsync(double seconds)
    {
        try
        {
            _isSeeking = true;

            var clamped = Math.Max(0, Math.Min(Duration, seconds));
            Position = clamped;

            await _playerService.SendCommandAsync(
                "seek",
                new Dictionary<string, object>
                {
                    ["position"] = clamped,
                });
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to seek");
        }
        finally
        {
            _isSeeking = false;
        }
    }

    private async Task SetVolumeAsync(int volume)
    {
        try
        {
            var clamped = Math.Max(0, Math.Min(100, volume));

            await _playerService.SendCommandAsync(
                "set_volume",
                new Dictionary<string, object>
                {
                    ["volume"] = clamped,
                });
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to set volume");
        }
    }

    #endregion

    #region Player State Updates

    private void OnPositionTimerTick(object? sender, EventArgs e)
    {
        // When server position exists: timer is only used as fallback / smoothing.
        if (_isSeeking || !IsPlaying || Duration <= 0)
        {
            return;
        }

        Position = Math.Min(Duration, Position + _positionTimer.Interval.TotalSeconds);
    }

    private void OnConnectionStateChanged(object? sender, ConnectionStateChangedEventArgs e)
    {
        _logger.LogInformation("Sendspin connection state: {State}", e.NewState);

        if (e.NewState != ConnectionState.Connected)
        {
            IsBuffering = false;
            IsPlaying = false;
        }
    }

    private void OnNavigationServicePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(INavigationService.IsNavigating))
        {
            IsNavigating = _navigationService.IsNavigating;
        }
    }

    private void OnPlaylistStorePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(IPlaylistStoreService.IsLoading))
        {
            OnPropertyChanged(nameof(IsLoadingPlaylists));
        }
    }

    private async void OnGroupStateChanged(object? sender, GroupState group)
    {
        try
        {
            // Update view model state
            var state = group.PlaybackState.ToString();
            IsPlaying = state.Equals("Playing", StringComparison.OrdinalIgnoreCase);
            IsBuffering =
                state.Equals("Buffering", StringComparison.OrdinalIgnoreCase)
                || state.Equals("Loading", StringComparison.OrdinalIgnoreCase);

            // Accept server volume without re-sending "set_volume" back.
            _suppressVolumeCommand = true;
            try
            {
                Volume = Math.Max(0, Math.Min(100, group.Volume));
            }
            finally
            {
                _suppressVolumeCommand = false;
            }

            // Map metadata.
            var md = group.Metadata;
            if (md is null)
            {
                CurrentTrack = null;
                CurrentQueueTracks = Array.Empty<Track>();

                Duration = 0;
                if (!_isSeeking)
                {
                    Position = 0;
                }

                return;
            }

            // Get full track metadata from Music Assistant if track changed
            if (!string.IsNullOrEmpty(md.Uri) && md.Uri != CurrentTrack?.Uri)
            {
                // TODO: Sendspin metadata Uri not implemented yet, so track change detection does not work (uri is allways null)
            }
            else
            {
                // Get track metadata every time
                if (!string.IsNullOrEmpty(_playerService.ClientId))
                {
                    try
                    {
                        var activeQueue = await _musicAssistant.GetActiveQueueForPlayerAsync(_playerService.ClientId);

                        // Set CurrentTrack, only if changed (based on uri or id) to avoid unnecessary UI updates
                        if (activeQueue?.CurrentItem?.MediaItem != null)
                        {
                            var nextCurrentTrack = activeQueue.CurrentItem.MediaItem;

                            var isSameCurrentTrack =
                                (!string.IsNullOrWhiteSpace(CurrentTrack?.Uri)
                                 && !string.IsNullOrWhiteSpace(nextCurrentTrack.Uri)
                                 && string.Equals(CurrentTrack.Uri, nextCurrentTrack.Uri, StringComparison.OrdinalIgnoreCase))
                                || (!string.IsNullOrWhiteSpace(CurrentTrack?.ItemId)
                                    && !string.IsNullOrWhiteSpace(nextCurrentTrack.ItemId)
                                    && string.Equals(CurrentTrack.ItemId, nextCurrentTrack.ItemId, StringComparison.Ordinal));

                            if (!isSameCurrentTrack)
                            {
                                await _musicAssistant.EnrichWithProviderInfoAsync(new List<Track> { nextCurrentTrack });
                                CurrentTrack = nextCurrentTrack;
                            }

                            if (!string.IsNullOrWhiteSpace(activeQueue.QueueId))
                            {
                                // Set CurrentQueueTracks only, when queue changed (based on track uris) to avoid unnecessary UI updates.
                                var queueItems = await _musicAssistant.GetQueueItemsAsync(activeQueue.QueueId);
                                var nextQueueTracks = queueItems
                                    .Select(queueItem => queueItem.MediaItem)
                                    .OfType<Track>()
                                    .ToList();

                                for (var index = 0; index < nextQueueTracks.Count; index++)
                                {
                                    var track = nextQueueTracks[index];
                                    track.Index = index + 1;
                                    track.Favorite = _userDataService.IsFavorite(track);
                                }

                                var currentQueueTrackKeys = _currentQueueTracks
                                    .Select(track => !string.IsNullOrWhiteSpace(track.Uri)
                                        ? $"uri:{track.Uri.Trim().ToUpperInvariant()}"
                                        : $"id:{track.ItemId}")
                                    .ToList();

                                var nextQueueTrackKeys = nextQueueTracks
                                    .Select(track => !string.IsNullOrWhiteSpace(track.Uri)
                                        ? $"uri:{track.Uri.Trim().ToUpperInvariant()}"
                                        : $"id:{track.ItemId}")
                                    .ToList();

                                if (!currentQueueTrackKeys.SequenceEqual(nextQueueTrackKeys))
                                {
                                    await _musicAssistant.EnrichWithProviderInfoAsync(nextQueueTracks);
                                    CurrentQueueTracks = nextQueueTracks;
                                }
                            }
                            else
                            {
                                CurrentQueueTracks = Array.Empty<Track>();
                            }

                            _logger.LogDebug("Retrieved current track from Music Assistant queue: {Name} by {Artist}",
                                CurrentTrack.Name, CurrentTrack.ArtistName);
                        }
                        else
                        {
                            _logger.LogDebug("No current item found in Music Assistant queue for player: {PlayerId}",
                                _playerService.ClientId);

                            CurrentTrack = null;
                            CurrentQueueTracks = Array.Empty<Track>();
                        }
                    }
                    catch (Exception ex)
                    {
                        _logger.LogWarning(ex, "Failed to retrieve current track from Music Assistant queue");
                        CurrentTrack = null;
                        CurrentQueueTracks = Array.Empty<Track>();
                    }
                }
            }

            // Duration/position come from track metadata (seconds).
            Duration = md.Duration is > 0 ? md.Duration.Value : 0;

            if (!_isSeeking)
            {
                var pos = md.Position ?? 0;
                Position = Duration > 0 ? Math.Max(0, Math.Min(Duration, pos)) : Math.Max(0, pos);
            }
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to process GroupState update");
        }
    }

    #endregion

    #region User Menu

    private void BuildUserMenuItems()
    {
        _userMenuItems.Clear();

        _userMenuItems.Add(new ContextMenuItem
        {
            Text = "Logout",
            Command = new Command(async () => await ExecuteLogoutAsync())
        });
    }

    #endregion

    #region Queue Context Menu

    private void BuildQueueContextMenuItems()
    {
        _queueContextMenuItems = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () =>
                    await MediaActions.PlayMediaAsync(CurrentQueueTracks.Where(track => track.IsSelected)))
            },
            new()
            {
                Text = "Als Nächstes spielen",
                Icon = FluentIcons.Add12,
                Command = new Command(async () =>
                    await MediaActions.PlayMediaNextAsync(CurrentQueueTracks.Where(track => track.IsSelected)))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.Add12,
                Command = new Command(async () =>
                    await MediaActions.PlayMediaLastAsync(CurrentQueueTracks.Where(track => track.IsSelected)))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Wiedergabeliste hinzufügen",
                Icon = FluentIcons.Add12,
                SubItems = BuildQueuePlaylistSubItems()
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufügen",
                Icon = FluentIcons.Add12,
                Command = new Command(async () =>
                    await MediaActions.AddToFavoritesAsync(CurrentQueueTracks.Where(track => track.IsSelected)))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentIcons.Add12,
                Command = new Command(async () =>
                    await MediaActions.RemoveFromFavoritesAsync(CurrentQueueTracks.Where(track => track.IsSelected)))
            }
        };
    }

    private ObservableRangeCollection<ContextMenuItem> BuildQueuePlaylistSubItems()
    {
        var items = new ObservableRangeCollection<ContextMenuItem>();

        foreach (var playlist in _playlistStore.Playlists)
        {
            if (playlist.Name.StartsWith("~", StringComparison.Ordinal))
            {
                continue;
            }

            items.Add(new ContextMenuItem
            {
                Text = playlist.DisplayName,
                Icon = FluentIcons.Add12,
                Command = new Command(async () =>
                    await MediaActions.AddToPlaylistAsync(CurrentQueueTracks.Where(track => track.IsSelected), playlist))
            });
        }

        return items;
    }

    private async Task ExecuteLogoutAsync()
    {
        try
        {
            await _musicAssistant.LogoutAsync();
            await _musicAssistant.TryAutoLoginAsync();
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Logout failed");
        }
    }

    #endregion


    #region Helpers (INotifyPropertyChanged)

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

    #region Helpers (Formatting)

    private static string FormatTime(double seconds)
    {
        if (seconds <= 0 || double.IsNaN(seconds) || double.IsInfinity(seconds))
        {
            return "0:00";
        }

        var ts = TimeSpan.FromSeconds(seconds);
        return ts.Hours > 0 ? ts.ToString(@"h\:mm\:ss") : ts.ToString(@"m\:ss");
    }

    #endregion
}