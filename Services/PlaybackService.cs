using mashin.Models;
using Microsoft.Extensions.Logging;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Runtime.CompilerServices;

namespace mashin.Services;

#region ChangeSet Types

public enum QueueItemChangeType
{
    Insert,
    Remove,
    Move,
    Replace
}

public sealed class QueueItemChange
{
    public QueueItemChangeType ChangeType { get; }
    public int Index { get; }
    public int NewIndex { get; }
    public QueueItem? Item { get; }

    private QueueItemChange(QueueItemChangeType changeType, int index, int newIndex, QueueItem? item)
    {
        ChangeType = changeType;
        Index = index;
        NewIndex = newIndex;
        Item = item;
    }

    public static QueueItemChange Insert(int index, QueueItem item) => new(QueueItemChangeType.Insert, index, -1, item);
    public static QueueItemChange Remove(int index) => new(QueueItemChangeType.Remove, index, -1, null);
    public static QueueItemChange Move(int index, int newIndex) => new(QueueItemChangeType.Move, index, newIndex, null);
    public static QueueItemChange Replace(int index, QueueItem item) => new(QueueItemChangeType.Replace, index, -1, item);
}

public sealed class QueueItemsChangeSet
{
    public static QueueItemsChangeSet Empty { get; } = new(Array.Empty<QueueItemChange>());

    public IReadOnlyList<QueueItemChange> Changes { get; }

    public QueueItemsChangeSet(IReadOnlyList<QueueItemChange> changes)
    {
        Changes = changes;
    }
}

public sealed class QueueItemsChangedEventArgs : EventArgs
{
    public QueueItemsChangeSet ChangeSet { get; }

    public QueueItemsChangedEventArgs(QueueItemsChangeSet changeSet)
    {
        ChangeSet = changeSet;
    }
}

#endregion

#region Interface

public interface IPlaybackService : IAsyncDisposable, INotifyPropertyChanged
{
    event EventHandler? CurrentPlayerQueueUpdated;
    event EventHandler? CurrentTrackUpdated;
    event EventHandler<QueueItemsChangedEventArgs>? CurrentQueueItemsUpdated;

    string? ActivePlayerId { get; }
    string? ActiveQueueId { get; }
    PlayerPlayState PlaybackState { get; set; }

    PlayerQueue? CurrentPlayerQueue { get; }
    Track? CurrentTrack { get; }
    IReadOnlyList<QueueItem> CurrentQueueItems { get; }

    Task InitializeAsync(CancellationToken cancellationToken = default);
    Task SetActivePlayerAsync(string? playerId, CancellationToken cancellationToken = default);

    Task StartAsync(CancellationToken cancellationToken = default);
    Task StopAsync();
    Task RefreshNowAsync(CancellationToken cancellationToken = default);

    Task TogglePlayPauseAsync(CancellationToken cancellationToken = default);
    Task NextTrackAsync(CancellationToken cancellationToken = default);
    Task PreviousTrackAsync(CancellationToken cancellationToken = default);
    Task SeekAsync(double seconds, double durationSeconds, CancellationToken cancellationToken = default);
    void BeginSeek();
    Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default);
    Task ToggleMuteAsync(bool currentMuted, CancellationToken cancellationToken = default);
    Task ToggleShuffleAsync(bool? currentShuffleEnabled, CancellationToken cancellationToken = default);
    Task ToggleRepeatModeAsync(string? currentRepeatMode, CancellationToken cancellationToken = default);
}

#endregion

public sealed class PlaybackService : IPlaybackService
{
#region Fields

    private static readonly TimeSpan QueueRefreshInterval = TimeSpan.FromSeconds(15);
    private static readonly TimeSpan BufferingResolveMaxDuration = TimeSpan.FromSeconds(8);
    private static readonly TimeSpan BufferingResolveLocalInterval = TimeSpan.FromMilliseconds(140);
    private static readonly TimeSpan BufferingResolveRemoteInterval = TimeSpan.FromMilliseconds(380);

