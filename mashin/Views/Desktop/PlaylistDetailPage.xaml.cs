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

        _viewModel?.Dispose();

        BindingContext = null;

    }

    private void OnPlaylistCoverSecondaryPointerPressed(object? sender, PointerEventArgs e)
    {
        if (BindingContext is not PlaylistDetailViewModel viewModel)
        {
            return;
        }

        var position = e.GetPosition(null);
        viewModel.ShowHeaderContextMenuAtPositionCommand?.Execute(position);
    }
}
