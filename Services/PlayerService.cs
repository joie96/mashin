using Microsoft.Extensions.Logging;
using Sendspin.SDK.Audio;
using Sendspin.SDK.Client;
using Sendspin.SDK.Connection;
using Sendspin.SDK.Models;
using Sendspin.SDK.Synchronization;
using mashin.Audio;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Text.Json;
using System.Threading;

namespace mashin.Services;

public interface ISendspinPlayerService : IAsyncDisposable, INotifyPropertyChanged
{
    #region Properties
    bool IsConnected { get; }
    string? ConnectedServerName { get; }
    string? PlayerId { get; }

    PlaybackStateModel PlayerState { get; set; }
    int Volume { get; }
    double DurationSeconds { get; }
    double PositionSeconds { get; set; }
    string? TrackTitle { get; }
    string? TrackArtist { get; }
    string? TrackAlbum { get; }
    bool IsMuted { get; }
    bool? ShuffleEnabled { get; }
    string? RepeatMode { get; }
    #endregion

    #region Lifecycle
    Task ConnectAsync(Uri serverUri, CancellationToken cancellationToken = default);
    Task DisconnectAsync();
    #endregion

    #region Commands
    Task<bool> EnsureConnectedAsync(string? playerId, CancellationToken cancellationToken = default);
    Task SendCommandAsync(string command, Dictionary<string, object>? parameters = null);
    Task UpdatePreferredAudioCodecAsync(string codec, CancellationToken cancellationToken = default);
    #endregion
}
public sealed class SendspinPlayerService : ISendspinPlayerService
{
    #region Fields
    private readonly ILogger<SendspinPlayerService> _logger;
    private readonly ILoggerFactory _loggerFactory;
    private readonly IAudioPlayer _audioPlayer;
    private readonly IAudioPipeline _audioPipeline;
    private readonly IClockSynchronizer _clockSynchronizer;
    private readonly SettingsService _settingsService;

    private SendspinClientService? _client;
    private ISendspinConnection? _connection;

    private readonly SemaphoreSlim _cleanupLock = new(1, 1);
    private readonly Timer _positionTimer;
    private readonly object _stateLock = new();

    private PlaybackStateModel _playerState = new(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow);
    private int _volume;
    private double _durationSeconds;
    private double _positionSeconds;
    private string? _trackTitle;
    private string? _trackArtist;
    private string? _trackAlbum;
    private bool _isMuted;
    private bool? _shuffleEnabled;
    private string? _repeatMode;
    private long _metadataTimestampUs;
    private double _metadataTrackProgressMs;
    private double _metadataTrackDurationMs;
    private double _metadataPlaybackSpeed;
    #endregion

    #region Events
    public event PropertyChangedEventHandler? PropertyChanged;
    #endregion

    #region Properties
    public bool IsConnected => _client?.ConnectionState == ConnectionState.Connected;

    public string? ConnectedServerName => _client?.ServerName;
    public string? PlayerId { get; private set; }

    public PlaybackStateModel PlayerState
    {
        get => _playerState;
        set => SetPlayerState(value);
    }

    public int Volume
    {
        get => _volume;
        private set => SetProperty(ref _volume, value);
    }

    public double DurationSeconds
    {
        get => _durationSeconds;
        private set => SetProperty(ref _durationSeconds, value);
    }

    public double PositionSeconds
    {
        get => _positionSeconds;
        set => SetProperty(ref _positionSeconds, Math.Max(0d, value));
    }

    public string? TrackTitle
    {
        get => _trackTitle;
        private set => SetProperty(ref _trackTitle, value);
    }

    public string? TrackArtist
    {
        get => _trackArtist;
        private set => SetProperty(ref _trackArtist, value);
    }

