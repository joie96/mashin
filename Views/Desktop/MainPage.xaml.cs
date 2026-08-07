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
    private Task? _initializeTask;

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

        _overlayService.Initialize(
            OverlayHost,
            OverlayContent,
            ContextMenuFlyoutHost,
            ContextMenuFlyoutContent,
            secondaryFlyoutHost: QueueFlyoutHost,
            secondaryFlyoutContent: QueueFlyoutContent);

        _overlayService.RegisterFlyoutHost(
            FlyoutHostType.PlayerBar,
            PlayerBarFlyoutHost,
            PlayerBarFlyoutContent);

        // Initialize navigation service with content container
        if (_navigationService is NavigationService navService)
        {
            navService.Initialize(ContentContainer);
        }

        Loaded += OnLoaded;

        _navigationService.IsNavigating = false;

        UpdateQueueIconColor();
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

    private void OnSidebarPlaylistRowPointerEntered(object? sender, PointerEventArgs e)
    {
        if (sender is not Border row)
        {
            return;
        }

        var menuButton = row.FindByName<Border>("SidebarPlaylistMenuButton");
        if (menuButton != null)
        {
            menuButton.IsVisible = true;
        }
    }

    private void OnSidebarPlaylistRowPointerExited(object? sender, PointerEventArgs e)
    {
        if (sender is not Border row)
        {
            return;
        }

        var menuButton = row.FindByName<Border>("SidebarPlaylistMenuButton");
        if (menuButton != null)
        {
            menuButton.IsVisible = false;
        }
    }

    private async void OnSidebarPlaylistRowSecondaryPointerPressed(object? sender, PointerEventArgs e)
    {
        if (sender is not Border row || row.BindingContext is not mashin.Models.Playlist playlist)
        {
            return;
        }

        var position = e.GetPosition(null);
        await _viewModel.ShowSidebarPlaylistContextMenuAtPositionAsync(playlist, position);
    }

    #endregion

    #region Queue Overlay

    private async Task OnCloseQueueViewRequestedAsync()
    {
        if (_overlayService.IsQueueOverlayAnimating)
        {
            return;
        }

        if (_overlayService.IsQueueOverlayOpen)
        {
            await _overlayService.HideQueueOverlayAsync();
            UpdateQueueIconColor();
        }
    }

    private async void OnQueueTapped(object? sender, TappedEventArgs e)
    {
        if (_overlayService.IsQueueOverlayAnimating)
        {
            return;
        }

        if (_overlayService.IsQueueOverlayOpen)
        {
            await _overlayService.HideQueueOverlayAsync();
            UpdateQueueIconColor();
            return;
        }

        var queueIconLabel = this.FindByName<Label>("QueueIconLabel");
        queueIconLabel?.SetDynamicResource(Label.TextColorProperty, "AccentColor");
        await _overlayService.ShowQueueOverlayAsync(_viewModel);
        UpdateQueueIconColor();
    }

    private void UpdateQueueIconColor()
    {
        var queueIconLabel = this.FindByName<Label>("QueueIconLabel");
        queueIconLabel?.SetDynamicResource(
            Label.TextColorProperty,
            _overlayService.IsQueueOverlayOpen ? "AccentColor" : "IconSecondary");
    }

    #endregion

    #region Overlay Host

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

}
