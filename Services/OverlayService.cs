using mashin.Models;
using mashin.Views.Desktop.Controls;
using mashin.Views.Mobile.Controls;
using Microsoft.Extensions.Logging;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using DesktopQueueOverlay = mashin.Views.Desktop.Controls.QueueOverlay;
using MobileQueueOverlay = mashin.Views.Mobile.Controls.QueueOverlay;

namespace mashin.Services;

public enum FlyoutHostType
{
    Default,
    ContextMenu,
    PlayerBar,
    Queue
}

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
        ContentPresenter flyoutContent,
        Grid? selectionIndicatorHost = null,
        ContentPresenter? selectionIndicatorContent = null,
        Grid? secondaryFlyoutHost = null,
        ContentPresenter? secondaryFlyoutContent = null);
    void RegisterFlyoutHost(FlyoutHostType flyoutHostType, Grid flyoutHost, ContentPresenter flyoutContent);
    void OnBackdropTapped();
    void OnDefaultFlyoutBackdropTapped();
    void OnContextMenuFlyoutBackdropTapped();
    void OnPlayerBarFlyoutBackdropTapped();
    void OnQueueFlyoutBackdropTapped();

    Task ShowContextMenuFlyoutAsync(View menuView, Action? onClose);
    Task HideContextMenuFlyoutAsync();
    Task ShowContextMenuSubMenuAsync(View subMenuView, Action? onClose);
    Task HideContextMenuSubMenuAsync();

    bool IsQueueOverlayOpen { get; }
    bool IsQueueOverlayAnimating { get; }
    bool IsQueueOverlayInteractiveOpening { get; }
    bool IsPlayerBarOverlayOpen { get; }
    bool IsPlayerBarOverlayInteractiveOpening { get; }
    bool IsOverlayOpen { get; }
    bool IsFlyoutOpen { get; }
    Task ShowPlayerBarOverlayAsync(object bindingContext);
    Task HidePlayerBarOverlayAsync();
    void BeginPlayerBarOverlayInteractiveOpen(object bindingContext);
    void UpdatePlayerBarOverlayInteractiveOpen(double upwardPullDistance);
    Task EndPlayerBarOverlayInteractiveOpenAsync(bool openPlayerBar);
    Task ShowQueueOverlayAsync(object bindingContext);
    Task HideQueueOverlayAsync();
    void BeginQueueOverlayInteractiveOpen(object bindingContext);
    void UpdateQueueOverlayInteractiveOpen(double upwardPullDistance);
    Task EndQueueOverlayInteractiveOpenAsync(bool openQueue);
    Task CloseOverlayAsync();
    Task CloseFlyoutAsync();
    Task ShowSelectionIndicatorAsync(object selectionControl);
    Task HideSelectionIndicatorAsync(object? selectionControl = null);

    Task<string?> ShowCreatePlaylistAsync();
    Task<string?> ShowUpdatePlaylistAsync(Playlist playlist);
    Task<bool> ShowDeletePlaylistAsync(Playlist playlist);
    Task<(string SortField, bool IsDescending)?> ShowSortContentOverlayAsync();
    Task<(string Username, string Password, string ServerUri)> ShowLoginAsync(
        string? initialUsername,
        string? initialServerUri,
        string? initialErrorMessage = null);
    Task SetLoginLoadingStateAsync(bool isLoading);
    Task ShowLoginErrorAsync(string message);
    Task HideLoginErrorAsync();
}

/// <summary>
/// Hosts and coordinates reusable overlay controls, exposing async methods for view models
/// and pages to request user interaction without owning overlay lifecycle details.
/// </summary>
public sealed class OverlayService : IOverlayService
{
    #region Fields

    private const int QueueOverlayForegroundZIndex = 126;
    private const int QueueOverlayBackgroundZIndex = -100;
    private const int PlayerBarOverlayForegroundZIndex = 125;
    private const int PlayerBarOverlayBackgroundZIndex = -101;

    private enum FlyoutLayoutMode
    {
        Bottom,
        FullHeight
    }

    private sealed class FlyoutHostRegistration
    {
        public required Grid Host { get; init; }
        public required ContentPresenter Content { get; init; }
        public Action? CloseAction { get; set; }
    }

    private readonly ILogger<OverlayService> _logger;
    private readonly SemaphoreSlim _overlayLock = new(1, 1);
    private bool _isQueueInteractiveOpening;
    private bool _isPlayerBarInteractiveOpening;

    private readonly CreatePlaylistOverlay _createPlaylistOverlay;
    private readonly UpdatePlaylistOverlay _updatePlaylistOverlay;
    private readonly DeletePlaylistOverlay _deletePlaylistOverlay;
    private readonly SortContentOverlay _sortContentOverlay;
    private readonly LoginOverlay _loginOverlay;
    private readonly DesktopQueueOverlay _desktopQueueOverlay;
    private readonly MobileQueueOverlay _mobileQueueOverlay;
    private readonly PlayerBarOverlay _playerBarOverlay;
    private readonly SelectionIndicatorOverlay _selectionIndicatorOverlay;
    private readonly Dictionary<FlyoutHostType, FlyoutHostRegistration> _flyoutHosts = new();

