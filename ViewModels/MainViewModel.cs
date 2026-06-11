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

    // Services
    private readonly SettingsService _settings;
    private readonly MusicAssistantService _musicAssistant;
    private readonly IUserDataService _userDataService;
    private readonly ISendspinPlayerService _sendspinPlayerService;
    private readonly INavigationService _navigationService;
    private readonly IOverlayService _overlayService;
    private readonly IContextMenuService _contextMenuService;
    private readonly IPlaybackService _playbackService;
    private readonly ILogger<MainViewModel> _logger;
    private readonly ObservableRangeCollection<Playlist> _playlists = new();


    // Player
    private bool _isMuted;
    private bool? _shuffleEnabled;
    private string? _repeatMode;
    private PlaybackStateModel _playState = new(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow);
    private double _duration;
    private double _position;
    private double _sliderPosition;
    private double _volume = 50;
    private bool _suppressVolumeCommand;
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

    // Search
    private string _searchQuery = string.Empty;
    private bool _isSearching;

    // Queue
    private Track? _currentTrack;
    private readonly ObservableRangeCollection<QueueItem> _currentQueueItems;

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
        ISendspinPlayerService playerService,
        INavigationService navigationService,
        IOverlayService overlayService,
        IMediaItemActions mediaActions,
        IContextMenuService contextMenuService,
        IPlaybackService playbackService,
        ILogger<MainViewModel> logger)
    {
        _settings = settings;
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _sendspinPlayerService = playerService;
        _navigationService = navigationService;
        _overlayService = overlayService;
        _contextMenuService = contextMenuService;
        _playbackService = playbackService;
        _currentQueueItems = _playbackService.CurrentQueueItems;
        _logger = logger;
        MediaActions = mediaActions;
        _selectedAudioQuality = _settings.GetSendspinPreferredAudioCodec();
        _volume = _settings.GetInitialVolume();
        _isMuted = _settings.GetInitialMuted();
        _currentQueueItems.CollectionChanged += OnCurrentQueueItemsCollectionChanged;
        _availablePlayers.CollectionChanged += OnAvailablePlayersCollectionChanged;

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

        // Subscribe to playback state events
        _playbackService.PropertyChanged += OnPlaybackServicePropertyChanged;
        PlayState = _playbackService.PlaybackState;
        _suppressVolumeCommand = true;
        try
        {
            Volume = _playbackService.Volume;
        }
        finally
        {
            _suppressVolumeCommand = false;
        }

        IsMuted = _playbackService.IsMuted;
        ShuffleEnabled = _playbackService.ShuffleEnabled;
        RepeatMode = _playbackService.RepeatMode;
        var dontStopTheMusicEnabled = _playbackService.DontStopTheMusicEnabled == true;
        if (_isDontStopTheMusicEnabled != dontStopTheMusicEnabled)
        {
            _isDontStopTheMusicEnabled = dontStopTheMusicEnabled;
            OnPropertyChanged(nameof(IsDontStopTheMusicEnabled));
        }

        Duration = _playbackService.DurationSeconds;
        if (PlayState.State != PlayerPlaybackState.Seeking)
        {
            Position = _playbackService.PositionSeconds;
            SliderPosition = _playbackService.PositionSeconds;
        }

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

    public PlaybackStateModel PlayState
    {
        get => _playState;
        private set => SetProperty(ref _playState, value);
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

                if (oldTrack?.Uri != value?.Uri)
                {
                    CurrentTrackChanged?.Invoke(this, value);
                }
            }
        }
    }

    public double PlayerBarProgress => Duration <= 0 ? 0 : Math.Clamp(Position / Duration, 0, 1);

    public Artist? CurrentTrackPrimaryArtist => CurrentTrack?.Artists?.FirstOrDefault();

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
        var autoLoginSucceeded = await _musicAssistant.AutoLoginAsync(raiseLoginRequest: true);
        if (!autoLoginSucceeded)
        {
            _logger.LogWarning("Startup initialization paused because user is not authenticated.");
            return;
        }

        CurrentSection = NavigationSection.Home;
        await _navigationService.NavigateToAsync<HomePage>();

        // Establish connection to Sendspin server (if configured).
        if (!string.IsNullOrWhiteSpace(_settings.SendspinUrl))
        {
            var uri = new Uri(_settings.SendspinUrl);
            await _sendspinPlayerService.ConnectAsync(uri);
        }

        await RefreshAvailablePlayersAsync();
        ApplyInitialSelectedPlayer();
        await _playbackService.InitializeAsync();

        // Load playlists once and keep local list refreshed as needed.
        await RefreshPlaylistsAsync();

        // Build Context Menus
        BuildUserMenuItems();
        BuildQueueContextMenuItems();
        BuildCurrentTrackContextMenuItems();

        // Set initial queue state
        var dontStopTheMusicEnabled = _playbackService.DontStopTheMusicEnabled == true;
        if (_isDontStopTheMusicEnabled != dontStopTheMusicEnabled)
        {
            _isDontStopTheMusicEnabled = dontStopTheMusicEnabled;
            OnPropertyChanged(nameof(IsDontStopTheMusicEnabled));
        }

        // Set initial position from playback service state
        if (_playbackService.DurationSeconds > 0)
        {
            Duration = _playbackService.DurationSeconds;
            Position = _playbackService.PositionSeconds;
            SliderPosition = _playbackService.PositionSeconds;
        }

    }

    public async ValueTask DisposeAsync()
    {
        _currentQueueItems.CollectionChanged -= OnCurrentQueueItemsCollectionChanged;
        _availablePlayers.CollectionChanged -= OnAvailablePlayersCollectionChanged;

        _musicAssistant.LoginRequired -= OnLoginRequired;

        _playbackService.PropertyChanged -= OnPlaybackServicePropertyChanged;

        _navigationService.PropertyChanged -= OnNavigationServicePropertyChanged;

        await Task.CompletedTask;
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

        var activePlayerId = _playbackService.ActivePlayerId;
        if (string.IsNullOrWhiteSpace(activePlayerId))
        {
            _logger.LogWarning("No active player available for playlist playback.");
            return;
        }

        try
        {
            _logger.LogInformation("Play playlist: {Name}", playlist.Name);
            await MediaActions.PlayMediaAsync(playlist);
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
            IsMuted = !IsMuted;
            _settings.SetInitialMuted(IsMuted);
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
            Position = clamped;
            SliderPosition = clamped;
            await _playbackService.SeekAsync(clamped, Duration);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to seek");
        }
    }

    private void BeginSeek()
    {
        _playbackService.PlaybackState = new PlaybackStateModel(PlayerPlaybackState.Seeking, DateTimeOffset.UtcNow);
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
            await _sendspinPlayerService.UpdatePreferredAudioCodecAsync(codec);
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
        if (e.PropertyName == nameof(IPlaybackService.PlaybackState))
        {
            PlayState = _playbackService.PlaybackState;
            return;
        }

        if (e.PropertyName == nameof(IPlaybackService.Volume))
        {
            _suppressVolumeCommand = true;
            try
            {
                Volume = _playbackService.Volume;
                _settings.SetInitialVolume(_playbackService.Volume);
            }
            finally
            {
                _suppressVolumeCommand = false;
            }

            return;
        }

        if (e.PropertyName == nameof(IPlaybackService.IsMuted))
        {
            IsMuted = _playbackService.IsMuted;
            _settings.SetInitialMuted(_playbackService.IsMuted);
            return;
        }

        if (e.PropertyName == nameof(IPlaybackService.ShuffleEnabled))
        {
            ShuffleEnabled = _playbackService.ShuffleEnabled;
            return;
        }

        if (e.PropertyName == nameof(IPlaybackService.RepeatMode))
        {
            RepeatMode = _playbackService.RepeatMode;
            return;
        }

        if (e.PropertyName == nameof(IPlaybackService.DurationSeconds))
        {
            Duration = _playbackService.DurationSeconds;
            if (SliderPosition > Duration)
            {
                SliderPosition = Duration;
            }

            return;
        }

        if (e.PropertyName == nameof(IPlaybackService.PositionSeconds))
        {
            if (PlayState.State != PlayerPlaybackState.Seeking)
            {
                var position = _playbackService.PositionSeconds;
                Position = position;
                SliderPosition = position;
            }

            return;
        }

        if (e.PropertyName == nameof(IPlaybackService.DontStopTheMusicEnabled))
        {
            var dontStopTheMusicEnabled = _playbackService.DontStopTheMusicEnabled == true;
            if (_isDontStopTheMusicEnabled != dontStopTheMusicEnabled)
            {
                _isDontStopTheMusicEnabled = dontStopTheMusicEnabled;
                OnPropertyChanged(nameof(IsDontStopTheMusicEnabled));
            }

            return;
        }

        if (e.PropertyName == nameof(IPlaybackService.CurrentTrack))
        {
            CurrentTrack = _playbackService.CurrentTrack;
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

    private void OnAvailablePlayersCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        OnPropertyChanged(nameof(HasAvailablePlayers));
        OnPropertyChanged(nameof(HasNoAvailablePlayers));
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

        await _playbackService.PlayQueueIndexAsync(zeroBasedIndex);
    }

    private async Task MoveSelectedQueueItemsNextAsync()
    {
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
        var currentIndex = Math.Max(0, _playbackService.CurrentQueueIndex ?? 0);
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
            _logger.LogInformation("No queue items selected for removal");
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
        var preferredPlayerId = _availablePlayers
            .FirstOrDefault(player => string.Equals(player.PlayerId, _sendspinPlayerService.PlayerId, StringComparison.Ordinal))?.PlayerId
            ?? _availablePlayers.FirstOrDefault(player => player.Available)?.PlayerId
            ?? _availablePlayers.FirstOrDefault()?.PlayerId;

        if (string.IsNullOrWhiteSpace(preferredPlayerId))
        {
            return;
        }

        SetSelectedPlayerSilently(preferredPlayerId);
        _ = _playbackService.SetOutputModeAsync(_playbackService.OutputMode, preferredPlayerId);
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
                .Where(player => player.Available)
                .Where(player => !string.IsNullOrWhiteSpace(player.PlayerId))
                .OrderByDescending(player => string.Equals(player.PlayerId, _sendspinPlayerService.PlayerId, StringComparison.Ordinal))
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
            await _playbackService.SetOutputModeAsync(_playbackService.OutputMode, playerId);
            var dontStopTheMusicEnabled = _playbackService.DontStopTheMusicEnabled == true;
            if (_isDontStopTheMusicEnabled != dontStopTheMusicEnabled)
            {
                _isDontStopTheMusicEnabled = dontStopTheMusicEnabled;
                OnPropertyChanged(nameof(IsDontStopTheMusicEnabled));
            }

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
                await _playbackService.SetOutputModeAsync(_playbackService.OutputMode, previousPlayerId);
            }
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

    #endregion

    #region Helper - Navigation

    private Task ExecuteOpenSettingsAsync()
    {
        _logger.LogInformation("Einstellungen ist noch nicht implementiert.");
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