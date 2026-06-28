using mashin.Collections;
using mashin.Models;
using Microsoft.Extensions.Logging;
using System.ComponentModel;
using System.Runtime.CompilerServices;

namespace mashin.Services;

public enum PlaybackOutputMode
{
    Local,
    Sendspin,
    MA_Remote
}

public sealed class PlaybackService
{
    #region Fields

    private readonly SettingsService _settingsService;
    private readonly ILogger<PlaybackService> _logger;
    private readonly Dictionary<PlaybackOutputMode, IPlayerService> _players;

    private readonly PlaybackQueue _playbackQueue = new();

    private PlaybackOutputMode _outputMode = PlaybackOutputMode.Sendspin;
    private IPlayerService _activePlayer;

    private PlayerState _playbackState = new()
    {
        State = PlayerStateType.Idle,
        ActiveSinceUtc = DateTimeOffset.UtcNow
    };

    private int _volume = 50;
    private bool _isMuted;
    private double _durationSeconds;
    private double _positionSeconds;

    private bool _initialized;

    #endregion

    #region Construction

    public PlaybackService(
        SettingsService settingsService,
        IEnumerable<IPlayerService> playerServices,
        ILogger<PlaybackService> logger)
    {
        _settingsService = settingsService;
        _logger = logger;

        _players = playerServices
            .GroupBy(service => service.OutputMode)
            .ToDictionary(group => group.Key, group => group.First());

        _activePlayer = _players.TryGetValue(PlaybackOutputMode.Sendspin, out var preferred)
            ? preferred
            : _players.Values.First();

        foreach (var player in _players.Values.Distinct())
        {
            player.PropertyChanged += OnPlayerPropertyChanged;
            player.QueueChanged += OnPlayerQueueChanged;
        }

        _outputMode = _activePlayer.OutputMode;
        SyncProjectedStateFromActivePlayer();
    }

    #endregion

    #region Events

    public event PropertyChangedEventHandler? PropertyChanged;

    #endregion

    #region Properties

    public PlaybackOutputMode OutputMode
    {
        get => _outputMode;
        private set => SetProperty(ref _outputMode, value);
    }

    public string? ActivePlayerId
        => Normalize(_activePlayer.PlayerId);

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
        => _playbackQueue.ShuffleEnabled;

    public string? RepeatMode
        => _playbackQueue.RepeatMode?.ToString();

    public bool? DontStopTheMusicEnabled
        => _playbackQueue.DontStopTheMusicEnabled;

    public int? CurrentQueueIndex
        => _playbackQueue.CurrentIndex;

    public int QueueItemCount
        => Math.Max(_playbackQueue.ItemCount, _playbackQueue.Items.Count);

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

    public QueueItem? CurrentQueueItem => ResolveCurrentQueueItem();

    public ObservableRangeCollection<QueueItem> CurrentQueueItems => _playbackQueue.Items;

    #endregion

    #region Lifecycle

    public async Task InitializeAsync(CancellationToken cancellationToken = default)
    {
        if (_initialized)
        {
            return;
        }

        if (_players.TryGetValue(PlaybackOutputMode.Local, out var localPlayer))
        {
            await localPlayer.ActivateAsync(null, cancellationToken);
            await localPlayer.SetQueueAsync(CloneQueue(_playbackQueue), cancellationToken);
        }

        if (_players.TryGetValue(PlaybackOutputMode.Sendspin, out var sendspinPlayer))
        {
            var sendspinPlayerId = _settingsService.GetSendspinMusicAssistantPlayerId();
            await sendspinPlayer.ActivateAsync(sendspinPlayerId, cancellationToken);
            await sendspinPlayer.SetQueueAsync(CloneQueue(_playbackQueue), cancellationToken);
        }

        var defaultMode = _players.ContainsKey(PlaybackOutputMode.Sendspin)
            ? PlaybackOutputMode.Sendspin
            : _players.ContainsKey(PlaybackOutputMode.Local)
                ? PlaybackOutputMode.Local
                : PlaybackOutputMode.MA_Remote;

        await SetOutputModeAsync(defaultMode, cancellationToken: cancellationToken);

        _initialized = true;
    }

