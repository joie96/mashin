using mashin.Models;
using Microsoft.Extensions.Logging;
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

    private sealed class ProgressAnchor
    {
        public long? ServerTimestampUs { get; init; }
        public double ServerPositionSeconds { get; init; }
        public DateTimeOffset LocalReceivedUtc { get; init; }
    }

    private const int PositionTimerIntervalMs = 250;
    private const double SeekGuardToleranceSeconds = 3;
    private const int SeekGuardLifetimeSeconds = 10;

    private readonly MusicAssistantService _musicAssistant;
    private readonly ILogger<SendspinPlayerService> _logger;
    private readonly SettingsService _settingsService;
    private readonly ISendspinClient _sendspinClient;
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
    private ProgressAnchor? _progressAnchor;
    private double? _seekGuardTargetPositionSeconds;
    private DateTimeOffset? _seekGuardExpiresUtc;
    private readonly Task _progressInterpolationTask;

    #endregion

    #region Construction

    public SendspinPlayerService(
        MusicAssistantService musicAssistant,
        ILogger<SendspinPlayerService> logger,
        SettingsService settingsService,
        ISendspinClient sendspinClient)
    {
        _musicAssistant = musicAssistant;
        _logger = logger;
        _settingsService = settingsService;
        _sendspinClient = sendspinClient;
        _playerId = _settingsService.GetSendspinClientId();
        _activePlayerId = _playerId;

        _sendspinClient.PlayerStateChanged += OnSendspinPlayerStateChanged;
        _sendspinClient.GroupStateChanged += OnSendspinGroupStateChanged;
        _sendspinClient.ConnectionStateChanged += OnSendspinConnectionStateChanged;

        _logger.LogInformation("Sendspin position interpolation task starting.");

        _progressInterpolationTask = Task.Run(async () =>
        {
            while (!_disposeCts.IsCancellationRequested)
            {
                try
                {
                    await Task.Delay(PositionTimerIntervalMs, _disposeCts.Token);
                    if (PlaybackState.State != PlaybackStateType.Playing)                    {
                        continue;
                    }

                    ProgressAnchor? anchor;
                    lock (_progressSync)
                    {
                        anchor = _progressAnchor;
                    }

                    if (anchor is null)
                    {
                        continue;
                    }

                    var elapsedSeconds = Math.Max(0, (DateTimeOffset.UtcNow - anchor.LocalReceivedUtc).TotalSeconds);
                    var interpolated = anchor.ServerPositionSeconds + elapsedSeconds;
                    if (DurationSeconds > 0)
                    {
                        interpolated = Math.Min(interpolated, DurationSeconds);
                    }

                    _logger.LogTrace(
                        "Interpolated playback position: {InterpolatedPositionSeconds:F3}s (anchor={AnchorPositionSeconds:F3}s, elapsed={ElapsedSeconds:F3}s)",
                        interpolated,
                        anchor.ServerPositionSeconds,
                        elapsedSeconds);

                    PositionSeconds = interpolated;
                }
                catch (OperationCanceledException)
                {
                    break;
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "Sendspin interpolation tick failed.");
                }
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
        UpdateProgressAnchor(clamped);
        lock (_progressSync)
        {
            _seekGuardTargetPositionSeconds = clamped;
            _seekGuardExpiresUtc = DateTimeOffset.UtcNow.AddSeconds(SeekGuardLifetimeSeconds);
        }
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
        _sendspinClient.GroupStateChanged -= OnSendspinGroupStateChanged;
        _sendspinClient.ConnectionStateChanged -= OnSendspinConnectionStateChanged;

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

        if (_sendspinClient.CurrentGroup is { } currentGroup)
        {
            ApplySendspinGroupState(currentGroup);
        }

        _logger.LogInformation("Sendspin client connected. Server={Server}, IsConnected={IsConnected}, PlayerId={PlayerId}", ConnectedServerName, IsConnected, PlayerId);
    }

    private async Task DisconnectAsync()
    {
        await _sendspinClient.DisconnectAsync("client_disconnect");
        IsConnected = false;
        ConnectedServerName = null;
        ResetProgressAnchor();
        PlaybackState = new PlaybackStateCustom { State = PlaybackStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
        _logger.LogInformation("Sendspin client disconnected by client request.");
    }

    #endregion

    #region Event Handlers

    private void OnSendspinPlayerStateChanged(object? sender, object state)
    {
        // PlayerState is used as a source for volume and mute state, but not for playback state or position, which are derived from GroupState
        if (state is null)
        {
            return;
        }

        try
        {
            dynamic playerState = state;
            Volume = Convert.ToInt32(playerState.Volume);
            IsMuted = Convert.ToBoolean(playerState.Muted);
            _logger.LogDebug("Sendspin player state update applied. Volume={Volume}, IsMuted={IsMuted}", Volume, IsMuted);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Sendspin PlayerStateChanged payload could not be read. Type={PayloadType}", state.GetType().FullName);
        }
    }

    private void OnSendspinGroupStateChanged(object? sender, object state)
    {
        if (state is null)
        {
            return;
        }

        if (state is GroupState groupState)
        {
            ApplySendspinGroupState(groupState);
            return;
        }

        _logger.LogWarning("Sendspin GroupStateChanged payload has unexpected type {Type}", state.GetType().FullName);
    }

    private void OnSendspinConnectionStateChanged(object? sender, ConnectionStateChangedEventArgs e)
    {
        _logger.LogInformation("Sendspin connection state changed. OldState={OldState}, NewState={NewState}", e.OldState, e.NewState);
        IsConnected = e.NewState == ConnectionState.Connected;
        if (!IsConnected)
        {
            ConnectedServerName = null;
            ResetProgressAnchor();
            PlaybackState = new PlaybackStateCustom { State = PlaybackStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
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
    private void UpdateProgressAnchor(double positionSeconds, long? serverTimestampUs = null)
    {
        var clamped = Math.Max(0, positionSeconds);
        var localReceivedUtc = DateTimeOffset.UtcNow;
        lock (_progressSync)
        {
            _progressAnchor = new ProgressAnchor
            {
                ServerTimestampUs = serverTimestampUs,
                ServerPositionSeconds = clamped,
                LocalReceivedUtc = localReceivedUtc
            };
        }
    }

    private void ResetProgressAnchor()
    {
        lock (_progressSync)
        {
            _progressAnchor = null;
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

    private void ApplySendspinGroupState(GroupState groupState)
    {
        var currentState = PlaybackState.State;
        var nextState = MapSendspinPlaybackState(groupState.PlaybackState, currentState);
        
        var metadata = groupState.Metadata;
        var metadataTimestamp = metadata?.Timestamp;
        var metadataProgress = metadata?.Progress;
        var metadataProgressPositionMs = metadataProgress?.TrackProgress;
        var metadataProgressDurationMs = metadataProgress?.TrackDuration;
        var metadataProgressPositionSeconds = metadataProgressPositionMs / 1000d;
        var metadataProgressDurationSeconds = metadataProgressDurationMs / 1000d;
        var nowUtc = DateTimeOffset.UtcNow;
        
        long? progressAnchorLocalTimestampUs;
        double progressAnchorServerPositionSeconds;
        long? progressAnchorServerTimestampUs;
        
        double? seekGuardTargetPositionSeconds;
        DateTimeOffset? seekGuardExpiresUtc;
        lock (_progressSync)
        {
            progressAnchorLocalTimestampUs = _progressAnchor is null
                ? null
                : _progressAnchor.LocalReceivedUtc.ToUnixTimeMilliseconds() * 1000;
            progressAnchorServerPositionSeconds = _progressAnchor?.ServerPositionSeconds ?? 0;
            progressAnchorServerTimestampUs = _progressAnchor?.ServerTimestampUs;
            
            if (_seekGuardExpiresUtc.HasValue && _seekGuardExpiresUtc.Value <= nowUtc)
            {
                _seekGuardExpiresUtc = null;
                _seekGuardTargetPositionSeconds = null;
            }

            seekGuardTargetPositionSeconds = _seekGuardTargetPositionSeconds;
            seekGuardExpiresUtc = _seekGuardExpiresUtc;
        }
        
        _logger.LogDebug(
            "Sendspin group update received. GroupPlaybackState={GroupPlaybackState}, MappedState={MappedState}, MetadataTimestamp={MetadataTimestamp}, LocalTimestamp={LocalTimestamp}, LocalServerTimestamp={LocalServerTimestamp}, LocalPositionSeconds={LocalPositionSeconds:F3}, ProgressDurationMs={ProgressDurationMs}, ProgressPositionMs={ProgressPositionMs}, PlaybackSpeed={PlaybackSpeed}",
            groupState.PlaybackState,
            nextState,
            metadataTimestamp,
            progressAnchorLocalTimestampUs,
            progressAnchorServerTimestampUs,
            progressAnchorServerPositionSeconds,
            metadataProgressDurationMs,
            metadataProgressPositionMs,
            metadataProgress?.PlaybackSpeed);

        // Set PlaybackState
        PlaybackState = new PlaybackStateCustom
        {
            State = nextState,
            ActiveSinceUtc = DateTimeOffset.UtcNow
        };

        // Set DurationSeconds
        if (metadataProgressDurationSeconds.HasValue)
        {
            DurationSeconds = Math.Max(0, metadataProgressDurationSeconds.Value);
        }

        // Don't update position & progress anchor if metadata timestamp is older than current progress anchor, except when resuming from paused to playing
        if (metadataTimestamp.HasValue
            && progressAnchorServerTimestampUs.HasValue
            && metadataTimestamp.Value <= progressAnchorServerTimestampUs.Value
            && !(currentState == PlaybackStateType.Paused && nextState == PlaybackStateType.Playing))
        {
            _logger.LogDebug(
                "Sendspin group update skipped because metadata timestamp is not newer than current progress anchor. MetadataTimestamp={MetadataTimestamp}, ProgressAnchorServerTimestamp={ProgressAnchorServerTimestamp}",
                metadataTimestamp,
                progressAnchorServerTimestampUs);
            return;
        }

        // Don't update position & progress anchor if seek guard is active and incoming position does not match seek target.
        if (seekGuardTargetPositionSeconds.HasValue
            && seekGuardExpiresUtc.HasValue
            && seekGuardExpiresUtc.Value > nowUtc)
        {
            if (seekGuardTargetPositionSeconds is not double seekGuardTargetPosition)
            {
                return;
            }

            if (!metadataProgressPositionSeconds.HasValue)
            {
                _logger.LogDebug(
                    "Sendspin group update skipped because seek guard is active but metadata position is missing. SeekTargetPositionSeconds={SeekTargetPositionSeconds}, SeekGuardExpiresUtc={SeekGuardExpiresUtc}",
                    seekGuardTargetPosition,
                    seekGuardExpiresUtc);
                return;
            }

            var seekGuardDeltaSeconds = Math.Abs(metadataProgressPositionSeconds.Value - seekGuardTargetPosition);
            if (seekGuardDeltaSeconds > SeekGuardToleranceSeconds)
            {
                _logger.LogDebug(
                    "Sendspin group update skipped by seek guard. MetadataPositionSeconds={MetadataPositionSeconds:F3}, SeekTargetPositionSeconds={SeekTargetPositionSeconds:F3}, DeltaSeconds={DeltaSeconds:F3}, ToleranceSeconds={ToleranceSeconds:F3}, SeekGuardExpiresUtc={SeekGuardExpiresUtc}",
                    metadataProgressPositionSeconds.Value,
                    seekGuardTargetPosition,
                    seekGuardDeltaSeconds,
                    SeekGuardToleranceSeconds,
                    seekGuardExpiresUtc);
                return;
            }

            lock (_progressSync)
            {
                _seekGuardTargetPositionSeconds = null;
                _seekGuardExpiresUtc = null;
            }
        }

        // Set PositionSeconds & progress anchor
        if (metadataProgressPositionSeconds.HasValue)
        {
            var nextPosition = Math.Max(0, metadataProgressPositionSeconds.Value);
            if (DurationSeconds > 0)
            {
                nextPosition = Math.Min(nextPosition, DurationSeconds);
            }
        
            PositionSeconds = nextPosition;
            UpdateProgressAnchor(nextPosition, metadataTimestamp);
        }
        
    }

    private static PlaybackStateType MapSendspinPlaybackState(Sendspin.SDK.Models.PlaybackState state, PlaybackStateType currentState)
    {
        // Keep paused state stable when Sendspin emits a transient Idle during resume.
        if (state == Sendspin.SDK.Models.PlaybackState.Idle
            && currentState == PlaybackStateType.Paused)
        {
            return PlaybackStateType.Paused;
        }

        return state switch
        {
            Sendspin.SDK.Models.PlaybackState.Playing => PlaybackStateType.Playing,
            Sendspin.SDK.Models.PlaybackState.Paused => PlaybackStateType.Paused,
            Sendspin.SDK.Models.PlaybackState.Idle => PlaybackStateType.Idle,
            Sendspin.SDK.Models.PlaybackState.Stopped => PlaybackStateType.Paused,
            Sendspin.SDK.Models.PlaybackState.Error => PlaybackStateType.Unknown,
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
