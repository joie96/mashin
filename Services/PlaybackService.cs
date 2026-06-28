using mashin.Collections;
using mashin.Models;
using Microsoft.Extensions.Logging;
using System.ComponentModel;
using System.IO;
using System.Runtime.CompilerServices;

namespace mashin.Services;

public enum PlaybackOutputMode
{
    // Play local/offline files directly on this device.
    LocalOffline,
    // Hybrid mode: use Sendspin as MA player while local-capable tracks can be rendered locally.
    LocalSendspin,
    // Control a different MA player/device (not this device's local audio output path).
    RemoteOnly
}

#region Interface

public interface IPlaybackService : IAsyncDisposable, INotifyPropertyChanged
{
    PlaybackOutputMode OutputMode { get; }
    string? ActivePlayerId { get; }
    PlayerState PlaybackState { get; set; }
    int Volume { get; }
    bool IsMuted { get; }
    bool? ShuffleEnabled { get; }
    string? RepeatMode { get; }
    bool? DontStopTheMusicEnabled { get; }
    int? CurrentQueueIndex { get; }
    int QueueItemCount { get; }
    double DurationSeconds { get; }
    double PositionSeconds { get; }

    QueueItem? CurrentQueueItem { get; }
    ObservableRangeCollection<QueueItem> CurrentQueueItems { get; }

    Task InitializeAsync(CancellationToken cancellationToken = default);
    Task SetOutputModeAsync(PlaybackOutputMode mode, string? targetPlayerId = null, CancellationToken cancellationToken = default);
    Task PlayMediaAsync(IReadOnlyList<MediaItem> items);
    Task PlayMediaNextAsync(IReadOnlyList<MediaItem> items);
    Task PlayMediaLastAsync(IReadOnlyList<MediaItem> items);
    Task ShufflePlayMediaAsync(IReadOnlyList<MediaItem> items);
    Task ClearQueueAsync(bool skipStop = false);

    Task PlayQueueIndexAsync(int index, CancellationToken cancellationToken = default);
    Task MoveQueueItemAsync(string queueItemId, int posShift, CancellationToken cancellationToken = default);
    Task DeleteQueueItemAsync(string queueItemId, CancellationToken cancellationToken = default);

    Task TogglePlayPauseAsync(CancellationToken cancellationToken = default);
    Task NextTrackAsync(CancellationToken cancellationToken = default);
    Task PreviousTrackAsync(CancellationToken cancellationToken = default);
    Task SeekAsync(double seconds, double durationSeconds, CancellationToken cancellationToken = default);
    Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default);
    Task ToggleMuteAsync(bool currentMuted, CancellationToken cancellationToken = default);
    Task SetPreferredAudioCodecAsync(string codec, CancellationToken cancellationToken = default);
    Task ToggleShuffleAsync(bool? currentShuffleEnabled, CancellationToken cancellationToken = default);
    Task ToggleRepeatModeAsync(string? currentRepeatMode, CancellationToken cancellationToken = default);
    Task SetDontStopTheMusicAsync(bool enabled, CancellationToken cancellationToken = default);
    
}

#endregion

public sealed class PlaybackService : IPlaybackService
{
    #region Fields
    
    // Preload window for online tracks in hybrid mode (Local + Sendspin). If the current track position is within this window and next track not cached, we will prewarm it on sendpsin
    private const double PrewarmWindowSeconds = 20;

    // Core services
    private readonly MusicAssistantService _musicAssistant;
    private readonly SettingsService _settingsService;
    private readonly IMusicAssistantEventHub _musicAssistantEventHub;
    private readonly ILogger<PlaybackService> _logger;
    private readonly Dictionary<PlaybackOutputMode, IPlayerService> _players;
    private readonly ILocalPlayerService? _localPlayer;
    private readonly SendspinPlayerService? _sendspinPlayer;
    private readonly Task _hybridMonitorTask;

    // Queue 
    private readonly ObservableRangeCollection<QueueItem> _currentQueueItems = new();
    private readonly CancellationTokenSource _disposeCts = new();
    private CancellationTokenSource? _queueConfirmCts;
    private int _queueConfirmRequestId;

    // Active output/player routing
    private PlaybackOutputMode _outputMode = PlaybackOutputMode.LocalSendspin;
    private string? _activePlayerId;
    private IPlayerService _activePlayer;

    // Playback states projected to UI
    private PlayerState _playbackState = new()
    {
        State = PlayerStateType.Idle,
        ActiveSinceUtc = DateTimeOffset.UtcNow
    };
    private int _volume = 50;
    private bool _isMuted;
    private bool? _shuffleEnabled;
    private string? _repeatMode;
    private bool? _dontStopTheMusicEnabled;
    private string? _activeQueueId;
    private QueueItem? _currentQueueItem;
    private int? _currentQueueIndex;
    private int _queueItemCount;
    private double _durationSeconds;
    private double _positionSeconds;

    // Hybrid LocalSendspin runtime state.
    private bool _isHybridLocalPlaybackActive;
    private string? _hybridActiveQueueItemId;
    private bool _prewarmTriggered;
    private int? _prewarmedQueueIndex;

    #endregion

    #region Construction

    public PlaybackService(
        MusicAssistantService musicAssistant,
        SettingsService settingsService,
        IMusicAssistantEventHub musicAssistantEventHub,
        IEnumerable<IPlayerService> playerServices,
        ILogger<PlaybackService> logger)
    {
        _musicAssistant = musicAssistant;
        _settingsService = settingsService;
        _musicAssistantEventHub = musicAssistantEventHub;
        _logger = logger;

        _players = playerServices
            .GroupBy(service => service.OutputMode)
            .ToDictionary(group => group.Key, group => group.First());

        _localPlayer = _players.TryGetValue(PlaybackOutputMode.LocalOffline, out var local)
            ? local as ILocalPlayerService
            : null;
        _sendspinPlayer = _players.TryGetValue(PlaybackOutputMode.LocalSendspin, out var sendspinPlayer)
            ? sendspinPlayer as SendspinPlayerService
            : null;

        _activePlayer = _players.TryGetValue(PlaybackOutputMode.LocalSendspin, out var sendspin)
            ? sendspin
            : _players.Values.First();

        _activePlayer.PropertyChanged += OnActivePlayerPropertyChanged;
        if (_localPlayer != null)
        {
            _localPlayer.CurrentPlayingItemEnded += OnLocalPlayerCurrentPlayingItemEnded;
            _localPlayer.PropertyChanged += OnLocalPlayerPropertyChanged;
        }

        _musicAssistantEventHub.QueueEventReceived += OnQueueEventReceived;

        // Start the hybrid monitor loop in the background to handle prewarming and hybrid playback (sendspin + local) state management
        _hybridMonitorTask = Task.Run(HybridMonitorLoopAsync, CancellationToken.None);
    }

    #endregion

    #region Events

    public event PropertyChangedEventHandler? PropertyChanged;

    #endregion

    #region Properties

    public string? ActivePlayerId
    {
        get => _activePlayerId;
        private set => SetProperty(ref _activePlayerId, Normalize(value));
    }

    public PlayerState PlaybackState
    {
        get => _playbackState;
        set => SetProperty(ref _playbackState, value ?? new PlayerState
        {
            State = PlayerStateType.Unknown,
            ActiveSinceUtc = DateTimeOffset.UtcNow
        });
    }

