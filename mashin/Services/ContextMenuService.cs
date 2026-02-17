using mashin.Models;
using System.Collections.ObjectModel;


#if WINDOWS
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.Maui.Platform;
using mashin.Views.Desktop.Controls;
#endif

namespace mashin.Services;

public interface IContextMenuService
{
    /// <summary>
    /// Shows context menu anchored to a specific view (e.g., button).
    /// </summary>
    Task ShowContextMenuAsync(ObservableCollection<ContextMenuItem> items, View anchor);

    /// <summary>
    /// Shows context menu at a specific position within the application window.
    /// </summary>
    Task ShowContextMenuAsync(ObservableCollection<ContextMenuItem> items, Point position);

    /// <summary>
    /// Shows submenu anchored to a menu item.
    /// </summary>
    Task ShowSubMenuAsync(ObservableCollection<ContextMenuItem> subItems, View anchor);
    
    /// <summary>
    /// Closes the currently open main menu (and any submenu).
    /// </summary>
    void CloseMenu();

    /// <summary>
    /// Closes the currently open submenu.
    /// </summary>
    void CloseSubMenu();

}

#if WINDOWS
/// <summary>
/// Windows-native
/// </summary>
public class WindowsContextMenuService : IContextMenuService
{
    private Popup? _currentPopup;
    private Popup? _currentSubMenuPopup;
    private Microsoft.UI.Xaml.FrameworkElement? _currentMenuElement;
    private Microsoft.UI.Xaml.FrameworkElement? _currentSubMenuElement;
    private Microsoft.UI.Xaml.Input.KeyEventHandler? _keyDownHandler;
    private Microsoft.UI.Xaml.Input.PointerEventHandler? _pointerPressedHandler;
    private Microsoft.UI.Xaml.Window? _currentWindow;
    private DateTime _ignorePointerUntilUtc;
    public Task ShowContextMenuAsync(ObservableCollection<ContextMenuItem> items, View anchor)
    {
        var contextMenu = new ContextMenu { MenuItems = items };
        contextMenu.RequestClose += (s, e) => CloseMenu();

        var mauiContext = anchor.Handler?.MauiContext
            ?? throw new InvalidOperationException("MauiContext not found");

        var platformView = contextMenu.ToPlatform(mauiContext);
        _currentMenuElement = platformView;

        if (anchor.Handler?.PlatformView is not Microsoft.UI.Xaml.FrameworkElement anchorElement)
        {
            throw new InvalidOperationException("Anchor element not found");
        }

        _currentPopup = new Popup
        {
            Child = platformView,
            IsLightDismissEnabled = true,
            XamlRoot = anchorElement.XamlRoot
        };

        _currentPopup.Closed += (s, e) =>
        {
            CloseSubMenu();
            DetachKeyHandler();
            _currentPopup = null;
        };

        var transform = anchorElement.TransformToVisual(null);
        var anchorPoint = transform.TransformPoint(new Windows.Foundation.Point(0, 0));
        var position = new Point(
            anchorPoint.X + anchorElement.ActualWidth,
            anchorPoint.Y + anchorElement.ActualHeight
        );

        _currentPopup.IsOpen = true;

        CalculateSmartPosition(_currentPopup, platformView, position, anchorElement.XamlRoot);
        
        AttachKeyHandler();

        return Task.CompletedTask;
    }

    public Task ShowContextMenuAsync(ObservableCollection<ContextMenuItem> items, Point position)
    {
        var contextMenu = new ContextMenu { MenuItems = items };
        contextMenu.RequestClose += (s, e) => CloseMenu();

        var window = Microsoft.Maui.Controls.Application.Current?.Windows[0];
        if (window?.Handler?.PlatformView is not Microsoft.UI.Xaml.Window nativeWindow)
        {
            throw new InvalidOperationException("Cannot get native window");
        }

        var mauiContext = window.Handler.MauiContext
            ?? throw new InvalidOperationException("MauiContext not found");

        var platformView = contextMenu.ToPlatform(mauiContext);
        _currentMenuElement = platformView;

        _currentPopup = new Popup
        {
            Child = platformView,
            IsLightDismissEnabled = true,
            XamlRoot = nativeWindow.Content.XamlRoot
        };

        _currentPopup.Closed += (s, e) =>
        {
            CloseSubMenu();
            DetachKeyHandler();
            _currentPopup = null;
        };

        _currentPopup.IsOpen = true;

        CalculateSmartPosition(_currentPopup, platformView, position, nativeWindow.Content.XamlRoot);
        
        AttachKeyHandler();

        return Task.CompletedTask;
    }

