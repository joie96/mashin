using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
using MauiIcons.Fluent;
using Microsoft.Extensions.Logging;
using Microsoft.Maui.Controls;
using Microsoft.Maui.Dispatching;
using Microsoft.Maui.Graphics;
using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Input;
using mashin.Collections;
using MauiIcons.Fluent.Filled;

namespace mashin.ViewModels;

public sealed class MainViewModel : INotifyPropertyChanged, IAsyncDisposable
{
    #region Fields

    // Services
    private readonly SettingsService _settings;
    private readonly MusicAssistantService _musicAssistant;
    private readonly IPlaylistService _playlistService;
    private readonly INavigationService _navigationService;
    private readonly IOverlayService _overlayService;
    private readonly IContextMenuService _contextMenuService;
    private readonly PlaybackService _playbackService;
    private readonly IConnectionService _connectionService;
    private readonly ILogger<MainViewModel> _logger;
    private readonly ObservableRangeCollection<Playlist> _playlists;


    // Player
    private bool _isSeeking;
    private double _sliderPosition;
    private bool _isAudioOptionsFlyoutOpen;
    private bool _isDeviceSelectionFlyoutOpen;
    private bool _isLoadingPlayers;
    private string? _selectedPlayerId;
    private bool _suppressSelectedPlayerChange;
    private string _selectedAudioQuality = "opus";
    private readonly ObservableRangeCollection<Player> _availablePlayers = new();

    private bool _isDarkTheme;
    private bool _isLoadingPlaylists;

    // Search
    private string _searchQuery = string.Empty;
    private bool _isSearching;

    // Queue
    // Context Menus
    private ObservableCollection<ContextMenuItem> _userMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _queueContextMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _currentTrackContextMenuItems = new();

    // Navigation
    private bool _isNavigating;
    private NavigationSection _currentSection = NavigationSection.Home;
    private bool _isLoginOverlayActive;
    private int _connectionStateDisplayVersion;
    private string _connectionState = string.Empty;

    public event Func<Task>? CloseQueueViewRequested;
    public event EventHandler? SearchSubmitted;

    #endregion

    #region Types

    public enum NavigationSection
    {
        None,
        Home,
        Explore,
        Favorites,
        Playlists,
        Search
    }

    #endregion

    #region Construction

