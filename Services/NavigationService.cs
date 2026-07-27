using Microsoft.Extensions.Logging;
using Microsoft.Extensions.DependencyInjection;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using FFImageLoading;
using mashin.Models;
using DesktopAlbumDetailPage = mashin.Views.Desktop.AlbumDetailPage;
using DesktopArtistDetailPage = mashin.Views.Desktop.ArtistDetailPage;
using DesktopExplorePage = mashin.Views.Desktop.ExplorePage;
using DesktopFavoritesPage = mashin.Views.Desktop.FavoritesPage;
using DesktopHomePage = mashin.Views.Desktop.HomePage;
using DesktopPlaylistsPage = mashin.Views.Desktop.PlaylistsPage;
using DesktopPlaylistDetailPage = mashin.Views.Desktop.PlaylistDetailPage;
using DesktopSearchPage = mashin.Views.Desktop.SearchPage;
using MobileAlbumDetailPage = mashin.Views.Mobile.AlbumDetailPage;
using MobileArtistDetailPage = mashin.Views.Mobile.ArtistDetailPage;
using MobileExplorePage = mashin.Views.Mobile.ExplorePage;
using MobileFavoritesPage = mashin.Views.Mobile.FavoritesPage;
using MobileHomePage = mashin.Views.Mobile.HomePage;
using MobilePlaylistsPage = mashin.Views.Mobile.PlaylistsPage;
using MobilePlaylistDetailPage = mashin.Views.Mobile.PlaylistDetailPage;
using MobileSearchPage = mashin.Views.Mobile.SearchPage;

namespace mashin.Services;

#region Navigation Service

/// <summary>
/// Provides navigation operations for ContentPage-based views.
/// </summary>
public interface INavigationService : INotifyPropertyChanged
{
    bool IsNavigating { get; set; }
    bool CanGoBack { get; }
    Type? CurrentPageType { get; }
    Task NavigateToAsync<TPage>() where TPage : ContentPage;
    Task NavigateToAsync<TPage>(object? parameter) where TPage : ContentPage;
    Task GoBackAsync();
}

/// <summary>
/// Notifies a view model about navigation lifecycle events.
/// </summary>
public interface INavigationAware
{
    Task OnNavigatedFromAsync();
    Task OnNavigatedToAsync(object? parameter);
}

/// <summary>
/// Implements page navigation using a ContentView container and a local stack.
/// </summary>
public class NavigationService : INavigationService
{
    private static readonly IReadOnlyDictionary<Type, Type> MobilePageTypeMap = new Dictionary<Type, Type>
    {
        [typeof(DesktopHomePage)] = typeof(MobileHomePage),
        [typeof(DesktopExplorePage)] = typeof(MobileExplorePage),
        [typeof(DesktopFavoritesPage)] = typeof(MobileFavoritesPage),
        [typeof(DesktopPlaylistsPage)] = typeof(MobilePlaylistsPage),
        [typeof(DesktopSearchPage)] = typeof(MobileSearchPage),
        [typeof(DesktopPlaylistDetailPage)] = typeof(MobilePlaylistDetailPage),
        [typeof(DesktopArtistDetailPage)] = typeof(MobileArtistDetailPage),
        [typeof(DesktopAlbumDetailPage)] = typeof(MobileAlbumDetailPage)
    };

    private readonly IServiceScopeFactory _scopeFactory;
    private readonly ILogger<NavigationService> _logger;
    private bool _isNavigating;
    private ContentView? _contentContainer;
    private readonly Stack<NavigationEntry> _navigationStack = new();
    private ContentPage? _currentPage;
    private IServiceScope? _currentScope;
    private Type? _currentPageType;

    private sealed record NavigationEntry(Type PageType, object? Parameter);

    public bool IsNavigating
    {
        get => _isNavigating;
        set
        {
            if (_isNavigating != value)
            {
                _isNavigating = value;
                OnPropertyChanged();
            }
        }
    }

    public bool CanGoBack => _navigationStack.Count > 1;

    public Type? CurrentPageType
    {
        get => _currentPageType;
        private set
        {
            if (_currentPageType != value)
            {
                _currentPageType = value;
                OnPropertyChanged();
            }
        }
    }

    public NavigationService(
        IServiceScopeFactory scopeFactory,
        ILogger<NavigationService> logger)
    {
        _scopeFactory = scopeFactory;
        _logger = logger;
    }

    public void Initialize(ContentView contentContainer)
    {
        _contentContainer = contentContainer;
    }

    public async Task NavigateToAsync<TPage>() where TPage : ContentPage
    {
        await NavigateToAsync<TPage>(null);
    }

