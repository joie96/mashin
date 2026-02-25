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

public interface IPlayerService : IAsyncDisposable, INotifyPropertyChanged
{
    #region Properties
    bool IsConnected { get; }
    string? ConnectedServerName { get; }
    string? ClientId { get; }

    bool IsPlaying { get; }
    bool IsBuffering { get; }
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

    private bool _isPlaying;
    private bool _isBuffering;
    private int _volume;
    private double _durationSeconds;
    private double _positionSeconds;
    private string? _trackTitle;
    private string? _trackArtist;
    private string? _trackAlbum;
    private bool _isMuted;
    private bool? _shuffleEnabled;
    private string? _repeatMode;
    private double _lastServerPositionSeconds;
    private DateTime _lastServerPositionTimestampUtc;
    #endregion

    #region Events
    public event PropertyChangedEventHandler? PropertyChanged;
    #endregion

    #region Properties
    public bool IsConnected => _client?.ConnectionState == ConnectionState.Connected;

    public string? ConnectedServerName => _client?.ServerName;
    public string? ClientId { get; private set; }

    public bool IsPlaying
    {
        get => _isPlaying;
        private set => SetProperty(ref _isPlaying, value);
    }

    public bool IsBuffering
    {
        get => _isBuffering;
        private set => SetProperty(ref _isBuffering, value);
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
            IsBuffering = false;
            IsPlaying = false;
        }

        OnPropertyChanged(nameof(IsConnected));
    }

    private void OnGroupStateChanged(object? sender, GroupState group)
    {
        var stateName = group.PlaybackState.ToString();
        IsPlaying = stateName.Equals("Playing", StringComparison.OrdinalIgnoreCase);
        IsBuffering = stateName.Equals("Buffering", StringComparison.OrdinalIgnoreCase)
            || stateName.Equals("Loading", StringComparison.OrdinalIgnoreCase);

        var clampedVolume = Math.Max(0, Math.Min(100, group.Volume));
        Volume = clampedVolume;
        IsMuted = group.Muted;

        ShuffleEnabled = group.Shuffle;
        RepeatMode = group.Repeat?.ToString();

        var md = group.Metadata;
        if (md == null)
        {
            DurationSeconds = 0;
            TrackTitle = null;
            TrackArtist = null;
            TrackAlbum = null;
            UpdateServerPosition(0);
            return;
        }

        DurationSeconds = md.Duration is > 0 ? md.Duration.Value : 0;
        TrackTitle = md.Title;
        TrackArtist = md.Artist;
        TrackAlbum = md.Album;
        var serverPosition = md.Position ?? 0;
        UpdateServerPosition(serverPosition);
    }
    #endregion

    #region Position Tracking

    private void UpdateServerPosition(double serverPositionSeconds)
    {
        lock (_stateLock)
        {
            _lastServerPositionSeconds = Math.Max(0, serverPositionSeconds);
            _lastServerPositionTimestampUtc = DateTime.UtcNow;
        }

        UpdatePositionFromTimer();
    }

    private void UpdatePositionFromTimer()
    {
        double lastPosition;
        DateTime lastTimestamp;
        double duration;
        bool playing;

        lock (_stateLock)
        {
            lastPosition = _lastServerPositionSeconds;
            lastTimestamp = _lastServerPositionTimestampUtc;
            duration = _durationSeconds;
            playing = _isPlaying;
        }

        double nextPosition;
        if (playing && lastTimestamp != default)
        {
            var delta = DateTime.UtcNow - lastTimestamp;
            nextPosition = lastPosition + delta.TotalSeconds;
        }
        else
        {
            nextPosition = lastPosition;
        }

        if (duration > 0)
        {
            nextPosition = Math.Max(0, Math.Min(duration, nextPosition));
        }

        if (Math.Abs(nextPosition - _positionSeconds) >= 0.1)
        {
            PositionSeconds = nextPosition;
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