using Microsoft.Extensions.Logging;
using System.Net.Sockets;

namespace mashin.Services;

public enum CustomConnectionState
{
    Offline,
    Reconnecting,
    Online
}

public interface IConnectionService : IAsyncDisposable
{
    CustomConnectionState ConnectionState { get; }

    event EventHandler<CustomConnectionState>? ConnectionStateChanged;

    Task StartAsync(CancellationToken cancellationToken = default);

    Task StopAsync(CancellationToken cancellationToken = default);

    Task SetOfflineModeAsync(bool offlineMode, CancellationToken cancellationToken = default);

    Task ConnectAsync(CancellationToken cancellationToken = default);
}

public sealed class ConnectionService : IConnectionService
{
    #region Constants

    private static readonly TimeSpan ReconnectInitialDelay = TimeSpan.FromSeconds(1);
    private static readonly TimeSpan ReconnectMaxDelay = TimeSpan.FromSeconds(5);

    #endregion

    #region Fields

    private readonly IMusicAssistantEventHub _eventHub;
    private readonly SendspinPlayerService _sendspinPlayer;
    private readonly SettingsService _settingsService;
    private readonly ILogger<ConnectionService> _logger;
    private readonly SemaphoreSlim _lifecycleGate = new(1, 1);

    private CancellationTokenSource? _loopCts;
    private Task? _reconnectLoopTask;
    private volatile CustomConnectionState _connectionState;

    #endregion

    #region Construction

    public ConnectionService(
        IMusicAssistantEventHub eventHub,
        SendspinPlayerService sendspinPlayer,
        SettingsService settingsService,
        ILogger<ConnectionService> logger)
    {
        _eventHub = eventHub;
        _sendspinPlayer = sendspinPlayer;
        _settingsService = settingsService;
        _logger = logger;
        _connectionState = _eventHub.IsConnected && _sendspinPlayer.IsConnected
            ? CustomConnectionState.Online
            : CustomConnectionState.Reconnecting;

        _eventHub.ConnectionStateChanged += OnUnderlyingConnectionStateChanged;
        _sendspinPlayer.ConnectionStateChanged += OnUnderlyingConnectionStateChanged;
    }

    #endregion

    #region Properties

    public CustomConnectionState ConnectionState => _connectionState;

    #endregion

    #region Events

    public event EventHandler<CustomConnectionState>? ConnectionStateChanged;

    #endregion

    #region Lifecycle

    public async Task StartAsync(CancellationToken cancellationToken = default)
    {
        await _lifecycleGate.WaitAsync(cancellationToken);
        try
        {
            if (_loopCts != null)
            {
                return;
            }

            _loopCts = new CancellationTokenSource();
            _reconnectLoopTask = Task.Run(() => RunReconnectLoopAsync(_loopCts.Token), CancellationToken.None);
            _logger.LogInformation("Connection service started.");
        }
        finally
        {
            _lifecycleGate.Release();
        }
    }

    public async Task StopAsync(CancellationToken cancellationToken = default)
    {
        CancellationTokenSource? loopCts;
        Task? reconnectLoopTask;

        await _lifecycleGate.WaitAsync(cancellationToken);
        try
        {
            loopCts = _loopCts;
            reconnectLoopTask = _reconnectLoopTask;

            _loopCts = null;
            _reconnectLoopTask = null;
        }
        finally
        {
            _lifecycleGate.Release();
        }

        if (loopCts != null)
        {
            loopCts.Cancel();
            loopCts.Dispose();
        }

        await AwaitLoopTaskAsync(reconnectLoopTask);

        _logger.LogInformation("Connection service stopped.");
    }

    public async Task SetOfflineModeAsync(bool offlineMode, CancellationToken cancellationToken = default)
    {
        if (offlineMode)
        {
            if (_connectionState == CustomConnectionState.Offline)
            {
                return;
            }

            SetConnectionState(CustomConnectionState.Offline);
            await _sendspinPlayer.DisconnectAsync(cancellationToken);
            await _eventHub.DisconnectAsync(cancellationToken);
            _logger.LogInformation("Connection service switched to offline mode.");
            return;
        }

        if (_connectionState != CustomConnectionState.Offline)
        {
            return;
        }

        _logger.LogInformation("Connection service switched to online mode.");
        SetConnectionState(CustomConnectionState.Reconnecting);
        await ConnectAsync(cancellationToken);
    }

    public async Task ConnectAsync(CancellationToken cancellationToken = default)
    {
        if (_connectionState == CustomConnectionState.Offline)
        {
            _logger.LogInformation("Manual connect ignored because offline mode is active.");
            return;
        }

        await _eventHub.ConnectAsync(cancellationToken);
        await _sendspinPlayer.ConnectAsync(cancellationToken);
        RefreshConnectionState();
    }