    public string? TrackAlbum
    {
        get => _trackAlbum;
        private set => SetProperty(ref _trackAlbum, value);
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
    
    #endregion

    #region Constructor


    public SendspinPlayerService(
        ILogger<SendspinPlayerService> logger,
        ILoggerFactory loggerFactory,
        IAudioPlayer audioPlayer,
        IAudioPipeline audioPipeline,
        IClockSynchronizer clockSynchronizer,
        SettingsService settingsService)
    {
        _logger = logger;
        _loggerFactory = loggerFactory;
        _audioPlayer = audioPlayer;
        _audioPipeline = audioPipeline;
        _clockSynchronizer = clockSynchronizer;
        _settingsService = settingsService;
        _positionTimer = new Timer(_ => UpdatePositionFromTimer(), null, TimeSpan.Zero, TimeSpan.FromMilliseconds(500));
        _audioPlayer.StateChanged += OnAudioPlayerStateChanged;
    }
    #endregion

    #region Lifecycle

    public async Task ConnectAsync(Uri serverUri, CancellationToken cancellationToken = default)
    {
        if (serverUri is null)
        {
            throw new ArgumentNullException(nameof(serverUri));
        }

        await _cleanupLock.WaitAsync(cancellationToken);
        try
        {
            if (_client?.ConnectionState == ConnectionState.Connected)
            {
                _logger.LogWarning("Connect requested, but client is already connected");
                return;
            }

            await CleanupClientCoreAsync();

            _connection = new SendspinConnection(
                _loggerFactory.CreateLogger<SendspinConnection>());

            var clientCapabilities = _settingsService.GetClientCapabilities();
            PlayerId = BuildLocalPlayerId(clientCapabilities);

            _client = new SendspinClientService(
                _loggerFactory.CreateLogger<SendspinClientService>(),
                _connection,
                clockSynchronizer: _clockSynchronizer,
                capabilities: clientCapabilities,
                audioPipeline: _audioPipeline);

            _client.ConnectionStateChanged += OnConnectionStateChanged;
            _client.GroupStateChanged += OnGroupStateChanged;

            _logger.LogInformation(
                "Connecting to Sendspin server: {ServerUri}. ClientCapabilities: {ClientCapabilitiesJson}",
                serverUri,
                JsonSerializer.Serialize(clientCapabilities));
            await _client.ConnectAsync(serverUri, cancellationToken);
        }
        finally
        {
            _cleanupLock.Release();
        }
    }

    public async Task DisconnectAsync()
    {
        await _cleanupLock.WaitAsync();
        try
        {
            await CleanupClientCoreAsync();
        }
        finally
        {
            _cleanupLock.Release();
        }
    }

    public async Task<bool> EnsureConnectedAsync(string? playerId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(playerId))
        {
            return false;
        }

        if (IsConnected)
        {
            return true;
        }

        var localPlayerId = BuildLocalPlayerId();
        if (string.IsNullOrWhiteSpace(localPlayerId)
            || !string.Equals(playerId, localPlayerId, StringComparison.Ordinal))
        {
            return true;
        }

        if (!Uri.TryCreate(_settingsService.SendspinUrl, UriKind.Absolute, out var serverUri))
        {
            _logger.LogWarning(
                "Cannot reconnect Sendspin for local player {PlayerId}: invalid URL {SendspinUrl}",
                playerId,
                _settingsService.SendspinUrl);
            return false;
        }

        try
        {
            await ConnectAsync(serverUri, cancellationToken);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Reconnect for local player {PlayerId} failed", playerId);
            return false;
        }

        if (!IsConnected)
        {
            _logger.LogWarning("Reconnect for local player {PlayerId} did not establish a connection", playerId);
            return false;
        }

        return true;
    }

    #endregion

    #region Commands

    public async Task SendCommandAsync(string command, Dictionary<string, object>? parameters = null)
    {
        if (_client?.ConnectionState != ConnectionState.Connected)
        {
            _logger.LogWarning("Cannot send command {Command}: not connected", command);
            return;
        }

        await _client.SendCommandAsync(command, parameters);
    }

