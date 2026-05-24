using mashin.Models;
using System.Collections.ObjectModel;

namespace mashin.Views.Mobile.Controls;

public partial class ContextMenu : ContentView
{
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

    public ContextMenu()
    {
        InitializeComponent();
    }

    public async Task AnimateInAsync()
    {
        Opacity = 0;
        TranslationY = 48;

        await Task.WhenAll(
            this.FadeToAsync(1, 180, Easing.CubicOut),
            this.TranslateToAsync(0, 0, 220, Easing.CubicOut));
    }

    public async Task AnimateOutAsync()
    {
        await Task.WhenAll(
            this.FadeToAsync(0, 140, Easing.CubicIn),
            this.TranslateToAsync(0, 40, 170, Easing.CubicIn));

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
}