    private Grid? _overlayHost;
    private ContentPresenter? _overlayContent;
    private Grid? _selectionIndicatorHost;
    private ContentPresenter? _selectionIndicatorContent;
    private object? _selectionIndicatorControl;

    private Action? _overlayCloseAction;

    private TaskCompletionSource<string?>? _createPlaylistTcs;
    private TaskCompletionSource<string?>? _updatePlaylistTcs;
    private TaskCompletionSource<bool>? _deletePlaylistTcs;
    private TaskCompletionSource<(string SortField, bool IsDescending)?>? _sortContentOverlayTcs;
    private TaskCompletionSource<(string Username, string Password, string ServerUri)>? _loginTcs;

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

        _sortContentOverlay = new SortContentOverlay();
        _sortContentOverlay.CancelClicked += OnSortContentOverlayCancelled;
        _sortContentOverlay.SortClicked += OnSortContentOverlayConfirmed;

        _loginOverlay = new LoginOverlay();
        _loginOverlay.UsernameCompleted += OnLoginUsernameCompleted;
        _loginOverlay.PasswordCompleted += OnLoginPasswordCompleted;
        _loginOverlay.LoginClicked += OnLoginClicked;

        _desktopQueueOverlay = new DesktopQueueOverlay();

        _mobileQueueOverlay = new MobileQueueOverlay();
        _mobileQueueOverlay.DismissRequested += async (_, _) => await HideQueueOverlayAsync();

        _playerBarOverlay = new PlayerBarOverlay();
        _playerBarOverlay.DismissRequested += async (_, _) => await HidePlayerBarOverlayAsync();

