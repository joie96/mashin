using mashin.Models;
using Microsoft.Extensions.Logging;
using mashin.Audio;
using mashin.Audio.Renderers;
using mashin.Audio.Pipeline;
using mashin.Audio.Sources;
using Sendspin.SDK.Audio;
using Sendspin.SDK.Client;
using Sendspin.SDK.Connection;
using Sendspin.SDK.Models;
using Sendspin.SDK.Protocol;
using Sendspin.SDK.Protocol.Messages;
using System.ComponentModel;
using System.Runtime.CompilerServices;

namespace mashin.Services;

#region Interface

public interface IPlayerService : INotifyPropertyChanged, IAsyncDisposable
{
    PlaybackOutputMode OutputMode { get; }
    string? PlayerId => null;
    Models.PlayerState PlaybackState { get; }
    PlaybackQueue? Queue => null;
    double PositionSeconds { get; }
    double DurationSeconds { get; }
    int Volume { get; }
    bool IsMuted { get; }

    event EventHandler<PlaybackQueue>? QueueChanged
    {
        add { }
        remove { }
    }

    // Lifecycle
    Task ActivateAsync(string? targetPlayerId, CancellationToken cancellationToken = default);
    Task DeactivateAsync();

    // Queue management
    Task SetQueueAsync(PlaybackQueue queue, CancellationToken cancellationToken = default) => Task.CompletedTask;

    // Media Commands
    Task PlayMediaAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default) => Task.CompletedTask;
    Task PlayMediaNextAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default) => Task.CompletedTask;
    Task PlayMediaReplaceNextAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default) => Task.CompletedTask;
    Task PlayMediaLastAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default) => Task.CompletedTask;
    Task PlayMediaRadioNextAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default) => Task.CompletedTask;
    Task ShufflePlayMediaAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default) => Task.CompletedTask;
    Task ClearQueueAsync(bool skipStop = false, CancellationToken cancellationToken = default) => Task.CompletedTask;
    Task PlayQueueIndexAsync(int index, CancellationToken cancellationToken = default) => Task.CompletedTask;
    Task MoveQueueItemAsync(string queueItemId, int posShift, CancellationToken cancellationToken = default) => Task.CompletedTask;
    Task DeleteQueueItemAsync(string queueItemId, CancellationToken cancellationToken = default) => Task.CompletedTask;

    // Transport Commands
    Task TogglePlayPauseAsync(CancellationToken cancellationToken = default);
    Task NextAsync(CancellationToken cancellationToken = default) => Task.CompletedTask;
    Task PreviousAsync(CancellationToken cancellationToken = default) => Task.CompletedTask;
    Task SeekAsync(double seconds, CancellationToken cancellationToken = default);
    Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default);
    Task SetMutedAsync(bool muted, CancellationToken cancellationToken = default);
    Task SetShuffleAsync(bool enabled, CancellationToken cancellationToken = default) => Task.CompletedTask;
    Task SetRepeatModeAsync(mashin.Models.RepeatMode repeatMode, CancellationToken cancellationToken = default) => Task.CompletedTask;
    Task SetDontStopTheMusicAsync(bool enabled, CancellationToken cancellationToken = default) => Task.CompletedTask;

}

#endregion

#region Sendspin Player

public sealed class SendspinPlayerService : IPlayerService, IAsyncDisposable
{
    #region Fields

    private const int PositionTimerIntervalMs = 250;
    private const int MaxBufferingHoldSeconds = 8;
    private const int ReconnectPipelineStableSamples = 3;
    private const int ReconnectPipelinePollIntervalMs = 150;
    private const int ReconnectPipelineMaxWaitMs = 5000;

    private readonly MusicAssistantService _musicAssistant;
    private readonly IMusicAssistantEventHub _musicAssistantEventHub;
    private readonly ILogger<SendspinPlayerService> _logger;
    private readonly SettingsService _settingsService;
    private readonly ISendspinClient _sendspinClient;
    private readonly IAudioRenderer _audioRenderer;
    private readonly IAudioPlayerStateFeed _audioPlayerStateFeed;
    private readonly IAudioPipeline _sendspinAudioPipeline;
    private readonly CancellationTokenSource _disposeCts = new();
    private readonly object _progressSync = new();
    private readonly SemaphoreSlim _reconnectSync = new(1, 1);

    private string? _playerId;
    private Models.PlayerState _playbackState = default!;
    private PlaybackQueue _queue = new();
    private double _positionSeconds;
    private double _durationSeconds;
    private double _bufferedMilliseconds;
    private int _volume;
    private bool _isMuted;

    private bool _isConnected;
    private string? _connectedServerName;
    private string? _activeQueueId;
    private double _lastServerPosition;
    private DateTimeOffset? _lastServerPositionUpdateUtc;
    private readonly Task _progressInterpolationTask;
    private bool _eventHandlersSubscribed;

    // Reconnect Recovery
    private bool _reconnectPending;
    private PlaybackQueue? _reconnectRestoreQueue;
    private int? _reconnectRestoreCurrentIndex;
    private string? _reconnectRestoreCurrentQueueItemId;
    private double _reconnectRestorePositionSeconds;


    #endregion

    #region Construction

    public SendspinPlayerService(
        MusicAssistantService musicAssistant,
        IMusicAssistantEventHub musicAssistantEventHub,
        ILogger<SendspinPlayerService> logger,
        SettingsService settingsService,
        ISendspinClient sendspinClient,
        IAudioRenderer audioRenderer,
        IAudioPlayerStateFeed audioPlayerStateFeed,
        IAudioPipeline sendspinAudioPipeline)
    {
        _musicAssistant = musicAssistant;
        _musicAssistantEventHub = musicAssistantEventHub;
        _logger = logger;
        _settingsService = settingsService;
        _sendspinClient = sendspinClient;
        _audioRenderer = audioRenderer;
        _audioPlayerStateFeed = audioPlayerStateFeed;
        _sendspinAudioPipeline = sendspinAudioPipeline;

        PlayerId = _settingsService.GetSendspinClientId();
        PlaybackState = new Models.PlayerState
        {
            State = PlayerStateType.Idle,
            ActiveSinceUtc = DateTimeOffset.UtcNow
        };
        PositionSeconds = 0;
        DurationSeconds = 0;
        BufferedMilliseconds = 0;
        Volume = 50;
        IsMuted = false;

        _logger.LogDebug("Music Assistant position interpolation task starting for Sendspin player.");

        _progressInterpolationTask = Task.Run(async () =>
        {
            while (!_disposeCts.IsCancellationRequested)
            {
                try
                {
                    await Task.Delay(PositionTimerIntervalMs, _disposeCts.Token);
                }
                catch (OperationCanceledException)
                {
                    break;
                }

                BufferedMilliseconds = _sendspinAudioPipeline.BufferStats?.BufferedMs ?? 0;

                // Set playing state when buffer is filled
                var rendererState = NormalizeLocalAudioPlayerState(_audioPlayerStateFeed.CurrentState);
                if (PlaybackState.State == PlayerStateType.Buffering
                    && PlaybackState.Reason == PlayerStateReason.Reconnect
                    && BufferedMilliseconds > 0
                    && rendererState == PlayerStateType.Playing)
                {
                    PlaybackState = new Models.PlayerState
                    {
                        State = PlayerStateType.Playing,
                        ActiveSinceUtc = DateTimeOffset.UtcNow
                    };
                    UpdateProgressAnchor(PositionSeconds);
                }

                // Set buffering state when buffer is empty 
                if (BufferedMilliseconds <= 0 && PlaybackState.State == PlayerStateType.Playing)
                {
                    PlaybackState = new Models.PlayerState
                    {
                        State = PlayerStateType.Buffering,
                        Reason = PlayerStateReason.Reconnect,
                        ActiveSinceUtc = DateTimeOffset.UtcNow
                    };
                    ResetProgressAnchor();
                    continue;
                }

                // Only interpolate position when playing
                if (PlaybackState.State != PlayerStateType.Playing)
                {
                    continue;
                }

                double anchorPosition;
                DateTimeOffset? anchorUpdatedUtc;
                lock (_progressSync)
                {
                    anchorPosition = _lastServerPosition;
                    anchorUpdatedUtc = _lastServerPositionUpdateUtc;
                }

                if (anchorUpdatedUtc is not DateTimeOffset updatedUtc)
                {
                    continue;
                }

                var elapsedSeconds = Math.Max(0, (DateTimeOffset.UtcNow - updatedUtc).TotalSeconds);
                var interpolated = anchorPosition + elapsedSeconds;
                if (DurationSeconds > 0)
                {
                    interpolated = Math.Min(interpolated, DurationSeconds);
                }

                PositionSeconds = interpolated;
            }
        }, CancellationToken.None);
    }

    #endregion

    #region Eventhandlers

    public event PropertyChangedEventHandler? PropertyChanged;
    public event EventHandler<PlaybackQueue>? QueueChanged;
    public event EventHandler<bool>? ConnectionStateChanged;

    #endregion

    #region Properties

    public PlaybackOutputMode OutputMode => PlaybackOutputMode.Sendspin;

    public string? PlayerId
    {
        get => _playerId;
        private set => SetProperty(ref _playerId, value);
    }

    public Models.PlayerState PlaybackState
    {
        get => _playbackState;
        private set => SetProperty(ref _playbackState, value ?? new Models.PlayerState
        {
            State = PlayerStateType.Unknown,
            ActiveSinceUtc = DateTimeOffset.UtcNow
        });
    }

    public PlaybackQueue? Queue => _queue;

    public double PositionSeconds
    {
        get => _positionSeconds;
        private set => SetProperty(ref _positionSeconds, Math.Max(0, value));
    }

    public double DurationSeconds
    {
        get => _durationSeconds;
        private set => SetProperty(ref _durationSeconds, Math.Max(0, value));
    }