    public int Volume
    {
        get => _volume;
        private set => SetProperty(ref _volume, Math.Clamp(value, 0, 100));
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

    public bool? DontStopTheMusicEnabled
    {
        get => _dontStopTheMusicEnabled;
        private set => SetProperty(ref _dontStopTheMusicEnabled, value);
    }

    public int? CurrentQueueIndex
    {
        get => _currentQueueIndex;
        private set => SetProperty(ref _currentQueueIndex, value);
    }

    public int QueueItemCount
    {
        get => _queueItemCount;
        private set => SetProperty(ref _queueItemCount, Math.Max(0, value));
    }

    public double DurationSeconds
    {
        get => _durationSeconds;
        private set => SetProperty(ref _durationSeconds, Math.Max(0, value));
    }

    public double PositionSeconds
    {
        get => _positionSeconds;
        private set => SetProperty(ref _positionSeconds, Math.Max(0, value));
    }

    public PlaybackOutputMode OutputMode
    {
        get => _outputMode;
        private set => SetProperty(ref _outputMode, value);
    }

    public QueueItem? CurrentQueueItem => _currentQueueItem;

    public ObservableRangeCollection<QueueItem> CurrentQueueItems => _currentQueueItems;

    #endregion

    #region Playback Lifecycle

    public async Task InitializeAsync(CancellationToken cancellationToken = default)
    {
        await SetOutputModeAsync(OutputMode, cancellationToken: cancellationToken);
    }

    public async Task SetOutputModeAsync(PlaybackOutputMode mode, string? targetPlayerId = null, CancellationToken cancellationToken = default)
    {
        _logger.LogDebug("SetOutputMode requested: Mode={Mode}, TargetPlayerId={TargetPlayerId}", mode, targetPlayerId);

        var resolvedTargetPlayerId = mode switch
        {
            PlaybackOutputMode.LocalOffline => null,
            PlaybackOutputMode.LocalSendspin => _settingsService.GetSendspinMusicAssistantPlayerId(),
            PlaybackOutputMode.RemoteOnly => !string.IsNullOrWhiteSpace(targetPlayerId)
                ? targetPlayerId
                : throw new ArgumentException("RemoteOnly mode requires a targetPlayerId.", nameof(targetPlayerId)),
            _ => throw new ArgumentOutOfRangeException(nameof(mode), mode, "Unsupported playback output mode.")
        };

        if (OutputMode == mode
            && string.Equals(ActivePlayerId, resolvedTargetPlayerId, StringComparison.Ordinal))
        {
            _logger.LogDebug("SetOutputMode skipped: already active. Mode={Mode}, ActivePlayerId={ActivePlayerId}", mode, ActivePlayerId);
            return;
        }

        var nextPlayer = mode switch
        {
            PlaybackOutputMode.LocalOffline => _players[PlaybackOutputMode.LocalOffline],
            PlaybackOutputMode.RemoteOnly => _players[PlaybackOutputMode.RemoteOnly],
            _ => _players[PlaybackOutputMode.LocalSendspin]
        };

        if (!ReferenceEquals(_activePlayer, nextPlayer))
        {
            _activePlayer.PropertyChanged -= OnActivePlayerPropertyChanged;

            await _activePlayer.DeactivateAsync();
            _activePlayer = nextPlayer;
            _activePlayer.PropertyChanged += OnActivePlayerPropertyChanged;
        }

        OutputMode = mode;

        await _activePlayer.ActivateAsync(resolvedTargetPlayerId, cancellationToken);
        ActivePlayerId = resolvedTargetPlayerId;

        // When leaving LocalSendspin: reset hybrid state and disable Sendspin external source.
        if (mode != PlaybackOutputMode.LocalSendspin)
        {
            _isHybridLocalPlaybackActive = false;
            _prewarmTriggered = false;
            _prewarmedQueueIndex = null;
            _hybridActiveQueueItemId = null;

            if (_sendspinPlayer != null)
            {
                _ = Task.Run(async () =>
                {
                    try
                    {
                        await _sendspinPlayer.SetExternalSourceAsync(false, _disposeCts.Token);
                    }
                    catch (Exception ex)
                    {
                        _logger.LogDebug(ex, "Failed to exit external source while switching away from LocalSendspin mode.");
                    }
                }, CancellationToken.None);
            }
        }

        _logger.LogDebug("Output mode activated: Mode={Mode}, ActivePlayerId={ActivePlayerId}, PlayerService={PlayerService}",
            OutputMode,
            ActivePlayerId,
            _activePlayer.GetType().Name);

        SyncStateFromPlayer();

        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            await _musicAssistantEventHub.StopAsync(cancellationToken);
            ResetQueueState();
            _logger.LogDebug("Switched to LocalOffline. Event hub stopped and queue state reset.");
            return;
        }

        await _musicAssistantEventHub.StartAsync(cancellationToken);
        await RefreshQueueAsync(cancellationToken);
        _logger.LogDebug("Event hub started and initial queue refresh completed for ActivePlayerId={ActivePlayerId}", ActivePlayerId);
    }

    #endregion

    #region Media Commands
    // Queue manipulation and enqueue commands.

    public async Task PlayMediaAsync(IReadOnlyList<MediaItem> items)
    {
        if (items == null || items.Count == 0)
        {
            _logger.LogWarning("No items selected to play");
            return;
        }

        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            await PlayMediaLocalAsync(items, QueueOption.Replace);
            return;
        }

        if (string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            _logger.LogWarning("PlayMedia ignored because there is no active player id.");
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            _logger.LogWarning("No playable tracks resolved for PlayMedia");
            return;
        }

        PlaybackState = new PlayerState { State = PlayerStateType.Buffering, ActiveSinceUtc = DateTimeOffset.UtcNow };
        _logger.LogDebug("PlayMedia request set PlaybackState to Buffering. OutputMode={OutputMode}, ActivePlayerId={ActivePlayerId}, ItemCount={ItemCount}", OutputMode, ActivePlayerId, items.Count);

        try
        {
            await _musicAssistant.PlayMediaAsync(ActivePlayerId!, resolvedItems, QueueOption.Replace);
        }
        catch (Exception ex)
        {
            PlaybackState = new PlayerState { State = PlayerStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
            _logger.LogError(ex, "PlayMedia request failed for ActivePlayerId={ActivePlayerId}", ActivePlayerId);
            throw;
        }
    }

    public async Task PlayMediaNextAsync(IReadOnlyList<MediaItem> items)
    {
        if (items == null || items.Count == 0)
        {
            _logger.LogWarning("No items selected to play");
            return;
        }

        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            await PlayMediaLocalAsync(items, QueueOption.Next);
            return;
        }

        if (string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            _logger.LogWarning("PlayMediaNext ignored because there is no active player id.");
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            _logger.LogWarning("No playable tracks resolved for PlayMediaNext");
            return;
        }

        PlaybackState = new PlayerState { State = PlayerStateType.Buffering, ActiveSinceUtc = DateTimeOffset.UtcNow };
        _logger.LogDebug("PlayMediaNext request set PlaybackState to Buffering. OutputMode={OutputMode}, ActivePlayerId={ActivePlayerId}, ItemCount={ItemCount}", OutputMode, ActivePlayerId, items.Count);

        try
        {
            await _musicAssistant.PlayMediaAsync(ActivePlayerId!, resolvedItems, QueueOption.Next);
        }
        catch (Exception ex)
        {
            PlaybackState = new PlayerState { State = PlayerStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
            _logger.LogError(ex, "PlayMediaNext request failed for ActivePlayerId={ActivePlayerId}", ActivePlayerId);
            throw;
        }
    }

    public async Task PlayMediaLastAsync(IReadOnlyList<MediaItem> items)
    {
        if (items == null || items.Count == 0)
        {
            _logger.LogWarning("No items selected to play");
            return;
        }

        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            await PlayMediaLocalAsync(items, QueueOption.Add);
            return;
        }

        if (string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            _logger.LogWarning("PlayMediaLast ignored because there is no active player id.");
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            _logger.LogWarning("No playable tracks resolved for PlayMediaLast");
            return;
        }

        PlaybackState = new PlayerState { State = PlayerStateType.Buffering, ActiveSinceUtc = DateTimeOffset.UtcNow };
        _logger.LogDebug("PlayMediaLast request set PlaybackState to Buffering. OutputMode={OutputMode}, ActivePlayerId={ActivePlayerId}, ItemCount={ItemCount}", OutputMode, ActivePlayerId, items.Count);

        try
        {
            await _musicAssistant.PlayMediaAsync(ActivePlayerId!, resolvedItems, QueueOption.Add);
        }
        catch (Exception ex)
        {
            PlaybackState = new PlayerState { State = PlayerStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
            _logger.LogError(ex, "PlayMediaLast request failed for ActivePlayerId={ActivePlayerId}", ActivePlayerId);
            throw;
        }
    }

