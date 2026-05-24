using mashin.Models;
using mashin.Views.Desktop.Controls;
using Microsoft.Extensions.Logging;
using System.Threading;

namespace mashin.Services;

/// <summary>
/// Centralizes application overlays (create/update/delete playlist and login),
/// serializes overlay display, and returns user input/results asynchronously to callers.
/// </summary>
public interface IOverlayService
{
    void Initialize(
        Grid overlayHost,
        ContentPresenter overlayContent,
        Grid flyoutHost,
        ContentPresenter flyoutContent);
    void OnBackdropTapped();
    void OnFlyoutBackdropTapped();

    Task ShowContextMenuMainAsync(View menuView, Action? onClose);
    Task HideContextMenuMainAsync();
    Task ShowContextMenuSubMenuAsync(View subMenuView, Action? onClose);
    Task HideContextMenuSubMenuAsync();

    bool IsQueueOverlayOpen { get; }
    bool IsQueueOverlayAnimating { get; }
    Task ShowQueueOverlayAsync(object bindingContext);
    Task HideQueueOverlayAsync();

    Task<string?> ShowCreatePlaylistAsync();
    Task<string?> ShowUpdatePlaylistAsync(Playlist playlist);
    Task<bool> ShowDeletePlaylistAsync(Playlist playlist);
    Task<bool> ShowLoginAsync(
        string? initialUsername,
        Func<string, string, Task<(bool Success, string? ErrorMessage)>> authenticateAsync,
        Func<Task<(bool Success, string? ErrorMessage)>>? tryAutoLoginAsync = null,
        string? autoLoginStatusMessage = null);
}

/// <summary>
/// Hosts and coordinates reusable overlay controls, exposing async methods for view models
/// and pages to request user interaction without owning overlay lifecycle details.
/// </summary>
public sealed class OverlayService : IOverlayService
{
    #region Fields

    private readonly ILogger<OverlayService> _logger;
    private readonly SemaphoreSlim _overlayLock = new(1, 1);

    private readonly CreatePlaylistOverlay _createPlaylistOverlay;
    private readonly UpdatePlaylistOverlay _updatePlaylistOverlay;
    private readonly DeletePlaylistOverlay _deletePlaylistOverlay;
    private readonly LoginOverlay _loginOverlay;
    private readonly QueueOverlay _queueOverlay;

    private Grid? _overlayHost;
    private ContentPresenter? _overlayContent;
    private Grid? _flyoutHost;
    private ContentPresenter? _flyoutContent;

    private Action? _overlayCloseAction;
    private Action? _flyoutCloseAction;

    private TaskCompletionSource<string?>? _createPlaylistTcs;
    private TaskCompletionSource<string?>? _updatePlaylistTcs;
    private TaskCompletionSource<bool>? _deletePlaylistTcs;
    private TaskCompletionSource<bool>? _loginTcs;

    private Func<string, string, Task<(bool Success, string? ErrorMessage)>>? _authenticateLoginAsync;

    #endregion

    #region Construction

    public OverlayService(ILogger<OverlayService> logger)
    {
        _logger = logger;

        _createPlaylistOverlay = new CreatePlaylistOverlay();
        _createPlaylistOverlay.CancelClicked += OnCreatePlaylistCancelled;
        _createPlaylistOverlay.CreateClicked += OnCreatePlaylistConfirmed;

        _updatePlaylistOverlay = new UpdatePlaylistOverlay();
        _updatePlaylistOverlay.CancelClicked += OnUpdatePlaylistCancelled;
        _updatePlaylistOverlay.UpdateClicked += OnUpdatePlaylistConfirmed;

        _deletePlaylistOverlay = new DeletePlaylistOverlay();
        _deletePlaylistOverlay.CancelClicked += OnDeletePlaylistCancelled;
        _deletePlaylistOverlay.DeleteClicked += OnDeletePlaylistConfirmed;

        _loginOverlay = new LoginOverlay();
        _loginOverlay.UsernameCompleted += OnLoginUsernameCompleted;
        _loginOverlay.PasswordCompleted += OnLoginPasswordCompleted;
        _loginOverlay.LoginClicked += OnLoginClicked;

        _queueOverlay = new QueueOverlay();
    }

    #endregion

    #region Host API

    public void Initialize(
        Grid overlayHost,
        ContentPresenter overlayContent,
        Grid flyoutHost,
        ContentPresenter flyoutContent)
    {
        _overlayHost = overlayHost;
        _overlayContent = overlayContent;
        _flyoutHost = flyoutHost;
        _flyoutContent = flyoutContent;
    }

