using mashin.Models;
using mashin.Services;
using mashin.ViewModels;
using mashin.Views.Desktop.Controls;
using Microsoft.Extensions.Logging;

namespace mashin.Views.Desktop;

public partial class MainPage : ContentPage
{
    #region Fields

    private readonly MainViewModel _viewModel;
    private readonly INavigationService _navigationService;
    private readonly MusicAssistantService _musicAssistantService;
    private readonly SettingsService _settingsService;
    private readonly ILogger<MainPage> _logger;
    private readonly CreatePlaylistOverlay _createPlaylistOverlay;
    private readonly UpdatePlaylistOverlay _updatePlaylistOverlay;
    private readonly DeletePlaylistOverlay _deletePlaylistOverlay;
    private readonly LoginOverlay _loginOverlay;
    private Action? _overlayCloseAction;
    private Playlist? _pendingUpdatePlaylist;
    private string? _pendingUpdateOriginalName;
    private Playlist? _pendingDeletePlaylist;

    #endregion

    #region Construction

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

        _createPlaylistOverlay = new CreatePlaylistOverlay();
        _createPlaylistOverlay.NameChanged += OnCreatePlaylistNameChanged;
        _createPlaylistOverlay.CancelClicked += OnCancelCreatePlaylistClicked;
        _createPlaylistOverlay.CreateClicked += OnCreatePlaylistClicked;

        _updatePlaylistOverlay = new UpdatePlaylistOverlay();
        _updatePlaylistOverlay.NameChanged += OnUpdatePlaylistNameChanged;
        _updatePlaylistOverlay.CancelClicked += OnCancelUpdatePlaylistClicked;
        _updatePlaylistOverlay.UpdateClicked += OnUpdatePlaylistClicked;

        _deletePlaylistOverlay = new DeletePlaylistOverlay();
        _deletePlaylistOverlay.CancelClicked += OnCancelDeletePlaylistClicked;
        _deletePlaylistOverlay.DeleteClicked += OnDeletePlaylistClicked;

        _loginOverlay = new LoginOverlay();
        _loginOverlay.UsernameCompleted += OnLoginUsernameCompleted;
        _loginOverlay.PasswordCompleted += OnLoginPasswordCompleted;
        _loginOverlay.LoginClicked += OnLoginClicked;

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

    #endregion

    #region Navigation

    private async void OnBackTapped(object? sender, TappedEventArgs e)
    {
        await _navigationService.GoBackAsync();
    }

    private void OnNavigatePlaylistsTapped(object? sender, TappedEventArgs e)
    {
        _logger.LogInformation("Navigate to playlists page not implemented yet.");
    }

    #endregion

    #region Login Overlay

    private void OnLoginRequired(object? sender, EventArgs e)
    {
        MainThread.BeginInvokeOnMainThread(ShowLoginOverlay);
    }

    public void ShowLoginOverlay()
    {
        PrefillLoginUsername();
        ShowOverlay(_loginOverlay, null);
    }

    private void HideLoginOverlay()
    {
        HideOverlay();
        _loginOverlay.ClearPassword();
    }

    private void PrefillLoginUsername()
    {
        if (!string.IsNullOrEmpty(_settingsService.Username))
        {
            _loginOverlay.Username = _settingsService.Username;
        }
    }

