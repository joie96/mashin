using mashin.Services;
using mashin.ViewModels;
using Microsoft.Extensions.Logging;

namespace mashin.Views.Desktop;

public partial class MainPage : ContentPage
{
    #region Fields

    private readonly MainViewModel _viewModel;
    private readonly INavigationService _navigationService;
    private readonly IOverlayService _overlayService;
    private readonly ILogger<MainPage> _logger;

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

        _overlayService.Initialize(OverlayHost, OverlayContent);

        // Initialize navigation service with content container
        if (_navigationService is NavigationService navService)
        {
            navService.Initialize(ContentContainer);
        }

        // Navigate to home page
        _ = _navigationService.NavigateToAsync<HomePage>();

        _ = _viewModel.InitializeAsync();
    }

    #endregion

    #region Navigation

    private async void OnBackTapped(object? sender, TappedEventArgs e)
    {
        await _navigationService.GoBackAsync();
    }

    private void OnNavigatePlaylistsTapped(object? sender, TappedEventArgs e)
    {
        _logger.LogInformation("Navigate to playlists page not implemented yet.");
    }

    #endregion

    #region Overlay Host

    private void OnOverlayBackdropTapped(object? sender, TappedEventArgs e)
    {
        _overlayService.OnBackdropTapped();
    }

    #endregion
}
