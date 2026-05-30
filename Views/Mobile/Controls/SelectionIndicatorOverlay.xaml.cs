namespace mashin.Views.Mobile.Controls;

public partial class SelectionIndicatorOverlay : ContentView
{
    public SelectionIndicatorOverlay()
    {
        InitializeComponent();
    }

    public event EventHandler? SelectAllTapped;
    public event EventHandler? MenuTapped;
    public event EventHandler? CloseTapped;

    public View MenuAnchor => MenuAnchorBorder;

    private void OnSelectAllTapped(object? sender, TappedEventArgs e)
    {
        SelectAllTapped?.Invoke(this, EventArgs.Empty);
    }

    private void OnMenuTapped(object? sender, TappedEventArgs e)
    {
        MenuTapped?.Invoke(this, EventArgs.Empty);
    }

    private void OnCloseTapped(object? sender, TappedEventArgs e)
    {
        CloseTapped?.Invoke(this, EventArgs.Empty);
    }
}
