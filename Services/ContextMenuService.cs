using mashin.Models;
using System.Collections.ObjectModel;
using System.Threading;

#if WINDOWS
using mashin.Views.Desktop.Controls;
using Microsoft.Maui.Platform;
using Microsoft.UI.Xaml.Controls.Primitives;
#else
using mashin.Views.Mobile.Controls;
using Microsoft.Maui.Devices;
#endif

namespace mashin.Services;

public interface IContextMenuService
{
    Task ShowContextMenuAsync(ObservableCollection<ContextMenuItem> items, View anchor);
    Task ShowContextMenuAsync(ObservableCollection<ContextMenuItem> items, Point position);
    Task ShowSubMenuAsync(ObservableCollection<ContextMenuItem> subItems, View anchor);
    void CloseMenu();
    void CloseSubMenu();
}

#if WINDOWS
// Windows-specific context menu service 
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

    // Opens the main menu next to an anchor control.
    public Task ShowContextMenuAsync(ObservableCollection<ContextMenuItem> items, View anchor)
    {
        var contextMenu = new ContextMenu { MenuItems = items };
        contextMenu.RequestClose += (_, _) => CloseMenu();

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

        _currentPopup.Closed += (_, _) =>
        {
            CloseSubMenu();
            DetachKeyHandler();
            _currentPopup = null;
        };

        var transform = anchorElement.TransformToVisual(null);
        var anchorPoint = transform.TransformPoint(new Windows.Foundation.Point(0, 0));
        var position = new Point(
            anchorPoint.X + anchorElement.ActualWidth,
            anchorPoint.Y + anchorElement.ActualHeight);

        _currentPopup.IsOpen = true;
        CalculateSmartPosition(_currentPopup, platformView, position, anchorElement.XamlRoot);
        AttachKeyHandler();

        return Task.CompletedTask;
    }

    // Opens the main menu at an absolute pointer position.
    public Task ShowContextMenuAsync(ObservableCollection<ContextMenuItem> items, Point position)
    {
        var contextMenu = new ContextMenu { MenuItems = items };
        contextMenu.RequestClose += (_, _) => CloseMenu();

        var window = Application.Current?.Windows[0];
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

        _currentPopup.Closed += (_, _) =>
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

    // Opens a submenu near the hovered/clicked menu item.
    public Task ShowSubMenuAsync(ObservableCollection<ContextMenuItem> subItems, View anchor)
    {
        CloseSubMenu();

        if (subItems == null || subItems.Count == 0)
        {
            return Task.CompletedTask;
        }

        var subMenu = new ContextSubMenu { MenuItems = subItems };
        subMenu.RequestClose += (_, _) => CloseMenu();

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
            IsLightDismissEnabled = false,
            XamlRoot = anchorElement.XamlRoot
        };

        _currentSubMenuPopup.Closed += (_, _) => _currentSubMenuPopup = null;

        var transform = anchorElement.TransformToVisual(null);
        var anchorPoint = transform.TransformPoint(new Windows.Foundation.Point(0, 0));

        var windowWidth = anchorElement.XamlRoot.Size.Width;
        var windowHeight = anchorElement.XamlRoot.Size.Height;

        _currentSubMenuPopup.IsOpen = true;

        MainThread.BeginInvokeOnMainThread(() =>
        {
            platformView.UpdateLayout();
            var menuWidth = platformView.ActualWidth > 0 ? platformView.ActualWidth : 200;
            var menuHeight = platformView.ActualHeight > 0 ? platformView.ActualHeight : 100;

            var offsetX = anchorPoint.X + anchorElement.ActualWidth + 4;
            var offsetY = anchorPoint.Y - 4;

            if (offsetX + menuWidth > windowWidth)
            {
                offsetX = anchorPoint.X - menuWidth - 4;
            }

            if (offsetY + menuHeight > windowHeight)
            {
                offsetY = Math.Max(0, windowHeight - menuHeight - 8);
            }

            _currentSubMenuPopup.HorizontalOffset = offsetX;
            _currentSubMenuPopup.VerticalOffset = offsetY;
        });

        return Task.CompletedTask;
    }

    // Closes the main menu and clears references.
    public void CloseMenu()
    {
        CloseSubMenu();

        if (_currentPopup != null)
        {
            _currentPopup.IsOpen = false;
            _currentPopup = null;
        }

        _currentMenuElement = null;
    }

    // Closes only the submenu popup.
    public void CloseSubMenu()
    {
        if (_currentSubMenuPopup?.IsOpen == true)
        {
            _currentSubMenuPopup.IsOpen = false;
            _currentSubMenuPopup = null;
            _currentSubMenuElement = null;
        }
    }

    // Registers Escape/outside-click handlers while a menu is open.
    private void AttachKeyHandler()
    {
        var window = Application.Current?.Windows.FirstOrDefault();
        _currentWindow = window?.Handler?.PlatformView as Microsoft.UI.Xaml.Window;

        if (_currentWindow?.Content is not Microsoft.UI.Xaml.FrameworkElement windowContent)
        {
            return;
        }

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

            CloseMenu();
            args.Handled = true;
        };

        windowContent.KeyDown += _keyDownHandler;
        windowContent.PointerPressed += _pointerPressedHandler;
    }

    // Unregisters global handlers when the menu closes.
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

    // Returns true if the pointer is inside either main menu or submenu.
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

    // Hit-test helper for a single framework element.
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

    // Keeps the popup on screen by clamping coordinates to the window bounds.
    private static void CalculateSmartPosition(
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

        var offsetX = position.X;
        var offsetY = position.Y;

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
// Default context menu service (non-Windows)
public sealed class DefaultContextMenuService : IContextMenuService
{
    private readonly IOverlayService _overlayService;
    private readonly SemaphoreSlim _menuLock = new(1, 1);

    private TaskCompletionSource<bool>? _currentMenuClosedTcs;
    private ContextMenu? _mainMenu;
    private ContextSubMenu? _subMenu;

    public DefaultContextMenuService(IOverlayService overlayService)
    {
        _overlayService = overlayService;
    }

    /// <summary>
    /// Shows the main context menu as a bottom sheet.
    /// Anchor is intentionally ignored on mobile.
    /// </summary>
    public Task ShowContextMenuAsync(ObservableCollection<ContextMenuItem> items, View anchor)
        => ShowMainMenuAsync(items);

    /// <summary>
    /// Shows the main context menu as a bottom sheet.
    /// Absolute position is intentionally ignored on mobile.
    /// </summary>
    public Task ShowContextMenuAsync(ObservableCollection<ContextMenuItem> items, Point position)
        => ShowMainMenuAsync(items);

    /// <summary>
    /// Shows submenu items centered over the currently open main menu overlay.
    /// </summary>
    public Task ShowSubMenuAsync(ObservableCollection<ContextMenuItem> subItems, View anchor)
    {
        if (subItems == null || subItems.Count == 0)
        {
            return Task.CompletedTask;
        }

        return ShowSubMenuCoreAsync(subItems, "Optionen");
    }

    public void CloseMenu()
    {
        _ = CloseMenuCoreAsync();
    }

    public void CloseSubMenu()
    {
        _ = HideSubMenuCoreAsync();
    }

    // Creates and shows the flyout main menu via OverlayService.
    private async Task ShowMainMenuAsync(ObservableCollection<ContextMenuItem> items)
    {
        if (items == null || items.Count == 0)
        {
            return;
        }

        await _menuLock.WaitAsync();

        try
        {
            if (_mainMenu != null)
            {
                await CloseMenuCoreAsync();
            }

            var maxSheetHeight = GetMaxMainMenuHeight();

            _mainMenu = new ContextMenu
            {
                MenuItems = items,
                MaxMenuHeight = maxSheetHeight
            };
            _mainMenu.ItemInvoked += async (_, item) => await OnMainMenuItemInvokedAsync(item);
            _mainMenu.DismissRequested += async (_, _) => await CloseMenuCoreAsync();

            _currentMenuClosedTcs = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);

            await _overlayService.ShowContextMenuFlyoutAsync(_mainMenu, CloseMenu);

            // Ensure first layout/frame before running the menu's own slide-in animation.
            await Task.Yield();
            await _mainMenu.AnimateInAsync();

            await _currentMenuClosedTcs.Task;
        }
        finally
        {
            _menuLock.Release();
        }
    }

    // Handles taps on main menu items. Subitems open overlay; normal items execute and close.
    private async Task OnMainMenuItemInvokedAsync(ContextMenuItem item)
    {
        if (_mainMenu == null)
        {
            return;
        }

        if (item.SubItems is { Count: > 0 })
        {
            await ShowSubMenuCoreAsync(item.SubItems, item.Text);
            return;
        }

        ExecuteItemCommand(item);
        await CloseMenuCoreAsync();
    }

    // Handles taps on submenu items, then closes all overlays.
    private async Task OnSubMenuItemInvokedAsync(ContextMenuItem item)
    {
        ExecuteItemCommand(item);
        await CloseMenuCoreAsync();
    }

    // Executes the command associated with a menu item if executable.
    private static void ExecuteItemCommand(ContextMenuItem item)
    {
        if (item.Command is not { } command)
        {
            return;
        }

        if (command.CanExecute(item.CommandParameter))
        {
            command.Execute(item.CommandParameter);
            return;
        }

        if (item.CommandParameter == null && command.CanExecute(null))
        {
            command.Execute(null);
        }
    }

    // Shows submenu content as centered overlay.
    private async Task ShowSubMenuCoreAsync(ObservableCollection<ContextMenuItem> subItems, string title)
    {
        if (_mainMenu == null || subItems == null || subItems.Count == 0)
        {
            return;
        }

        if (_subMenu != null)
        {
            await HideSubMenuCoreAsync();
        }

        _subMenu = new ContextSubMenu
        {
            Title = title,
            MenuItems = subItems,
            MaxMenuHeight = GetMaxSubMenuHeight()
        };
        _subMenu.ItemInvoked += async (_, item) => await OnSubMenuItemInvokedAsync(item);

        await _overlayService.ShowContextMenuSubMenuAsync(_subMenu, CloseSubMenu);
        await _subMenu.AnimateInAsync();
    }

    // Hides only the centered submenu.
    private async Task HideSubMenuCoreAsync()
    {
        if (_subMenu == null)
        {
            return;
        }

        await _subMenu.AnimateOutAsync();
        await _overlayService.HideContextMenuSubMenuAsync();
        _subMenu = null;
    }

    // Closes the full mobile context menu flow.
    private async Task CloseMenuCoreAsync()
    {
        if (_mainMenu == null)
        {
            return;
        }

        await HideSubMenuCoreAsync();

        await _mainMenu.AnimateOutAsync();
        await _overlayService.HideContextMenuFlyoutAsync();

        _mainMenu = null;

        _currentMenuClosedTcs?.TrySetResult(true);
        _currentMenuClosedTcs = null;
    }

    // Calculates max main menu height as 75% of display height.
    private static double GetMaxMainMenuHeight()
    {
        var display = DeviceDisplay.Current.MainDisplayInfo;
        if (display.Density <= 0)
        {
            return 520;
        }

        var heightInDp = display.Height / display.Density;
        return Math.Max(260, Math.Floor(heightInDp * 0.75));
    }

    // Calculates max submenu height as 55% of display height.
    private static double GetMaxSubMenuHeight()
    {
        var display = DeviceDisplay.Current.MainDisplayInfo;
        if (display.Density <= 0)
        {
            return 420;
        }

        var heightInDp = display.Height / display.Density;
        return Math.Max(220, Math.Floor(heightInDp * 0.55));
    }

}
#endif
