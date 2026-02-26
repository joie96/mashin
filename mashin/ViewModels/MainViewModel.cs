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
using System.Collections.Specialized;
using System.ComponentModel;
using System.Runtime.CompilerServices;
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
    private readonly IUserDataService _userDataService;
    private readonly IPlayerService _playerService;
    private readonly INavigationService _navigationService;
    private readonly IOverlayService _overlayService;
    private readonly IPlaylistStoreService _playlistStore;
    private readonly IContextMenuService _contextMenuService;
    private readonly IQueueSyncService _queueSyncService;
    private readonly ILogger<MainViewModel> _logger;


    // Player
    private bool _isPlaying;
    private bool _isBuffering;
    private bool _isMuted;
    private bool? _shuffleEnabled;
    private string? _repeatMode;
    private double _duration;
    private double _position;
    private double _sliderPosition;
    private bool _isSeeking;
    private double _volume = 50;
    private bool _suppressVolumeCommand;

    private bool _isDontStopTheMusicEnabled;

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

    public event EventHandler<Track?>? CurrentTrackChanged;
    public event Func<Task>? CloseQueueViewRequested;

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
        IQueueSyncService queueSyncService,
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
        _queueSyncService = queueSyncService;
        _logger = logger;
        MediaActions = mediaActions;
        _currentQueueItems.CollectionChanged += OnCurrentQueueItemsCollectionChanged;

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
        ToggleMuteCommand = new Command(async () => await ToggleMuteAsync());
        ToggleDontStopTheMusicCommand = new Command(() => IsDontStopTheMusicEnabled = !IsDontStopTheMusicEnabled);
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

        _navigationService.PropertyChanged += OnNavigationServicePropertyChanged;
        IsNavigating = _navigationService.IsNavigating;

        _musicAssistant.LoginRequired += OnLoginRequired;
        _playlistStore.PropertyChanged += OnPlaylistStorePropertyChanged;

        // Subscribe to player state events
        _playerService.PropertyChanged += OnPlayerServicePropertyChanged;

        // Subscribe to queue sync updates
        _queueSyncService.CurrentPlayerQueueUpdated += OnCurrentPlayerQueueUpdated;
        _queueSyncService.CurrentTrackUpdated += OnCurrentTrackUpdated;
        _queueSyncService.CurrentQueueItemsUpdated += OnCurrentQueueItemsUpdated;
        
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

    public bool IsMuted
    {
        get => _isMuted;
        private set => SetProperty(ref _isMuted, value);
    }

    public bool? ShuffleEnabled
    {
        get => _shuffleEnabled;
        private set => SetProperty(ref _shuffleEnabled, value);
    }

    public string? RepeatMode
    {
        get => _repeatMode;
        private set => SetProperty(ref _repeatMode, value);
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

            _ = SetDontStopTheMusicAsync(value);
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
    public ICommand NavigateToPlaylistCommand { get; }

    // Playback Commands
    public ICommand PreviousTrackCommand { get; }
    public ICommand NextTrackCommand { get; }
    public ICommand TogglePlayPauseCommand { get; }
    public ICommand ToggleMuteCommand { get; }
    public ICommand ToggleDontStopTheMusicCommand { get; }
    public ICommand ToggleCurrentTrackFavoriteCommand { get; }
    public ICommand ShowCurrentTrackContextMenuCommand { get; }
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

        // Load playlists and store locally
        await _playlistStore.RefreshAsync();

        // Build Context Menus
        BuildUserMenuItems();
        BuildQueueContextMenuItems();
        BuildCurrentTrackContextMenuItems();  

        // Set initial queue state
        await _queueSyncService.RefreshNowAsync();

        // Start queue sync loop
        await _queueSyncService.StartAsync();

        
    }

    public async ValueTask DisposeAsync()
    {
        _currentQueueItems.CollectionChanged -= OnCurrentQueueItemsCollectionChanged;

        _musicAssistant.LoginRequired -= OnLoginRequired;

        _playerService.PropertyChanged -= OnPlayerServicePropertyChanged;
        _queueSyncService.CurrentPlayerQueueUpdated -= OnCurrentPlayerQueueUpdated;
        _queueSyncService.CurrentTrackUpdated -= OnCurrentTrackUpdated;
        _queueSyncService.CurrentQueueItemsUpdated -= OnCurrentQueueItemsUpdated;
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

    #region Playlist Actions

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

    #region Player Actions

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

    private async Task ToggleMuteAsync()
    {
        try
        {
            await _playerService.SendCommandAsync(
                "mute",
                new Dictionary<string, object>
                {
                    ["muted"] = !IsMuted,
                });
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to toggle mute");
        }
    }

    private async Task SeekAsync(double seconds)
    {
        try
        {
            _isSeeking = true;

            var clamped = Math.Max(0, Math.Min(Duration, seconds));
            Position = clamped;
            SliderPosition = clamped;

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
            _isSeeking = false;
        }
    }

    private async Task SetVolumeAsync(int volume)
    {
        try
        {
            var clamped = Math.Max(0, Math.Min(100, volume));

            await _playerService.SendCommandAsync(
                "volume",
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

    #region Event Handlers

    private void OnPlayerServicePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        switch (e.PropertyName)
        {
            case nameof(IPlayerService.IsPlaying):
                IsPlaying = _playerService.IsPlaying;
                break;

            case nameof(IPlayerService.IsBuffering):
                IsBuffering = _playerService.IsBuffering;
                break;

            case nameof(IPlayerService.Volume):
                _suppressVolumeCommand = true;
                try
                {
                    Volume = _playerService.Volume;
                }
                finally
                {
                    _suppressVolumeCommand = false;
                }
                break;

            case nameof(IPlayerService.IsMuted):
                IsMuted = _playerService.IsMuted;
                break;

            case nameof(IPlayerService.ShuffleEnabled):
                ShuffleEnabled = _playerService.ShuffleEnabled;
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
                if (!_isSeeking)
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

    private void OnPlaylistStorePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(IPlaylistStoreService.IsLoading))
        {
            OnPropertyChanged(nameof(IsLoadingPlaylists));

            if (!_playlistStore.IsLoading)
            {
                BuildCurrentTrackContextMenuItems();
            }
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
        IsDontStopTheMusicEnabled = _queueSyncService.CurrentPlayerQueue?.DontStopTheMusicEnabled == true;

    }

    private void OnCurrentTrackUpdated(object? sender, EventArgs e)
    {
        CurrentTrack = _queueSyncService.CurrentTrack;
    }

    private void OnCurrentQueueItemsUpdated(object? sender, QueueItemsChangedEventArgs e)
    {
        _playerService.PositionSeconds = _queueSyncService.CurrentPlayerQueue.ElapsedTime;
        
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
                    _playlistStore.Playlists
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
            _playlistStore.Playlists
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

    #region Helpers
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

    #endregion
}