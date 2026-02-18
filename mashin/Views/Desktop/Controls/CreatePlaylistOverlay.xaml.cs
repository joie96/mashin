namespace mashin.Views.Desktop.Controls;

public partial class CreatePlaylistOverlay : ContentView
{
    public CreatePlaylistOverlay()
    {
        InitializeComponent();
    }

    public event EventHandler? CancelClicked;
    public event EventHandler? CreateClicked;
    public event EventHandler<TextChangedEventArgs>? NameChanged;

    public string PlaylistName
    {
        get => CreatePlaylistNameEntry.Text ?? string.Empty;
        set => CreatePlaylistNameEntry.Text = value;
    }

    public bool IsCreateEnabled
    {
        get => CreatePlaylistButton.IsEnabled;
        set => CreatePlaylistButton.IsEnabled = value;
    }

    private void OnCancelClicked(object? sender, EventArgs e)
    {
        CancelClicked?.Invoke(this, EventArgs.Empty);
    }

    private void OnCancelTapped(object? sender, TappedEventArgs e)
    {
        CancelClicked?.Invoke(this, EventArgs.Empty);
    }

    private void OnCreateClicked(object? sender, EventArgs e)
    {
        CreateClicked?.Invoke(this, EventArgs.Empty);
    }

    private void OnCreatePlaylistNameChanged(object? sender, TextChangedEventArgs e)
    {
        NameChanged?.Invoke(this, e);
    }
}