    private readonly MusicAssistantService _musicAssistant;
    private readonly IUserDataService _userDataService;
    private readonly ISendspinPlayerService _sendspinPlayerService;
    private readonly ILogger<PlaybackService> _logger;

    private readonly SemaphoreSlim _startStopLock = new(1, 1);
    private readonly SemaphoreSlim _refreshLock = new(1, 1);
    private readonly SemaphoreSlim _bufferingResolveLock = new(1, 1);

    private CancellationTokenSource? _loopCts;
    private Task? _loopTask;
    private CancellationTokenSource? _bufferingResolveCts;
    private Task? _bufferingResolveTask;

    private string? _activePlayerId;
    private string? _activeQueueId;
    private PlayerPlayState _playbackState = new(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow);
    private PlayerQueue? _currentPlayerQueue;
    private Track? _currentTrack;
    private readonly List<QueueItem> _currentQueueItems = new();

#endregion

#region Events

    public event PropertyChangedEventHandler? PropertyChanged;
    public event EventHandler? CurrentPlayerQueueUpdated;
    public event EventHandler? CurrentTrackUpdated;
    public event EventHandler<QueueItemsChangedEventArgs>? CurrentQueueItemsUpdated;

#endregion

#region Properties

    public string? ActivePlayerId
    {
        get => _activePlayerId;
        private set => SetProperty(ref _activePlayerId, NormalizeId(value));
    }

    public string? ActiveQueueId
    {
        get => _activeQueueId;
        private set => SetProperty(ref _activeQueueId, NormalizeId(value));
    }

    public PlayerPlayState PlaybackState
    {
        get => _playbackState;
        set
        {
            var normalizedTimestamp = value.TimestampUtc == default
                ? DateTimeOffset.UtcNow
                : value.TimestampUtc;
            var normalizedValue = value with { TimestampUtc = normalizedTimestamp };

            if (!SetProperty(ref _playbackState, normalizedValue))
            {
                return;
            }

            if (normalizedValue.State == PlayerPlaybackState.Buffering)
            {
                TriggerBufferingResolution();
                return;
            }

            _ = StopBufferingResolutionLoopAsync();
        }
    }

    public PlayerQueue? CurrentPlayerQueue
    {
        get => _currentPlayerQueue;
        private set => SetProperty(ref _currentPlayerQueue, value);
    }

    public Track? CurrentTrack
    {
        get => _currentTrack;
        private set => SetProperty(ref _currentTrack, value);
    }

    public IReadOnlyList<QueueItem> CurrentQueueItems => new ReadOnlyCollection<QueueItem>(_currentQueueItems);

#endregion

#region Construction

    public PlaybackService(
        MusicAssistantService musicAssistant,
        IUserDataService userDataService,
        ISendspinPlayerService sendspinPlayerService,
        ILogger<PlaybackService> logger)
    {
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _sendspinPlayerService = sendspinPlayerService;
        _logger = logger;

        _sendspinPlayerService.PropertyChanged += OnSendspinPlayerPropertyChanged;
        PlaybackState = _sendspinPlayerService.PlayState;
    }

#endregion

#region Lifecycle

    public async Task InitializeAsync(CancellationToken cancellationToken = default)
    {
        var initialPlayerId = ActivePlayerId ?? _sendspinPlayerService.PlayerId;
        await SetActivePlayerAsync(initialPlayerId, cancellationToken);
    }

    public async Task SetActivePlayerAsync(string? playerId, CancellationToken cancellationToken = default)
    {
        ActivePlayerId = playerId;
        await RefreshNowAsync(cancellationToken);
    }

    public async Task StartAsync(CancellationToken cancellationToken = default)
    {
        await _startStopLock.WaitAsync(cancellationToken);
        try
        {
            if (_loopTask != null)
            {
                return;
            }

            _loopCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            _loopTask = RunLoopAsync(_loopCts.Token);
        }
        finally
        {
            _startStopLock.Release();
        }

        await RefreshNowAsync(cancellationToken);
    }