    public async Task UpdatePreferredAudioCodecAsync(string codec, CancellationToken cancellationToken = default)
    {
        var normalizedCodec = codec?.Trim().ToLowerInvariant();
        if (string.IsNullOrWhiteSpace(normalizedCodec))
        {
            _logger.LogWarning("Cannot update preferred audio codec: codec is empty");
            return;
        }

        var changed = _settingsService.SetPreferredAudioCodec(normalizedCodec);
        if (!changed)
        {
            _logger.LogDebug("Preferred audio codec is already {Codec}; skipping reconnect.", normalizedCodec);
            return;
        }

        if (_client?.ConnectionState != ConnectionState.Connected)
        {
            _logger.LogInformation(
                "Saved preferred audio codec {Codec} in settings. It will be applied on next connect.",
                normalizedCodec);
            return;
        }

        if (!Uri.TryCreate(_settingsService.SendspinUrl, UriKind.Absolute, out var serverUri))
        {
            _logger.LogWarning(
                "Preferred audio codec {Codec} saved, but reconnect skipped due to invalid Sendspin URL: {Url}",
                normalizedCodec,
                _settingsService.SendspinUrl);
            return;
        }

        _logger.LogInformation(
            "Preferred audio codec updated to {Codec}. Reconnecting to apply updated client capabilities.",
            normalizedCodec);

        var wasPlayingBeforeReconnect = PlayerState.State == PlayerPlaybackState.Playing;

        // Reflect codec switch in the UI immediately while reconnect is in progress.
        PlayerState = new PlaybackStateModel(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);

        await DisconnectAsync();
        await ConnectAsync(serverUri, cancellationToken);

        // If the player was playing before the reconnect, attempt to resume playback after reconnecting.
        if (wasPlayingBeforeReconnect)
        {
            _logger.LogInformation("Resuming playback after codec reconnect.");

            const int maxResumeAttempts = 5;
            var playbackResumed = PlayerState.State == PlayerPlaybackState.Playing;

            for (var attempt = 1; attempt <= maxResumeAttempts; attempt++)
            {
                cancellationToken.ThrowIfCancellationRequested();

                if (playbackResumed)
                {
                    break;
                }

                if (_client?.ConnectionState == ConnectionState.Connected)
                {
                    try
                    {
                        await _client.SendCommandAsync("play");
                        PlayerState = new PlaybackStateModel(PlayerPlaybackState.Buffering, DateTimeOffset.UtcNow);
                        _logger.LogDebug(
                            "Playback resume command issued after reconnect (attempt {Attempt}/{MaxAttempts}).",
                            attempt,
                            maxResumeAttempts);
                    }
                    catch (Exception ex)
                    {
                        _logger.LogDebug(
                            ex,
                            "Failed to send resume command after reconnect (attempt {Attempt}/{MaxAttempts}).",
                            attempt,
                            maxResumeAttempts);
                    }
                }

                await Task.Delay(TimeSpan.FromMilliseconds(500), cancellationToken);

                playbackResumed = PlayerState.State == PlayerPlaybackState.Playing;

                if (playbackResumed)
                {
                    _logger.LogDebug(
                        "Playback resume confirmed after reconnect (attempt {Attempt}/{MaxAttempts}).",
                        attempt,
                        maxResumeAttempts);
                    break;
                }
            }

            if (!playbackResumed)
            {
                _logger.LogWarning("Could not resume playback automatically after codec reconnect.");
            }
        }
    }
    #endregion

    #region ID Helpers

    private string? BuildLocalPlayerId(ClientCapabilities? capabilities = null)
    {
        var clientCapabilities = capabilities ?? _settingsService.GetClientCapabilities();
        if (string.IsNullOrWhiteSpace(clientCapabilities.ClientId))
        {
            return null;
        }

        return $"up{clientCapabilities.ClientId.Replace("-", string.Empty, StringComparison.Ordinal)}";
    }

    #endregion

    #region Event Handlers

    private void OnConnectionStateChanged(object? sender, ConnectionStateChangedEventArgs e)
    {
        if (e.NewState != ConnectionState.Connected)
        {
            SetPlayerState(new PlaybackStateModel(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow));
        }

        OnPropertyChanged(nameof(IsConnected));
    }

