using mashin.Models;
using mashin.ViewModels;
using FFImageLoading;

namespace mashin.Views.Desktop;

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

        _viewModel?.Dispose();

        BindingContext = null;
    }
}