    public async Task StopAsync()
    {
        await StopBufferingResolutionLoopAsync();
        await _startStopLock.WaitAsync();

        CancellationTokenSource? ctsToCancel = null;
        Task? loopTaskToAwait = null;

        try
        {
            if (_loopTask == null)
            {
                return;
            }

            ctsToCancel = _loopCts;
            loopTaskToAwait = _loopTask;

            _loopCts = null;
            _loopTask = null;
        }
        finally
        {
            _startStopLock.Release();
        }

        if (ctsToCancel != null)
        {
            await ctsToCancel.CancelAsync();
            ctsToCancel.Dispose();
        }

        if (loopTaskToAwait != null)
        {
            try
            {
                await loopTaskToAwait;
            }
            catch (OperationCanceledException)
            {
                // Expected on stop.
            }
        }
    }

    public async Task RefreshNowAsync(CancellationToken cancellationToken = default)
    {
        await _refreshLock.WaitAsync(cancellationToken);
        try
        {
            await RefreshStateCoreAsync(cancellationToken);
        }
        finally
        {
            _refreshLock.Release();
        }
    }

    public async ValueTask DisposeAsync()
    {
        await StopAsync();
        _sendspinPlayerService.PropertyChanged -= OnSendspinPlayerPropertyChanged;
        _startStopLock.Dispose();
        _refreshLock.Dispose();
        _bufferingResolveLock.Dispose();
    }

#endregion

#region Playback Commands

    public async Task TogglePlayPauseAsync(CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(ActiveQueueId))
        {
            _logger.LogWarning("No active queue available for play/pause");
            return;
        }

        await _musicAssistant.PlayPauseAsync(ActiveQueueId);
        PlaybackState = new PlayerPlayState(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);
    }

    public async Task NextTrackAsync(CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(ActiveQueueId))
        {
            _logger.LogWarning("No active queue available for next track");
            return;
        }

        await _musicAssistant.NextAsync(ActiveQueueId);
        PlaybackState = new PlayerPlayState(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);
    }

    public async Task PreviousTrackAsync(CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(ActiveQueueId))
        {
            _logger.LogWarning("No active queue available for previous track");
            return;
        }

        await _musicAssistant.PreviousAsync(ActiveQueueId);
        PlaybackState = new PlayerPlayState(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);
    }

    public async Task SeekAsync(double seconds, double durationSeconds, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(ActiveQueueId))
        {
            _logger.LogWarning("No active queue available for seek");
            return;
        }

        BeginSeek();

        try
        {
            var clamped = Math.Max(0, Math.Min(durationSeconds, seconds));
            if (IsLocalTarget())
            {
                _sendspinPlayerService.PositionSeconds = clamped;
            }

            await _musicAssistant.SeekAsync(ActiveQueueId, (int)Math.Round(clamped));
        }
        finally
        {
            PlaybackState = new PlayerPlayState(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);
        }
    }

    public void BeginSeek()
    {
        SetState(PlayerPlaybackState.Seeking);
    }

    public async Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            _logger.LogWarning("No active player available for volume update");
            return;
        }

        var clamped = Math.Clamp(volume, 0, 100);
        await _musicAssistant.SetPlayerVolumeAsync(ActivePlayerId, clamped);
    }

    public async Task ToggleMuteAsync(bool currentMuted, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(ActivePlayerId))
        {
            _logger.LogWarning("No active player available for mute toggle");
            return;
        }

        await _musicAssistant.SetPlayerMuteAsync(ActivePlayerId, !currentMuted);
    }

    public async Task ToggleShuffleAsync(bool? currentShuffleEnabled, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(ActiveQueueId))
        {
            _logger.LogWarning("No active queue available for shuffle toggle");
            return;
        }

        var nextShuffleEnabled = !(currentShuffleEnabled ?? false);
        await _musicAssistant.SetShuffleAsync(ActiveQueueId, nextShuffleEnabled);
        await RefreshNowAsync(cancellationToken);
    }

    public async Task ToggleRepeatModeAsync(string? currentRepeatMode, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(ActiveQueueId))
        {
            _logger.LogWarning("No active queue available for repeat toggle");
            return;
        }

        var nextRepeatMode = GetNextRepeatMode(currentRepeatMode);
        await _musicAssistant.SetRepeatAsync(ActiveQueueId, nextRepeatMode);
    }

