using mashin.ViewModels;

namespace mashin.Views.Desktop;

public partial class PlaylistDetailPage : ContentPage
{
    private readonly PlaylistDetailViewModel _viewModel;

    public PlaylistDetailPage(PlaylistDetailViewModel viewModel)
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
