namespace mashin.Views.Desktop.Controls;

public partial class SortContentOverlay : ContentView
{
    private bool _isSortDescending;

    public SortContentOverlay()
    {
        InitializeComponent();
        ResetSelection();
    }

    public event EventHandler? CancelClicked;
    public event EventHandler? SortClicked;

    public string SelectedSortField => SortByPicker.SelectedItem?.ToString() ?? "Titel";

    public bool IsSortDescending
    {
        get => _isSortDescending;
        set
        {
            _isSortDescending = value;
            DirectionUpIcon.IsVisible = !_isSortDescending;
            DirectionDownIcon.IsVisible = _isSortDescending;
        }
    }

    public bool IsSortEnabled
    {
        get => SortPlaylistButton.IsEnabled;
        set => SortPlaylistButton.IsEnabled = value;
    }

    public void ResetSelection()
    {
        SortByPicker.SelectedIndex = 0;
        IsSortDescending = false;
        IsSortEnabled = true;
    }

    private void OnCancelClicked(object? sender, EventArgs e)
    {
        CancelClicked?.Invoke(this, EventArgs.Empty);
    }

    private void OnCancelTapped(object? sender, TappedEventArgs e)
    {
        CancelClicked?.Invoke(this, EventArgs.Empty);
    }

    private void OnSortClicked(object? sender, EventArgs e)
    {
        SortClicked?.Invoke(this, EventArgs.Empty);
    }

    private void OnToggleSortDirectionTapped(object? sender, TappedEventArgs e)
    {
        IsSortDescending = !IsSortDescending;
    }
}