#endregion

#region Refresh

#region Buffering Resolve

    private void TriggerBufferingResolution()
    {
        _ = RestartBufferingResolutionLoopAsync();
    }

    private async Task RestartBufferingResolutionLoopAsync()
    {
        await _bufferingResolveLock.WaitAsync();
        try
        {
            if (PlaybackState.State != PlayerPlaybackState.Buffering)
            {
                return;
            }

            var previousCts = _bufferingResolveCts;
            var previousTask = _bufferingResolveTask;

            _bufferingResolveCts = new CancellationTokenSource();
            _bufferingResolveTask = RunBufferingResolutionLoopAsync(_bufferingResolveCts.Token);

            if (previousCts != null)
            {
                try
                {
                    await previousCts.CancelAsync();
                    if (previousTask != null)
                    {
                        await previousTask;
                    }
                }
                catch (OperationCanceledException)
                {
                    // Expected when switching buffering resolve loops.
                }
                finally
                {
                    previousCts.Dispose();
                }
            }
        }
        finally
        {
            _bufferingResolveLock.Release();
        }
    }

    private async Task StopBufferingResolutionLoopAsync()
    {
        await _bufferingResolveLock.WaitAsync();
        try
        {
            var cts = _bufferingResolveCts;
            var task = _bufferingResolveTask;

            _bufferingResolveCts = null;
            _bufferingResolveTask = null;

            if (cts == null)
            {
                return;
            }

            try
            {
                await cts.CancelAsync();
                if (task != null)
                {
                    await task;
                }
            }
            catch (OperationCanceledException)
            {
                // Expected while stopping.
            }
            finally
            {
                cts.Dispose();
            }
        }
        finally
        {
            _bufferingResolveLock.Release();
        }
    }

    private async Task RunBufferingResolutionLoopAsync(CancellationToken cancellationToken)
    {
        try
        {
            var startUtc = DateTimeOffset.UtcNow;

            while (!cancellationToken.IsCancellationRequested)
            {
                if (PlaybackState.State != PlayerPlaybackState.Buffering)
                {
                    return;
                }

                if (IsLocalTarget())
                {
                    var localState = _sendspinPlayerService.PlayState.State;
                    if (localState != PlayerPlaybackState.Buffering && localState != PlayerPlaybackState.Unknown)
                    {
                        SetState(localState);
                        return;
                    }

                    await Task.Delay(BufferingResolveLocalInterval, cancellationToken);
                }
                else
                {
                    await RefreshNowSafeAsync(cancellationToken);
                    await Task.Delay(BufferingResolveRemoteInterval, cancellationToken);
                }

                if (DateTimeOffset.UtcNow - startUtc >= BufferingResolveMaxDuration)
                {
                    await RefreshNowSafeAsync(cancellationToken);
                    return;
                }
            }
        }
        catch (OperationCanceledException)
        {
            // Expected when a new buffering phase starts or service is stopped.
        }
    }

    private async Task RefreshNowSafeAsync(CancellationToken cancellationToken)
    {
        try
        {
            await RefreshNowAsync(cancellationToken);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex)
        {
            _logger.LogDebug(ex, "Fast buffering refresh failed");
        }
    }

