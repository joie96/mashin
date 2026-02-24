using mashin.Services;
using mashin.ViewModels;
using mashin.Views.Desktop.Controls;
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
        _viewModel.CloseQueueViewRequested += OnCloseQueueViewRequestedAsync;

        _overlayService.Initialize(OverlayHost, OverlayContent);

        // Initialize navigation service with content container
        if (_navigationService is NavigationService navService)
        {
            navService.Initialize(ContentContainer);
        }

        // Navigate to home page
        _ = _navigationService.NavigateToAsync<HomePage>();

        _ = _viewModel.InitializeAsync();

        UpdateQueueIconColor();
    }

    #endregion

    #region Navigation

    private void OnBackTapped(object? sender, TappedEventArgs e)
    {
        _ = _navigationService.GoBackAsync();
    }

    private void OnNavigatePlaylistsTapped(object? sender, TappedEventArgs e)
    {
        _logger.LogInformation("Navigate to playlists page not implemented yet.");
    }

    #endregion

    #region Queue Overlay

    private async Task OnCloseQueueViewRequestedAsync()
    {
        if (QueueOverlay.IsAnimating)
        {
            return;
        }

        if (QueueOverlay.IsOpen)
        {
            await QueueOverlay.HideAsync();
            UpdateQueueIconColor();
        }
    }

    private async void OnQueueTapped(object? sender, TappedEventArgs e)
    {
        if (QueueOverlay.IsAnimating)
        {
            return;
        }

        if (QueueOverlay.IsOpen)
        {
            await QueueOverlay.HideAsync();
            UpdateQueueIconColor();
            return;
        }

        var queueIconLabel = this.FindByName<Label>("QueueIconLabel");
        queueIconLabel?.SetDynamicResource(Label.TextColorProperty, "AccentColor");
        await QueueOverlay.ShowAsync();
        UpdateQueueIconColor();
    }

    private void UpdateQueueIconColor()
    {
        var queueIconLabel = this.FindByName<Label>("QueueIconLabel");
        queueIconLabel?.SetDynamicResource(
            Label.TextColorProperty,
            QueueOverlay.IsOpen ? "AccentColor" : "IconSecondary");
    }

    #endregion

    #region Overlay Host

    private void OnOverlayBackdropTapped(object? sender, TappedEventArgs e)
    {
        _overlayService.OnBackdropTapped();
    }

    #endregion

}
