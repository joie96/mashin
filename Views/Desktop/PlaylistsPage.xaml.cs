using mashin.ViewModels;

namespace mashin.Views.Desktop;

public partial class PlaylistsPage : ContentPage
{
    private readonly PlaylistsViewModel _viewModel;

    public PlaylistsPage(PlaylistsViewModel viewModel)
    {
        InitializeComponent();
        _viewModel = viewModel;
        BindingContext = _viewModel;
    }

    protected override void OnDisappearing()
    {
        base.OnDisappearing();
        BindingContext = null;
    }
}
