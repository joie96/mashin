using FFImageLoading;
using mashin.ViewModels;

namespace mashin.Views.Desktop;

public partial class HomePage : ContentPage
{
    private readonly MainViewModel _viewModel;

    public HomePage(MainViewModel mainViewModel)
    {
        InitializeComponent();
        _viewModel = mainViewModel;
        BindingContext = _viewModel;
    }

    private async void OnUserIconTapped(object sender, EventArgs e)
    {
        // Navigation zu Settings
        
    }

    protected override void OnDisappearing()
    {
        base.OnDisappearing();

        BindingContext = null;

    }
}