    public void OnBackdropTapped()
    {
        _overlayCloseAction?.Invoke();
    }

    public void OnFlyoutBackdropTapped()
    {
        _flyoutCloseAction?.Invoke();
    }

    #endregion

    #region Context Menu API

    public Task ShowContextMenuMainAsync(View menuView, Action? onClose)
    {
        return ShowFlyoutLayerAsync(menuView, onClose);
    }

    public Task HideContextMenuMainAsync()
    {
        return HideFlyoutLayerAsync();
    }

    public Task ShowContextMenuSubMenuAsync(View subMenuView, Action? onClose)
    {
        return MainThread.InvokeOnMainThreadAsync(() =>
        {
            EnsureInitialized();
            ShowCenteredOverlayInternal(subMenuView, onClose);
        });
    }

    public Task HideContextMenuSubMenuAsync()
    {
        return MainThread.InvokeOnMainThreadAsync(() =>
        {
            if (_overlayHost == null || _overlayContent == null)
            {
                return;
            }

            _overlayHost.IsVisible = false;
            _overlayContent.Content = null;
            _overlayCloseAction = null;
        });
    }

    #endregion

    #region Queue API

    public bool IsQueueOverlayOpen => _queueOverlay.IsOpen;

    public bool IsQueueOverlayAnimating => _queueOverlay.IsAnimating;

    public async Task ShowQueueOverlayAsync(object bindingContext)
    {
        if (bindingContext == null)
        {
            return;
        }

        EnsureInitialized();

        await MainThread.InvokeOnMainThreadAsync(async () =>
        {
            // Force a clean rebind cycle in case the same view model instance is reused.
            _queueOverlay.BindingContext = null;
            _queueOverlay.BindingContext = bindingContext;

            await ShowFlyoutLayerAsync(_queueOverlay, () => _ = HideQueueOverlayAsync());
            await _queueOverlay.ShowAsync();
        });
    }

    public async Task HideQueueOverlayAsync()
    {
        EnsureInitialized();

        await MainThread.InvokeOnMainThreadAsync(async () =>
        {
            await _queueOverlay.HideAsync();

            if (_flyoutHost != null)
            {
                _flyoutHost.IsVisible = false;
            }

            // Keep queue content mounted so TableView does not run full unload cleanup.
            _flyoutCloseAction = null;
        });
    }

    #endregion

    #region Playlist Overlay API

    public async Task<string?> ShowCreatePlaylistAsync()
    {
        await _overlayLock.WaitAsync();

        try
        {
            EnsureInitialized();

            _createPlaylistTcs = new TaskCompletionSource<string?>(TaskCreationOptions.RunContinuationsAsynchronously);

            await MainThread.InvokeOnMainThreadAsync(() =>
            {
                _createPlaylistOverlay.PlaylistName = string.Empty;
                ShowCenteredOverlayInternal(_createPlaylistOverlay, () => _createPlaylistTcs.TrySetResult(null));
            });

            return await _createPlaylistTcs.Task;
        }
        finally
        {
            _createPlaylistTcs = null;
            await HideCenteredOverlayInternalAsync();
            _overlayLock.Release();
        }
    }

    public async Task<string?> ShowUpdatePlaylistAsync(Playlist playlist)
    {
        if (playlist is null)
        {
            return null;
        }

        await _overlayLock.WaitAsync();

        try
        {
            EnsureInitialized();

            _updatePlaylistTcs = new TaskCompletionSource<string?>(TaskCreationOptions.RunContinuationsAsynchronously);

            await MainThread.InvokeOnMainThreadAsync(() =>
            {
                _updatePlaylistOverlay.PlaylistName = playlist.DisplayName ?? playlist.Name ?? string.Empty;
                ShowCenteredOverlayInternal(_updatePlaylistOverlay, () => _updatePlaylistTcs.TrySetResult(null));
            });

            return await _updatePlaylistTcs.Task;
        }
        finally
        {
            _updatePlaylistTcs = null;
            await HideCenteredOverlayInternalAsync();
            _overlayLock.Release();
        }
    }

