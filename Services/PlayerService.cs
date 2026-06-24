using mashin.Models;
using Microsoft.Extensions.Logging;
using mashin.Audio;
using Sendspin.SDK.Audio;
using Sendspin.SDK.Client;
using Sendspin.SDK.Connection;
using Sendspin.SDK.Models;
using Sendspin.SDK.Protocol.Messages;
using System.ComponentModel;
using System.Runtime.CompilerServices;

namespace mashin.Services;

#region Interface

public interface IPlayerService : INotifyPropertyChanged, IAsyncDisposable
{
    PlaybackOutputMode OutputMode { get; }
    PlaybackStateCustom PlaybackState { get; }
    double PositionSeconds { get; }
    double DurationSeconds { get; }
    int Volume { get; }
    bool IsMuted { get; }

    Task ActivateAsync(string? targetPlayerId, CancellationToken cancellationToken = default);
    Task DeactivateAsync();
    Task TogglePlayPauseAsync(CancellationToken cancellationToken = default);
    Task NextAsync(CancellationToken cancellationToken = default);
    Task PreviousAsync(CancellationToken cancellationToken = default);
    Task SeekAsync(double seconds, CancellationToken cancellationToken = default);
    Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default);
    Task SetMutedAsync(bool muted, CancellationToken cancellationToken = default);
    Task SetShuffleAsync(bool enabled, CancellationToken cancellationToken = default);
    Task SetRepeatModeAsync(mashin.Models.RepeatMode repeatMode, CancellationToken cancellationToken = default);
}

#endregion

#region Sendspin Player

public sealed class SendspinPlayerService : IPlayerService
{
    #region Fields

    private const int PositionTimerIntervalMs = 250;

    private readonly MusicAssistantService _musicAssistant;
    private readonly IMusicAssistantEventHub _musicAssistantEventHub;
    private readonly ILogger<SendspinPlayerService> _logger;
    private readonly SettingsService _settingsService;
    private readonly ISendspinClient _sendspinClient;
    private readonly IAudioPlayerStateFeed _audioPlayerStateFeed;
    private readonly CancellationTokenSource _disposeCts = new();
    private readonly object _progressSync = new();

    private bool _isConnected;
    private string? _connectedServerName;
    private string? _playerId;
    private PlaybackStateCustom _playbackState = new()
    {
        State = PlaybackStateType.Idle,
        ActiveSinceUtc = DateTimeOffset.UtcNow
    };
    private double _positionSeconds;
    private double _durationSeconds;
    private int _volume = 50;
    private bool _isMuted;
    private string? _activePlayerId;
    private string? _activeQueueId;
    private double _lastServerPosition;
    private DateTimeOffset? _lastServerPositionUpdateUtc;
    private readonly Task _progressInterpolationTask;
    

    #endregion

    #region Construction

