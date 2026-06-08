using mashin.Services;
using mashin.ViewModels;
using Microsoft.Extensions.Logging;

namespace mashin.Views.Mobile;

public partial class MainPage : ContentPage
{
    #region Constants

    private const double PlayerBarOpenThreshold = 70d;
    private const double PlayerBarOpenReleaseThreshold = 130d;
    private const double PlayerBarDragResistance = 0.92d;

    #endregion

    #region Fields

    private readonly MainViewModel _viewModel;
    private readonly INavigationService _navigationService;
    private readonly IOverlayService _overlayService;
    private readonly ILogger<MainPage> _logger;
    private bool _isHandlingBackNavigation;
    private Task? _initializeTask;
    private bool _playerBarDragStarted;
    private bool _suppressNextPlayerBarTap;
    private double _playerBarDragDistance;

    #endregion

    #region Construction

    public MainPage(
        MainViewModel viewModel,
        INavigationService navigationService,
        IOverlayService overlayService,
        ILogger<MainPage> logger)
    {
        InitializeComponent();

        _viewModel = viewModel;
        _navigationService = navigationService;
        _overlayService = overlayService;
        _logger = logger;
        BindingContext = _viewModel;

        var selectionIndicatorHost = this.FindByName<Grid>("SelectionIndicatorHost");
        var selectionIndicatorContent = this.FindByName<ContentPresenter>("SelectionIndicatorContent");

        _overlayService.Initialize(
            OverlayHost,
            OverlayContent,
            ContextMenuFlyoutHost,
            ContextMenuFlyoutContent,
            selectionIndicatorHost,
            selectionIndicatorContent,
            QueueFlyoutHost,
            QueueFlyoutContent);

        _overlayService.RegisterFlyoutHost(
            FlyoutHostType.PlayerBar,
            PlayerBarFlyoutHost,
            PlayerBarFlyoutContent);

        if (_navigationService is NavigationService navService)
        {
            navService.Initialize(ContentContainer);
        }

        Loaded += OnLoaded;
        _navigationService.IsNavigating = false;
    }

    #endregion

    #region Lifecycle

    private void OnLoaded(object? sender, EventArgs e)
    {
        _initializeTask ??= InitializeViewModelAsync();
    }

    private async Task InitializeViewModelAsync()
    {
        try
        {
            await _viewModel.InitializeAsync();
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to initialize main view model");
        }
    }

    #endregion

    #region Overlay And Flyout Backdrops

    private void OnOverlayBackdropTapped(object? sender, TappedEventArgs e)
    {
        _overlayService.OnBackdropTapped();
    }

    private void OnContextMenuFlyoutBackdropTapped(object? sender, TappedEventArgs e)
    {
        _overlayService.OnContextMenuFlyoutBackdropTapped();
    }

    private void OnPlayerBarFlyoutBackdropTapped(object? sender, TappedEventArgs e)
    {
        _overlayService.OnPlayerBarFlyoutBackdropTapped();
    }

    private void OnQueueFlyoutBackdropTapped(object? sender, TappedEventArgs e)
    {
        _overlayService.OnQueueFlyoutBackdropTapped();
    }

    #endregion

    #region Player Bar Gestures

    private async void OnPlayerBarTapped(object? sender, TappedEventArgs e)
    {
        if (_suppressNextPlayerBarTap)
        {
            _suppressNextPlayerBarTap = false;
            return;
        }

        if (_viewModel.CurrentTrack is null)
        {
            return;
        }

        await _overlayService.ShowPlayerBarOverlayAsync(_viewModel);
    }

    private void OnPlayerBarPanUpdated(object? sender, PanUpdatedEventArgs e)
    {
        if (_viewModel.CurrentTrack is null)
        {
            return;
        }

        switch (e.StatusType)
        {
            case GestureStatus.Started:
                _playerBarDragStarted = false;
                _playerBarDragDistance = 0;
                break;

            case GestureStatus.Running:
                if (Math.Abs(e.TotalX) > Math.Abs(e.TotalY))
                {
                    return;
                }

                if (e.TotalY >= 0)
                {
                    return;
                }

                var upwardPullDistance = Math.Abs(e.TotalY) * PlayerBarDragResistance;

                if (!_playerBarDragStarted && upwardPullDistance >= PlayerBarOpenThreshold)
                {
                    _overlayService.BeginPlayerBarOverlayInteractiveOpen(_viewModel);
                    _playerBarDragStarted = true;
                }

                if (_playerBarDragStarted)
                {
                    _playerBarDragDistance = upwardPullDistance;
                    _overlayService.UpdatePlayerBarOverlayInteractiveOpen(_playerBarDragDistance);
                }

                break;

            case GestureStatus.Completed:
            case GestureStatus.Canceled:
                if (_playerBarDragStarted)
                {
                    var shouldOpenOverlay = _playerBarDragDistance >= PlayerBarOpenReleaseThreshold;
                    _ = _overlayService.EndPlayerBarOverlayInteractiveOpenAsync(shouldOpenOverlay);
                    _suppressNextPlayerBarTap = true;
                }

                _playerBarDragStarted = false;
                _playerBarDragDistance = 0;
                break;
        }
    }

    #endregion

    #region Android Back Button Handling
    protected override bool OnBackButtonPressed()
    {
        if (DeviceInfo.Current.Platform != DevicePlatform.Android)
        {
            return base.OnBackButtonPressed();
        }

        if (_isHandlingBackNavigation)
        {
            return true;
        }

        if (_overlayService.IsOverlayOpen)
        {
            _ = HandleAndroidBackAsync(
                shouldCloseOverlay: true,
                shouldCloseSecondaryFlyout: false,
                shouldClosePrimaryFlyout: false,
                shouldNavigateBack: false);
            return true;
        }

        if (_overlayService.IsQueueOverlayOpen)
        {
            _ = HandleAndroidBackAsync(
                shouldCloseOverlay: false,
                shouldCloseSecondaryFlyout: true,
                shouldClosePrimaryFlyout: false,
                shouldNavigateBack: false);
            return true;
        }

        if (_overlayService.IsFlyoutOpen)
        {
            _ = HandleAndroidBackAsync(
                shouldCloseOverlay: false,
                shouldCloseSecondaryFlyout: false,
                shouldClosePrimaryFlyout: true,
                shouldNavigateBack: false);
            return true;
        }

        if (_navigationService.CanGoBack)
        {
            _ = HandleAndroidBackAsync(
                shouldCloseOverlay: false,
                shouldCloseSecondaryFlyout: false,
                shouldClosePrimaryFlyout: false,
                shouldNavigateBack: true);
            return true;
        }

        return base.OnBackButtonPressed();
    }

    private async Task HandleAndroidBackAsync(
        bool shouldCloseOverlay,
        bool shouldCloseSecondaryFlyout,
        bool shouldClosePrimaryFlyout,
        bool shouldNavigateBack)
    {
        if (_isHandlingBackNavigation)
        {
            return;
        }

        _isHandlingBackNavigation = true;
        try
        {
            if (shouldCloseOverlay)
            {
                await _overlayService.CloseOverlayAsync();
                return;
            }

            if (shouldCloseSecondaryFlyout)
            {
                await _overlayService.CloseFlyoutAsync();
                return;
            }

            if (shouldClosePrimaryFlyout)
            {
                await _overlayService.CloseFlyoutAsync();
                return;
            }

            if (shouldNavigateBack)
            {
                await _navigationService.GoBackAsync();
            }
        }
        finally
        {
            _isHandlingBackNavigation = false;
        }
    }
    #endregion
}