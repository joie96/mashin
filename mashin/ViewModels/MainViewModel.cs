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
    private bool _isDontStopTheMusicEnabled;

    private int _volume = 50;
    private bool _suppressVolumeCommand;

    private string _searchQuery = string.Empty;
    private bool _isSearching;

    private PlayerQueue? _currentPlayerQueue;
    private Track? _currentTrack;
    private readonly ObservableRangeCollection<Track> _currentQueueTracks = new();

    private ObservableCollection<ContextMenuItem> _userMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _queueContextMenuItems = new();

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
        _currentQueueTracks.CollectionChanged += OnCurrentQueueTracksCollectionChanged;

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

    public PlayerQueue? CurrentPlayerQueue
    {
        get => _currentPlayerQueue;
        private set => SetProperty(ref _currentPlayerQueue, value);
    }

    public ObservableRangeCollection<Track> CurrentQueueTracks => _currentQueueTracks;

    public string QueueTrackCountText => $"{_currentQueueTracks.Count} Titel";

    public string QueueTotalDurationText
    {
        get
        {
            var totalSeconds = _currentQueueTracks.Sum(track => Math.Max(0, track.Duration));
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

        _currentQueueTracks.CollectionChanged -= OnCurrentQueueTracksCollectionChanged;

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

    private void OnCurrentQueueTracksCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        OnPropertyChanged(nameof(QueueTrackCountText));
        OnPropertyChanged(nameof(QueueTotalDurationText));
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
                CurrentPlayerQueue = null;
                CurrentTrack = null;
                _currentQueueTracks.Clear();

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
                        CurrentPlayerQueue = activeQueue;

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
                                    _currentQueueTracks.ReplaceRange(nextQueueTracks);
                                }
                            }
                            else
                            {
                                CurrentPlayerQueue = activeQueue;
                                _currentQueueTracks.Clear();
                            }

                            _logger.LogDebug("Retrieved current track from Music Assistant queue: {Name} by {Artist}",
                                CurrentTrack.Name, CurrentTrack.ArtistName);
                        }
                        else
                        {
                            _logger.LogDebug("No current item found in Music Assistant queue for player: {PlayerId}",
                                _playerService.ClientId);

                            CurrentPlayerQueue = activeQueue;
                            CurrentTrack = null;
                            _currentQueueTracks.Clear();
                        }
                    }
                    catch (Exception ex)
                    {
                        _logger.LogWarning(ex, "Failed to retrieve current track from Music Assistant queue");
                        CurrentPlayerQueue = null;
                        CurrentTrack = null;
                        _currentQueueTracks.Clear();
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
                                    CurrentQueueTracks.Where(track => track.IsSelected),
                                    playlist))
                        }))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufügen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                    await MediaActions.AddToFavoritesAsync(CurrentQueueTracks.Where(track => track.IsSelected)))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                Command = new Command(async () =>
                    await MediaActions.RemoveFromFavoritesAsync(CurrentQueueTracks.Where(track => track.IsSelected)))
            }
        };
    }

    #region Queue Menu Actions

    private async Task PlaySelectedQueueIndexAsync()
    {
        var selectedTracks = CurrentQueueTracks
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

        var activeQueue = await GetActiveQueueForContextMenuAsync();
        if (activeQueue == null)
        {
            return;
        }

        await MediaActions.PlayIndexAsync(activeQueue.QueueId, zeroBasedIndex);
    }

    private async Task MoveSelectedQueueItemsNextAsync()
    {
        var activeQueue = await GetActiveQueueForContextMenuAsync();
        if (activeQueue == null)
        {
            return;
        }

        var queueItems = await _musicAssistant.GetQueueItemsAsync(activeQueue.QueueId);
        if (queueItems.Count == 0)
        {
            _logger.LogInformation("Queue is empty, nothing to move");
            return;
        }

        var selectedIndices = CurrentQueueTracks
            .Where(track => track.IsSelected)
            .Select(track => Math.Max(0, track.Index - 1))
            .Distinct()
            .OrderBy(index => index)
            .ToList();

        if (selectedIndices.Count == 0)
        {
            _logger.LogInformation("No queue items selected for 'Als Nächstes spielen'");
            return;
        }

        var selectedQueueItemIds = selectedIndices
            .Where(index => index < queueItems.Count)
            .Select(index => queueItems[index].QueueItemId)
            .Where(id => !string.IsNullOrWhiteSpace(id))
            .ToList();

        if (selectedQueueItemIds.Count == 0)
        {
            _logger.LogInformation("No valid queue items selected for move");
            return;
        }

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

        // Update local queue track order to reflect the move, before reloading from server (optimistic update).
        var validSelectedIndices = selectedIndices
            .Where(index => index >= 0 && index < _currentQueueTracks.Count)
            .Distinct()
            .OrderBy(index => index)
            .ToList();

        if (validSelectedIndices.Count > 0)
        {
            var selectedTracks = validSelectedIndices
                .Select(index => _currentQueueTracks[index])
                .ToList();

            for (var index = validSelectedIndices.Count - 1; index >= 0; index--)
            {
                _currentQueueTracks.RemoveAt(validSelectedIndices[index]);
            }

            var removedBeforeOrAtCurrent = validSelectedIndices.Count(index => index <= currentIndex);
            var localInsertPosition = Math.Min(currentIndex + 1 - removedBeforeOrAtCurrent, _currentQueueTracks.Count);
            localInsertPosition = Math.Max(0, localInsertPosition);

            foreach (var track in selectedTracks)
            {
                _currentQueueTracks.Insert(localInsertPosition, track);
                localInsertPosition++;
            }
        }

        for (var index = 0; index < _currentQueueTracks.Count; index++)
        {
            _currentQueueTracks[index].Index = index + 1;
        }

        await ReloadCurrentQueueTracksAsync(activeQueue.QueueId);
    }

    private async Task MoveSelectedQueueItemsLastAsync()
    {
        var activeQueue = await GetActiveQueueForContextMenuAsync();
        if (activeQueue == null)
        {
            return;
        }

        var queueItems = await _musicAssistant.GetQueueItemsAsync(activeQueue.QueueId);
        if (queueItems.Count == 0)
        {
            _logger.LogInformation("Queue is empty, nothing to move");
            return;
        }

        var selectedIndices = CurrentQueueTracks
            .Where(track => track.IsSelected)
            .Select(track => Math.Max(0, track.Index - 1))
            .Distinct()
            .OrderBy(index => index)
            .ToList();

        if (selectedIndices.Count == 0)
        {
            _logger.LogInformation("No queue items selected for 'Als Letztes spielen'");
            return;
        }

        var selectedQueueItemIds = selectedIndices
            .Where(index => index < queueItems.Count)
            .Select(index => queueItems[index].QueueItemId)
            .Where(id => !string.IsNullOrWhiteSpace(id))
            .ToList();

        if (selectedQueueItemIds.Count == 0)
        {
            _logger.LogInformation("No valid queue items selected for move");
            return;
        }

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

        // Update local queue track order to reflect the move, before reloading from server (optimistic update).
        var validSelectedIndices = selectedIndices
            .Where(index => index >= 0 && index < _currentQueueTracks.Count)
            .Distinct()
            .OrderBy(index => index)
            .ToList();

        if (validSelectedIndices.Count > 0)
        {
            var selectedTracks = validSelectedIndices
                .Select(index => _currentQueueTracks[index])
                .ToList();

            for (var index = validSelectedIndices.Count - 1; index >= 0; index--)
            {
                _currentQueueTracks.RemoveAt(validSelectedIndices[index]);
            }

            foreach (var track in selectedTracks)
            {
                _currentQueueTracks.Add(track);
            }
        }

        for (var index = 0; index < _currentQueueTracks.Count; index++)
        {
            _currentQueueTracks[index].Index = index + 1;
        }

        await ReloadCurrentQueueTracksAsync(activeQueue.QueueId);
    }

    private async Task RemoveSelectedQueueItemsAsync()
    {
        var activeQueue = await GetActiveQueueForContextMenuAsync();
        if (activeQueue == null)
        {
            return;
        }

        var selectedIndices = CurrentQueueTracks
            .Where(track => track.IsSelected)
            .Select(track => Math.Max(0, track.Index - 1))
            .Distinct()
            .OrderByDescending(index => index)
            .ToList();

        if (selectedIndices.Count == 0)
        {
            _logger.LogInformation("No queue items selected for removal");
            return;
        }

        foreach (var selectedIndex in selectedIndices)
        {
            await MediaActions.DeleteQueueItemAsync(activeQueue.QueueId, selectedIndex);

            if (selectedIndex >= 0 && selectedIndex < _currentQueueTracks.Count)
            {
                _currentQueueTracks.RemoveAt(selectedIndex);
            }
        }

        for (var index = 0; index < _currentQueueTracks.Count; index++)
        {
            _currentQueueTracks[index].Index = index + 1;
        }

        await ReloadCurrentQueueTracksAsync(activeQueue.QueueId);
    }

    private async Task ReloadCurrentQueueTracksAsync(string queueId)
    {
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        var queueItems = await _musicAssistant.GetQueueItemsAsync(queueId);
        var reloadedTracks = queueItems
            .Select(queueItem => queueItem.MediaItem)
            .OfType<Track>()
            .ToList();

        for (var index = 0; index < reloadedTracks.Count; index++)
        {
            var track = reloadedTracks[index];
            track.Index = index + 1;
            track.Favorite = _userDataService.IsFavorite(track);
        }

        var localTrackKeys = _currentQueueTracks
            .Select(track => !string.IsNullOrWhiteSpace(track.Uri)
                ? $"uri:{track.Uri.Trim().ToUpperInvariant()}"
                : $"id:{track.ItemId}")
            .ToList();

        var reloadedTrackKeys = reloadedTracks
            .Select(track => !string.IsNullOrWhiteSpace(track.Uri)
                ? $"uri:{track.Uri.Trim().ToUpperInvariant()}"
                : $"id:{track.ItemId}")
            .ToList();

        if (!localTrackKeys.SequenceEqual(reloadedTrackKeys))
        {
            if (reloadedTracks.Count > 0)
            {
                await _musicAssistant.EnrichWithProviderInfoAsync(reloadedTracks);
            }

            _currentQueueTracks.ReplaceRange(reloadedTracks);
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

    private async Task<PlayerQueue?> GetActiveQueueForContextMenuAsync()
    {
        if (string.IsNullOrWhiteSpace(_playerService.ClientId))
        {
            _logger.LogWarning("ClientId is not available. Player connection is missing.");
            return null;
        }

        var activeQueue = await _musicAssistant.GetActiveQueueForPlayerAsync(_playerService.ClientId);
        if (activeQueue == null || string.IsNullOrWhiteSpace(activeQueue.QueueId))
        {
            _logger.LogWarning("No active queue found for player: {PlayerId}", _playerService.ClientId);
            return null;
        }

        return activeQueue;
    }

    #endregion

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