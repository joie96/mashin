using System.Collections.ObjectModel;
using mashin.Models;
using Microsoft.Maui.Controls;

namespace mashin.Views.Desktop.Controls;

/// <summary>
/// Custom submenu control for displaying nested context menu items.
/// </summary>
public partial class ContextSubMenu : ContentView
{
    private static readonly WeakEventManager _pointerOverChangedManager = new();

    public static event EventHandler<bool> PointerOverChanged
    {
        add => _pointerOverChangedManager.AddEventHandler(value);
        remove => _pointerOverChangedManager.RemoveEventHandler(value);
    }
    public static readonly BindableProperty MenuItemsProperty =
        BindableProperty.Create(
            nameof(MenuItems),
            typeof(ObservableCollection<ContextMenuItem>),
            typeof(ContextSubMenu),
            new ObservableCollection<ContextMenuItem>());

    public ObservableCollection<ContextMenuItem> MenuItems
    {
        get => (ObservableCollection<ContextMenuItem>)GetValue(MenuItemsProperty);
        set => SetValue(MenuItemsProperty, value);
    }

    public event EventHandler<ContextMenuItem>? ItemSelected;
    public event EventHandler? RequestClose;

    public ContextSubMenu()
    {
        InitializeComponent();
    }

    private void OnPointerEntered(object sender, PointerEventArgs e)
    {
        if (sender is Border border && border.BindingContext is ContextMenuItem item && item.IsEnabled)
        {
            border.BackgroundColor = Application.Current?.Resources.TryGetValue("HoverBackground", out var value) == true && value is Color color ? color : Colors.Transparent;
        }
    }

    private void OnPointerExited(object sender, PointerEventArgs e)
    {
        if (sender is Border border)
        {
            border.BackgroundColor = Colors.Transparent;
        }
    }

    private void OnSubMenuPointerEntered(object sender, PointerEventArgs e)
    {
        _pointerOverChangedManager.HandleEvent(this, true, nameof(PointerOverChanged));
    }

    private void OnSubMenuPointerExited(object sender, PointerEventArgs e)
    {
        _pointerOverChangedManager.HandleEvent(this, false, nameof(PointerOverChanged));
    }

    private void OnSubMenuItemTapped(object sender, TappedEventArgs e)
    {
        if (sender is Border border && border.BindingContext is ContextMenuItem item)
        {
            if (!item.IsEnabled)
                return;

            // Execute command
            item.Command?.Execute(item.CommandParameter);
            ItemSelected?.Invoke(this, item);
            RequestClose?.Invoke(this, EventArgs.Empty);
        }
    }
}