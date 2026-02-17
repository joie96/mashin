using Microsoft.Extensions.Logging;
using Sendspin.SDK.Audio;
using Sendspin.SDK.Client;
using Sendspin.SDK.Connection;
using Sendspin.SDK.Models;
using Sendspin.SDK.Synchronization;
using mashin.Audio;

namespace mashin.Services;

public interface IPlayerService : IAsyncDisposable
{
    event EventHandler<ConnectionStateChangedEventArgs>? ConnectionStateChanged;
    event EventHandler<GroupState>? GroupStateChanged;
    event EventHandler<byte[]>? ArtworkReceived;

    bool IsConnected { get; }
    string? ConnectedServerName { get; }
    string? ClientId { get; }

    Task ConnectAsync(Uri serverUri, CancellationToken cancellationToken = default);
    Task DisconnectAsync();

    Task SendCommandAsync(string command, Dictionary<string, object>? parameters = null);
}

public sealed class PlayerService : IPlayerService
{
    private readonly ILogger<PlayerService> _logger;
    private readonly ILoggerFactory _loggerFactory;
    private readonly IAudioPipeline _audioPipeline;
    private readonly IClockSynchronizer _clockSynchronizer;
    private readonly SettingsService _settingsService;

    private CustomSendspinClientService? _client;
    private ISendspinConnection? _connection;

    private readonly SemaphoreSlim _cleanupLock = new(1, 1);

    public event EventHandler<ConnectionStateChangedEventArgs>? ConnectionStateChanged;
    public event EventHandler<GroupState>? GroupStateChanged;
    public event EventHandler<byte[]>? ArtworkReceived;

    public bool IsConnected => _client?.ConnectionState == ConnectionState.Connected;

    public string? ConnectedServerName => _client?.ServerName;
    public string? ClientId { get; private set; }

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
    }

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

            _client = new CustomSendspinClientService(
                _loggerFactory.CreateLogger<SendspinClientService>(),
                _connection,
                clockSynchronizer: _clockSynchronizer,
                capabilities: clientCapabilities,
                audioPipeline: _audioPipeline);

            _client.ConnectionStateChanged += OnConnectionStateChanged;
            _client.GroupStateChanged += OnGroupStateChanged;
            _client.ArtworkReceived += OnArtworkReceived;

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

    public async Task SendCommandAsync(string command, Dictionary<string, object>? parameters = null)
    {
        if (_client?.ConnectionState != ConnectionState.Connected)
        {
            _logger.LogWarning("Cannot send command {Command}: not connected", command);
            return;
        }

        await _client.SendCommandAsync(command, parameters);
    }

    private void OnConnectionStateChanged(object? sender, ConnectionStateChangedEventArgs e)
         => Task.Run(() => ConnectionStateChanged?.Invoke(this, e));

    private void OnGroupStateChanged(object? sender, GroupState group)
         => Task.Run(() => GroupStateChanged?.Invoke(this, group));

    private void OnArtworkReceived(object? sender, byte[] imageData)
         => Task.Run(() => ArtworkReceived?.Invoke(this, imageData));

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
        _client.ArtworkReceived -= OnArtworkReceived;

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

    public async ValueTask DisposeAsync()
    {
        await DisconnectAsync();
        _cleanupLock.Dispose();
    }
}