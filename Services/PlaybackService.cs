using mashin.Collections;
using mashin.Models;
using Microsoft.Extensions.Logging;
using System.ComponentModel;
using System.Runtime.CompilerServices;

namespace mashin.Services;

public enum PlaybackOutputMode
{
    LocalOffline,
    LocalSendspin,
    RemoteOnly
}

#region Interface

public interface IPlaybackService : IAsyncDisposable, INotifyPropertyChanged
{
    PlaybackOutputMode OutputMode { get; }
    string? ActivePlayerId { get; }
    PlaybackState PlaybackState { get; set; }
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

    private readonly MusicAssistantService _musicAssistant;
    private readonly SettingsService _settingsService;
    private readonly IMusicAssistantEventHub _musicAssistantEventHub;
    private readonly ILogger<PlaybackService> _logger;
    private readonly Dictionary<PlaybackOutputMode, IPlayerService> _players;

    private readonly ObservableRangeCollection<QueueItem> _currentQueueItems = new();
    private readonly SemaphoreSlim _queueRefreshGate = new(1, 1);
    private readonly CancellationTokenSource _disposeCts = new();
    private readonly object _progressSync = new();
    private CancellationTokenSource? _queueConfirmCts;
    private int _queueConfirmRequestId;
    private Task? _progressInterpolationTask;

    private PlaybackOutputMode _outputMode = PlaybackOutputMode.LocalSendspin;

    private string? _activePlayerId;
    private IPlayerService _activePlayer;

    private PlaybackState _playbackState = mashin.Models.PlaybackState.Idle;
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
    private double? _elapsedTimeAnchorSeconds;
    private DateTimeOffset? _elapsedTimeLastUpdatedUtc;

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

        _activePlayer = _players.TryGetValue(PlaybackOutputMode.LocalSendspin, out var sendspin)
            ? sendspin
            : _players.Values.First();

        _activePlayer.PropertyChanged += OnActivePlayerPropertyChanged;
        _musicAssistantEventHub.QueueEventReceived += OnQueueEventReceived;

        _progressInterpolationTask = Task.Run(async () =>
        {
            while (!_disposeCts.IsCancellationRequested)
            {
                try
                {
                    await Task.Delay(TimeSpan.FromMilliseconds(250), _disposeCts.Token);
                }
                catch (OperationCanceledException)
                {
                    break;
                }

                if (_playbackState != mashin.Models.PlaybackState.Playing)
                {
                    continue;
                }

                double? elapsedAnchor;
                DateTimeOffset? elapsedAnchorUpdatedUtc;
                lock (_progressSync)
                {
                    elapsedAnchor = _elapsedTimeAnchorSeconds;
                    elapsedAnchorUpdatedUtc = _elapsedTimeLastUpdatedUtc;
                }

                if (elapsedAnchor is not double anchorSeconds || elapsedAnchorUpdatedUtc is not DateTimeOffset updatedUtc)
                {
                    continue;
                }

                var interpolated = anchorSeconds + Math.Max(0, (DateTimeOffset.UtcNow - updatedUtc).TotalSeconds);
                if (_durationSeconds > 0)
                {
                    interpolated = Math.Min(interpolated, _durationSeconds);
                }

                PositionSeconds = interpolated;
            }
        }, CancellationToken.None);
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

