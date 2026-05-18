using mashin.ViewModels;

namespace mashin.Views.Mobile;

public partial class AlbumDetailPage : ContentPage
{
    private readonly AlbumDetailViewModel _viewModel;

    public AlbumDetailPage(AlbumDetailViewModel viewModel)
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
