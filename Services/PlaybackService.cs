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
    PlaybackStateModel PlaybackState { get; set; }
    int Volume { get; }
    bool IsMuted { get; }
    bool? ShuffleEnabled { get; }
    string? RepeatMode { get; }
    bool? DontStopTheMusicEnabled { get; }
    bool? FlowModeEnabled { get; }
    int? CurrentQueueIndex { get; }
    int QueueItemCount { get; }
    double DurationSeconds { get; }
    double PositionSeconds { get; }

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
    Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default);
    Task ToggleMuteAsync(bool currentMuted, CancellationToken cancellationToken = default);
    Task ToggleShuffleAsync(bool? currentShuffleEnabled, CancellationToken cancellationToken = default);
    Task ToggleRepeatModeAsync(string? currentRepeatMode, CancellationToken cancellationToken = default);
}

#endregion

public sealed class PlaybackService : IPlaybackService
{
#region Fields

    // Poll interval for periodic queue refresh.
    private static readonly TimeSpan QueueRefreshInterval = TimeSpan.FromSeconds(15);

    private readonly MusicAssistantService _musicAssistant;
    private readonly IUserDataService _userDataService;
    private readonly ISendspinPlayerService _sendspinPlayerService;
    private readonly ILogger<PlaybackService> _logger;

    private readonly SemaphoreSlim _startStopLock = new(1, 1);
    private readonly SemaphoreSlim _refreshLock = new(1, 1);

    private CancellationTokenSource? _loopCts;
    private Task? _loopTask;

    private string? _activePlayerId;
    private string? _activeQueueId;
    private PlaybackStateModel _playbackState = new(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow);
    private int _volume;
    private bool _isMuted;
    private bool? _shuffleEnabled;
    private string? _repeatMode;
    private bool? _dontStopTheMusicEnabled;
    private bool? _flowModeEnabled;
    private int? _currentQueueIndex;
    private int _queueItemCount;
    private double _durationSeconds;
    private double _positionSeconds;
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

    public bool? FlowModeEnabled
    {
        get => _flowModeEnabled;
        private set => SetProperty(ref _flowModeEnabled, value);
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
        PlaybackState = _sendspinPlayerService.PlayerState;
        Volume = _sendspinPlayerService.Volume;
        IsMuted = _sendspinPlayerService.IsMuted;
        ShuffleEnabled = _sendspinPlayerService.ShuffleEnabled;
        RepeatMode = _sendspinPlayerService.RepeatMode;
        DurationSeconds = _sendspinPlayerService.DurationSeconds;
        PositionSeconds = _sendspinPlayerService.PositionSeconds;
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

        var canSendPlayAction = await _sendspinPlayerService.EnsureConnectedAsync(ActivePlayerId, cancellationToken);
        if (!canSendPlayAction)
        {
            _logger.LogWarning("Play action aborted: local Sendspin connection is not available");
            return;
        }

        PlaybackState = new PlaybackStateModel(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);

        await _musicAssistant.PlayPauseAsync(ActiveQueueId);
    }

    public async Task NextTrackAsync(CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(ActiveQueueId))
        {
            _logger.LogWarning("No active queue available for next track");
            return;
        }

        PlaybackState = new PlaybackStateModel(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);
        await _musicAssistant.NextAsync(ActiveQueueId);
    }

    public async Task PreviousTrackAsync(CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(ActiveQueueId))
        {
            _logger.LogWarning("No active queue available for previous track");
            return;
        }

        PlaybackState = new PlaybackStateModel(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);
        await _musicAssistant.PreviousAsync(ActiveQueueId);
    }

    public async Task SeekAsync(double seconds, double durationSeconds, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(ActiveQueueId))
        {
            _logger.LogWarning("No active queue available for seek");
            return;
        }

        PlaybackState = new PlaybackStateModel(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);

        var clamped = Math.Max(0, Math.Min(durationSeconds, seconds));
        if (IsLocalTarget())
        {
            _sendspinPlayerService.PositionSeconds = clamped;
        }

        await _musicAssistant.SeekAsync(ActiveQueueId, (int)Math.Round(clamped));
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

