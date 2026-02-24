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
    event EventHandler<QueueTracksChangedEventArgs>? CurrentQueueTracksUpdated;

    PlayerQueue? CurrentPlayerQueue { get; }
    Track? CurrentTrack { get; }
    IReadOnlyList<Track> CurrentQueueTracks { get; }

    Task StartAsync(CancellationToken cancellationToken = default);
    Task StopAsync();
    Task RefreshNowAsync(CancellationToken cancellationToken = default);
}

public enum QueueTrackChangeType
{
    Insert,
    Remove,
    Move,
    Replace
}

public sealed class QueueTrackChange
{
    public QueueTrackChangeType ChangeType { get; }
    public int Index { get; }
    public int NewIndex { get; }
    public Track? Track { get; }

    private QueueTrackChange(QueueTrackChangeType changeType, int index, int newIndex, Track? track)
    {
        ChangeType = changeType;
        Index = index;
        NewIndex = newIndex;
        Track = track;
    }

    public static QueueTrackChange Insert(int index, Track track) => new(QueueTrackChangeType.Insert, index, -1, track);
    public static QueueTrackChange Remove(int index) => new(QueueTrackChangeType.Remove, index, -1, null);
    public static QueueTrackChange Move(int index, int newIndex) => new(QueueTrackChangeType.Move, index, newIndex, null);
    public static QueueTrackChange Replace(int index, Track track) => new(QueueTrackChangeType.Replace, index, -1, track);
}

public sealed class QueueTracksChangeSet
{
    public static QueueTracksChangeSet Empty { get; } = new(Array.Empty<QueueTrackChange>());

    public IReadOnlyList<QueueTrackChange> Changes { get; }

    public QueueTracksChangeSet(IReadOnlyList<QueueTrackChange> changes)
    {
        Changes = changes;
    }
}

public sealed class QueueTracksChangedEventArgs : EventArgs
{
    public QueueTracksChangeSet ChangeSet { get; }

    public QueueTracksChangedEventArgs(QueueTracksChangeSet changeSet)
    {
        ChangeSet = changeSet;
    }
}

#endregion

public sealed class QueueSyncService : IQueueSyncService
{
    #region Fields

    private static readonly TimeSpan RefreshInterval = TimeSpan.FromSeconds(30);

    private readonly MusicAssistantService _musicAssistant;
    private readonly IUserDataService _userDataService;
    private readonly IPlayerService _playerService;
    private readonly ILogger<QueueSyncService> _logger;

    private readonly SemaphoreSlim _startStopLock = new(1, 1);
    private readonly SemaphoreSlim _refreshLock = new(1, 1);

    private CancellationTokenSource? _loopCts;
    private Task? _loopTask;
    private readonly List<Track> _currentQueueTracks = new();

    #endregion

    #region Events

    public event EventHandler? CurrentPlayerQueueUpdated;
    public event EventHandler? CurrentTrackUpdated;
    public event EventHandler<QueueTracksChangedEventArgs>? CurrentQueueTracksUpdated;

    #endregion

    #region Properties

    public PlayerQueue? CurrentPlayerQueue { get; private set; }
    public Track? CurrentTrack { get; private set; }
    public IReadOnlyList<Track> CurrentQueueTracks => new ReadOnlyCollection<Track>(_currentQueueTracks);

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
        if (string.IsNullOrWhiteSpace(_playerService.ClientId))
        {
            UpdateState(null, null, Array.Empty<Track>());
            return;
        }

        try
        {
            // Get active queue for player
            var activeQueue = await _musicAssistant.GetActiveQueueForPlayerAsync(_playerService.ClientId);
            if (activeQueue == null)
            {
                UpdateState(null, null, Array.Empty<Track>());
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
            var queueTracks = new List<Track>();
            if (!string.IsNullOrWhiteSpace(activeQueue.QueueId))
            {
                var queueItems = await _musicAssistant.GetQueueItemsAsync(activeQueue.QueueId);
                queueTracks = queueItems
                    .Select(queueItem => queueItem.MediaItem)
                    .OfType<Track>()
                    .ToList();

                if (queueTracks.Count > 0)
                {
                    await _musicAssistant.EnrichWithProviderInfoAsync(queueTracks);
                }

                for (var index = 0; index < queueTracks.Count; index++)
                {
                    var track = queueTracks[index];
                    track.Index = index + 1;
                    track.Favorite = await _userDataService.IsFavoriteAsync(track, cancellationToken);
                }
            }

            // Set current track to first in queue if no active track
            if (currentTrack == null && queueTracks.Count > 0)
            {
                currentTrack = queueTracks[0];
            }

            /*
            // Prefer the corresponding instance from queueTracks so CurrentTrack and queue list share the same object reference.
            if (currentTrack != null && queueTracks.Count > 0)
            {
                var queueCurrentTrack = queueTracks.FirstOrDefault(track =>
                    (!string.IsNullOrWhiteSpace(track.Uri)
                        && !string.IsNullOrWhiteSpace(currentTrack.Uri)
                        && string.Equals(track.Uri, currentTrack.Uri, StringComparison.Ordinal))
                    || (string.Equals(track.ItemId, currentTrack.ItemId, StringComparison.Ordinal)
                        && string.Equals(track.Provider, currentTrack.Provider, StringComparison.OrdinalIgnoreCase)));

                if (queueCurrentTrack != null)
                {
                    currentTrack = queueCurrentTrack;
                }
            }

            currentTrack?.IsPlaying = true;

            */

            UpdateState(activeQueue, currentTrack, queueTracks);
            return;
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to refresh queue state");
            UpdateState(null, null, Array.Empty<Track>());
            return;
        }
    }

