using mashin.Services;
using mashin.ViewModels;

namespace mashin.Views.Desktop;

public partial class HomePage : ContentPage
{
    private readonly MainViewModel _viewModel;
    private readonly MusicAssistantService _musicAssistant;

    public HomePage(MainViewModel mainViewModel, MusicAssistantService musicAssistant)
    {
        InitializeComponent();
        _viewModel = mainViewModel;
        _musicAssistant = musicAssistant;
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

    protected override async void OnAppearing()
    {
        base.OnAppearing();
    }


}