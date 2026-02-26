using Microsoft.Extensions.Logging;
using Sendspin.SDK.Audio;
using Sendspin.SDK.Client;
using Sendspin.SDK.Connection;
using Sendspin.SDK.Models;
using Sendspin.SDK.Synchronization;
using mashin.Audio;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Threading;

namespace mashin.Services;

public enum PlayerPlaybackState
{
    Unknown,
    Stopped,
    Paused,
    Buffering,
    Playing,
    Seeking,
}

public sealed record PlayerPlayState(PlayerPlaybackState State, DateTimeOffset TimestampUtc);

public interface IPlayerService : IAsyncDisposable, INotifyPropertyChanged
{
    #region Properties
    bool IsConnected { get; }
    string? ConnectedServerName { get; }
    string? ClientId { get; }

    PlayerPlayState PlayState { get; set; }
    int Volume { get; }
    double DurationSeconds { get; }
    double PositionSeconds { get; }
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
    Task SendCommandAsync(string command, Dictionary<string, object>? parameters = null);
    #endregion
}
public sealed class PlayerService : IPlayerService
{
    #region Fields
    private readonly ILogger<PlayerService> _logger;
    private readonly ILoggerFactory _loggerFactory;
    private readonly IAudioPipeline _audioPipeline;
    private readonly IClockSynchronizer _clockSynchronizer;
    private readonly SettingsService _settingsService;

    private SendspinClientService? _client;
    private ISendspinConnection? _connection;

    private readonly SemaphoreSlim _cleanupLock = new(1, 1);
    private readonly Timer _positionTimer;
    private readonly object _stateLock = new();

    private PlayerPlayState _playState = new(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow);
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
    public string? ClientId { get; private set; }

    public PlayerPlayState PlayState
    {
        get => _playState;
        set => SetPlayState(value);
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
        private set => SetProperty(ref _positionSeconds, value);
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


    public PlayerService(
        ILogger<PlayerService> logger,
        ILoggerFactory loggerFactory,
        IAudioPipeline audioPipeline,
        IClockSynchronizer clockSynchronizer,
        SettingsService settingsService)
    {
        _logger = logger;
        _loggerFactory = loggerFactory;
        _audioPipeline = audioPipeline;
        _clockSynchronizer = clockSynchronizer;
        _settingsService = settingsService;
        _positionTimer = new Timer(_ => UpdatePositionFromTimer(), null, TimeSpan.Zero, TimeSpan.FromMilliseconds(500));
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
            ClientId = clientCapabilities.ClientId;

            _client = new SendspinClientService(
                _loggerFactory.CreateLogger<SendspinClientService>(),
                _connection,
                clockSynchronizer: _clockSynchronizer,
                capabilities: clientCapabilities,
                audioPipeline: _audioPipeline);

            _client.ConnectionStateChanged += OnConnectionStateChanged;
            _client.GroupStateChanged += OnGroupStateChanged;

            _logger.LogInformation("Connecting to Sendspin server: {ServerUri} (BufferCapacity: {BufferCapacity})",
                serverUri, clientCapabilities.BufferCapacity);
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
    #endregion

    #region Event Handlers

    private void OnConnectionStateChanged(object? sender, ConnectionStateChangedEventArgs e)
    {
        if (e.NewState != ConnectionState.Connected)
        {
            SetPlayState(new PlayerPlayState(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow));
        }

        OnPropertyChanged(nameof(IsConnected));
    }

    private void OnGroupStateChanged(object? sender, GroupState group)
    {
        // Playing state
        var stateName = group.PlaybackState.ToString();
        var mappedPlayState = MapServerPlaybackState(stateName);
        SetPlayState(new PlayerPlayState(mappedPlayState, DateTimeOffset.UtcNow));

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
        TrackTitle = md.Title;
        TrackArtist = md.Artist;
        TrackAlbum = md.Album;

        // Position tracking
        var progress = md.Progress;
        _logger.LogDebug("md.Progress: progress={Progress}, duration={Duration}, speed={Speed}, timestamp={Timestamp}",
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
    #endregion

    #region Position Tracking

    private static long GetCurrentClientTimestampUs()
        => (DateTime.UtcNow.Ticks - DateTime.UnixEpoch.Ticks) / 10;

    private void UpdatePositionFromTimer()
    {
        var nowUtc = DateTimeOffset.UtcNow;

        PlayerPlayState playState;
        long metadataTimestampUs;
        double trackProgressMs;
        double trackDurationMs;
        double playbackSpeed;

        lock (_stateLock)
        {
            playState = _playState;
            metadataTimestampUs = _metadataTimestampUs;
            trackProgressMs = _metadataTrackProgressMs;
            trackDurationMs = _metadataTrackDurationMs;
            playbackSpeed = _metadataPlaybackSpeed;
        }

        if (playState.State != PlayerPlaybackState.Playing && playState.State != PlayerPlaybackState.Seeking)
        {
            return;
        }

        var durationSeconds = Math.Max(0d, trackDurationMs / 1000d);
        if (Math.Abs(durationSeconds - _durationSeconds) >= 0.001)
        {
            DurationSeconds = durationSeconds;
        }

        var stateAge = nowUtc - playState.TimestampUtc;
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
            _logger.LogDebug(
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

    private void SetPlayState(PlayerPlayState playState)
    {
        var normalizedTimestamp = playState.TimestampUtc == default
            ? DateTimeOffset.UtcNow
            : playState.TimestampUtc;
        var normalizedPlayState = playState with { TimestampUtc = normalizedTimestamp };

        var stateChanged = SetProperty(ref _playState, normalizedPlayState, nameof(PlayState));

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
            ClientId = null;
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
        ClientId = null;
    }
    #endregion

    #region IDisposable

    public async ValueTask DisposeAsync()
    {
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