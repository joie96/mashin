namespace mashin.Views.Desktop.Controls;

public partial class UpdatePlaylistOverlay : ContentView
{
    public UpdatePlaylistOverlay()
    {
        InitializeComponent();
    }

    public event EventHandler? CancelClicked;
    public event EventHandler? UpdateClicked;
    public event EventHandler<TextChangedEventArgs>? NameChanged;

    public string PlaylistName
    {
        get => UpdatePlaylistNameEntry.Text ?? string.Empty;
        set => UpdatePlaylistNameEntry.Text = value;
    }

    public bool IsUpdateEnabled
    {
        get => UpdatePlaylistButton.IsEnabled;
        set => UpdatePlaylistButton.IsEnabled = value;
    }

    private void OnCancelClicked(object? sender, EventArgs e)
    {
        CancelClicked?.Invoke(this, EventArgs.Empty);
    }

    private void OnCancelTapped(object? sender, TappedEventArgs e)
    {
        CancelClicked?.Invoke(this, EventArgs.Empty);
    }

    private void OnUpdateClicked(object? sender, EventArgs e)
    {
        UpdateClicked?.Invoke(this, EventArgs.Empty);
    }

    private void OnUpdatePlaylistNameChanged(object? sender, TextChangedEventArgs e)
    {
        NameChanged?.Invoke(this, e);
    }
}
