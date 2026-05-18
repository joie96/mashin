using mashin.Models;
using System.Windows.Input;

namespace mashin.Views.Mobile.Controls;

public partial class TableView : ContentView
{
    public static readonly BindableProperty ItemsSourceProperty =
        BindableProperty.Create(nameof(ItemsSource), typeof(IEnumerable<object>), typeof(TableView));

    public static readonly BindableProperty PrimaryInfoTappedCommandProperty =
        BindableProperty.Create(nameof(PrimaryInfoTappedCommand), typeof(ICommand), typeof(TableView));

    public static readonly BindableProperty ShowContextMenuAtAnchorCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtAnchorCommand), typeof(ICommand), typeof(TableView));

    public TableView()
    {
        InitializeComponent();
    }

    public IEnumerable<object>? ItemsSource
    {
        get => (IEnumerable<object>?)GetValue(ItemsSourceProperty);
        set => SetValue(ItemsSourceProperty, value);
    }

    public ICommand? PrimaryInfoTappedCommand
    {
        get => (ICommand?)GetValue(PrimaryInfoTappedCommandProperty);
        set => SetValue(PrimaryInfoTappedCommandProperty, value);
    }

    public ICommand? ShowContextMenuAtAnchorCommand
    {
        get => (ICommand?)GetValue(ShowContextMenuAtAnchorCommandProperty);
        set => SetValue(ShowContextMenuAtAnchorCommandProperty, value);
    }
}

public sealed class MobileTableViewTemplateSelector : DataTemplateSelector
{
    public DataTemplate? PlaylistTemplate { get; set; }
    public DataTemplate? SkeletonTemplate { get; set; }

    protected override DataTemplate OnSelectTemplate(object item, BindableObject container)
    {
        if (item is TableViewSkeleton && SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        if (item is Playlist && PlaylistTemplate != null)
        {
            return PlaylistTemplate;
        }

        if (PlaylistTemplate != null)
        {
            return PlaylistTemplate;
        }

        if (SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        throw new InvalidOperationException("MobileTableViewTemplateSelector requires PlaylistTemplate or SkeletonTemplate.");
    }
}
