using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
using MauiIcons.Fluent;
using Microsoft.Extensions.Logging;
using Microsoft.Maui.Controls;
using Microsoft.Maui.Dispatching;
using Microsoft.Maui.Graphics;
using Sendspin.SDK.Connection;
using Sendspin.SDK.Models;
using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Windows.Input;
using mashin.Collections;
using MauiIcons.Fluent.Filled;
using PlayerRepeatMode = mashin.Models.RepeatMode;

namespace mashin.ViewModels;

public sealed class MainViewModel : INotifyPropertyChanged, IAsyncDisposable
{
    #region Fields

    private const string PlayerBarAccentFallbackColorKey = "PlayerBarAccentFallbackColor";

    // Services
    private readonly SettingsService _settings;
    private readonly MusicAssistantService _musicAssistant;
    private readonly IUserDataService _userDataService;
    private readonly IPlayerService _playerService;
    private readonly INavigationService _navigationService;
    private readonly IOverlayService _overlayService;
    private readonly IContextMenuService _contextMenuService;
    private readonly IQueueSyncService _queueSyncService;
    private readonly ILogger<MainViewModel> _logger;
    private readonly ObservableRangeCollection<Playlist> _playlists = new();


    // Player
    private bool _isMuted;
    private bool? _shuffleEnabled;
    private string? _repeatMode;
    private PlayerPlayState _playState = new(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow);
    private double _duration;
    private double _position;
    private double _sliderPosition;
    private double _volume = 50;
    private bool _suppressVolumeCommand;
    private bool _isApplyingQueueSettings;
    private bool _isAudioOptionsFlyoutOpen;
    private bool _isDeviceSelectionFlyoutOpen;
    private bool _isLoadingPlayers;
    private string? _selectedPlayerId;
    private bool _suppressSelectedPlayerChange;
    private string _selectedAudioQuality = "opus";
    private readonly ObservableRangeCollection<Player> _availablePlayers = new();

    private bool _isDontStopTheMusicEnabled;
    private bool _isDarkTheme;
    private bool _isLoadingPlaylists;
    private Color _playerBarAccentColor = null!;
    private Color _playerBarBackgroundColor = null!;

    // Search
    private string _searchQuery = string.Empty;
    private bool _isSearching;

    // Queue
    private PlayerQueue? _currentPlayerQueue;
    private Track? _currentTrack;
    private readonly ObservableRangeCollection<QueueItem> _currentQueueItems = new();

    // Context Menus
    private ObservableCollection<ContextMenuItem> _userMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _queueContextMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _currentTrackContextMenuItems = new();

    // Navigation
    private bool _isNavigating;
    private NavigationSection _currentSection = NavigationSection.Home;
    private bool _isLoginOverlayActive;