    public MainViewModel(
        SettingsService settings,
        MusicAssistantService musicAssistant,
        IPlaylistService playlistService,
        INavigationService navigationService,
        IOverlayService overlayService,
        IMediaItemActions mediaActions,
        IContextMenuService contextMenuService,
        PlaybackService playbackService,
        IConnectionService connectionService,
        ILogger<MainViewModel> logger)
    {
        _settings = settings;
        _musicAssistant = musicAssistant;
        _playlistService = playlistService;
        _navigationService = navigationService;
        _overlayService = overlayService;
        _contextMenuService = contextMenuService;
        _playbackService = playbackService;
        _connectionService = connectionService;
        _logger = logger;
        _playlists = _playlistService.Playlists;
        MediaActions = mediaActions;
        _selectedAudioQuality = _settings.GetSendspinPreferredAudioCodec();
        _sliderPosition = _playbackService.PositionSeconds;
        _playbackService.CurrentQueueItems.CollectionChanged += OnCurrentQueueItemsCollectionChanged;
        _availablePlayers.CollectionChanged += OnAvailablePlayersCollectionChanged;
        _playlists.CollectionChanged += OnPlaylistsCollectionChanged;
        _playlistService.PropertyChanged += OnPlaylistServicePropertyChanged;

        // Navigation Commands
        NavigateToHomeCommand = new Command(async () => await NavigateToSectionAsync<HomePage>(NavigationSection.Home));
        NavigateToExploreCommand = new Command(async () => await NavigateToSectionAsync<ExplorePage>(NavigationSection.Explore));
        NavigateToFavoritesCommand = new Command(async () => await NavigateToSectionAsync<FavoritesPage>(NavigationSection.Favorites));
        NavigateToPlaylistsCommand = new Command(async () => await NavigateToSectionAsync<PlaylistsPage>(NavigationSection.Playlists));
        NavigateToSearchCommand = new Command(async () => await NavigateToSectionAsync<SearchPage>(NavigationSection.Search));
        NavigateToPlaylistCommand = new Command<Playlist>(async playlist =>
            await NavigateToSectionAsync<PlaylistDetailPage>(NavigationSection.Playlists, playlist));

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

        ShowCurrentTrackContextMenuCommand = new Command<View>(async anchor =>
        {
            if (anchor == null || CurrentQueueItem?.MediaItem == null)
            {
                return;
            }

            if (_currentTrackContextMenuItems.Count == 0)
            {
                return;
            }

            await _contextMenuService.ShowContextMenuAsync(_currentTrackContextMenuItems, anchor);
        });

        // LinkLabel Commands
        AlbumTappedCommand = new Command<object>(async parameter =>
        {
            if (CloseQueueViewRequested is { } closeQueueViewRequested)
            {
                foreach (var callback in closeQueueViewRequested.GetInvocationList().OfType<Func<Task>>())
                {
                    await callback();
                }
            }

            await _navigationService.NavigateToAsync<AlbumDetailPage>(parameter);
        });

        ArtistTappedCommand = new Command<object>(async parameter =>
        {
            if (CloseQueueViewRequested is { } closeQueueViewRequested)
            {
                foreach (var callback in closeQueueViewRequested.GetInvocationList().OfType<Func<Task>>())
                {
                    await callback();
                }
            }

            await _navigationService.NavigateToAsync<ArtistDetailPage>(parameter);
        });

        // Playback Commands
        PreviousTrackCommand = new Command(async () => await PreviousTrackAsync());
        NextTrackCommand = new Command(async () => await NextTrackAsync());
        TogglePlayPauseCommand = new Command(async () => await TogglePlayPauseAsync());
        ToggleShuffleCommand = new Command(async () => await ToggleShuffleAsync());
        ToggleRepeatModeCommand = new Command(async () => await ToggleRepeatModeAsync());
        ToggleMuteCommand = new Command(async () => await ToggleMuteAsync());
        ToggleDontStopTheMusicCommand = new Command(() => IsDontStopTheMusicEnabled = !IsDontStopTheMusicEnabled);
        ToggleAudioOptionsFlyoutCommand = new Command(() => IsAudioOptionsFlyoutOpen = !IsAudioOptionsFlyoutOpen);
        CloseAudioOptionsFlyoutCommand = new Command(() => IsAudioOptionsFlyoutOpen = false);
        ToggleDeviceSelectionFlyoutCommand = new Command(async () => await ToggleDeviceSelectionFlyoutAsync());
        CloseDeviceSelectionFlyoutCommand = new Command(() => IsDeviceSelectionFlyoutOpen = false);
        BeginSeekCommand = new Command<double>(_ => BeginSeek());
        SeekCommand = new Command<double>(async seconds => await SeekAsync(seconds));
        PlayPlaylistCommand = new Command<Playlist>(async playlist => await PlayPlaylistAsync(playlist));

        ToggleCurrentTrackFavoriteCommand = new Command(async () =>
        {
            var currentTrack = CurrentQueueItem?.MediaItem;
            if (currentTrack == null)
            {
                return;
            }

            if (currentTrack.Favorite)
            {
                await MediaActions.RemoveFromFavoritesAsync(currentTrack);
            }
            else
            {
                await MediaActions.AddToFavoritesAsync(currentTrack);
            }
        });

        // Theme Command
        ToggleThemeCommand = new Command(ToggleTheme);

        _navigationService.PropertyChanged += OnNavigationServicePropertyChanged;
        IsNavigating = _navigationService.IsNavigating;

        _musicAssistant.LoginRequired += OnLoginRequired;

        _connectionService.ConnectionStateChanged += OnConnectionServiceStateChanged;
        UpdateConnectionState(_connectionService.ConnectionState);

        // Subscribe to playback state events
        _playbackService.PropertyChanged += OnPlaybackServicePropertyChanged;

        IsLoadingPlaylists = _playlistService.IsLoading;

        var activePlayerId = _playbackService.ActivePlayerId;
        if (!string.IsNullOrWhiteSpace(activePlayerId))
        {
            SetSelectedPlayerSilently(activePlayerId);
        }

        OnPropertyChanged(nameof(PlayState));
        OnPropertyChanged(nameof(Volume));
        OnPropertyChanged(nameof(IsMuted));
        OnPropertyChanged(nameof(ShuffleEnabled));
        OnPropertyChanged(nameof(RepeatMode));
        OnPropertyChanged(nameof(Duration));
        OnPropertyChanged(nameof(Position));
        OnPropertyChanged(nameof(CurrentQueueItem));
        OnPropertyChanged(nameof(CurrentQueueItemPrimaryArtist));
        OnPropertyChanged(nameof(QueueTrackCountText));
        OnPropertyChanged(nameof(QueueTotalDurationText));

        // Keep the UI toggle in sync with persisted theme preference.
        IsDarkTheme = _settings.ThemeMode != AppTheme.Light;
        
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

    public string ConnectionState
    {
        get => _connectionState;
        private set => SetProperty(ref _connectionState, value);
    }

    #endregion

    #region Bindable Properties (Playlists)

    public ObservableRangeCollection<Playlist> Playlists
    {
        get => _playlists;
    }

    public bool IsLoadingPlaylists
    {
        get => _isLoadingPlaylists;
        private set => SetProperty(ref _isLoadingPlaylists, value);
    }

    #endregion

    #region Bindable Properties (Playback)

    public PlayerState PlayState
    {
        get => _playbackService.PlaybackState;
    }

    public bool IsMuted
    {
        get => _playbackService.IsMuted;
    }

    public bool IsDarkTheme
    {
        get => _isDarkTheme;
        private set => SetProperty(ref _isDarkTheme, value);
    }

    public bool? ShuffleEnabled
    {
        get => _playbackService.ShuffleEnabled;
    }

    public string? RepeatMode
    {
        get => _playbackService.RepeatMode;
    }

    public bool IsShuffleActive => ShuffleEnabled == true;

    public bool IsRepeatEnabled => GetNormalizedRepeatMode(RepeatMode) is not mashin.Models.RepeatMode.Off;

    public bool IsRepeatOne => GetNormalizedRepeatMode(RepeatMode) == mashin.Models.RepeatMode.One;

    public double Duration
    {
        get => _playbackService.DurationSeconds;
    }

    public double Position
    {
        get => _isSeeking ? _sliderPosition : _playbackService.PositionSeconds;
    }

    public double SliderPosition
    {
        get => _sliderPosition;
        set => SetProperty(ref _sliderPosition, value);
    }

    public string DurationText => FormatTime(Duration);

    public string PositionText => FormatTime(Position);

    public double Volume
    {
        get => _playbackService.Volume;
        set
        {
            var clamped = (int)Math.Round(Math.Max(0, Math.Min(100, value)));
            if (clamped == _playbackService.Volume)
            {
                return;
            }
            _ = SetVolumeAsync(clamped);
        }
    }

    public QueueItem? CurrentQueueItem => _playbackService.CurrentQueueItem;

    public double PlayerBarProgress => Duration <= 0 ? 0 : Math.Clamp(Position / Duration, 0, 1);

    public Artist? CurrentQueueItemPrimaryArtist => CurrentQueueItem?.MediaItem?.Artists?.FirstOrDefault();

    public ObservableRangeCollection<QueueItem> CurrentQueueItems => _playbackService.CurrentQueueItems;

    public string QueueTrackCountText => $"{_playbackService.QueueItemCount} Titel";

    public string QueueTotalDurationText
    {
        get
        {
            var totalSeconds = CurrentQueueItems
                .Select(item => item.MediaItem)
                .OfType<Track>()
                .Sum(track => Math.Max(0, track.Duration));
            return FormatQueueDuration(totalSeconds);
        }
    }

    public bool IsDontStopTheMusicEnabled
    {
        get => _playbackService.DontStopTheMusicEnabled == true;
        set
        {
            if (value == (_playbackService.DontStopTheMusicEnabled == true))
            {
                return;
            }

            _ = SetDontStopTheMusicAsync(value);
        }
    }

    public bool IsAudioOptionsFlyoutOpen
    {
        get => _isAudioOptionsFlyoutOpen;
        set => SetProperty(ref _isAudioOptionsFlyoutOpen, value);
    }

    public bool IsDeviceSelectionFlyoutOpen
    {
        get => _isDeviceSelectionFlyoutOpen;
        set => SetProperty(ref _isDeviceSelectionFlyoutOpen, value);
    }

    public bool IsLoadingPlayers
    {
        get => _isLoadingPlayers;
        private set => SetProperty(ref _isLoadingPlayers, value);
    }

    public ObservableRangeCollection<Player> AvailablePlayers => _availablePlayers;

    public bool HasAvailablePlayers => _availablePlayers.Count > 0;

    public bool HasNoAvailablePlayers => !HasAvailablePlayers;

    public string? SelectedPlayerId
    {
        get => _selectedPlayerId;
        set
        {
            var previousPlayerId = _selectedPlayerId;
            if (!SetProperty(ref _selectedPlayerId, value))
            {
                return;
            }

            if (_suppressSelectedPlayerChange)
            {
                return;
            }

            _ = SelectActivePlayerAsync(value, previousPlayerId);
        }
    }

    public string SelectedAudioQuality
    {
        get => _selectedAudioQuality;
        set
        {
            if (!SetProperty(ref _selectedAudioQuality, value))
            {
                return;
            }

            OnPropertyChanged(nameof(IsAudioQualityOpus));
            OnPropertyChanged(nameof(IsAudioQualityFlac));
            OnPropertyChanged(nameof(IsAudioQualityPcm));

            _ = ApplyAudioQualityAsync(_selectedAudioQuality);
        }
    }

    public bool IsAudioQualityOpus
    {
        get => string.Equals(SelectedAudioQuality, "opus", StringComparison.OrdinalIgnoreCase);
        set
        {
            if (value)
            {
                SelectedAudioQuality = "opus";
            }
        }
    }

    public bool IsAudioQualityFlac
    {
        get => string.Equals(SelectedAudioQuality, "flac", StringComparison.OrdinalIgnoreCase);
        set
        {
            if (value)
            {
                SelectedAudioQuality = "flac";
            }
        }
    }

    public bool IsAudioQualityPcm
    {
        get => string.Equals(SelectedAudioQuality, "pcm", StringComparison.OrdinalIgnoreCase);
        set
        {
            if (value)
            {
                SelectedAudioQuality = "pcm";
            }
        }
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
    public ICommand NavigateToPlaylistsCommand { get; }
    public ICommand NavigateToSearchCommand { get; }
    public ICommand NavigateToPlaylistCommand { get; }

    // Playback Commands
    public ICommand PreviousTrackCommand { get; }
    public ICommand NextTrackCommand { get; }
    public ICommand TogglePlayPauseCommand { get; }
    public ICommand ToggleShuffleCommand { get; }
    public ICommand ToggleRepeatModeCommand { get; }
    public ICommand ToggleMuteCommand { get; }
    public ICommand ToggleThemeCommand { get; }
    public ICommand ToggleDontStopTheMusicCommand { get; }
    public ICommand ToggleAudioOptionsFlyoutCommand { get; }
    public ICommand CloseAudioOptionsFlyoutCommand { get; }
    public ICommand ToggleDeviceSelectionFlyoutCommand { get; }
    public ICommand CloseDeviceSelectionFlyoutCommand { get; }
    public ICommand ToggleCurrentTrackFavoriteCommand { get; }
    public ICommand ShowCurrentTrackContextMenuCommand { get; }
    public ICommand BeginSeekCommand { get; }
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

        SearchSubmitted?.Invoke(this, EventArgs.Empty);

        try
        {
            _isSearching = true;
            CurrentSection = NavigationSection.Search;
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

    private async Task NavigateToSectionAsync<TPage>(NavigationSection section) where TPage : ContentPage
    {
        CurrentSection = section;
        await _navigationService.NavigateToAsync<TPage>();
    }

    private async Task NavigateToSectionAsync<TPage>(NavigationSection section, object? parameter) where TPage : ContentPage
    {
        CurrentSection = section;
        await _navigationService.NavigateToAsync<TPage>(parameter);
    }

    #region Lifecycle

    public async Task InitializeAsync()
    {
        var autoLoginSucceeded = await _musicAssistant.AutoLoginAsync(raiseLoginRequest: true);
        if (!autoLoginSucceeded)
        {
            _logger.LogWarning("Startup initialization paused because user is not authenticated.");
            return;
        }

        CurrentSection = NavigationSection.Home;
        await _navigationService.NavigateToAsync<HomePage>();

        await RefreshAvailablePlayersAsync();
        ApplyInitialSelectedPlayer();
        try
        {
            await _playbackService.InitializeAsync();
        }
        catch (Exception ex) when (ex is TimeoutException or TaskCanceledException)
        {
            _logger.LogWarning(ex, "Playback initialization failed for Sendspin. Falling back to Local mode.");

            try
            {
                await _playbackService.SetOutputModeAsync(PlaybackOutputMode.Local);
            }
            catch (Exception fallbackEx)
            {
                _logger.LogWarning(fallbackEx, "Failed to switch playback output to Local after initialization error.");
            }
        }

        // Load playlists once and keep local list refreshed as needed.
        await _playlistService.RefreshAsync();

        // Build Context Menus
        BuildUserMenuItems();
        BuildQueueContextMenuItems();
        BuildCurrentTrackContextMenuItems();

        // Sync playback projection (queue + states) after service initialization.
        SliderPosition = _playbackService.PositionSeconds;

        var activePlayerId = _playbackService.ActivePlayerId;
        if (!string.IsNullOrWhiteSpace(activePlayerId))
        {
            SetSelectedPlayerSilently(activePlayerId);
        }

        OnPropertyChanged(nameof(PlayState));
        OnPropertyChanged(nameof(Volume));
        OnPropertyChanged(nameof(IsMuted));
        OnPropertyChanged(nameof(ShuffleEnabled));
        OnPropertyChanged(nameof(RepeatMode));
        OnPropertyChanged(nameof(Duration));
        OnPropertyChanged(nameof(Position));
        OnPropertyChanged(nameof(CurrentQueueItem));
        OnPropertyChanged(nameof(CurrentQueueItemPrimaryArtist));
        OnPropertyChanged(nameof(QueueTrackCountText));
        OnPropertyChanged(nameof(QueueTotalDurationText));

    }

    public async ValueTask DisposeAsync()
    {
        _playlists.CollectionChanged -= OnPlaylistsCollectionChanged;
        _playlistService.PropertyChanged -= OnPlaylistServicePropertyChanged;
        _playbackService.CurrentQueueItems.CollectionChanged -= OnCurrentQueueItemsCollectionChanged;
        _availablePlayers.CollectionChanged -= OnAvailablePlayersCollectionChanged;

        _musicAssistant.LoginRequired -= OnLoginRequired;

        _connectionService.ConnectionStateChanged -= OnConnectionServiceStateChanged;

        _playbackService.PropertyChanged -= OnPlaybackServicePropertyChanged;

        _navigationService.PropertyChanged -= OnNavigationServicePropertyChanged;

        await Task.CompletedTask;
    }

    #endregion

    #region Connection State

    private void OnConnectionServiceStateChanged(object? sender, CustomConnectionState state)
    {
        ExecuteOnMainThread(() =>
        {
            UpdateConnectionState(state);
        });
    }

    private void UpdateConnectionState(CustomConnectionState state)
    {
        var displayVersion = Interlocked.Increment(ref _connectionStateDisplayVersion);

        ConnectionState = state switch
        {
            CustomConnectionState.Offline => "Offline",
            CustomConnectionState.Reconnecting => "Connecting",
            CustomConnectionState.Online => "Connected",
            _ => string.Empty
        };

        if (state == CustomConnectionState.Online)
        {
            _ = HideConnectedStateAsync(displayVersion);
        }
    }

    private async Task HideConnectedStateAsync(int displayVersion)
    {
        try
        {
            await Task.Delay(3000);

            ExecuteOnMainThread(() =>
            {
                if (displayVersion != Volatile.Read(ref _connectionStateDisplayVersion))
                {
                    return;
                }

                if (_connectionService.ConnectionState != CustomConnectionState.Online)
                {
                    return;
                }

                ConnectionState = string.Empty;
            });
        }
        catch (Exception ex)
        {
            _logger.LogDebug(ex, "Failed to hide transient connected state.");
        }
    }

    private static void ExecuteOnMainThread(Action action)
    {
        if (MainThread.IsMainThread)
        {
            action();
            return;
        }

        MainThread.BeginInvokeOnMainThread(action);
    }

    #endregion

    #region Login Event Handler & Overlay

    private async void OnLoginRequired(object? sender, EventArgs e)
    {
        if (_isLoginOverlayActive)
        {
            return;
        }

        _isLoginOverlayActive = true;
        bool loginSucceeded;
        try
        {
            loginSucceeded = await _overlayService.ShowLoginAsync(
                _settings.Username,
                ExecuteLogin);
        }
        finally
        {
            _isLoginOverlayActive = false;
        }

        if (loginSucceeded)
        {
            await InitializeAsync();
        }
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

        var prefix = string.Concat(_settings.Username, "--");
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

    #region Playlist Actions

    private async Task PlayPlaylistAsync(Playlist playlist)
    {
        if (playlist is null)
        {
            return;
        }

        var activePlayerId = _playbackService.ActivePlayerId;
        if (string.IsNullOrWhiteSpace(activePlayerId))
        {
            _logger.LogWarning("No active player available for playlist playback.");
            return;
        }

        try
        {
            _logger.LogInformation("Play playlist: {Name}", playlist.Name);
            await _playbackService.PlayMediaAsync(new List<MediaItem> { playlist });
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to play playlist: {Name}", playlist.Name);
        }
    }

    #endregion

    #region Playback Actions

    private async Task TogglePlayPauseAsync()
    {
        try
        {
            await _playbackService.TogglePlayPauseAsync();
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
            await _playbackService.NextTrackAsync();
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
            await _playbackService.PreviousTrackAsync();
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to go previous");
        }
    }

    private async Task ToggleMuteAsync()
    {
        try
        {
            await _playbackService.ToggleMuteAsync(IsMuted);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to toggle mute");
        }
    }

    private async Task ToggleShuffleAsync()
    {
        try
        {
            await _playbackService.ToggleShuffleAsync(ShuffleEnabled);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to toggle shuffle state");
        }
    }

    private async Task ToggleRepeatModeAsync()
    {
        try
        {
            await _playbackService.ToggleRepeatModeAsync(RepeatMode);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to toggle repeat mode");
        }
    }

    private async Task SeekAsync(double seconds)
    {
        try
        {
            var clamped = Math.Max(0, Math.Min(Duration, seconds));
            SliderPosition = clamped;
            OnPropertyChanged(nameof(Position));
            OnPropertyChanged(nameof(PositionText));
            OnPropertyChanged(nameof(PlayerBarProgress));
            await _playbackService.SeekAsync(clamped, Duration);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to seek");
            _isSeeking = false;
        }
    }

    private void BeginSeek()
    {
        _isSeeking = true;
    }

    private async Task SetVolumeAsync(int volume)
    {
        try
        {
            var clamped = Math.Max(0, Math.Min(100, volume));
            await _playbackService.SetVolumeAsync(clamped);
            _settings.SetInitialVolume(clamped);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to set volume");
        }
    }

    private async Task ApplyAudioQualityAsync(string codec)
    {
        try
        {
            await _playbackService.SetPreferredAudioCodecAsync(codec);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to apply preferred audio codec {Codec}", codec);
        }
    }

    #endregion

    #region Event Handlers

    private void OnPlaybackServicePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(PlaybackService.PlaybackState))
        {
            if (_playbackService.PlaybackState.State != PlayerStateType.Buffering
                && _playbackService.PlaybackState.State != PlayerStateType.Seeking)
            {
                _isSeeking = false;
            }

            OnPropertyChanged(nameof(PlayState));
            return;
        }

        if (e.PropertyName == nameof(PlaybackService.Volume))
        {
            _settings.SetInitialVolume(_playbackService.Volume);
            OnPropertyChanged(nameof(Volume));

            return;
        }

        if (e.PropertyName == nameof(PlaybackService.IsMuted))
        {
            _settings.SetInitialMuted(_playbackService.IsMuted);
            OnPropertyChanged(nameof(IsMuted));
            return;
        }

        if (e.PropertyName == nameof(PlaybackService.ShuffleEnabled))
        {
            OnPropertyChanged(nameof(ShuffleEnabled));
            OnPropertyChanged(nameof(IsShuffleActive));
            return;
        }

        if (e.PropertyName == nameof(PlaybackService.RepeatMode))
        {
            OnPropertyChanged(nameof(RepeatMode));
            OnPropertyChanged(nameof(IsRepeatEnabled));
            OnPropertyChanged(nameof(IsRepeatOne));
            return;
        }

        if (e.PropertyName == nameof(PlaybackService.DurationSeconds))
        {
            if (SliderPosition > Duration)
            {
                SliderPosition = Duration;
            }

            OnPropertyChanged(nameof(Duration));
            OnPropertyChanged(nameof(DurationText));
            OnPropertyChanged(nameof(PlayerBarProgress));

            return;
        }

        if (e.PropertyName == nameof(PlaybackService.PositionSeconds))
        {
            if (!_isSeeking)
            {
                SliderPosition = _playbackService.PositionSeconds;
            }

            OnPropertyChanged(nameof(Position));
            OnPropertyChanged(nameof(PositionText));
            OnPropertyChanged(nameof(PlayerBarProgress));

            return;
        }

        if (e.PropertyName == nameof(PlaybackService.DontStopTheMusicEnabled))
        {
            OnPropertyChanged(nameof(IsDontStopTheMusicEnabled));

            return;
        }

        if (e.PropertyName == nameof(PlaybackService.CurrentQueueItem))
        {
            OnPropertyChanged(nameof(CurrentQueueItem));
            OnPropertyChanged(nameof(CurrentQueueItemPrimaryArtist));
            return;
        }

        if (e.PropertyName == nameof(PlaybackService.CurrentQueueItems)
            || e.PropertyName == nameof(PlaybackService.QueueItemCount))
        {
            OnPropertyChanged(nameof(CurrentQueueItem));
            OnPropertyChanged(nameof(CurrentQueueItemPrimaryArtist));
            OnPropertyChanged(nameof(QueueTrackCountText));
            OnPropertyChanged(nameof(QueueTotalDurationText));
            return;
        }

        if (e.PropertyName == nameof(PlaybackService.ActivePlayerId))
        {
            var activePlayerId = _playbackService.ActivePlayerId;
            if (!string.IsNullOrWhiteSpace(activePlayerId))
            {
                SetSelectedPlayerSilently(activePlayerId);
            }
        }
    }

    private void OnNavigationServicePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(INavigationService.IsNavigating))
        {
            IsNavigating = _navigationService.IsNavigating;
            return;
        }

        if (e.PropertyName == nameof(INavigationService.CurrentPageType))
        {
            var currentPageType = _navigationService.CurrentPageType;
            var isSearchPage = string.Equals(currentPageType?.Name, nameof(SearchPage), StringComparison.Ordinal);

            if (isSearchPage)
            {
                if (CurrentSection != NavigationSection.Search)
                {
                    CurrentSection = NavigationSection.Search;
                }

                return;
            }

            if (CurrentSection == NavigationSection.Search)
            {
                CurrentSection = NavigationSection.None;
            }
        }
    }

    private void OnCurrentQueueItemsCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        OnPropertyChanged(nameof(CurrentQueueItem));
        OnPropertyChanged(nameof(CurrentQueueItemPrimaryArtist));
        OnPropertyChanged(nameof(QueueTrackCountText));
        OnPropertyChanged(nameof(QueueTotalDurationText));
    }

    private void OnAvailablePlayersCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        OnPropertyChanged(nameof(HasAvailablePlayers));
        OnPropertyChanged(nameof(HasNoAvailablePlayers));
    }

    private void OnPlaylistsCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        BuildQueueContextMenuItems();
        BuildCurrentTrackContextMenuItems();
        OnPropertyChanged(nameof(Playlists));
    }

    private void OnPlaylistServicePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(IPlaylistService.IsLoading))
        {
            IsLoadingPlaylists = _playlistService.IsLoading;
        }
    }

    #endregion

    #region User Menu

    private void BuildUserMenuItems()
    {
        _userMenuItems.Clear();

        _userMenuItems.Add(new ContextMenuItem
        {
            Text = "Einstellungen",
            Icon = FluentIcons.Settings24,
            Command = new Command(async () => await ExecuteOpenSettingsAsync())
        });

        _userMenuItems.Add(new ContextMenuItem { IsSeparator = true });

        _userMenuItems.Add(new ContextMenuItem
        {
            Text = "Abmelden",
            Icon = FluentIcons.SignOut24,
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
                Command = new Command(async () => await PlaySelectedQueueIndexAsync())
            },
            new()
            {
                Text = "Als Nächstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () => await MoveSelectedQueueItemsNextAsync())
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () => await MoveSelectedQueueItemsLastAsync())
            },
            new()
            {
                Text = "Aus Queue entfernen",
                Icon = FluentIcons.Dismiss12,
                Command = new Command(async () => await RemoveSelectedQueueItemsAsync())
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Wiedergabeliste hinzufügen",
                Icon = FluentIcons.Add12,
                SubItems = new ObservableCollection<ContextMenuItem>(
                    _playlists
                        .Select(playlist => new ContextMenuItem
                        {
                            Text = playlist.DisplayName,
                            Icon = FluentIcons.TextBulletListLtr16,
                            Command = new Command(async () =>
                                    await MediaActions.AddToPlaylistAsync(
                                        CurrentQueueItems
                                            .Select(item => item.MediaItem)
                                            .OfType<Track>()
                                            .Where(track => track.IsSelected),
                                        playlist))
                        }))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufügen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                        await MediaActions.AddToFavoritesAsync(
                            CurrentQueueItems
                                .Select(item => item.MediaItem)
                                .OfType<Track>()
                                .Where(track => track.IsSelected)))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                        await MediaActions.RemoveFromFavoritesAsync(
                            CurrentQueueItems
                                .Select(item => item.MediaItem)
                                .OfType<Track>()
                                .Where(track => track.IsSelected)))
            }
        };
    }

    #endregion

    #region Current Track Context Menu

    private void BuildCurrentTrackContextMenuItems()
    {
        _currentTrackContextMenuItems = new ObservableRangeCollection<ContextMenuItem>(
            _playlists
                .Select(playlist => new ContextMenuItem
                {
                    Text = playlist.DisplayName,
                    Icon = FluentIcons.TextBulletListLtr16,
                    Command = new Command(async () =>
                    {
                        var currentTrack = CurrentQueueItem?.MediaItem;
                        if (currentTrack == null)
                        {
                            return;
                        }

                        await MediaActions.AddToPlaylistAsync(currentTrack, playlist);
                    })
                }));
    }

    #endregion

    #region Queue Actions

    private async Task PlaySelectedQueueIndexAsync()
    {
        var selectedTracks = CurrentQueueItems
            .Select(item => item.MediaItem)
            .OfType<Track>()
            .Where(track => track.IsSelected)
            .OrderBy(track => track.Index)
            .ToList();

        if (selectedTracks.Count == 0)
        {
            _logger.LogDebug("No queue items selected for play");
            return;
        }

        var firstSelected = selectedTracks.First();
        var zeroBasedIndex = Math.Max(0, firstSelected.Index - 1);

        await _playbackService.PlayQueueIndexAsync(zeroBasedIndex);
    }

    private async Task MoveSelectedQueueItemsNextAsync()
    {
        var queueItems = CurrentQueueItems.ToList();
        if (queueItems.Count == 0)
        {
            _logger.LogDebug("Queue is empty, nothing to move");
            return;
        }

        var selectedQueueItemIds = CurrentQueueItems
            .Where(item => item.MediaItem?.IsSelected == true)
            .Select(item => item.QueueItemId)
            .Where(id => !string.IsNullOrWhiteSpace(id))
            .ToList();

        if (selectedQueueItemIds.Count == 0)
        {
            _logger.LogDebug("No queue items selected for 'Als Nächstes spielen'");
            return;
        }

        var selectedIdSet = selectedQueueItemIds.ToHashSet(StringComparer.Ordinal);
        selectedQueueItemIds = queueItems
            .Select(item => item.QueueItemId)
            .Where(id => selectedIdSet.Contains(id))
            .ToList();

        // Calculate target position based strictly on the derived current queue item index.
        var currentIndex = _playbackService.CurrentQueueIndex;
        if (currentIndex is null)
        {
            _logger.LogDebug("Current queue index is unknown, cannot move selected items next.");
            return;
        }

        var queueItemOrder = queueItems.Select(item => item.QueueItemId).ToList();
        var insertPosition = Math.Min(currentIndex.Value + 1, queueItemOrder.Count);

        foreach (var queueItemId in selectedQueueItemIds)
        {
            var currentPosition = queueItemOrder.IndexOf(queueItemId);
            if (currentPosition < 0)
            {
                continue;
            }

            if (currentPosition < insertPosition)
            {
                insertPosition--;
            }

            var positionShift = insertPosition - currentPosition;
            if (positionShift != 0)
            {
                await _playbackService.MoveQueueItemAsync(queueItemId, positionShift);
            }

            queueItemOrder.RemoveAt(currentPosition);
            queueItemOrder.Insert(insertPosition, queueItemId);
            insertPosition++;
        }

    }

    private async Task MoveSelectedQueueItemsLastAsync()
    {
        var queueItems = CurrentQueueItems.ToList();
        if (queueItems.Count == 0)
        {
            _logger.LogDebug("Queue is empty, nothing to move");
            return;
        }

        var selectedQueueItemIds = CurrentQueueItems
            .Where(item => item.MediaItem?.IsSelected == true)
            .Select(item => item.QueueItemId)
            .Where(id => !string.IsNullOrWhiteSpace(id))
            .ToList();

        if (selectedQueueItemIds.Count == 0)
        {
            _logger.LogDebug("No queue items selected for 'Als Letztes spielen'");
            return;
        }

        var selectedIdSet = selectedQueueItemIds.ToHashSet(StringComparer.Ordinal);
        selectedQueueItemIds = queueItems
            .Select(item => item.QueueItemId)
            .Where(id => selectedIdSet.Contains(id))
            .ToList();

        // Calculate target position for each selected item, based on current queue order.
        var queueItemOrder = queueItems.Select(item => item.QueueItemId).ToList();

        foreach (var queueItemId in selectedQueueItemIds)
        {
            var currentPosition = queueItemOrder.IndexOf(queueItemId);
            if (currentPosition < 0)
            {
                continue;
            }

            var targetPosition = queueItemOrder.Count - 1;
            var positionShift = targetPosition - currentPosition;
            if (positionShift != 0)
            {
                await _playbackService.MoveQueueItemAsync(queueItemId, positionShift);
            }

            queueItemOrder.RemoveAt(currentPosition);
            queueItemOrder.Add(queueItemId);
        }

    }

    private async Task RemoveSelectedQueueItemsAsync()
    {
        var selectedQueueItemIds = CurrentQueueItems
            .Where(item => item.MediaItem?.IsSelected == true)
            .Select(item => item.QueueItemId)
            .Where(id => !string.IsNullOrWhiteSpace(id))
            .ToList();

        if (selectedQueueItemIds.Count == 0)
        {
            _logger.LogDebug("No queue items selected for removal");
            return;
        }

        foreach (var queueItemId in selectedQueueItemIds)
        {
            await _playbackService.DeleteQueueItemAsync(queueItemId);
        }

    }

    private async Task SetDontStopTheMusicAsync(bool dontStopTheMusicEnabled)
    {
        try
        {
            await _playbackService.SetDontStopTheMusicAsync(dontStopTheMusicEnabled);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to set dont-stop-the-music state to {Enabled}", dontStopTheMusicEnabled);
        }
    }

    #endregion

    #region Player Selection

    private void ApplyInitialSelectedPlayer()
    {
        var preferredSendspinPlayerId = _settings.GetSendspinMusicAssistantPlayerId();

        var preferredPlayerId = _availablePlayers
            .FirstOrDefault(player => string.Equals(player.PlayerId, preferredSendspinPlayerId, StringComparison.Ordinal))?.PlayerId
            ?? _availablePlayers.FirstOrDefault(player => player.Available)?.PlayerId
            ?? _availablePlayers.FirstOrDefault()?.PlayerId;

        if (string.IsNullOrWhiteSpace(preferredPlayerId))
        {
            return;
        }

        SetSelectedPlayerSilently(preferredPlayerId);
    }

    private async Task ToggleDeviceSelectionFlyoutAsync()
    {
        IsDeviceSelectionFlyoutOpen = !IsDeviceSelectionFlyoutOpen;
        if (!IsDeviceSelectionFlyoutOpen)
        {
            return;
        }

        await RefreshAvailablePlayersAsync();
    }

    private async Task RefreshAvailablePlayersAsync()
    {
        if (IsLoadingPlayers)
        {
            return;
        }

        IsLoadingPlayers = true;

        try
        {
            var preferredSendspinPlayerId = _settings.GetSendspinMusicAssistantPlayerId();

            var players = await _musicAssistant.GetPlayersAsync(returnUnavailable: true);
            var orderedPlayers = players
                .Where(player => player.Available)
                .Where(player => !string.IsNullOrWhiteSpace(player.PlayerId))
                .OrderByDescending(player => string.Equals(player.PlayerId, preferredSendspinPlayerId, StringComparison.Ordinal))
                .ThenBy(player => player.Name, StringComparer.CurrentCultureIgnoreCase)
                .ToList();

            _availablePlayers.ReplaceRange(orderedPlayers);

            var activePlayerId = _playbackService.ActivePlayerId?.Trim();
            var selectedPlayerId = SelectedPlayerId?.Trim();

            var preferredPlayerId = orderedPlayers
                .FirstOrDefault(player => string.Equals(player.PlayerId, activePlayerId, StringComparison.OrdinalIgnoreCase))?.PlayerId
                ?? orderedPlayers.FirstOrDefault(player => string.Equals(player.PlayerId, selectedPlayerId, StringComparison.OrdinalIgnoreCase))?.PlayerId
                ?? orderedPlayers.FirstOrDefault(player => string.Equals(player.PlayerId, preferredSendspinPlayerId, StringComparison.OrdinalIgnoreCase))?.PlayerId
                ?? orderedPlayers.FirstOrDefault()?.PlayerId;

            if (!string.IsNullOrWhiteSpace(preferredPlayerId))
            {
                SetSelectedPlayerSilently(preferredPlayerId);
            }
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load available players for device selection");
        }
        finally
        {
            IsLoadingPlayers = false;
        }
    }

    private async Task SelectActivePlayerAsync(string? playerId, string? previousPlayerId)
    {
        if (string.IsNullOrWhiteSpace(playerId))
        {
            return;
        }

        var selectedPlayer = _availablePlayers.FirstOrDefault(player => string.Equals(player.PlayerId, playerId, StringComparison.Ordinal));
        if (selectedPlayer != null && !selectedPlayer.Available)
        {
            _logger.LogDebug("Ignoring player switch to unavailable player: {PlayerId}", playerId);
            if (!string.IsNullOrWhiteSpace(previousPlayerId))
            {
                SetSelectedPlayerSilently(previousPlayerId);
            }

            return;
        }

        try
        {
            var sendspinPlayerId = _settings.GetSendspinMusicAssistantPlayerId();
            var nextOutputMode = string.Equals(playerId.Trim(), sendspinPlayerId, StringComparison.Ordinal)
                ? PlaybackOutputMode.Sendspin
                : PlaybackOutputMode.MA_Remote;
            await _playbackService.SetOutputModeAsync(nextOutputMode, playerId);
            OnPropertyChanged(nameof(IsDontStopTheMusicEnabled));

            var refreshedPlayer = selectedPlayer ?? await _musicAssistant.GetPlayerAsync(playerId, raiseUnavailable: true);
            if (refreshedPlayer != null)
            {
                if (refreshedPlayer.VolumeLevel.HasValue)
                {
                    await _playbackService.SetVolumeAsync(refreshedPlayer.VolumeLevel.Value);
                }

                if (refreshedPlayer.VolumeMuted.HasValue)
                {
                    if (refreshedPlayer.VolumeMuted.Value != _playbackService.IsMuted)
                    {
                        await _playbackService.ToggleMuteAsync(_playbackService.IsMuted);
                    }
                }

                OnPropertyChanged(nameof(Volume));
                OnPropertyChanged(nameof(IsMuted));
            }

            IsDeviceSelectionFlyoutOpen = false;
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to switch active player to {PlayerId}", playerId);

            if (!string.IsNullOrWhiteSpace(previousPlayerId))
            {
                SetSelectedPlayerSilently(previousPlayerId);
                var sendspinPlayerId = _settings.GetSendspinMusicAssistantPlayerId();
                var previousOutputMode = string.Equals(previousPlayerId.Trim(), sendspinPlayerId, StringComparison.Ordinal)
                    ? PlaybackOutputMode.Sendspin
                    : PlaybackOutputMode.MA_Remote;
                await _playbackService.SetOutputModeAsync(previousOutputMode, previousPlayerId);
            }
        }
    }

    private void SetSelectedPlayerSilently(string playerId)
    {
        _suppressSelectedPlayerChange = true;
        try
        {
            _selectedPlayerId = playerId;
            OnPropertyChanged(nameof(SelectedPlayerId));
        }
        finally
        {
            _suppressSelectedPlayerChange = false;
        }
    }

    #endregion

    #region Helper - Navigation

    private Task ExecuteOpenSettingsAsync()
    {
        _logger.LogDebug("Einstellungen ist noch nicht implementiert.");
        return Task.CompletedTask;
    }

    #endregion

    #region Helper - Theme

    private void ToggleTheme()
    {
        var nextTheme = IsDarkTheme ? AppTheme.Light : AppTheme.Dark;

        _settings.ThemeMode = nextTheme;
        _settings.Save();

        if (Application.Current is App app)
        {
            app.SetTheme(nextTheme);
        }

        IsDarkTheme = nextTheme == AppTheme.Dark;
    }

    #endregion

    #region Helper - Authentication

    private async Task ExecuteLogoutAsync()
    {
        try
        {
            await _musicAssistant.LogoutAsync();
            _musicAssistant.RequestLogin();
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Logout failed");
        }
    }

    private async Task<(bool Success, string? ErrorMessage)> ExecuteLogin(string username, string password)
    {
        try
        {
            _logger.LogDebug("Attempting login for user: {Username}", username);
            var success = await _musicAssistant.LoginAsync(username, password);

            if (success)
            {
                _logger.LogDebug("Login successful for user: {Username}", username);
                return (true, null);
            }

            _logger.LogWarning("Login failed for user: {Username}", username);
            return (false, "Anmeldung fehlgeschlagen. Bitte ueberpruefen Sie Ihre Anmeldedaten.");
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Login error for user: {Username}", username);
            return (false, $"Verbindungsfehler: {ex.Message}");
        }
    }

    #endregion

    #region Helper - Property Change

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

    #region Helper - Formatting

    private static string FormatTime(double seconds)
    {
        if (seconds <= 0 || double.IsNaN(seconds) || double.IsInfinity(seconds))
        {
            return "0:00";
        }

        var ts = TimeSpan.FromSeconds(seconds);
        return ts.Hours > 0 ? ts.ToString(@"h\:mm\:ss") : ts.ToString(@"m\:ss");
    }

    private static string FormatQueueDuration(int totalSeconds)
    {
        if (totalSeconds <= 0)
        {
            return "0 Minuten";
        }

        var ts = TimeSpan.FromSeconds(totalSeconds);
        var totalHours = (int)ts.TotalHours;

        if (totalHours > 0)
        {
            return $"{totalHours} Std. {ts.Minutes:00} Minuten";
        }

        return $"{ts.Minutes} Minuten";
    }

    #endregion

    #region Helper - Repeat Mode

    private static mashin.Models.RepeatMode GetNextRepeatMode(string? repeatMode)
    {
        return GetNormalizedRepeatMode(repeatMode) switch
        {
            mashin.Models.RepeatMode.All => mashin.Models.RepeatMode.One,
            mashin.Models.RepeatMode.One => mashin.Models.RepeatMode.Off,
            _ => mashin.Models.RepeatMode.All,
        };
    }

    private static mashin.Models.RepeatMode GetNormalizedRepeatMode(string? repeatMode)
    {
        if (Enum.TryParse<mashin.Models.RepeatMode>(repeatMode, true, out var parsedMode))
        {
            return parsedMode;
        }

        return mashin.Models.RepeatMode.Off;
    }

    #endregion

}