    public async ValueTask DisposeAsync()
    {
        _eventHub.ConnectionStateChanged -= OnUnderlyingConnectionStateChanged;
        _sendspinPlayer.ConnectionStateChanged -= OnUnderlyingConnectionStateChanged;

        await StopAsync();
        _lifecycleGate.Dispose();
    }

    #endregion

    #region Reconnect Loop

    private async Task RunReconnectLoopAsync(CancellationToken cancellationToken)
    {
        var retryDelay = ReconnectInitialDelay;

        while (!cancellationToken.IsCancellationRequested)
        {
            if (_connectionState == CustomConnectionState.Offline
                || (_eventHub.IsConnected && _sendspinPlayer.IsConnected))
            {
                retryDelay = ReconnectInitialDelay;
                await DelaySafeAsync(ReconnectInitialDelay, cancellationToken);
                continue;
            }

            try
            {
                if (!await TestServerConnectionReachabilityAsync(cancellationToken))
                {
                    await DelaySafeAsync(retryDelay, cancellationToken);
                    retryDelay = TimeSpan.FromSeconds(Math.Min(retryDelay.TotalSeconds * 2, ReconnectMaxDelay.TotalSeconds));
                    continue;
                }

                if (!_eventHub.IsConnected)
                {
                    await _eventHub.ConnectAsync(cancellationToken);
                }

                if (!_sendspinPlayer.IsConnected)
                {
                    await _sendspinPlayer.ReconnectAsync(cancellationToken);
                }

                RefreshConnectionState();

                if (_eventHub.IsConnected && _sendspinPlayer.IsConnected)
                {
                    retryDelay = ReconnectInitialDelay;
                    continue;
                }
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                return;
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Reconnect attempt failed.");
            }

            await DelaySafeAsync(retryDelay, cancellationToken);
            retryDelay = TimeSpan.FromSeconds(Math.Min(retryDelay.TotalSeconds * 2, ReconnectMaxDelay.TotalSeconds));
        }
    }

    #endregion

    #region Event Handlers

    private void OnUnderlyingConnectionStateChanged(object? sender, bool _)
    {
        RefreshConnectionState();
    }

    #endregion

    #region State Management

    private void RefreshConnectionState()
    {
        if (_connectionState == CustomConnectionState.Offline)
        {
            return;
        }

        SetConnectionState(_eventHub.IsConnected && _sendspinPlayer.IsConnected
            ? CustomConnectionState.Online
            : CustomConnectionState.Reconnecting);
    }

    private void SetConnectionState(CustomConnectionState nextState)
    {
        if (_connectionState == nextState)
        {
            return;
        }

        _connectionState = nextState;
        ConnectionStateChanged?.Invoke(this, nextState);
    }

    #endregion

    #region Helpers

    private static async Task AwaitLoopTaskAsync(Task? loopTask)
    {
        if (loopTask == null)
        {
            return;
        }

        try
        {
            await loopTask;
        }
        catch (OperationCanceledException)
        {
            // Expected on shutdown.
        }
    }

    private static async Task DelaySafeAsync(TimeSpan delay, CancellationToken cancellationToken)
    {
        try
        {
            await Task.Delay(delay, cancellationToken);
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            // Expected on cancellation.
        }
    }

    private async Task<bool> TestServerConnectionReachabilityAsync(CancellationToken cancellationToken)
    {
        if (!Uri.TryCreate(_settingsService.SendspinUrl, UriKind.Absolute, out var sendspinUri))
        {
            _logger.LogWarning("Reachability check skipped because configured URL is invalid. Url={SendspinUrl}", _settingsService.SendspinUrl);
            return false;
        }

        var host = sendspinUri.Host;
        if (string.IsNullOrWhiteSpace(host))
        {
            _logger.LogWarning("Reachability check skipped because endpoint host is empty. Url={SendspinUrl}", _settingsService.SendspinUrl);
            return false;
        }

        var port = sendspinUri.Port;
        if (port <= 0)
        {
            port = string.Equals(sendspinUri.Scheme, "wss", StringComparison.OrdinalIgnoreCase) ? 443 : 80;
        }

        try
        {
            using var tcpClient = new TcpClient();
            using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            timeoutCts.CancelAfter(TimeSpan.FromSeconds(2));

            await tcpClient.ConnectAsync(host, port, timeoutCts.Token);
            _logger.LogInformation("Server is reachable. Host={Host}, Port={Port}", host, port);
            return true;
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            throw;
        }
        catch (OperationCanceledException)
        {
            _logger.LogDebug("Server reachability check timed out. Host={Host}, Port={Port}", host, port);
            return false;
        }
        catch (Exception ex)
        {
            _logger.LogDebug("Server is unreachable. Host={Host}, Port={Port}", host, port);
            return false;
        }
    }

    #endregion
}