    public async Task SetOutputModeAsync(PlaybackOutputMode mode, string? targetPlayerId = null, CancellationToken cancellationToken = default)
    {
        if (!_players.TryGetValue(mode, out var nextPlayer))
        {
            throw new InvalidOperationException($"No player registered for output mode '{mode}'.");
        }

        var resolvedTargetPlayerId = mode switch
        {
            PlaybackOutputMode.Local => null,
            PlaybackOutputMode.Sendspin => _settingsService.GetSendspinMusicAssistantPlayerId(),
            PlaybackOutputMode.MA_Remote => !string.IsNullOrWhiteSpace(targetPlayerId)
                ? targetPlayerId
                : throw new ArgumentException("MA_Remote mode requires a targetPlayerId.", nameof(targetPlayerId)),
            _ => throw new ArgumentOutOfRangeException(nameof(mode), mode, "Unsupported playback output mode.")
        };

        // if remote player: pull queue
        if (mode == PlaybackOutputMode.MA_Remote)
        {
            await nextPlayer.ActivateAsync(resolvedTargetPlayerId, cancellationToken);

            var remoteQueue = nextPlayer.Queue;
            if (remoteQueue != null)
            {
                ApplyPlaybackQueue(remoteQueue);
            }
        }
        // if local or sendspin: push queue (full-copy)
        else
        {
            await nextPlayer.SetQueueAsync(CloneQueue(_playbackQueue), cancellationToken);
        }

        _activePlayer = nextPlayer;
        OutputMode = mode;
        RaisePropertyChanged(nameof(ActivePlayerId));

        SyncProjectedStateFromActivePlayer();

        _logger.LogDebug("Output mode switched. Mode={Mode}, ActivePlayerId={ActivePlayerId}, PlayerType={PlayerType}",
            OutputMode,
            ActivePlayerId,
            _activePlayer.GetType().Name);
    }

    #endregion

    #region Routing

    private async Task RouteOnActivePlayerAsync(Func<IPlayerService, Task> action, CancellationToken cancellationToken = default)
    {
        if (action == null)
        {
            return;
        }

        await SetOutputModeAsync(OutputMode, ActivePlayerId, cancellationToken);
        await action(_activePlayer);
    }

    #endregion

    #region Media Commands

    public Task PlayMediaAsync(IReadOnlyList<MediaItem> items)
        => RouteOnActivePlayerAsync(player => player.PlayMediaAsync(items ?? Array.Empty<MediaItem>()));

    public Task PlayMediaNextAsync(IReadOnlyList<MediaItem> items)
        => RouteOnActivePlayerAsync(player => player.PlayMediaNextAsync(items ?? Array.Empty<MediaItem>()));

    public Task PlayMediaLastAsync(IReadOnlyList<MediaItem> items)
        => RouteOnActivePlayerAsync(player => player.PlayMediaLastAsync(items ?? Array.Empty<MediaItem>()));

    public Task ShufflePlayMediaAsync(IReadOnlyList<MediaItem> items)
        => RouteOnActivePlayerAsync(player => player.ShufflePlayMediaAsync(items ?? Array.Empty<MediaItem>()));

    public Task ClearQueueAsync(bool skipStop = false)
        => RouteOnActivePlayerAsync(player => player.ClearQueueAsync(skipStop));

    public Task PlayQueueIndexAsync(int index, CancellationToken cancellationToken = default)
        => RouteOnActivePlayerAsync(player => player.PlayQueueIndexAsync(index, cancellationToken), cancellationToken);

    public Task MoveQueueItemAsync(string queueItemId, int posShift, CancellationToken cancellationToken = default)
        => RouteOnActivePlayerAsync(player => player.MoveQueueItemAsync(queueItemId, posShift, cancellationToken), cancellationToken);

    public Task DeleteQueueItemAsync(string queueItemId, CancellationToken cancellationToken = default)
        => RouteOnActivePlayerAsync(player => player.DeleteQueueItemAsync(queueItemId, cancellationToken), cancellationToken);

    #endregion

    #region Transport Commands

    public Task TogglePlayPauseAsync(CancellationToken cancellationToken = default)
        => RouteOnActivePlayerAsync(player => player.TogglePlayPauseAsync(cancellationToken), cancellationToken);

    public Task NextTrackAsync(CancellationToken cancellationToken = default)
        => RouteOnActivePlayerAsync(player => player.NextAsync(cancellationToken), cancellationToken);

    public Task PreviousTrackAsync(CancellationToken cancellationToken = default)
        => RouteOnActivePlayerAsync(player => player.PreviousAsync(cancellationToken), cancellationToken);

    public Task SeekAsync(double seconds, double durationSeconds, CancellationToken cancellationToken = default)
    {
        var clamped = Math.Round(Math.Clamp(seconds, 0, Math.Max(0, durationSeconds)));
        return RouteOnActivePlayerAsync(player => player.SeekAsync(clamped, cancellationToken), cancellationToken);
    }