#endregion

    private async Task RunLoopAsync(CancellationToken cancellationToken)
    {
        using var timer = new PeriodicTimer(QueueRefreshInterval);

        while (await timer.WaitForNextTickAsync(cancellationToken))
        {
            try
            {
                await RefreshNowAsync(cancellationToken);
            }
            catch (OperationCanceledException)
            {
                throw;
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Playback refresh loop failed");
            }
        }
    }

    #endregion

    #region Queue Refresh

    private async Task RefreshStateCoreAsync(CancellationToken cancellationToken)
    {
        var activePlayerId = !string.IsNullOrWhiteSpace(ActivePlayerId)
            ? ActivePlayerId
            : _sendspinPlayerService.PlayerId;

        if (string.IsNullOrWhiteSpace(activePlayerId))
        {
            UpdateState(null, null, Array.Empty<QueueItem>());
            return;
        }

        if (!string.Equals(activePlayerId, ActivePlayerId, StringComparison.Ordinal))
        {
            ActivePlayerId = activePlayerId;
        }

        try
        {
            var favoritesSnapshot = await _userDataService.GetFavoritesSnapshotAsync(cancellationToken);
            var favoriteTrackUris = favoritesSnapshot?.Tracks
                .Select(track => track.Uri)
                .Where(uri => !string.IsNullOrWhiteSpace(uri))
                .ToHashSet(StringComparer.OrdinalIgnoreCase)
                ?? new HashSet<string>(StringComparer.OrdinalIgnoreCase);

            var activeQueue = await _musicAssistant.GetActiveQueueForPlayerAsync(activePlayerId);
            if (activeQueue == null)
            {
                UpdateState(null, null, Array.Empty<QueueItem>());
                return;
            }

            Track? currentTrack = activeQueue.CurrentItem?.MediaItem;
            if (currentTrack != null)
            {
                currentTrack.Favorite = !string.IsNullOrWhiteSpace(currentTrack.Uri)
                    && favoriteTrackUris.Contains(currentTrack.Uri);
            }

            var queueItems = new List<QueueItem>();
            if (!string.IsNullOrWhiteSpace(activeQueue.QueueId))
            {
                queueItems = await _musicAssistant.GetQueueItemsAsync(activeQueue.QueueId);

                for (var index = 0; index < queueItems.Count; index++)
                {
                    var track = queueItems[index].MediaItem;
                    if (track == null)
                    {
                        continue;
                    }

                    track.Index = index + 1;
                    track.Favorite = !string.IsNullOrWhiteSpace(track.Uri)
                        && favoriteTrackUris.Contains(track.Uri);
                }
            }

            var firstQueueTrack = queueItems.Select(item => item.MediaItem).OfType<Track>().FirstOrDefault();
            if (currentTrack == null && firstQueueTrack != null)
            {
                currentTrack = firstQueueTrack;
            }

            UpdateState(activeQueue, currentTrack, queueItems);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to refresh playback state");
            UpdateState(null, null, Array.Empty<QueueItem>());
        }
    }

#endregion

#region Queue State Update

    private void UpdateState(PlayerQueue? nextQueue, Track? nextTrack, IReadOnlyList<QueueItem> nextQueueItems)
    {
        var queueChanged = !ArePlayerQueuesEqual(CurrentPlayerQueue, nextQueue);
        var currentTrackChanged = !AreTracksEqual(CurrentTrack, nextTrack);
        var queueItemsChanged = !AreQueueItemListsEqual(_currentQueueItems, nextQueueItems);

        if (queueChanged)
        {
            CurrentPlayerQueue = nextQueue;
            ActiveQueueId = nextQueue?.QueueId;
            SyncPlaybackStateFromQueue(nextQueue);
            CurrentPlayerQueueUpdated?.Invoke(this, EventArgs.Empty);
        }

        if (currentTrackChanged)
        {
            CurrentTrack = nextTrack;
            CurrentTrackUpdated?.Invoke(this, EventArgs.Empty);
        }

        if (queueItemsChanged)
        {
            var changeSet = BuildQueueItemsChangeSet(_currentQueueItems, nextQueueItems);
            _currentQueueItems.Clear();
            _currentQueueItems.AddRange(nextQueueItems);
            CurrentQueueItemsUpdated?.Invoke(this, new QueueItemsChangedEventArgs(changeSet));
        }
    }

