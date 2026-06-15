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
    PlaybackStateCustom PlaybackState { get; set; }
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
    private readonly CancellationTokenSource _disposeCts = new();
    private readonly object _progressSync = new();
    private CancellationTokenSource? _queueConfirmCts;
    private int _queueConfirmRequestId;
    private Task? _progressInterpolationTask;

    private PlaybackOutputMode _outputMode = PlaybackOutputMode.LocalSendspin;

    private string? _activePlayerId;
    private IPlayerService _activePlayer;

    private PlaybackStateCustom _playbackState = new()
    {
        State = PlaybackStateKind.Idle,
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

    // Anchors for interpolation of playback position between MA websocket events
    private double? _elapsedTimeAnchorSeconds;
    private DateTimeOffset? _elapsedTimeLastUpdatedUtc;
    
    // Seek protection to prevent regression of playback position when MA sends stale elapsed time updates after a seek operation.
    private DateTimeOffset? _seekProtectionUntilUtc;
    private double? _pendingSeekTargetSeconds;
    private string? _pendingSeekQueueItemId;
    
    // Track transition guard: after item change, accept only plausible queue_time updates for a short window to avoid old-track elapsed events causing forward jumps.
    private string? _transitionGuardQueueItemId;
    private DateTimeOffset? _transitionGuardStartedUtc;
    private double? _transitionGuardStartElapsedSeconds;
    private DateTimeOffset? _transitionGuardUntilUtc;
    
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

                if (_playbackState.State != PlaybackStateKind.Playing)
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

    public PlaybackStateCustom PlaybackState
    {
        get => _playbackState;
        set => SetProperty(ref _playbackState, value ?? new PlaybackStateCustom
        {
            State = PlaybackStateKind.Unknown,
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

        PlaybackState = new PlaybackStateCustom { State = PlaybackStateKind.PendingToPlaying, ActiveSinceUtc = DateTimeOffset.UtcNow };

        try
        {
            await _musicAssistant.PlayMediaAsync(ActivePlayerId!, resolvedItems, QueueOption.Replace);
        }
        catch (Exception ex)
        {
            PlaybackState = new PlaybackStateCustom { State = PlaybackStateKind.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
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

        PlaybackState = new PlaybackStateCustom { State = PlaybackStateKind.PendingToPlaying, ActiveSinceUtc = DateTimeOffset.UtcNow };

        try
        {
            await _musicAssistant.PlayMediaAsync(ActivePlayerId!, resolvedItems, QueueOption.Next);
        }
        catch (Exception ex)
        {
            PlaybackState = new PlaybackStateCustom { State = PlaybackStateKind.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
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

        PlaybackState = new PlaybackStateCustom { State = PlaybackStateKind.PendingToPlaying, ActiveSinceUtc = DateTimeOffset.UtcNow };

        try
        {
            await _musicAssistant.PlayMediaAsync(ActivePlayerId!, resolvedItems, QueueOption.Add);
        }
        catch (Exception ex)
        {
            PlaybackState = new PlaybackStateCustom { State = PlaybackStateKind.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
            _logger.LogError(ex, "PlayMediaLast request failed for ActivePlayerId={ActivePlayerId}", ActivePlayerId);
            throw;
        }
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

        PlaybackState = new PlaybackStateCustom { State = PlaybackStateKind.PendingToPlaying, ActiveSinceUtc = DateTimeOffset.UtcNow };

        try
        {
            await _musicAssistant.PlayMediaAsync(ActivePlayerId!, mediaItems, QueueOption.Replace);
        }
        catch (Exception ex)
        {
            PlaybackState = new PlaybackStateCustom { State = PlaybackStateKind.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
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
        var wasPlayingBeforeToggle = PlaybackState.State == PlaybackStateKind.Playing;

        PlaybackState = new PlaybackStateCustom
        {
            State = wasPlayingBeforeToggle ? PlaybackStateKind.PendingToPaused : PlaybackStateKind.PendingToPlaying,
            ActiveSinceUtc = DateTimeOffset.UtcNow
        };
        await _activePlayer.TogglePlayPauseAsync(cancellationToken);

        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            PlaybackState = new PlaybackStateCustom
            {
                State = MapState(_activePlayer.PlayerState),
                ActiveSinceUtc = DateTimeOffset.UtcNow
            };
        }
    }

    public async Task NextTrackAsync(CancellationToken cancellationToken = default)
    {
        PlaybackState = new PlaybackStateCustom { State = PlaybackStateKind.PendingToNextTrack, ActiveSinceUtc = DateTimeOffset.UtcNow };
        await _activePlayer.NextAsync(cancellationToken);
    }

    public async Task PreviousTrackAsync(CancellationToken cancellationToken = default)
    {
        PlaybackState = new PlaybackStateCustom { State = PlaybackStateKind.PendingToPreviousTrack, ActiveSinceUtc = DateTimeOffset.UtcNow };
        await _activePlayer.PreviousAsync(cancellationToken);
    }

    public async Task SeekAsync(double seconds, double durationSeconds, CancellationToken cancellationToken = default)
    {
        PlaybackState = new PlaybackStateCustom { State = PlaybackStateKind.PendingToSeek, ActiveSinceUtc = DateTimeOffset.UtcNow };
        var clamped = (int)Math.Round(Math.Clamp(seconds, 0, Math.Max(0, durationSeconds)));
        await _activePlayer.SeekAsync(clamped, cancellationToken);
        PositionSeconds = clamped;

        lock (_progressSync)
        {
            _elapsedTimeAnchorSeconds = clamped;
            _elapsedTimeLastUpdatedUtc = DateTimeOffset.UtcNow;
            _pendingSeekTargetSeconds = clamped;
            _pendingSeekQueueItemId = Normalize(_currentQueueItem?.QueueItemId);
            _seekProtectionUntilUtc = DateTimeOffset.UtcNow.AddMilliseconds(1200);
            _transitionGuardQueueItemId = null;
            _transitionGuardStartedUtc = null;
            _transitionGuardStartElapsedSeconds = null;
            _transitionGuardUntilUtc = null;
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

        // queue time update events: keep playback progress event-driven without queue HTTP fetches
        if (string.Equals(e.Event, "queue_time_updated", StringComparison.OrdinalIgnoreCase))
        {
            if (e.ElapsedTimeSeconds is double elapsedSeconds)
            {
                var clampedElapsed = Math.Max(0, elapsedSeconds);

                // Seek guard: ignore elapsed time updates that regress behind a recent seek target within a short time window, as they are likely stale events from before the seek was issued
                if (ShouldIgnoreSeekRegression(clampedElapsed, source: "queue_time_updated"))
                {
                    return;
                }

                // Transition guard: after a track change, ignore elapsed time updates that don't show plausible forward progress for the new track within a short time window, as they are likely stale events from the previous track
                var currentItemId = Normalize(_currentQueueItem?.QueueItemId);
                if (ShouldIgnoreTrackTransitionOutlier(currentItemId, clampedElapsed, source: "queue_time_updated"))
                {
                    return;
                }

                PositionSeconds = clampedElapsed;

                _logger.LogDebug(
                    "Queue time updated: QueueId={QueueId}, ElapsedSeconds={ElapsedSeconds}",
                    eventQueueId,
                    clampedElapsed);

                if (PlaybackState.IsPending || PlaybackState.State == PlaybackStateKind.Buffering)
                {
                    PlaybackState = new PlaybackStateCustom { State = PlaybackStateKind.Playing, ActiveSinceUtc = DateTimeOffset.UtcNow };
                }

                lock (_progressSync)
                {
                    _elapsedTimeAnchorSeconds = clampedElapsed;
                    _elapsedTimeLastUpdatedUtc = DateTimeOffset.UtcNow;
                }
            }

            return;
        }

        // other queue events: apply event state updates immediately, enrich with debounced HTTP refreshes if queue items changed
        {
            // Apply event state updates
            _activeQueueId = Normalize(e.Queue.QueueId) ?? _activeQueueId;

            var previousQueueItemId = Normalize(_currentQueueItem?.QueueItemId);
            var incomingQueueItemId = Normalize(e.Queue.CurrentItem?.QueueItemId);
            var isTrackChanged = !string.Equals(previousQueueItemId, incomingQueueItemId, StringComparison.Ordinal);

            QueueItemCount = e.Queue.ItemCount;
            ShuffleEnabled = e.Queue.ShuffleEnabled;
            RepeatMode = e.Queue.RepeatMode?.ToString();
            DontStopTheMusicEnabled = e.Queue.DontStopTheMusicEnabled;

            SetProperty(ref _currentQueueItem, e.Queue.CurrentItem, nameof(CurrentQueueItem));

            CurrentQueueIndex = e.Queue.CurrentIndex;

            DurationSeconds = Math.Max(0, e.Queue.CurrentItem?.Duration ?? 0);
            var queueElapsed = e.Queue.ElapsedTime.HasValue
                ? Math.Max(0, e.Queue.ElapsedTime.Value)
                : (isTrackChanged ? 0 : PositionSeconds);

            // Seek guard: if the event contains an elapsed time that regresses behind a recent seek target within a short time window, treat it as a likely stale event from before the seek was issued and ignore the regression by keeping the current position
            if (ShouldIgnoreSeekRegression(queueElapsed, source: "queue_payload"))
            {
                queueElapsed = PositionSeconds;
            }

            // Transition guard: if this is a track change and the event contains an elapsed time that doesn't show plausible forward progress for the new track within a short time window, treat it as a likely stale event from the previous track and ignore it by keeping the current position
            var ignoreQueuePayloadProgress = !isTrackChanged
                && ShouldIgnoreTransitionQueuePayloadProgress(incomingQueueItemId, queueElapsed);

            if (!ignoreQueuePayloadProgress)
            {
                PositionSeconds = queueElapsed;
            }

            var elapsedUpdatedAtUtc = DateTimeOffset.UtcNow;
            if (e.Queue.ElapsedTimeLastUpdated is double elapsedUpdatedEpochSeconds && elapsedUpdatedEpochSeconds > 0)
            {
                var elapsedUpdatedEpochMilliseconds = (long)Math.Round(elapsedUpdatedEpochSeconds * 1000d, MidpointRounding.AwayFromZero);
                if (elapsedUpdatedEpochMilliseconds > 0)
                {
                    elapsedUpdatedAtUtc = DateTimeOffset.FromUnixTimeMilliseconds(elapsedUpdatedEpochMilliseconds);
                }
            }

            // If the track changed but the elapsed time in the event is very low (e.g. 0 or 1 second), it's likely that the elapsed time was reset by the player before the new track actually started playing. In this case, update the elapsed time anchor to prevent interpolation from jumping back to a high elapsed time for the new track.
            if (isTrackChanged && queueElapsed <= 1)
            {
                elapsedUpdatedAtUtc = DateTimeOffset.UtcNow;
            }

            if (isTrackChanged)
            {
                var transitionStartedUtc = DateTimeOffset.UtcNow;
                lock (_progressSync)
                {
                    _transitionGuardQueueItemId = incomingQueueItemId;
                    _transitionGuardStartedUtc = transitionStartedUtc;
                    _transitionGuardStartElapsedSeconds = queueElapsed;
                    _transitionGuardUntilUtc = transitionStartedUtc.AddSeconds(4);
                }
            }

            if (!ignoreQueuePayloadProgress)
            {
                lock (_progressSync)
                {
                    _elapsedTimeAnchorSeconds = queueElapsed;
                    _elapsedTimeLastUpdatedUtc = elapsedUpdatedAtUtc;
                }
            }

            var mappedState = MapState(e.Queue.State);
            var effectiveState = ResolveEffectivePlaybackState(mappedState);

            if (effectiveState != mappedState)
            {
                _logger.LogDebug(
                    "Keeping pending playback state. QueueId={QueueId}, PendingState={PendingState}, IncomingState={IncomingState}",
                    eventQueueId,
                    PlaybackState.State,
                    mappedState);
            }

            PlaybackState = new PlaybackStateCustom { State = effectiveState, ActiveSinceUtc = DateTimeOffset.UtcNow };
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

                _logger.LogInformation("Queue refresh triggered by event. Event={Event}, QueueId={QueueId}, RequestId={RequestId}", e.Event, _activeQueueId, requestId);
                await RefreshQueueAsync(nextCts.Token);
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

            var mappedState = MapState(queue.State);
            var effectiveState = ResolveEffectivePlaybackState(mappedState);

            if (effectiveState != mappedState)
            {
                _logger.LogDebug(
                    "Keeping pending playback state from HTTP snapshot. QueueId={QueueId}, PendingState={PendingState}, IncomingState={IncomingState}",
                    queue.QueueId,
                    PlaybackState.State,
                    mappedState);
            }

            PlaybackState = new PlaybackStateCustom { State = effectiveState, ActiveSinceUtc = DateTimeOffset.UtcNow };

            _logger.LogDebug(
                "Full Queue refresh via HTTP applied. QueueId={QueueId}, Items={ItemCount}, CurrentIndex={CurrentIndex}, CurrentItemId={CurrentItemId}, State={State}, Elapsed={Elapsed}",
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
            _seekProtectionUntilUtc = null;
            _pendingSeekTargetSeconds = null;
            _pendingSeekQueueItemId = null;
            _transitionGuardQueueItemId = null;
            _transitionGuardStartedUtc = null;
            _transitionGuardStartElapsedSeconds = null;
            _transitionGuardUntilUtc = null;
        }

        CurrentQueueIndex = null;
        QueueItemCount = 0;
        DurationSeconds = 0;
        PositionSeconds = 0;
        ShuffleEnabled = null;
        RepeatMode = null;
        DontStopTheMusicEnabled = null;
        PlaybackState = new PlaybackStateCustom { State = PlaybackStateKind.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
    }

    #endregion

    #region Helpers

    private void SyncStateFromPlayer()
    {
        Volume = _activePlayer.Volume;
        IsMuted = _activePlayer.IsMuted;

        // Only sync playback state from the player when in LocalOffline mode, as in other modes the MA event hub is the source of truth for playback state.
        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            PlaybackState = new PlaybackStateCustom
            {
                State = MapState(_activePlayer.PlayerState),
                ActiveSinceUtc = DateTimeOffset.UtcNow
            };
        }
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

    private PlaybackStateKind ResolveEffectivePlaybackState(PlaybackStateKind mappedState)
    {
        var currentState = PlaybackState;

        if (!currentState.IsPending)
        {
            return mappedState;
        }

        return currentState.State switch
        {
            PlaybackStateKind.PendingToPlaying
                when mappedState is PlaybackStateKind.Idle or PlaybackStateKind.Buffering or PlaybackStateKind.Unknown
                => PlaybackStateKind.PendingToPlaying,

            PlaybackStateKind.PendingToNextTrack
                when mappedState is PlaybackStateKind.Idle or PlaybackStateKind.Buffering or PlaybackStateKind.Unknown
                => PlaybackStateKind.PendingToNextTrack,

            PlaybackStateKind.PendingToPreviousTrack
                when mappedState is PlaybackStateKind.Idle or PlaybackStateKind.Buffering or PlaybackStateKind.Unknown
                => PlaybackStateKind.PendingToPreviousTrack,

            PlaybackStateKind.PendingToSeek
                when mappedState is PlaybackStateKind.Idle or PlaybackStateKind.Buffering or PlaybackStateKind.Unknown
                => PlaybackStateKind.PendingToSeek,

            PlaybackStateKind.PendingToPaused
                when mappedState is PlaybackStateKind.Playing or PlaybackStateKind.Buffering or PlaybackStateKind.Unknown
                => PlaybackStateKind.PendingToPaused,

            _ => mappedState,
        };
    }

    private bool ShouldIgnoreSeekRegression(double candidateSeconds, string source)
    {
        DateTimeOffset? seekProtectionUntilUtc;
        double? pendingSeekTargetSeconds;
        string? pendingSeekQueueItemId;

        lock (_progressSync)
        {
            seekProtectionUntilUtc = _seekProtectionUntilUtc;
            pendingSeekTargetSeconds = _pendingSeekTargetSeconds;
            pendingSeekQueueItemId = _pendingSeekQueueItemId;
        }

        if (seekProtectionUntilUtc is not DateTimeOffset protectionUntil
            || pendingSeekTargetSeconds is not double seekTarget)
        {
            return false;
        }

        var currentItemId = Normalize(_currentQueueItem?.QueueItemId);
        if (!string.Equals(pendingSeekQueueItemId, currentItemId, StringComparison.Ordinal))
        {
            lock (_progressSync)
            {
                _seekProtectionUntilUtc = null;
                _pendingSeekTargetSeconds = null;
                _pendingSeekQueueItemId = null;
            }

            return false;
        }

        if (DateTimeOffset.UtcNow >= protectionUntil)
        {
            lock (_progressSync)
            {
                _seekProtectionUntilUtc = null;
                _pendingSeekTargetSeconds = null;
                _pendingSeekQueueItemId = null;
            }

            return false;
        }

        var lowerBound = Math.Max(0, seekTarget - 1.5);
        var upperBound = seekTarget + 3.0;
        if (candidateSeconds < lowerBound || candidateSeconds > upperBound)
        {
            _logger.LogDebug(
                "Ignored seek regression/outlier. Source={Source}, Candidate={Candidate}, SeekTarget={SeekTarget}",
                source,
                candidateSeconds,
                seekTarget);
            return true;
        }

        lock (_progressSync)
        {
            _seekProtectionUntilUtc = null;
            _pendingSeekTargetSeconds = null;
            _pendingSeekQueueItemId = null;
        }

        return false;
    }

    private bool ShouldIgnoreTrackTransitionOutlier(string? currentItemId, double candidateSeconds, string source)
    {
        string? guardItemId;
        DateTimeOffset? guardStartedUtc;
        double? guardStartElapsedSeconds;
        DateTimeOffset? guardUntilUtc;

        lock (_progressSync)
        {
            guardItemId = _transitionGuardQueueItemId;
            guardStartedUtc = _transitionGuardStartedUtc;
            guardStartElapsedSeconds = _transitionGuardStartElapsedSeconds;
            guardUntilUtc = _transitionGuardUntilUtc;
        }

        if (string.IsNullOrWhiteSpace(guardItemId)
            || guardStartedUtc is not DateTimeOffset startedUtc
            || guardStartElapsedSeconds is not double startElapsedSeconds
            || guardUntilUtc is not DateTimeOffset untilUtc)
        {
            return false;
        }

        // Guard belongs to a previous item; drop it.
        if (!string.Equals(guardItemId, currentItemId, StringComparison.Ordinal))
        {
            lock (_progressSync)
            {
                _transitionGuardQueueItemId = null;
                _transitionGuardStartedUtc = null;
                _transitionGuardStartElapsedSeconds = null;
                _transitionGuardUntilUtc = null;
            }

            return false;
        }

        var now = DateTimeOffset.UtcNow;
        if (now >= untilUtc)
        {
            lock (_progressSync)
            {
                _transitionGuardQueueItemId = null;
                _transitionGuardStartedUtc = null;
                _transitionGuardStartElapsedSeconds = null;
                _transitionGuardUntilUtc = null;
            }

            return false;
        }

        // queue_time_updated should not jump far ahead of what is plausible since transition start.
        const double jitterSeconds = 1.5;
        var maxPlausible = Math.Max(0, startElapsedSeconds + (now - startedUtc).TotalSeconds + jitterSeconds);
        if (candidateSeconds > maxPlausible)
        {
            _logger.LogDebug(
                "Ignored transition outlier. Source={Source}, Candidate={Candidate}, MaxPlausible={MaxPlausible}, ItemId={ItemId}",
                source,
                candidateSeconds,
                maxPlausible,
                currentItemId);
            return true;
        }

        // First plausible queue_time_updated confirms the new track timing model.
        lock (_progressSync)
        {
            _transitionGuardQueueItemId = null;
            _transitionGuardStartedUtc = null;
            _transitionGuardStartElapsedSeconds = null;
            _transitionGuardUntilUtc = null;
        }

        return false;
    }

    private bool ShouldIgnoreTransitionQueuePayloadProgress(string? currentItemId, double candidateSeconds)
    {
        string? guardItemId;
        DateTimeOffset? guardStartedUtc;
        double? guardStartElapsedSeconds;
        DateTimeOffset? guardUntilUtc;

        lock (_progressSync)
        {
            guardItemId = _transitionGuardQueueItemId;
            guardStartedUtc = _transitionGuardStartedUtc;
            guardStartElapsedSeconds = _transitionGuardStartElapsedSeconds;
            guardUntilUtc = _transitionGuardUntilUtc;
        }

        if (string.IsNullOrWhiteSpace(guardItemId)
            || guardStartedUtc is not DateTimeOffset startedUtc
            || guardStartElapsedSeconds is not double startElapsedSeconds
            || guardUntilUtc is not DateTimeOffset untilUtc)
        {
            return false;
        }

        if (!string.Equals(guardItemId, currentItemId, StringComparison.Ordinal))
        {
            lock (_progressSync)
            {
                _transitionGuardQueueItemId = null;
                _transitionGuardStartedUtc = null;
                _transitionGuardStartElapsedSeconds = null;
                _transitionGuardUntilUtc = null;
            }

            return false;
        }

        var now = DateTimeOffset.UtcNow;
        if (now >= untilUtc)
        {
            lock (_progressSync)
            {
                _transitionGuardQueueItemId = null;
                _transitionGuardStartedUtc = null;
                _transitionGuardStartElapsedSeconds = null;
                _transitionGuardUntilUtc = null;
            }

            return false;
        }

        const double jitterSeconds = 1.0;
        var maxPlausible = Math.Max(0, startElapsedSeconds + (now - startedUtc).TotalSeconds + jitterSeconds);
        if (candidateSeconds > maxPlausible)
        {
            _logger.LogDebug(
                "Ignored queue payload transition outlier. Candidate={Candidate}, MaxPlausible={MaxPlausible}, ItemId={ItemId}",
                candidateSeconds,
                maxPlausible,
                currentItemId);
        }

        // While transition guard is active, queue_time_updated is authoritative for progress.
        return true;
    }

    private static PlaybackStateKind MapState(mashin.Models.PlaybackState? state)
    {
        return state switch
        {
            mashin.Models.PlaybackState.Playing => PlaybackStateKind.Playing,
            mashin.Models.PlaybackState.Paused => PlaybackStateKind.Paused,
            mashin.Models.PlaybackState.Buffering => PlaybackStateKind.Buffering,
            mashin.Models.PlaybackState.Idle => PlaybackStateKind.Idle,
            _ => PlaybackStateKind.Unknown
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
