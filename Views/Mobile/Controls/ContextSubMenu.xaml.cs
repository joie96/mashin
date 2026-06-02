using mashin.Models;
using System.Collections.ObjectModel;

namespace mashin.Views.Mobile.Controls;

public partial class ContextSubMenu : ContentView
{
    private const uint InFadeDurationMs = 200;
    private const uint InScaleDurationMs = 230;
    private const uint OutFadeDurationMs = 180;
    private const uint OutScaleDurationMs = 210;

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
        Scale = 0.94;

        await Task.WhenAll(
            this.FadeToAsync(1, InFadeDurationMs, Easing.SinOut),
            this.ScaleToAsync(1, InScaleDurationMs, Easing.SinOut));
    }

    public async Task AnimateOutAsync()
    {
        await Task.WhenAll(
            this.FadeToAsync(0, OutFadeDurationMs, Easing.SinIn),
            this.ScaleToAsync(0.94, OutScaleDurationMs, Easing.SinIn));

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