    public Task ShowSubMenuAsync(ObservableCollection<ContextMenuItem> subItems, View anchor)
    {
        // Close the previous submenu
        CloseSubMenu();

        if (subItems == null || subItems.Count == 0)
            return Task.CompletedTask;

        var subMenu = new ContextSubMenu { MenuItems = subItems };
        subMenu.RequestClose += (s, e) => CloseMenu();

        var mauiContext = anchor.Handler?.MauiContext
            ?? throw new InvalidOperationException("MauiContext not found");

        var platformView = subMenu.ToPlatform(mauiContext);
        _currentSubMenuElement = platformView;

        if (anchor.Handler?.PlatformView is not Microsoft.UI.Xaml.FrameworkElement anchorElement)
        {
            throw new InvalidOperationException("Anchor element not found");
        }

        _currentSubMenuPopup = new Popup
        {
            Child = platformView,
            // Keep hover working on the main menu by disabling light dismiss for submenus.
            IsLightDismissEnabled = false,
            XamlRoot = anchorElement.XamlRoot
        };

        _currentSubMenuPopup.Closed += (s, e) => _currentSubMenuPopup = null;

        // Position to the right of the anchor
        var transform = anchorElement.TransformToVisual(null);
        var anchorPoint = transform.TransformPoint(new Windows.Foundation.Point(0, 0));

        var windowWidth = anchorElement.XamlRoot.Size.Width;
        var windowHeight = anchorElement.XamlRoot.Size.Height;

        // Open popup BEFORE Measure (important for ActualWidth/Height)
        _currentSubMenuPopup.IsOpen = true;

        // Wait briefly for rendering, then position
        MainThread.BeginInvokeOnMainThread(() =>
        {
            platformView.UpdateLayout();
            var menuWidth = platformView.ActualWidth > 0 ? platformView.ActualWidth : 200;
            var menuHeight = platformView.ActualHeight > 0 ? platformView.ActualHeight : 100;

            // To the right of the menu item
            double offsetX = anchorPoint.X + anchorElement.ActualWidth + 4;
            double offsetY = anchorPoint.Y - 4;

            // Overflow handling: show on the left if there is no space
            if (offsetX + menuWidth > windowWidth)
            {
                offsetX = anchorPoint.X - menuWidth - 4;
            }

            // Vertical overflow
            if (offsetY + menuHeight > windowHeight)
            {
                offsetY = Math.Max(0, windowHeight - menuHeight - 8);
            }

            _currentSubMenuPopup.HorizontalOffset = offsetX;
            _currentSubMenuPopup.VerticalOffset = offsetY;
        });

        return Task.CompletedTask;
    }

    public void CloseMenu()
    {
        CloseSubMenu(); // Close submenus as well
        if (_currentPopup != null)
        {
            _currentPopup.IsOpen = false;
            _currentPopup = null;
        }

        _currentMenuElement = null;
    }    

    public void CloseSubMenu()
    {
        if (_currentSubMenuPopup?.IsOpen == true)
        {
            _currentSubMenuPopup.IsOpen = false;
            _currentSubMenuPopup = null;
            _currentSubMenuElement = null;
        }
    }



    // Global key and pointer handlers to close the menu on Escape or outside clicks ()
    private void AttachKeyHandler()
    {
        var window = Microsoft.Maui.Controls.Application.Current?.Windows.FirstOrDefault();
        _currentWindow = window?.Handler?.PlatformView as Microsoft.UI.Xaml.Window;

        if (_currentWindow?.Content is not Microsoft.UI.Xaml.FrameworkElement windowContent)
        {
            return;
        }

        // Avoid closing immediately on the same click that opened the menu.
        _ignorePointerUntilUtc = DateTime.UtcNow.AddMilliseconds(150);

        _keyDownHandler = (_, args) =>
        {
            if (args.Key == Windows.System.VirtualKey.Escape && _currentPopup?.IsOpen == true)
            {
                CloseMenu();
                args.Handled = true;
            }
        };

        _pointerPressedHandler = (_, args) =>
        {
            if (_currentPopup?.IsOpen != true)
            {
                return;
            }

            if (DateTime.UtcNow < _ignorePointerUntilUtc)
            {
                return;
            }

            var point = args.GetCurrentPoint(windowContent).Position;
            if (IsPointInsideMenu(windowContent, point))
            {
                return;
            }

            // Outside click closes main menu and any submenu.
            CloseMenu();
            args.Handled = true;
        };

        windowContent.KeyDown += _keyDownHandler;
        windowContent.PointerPressed += _pointerPressedHandler;
    }

