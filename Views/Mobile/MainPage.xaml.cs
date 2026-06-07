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

    private async void OnPlayerBarTapped(object? sender, TappedEventArgs e)
    {
        if (_viewModel.CurrentTrack is null)
        {
            return;
        }

        await _overlayService.ShowPlayerBarOverlayAsync(_viewModel);
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