#region Remote Path: Queue Polling via MusicAssistant

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
                DontStopTheMusicEnabled = null;
                FlowModeEnabled = null;
                CurrentQueueIndex = null;
                QueueItemCount = 0;
                UpdateState(null, null, Array.Empty<QueueItem>());
                return;
            }

            DontStopTheMusicEnabled = activeQueue.DontStopTheMusicEnabled;
            FlowModeEnabled = activeQueue.FlowMode;
            CurrentQueueIndex = activeQueue.CurrentIndex;
            QueueItemCount = Math.Max(0, activeQueue.ItemCount);

            Track? currentTrack = activeQueue.CurrentItem?.MediaItem;
            if (currentTrack != null)
            {
                currentTrack.Favorite = !string.IsNullOrWhiteSpace(currentTrack.Uri)
                    && favoriteTrackUris.Contains(currentTrack.Uri);

                DurationSeconds = Math.Max(0, currentTrack.Duration);
            }

            if (activeQueue.ElapsedTime is double elapsedTime)
            {
                PositionSeconds = Math.Max(0, elapsedTime);
            }

            if (activeQueue.ShuffleEnabled.HasValue)
            {
                ShuffleEnabled = activeQueue.ShuffleEnabled;
            }

            if (activeQueue.RepeatMode.HasValue)
            {
                RepeatMode = activeQueue.RepeatMode.Value.ToString();
            }

            if (!IsLocalTarget())
            {
                var player = await _musicAssistant.GetPlayerAsync(activePlayerId, raiseUnavailable: false);
                if (player?.VolumeLevel is int volumeLevel)
                {
                    Volume = volumeLevel;
                }

                if (player?.VolumeMuted is bool volumeMuted)
                {
                    IsMuted = volumeMuted;
                }
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

#region Remote Refresh Path: Queue Apply and Diff

    private void UpdateState(PlayerQueue? nextQueue, Track? nextTrack, IReadOnlyList<QueueItem> nextQueueItems)
    {
        var localTrackEnriched = false;
        if (IsLocalTarget())
        {
            (nextTrack, localTrackEnriched) = MergeLocalTrackWithQueueTrack(nextTrack);
        }

        var queueChanged = !ArePlayerQueuesEqual(CurrentPlayerQueue, nextQueue);
        var currentTrackChanged = !AreTracksEqual(CurrentTrack, nextTrack) || localTrackEnriched;
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
            if (!ReferenceEquals(CurrentTrack, nextTrack))
            {
                CurrentTrack = nextTrack;
            }

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

#region Local Refresh Path: Sendspin Event-Based Immediate Updates

    private void OnSendspinPlayerPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (!IsLocalTarget())
        {
            return;
        }

        if (e.PropertyName == nameof(ISendspinPlayerService.PlayerState))
        {
            var localState = _sendspinPlayerService.PlayerState;

            if (localState.State == PlayerPlaybackState.Unknown)
            {
                return;
            }

            PlaybackState = localState;
            return;
        }

        if (e.PropertyName == nameof(ISendspinPlayerService.Volume))
        {
            Volume = _sendspinPlayerService.Volume;
            return;
        }

        if (e.PropertyName == nameof(ISendspinPlayerService.IsMuted))
        {
            IsMuted = _sendspinPlayerService.IsMuted;
            return;
        }

        if (e.PropertyName == nameof(ISendspinPlayerService.DurationSeconds))
        {
            DurationSeconds = _sendspinPlayerService.DurationSeconds;
            return;
        }

        if (e.PropertyName == nameof(ISendspinPlayerService.PositionSeconds))
        {
            PositionSeconds = _sendspinPlayerService.PositionSeconds;
            return;
        }

        if (e.PropertyName == nameof(ISendspinPlayerService.TrackTitle)
            || e.PropertyName == nameof(ISendspinPlayerService.TrackArtist)
            || e.PropertyName == nameof(ISendspinPlayerService.TrackAlbum)
            || e.PropertyName == nameof(ISendspinPlayerService.TrackImageUri))
        {
            SyncLocalTrackFromSendspin();
            return;
        }

        if (e.PropertyName == nameof(ISendspinPlayerService.ShuffleEnabled)
            || e.PropertyName == nameof(ISendspinPlayerService.RepeatMode))
        {
            SyncLocalQueueSettingsFromSendspin();
        }
    }

    private void SyncLocalTrackFromSendspin()
    {
        var updatedTrack = CreateTrackFromSendspinSnapshot(CurrentTrack);
        if (CurrentTrack != null && AreTrackUiFieldsEqual(CurrentTrack, updatedTrack))
        {
            return;
        }

        CurrentTrack = updatedTrack;
        CurrentTrackUpdated?.Invoke(this, EventArgs.Empty);
    }

    private void SyncLocalQueueSettingsFromSendspin()
    {
        ShuffleEnabled = _sendspinPlayerService.ShuffleEnabled;
        RepeatMode = _sendspinPlayerService.RepeatMode;

        var existingQueue = CurrentPlayerQueue;
        if (existingQueue == null)
        {
            return;
        }

        var changed = false;

        var sendspinShuffle = _sendspinPlayerService.ShuffleEnabled;
        if (existingQueue.ShuffleEnabled != sendspinShuffle)
        {
            existingQueue.ShuffleEnabled = sendspinShuffle;
            changed = true;
        }

        if (TryParseRepeatMode(_sendspinPlayerService.RepeatMode, out var sendspinRepeatMode)
            && existingQueue.RepeatMode != sendspinRepeatMode)
        {
            existingQueue.RepeatMode = sendspinRepeatMode;
            changed = true;
        }

        if (!changed)
        {
            return;
        }

        CurrentPlayerQueueUpdated?.Invoke(this, EventArgs.Empty);
    }

    private (Track? Track, bool EnrichedExistingTrack) MergeLocalTrackWithQueueTrack(Track? queueTrack)
    {
        if (queueTrack == null)
        {
            return (CurrentTrack, false);
        }

        if (CurrentTrack == null)
        {
            var localTrack = CreateTrackFromSendspinSnapshot(queueTrack);
            var enriched = MergeMissingTrackDataFromQueue(localTrack, queueTrack);
            return (localTrack, enriched);
        }

        if (!AreSameTrackIdentity(CurrentTrack, queueTrack))
        {
            var switchedTrack = CreateTrackFromSendspinSnapshot(queueTrack);
            var enriched = MergeMissingTrackDataFromQueue(switchedTrack, queueTrack);
            return (switchedTrack, enriched);
        }

        var changed = MergeMissingTrackDataFromQueue(CurrentTrack, queueTrack);
        return (CurrentTrack, changed);
    }

    private Track CreateTrackFromSendspinSnapshot(Track? baseTrack)
    {
        var track = new Track
        {
            ItemId = baseTrack?.ItemId ?? string.Empty,
            Provider = baseTrack?.Provider ?? string.Empty,
            SortName = baseTrack?.SortName,
            Uri = baseTrack?.Uri,
            Duration = baseTrack?.Duration ?? 0,
            Artists = baseTrack?.Artists,
            Album = baseTrack?.Album,
            DiscNumber = baseTrack?.DiscNumber ?? 0,
            TrackNumber = baseTrack?.TrackNumber ?? 0,
            ProviderMappings = baseTrack?.ProviderMappings?.ToList() ?? new List<ProviderMapping>(),
            Metadata = baseTrack?.Metadata,
            Favorite = baseTrack?.Favorite ?? false,
            ExternalIds = baseTrack?.ExternalIds,
            Index = baseTrack?.Index ?? 0,
        };

        var title = _sendspinPlayerService.TrackTitle;
        if (!string.IsNullOrWhiteSpace(title))
        {
            track.Name = title;
        }

        var artist = _sendspinPlayerService.TrackArtist;
        if (!string.IsNullOrWhiteSpace(artist))
        {
            track.Artists = new List<Artist> { new() { Name = artist } };
        }

        var album = _sendspinPlayerService.TrackAlbum;
        if (!string.IsNullOrWhiteSpace(album))
        {
            track.Album ??= new Album();
            track.Album.Name = album;
        }

        var imageUri = _sendspinPlayerService.TrackImageUri;
        if (!string.IsNullOrWhiteSpace(imageUri))
        {
            SetTrackImage(track, imageUri);
        }

        return track;
    }

    private static bool MergeMissingTrackDataFromQueue(Track target, Track source)
    {
        var changed = false;

        if (string.IsNullOrWhiteSpace(target.ItemId) && !string.IsNullOrWhiteSpace(source.ItemId))
        {
            target.ItemId = source.ItemId;
            changed = true;
        }

        if (string.IsNullOrWhiteSpace(target.Provider) && !string.IsNullOrWhiteSpace(source.Provider))
        {
            target.Provider = source.Provider;
            changed = true;
        }

        if (string.IsNullOrWhiteSpace(target.Uri) && !string.IsNullOrWhiteSpace(source.Uri))
        {
            target.Uri = source.Uri;
            changed = true;
        }

        if (string.IsNullOrWhiteSpace(target.Name) && !string.IsNullOrWhiteSpace(source.Name))
        {
            target.Name = source.Name;
            changed = true;
        }

        if (string.IsNullOrWhiteSpace(target.SortName) && !string.IsNullOrWhiteSpace(source.SortName))
        {
            target.SortName = source.SortName;
            changed = true;
        }

        if (target.Duration <= 0 && source.Duration > 0)
        {
            target.Duration = source.Duration;
            changed = true;
        }

        if (target.DiscNumber <= 0 && source.DiscNumber > 0)
        {
            target.DiscNumber = source.DiscNumber;
            changed = true;
        }

        if (target.TrackNumber <= 0 && source.TrackNumber > 0)
        {
            target.TrackNumber = source.TrackNumber;
            changed = true;
        }

        if ((target.Artists == null || target.Artists.Count == 0) && source.Artists is { Count: > 0 })
        {
            target.Artists = source.Artists;
            changed = true;
        }

        if (target.Album == null && source.Album != null)
        {
            target.Album = source.Album;
            changed = true;
        }
        else if (target.Album != null && source.Album != null)
        {
            changed |= MergeMissingAlbumData(target.Album, source.Album);
        }

        if ((target.ProviderMappings == null || target.ProviderMappings.Count == 0)
            && source.ProviderMappings is { Count: > 0 })
        {
            target.ProviderMappings = source.ProviderMappings;
            changed = true;
        }

        if (target.ExternalIds == null && source.ExternalIds != null)
        {
            target.ExternalIds = source.ExternalIds;
            changed = true;
        }

        if (target.Metadata == null && source.Metadata != null)
        {
            target.Metadata = source.Metadata;
            changed = true;
        }
        else if (target.Metadata != null && source.Metadata != null)
        {
            if ((target.Metadata.Images == null || target.Metadata.Images.Count == 0)
                && source.Metadata.Images is { Count: > 0 })
            {
                target.Metadata.Images = source.Metadata.Images;
                changed = true;
            }
        }

        if (!target.Favorite && source.Favorite)
        {
            target.Favorite = true;
            changed = true;
        }

        if (target.Index <= 0 && source.Index > 0)
        {
            target.Index = source.Index;
            changed = true;
        }

        return changed;
    }

    private static bool MergeMissingAlbumData(Album target, Album source)
    {
        var changed = false;

        if (string.IsNullOrWhiteSpace(target.ItemId) && !string.IsNullOrWhiteSpace(source.ItemId))
        {
            target.ItemId = source.ItemId;
            changed = true;
        }

        if (string.IsNullOrWhiteSpace(target.Provider) && !string.IsNullOrWhiteSpace(source.Provider))
        {
            target.Provider = source.Provider;
            changed = true;
        }

        if (string.IsNullOrWhiteSpace(target.Uri) && !string.IsNullOrWhiteSpace(source.Uri))
        {
            target.Uri = source.Uri;
            changed = true;
        }

        if (string.IsNullOrWhiteSpace(target.Name) && !string.IsNullOrWhiteSpace(source.Name))
        {
            target.Name = source.Name;
            changed = true;
        }

        if (string.IsNullOrWhiteSpace(target.SortName) && !string.IsNullOrWhiteSpace(source.SortName))
        {
            target.SortName = source.SortName;
            changed = true;
        }

        if (target.Metadata == null && source.Metadata != null)
        {
            target.Metadata = source.Metadata;
            changed = true;
        }
        else if (target.Metadata != null && source.Metadata != null)
        {
            if ((target.Metadata.Images == null || target.Metadata.Images.Count == 0)
                && source.Metadata.Images is { Count: > 0 })
            {
                target.Metadata.Images = source.Metadata.Images;
                changed = true;
            }
        }

        if ((target.Artists == null || target.Artists.Count == 0) && source.Artists is { Count: > 0 })
        {
            target.Artists = source.Artists;
            changed = true;
        }

        if (!target.Year.HasValue && source.Year.HasValue)
        {
            target.Year = source.Year;
            changed = true;
        }

        return changed;
    }

    private static void SetTrackImage(Track track, string imageUri)
    {
        track.Metadata ??= new MediaItemMetadata();
        track.Metadata.Images =
        [
            new MediaItemImage
            {
                Type = "thumb",
                Path = imageUri,
                Provider = track.Provider,
                RemotelyAccessible = true,
            }
        ];

        track.Album ??= new Album();
        track.Album.Metadata ??= new MediaItemMetadata();
        track.Album.Metadata.Images =
        [
            new MediaItemImage
            {
                Type = "thumb",
                Path = imageUri,
                Provider = track.Provider,
                RemotelyAccessible = true,
            }
        ];
    }

    private static bool AreSameTrackIdentity(Track left, Track right)
    {
        if (!string.IsNullOrWhiteSpace(left.Uri) && !string.IsNullOrWhiteSpace(right.Uri))
        {
            return string.Equals(left.Uri, right.Uri, StringComparison.OrdinalIgnoreCase);
        }

        if (!string.IsNullOrWhiteSpace(left.ItemId) && !string.IsNullOrWhiteSpace(right.ItemId))
        {
            return string.Equals(left.ItemId, right.ItemId, StringComparison.OrdinalIgnoreCase);
        }

        return false;
    }

    private static bool AreTrackUiFieldsEqual(Track left, Track right)
    {
        return string.Equals(left.Name, right.Name, StringComparison.Ordinal)
            && string.Equals(left.ArtistName, right.ArtistName, StringComparison.Ordinal)
            && string.Equals(left.AlbumName, right.AlbumName, StringComparison.Ordinal)
            && string.Equals(left.ImageUri, right.ImageUri, StringComparison.Ordinal)
            && string.Equals(left.Uri, right.Uri, StringComparison.Ordinal)
            && string.Equals(left.ItemId, right.ItemId, StringComparison.Ordinal);
    }

#endregion

#region Queue-ChangeSet Builder

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

#region Shared Helpers

    private void SyncPlaybackStateFromQueue(PlayerQueue? queue)
    {
        if (IsLocalTarget())
        {
            return;
        }

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

        PlaybackState = new PlaybackStateModel(mappedState, DateTimeOffset.UtcNow);
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
        if (left == null && right == null)
        {
            return true;
        }

        if (left == null || right == null)
        {
            return false;
        }

        if (!string.IsNullOrWhiteSpace(left.Uri) && !string.IsNullOrWhiteSpace(right.Uri))
        {
            return string.Equals(left.Uri, right.Uri, StringComparison.OrdinalIgnoreCase);
        }

        if (!string.IsNullOrWhiteSpace(left.ItemId) && !string.IsNullOrWhiteSpace(right.ItemId))
        {
            return string.Equals(left.ItemId, right.ItemId, StringComparison.OrdinalIgnoreCase);
        }

        return string.Equals(left.Name, right.Name, StringComparison.Ordinal)
            && string.Equals(left.ArtistName, right.ArtistName, StringComparison.Ordinal)
            && string.Equals(left.AlbumName, right.AlbumName, StringComparison.Ordinal);
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

    private static mashin.Models.RepeatMode GetNextRepeatMode(string? repeatMode)
    {
        var currentMode = repeatMode?.Trim().ToLowerInvariant() switch
        {
            "all" => mashin.Models.RepeatMode.All,
            "one" => mashin.Models.RepeatMode.One,
            _ => mashin.Models.RepeatMode.Off
        };

        return currentMode switch
        {
            mashin.Models.RepeatMode.Off => mashin.Models.RepeatMode.All,
            mashin.Models.RepeatMode.All => mashin.Models.RepeatMode.One,
            mashin.Models.RepeatMode.One => mashin.Models.RepeatMode.Off,
            _ => mashin.Models.RepeatMode.Off
        };
    }

    private static bool TryParseRepeatMode(string? repeatMode, out mashin.Models.RepeatMode parsedMode)
    {
        parsedMode = repeatMode?.Trim().ToLowerInvariant() switch
        {
            "all" => mashin.Models.RepeatMode.All,
            "one" => mashin.Models.RepeatMode.One,
            "off" => mashin.Models.RepeatMode.Off,
            _ => mashin.Models.RepeatMode.Off
        };

        return !string.IsNullOrWhiteSpace(repeatMode);
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
