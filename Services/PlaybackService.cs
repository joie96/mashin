using mashin.Models;
using mashin.Collections;
using Microsoft.Extensions.Logging;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Threading.Channels;

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
    PlaybackStateModel PlaybackState { get; set; }
    int Volume { get; }
    bool IsMuted { get; }
    bool? ShuffleEnabled { get; }
    string? RepeatMode { get; }
    bool? DontStopTheMusicEnabled { get; }
    int? CurrentQueueIndex { get; }
    int QueueItemCount { get; }
    double DurationSeconds { get; }
    double PositionSeconds { get; }

    Track? CurrentTrack { get; }
    ObservableRangeCollection<QueueItem> CurrentQueueItems { get; }

    Task InitializeAsync(CancellationToken cancellationToken = default);
    Task SetOutputModeAsync(PlaybackOutputMode mode, string? targetPlayerId = null, CancellationToken cancellationToken = default);
    Task PlayMediaAsync(IReadOnlyList<MediaItem> items);
    Task PlayMediaNextAsync(IReadOnlyList<MediaItem> items);
    Task PlayMediaLastAsync(IReadOnlyList<MediaItem> items);
    Task ShufflePlayMediaAsync(IReadOnlyList<MediaItem> items);
    Task ClearQueueAsync(bool skipStop = false);

    Task TogglePlayPauseAsync(CancellationToken cancellationToken = default);
    Task NextTrackAsync(CancellationToken cancellationToken = default);
    Task PreviousTrackAsync(CancellationToken cancellationToken = default);
    Task SeekAsync(double seconds, double durationSeconds, CancellationToken cancellationToken = default);
    Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default);
    Task ToggleMuteAsync(bool currentMuted, CancellationToken cancellationToken = default);
    Task ToggleShuffleAsync(bool? currentShuffleEnabled, CancellationToken cancellationToken = default);
    Task ToggleRepeatModeAsync(string? currentRepeatMode, CancellationToken cancellationToken = default);
    Task SetDontStopTheMusicAsync(bool enabled, CancellationToken cancellationToken = default);
    Task PlayQueueIndexAsync(int index, CancellationToken cancellationToken = default);
    Task MoveQueueItemAsync(string queueItemId, int posShift, CancellationToken cancellationToken = default);
    Task DeleteQueueItemAsync(string queueItemId, CancellationToken cancellationToken = default);
}

#endregion

public sealed class PlaybackService : IPlaybackService
{
    private readonly record struct PlayerStreamEvent(QueueItem? PlayingItem);

    #region Fields

    private readonly MusicAssistantService _musicAssistant;
    private readonly SettingsService _settingsService;
    private readonly ILogger<PlaybackService> _logger;
    private readonly Dictionary<PlayerMode, IPlayerService> _players;

    private readonly ObservableRangeCollection<QueueItem> _currentQueueItems = new();

    private PlaybackOutputMode _outputMode = PlaybackOutputMode.LocalSendspin;

    private string? _activePlayerId;
    private IPlayerService _activePlayer;

    private PlaybackStateModel _playbackState = new(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow);
    private int _volume = 50;
    private bool _isMuted;
    private bool? _shuffleEnabled;
    private string? _repeatMode;
    private bool? _dontStopTheMusicEnabled;
    private int? _currentQueueIndex;
    private int _queueItemCount;
    private double _durationSeconds;
    private double _positionSeconds;
    private Track? _currentTrack;

    private bool _suppressLocalQueueEvents;
    private string? _lastPlayerPlayingItemId;
    private readonly SemaphoreSlim _maQueueWindowUpdateGate = new(1, 1);
    private CancellationTokenSource? _maQueueWindowUpdateCts;
    private int _maQueueWindowUpdateRequestId;

    private readonly Channel<PlayerStreamEvent> _playerEventChannel = Channel.CreateUnbounded<PlayerStreamEvent>();
    private readonly CancellationTokenSource _playbackEventLoopCts = new();
    private readonly Task _playbackEventLoopTask;

    #endregion

    #region Construction

