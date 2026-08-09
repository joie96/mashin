using Microsoft.Extensions.Logging;

namespace mashin.Services;

public sealed class AppRuntimeCoordinator
{
    public enum RuntimeState
    {
        Uninitialized,
        Starting,
        NeedsAuthentication,
        Running,
        DegradedOffline,
        Stopped
    }

    public enum StartupReason
    {
        UiForeground,
        AndroidAutoBrowse,
        TransportNotification,
        ManualRetry
    }

    public enum ShutdownReason
    {
        UserLogout,
        AppTermination,
        ManualStop
    }

    private readonly SettingsService _settingsService;
    private readonly MusicAssistantService _musicAssistantService;
    private readonly IConnectionService _connectionService;
    private readonly UserDataService _userDataService;
    private readonly PlaybackService _playbackService;
    private readonly ILogger<AppRuntimeCoordinator> _logger;
    private readonly SemaphoreSlim _startupGate = new(1, 1);

    private Task? _startupTask;
    private bool _userDataLoaded;
    private RuntimeState _state = RuntimeState.Uninitialized;

    public AppRuntimeCoordinator(
        SettingsService settingsService,
        MusicAssistantService musicAssistantService,
        IConnectionService connectionService,
        UserDataService userDataService,
        PlaybackService playbackService,
        ILogger<AppRuntimeCoordinator> logger)
    {
        _settingsService = settingsService;
        _musicAssistantService = musicAssistantService;
        _connectionService = connectionService;
        _userDataService = userDataService;
        _playbackService = playbackService;
        _logger = logger;
    }

    public event EventHandler<RuntimeState>? StateChanged;

    public RuntimeState State => _state;

    public async Task EnsureStartedAsync(StartupReason reason, CancellationToken cancellationToken = default)
    {
        Task startupTask;

        await _startupGate.WaitAsync(cancellationToken);
        try
        {
            if (_state == RuntimeState.Running)
            {
                return;
            }

            if (_startupTask == null
                || _startupTask.IsFaulted
                || _startupTask.IsCanceled
                || (_startupTask.IsCompleted && _state != RuntimeState.Starting))
            {
                SetState(RuntimeState.Starting);
                _startupTask = StartCoreAsync(reason);
            }

            startupTask = _startupTask;
        }
        finally
        {
            _startupGate.Release();
        }

        await startupTask.WaitAsync(cancellationToken);

        await _startupGate.WaitAsync(cancellationToken);
        try
        {
            if (ReferenceEquals(_startupTask, startupTask) && startupTask.IsCompleted)
            {
                _startupTask = null;
            }
        }
        finally
        {
            _startupGate.Release();
        }
    }

    public async Task SubmitCredentialsAsync(
        string username,
        string password,
        string serverUri,
        CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(username))
        {
            throw new ArgumentException("Benutzername fehlt.", nameof(username));
        }

        if (string.IsNullOrWhiteSpace(password))
        {
            throw new ArgumentException("Passwort fehlt.", nameof(password));
        }

        if (!Uri.TryCreate(serverUri?.Trim(), UriKind.Absolute, out var parsedServerUri))
        {
            throw new ArgumentException("Ungültige Server-URI.", nameof(serverUri));
        }

        _settingsService.MusicAssistantUrl = parsedServerUri.ToString().TrimEnd('/');
        _settingsService.Username = username.Trim();
        _settingsService.Save();

        await _musicAssistantService.LoginAsync(username.Trim(), password, cancellationToken);
        await EnsureStartedAsync(StartupReason.ManualRetry, cancellationToken);
    }

    public async Task LogoutAsync(CancellationToken cancellationToken = default)
    {
        try
        {
            await _musicAssistantService.LogoutAsync(cancellationToken);
        }
        catch (Exception ex) when (ex.Message.Contains("No token in context", StringComparison.OrdinalIgnoreCase))
        {
            _logger.LogInformation(ex, "Logout skipped because no token was available in context.");
        }

        await StopAsync(ShutdownReason.UserLogout, cancellationToken);
    }

    public async Task StopAsync(ShutdownReason reason, CancellationToken cancellationToken = default)
    {
        _logger.LogInformation("Runtime stop requested. Reason={Reason}", reason);

        if (reason == ShutdownReason.AppTermination)
        {
            try
            {
                await _playbackService.TerminateForAppShutdownAsync(cancellationToken);
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Playback shutdown failed during runtime stop.");
            }
        }

        await _connectionService.StopReconnectLoopAsync(cancellationToken);
        await _connectionService.DisconnectAsync(cancellationToken);

        await _startupGate.WaitAsync(cancellationToken);
        try
        {
            _startupTask = null;
            if (reason is ShutdownReason.UserLogout or ShutdownReason.AppTermination)
            {
                _userDataLoaded = false;
            }

            SetState(RuntimeState.Stopped);
        }
        finally
        {
            _startupGate.Release();
        }
    }

    private async Task StartCoreAsync(StartupReason reason)
    {
        _logger.LogInformation("Runtime startup requested. Reason={Reason}", reason);

        try
        {
            if (!Uri.TryCreate(_settingsService.MusicAssistantUrl?.Trim(), UriKind.Absolute, out _))
            {
                _logger.LogInformation("Startup paused because no valid server URL is configured.");
                SetState(RuntimeState.NeedsAuthentication);
                return;
            }

            var serverReachable = await _connectionService.TestServerReachabilityAsync();
            if (!serverReachable)
            {
                _logger.LogInformation("Startup paused because server is unreachable.");
                SetState(RuntimeState.DegradedOffline);
                return;
            }

            var isAuthenticated = await _musicAssistantService.TestAuthentificatonAsync();
            if (!isAuthenticated)
            {
                _logger.LogInformation("Startup paused because authentication is required.");
                SetState(RuntimeState.NeedsAuthentication);
                return;
            }

            await _connectionService.ConnectAsync();
            await _connectionService.StartReconnectLoopAsync();

            if (!_userDataLoaded)
            {
                await _userDataService.LoadPreferencesAsync();
                _userDataLoaded = true;
            }

            if (!_playbackService.IsInitialized)
            {
                try
                {
                    await _playbackService.InitializeAsync();
                }
                catch (Exception ex) when (ex is TimeoutException or TaskCanceledException)
                {
                    _logger.LogWarning(ex, "Playback initialization failed for Sendspin. Falling back to Local mode.");

                    try
                    {
                        await _playbackService.SetOutputModeAsync(PlaybackOutputMode.Local);
                    }
                    catch (Exception fallbackEx)
                    {
                        _logger.LogWarning(fallbackEx, "Failed to switch playback output to Local after initialization error.");
                    }
                }
            }

            SetState(RuntimeState.Running);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Runtime startup failed. Falling back to degraded offline state.");
            SetState(RuntimeState.DegradedOffline);
        }
    }

    private void SetState(RuntimeState nextState)
    {
        if (_state == nextState)
        {
            return;
        }

        _state = nextState;
        StateChanged?.Invoke(this, nextState);
    }
}