    public async Task NavigateToAsync<TPage>(object? parameter) where TPage : ContentPage
    {
        try
        {
            if (_contentContainer == null)
            {
                _logger.LogError("NavigationService not initialized with ContentView");
                return;
            }

            IsNavigating = true;
            await Task.Delay(10);

            var previousPage = _currentPage;
            var previousScope = _currentScope;

            // Save new page type and parameter on stack
            var entry = new NavigationEntry(typeof(TPage), parameter);
            _navigationStack.Push(entry);
            OnPropertyChanged(nameof(CanGoBack));
            
            // Show new page
            await ShowPageAsync(entry);

            if (previousPage != null)
            {
                _ = CleanupPageAsync(previousPage, previousScope);
            }
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to navigate to {PageName}", typeof(TPage).Name);
            throw;
        }
        finally
        {
            //IsNavigating = false; --> set when data is loaded in viewmodels
        }
    }

    public async Task GoBackAsync()
    {
        try
        {
            if (_contentContainer == null || _navigationStack.Count <= 1)
            {
                _logger.LogWarning("Cannot navigate back");
                return;
            }

            IsNavigating = true;
            await Task.Delay(10);

            // Remove current entry
            _navigationStack.Pop();
            OnPropertyChanged(nameof(CanGoBack));
            var previousPage = _currentPage;
            var previousScope = _currentScope;

            // Show previous page by recreating it from type+parameter
            var previousEntry = _navigationStack.Peek();
            await ShowPageAsync(previousEntry);

            if (previousPage != null)
            {
                _ = CleanupPageAsync(previousPage, previousScope);
            }
            
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to navigate back");
            throw;
        }
        finally
        {
            //IsNavigating = false; --> set when data is loaded in viewmodels
        }
    }

    private async Task ShowPageAsync(NavigationEntry entry)
    {
        var targetPageType = ResolveTargetPageType(entry.PageType);
        CurrentPageType = targetPageType;
        var scope = _scopeFactory.CreateScope();
        ContentPage newPage;
        try
        {
            newPage = (ContentPage)scope.ServiceProvider.GetRequiredService(targetPageType);
        }
        catch
        {
            scope.Dispose();
            throw;
        }
        _logger.LogDebug("Navigating to {PageName} (requested: {RequestedPageName})", targetPageType.Name, entry.PageType.Name);

        _currentPage = newPage;
        _currentScope = scope;

        if (newPage.Content != null && _contentContainer != null)
        {
            var content = newPage.Content;
            newPage.Content = null;
            content.BindingContext = newPage.BindingContext;
            _contentContainer.Content = content;
        }

        newPage.SendAppearing();

        if (newPage.BindingContext is INavigationAware navigationAware)
        {
            await navigationAware.OnNavigatedToAsync(entry.Parameter);
        }
    }

    private static Type ResolveTargetPageType(Type requestedPageType)
    {
        var isMobile = SettingsService.IsMobile();

        if (!isMobile)
        {
            return requestedPageType;
        }

        if (MobilePageTypeMap.TryGetValue(requestedPageType, out var mappedPageType))
        {
            return mappedPageType;
        }

        return requestedPageType;
    }

    private async Task CleanupPageAsync(ContentPage page, IServiceScope? scope)
    {
        _logger.LogDebug("Disposing page: {PageName}", page.GetType().Name);

        var bindingContext = page.BindingContext;

        // Call OnNavigatedFromAsync if page is navigation aware
        if (bindingContext is INavigationAware navAware)
        {
            await navAware.OnNavigatedFromAsync();
        }

        // Trigger lifecycle
        page.SendDisappearing();

        // Clear BindingContext and content references
        if (page.Content != null)
        {
            page.Content.BindingContext = null;
        }
        page.BindingContext = null;
        page.Content = null;

        // Dispose view model on the UI thread because it may mutate UI-bound collections.
        try
        {
            if (bindingContext is IDisposable disposable)
            {
                if (MainThread.IsMainThread)
                {
                    disposable.Dispose();
                }
                else
                {
                    await MainThread.InvokeOnMainThreadAsync(disposable.Dispose);
                }
            }
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to dispose binding context for {PageName}", page.GetType().Name);
        }

        // Dispose scope asynchronously to minimize navigation latency.
        _ = Task.Run(() =>
        {
            try
            {
                scope?.Dispose();
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Failed to dispose scope for {PageName}", page.GetType().Name);
            }
        });

    }

    public event PropertyChangedEventHandler? PropertyChanged;

    protected void OnPropertyChanged([CallerMemberName] string? propertyName = null)
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
    }


    
}

#endregion