    private void OnGroupStateChanged(object? sender, GroupState group)
    {
        // Playing state -> use AudioPlayer state for more immediate feedback (instead of waiting for metadata update with progress info)
        //var stateName = group.PlaybackState.ToString();
        //var mappedPlayState = MapServerPlaybackState(stateName);
        //SetPlayerState(new PlaybackStateModel(mappedPlayState, DateTimeOffset.UtcNow));

        // Volume and mute state
        var clampedVolume = Math.Max(0, Math.Min(100, group.Volume));
        Volume = clampedVolume;
        IsMuted = group.Muted;

        // Shuffle and repeat state
        ShuffleEnabled = group.Shuffle;
        RepeatMode = group.Repeat?.ToString();

        // Metadata
        var md = group.Metadata;
        if (md == null)
        {
            _logger.LogDebug("Group update without metadata; keeping last known duration/track/position.");
            return;
        }

        var trackChanged = !string.Equals(TrackTitle, md.Title, StringComparison.Ordinal)
            || !string.Equals(TrackArtist, md.Artist, StringComparison.Ordinal)
            || !string.Equals(TrackAlbum, md.Album, StringComparison.Ordinal);

        if (trackChanged)
        {
            PositionSeconds = 0;
        }

        TrackTitle = md.Title;
        TrackArtist = md.Artist;
        TrackAlbum = md.Album;

        // Position tracking
        var progress = md.Progress;
        _logger.LogTrace("md.Progress: progress={Progress}, duration={Duration}, speed={Speed}, timestamp={Timestamp}",
            progress?.TrackProgress, progress?.TrackDuration, progress?.PlaybackSpeed, md.Timestamp);
        if (progress?.TrackProgress is null
            || progress.TrackDuration is null
            || progress.PlaybackSpeed is null
            || md.Timestamp is not > 0)
        {
            _logger.LogDebug("Metadata without complete progress/timestamp; skipping position baseline update.");
            return;
        }

        lock (_stateLock)
        {
            _metadataTimestampUs = md.Timestamp.Value;
            _metadataTrackProgressMs = Math.Max(0d, progress.TrackProgress.Value);
            _metadataTrackDurationMs = Math.Max(0d, progress.TrackDuration.Value);
            _metadataPlaybackSpeed = Math.Max(0d, progress.PlaybackSpeed.Value);
        }
    }

    private void OnAudioPlayerStateChanged(object? sender, AudioPlayerState state)
    {
        var mappedState = state switch
        {
            AudioPlayerState.Playing => PlayerPlaybackState.Playing,
            AudioPlayerState.Paused => PlayerPlaybackState.Paused,
            AudioPlayerState.Stopped => PlayerPlaybackState.Stopped,
            AudioPlayerState.Uninitialized => PlayerPlaybackState.Stopped,
            AudioPlayerState.Error => PlayerPlaybackState.Unknown,
            _ => PlayerPlaybackState.Unknown,
        };

        SetPlayerState(new PlaybackStateModel(mappedState, DateTimeOffset.UtcNow));
    }

    #endregion

    #region Position Tracking

    private static long GetCurrentClientTimestampUs()
        => (DateTime.UtcNow.Ticks - DateTime.UnixEpoch.Ticks) / 10;