    private void DetachKeyHandler()
    {
        if (_currentWindow?.Content is Microsoft.UI.Xaml.FrameworkElement windowContent)
        {
            if (_keyDownHandler != null)
            {
                windowContent.KeyDown -= _keyDownHandler;
                _keyDownHandler = null;
            }

            if (_pointerPressedHandler != null)
            {
                windowContent.PointerPressed -= _pointerPressedHandler;
                _pointerPressedHandler = null;
            }
        }

        _currentWindow = null;
    }

    // Checks if the pointer is within the bounds of the main menu or submenu to prevent closing when interacting with them.
    private bool IsPointInsideMenu(Microsoft.UI.Xaml.FrameworkElement windowContent, Windows.Foundation.Point point)
    {
        if (IsPointInsideElement(windowContent, _currentMenuElement, point))
        {
            return true;
        }

        if (IsPointInsideElement(windowContent, _currentSubMenuElement, point))
        {
            return true;
        }

        return false;
    }

    // Checks if a point is within the bounds of a given element
    private static bool IsPointInsideElement(
        Microsoft.UI.Xaml.FrameworkElement windowContent,
        Microsoft.UI.Xaml.FrameworkElement? element,
        Windows.Foundation.Point point)
    {
        if (element == null || element.ActualWidth <= 0 || element.ActualHeight <= 0)
        {
            return false;
        }

        var transform = element.TransformToVisual(windowContent);
        var topLeft = transform.TransformPoint(new Windows.Foundation.Point(0, 0));
        var rect = new Windows.Foundation.Rect(topLeft.X, topLeft.Y, element.ActualWidth, element.ActualHeight);
        return rect.Contains(point);
    }


    // Calculates smart position with overflow handling for both anchor-based and position-based menus.
    private void CalculateSmartPosition(
        Popup popup,
        Microsoft.UI.Xaml.FrameworkElement menuElement,
        Point position,
        Microsoft.UI.Xaml.XamlRoot xamlRoot)
    {
        var windowWidth = xamlRoot.Size.Width;
        var windowHeight = xamlRoot.Size.Height;

        menuElement.UpdateLayout();
        var menuWidth = menuElement.ActualWidth;
        var menuHeight = menuElement.ActualHeight;

        double offsetX = position.X;
        double offsetY = position.Y;

        if (offsetX + menuWidth > windowWidth)
        {
            offsetX = Math.Max(0, position.X - menuWidth);
        }

        if (offsetY + menuHeight > windowHeight)
        {
            offsetY = Math.Max(0, position.Y - menuHeight);
        }

        popup.HorizontalOffset = offsetX;
        popup.VerticalOffset = offsetY;
    }

}

#else
public class DefaultContextMenuService : IContextMenuService
{
    public Task ShowContextMenuAsync(ObservableCollection<ContextMenuItem> items, View anchor)
    {
        throw new PlatformNotSupportedException("Context Menu wird nur auf Windows unterstützt.");
    }

    public Task ShowContextMenuAsync(ObservableCollection<ContextMenuItem> items, Point position)
    {
        throw new PlatformNotSupportedException("Context Menu wird nur auf Windows unterstützt.");
    }

    public Task ShowSubMenuAsync(ObservableCollection<ContextMenuItem> subItems, View anchor)
    {
        throw new PlatformNotSupportedException("Context Menu wird nur auf Windows unterstützt.");
    }
  
    public void CloseMenu()
    {
        throw new PlatformNotSupportedException("Context Menu wird nur auf Windows unterstützt.");
    }

    public void CloseSubMenu()
    {
        throw new PlatformNotSupportedException("Context Menu wird nur auf Windows unterstützt.");
    }

}
#endif