    public async Task ShufflePlayMediaAsync(IReadOnlyList<MediaItem> items)
    {
        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            await PlayMediaLocalAsync(items ?? Array.Empty<MediaItem>(), QueueOption.Replace, shuffle: true);
            return;
        }

        if (string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            _logger.LogWarning("ShufflePlayMedia ignored because there is no active player id.");
            return;
        }

        var mediaItems = await ResolvePlayableMediaItemsAsync(items ?? Array.Empty<MediaItem>());
        if (mediaItems.Count == 0)
        {
            _logger.LogWarning("No items available to shuffle play");
            return;
        }

        for (var i = mediaItems.Count - 1; i > 0; i--)
        {
            var j = Random.Shared.Next(i + 1);
            (mediaItems[i], mediaItems[j]) = (mediaItems[j], mediaItems[i]);
        }

        PlaybackState = new PlayerState { State = PlayerStateType.Buffering, ActiveSinceUtc = DateTimeOffset.UtcNow };
        _logger.LogDebug("ShufflePlayMedia request set PlaybackState to Buffering. OutputMode={OutputMode}, ActivePlayerId={ActivePlayerId}, ItemCount={ItemCount}", OutputMode, ActivePlayerId, mediaItems.Count);

        try
        {
            await _musicAssistant.PlayMediaAsync(ActivePlayerId!, mediaItems, QueueOption.Replace);
        }
        catch (Exception ex)
        {
            PlaybackState = new PlayerState { State = PlayerStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
            _logger.LogError(ex, "ShufflePlayMedia request failed for ActivePlayerId={ActivePlayerId}", ActivePlayerId);
            throw;
        }
    }

    public async Task ClearQueueAsync(bool skipStop = false)
    {
        if (OutputMode == PlaybackOutputMode.LocalOffline || string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            ResetQueueState();
            return;
        }

        await _musicAssistant.ClearQueueAsync(ActivePlayerId!, skipStop);
    }

    
    public async Task PlayQueueIndexAsync(int index, CancellationToken cancellationToken = default)
    {
        if (index < 0)
        {
            return;
        }

        if (OutputMode == PlaybackOutputMode.LocalSendspin)
        {
            if (index >= _currentQueueItems.Count)
            {
                _logger.LogWarning("PlayQueueIndex ignored because index is out of range. Index={Index}, QueueCount={QueueCount}", index, _currentQueueItems.Count);
                return;
            }

            await StartQueueItemByModeAsync(_currentQueueItems[index], index, cancellationToken);
            return;
        }

        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            if (index >= _currentQueueItems.Count)
            {
                _logger.LogWarning("PlayQueueIndex ignored for local queue because index is out of range. Index={Index}, QueueCount={QueueCount}", index, _currentQueueItems.Count);
                return;
            }

            var queueItem = _currentQueueItems[index];
            await StartLocalQueueItemAsync(queueItem, index);
            return;
        }

        var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(ActivePlayerId!);
        if (string.IsNullOrWhiteSpace(queue?.QueueId))
        {
            return;
        }