    public double BufferedMilliseconds
    {
        get => _bufferedMilliseconds;
        private set => SetProperty(ref _bufferedMilliseconds, Math.Max(0, value));
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

    public bool IsConnected
    {
        get => _isConnected;
        private set
        {
            if (!SetProperty(ref _isConnected, value))
            {
                return;
            }

            ConnectionStateChanged?.Invoke(this, value);
        }
    }

    #endregion

    #region Lifecycle

    public async Task ActivateAsync(string? targetPlayerId, CancellationToken cancellationToken = default)
    {
        _logger.LogDebug("Sendspin activate requested. TargetPlayerId={TargetPlayerId}, CurrentPlayerId={CurrentPlayerId}", targetPlayerId, PlayerId);
        if (!_eventHandlersSubscribed)
        {
            _sendspinClient.PlayerStateChanged += OnSendspinPlayerStateChanged;
            _sendspinClient.ConnectionStateChanged += OnSendspinConnectionStateChanged;
            _audioPlayerStateFeed.StateChanged += OnLocalAudioPlayerStateChanged;
            _musicAssistantEventHub.QueueEventReceived += OnMusicAssistantQueueEventReceived;
            _eventHandlersSubscribed = true;
        }

        if (!string.IsNullOrWhiteSpace(targetPlayerId))
        {
            PlayerId = targetPlayerId;
        }

        if (string.IsNullOrWhiteSpace(PlayerId))
        {
            PlayerId = _settingsService.GetSendspinClientId();
        }

        _logger.LogDebug("Sendspin activation resolved player id. PlayerId={PlayerId}", PlayerId);

        await Task.CompletedTask;
    }

    public async Task DeactivateAsync()
    {
        _logger.LogDebug("Sendspin deactivate requested. PlayerId={PlayerId}", PlayerId);
        _reconnectPending = false;
        _activeQueueId = null;
        PositionSeconds = 0;
        DurationSeconds = 0;
        ResetProgressAnchor();
        PlaybackState = new Models.PlayerState { State = PlayerStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
        if (_eventHandlersSubscribed)
        {
            _sendspinClient.PlayerStateChanged -= OnSendspinPlayerStateChanged;
            _sendspinClient.ConnectionStateChanged -= OnSendspinConnectionStateChanged;
            _audioPlayerStateFeed.StateChanged -= OnLocalAudioPlayerStateChanged;
            _musicAssistantEventHub.QueueEventReceived -= OnMusicAssistantQueueEventReceived;
            _eventHandlersSubscribed = false;
        }
    }

    public async Task TerminateAsync(CancellationToken cancellationToken = default)
    {
        _logger.LogInformation("Sendspin terminate requested. PlayerId={PlayerId}", PlayerId);

        try
        {
            var queueId = await ResolveQueueIdAsync();
            if (!string.IsNullOrWhiteSpace(queueId))
            {
                await _musicAssistant.ClearQueueAsync(queueId, skipStop: false);
            }
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to clear queue during Sendspin terminate.");
        }

        try
        {
            _audioRenderer.Stop();
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to stop audio renderer during Sendspin terminate.");
        }

        try
        {
            await _sendspinAudioPipeline.StopAsync();
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to stop Sendspin audio pipeline during terminate.");
        }

        try
        {
            await DisconnectAsync(cancellationToken);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to disconnect Sendspin client during terminate.");
        }

        await DeactivateAsync();
    }

    public async ValueTask DisposeAsync()
    {
        if (_eventHandlersSubscribed)
        {
            _sendspinClient.PlayerStateChanged -= OnSendspinPlayerStateChanged;
            _sendspinClient.ConnectionStateChanged -= OnSendspinConnectionStateChanged;
            _audioPlayerStateFeed.StateChanged -= OnLocalAudioPlayerStateChanged;
            _musicAssistantEventHub.QueueEventReceived -= OnMusicAssistantQueueEventReceived;
            _eventHandlersSubscribed = false;
        }

        if (_isConnected)
        {
            await DisconnectAsync();
        }

        _disposeCts.Cancel();
        try
        {
            await _progressInterpolationTask;
        }
        catch (OperationCanceledException)
        {
            // Expected during shutdown.
        }

        _disposeCts.Dispose();
        _reconnectSync.Dispose();
    }



    #endregion

    #region Queue Management

    public async Task SetQueueAsync(PlaybackQueue queue, CancellationToken cancellationToken = default)
    {
        var previousQueue = _queue;

        var nextQueueId = Normalize(queue?.QueueId);
        var queueIdChanged = !string.Equals(Normalize(previousQueue?.QueueId), nextQueueId, StringComparison.Ordinal);
        _activeQueueId = nextQueueId;

        // if no queue provided, clear the queue and reset playback state
        if (queue == null)
        {
            var queueIdToClear = _activeQueueId;
            if (string.IsNullOrWhiteSpace(queueIdToClear))
            {
                queueIdToClear = await ResolveQueueIdAsync();
            }

            if (!string.IsNullOrWhiteSpace(queueIdToClear))
            {
                await _musicAssistant.ClearQueueAsync(queueIdToClear, skipStop: true);
            }

            _queue = new PlaybackQueue();
            _activeQueueId = null;
            DurationSeconds = 0;
            PositionSeconds = 0;
            ResetProgressAnchor();
            PlaybackState = new Models.PlayerState { State = PlayerStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
            return;
        }

        // else set the queue and update playback state accordingly
        var queueId = _activeQueueId;
        if (string.IsNullOrWhiteSpace(queueId))
        {
            queueId = await ResolveQueueIdAsync();
        }

        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        _queue = queue;

        var mediaItems = queue.Items
            .Select(item => item.MediaItem)
            .OfType<MediaItem>()
            .ToList();

        var queueItemsChanged = queueIdChanged || !QueueItemsSequenceEquals(previousQueue?.Items, queue.Items);
        var itemCountChanged = (previousQueue?.ItemCount ?? 0) != queue.ItemCount;

        if ((queueItemsChanged || itemCountChanged) && mediaItems.Count > 0)
        {
            var resolvedItems = await ResolvePlayableMediaItemsAsync(mediaItems);
            if (resolvedItems.Count > 0)
            {
                await _musicAssistant.PlayMediaAsync(queueId, resolvedItems, mashin.Models.QueueOption.ReplaceNext);
            }
        }
        else if ((queueItemsChanged || itemCountChanged) && queue.ItemCount == 0)
        {
            await _musicAssistant.ClearQueueAsync(queueId, skipStop: true);
        }

        if (queue.ShuffleEnabled.HasValue
            && (queueIdChanged || previousQueue?.ShuffleEnabled != queue.ShuffleEnabled))
        {
            await _musicAssistant.SetShuffleAsync(queueId, queue.ShuffleEnabled.Value);
        }

        if (queue.RepeatMode.HasValue
            && (queueIdChanged || previousQueue?.RepeatMode != queue.RepeatMode))
        {
            await _musicAssistant.SetRepeatAsync(queueId, queue.RepeatMode.Value);
        }

        if (queue.DontStopTheMusicEnabled.HasValue
            && (queueIdChanged || previousQueue?.DontStopTheMusicEnabled != queue.DontStopTheMusicEnabled))
        {
            await _musicAssistant.SetDontStopTheMusicAsync(queueId, queue.DontStopTheMusicEnabled.Value);
        }

        if (queue.CurrentIndex is int index
            && index >= 0
            && (queueIdChanged || previousQueue?.CurrentIndex != queue.CurrentIndex))
        {
            //await _musicAssistant.PlayIndexAsync(queueId, index);
        }
    }

    #endregion

    #region Sendspin Commands

    public Task SendCommandAsync(string command, Dictionary<string, object>? parameters = null)
    {
        return _sendspinClient.SendCommandAsync(command, parameters);
    }

    public async Task SetPreferredAudioCodecAsync(string codec, CancellationToken cancellationToken = default)
    {
        var normalizedCodec = codec?.Trim().ToLowerInvariant();
        if (string.IsNullOrWhiteSpace(normalizedCodec))
        {
            return;
        }

        if (normalizedCodec is not "opus" and not "flac" and not "pcm")
        {
            _logger.LogWarning("Ignored unsupported Sendspin codec request. Codec={Codec}", normalizedCodec);
            return;
        }

        // Save Settings
        _settingsService.SetSendspinPreferredAudioCodec(normalizedCodec);

        if (_sendspinClient.ConnectionState != ConnectionState.Connected)
        {
            _logger.LogDebug(
                "Preferred Sendspin codec saved locally but not applied immediately because client is not connected. Codec={Codec}",
                normalizedCodec);
            return;
        }

        cancellationToken.ThrowIfCancellationRequested();

        var wasPlaying = PlaybackState.State == PlayerStateType.Playing;

        // Pause playback       
        if (wasPlaying)
        {
            await _sendspinClient.SendCommandAsync(Commands.Pause);
        }

        // Set Buffering
        PlaybackState = new Models.PlayerState
        {
            State = PlayerStateType.Buffering,
            Reason = PlayerStateReason.Reconnect,
            ActiveSinceUtc = DateTimeOffset.UtcNow
        };
        ResetProgressAnchor();

        var codecRequestSucceeded = false;

        // Set Sendspin codec
        try
        {
            await _sendspinClient.RequestPlayerFormatAsync(codec: normalizedCodec);
            codecRequestSucceeded = true;
            _logger.LogInformation("Requested Sendspin player format change. Codec={Codec}", normalizedCodec);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to request Sendspin player format change. Codec={Codec}", normalizedCodec);
        }

        if (!wasPlaying)
        {
            return;
        }

        // Resume playback with retries
        const int playRetryAttempts = 10;
        const int playRetryDelayMs = 500;
        var resumed = false;

        for (var attempt = 1; attempt <= playRetryAttempts; attempt++)
        {
            cancellationToken.ThrowIfCancellationRequested();

            try
            {
                await _sendspinClient.SendCommandAsync(Commands.Play);
                _logger.LogDebug(
                    "Sent play command after Sendspin codec change. Codec={Codec}, Attempt={Attempt}, CodecRequestSucceeded={CodecRequestSucceeded}",
                    normalizedCodec,
                    attempt,
                    codecRequestSucceeded);
            }
            catch (Exception playEx)
            {
                _logger.LogWarning(
                    playEx,
                    "Play command after Sendspin codec change failed. Codec={Codec}, Attempt={Attempt}, CodecRequestSucceeded={CodecRequestSucceeded}",
                    normalizedCodec,
                    attempt,
                    codecRequestSucceeded);
            }

            await Task.Delay(playRetryDelayMs, cancellationToken);

            if (PlaybackState.State == PlayerStateType.Playing
                || (PlaybackState.State == PlayerStateType.Buffering
                    && PlaybackState.Reason != PlayerStateReason.Reconnect))
            {
                resumed = true;
                break;
            }
        }

        if (!resumed)
        {
            PlaybackState = new Models.PlayerState
            {
                State = PlayerStateType.Idle,
                ActiveSinceUtc = DateTimeOffset.UtcNow
            };

            _logger.LogError(
                "Failed to resume playback after Sendspin codec change. Codec={Codec}, Attempts={Attempts}, CodecRequestSucceeded={CodecRequestSucceeded}",
                normalizedCodec,
                playRetryAttempts,
                codecRequestSucceeded);
        }
    }

    #endregion

    #region Media Commands

    public async Task PlayMediaAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId) || items.Count == 0)
        {
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            return;
        }

        await _sendspinClient.SendCommandAsync(Commands.Pause);
        PlaybackState = new Models.PlayerState
        {
            State = PlayerStateType.Buffering,
            Reason = PlayerStateReason.PlayMedia,
            ActiveSinceUtc = DateTimeOffset.UtcNow
        };
        await _musicAssistant.PlayMediaAsync(queueId, resolvedItems, mashin.Models.QueueOption.Replace);
    }