#endregion

#region Event Handlers

    private void OnSendspinPlayerPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (!IsLocalTarget())
        {
            return;
        }

        if (e.PropertyName == nameof(ISendspinPlayerService.PlayState))
        {
            var localState = _sendspinPlayerService.PlayState.State;
            SetState(localState);

            if (localState != PlayerPlaybackState.Buffering)
            {
                _ = StopBufferingResolutionLoopAsync();
            }
        }
    }

#endregion

#region ChangeSet Builder for Queue Items

    private static QueueItemsChangeSet BuildQueueItemsChangeSet(IReadOnlyList<QueueItem> previous, IReadOnlyList<QueueItem> next)
    {
        if (previous.Count == 0 && next.Count == 0)
        {
            return QueueItemsChangeSet.Empty;
        }

        var previousEntries = BuildTokenEntries(previous);
        var nextEntries = BuildTokenEntries(next);

        var previousByToken = previousEntries.ToDictionary(entry => entry.Token, entry => entry.Item);
        var nextByToken = nextEntries.ToDictionary(entry => entry.Token, entry => entry.Item);

        var workingTokens = previousEntries.Select(entry => entry.Token).ToList();
        var nextTokenSet = nextEntries.Select(entry => entry.Token).ToHashSet(StringComparer.Ordinal);
        var changes = new List<QueueItemChange>();

        for (var index = workingTokens.Count - 1; index >= 0; index--)
        {
            if (nextTokenSet.Contains(workingTokens[index]))
            {
                continue;
            }

            changes.Add(QueueItemChange.Remove(index));
            workingTokens.RemoveAt(index);
        }

        for (var targetIndex = 0; targetIndex < nextEntries.Count; targetIndex++)
        {
            var desiredToken = nextEntries[targetIndex].Token;

            if (targetIndex < workingTokens.Count && string.Equals(workingTokens[targetIndex], desiredToken, StringComparison.Ordinal))
            {
                continue;
            }

            var existingIndex = workingTokens.IndexOf(desiredToken);
            if (existingIndex >= 0)
            {
                changes.Add(QueueItemChange.Move(existingIndex, targetIndex));
                workingTokens.RemoveAt(existingIndex);
                workingTokens.Insert(targetIndex, desiredToken);
                continue;
            }

            changes.Add(QueueItemChange.Insert(targetIndex, nextByToken[desiredToken]));
            workingTokens.Insert(targetIndex, desiredToken);
        }

        for (var index = 0; index < nextEntries.Count; index++)
        {
            var token = nextEntries[index].Token;
            if (!previousByToken.TryGetValue(token, out var previousItem))
            {
                continue;
            }

            var nextItem = nextByToken[token];
            if (!AreQueueItemsEqual(previousItem, nextItem))
            {
                changes.Add(QueueItemChange.Replace(index, nextItem));
            }
        }

        return changes.Count == 0
            ? QueueItemsChangeSet.Empty
            : new QueueItemsChangeSet(changes);
    }

    private static List<(string Token, QueueItem Item)> BuildTokenEntries(IReadOnlyList<QueueItem> items)
    {
        var keyCounts = new Dictionary<string, int>(StringComparer.Ordinal);
        var entries = new List<(string Token, QueueItem Item)>(items.Count);

        foreach (var item in items)
        {
            var key = GetQueueItemKey(item);
            var nextOccurrence = keyCounts.TryGetValue(key, out var existingCount)
                ? existingCount + 1
                : 1;

            keyCounts[key] = nextOccurrence;
            entries.Add(($"{key}#{nextOccurrence}", item));
        }

        return entries;
    }

#endregion

