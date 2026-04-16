namespace mashin.Views.Desktop.Controls;

public partial class DeletePlaylistOverlay : ContentView
{
    private string _playlistName = string.Empty;

    public DeletePlaylistOverlay()
    {
        InitializeComponent();
    }

    public event EventHandler? CancelClicked;
    public event EventHandler? DeleteClicked;

    public string PlaylistName
    {
        get => _playlistName;
        set => _playlistName = value;
    }

    public bool IsDeleteEnabled
    {
        get => DeletePlaylistButton.IsEnabled;
        set => DeletePlaylistButton.IsEnabled = value;
    }

    private void OnCancelClicked(object? sender, EventArgs e)
    {
        CancelClicked?.Invoke(this, EventArgs.Empty);
    }

    private void OnCancelTapped(object? sender, TappedEventArgs e)
    {
        CancelClicked?.Invoke(this, EventArgs.Empty);
    }

    private void OnDeleteClicked(object? sender, EventArgs e)
    {
        DeleteClicked?.Invoke(this, EventArgs.Empty);
    }
}