    public event EventHandler<Track?>? CurrentTrackChanged;
    public event Func<Task>? CloseQueueViewRequested;

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
        IUserDataService userDataService,
        IPlayerService playerService,
        INavigationService navigationService,
        IOverlayService overlayService,
        IMediaItemActions mediaActions,
        IContextMenuService contextMenuService,
        IQueueSyncService queueSyncService,
        ILogger<MainViewModel> logger)
    {
        _settings = settings;
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _playerService = playerService;
        _navigationService = navigationService;
        _overlayService = overlayService;
        _contextMenuService = contextMenuService;
        _queueSyncService = queueSyncService;
        _logger = logger;
        MediaActions = mediaActions;
        _selectedAudioQuality = _settings.GetPreferredAudioCodec();
        _volume = _settings.GetInitialVolume();
        _isMuted = _settings.GetInitialMuted();
        _currentQueueItems.CollectionChanged += OnCurrentQueueItemsCollectionChanged;
        _availablePlayers.CollectionChanged += OnAvailablePlayersCollectionChanged;

        var fallbackAccentColor = GetRequiredColorResource(PlayerBarAccentFallbackColorKey);
        _playerBarAccentColor = fallbackAccentColor;
        _playerBarBackgroundColor = fallbackAccentColor.WithAlpha(0.5f);

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
            if (anchor == null || CurrentTrack == null)
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
            if (CurrentTrack == null)
            {
                return;
            }

            if (CurrentTrack.Favorite)
            {
                await MediaActions.RemoveFromFavoritesAsync(CurrentTrack);
            }
            else
            {
                await MediaActions.AddToFavoritesAsync(CurrentTrack);
            }
        });

        // Theme Command
        ToggleThemeCommand = new Command(ToggleTheme);

        _navigationService.PropertyChanged += OnNavigationServicePropertyChanged;
        IsNavigating = _navigationService.IsNavigating;

        _musicAssistant.LoginRequired += OnLoginRequired;

        // Subscribe to player state events
        _playerService.PropertyChanged += OnPlayerServicePropertyChanged;
        PlayState = _playerService.PlayState;

        // Subscribe to queue sync updates
        _queueSyncService.CurrentPlayerQueueUpdated += OnCurrentPlayerQueueUpdated;
        _queueSyncService.CurrentTrackUpdated += OnCurrentTrackUpdated;
        _queueSyncService.CurrentQueueItemsUpdated += OnCurrentQueueItemsUpdated;

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

    public PlayerPlayState PlayState
    {
        get => _playState;
        private set
        {
            SetProperty(ref _playState, value);
        }
    }

    public bool IsMuted
    {
        get => _isMuted;
        private set => SetProperty(ref _isMuted, value);
    }

    public bool IsDarkTheme
    {
        get => _isDarkTheme;
        private set => SetProperty(ref _isDarkTheme, value);
    }

    public bool? ShuffleEnabled
    {
        get => _shuffleEnabled;
        private set
        {
            if (SetProperty(ref _shuffleEnabled, value))
            {
                OnPropertyChanged(nameof(IsShuffleActive));
            }
        }
    }

    public string? RepeatMode
    {
        get => _repeatMode;
        private set
        {
            if (SetProperty(ref _repeatMode, value))
            {
                OnPropertyChanged(nameof(IsRepeatEnabled));
                OnPropertyChanged(nameof(IsRepeatOne));
            }
        }
    }

    public bool IsShuffleActive => ShuffleEnabled == true;

    public bool IsRepeatEnabled => GetNormalizedRepeatMode(RepeatMode) is not PlayerRepeatMode.Off;

    public bool IsRepeatOne => GetNormalizedRepeatMode(RepeatMode) == PlayerRepeatMode.One;

    public double Duration
    {
        get => _duration;
        private set
        {
            if (SetProperty(ref _duration, value))
            {
                OnPropertyChanged(nameof(DurationText));
                OnPropertyChanged(nameof(PlayerBarProgress));
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
                OnPropertyChanged(nameof(PlayerBarProgress));
            }
        }
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

            var clamped = (int)Math.Round(Math.Max(0, Math.Min(100, value)));
            _ = SetVolumeAsync(clamped);
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
                OnPropertyChanged(nameof(CurrentTrackPrimaryArtist));
                _ = UpdatePlayerBarAccentColorAsync(value);

                if (oldTrack?.Uri != value?.Uri)
                {
                    CurrentTrackChanged?.Invoke(this, value);
                }
            }
        }
    }

    public Color PlayerBarAccentColor
    {
        get => _playerBarAccentColor;
        private set => SetProperty(ref _playerBarAccentColor, value);
    }

    public Color PlayerBarBackgroundColor
    {
        get => _playerBarBackgroundColor;
        private set => SetProperty(ref _playerBarBackgroundColor, value);
    }

    public double PlayerBarProgress => Duration <= 0 ? 0 : Math.Clamp(Position / Duration, 0, 1);

    public Artist? CurrentTrackPrimaryArtist => CurrentTrack?.Artists?.FirstOrDefault();

    public PlayerQueue? CurrentPlayerQueue
    {
        get => _currentPlayerQueue;
        private set => SetProperty(ref _currentPlayerQueue, value);
    }

    public ObservableRangeCollection<QueueItem> CurrentQueueItems => _currentQueueItems;

    public string QueueTrackCountText => $"{_currentQueueItems.Count} Titel";

    public string QueueTotalDurationText
    {
        get
        {
            var totalSeconds = _currentQueueItems
                .Select(item => item.MediaItem)
                .OfType<Track>()
                .Sum(track => Math.Max(0, track.Duration));
            return FormatQueueDuration(totalSeconds);
        }
    }

    public bool IsDontStopTheMusicEnabled
    {
        get => _isDontStopTheMusicEnabled;
        set
        {
            if (!SetProperty(ref _isDontStopTheMusicEnabled, value))
            {
                return;
            }

            if (_isApplyingQueueSettings)
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
        await EnsureAuthenticatedAtStartupAsync();

        if (!_musicAssistant.IsAuthenticated)
        {
            _logger.LogWarning("Startup initialization paused because user is not authenticated.");
            return;
        }

        // Establish connection to Sendspin server (if configured).
        if (!string.IsNullOrWhiteSpace(_settings.SendspinUrl))
        {
            var uri = new Uri(_settings.SendspinUrl);
            await _playerService.ConnectAsync(uri);
        }

        await RefreshAvailablePlayersAsync();
        ApplyInitialSelectedPlayer();

        // Load playlists once and keep local list refreshed as needed.
        await RefreshPlaylistsAsync();

        // Build Context Menus
        BuildUserMenuItems();
        BuildQueueContextMenuItems();
        BuildCurrentTrackContextMenuItems();  

        // Set initial queue state
        await _queueSyncService.RefreshNowAsync();
        CurrentPlayerQueue = _queueSyncService.CurrentPlayerQueue;
        ApplyQueueSettingsFromCurrentQueue(_queueSyncService.CurrentPlayerQueue);

        // Set position slider
        if (_queueSyncService.CurrentPlayerQueue?.ElapsedTime is double elapsedTime
            && _queueSyncService.CurrentPlayerQueue?.CurrentItem?.MediaItem?.Duration is int duration)
        {
            Duration = duration;
            _playerService.PositionSeconds = elapsedTime;
            Position = elapsedTime;
            SliderPosition = elapsedTime;
        }

        // Start queue sync loop
        await _queueSyncService.StartAsync();

        
    }

    public async ValueTask DisposeAsync()
    {
        _currentQueueItems.CollectionChanged -= OnCurrentQueueItemsCollectionChanged;
        _availablePlayers.CollectionChanged -= OnAvailablePlayersCollectionChanged;

        _musicAssistant.LoginRequired -= OnLoginRequired;

        _playerService.PropertyChanged -= OnPlayerServicePropertyChanged;
        _queueSyncService.CurrentPlayerQueueUpdated -= OnCurrentPlayerQueueUpdated;
        _queueSyncService.CurrentTrackUpdated -= OnCurrentTrackUpdated;
        _queueSyncService.CurrentQueueItemsUpdated -= OnCurrentQueueItemsUpdated;

        _navigationService.PropertyChanged -= OnNavigationServicePropertyChanged;

        await Task.CompletedTask;
    }

    #endregion

    #region Artwork & Colors

    private async Task UpdatePlayerBarAccentColorAsync(Track? track)
    {
        var fallbackColor = GetRequiredColorResource(PlayerBarAccentFallbackColorKey);
        PlayerBarAccentColor = fallbackColor;
        PlayerBarBackgroundColor = fallbackColor.WithAlpha(0.5f);
        await Task.CompletedTask;
    }

    private static Color GetRequiredColorResource(string key)
    {
        var resources = Application.Current?.Resources;
        if (resources != null && resources.TryGetValue(key, out var value) && value is Color color)
        {
            return color;
        }

        throw new InvalidOperationException($"Required color resource '{key}' is not defined.");
    }

    #endregion

    #region Login Overlay

    private async void OnLoginRequired(object? sender, EventArgs e)
    {
        if (_isLoginOverlayActive)
        {
            return;
        }

        _isLoginOverlayActive = true;
        try
        {
            await _overlayService.ShowLoginAsync(
                _settings.Username,
                AuthenticateWithCredentialsAsync);
        }
        finally
        {
            _isLoginOverlayActive = false;
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

        await _userDataService.GetPreferencesAsync();
        var prefix = GetUserPlaylistPrefix();
        if (!string.IsNullOrWhiteSpace(prefix)
            && !name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
        {
            name = string.Concat(prefix, name);
        }

        try
        {
            await _musicAssistant.CreatePlaylistAsync(name);
            await RefreshPlaylistsAsync();
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

        var activePlayerId = _queueSyncService.TargetPlayerId;
        if (string.IsNullOrWhiteSpace(activePlayerId))
        {
            _logger.LogWarning("No active player available for playlist playback.");
            return;
        }

        try
        {
            _logger.LogInformation("Play playlist: {Name}", playlist.Name);

            await _musicAssistant.PlayMediaAsync(
                activePlayerId,
                new List<MediaItem> { playlist },
                QueueOption.Play);

            _playerService.PlayState = new PlayerPlayState(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to play playlist: {Name}", playlist.Name);
        }
    }

    #endregion

    #region Player Actions

    private async Task TogglePlayPauseAsync()
    {
        try
        {
            var queueId = CurrentPlayerQueue?.QueueId;
            if (!string.IsNullOrWhiteSpace(queueId))
            {
                await _musicAssistant.PlayPauseAsync(queueId);
                PlayState = new PlayerPlayState(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);
                return;
            }

            _logger.LogWarning("No active queue available for play/pause");
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
            var queueId = CurrentPlayerQueue?.QueueId;
            if (!string.IsNullOrWhiteSpace(queueId))
            {
                await _musicAssistant.NextAsync(queueId);
                PlayState = new PlayerPlayState(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);
                return;
            }

            _logger.LogWarning("No active queue available for next track");
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
            var queueId = CurrentPlayerQueue?.QueueId;
            if (!string.IsNullOrWhiteSpace(queueId))
            {
                await _musicAssistant.PreviousAsync(queueId);
                PlayState = new PlayerPlayState(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);
                return;
            }

            _logger.LogWarning("No active queue available for previous track");
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to go previous");
        }
    }

    private async Task ToggleMuteAsync()
    {
        var nextMuted = !IsMuted;

        try
        {
            var activePlayerId = _queueSyncService.TargetPlayerId;
            if (string.IsNullOrWhiteSpace(activePlayerId))
            {
                _logger.LogWarning("No active player available for mute toggle");
                return;
            }

            await _musicAssistant.SetPlayerMuteAsync(activePlayerId, nextMuted);
            IsMuted = nextMuted;

            _settings.SetInitialMuted(nextMuted);
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
            var queueId = CurrentPlayerQueue?.QueueId;
            if (string.IsNullOrWhiteSpace(queueId))
            {
                _logger.LogDebug("No active queue available for shuffle toggle");
                return;
            }

            var nextShuffleEnabled = !(ShuffleEnabled ?? false);
            await _musicAssistant.SetShuffleAsync(queueId, nextShuffleEnabled);
            ShuffleEnabled = nextShuffleEnabled;

            // Shuffle can reorder items server-side, so force an immediate queue resync.
            await _queueSyncService.RefreshNowAsync();
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
            var queueId = CurrentPlayerQueue?.QueueId;
            if (string.IsNullOrWhiteSpace(queueId))
            {
                _logger.LogDebug("No active queue available for repeat toggle");
                return;
            }

            var nextRepeatMode = GetNextRepeatMode(RepeatMode);
            await _musicAssistant.SetRepeatAsync(queueId, nextRepeatMode);
            RepeatMode = nextRepeatMode.ToString();
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to toggle repeat mode");
        }
    }

    private async Task SeekAsync(double seconds)
    {
        if (PlayState.State != PlayerPlaybackState.Seeking)
        {
            _playerService.PlayState = new PlayerPlayState(PlayerPlaybackState.Seeking, DateTimeOffset.UtcNow);
        }
        
        try
        {
            var clamped = Math.Max(0, Math.Min(Duration, seconds));
            Position = clamped;
            SliderPosition = clamped;
            _playerService.PositionSeconds = clamped;
            var queueId = CurrentPlayerQueue?.QueueId;
            if (string.IsNullOrWhiteSpace(queueId))
            {
                _logger.LogWarning("No active queue available for seek");
                return;
            }

            await _musicAssistant.SeekAsync(queueId, (int)Math.Round(clamped));
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to seek");
        }
        finally
        {
            _playerService.PlayState = new PlayerPlayState(PlayerPlaybackState.Playing, DateTimeOffset.UtcNow);
        }
    }

    private void BeginSeek()
    {
        _playerService.PlayState = new PlayerPlayState(PlayerPlaybackState.Seeking, DateTimeOffset.UtcNow);
    }

    private async Task SetVolumeAsync(int volume)
    {
        try
        {
            var clamped = Math.Max(0, Math.Min(100, volume));
            var activePlayerId = _queueSyncService.TargetPlayerId;
            if (string.IsNullOrWhiteSpace(activePlayerId))
            {
                _logger.LogWarning("No active player available for volume update");
                return;
            }

            await _musicAssistant.SetPlayerVolumeAsync(activePlayerId, clamped);

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
            await _playerService.UpdatePreferredAudioCodecAsync(codec);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to apply preferred audio codec {Codec}", codec);
        }
    }

    #endregion

    #region Event Handlers

    private void OnPlayerServicePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        switch (e.PropertyName)
        {
            case nameof(IPlayerService.PlayState):
                PlayState = _playerService.PlayState;
                break;

            case nameof(IPlayerService.Volume):
                _suppressVolumeCommand = true;
                try
                {
                    Volume = _playerService.Volume;
                    _settings.SetInitialVolume(_playerService.Volume);
                }
                finally
                {
                    _suppressVolumeCommand = false;
                }
                break;

            case nameof(IPlayerService.IsMuted):
                IsMuted = _playerService.IsMuted;
                _settings.SetInitialMuted(_playerService.IsMuted);
                break;

            case nameof(IPlayerService.RepeatMode):
                RepeatMode = _playerService.RepeatMode;
                break;

            case nameof(IPlayerService.DurationSeconds):
                Duration = _playerService.DurationSeconds;
                if (SliderPosition > Duration)
                {
                    SliderPosition = Duration;
                }
                break;

            case nameof(IPlayerService.PositionSeconds):
                if (PlayState.State != PlayerPlaybackState.Seeking)
                {
                    var position = _playerService.PositionSeconds;
                    Position = position;
                    SliderPosition = position;
                }
                break;

            case nameof(IPlayerService.TrackTitle):
            case nameof(IPlayerService.TrackArtist):
            case nameof(IPlayerService.TrackAlbum):
                if (CurrentTrack == null
                    || !string.Equals(CurrentTrack.Name, _playerService.TrackTitle, StringComparison.Ordinal)
                    || !string.Equals(CurrentTrack.ArtistName, _playerService.TrackArtist, StringComparison.Ordinal)
                    || !string.Equals(CurrentTrack.AlbumName, _playerService.TrackAlbum, StringComparison.Ordinal))
                {
                    _ = _queueSyncService.RefreshNowAsync();
                }
                break;
        }
    }

    private void OnNavigationServicePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(INavigationService.IsNavigating))
        {
            IsNavigating = _navigationService.IsNavigating;
        }
    }

    private void OnCurrentQueueItemsCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        OnPropertyChanged(nameof(QueueTrackCountText));
        OnPropertyChanged(nameof(QueueTotalDurationText));
    }

    private void OnCurrentPlayerQueueUpdated(object? sender, EventArgs e)
    {
        CurrentPlayerQueue = _queueSyncService.CurrentPlayerQueue;
        ApplyQueueSettingsFromCurrentQueue(_queueSyncService.CurrentPlayerQueue);

    }

    private void OnAvailablePlayersCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        OnPropertyChanged(nameof(HasAvailablePlayers));
        OnPropertyChanged(nameof(HasNoAvailablePlayers));
    }

    private void OnCurrentTrackUpdated(object? sender, EventArgs e)
    {
        CurrentTrack = _queueSyncService.CurrentTrack;
    }

    private void OnCurrentQueueItemsUpdated(object? sender, QueueItemsChangedEventArgs e)
    {
      
        if (e.ChangeSet.Changes.Count == 0)
        {
            return;
        }

        // Update the _currentQueueItems collection based on the changes from the service, to avoid a full refresh and keep UI selection in place when possible.
        foreach (var change in e.ChangeSet.Changes)
        {
            switch (change.ChangeType)
            {
                case QueueItemChangeType.Remove:
                    if (change.Index >= 0 && change.Index < _currentQueueItems.Count)
                    {
                        _currentQueueItems.RemoveAt(change.Index);
                    }

                    break;

                case QueueItemChangeType.Insert:
                    if (change.Item != null)
                    {
                        var insertIndex = Math.Clamp(change.Index, 0, _currentQueueItems.Count);
                        _currentQueueItems.Insert(insertIndex, change.Item);
                    }

                    break;

                case QueueItemChangeType.Move:
                    if (change.Index >= 0
                        && change.Index < _currentQueueItems.Count
                        && change.NewIndex >= 0
                        && change.NewIndex < _currentQueueItems.Count
                        && change.Index != change.NewIndex)
                    {
                        var movedItem = _currentQueueItems[change.Index];
                        _currentQueueItems.RemoveAt(change.Index);

                        var insertIndex = Math.Clamp(change.NewIndex, 0, _currentQueueItems.Count);
                        _currentQueueItems.Insert(insertIndex, movedItem);
                    }

                    break;

                case QueueItemChangeType.Replace:
                    if (change.Item != null && change.Index >= 0 && change.Index < _currentQueueItems.Count)
                    {
                        var existingItem = _currentQueueItems[change.Index];
                        if (existingItem.MediaItem != null && change.Item.MediaItem != null)
                        {
                            change.Item.MediaItem.IsSelected = existingItem.MediaItem.IsSelected;
                        }

                        _currentQueueItems[change.Index] = change.Item;
                    }

                    break;
            }
        }

        // Update indices
        for (var index = 0; index < _currentQueueItems.Count; index++)
        {
            var track = _currentQueueItems[index].MediaItem;
            if (track != null)
            {
                track.Index = index + 1;
            }
        }

        // Check if now equal and if not do full refresh
        var serviceQueue = _queueSyncService.CurrentQueueItems;
        var isEqual = _currentQueueItems.Count == serviceQueue.Count;

        if (isEqual)
        {
            for (var index = 0; index < _currentQueueItems.Count; index++)
            {
                var leftItem = _currentQueueItems[index];
                var rightItem = serviceQueue[index];

                if (!string.Equals(leftItem.QueueItemId, rightItem.QueueItemId, StringComparison.Ordinal))
                {
                    isEqual = false;
                    break;
                }
            }
        }

        if (!isEqual)
        {
            _currentQueueItems.ReplaceRange(serviceQueue);
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
                        .Where(playlist => !playlist.Name.StartsWith("~", StringComparison.Ordinal))
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
                .Where(playlist => !playlist.Name.StartsWith("~", StringComparison.Ordinal))
                .Select(playlist => new ContextMenuItem
                {
                    Text = playlist.DisplayName,
                    Icon = FluentIcons.TextBulletListLtr16,
                    Command = new Command(async () =>
                    {
                        if (CurrentTrack == null)
                        {
                            return;
                        }

                        await MediaActions.AddToPlaylistAsync(CurrentTrack, playlist);
                    })
                }));
    }

    private async Task RefreshPlaylistsAsync()
    {
        IsLoadingPlaylists = true;

        try
        {
            await _userDataService.GetPreferencesAsync();
            var prefix = GetUserPlaylistPrefix();
            var playlists = await _musicAssistant.GetLibraryPlaylistsAsync(
                search: string.IsNullOrWhiteSpace(prefix) ? null : prefix,
                orderBy: "sort_name");

            ApplyPlaylistDisplayNames(playlists, prefix);
            _playlists.ReplaceRange(playlists);

            BuildQueueContextMenuItems();
            BuildCurrentTrackContextMenuItems();
            OnPropertyChanged(nameof(Playlists));
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to refresh playlists");
        }
        finally
        {
            IsLoadingPlaylists = false;
        }
    }

    private string? GetUserPlaylistPrefix()
    {
        var username = _userDataService.CurrentUser?.Username;
        if (string.IsNullOrWhiteSpace(username))
        {
            return null;
        }

        return string.Concat(username, "--");
    }

    private static void ApplyPlaylistDisplayNames(IEnumerable<Playlist> playlists, string? prefix)
    {
        foreach (var playlist in playlists)
        {
            playlist.DisplayName = playlist.Name;

            if (!string.IsNullOrWhiteSpace(prefix)
                && !string.IsNullOrWhiteSpace(playlist.Name)
                && playlist.Name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
            {
                playlist.DisplayName = playlist.Name[prefix.Length..];
            }
        }
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
            _logger.LogInformation("No queue items selected for play");
            return;
        }

        var firstSelected = selectedTracks.First();
        var zeroBasedIndex = Math.Max(0, firstSelected.Index - 1);

        var activeQueue = CurrentPlayerQueue;
        if (activeQueue == null || string.IsNullOrWhiteSpace(activeQueue.QueueId))
        {
            _logger.LogWarning("No active queue available for play index");
            return;
        }

        await MediaActions.PlayIndexAsync(activeQueue.QueueId, zeroBasedIndex);
    }

    private async Task MoveSelectedQueueItemsNextAsync()
    {
        var activeQueue = CurrentPlayerQueue;
        if (activeQueue == null || string.IsNullOrWhiteSpace(activeQueue.QueueId))
        {
            _logger.LogWarning("No active queue available for move next");
            return;
        }

        var queueItems = CurrentQueueItems.ToList();
        if (queueItems.Count == 0)
        {
            _logger.LogInformation("Queue is empty, nothing to move");
            return;
        }

        var selectedQueueItemIds = CurrentQueueItems
            .Where(item => item.MediaItem?.IsSelected == true)
            .Select(item => item.QueueItemId)
            .Where(id => !string.IsNullOrWhiteSpace(id))
            .ToList();

        if (selectedQueueItemIds.Count == 0)
        {
            _logger.LogInformation("No queue items selected for 'Als Nächstes spielen'");
            return;
        }

        var selectedIdSet = selectedQueueItemIds.ToHashSet(StringComparer.Ordinal);
        selectedQueueItemIds = queueItems
            .Select(item => item.QueueItemId)
            .Where(id => selectedIdSet.Contains(id))
            .ToList();

        // Calculate target position for each selected item, based on current queue order and current playing index.
        var currentIndex = Math.Max(0, activeQueue.CurrentIndex ?? 0);
        var queueItemOrder = queueItems.Select(item => item.QueueItemId).ToList();
        var insertPosition = Math.Min(currentIndex + 1, queueItemOrder.Count);

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
                await MediaActions.MoveQueueItemAsync(activeQueue.QueueId, queueItemId, positionShift);
            }

            queueItemOrder.RemoveAt(currentPosition);
            queueItemOrder.Insert(insertPosition, queueItemId);
            insertPosition++;
        }

    }

    private async Task MoveSelectedQueueItemsLastAsync()
    {
        var activeQueue = CurrentPlayerQueue;
        if (activeQueue == null || string.IsNullOrWhiteSpace(activeQueue.QueueId))
        {
            _logger.LogWarning("No active queue available for move last");
            return;
        }

        var queueItems = CurrentQueueItems.ToList();
        if (queueItems.Count == 0)
        {
            _logger.LogInformation("Queue is empty, nothing to move");
            return;
        }

        var selectedQueueItemIds = CurrentQueueItems
            .Where(item => item.MediaItem?.IsSelected == true)
            .Select(item => item.QueueItemId)
            .Where(id => !string.IsNullOrWhiteSpace(id))
            .ToList();

        if (selectedQueueItemIds.Count == 0)
        {
            _logger.LogInformation("No queue items selected for 'Als Letztes spielen'");
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
                await MediaActions.MoveQueueItemAsync(activeQueue.QueueId, queueItemId, positionShift);
            }

            queueItemOrder.RemoveAt(currentPosition);
            queueItemOrder.Add(queueItemId);
        }

    }

    private async Task RemoveSelectedQueueItemsAsync()
    {
        var activeQueue = CurrentPlayerQueue;
        if (activeQueue == null || string.IsNullOrWhiteSpace(activeQueue.QueueId))
        {
            _logger.LogWarning("No active queue available for remove");
            return;
        }

        var selectedQueueItemIds = CurrentQueueItems
            .Where(item => item.MediaItem?.IsSelected == true)
            .Select(item => item.QueueItemId)
            .Where(id => !string.IsNullOrWhiteSpace(id))
            .ToList();

        if (selectedQueueItemIds.Count == 0)
        {
            _logger.LogInformation("No queue items selected for removal");
            return;
        }

        foreach (var queueItemId in selectedQueueItemIds)
        {
            await MediaActions.DeleteQueueItemAsync(activeQueue.QueueId, queueItemId);
        }

    }

    private async Task SetDontStopTheMusicAsync(bool dontStopTheMusicEnabled)
    {
        try
        {
            var queueId = CurrentPlayerQueue?.QueueId;
            if (string.IsNullOrWhiteSpace(queueId))
            {
                _logger.LogDebug("No active queue available for dont-stop-the-music toggle");
                return;
            }

            await MediaActions.SetDontStopTheMusicAsync(queueId, dontStopTheMusicEnabled);
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
        var preferredPlayerId = _availablePlayers
            .FirstOrDefault(player => string.Equals(player.PlayerId, _playerService.PlayerId, StringComparison.Ordinal))?.PlayerId
            ?? _availablePlayers.FirstOrDefault(player => player.Available)?.PlayerId
            ?? _availablePlayers.FirstOrDefault()?.PlayerId;

        if (string.IsNullOrWhiteSpace(preferredPlayerId))
        {
            return;
        }

        SetSelectedPlayerSilently(preferredPlayerId);
        _queueSyncService.SetTargetPlayerId(preferredPlayerId);
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
            var players = await _musicAssistant.GetPlayersAsync(returnUnavailable: true);
            var orderedPlayers = players
                .Where(player => !string.IsNullOrWhiteSpace(player.PlayerId))
                .OrderByDescending(player => string.Equals(player.PlayerId, _playerService.PlayerId, StringComparison.Ordinal))
                .ThenBy(player => player.Name, StringComparer.CurrentCultureIgnoreCase)
                .ToList();

            _availablePlayers.ReplaceRange(orderedPlayers);

            if (string.IsNullOrWhiteSpace(SelectedPlayerId)
                || orderedPlayers.All(player => !string.Equals(player.PlayerId, SelectedPlayerId, StringComparison.Ordinal)))
            {
                ApplyInitialSelectedPlayer();
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
            _logger.LogInformation("Ignoring player switch to unavailable player: {PlayerId}", playerId);
            if (!string.IsNullOrWhiteSpace(previousPlayerId))
            {
                SetSelectedPlayerSilently(previousPlayerId);
            }

            return;
        }

        try
        {
            _queueSyncService.SetTargetPlayerId(playerId);
            await _queueSyncService.RefreshNowAsync();
            CurrentPlayerQueue = _queueSyncService.CurrentPlayerQueue;
            ApplyQueueSettingsFromCurrentQueue(_queueSyncService.CurrentPlayerQueue);

            var refreshedPlayer = selectedPlayer ?? await _musicAssistant.GetPlayerAsync(playerId, raiseUnavailable: true);
            if (refreshedPlayer != null)
            {
                if (refreshedPlayer.VolumeLevel.HasValue)
                {
                    _suppressVolumeCommand = true;
                    try
                    {
                        Volume = refreshedPlayer.VolumeLevel.Value;
                    }
                    finally
                    {
                        _suppressVolumeCommand = false;
                    }
                }

                if (refreshedPlayer.VolumeMuted.HasValue)
                {
                    IsMuted = refreshedPlayer.VolumeMuted.Value;
                }
            }

            IsDeviceSelectionFlyoutOpen = false;
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to switch active player to {PlayerId}", playerId);

            if (!string.IsNullOrWhiteSpace(previousPlayerId))
            {
                SetSelectedPlayerSilently(previousPlayerId);
                _queueSyncService.SetTargetPlayerId(previousPlayerId);
            }
        }
    }

    private void ApplyQueueSettingsFromCurrentQueue(PlayerQueue? queue)
    {
        _isApplyingQueueSettings = true;
        try
        {
            ShuffleEnabled = queue?.ShuffleEnabled;
            RepeatMode = queue?.RepeatMode?.ToString();
            IsDontStopTheMusicEnabled = queue?.DontStopTheMusicEnabled == true;
            SyncPlayStateFromQueue(queue);
        }
        finally
        {
            _isApplyingQueueSettings = false;
        }
    }

    private void SetSelectedPlayerSilently(string playerId)
    {
        if (string.Equals(_selectedPlayerId, playerId, StringComparison.Ordinal))
        {
            return;
        }

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

    private void SyncPlayStateFromQueue(PlayerQueue? queue)
    {
        if (queue?.State == null)
        {
            return;
        }

        var mappedState = queue.State.Value switch
        {
            mashin.Models.PlaybackState.Playing => PlayerPlaybackState.Playing,
            mashin.Models.PlaybackState.Paused => PlayerPlaybackState.Paused,
            mashin.Models.PlaybackState.Buffering => PlayerPlaybackState.Buffering,
            mashin.Models.PlaybackState.Idle => PlayerPlaybackState.Stopped,
            _ => PlayerPlaybackState.Unknown
        };

        PlayState = new PlayerPlayState(mappedState, DateTimeOffset.UtcNow);
    }
    #endregion

    #region Helpers
    private Task ExecuteOpenSettingsAsync()
    {
        _logger.LogInformation("Einstellungen ist noch nicht implementiert.");
        return Task.CompletedTask;
    }

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

    private async Task EnsureAuthenticatedAtStartupAsync()
    {
        _isLoginOverlayActive = true;

        try
        {
            await _overlayService.ShowLoginAsync(
                _settings.Username,
                AuthenticateWithCredentialsAsync,
                async () =>
                {
                    var success = await _musicAssistant.TryAutoLoginAsync(raiseLoginRequiredEvent: false);
                    return success
                        ? (true, null)
                        : (false, (string?)null);
                },
                "Sie werden angemeldet...");
        }
        finally
        {
            _isLoginOverlayActive = false;
        }
    }

    private async Task<(bool Success, string? ErrorMessage)> AuthenticateWithCredentialsAsync(string username, string password)
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
            return (false, "Anmeldung fehlgeschlagen. Bitte ueberpruefen Sie Ihre Anmeldedaten.");
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Login error for user: {Username}", username);
            return (false, $"Verbindungsfehler: {ex.Message}");
        }
    }

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

    private static PlayerRepeatMode GetNextRepeatMode(string? repeatMode)
    {
        return GetNormalizedRepeatMode(repeatMode) switch
        {
            PlayerRepeatMode.All => PlayerRepeatMode.One,
            PlayerRepeatMode.One => PlayerRepeatMode.Off,
            _ => PlayerRepeatMode.All,
        };
    }

    private static PlayerRepeatMode GetNormalizedRepeatMode(string? repeatMode)
    {
        if (Enum.TryParse<PlayerRepeatMode>(repeatMode, true, out var parsedMode))
        {
            return parsedMode;
        }

        return PlayerRepeatMode.Off;
    }

    #endregion

}