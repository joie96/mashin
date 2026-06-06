using mashin.Models;
using System.Collections.ObjectModel;

namespace mashin.Views.Mobile.Controls;

public partial class ContextMenu : ContentView
{
    private const double DragDismissThreshold = 100d;
    private const double DragResistance = 0.92d;
    private const uint InSlideDurationMs = 320;
    private const uint OutSlideDurationMs = 300;
    private const uint DragCancelSnapBackDurationMs = 180;

    private bool _isSheetDragging;
    private bool _canDismissForCurrentPan;
    private bool _dismissTriggered;
    private double _menuVerticalOffset;

    public static readonly BindableProperty MenuItemsProperty =
        BindableProperty.Create(
            nameof(MenuItems),
            typeof(ObservableCollection<ContextMenuItem>),
            typeof(ContextMenu),
            new ObservableCollection<ContextMenuItem>());

    public static readonly BindableProperty MaxMenuHeightProperty =
        BindableProperty.Create(
            nameof(MaxMenuHeight),
            typeof(double),
            typeof(ContextMenu),
            520d);

    public ObservableCollection<ContextMenuItem> MenuItems
    {
        get => (ObservableCollection<ContextMenuItem>)GetValue(MenuItemsProperty);
        set => SetValue(MenuItemsProperty, value);
    }

    public double MaxMenuHeight
    {
        get => (double)GetValue(MaxMenuHeightProperty);
        set => SetValue(MaxMenuHeightProperty, value);
    }

    public event EventHandler<ContextMenuItem>? ItemInvoked;
    public event EventHandler? DismissRequested;

    public ContextMenu()
    {
        InitializeComponent();
    }

    public async Task AnimateInAsync()
    {
        _dismissTriggered = false;
        _menuVerticalOffset = 0;

        TranslationY = GetSlideDistance();

        await this.TranslateToAsync(0, 0, InSlideDurationMs, Easing.SinOut);
    }

    public async Task AnimateOutAsync()
    {
        _dismissTriggered = true;

        await this.TranslateToAsync(0, GetSlideDistance(), OutSlideDurationMs, Easing.SinIn);

        TranslationY = 0;
    }

    private void OnMenuItemTapped(object? sender, TappedEventArgs e)
    {
        if (sender is not Element element || element.BindingContext is not ContextMenuItem item)
        {
            return;
        }

        if (!item.IsEnabled || item.IsSeparator)
        {
            return;
        }

        ItemInvoked?.Invoke(this, item);
    }

    private void OnMenuPanUpdated(object? sender, PanUpdatedEventArgs e)
    {
        HandlePanUpdated(e);
    }

    private void OnMenuItemsScrolled(object? sender, ItemsViewScrolledEventArgs e)
    {
        _menuVerticalOffset = e.VerticalOffset;
    }

    private void HandlePanUpdated(PanUpdatedEventArgs e)
    {
        switch (e.StatusType)
        {
            case GestureStatus.Started:
                _canDismissForCurrentPan = IsScrollAtTop();
                _isSheetDragging = _canDismissForCurrentPan;

                break;

            case GestureStatus.Running when _isSheetDragging:
                if (_dismissTriggered)
                {
                    break;
                }

                if (!_canDismissForCurrentPan)
                {
                    break;
                }

                if (e.TotalY <= 0)
                {
                    return;
                }

                var translationY = Math.Max(0, e.TotalY * DragResistance);
                TranslationY = translationY;

                if (translationY >= DragDismissThreshold)
                {
                    _dismissTriggered = true;
                    DismissRequested?.Invoke(this, EventArgs.Empty);
                }

                break;

            case GestureStatus.Completed:
            case GestureStatus.Canceled:
                if (!_dismissTriggered)
                {
                    _ = this.TranslateToAsync(0, 0, DragCancelSnapBackDurationMs, Easing.SinOut);
                }

                _isSheetDragging = false;
                _canDismissForCurrentPan = false;
                break;
        }
    }

    private bool IsScrollAtTop()
    {
        var scrollY = GetCurrentScrollY();
        return scrollY <= 0d;
    }

    private double GetCurrentScrollY()
    {
        return _menuVerticalOffset;
    }

    private double GetSlideDistance()
    {
        var menuHeight = GetMenuSheet()?.Height ?? Height;
        if (menuHeight <= 0)
        {
            menuHeight = MaxMenuHeight;
        }

        return Math.Max(120d, menuHeight + 24d);
    }

    private Border? GetMenuSheet()
    {
        return FindByName("MenuSheet") as Border;
    }

}
