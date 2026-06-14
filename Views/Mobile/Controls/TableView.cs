using mashin.Collections;
using mashin.Models;
using mashin.Services;
using Microsoft.Extensions.DependencyInjection;
using System.Collections;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Linq;
using System.Windows.Input;

namespace mashin.Views.Mobile.Controls;

public partial class TableView : ContentView
{
    #region Fields

    private const int DefaultInitialLoadCount = 15;
    private const int LoadMoreCount = 10;

    private readonly HashSet<MediaItem> _suppressNextTap = new();
    private readonly HashSet<INotifyPropertyChanged> _observedItems = new();
    private readonly ObservableRangeCollection<object> _visibleItems = new();
    private INotifyCollectionChanged? _observedCollection;
    private int _loadedItemCount = DefaultInitialLoadCount;
    private bool _hasMoreItems;
    private bool _hasSelection;
    private double _verticalOffset;

    #endregion

    #region Bindable properties

    public static readonly BindableProperty ItemsSourceProperty =
        BindableProperty.Create(
            nameof(ItemsSource),
            typeof(IEnumerable<object>),
            typeof(TableView),
            propertyChanged: OnItemsSourceChanged);

    public static readonly BindableProperty ShortPressCommandProperty =
        BindableProperty.Create(nameof(ShortPressCommand), typeof(ICommand), typeof(TableView));

    public static readonly BindableProperty PlaybackContextItemProperty =
        BindableProperty.Create(nameof(PlaybackContextItem), typeof(object), typeof(TableView));