    public PlaybackService(
        MusicAssistantService musicAssistant,
        SettingsService settingsService,
        IEnumerable<IPlayerService> playerServices,
        ILogger<PlaybackService> logger)
    {
        _musicAssistant = musicAssistant;
        _settingsService = settingsService;
        _logger = logger;

        _players = playerServices
            .GroupBy(service => service.Mode)
            .ToDictionary(group => group.Key, group => group.First());

        _activePlayer = _players.TryGetValue(PlayerMode.Sendspin, out var sendspin)
            ? sendspin
            : _players.Values.First();

        _activePlayer.PropertyChanged += OnActivePlayerPropertyChanged;
        _activePlayer.RuntimeEvent += OnActivePlayerRuntimeEvent;
        _currentQueueItems.CollectionChanged += OnLocalQueueCollectionChanged;

        _playbackEventLoopTask = Task.Run(() => ProcessPlaybackEventLoopAsync(_playbackEventLoopCts.Token));
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

    public PlaybackStateModel PlaybackState
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
        private set
        {
            if (!SetProperty(ref _currentQueueIndex, value))
            {
                return;
            }

            UpdateCurrentTrackFromLocalQueueState();
            if (!_suppressLocalQueueEvents)
            {
                RequestLocalToMaQueueWindowUpdate();
            }
        }
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

    public Track? CurrentTrack
    {
        get => _currentTrack;
        private set => SetProperty(ref _currentTrack, value);
    }

    public ObservableRangeCollection<QueueItem> CurrentQueueItems => _currentQueueItems;

    #endregion

    #region Playback Lifecycle

    public async Task InitializeAsync(CancellationToken cancellationToken = default)
    {
        await SetOutputModeAsync(OutputMode, cancellationToken: cancellationToken);
    }

    public async Task SetOutputModeAsync(PlaybackOutputMode mode, string? targetPlayerId = null, CancellationToken cancellationToken = default)
    {
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
            return;
        }

        var nextPlayer = mode switch
        {
            PlaybackOutputMode.LocalOffline => _players[PlayerMode.LocalDummy],
            PlaybackOutputMode.RemoteOnly => _players[PlayerMode.Remote],
            _ => _players[PlayerMode.Sendspin]
        };

        if (!ReferenceEquals(_activePlayer, nextPlayer))
        {
            _activePlayer.PropertyChanged -= OnActivePlayerPropertyChanged;
            _activePlayer.RuntimeEvent -= OnActivePlayerRuntimeEvent;
            await _activePlayer.DeactivateAsync();
            _activePlayer = nextPlayer;
            _activePlayer.PropertyChanged += OnActivePlayerPropertyChanged;
            _activePlayer.RuntimeEvent += OnActivePlayerRuntimeEvent;
        }

        OutputMode = mode;

        await _activePlayer.ActivateAsync(resolvedTargetPlayerId, cancellationToken);
        ActivePlayerId = resolvedTargetPlayerId;

        SyncStateFromPlayer();

        if (OutputMode == PlaybackOutputMode.RemoteOnly)
        {
            await HydrateRemoteQueueAsync(cancellationToken);
        }

        RequestLocalToMaQueueWindowUpdate();
    }

    #endregion

    #region Remote Queue Hydration

    private async Task HydrateRemoteQueueAsync(CancellationToken cancellationToken = default)
    {
        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            return;
        }

        var activePlayerId = ActivePlayerId;
        if (string.IsNullOrWhiteSpace(activePlayerId))
        {
            return;
        }

        var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(activePlayerId);