        _selectionIndicatorOverlay = new SelectionIndicatorOverlay();
        _selectionIndicatorOverlay.SelectAllTapped += OnSelectionIndicatorSelectAllTapped;
        _selectionIndicatorOverlay.MenuTapped += OnSelectionIndicatorMenuTapped;
        _selectionIndicatorOverlay.CloseTapped += OnSelectionIndicatorCloseTapped;
    }

    #endregion

    #region Host API

    public void Initialize(
        Grid overlayHost,
        ContentPresenter overlayContent,
        Grid flyoutHost,
        ContentPresenter flyoutContent,
        Grid? selectionIndicatorHost = null,
        ContentPresenter? selectionIndicatorContent = null,
        Grid? secondaryFlyoutHost = null,
        ContentPresenter? secondaryFlyoutContent = null)
    {
        _overlayHost = overlayHost;
        _overlayContent = overlayContent;
        _flyoutHosts.Clear();
        RegisterFlyoutHost(FlyoutHostType.Default, flyoutHost, flyoutContent);
        RegisterFlyoutHost(FlyoutHostType.ContextMenu, flyoutHost, flyoutContent);

        if (secondaryFlyoutHost != null && secondaryFlyoutContent != null)
        {
            RegisterFlyoutHost(FlyoutHostType.Queue, secondaryFlyoutHost, secondaryFlyoutContent);
        }

        // Preload queue overlay
        if (TryGetQueueHostRegistration(out var queueRegistration)
            && overlayHost.BindingContext is not null)
        {
            if (SettingsService.IsMobile())
            {
                BindMobileQueueOverlayContext(overlayHost.BindingContext);
                MountMobileQueueOverlay(queueRegistration);
            }
            else
            {
                BindDesktopQueueOverlayContext(overlayHost.BindingContext);
                MountDesktopQueueOverlay(queueRegistration);
            }

            MoveQueueHostToBackgroundState(queueRegistration);
        }

        // Preload player bar overlay
        if (_flyoutHosts.TryGetValue(FlyoutHostType.PlayerBar, out var playerBarRegistration)
            && overlayHost.BindingContext is not null
            && SettingsService.IsMobile())
        {
            BindMobilePlayerBarOverlayContext(overlayHost.BindingContext);
            MountMobilePlayerBarOverlay(playerBarRegistration);
            MovePlayerBarHostToBackgroundState(playerBarRegistration);
        }

        _selectionIndicatorHost = selectionIndicatorHost;
        _selectionIndicatorContent = selectionIndicatorContent;
    }

    public void RegisterFlyoutHost(FlyoutHostType flyoutHostType, Grid flyoutHost, ContentPresenter flyoutContent)
    {
        _flyoutHosts[flyoutHostType] = new FlyoutHostRegistration
        {
            Host = flyoutHost,
            Content = flyoutContent
        };
    }

    public void OnBackdropTapped()
    {
        _overlayCloseAction?.Invoke();
    }

    public void OnDefaultFlyoutBackdropTapped()
    {
        InvokeFlyoutBackdrop(FlyoutHostType.Default);
    }

    public void OnContextMenuFlyoutBackdropTapped()
    {
        InvokeFlyoutBackdrop(FlyoutHostType.ContextMenu);
    }

    public void OnPlayerBarFlyoutBackdropTapped()
    {
        InvokeFlyoutBackdrop(FlyoutHostType.PlayerBar);
    }

    public void OnQueueFlyoutBackdropTapped()
    {
        InvokeFlyoutBackdrop(FlyoutHostType.Queue);
    }

    #endregion

    #region Context Menu API

    public Task ShowContextMenuFlyoutAsync(View menuView, Action? onClose)
    {
        var hostType = SettingsService.IsMobile() ? FlyoutHostType.ContextMenu : FlyoutHostType.Default;
        return ShowFlyoutLayerAsync(menuView, onClose, FlyoutLayoutMode.Bottom, hostType);
    }

    public Task HideContextMenuFlyoutAsync()
    {
        var hostType = SettingsService.IsMobile() ? FlyoutHostType.ContextMenu : FlyoutHostType.Default;
        return HideFlyoutLayerAsync(hostType);
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

    #region Overlay State

    public bool IsQueueOverlayOpen => SettingsService.IsMobile() ? _mobileQueueOverlay.IsOpen : _desktopQueueOverlay.IsOpen;

    public bool IsQueueOverlayAnimating => SettingsService.IsMobile() ? _mobileQueueOverlay.IsAnimating : _desktopQueueOverlay.IsAnimating;

    public bool IsQueueOverlayInteractiveOpening => SettingsService.IsMobile() && _isQueueInteractiveOpening;

    public bool IsPlayerBarOverlayOpen => _playerBarOverlay.IsOpen;

    public bool IsPlayerBarOverlayInteractiveOpening => SettingsService.IsMobile() && _isPlayerBarInteractiveOpening;

    public bool IsOverlayOpen => _overlayHost?.IsVisible == true;

    public bool IsFlyoutOpen => _flyoutHosts.Any(entry =>
    {
        if (entry.Key == FlyoutHostType.Queue)
        {
            return IsQueueHostForeground(entry.Value);
        }

        if (entry.Key == FlyoutHostType.PlayerBar)
        {
            return IsPlayerBarHostForeground(entry.Value);
        }

        return entry.Value.Host.IsVisible;
    });

    #endregion

    #region Player Bar Overlay

    public Task ShowPlayerBarOverlayAsync(object bindingContext)
    {
        if (!SettingsService.IsMobile() || bindingContext is null)
        {
            return Task.CompletedTask;
        }

        return MainThread.InvokeOnMainThreadAsync(async () =>
        {
            EnsureInitialized();

            if (!_flyoutHosts.TryGetValue(FlyoutHostType.PlayerBar, out var registration))
            {
                return;
            }

            BindMobilePlayerBarOverlayContext(bindingContext);
            MountMobilePlayerBarOverlay(registration);

            _isPlayerBarInteractiveOpening = false;
            MovePlayerBarHostToForegroundState(registration);
            registration.CloseAction = () => _ = HidePlayerBarOverlayAsync();

            await _playerBarOverlay.ShowAsync(PlayerBarOverlayForegroundZIndex);
        });
    }

    public Task HidePlayerBarOverlayAsync()
    {
        if (!SettingsService.IsMobile())
        {
            return Task.CompletedTask;
        }

        return MainThread.InvokeOnMainThreadAsync(async () =>
        {
            _isPlayerBarInteractiveOpening = false;

            if (!_flyoutHosts.TryGetValue(FlyoutHostType.PlayerBar, out var registration))
            {
                return;
            }

            await _playerBarOverlay.HideAsync(PlayerBarOverlayBackgroundZIndex);

            MovePlayerBarHostToBackgroundState(registration);

            ClearFlyoutCloseAction(FlyoutHostType.PlayerBar);
        });
    }

    public void BeginPlayerBarOverlayInteractiveOpen(object bindingContext)
    {
        if (!SettingsService.IsMobile() || bindingContext is null)
        {
            return;
        }

        MainThread.BeginInvokeOnMainThread(() =>
        {
            EnsureInitialized();

            if (!_flyoutHosts.TryGetValue(FlyoutHostType.PlayerBar, out var registration))
            {
                return;
            }

            if (_playerBarOverlay.IsOpen || _playerBarOverlay.IsAnimating)
            {
                return;
            }

            BindMobilePlayerBarOverlayContext(bindingContext);
            MountMobilePlayerBarOverlay(registration);

            MovePlayerBarHostToForegroundState(registration);
            registration.CloseAction = () => _ = HidePlayerBarOverlayAsync();

            _playerBarOverlay.BeginInteractiveOpen(PlayerBarOverlayForegroundZIndex);
            _isPlayerBarInteractiveOpening = true;
        });
    }

    public void UpdatePlayerBarOverlayInteractiveOpen(double upwardPullDistance)
    {
        if (!SettingsService.IsMobile())
        {
            return;
        }

        MainThread.BeginInvokeOnMainThread(() =>
        {
            if (!_isPlayerBarInteractiveOpening)
            {
                return;
            }

            _playerBarOverlay.UpdateInteractiveOpen(upwardPullDistance);
        });
    }

    public Task EndPlayerBarOverlayInteractiveOpenAsync(bool openPlayerBar)
    {
        if (!SettingsService.IsMobile())
        {
            return Task.CompletedTask;
        }

        return MainThread.InvokeOnMainThreadAsync(async () =>
        {
            if (!_isPlayerBarInteractiveOpening)
            {
                return;
            }

            try
            {
                if (openPlayerBar)
                {
                    await _playerBarOverlay.CompleteInteractiveOpenAsync();
                }
                else
                {
                    await _playerBarOverlay.CancelInteractiveOpenAsync(PlayerBarOverlayBackgroundZIndex);

                    if (_flyoutHosts.TryGetValue(FlyoutHostType.PlayerBar, out var registration))
                    {
                        MovePlayerBarHostToBackgroundState(registration);
                    }

                    ClearFlyoutCloseAction(FlyoutHostType.PlayerBar);
                }
            }
            finally
            {
                _isPlayerBarInteractiveOpening = false;
            }
        });
    }

    #endregion

    #region Queue Overlay

    public Task ShowQueueOverlayAsync(object bindingContext)
    {
        if (bindingContext is null)
        {
            return Task.CompletedTask;
        }

        return MainThread.InvokeOnMainThreadAsync(async () =>
        {
            EnsureInitialized();

            if (!TryGetQueueHostRegistration(out var queueRegistration))
            {
                return;
            }

            if (SettingsService.IsMobile())
            {
                BindMobileQueueOverlayContext(bindingContext);
                MountMobileQueueOverlay(queueRegistration);
                MoveQueueHostToForegroundState(queueRegistration);
                queueRegistration.CloseAction = () => _ = HideQueueOverlayAsync();

                _isQueueInteractiveOpening = false;
                await _mobileQueueOverlay.ShowAsync(QueueOverlayForegroundZIndex);
                return;
            }

            var hostType = ResolveDesktopQueueHostType();

            BindDesktopQueueOverlayContext(bindingContext);
            MountDesktopQueueOverlay(queueRegistration);
            MoveQueueHostToForegroundState(queueRegistration);
            queueRegistration.CloseAction = () => _ = HideQueueOverlayAsync();

            await _desktopQueueOverlay.ShowAsync(QueueOverlayForegroundZIndex);
        });
    }

    public Task HideQueueOverlayAsync()
    {
        return MainThread.InvokeOnMainThreadAsync(async () =>
        {
            if (!TryGetQueueHostRegistration(out var queueRegistration))
            {
                return;
            }

            if (SettingsService.IsMobile())
            {
                _isQueueInteractiveOpening = false;

                await _mobileQueueOverlay.HideAsync(QueueOverlayBackgroundZIndex);

                MoveQueueHostToBackgroundState(queueRegistration);
                ClearFlyoutCloseAction(FlyoutHostType.Queue);
                return;
            }

            var hostType = ResolveDesktopQueueHostType();

            await _desktopQueueOverlay.HideAsync(QueueOverlayBackgroundZIndex);

            MoveQueueHostToBackgroundState(queueRegistration);

            queueRegistration.Content.VerticalOptions = LayoutOptions.End;
            ClearFlyoutCloseAction(hostType);
        });
    }

    public void BeginQueueOverlayInteractiveOpen(object bindingContext)
    {
        if (!SettingsService.IsMobile() || bindingContext is null)
        {
            return;
        }

        MainThread.BeginInvokeOnMainThread(() =>
        {
            EnsureInitialized();

            if (!TryGetQueueHostRegistration(out var queueRegistration))
            {
                return;
            }

            if (_mobileQueueOverlay.IsOpen || _mobileQueueOverlay.IsAnimating)
            {
                return;
            }

            BindMobileQueueOverlayContext(bindingContext);
            MountMobileQueueOverlay(queueRegistration);
            MoveQueueHostToForegroundState(queueRegistration);
            queueRegistration.CloseAction = () => _ = HideQueueOverlayAsync();

            _mobileQueueOverlay.BeginInteractiveOpen(QueueOverlayForegroundZIndex);
            _isQueueInteractiveOpening = true;
        });
    }

    public void UpdateQueueOverlayInteractiveOpen(double upwardPullDistance)
    {
        if (!SettingsService.IsMobile())
        {
            return;
        }

        MainThread.BeginInvokeOnMainThread(() =>
        {
            if (!_isQueueInteractiveOpening)
            {
                return;
            }

            _mobileQueueOverlay.UpdateInteractiveOpen(upwardPullDistance);
        });
    }

    public Task EndQueueOverlayInteractiveOpenAsync(bool openQueue)
    {
        if (!SettingsService.IsMobile())
        {
            return Task.CompletedTask;
        }

        return MainThread.InvokeOnMainThreadAsync(async () =>
        {
            if (!_isQueueInteractiveOpening)
            {
                return;
            }

            try
            {
                if (openQueue)
                {
                    await _mobileQueueOverlay.CompleteInteractiveOpenAsync();
                }
                else
                {
                    await _mobileQueueOverlay.CancelInteractiveOpenAsync(QueueOverlayBackgroundZIndex);

                    if (TryGetQueueHostRegistration(out var queueRegistration))
                    {
                        MoveQueueHostToBackgroundState(queueRegistration);
                    }

                    ClearFlyoutCloseAction(FlyoutHostType.Queue);
                }
            }
            finally
            {
                _isQueueInteractiveOpening = false;
            }
        });
    }

    

    #endregion

    #region General Overlay Visibility And Selection

    public Task CloseOverlayAsync()
    {
        return MainThread.InvokeOnMainThreadAsync(async () =>
        {
            if (!IsOverlayOpen)
            {
                return;
            }

            if (_overlayCloseAction != null)
            {
                _overlayCloseAction.Invoke();
                return;
            }

            await HideCenteredOverlayInternalAsync();
        });
    }

    public Task CloseFlyoutAsync()
    {
        return MainThread.InvokeOnMainThreadAsync(async () =>
        {
            if (!IsFlyoutOpen)
            {
                return;
            }

            if (TryInvokeFlyoutCloseAction(FlyoutHostType.ContextMenu))
            {
                return;
            }

            if (IsFlyoutVisible(FlyoutHostType.ContextMenu))
            {
                await HideFlyoutLayerAsync(FlyoutHostType.ContextMenu);
                return;
            }

            if (IsPlayerBarOverlayOpen)
            {
                await HidePlayerBarOverlayAsync();
                return;
            }

            if (IsQueueOverlayOpen)
            {
                await HideQueueOverlayAsync();
                return;
            }

            if (TryInvokeFlyoutCloseAction(FlyoutHostType.PlayerBar))
            {
                return;
            }

            if (TryInvokeFlyoutCloseAction(FlyoutHostType.Queue))
            {
                return;
            }

            if (TryInvokeFlyoutCloseAction(FlyoutHostType.Default))
            {
                return;
            }

            if (IsFlyoutVisible(FlyoutHostType.PlayerBar))
            {
                await HideFlyoutLayerAsync(FlyoutHostType.PlayerBar);
                return;
            }

            if (IsFlyoutVisible(FlyoutHostType.Queue))
            {
                await HideFlyoutLayerAsync(FlyoutHostType.Queue);
                return;
            }

            if (IsFlyoutVisible(FlyoutHostType.Default))
            {
                await HideFlyoutLayerAsync(FlyoutHostType.Default);
            }
        });
    }

    public Task ShowSelectionIndicatorAsync(object selectionControl)
    {
        if (!IsSupportedSelectionControl(selectionControl))
        {
            return Task.CompletedTask;
        }

        return MainThread.InvokeOnMainThreadAsync(() =>
        {
            if (_selectionIndicatorHost == null || _selectionIndicatorContent == null)
            {
                return;
            }

            _selectionIndicatorControl = selectionControl;
            _selectionIndicatorContent.Content = _selectionIndicatorOverlay;
            _selectionIndicatorHost.IsVisible = true;
        });
    }

    public Task HideSelectionIndicatorAsync(object? selectionControl = null)
    {
        if (selectionControl != null && !IsSupportedSelectionControl(selectionControl))
        {
            return Task.CompletedTask;
        }

        return MainThread.InvokeOnMainThreadAsync(() =>
        {
            if (_selectionIndicatorHost == null || _selectionIndicatorContent == null)
            {
                return;
            }

            if (selectionControl != null && !ReferenceEquals(selectionControl, _selectionIndicatorControl))
            {
                return;
            }

            _selectionIndicatorHost.IsVisible = false;
            _selectionIndicatorContent.Content = null;
            _selectionIndicatorControl = null;
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

    #region Sort Playlist Content Overlay API

    public async Task<(string SortField, bool IsDescending)?> ShowSortContentOverlayAsync()
    {
        await _overlayLock.WaitAsync();

        try
        {
            EnsureInitialized();

            _sortContentOverlayTcs = new TaskCompletionSource<(string SortField, bool IsDescending)?>(TaskCreationOptions.RunContinuationsAsynchronously);

            await MainThread.InvokeOnMainThreadAsync(() =>
            {
                _sortContentOverlay.ResetSelection();
                ShowCenteredOverlayInternal(_sortContentOverlay, () => _sortContentOverlayTcs.TrySetResult(null));
            });

            return await _sortContentOverlayTcs.Task;
        }
        finally
        {
            _sortContentOverlayTcs = null;
            await HideCenteredOverlayInternalAsync();
            _overlayLock.Release();
        }
    }

    #endregion

    #region Login Overlay API

    public async Task<(string Username, string Password, string ServerUri)> ShowLoginAsync(
        string? initialUsername,
        string? initialServerUri,
        string? initialErrorMessage = null)
    {
        await _overlayLock.WaitAsync();

        try
        {
            EnsureInitialized();

            _loginTcs = new TaskCompletionSource<(string Username, string Password, string ServerUri)>(TaskCreationOptions.RunContinuationsAsynchronously);

            await MainThread.InvokeOnMainThreadAsync(() =>
            {
                var isLoginOverlayAlreadyOpen = _overlayHost?.IsVisible == true
                    && ReferenceEquals(_overlayContent?.Content, _loginOverlay);

                if (!isLoginOverlayAlreadyOpen)
                {
                    _loginOverlay.Username = initialUsername ?? string.Empty;
                    _loginOverlay.ServerUri = initialServerUri ?? string.Empty;
                    _loginOverlay.Password = string.Empty;
                    _loginOverlay.HideError();
                    _loginOverlay.SetStatusMessage(string.Empty);
                    _loginOverlay.SetLoadingState(false);

                    if (!string.IsNullOrWhiteSpace(initialErrorMessage))
                    {
                        _loginOverlay.ShowError(initialErrorMessage);
                    }

                    ShowCenteredOverlayInternal(_loginOverlay, null);

                    if (string.IsNullOrWhiteSpace(_loginOverlay.Username))
                    {
                        _loginOverlay.FocusUsername();
                    }
                    else
                    {
                        _loginOverlay.FocusPassword();
                    }
                }
                else
                {
                    _loginOverlay.SetLoadingState(false);
                    _loginOverlay.FocusPassword();
                }
            });

            return await _loginTcs.Task;
        }
        finally
        {
            _loginTcs = null;
            _overlayLock.Release();
        }
    }

    public Task SetLoginLoadingStateAsync(bool isLoading)
    {
        return MainThread.InvokeOnMainThreadAsync(() =>
        {
            _loginOverlay.SetLoadingState(isLoading);
        });
    }

    public Task ShowLoginErrorAsync(string message)
    {
        return MainThread.InvokeOnMainThreadAsync(() =>
        {
            _loginOverlay.SetLoadingState(false);
            _loginOverlay.ShowError(message);
            _loginOverlay.FocusPassword();
        });
    }

    public Task HideLoginErrorAsync()
    {
        return MainThread.InvokeOnMainThreadAsync(() =>
        {
            _loginOverlay.HideError();
        });
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

    #endregion

    #region Sort Playlist Content Overlay Event Handlers

    private void OnSortContentOverlayCancelled(object? sender, EventArgs e)
    {
        _sortContentOverlayTcs?.TrySetResult(null);
    }

    private void OnSortContentOverlayConfirmed(object? sender, EventArgs e)
    {
        _sortContentOverlay.IsSortEnabled = false;

        var result = (
            SortField: _sortContentOverlay.SelectedSortField,
            IsDescending: _sortContentOverlay.IsSortDescending);

        _sortContentOverlayTcs?.TrySetResult(result);
    }

    #endregion

    #region Login Overlay Event Handlers

    private void OnLoginUsernameCompleted(object? sender, EventArgs e)
    {
        _loginOverlay.FocusPassword();
    }

    private async void OnLoginPasswordCompleted(object? sender, EventArgs e)
    {
        await TrySubmitLoginAsync();
    }

    private async void OnLoginClicked(object? sender, EventArgs e)
    {
        await TrySubmitLoginAsync();
    }

    private void OnSelectionIndicatorSelectAllTapped(object? sender, EventArgs e)
    {
        if (_selectionIndicatorControl is mashin.Views.Mobile.Controls.TableView tableView)
        {
            tableView.SelectAllItems();
            return;
        }

        if (_selectionIndicatorControl is mashin.Views.Mobile.Controls.RowView rowView)
        {
            rowView.SelectAllItems();
            return;
        }

        if (_selectionIndicatorControl is mashin.Views.Mobile.Controls.SlideView slideView)
        {
            slideView.SelectAllItems();
        }
    }

    private void OnSelectionIndicatorMenuTapped(object? sender, EventArgs e)
    {
        if (_selectionIndicatorControl is mashin.Views.Mobile.Controls.TableView tableView)
        {
            tableView.OpenContextMenuForSelection(_selectionIndicatorOverlay.MenuAnchor);
            return;
        }

        if (_selectionIndicatorControl is mashin.Views.Mobile.Controls.RowView rowView)
        {
            rowView.OpenContextMenuForSelection(_selectionIndicatorOverlay.MenuAnchor);
            return;
        }

        if (_selectionIndicatorControl is mashin.Views.Mobile.Controls.SlideView slideView)
        {
            slideView.OpenContextMenuForSelection(_selectionIndicatorOverlay.MenuAnchor);
        }
    }

    private void OnSelectionIndicatorCloseTapped(object? sender, EventArgs e)
    {
        if (_selectionIndicatorControl is mashin.Views.Mobile.Controls.TableView tableView)
        {
            tableView.ClearSelection();
            return;
        }

        if (_selectionIndicatorControl is mashin.Views.Mobile.Controls.RowView rowView)
        {
            rowView.ClearSelection();
            return;
        }

        if (_selectionIndicatorControl is mashin.Views.Mobile.Controls.SlideView slideView)
        {
            slideView.ClearSelection();
        }
    }

    #endregion

    #region Login Flow

    private async Task TrySubmitLoginAsync()
    {
        if (_loginTcs is null)
        {
            return;
        }

        var username = _loginOverlay.Username.Trim();
        var serverUri = _loginOverlay.ServerUri.Trim();
        var password = _loginOverlay.Password;

        if (string.IsNullOrWhiteSpace(serverUri))
        {
            _loginOverlay.ShowError("Bitte geben Sie eine Server-URI ein.");
            return;
        }

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
        await MainThread.InvokeOnMainThreadAsync(() =>
        {
            _loginTcs.TrySetResult((username, password, serverUri));
        });
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

    private Task ShowFlyoutLayerAsync(View flyout, Action? onClose, FlyoutLayoutMode layoutMode, FlyoutHostType hostType)
    {
        return MainThread.InvokeOnMainThreadAsync(() =>
        {
            EnsureInitialized();
            ShowFlyoutInternal(flyout, onClose, layoutMode, hostType);
        });
    }

    private Task HideFlyoutLayerAsync(FlyoutHostType hostType, bool clearContent = true)
    {
        return MainThread.InvokeOnMainThreadAsync(() =>
        {
            if (!_flyoutHosts.TryGetValue(hostType, out var registration))
            {
                return;
            }

            registration.Host.IsVisible = false;

            if (clearContent)
            {
                registration.Content.Content = null;
            }

            registration.Content.VerticalOptions = LayoutOptions.End;

            registration.CloseAction = null;
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

    private void ShowFlyoutInternal(View flyout, Action? onClose, FlyoutLayoutMode layoutMode, FlyoutHostType hostType)
    {
        if (!_flyoutHosts.TryGetValue(hostType, out var registration))
        {
            return;
        }

        var useFullHeight = layoutMode == FlyoutLayoutMode.FullHeight;
        registration.Content.VerticalOptions = useFullHeight ? LayoutOptions.Fill : LayoutOptions.End;
        flyout.VerticalOptions = useFullHeight ? LayoutOptions.Fill : LayoutOptions.End;

        if (useFullHeight)
        {
            flyout.HeightRequest = -1;
        }

        registration.Content.Content = flyout;
        registration.Host.IsVisible = true;
        registration.CloseAction = onClose;
    }

    private bool TryInvokeFlyoutCloseAction(FlyoutHostType hostType)
    {
        if (!_flyoutHosts.TryGetValue(hostType, out var registration) || registration.CloseAction == null)
        {
            return false;
        }

        registration.CloseAction.Invoke();
        return true;
    }

    private void InvokeFlyoutBackdrop(FlyoutHostType hostType)
    {
        if (_flyoutHosts.TryGetValue(hostType, out var registration))
        {
            registration.CloseAction?.Invoke();
        }
    }

    private bool IsFlyoutVisible(FlyoutHostType hostType)
    {
        if (!_flyoutHosts.TryGetValue(hostType, out var registration))
        {
            return false;
        }

        if (hostType == FlyoutHostType.Queue)
        {
            return IsQueueHostForeground(registration);
        }

        if (hostType == FlyoutHostType.PlayerBar)
        {
            return IsPlayerBarHostForeground(registration);
        }

        return registration.Host.IsVisible;
    }

    private void ClearFlyoutCloseAction(FlyoutHostType hostType)
    {
        if (_flyoutHosts.TryGetValue(hostType, out var registration))
        {
            registration.CloseAction = null;
        }
    }

    private void MoveQueueHostToForegroundState(FlyoutHostRegistration registration)
    {
        registration.Host.IsVisible = true;
        registration.Host.Opacity = 1;
        registration.Host.InputTransparent = false;
        registration.Host.ZIndex = QueueOverlayForegroundZIndex;
    }

    private void MoveQueueHostToBackgroundState(FlyoutHostRegistration registration)
    {
        registration.Host.IsVisible = true;
        registration.Host.Opacity = 1;
        registration.Host.InputTransparent = true;
        registration.Host.ZIndex = QueueOverlayBackgroundZIndex;
    }

    private void MovePlayerBarHostToForegroundState(FlyoutHostRegistration registration)
    {
        registration.Host.IsVisible = true;
        registration.Host.Opacity = 1;
        registration.Host.InputTransparent = false;
        registration.Host.ZIndex = PlayerBarOverlayForegroundZIndex;
    }

    private void MovePlayerBarHostToBackgroundState(FlyoutHostRegistration registration)
    {
        registration.Host.IsVisible = true;
        registration.Host.Opacity = 1;
        registration.Host.InputTransparent = true;
        registration.Host.ZIndex = PlayerBarOverlayBackgroundZIndex;
    }

    private void BindMobileQueueOverlayContext(object bindingContext)
    {
        if (!ReferenceEquals(_mobileQueueOverlay.BindingContext, bindingContext))
        {
            _mobileQueueOverlay.BindingContext = bindingContext;
        }
    }

    private void BindDesktopQueueOverlayContext(object bindingContext)
    {
        if (!ReferenceEquals(_desktopQueueOverlay.BindingContext, bindingContext))
        {
            _desktopQueueOverlay.BindingContext = bindingContext;
        }
    }

    private void BindMobilePlayerBarOverlayContext(object bindingContext)
    {
        if (!ReferenceEquals(_playerBarOverlay.BindingContext, bindingContext))
        {
            _playerBarOverlay.BindingContext = bindingContext;
        }
    }

    private void MountMobileQueueOverlay(FlyoutHostRegistration queueRegistration)
    {
        queueRegistration.Content.VerticalOptions = LayoutOptions.Fill;
        _mobileQueueOverlay.VerticalOptions = LayoutOptions.Fill;
        _mobileQueueOverlay.HeightRequest = -1;

        if (!ReferenceEquals(queueRegistration.Content.Content, _mobileQueueOverlay))
        {
            queueRegistration.Content.Content = _mobileQueueOverlay;
        }
    }

    private void MountDesktopQueueOverlay(FlyoutHostRegistration registration)
    {
        registration.Content.VerticalOptions = LayoutOptions.Fill;
        _desktopQueueOverlay.VerticalOptions = LayoutOptions.Fill;
        _desktopQueueOverlay.HeightRequest = -1;

        if (!ReferenceEquals(registration.Content.Content, _desktopQueueOverlay))
        {
            registration.Content.Content = _desktopQueueOverlay;
        }
    }


    private static bool IsQueueHostForeground(FlyoutHostRegistration registration)
    {
        return registration.Host.IsVisible
            && !registration.Host.InputTransparent
            && registration.Host.ZIndex >= QueueOverlayForegroundZIndex;
    }

    private static bool IsPlayerBarHostForeground(FlyoutHostRegistration registration)
    {
        return registration.Host.IsVisible
            && !registration.Host.InputTransparent
            && registration.Host.ZIndex >= PlayerBarOverlayForegroundZIndex;
    }

    private void MountMobilePlayerBarOverlay(FlyoutHostRegistration registration)
    {
        registration.Content.VerticalOptions = LayoutOptions.Fill;
        _playerBarOverlay.VerticalOptions = LayoutOptions.Fill;
        _playerBarOverlay.HeightRequest = -1;

        if (!ReferenceEquals(registration.Content.Content, _playerBarOverlay))
        {
            registration.Content.Content = _playerBarOverlay;
        }
    }

    private bool TryGetQueueHostRegistration(out FlyoutHostRegistration queueRegistration)
    {
        return _flyoutHosts.TryGetValue(FlyoutHostType.Queue, out queueRegistration!);
    }

    private FlyoutHostType ResolveDesktopQueueHostType()
    {
        return _flyoutHosts.ContainsKey(FlyoutHostType.Queue)
            ? FlyoutHostType.Queue
            : FlyoutHostType.Default;
    }

    private void EnsureInitialized()
    {
        if (_overlayHost != null
            && _overlayContent != null
            && _flyoutHosts.ContainsKey(FlyoutHostType.Default))
        {
            return;
        }

        throw new InvalidOperationException("OverlayService is not initialized. Call Initialize from MainPage first.");
    }

    private static bool IsSupportedSelectionControl(object selectionControl)
    {
        return selectionControl is mashin.Views.Mobile.Controls.TableView
            or mashin.Views.Mobile.Controls.RowView
            or mashin.Views.Mobile.Controls.SlideView;
    }

    #endregion
}