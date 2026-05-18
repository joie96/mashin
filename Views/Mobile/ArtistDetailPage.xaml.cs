using mashin.ViewModels;

namespace mashin.Views.Mobile;

public partial class ArtistDetailPage : ContentPage
{
    private readonly ArtistDetailViewModel _viewModel;

    public ArtistDetailPage(ArtistDetailViewModel viewModel)
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
