using mashin.Services;
using mashin.ViewModels;
using Microsoft.Extensions.Logging;

namespace mashin.Views.Mobile;

public partial class MainPage : ContentPage
{
    private readonly MainViewModel _viewModel;
    private readonly INavigationService _navigationService;
    private readonly IOverlayService _overlayService;
    private readonly ILogger<MainPage> _logger;
    private bool _isHandlingBackNavigation;
    private Task? _initializeTask;

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
            FlyoutHost,
            FlyoutContent,
            selectionIndicatorHost,
            selectionIndicatorContent);

        if (_navigationService is NavigationService navService)
        {
            navService.Initialize(ContentContainer);
        }

        Loaded += OnLoaded;
        _navigationService.IsNavigating = false;
    }

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

    private void OnOverlayBackdropTapped(object? sender, TappedEventArgs e)
    {
        _overlayService.OnBackdropTapped();
    }

    private void OnFlyoutBackdropTapped(object? sender, TappedEventArgs e)
    {
        _overlayService.OnFlyoutBackdropTapped();
    }

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
            _ = HandleAndroidBackAsync(closeOverlay: true, closeFlyout: false, navigateBack: false);
            return true;
        }

        if (_overlayService.IsFlyoutOpen)
        {
            _ = HandleAndroidBackAsync(closeOverlay: false, closeFlyout: true, navigateBack: false);
            return true;
        }

        if (_navigationService.CanGoBack)
        {
            _ = HandleAndroidBackAsync(closeOverlay: false, closeFlyout: false, navigateBack: true);
            return true;
        }

        return base.OnBackButtonPressed();
    }

    private async Task HandleAndroidBackAsync(bool closeOverlay, bool closeFlyout, bool navigateBack)
    {
        if (_isHandlingBackNavigation)
        {
            return;
        }

        _isHandlingBackNavigation = true;
        try
        {
            if (closeOverlay)
            {
                await _overlayService.CloseOverlayAsync();
                return;
            }

            if (closeFlyout)
            {
                await _overlayService.CloseFlyoutAsync();
                return;
            }

            if (navigateBack)
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