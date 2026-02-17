using mashin.ViewModels;

namespace mashin.Views.Desktop;

public partial class SearchPage : ContentPage
{
    private readonly SearchViewModel _viewModel;

    public SearchPage(SearchViewModel viewModel)
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