    public SendspinPlayerService(
        MusicAssistantService musicAssistant,
        IMusicAssistantEventHub musicAssistantEventHub,
        ILogger<SendspinPlayerService> logger,
        SettingsService settingsService,
        ISendspinClient sendspinClient,
        IAudioPlayerStateFeed audioPlayerStateFeed)
    {
        _musicAssistant = musicAssistant;
        _musicAssistantEventHub = musicAssistantEventHub;
        _logger = logger;
        _settingsService = settingsService;
        _sendspinClient = sendspinClient;
        _audioPlayerStateFeed = audioPlayerStateFeed;
        _playerId = _settingsService.GetSendspinClientId();
        _activePlayerId = _playerId;

        _sendspinClient.PlayerStateChanged += OnSendspinPlayerStateChanged;
        _sendspinClient.ConnectionStateChanged += OnSendspinConnectionStateChanged;
        _audioPlayerStateFeed.StateChanged += OnLocalAudioPlayerStateChanged;
        _musicAssistantEventHub.QueueEventReceived += OnMusicAssistantQueueEventReceived;
        _musicAssistantEventHub.PlayerEventReceived += OnMusicAssistantPlayerEventReceived;

        _logger.LogInformation("Music Assistant position interpolation task starting for Sendspin player.");

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

                if (PlaybackState.State != PlaybackStateType.Playing)
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

    #endregion

    #region Properties

    public PlaybackOutputMode OutputMode => PlaybackOutputMode.LocalSendspin;

    public bool IsConnected
    {
        get => _isConnected;
        private set => SetProperty(ref _isConnected, value);
    }

    public string? ConnectedServerName
    {
        get => _connectedServerName;
        private set => SetProperty(ref _connectedServerName, value);
    }

    public string? PlayerId
    {
        get => _playerId;
        private set => SetProperty(ref _playerId, value);
    }

    public PlaybackStateCustom PlaybackState
    {
        get => _playbackState;
        private set => SetProperty(ref _playbackState, value ?? new PlaybackStateCustom
        {
            State = PlaybackStateType.Unknown,
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

    #region Commands

    public Task SendCommandAsync(string command, Dictionary<string, object>? parameters = null)
    {
        return _sendspinClient.SendCommandAsync(command, parameters);
    }

    public Task UpdatePreferredAudioCodecAsync(string codec, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(codec))
        {
            return Task.CompletedTask;
        }

        _settingsService.SetSendspinPreferredAudioCodec(codec);
        return Task.CompletedTask;
    }

    public async Task ActivateAsync(string? targetPlayerId, CancellationToken cancellationToken = default)
    {
        _logger.LogDebug("Sendspin activate requested. TargetPlayerId={TargetPlayerId}, CurrentPlayerId={CurrentPlayerId}", targetPlayerId, PlayerId);

        if (!string.IsNullOrWhiteSpace(targetPlayerId))
        {
            PlayerId = targetPlayerId;
        }

        _activePlayerId = PlayerId;

        _logger.LogDebug("Sendspin activation resolved player id. ActivePlayerId={ActivePlayerId}", _activePlayerId);

        if (!IsConnected
            && Uri.TryCreate(_settingsService.SendspinUrl, UriKind.Absolute, out var configuredServerUri))
        {
            _logger.LogInformation("Connecting Sendspin client. Url={SendspinUrl}, ActivePlayerId={ActivePlayerId}", configuredServerUri, _activePlayerId);
            await ConnectAsync(configuredServerUri, cancellationToken);
        }
        else if (!IsConnected)
        {
            _logger.LogWarning("Sendspin activate skipped connection because configured URL is invalid. Url={SendspinUrl}", _settingsService.SendspinUrl);
        }
    }

    public async Task DeactivateAsync()
    {
        _logger.LogDebug("Sendspin deactivate requested. ActivePlayerId={ActivePlayerId}", _activePlayerId);
        _activePlayerId = null;
        _activeQueueId = null;
        PositionSeconds = 0;
        DurationSeconds = 0;
        ResetProgressAnchor();
        PlaybackState = new PlaybackStateCustom { State = PlaybackStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
        await DisconnectAsync();
    }

    public async Task TogglePlayPauseAsync(CancellationToken cancellationToken = default)
    {
        if (!IsConnected)
        {
            _logger.LogWarning("TogglePlayPause ignored: Sendspin client is not connected.");
            return;
        }

        var command = PlaybackState.State == PlaybackStateType.Playing
            ? Commands.Pause
            : Commands.Play;

        PlaybackState = new PlaybackStateCustom { State = PlaybackStateType.Buffering, ActiveSinceUtc = DateTimeOffset.UtcNow };

        _logger.LogInformation("Sendspin command dispatch. Command={Command}, ActivePlayerId={ActivePlayerId}, PlaybackState={PlaybackState}", command, _activePlayerId, PlaybackState.State);
        await _sendspinClient.SendCommandAsync(command);
    }

    public async Task NextAsync(CancellationToken cancellationToken = default)
    {
        if (!IsConnected)
        {
            _logger.LogWarning("Next ignored: Sendspin client is not connected.");
            return;
        }

        PlaybackState = new PlaybackStateCustom { State = PlaybackStateType.Buffering, ActiveSinceUtc = DateTimeOffset.UtcNow };

        _logger.LogInformation("Sendspin command dispatch. Command={Command}, ActivePlayerId={ActivePlayerId}", Commands.Next, _activePlayerId);
        await _sendspinClient.SendCommandAsync(Commands.Next);
    }

    public async Task PreviousAsync(CancellationToken cancellationToken = default)
    {
        if (!IsConnected)
        {
            _logger.LogWarning("Previous ignored: Sendspin client is not connected.");
            return;
        }

        PlaybackState = new PlaybackStateCustom { State = PlaybackStateType.Buffering, ActiveSinceUtc = DateTimeOffset.UtcNow };

        _logger.LogInformation("Sendspin command dispatch. Command={Command}, ActivePlayerId={ActivePlayerId}", Commands.Previous, _activePlayerId);
        await _sendspinClient.SendCommandAsync(Commands.Previous);
    }

    public Task SeekAsync(double seconds, CancellationToken cancellationToken = default)
    {
        var clamped = Math.Max(0, (int)Math.Round(seconds));
        var targetPlayerId = Normalize(_activePlayerId) ?? Normalize(PlayerId);
        if (string.IsNullOrWhiteSpace(targetPlayerId))
        {
            _logger.LogWarning("Seek ignored: no active player id for Sendspin player {PlayerId}", PlayerId);
            return Task.CompletedTask;
        }

        PositionSeconds = clamped;
        PlaybackState = new PlaybackStateCustom { State = PlaybackStateType.Seeking, ActiveSinceUtc = DateTimeOffset.UtcNow };

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
        _logger.LogInformation("Sendspin command dispatch. Command={Command}, ActivePlayerId={ActivePlayerId}", enabled ? Commands.Shuffle : Commands.Unshuffle, _activePlayerId);
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

        _logger.LogInformation("Sendspin command dispatch. Command={Command}, ActivePlayerId={ActivePlayerId}, RepeatMode={RepeatMode}", command, _activePlayerId, repeatMode);
        await _sendspinClient.SendCommandAsync(command);
    }

    public async ValueTask DisposeAsync()
    {
        _sendspinClient.PlayerStateChanged -= OnSendspinPlayerStateChanged;
        _sendspinClient.ConnectionStateChanged -= OnSendspinConnectionStateChanged;
        _audioPlayerStateFeed.StateChanged -= OnLocalAudioPlayerStateChanged;
        _musicAssistantEventHub.QueueEventReceived -= OnMusicAssistantQueueEventReceived;
        _musicAssistantEventHub.PlayerEventReceived -= OnMusicAssistantPlayerEventReceived;

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

    #region Connection

    private async Task ConnectAsync(Uri serverUri, CancellationToken cancellationToken)
    {
        await _sendspinClient.ConnectAsync(serverUri, cancellationToken);
        ConnectedServerName = _sendspinClient.ServerName ?? serverUri.Host;
        IsConnected = _sendspinClient.ConnectionState == ConnectionState.Connected;
        PlayerId ??= _settingsService.GetSendspinClientId();

        var currentPlayerState = _sendspinClient.CurrentPlayerState;
        Volume = currentPlayerState.Volume;
        IsMuted = currentPlayerState.Muted;

        await RefreshQueueStateAsync(cancellationToken);

        _logger.LogInformation("Sendspin client connected. Server={Server}, IsConnected={IsConnected}, PlayerId={PlayerId}", ConnectedServerName, IsConnected, PlayerId);
    }

    private async Task DisconnectAsync()
    {
        await _sendspinClient.DisconnectAsync("client_disconnect");
        IsConnected = false;
        ConnectedServerName = null;
        _activeQueueId = null;
        ResetProgressAnchor();
        PlaybackState = new PlaybackStateCustom { State = PlaybackStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
        _logger.LogInformation("Sendspin client disconnected by client request.");
    }

    #endregion

    #region Event Handlers

    // Use PlayerStateChanged event to update volume and mute state
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
        _logger.LogInformation("Sendspin connection state changed. OldState={OldState}, NewState={NewState}, WasConnected={WasConnected}, ActivePlayerId={ActivePlayerId}, ActiveQueueId={ActiveQueueId}", e.OldState, e.NewState, IsConnected, _activePlayerId, _activeQueueId);
        IsConnected = e.NewState == ConnectionState.Connected;
        if (!IsConnected)
        {
            ConnectedServerName = null;
            _activeQueueId = null;
            ResetProgressAnchor();
            PlaybackState = new PlaybackStateCustom { State = PlaybackStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
        }
    }

    // Set PlaybackState from local audio player state changes
    private void OnLocalAudioPlayerStateChanged(object? sender, AudioPlayerState state)
    {
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return;
        }

        var mappedState = MapLocalAudioPlayerState(state);

        PlaybackState = new PlaybackStateCustom
        {
            State = mappedState,
            ActiveSinceUtc = DateTimeOffset.UtcNow
        };

        _logger.LogDebug("Local audio player state applied. SourceState={SourceState}, MappedState={MappedState}, ActivePlayerId={ActivePlayerId}", state, mappedState, _activePlayerId);

        if (mappedState != PlaybackStateType.Playing)
        {
            ResetProgressAnchor();
        }
    }

    // Use MusicAssistant Player Events to update playback state and position when the active player is updated
    private void OnMusicAssistantPlayerEventReceived(object? sender, MusicAssistantPlayerEvent e)
    {
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            _logger.LogDebug("MusicAssistant PlayerEvent ignored because no active Sendspin player is selected.");
            return;
        }

        var eventPlayerId = Normalize(e.Player?.PlayerId) ?? Normalize(e.PlayerId);
        if (string.IsNullOrWhiteSpace(eventPlayerId)
            || !string.Equals(eventPlayerId, Normalize(_activePlayerId), StringComparison.Ordinal))
        {
            _logger.LogDebug("MusicAssistant PlayerEvent ignored due to player mismatch. EventPlayerId={EventPlayerId}, ActivePlayerId={ActivePlayerId}", eventPlayerId, _activePlayerId);
            return;
        }

        // Keep active queue id in sync with MA player updates.
        var nextQueueId = Normalize(e.Player?.ActiveSource);
        if (!string.IsNullOrWhiteSpace(nextQueueId))
        {
            _activeQueueId = nextQueueId;
        }

        // Keep playback state in sync when queue state is not updated yet.
        // var mappedState = MapMusicAssistantPlaybackStateFromPlayer(e.Player?.State);
        // PlaybackState = new PlaybackStateCustom
        // {
        //     State = mappedState,
        //     ActiveSinceUtc = DateTimeOffset.UtcNow
        // };
        // if (mappedState != PlaybackStateType.Playing)
        // {
        //     ResetProgressAnchor();
        // }

        _logger.LogDebug("MusicAssistant PlayerEvent applied (state ignored). EventPlayerId={EventPlayerId}, PayloadPlayerId={PayloadPlayerId}, SourceState={SourceState}, ActiveQueueId={ActiveQueueId}", e.PlayerId, e.Player?.PlayerId, e.Player?.State, _activeQueueId);
    }

    // Use MusicAssistant Queue Events to update playback state and position when the active queue is updated
    private void OnMusicAssistantQueueEventReceived(object? sender, MusicAssistantQueueEvent e)
    {
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            _logger.LogDebug("MusicAssistant QueueEvent ignored because no active Sendspin player is selected.");
            return;
        }

        var eventQueueId = Normalize(e.Queue?.QueueId) ?? Normalize(e.QueueItems?.QueueId) ?? Normalize(e.QueueId);
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

        // Set PositionSeconds from queue_time_updated.
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

        // Set DurationSeconds from queue payload or queue items payload.
        if (e.Queue?.CurrentItem?.Duration is int queueItemDuration)
        {
            DurationSeconds = Math.Max(0, queueItemDuration);
        }
        else if (e.QueueItems?.CurrentItem?.Duration is int queueItemsDuration)
        {
            DurationSeconds = Math.Max(0, queueItemsDuration);
        }

        // Set PositionSeconds from queue elapsed time when present.
        if (e.Queue?.ElapsedTime.HasValue == true)
        {
            var clamped = Math.Max(0, e.Queue.ElapsedTime.Value);
            PositionSeconds = clamped;
            UpdateProgressAnchor(clamped);
        }

        // PlaybackState is intentionally not taken from MusicAssistant queue events.
        if (e.Queue?.State is mashin.Models.PlaybackState queueState)
        {
            // var mappedState = MapMusicAssistantPlaybackStateFromQueue(queueState);
            // PlaybackState = new PlaybackStateCustom
            // {
            //     State = mappedState,
            //     ActiveSinceUtc = DateTimeOffset.UtcNow
            // };
            // if (PlaybackState.State != PlaybackStateType.Playing)
            // {
            //     ResetProgressAnchor();
            // }

            _logger.LogDebug("MusicAssistant QueueEvent applied (state ignored). Event={EventName}, QueueId={QueueId}, DurationSeconds={DurationSeconds}, PositionSeconds={PositionSeconds}, QueueState={QueueState}, ActiveQueueId={ActiveQueueId}", e.Event, e.QueueId, DurationSeconds, PositionSeconds, queueState, _activeQueueId);
        }
        else
        {
            _logger.LogDebug("MusicAssistant QueueEvent processed without queue state. Event={EventName}, QueueId={QueueId}, DurationSeconds={DurationSeconds}, PositionSeconds={PositionSeconds}, ActiveQueueId={ActiveQueueId}", e.Event, e.QueueId, DurationSeconds, PositionSeconds, _activeQueueId);
        }
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

    private async Task RefreshQueueStateAsync(CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return;
        }

        try
        {
            var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(_activePlayerId);
            if (queue == null)
            {
                return;
            }

            _activeQueueId = Normalize(queue.QueueId);
            DurationSeconds = Math.Max(0, queue.CurrentItem?.Duration ?? 0);

            if (queue.ElapsedTime.HasValue)
            {
                var clamped = Math.Max(0, queue.ElapsedTime.Value);
                PositionSeconds = clamped;
                UpdateProgressAnchor(clamped);
            }

            PlaybackState = new PlaybackStateCustom
            {
                State = MapMusicAssistantPlaybackStateFromQueue(queue.State),
                ActiveSinceUtc = DateTimeOffset.UtcNow
            };

            if (PlaybackState.State != PlaybackStateType.Playing)
            {
                ResetProgressAnchor();
            }
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

    private static PlaybackStateType MapMusicAssistantPlaybackStateFromQueue(mashin.Models.PlaybackState? state)
    {
        return state switch
        {
            mashin.Models.PlaybackState.Playing => PlaybackStateType.Playing,
            mashin.Models.PlaybackState.Paused => PlaybackStateType.Paused,
            mashin.Models.PlaybackState.Buffering => PlaybackStateType.Buffering,
            mashin.Models.PlaybackState.Idle => PlaybackStateType.Idle,
            _ => PlaybackStateType.Unknown
        };
    }

    private static PlaybackStateType MapMusicAssistantPlaybackStateFromPlayer(string? state)
    {
        return state?.Trim().ToLowerInvariant() switch
        {
            "playing" => PlaybackStateType.Playing,
            "paused" => PlaybackStateType.Paused,
            "buffering" => PlaybackStateType.Buffering,
            "idle" => PlaybackStateType.Idle,
            _ => PlaybackStateType.Unknown
        };
    }

    private static PlaybackStateType MapLocalAudioPlayerState(AudioPlayerState state)
    {
        return state switch
        {
            AudioPlayerState.Playing => PlaybackStateType.Playing,
            AudioPlayerState.Paused => PlaybackStateType.Paused,
            AudioPlayerState.Stopped => PlaybackStateType.Idle,
            AudioPlayerState.Uninitialized => PlaybackStateType.Idle,
            _ => PlaybackStateType.Unknown
        };
    }

    #endregion
}

#endregion

#region Local Dummy Player

public sealed class LocalDummyPlayerService : IPlayerService
{
    #region Fields

    private PlaybackStateCustom _playbackState = new()
    {
        State = PlaybackStateType.Idle,
        ActiveSinceUtc = DateTimeOffset.UtcNow
    };
    private double _positionSeconds;
    private double _durationSeconds;
    private int _volume = 50;
    private bool _isMuted;

    #endregion

    #region Events

    public event PropertyChangedEventHandler? PropertyChanged;

    #endregion

    #region Properties

    public PlaybackOutputMode OutputMode => PlaybackOutputMode.LocalOffline;

    public PlaybackStateCustom PlaybackState
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

    #endregion

    #region Commands

    public Task ActivateAsync(string? targetPlayerId, CancellationToken cancellationToken = default) => Task.CompletedTask;
    public Task DeactivateAsync() => Task.CompletedTask;

    public Task TogglePlayPauseAsync(CancellationToken cancellationToken = default)
    {
        var next = PlaybackState.State == PlaybackStateType.Playing
            ? PlaybackStateType.Paused
            : PlaybackStateType.Playing;
        PlaybackState = new PlaybackStateCustom { State = next, ActiveSinceUtc = DateTimeOffset.UtcNow };
        return Task.CompletedTask;
    }

    public Task NextAsync(CancellationToken cancellationToken = default)
    {
        return Task.CompletedTask;
    }

    public Task PreviousAsync(CancellationToken cancellationToken = default)
    {
        return Task.CompletedTask;
    }

    public Task SeekAsync(double seconds, CancellationToken cancellationToken = default)
    {
        PositionSeconds = seconds;
        return Task.CompletedTask;
    }

    public Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default)
    {
        Volume = volume;
        return Task.CompletedTask;
    }

    public Task SetMutedAsync(bool muted, CancellationToken cancellationToken = default)
    {
        IsMuted = muted;
        return Task.CompletedTask;
    }

    public Task SetShuffleAsync(bool enabled, CancellationToken cancellationToken = default) => Task.CompletedTask;
    public Task SetRepeatModeAsync(mashin.Models.RepeatMode repeatMode, CancellationToken cancellationToken = default) => Task.CompletedTask;
    public ValueTask DisposeAsync() => ValueTask.CompletedTask;

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

    #endregion

}

#endregion

#region Remote Player

public sealed class RemotePlayerService : IPlayerService
{
    #region Fields

    private const int PositionTimerIntervalMs = 250;

    private readonly MusicAssistantService _musicAssistant;
    private readonly IMusicAssistantEventHub _musicAssistantEventHub;
    private readonly CancellationTokenSource _disposeCts = new();
    private readonly object _progressSync = new();

    private PlaybackStateCustom _playbackState = new()
    {
        State = PlaybackStateType.Idle,
        ActiveSinceUtc = DateTimeOffset.UtcNow
    };
    private double _positionSeconds;
    private double _durationSeconds;
    private int _volume = 50;
    private bool _isMuted;
    private string? _activePlayerId;
    private string? _activeQueueId;
    private double _lastServerPosition;
    private DateTimeOffset? _lastServerPositionUpdateUtc;
    private readonly Task _progressInterpolationTask;

    #endregion

    #region Construction

    public RemotePlayerService(
        MusicAssistantService musicAssistant,
        IMusicAssistantEventHub musicAssistantEventHub)
    {
        _musicAssistant = musicAssistant;
        _musicAssistantEventHub = musicAssistantEventHub;
        _musicAssistantEventHub.QueueEventReceived += OnQueueEventReceived;

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

                if (PlaybackState.State != PlaybackStateType.Playing)
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

    #endregion

    #region Properties

    public PlaybackOutputMode OutputMode => PlaybackOutputMode.RemoteOnly;

    public PlaybackStateCustom PlaybackState
    {
        get => _playbackState;
        private set => SetProperty(ref _playbackState, value ?? new PlaybackStateCustom
        {
            State = PlaybackStateType.Unknown,
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

    #region Commands

    public async Task ActivateAsync(string? targetPlayerId, CancellationToken cancellationToken = default)
    {
        _activePlayerId = targetPlayerId;
        _activeQueueId = null;
        await RefreshQueueStateAsync(cancellationToken);
    }

    public Task DeactivateAsync()
    {
        _activePlayerId = null;
        _activeQueueId = null;
        PositionSeconds = 0;
        DurationSeconds = 0;
        ResetProgressAnchor();
        PlaybackState = new PlaybackStateCustom { State = PlaybackStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
        return Task.CompletedTask;
    }

    public Task TogglePlayPauseAsync(CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return Task.CompletedTask;
        }

        return _musicAssistant.PlayerPlayPauseAsync(_activePlayerId);
    }

    public async Task NextAsync(CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return;
        }

        await _musicAssistant.PlayerNextAsync(_activePlayerId);
    }

    public async Task PreviousAsync(CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return;
        }

        await _musicAssistant.PlayerPreviousAsync(_activePlayerId);
    }

    public Task SeekAsync(double seconds, CancellationToken cancellationToken = default)
    {
        var clamped = Math.Max(0, (int)Math.Round(seconds));
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return Task.CompletedTask;
        }

        PositionSeconds = clamped;
        UpdateProgressAnchor(clamped);
        return _musicAssistant.PlayerSeekAsync(_activePlayerId, clamped);
    }

    public Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default)
    {
        Volume = volume;
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return Task.CompletedTask;
        }

        return _musicAssistant.SetPlayerVolumeAsync(_activePlayerId, Volume);
    }

    public Task SetMutedAsync(bool muted, CancellationToken cancellationToken = default)
    {
        IsMuted = muted;
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return Task.CompletedTask;
        }

        return _musicAssistant.SetPlayerMuteAsync(_activePlayerId, IsMuted);
    }

    public async Task SetShuffleAsync(bool enabled, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.SetShuffleAsync(queueId, enabled);
    }

    public async Task SetRepeatModeAsync(mashin.Models.RepeatMode repeatMode, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.SetRepeatAsync(queueId, repeatMode);
    }

    public async ValueTask DisposeAsync()
    {
        _musicAssistantEventHub.QueueEventReceived -= OnQueueEventReceived;

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
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return null;
        }

        var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(_activePlayerId);
        return queue?.QueueId;
    }

    private void OnQueueEventReceived(object? sender, MusicAssistantQueueEvent e)
    {
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return;
        }

        var eventQueueId = Normalize(e.Queue?.QueueId) ?? Normalize(e.QueueId);
        if (!string.IsNullOrWhiteSpace(_activeQueueId)
            && !string.IsNullOrWhiteSpace(eventQueueId)
            && !string.Equals(eventQueueId, _activeQueueId, StringComparison.Ordinal))
        {
            return;
        }

        if (string.IsNullOrWhiteSpace(_activeQueueId) && !string.IsNullOrWhiteSpace(eventQueueId))
        {
            _activeQueueId = eventQueueId;
        }

        if (string.Equals(e.Event, "queue_time_updated", StringComparison.OrdinalIgnoreCase))
        {
            if (e.ElapsedTimeSeconds is double elapsedSeconds)
            {
                var clamped = Math.Max(0, elapsedSeconds);
                PositionSeconds = clamped;
                UpdateProgressAnchor(clamped);
            }

            return;
        }

        var queue = e.Queue;
        if (queue == null)
        {
            return;
        }

        _activeQueueId = Normalize(queue.QueueId) ?? _activeQueueId;
        DurationSeconds = Math.Max(0, queue.CurrentItem?.Duration ?? 0);

        if (queue.ElapsedTime.HasValue)
        {
            var clamped = Math.Max(0, queue.ElapsedTime.Value);
            PositionSeconds = clamped;
            UpdateProgressAnchor(clamped);
        }

        PlaybackState = new PlaybackStateCustom
        {
            State = MapState(queue.State),
            ActiveSinceUtc = DateTimeOffset.UtcNow
        };

        if (PlaybackState.State != PlaybackStateType.Playing)
        {
            ResetProgressAnchor();
        }
    }