    private void OnLoginUsernameCompleted(object? sender, EventArgs e)
    {
        _loginOverlay.FocusPassword();
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
        var username = _loginOverlay.Username.Trim();
        var password = _loginOverlay.Password;

        if (string.IsNullOrEmpty(username))
        {
            ShowLoginError("Bitte geben Sie einen Benutzernamen ein.");
            _loginOverlay.FocusUsername();
            return;
        }

        if (string.IsNullOrEmpty(password))
        {
            ShowLoginError("Bitte geben Sie ein Passwort ein.");
            _loginOverlay.FocusPassword();
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
                _loginOverlay.ClearPassword();
                _loginOverlay.FocusPassword();
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
        _loginOverlay.SetLoadingState(isLoading);
    }

    private void ShowLoginError(string message)
    {
        _loginOverlay.ShowError(message);
    }

    private void HideLoginError()
    {
        _loginOverlay.HideError();
    }

    #endregion

    #region Create Playlist Overlay

    private void OnOpenCreatePlaylistTapped(object? sender, TappedEventArgs e)
    {
        ShowCreatePlaylistOverlay();
    }

    private void OnCreatePlaylistNameChanged(object? sender, TextChangedEventArgs e)
    {
        UpdateCreatePlaylistButtonState();
    }

    private void OnCancelCreatePlaylistClicked(object? sender, EventArgs e)
    {
        CloseCreatePlaylistOverlay();
    }

    private async void OnCreatePlaylistClicked(object? sender, EventArgs e)
    {
        var name = _createPlaylistOverlay.PlaylistName;
        _createPlaylistOverlay.IsCreateEnabled = false;

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
        HideOverlay();
    }

    private void UpdateCreatePlaylistButtonState()
    {
        _createPlaylistOverlay.IsCreateEnabled = !string.IsNullOrWhiteSpace(_createPlaylistOverlay.PlaylistName);
    }

    public void ShowCreatePlaylistOverlay()
    {
        _createPlaylistOverlay.PlaylistName = string.Empty;
        UpdateCreatePlaylistButtonState();
        ShowOverlay(_createPlaylistOverlay, CloseCreatePlaylistOverlay);
    }

    #endregion

    #region Update Playlist Overlay

    private void OnUpdatePlaylistNameChanged(object? sender, TextChangedEventArgs e)
    {
        UpdateUpdatePlaylistButtonState();
    }

    private void OnCancelUpdatePlaylistClicked(object? sender, EventArgs e)
    {
        CloseUpdatePlaylistOverlay();
    }

    private async void OnUpdatePlaylistClicked(object? sender, EventArgs e)
    {
        if (_pendingUpdatePlaylist is null)
        {
            CloseUpdatePlaylistOverlay();
            return;
        }

        var name = _updatePlaylistOverlay.PlaylistName;
        _updatePlaylistOverlay.IsUpdateEnabled = false;

        var updated = await _viewModel.UpdatePlaylistAsync(_pendingUpdatePlaylist, name);
        if (updated)
        {
            CloseUpdatePlaylistOverlay();
        }
        else
        {
            UpdateUpdatePlaylistButtonState();
        }
    }

    private void CloseUpdatePlaylistOverlay()
    {
        _pendingUpdatePlaylist = null;
        _pendingUpdateOriginalName = null;
        HideOverlay();
    }

    private void UpdateUpdatePlaylistButtonState()
    {
        var name = _updatePlaylistOverlay.PlaylistName;
        var hasName = !string.IsNullOrWhiteSpace(name);
        var isChanged = !string.Equals(name?.Trim(), _pendingUpdateOriginalName, StringComparison.Ordinal);
        _updatePlaylistOverlay.IsUpdateEnabled = hasName && isChanged;
    }

    public void ShowUpdatePlaylistOverlay(Playlist playlist)
    {
        if (playlist is null)
        {
            return;
        }

        _pendingUpdatePlaylist = playlist;
        _pendingUpdateOriginalName = playlist.DisplayName ?? playlist.Name ?? string.Empty;
        _updatePlaylistOverlay.PlaylistName = _pendingUpdateOriginalName;
        UpdateUpdatePlaylistButtonState();
        ShowOverlay(_updatePlaylistOverlay, CloseUpdatePlaylistOverlay);
    }

    #endregion

    #region Delete Playlist Overlay

    private void OnCancelDeletePlaylistClicked(object? sender, EventArgs e)
    {
        CloseDeletePlaylistOverlay();
    }

    private async void OnDeletePlaylistClicked(object? sender, EventArgs e)
    {
        if (_pendingDeletePlaylist is null)
        {
            CloseDeletePlaylistOverlay();
            return;
        }

        _deletePlaylistOverlay.IsDeleteEnabled = false;

        var removed = await _viewModel.RemovePlaylistAsync(_pendingDeletePlaylist);
        if (removed)
        {
            CloseDeletePlaylistOverlay();
        }
        else
        {
            _deletePlaylistOverlay.IsDeleteEnabled = true;
        }
    }

    private void CloseDeletePlaylistOverlay()
    {
        _pendingDeletePlaylist = null;
        _deletePlaylistOverlay.PlaylistName = string.Empty;
        HideOverlay();
    }

    public void ShowDeletePlaylistOverlay(Playlist playlist)
    {
        if (playlist is null)
        {
            return;
        }

        _pendingDeletePlaylist = playlist;
        _deletePlaylistOverlay.PlaylistName = playlist.DisplayName ?? playlist.Name ?? string.Empty;
        _deletePlaylistOverlay.IsDeleteEnabled = true;
        ShowOverlay(_deletePlaylistOverlay, CloseDeletePlaylistOverlay);
    }

    #endregion

    #region Overlay Host

    private void OnOverlayBackdropTapped(object? sender, TappedEventArgs e)
    {
        _overlayCloseAction?.Invoke();
    }

    private void ShowOverlay(View overlay, Action? onClose)
    {
        OverlayContent.Content = overlay;
        _overlayCloseAction = onClose;
        OverlayHost.IsVisible = true;
    }

    private void HideOverlay()
    {
        OverlayHost.IsVisible = false;
        OverlayContent.Content = null;
        _overlayCloseAction = null;
    }

    #endregion
}
