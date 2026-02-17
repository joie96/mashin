using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
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
    private readonly IContextMenuService _contextMenuService;
    private readonly ILogger<MainViewModel> _logger;

    private readonly IDispatcherTimer _positionTimer;

    private bool _isLoadingPlaylists;
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

    private readonly ObservableCollection<ContextMenuItem> _userMenuItems;

    private ObservableRangeCollection<Playlist> _playlists = new();

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
        IContextMenuService contextMenuService,
        ILogger<MainViewModel> logger)
    {
        _settings = settings;
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _playerService = playerService;
        _navigationService = navigationService;
        _contextMenuService = contextMenuService;
        _logger = logger;

        _userMenuItems = new ObservableCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Logout",
                Command = new Command(async () => await ExecuteLogoutAsync())
            }
        };

        // Navigation Commands
        NavigateToHomeCommand = new Command(async () => await navigationService.NavigateToAsync<HomePage>());
        NavigateToExploreCommand = new Command(async () => await navigationService.NavigateToAsync<ExplorePage>());
        NavigateToFavoritesCommand = new Command(async () => await navigationService.NavigateToAsync<FavoritesPage>());
        NavigateToPlaylistCommand = new Command<Playlist>(async (playlist) => await navigationService.NavigateToAsync<PlaylistDetailPage>(playlist));

        SearchCommand = new Command(async () => await ExecuteSearchAsync());

        ShowUserMenuCommand = new Command<View>(async (anchor) => await ShowUserMenuAsync(anchor));

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
        get => _playlists;
        private set => SetProperty(ref _playlists, value);
    }

    public bool IsLoadingPlaylists
    {
        get => _isLoadingPlaylists;
        private set => SetProperty(ref _isLoadingPlaylists, value);
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

    #endregion

    #region Bindable Properties (Search)

    public string SearchQuery
    {
        get => _searchQuery;
        set => SetProperty(ref _searchQuery, value);
    }

    #endregion

    #region Commands

    public ICommand NavigateToHomeCommand { get; }
    public ICommand NavigateToExploreCommand { get; }
    public ICommand NavigateToFavoritesCommand { get; }
    public ICommand NavigateToPlaylistCommand { get; }

    public ICommand PreviousTrackCommand { get; }

    public ICommand NextTrackCommand { get; }

    public ICommand TogglePlayPauseCommand { get; }

    public ICommand SeekCommand { get; }

    public ICommand PlayPlaylistCommand { get; }

    public ICommand SearchCommand { get; }

    public ICommand ShowUserMenuCommand { get; }

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

        _ = await _userDataService.EnsureLoadedAsync(forceRefresh: true);

        await LoadPlaylistsAsync();
    }

    public async ValueTask DisposeAsync()
    {
        _positionTimer.Tick -= OnPositionTimerTick;
        _positionTimer.Stop();

        _playerService.ConnectionStateChanged -= OnConnectionStateChanged;
        _playerService.GroupStateChanged -= OnGroupStateChanged;

        _navigationService.PropertyChanged -= OnNavigationServicePropertyChanged;

        await Task.CompletedTask;
    }

    #endregion

    #region Playlist Logic

    private async Task LoadPlaylistsAsync()
    {
        if (IsLoadingPlaylists)
        {
            return;
        }

        IsLoadingPlaylists = true;
        try
        {
            // Get all user playlists (username--*) ordered by sort name.
            var prefix = GetUserPlaylistPrefix();
            var playlists = await _musicAssistant.GetLibraryPlaylistsAsync(
                search: string.IsNullOrWhiteSpace(prefix) ? null : prefix,
                orderBy: "sort_name");

            // Remove username prefix from display name
            foreach (var playlist in playlists)
            {
                if (!string.IsNullOrWhiteSpace(prefix)
                    && !string.IsNullOrWhiteSpace(playlist.Name)
                    && playlist.Name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
                {
                    playlist.DisplayName = playlist.Name[prefix.Length..];
                }
            }

            // Load playlists progressively.
            var visiblePlaylists = playlists.Take(10).ToList();
            Playlists = new ObservableRangeCollection<Playlist>(visiblePlaylists);
            await Task.Yield();
            IsLoadingPlaylists = false;

            var remainingPlaylists = playlists.Skip(visiblePlaylists.Count).ToList();
            if (remainingPlaylists.Count > 0)
            {
                foreach (var batch in remainingPlaylists.Chunk(20))
                {
                    Playlists.AddRange(batch);
                    await Task.Yield();
                }
            }

            _logger.LogInformation("Loaded {Count} playlists", Playlists.Count);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to load playlists");
        }
        finally
        {
            IsLoadingPlaylists = false;
        }
    }

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

    public async Task<bool> CreatePlaylistAsync(string name)
    {
        name = name?.Trim() ?? string.Empty;
        if (string.IsNullOrWhiteSpace(name))
        {
            return false;
        }

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
            if (playlist != null)
            {
                await LoadPlaylistsAsync();
                return true;
            }
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to create playlist: {Name}", name);
        }

        return false;
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

                        if (activeQueue?.CurrentItem?.MediaItem != null)
                        {
                            CurrentTrack = activeQueue.CurrentItem.MediaItem;

                            _logger.LogDebug("Retrieved current track from Music Assistant queue: {Name} by {Artist}",
                                CurrentTrack.Name, CurrentTrack.ArtistName);
                        }
                        else
                        {
                            _logger.LogDebug("No current item found in Music Assistant queue for player: {PlayerId}",
                                _playerService.ClientId);

                            CurrentTrack = null;
                        }
                    }
                    catch (Exception ex)
                    {
                        _logger.LogWarning(ex, "Failed to retrieve current track from Music Assistant queue");
                        CurrentTrack = null;
                    }
                }
                else
                {
                    CurrentTrack = null;
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

    private async Task ShowUserMenuAsync(View? anchor)
    {
        if (anchor is null || _userMenuItems.Count == 0)
        {
            return;
        }

        await _contextMenuService.ShowContextMenuAsync(_userMenuItems, anchor);
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