#region Helpers

    private void SyncPlaybackStateFromQueue(PlayerQueue? queue)
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

        SetState(mappedState);

        if (mappedState != PlayerPlaybackState.Buffering)
        {
            _ = StopBufferingResolutionLoopAsync();
        }
    }

    private void SetState(PlayerPlaybackState state)
    {
        PlaybackState = new PlayerPlayState(state, DateTimeOffset.UtcNow);
    }

    private bool IsLocalTarget()
    {
        return !string.IsNullOrWhiteSpace(ActivePlayerId)
            && !string.IsNullOrWhiteSpace(_sendspinPlayerService.PlayerId)
            && string.Equals(ActivePlayerId, _sendspinPlayerService.PlayerId, StringComparison.Ordinal);
    }

    private static bool ArePlayerQueuesEqual(PlayerQueue? left, PlayerQueue? right)
    {
        if (left == null && right == null)
        {
            return true;
        }

        if (left == null || right == null)
        {
            return false;
        }

        return string.Equals(left.QueueId, right.QueueId, StringComparison.Ordinal)
            && left.CurrentIndex == right.CurrentIndex
            && left.ShuffleEnabled == right.ShuffleEnabled
            && left.RepeatMode == right.RepeatMode
            && left.DontStopTheMusicEnabled == right.DontStopTheMusicEnabled
            && left.FlowMode == right.FlowMode
            && left.State == right.State;
    }

    private static bool AreTracksEqual(Track? left, Track? right)
    {
        return string.Equals(GetTrackKey(left), GetTrackKey(right), StringComparison.Ordinal);
    }

    private static bool AreQueueItemListsEqual(IReadOnlyList<QueueItem> left, IReadOnlyList<QueueItem> right)
    {
        if (left.Count != right.Count)
        {
            return false;
        }

        for (var index = 0; index < left.Count; index++)
        {
            if (!AreQueueItemsEqual(left[index], right[index]))
            {
                return false;
            }
        }

        return true;
    }

    private static bool AreQueueItemsEqual(QueueItem? left, QueueItem? right)
    {
        return string.Equals(GetQueueItemKey(left), GetQueueItemKey(right), StringComparison.Ordinal);
    }

    private static string GetTrackKey(Track? track)
    {
        if (string.IsNullOrWhiteSpace(track?.Uri))
        {
            return string.Empty;
        }

        return $"uri:{track.Uri.Trim().ToUpperInvariant()}";
    }

    private static string GetQueueItemKey(QueueItem? item)
    {
        if (!string.IsNullOrWhiteSpace(item?.QueueItemId))
        {
            return $"id:{item.QueueItemId.Trim()}";
        }

        var track = item?.MediaItem;
        if (!string.IsNullOrWhiteSpace(track?.Uri))
        {
            return $"uri:{track.Uri.Trim().ToUpperInvariant()}";
        }

        if (!string.IsNullOrWhiteSpace(track?.ItemId))
        {
            return $"item:{track.ItemId.Trim()}";
        }

        return string.Empty;
    }

    private static string? NormalizeId(string? value)
    {
        return string.IsNullOrWhiteSpace(value) ? null : value.Trim();
    }

    private static RepeatMode GetNextRepeatMode(string? repeatMode)
    {
        var currentMode = repeatMode?.Trim().ToLowerInvariant() switch
        {
            "all" => RepeatMode.All,
            "one" => RepeatMode.One,
            _ => RepeatMode.Off
        };

        return currentMode switch
        {
            RepeatMode.Off => RepeatMode.All,
            RepeatMode.All => RepeatMode.One,
            RepeatMode.One => RepeatMode.Off,
            _ => RepeatMode.Off
        };
    }

    private bool SetProperty<T>(ref T field, T value, [CallerMemberName] string propertyName = "")
    {
        if (EqualityComparer<T>.Default.Equals(field, value))
        {
            return false;
        }

        field = value;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
        return true;
    }

#endregion
}