        _suppressLocalQueueEvents = true;
        try
        {
            if (queue == null)
            {
                _currentQueueItems.ReplaceRange(Array.Empty<QueueItem>());
                CurrentTrack = null;
                CurrentQueueIndex = null;
                QueueItemCount = 0;
                DurationSeconds = 0;
                PositionSeconds = 0;
                PlaybackState = new PlaybackStateModel(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow);
                return;
            }

            CurrentTrack = queue.CurrentItem?.MediaItem;
            CurrentQueueIndex = queue.CurrentIndex;
            QueueItemCount = queue.ItemCount;
            ShuffleEnabled = queue.ShuffleEnabled;
            RepeatMode = queue.RepeatMode?.ToString();
            DontStopTheMusicEnabled = queue.DontStopTheMusicEnabled;
            DurationSeconds = Math.Max(0, queue.CurrentItem?.Duration ?? 0);
            PositionSeconds = Math.Max(0, queue.ElapsedTime ?? 0);
            PlaybackState = new PlaybackStateModel(MapState(queue.State), DateTimeOffset.UtcNow);

            var queueItems = await _musicAssistant.GetQueueItemsAsync(queue.QueueId);
            _currentQueueItems.ReplaceRange(queueItems);
        }
        finally
        {
            _suppressLocalQueueEvents = false;
        }
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

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            _logger.LogWarning("No playable tracks resolved for PlayMedia");
            return;
        }

        PlaybackState = new PlaybackStateModel(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);

        ApplyLocalQueueReplace(resolvedItems);
    }

    public async Task PlayMediaNextAsync(IReadOnlyList<MediaItem> items)
    {
        if (items == null || items.Count == 0)
        {
            _logger.LogWarning("No items selected to play");
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            _logger.LogWarning("No playable tracks resolved for PlayMediaNext");
            return;
        }

        PlaybackState = new PlaybackStateModel(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);

        ApplyLocalQueueInsert(resolvedItems, CurrentQueueIndex);
    }

    public async Task PlayMediaLastAsync(IReadOnlyList<MediaItem> items)
    {
        if (items == null || items.Count == 0)
        {
            _logger.LogWarning("No items selected to play");
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            _logger.LogWarning("No playable tracks resolved for PlayMediaLast");
            return;
        }

        PlaybackState = new PlaybackStateModel(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);

        ApplyLocalQueueAppendLast(resolvedItems);
    }

    public async Task ShufflePlayMediaAsync(IReadOnlyList<MediaItem> items)
    {
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

        ApplyLocalQueueReplace(mediaItems);
    }

    public Task ClearQueueAsync(bool skipStop = false)
    {
        ApplyLocalQueueClear();
        return Task.CompletedTask;
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

        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            ShiftLocalCurrentQueueIndex(1);
        }
    }

    public async Task PreviousTrackAsync(CancellationToken cancellationToken = default)
    {
        await _activePlayer.PreviousAsync(cancellationToken);

        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            ShiftLocalCurrentQueueIndex(-1);
        }
    }

    public async Task SeekAsync(double seconds, double durationSeconds, CancellationToken cancellationToken = default)
    {
        var clamped = (int)Math.Round(Math.Clamp(seconds, 0, Math.Max(0, durationSeconds)));
        await _activePlayer.SeekAsync(clamped, cancellationToken);
        PositionSeconds = clamped;
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
        DontStopTheMusicEnabled = enabled;
        await _activePlayer.SetDontStopTheMusicAsync(enabled, cancellationToken);
    }

    public async Task PlayQueueIndexAsync(int index, CancellationToken cancellationToken = default)
    {
        if (index < 0)
        {
            return;
        }

        if (_currentQueueItems.Count == 0)
        {
            CurrentQueueIndex = null;
            CurrentTrack = null;
            return;
        }

        var clampedIndex = Math.Clamp(index, 0, _currentQueueItems.Count - 1);
        _suppressLocalQueueEvents = true;
        try
        {
            CurrentQueueIndex = clampedIndex;
            CurrentTrack = _currentQueueItems[clampedIndex].MediaItem;
        }
        finally
        {
            _suppressLocalQueueEvents = false;
        }

        await _activePlayer.PlayQueueIndexAsync(clampedIndex, cancellationToken);
        RequestLocalToMaQueueWindowUpdate();
    }

    public async Task MoveQueueItemAsync(string queueItemId, int posShift, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(queueItemId))
        {
            return;
        }

        _suppressLocalQueueEvents = true;
        try
        {
            var sourceIndex = _currentQueueItems
                .Select((queueItem, idx) => (queueItem, idx))
                .FirstOrDefault(tuple => string.Equals(tuple.queueItem.QueueItemId, queueItemId, StringComparison.Ordinal))
                .idx;

            if (sourceIndex < 0 || sourceIndex >= _currentQueueItems.Count)
            {
                return;
            }

            var targetIndex = Math.Clamp(sourceIndex + posShift, 0, _currentQueueItems.Count - 1);
            if (targetIndex == sourceIndex)
            {
                return;
            }

            var movedItem = _currentQueueItems[sourceIndex];
            _currentQueueItems.RemoveAt(sourceIndex);
            _currentQueueItems.Insert(targetIndex, movedItem);

            if (CurrentQueueIndex is int currentIndex)
            {
                if (currentIndex == sourceIndex)
                {
                    CurrentQueueIndex = targetIndex;
                }
                else if (sourceIndex < currentIndex && targetIndex >= currentIndex)
                {
                    CurrentQueueIndex = currentIndex - 1;
                }
                else if (sourceIndex > currentIndex && targetIndex <= currentIndex)
                {
                    CurrentQueueIndex = currentIndex + 1;
                }
            }

            QueueItemCount = _currentQueueItems.Count;

            if (CurrentQueueIndex is int updatedIndex
                && updatedIndex >= 0
                && updatedIndex < _currentQueueItems.Count)
            {
                CurrentTrack = _currentQueueItems[updatedIndex].MediaItem;
            }
        }
        finally
        {
            _suppressLocalQueueEvents = false;
        }

        await _activePlayer.MoveQueueItemAsync(queueItemId, posShift, cancellationToken);
        RequestLocalToMaQueueWindowUpdate();
    }

    public async Task DeleteQueueItemAsync(string queueItemId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(queueItemId))
        {
            return;
        }

        _suppressLocalQueueEvents = true;
        try
        {
            var removeIndex = _currentQueueItems
                .Select((queueItem, idx) => (queueItem, idx))
                .FirstOrDefault(tuple => string.Equals(tuple.queueItem.QueueItemId, queueItemId, StringComparison.Ordinal))
                .idx;

            if (removeIndex < 0 || removeIndex >= _currentQueueItems.Count)
            {
                return;
            }

            _currentQueueItems.RemoveAt(removeIndex);
            QueueItemCount = _currentQueueItems.Count;

            if (_currentQueueItems.Count == 0)
            {
                CurrentQueueIndex = null;
                CurrentTrack = null;
                return;
            }

            if (CurrentQueueIndex is int currentIndex)
            {
                if (removeIndex < currentIndex)
                {
                    CurrentQueueIndex = currentIndex - 1;
                }
                else if (removeIndex == currentIndex)
                {
                    CurrentQueueIndex = Math.Min(currentIndex, _currentQueueItems.Count - 1);
                }
            }
            else
            {
                CurrentQueueIndex = 0;
            }

            if (CurrentQueueIndex is int updatedIndex
                && updatedIndex >= 0
                && updatedIndex < _currentQueueItems.Count)
            {
                CurrentTrack = _currentQueueItems[updatedIndex].MediaItem;
            }
        }
        finally
        {
            _suppressLocalQueueEvents = false;
        }

        await _activePlayer.DeleteQueueItemAsync(queueItemId, cancellationToken);
        RequestLocalToMaQueueWindowUpdate();
    }

    #endregion

    #region Disposal

    public async ValueTask DisposeAsync()
    {
        _currentQueueItems.CollectionChanged -= OnLocalQueueCollectionChanged;
        _activePlayer.RuntimeEvent -= OnActivePlayerRuntimeEvent;

        _playbackEventLoopCts.Cancel();
        try
        {
            await _playbackEventLoopTask;
        }
        catch (OperationCanceledException)
        {
            // Expected during shutdown.
        }
        finally
        {
            _playbackEventLoopCts.Dispose();
        }

        var cts = Interlocked.Exchange(ref _maQueueWindowUpdateCts, null);
        cts?.Cancel();
        cts?.Dispose();

        _maQueueWindowUpdateGate.Dispose();

        foreach (var player in _players.Values.Distinct())
        {
            await player.DisposeAsync();
        }

    }

    #endregion

    #region Property Change Handling

    private void OnLocalQueueCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        if (_suppressLocalQueueEvents)
        {
            return;
        }

        QueueItemCount = _currentQueueItems.Count;
        UpdateCurrentTrackFromLocalQueueState();
        RequestLocalToMaQueueWindowUpdate();
    }

    private void OnActivePlayerRuntimeEvent(object? sender, PlayerRuntimeEventArgs e)
    {
        switch (e.Kind)
        {
            case PlayerRuntimeEventKind.PlayingItemChanged:
                EnqueuePlayerEvent(e.PlayingItem);
                break;
        }
    }

    private void OnActivePlayerPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        SyncStateFromPlayer();
    }

    private void SyncStateFromPlayer()
    {
        Volume = _activePlayer.Volume;
        IsMuted = _activePlayer.IsMuted;
        DurationSeconds = _activePlayer.DurationSeconds > 0 ? _activePlayer.DurationSeconds : DurationSeconds;
        PositionSeconds = _activePlayer.PositionSeconds > 0 ? _activePlayer.PositionSeconds : PositionSeconds;

        if (PlaybackState.State == PlayerPlaybackState.Unknown)
        {
            PlaybackState = _activePlayer.PlayerState;
        }
    }

    #endregion

    #region Event Stream

    private void EnqueuePlayerEvent(QueueItem? playingItem)
    {
        _playerEventChannel.Writer.TryWrite(new PlayerStreamEvent(playingItem));
    }

    private async Task ProcessPlaybackEventLoopAsync(CancellationToken cancellationToken)
    {
        while (await _playerEventChannel.Reader.WaitToReadAsync(cancellationToken))
        {
            PlayerStreamEvent latestEvent = default;
            var hasEvent = false;

            while (_playerEventChannel.Reader.TryRead(out var queuedEvent))
            {
                latestEvent = queuedEvent;
                hasEvent = true;
            }

            if (!hasEvent)
            {
                continue;
            }

            if (latestEvent.PlayingItem != null)
            {
                await SyncLocalCurrentTrackFromPlayerPlayingItemAsync(latestEvent.PlayingItem);
            }
        }
    }

    private async Task SyncLocalCurrentTrackFromPlayerPlayingItemAsync(QueueItem playingItem)
    {
        if (_currentQueueItems.Count == 0 || string.IsNullOrWhiteSpace(playingItem.QueueItemId))
        {
            _logger.LogDebug("Skipping playing item sync: local queue empty or missing playing item id.");
            return;
        }

        var matchedIndex = _currentQueueItems
            .Select((queueItem, index) => (queueItem, index))
            .Where(tuple => !string.IsNullOrWhiteSpace(tuple.queueItem.QueueItemId))
            .Where(tuple => string.Equals(tuple.queueItem.QueueItemId, playingItem.QueueItemId, StringComparison.Ordinal))
            .Select(tuple => tuple.index)
            .DefaultIfEmpty(-1)
            .First();

        if (matchedIndex < 0)
        {
            _logger.LogDebug(
                "Playing item {PlayingItemId} not found in local queue. Hydrating from MA.",
                playingItem.QueueItemId);
            await HydrateRemoteQueueAsync();
            return;
        }

        _suppressLocalQueueEvents = true;
        try
        {
            CurrentQueueIndex = matchedIndex;
            CurrentTrack = _currentQueueItems[matchedIndex].MediaItem;
        }
        finally
        {
            _suppressLocalQueueEvents = false;
        }

        _lastPlayerPlayingItemId = playingItem.QueueItemId;
        _logger.LogDebug(
            "Synced local current index from playing item. QueueItemId={QueueItemId}, MatchedIndex={MatchedIndex}",
            playingItem.QueueItemId,
            matchedIndex);
        RequestLocalToMaQueueWindowUpdate();
    }

    #endregion

    #region Local Queue To MA Queue Window

    private void RequestLocalToMaQueueWindowUpdate()
    {
        if (_suppressLocalQueueEvents)
        {
            return;
        }

        var requestId = Interlocked.Increment(ref _maQueueWindowUpdateRequestId);
        var nextCts = new CancellationTokenSource();
        var previousCts = Interlocked.Exchange(ref _maQueueWindowUpdateCts, nextCts);

        previousCts?.Cancel();
        previousCts?.Dispose();

        _logger.LogDebug("Scheduling MA queue window update. RequestId={RequestId}", requestId);

        _ = ApplyLocalQueueWindowToMaAsync(requestId, nextCts.Token);
    }

    private async Task ApplyLocalQueueWindowToMaAsync(int requestId, CancellationToken cancellationToken)
    {
        // In LocalOffline mode, local queue state is not projected to MA.
        if (OutputMode == PlaybackOutputMode.LocalOffline)
        {
            return;
        }

        var activePlayerId = ActivePlayerId;
        if (string.IsNullOrWhiteSpace(activePlayerId))
        {
            return;
        }

        try
        {
            await _maQueueWindowUpdateGate.WaitAsync(cancellationToken);
        }
        catch (OperationCanceledException)
        {
            return;
        }

        try
        {
            // LWW: discard stale requests early.
            if (cancellationToken.IsCancellationRequested)
            {
                return;
            }

            if (requestId != Volatile.Read(ref _maQueueWindowUpdateRequestId))
            {
                return;
            }

            // Build the current 2-item window (Current + Next) from the local queue.
            var queueWindowItems = new List<QueueItem>(2);

            if (_currentQueueItems.Count > 0)
            {
                var startIndex = CurrentQueueIndex ?? 0;
                startIndex = Math.Clamp(startIndex, 0, _currentQueueItems.Count - 1);

                for (var i = startIndex; i < _currentQueueItems.Count && queueWindowItems.Count < 2; i++)
                {
                    var queueItem = _currentQueueItems[i];
                    if (queueItem.MediaItem != null)
                    {
                        queueWindowItems.Add(queueItem);
                    }
                }
            }

            if (queueWindowItems.Count == 0)
            {
                // Empty window means the MA queue should be cleared.
                _logger.LogDebug("Clearing MA queue because local queue window is empty.");
                await _musicAssistant.ClearQueueAsync(activePlayerId, skipStop: false);
                return;
            }

            var tracks = queueWindowItems
                .Select(item => item.MediaItem)
                .Where(track => track != null)
                .Cast<MediaItem>()
                .ToList();

            if (tracks.Count == 0)
            {
                // If there are no playable media items, clear the MA queue as well.
                _logger.LogDebug("Clearing MA queue because local queue window contains no playable tracks.");
                await _musicAssistant.ClearQueueAsync(activePlayerId, skipStop: false);
                return;
            }

            var currentWindowItem = queueWindowItems[0];
            var currentMatchesPlayerPlayingItem =
                !string.IsNullOrWhiteSpace(_lastPlayerPlayingItemId)
                && !string.IsNullOrWhiteSpace(currentWindowItem.QueueItemId)
                && string.Equals(currentWindowItem.QueueItemId, _lastPlayerPlayingItemId, StringComparison.Ordinal);

            if (currentMatchesPlayerPlayingItem)
            {
                // Happy path: current item already plays on MA, push only the next local item.
                if (queueWindowItems.Count < 2 || queueWindowItems[1].MediaItem is not MediaItem nextTrack)
                {
                    _logger.LogDebug("Skipping MA next update: no local next track available while current item is already synced.");
                    return;
                }

                _logger.LogDebug(
                    "Applying MA next update. CurrentQueueItemId={CurrentQueueItemId}, NextTrackUri={NextTrackUri}",
                    currentWindowItem.QueueItemId,
                    nextTrack.Uri);

                await _musicAssistant.PlayMediaAsync(
                    activePlayerId,
                    new List<MediaItem> { nextTrack },
                    QueueOption.Next);

                if (cancellationToken.IsCancellationRequested || requestId != Volatile.Read(ref _maQueueWindowUpdateRequestId))
                {
                    return;
                }

                // Read back only the newly appended next item id and map it locally.
                var activeQueueAfterNext = await _musicAssistant.GetActiveQueueForPlayerAsync(activePlayerId);
                if (activeQueueAfterNext == null || string.IsNullOrWhiteSpace(activeQueueAfterNext.QueueId))
                {
                    return;
                }

                var remoteItemsAfterNext = await _musicAssistant.GetQueueItemsAsync(activeQueueAfterNext.QueueId);
                if (remoteItemsAfterNext.Count == 0 || cancellationToken.IsCancellationRequested)
                {
                    return;
                }

                var remoteNextIndex = (activeQueueAfterNext.CurrentIndex ?? 0) + 1;
                remoteNextIndex = Math.Clamp(remoteNextIndex, 0, Math.Max(0, remoteItemsAfterNext.Count - 1));

                var remoteNextItem = remoteItemsAfterNext[remoteNextIndex];
                if (!string.IsNullOrWhiteSpace(remoteNextItem.QueueItemId))
                {
                    queueWindowItems[1].QueueItemId = remoteNextItem.QueueItemId;
                    _logger.LogDebug(
                        "Mapped MA next queue item id back to local item. NewQueueItemId={QueueItemId}",
                        remoteNextItem.QueueItemId);
                }

                return;
            }

            _logger.LogDebug(
                "Applying MA hard replace. LocalCurrentQueueItemId={LocalCurrentQueueItemId}, LastPlayerPlayingItemId={LastPlayerPlayingItemId}, TrackCount={TrackCount}",
                currentWindowItem.QueueItemId,
                _lastPlayerPlayingItemId,
                tracks.Count);

            // Hard-apply this window to MA using replace.
            await _musicAssistant.PlayMediaAsync(
                activePlayerId,
                tracks,
                QueueOption.Replace);

            if (cancellationToken.IsCancellationRequested || requestId != Volatile.Read(ref _maQueueWindowUpdateRequestId))
            {
                return;
            }

            // After write, read back MA queue_item_id values and map them onto the local window.
            var activeQueue = await _musicAssistant.GetActiveQueueForPlayerAsync(activePlayerId);
            if (activeQueue == null || string.IsNullOrWhiteSpace(activeQueue.QueueId))
            {
                return;
            }

            var remoteItems = await _musicAssistant.GetQueueItemsAsync(activeQueue.QueueId);
            if (remoteItems.Count == 0 || cancellationToken.IsCancellationRequested)
            {
                return;
            }

            var remoteStartIndex = activeQueue.CurrentIndex ?? 0;
            remoteStartIndex = Math.Clamp(remoteStartIndex, 0, Math.Max(0, remoteItems.Count - 1));

            var remoteWindow = remoteItems
                .Skip(remoteStartIndex)
                .Take(queueWindowItems.Count)
                .ToList();

            var mapCount = Math.Min(queueWindowItems.Count, remoteWindow.Count);
            for (var i = 0; i < mapCount; i++)
            {
                if (!string.IsNullOrWhiteSpace(remoteWindow[i].QueueItemId))
                {
                    queueWindowItems[i].QueueItemId = remoteWindow[i].QueueItemId;
                }
            }
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to update MA queue window from local queue for player {PlayerId}", activePlayerId);
        }
        finally
        {
            _maQueueWindowUpdateGate.Release();
        }
    }

    #endregion

    #region Local Queue Handling

    private void UpdateCurrentTrackFromLocalQueueState()
    {
        if (_currentQueueItems.Count == 0)
        {
            CurrentTrack = null;
            if (CurrentQueueIndex != null)
            {
                _suppressLocalQueueEvents = true;
                try
                {
                    CurrentQueueIndex = null;
                }
                finally
                {
                    _suppressLocalQueueEvents = false;
                }
            }

            return;
        }

        var targetIndex = CurrentQueueIndex ?? 0;
        targetIndex = Math.Clamp(targetIndex, 0, _currentQueueItems.Count - 1);

        if (CurrentQueueIndex != targetIndex)
        {
            _suppressLocalQueueEvents = true;
            try
            {
                CurrentQueueIndex = targetIndex;
            }
            finally
            {
                _suppressLocalQueueEvents = false;
            }
        }

        CurrentTrack = _currentQueueItems[targetIndex].MediaItem;
    }

    private void ApplyLocalQueueReplace(IReadOnlyList<MediaItem> items)
    {
        _suppressLocalQueueEvents = true;
        try
        {
            var queueItems = BuildLocalQueueItems(items);
            _currentQueueItems.ReplaceRange(queueItems);
            QueueItemCount = _currentQueueItems.Count;

            if (_currentQueueItems.Count == 0)
            {
                CurrentQueueIndex = null;
                CurrentTrack = null;
                return;
            }

            CurrentQueueIndex = 0;
            CurrentTrack = _currentQueueItems[0].MediaItem;
        }
        finally
        {
            _suppressLocalQueueEvents = false;
        }

        RequestLocalToMaQueueWindowUpdate();
    }

    private void ApplyLocalQueueInsert(IReadOnlyList<MediaItem> items, int? insertAfterIndex)
    {
        _suppressLocalQueueEvents = true;
        try
        {
            var queueItems = BuildLocalQueueItems(items);
            if (queueItems.Count == 0)
            {
                return;
            }

            var insertIndex = insertAfterIndex is int positionIndex
                ? Math.Clamp(positionIndex + 1, 0, _currentQueueItems.Count)
                : _currentQueueItems.Count;

            for (var i = 0; i < queueItems.Count; i++)
            {
                _currentQueueItems.Insert(insertIndex + i, queueItems[i]);
            }

            QueueItemCount = _currentQueueItems.Count;

            if (CurrentQueueIndex == null && _currentQueueItems.Count > 0)
            {
                CurrentQueueIndex = 0;
                CurrentTrack = _currentQueueItems[0].MediaItem;
            }
        }
        finally
        {
            _suppressLocalQueueEvents = false;
        }

        RequestLocalToMaQueueWindowUpdate();
    }

    private void ApplyLocalQueueAppendLast(IReadOnlyList<MediaItem> items)
    {
        _suppressLocalQueueEvents = true;
        try
        {
            var queueItems = BuildLocalQueueItems(items);
            if (queueItems.Count == 0)
            {
                return;
            }

            foreach (var queueItem in queueItems)
            {
                _currentQueueItems.Add(queueItem);
            }

            QueueItemCount = _currentQueueItems.Count;

            if (CurrentQueueIndex == null && _currentQueueItems.Count > 0)
            {
                CurrentQueueIndex = 0;
                CurrentTrack = _currentQueueItems[0].MediaItem;
            }
        }
        finally
        {
            _suppressLocalQueueEvents = false;
        }

        RequestLocalToMaQueueWindowUpdate();
    }

    private void ApplyLocalQueueClear()
    {
        _suppressLocalQueueEvents = true;
        try
        {
            _currentQueueItems.ReplaceRange(Array.Empty<QueueItem>());
            CurrentQueueIndex = null;
            CurrentTrack = null;
            QueueItemCount = 0;
            DurationSeconds = 0;
            PositionSeconds = 0;
        }
        finally
        {
            _suppressLocalQueueEvents = false;
        }

        RequestLocalToMaQueueWindowUpdate();
    }

    private static List<QueueItem> BuildLocalQueueItems(IReadOnlyList<MediaItem> items)
    {
        var result = new List<QueueItem>(items.Count);

        foreach (var mediaItem in items)
        {
            if (mediaItem == null)
            {
                continue;
            }

            var track = mediaItem as Track;
            result.Add(new QueueItem
            {
                QueueItemId = $"local-{Guid.NewGuid():N}",
                Name = mediaItem.Name,
                Duration = track?.Duration,
                MediaItem = track
            });
        }

        return result;
    }

    private void ShiftLocalCurrentQueueIndex(int delta)
    {
        if (_currentQueueItems.Count == 0)
        {
            CurrentQueueIndex = null;
            CurrentTrack = null;
            return;
        }

        var currentIndex = CurrentQueueIndex ?? 0;
        var nextIndex = Math.Clamp(currentIndex + delta, 0, _currentQueueItems.Count - 1);

        _suppressLocalQueueEvents = true;
        try
        {
            CurrentQueueIndex = nextIndex;
            CurrentTrack = _currentQueueItems[nextIndex].MediaItem;
        }
        finally
        {
            _suppressLocalQueueEvents = false;
        }

        RequestLocalToMaQueueWindowUpdate();
    }

    #endregion

    #region Helpers

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

    private static PlayerPlaybackState MapState(PlaybackState? state)
    {
        return state switch
        {
            Models.PlaybackState.Playing => PlayerPlaybackState.Playing,
            Models.PlaybackState.Paused => PlayerPlaybackState.Paused,
            Models.PlaybackState.Buffering => PlayerPlaybackState.Buffering,
            Models.PlaybackState.Idle => PlayerPlaybackState.Stopped,
            _ => PlayerPlaybackState.Unknown
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
        var mapping = item.ProviderMappings.FirstOrDefault();

        if (!string.IsNullOrWhiteSpace(mapping?.ProviderInstance))
        {
            return mapping.ProviderInstance;
        }

        if (!string.IsNullOrWhiteSpace(mapping?.ProviderDomain))
        {
            return mapping.ProviderDomain;
        }

        if (!string.IsNullOrWhiteSpace(item.Provider))
        {
            return item.Provider;
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