    public Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default)
    {
        var clamped = Math.Clamp(volume, 0, 100);
        return RouteOnActivePlayerAsync(player => player.SetVolumeAsync(clamped, cancellationToken), cancellationToken);
    }

    public Task ToggleMuteAsync(bool currentMuted, CancellationToken cancellationToken = default)
    {
        var nextMuted = !currentMuted;
        return RouteOnActivePlayerAsync(player => player.SetMutedAsync(nextMuted, cancellationToken), cancellationToken);
    }

    public async Task SetPreferredAudioCodecAsync(string codec, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(codec))
        {
            return;
        }

        if (_activePlayer is SendspinPlayerService sendspinPlayer)
        {
            await sendspinPlayer.UpdatePreferredAudioCodecAsync(codec, cancellationToken);
        }
    }

    public Task ToggleShuffleAsync(bool? currentShuffleEnabled, CancellationToken cancellationToken = default)
    {
        var next = !(currentShuffleEnabled ?? false);
        return RouteOnActivePlayerAsync(player => player.SetShuffleAsync(next, cancellationToken), cancellationToken);
    }

    public Task ToggleRepeatModeAsync(string? currentRepeatMode, CancellationToken cancellationToken = default)
    {
        var next = ToRepeatMode(currentRepeatMode) switch
        {
            mashin.Models.RepeatMode.Off => mashin.Models.RepeatMode.All,
            mashin.Models.RepeatMode.All => mashin.Models.RepeatMode.One,
            _ => mashin.Models.RepeatMode.Off
        };

        return RouteOnActivePlayerAsync(player => player.SetRepeatModeAsync(next, cancellationToken), cancellationToken);
    }

    public Task SetDontStopTheMusicAsync(bool enabled, CancellationToken cancellationToken = default)
        => RouteOnActivePlayerAsync(player => player.SetDontStopTheMusicAsync(enabled, cancellationToken), cancellationToken);

    #endregion

    #region Event Handling

    private void OnPlayerPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (!ReferenceEquals(sender, _activePlayer))
        {
            return;
        }

        if (string.IsNullOrWhiteSpace(e.PropertyName)
            || e.PropertyName == nameof(IPlayerService.PlaybackState)
            || e.PropertyName == nameof(IPlayerService.PositionSeconds)
            || e.PropertyName == nameof(IPlayerService.DurationSeconds)
            || e.PropertyName == nameof(IPlayerService.Volume)
            || e.PropertyName == nameof(IPlayerService.IsMuted))
        {
            SyncProjectedStateFromActivePlayer();
        }
    }

    private void OnPlayerQueueChanged(object? sender, PlaybackQueue queue)
    {
        if (!ReferenceEquals(sender, _activePlayer))
        {
            return;
        }

        ApplyPlaybackQueue(queue);
    }

    #endregion

    #region Queue Projection

    private void ApplyPlaybackQueue(PlaybackQueue queue)
    {
        var queueIdChanged = !string.Equals(_playbackQueue.QueueId, Normalize(queue.QueueId), StringComparison.Ordinal);
        if (queueIdChanged)
        {
            _playbackQueue.QueueId = Normalize(queue.QueueId);
        }

        var currentIndexChanged = _playbackQueue.CurrentIndex != queue.CurrentIndex;
        if (currentIndexChanged)
        {
            _playbackQueue.CurrentIndex = queue.CurrentIndex;
            RaisePropertyChanged(nameof(CurrentQueueIndex));
            RaisePropertyChanged(nameof(CurrentQueueItem));
        }

        var currentQueueItemIdChanged = !string.Equals(_playbackQueue.CurrentQueueItemId, Normalize(queue.CurrentQueueItemId), StringComparison.Ordinal);
        if (currentQueueItemIdChanged)
        {
            _playbackQueue.CurrentQueueItemId = Normalize(queue.CurrentQueueItemId);
            RaisePropertyChanged(nameof(CurrentQueueItem));
        }

        var nextItemCount = Math.Max(0, queue.ItemCount);
        var itemCountChanged = _playbackQueue.ItemCount != nextItemCount;
        if (itemCountChanged)
        {
            _playbackQueue.ItemCount = nextItemCount;
            RaisePropertyChanged(nameof(QueueItemCount));
        }

        var shuffleChanged = _playbackQueue.ShuffleEnabled != queue.ShuffleEnabled;
        if (shuffleChanged)
        {
            _playbackQueue.ShuffleEnabled = queue.ShuffleEnabled;
            RaisePropertyChanged(nameof(ShuffleEnabled));
        }

        var repeatChanged = _playbackQueue.RepeatMode != queue.RepeatMode;
        if (repeatChanged)
        {
            _playbackQueue.RepeatMode = queue.RepeatMode;
            RaisePropertyChanged(nameof(RepeatMode));
        }

        var dontStopTheMusicChanged = _playbackQueue.DontStopTheMusicEnabled != queue.DontStopTheMusicEnabled;
        if (dontStopTheMusicChanged)
        {
            _playbackQueue.DontStopTheMusicEnabled = queue.DontStopTheMusicEnabled;
            RaisePropertyChanged(nameof(DontStopTheMusicEnabled));
        }

        var itemsChanged = !QueueItemsSequenceEquals(_playbackQueue.Items, queue.Items);
        if (itemsChanged)
        {
            _playbackQueue.Items.ReplaceRange(queue.Items.Select(CloneQueueItem));
            RaisePropertyChanged(nameof(CurrentQueueItems));
            RaisePropertyChanged(nameof(QueueItemCount));
            RaisePropertyChanged(nameof(CurrentQueueItem));
        }

        if (queueIdChanged)
        {
            // QueueId has no public projection property currently.
        }
    }

    private QueueItem? ResolveCurrentQueueItem()
    {
        var currentItemId = Normalize(_playbackQueue.CurrentQueueItemId);
        if (!string.IsNullOrWhiteSpace(currentItemId))
        {
            var byId = _playbackQueue.Items.FirstOrDefault(item =>
                string.Equals(Normalize(item.QueueItemId), currentItemId, StringComparison.Ordinal));
            if (byId != null)
            {
                return byId;
            }
        }

        if (_playbackQueue.CurrentIndex is int currentIndex
            && currentIndex >= 0
            && currentIndex < _playbackQueue.Items.Count)
        {
            return _playbackQueue.Items[currentIndex];
        }

        return null;
    }

    private static PlaybackQueue CloneQueue(PlaybackQueue source)
    {
        var clone = new PlaybackQueue
        {
            QueueId = source.QueueId,
            CurrentIndex = source.CurrentIndex,
            CurrentQueueItemId = source.CurrentQueueItemId,
            ItemCount = source.ItemCount,
            ShuffleEnabled = source.ShuffleEnabled,
            RepeatMode = source.RepeatMode,
            DontStopTheMusicEnabled = source.DontStopTheMusicEnabled
        };

        clone.Items.ReplaceRange(source.Items.Select(CloneQueueItem));
        return clone;
    }

    private static QueueItem CloneQueueItem(QueueItem source)
    {
        return new QueueItem
        {
            QueueId = source.QueueId,
            QueueItemId = source.QueueItemId,
            Name = source.Name,
            Duration = source.Duration,
            SortIndex = source.SortIndex,
            StreamDetails = source.StreamDetails,
            MediaItem = source.MediaItem,
            Image = source.Image,
            Index = source.Index,
            Available = source.Available,
            ExtraAttributes = source.ExtraAttributes
        };
    }

    private static bool QueueItemsSequenceEquals(IReadOnlyList<QueueItem> currentItems, IReadOnlyList<QueueItem> nextItems)
    {
        if (ReferenceEquals(currentItems, nextItems))
        {
            return true;
        }

        if (currentItems.Count != nextItems.Count)
        {
            return false;
        }

        for (var i = 0; i < currentItems.Count; i++)
        {
            if (!QueueItemEquals(currentItems[i], nextItems[i]))
            {
                return false;
            }
        }

        return true;
    }

    private static bool QueueItemEquals(QueueItem left, QueueItem right)
    {
        if (!string.Equals(left.QueueItemId, right.QueueItemId, StringComparison.Ordinal)
            || !string.Equals(left.QueueId, right.QueueId, StringComparison.Ordinal)
            || !string.Equals(left.Name, right.Name, StringComparison.Ordinal)
            || left.Duration != right.Duration
            || left.SortIndex != right.SortIndex
            || left.Index != right.Index
            || left.Available != right.Available)
        {
            return false;
        }

        var leftTrack = left.MediaItem;
        var rightTrack = right.MediaItem;
        if (!string.Equals(leftTrack?.ItemId, rightTrack?.ItemId, StringComparison.Ordinal)
            || !string.Equals(leftTrack?.Provider, rightTrack?.Provider, StringComparison.Ordinal)
            || !string.Equals(leftTrack?.LocalPath, rightTrack?.LocalPath, StringComparison.Ordinal))
        {
            return false;
        }

        return true;
    }

    #endregion

    #region State Sync

    private void SyncProjectedStateFromActivePlayer()
    {
        Volume = _activePlayer.Volume;
        IsMuted = _activePlayer.IsMuted;
        DurationSeconds = _activePlayer.DurationSeconds;
        PositionSeconds = _activePlayer.PositionSeconds;
        PlaybackState = _activePlayer.PlaybackState;

        if (_activePlayer.Queue != null)
        {
            ApplyPlaybackQueue(_activePlayer.Queue);
        }
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

    private void RaisePropertyChanged(string propertyName)
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
    }

    #endregion

    #region Disposal

    public async ValueTask DisposeAsync()
    {
        foreach (var player in _players.Values.Distinct())
        {
            player.PropertyChanged -= OnPlayerPropertyChanged;
            player.QueueChanged -= OnPlayerQueueChanged;
        }

        foreach (var player in _players.Values.Distinct())
        {
            await player.DisposeAsync();
        }
    }

    #endregion
}
