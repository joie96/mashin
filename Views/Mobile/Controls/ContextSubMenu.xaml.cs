using mashin.Models;
using System.Collections.ObjectModel;

namespace mashin.Views.Mobile.Controls;

public partial class ContextSubMenu : ContentView
{
    public static readonly BindableProperty TitleProperty =
        BindableProperty.Create(
            nameof(Title),
            typeof(string),
            typeof(ContextSubMenu),
            "Optionen");

    public static readonly BindableProperty MenuItemsProperty =
        BindableProperty.Create(
            nameof(MenuItems),
            typeof(ObservableCollection<ContextMenuItem>),
            typeof(ContextSubMenu),
            new ObservableCollection<ContextMenuItem>());

    public static readonly BindableProperty MaxMenuHeightProperty =
        BindableProperty.Create(
            nameof(MaxMenuHeight),
            typeof(double),
            typeof(ContextSubMenu),
            420d);

    public string Title
    {
        get => (string)GetValue(TitleProperty);
        set => SetValue(TitleProperty, value);
    }

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

    public ContextSubMenu()
    {
        InitializeComponent();
    }

    public async Task AnimateInAsync()
    {
        Opacity = 0;
        Scale = 0.96;

        await Task.WhenAll(
            this.FadeToAsync(1, 160, Easing.CubicOut),
            this.ScaleToAsync(1, 180, Easing.CubicOut));
    }

    public async Task AnimateOutAsync()
    {
        await Task.WhenAll(
            this.FadeToAsync(0, 120, Easing.CubicIn),
            this.ScaleToAsync(0.96, 140, Easing.CubicIn));

        Scale = 1;
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