    public async Task<bool> ShowDeletePlaylistAsync(Playlist playlist)
    {
        if (playlist is null)
        {
            return false;
        }

        await _overlayLock.WaitAsync();

        try
        {
            EnsureInitialized();

            _deletePlaylistTcs = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);

            await MainThread.InvokeOnMainThreadAsync(() =>
            {
                _deletePlaylistOverlay.PlaylistName = playlist.DisplayName ?? playlist.Name ?? string.Empty;
                _deletePlaylistOverlay.IsDeleteEnabled = true;
                ShowCenteredOverlayInternal(_deletePlaylistOverlay, () => _deletePlaylistTcs.TrySetResult(false));
            });

            return await _deletePlaylistTcs.Task;
        }
        finally
        {
            _deletePlaylistTcs = null;
            await HideCenteredOverlayInternalAsync();
            _overlayLock.Release();
        }
    }

    #endregion

    #region Login Overlay API

    public async Task<bool> ShowLoginAsync(
        string? initialUsername,
        Func<string, string, Task<(bool Success, string? ErrorMessage)>> authenticateAsync,
        Func<Task<(bool Success, string? ErrorMessage)>>? tryAutoLoginAsync = null,
        string? autoLoginStatusMessage = null)
    {
        if (authenticateAsync is null)
        {
            return false;
        }

        await _overlayLock.WaitAsync();

        try
        {
            EnsureInitialized();

            _authenticateLoginAsync = authenticateAsync;
            _loginTcs = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);

            await MainThread.InvokeOnMainThreadAsync(() =>
            {
                _loginOverlay.Username = initialUsername ?? string.Empty;
                _loginOverlay.Password = string.Empty;
                _loginOverlay.HideError();
                _loginOverlay.SetStatusMessage(string.Empty);
                _loginOverlay.SetLoadingState(false);
                ShowCenteredOverlayInternal(_loginOverlay, null);

                if (string.IsNullOrWhiteSpace(_loginOverlay.Username))
                {
                    _loginOverlay.FocusUsername();
                }
                else
                {
                    _loginOverlay.FocusPassword();
                }
            });

            if (tryAutoLoginAsync is not null)
            {
                await MainThread.InvokeOnMainThreadAsync(() =>
                {
                    _loginOverlay.SetStatusMessage(autoLoginStatusMessage ?? "Verbindung wird getestet...");
                    _loginOverlay.SetLoadingState(true);
                });

                try
                {
                    var (success, errorMessage) = await tryAutoLoginAsync();
                    if (success)
                    {
                        _loginTcs.TrySetResult(true);
                    }
                    else
                    {
                        await MainThread.InvokeOnMainThreadAsync(() =>
                        {
                            _loginOverlay.SetLoadingState(false);
                            _loginOverlay.SetStatusMessage(string.Empty);

                            if (!string.IsNullOrWhiteSpace(errorMessage))
                            {
                                _loginOverlay.ShowError(errorMessage);
                            }

                            if (string.IsNullOrWhiteSpace(_loginOverlay.Username))
                            {
                                _loginOverlay.FocusUsername();
                            }
                            else
                            {
                                _loginOverlay.FocusPassword();
                            }
                        });
                    }
                }
                catch (Exception ex)
                {
                    _logger.LogWarning(ex, "Auto login failed in overlay service");
                    await MainThread.InvokeOnMainThreadAsync(() =>
                    {
                        _loginOverlay.SetLoadingState(false);
                        _loginOverlay.SetStatusMessage(string.Empty);
                        _loginOverlay.ShowError($"Verbindungsfehler: {ex.Message}");

                        if (string.IsNullOrWhiteSpace(_loginOverlay.Username))
                        {
                            _loginOverlay.FocusUsername();
                        }
                        else
                        {
                            _loginOverlay.FocusPassword();
                        }
                    });
                }
            }

            return await _loginTcs.Task;
        }
        finally
        {
            _authenticateLoginAsync = null;
            _loginTcs = null;
            await MainThread.InvokeOnMainThreadAsync(() =>
            {
                _loginOverlay.ClearPassword();
                _loginOverlay.SetStatusMessage(string.Empty);
            });
            await HideCenteredOverlayInternalAsync();
            _overlayLock.Release();
        }
    }

    #endregion

    #region Overlay Event Handlers

    private void OnCreatePlaylistCancelled(object? sender, EventArgs e)
    {
        _createPlaylistTcs?.TrySetResult(null);
    }

    private void OnCreatePlaylistConfirmed(object? sender, EventArgs e)
    {
        var name = _createPlaylistOverlay.PlaylistName?.Trim();
        if (string.IsNullOrWhiteSpace(name))
        {
            return;
        }

        _createPlaylistTcs?.TrySetResult(name);
    }

    private void OnUpdatePlaylistCancelled(object? sender, EventArgs e)
    {
        _updatePlaylistTcs?.TrySetResult(null);
    }

    private void OnUpdatePlaylistConfirmed(object? sender, EventArgs e)
    {
        var name = _updatePlaylistOverlay.PlaylistName?.Trim();
        if (string.IsNullOrWhiteSpace(name))
        {
            return;
        }

        _updatePlaylistTcs?.TrySetResult(name);
    }

    private void OnDeletePlaylistCancelled(object? sender, EventArgs e)
    {
        _deletePlaylistTcs?.TrySetResult(false);
    }

    private void OnDeletePlaylistConfirmed(object? sender, EventArgs e)
    {
        _deletePlaylistOverlay.IsDeleteEnabled = false;
        _deletePlaylistTcs?.TrySetResult(true);
    }

    private void OnLoginUsernameCompleted(object? sender, EventArgs e)
    {
        _loginOverlay.FocusPassword();
    }

    private async void OnLoginPasswordCompleted(object? sender, EventArgs e)
    {
        await TryAuthenticateLoginAsync();
    }

    private async void OnLoginClicked(object? sender, EventArgs e)
    {
        await TryAuthenticateLoginAsync();
    }

    #endregion

    #region Login Flow

    private async Task TryAuthenticateLoginAsync()
    {
        var authenticate = _authenticateLoginAsync;
        if (authenticate is null || _loginTcs is null)
        {
            return;
        }

        var username = _loginOverlay.Username.Trim();
        var password = _loginOverlay.Password;

        if (string.IsNullOrWhiteSpace(username))
        {
            _loginOverlay.ShowError("Bitte geben Sie einen Benutzernamen ein.");
            _loginOverlay.FocusUsername();
            return;
        }

        if (string.IsNullOrWhiteSpace(password))
        {
            _loginOverlay.ShowError("Bitte geben Sie ein Passwort ein.");
            _loginOverlay.FocusPassword();
            return;
        }

        _loginOverlay.HideError();
        _loginOverlay.SetLoadingState(true);

        try
        {
            var (success, errorMessage) = await authenticate(username, password);
            if (success)
            {
                _loginTcs.TrySetResult(true);
                return;
            }

            _loginOverlay.ShowError(errorMessage ?? "Anmeldung fehlgeschlagen. Bitte überprüfen Sie Ihre Anmeldedaten.");
            _loginOverlay.ClearPassword();
            _loginOverlay.FocusPassword();
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Login handling failed in overlay service");
            _loginOverlay.ShowError($"Verbindungsfehler: {ex.Message}");
            _loginOverlay.ClearPassword();
            _loginOverlay.FocusPassword();
        }
        finally
        {
            _loginOverlay.SetLoadingState(false);
        }
    }

    #endregion

    #region Host Helpers

    private Task HideCenteredOverlayInternalAsync()
    {
        return MainThread.InvokeOnMainThreadAsync(() =>
        {
            if (_overlayHost == null || _overlayContent == null)
            {
                return;
            }

            _overlayHost.IsVisible = false;
            _overlayContent.Content = null;
            _overlayCloseAction = null;
        });
    }

    private Task ShowFlyoutLayerAsync(View flyout, Action? onClose)
    {
        return MainThread.InvokeOnMainThreadAsync(() =>
        {
            EnsureInitialized();
            ShowFlyoutInternal(flyout, onClose);
        });
    }

    private Task HideFlyoutLayerAsync()
    {
        return MainThread.InvokeOnMainThreadAsync(() =>
        {
            if (_flyoutHost == null || _flyoutContent == null)
            {
                return;
            }

            _flyoutHost.IsVisible = false;
            _flyoutContent.Content = null;
            _flyoutCloseAction = null;
        });
    }

    private void ShowCenteredOverlayInternal(View overlay, Action? onClose)
    {
        if (_overlayHost == null || _overlayContent == null)
        {
            return;
        }

        _overlayContent.Content = overlay;
        _overlayCloseAction = onClose;
        _overlayHost.IsVisible = true;
    }

    private void ShowFlyoutInternal(View flyout, Action? onClose)
    {
        if (_flyoutHost == null || _flyoutContent == null)
        {
            return;
        }

        _flyoutContent.Content = flyout;
        _flyoutCloseAction = onClose;
        _flyoutHost.IsVisible = true;
    }

    private void EnsureInitialized()
    {
        if (_overlayHost != null && _overlayContent != null && _flyoutHost != null && _flyoutContent != null)
        {
            return;
        }

        throw new InvalidOperationException("OverlayService is not initialized. Call Initialize from MainPage first.");
    }

    #endregion
}