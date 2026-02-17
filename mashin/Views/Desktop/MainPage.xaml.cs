using mashin.Services;
using mashin.ViewModels;

namespace mashin.Views.Desktop;

public partial class MainPage : ContentPage
{
    private readonly MainViewModel _viewModel;
    private readonly INavigationService _navigationService;

    public MainPage(MainViewModel viewModel, INavigationService navigationService)
    {
        InitializeComponent();

        _viewModel = viewModel;
        _navigationService = navigationService;
        BindingContext = _viewModel;

        _ = _viewModel.InitializeAsync();

        // Initialize navigation service with content container
        if (_navigationService is NavigationService navService)
        {
            navService.Initialize(ContentContainer);
        }

        // Navigate to home page
        _ = _navigationService.NavigateToAsync<HomePage>();
    }

    private async void OnBackTapped(object? sender, TappedEventArgs e)
    {
        await _navigationService.GoBackAsync();
    }
}
