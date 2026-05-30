using mashin.Services;
using mashin.ViewModels;
using Microsoft.Extensions.Logging;

namespace mashin.Views.Mobile;

public partial class MainPage : ContentPage
{
    private readonly MainViewModel _viewModel;
    private readonly INavigationService _navigationService;
    private readonly IOverlayService _overlayService;
    private bool _isHandlingBackNavigation;

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
        BindingContext = _viewModel;

        _overlayService.Initialize(OverlayHost, OverlayContent, FlyoutHost, FlyoutContent);

        if (_navigationService is NavigationService navService)
        {
            navService.Initialize(ContentContainer);
        }

        _ = _navigationService.NavigateToAsync<HomePage>();
        _ = _viewModel.InitializeAsync();
        _navigationService.IsNavigating = false;
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