    private void UpdatePositionFromTimer()
    {
        var nowUtc = DateTimeOffset.UtcNow;

        PlaybackStateModel playerState;
        long metadataTimestampUs;
        double trackProgressMs;
        double trackDurationMs;
        double playbackSpeed;

        lock (_stateLock)
        {
            playerState = _playerState;
            metadataTimestampUs = _metadataTimestampUs;
            trackProgressMs = _metadataTrackProgressMs;
            trackDurationMs = _metadataTrackDurationMs;
            playbackSpeed = _metadataPlaybackSpeed;
        }

        if (playerState.State != PlayerPlaybackState.Playing && playerState.State != PlayerPlaybackState.Seeking)
        {
            return;
        }

        var durationSeconds = Math.Max(0d, trackDurationMs / 1000d);
        if (Math.Abs(durationSeconds - _durationSeconds) >= 0.001)
        {
            DurationSeconds = durationSeconds;
        }

        var stateAge = nowUtc - playerState.TimestampUtc;
        if (stateAge < TimeSpan.FromSeconds(5))
        {
            var localPosition = _positionSeconds + 0.5d;
            if (durationSeconds > 0)
            {
                localPosition = Math.Min(localPosition, durationSeconds);
            }

            if (Math.Abs(localPosition - _positionSeconds) >= 0.01)
            {
                PositionSeconds = Math.Max(0d, localPosition);
            }

            return;
        }

        if (metadataTimestampUs <= 0)
        {
            return;
        }

        var currentServerTimeUs = metadataTimestampUs;
        if (_clockSynchronizer.HasMinimalSync)
        {
            var currentClientTimeUs = GetCurrentClientTimestampUs();
            currentServerTimeUs = _clockSynchronizer.ClientToServerTime(currentClientTimeUs);
        }

        var deltaUs = Math.Max(0L, currentServerTimeUs - metadataTimestampUs);
        var calculatedProgressMs = trackProgressMs + (deltaUs * playbackSpeed / 1_000_000d);

        double currentTrackProgressMs;
        if (trackDurationMs != 0)
        {
            currentTrackProgressMs = Math.Max(0d, Math.Min(calculatedProgressMs, trackDurationMs));
        }
        else
        {
            currentTrackProgressMs = Math.Max(0d, calculatedProgressMs);
        }

        var nextPositionSeconds = currentTrackProgressMs / 1000d;

        if (Math.Abs(nextPositionSeconds - _positionSeconds) >= 0.1)
        {
            _logger.LogTrace(
                "Calculated position: {CalculatedPosition:F3}s (trackProgressMs={TrackProgressMs:F0}, speed={Speed:F0}, durationMs={DurationMs:F0})",
                nextPositionSeconds,
                trackProgressMs,
                playbackSpeed,
                trackDurationMs);

            PositionSeconds = nextPositionSeconds;
        }
    }
    #endregion

    #region State Helpers

    private static PlayerPlaybackState MapServerPlaybackState(string? serverState)
    {
        if (serverState is null)
        {
            return PlayerPlaybackState.Unknown;
        }

        return serverState.ToLowerInvariant() switch
        {
            "playing" => PlayerPlaybackState.Playing,
            "buffering" => PlayerPlaybackState.Buffering,
            "loading" => PlayerPlaybackState.Buffering,
            "paused" => PlayerPlaybackState.Paused,
            "seeking" => PlayerPlaybackState.Seeking,
            "stopped" => PlayerPlaybackState.Stopped,
            "idle" => PlayerPlaybackState.Stopped,
            _ => PlayerPlaybackState.Unknown,
        };
    }

    private void SetPlayerState(PlaybackStateModel playerState)
    {
        var normalizedTimestamp = playerState.TimestampUtc == default
            ? DateTimeOffset.UtcNow
            : playerState.TimestampUtc;
        var normalizedPlayerState = playerState with { TimestampUtc = normalizedTimestamp };

        var stateChanged = SetProperty(ref _playerState, normalizedPlayerState, nameof(PlayerState));

        if (!stateChanged)
        {
            return;
        }
    }

    #endregion

    #region Cleanup

    private async Task CleanupClientCoreAsync()
    {
        if (_client == null)
        {
            _connection = null;
            PlayerId = null;
            return;
        }

        _client.ConnectionStateChanged -= OnConnectionStateChanged;
        _client.GroupStateChanged -= OnGroupStateChanged;

        try
        {
            await _client.DisconnectAsync();
            await _client.DisposeAsync();
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Error while disconnecting/disposing Sendspin client");
        }

        _client = null;
        _connection = null;
        PlayerId = null;
    }
    #endregion

    #region IDisposable

    public async ValueTask DisposeAsync()
    {
        _audioPlayer.StateChanged -= OnAudioPlayerStateChanged;
        await DisconnectAsync();
        _positionTimer.Dispose();
        _cleanupLock.Dispose();
    }
    #endregion

    #region Property Helpers

    private void OnPropertyChanged([CallerMemberName] string? propertyName = null)
        => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));

    private bool SetProperty<T>(ref T field, T value, [CallerMemberName] string? propertyName = null)
    {
        if (EqualityComparer<T>.Default.Equals(field, value))
        {
            return false;
        }

        field = value;
        OnPropertyChanged(propertyName);
        return true;
    }
    #endregion
}