    public static readonly BindableProperty ShowContextMenuAtAnchorCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtAnchorCommand), typeof(ICommand), typeof(TableView));

    public static readonly BindableProperty LongPressCommandProperty =
        BindableProperty.Create(nameof(LongPressCommand), typeof(ICommand), typeof(TableView));

    public static readonly BindableProperty InitialLoadCountProperty =
        BindableProperty.Create(
            nameof(InitialLoadCount),
            typeof(int),
            typeof(TableView),
            DefaultInitialLoadCount,
            propertyChanged: OnInitialLoadCountChanged);

    #endregion

    #region Construction

    public TableView()
    {
        InitializeComponent();
    }

    #endregion

    #region Properties

    public IEnumerable<object>? ItemsSource
    {
        get => (IEnumerable<object>?)GetValue(ItemsSourceProperty);
        set => SetValue(ItemsSourceProperty, value);
    }

    public ICommand? ShortPressCommand
    {
        get => (ICommand?)GetValue(ShortPressCommandProperty);
        set => SetValue(ShortPressCommandProperty, value);
    }

    public object? PlaybackContextItem
    {
        get => GetValue(PlaybackContextItemProperty);
        set => SetValue(PlaybackContextItemProperty, value);
    }

    public ICommand? ShowContextMenuAtAnchorCommand
    {
        get => (ICommand?)GetValue(ShowContextMenuAtAnchorCommandProperty);
        set => SetValue(ShowContextMenuAtAnchorCommandProperty, value);
    }

    public ICommand? LongPressCommand
    {
        get => (ICommand?)GetValue(LongPressCommandProperty);
        set => SetValue(LongPressCommandProperty, value);
    }

    public int InitialLoadCount
    {
        get => (int)GetValue(InitialLoadCountProperty);
        set => SetValue(InitialLoadCountProperty, value);
    }

    public bool HasMoreItems => _hasMoreItems;

    public IReadOnlyList<object> VisibleItems => _visibleItems;

    public bool HasSelection => _hasSelection;

    public double VerticalOffset => _verticalOffset;

    public bool IsScrolledToTop => _verticalOffset <= 0d;

    public event EventHandler<PanUpdatedEventArgs>? ItemsPanUpdated;

    public event EventHandler<double>? VerticalOffsetChanged;

    #endregion

    #region Selection state synchronization

    private static void OnItemsSourceChanged(BindableObject bindable, object? oldValue, object? newValue)
    {
        if (bindable is not TableView tableView)
        {
            return;
        }

        if (!ReferenceEquals(oldValue, newValue))
        {
            tableView._loadedItemCount = tableView.InitialLoadCount > 0
                ? tableView.InitialLoadCount
                : DefaultInitialLoadCount;
        }

        tableView.DetachSelectionObservers();
        tableView.AttachSelectionObservers(newValue as IEnumerable<object>);
        tableView.RefreshVisibleItems();
        tableView.UpdateSelectionIndicator();
    }

    private static void OnInitialLoadCountChanged(BindableObject bindable, object? oldValue, object? newValue)
    {
        if (bindable is not TableView tableView)
        {
            return;
        }

        var configuredInitialLoadCount = newValue as int? ?? tableView.InitialLoadCount;
        var initialLoadCount = configuredInitialLoadCount > 0
            ? configuredInitialLoadCount
            : DefaultInitialLoadCount;
        if (tableView._loadedItemCount == initialLoadCount && Equals(oldValue, newValue))
        {
            return;
        }

        tableView._loadedItemCount = initialLoadCount;
        tableView.RefreshVisibleItems();
    }

    private void AttachSelectionObservers(IEnumerable<object>? items)
    {
        if (items is INotifyCollectionChanged collectionChanged)
        {
            _observedCollection = collectionChanged;
            _observedCollection.CollectionChanged += OnItemsCollectionChanged;
        }

        if (items == null)
        {
            return;
        }

        ObserveItems(items);
    }

    private void DetachSelectionObservers()
    {
        if (_observedCollection != null)
        {
            _observedCollection.CollectionChanged -= OnItemsCollectionChanged;
            _observedCollection = null;
        }

        foreach (var item in _observedItems)
        {
            item.PropertyChanged -= OnObservedItemPropertyChanged;
        }

        _observedItems.Clear();
    }

    private void OnItemsCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        switch (e.Action)
        {
            case NotifyCollectionChangedAction.Add:
                ObserveItems(e.NewItems);
                break;

            case NotifyCollectionChangedAction.Remove:
                UnobserveItems(e.OldItems);
                break;

            case NotifyCollectionChangedAction.Replace:
                UnobserveItems(e.OldItems);
                ObserveItems(e.NewItems);
                break;

            case NotifyCollectionChangedAction.Move:
                break;

            case NotifyCollectionChangedAction.Reset:
            default:
                DetachSelectionObservers();
                AttachSelectionObservers(ItemsSource);
                break;
        }

        RefreshVisibleItems();
        UpdateSelectionIndicator();
    }

    private void ObserveItems(IEnumerable? items)
    {
        if (items == null)
        {
            return;
        }

        foreach (var item in items)
        {
            ObserveItem(item);
        }
    }

    private void UnobserveItems(IEnumerable? items)
    {
        if (items == null)
        {
            return;
        }

        foreach (var item in items)
        {
            UnobserveItem(item);
        }
    }

    private void ObserveItem(object? item)
    {
        if (item is INotifyPropertyChanged itemNotifier && _observedItems.Add(itemNotifier))
        {
            itemNotifier.PropertyChanged += OnObservedItemPropertyChanged;
        }

        if (ResolveMediaItem(item) is INotifyPropertyChanged mediaItemNotifier && _observedItems.Add(mediaItemNotifier))
        {
            mediaItemNotifier.PropertyChanged += OnObservedItemPropertyChanged;
        }
    }

    private void UnobserveItem(object? item)
    {
        if (item is INotifyPropertyChanged itemNotifier && _observedItems.Remove(itemNotifier))
        {
            itemNotifier.PropertyChanged -= OnObservedItemPropertyChanged;
        }

        if (ResolveMediaItem(item) is INotifyPropertyChanged mediaItemNotifier && _observedItems.Remove(mediaItemNotifier))
        {
            mediaItemNotifier.PropertyChanged -= OnObservedItemPropertyChanged;
        }
    }

    private void OnObservedItemPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (!string.Equals(e.PropertyName, nameof(MediaItem.IsSelected), StringComparison.Ordinal))
        {
            return;
        }

        UpdateSelectionIndicator();
    }

    #endregion

    #region Input handling

    private void OnRowTouchCompleted(object? sender, EventArgs e)
    {
        if (sender is not BindableObject { BindingContext: { } rowItem })
        {
            return;
        }

        var mediaItem = ResolveMediaItem(rowItem);
        if (mediaItem == null)
        {
            return;
        }

        EnsureSelectionOwnership();

        _ = ExecuteShortPressAsync(mediaItem);
    }

    private void OnRowLongPressCompleted(object? sender, EventArgs e)
    {
        if (sender is not BindableObject { BindingContext: { } rowItem })
        {
            return;
        }

        var mediaItem = ResolveMediaItem(rowItem);
        if (mediaItem == null)
        {
            return;
        }

        EnsureSelectionOwnership();

        mediaItem.IsSelected = !mediaItem.IsSelected;
        UpdateSelectionIndicator();

        _suppressNextTap.Add(mediaItem);
    }

    private void OnRowTapped(object? sender, TappedEventArgs e)
    {
        if (sender is not BindableObject { BindingContext: { } rowItem })
        {
            return;
        }

        var mediaItem = ResolveMediaItem(rowItem);
        if (mediaItem == null)
        {
            return;
        }

        EnsureSelectionOwnership();

        _ = ExecuteShortPressAsync(mediaItem);
    }

    private async Task ExecuteShortPressAsync(MediaItem mediaItem)
    {
        if (_suppressNextTap.Remove(mediaItem))
        {
            return;
        }

        // If at least one row is selected, keep taps in selection mode until all are deselected.
        if (HasAnySelectedItems())
        {
            mediaItem.IsSelected = !mediaItem.IsSelected;
            UpdateSelectionIndicator();
            return;
        }

        // Default mobile behavior: tapping a track directly plays it.
        if (mediaItem is Track track)
        {
            var playbackService = ResolvePlaybackService();
            if (playbackService != null)
            {
                if (PlaybackContextItem is MediaItem parentItem)
                {
                    await playbackService.PlayMediaAsync(new List<MediaItem> { track });
                    return;
                }

                await playbackService.PlayMediaAsync(new List<MediaItem> { track });
                return;
            }
        }

        var command = ShortPressCommand;
        if (command?.CanExecute(mediaItem) == true)
        {
            command.Execute(mediaItem);
        }
    }

    private void OnMoreButtonTapped(object? sender, TappedEventArgs e)
    {
        if (sender is not View anchorView || sender is not BindableObject { BindingContext: { } rowItem })
        {
            return;
        }

        if (ResolveMediaItem(rowItem) == null)
        {
            return;
        }

        var contextMenuCommand = ShowContextMenuAtAnchorCommand;
        if (contextMenuCommand?.CanExecute(anchorView) == true)
        {
            contextMenuCommand.Execute(anchorView);
        }
    }

    private void OnLoadMoreTapped(object? sender, TappedEventArgs e)
    {
        if (ItemsSource == null)
        {
            return;
        }

        AppendVisibleItemsPage();
    }

    private void OnItemsCollectionPanUpdated(object? sender, PanUpdatedEventArgs e)
    {
        ItemsPanUpdated?.Invoke(this, e);
    }

    private void OnItemsCollectionScrolled(object? sender, ItemsViewScrolledEventArgs e)
    {
        _verticalOffset = e.VerticalOffset;
        VerticalOffsetChanged?.Invoke(this, _verticalOffset);
    }

    #endregion

    #region Helpers

    public void SelectAllItems()
    {
        if (ItemsSource == null)
        {
            return;
        }

        foreach (var item in ItemsSource)
        {
            var mediaItem = ResolveMediaItem(item);
            if (mediaItem != null)
            {
                mediaItem.IsSelected = true;
            }
        }

        UpdateSelectionIndicator();
    }

    public void ClearSelection()
    {
        if (ItemsSource == null)
        {
            return;
        }

        foreach (var item in ItemsSource)
        {
            var mediaItem = ResolveMediaItem(item);
            if (mediaItem != null)
            {
                mediaItem.IsSelected = false;
            }
        }

        UpdateSelectionIndicator();
    }

    public void OpenContextMenuForSelection(View anchor)
    {
        if (anchor == null || !HasAnySelectedItems())
        {
            return;
        }

        var contextMenuCommand = ShowContextMenuAtAnchorCommand;
        if (contextMenuCommand?.CanExecute(anchor) == true)
        {
            contextMenuCommand.Execute(anchor);
        }
    }

    private void EnsureSelectionOwnership()
    {
        if (mashin.Services.FocusManager.GetFocusedControl<TableView>(out var focusedTable)
            && focusedTable != null
            && !ReferenceEquals(focusedTable, this)
            && focusedTable.HasSelection)
        {
            focusedTable.ClearSelection();
        }

        if (mashin.Services.FocusManager.GetFocusedControl<RowView>(out var focusedRow)
            && focusedRow != null
            && focusedRow.HasSelection)
        {
            focusedRow.ClearSelection();
        }

        mashin.Services.FocusManager.SetFocus(this);
    }

    private void UpdateSelectionIndicator()
    {
        var hasSelection = HasAnySelectedItems();
        if (_hasSelection != hasSelection)
        {
            _hasSelection = hasSelection;
            OnPropertyChanged(nameof(HasSelection));
        }

        var overlayService = ResolveOverlayService();
        if (overlayService == null)
        {
            return;
        }

        if (hasSelection)
        {
            _ = overlayService.ShowSelectionIndicatorAsync(this);
            return;
        }

        _ = overlayService.HideSelectionIndicatorAsync(this);
    }

    private static IOverlayService? ResolveOverlayService()
    {
        var services = Application.Current?.Handler?.MauiContext?.Services;
        if (services == null)
        {
            return null;
        }

        return services.GetService(typeof(IOverlayService)) as IOverlayService;
    }

    private void UpdateHasMoreItems(bool hasMoreItems)
    {
        if (_hasMoreItems == hasMoreItems)
        {
            return;
        }

        _hasMoreItems = hasMoreItems;
        OnPropertyChanged(nameof(HasMoreItems));
    }

    private static IPlaybackService? ResolvePlaybackService()
    {
        var services = Application.Current?.Handler?.MauiContext?.Services;
        if (services == null)
        {
            return null;
        }

        return services.GetService<IPlaybackService>();
    }

    private bool HasAnySelectedItems()
    {
        if (ItemsSource == null)
        {
            return false;
        }

        return ItemsSource.Select(ResolveMediaItem).Any(mediaItem => mediaItem?.IsSelected == true);
    }

    private static MediaItem? ResolveMediaItem(object? rowItem)
    {
        return rowItem switch
        {
            MediaItem mediaItem => mediaItem,
            QueueItem queueItem => queueItem.MediaItem,
            _ => null
        };
    }

    #region Paging

    private void RefreshVisibleItems()
    {
        if (ItemsSource == null)
        {
            _visibleItems.Clear();
            _loadedItemCount = 0;
            UpdateHasMoreItems(false);
            return;
        }

        if (_loadedItemCount <= 0)
        {
            _loadedItemCount = InitialLoadCount > 0
                ? InitialLoadCount
                : DefaultInitialLoadCount;
        }

        var requestedCount = Math.Max(0, _loadedItemCount);
        var nextItems = TakePrefixItems(ItemsSource, requestedCount + 1);

        if (nextItems.Count == 0)
        {
            _visibleItems.Clear();
            _loadedItemCount = 0;
            UpdateHasMoreItems(false);
            return;
        }

        var hasMoreItems = nextItems.Count > requestedCount;
        if (hasMoreItems)
        {
            nextItems.RemoveAt(nextItems.Count - 1);
        }

        _visibleItems.ReplaceRange(nextItems);
        UpdateHasMoreItems(hasMoreItems);
    }

    private void AppendVisibleItemsPage()
    {
        if (ItemsSource == null)
        {
            return;
        }

        _loadedItemCount = Math.Max(0, _loadedItemCount) + LoadMoreCount;
        RefreshVisibleItems();
    }

    private static List<object> TakePrefixItems(IEnumerable<object> source, int maxCount)
    {
        var prefix = new List<object>();
        if (maxCount <= 0)
        {
            return prefix;
        }

        if (source is IList<object> list)
        {
            var count = Math.Min(list.Count, maxCount);
            for (var index = 0; index < count; index++)
            {
                prefix.Add(list[index]);
            }

            return prefix;
        }

        using var enumerator = source.GetEnumerator();
        while (prefix.Count < maxCount && enumerator.MoveNext())
        {
            prefix.Add(enumerator.Current);
        }

        return prefix;
    }

    #endregion

    protected override void OnBindingContextChanged()
    {
        base.OnBindingContextChanged();
        RefreshVisibleItems();
        UpdateSelectionIndicator();
    }

    protected override void OnHandlerChanging(HandlerChangingEventArgs args)
    {
        if (args.NewHandler == null)
        {
            DetachSelectionObservers();
            UpdateSelectionIndicator();
        }

        base.OnHandlerChanging(args);
    }

    #endregion
}

public sealed class MobileTableViewTemplateSelector : DataTemplateSelector
{
    public DataTemplate? PlaylistTemplate { get; set; }
    public DataTemplate? QueueItemTemplate { get; set; }
    public DataTemplate? TrackTemplate { get; set; }
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

        if (item is QueueItem && QueueItemTemplate != null)
        {
            return QueueItemTemplate;
        }

        if (item is Track && TrackTemplate != null)
        {
            return TrackTemplate;
        }

        if (PlaylistTemplate != null)
        {
            return PlaylistTemplate;
        }

        if (TrackTemplate != null)
        {
            return TrackTemplate;
        }

        if (QueueItemTemplate != null)
        {
            return QueueItemTemplate;
        }

        if (SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        throw new InvalidOperationException("MobileTableViewTemplateSelector requires PlaylistTemplate, QueueItemTemplate, TrackTemplate, or SkeletonTemplate.");
    }
}
