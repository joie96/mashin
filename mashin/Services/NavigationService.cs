using Microsoft.Extensions.Logging;
using Microsoft.Extensions.DependencyInjection;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using FFImageLoading;
using mashin.Models;

namespace mashin.Services;

#region Navigation Service

/// <summary>
/// Provides navigation operations for ContentPage-based views.
/// </summary>
public interface INavigationService : INotifyPropertyChanged
{
    bool IsNavigating { get; set; }
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
    private readonly IServiceScopeFactory _scopeFactory;
    private readonly ILogger<NavigationService> _logger;
    private bool _isNavigating;
    private ContentView? _contentContainer;
    private readonly Stack<NavigationEntry> _navigationStack = new();
    private ContentPage? _currentPage;
    private IServiceScope? _currentScope;

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
            await Task.Yield();

            // Dispose current page
            if (_currentPage != null)
            {
                await CleanupPageAsync(_currentPage);
                _currentPage = null;
            }

            // Save new page type and parameter on stack
            var entry = new NavigationEntry(typeof(TPage), parameter);
            _navigationStack.Push(entry);
            
            // Show new page
            await ShowPageAsync(entry);
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
            await Task.Yield();

            // Remove current entry and dispose current page
            _navigationStack.Pop();
            if (_currentPage != null)
            {
                await CleanupPageAsync(_currentPage);
                _currentPage = null;
            }

            // Show previous page by recreating it from type+parameter
            var previousEntry = _navigationStack.Peek();
            await ShowPageAsync(previousEntry);
            
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
        var scope = _scopeFactory.CreateScope();
        ContentPage newPage;
        try
        {
            newPage = (ContentPage)scope.ServiceProvider.GetRequiredService(entry.PageType);
        }
        catch
        {
            scope.Dispose();
            throw;
        }
        _logger.LogDebug("Navigating to {PageName}", entry.PageType.Name);

        if (entry.Parameter != null && newPage.BindingContext is INavigationAware navigationAware)
        {
            await navigationAware.OnNavigatedToAsync(entry.Parameter);
        }

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
    }

    private async Task CleanupPageAsync(ContentPage page)
    {
        _logger.LogDebug("Disposing page: {PageName}", page.GetType().Name);

        // Call OnNavigatedFromAsync if page is navigation aware
        if (page.BindingContext is INavigationAware navAware)
        {
            await navAware.OnNavigatedFromAsync();
        }

        // Trigger lifecycle
        page.SendDisappearing();

        // Clear image cache
        ImageService.Instance.InvalidateMemoryCache();

        // Dispose ViewModel if IDisposable
        if (page.BindingContext is IDisposable disposable)
        {
            disposable.Dispose();
        }

        // Clear BindingContext and content references
        if (page.Content != null)
        {
            page.Content.BindingContext = null;
        }
        page.BindingContext = null;
        page.Content = null;

        // Detach from container
        if (_contentContainer != null)
        {
            var containerContent = _contentContainer.Content;
            if (containerContent != null)
            {
                containerContent.BindingContext = null;
            }

            _contentContainer.Content = null;
        }

        if (_currentScope != null)
        {
            _currentScope.Dispose();
            _currentScope = null;
        }

    }

    public event PropertyChangedEventHandler? PropertyChanged;

    protected void OnPropertyChanged([CallerMemberName] string? propertyName = null)
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
    }


    
}

#endregion