        await _musicAssistant.PlayIndexAsync(queue.QueueId, index);
    }

    public async Task MoveQueueItemAsync(string queueItemId, int posShift, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(queueItemId))
        {
            return;
        }

        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            if (_currentQueueItems.Count == 0 || posShift == 0)
            {
                return;
            }

            var queueItems = _currentQueueItems.ToList();
            var sourceIndex = queueItems.FindIndex(item => string.Equals(item.QueueItemId, queueItemId, StringComparison.Ordinal));
            if (sourceIndex < 0)
            {
                _logger.LogWarning("MoveQueueItem ignored for local queue because queue item was not found. QueueItemId={QueueItemId}", queueItemId);
                return;
            }

            var targetIndex = Math.Clamp(sourceIndex + posShift, 0, queueItems.Count - 1);
            if (targetIndex == sourceIndex)
            {
                return;
            }

            var movedItem = queueItems[sourceIndex];
            queueItems.RemoveAt(sourceIndex);
            queueItems.Insert(targetIndex, movedItem);

            for (var i = 0; i < queueItems.Count; i++)
            {
                queueItems[i].Index = i;
                queueItems[i].SortIndex = i;
            }

            var currentItemId = Normalize(_currentQueueItem?.QueueItemId);
            QueueItem? currentItem = null;
            int? currentIndex = null;
            if (!string.IsNullOrWhiteSpace(currentItemId))
            {
                var indexById = queueItems.FindIndex(item => string.Equals(item.QueueItemId, currentItemId, StringComparison.Ordinal));
                if (indexById >= 0)
                {
                    currentIndex = indexById;
                    currentItem = queueItems[indexById];
                }
            }

            _currentQueueItems.ReplaceRange(queueItems);

            CurrentQueueIndex = currentIndex;
            SetProperty(ref _currentQueueItem, currentItem, nameof(CurrentQueueItem));

            if (_activePlayer is ILocalPlayerService localPlayer)
            {
                localPlayer.UpdateCurrentQueueItem(currentItem);
            }

            return;
        }

        if (string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            return;
        }

        var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(ActivePlayerId);
        if (string.IsNullOrWhiteSpace(queue?.QueueId))
        {
            return;
        }

        await _musicAssistant.MoveQueueItemAsync(queue.QueueId, queueItemId, posShift);
    }

    public async Task DeleteQueueItemAsync(string queueItemId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(queueItemId))
        {
            return;
        }

        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            if (_currentQueueItems.Count == 0)
            {
                return;
            }

            var queueItems = _currentQueueItems.ToList();
            var removeIndex = queueItems.FindIndex(item => string.Equals(item.QueueItemId, queueItemId, StringComparison.Ordinal));
            if (removeIndex < 0)
            {
                _logger.LogWarning("DeleteQueueItem ignored for local queue because queue item was not found. QueueItemId={QueueItemId}", queueItemId);
                return;
            }

            queueItems.RemoveAt(removeIndex);

            for (var i = 0; i < queueItems.Count; i++)
            {
                queueItems[i].Index = i;
                queueItems[i].SortIndex = i;
            }

            var currentItemId = Normalize(_currentQueueItem?.QueueItemId);
            QueueItem? currentItem = null;
            int? currentIndex = null;
            if (!string.IsNullOrWhiteSpace(currentItemId))
            {
                var indexById = queueItems.FindIndex(item => string.Equals(item.QueueItemId, currentItemId, StringComparison.Ordinal));
                if (indexById >= 0)
                {
                    currentIndex = indexById;
                    currentItem = queueItems[indexById];
                }
            }

            _currentQueueItems.ReplaceRange(queueItems);
            QueueItemCount = queueItems.Count;
            CurrentQueueIndex = currentIndex;
            SetProperty(ref _currentQueueItem, currentItem, nameof(CurrentQueueItem));

            if (_activePlayer is ILocalPlayerService localPlayer)
            {
                localPlayer.UpdateCurrentQueueItem(currentItem);
            }

            return;
        }

        if (string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            return;
        }

        var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(ActivePlayerId!);
        if (string.IsNullOrWhiteSpace(queue?.QueueId))
        {
            return;
        }

        await _musicAssistant.DeleteQueueItemAsync(queue.QueueId, queueItemId);
    }

    #endregion

    #region Transport Commands
    // Runtime playback controls (play/pause/seek/next/previous).

    public async Task TogglePlayPauseAsync(CancellationToken cancellationToken = default)
    {
        if (OutputMode == PlaybackOutputMode.LocalSendspin && _isHybridLocalPlaybackActive && _localPlayer != null)
        {
            await _localPlayer.TogglePlayPauseAsync(cancellationToken);
            return;
        }

        await _activePlayer.TogglePlayPauseAsync(cancellationToken);
    }

    public async Task NextTrackAsync(CancellationToken cancellationToken = default)
    {
        if (OutputMode == PlaybackOutputMode.LocalSendspin && _currentQueueItems.Count > 0)
        {
            var nextIndex = _currentQueueIndex.HasValue
                ? _currentQueueIndex.Value + 1
                : 0;

            if (nextIndex < 0 || nextIndex >= _currentQueueItems.Count)
            {
                _logger.LogDebug("NextTrack ignored because end was reached. CurrentIndex={CurrentIndex}, QueueCount={QueueCount}", _currentQueueIndex, _currentQueueItems.Count);
                return;
            }

            await StartQueueItemByModeAsync(_currentQueueItems[nextIndex], nextIndex, cancellationToken);
            return;
        }

        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            if (_currentQueueItems.Count == 0)
            {
                return;
            }

            var nextIndex = _currentQueueIndex.HasValue
                ? _currentQueueIndex.Value + 1
                : 0;

            if (nextIndex < 0 || nextIndex >= _currentQueueItems.Count)
            {
                _logger.LogDebug("NextTrack ignored for local queue because end was reached. CurrentIndex={CurrentIndex}, QueueCount={QueueCount}", _currentQueueIndex, _currentQueueItems.Count);
                return;
            }

            await StartLocalQueueItemAsync(_currentQueueItems[nextIndex], nextIndex);
            return;
        }

        if (_activePlayer is IRemotePlayerService remotePlayer)
        {
            await remotePlayer.NextAsync(cancellationToken);
            return;
        }

        _logger.LogWarning("NextTrack requested in non-local mode but active player does not implement IRemotePlayerService. PlayerType={PlayerType}", _activePlayer.GetType().Name);
    }

    public async Task PreviousTrackAsync(CancellationToken cancellationToken = default)
    {
        if (OutputMode == PlaybackOutputMode.LocalSendspin && _currentQueueItems.Count > 0)
        {
            var previousIndex = _currentQueueIndex.HasValue
                ? _currentQueueIndex.Value - 1
                : 0;

            if (previousIndex < 0 || previousIndex >= _currentQueueItems.Count)
            {
                _logger.LogDebug("PreviousTrack ignored because start was reached. CurrentIndex={CurrentIndex}, QueueCount={QueueCount}", _currentQueueIndex, _currentQueueItems.Count);
                return;
            }

            await StartQueueItemByModeAsync(_currentQueueItems[previousIndex], previousIndex, cancellationToken);
            return;
        }

        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            if (_currentQueueItems.Count == 0)
            {
                return;
            }

            var previousIndex = _currentQueueIndex.HasValue
                ? _currentQueueIndex.Value - 1
                : 0;

            if (previousIndex < 0 || previousIndex >= _currentQueueItems.Count)
            {
                _logger.LogDebug("PreviousTrack ignored for local queue because start was reached. CurrentIndex={CurrentIndex}, QueueCount={QueueCount}", _currentQueueIndex, _currentQueueItems.Count);
                return;
            }

            await StartLocalQueueItemAsync(_currentQueueItems[previousIndex], previousIndex);
            return;
        }

        if (_activePlayer is IRemotePlayerService remotePlayer)
        {
            await remotePlayer.PreviousAsync(cancellationToken);
            return;
        }

        _logger.LogWarning("PreviousTrack requested in non-local mode but active player does not implement IRemotePlayerService. PlayerType={PlayerType}", _activePlayer.GetType().Name);
    }

    public async Task SeekAsync(double seconds, double durationSeconds, CancellationToken cancellationToken = default)
    {
        var clamped = (int)Math.Round(Math.Clamp(seconds, 0, Math.Max(0, durationSeconds)));

        if (OutputMode == PlaybackOutputMode.LocalSendspin && _isHybridLocalPlaybackActive && _localPlayer != null)
        {
            await _localPlayer.SeekAsync(clamped, cancellationToken);
            return;
        }

        await _activePlayer.SeekAsync(clamped, cancellationToken);
    }

    public async Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default)
    {
        var clamped = Math.Clamp(volume, 0, 100);

        if (OutputMode == PlaybackOutputMode.LocalSendspin && _isHybridLocalPlaybackActive && _localPlayer != null)
        {
            await _localPlayer.SetVolumeAsync(clamped, cancellationToken);
            return;
        }

        await _activePlayer.SetVolumeAsync(clamped, cancellationToken);
    }

    public async Task ToggleMuteAsync(bool currentMuted, CancellationToken cancellationToken = default)
    {
        var next = !currentMuted;

        if (OutputMode == PlaybackOutputMode.LocalSendspin && _isHybridLocalPlaybackActive && _localPlayer != null)
        {
            await _localPlayer.SetMutedAsync(next, cancellationToken);
            return;
        }

        await _activePlayer.SetMutedAsync(next, cancellationToken);
    }

    public async Task SetPreferredAudioCodecAsync(string codec, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(codec))
        {
            return;
        }

        if (OutputMode != PlaybackOutputMode.LocalSendspin)
        {
            return;
        }

        if (_activePlayer is SendspinPlayerService sendspinPlayer)
        {
            await sendspinPlayer.UpdatePreferredAudioCodecAsync(codec, cancellationToken);
        }
    }

    public async Task ToggleShuffleAsync(bool? currentShuffleEnabled, CancellationToken cancellationToken = default)
    {
        var next = !(currentShuffleEnabled ?? false);
        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            ShuffleEnabled = next;
            return;
        }

        if (_activePlayer is IRemotePlayerService remotePlayer)
        {
            await remotePlayer.SetShuffleAsync(next, cancellationToken);
            return;
        }

        _logger.LogWarning("ToggleShuffle requested in non-local mode but active player does not implement IRemotePlayerService. PlayerType={PlayerType}", _activePlayer.GetType().Name);
    }

    public async Task ToggleRepeatModeAsync(string? currentRepeatMode, CancellationToken cancellationToken = default)
    {
        var next = ToRepeatMode(currentRepeatMode) switch
        {
            mashin.Models.RepeatMode.Off => mashin.Models.RepeatMode.All,
            mashin.Models.RepeatMode.All => mashin.Models.RepeatMode.One,
            _ => mashin.Models.RepeatMode.Off
        };

        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            RepeatMode = next.ToString();
            return;
        }

        if (_activePlayer is IRemotePlayerService remotePlayer)
        {
            await remotePlayer.SetRepeatModeAsync(next, cancellationToken);
            return;
        }

        _logger.LogWarning("ToggleRepeatMode requested in non-local mode but active player does not implement IRemotePlayerService. PlayerType={PlayerType}", _activePlayer.GetType().Name);
    }

    public async Task SetDontStopTheMusicAsync(bool enabled, CancellationToken cancellationToken = default)
    {
        if (OutputMode == PlaybackOutputMode.LocalOffline || string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            _logger.LogWarning("SetDontStopTheMusic ignored in LocalOffline mode.");
            return;
        }

        var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(ActivePlayerId);
        if (string.IsNullOrWhiteSpace(queue?.QueueId))
        {
            return;
        }

        await _musicAssistant.SetDontStopTheMusicAsync(queue.QueueId, enabled);
    }

    #endregion

    #region Disposal
    // Shutdown sequence and subscription cleanup.

    public async ValueTask DisposeAsync()
    {
        await _musicAssistantEventHub.StopAsync(CancellationToken.None);
        _musicAssistantEventHub.QueueEventReceived -= OnQueueEventReceived;
        _activePlayer.PropertyChanged -= OnActivePlayerPropertyChanged;
        if (_localPlayer != null)
        {
            _localPlayer.CurrentPlayingItemEnded -= OnLocalPlayerCurrentPlayingItemEnded;
            _localPlayer.PropertyChanged -= OnLocalPlayerPropertyChanged;
        }

        var queueConfirmCts = Interlocked.Exchange(ref _queueConfirmCts, null);
        queueConfirmCts?.Cancel();
        queueConfirmCts?.Dispose();

        _disposeCts.Cancel();

        try
        {
            await _hybridMonitorTask;
        }
        catch (OperationCanceledException)
        {
            // Expected during shutdown.
        }

        _disposeCts.Dispose();

        foreach (var player in _players.Values.Distinct())
        {
            await player.DisposeAsync();
        }
    }

    #endregion

    #region Event Handling

    // MA Queue Events for queue state updates
    private void OnQueueEventReceived(object? sender, MusicAssistantQueueEvent e)
    {
        if (OutputMode == PlaybackOutputMode.LocalOffline || string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            return;
        }

        var eventQueueId = Normalize(e.Queue?.QueueId) ?? Normalize(e.QueueId);

        // Only process events for the active queue
        if (!string.IsNullOrWhiteSpace(_activeQueueId)
            && !string.IsNullOrWhiteSpace(eventQueueId)
            && !string.Equals(eventQueueId, _activeQueueId, StringComparison.Ordinal))
        {
            return;
        }

        // Learn active queue id as soon as possible
        if (string.IsNullOrWhiteSpace(_activeQueueId) && !string.IsNullOrWhiteSpace(eventQueueId))
        {
            _activeQueueId = eventQueueId;
        }

        if (string.Equals(e.Event, "queue_time_updated", StringComparison.OrdinalIgnoreCase))
        {
            return;
        }

        var queue = e.Queue;
        if (queue == null)
        {
            return;
        }
   
        // Apply event state updates
        _activeQueueId = Normalize(queue.QueueId) ?? _activeQueueId;

        QueueItemCount = queue.ItemCount;
        ShuffleEnabled = queue.ShuffleEnabled;
        RepeatMode = queue.RepeatMode?.ToString();
        DontStopTheMusicEnabled = queue.DontStopTheMusicEnabled;

        SetProperty(ref _currentQueueItem, queue.CurrentItem, nameof(CurrentQueueItem));

        CurrentQueueIndex = queue.CurrentIndex;

        _logger.LogDebug(
            "Queue event applied. Event={Event}, QueueId={QueueId}, ItemCount={ItemCount}, CurrentIndex={CurrentIndex}, CurrentItemId={CurrentItemId}, ShuffleEnabled={ShuffleEnabled}, RepeatMode={RepeatMode}, DontStopTheMusicEnabled={DontStopTheMusicEnabled}",
            e.Event,
            _activeQueueId,
            queue.ItemCount,
            queue.CurrentIndex,
            queue.CurrentItem?.QueueItemId,
            queue.ShuffleEnabled,
            queue.RepeatMode,
            queue.DontStopTheMusicEnabled);

        if (OutputMode == PlaybackOutputMode.LocalSendspin)
        {
            _ = Task.Run(async () =>
            {
                try
                {
                    await AdoptHybridCurrentItemAsync(_disposeCts.Token);
                }
                catch (OperationCanceledException)
                {
                    // Ignore cancellation.
                }
                catch (Exception ex)
                {
                    _logger.LogDebug(ex, "Hybrid adoption by policy failed after queue event.");
                }
            }, CancellationToken.None);
        }
        

        if (!ShouldRefreshQueue(e))
        {
            return;
        }

        // Coalesce bursts so only the newest event triggers an HTTP confirmation snapshot.
        var requestId = Interlocked.Increment(ref _queueConfirmRequestId);
        var nextCts = CancellationTokenSource.CreateLinkedTokenSource(_disposeCts.Token);
        var previousCts = Interlocked.Exchange(ref _queueConfirmCts, nextCts);

        previousCts?.Cancel();
        previousCts?.Dispose();

        _ = Task.Run(async () =>
        {
            try
            {
                // Debounce event bursts, then confirm queue state via HTTP.
                await Task.Delay(TimeSpan.FromMilliseconds(250), nextCts.Token);

                if (nextCts.Token.IsCancellationRequested)
                {
                    return;
                }

                if (requestId != Volatile.Read(ref _queueConfirmRequestId))
                {
                    return;
                }

                _logger.LogInformation("Queue HTTP refresh triggered. Event={Event}, QueueId={QueueId}, RequestId={RequestId}", e.Event, _activeQueueId, requestId);
                await RefreshQueueAsync(nextCts.Token);
            }
            catch (OperationCanceledException)
            {
                // Expected during rapid updates and shutdown.
            }
        }, CancellationToken.None);
    }

    // Active player property change events for playback state, position, duration, volume, and mute state.
    private void OnActivePlayerPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (OutputMode == PlaybackOutputMode.LocalSendspin && _isHybridLocalPlaybackActive)
        {
            // While local track is active in hybrid mode, authoritative state comes from local player.
            if (e.PropertyName == nameof(IPlayerService.PlaybackState)
                || e.PropertyName == nameof(IPlayerService.PositionSeconds)
                || e.PropertyName == nameof(IPlayerService.DurationSeconds))
            {
                return;
            }
        }

        if (string.IsNullOrWhiteSpace(e.PropertyName))
        {
            SyncStateFromPlayer();
            return;
        }

        if (e.PropertyName == nameof(IPlayerService.Volume))
        {
            Volume = _activePlayer.Volume;
            return;
        }

        if (e.PropertyName == nameof(IPlayerService.IsMuted))
        {
            IsMuted = _activePlayer.IsMuted;
            return;
        }

        if (e.PropertyName == nameof(IPlayerService.DurationSeconds))
        {
            DurationSeconds = _activePlayer.DurationSeconds;
            return;
        }

        if (e.PropertyName == nameof(IPlayerService.PositionSeconds))
        {
            PositionSeconds = _activePlayer.PositionSeconds;
            return;
        }

        if (e.PropertyName == nameof(IPlayerService.PlaybackState))
        {
            PlaybackState = _activePlayer.PlaybackState;
        }
    }

    // Local player property change events for playback state, position, duration, volume, and mute state (needed for hybrid local playback)
    private void OnLocalPlayerPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (OutputMode != PlaybackOutputMode.LocalSendspin || !_isHybridLocalPlaybackActive || _localPlayer == null)
        {
            return;
        }

        if (string.IsNullOrWhiteSpace(e.PropertyName))
        {
            Volume = _localPlayer.Volume;
            IsMuted = _localPlayer.IsMuted;
            DurationSeconds = _localPlayer.DurationSeconds;
            PositionSeconds = _localPlayer.PositionSeconds;
            PlaybackState = _localPlayer.PlaybackState;
            return;
        }

        if (e.PropertyName == nameof(IPlayerService.Volume))
        {
            Volume = _localPlayer.Volume;
            return;
        }

        if (e.PropertyName == nameof(IPlayerService.IsMuted))
        {
            IsMuted = _localPlayer.IsMuted;
            return;
        }

        if (e.PropertyName == nameof(IPlayerService.DurationSeconds))
        {
            DurationSeconds = _localPlayer.DurationSeconds;
            return;
        }

        if (e.PropertyName == nameof(IPlayerService.PositionSeconds))
        {
            PositionSeconds = _localPlayer.PositionSeconds;
            return;
        }

        if (e.PropertyName == nameof(IPlayerService.PlaybackState))
        {
            PlaybackState = _localPlayer.PlaybackState;
        }
    }

    // Local player event when the current playing item ends for to set next item in hybrid local playback mode or local offline mode
    private void OnLocalPlayerCurrentPlayingItemEnded(object? sender, QueueItem? endedItem)
    {
        if (_disposeCts.IsCancellationRequested)
        {
            return;
        }

        if (OutputMode == PlaybackOutputMode.LocalSendspin)
        {
            if (!_isHybridLocalPlaybackActive)
            {
                return;
            }

            if (_currentQueueIndex is not int hybridCurrentIndex)
            {
                return;
            }

            var hybridNextIndex = hybridCurrentIndex + 1;
            if (hybridNextIndex < 0 || hybridNextIndex >= _currentQueueItems.Count)
            {
                _logger.LogDebug("Hybrid local queue reached end. CurrentIndex={CurrentIndex}, QueueCount={QueueCount}", hybridCurrentIndex, _currentQueueItems.Count);
                _isHybridLocalPlaybackActive = false;
                _hybridActiveQueueItemId = null;
                return;
            }

            var hybridNextItem = _currentQueueItems[hybridNextIndex];
            _ = Task.Run(async () =>
            {
                try
                {
                    await StartQueueItemByModeAsync(hybridNextItem, hybridNextIndex, _disposeCts.Token);
                }
                catch (OperationCanceledException)
                {
                    // Ignore cancellation.
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "Failed to advance hybrid queue after local track end. NextIndex={NextIndex}, QueueItemId={QueueItemId}", hybridNextIndex, hybridNextItem.QueueItemId);
                }
            }, CancellationToken.None);

            return;
        }

        if (OutputMode != PlaybackOutputMode.LocalOffline)
        {
            return;
        }

        if (_activePlayer is not ILocalPlayerService)
        {
            return;
        }

        if (_currentQueueIndex is not int currentIndex)
        {
            return;
        }

        var nextIndex = currentIndex + 1;
        if (nextIndex < 0 || nextIndex >= _currentQueueItems.Count)
        {
            _logger.LogDebug("Local queue reached end. CurrentIndex={CurrentIndex}, QueueCount={QueueCount}", currentIndex, _currentQueueItems.Count);
            return;
        }

        var nextItem = _currentQueueItems[nextIndex];

        _ = Task.Run(async () =>
        {
            try
            {
                await StartLocalQueueItemAsync(nextItem, nextIndex);
            }
            catch (Exception ex)
            {
                _logger.LogError(ex, "Failed to advance local queue after track end. NextIndex={NextIndex}, QueueItemId={QueueItemId}", nextIndex, nextItem.QueueItemId);
            }
        }, CancellationToken.None);
    }

    #endregion

    #region Queue Sync (Music Assistant)

    private async Task RefreshQueueAsync(CancellationToken cancellationToken = default)
    {
        if (OutputMode == PlaybackOutputMode.LocalOffline || string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            return;
        }

        try
        {
            var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(ActivePlayerId!);
            if (queue == null)
            {
                _logger.LogDebug("No active queue found for ActivePlayerId={ActivePlayerId}. Resetting queue state.", ActivePlayerId);
                ResetQueueState();
                return;
            }

            _activeQueueId = Normalize(queue.QueueId);

            // Reconcile event-based state with an authoritative snapshot from MA.
            _logger.LogInformation("Executing queue items refresh via HTTP. QueueId={QueueId}, ActivePlayerId={ActivePlayerId}", queue.QueueId, ActivePlayerId);
            var queueItems = await _musicAssistant.GetQueueItemsAsync(queue.QueueId);
            _currentQueueItems.ReplaceRange(queueItems);

            QueueItemCount = queue.ItemCount;
            ShuffleEnabled = queue.ShuffleEnabled;
            RepeatMode = queue.RepeatMode?.ToString();
            DontStopTheMusicEnabled = queue.DontStopTheMusicEnabled;

            SetProperty(ref _currentQueueItem, queue.CurrentItem, nameof(CurrentQueueItem));

            CurrentQueueIndex = queue.CurrentIndex;

            _logger.LogDebug(
                "Queue HTTP refresh applied. QueueId={QueueId}, ItemCount={ItemCount}, CurrentIndex={CurrentIndex}, CurrentItemId={CurrentItemId}, ShuffleEnabled={ShuffleEnabled}, RepeatMode={RepeatMode}, DontStopTheMusicEnabled={DontStopTheMusicEnabled}",
                queue.QueueId,
                queue.ItemCount,
                queue.CurrentIndex,
                queue.CurrentItem?.QueueItemId,
                queue.ShuffleEnabled,
                queue.RepeatMode,
                queue.DontStopTheMusicEnabled);

            if (OutputMode == PlaybackOutputMode.LocalSendspin)
            {
                await AdoptHybridCurrentItemAsync(cancellationToken);
            }

        }
        catch (OperationCanceledException)
        {
            // Expected on cancellation.
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to refresh queue snapshot for player {PlayerId}", ActivePlayerId);
        }
    }

    private void ResetQueueState()
    {
        _logger.LogDebug("Resetting queue state.");

        _activeQueueId = null;
        _currentQueueItems.ReplaceRange(Array.Empty<QueueItem>());
        SetProperty(ref _currentQueueItem, null, nameof(CurrentQueueItem));
        PositionSeconds = 0;
        DurationSeconds = 0;

        CurrentQueueIndex = null;
        QueueItemCount = 0;
        ShuffleEnabled = null;
        RepeatMode = null;
        DontStopTheMusicEnabled = null;
    }

    #endregion

    #region Queue Playback (Local)

    private async Task PlayMediaLocalAsync(IReadOnlyList<MediaItem> items, QueueOption queueOption, bool shuffle = false)
    {
        if (OutputMode != PlaybackOutputMode.LocalOffline)
        {
            _logger.LogWarning("PlayMediaLocal ignored because OutputMode is not LocalOffline.");
            return;
        }

        if (_activePlayer is not ILocalPlayerService localPlayer)
        {
            _logger.LogWarning("PlayMediaLocal ignored because active player is not local.");
            return;
        }

        var resolvedTracks = ResolvePlayableLocalMediaItems(items ?? Array.Empty<MediaItem>());
        if (resolvedTracks.Count == 0)
        {
            _logger.LogWarning("No playable tracks resolved for local queue update");
            return;
        }

        if (shuffle)
        {
            for (var i = resolvedTracks.Count - 1; i > 0; i--)
            {
                var j = Random.Shared.Next(i + 1);
                (resolvedTracks[i], resolvedTracks[j]) = (resolvedTracks[j], resolvedTracks[i]);
            }
        }

        var incomingItems = CreateLocalQueueItems(resolvedTracks);
        if (incomingItems.Count == 0)
        {
            _logger.LogWarning("Local queue update ignored because no queue items could be created.");
            return;
        }

        var mergedItems = _currentQueueItems.ToList();
        var previousCurrentItemId = Normalize(_currentQueueItem?.QueueItemId);
        var hadCurrentItem = !string.IsNullOrWhiteSpace(previousCurrentItemId);
        var insertionIndex = 0;

        switch (queueOption)
        {
            case QueueOption.Replace:
                mergedItems = incomingItems;
                break;

            case QueueOption.Next:
                insertionIndex = _currentQueueIndex.HasValue
                    ? Math.Clamp(_currentQueueIndex.Value + 1, 0, mergedItems.Count)
                    : 0;
                mergedItems.InsertRange(insertionIndex, incomingItems);
                break;

            default:
                insertionIndex = mergedItems.Count;
                mergedItems.AddRange(incomingItems);
                break;
        }

        var resolvedCurrentIndex = ResolveLocalCurrentIndex(
            mergedItems,
            previousCurrentItemId,
            _currentQueueIndex,
            queueOption,
            insertionIndex);

        ApplyLocalQueueState(localPlayer, mergedItems, resolvedCurrentIndex);

        var shouldStartPlayback = queueOption == QueueOption.Replace || !hadCurrentItem;
        if (!shouldStartPlayback || resolvedCurrentIndex is not int startIndex || startIndex < 0 || startIndex >= mergedItems.Count)
        {
            return;
        }

        await StartLocalQueueItemAsync(mergedItems[startIndex], startIndex);
    }

    private static List<Track> ResolvePlayableLocalMediaItems(IReadOnlyList<MediaItem> items)
    {
        var resolvedTracks = new List<Track>();

        // Offline placeholder: only directly supplied Track items are considered playable.
        // Playlist/Album/Artist expansion will be implemented later via local DB integration.
        foreach (var mediaItem in items)
        {
            if (mediaItem is Track track)
            {
                resolvedTracks.Add(track);
            }
        }

        return resolvedTracks;
    }

    private async Task StartLocalQueueItemAsync(QueueItem queueItem, int queueIndex)
    {
        if (_activePlayer is not ILocalPlayerService localPlayer)
        {
            _logger.LogWarning("StartLocalQueueItem ignored because active player does not implement ILocalPlayerService.");
            return;
        }

        var sourcePath = queueItem.MediaItem?.LocalPath;

        if (string.IsNullOrWhiteSpace(sourcePath))
        {
            _logger.LogWarning("Cannot start local queue item because no local source path is available. QueueItemId={QueueItemId}", queueItem.QueueItemId);
            return;
        }

        await localPlayer.SetSourceAsync(sourcePath);
        localPlayer.UpdateCurrentQueueItem(queueItem);
        SetProperty(ref _currentQueueItem, queueItem, nameof(CurrentQueueItem));
        CurrentQueueIndex = queueIndex;
        await localPlayer.TogglePlayPauseAsync();
    }

    private void ApplyLocalQueueState(ILocalPlayerService localPlayer, List<QueueItem> queueItems, int? currentIndex)
    {
        for (var i = 0; i < queueItems.Count; i++)
        {
            queueItems[i].Index = i;
        }

        _currentQueueItems.ReplaceRange(queueItems);
        QueueItemCount = queueItems.Count;

        QueueItem? currentItem = null;
        if (currentIndex is int index && index >= 0 && index < queueItems.Count)
        {
            currentItem = queueItems[index];
        }

        CurrentQueueIndex = currentItem == null ? null : currentIndex;
        SetProperty(ref _currentQueueItem, currentItem, nameof(CurrentQueueItem));
        localPlayer.UpdateCurrentQueueItem(currentItem);
    }

    private static int? ResolveLocalCurrentIndex(
        List<QueueItem> queueItems,
        string? previousCurrentItemId,
        int? previousCurrentIndex,
        QueueOption queueOption,
        int insertionIndex)
    {
        if (queueItems.Count == 0)
        {
            return null;
        }

        if (queueOption == QueueOption.Replace)
        {
            return 0;
        }

        if (!string.IsNullOrWhiteSpace(previousCurrentItemId))
        {
            var indexById = queueItems.FindIndex(item => string.Equals(item.QueueItemId, previousCurrentItemId, StringComparison.Ordinal));
            if (indexById >= 0)
            {
                return indexById;
            }
        }

        if (previousCurrentIndex is int index && index >= 0 && index < queueItems.Count)
        {
            return index;
        }

        return Math.Clamp(insertionIndex, 0, queueItems.Count - 1);
    }

    private static List<QueueItem> CreateLocalQueueItems(IReadOnlyList<Track> tracks)
    {
        const string localQueueId = "local-offline";
        var queueItems = new List<QueueItem>(tracks.Count);

        for (var i = 0; i < tracks.Count; i++)
        {
            var track = tracks[i];
            queueItems.Add(new QueueItem
            {
                QueueId = localQueueId,
                QueueItemId = Guid.NewGuid().ToString("N"),
                Name = track.Name,
                Duration = track.Duration,
                SortIndex = i,
                MediaItem = track,
                Image = track.PrimaryImage,
                Index = i,
                Available = true
            });
        }

        return queueItems;
    }

    #endregion

    #region Queue Playback (Hybrid Sendspin)

    private async Task HybridMonitorLoopAsync()
    {
        while (!_disposeCts.IsCancellationRequested)
        {
            try
            {
                await Task.Delay(TimeSpan.FromMilliseconds(500), _disposeCts.Token);
            }
            catch (OperationCanceledException)
            {
                break;
            }

            if (OutputMode != PlaybackOutputMode.LocalSendspin || !_isHybridLocalPlaybackActive)
            {
                continue;
            }

            if (_prewarmTriggered)
            {
                continue;
            }

            if (_currentQueueIndex is not int currentIndex)
            {
                continue;
            }

            var nextIndex = currentIndex + 1;
            if (nextIndex < 0 || nextIndex >= _currentQueueItems.Count)
            {
                continue;
            }

            var nextItem = _currentQueueItems[nextIndex];
            if (IsLocalPlayable(nextItem))
            {
                continue;
            }

            if (!IsSendspinAvailable)
            {
                continue;
            }

            var remaining = Math.Max(0, DurationSeconds - PositionSeconds);
            if (remaining > PrewarmWindowSeconds)
            {
                continue;
            }

            try
            {
                await PrewarmHybridSendspinAsync(_disposeCts.Token);
            }
            catch (Exception ex)
            {
                _logger.LogDebug(ex, "Hybrid Sendspin prewarm failed.");
            }
        }
    }

    private async Task AdoptHybridCurrentItemAsync(CancellationToken cancellationToken)
    {
        if (OutputMode != PlaybackOutputMode.LocalSendspin)
        {
            return;
        }

        if (_currentQueueIndex is not int currentIndex)
        {
            return;
        }

        if (_currentQueueItem == null)
        {
            return;
        }

        if (!IsLocalPlayable(_currentQueueItem))
        {
            return;
        }

        await StartHybridLocalItemAsync(_currentQueueItem, currentIndex, PositionSeconds, cancellationToken);
    }

    private async Task StartQueueItemByModeAsync(QueueItem queueItem, int queueIndex, CancellationToken cancellationToken)
    {
        if (OutputMode != PlaybackOutputMode.LocalSendspin)
        {
            await StartLocalQueueItemAsync(queueItem, queueIndex);
            return;
        }

        if (IsLocalPlayable(queueItem))
        {
            await StartHybridLocalItemAsync(queueItem, queueIndex, 0, cancellationToken);
            return;
        }

        if (IsSendspinAvailable)
        {
            await StartHybridSendspinItemAsync(queueIndex, 0, cancellationToken);
            return;
        }

        _isHybridLocalPlaybackActive = false;
        _hybridActiveQueueItemId = null;
        _prewarmTriggered = false;
        _prewarmedQueueIndex = null;
        SetProperty(ref _currentQueueItem, queueItem, nameof(CurrentQueueItem));
        CurrentQueueIndex = queueIndex;
        PlaybackState = new PlayerState
        {
            State = PlayerStateType.Buffering,
            ActiveSinceUtc = DateTimeOffset.UtcNow
        };

        _logger.LogInformation("Hybrid policy stalled at non-local item due to missing remote connectivity. QueueItemId={QueueItemId}, Index={Index}", queueItem.QueueItemId, queueIndex);
    }

    private async Task StartHybridLocalItemAsync(QueueItem queueItem, int queueIndex, double startSeconds, CancellationToken cancellationToken)
    {
        if (_localPlayer == null)
        {
            return;
        }

        var localPath = GetLocalPath(queueItem);
        if (string.IsNullOrWhiteSpace(localPath))
        {
            return;
        }

        var itemId = Normalize(queueItem.QueueItemId);
        if (_isHybridLocalPlaybackActive
            && string.Equals(itemId, _hybridActiveQueueItemId, StringComparison.Ordinal)
            && _localPlayer.PlaybackState.State == PlayerStateType.Playing)
        {
            return;
        }

        if (_sendspinPlayer != null)
        {
            await _sendspinPlayer.SetExternalSourceAsync(true, cancellationToken);
        }

        await _localPlayer.SetSourceAsync(localPath, startSeconds, cancellationToken);
        _localPlayer.UpdateCurrentQueueItem(queueItem);

        if (_localPlayer.PlaybackState.State != PlayerStateType.Playing)
        {
            await _localPlayer.TogglePlayPauseAsync(cancellationToken);
        }

        _isHybridLocalPlaybackActive = true;
        _prewarmTriggered = false;
        _hybridActiveQueueItemId = itemId;

        SetProperty(ref _currentQueueItem, queueItem, nameof(CurrentQueueItem));
        CurrentQueueIndex = queueIndex;

        Volume = _localPlayer.Volume;
        IsMuted = _localPlayer.IsMuted;
        DurationSeconds = _localPlayer.DurationSeconds;
        PositionSeconds = _localPlayer.PositionSeconds;
        PlaybackState = _localPlayer.PlaybackState;

        _logger.LogInformation("Hybrid local playback started. QueueItemId={QueueItemId}, Index={Index}, StartSeconds={StartSeconds:F2}", queueItem.QueueItemId, queueIndex, startSeconds);
    }

    private async Task StartHybridSendspinItemAsync(int queueIndex, int seekSeconds, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            return;
        }

        var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(ActivePlayerId!);
        if (string.IsNullOrWhiteSpace(queue?.QueueId))
        {
            return;
        }

        if (_sendspinPlayer != null)
        {
            await _sendspinPlayer.SetExternalSourceAsync(false, cancellationToken);
        }

        if (_prewarmTriggered && _prewarmedQueueIndex == queueIndex)
        {
            await _musicAssistant.PreviousAsync(queue.QueueId);
        }
        else
        {
            await _musicAssistant.PlayIndexAsync(queue.QueueId, queueIndex);
            if (seekSeconds > 0)
            {
                await _musicAssistant.SeekAsync(queue.QueueId, seekSeconds);
            }
        }

        _isHybridLocalPlaybackActive = false;
        _hybridActiveQueueItemId = null;
        _prewarmTriggered = false;
        _prewarmedQueueIndex = null;

        PlaybackState = new PlayerState
        {
            State = PlayerStateType.Buffering,
            ActiveSinceUtc = DateTimeOffset.UtcNow
        };

        _logger.LogInformation("Hybrid Sendspin playback started. QueueId={QueueId}, Index={Index}, SeekSeconds={SeekSeconds}", queue.QueueId, queueIndex, seekSeconds);
    }

    private async Task PrewarmHybridSendspinAsync(CancellationToken cancellationToken)
    {
        if (!_isHybridLocalPlaybackActive)
        {
            return;
        }

        if (!IsSendspinAvailable)
        {
            return;
        }

        if (_currentQueueIndex is not int currentIndex)
        {
            return;
        }

        var nextIndex = currentIndex + 1;
        if (nextIndex < 0 || nextIndex >= _currentQueueItems.Count)
        {
            return;
        }

        if (string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            return;
        }

        var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(ActivePlayerId!);
        if (string.IsNullOrWhiteSpace(queue?.QueueId))
        {
            return;
        }

        if (_sendspinPlayer != null)
        {
            await _sendspinPlayer.SetExternalSourceAsync(false, cancellationToken);
        }

        await _musicAssistant.PlayIndexAsync(queue.QueueId, nextIndex);
        await _musicAssistant.PauseAsync(queue.QueueId);

        _prewarmTriggered = true;
        _prewarmedQueueIndex = nextIndex;

        _logger.LogInformation("Hybrid Sendspin prewarm prepared. QueueId={QueueId}, PrewarmedIndex={PrewarmedIndex}", queue.QueueId, nextIndex);
    }

    private bool IsSendspinAvailable =>
        OutputMode == PlaybackOutputMode.LocalSendspin
        && !string.IsNullOrWhiteSpace(ActivePlayerId)
        && _sendspinPlayer?.IsConnected == true;

    private static string? GetLocalPath(QueueItem? queueItem)
    {
        var candidate = queueItem?.MediaItem?.LocalPath;
        if (string.IsNullOrWhiteSpace(candidate))
        {
            return null;
        }

        var value = candidate.Trim();
        if (Uri.TryCreate(value, UriKind.Absolute, out var uri) && uri.IsFile)
        {
            value = uri.LocalPath;
        }

        return value;
    }

    private static bool IsLocalPlayable(QueueItem? queueItem)
    {
        var localPath = GetLocalPath(queueItem);
        return !string.IsNullOrWhiteSpace(localPath) && File.Exists(localPath);
    }

    #endregion

    #region Helpers
    // Generic utility helpers used across orchestration code paths.

    private void SyncStateFromPlayer()
    {
        if (OutputMode == PlaybackOutputMode.LocalSendspin && _isHybridLocalPlaybackActive && _localPlayer != null)
        {
            Volume = _localPlayer.Volume;
            IsMuted = _localPlayer.IsMuted;
            DurationSeconds = _localPlayer.DurationSeconds;
            PositionSeconds = _localPlayer.PositionSeconds;
            PlaybackState = _localPlayer.PlaybackState;
            return;
        }

        Volume = _activePlayer.Volume;
        IsMuted = _activePlayer.IsMuted;
        DurationSeconds = _activePlayer.DurationSeconds;
        PositionSeconds = _activePlayer.PositionSeconds;
        PlaybackState = _activePlayer.PlaybackState;
    }

    private static string? Normalize(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return null;
        }

        return value.Trim();
    }

    private static mashin.Models.RepeatMode ToRepeatMode(string? value)
    {
        if (Enum.TryParse<mashin.Models.RepeatMode>(value, true, out var parsed))
        {
            return parsed;
        }

        return mashin.Models.RepeatMode.Off;
    }

    private bool ShouldRefreshQueue(MusicAssistantQueueEvent queueEvent)
    {
        if (string.Equals(queueEvent.Event, "queue_items_updated", StringComparison.OrdinalIgnoreCase)
            || string.Equals(queueEvent.Event, "queue_added", StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        var queue = queueEvent.Queue;
        if (queue == null)
        {
            return false;
        }

        if (_currentQueueItems.Count == 0)
        {
            return queue.ItemCount > 0;
        }

        if (_currentQueueItems.Count != queue.ItemCount)
        {
            return true;
        }

        var snapshotCurrentItemId = Normalize(queue.CurrentItem?.QueueItemId);
        if (string.IsNullOrWhiteSpace(snapshotCurrentItemId))
        {
            return false;
        }

        if (queue.CurrentIndex is int currentIndex
            && currentIndex >= 0
            && currentIndex < _currentQueueItems.Count)
        {
            var localItemIdAtIndex = Normalize(_currentQueueItems[currentIndex]?.QueueItemId);
            return !string.Equals(localItemIdAtIndex, snapshotCurrentItemId, StringComparison.Ordinal);
        }

        return false;
    }

    private async Task<List<MediaItem>> ResolvePlayableMediaItemsAsync(IReadOnlyList<MediaItem> items)
    {
        const int artistTopTracksLimit = 25;
        var resolvedTracks = new List<MediaItem>();

        foreach (var mediaItem in items)
        {
            if (mediaItem == null)
            {
                continue;
            }

            switch (mediaItem)
            {
                case Track track:
                    resolvedTracks.Add(track);
                    break;

                case Playlist playlist:
                {
                    var provider = ResolveProviderInstanceOrDomain(playlist);
                    if (string.IsNullOrWhiteSpace(provider) || string.IsNullOrWhiteSpace(playlist.ItemId))
                    {
                        _logger.LogWarning("Cannot resolve playlist tracks due to missing provider or item id. Playlist={PlaylistName}", playlist.Name);
                        break;
                    }

                    var playlistTracks = await _musicAssistant.GetPlaylistTracksAsync(playlist.ItemId, provider);
                    resolvedTracks.AddRange(playlistTracks);
                    break;
                }

                case Album album:
                {
                    var provider = ResolveProviderInstanceOrDomain(album);
                    if (string.IsNullOrWhiteSpace(provider) || string.IsNullOrWhiteSpace(album.ItemId))
                    {
                        _logger.LogWarning("Cannot resolve album tracks due to missing provider or item id. Album={AlbumName}", album.Name);
                        break;
                    }

                    var albumTracks = await _musicAssistant.GetAlbumTracksAsync(album.ItemId, provider);
                    resolvedTracks.AddRange(albumTracks);
                    break;
                }

                case Artist artist:
                {
                    var provider = ResolveProviderInstanceOrDomain(artist);
                    if (string.IsNullOrWhiteSpace(provider) || string.IsNullOrWhiteSpace(artist.ItemId))
                    {
                        _logger.LogWarning("Cannot resolve artist tracks due to missing provider or item id. Artist={ArtistName}", artist.Name);
                        break;
                    }

                    var artistTracks = await _musicAssistant.GetArtistTracksAsync(artist.ItemId, provider);
                    resolvedTracks.AddRange(artistTracks.Take(artistTopTracksLimit));
                    break;
                }

                default:
                    _logger.LogDebug("Ignoring unsupported media item type for play queue resolution. Type={MediaType}", mediaItem.MediaType);
                    break;
            }
        }

        return resolvedTracks;
    }

    private static string? ResolveProviderInstanceOrDomain(MediaItem item)
    {
        if (!string.IsNullOrWhiteSpace(item.Provider))
        {
            return item.Provider;
        }

        var mapping = item.ProviderMappings.FirstOrDefault();

        if (!string.IsNullOrWhiteSpace(mapping?.ProviderInstance))
        {
            return mapping.ProviderInstance;
        }

        if (!string.IsNullOrWhiteSpace(mapping?.ProviderDomain))
        {
            return mapping.ProviderDomain;
        }

        return null;
    }

    private bool SetProperty<T>(ref T storage, T value, [CallerMemberName] string? propertyName = null)
    {
        if (EqualityComparer<T>.Default.Equals(storage, value))
        {
            return false;
        }

        storage = value;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
        return true;
    }

    #endregion
}
