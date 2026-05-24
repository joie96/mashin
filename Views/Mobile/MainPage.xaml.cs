using mashin.Services;
using mashin.ViewModels;
using Microsoft.Extensions.Logging;

namespace mashin.Views.Mobile;

public partial class MainPage : ContentPage
{
    private readonly MainViewModel _viewModel;
    private readonly INavigationService _navigationService;
    private readonly IOverlayService _overlayService;

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
}