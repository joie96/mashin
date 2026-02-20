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

        BindingContext = null;
    }

    private void OnAlbumCoverSecondaryPointerPressed(object? sender, PointerEventArgs e)
    {
        if (BindingContext is not AlbumDetailViewModel viewModel)
        {
            return;
        }

        var position = e.GetPosition(null);
        viewModel.ShowHeaderContextMenuAtPositionCommand.Execute(position);
    }
}