    private async Task RefreshQueueStateAsync(CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return;
        }

        try
        {
            var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(_activePlayerId);
            if (queue == null)
            {
                return;
            }

            _activeQueueId = Normalize(queue.QueueId);
            DurationSeconds = Math.Max(0, queue.CurrentItem?.Duration ?? 0);
            var clamped = Math.Max(0, queue.ElapsedTime ?? 0);
            PositionSeconds = clamped;
            UpdateProgressAnchor(clamped);
            PlaybackState = new PlaybackStateCustom
            {
                State = MapState(queue.State),
                ActiveSinceUtc = DateTimeOffset.UtcNow
            };

            if (PlaybackState.State != PlaybackStateType.Playing)
            {
                ResetProgressAnchor();
            }
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

    private static string? Normalize(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return null;
        }

        return value.Trim();
    }

    private static PlaybackStateType MapState(mashin.Models.PlaybackState? state)
    {
        return state switch
        {
            mashin.Models.PlaybackState.Playing => PlaybackStateType.Playing,
            mashin.Models.PlaybackState.Paused => PlaybackStateType.Paused,
            mashin.Models.PlaybackState.Buffering => PlaybackStateType.Buffering,
            mashin.Models.PlaybackState.Idle => PlaybackStateType.Idle,
            _ => PlaybackStateType.Unknown
        };
    }

    #endregion
}

#endregion
