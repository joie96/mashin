using mashin.Services;
using mashin.ViewModels;
using Microsoft.Extensions.Logging;

namespace mashin.Views.Desktop;

public partial class MainPage : ContentPage
{
    private readonly MainViewModel _viewModel;
    private readonly INavigationService _navigationService;
    private readonly MusicAssistantService _musicAssistantService;
    private readonly SettingsService _settingsService;
    private readonly ILogger<MainPage> _logger;

    public MainPage(
        MainViewModel viewModel,
        INavigationService navigationService,
        MusicAssistantService musicAssistantService,
        SettingsService settingsService,
        ILogger<MainPage> logger)
    {
        InitializeComponent();

        _viewModel = viewModel;
        _navigationService = navigationService;
        _musicAssistantService = musicAssistantService;
        _settingsService = settingsService;
        _logger = logger;
        BindingContext = _viewModel;

        _musicAssistantService.LoginRequired += OnLoginRequired;

        PrefillLoginUsername();

        // Initialize navigation service with content container
        if (_navigationService is NavigationService navService)
        {
            navService.Initialize(ContentContainer);
        }

        // Navigate to home page
        _ = _navigationService.NavigateToAsync<HomePage>();

        _ = _viewModel.InitializeAsync();
    }

    private async void OnBackTapped(object? sender, TappedEventArgs e)
    {
        await _navigationService.GoBackAsync();
    }

    private void OnLoginRequired(object? sender, EventArgs e)
    {
        MainThread.BeginInvokeOnMainThread(ShowLoginOverlay);
    }

    private void ShowLoginOverlay()
    {
        LoginOverlay.IsVisible = true;
        PrefillLoginUsername();
    }

    private void HideLoginOverlay()
    {
        LoginOverlay.IsVisible = false;
        LoginPasswordEntry.Text = string.Empty;
    }

    private void PrefillLoginUsername()
    {
        if (!string.IsNullOrEmpty(_settingsService.Username))
        {
            LoginUsernameEntry.Text = _settingsService.Username;
        }
    }

    private void OnLoginUsernameCompleted(object? sender, EventArgs e)
    {
        LoginPasswordEntry.Focus();
    }

    private async void OnLoginPasswordCompleted(object? sender, EventArgs e)
    {
        await PerformLoginAsync();
    }

    private async void OnLoginClicked(object? sender, EventArgs e)
    {
        await PerformLoginAsync();
    }

    private async Task PerformLoginAsync()
    {
        var username = LoginUsernameEntry.Text?.Trim();
        var password = LoginPasswordEntry.Text;

        if (string.IsNullOrEmpty(username))
        {
            ShowLoginError("Bitte geben Sie einen Benutzernamen ein.");
            LoginUsernameEntry.Focus();
            return;
        }

        if (string.IsNullOrEmpty(password))
        {
            ShowLoginError("Bitte geben Sie ein Passwort ein.");
            LoginPasswordEntry.Focus();
            return;
        }

        HideLoginError();
        SetLoginLoadingState(true);

        try
        {
            _logger.LogInformation("Attempting login for user: {Username}", username);

            var success = await _musicAssistantService.LoginAsync(username, password);
            if (success)
            {
                _logger.LogInformation("Login successful for user: {Username}", username);
                HideLoginOverlay();
            }
            else
            {
                _logger.LogWarning("Login failed for user: {Username}", username);
                ShowLoginError("Anmeldung fehlgeschlagen. Bitte überprüfen Sie Ihre Anmeldedaten.");
                LoginPasswordEntry.Text = string.Empty;
                LoginPasswordEntry.Focus();
            }
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Login error for user: {Username}", username);
            ShowLoginError($"Verbindungsfehler: {ex.Message}");
        }
        finally
        {
            SetLoginLoadingState(false);
        }
    }

    private void SetLoginLoadingState(bool isLoading)
    {
        LoginButton.IsEnabled = !isLoading;
        LoginButton.Text = isLoading ? "Anmelden..." : "Anmelden";
        LoginLoadingIndicator.IsVisible = isLoading;
        LoginLoadingIndicator.IsRunning = isLoading;
        LoginUsernameEntry.IsEnabled = !isLoading;
        LoginPasswordEntry.IsEnabled = !isLoading;
    }

    private void ShowLoginError(string message)
    {
        LoginErrorLabel.Text = message;
        LoginErrorBorder.IsVisible = true;
    }

    private void HideLoginError()
    {
        LoginErrorBorder.IsVisible = false;
        LoginErrorLabel.Text = string.Empty;
    }

    private void OnOpenCreatePlaylistTapped(object? sender, TappedEventArgs e)
    {
        CreatePlaylistNameEntry.Text = string.Empty;
        UpdateCreatePlaylistButtonState();
        CreatePlaylistOverlay.IsVisible = true;
    }

    private void OnCreatePlaylistNameChanged(object? sender, TextChangedEventArgs e)
    {
        UpdateCreatePlaylistButtonState();
    }

    private void OnCancelCreatePlaylistTapped(object? sender, TappedEventArgs e)
    {
        CloseCreatePlaylistOverlay();
    }

    private void OnCancelCreatePlaylistClicked(object? sender, EventArgs e)
    {
        CloseCreatePlaylistOverlay();
    }

    private async void OnCreatePlaylistClicked(object? sender, EventArgs e)
    {
        var name = CreatePlaylistNameEntry.Text ?? string.Empty;
        CreatePlaylistButton.IsEnabled = false;

        var created = await _viewModel.CreatePlaylistAsync(name);
        if (created)
        {
            CloseCreatePlaylistOverlay();
        }
        else
        {
            UpdateCreatePlaylistButtonState();
        }
    }

    private void CloseCreatePlaylistOverlay()
    {
        CreatePlaylistOverlay.IsVisible = false;
    }

    private void UpdateCreatePlaylistButtonState()
    {
        CreatePlaylistButton.IsEnabled = !string.IsNullOrWhiteSpace(CreatePlaylistNameEntry.Text);
    }

    private void OnNavigatePlaylistsTapped(object? sender, TappedEventArgs e)
    {
        _logger.LogInformation("Navigate to playlists page not implemented yet.");
    }
}
