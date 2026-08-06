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

    Task StartReconnectLoopAsync(CancellationToken cancellationToken = default);

    Task StopReconnectLoopAsync(CancellationToken cancellationToken = default);

    Task SetOfflineModeAsync(bool offlineMode, CancellationToken cancellationToken = default);

    Task ConnectAsync(CancellationToken cancellationToken = default);

    Task DisconnectAsync(CancellationToken cancellationToken = default);

    Task<bool> TestServerReachabilityAsync(CancellationToken cancellationToken = default);
}

public sealed class ConnectionService : IConnectionService
{
    #region Constants

    private static readonly TimeSpan ReconnectInitialDelay = TimeSpan.FromSeconds(1);
    private static readonly TimeSpan ReconnectMaxDelay = TimeSpan.FromSeconds(5);
    private static readonly TimeSpan ProbeConnectTimeout = TimeSpan.FromSeconds(3);

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

    public async Task StartReconnectLoopAsync(CancellationToken cancellationToken = default)
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
            RefreshConnectionState();
            _logger.LogInformation("Connection service started.");
        }
        finally
        {
            _lifecycleGate.Release();
        }
    }

    public async Task StopReconnectLoopAsync(CancellationToken cancellationToken = default)
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

        RefreshConnectionState();

        _logger.LogInformation("Connection service stopped.");
    }

    public async Task SetOfflineModeAsync(bool offlineMode, CancellationToken cancellationToken = default)
    {
        if (offlineMode)
        {
            if (!IsReconnectLoopRunning() && !_eventHub.IsConnected && !_sendspinPlayer.IsConnected)
            {
                return;
            }

            await StopReconnectLoopAsync(cancellationToken);
            await DisconnectAsync(cancellationToken);
            _logger.LogInformation("Connection service switched to offline mode.");
            return;
        }

        if (IsReconnectLoopRunning())
        {
            return;
        }

        _logger.LogInformation("Connection service switched to online mode.");
        await ConnectAsync(cancellationToken);
        await StartReconnectLoopAsync(cancellationToken);
    }

    public async Task ConnectAsync(CancellationToken cancellationToken = default)
    {
        if (_connectionState == CustomConnectionState.Offline)
        {
            _logger.LogInformation("Manual connect ignored because offline mode is active.");
            return;
        }

        var isOnline = await TestServerReachabilityAsync(cancellationToken);
        if (!isOnline)
        {
            _logger.LogInformation("Manual connect ignored because server is offline.");
            return;
        }

        await _eventHub.ConnectAsync(cancellationToken);
        await _sendspinPlayer.ConnectAsync(cancellationToken);
        RefreshConnectionState();
    }

    public async Task DisconnectAsync(CancellationToken cancellationToken = default)
    {
        await _sendspinPlayer.DisconnectAsync(cancellationToken);
        await _eventHub.DisconnectAsync(cancellationToken);
        RefreshConnectionState();
    }

    public async Task<bool> TestServerReachabilityAsync(CancellationToken cancellationToken = default)
    {
        if (!Uri.TryCreate(_settingsService.MusicAssistantUrl?.Trim(), UriKind.Absolute, out var serverUri))
        {
            return false;
        }

        var port = serverUri.IsDefaultPort
            ? (string.Equals(serverUri.Scheme, Uri.UriSchemeHttps, StringComparison.OrdinalIgnoreCase) ? 443 : 80)
            : serverUri.Port;

        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeoutCts.CancelAfter(ProbeConnectTimeout);

        try
        {
            using var tcpClient = new TcpClient();
            await tcpClient.ConnectAsync(serverUri.Host, port, timeoutCts.Token);
            return true;
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            throw;
        }
        catch (TaskCanceledException)
        {
            return false;
        }
        catch (OperationCanceledException)
        {
            return false;
        }
        catch (SocketException)
        {
            return false;
        }
        catch
        {
            return false;
        }
    }

    public async ValueTask DisposeAsync()
    {
        _eventHub.ConnectionStateChanged -= OnUnderlyingConnectionStateChanged;
        _sendspinPlayer.ConnectionStateChanged -= OnUnderlyingConnectionStateChanged;

        await StopReconnectLoopAsync();
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
                var isOnline = await TestServerReachabilityAsync(cancellationToken);
                if (!isOnline)
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
        if (_eventHub.IsConnected && _sendspinPlayer.IsConnected)
        {
            SetConnectionState(CustomConnectionState.Online);
            return;
        }

        if (IsReconnectLoopRunning())
        {
            SetConnectionState(CustomConnectionState.Reconnecting);
            return;
        }

        SetConnectionState(CustomConnectionState.Offline);
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

    private bool IsReconnectLoopRunning()
    {
        return _loopCts != null
            && _reconnectLoopTask != null
            && !_reconnectLoopTask.IsCompleted;
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
    #endregion
}
