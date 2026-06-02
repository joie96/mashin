using mashin.Models;
using System.Collections.ObjectModel;

namespace mashin.Views.Mobile.Controls;

public partial class ContextMenu : ContentView
{
    private const double DragDismissThreshold = 120d;
    private const double DragResistance = 0.92d;

    private bool _isHandleDragging;
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
        ResetHandleBarVisual();

        Opacity = 0;
        TranslationY = 48;

        await Task.WhenAll(
            this.FadeToAsync(1, 180, Easing.CubicOut),
            this.TranslateToAsync(0, 0, 220, Easing.CubicOut));
    }

    public async Task AnimateOutAsync()
    {
        _dismissTriggered = true;

        await Task.WhenAll(
            this.FadeToAsync(0, 140, Easing.CubicIn),
            this.TranslateToAsync(0, 40, 170, Easing.CubicIn));

        TranslationY = 0;
        Opacity = 1;
        ResetHandleBarVisual();
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

    private void OnHandlePanUpdated(object? sender, PanUpdatedEventArgs e)
    {
        HandlePanUpdated(e, startedFromHandle: true);
    }

    private void OnMenuPanUpdated(object? sender, PanUpdatedEventArgs e)
    {
        HandlePanUpdated(e, startedFromHandle: false);
    }

    private void OnMenuItemsScrolled(object? sender, ItemsViewScrolledEventArgs e)
    {
        _menuVerticalOffset = e.VerticalOffset;
    }

    private void HandlePanUpdated(PanUpdatedEventArgs e, bool startedFromHandle)
    {
        switch (e.StatusType)
        {
            case GestureStatus.Started:
                _canDismissForCurrentPan = startedFromHandle || IsScrollAtTop();
                _isHandleDragging = _canDismissForCurrentPan;

                if (startedFromHandle)
                {
                    GetHandleBar()?.SetValue(Border.BackgroundColorProperty, Colors.White);
                }

                break;

            case GestureStatus.Running when _isHandleDragging:
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
                    _ = this.TranslateToAsync(0, 0, 120, Easing.CubicOut);
                }

                _isHandleDragging = false;
                _canDismissForCurrentPan = false;
                ResetHandleBarVisual();
                break;
        }
    }

    private void ResetHandleBarVisual()
    {
        GetHandleBar()?.SetDynamicResource(Border.BackgroundColorProperty, "SeparatorColor");
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

    private Border? GetHandleBar()
    {
        return FindByName("HandleBar") as Border;
    }
}
