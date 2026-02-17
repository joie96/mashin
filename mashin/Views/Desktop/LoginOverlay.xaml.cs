using mashin.Services;
using Microsoft.Extensions.Logging;

namespace mashin.Views.Desktop;

public partial class LoginOverlay : ContentView
{
    private readonly MusicAssistantService _musicAssistantService;
    private readonly SettingsService _settingsService;
    private readonly ILogger<LoginOverlay> _logger;

    public event EventHandler? LoginSuccessful;

    public LoginOverlay(
        MusicAssistantService musicAssistantService,
        SettingsService settingsService,
        ILogger<LoginOverlay> logger)
    {
        InitializeComponent();

        _musicAssistantService = musicAssistantService;
        _settingsService = settingsService;
        _logger = logger;

        // Pre-fill username if saved
        if (!string.IsNullOrEmpty(_settingsService.Username))
        {
            UsernameEntry.Text = _settingsService.Username;
        }
    }

    private void OnUsernameCompleted(object? sender, EventArgs e)
    {
        PasswordEntry.Focus();
    }

    private async void OnPasswordCompleted(object? sender, EventArgs e)
    {
        await PerformLoginAsync();
    }

    private async void OnLoginClicked(object? sender, EventArgs e)
    {
        await PerformLoginAsync();
    }

    private async Task PerformLoginAsync()
    {
        var username = UsernameEntry.Text?.Trim();
        var password = PasswordEntry.Text;

        // Validation
        if (string.IsNullOrEmpty(username))
        {
            ShowError("Bitte geben Sie einen Benutzernamen ein.");
            UsernameEntry.Focus();
            return;
        }

        if (string.IsNullOrEmpty(password))
        {
            ShowError("Bitte geben Sie ein Passwort ein.");
            PasswordEntry.Focus();
            return;
        }

        // Clear previous error
        HideError();

        // Show loading state
        SetLoadingState(true);

        try
        {
            _logger.LogInformation("Attempting login for user: {Username}", username);

            var success = await _musicAssistantService.LoginAsync(username, password);

            if (success)
            {
                _logger.LogInformation("Login successful for user: {Username}", username);
                
                // Clear password from UI
                PasswordEntry.Text = string.Empty;
                
                // Notify parent that login was successful
                LoginSuccessful?.Invoke(this, EventArgs.Empty);
            }
            else
            {
                _logger.LogWarning("Login failed for user: {Username}", username);
                ShowError("Anmeldung fehlgeschlagen. Bitte überprüfen Sie Ihre Anmeldedaten.");
                PasswordEntry.Text = string.Empty;
                PasswordEntry.Focus();
            }
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Login error for user: {Username}", username);
            ShowError($"Verbindungsfehler: {ex.Message}");
        }
        finally
        {
            SetLoadingState(false);
        }
    }

    private void SetLoadingState(bool isLoading)
    {
        LoginButton.IsEnabled = !isLoading;
        LoginButton.Text = isLoading ? "Anmelden..." : "Anmelden";
        LoadingIndicator.IsVisible = isLoading;
        LoadingIndicator.IsRunning = isLoading;
        UsernameEntry.IsEnabled = !isLoading;
        PasswordEntry.IsEnabled = !isLoading;
    }

    private void ShowError(string message)
    {
        ErrorLabel.Text = message;
        ErrorBorder.IsVisible = true;
    }

    private void HideError()
    {
        ErrorBorder.IsVisible = false;
        ErrorLabel.Text = string.Empty;
    }
}
