namespace mashin.Views.Desktop.Controls;

public partial class LoginOverlay : ContentView
{
    public LoginOverlay()
    {
        InitializeComponent();
    }

    public event EventHandler? UsernameCompleted;
    public event EventHandler? PasswordCompleted;
    public event EventHandler? LoginClicked;

    public string Username
    {
        get => LoginUsernameEntry.Text ?? string.Empty;
        set => LoginUsernameEntry.Text = value;
    }

    public string Password
    {
        get => LoginPasswordEntry.Text ?? string.Empty;
        set => LoginPasswordEntry.Text = value;
    }

    public void FocusUsername() => LoginUsernameEntry.Focus();

    public void FocusPassword() => LoginPasswordEntry.Focus();

    public void ClearPassword() => LoginPasswordEntry.Text = string.Empty;

    public void SetLoadingState(bool isLoading)
    {
        LoginButton.IsEnabled = !isLoading;
        LoginButton.Text = isLoading ? "Anmelden..." : "Anmelden";
        LoginLoadingIndicator.IsVisible = isLoading;
        LoginLoadingIndicator.IsRunning = isLoading;
        LoginUsernameEntry.IsEnabled = !isLoading;
        LoginPasswordEntry.IsEnabled = !isLoading;
    }

    public void ShowError(string message)
    {
        SetStatusMessage(string.Empty);
        LoginErrorLabel.Text = message;
        LoginErrorBorder.IsVisible = true;
    }

    public void HideError()
    {
        LoginErrorBorder.IsVisible = false;
        LoginErrorLabel.Text = string.Empty;
    }

    public void SetStatusMessage(string? message)
    {
        LoginStatusLabel.Text = message ?? string.Empty;
        LoginStatusLabel.IsVisible = !string.IsNullOrWhiteSpace(message);
    }

    private void OnLoginUsernameCompleted(object? sender, EventArgs e)
    {
        UsernameCompleted?.Invoke(this, EventArgs.Empty);
    }

    private void OnLoginPasswordCompleted(object? sender, EventArgs e)
    {
        PasswordCompleted?.Invoke(this, EventArgs.Empty);
    }

    private void OnLoginClicked(object? sender, EventArgs e)
    {
        LoginClicked?.Invoke(this, EventArgs.Empty);
    }
}
