using mashin.Models;
using mashin.ViewModels;
using FFImageLoading;

namespace mashin.Views.Desktop;

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
                
        _viewModel?.Dispose();
        
        BindingContext = null;

    }
}