    public async Task PlayMediaNextAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId) || items.Count == 0)
        {
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            return;
        }

        await _musicAssistant.PlayMediaAsync(queueId, resolvedItems, mashin.Models.QueueOption.Next);
    }

    public async Task PlayMediaReplaceNextAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId) || items.Count == 0)
        {
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            return;
        }

        await _musicAssistant.PlayMediaAsync(queueId, resolvedItems, mashin.Models.QueueOption.ReplaceNext);
    }

    public async Task PlayMediaLastAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId) || items.Count == 0)
        {
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            return;
        }

        await _musicAssistant.PlayMediaAsync(queueId, resolvedItems, mashin.Models.QueueOption.Add);
    }

    public async Task PlayMediaRadioNextAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId) || items.Count == 0)
        {
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            return;
        }

        await _musicAssistant.PlayMediaAsync(
            queueId,
            resolvedItems,
            mashin.Models.QueueOption.Next,
            radioMode: true,
            startItem: resolvedItems[0]);
    }



    public async Task ShufflePlayMediaAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId) || items.Count == 0)
        {
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            return;
        }

        for (var i = resolvedItems.Count - 1; i > 0; i--)
        {
            var j = Random.Shared.Next(i + 1);
            (resolvedItems[i], resolvedItems[j]) = (resolvedItems[j], resolvedItems[i]);
        }

        if (PlaybackState.State == PlayerStateType.Playing)
        {
            await _sendspinClient.SendCommandAsync(Commands.Pause);
        }

        PlaybackState = new Models.PlayerState
        {
            State = PlayerStateType.Buffering,
            Reason = PlayerStateReason.PlayMedia,
            ActiveSinceUtc = DateTimeOffset.UtcNow
        };

        await _musicAssistant.PlayMediaAsync(queueId, resolvedItems, mashin.Models.QueueOption.Replace);
    }

    public async Task ClearQueueAsync(bool skipStop = false, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.ClearQueueAsync(queueId, skipStop);
    }

    public async Task PlayQueueIndexAsync(int index, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.PlayIndexAsync(queueId, index);
    }

    public async Task MoveQueueItemAsync(string queueItemId, int posShift, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId) || string.IsNullOrWhiteSpace(queueItemId))
        {
            return;
        }

        await _musicAssistant.MoveQueueItemAsync(queueId, queueItemId, posShift);
    }

    public async Task DeleteQueueItemAsync(string queueItemId, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId) || string.IsNullOrWhiteSpace(queueItemId))
        {
            return;
        }

        await _musicAssistant.DeleteQueueItemAsync(queueId, queueItemId);
    }

    #endregion

    #region Transport Commands

    public async Task TogglePlayPauseAsync(CancellationToken cancellationToken = default)
    {
        
        if (PlaybackState.State == PlayerStateType.Playing)
        {
                    
            PlaybackState = new Models.PlayerState { State = PlayerStateType.Buffering, ActiveSinceUtc = DateTimeOffset.UtcNow };

            _audioRenderer.Pause();

            if (!_isConnected)
            {
                return;
            }

            _logger.LogInformation("Sendspin command dispatch. Command={Command}, PlayerId={PlayerId}, PlaybackState={PlaybackState}", Commands.Pause, PlayerId, PlaybackState.State);
            await _sendspinClient.SendCommandAsync(Commands.Pause);
            return;
        }

        if (!_isConnected)
        {
            _logger.LogWarning("Play request ignored: Sendspin client is not connected.");
            return;
        }

        PlaybackState = new Models.PlayerState { State = PlayerStateType.Buffering, ActiveSinceUtc = DateTimeOffset.UtcNow };

        _logger.LogInformation("Sendspin command dispatch. Command={Command}, PlayerId={PlayerId}, PlaybackState={PlaybackState}", Commands.Play, PlayerId, PlaybackState.State);
        await _sendspinClient.SendCommandAsync(Commands.Play);
    }

    public async Task NextAsync(CancellationToken cancellationToken = default)
    {
        if (!_isConnected)
        {
            _logger.LogWarning("Next ignored: Sendspin client is not connected.");
            return;
        }

        PlaybackState = new Models.PlayerState { State = PlayerStateType.Buffering, ActiveSinceUtc = DateTimeOffset.UtcNow };

        _logger.LogInformation("Sendspin command dispatch. Command={Command}, PlayerId={PlayerId}", Commands.Next, PlayerId);
        await _sendspinClient.SendCommandAsync(Commands.Next);
    }

    public async Task PreviousAsync(CancellationToken cancellationToken = default)
    {
        if (!_isConnected)
        {
            _logger.LogWarning("Previous ignored: Sendspin client is not connected.");
            return;
        }

        PlaybackState = new Models.PlayerState { State = PlayerStateType.Buffering, ActiveSinceUtc = DateTimeOffset.UtcNow };

        _logger.LogInformation("Sendspin command dispatch. Command={Command}, PlayerId={PlayerId}", Commands.Previous, PlayerId);
        await _sendspinClient.SendCommandAsync(Commands.Previous);
    }

    public Task SeekAsync(double seconds, CancellationToken cancellationToken = default)
    {
        var clamped = Math.Max(0, (int)Math.Round(seconds));
        var targetPlayerId = Normalize(PlayerId);
        if (string.IsNullOrWhiteSpace(targetPlayerId))
        {
            _logger.LogWarning("Seek ignored: no active player id for Sendspin player {PlayerId}", PlayerId);
            return Task.CompletedTask;
        }

        PositionSeconds = clamped;
        PlaybackState = new Models.PlayerState { State = PlayerStateType.Seeking, ActiveSinceUtc = DateTimeOffset.UtcNow };

        _logger.LogInformation("Sendspin seek requested via MusicAssistant. TargetPlayerId={TargetPlayerId}, Seconds={Seconds}", targetPlayerId, clamped);
        return _musicAssistant.PlayerSeekAsync(targetPlayerId, clamped);
    }

    public Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default)
    {
        Volume = volume;
        _logger.LogDebug("Sendspin volume update requested. Volume={Volume}", Volume);
        return _sendspinClient.SetVolumeAsync(Volume);
    }

    public Task SetMutedAsync(bool muted, CancellationToken cancellationToken = default)
    {
        IsMuted = muted;
        _logger.LogDebug("Sendspin mute update requested. IsMuted={IsMuted}", IsMuted);
        return _sendspinClient.SetMuteAsync(IsMuted);
    }

    public async Task SetShuffleAsync(bool enabled, CancellationToken cancellationToken = default)
    {
        _logger.LogInformation("Sendspin command dispatch. Command={Command}, PlayerId={PlayerId}", enabled ? Commands.Shuffle : Commands.Unshuffle, PlayerId);
        await _sendspinClient.SendCommandAsync(enabled ? Commands.Shuffle : Commands.Unshuffle);
    }

    public async Task SetRepeatModeAsync(mashin.Models.RepeatMode repeatMode, CancellationToken cancellationToken = default)
    {
        var command = repeatMode switch
        {
            mashin.Models.RepeatMode.All => Commands.RepeatAll,
            mashin.Models.RepeatMode.One => Commands.RepeatOne,
            _ => Commands.RepeatOff
        };

        _logger.LogInformation("Sendspin command dispatch. Command={Command}, PlayerId={PlayerId}, RepeatMode={RepeatMode}", command, PlayerId, repeatMode);
        await _sendspinClient.SendCommandAsync(command);
    }

    public async Task SetDontStopTheMusicAsync(bool enabled, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.SetDontStopTheMusicAsync(queueId, enabled);
    }

    #endregion

    #region Connection

    public async Task<bool> ConnectAsync(CancellationToken cancellationToken = default)
    {
        _logger.LogInformation("Sendspin player connect requested.");

        if (IsConnected)
        {
            return true;
        }

        if (!Uri.TryCreate(_settingsService.SendspinUrl, UriKind.Absolute, out var configuredServerUri))
        {
            _logger.LogWarning("Sendspin connect skipped because configured URL is invalid. Url={SendspinUrl}", _settingsService.SendspinUrl);
            return false;
        }

        try
        {
            await _sendspinClient.ConnectAsync(configuredServerUri, cancellationToken);
            _connectedServerName = _sendspinClient.ServerName ?? configuredServerUri.Host;
            IsConnected = _sendspinClient.ConnectionState == ConnectionState.Connected;
            PlayerId ??= _settingsService.GetSendspinClientId();

            var currentPlayerState = _sendspinClient.CurrentPlayerState;
            Volume = currentPlayerState.Volume;
            IsMuted = currentPlayerState.Muted;

            await RefreshQueueAsync(cancellationToken);

            _logger.LogInformation(
                "Sendspin player connected. Server={Server}, IsConnected={IsConnected}, PlayerId={PlayerId}",
                _connectedServerName,
                _isConnected,
                PlayerId);

            return IsConnected;
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            throw;
        }
        catch (Exception ex)
        {
            _logger.LogWarning("Sendspin player connection ended. Error={Error}", ex.Message);
            return false;
        }
    }

    public async Task<bool> ReconnectAsync(CancellationToken cancellationToken = default)
    {
        _logger.LogInformation("Sendspin player reconnect requested.");

        IsConnected = false;
        _connectedServerName = null;
        _activeQueueId = null;

        var connected = await ConnectAsync(cancellationToken);
        if (connected)
        {
            await WaitForPipelineStabilizationAsync(cancellationToken);
            await ApplyReconnectRecoveryAsync(cancellationToken);
        }

        return connected;
    }

    private async Task WaitForPipelineStabilizationAsync(CancellationToken cancellationToken)
    {
        var stableSamples = 0;
        var waitedMs = 0;

        while (waitedMs < ReconnectPipelineMaxWaitMs)
        {
            cancellationToken.ThrowIfCancellationRequested();

            var state = _sendspinAudioPipeline.State;
            var stableNow = state != AudioPipelineState.Starting
                && state != AudioPipelineState.Stopping
                && state != AudioPipelineState.Error;

            stableSamples = stableNow ? stableSamples + 1 : 0;
            if (stableSamples >= ReconnectPipelineStableSamples)
            {
                return;
            }

            await Task.Delay(ReconnectPipelinePollIntervalMs, cancellationToken);
            waitedMs += ReconnectPipelinePollIntervalMs;
        }

        _logger.LogDebug(
            "Sendspin reconnect proceeds without full pipeline stabilization. LastState={PipelineState}, WaitedMs={WaitedMs}",
            _sendspinAudioPipeline.State,
            waitedMs);
    }

    public async Task DisconnectAsync(CancellationToken cancellationToken = default)
    {
        if (!IsConnected)
        {
            return;
        }

        await _sendspinClient.DisconnectAsync("client_disconnect");
        IsConnected = false;
        _connectedServerName = null;
        _activeQueueId = null;
        _logger.LogInformation("Sendspin player disconnected.");
    }

    public void SetReconnectSnapshot()
    {
        if (_reconnectPending)
        {
            return;
        }

        _reconnectRestoreQueue = CloneQueue(_queue);
        _reconnectRestoreCurrentIndex = _queue.CurrentIndex;
        _reconnectRestoreCurrentQueueItemId = Normalize(_queue.CurrentQueueItemId);
        _reconnectRestorePositionSeconds = PositionSeconds;
        _reconnectPending = true;
        _logger.LogInformation("Sendspin reconnect recovery marked as pending.");
    }

    public async Task ApplyReconnectRecoveryAsync(CancellationToken cancellationToken = default)
    {

        if (!_reconnectPending || !IsConnected)
        {
            return;
        }

        await _reconnectSync.WaitAsync(cancellationToken);
        try
        {
            if (!_reconnectPending || !IsConnected)
            {
                return;
            }

            var restoreQueue = _reconnectRestoreQueue != null
                ? CloneQueue(_reconnectRestoreQueue)
                : CloneQueue(_queue);
            var restoreCurrentIndex = _reconnectRestoreCurrentIndex ?? _queue.CurrentIndex;
            var restoreCurrentQueueItemId = Normalize(_reconnectRestoreCurrentQueueItemId) ?? Normalize(_queue.CurrentQueueItemId);
            var restorePositionSeconds = _reconnectRestorePositionSeconds > 0
                ? _reconnectRestorePositionSeconds
                : PositionSeconds;

            var mediaItems = restoreQueue.Items
                .Select(item => item.MediaItem)
                .OfType<MediaItem>()
                .ToList();

            var queueRestored = false;
            var indexRestored = false;
            var seekRestored = false;
            var resumedByPlayCommand = false;

            // Check if restoring is needed
            var queueNeedsRestore = !QueueEquals(_queue, restoreQueue);

            var resumeIndex = ResolveResumeIndex(restoreQueue, restoreCurrentIndex, restoreCurrentQueueItemId, mediaItems.Count);
            var indexNeedsRestore = resumeIndex.HasValue
                && (_queue.CurrentIndex != resumeIndex
                    || !string.Equals(Normalize(_queue.CurrentQueueItemId), restoreCurrentQueueItemId, StringComparison.Ordinal));
            var seekNeedsRestore = restorePositionSeconds > 0
                && Math.Abs(PositionSeconds - restorePositionSeconds) > 2;
            var restoreActionsRequired = queueNeedsRestore || indexNeedsRestore || seekNeedsRestore;

            // Pause playback before mutating queue/index/position to avoid racey transport state.
            if (restoreActionsRequired)
            {
                await _sendspinClient.SendCommandAsync(Commands.Pause);
            }

            // Restore queue items
            if (queueNeedsRestore && mediaItems.Count > 0)
            {
                await PlayMediaReplaceNextAsync(mediaItems, cancellationToken);
                queueRestored = true;
            }
            
            // Restore queue index
            if (indexNeedsRestore && resumeIndex is int resumeIndexValue)
            {
                await PlayQueueIndexAsync(resumeIndexValue, cancellationToken);
                indexRestored = true;
            }

            // Restore position (starts also playback))
            if (seekNeedsRestore)
            {
                await SeekAsync(restorePositionSeconds, cancellationToken);
                seekRestored = true;
            }
            else if (restoreActionsRequired)
            {
                await _sendspinClient.SendCommandAsync(Commands.Play);
                resumedByPlayCommand = true;
            }

            var anyRestoreActionExecuted = queueRestored || indexRestored || seekRestored;
            if (!restoreActionsRequired)
            {
                var resumeAnchorSeconds = restorePositionSeconds > 0
                    ? restorePositionSeconds
                    : PositionSeconds;
                PositionSeconds = resumeAnchorSeconds;
                UpdateProgressAnchor(resumeAnchorSeconds);

                _logger.LogInformation(
                    "Reconnect recovery executed no queue/index/seek restore actions. Skipped pause/play transport commands.");
            }
            else if (!anyRestoreActionExecuted && resumedByPlayCommand)
            {
                _logger.LogInformation(
                    "Reconnect recovery had pending transport reset only. No queue/index/seek change applied; playback resumed via play command.");
            }
            else
            {
                _logger.LogInformation(
                    "Reconnect recovery actions. QueueRestored={QueueRestored}, IndexRestored={IndexRestored}, SeekRestored={SeekRestored}, ResumedByPlayCommand={ResumedByPlayCommand}",
                    queueRestored,
                    indexRestored,
                    seekRestored,
                    resumedByPlayCommand);
            }

            _reconnectPending = false;
            _reconnectRestoreQueue = null;
            _reconnectRestoreCurrentIndex = null;
            _reconnectRestoreCurrentQueueItemId = null;
            _reconnectRestorePositionSeconds = 0;

            _logger.LogInformation(
                "Sendspin reconnect recovery applied. ResumeIndex={ResumeIndex}, PositionSeconds={PositionSeconds:F1}, QueueItems={QueueItems}",
                resumeIndex,
                restorePositionSeconds,
                mediaItems.Count);
        }
        finally
        {
            _reconnectSync.Release();
        }
    }

    private static int? ResolveResumeIndex(PlaybackQueue queue, int? currentIndexCandidate, string? currentQueueItemIdCandidate, int queueItemCount)
    {
        if (queueItemCount <= 0)
        {
            return null;
        }

        if (currentIndexCandidate is int currentIndex
            && currentIndex >= 0
            && currentIndex < queueItemCount)
        {
            return currentIndex;
        }

        var currentQueueItemId = Normalize(currentQueueItemIdCandidate);
        if (string.IsNullOrWhiteSpace(currentQueueItemId))
        {
            return null;
        }

        var idx = queue.Items
            .ToList()
            .FindIndex(item => string.Equals(Normalize(item.QueueItemId), currentQueueItemId, StringComparison.Ordinal));

        return idx >= 0 ? idx : null;
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

        clone.Items.ReplaceRange(source.Items.OfType<QueueItem>().Select(CloneQueueItem));
        return clone;
    }

    #endregion

    #region Event Handlers

    // Set Volume and Mute from Sendspin PlayerStateChanged
    private void OnSendspinPlayerStateChanged(object? sender, object state)
    {
        // PlayerState is used as a source for volume and mute state, but not for playback state or position, which are derived from GroupState
        if (state is null)
        {
            _logger.LogDebug("Sendspin PlayerStateChanged ignored because payload is null.");
            return;
        }

        try
        {
            dynamic playerState = state;
            Volume = Convert.ToInt32(playerState.Volume);
            IsMuted = Convert.ToBoolean(playerState.Muted);
            _logger.LogDebug("Sendspin PlayerStateChanged applied. PayloadType={PayloadType}, VolumeRaw={VolumeRaw}, MutedRaw={MutedRaw}, Volume={Volume}, IsMuted={IsMuted}", state.GetType().FullName, (object?)playerState.Volume, (object?)playerState.Muted, Volume, IsMuted);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Sendspin PlayerStateChanged payload could not be read. Type={PayloadType}", state.GetType().FullName);
        }
    }

    private void OnSendspinConnectionStateChanged(object? sender, ConnectionStateChangedEventArgs e)
    {
        var wasConnected = IsConnected;
        var isConnectedNow = e.NewState == ConnectionState.Connected;

        _logger.LogInformation(
            "Sendspin connection state changed. OldState={OldState}, NewState={NewState}, Reason={Reason}, WasConnected={WasConnected}, PlayerId={PlayerId}, ActiveQueueId={ActiveQueueId}",
            e.OldState,
            e.NewState,
            e.Reason,
            wasConnected,
            PlayerId,
            _activeQueueId);

        IsConnected = isConnectedNow;

        var isIntentionalDisconnect = string.Equals(e.Reason, "client_disconnect", StringComparison.OrdinalIgnoreCase);
        if (wasConnected && !isConnectedNow && !isIntentionalDisconnect)
        {
            SetReconnectSnapshot();
        }

        if (!isConnectedNow)
        {
            _connectedServerName = null;
            _activeQueueId = null;
        }
    }

    // Set PlaybackState from local audio renderer state changes
    private void OnLocalAudioPlayerStateChanged(object? sender, PlayerStateType state)
    {
        if (string.IsNullOrWhiteSpace(PlayerId))
        {
            return;
        }

        var mappedState = NormalizeLocalAudioPlayerState(state);

        var isBufferingHoldActive =
            mappedState is PlayerStateType.Idle or PlayerStateType.Paused
            && PlaybackState.State == PlayerStateType.Buffering
            && PlaybackState.Reason is PlayerStateReason.PlayMedia or PlayerStateReason.Reconnect
            && (DateTimeOffset.UtcNow - PlaybackState.ActiveSinceUtc) <= TimeSpan.FromSeconds(MaxBufferingHoldSeconds);

        if (isBufferingHoldActive)
        {
            _logger.LogDebug(
                "Local audio renderer state ignored during buffering hold. SourceState={SourceState}, MappedState={MappedState}, PlayerId={PlayerId}, Reason={Reason}, HoldSeconds={HoldSeconds}",
                state,
                mappedState,
                PlayerId,
                PlaybackState.Reason,
                TimeSpan.FromSeconds(MaxBufferingHoldSeconds).TotalSeconds);
            return;
        }

        PlaybackState = new Models.PlayerState
        {
            State = mappedState,
            ActiveSinceUtc = DateTimeOffset.UtcNow
        };

        _logger.LogDebug("Local audio renderer state applied. SourceState={SourceState}, MappedState={MappedState}, PlayerId={PlayerId}", state, mappedState, PlayerId);

        if (mappedState != PlayerStateType.Playing)
        {
            ResetProgressAnchor();
        }
    }

    // Set Queue, PositionSeconds and DurationSeconds from MusicAssistant queue events
    private async void OnMusicAssistantQueueEventReceived(object? sender, MusicAssistantQueueEvent e)
    {
        if (string.IsNullOrWhiteSpace(PlayerId))
        {
            _logger.LogDebug("MusicAssistant QueueEvent ignored because no active Sendspin player is selected.");
            return;
        }

        var eventQueueId = Normalize(e.Queue?.QueueId) ?? Normalize(e.QueueId);
        if (!string.IsNullOrWhiteSpace(_activeQueueId)
            && !string.IsNullOrWhiteSpace(eventQueueId)
            && !string.Equals(eventQueueId, _activeQueueId, StringComparison.Ordinal))
        {
            _logger.LogDebug("MusicAssistant QueueEvent ignored due to queue mismatch. EventQueueId={EventQueueId}, ActiveQueueId={ActiveQueueId}", eventQueueId, _activeQueueId);
            return;
        }

        if (string.IsNullOrWhiteSpace(_activeQueueId) && !string.IsNullOrWhiteSpace(eventQueueId))
        {
            _activeQueueId = eventQueueId;
        }

        var normalizedQueueId = Normalize(eventQueueId) ?? Normalize(_activeQueueId);
        if (!string.IsNullOrWhiteSpace(normalizedQueueId))
        {
            _queue.QueueId = normalizedQueueId;
        }
        var queue = _queue;

        // Set PositionSeconds from queue_time_updated.
        if (string.Equals(e.Event, "queue_time_updated", StringComparison.OrdinalIgnoreCase))
        {
            if (e.ElapsedTimeSeconds is double elapsedSeconds)
            {
                var clamped = Math.Max(0, elapsedSeconds);
                PositionSeconds = clamped;
                UpdateProgressAnchor(clamped);
                _logger.LogDebug("MusicAssistant queue_time_updated applied. QueueId={QueueId}, ElapsedSecondsRaw={ElapsedSecondsRaw}, ElapsedSecondsClamped={ElapsedSecondsClamped}, ActiveQueueId={ActiveQueueId}", e.QueueId, elapsedSeconds, clamped, _activeQueueId);
            }
            else
            {
                _logger.LogDebug("MusicAssistant queue_time_updated ignored because elapsed time is missing. QueueId={QueueId}, ActiveQueueId={ActiveQueueId}", e.QueueId, _activeQueueId);
            }

            return;
        }

        // Set Queue properties
        if (e.Queue != null)
        {
            _activeQueueId = Normalize(e.Queue.QueueId) ?? _activeQueueId;
            queue.QueueId = Normalize(e.Queue.QueueId) ?? queue.QueueId;
            queue.CurrentIndex = e.Queue.CurrentIndex ?? 0;
            queue.CurrentQueueItemId = Normalize(e.Queue.CurrentItem?.QueueItemId);
            queue.ItemCount = Math.Max(0, e.Queue.ItemCount);
            queue.ShuffleEnabled = e.Queue.ShuffleEnabled;
            queue.RepeatMode = e.Queue.RepeatMode;
            queue.DontStopTheMusicEnabled = e.Queue.DontStopTheMusicEnabled;
        }

        // Set Queue items
        if (string.Equals(e.Event, "queue_items_updated", StringComparison.OrdinalIgnoreCase))
        {
            var queueIdForItems = Normalize(queue.QueueId) ?? Normalize(_activeQueueId);
            if (!string.IsNullOrWhiteSpace(queueIdForItems))
            {
                _logger.LogDebug("MusicAssistant queue_items_updated received. Refreshing queue items. QueueId={QueueId}, ActiveQueueId={ActiveQueueId}", queueIdForItems, _activeQueueId);
                await RefreshQueueItemsAsync(queueIdForItems);
            }
        }

        if (e.QueueSettings != null)
        {
            queue.ShuffleEnabled = e.QueueSettings.ShuffleEnabled;
            queue.RepeatMode = e.QueueSettings.RepeatMode;
            queue.DontStopTheMusicEnabled = e.QueueSettings.DontStopTheMusicEnabled;
        }

        // Set DurationSeconds from queue payload
        if (e.Queue?.CurrentItem?.Duration is int queueItemDuration)
        {
            DurationSeconds = Math.Max(0, queueItemDuration);
        }

        // Set PositionSeconds from queue elapsed time when present.
        if (e.Queue?.ElapsedTime.HasValue == true)
        {
            var clamped = Math.Max(0, e.Queue.ElapsedTime.Value);
            PositionSeconds = clamped;
            UpdateProgressAnchor(clamped);
        }

        // Fire QueueChanged event after updating the queue state
        _queue = queue;
        QueueChanged?.Invoke(this, queue);
        
    }


    #endregion

    #region Helpers

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

    private static string? Normalize(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return null;
        }

        return value.Trim();
    }

    private static bool QueueItemsSequenceEquals(IReadOnlyList<QueueItem>? currentItems, IReadOnlyList<QueueItem>? nextItems)
    {
        if (ReferenceEquals(currentItems, nextItems))
        {
            return true;
        }

        if (currentItems == null || nextItems == null)
        {
            return false;
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

    private static bool QueueEquals(PlaybackQueue? left, PlaybackQueue? right)
    {
        if (ReferenceEquals(left, right))
        {
            return true;
        }

        if (left == null || right == null)
        {
            return false;
        }

        if (!string.Equals(Normalize(left.QueueId), Normalize(right.QueueId), StringComparison.Ordinal)
            || left.CurrentIndex != right.CurrentIndex
            || !string.Equals(Normalize(left.CurrentQueueItemId), Normalize(right.CurrentQueueItemId), StringComparison.Ordinal)
            || left.ItemCount != right.ItemCount
            || left.ShuffleEnabled != right.ShuffleEnabled
            || left.RepeatMode != right.RepeatMode
            || left.DontStopTheMusicEnabled != right.DontStopTheMusicEnabled)
        {
            return false;
        }

        return QueueItemsSequenceEquals(left.Items, right.Items);
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

        var leftMedia = left.MediaItem;
        var rightMedia = right.MediaItem;
        if (!string.Equals(leftMedia?.ItemId, rightMedia?.ItemId, StringComparison.Ordinal)
            || !string.Equals(leftMedia?.Provider, rightMedia?.Provider, StringComparison.Ordinal)
            || !string.Equals(leftMedia?.LocalPath, rightMedia?.LocalPath, StringComparison.Ordinal))
        {
            return false;
        }

        return true;
    }

    private static QueueItem CloneQueueItem(QueueItem? source)
    {
        if (source == null)
        {
            return new QueueItem();
        }

        return new QueueItem
        {
            QueueId = source.QueueId,
            QueueItemId = source.QueueItemId,
            Name = source.Name,
            Duration = source.Duration,
            SortIndex = source.SortIndex,
            Index = source.Index,
            StreamDetails = source.StreamDetails,
            MediaItem = source.MediaItem,
            Image = source.Image,
            Available = source.Available,
            ExtraAttributes = source.ExtraAttributes != null
                ? new Dictionary<string, System.Text.Json.JsonElement>(source.ExtraAttributes)
                : null
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

                    var artistTopTracks = await _musicAssistant.GetArtistTopTracksAsync(artist.ItemId, provider);
                    if (artistTopTracks.Count == 0)
                    {
                        _logger.LogWarning("No top tracks available for artist {ArtistId}. Falling back to artist tracks.", artist.ItemId);
                        var artistTracks = await _musicAssistant.GetArtistTracksAsync(artist.ItemId, provider);
                        resolvedTracks.AddRange(artistTracks.Take(artistTopTracksLimit));
                        break;
                    }

                    resolvedTracks.AddRange(artistTopTracks.Take(artistTopTracksLimit));
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

    private async Task<string?> ResolveQueueIdAsync()
    {
        if (!string.IsNullOrWhiteSpace(_activeQueueId))
        {
            return _activeQueueId;
        }

        if (string.IsNullOrWhiteSpace(PlayerId))
        {
            return null;
        }

        var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(PlayerId);
        _activeQueueId = Normalize(queue?.QueueId);
        return _activeQueueId;
    }

    private async Task RefreshQueueItemsAsync(string queueId)
    {
        var normalizedQueueId = Normalize(queueId);
        if (string.IsNullOrWhiteSpace(normalizedQueueId))
        {
            return;
        }

        try
        {
            var queueItems = await _musicAssistant.GetQueueItemsAsync(
                normalizedQueueId,
                useSortIndexRankForDisplay: _queue.ShuffleEnabled == true);
            if (!string.Equals(normalizedQueueId, Normalize(_activeQueueId), StringComparison.Ordinal))
            {
                return;
            }

            _queue.Items.ReplaceRange(queueItems.OfType<QueueItem>().Select(CloneQueueItem));
            _queue.ItemCount = Math.Max(_queue.ItemCount, _queue.Items.Count);
        }
        catch (Exception ex)
        {
            _logger.LogDebug(ex, "Failed to refresh queue items snapshot for queue {QueueId}.", normalizedQueueId);
        }
    }

    private async Task RefreshQueueAsync(CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(PlayerId))
        {
            return;
        }

        try
        {
            var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(PlayerId);
            if (queue == null)
            {
                return;
            }

            _activeQueueId = Normalize(queue.QueueId);
            _queue.QueueId = _activeQueueId;
            _queue.CurrentIndex = queue.CurrentIndex;
            _queue.CurrentQueueItemId = Normalize(queue.CurrentItem?.QueueItemId);
            _queue.ItemCount = Math.Max(0, queue.ItemCount);
            _queue.ShuffleEnabled = queue.ShuffleEnabled;
            _queue.RepeatMode = queue.RepeatMode;
            _queue.DontStopTheMusicEnabled = queue.DontStopTheMusicEnabled;
            DurationSeconds = Math.Max(0, queue.CurrentItem?.Duration ?? 0);

            if (queue.ElapsedTime.HasValue)
            {
                var clamped = Math.Max(0, queue.ElapsedTime.Value);
                PositionSeconds = clamped;
                UpdateProgressAnchor(clamped);
            }

            if (!string.IsNullOrWhiteSpace(_activeQueueId))
            {
                await RefreshQueueItemsAsync(_activeQueueId);
            }

            QueueChanged?.Invoke(this, _queue);
        }
        catch
        {
            // Snapshot refresh is best-effort.
        }
    }

    private void UpdateProgressAnchor(double positionSeconds)
    {
        var clamped = Math.Max(0, positionSeconds);
        lock (_progressSync)
        {
            _lastServerPosition = clamped;
            _lastServerPositionUpdateUtc = DateTimeOffset.UtcNow;
        }
    }

    private void ResetProgressAnchor()
    {
        lock (_progressSync)
        {
            _lastServerPosition = PositionSeconds;
            _lastServerPositionUpdateUtc = null;
        }
    }

    private static PlayerStateType MapMusicAssistantPlaybackStateFromQueue(mashin.Models.PlaybackState? state)
    {
        return state switch
        {
            mashin.Models.PlaybackState.Playing => PlayerStateType.Playing,
            mashin.Models.PlaybackState.Paused => PlayerStateType.Paused,
            mashin.Models.PlaybackState.Buffering => PlayerStateType.Buffering,
            mashin.Models.PlaybackState.Idle => PlayerStateType.Idle,
            _ => PlayerStateType.Unknown
        };
    }

    private static PlayerStateType MapMusicAssistantPlaybackStateFromPlayer(string? state)
    {
        return state?.Trim().ToLowerInvariant() switch
        {
            "playing" => PlayerStateType.Playing,
            "paused" => PlayerStateType.Paused,
            "buffering" => PlayerStateType.Buffering,
            "idle" => PlayerStateType.Idle,
            _ => PlayerStateType.Unknown
        };
    }

    private static PlayerStateType NormalizeLocalAudioPlayerState(PlayerStateType state)
    {
        return state switch
        {
            PlayerStateType.Uninitialized => PlayerStateType.Idle,
            _ => state
        };
    }

    #endregion
}

#endregion


#region Remote Player

public sealed class RemotePlayerService : IPlayerService, IAsyncDisposable
{
    #region Fields

    private const int PositionTimerIntervalMs = 250;

    private readonly MusicAssistantService _musicAssistant;
    private readonly IMusicAssistantEventHub _musicAssistantEventHub;
    private readonly ILogger<RemotePlayerService> _logger;
    private readonly CancellationTokenSource _disposeCts = new();
    private readonly object _progressSync = new();

    private Models.PlayerState _playbackState = new()
    {
        State = PlayerStateType.Idle,
        ActiveSinceUtc = DateTimeOffset.UtcNow
    };
    private double _positionSeconds;
    private double _durationSeconds;
    private int _volume = 50;
    private bool _isMuted;
    private readonly PlaybackQueue _queue = new();
    private string? _playerId;
    private string? _activeQueueId;
    private double _lastServerPosition;
    private DateTimeOffset? _lastServerPositionUpdateUtc;
    private readonly Task _progressInterpolationTask;
    private bool _eventHandlersSubscribed;

    #endregion

    #region Construction

    public RemotePlayerService(
        MusicAssistantService musicAssistant,
        IMusicAssistantEventHub musicAssistantEventHub,
        ILogger<RemotePlayerService> logger)
    {
        _musicAssistant = musicAssistant;
        _musicAssistantEventHub = musicAssistantEventHub;
        _logger = logger;

        _logger.LogDebug("Music Assistant position interpolation task starting for Remote player.");

        _progressInterpolationTask = Task.Run(async () =>
        {
            while (!_disposeCts.IsCancellationRequested)
            {
                try
                {
                    await Task.Delay(PositionTimerIntervalMs, _disposeCts.Token);
                }
                catch (OperationCanceledException)
                {
                    break;
                }

                if (PlaybackState.State != PlayerStateType.Playing)
                {
                    continue;
                }

                double anchorPosition;
                DateTimeOffset? anchorUpdatedUtc;
                lock (_progressSync)
                {
                    anchorPosition = _lastServerPosition;
                    anchorUpdatedUtc = _lastServerPositionUpdateUtc;
                }

                if (anchorUpdatedUtc is not DateTimeOffset updatedUtc)
                {
                    continue;
                }

                var elapsedSeconds = Math.Max(0, (DateTimeOffset.UtcNow - updatedUtc).TotalSeconds);
                var interpolated = anchorPosition + elapsedSeconds;
                if (DurationSeconds > 0)
                {
                    interpolated = Math.Min(interpolated, DurationSeconds);
                }

                PositionSeconds = interpolated;
            }
        }, CancellationToken.None);
    }

    #endregion

    #region Events

    public event PropertyChangedEventHandler? PropertyChanged;
    public event EventHandler<PlaybackQueue>? QueueChanged;

    #endregion

    #region Properties

    public PlaybackOutputMode OutputMode => PlaybackOutputMode.MA_Remote;

    public string? PlayerId => Normalize(_playerId);

    public PlaybackQueue? Queue => _queue;

    public Models.PlayerState PlaybackState
    {
        get => _playbackState;
        private set => SetProperty(ref _playbackState, value ?? new Models.PlayerState
        {
            State = PlayerStateType.Unknown,
            ActiveSinceUtc = DateTimeOffset.UtcNow
        });
    }

    public double PositionSeconds
    {
        get => _positionSeconds;
        private set => SetProperty(ref _positionSeconds, Math.Max(0, value));
    }

    public double DurationSeconds
    {
        get => _durationSeconds;
        private set => SetProperty(ref _durationSeconds, Math.Max(0, value));
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

    #endregion

    #region Lifecycle

    public async Task ActivateAsync(string? targetPlayerId, CancellationToken cancellationToken = default)
    {
        _logger.LogDebug("Remote activate requested. TargetPlayerId={TargetPlayerId}, CurrentPlayerId={CurrentPlayerId}", targetPlayerId, _playerId);
        if (!_eventHandlersSubscribed)
        {
            _musicAssistantEventHub.QueueEventReceived += OnMusicAssistantQueueEventReceived;
            _musicAssistantEventHub.PlayerEventReceived += OnMusicAssistantPlayerEventReceived;
            _eventHandlersSubscribed = true;
        }
        _playerId = Normalize(targetPlayerId);
        _activeQueueId = null;
        _logger.LogDebug("Remote activation resolved player id. PlayerId={PlayerId}", _playerId);
        await RefreshQueueAsync(cancellationToken);
    }

    public Task DeactivateAsync()
    {
        _logger.LogDebug("Remote deactivate requested. PlayerId={PlayerId}", _playerId);
        if (_eventHandlersSubscribed)
        {
            _musicAssistantEventHub.QueueEventReceived -= OnMusicAssistantQueueEventReceived;
            _musicAssistantEventHub.PlayerEventReceived -= OnMusicAssistantPlayerEventReceived;
            _eventHandlersSubscribed = false;
        }
        _playerId = null;
        _activeQueueId = null;

        _queue.QueueId = null;
        _queue.CurrentIndex = null;
        _queue.CurrentQueueItemId = null;
        _queue.ItemCount = 0;
        _queue.ShuffleEnabled = null;
        _queue.RepeatMode = null;
        _queue.DontStopTheMusicEnabled = null;
        _queue.Items.Clear();

        PositionSeconds = 0;
        DurationSeconds = 0;
        ResetProgressAnchor();
        PlaybackState = new Models.PlayerState { State = PlayerStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
        QueueChanged?.Invoke(this, _queue);
        return Task.CompletedTask;
    }

    public async ValueTask DisposeAsync()
    {
        if (_eventHandlersSubscribed)
        {
            _musicAssistantEventHub.QueueEventReceived -= OnMusicAssistantQueueEventReceived;
            _musicAssistantEventHub.PlayerEventReceived -= OnMusicAssistantPlayerEventReceived;
            _eventHandlersSubscribed = false;
        }

        _disposeCts.Cancel();
        try
        {
            await _progressInterpolationTask;
        }
        catch (OperationCanceledException)
        {
            // Expected during shutdown.
        }

        _disposeCts.Dispose();
    }

    #endregion

    #region Queue Management

    public Task SetQueueAsync(PlaybackQueue queue, CancellationToken cancellationToken = default)
    {
        if (queue == null)
        {
            _queue.QueueId = null;
            _queue.CurrentIndex = null;
            _queue.CurrentQueueItemId = null;
            _queue.ItemCount = 0;
            _queue.ShuffleEnabled = null;
            _queue.RepeatMode = null;
            _queue.DontStopTheMusicEnabled = null;
            _queue.Items.Clear();
            QueueChanged?.Invoke(this, _queue);
            return Task.CompletedTask;
        }

        _queue.QueueId = Normalize(queue.QueueId);
        _queue.CurrentIndex = queue.CurrentIndex;
        _queue.CurrentQueueItemId = Normalize(queue.CurrentQueueItemId);
        _queue.ItemCount = Math.Max(0, queue.ItemCount);
        _queue.ShuffleEnabled = queue.ShuffleEnabled;
        _queue.RepeatMode = queue.RepeatMode;
        _queue.DontStopTheMusicEnabled = queue.DontStopTheMusicEnabled;
        _queue.Items.ReplaceRange(queue.Items.Select(CloneQueueItem));
        QueueChanged?.Invoke(this, _queue);
        return Task.CompletedTask;
    }

    #endregion

    #region Media Commands

    public async Task PlayMediaAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId) || items.Count == 0)
        {
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            return;
        }

        await _musicAssistant.PauseAsync(queueId);
        await _musicAssistant.PlayMediaAsync(queueId, resolvedItems, mashin.Models.QueueOption.Replace);
    }

    public async Task PlayMediaNextAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId) || items.Count == 0)
        {
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            return;
        }

        await _musicAssistant.PlayMediaAsync(queueId, resolvedItems, mashin.Models.QueueOption.Next);
    }

    public async Task PlayMediaReplaceNextAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId) || items.Count == 0)
        {
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            return;
        }

        await _musicAssistant.PlayMediaAsync(queueId, resolvedItems, mashin.Models.QueueOption.ReplaceNext);
    }

    public async Task PlayMediaLastAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId) || items.Count == 0)
        {
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            return;
        }

        await _musicAssistant.PlayMediaAsync(queueId, resolvedItems, mashin.Models.QueueOption.Add);
    }

    public async Task PlayMediaRadioNextAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId) || items.Count == 0)
        {
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            return;
        }

        await _musicAssistant.PlayMediaAsync(
            queueId,
            resolvedItems,
            mashin.Models.QueueOption.Next,
            radioMode: true,
            startItem: resolvedItems[0]);
    }

    public async Task ShufflePlayMediaAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId) || items.Count == 0)
        {
            return;
        }

        var resolvedItems = await ResolvePlayableMediaItemsAsync(items);
        if (resolvedItems.Count == 0)
        {
            return;
        }

        for (var i = resolvedItems.Count - 1; i > 0; i--)
        {
            var j = Random.Shared.Next(i + 1);
            (resolvedItems[i], resolvedItems[j]) = (resolvedItems[j], resolvedItems[i]);
        }

        await _musicAssistant.PauseAsync(queueId);
        await _musicAssistant.PlayMediaAsync(queueId, resolvedItems, mashin.Models.QueueOption.Replace);
    }

    public async Task ClearQueueAsync(bool skipStop = false, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.ClearQueueAsync(queueId, skipStop);
    }

    public async Task PlayQueueIndexAsync(int index, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.PlayIndexAsync(queueId, index);
    }

    public async Task MoveQueueItemAsync(string queueItemId, int posShift, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId) || string.IsNullOrWhiteSpace(queueItemId))
        {
            return;
        }

        await _musicAssistant.MoveQueueItemAsync(queueId, queueItemId, posShift);
    }

    public async Task DeleteQueueItemAsync(string queueItemId, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId) || string.IsNullOrWhiteSpace(queueItemId))
        {
            return;
        }

        await _musicAssistant.DeleteQueueItemAsync(queueId, queueItemId);
    }

    #endregion

    #region Transport Commands

    public Task TogglePlayPauseAsync(CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(_playerId))
        {
            _logger.LogWarning("TogglePlayPause ignored: no active remote player is selected.");
            return Task.CompletedTask;
        }

        _logger.LogInformation("Remote command dispatch. Command={Command}, PlayerId={PlayerId}", "player_play_pause", _playerId);
        return _musicAssistant.PlayerPlayPauseAsync(_playerId);
    }

    public async Task NextAsync(CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(_playerId))
        {
            _logger.LogWarning("Next ignored: no active remote player is selected.");
            return;
        }

        _logger.LogInformation("Remote command dispatch. Command={Command}, PlayerId={PlayerId}", "player_next", _playerId);
        await _musicAssistant.PlayerNextAsync(_playerId);
    }

    public async Task PreviousAsync(CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(_playerId))
        {
            _logger.LogWarning("Previous ignored: no active remote player is selected.");
            return;
        }

        _logger.LogInformation("Remote command dispatch. Command={Command}, PlayerId={PlayerId}", "player_previous", _playerId);
        await _musicAssistant.PlayerPreviousAsync(_playerId);
    }

    public Task SeekAsync(double seconds, CancellationToken cancellationToken = default)
    {
        var clamped = Math.Max(0, (int)Math.Round(seconds));
        if (string.IsNullOrWhiteSpace(_playerId))
        {
            _logger.LogWarning("Seek ignored: no active remote player is selected.");
            return Task.CompletedTask;
        }

        PositionSeconds = clamped;
        UpdateProgressAnchor(clamped);
        _logger.LogInformation("Remote seek requested via MusicAssistant. PlayerId={PlayerId}, Seconds={Seconds}", _playerId, clamped);
        return _musicAssistant.PlayerSeekAsync(_playerId, clamped);
    }

    public Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default)
    {
        Volume = volume;
        _logger.LogDebug("Remote volume update requested. Volume={Volume}", Volume);
        if (string.IsNullOrWhiteSpace(_playerId))
        {
            return Task.CompletedTask;
        }

        return _musicAssistant.SetPlayerVolumeAsync(_playerId, Volume);
    }

    public Task SetMutedAsync(bool muted, CancellationToken cancellationToken = default)
    {
        IsMuted = muted;
        _logger.LogDebug("Remote mute update requested. IsMuted={IsMuted}", IsMuted);
        if (string.IsNullOrWhiteSpace(_playerId))
        {
            return Task.CompletedTask;
        }

        return _musicAssistant.SetPlayerMuteAsync(_playerId, IsMuted);
    }

    public async Task SetShuffleAsync(bool enabled, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            _logger.LogWarning("SetShuffle ignored: no active queue id is available for remote player {PlayerId}", _playerId);
            return;
        }

        _logger.LogInformation("Remote queue setting update requested. Setting={Setting}, Value={Value}, QueueId={QueueId}, PlayerId={PlayerId}", "shuffle", enabled, queueId, _playerId);
        await _musicAssistant.SetShuffleAsync(queueId, enabled);
    }

    public async Task SetRepeatModeAsync(mashin.Models.RepeatMode repeatMode, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            _logger.LogWarning("SetRepeatMode ignored: no active queue id is available for remote player {PlayerId}", _playerId);
            return;
        }

        _logger.LogInformation("Remote queue setting update requested. Setting={Setting}, Value={Value}, QueueId={QueueId}, PlayerId={PlayerId}", "repeat", repeatMode, queueId, _playerId);
        await _musicAssistant.SetRepeatAsync(queueId, repeatMode);
    }

    public async Task SetDontStopTheMusicAsync(bool enabled, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            _logger.LogWarning("SetDontStopTheMusic ignored: no active queue id is available for remote player {PlayerId}", _playerId);
            return;
        }

        _logger.LogInformation("Remote queue setting update requested. Setting={Setting}, Value={Value}, QueueId={QueueId}, PlayerId={PlayerId}", "dont_stop_the_music", enabled, queueId, _playerId);
        await _musicAssistant.SetDontStopTheMusicAsync(queueId, enabled);
    }

    #endregion

    #region Event Handlers

    private void OnMusicAssistantPlayerEventReceived(object? sender, MusicAssistantPlayerEvent e)
    {
        if (string.IsNullOrWhiteSpace(_playerId))
        {
            _logger.LogDebug("MusicAssistant PlayerEvent ignored because no active remote player is selected.");
            return;
        }

        var eventPlayerId = Normalize(e.Player?.PlayerId) ?? Normalize(e.PlayerId) ?? Normalize(e.PlayerConfig?.PlayerId);
        if (!string.IsNullOrWhiteSpace(eventPlayerId)
            && !string.Equals(eventPlayerId, _playerId, StringComparison.Ordinal))
        {
            _logger.LogDebug("MusicAssistant PlayerEvent ignored due to player mismatch. EventPlayerId={EventPlayerId}, ActivePlayerId={ActivePlayerId}", eventPlayerId, _playerId);
            return;
        }

        if (e.Player == null)
        {
            _logger.LogDebug("MusicAssistant PlayerEvent ignored because payload does not include player details. Event={EventName}, PlayerId={PlayerId}", e.Event, e.PlayerId);
            return;
        }

        _playerId = Normalize(e.Player.PlayerId) ?? _playerId;

        if (e.Player.VolumeLevel.HasValue)
        {
            Volume = e.Player.VolumeLevel.Value;
        }

        if (e.Player.VolumeMuted.HasValue)
        {
            IsMuted = e.Player.VolumeMuted.Value;
        }

        PlaybackState = new Models.PlayerState
        {
            State = MapMusicAssistantPlaybackStateFromPlayer(e.Player.State),
            ActiveSinceUtc = DateTimeOffset.UtcNow
        };

        if (PlaybackState.State != PlayerStateType.Playing)
        {
            ResetProgressAnchor();
        }

        _logger.LogDebug("MusicAssistant PlayerEvent processed. Event={EventName}, PlayerId={PlayerId}, PlaybackState={PlaybackState}, Volume={Volume}, IsMuted={IsMuted}", e.Event, _playerId, PlaybackState.State, Volume, IsMuted);
    }

    private async void OnMusicAssistantQueueEventReceived(object? sender, MusicAssistantQueueEvent e)
    {
        if (string.IsNullOrWhiteSpace(_playerId))
        {
            _logger.LogDebug("MusicAssistant QueueEvent ignored because no active remote player is selected.");
            return;
        }

        var eventQueueId = Normalize(e.Queue?.QueueId) ?? Normalize(e.QueueId);
        if (!string.IsNullOrWhiteSpace(_activeQueueId)
            && !string.IsNullOrWhiteSpace(eventQueueId)
            && !string.Equals(eventQueueId, _activeQueueId, StringComparison.Ordinal))
        {
            _logger.LogDebug("MusicAssistant QueueEvent ignored due to queue mismatch. EventQueueId={EventQueueId}, ActiveQueueId={ActiveQueueId}", eventQueueId, _activeQueueId);
            return;
        }

        if (string.IsNullOrWhiteSpace(_activeQueueId) && !string.IsNullOrWhiteSpace(eventQueueId))
        {
            _activeQueueId = eventQueueId;
        }

        var normalizedQueueId = Normalize(eventQueueId) ?? Normalize(_activeQueueId);
        if (!string.IsNullOrWhiteSpace(normalizedQueueId))
        {
            _queue.QueueId = normalizedQueueId;
        }

        if (string.Equals(e.Event, "queue_time_updated", StringComparison.OrdinalIgnoreCase))
        {
            if (e.ElapsedTimeSeconds is double elapsedSeconds)
            {
                var clamped = Math.Max(0, elapsedSeconds);
                PositionSeconds = clamped;
                UpdateProgressAnchor(clamped);
                _logger.LogDebug("MusicAssistant QueueEvent applied. Event={EventName}, QueueId={QueueId}, ElapsedSecondsRaw={ElapsedSecondsRaw}, ElapsedSecondsClamped={ElapsedSecondsClamped}, ActiveQueueId={ActiveQueueId}", e.Event, e.QueueId, elapsedSeconds, clamped, _activeQueueId);
            }
            else
            {
                _logger.LogDebug("MusicAssistant QueueEvent ignored because elapsed time is missing. Event={EventName}, QueueId={QueueId}, ActiveQueueId={ActiveQueueId}", e.Event, e.QueueId, _activeQueueId);
            }

            return;
        }

        if (e.Queue != null)
        {
            _activeQueueId = Normalize(e.Queue.QueueId) ?? _activeQueueId;
            _queue.QueueId = Normalize(e.Queue.QueueId) ?? _queue.QueueId;
            _queue.CurrentIndex = e.Queue.CurrentIndex;
            _queue.CurrentQueueItemId = Normalize(e.Queue.CurrentItem?.QueueItemId);
            _queue.ItemCount = Math.Max(0, e.Queue.ItemCount);
            _queue.ShuffleEnabled = e.Queue.ShuffleEnabled;
            _queue.RepeatMode = e.Queue.RepeatMode;
            _queue.DontStopTheMusicEnabled = e.Queue.DontStopTheMusicEnabled;

            PlaybackState = new Models.PlayerState
            {
                State = MapMusicAssistantPlaybackStateFromQueue(e.Queue.State),
                ActiveSinceUtc = DateTimeOffset.UtcNow
            };

            if (e.Queue.ElapsedTime.HasValue)
            {
                var clamped = Math.Max(0, e.Queue.ElapsedTime.Value);
                PositionSeconds = clamped;
                UpdateProgressAnchor(clamped);
            }

            DurationSeconds = Math.Max(0, e.Queue.CurrentItem?.Duration ?? 0);
        }

        if (string.Equals(e.Event, "queue_items_updated", StringComparison.OrdinalIgnoreCase))
        {
            var queueIdForItems = Normalize(_queue.QueueId) ?? Normalize(_activeQueueId);
            if (!string.IsNullOrWhiteSpace(queueIdForItems))
            {
                _logger.LogDebug("MusicAssistant queue_items_updated received. Refreshing queue items. QueueId={QueueId}, ActiveQueueId={ActiveQueueId}", queueIdForItems, _activeQueueId);
                await RefreshQueueItemsAsync(queueIdForItems);
            }
        }

        if (e.QueueSettings != null)
        {
            _queue.ShuffleEnabled = e.QueueSettings.ShuffleEnabled;
            _queue.RepeatMode = e.QueueSettings.RepeatMode;
            _queue.DontStopTheMusicEnabled = e.QueueSettings.DontStopTheMusicEnabled;
        }

        QueueChanged?.Invoke(this, _queue);

        if (PlaybackState.State != PlayerStateType.Playing)
        {
            ResetProgressAnchor();
        }

        _logger.LogDebug("MusicAssistant QueueEvent processed. Event={EventName}, QueueId={QueueId}, DurationSeconds={DurationSeconds}, PositionSeconds={PositionSeconds}, ActiveQueueId={ActiveQueueId}", e.Event, e.QueueId, DurationSeconds, PositionSeconds, _activeQueueId);
    }

    #endregion

    #region Helpers

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

    private async Task<string?> ResolveQueueIdAsync()
    {
        if (!string.IsNullOrWhiteSpace(_activeQueueId))
        {
            return _activeQueueId;
        }

        if (string.IsNullOrWhiteSpace(_playerId))
        {
            return null;
        }

        var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(_playerId);
        _activeQueueId = Normalize(queue?.QueueId);
        return _activeQueueId;
    }

    private async Task RefreshQueueItemsAsync(string queueId)
    {
        var normalizedQueueId = Normalize(queueId);
        if (string.IsNullOrWhiteSpace(normalizedQueueId))
        {
            return;
        }

        try
        {
            var queueItems = await _musicAssistant.GetQueueItemsAsync(
                normalizedQueueId,
                useSortIndexRankForDisplay: _queue.ShuffleEnabled == true);
            if (!string.Equals(normalizedQueueId, Normalize(_activeQueueId), StringComparison.Ordinal))
            {
                return;
            }

            _queue.Items.ReplaceRange(queueItems.OfType<QueueItem>().Select(CloneQueueItem));
            _queue.ItemCount = Math.Max(_queue.ItemCount, _queue.Items.Count);
        }
        catch (Exception ex)
        {
            _logger.LogDebug(ex, "Failed to refresh queue items snapshot for queue {QueueId}.", normalizedQueueId);
        }
    }

    private async Task RefreshQueueAsync(CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(_playerId))
        {
            return;
        }

        try
        {
            var player = await _musicAssistant.GetPlayerAsync(_playerId);
            if (player != null)
            {
                if (player.VolumeLevel.HasValue)
                {
                    Volume = player.VolumeLevel.Value;
                }

                if (player.VolumeMuted.HasValue)
                {
                    IsMuted = player.VolumeMuted.Value;
                }

                PlaybackState = new Models.PlayerState
                {
                    State = MapMusicAssistantPlaybackStateFromPlayer(player.State),
                    ActiveSinceUtc = DateTimeOffset.UtcNow
                };
            }

            var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(_playerId);
            if (queue == null)
            {
                return;
            }

            _activeQueueId = Normalize(queue.QueueId);
            _queue.QueueId = _activeQueueId;
            _queue.CurrentIndex = queue.CurrentIndex;
            _queue.CurrentQueueItemId = Normalize(queue.CurrentItem?.QueueItemId);
            _queue.ItemCount = Math.Max(0, queue.ItemCount);
            _queue.ShuffleEnabled = queue.ShuffleEnabled;
            _queue.RepeatMode = queue.RepeatMode;
            _queue.DontStopTheMusicEnabled = queue.DontStopTheMusicEnabled;

            DurationSeconds = Math.Max(0, queue.CurrentItem?.Duration ?? 0);
            if (queue.ElapsedTime.HasValue)
            {
                var clamped = Math.Max(0, queue.ElapsedTime.Value);
                PositionSeconds = clamped;
                UpdateProgressAnchor(clamped);
            }

            PlaybackState = new Models.PlayerState
            {
                State = MapMusicAssistantPlaybackStateFromQueue(queue.State),
                ActiveSinceUtc = DateTimeOffset.UtcNow
            };

            if (!string.IsNullOrWhiteSpace(_activeQueueId))
            {
                await RefreshQueueItemsAsync(_activeQueueId);
            }

            QueueChanged?.Invoke(this, _queue);

            if (PlaybackState.State != PlayerStateType.Playing)
            {
                ResetProgressAnchor();
            }
        }
        catch
        {
            // Snapshot refresh is best-effort.
        }
    }

    private static QueueItem CloneQueueItem(QueueItem? source)
    {
        if (source == null)
        {
            return new QueueItem();
        }

        return new QueueItem
        {
            QueueId = source.QueueId,
            QueueItemId = source.QueueItemId,
            Name = source.Name,
            Duration = source.Duration,
            SortIndex = source.SortIndex,
            Index = source.Index,
            StreamDetails = source.StreamDetails,
            MediaItem = source.MediaItem,
            Image = source.Image,
            Available = source.Available,
            ExtraAttributes = source.ExtraAttributes != null
                ? new Dictionary<string, System.Text.Json.JsonElement>(source.ExtraAttributes)
                : null
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
                        break;
                    }

                    var artistTopTracks = await _musicAssistant.GetArtistTopTracksAsync(artist.ItemId, provider);
                    if (artistTopTracks.Count == 0)
                    {
                        _logger.LogWarning("No top tracks available for artist {ArtistId}. Falling back to artist tracks.", artist.ItemId);
                        var artistTracks = await _musicAssistant.GetArtistTracksAsync(artist.ItemId, provider);
                        resolvedTracks.AddRange(artistTracks.Take(artistTopTracksLimit));
                        break;
                    }

                    resolvedTracks.AddRange(artistTopTracks.Take(artistTopTracksLimit));
                    break;
                }
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

    private void UpdateProgressAnchor(double positionSeconds)
    {
        var clamped = Math.Max(0, positionSeconds);
        lock (_progressSync)
        {
            _lastServerPosition = clamped;
            _lastServerPositionUpdateUtc = DateTimeOffset.UtcNow;
        }
    }

    private void ResetProgressAnchor()
    {
        lock (_progressSync)
        {
            _lastServerPosition = PositionSeconds;
            _lastServerPositionUpdateUtc = null;
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

    private static PlayerStateType MapMusicAssistantPlaybackStateFromQueue(mashin.Models.PlaybackState? state)
    {
        return state switch
        {
            mashin.Models.PlaybackState.Playing => PlayerStateType.Playing,
            mashin.Models.PlaybackState.Paused => PlayerStateType.Paused,
            mashin.Models.PlaybackState.Buffering => PlayerStateType.Buffering,
            mashin.Models.PlaybackState.Idle => PlayerStateType.Idle,
            _ => PlayerStateType.Unknown
        };
    }

    private static PlayerStateType MapMusicAssistantPlaybackStateFromPlayer(string? state)
    {
        return state?.Trim().ToLowerInvariant() switch
        {
            "playing" => PlayerStateType.Playing,
            "paused" => PlayerStateType.Paused,
            "buffering" => PlayerStateType.Buffering,
            "idle" => PlayerStateType.Idle,
            _ => PlayerStateType.Unknown
        };
    }

    #endregion
}

#endregion


#region Local Audio Player

public sealed class LocalAudioPlayerService : IPlayerService, IAsyncDisposable
{
    #region Fields

    private readonly ILogger<LocalAudioPlayerService> _logger;
    private readonly IAudioPipeline _audioPipeline;
    private readonly IAudioRenderer _audioRenderer;
    private readonly LocalAudioChunkSource _localAudioChunkSource;

    private Models.PlayerState _playbackState = new()
    {
        State = PlayerStateType.Idle,
        ActiveSinceUtc = DateTimeOffset.UtcNow
    };
    private double _positionSeconds;
    private double _durationSeconds;
    private int _volume = 50;
    private bool _isMuted;
    private readonly PlaybackQueue _queue = new();
    private QueueItem? _currentQueueItem;
    private string? _sourcePath;
    private LocalAudioChunkStream? _chunkStream;
    private CancellationTokenSource? _chunkFeedCts;
    private Task? _chunkFeedTask;
    private bool _pipelineStarted;

    #endregion

    #region Construction

    public LocalAudioPlayerService(
        ILogger<LocalAudioPlayerService> logger,
        IAudioPipeline audioPipeline,
        IAudioRenderer audioRenderer,
        LocalAudioChunkSource localAudioChunkSource)
    {
        _logger = logger;
        _audioPipeline = audioPipeline;
        _audioRenderer = audioRenderer;
        _localAudioChunkSource = localAudioChunkSource;

        _audioRenderer.StateChanged += OnAudioRendererStateChanged;
        _audioRenderer.ErrorOccurred += OnAudioRendererErrorOccurred;
    }

    #endregion

    #region Events

    public event PropertyChangedEventHandler? PropertyChanged;

    public event EventHandler<QueueItem?>? CurrentPlayingItemEnded;

    #endregion

    #region Properties

    public PlaybackOutputMode OutputMode => PlaybackOutputMode.Local;

    public string? PlayerId => null;

    public PlaybackQueue? Queue => _queue;

    public Models.PlayerState PlaybackState
    {
        get => _playbackState;
        private set => SetProperty(ref _playbackState, value);
    }

    public double PositionSeconds
    {
        get => _positionSeconds;
        private set => SetProperty(ref _positionSeconds, Math.Max(0, value));
    }

    public double DurationSeconds
    {
        get => _durationSeconds;
        private set => SetProperty(ref _durationSeconds, Math.Max(0, value));
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

    public QueueItem? CurrentQueueItem
    {
        get => _currentQueueItem;
        private set
        {
            if (ReferenceEquals(_currentQueueItem, value))
            {
                return;
            }

            _currentQueueItem = value;
        }
    }

    #endregion

    #region Commands

    public Task ActivateAsync(string? targetPlayerId, CancellationToken cancellationToken = default)
    {
        _sourcePath = NormalizeLocalPath(targetPlayerId);
        return Task.CompletedTask;
    }

    public async Task DeactivateAsync()
    {
        await StopPipelinePlaybackAsync();
        _audioRenderer.Stop();
        _chunkStream = null;
        CurrentQueueItem = null;
        PositionSeconds = 0;
        DurationSeconds = 0;
        PlaybackState = new Models.PlayerState { State = PlayerStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
    }

    public Task PlayMediaRadioNextAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default)
    {
        _logger.LogDebug("PlayMediaRadioNextAsync is not supported in local playback mode.");
        return Task.CompletedTask;
    }

    public async Task TogglePlayPauseAsync(CancellationToken cancellationToken = default)
    {
        if (PlaybackState.State == PlayerStateType.Playing)
        {
            _audioRenderer.Pause();
            return;
        }

        if (!_pipelineStarted)
        {
            if (string.IsNullOrWhiteSpace(_sourcePath))
            {
                _logger.LogWarning("Local playback requested without a configured source path.");
                return;
            }

            await ConfigureTrackAsync(_sourcePath, 0, cancellationToken);
        }

        _audioRenderer.Play();
    }

    public async Task SeekAsync(double seconds, CancellationToken cancellationToken = default)
    {
        if (_chunkStream == null)
        {
            return;
        }

        var clamped = Math.Max(0, Math.Min(seconds, _chunkStream.DurationSeconds));
        await ConfigureTrackAsync(_chunkStream.SourcePath, clamped, cancellationToken);
        PositionSeconds = clamped;

        if (PlaybackState.State == PlayerStateType.Playing || PlaybackState.State == PlayerStateType.Buffering)
        {
            _audioRenderer.Play();
        }
    }

    public Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default)
    {
        Volume = volume;
        _audioRenderer.Volume = Volume / 100f;
        if (_pipelineStarted)
        {
            _audioPipeline.SetVolume(Volume);
        }

        return Task.CompletedTask;
    }

    public Task SetMutedAsync(bool muted, CancellationToken cancellationToken = default)
    {
        IsMuted = muted;
        _audioRenderer.IsMuted = muted;
        if (_pipelineStarted)
        {
            _audioPipeline.SetMuted(muted);
        }

        return Task.CompletedTask;
    }

    public async ValueTask DisposeAsync()
    {
        _audioRenderer.StateChanged -= OnAudioRendererStateChanged;
        _audioRenderer.ErrorOccurred -= OnAudioRendererErrorOccurred;
        await StopPipelinePlaybackAsync();
    }

    public async Task SetSourceAsync(string sourcePath, double startSeconds = 0, CancellationToken cancellationToken = default)
    {
        var normalized = NormalizeLocalPath(sourcePath);
        if (string.IsNullOrWhiteSpace(normalized))
        {
            _logger.LogWarning("SetSourceAsync ignored because no valid local source path was provided.");
            return;
        }

        _sourcePath = normalized;
        await ConfigureTrackAsync(normalized, startSeconds, cancellationToken);
    }

    public void UpdateCurrentQueueItem(QueueItem? queueItem)
    {
        CurrentQueueItem = queueItem;
    }

    #endregion

    #region Helpers

    private async Task ConfigureTrackAsync(string sourcePath, double startSeconds, CancellationToken cancellationToken)
    {
        await StopPipelinePlaybackAsync();

        _chunkStream = _localAudioChunkSource.ReadChunks(sourcePath, startSeconds);

        var pipelineFormat = new Sendspin.SDK.Models.AudioFormat
        {
            Codec = "pcm",
            SampleRate = _chunkStream.Format.SampleRate,
            Channels = _chunkStream.Format.Channels,
            BitDepth = _chunkStream.Format.BitDepth,
            Bitrate = _chunkStream.Format.Bitrate
        };

        await _audioPipeline.StartAsync(pipelineFormat, cancellationToken: cancellationToken);
        _pipelineStarted = true;
        _audioPipeline.SetVolume(Volume);
        _audioPipeline.SetMuted(IsMuted);

        _chunkFeedCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        _chunkFeedTask = Task.Run(() => FeedPipelineChunksAsync(_chunkStream, _chunkFeedCts.Token), _chunkFeedCts.Token);

        DurationSeconds = _chunkStream.DurationSeconds;
        PositionSeconds = Math.Max(0, Math.Min(startSeconds, DurationSeconds));
        PlaybackState = new Models.PlayerState { State = PlayerStateType.Buffering, ActiveSinceUtc = DateTimeOffset.UtcNow };

        _logger.LogInformation("Configured local track. Source={Source}, StartSeconds={StartSeconds:F2}, DurationSeconds={DurationSeconds:F2}", sourcePath, PositionSeconds, DurationSeconds);
    }

    private async Task FeedPipelineChunksAsync(LocalAudioChunkStream stream, CancellationToken cancellationToken)
    {
        if (stream.Chunks.Count == 0)
        {
            _logger.LogWarning("Local chunk source produced no chunks for {Source}", stream.SourcePath);
            return;
        }

        var sampleRate = Math.Max(1, stream.Format.SampleRate);
        var channels = Math.Max(1, stream.Format.Channels);
        var bitDepth = stream.Format.BitDepth.GetValueOrDefault(16);
        var bytesPerSample = Math.Max(1, bitDepth / 8);
        var bytesPerFrame = Math.Max(1, bytesPerSample * channels);

        var leadTimeUs = 400_000L;
        var maxSendAheadUs = 2_000_000L;
        var baseTimestampUs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() * 1000L + leadTimeUs;

        long framesSent = 0;

        foreach (var chunk in stream.Chunks)
        {
            cancellationToken.ThrowIfCancellationRequested();

            var chunkTimestampUs = baseTimestampUs + (framesSent * 1_000_000L / sampleRate);
            while (chunkTimestampUs > (DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() * 1000L) + maxSendAheadUs)
            {
                await Task.Delay(10, cancellationToken);
            }

            _audioPipeline.ProcessAudioChunk(new AudioChunk
            {
                Slot = 0,
                ServerTimestamp = chunkTimestampUs,
                EncodedData = chunk
            });

            framesSent += chunk.Length / bytesPerFrame;
        }

        _logger.LogInformation(
            "Finished feeding local chunks to audio pipeline. Source={Source}, Chunks={Chunks}, FramesSent={FramesSent}",
            stream.SourcePath,
            stream.Chunks.Count,
            framesSent);

        CurrentPlayingItemEnded?.Invoke(this, CurrentQueueItem);
    }

    private async Task StopPipelinePlaybackAsync()
    {
        if (_chunkFeedCts != null)
        {
            _chunkFeedCts.Cancel();
            if (_chunkFeedTask != null)
            {
                try
                {
                    await _chunkFeedTask;
                }
                catch (OperationCanceledException)
                {
                }
            }

            _chunkFeedCts.Dispose();
            _chunkFeedCts = null;
            _chunkFeedTask = null;
        }

        if (_pipelineStarted)
        {
            try
            {
                await _audioPipeline.StopAsync();
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Stopping local audio pipeline failed.");
            }

            _pipelineStarted = false;
        }
    }

    private static string? NormalizeLocalPath(string? source)
    {
        if (string.IsNullOrWhiteSpace(source))
        {
            return null;
        }

        var value = source.Trim();
        if (Uri.TryCreate(value, UriKind.Absolute, out var uri) && uri.IsFile)
        {
            return uri.LocalPath;
        }

        return value;
    }

    private void OnAudioRendererStateChanged(object? sender, PlayerStateType state)
    {
        var mapped = state == PlayerStateType.Uninitialized ? PlayerStateType.Idle : state;
        PlaybackState = new Models.PlayerState
        {
            State = mapped,
            ActiveSinceUtc = DateTimeOffset.UtcNow
        };
    }

    private void OnAudioRendererErrorOccurred(object? sender, Exception ex)
    {
        _logger.LogError(ex, "Local renderer reported an audio error.");
        PlaybackState = new Models.PlayerState
        {
            State = PlayerStateType.Error,
            ActiveSinceUtc = DateTimeOffset.UtcNow
        };
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

#endregion
