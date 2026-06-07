using mashin.Collections;
using mashin.Models;
using mashin.Services;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Linq;
using System.Windows.Input;

namespace mashin.Views.Mobile.Controls;

public partial class TableView : ContentView
{
    #region Fields

    private const int InitialLoadCount = 15;
    private const int LoadMoreCount = 10;

    private readonly HashSet<MediaItem> _suppressNextTap = new();
    private readonly HashSet<INotifyPropertyChanged> _observedItems = new();
    private readonly ObservableRangeCollection<object> _visibleItems = new();
    private INotifyCollectionChanged? _observedCollection;
    private int _loadedItemCount = InitialLoadCount;
    private bool _hasMoreItems;
    private bool _hasSelection;

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

    public bool HasMoreItems => _hasMoreItems;

    public IReadOnlyList<object> VisibleItems => _visibleItems;

    public bool HasSelection => _hasSelection;

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
            tableView._loadedItemCount = InitialLoadCount;
        }

        tableView.DetachSelectionObservers();
        tableView.AttachSelectionObservers(newValue as IEnumerable<object>);
        tableView.RefreshVisibleItems();
        tableView.UpdateSelectionIndicator();
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

        foreach (var item in items)
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
        DetachSelectionObservers();
        AttachSelectionObservers(ItemsSource);

        RefreshVisibleItems();
        UpdateSelectionIndicator();
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
            var mediaActions = ResolveMediaActions();
            if (mediaActions != null)
            {
                if (PlaybackContextItem is MediaItem parentItem)
                {
                    await mediaActions.PlayMediaAsync(parentItem, track);
                    return;
                }

                await mediaActions.PlayMediaAsync(track);
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

    private static IMediaItemActions? ResolveMediaActions()
    {
        var services = Application.Current?.Handler?.MauiContext?.Services;
        if (services == null)
        {
            return null;
        }

        return services.GetService(typeof(IMediaItemActions)) as IMediaItemActions;
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

        var sourceItems = ItemsSource.ToList();
        var totalCount = sourceItems.Count;
        if (totalCount == 0)
        {
            _visibleItems.Clear();
            _loadedItemCount = 0;
            UpdateHasMoreItems(false);
            return;
        }

        var visibleCount = Math.Min(_loadedItemCount, totalCount);
        _visibleItems.ReplaceRange(sourceItems.Take(visibleCount));
        _loadedItemCount = visibleCount;
        UpdateHasMoreItems(visibleCount < totalCount);
    }

    private void AppendVisibleItemsPage()
    {
        if (ItemsSource == null)
        {
            return;
        }

        var sourceItems = ItemsSource.ToList();
        var totalCount = sourceItems.Count;
        if (totalCount == 0)
        {
            _visibleItems.Clear();
            _loadedItemCount = 0;
            UpdateHasMoreItems(false);
            return;
        }

        if (_visibleItems.Count > totalCount)
        {
            RefreshVisibleItems();
            return;
        }

        var currentCount = _visibleItems.Count;
        var nextCount = Math.Min(currentCount + LoadMoreCount, totalCount);
        if (nextCount > currentCount)
        {
            _visibleItems.AddRange(sourceItems.Skip(currentCount).Take(nextCount - currentCount));
        }

        _loadedItemCount = nextCount;
        UpdateHasMoreItems(nextCount < totalCount);
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