    public PlaybackState PlaybackState
    {
        get => _playbackState;
        set => SetProperty(ref _playbackState, value);
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

    public async Task PlayMediaAsync(IReadOnlyList<MediaItem> items)
    {
        if (items == null || items.Count == 0)
        {
            _logger.LogWarning("No items selected to play");
            return;
        }

        if (OutputMode == PlaybackOutputMode.LocalOffline || string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            _logger.LogWarning("PlayMedia ignored in LocalOffline mode.");
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            _logger.LogWarning("No playable tracks resolved for PlayMedia");
            return;
        }

        PlaybackState = mashin.Models.PlaybackState.Buffering;

        await _musicAssistant.PlayMediaAsync(ActivePlayerId!, resolvedItems, QueueOption.Replace);
    }

    public async Task PlayMediaNextAsync(IReadOnlyList<MediaItem> items)
    {
        if (items == null || items.Count == 0)
        {
            _logger.LogWarning("No items selected to play");
            return;
        }

        if (OutputMode == PlaybackOutputMode.LocalOffline || string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            _logger.LogWarning("PlayMediaNext ignored in LocalOffline mode.");
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            _logger.LogWarning("No playable tracks resolved for PlayMediaNext");
            return;
        }

        PlaybackState = mashin.Models.PlaybackState.Buffering;

        await _musicAssistant.PlayMediaAsync(ActivePlayerId!, resolvedItems, QueueOption.Next);
    }

    public async Task PlayMediaLastAsync(IReadOnlyList<MediaItem> items)
    {
        if (items == null || items.Count == 0)
        {
            _logger.LogWarning("No items selected to play");
            return;
        }

        if (OutputMode == PlaybackOutputMode.LocalOffline || string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            _logger.LogWarning("PlayMediaLast ignored in LocalOffline mode.");
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            _logger.LogWarning("No playable tracks resolved for PlayMediaLast");
            return;
        }

        PlaybackState = mashin.Models.PlaybackState.Buffering;

        await _musicAssistant.PlayMediaAsync(ActivePlayerId!, resolvedItems, QueueOption.Add);
    }

    public async Task ShufflePlayMediaAsync(IReadOnlyList<MediaItem> items)
    {
        if (OutputMode == PlaybackOutputMode.LocalOffline || string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            _logger.LogWarning("ShufflePlayMedia ignored in LocalOffline mode.");
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

        await _musicAssistant.PlayMediaAsync(ActivePlayerId!, mediaItems, QueueOption.Replace);
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

        if (OutputMode == PlaybackOutputMode.LocalOffline || string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            _logger.LogWarning("PlayQueueIndex ignored in LocalOffline mode.");
            return;
        }

        var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(ActivePlayerId);
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

        if (OutputMode == PlaybackOutputMode.LocalOffline || string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            _logger.LogWarning("MoveQueueItem ignored in LocalOffline mode.");
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

        if (OutputMode == PlaybackOutputMode.LocalOffline || string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            _logger.LogWarning("DeleteQueueItem ignored in LocalOffline mode.");
            return;
        }

        var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(ActivePlayerId);
        if (string.IsNullOrWhiteSpace(queue?.QueueId))
        {
            return;
        }

        await _musicAssistant.DeleteQueueItemAsync(queue.QueueId, queueItemId);
    }

    #endregion

    #region Transport Commands

    public async Task TogglePlayPauseAsync(CancellationToken cancellationToken = default)
    {
        await _activePlayer.TogglePlayPauseAsync(cancellationToken);
        PlaybackState = _activePlayer.PlayerState;
    }

    public async Task NextTrackAsync(CancellationToken cancellationToken = default)
    {
        await _activePlayer.NextAsync(cancellationToken);
    }

    public async Task PreviousTrackAsync(CancellationToken cancellationToken = default)
    {
        await _activePlayer.PreviousAsync(cancellationToken);
    }

    public async Task SeekAsync(double seconds, double durationSeconds, CancellationToken cancellationToken = default)
    {
        var clamped = (int)Math.Round(Math.Clamp(seconds, 0, Math.Max(0, durationSeconds)));
        await _activePlayer.SeekAsync(clamped, cancellationToken);
        PositionSeconds = clamped;

        lock (_progressSync)
        {
            _elapsedTimeAnchorSeconds = clamped;
            _elapsedTimeLastUpdatedUtc = DateTimeOffset.UtcNow;
        }
    }

    public async Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default)
    {
        var clamped = Math.Clamp(volume, 0, 100);
        await _activePlayer.SetVolumeAsync(clamped, cancellationToken);
        Volume = clamped;
    }

    public async Task ToggleMuteAsync(bool currentMuted, CancellationToken cancellationToken = default)
    {
        var next = !currentMuted;
        await _activePlayer.SetMutedAsync(next, cancellationToken);
        IsMuted = next;
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
        ShuffleEnabled = next;
        await _activePlayer.SetShuffleAsync(next, cancellationToken);
    }

    public async Task ToggleRepeatModeAsync(string? currentRepeatMode, CancellationToken cancellationToken = default)
    {
        var next = ToRepeatMode(currentRepeatMode) switch
        {
            mashin.Models.RepeatMode.Off => mashin.Models.RepeatMode.All,
            mashin.Models.RepeatMode.All => mashin.Models.RepeatMode.One,
            _ => mashin.Models.RepeatMode.Off
        };

        RepeatMode = next.ToString();
        await _activePlayer.SetRepeatModeAsync(next, cancellationToken);
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
        DontStopTheMusicEnabled = enabled;
    }

    #endregion

    #region Disposal

    public async ValueTask DisposeAsync()
    {
        await _musicAssistantEventHub.StopAsync(CancellationToken.None);
        _musicAssistantEventHub.QueueEventReceived -= OnQueueEventReceived;
        _activePlayer.PropertyChanged -= OnActivePlayerPropertyChanged;

        var queueConfirmCts = Interlocked.Exchange(ref _queueConfirmCts, null);
        queueConfirmCts?.Cancel();
        queueConfirmCts?.Dispose();

        _disposeCts.Cancel();

        if (_progressInterpolationTask != null)
        {
            try
            {
                await _progressInterpolationTask;
            }
            catch (OperationCanceledException)
            {
                // Expected on shutdown.
            }
        }

        _disposeCts.Dispose();

        _queueRefreshGate.Dispose();

        foreach (var player in _players.Values.Distinct())
        {
            await player.DisposeAsync();
        }
    }

    #endregion

    #region Event Handling

    private void OnQueueEventReceived(object? sender, MusicAssistantQueueEvent e)
    {
        if (OutputMode == PlaybackOutputMode.LocalOffline || string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            return;
        }

        var eventQueueId = Normalize(e.Queue?.QueueId) ?? Normalize(e.QueueId);

        // Filter by active queue id, not player id. In MA these ids can differ
        // when a player has an active source/group queue.
        if (!string.IsNullOrWhiteSpace(_activeQueueId)
            && !string.IsNullOrWhiteSpace(eventQueueId)
            && !string.Equals(eventQueueId, _activeQueueId, StringComparison.Ordinal))
        {
            _logger.LogDebug("Ignored queue event for different queue. Event={Event}, EventQueueId={EventQueueId}, ActiveQueueId={ActiveQueueId}",
                e.Event,
                eventQueueId,
                _activeQueueId);
            return;
        }

        // Learn active queue id as soon as we can to make subsequent filtering precise.
        if (string.IsNullOrWhiteSpace(_activeQueueId) && !string.IsNullOrWhiteSpace(eventQueueId))
        {
            _activeQueueId = eventQueueId;
        }

        if (string.Equals(e.Event, "queue_time_updated", StringComparison.OrdinalIgnoreCase))
        {
            // Keep playback progress event-driven without queue HTTP fetches.
            if (e.ElapsedTimeSeconds is double elapsedSeconds)
            {
                var clampedElapsed = Math.Max(0, elapsedSeconds);
                PositionSeconds = clampedElapsed;

                lock (_progressSync)
                {
                    _elapsedTimeAnchorSeconds = clampedElapsed;
                    _elapsedTimeLastUpdatedUtc = DateTimeOffset.UtcNow;
                }
            }

            return;
        }

        _logger.LogDebug("Queue event received: Event={Event}, QueueId={QueueId}, HasQueuePayload={HasQueuePayload}",
            e.Event,
            eventQueueId,
            e.Queue != null);

        if (e.Queue != null)
        {
            // Apply queue state immediately from the MA websocket event stream.
            _activeQueueId = Normalize(e.Queue.QueueId) ?? _activeQueueId;

            QueueItemCount = e.Queue.ItemCount;
            ShuffleEnabled = e.Queue.ShuffleEnabled;
            RepeatMode = e.Queue.RepeatMode?.ToString();
            DontStopTheMusicEnabled = e.Queue.DontStopTheMusicEnabled;

            SetProperty(ref _currentQueueItem, e.Queue.CurrentItem, nameof(CurrentQueueItem));

            CurrentQueueIndex = e.Queue.CurrentIndex;

            DurationSeconds = Math.Max(0, e.Queue.CurrentItem?.Duration ?? 0);
            var queueElapsed = Math.Max(0, e.Queue.ElapsedTime ?? PositionSeconds);
            PositionSeconds = queueElapsed;

            var elapsedUpdatedAtUtc = DateTimeOffset.UtcNow;
            if (e.Queue.ElapsedTimeLastUpdated is double elapsedUpdatedEpochSeconds && elapsedUpdatedEpochSeconds > 0)
            {
                var elapsedUpdatedEpochMilliseconds = (long)Math.Round(elapsedUpdatedEpochSeconds * 1000d, MidpointRounding.AwayFromZero);
                if (elapsedUpdatedEpochMilliseconds > 0)
                {
                    elapsedUpdatedAtUtc = DateTimeOffset.FromUnixTimeMilliseconds(elapsedUpdatedEpochMilliseconds);
                }
            }

            lock (_progressSync)
            {
                _elapsedTimeAnchorSeconds = queueElapsed;
                _elapsedTimeLastUpdatedUtc = elapsedUpdatedAtUtc;
            }

            PlaybackState = MapState(e.Queue.State);
        }

        var shouldReloadQueueItems = string.Equals(e.Event, "queue_items_updated", StringComparison.OrdinalIgnoreCase)
            || string.Equals(e.Event, "queue_added", StringComparison.OrdinalIgnoreCase);

        var shouldConfirmQueueState = shouldReloadQueueItems
            || string.Equals(e.Event, "queue_updated", StringComparison.OrdinalIgnoreCase)
            || string.Equals(e.Event, "queue_settings_updated", StringComparison.OrdinalIgnoreCase);

        if (!shouldConfirmQueueState)
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

                _logger.LogDebug("Confirming queue snapshot after event burst. RequestId={RequestId}, QueueId={QueueId}, ReloadQueueItems={ReloadQueueItems}", requestId, _activeQueueId, shouldReloadQueueItems);
                await RefreshQueueAsync(nextCts.Token, shouldReloadQueueItems);
            }
            catch (OperationCanceledException)
            {
                // Expected during rapid updates and shutdown.
            }
        }, CancellationToken.None);
    }

    private void OnActivePlayerPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        SyncStateFromPlayer();
    }

    #endregion

    #region Queue Sync

    private async Task RefreshQueueAsync(CancellationToken cancellationToken = default, bool reloadQueueItems = true)
    {
        if (OutputMode == PlaybackOutputMode.LocalOffline || string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            return;
        }

        _logger.LogDebug("Refreshing queue snapshot. ActivePlayerId={ActivePlayerId}, ReloadQueueItems={ReloadQueueItems}", ActivePlayerId, reloadQueueItems);

        try
        {
            await _queueRefreshGate.WaitAsync(cancellationToken);
        }
        catch (OperationCanceledException)
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
            if (reloadQueueItems || _currentQueueItems.Count == 0)
            {
                var queueItems = await _musicAssistant.GetQueueItemsAsync(queue.QueueId);
                _currentQueueItems.ReplaceRange(queueItems);
            }

            QueueItemCount = queue.ItemCount;
            ShuffleEnabled = queue.ShuffleEnabled;
            RepeatMode = queue.RepeatMode?.ToString();
            DontStopTheMusicEnabled = queue.DontStopTheMusicEnabled;

            SetProperty(ref _currentQueueItem, queue.CurrentItem, nameof(CurrentQueueItem));

            CurrentQueueIndex = queue.CurrentIndex;

            DurationSeconds = Math.Max(0, queue.CurrentItem?.Duration ?? 0);
            var queueElapsed = Math.Max(0, queue.ElapsedTime ?? 0);
            PositionSeconds = queueElapsed;

            var elapsedUpdatedAtUtc = DateTimeOffset.UtcNow;
            if (queue.ElapsedTimeLastUpdated is double elapsedUpdatedEpochSeconds && elapsedUpdatedEpochSeconds > 0)
            {
                var elapsedUpdatedEpochMilliseconds = (long)Math.Round(elapsedUpdatedEpochSeconds * 1000d, MidpointRounding.AwayFromZero);
                if (elapsedUpdatedEpochMilliseconds > 0)
                {
                    elapsedUpdatedAtUtc = DateTimeOffset.FromUnixTimeMilliseconds(elapsedUpdatedEpochMilliseconds);
                }
            }

            lock (_progressSync)
            {
                _elapsedTimeAnchorSeconds = queueElapsed;
                _elapsedTimeLastUpdatedUtc = elapsedUpdatedAtUtc;
            }

            PlaybackState = MapState(queue.State);

            _logger.LogDebug(
                "Queue snapshot applied. QueueId={QueueId}, Items={ItemCount}, CurrentIndex={CurrentIndex}, CurrentItemId={CurrentItemId}, State={State}, Elapsed={Elapsed}",
                queue.QueueId,
                queue.ItemCount,
                queue.CurrentIndex,
                queue.CurrentItem?.QueueItemId,
                queue.State,
                queue.ElapsedTime);

        }
        catch (OperationCanceledException)
        {
            // Expected on cancellation.
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to refresh queue snapshot for player {PlayerId}", ActivePlayerId);
        }
        finally
        {
            _queueRefreshGate.Release();
        }
    }

    private void ResetQueueState()
    {
        _logger.LogDebug("Resetting queue state.");

        _activeQueueId = null;
        _currentQueueItems.ReplaceRange(Array.Empty<QueueItem>());
        SetProperty(ref _currentQueueItem, null, nameof(CurrentQueueItem));

        lock (_progressSync)
        {
            _elapsedTimeAnchorSeconds = null;
            _elapsedTimeLastUpdatedUtc = null;
        }

        CurrentQueueIndex = null;
        QueueItemCount = 0;
        DurationSeconds = 0;
        PositionSeconds = 0;
        ShuffleEnabled = null;
        RepeatMode = null;
        DontStopTheMusicEnabled = null;
        PlaybackState = mashin.Models.PlaybackState.Idle;
    }

    #endregion

    #region Helpers

    private void SyncStateFromPlayer()
    {
        Volume = _activePlayer.Volume;
        IsMuted = _activePlayer.IsMuted;

        PlaybackState = _activePlayer.PlayerState;
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

    private static PlaybackState MapState(mashin.Models.PlaybackState? state)
    {
        return state switch
        {
            mashin.Models.PlaybackState.Playing => mashin.Models.PlaybackState.Playing,
            mashin.Models.PlaybackState.Paused => mashin.Models.PlaybackState.Paused,
            mashin.Models.PlaybackState.Buffering => mashin.Models.PlaybackState.Buffering,
            mashin.Models.PlaybackState.Idle => mashin.Models.PlaybackState.Idle,
            _ => mashin.Models.PlaybackState.Unknown
        };
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
