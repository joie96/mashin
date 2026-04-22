using mashin.Collections;
using mashin.Models;
using mashin.Services;
using System.Collections.Specialized;
using System.Windows.Input;

namespace mashin.Views.Desktop.Controls;

public partial class ListView : ContentView
{
    #region Bindable properties

    public static readonly BindableProperty ItemsSourceProperty =
        BindableProperty.Create(nameof(ItemsSource), typeof(IEnumerable<object>), typeof(ListView), propertyChanged: OnItemsSourceChanged);

    public static readonly BindableProperty MediaActionsProperty =
        BindableProperty.Create(nameof(MediaActions), typeof(IMediaItemActions), typeof(ListView));

    public static readonly BindableProperty PrimaryInfoTappedCommandProperty =
        BindableProperty.Create(nameof(PrimaryInfoTappedCommand), typeof(ICommand), typeof(ListView));

    public static readonly BindableProperty MaxItemsProperty =
        BindableProperty.Create(nameof(MaxItems), typeof(int), typeof(ListView), 9, propertyChanged: OnMaxItemsChanged);

    #endregion

    #region Fields

    private readonly ObservableRangeCollection<object> _visibleItems = new();
    private INotifyCollectionChanged? _itemsSourceCollection;

    #endregion

    #region Properties

    public IEnumerable<object>? ItemsSource
    {
        get => (IEnumerable<object>?)GetValue(ItemsSourceProperty);
        set => SetValue(ItemsSourceProperty, value);
    }

    public IMediaItemActions? MediaActions
    {
        get => (IMediaItemActions?)GetValue(MediaActionsProperty);
        set => SetValue(MediaActionsProperty, value);
    }

    public ICommand? PrimaryInfoTappedCommand
    {
        get => (ICommand?)GetValue(PrimaryInfoTappedCommandProperty);
        set => SetValue(PrimaryInfoTappedCommandProperty, value);
    }

    public int MaxItems
    {
        get => (int)GetValue(MaxItemsProperty);
        set => SetValue(MaxItemsProperty, value);
    }

    #endregion

    public ListView()
    {
        InitializeComponent();
        ItemsCollectionView.ItemsSource = _visibleItems;
        RefreshVisibleItems();
    }

    private static void OnItemsSourceChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not ListView view)
        {
            return;
        }

        view.AttachItemsSourceCollection(newValue as IEnumerable<object>);
        view.RefreshVisibleItems();
    }

    private static void OnMaxItemsChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not ListView view)
        {
            return;
        }

        view.RefreshVisibleItems();
    }

    private void AttachItemsSourceCollection(IEnumerable<object>? source)
    {
        if (_itemsSourceCollection != null)
        {
            _itemsSourceCollection.CollectionChanged -= OnItemsSourceCollectionChanged;
            _itemsSourceCollection = null;
        }

        if (source is INotifyCollectionChanged collection)
        {
            _itemsSourceCollection = collection;
            _itemsSourceCollection.CollectionChanged += OnItemsSourceCollectionChanged;
        }
    }

    private void OnItemsSourceCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        if (!MainThread.IsMainThread)
        {
            MainThread.BeginInvokeOnMainThread(RefreshVisibleItems);
            return;
        }

        RefreshVisibleItems();
    }

    private void RefreshVisibleItems()
    {
        var source = ItemsSource;
        var maxItems = Math.Max(1, MaxItems);

        if (source == null)
        {
            _visibleItems.Clear();
            return;
        }

        var items = source.Take(maxItems).ToList();
        _visibleItems.ReplaceRange(items);
    }

    private async void OnPlayOverlayClicked(object? sender, TappedEventArgs e)
    {
        if (sender is not Border playButton || playButton.BindingContext is not MediaItem item || MediaActions == null)
        {
            return;
        }

        await MediaActions.PlayMediaAsync(item);
    }
}

public sealed class ListViewTemplateSelector : DataTemplateSelector
{
    public DataTemplate? PlaylistTemplate { get; set; }
    public DataTemplate? SkeletonTemplate { get; set; }

    protected override DataTemplate OnSelectTemplate(object item, BindableObject container)
    {
        if (item is ListViewSkeleton && SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        if (item is Playlist && PlaylistTemplate != null)
        {
            return PlaylistTemplate;
        }

        if (SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        throw new InvalidOperationException("ListViewTemplateSelector requires PlaylistTemplate or SkeletonTemplate.");
    }
}
