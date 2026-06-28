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
    Task PlayMediaLastAsync(IReadOnlyList<MediaItem> items, CancellationToken cancellationToken = default) => Task.CompletedTask;
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
    private Models.PlayerState _playbackState = new()
    {
        State = PlayerStateType.Idle,
        ActiveSinceUtc = DateTimeOffset.UtcNow
    };
    private double _positionSeconds;
    private double _durationSeconds;
    private int _volume = 50;
    private bool _isMuted;
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

        _sendspinClient.PlayerStateChanged += OnSendspinPlayerStateChanged;
        _sendspinClient.ConnectionStateChanged += OnSendspinConnectionStateChanged;
        _audioPlayerStateFeed.StateChanged += OnLocalAudioPlayerStateChanged;
        _musicAssistantEventHub.QueueEventReceived += OnMusicAssistantQueueEventReceived;

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

    #endregion

    #region Properties

    public PlaybackOutputMode OutputMode => PlaybackOutputMode.Sendspin;

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

    public bool IsExternalSource => _sendspinClient.IsExternalSource;

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

    public async Task SetExternalSourceAsync(bool enabled, CancellationToken cancellationToken = default)
    {
        if (!IsConnected)
        {
            return;
        }

        if (enabled)
        {
            if (_sendspinClient.IsExternalSource)
            {
                return;
            }

            _logger.LogInformation("Sendspin external source enter requested. PlayerId={PlayerId}", PlayerId);
            await _sendspinClient.EnterExternalSourceAsync();
            return;
        }

        if (!_sendspinClient.IsExternalSource)
        {
            return;
        }

        _logger.LogInformation("Sendspin external source exit requested. PlayerId={PlayerId}", PlayerId);
        await _sendspinClient.ExitExternalSourceAsync();
    }

    public async Task ActivateAsync(string? targetPlayerId, CancellationToken cancellationToken = default)
    {
        _logger.LogDebug("Sendspin activate requested. TargetPlayerId={TargetPlayerId}, CurrentPlayerId={CurrentPlayerId}", targetPlayerId, PlayerId);

        if (!string.IsNullOrWhiteSpace(targetPlayerId))
        {
            PlayerId = targetPlayerId;
        }

        if (string.IsNullOrWhiteSpace(PlayerId))
        {
            PlayerId = _settingsService.GetSendspinClientId();
        }

        _logger.LogDebug("Sendspin activation resolved player id. PlayerId={PlayerId}", PlayerId);

        if (!IsConnected
            && Uri.TryCreate(_settingsService.SendspinUrl, UriKind.Absolute, out var configuredServerUri))
        {
            _logger.LogInformation("Connecting Sendspin client. Url={SendspinUrl}, PlayerId={PlayerId}", configuredServerUri, PlayerId);
            await ConnectAsync(configuredServerUri, cancellationToken);
        }
        else if (!IsConnected)
        {
            _logger.LogWarning("Sendspin activate skipped connection because configured URL is invalid. Url={SendspinUrl}", _settingsService.SendspinUrl);
        }
    }

    public async Task DeactivateAsync()
    {
        _logger.LogDebug("Sendspin deactivate requested. PlayerId={PlayerId}", PlayerId);
        _activeQueueId = null;
        PositionSeconds = 0;
        DurationSeconds = 0;
        ResetProgressAnchor();
        PlaybackState = new Models.PlayerState { State = PlayerStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
        await DisconnectAsync();
    }

    public async Task TogglePlayPauseAsync(CancellationToken cancellationToken = default)
    {
        if (!IsConnected)
        {
            _logger.LogWarning("TogglePlayPause ignored: Sendspin client is not connected.");
            return;
        }

        var command = PlaybackState.State == PlayerStateType.Playing
            ? Commands.Pause
            : Commands.Play;

        PlaybackState = new Models.PlayerState { State = PlayerStateType.Buffering, ActiveSinceUtc = DateTimeOffset.UtcNow };

        _logger.LogInformation("Sendspin command dispatch. Command={Command}, PlayerId={PlayerId}, PlaybackState={PlaybackState}", command, PlayerId, PlaybackState.State);
        await _sendspinClient.SendCommandAsync(command);
    }

    public async Task NextAsync(CancellationToken cancellationToken = default)
    {
        if (!IsConnected)
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
        if (!IsConnected)
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

    public async ValueTask DisposeAsync()
    {
        _sendspinClient.PlayerStateChanged -= OnSendspinPlayerStateChanged;
        _sendspinClient.ConnectionStateChanged -= OnSendspinConnectionStateChanged;
        _audioPlayerStateFeed.StateChanged -= OnLocalAudioPlayerStateChanged;
        _musicAssistantEventHub.QueueEventReceived -= OnMusicAssistantQueueEventReceived;

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
        PlaybackState = new Models.PlayerState { State = PlayerStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
        _logger.LogInformation("Sendspin client disconnected by client request.");
    }

    #endregion

    #region Event Handlers

    // Set Volume and Mute
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
        _logger.LogInformation("Sendspin connection state changed. OldState={OldState}, NewState={NewState}, WasConnected={WasConnected}, PlayerId={PlayerId}, ActiveQueueId={ActiveQueueId}", e.OldState, e.NewState, IsConnected, PlayerId, _activeQueueId);
        IsConnected = e.NewState == ConnectionState.Connected;
        if (!IsConnected)
        {
            ConnectedServerName = null;
            _activeQueueId = null;
            ResetProgressAnchor();
            PlaybackState = new Models.PlayerState { State = PlayerStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
        }
    }

    // Set PlaybackState from locla audio renderer state changes
    private void OnLocalAudioPlayerStateChanged(object? sender, PlayerStateType state)
    {
        if (string.IsNullOrWhiteSpace(PlayerId))
        {
            return;
        }

        var mappedState = NormalizeLocalAudioPlayerState(state);

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

    // Set PositionSeconds and DurationSeconds from MusicAssistant queue events
    private void OnMusicAssistantQueueEventReceived(object? sender, MusicAssistantQueueEvent e)
    {
        if (string.IsNullOrWhiteSpace(PlayerId))
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
    private string? _playerId;
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

    #endregion

    #region Properties

    public PlaybackOutputMode OutputMode => PlaybackOutputMode.MA_Remote;

    public Models.PlayerState PlaybackState
    {
        get => _playbackState;
        private set => SetProperty(ref _playbackState, value ?? new Models.PlayerState
        {
            State = PlayerStateType.Unknown,
            ActiveSinceUtc = DateTimeOffset.UtcNow
        });
    }

    public string? PlayerId => Normalize(_playerId);

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
        _playerId = targetPlayerId;
        _activeQueueId = null;
        await RefreshQueueStateAsync(cancellationToken);
    }

    public Task DeactivateAsync()
    {
        _playerId = null;
        _activeQueueId = null;
        PositionSeconds = 0;
        DurationSeconds = 0;
        ResetProgressAnchor();
        PlaybackState = new Models.PlayerState { State = PlayerStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };
        return Task.CompletedTask;
    }

    public Task TogglePlayPauseAsync(CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(_playerId))
        {
            return Task.CompletedTask;
        }

        return _musicAssistant.PlayerPlayPauseAsync(_playerId);
    }

    public async Task NextAsync(CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(_playerId))
        {
            return;
        }

        await _musicAssistant.PlayerNextAsync(_playerId);
    }

    public async Task PreviousAsync(CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(_playerId))
        {
            return;
        }

        await _musicAssistant.PlayerPreviousAsync(_playerId);
    }

    public Task SeekAsync(double seconds, CancellationToken cancellationToken = default)
    {
        var clamped = Math.Max(0, (int)Math.Round(seconds));
        if (string.IsNullOrWhiteSpace(_playerId))
        {
            return Task.CompletedTask;
        }

        PositionSeconds = clamped;
        UpdateProgressAnchor(clamped);
        return _musicAssistant.PlayerSeekAsync(_playerId, clamped);
    }

    public Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default)
    {
        Volume = volume;
        if (string.IsNullOrWhiteSpace(_playerId))
        {
            return Task.CompletedTask;
        }

        return _musicAssistant.SetPlayerVolumeAsync(_playerId, Volume);
    }

    public Task SetMutedAsync(bool muted, CancellationToken cancellationToken = default)
    {
        IsMuted = muted;
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
        if (string.IsNullOrWhiteSpace(_playerId))
        {
            return null;
        }

        var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(_playerId);
        return queue?.QueueId;
    }

    private void OnQueueEventReceived(object? sender, MusicAssistantQueueEvent e)
    {
        if (string.IsNullOrWhiteSpace(_playerId))
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

        PlaybackState = new Models.PlayerState
        {
            State = MapState(queue.State),
            ActiveSinceUtc = DateTimeOffset.UtcNow
        };

        if (PlaybackState.State != PlayerStateType.Playing)
        {
            ResetProgressAnchor();
        }
    }

    private async Task RefreshQueueStateAsync(CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(_playerId))
        {
            return;
        }

        try
        {
            var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(_playerId);
            if (queue == null)
            {
                return;
            }

            _activeQueueId = Normalize(queue.QueueId);
            DurationSeconds = Math.Max(0, queue.CurrentItem?.Duration ?? 0);
            var clamped = Math.Max(0, queue.ElapsedTime ?? 0);
            PositionSeconds = clamped;
            UpdateProgressAnchor(clamped);
            PlaybackState = new Models.PlayerState
            {
                State = MapState(queue.State),
                ActiveSinceUtc = DateTimeOffset.UtcNow
            };

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

    private static PlayerStateType MapState(mashin.Models.PlaybackState? state)
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

    public Models.PlayerState PlaybackState
    {
        get => _playbackState;
        private set => SetProperty(ref _playbackState, value);
    }

    public string? PlayerId => null;

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
