using mashin.Models;
using Microsoft.Extensions.Logging;
using System.Collections.ObjectModel;

namespace mashin.Services;

/// <summary>
/// Keeps the player queue state in sync for the active client by polling periodically and on-demand,
/// stores the latest queue/track data as a single source of truth, and publishes targeted queue
/// change events so view models can apply efficient in-place UI updates.
/// </summary>

#region Interface and ChangeSet Definitions

public interface IQueueSyncService : IAsyncDisposable
{
    event EventHandler? CurrentPlayerQueueUpdated;
    event EventHandler? CurrentTrackUpdated;
    event EventHandler<QueueItemsChangedEventArgs>? CurrentQueueItemsUpdated;

    PlayerQueue? CurrentPlayerQueue { get; }
    Track? CurrentTrack { get; }
    IReadOnlyList<QueueItem> CurrentQueueItems { get; }

    Task StartAsync(CancellationToken cancellationToken = default);
    Task StopAsync();
    Task RefreshNowAsync(CancellationToken cancellationToken = default);
}

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

public sealed class QueueSyncService : IQueueSyncService
{
    #region Fields

    private static readonly TimeSpan RefreshInterval = TimeSpan.FromSeconds(15);

    private readonly MusicAssistantService _musicAssistant;
    private readonly IUserDataService _userDataService;
    private readonly IPlayerService _playerService;
    private readonly ILogger<QueueSyncService> _logger;

    private readonly SemaphoreSlim _startStopLock = new(1, 1);
    private readonly SemaphoreSlim _refreshLock = new(1, 1);

    private CancellationTokenSource? _loopCts;
    private Task? _loopTask;
    private readonly List<QueueItem> _currentQueueItems = new();

    #endregion

    #region Events

    public event EventHandler? CurrentPlayerQueueUpdated;
    public event EventHandler? CurrentTrackUpdated;
    public event EventHandler<QueueItemsChangedEventArgs>? CurrentQueueItemsUpdated;

    #endregion

    #region Properties

    public PlayerQueue? CurrentPlayerQueue { get; private set; }
    public Track? CurrentTrack { get; private set; }
    public IReadOnlyList<QueueItem> CurrentQueueItems => new ReadOnlyCollection<QueueItem>(_currentQueueItems);

    #endregion

    #region Construction

    public QueueSyncService(
        MusicAssistantService musicAssistant,
        IUserDataService userDataService,
        IPlayerService playerService,
        ILogger<QueueSyncService> logger)
    {
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _playerService = playerService;
        _logger = logger;
    }

    #endregion

    #region Lifecycle

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
        _startStopLock.Dispose();
        _refreshLock.Dispose();
    }

    #endregion

    #region Refresh

    private async Task RunLoopAsync(CancellationToken cancellationToken)
    {
        using var timer = new PeriodicTimer(RefreshInterval);

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
                _logger.LogWarning(ex, "Queue sync refresh loop failed");
            }
        }
    }

    private async Task RefreshStateCoreAsync(CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(_playerService.PlayerId))
        {
            UpdateState(null, null, Array.Empty<QueueItem>());
            return;
        }

        try
        {
            // Get active queue for player
            var activeQueue = await _musicAssistant.GetActiveQueueForPlayerAsync(_playerService.PlayerId);
            if (activeQueue == null)
            {
                UpdateState(null, null, Array.Empty<QueueItem>());
                return;
            }

            // Set current track 
            Track? currentTrack = activeQueue.CurrentItem?.MediaItem;
            if (currentTrack != null)
            {
                await _musicAssistant.EnrichWithProviderInfoAsync(new List<Track> { currentTrack });
                currentTrack.Favorite = await _userDataService.IsFavoriteAsync(currentTrack, cancellationToken);
            }

            // Get queue tracks
            var queueItems = new List<QueueItem>();
            if (!string.IsNullOrWhiteSpace(activeQueue.QueueId))
            {
                queueItems = await _musicAssistant.GetQueueItemsAsync(activeQueue.QueueId);
                var queueTracks = queueItems
                    .Select(queueItem => queueItem.MediaItem)
                    .OfType<Track>()
                    .ToList();

                if (queueTracks.Count > 0)
                {
                    await _musicAssistant.EnrichWithProviderInfoAsync(queueTracks);
                }

                for (var index = 0; index < queueItems.Count; index++)
                {
                    var track = queueItems[index].MediaItem;
                    if (track == null)
                    {
                        continue;
                    }

                    track.Index = index + 1;
                    track.Favorite = await _userDataService.IsFavoriteAsync(track, cancellationToken);
                }
            }

            // Set current track to first in queue if no active track
            var firstQueueTrack = queueItems.Select(item => item.MediaItem).OfType<Track>().FirstOrDefault();
            if (currentTrack == null && firstQueueTrack != null)
            {
                currentTrack = firstQueueTrack;
            }

            UpdateState(activeQueue, currentTrack, queueItems);
            return;
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to refresh queue state");
            UpdateState(null, null, Array.Empty<QueueItem>());
            return;
        }
    }

    #endregion

   #region State Update

    private void UpdateState(PlayerQueue? nextQueue, Track? nextTrack, IReadOnlyList<QueueItem> nextQueueItems)
    {
        var queueChanged = !ArePlayerQueuesEqual(CurrentPlayerQueue, nextQueue);
        var currentTrackChanged = !AreTracksEqual(CurrentTrack, nextTrack);
        var queueItemsChanged = !AreQueueItemListsEqual(_currentQueueItems, nextQueueItems);

        if (queueChanged)
        {
            CurrentPlayerQueue = nextQueue;
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

    #region ChangeSet Builder

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

    #region State Comparison Helper

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
            && left.CurrentIndex == right.CurrentIndex;
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

    #endregion
}