    #endregion

   #region State Update

    private void UpdateState(PlayerQueue? nextQueue, Track? nextTrack, IReadOnlyList<Track> nextQueueTracks)
    {
        var queueChanged = !ArePlayerQueuesEqual(CurrentPlayerQueue, nextQueue);
        var currentTrackChanged = !AreTracksEqual(CurrentTrack, nextTrack);
        var queueTracksChanged = !AreTrackListsEqual(_currentQueueTracks, nextQueueTracks);

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

        if (queueTracksChanged)
        {
            var changeSet = BuildQueueTracksChangeSet(_currentQueueTracks, nextQueueTracks);
            _currentQueueTracks.Clear();
            _currentQueueTracks.AddRange(nextQueueTracks);
            CurrentQueueTracksUpdated?.Invoke(this, new QueueTracksChangedEventArgs(changeSet));
        }
    }

    #endregion

    #region ChangeSet Builder

    private static QueueTracksChangeSet BuildQueueTracksChangeSet(IReadOnlyList<Track> previous, IReadOnlyList<Track> next)
    {
        if (previous.Count == 0 && next.Count == 0)
        {
            return QueueTracksChangeSet.Empty;
        }

        var previousEntries = BuildTokenEntries(previous);
        var nextEntries = BuildTokenEntries(next);

        var previousByToken = previousEntries.ToDictionary(entry => entry.Token, entry => entry.Track);
        var nextByToken = nextEntries.ToDictionary(entry => entry.Token, entry => entry.Track);

        var workingTokens = previousEntries.Select(entry => entry.Token).ToList();
        var nextTokenSet = nextEntries.Select(entry => entry.Token).ToHashSet(StringComparer.Ordinal);
        var changes = new List<QueueTrackChange>();

        for (var index = workingTokens.Count - 1; index >= 0; index--)
        {
            if (nextTokenSet.Contains(workingTokens[index]))
            {
                continue;
            }

            changes.Add(QueueTrackChange.Remove(index));
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
                changes.Add(QueueTrackChange.Move(existingIndex, targetIndex));
                workingTokens.RemoveAt(existingIndex);
                workingTokens.Insert(targetIndex, desiredToken);
                continue;
            }

            changes.Add(QueueTrackChange.Insert(targetIndex, nextByToken[desiredToken]));
            workingTokens.Insert(targetIndex, desiredToken);
        }

        for (var index = 0; index < nextEntries.Count; index++)
        {
            var token = nextEntries[index].Token;
            if (!previousByToken.TryGetValue(token, out var previousTrack))
            {
                continue;
            }

            var nextTrack = nextByToken[token];
            if (!AreTracksEqual(previousTrack, nextTrack))
            {
                changes.Add(QueueTrackChange.Replace(index, nextTrack));
            }
        }

        return changes.Count == 0
            ? QueueTracksChangeSet.Empty
            : new QueueTracksChangeSet(changes);
    }

    private static List<(string Token, Track Track)> BuildTokenEntries(IReadOnlyList<Track> tracks)
    {
        var keyCounts = new Dictionary<string, int>(StringComparer.Ordinal);
        var entries = new List<(string Token, Track Track)>(tracks.Count);

        foreach (var track in tracks)
        {
            var key = GetTrackKey(track);
            var nextOccurrence = keyCounts.TryGetValue(key, out var existingCount)
                ? existingCount + 1
                : 1;

            keyCounts[key] = nextOccurrence;
            entries.Add(($"{key}#{nextOccurrence}", track));
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

    private static bool AreTrackListsEqual(IReadOnlyList<Track> left, IReadOnlyList<Track> right)
    {
        if (left.Count != right.Count)
        {
            return false;
        }

        for (var index = 0; index < left.Count; index++)
        {
            if (!AreTracksEqual(left[index], right[index]))
            {
                return false;
            }
        }

        return true;
    }

    private static string GetTrackKey(Track? track)
    {
        if (string.IsNullOrWhiteSpace(track?.Uri))
        {
            return string.Empty;
        }

        return $"uri:{track.Uri.Trim().ToUpperInvariant()}";
    }

    #endregion
}