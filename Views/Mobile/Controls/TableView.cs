using mashin.Collections;
using mashin.Models;
using mashin.Services;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Maui.ApplicationModel;
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
    private const int DefaultLoadMoreCount = 25;

    private readonly HashSet<MediaItem> _suppressNextTap = new();
    private readonly HashSet<INotifyPropertyChanged> _observedItems = new();
    private readonly ObservableRangeCollection<object> _visibleItems = new();
    private INotifyCollectionChanged? _observedCollection;
    private IEnumerable<object>? _observedItemsSource;
    private PlaybackService? _playbackService;
    private QueueItem? _currentQueueItem;
    private Track? _currentTrackMediaItem;
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

    public static readonly BindableProperty CurrentTrackUriProperty =
        BindableProperty.Create(nameof(CurrentTrackUri), typeof(string), typeof(TableView));

    public static readonly BindableProperty CurrentPlayStateProperty =
        BindableProperty.Create(
            nameof(CurrentPlayState),
            typeof(PlayerState),
            typeof(TableView),
            defaultValue: new PlayerState
            {
                State = PlayerStateType.Idle,
                ActiveSinceUtc = DateTimeOffset.UtcNow
            });

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

    public static readonly BindableProperty LoadMoreCountProperty =
        BindableProperty.Create(
            nameof(LoadMoreCount),
            typeof(int),
            typeof(TableView),
            DefaultLoadMoreCount);

    #endregion

    #region Construction

    public TableView()
    {
        InitializeComponent();
    }

    protected override void OnHandlerChanging(HandlerChangingEventArgs args)
    {
        if (args.NewHandler == null)
        {
            DetachPlaybackStateSource();
            DetachSelectionObservers();
        }

        base.OnHandlerChanging(args);
    }

    protected override void OnHandlerChanged()
    {
        base.OnHandlerChanged();
    }

    private void OnTableViewLoaded(object? sender, EventArgs e)
    {
        SyncSelectionObservers(ItemsSource);
        AttachPlaybackStateSource();
        RefreshVisibleItems();
        UpdateSelectionIndicator();
        UpdateFavoriteStateForVisibleItems();
    }

    private void OnTableViewUnloaded(object? sender, EventArgs e)
    {
        // Android may raise Unloaded for transient layout changes.
        // Keep observers/state intact; cleanup is done when handler is detached.
        _suppressNextTap.Clear();

        // Hide selection indicator when this view is not active.
        _ = ResolveOverlayService()?.HideSelectionIndicatorAsync(this);
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

    public string? CurrentTrackUri
    {
        get => (string?)GetValue(CurrentTrackUriProperty);
        private set => SetValue(CurrentTrackUriProperty, value);
    }

    public PlayerState CurrentPlayState
    {
        get => (PlayerState)GetValue(CurrentPlayStateProperty);
        private set => SetValue(CurrentPlayStateProperty, value);
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

    public int LoadMoreCount
    {
        get => (int)GetValue(LoadMoreCountProperty);
        set => SetValue(LoadMoreCountProperty, value);
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
        _observedItemsSource = items;

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
        _observedItemsSource = null;

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

    private void SyncSelectionObservers(IEnumerable<object>? items)
    {
        if (ReferenceEquals(_observedItemsSource, items))
        {
            return;
        }

        DetachSelectionObservers();
        AttachSelectionObservers(items);
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
            if (_playbackService == null)
            {
                return;
            }

            var playbackService = _playbackService;
            if (playbackService != null)
            {
                var itemIndex = GetIndexOf(mediaItem);

                // if queue, play the queue index via playback service
                if (PlaybackContextItem is IEnumerable<QueueItem> && itemIndex is >= 0)
                {
                    await playbackService.PlayQueueIndexAsync(itemIndex.Value);
                    return;
                }

                // otherwise, play current and following items
                if (itemIndex is >= 0)
                {
                    var playItems = GetPlayableItemsFromIndex(itemIndex.Value);
                    if (playItems.Count > 0)
                    {
                        await playbackService.PlayMediaAsync(playItems);
                        return;
                    }
                }
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
        foreach (var item in EnumerateSelectableItems())
        {
            item.IsSelected = true;
        }

        UpdateSelectionIndicator();
    }

    public void ClearSelection()
    {
        foreach (var item in EnumerateSelectableItems())
        {
            item.IsSelected = false;
        }

        UpdateSelectionIndicator();
    }

    public void ResetVisibleItemsToInitialCount()
    {
        _loadedItemCount = GetConfiguredInitialLoadCount();
        RefreshVisibleItems();
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

    private void AttachPlaybackStateSource()
    {
        if (_playbackService != null)
        {
            return;
        }

        var mauiContext = Handler?.MauiContext
            ?? Microsoft.Maui.Controls.Application.Current?.Handler?.MauiContext;
        if (mauiContext == null)
        {
            return;
        }

        _playbackService = mauiContext.Services.GetService<PlaybackService>();
        if (_playbackService == null)
        {
            return;
        }

        _currentQueueItem = _playbackService.CurrentQueueItem;
        _currentTrackMediaItem = _currentQueueItem?.MediaItem;
        SetCurrentTrackUri(_currentTrackMediaItem?.Uri);
        SetCurrentPlayState(_playbackService.PlaybackState);

        _playbackService.PropertyChanged += OnPlaybackServicePropertyChanged;

        if (_currentTrackMediaItem != null)
        {
            _currentTrackMediaItem.PropertyChanged += OnCurrentTrackPropertyChanged;
        }
    }

    private void DetachPlaybackStateSource()
    {
        if (_playbackService != null)
        {
            _playbackService.PropertyChanged -= OnPlaybackServicePropertyChanged;
        }

        if (_currentTrackMediaItem != null)
        {
            _currentTrackMediaItem.PropertyChanged -= OnCurrentTrackPropertyChanged;
        }

        _playbackService = null;
        _currentQueueItem = null;
        _currentTrackMediaItem = null;
        SetCurrentTrackUri(null);
        SetCurrentPlayState(new PlayerState
        {
            State = PlayerStateType.Idle,
            ActiveSinceUtc = DateTimeOffset.UtcNow
        });
    }

    private void OnPlaybackServicePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(PlaybackService.PlaybackState))
        {
            var currentPlayerState = _playbackService?.PlaybackState
                ?? new PlayerState { State = PlayerStateType.Idle, ActiveSinceUtc = DateTimeOffset.UtcNow };

            if (MainThread.IsMainThread)
            {
                SetCurrentPlayState(currentPlayerState);
            }
            else
            {
                MainThread.BeginInvokeOnMainThread(() => SetCurrentPlayState(currentPlayerState));
            }

            return;
        }

        if (e.PropertyName != nameof(PlaybackService.CurrentQueueItem))
        {
            return;
        }

        OnCurrentTrackUpdated();
    }

    private void OnCurrentTrackUpdated()
    {
        if (_currentTrackMediaItem != null)
        {
            _currentTrackMediaItem.PropertyChanged -= OnCurrentTrackPropertyChanged;
        }

        _currentQueueItem = _playbackService?.CurrentQueueItem;
        _currentTrackMediaItem = _currentQueueItem?.MediaItem;
        var currentTrackUri = _currentTrackMediaItem?.Uri;

        if (_currentTrackMediaItem != null)
        {
            _currentTrackMediaItem.PropertyChanged += OnCurrentTrackPropertyChanged;
        }

        if (MainThread.IsMainThread)
        {
            SetCurrentTrackUri(currentTrackUri);
            UpdateFavoriteStateForVisibleItems();
            return;
        }

        MainThread.BeginInvokeOnMainThread(() =>
        {
            SetCurrentTrackUri(currentTrackUri);
            UpdateFavoriteStateForVisibleItems();
        });
    }

    private void OnCurrentTrackPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName != nameof(MediaItem.Favorite))
        {
            return;
        }

        if (MainThread.IsMainThread)
        {
            UpdateFavoriteStateForVisibleItems();
            return;
        }

        MainThread.BeginInvokeOnMainThread(UpdateFavoriteStateForVisibleItems);
    }

    private void UpdateFavoriteStateForVisibleItems()
    {
        var activeTrack = _currentTrackMediaItem;
        if (activeTrack == null)
        {
            return;
        }

        foreach (var item in EnumerateSelectableItems())
        {
            if (!IsSameTrack(item, activeTrack) || item.Favorite == activeTrack.Favorite)
            {
                continue;
            }

            item.Favorite = activeTrack.Favorite;
        }
    }

    private static bool IsSameTrack(MediaItem item, Track activeTrack)
    {
        if (!string.IsNullOrWhiteSpace(item.Uri)
            && !string.IsNullOrWhiteSpace(activeTrack.Uri)
            && string.Equals(item.Uri, activeTrack.Uri, StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        if (!string.IsNullOrWhiteSpace(item.ItemId)
            && !string.IsNullOrWhiteSpace(activeTrack.ItemId)
            && string.Equals(item.ItemId, activeTrack.ItemId, StringComparison.Ordinal))
        {
            return true;
        }

        return false;
    }

    private void SetCurrentTrackUri(string? uri)
    {
        if (string.Equals(CurrentTrackUri, uri, StringComparison.OrdinalIgnoreCase))
        {
            return;
        }

        CurrentTrackUri = uri;
    }

    private void SetCurrentPlayState(PlayerState playerState)
    {
        if (CurrentPlayState.State == playerState.State)
        {
            return;
        }

        CurrentPlayState = playerState;
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

    private bool HasAnySelectedItems()
    {
        return EnumerateSelectableItems().Any(mediaItem => mediaItem.IsSelected);
    }

    private int? GetIndexOf(MediaItem target)
    {
        var index = 0;
        foreach (var item in EnumerateSelectableItems())
        {
            if (ReferenceEquals(item, target))
            {
                return index;
            }

            index++;
        }

        return null;
    }

    private List<MediaItem> GetPlayableItemsFromIndex(int startIndex)
    {
        if (startIndex < 0)
        {
            return new List<MediaItem>();
        }

        return EnumerateSelectableItems()
            .Skip(startIndex)
            .ToList();
    }

    private IEnumerable<MediaItem> EnumerateSelectableItems()
    {
        if (ItemsSource == null)
        {
            yield break;
        }

        foreach (var sourceItem in ItemsSource)
        {
            var mediaItem = ResolveMediaItem(sourceItem);
            if (mediaItem != null)
            {
                yield return mediaItem;
            }
        }
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
            if (_visibleItems.Count > 0)
            {
                _visibleItems.Clear();
            }

            _loadedItemCount = 0;
            UpdateHasMoreItems(false);
            return;
        }

        var configuredInitialLoadCount = GetConfiguredInitialLoadCount();
        if (_loadedItemCount <= 0)
        {
            _loadedItemCount = configuredInitialLoadCount;
        }
        else if (_loadedItemCount < configuredInitialLoadCount)
        {
            // Keep the paging target stable so temporary short lists do not shrink future refreshes.
            _loadedItemCount = configuredInitialLoadCount;
        }

        var requestedCount = Math.Max(0, _loadedItemCount);
        var nextItems = TakePrefixItems(ItemsSource, requestedCount + 1);

        if (nextItems.Count == 0)
        {
            if (_visibleItems.Count > 0)
            {
                _visibleItems.Clear();
            }

            _loadedItemCount = 0;
            UpdateHasMoreItems(false);
            return;
        }

        var hasMoreItems = nextItems.Count > requestedCount;
        if (hasMoreItems)
        {
            nextItems.RemoveAt(nextItems.Count - 1);
        }

        ApplyVisibleItemsSnapshot(nextItems);

        _loadedItemCount = requestedCount;
        UpdateHasMoreItems(hasMoreItems);
    }

    private void AppendVisibleItemsPage()
    {
        if (ItemsSource == null)
        {
            return;
        }

        var currentCount = _visibleItems.Count;
        var configuredInitialLoadCount = GetConfiguredInitialLoadCount();
        var requestedCount = Math.Max(Math.Max(configuredInitialLoadCount, _loadedItemCount), currentCount) + GetConfiguredLoadMoreCount();
        var nextItems = TakePrefixItems(ItemsSource, requestedCount + 1);

        if (nextItems.Count == 0)
        {
            if (_visibleItems.Count > 0)
            {
                _visibleItems.Clear();
            }

            _loadedItemCount = 0;
            UpdateHasMoreItems(false);
            return;
        }

        var hasMoreItems = nextItems.Count > requestedCount;
        if (hasMoreItems)
        {
            nextItems.RemoveAt(nextItems.Count - 1);
        }

        ApplyVisibleItemsSnapshot(nextItems);

        _loadedItemCount = requestedCount;
        UpdateHasMoreItems(hasMoreItems);
    }

    private int GetConfiguredInitialLoadCount()
    {
        return InitialLoadCount > 0
            ? InitialLoadCount
            : DefaultInitialLoadCount;
    }

    private int GetConfiguredLoadMoreCount()
    {
        return LoadMoreCount > 0
            ? LoadMoreCount
            : DefaultLoadMoreCount;
    }

    private void ApplyVisibleItemsSnapshot(List<object> nextItems)
    {
        var currentCount = _visibleItems.Count;
        var nextCount = nextItems.Count;

        if (currentCount == nextCount)
        {
            for (var index = 0; index < nextCount; index++)
            {
                if (!ReferenceEquals(_visibleItems[index], nextItems[index]))
                {
                    _visibleItems.ReplaceRange(nextItems);
                    return;
                }
            }

            return;
        }

        if (currentCount < nextCount)
        {
            for (var index = 0; index < currentCount; index++)
            {
                if (!ReferenceEquals(_visibleItems[index], nextItems[index]))
                {
                    _visibleItems.ReplaceRange(nextItems);
                    return;
                }
            }

            var itemsToAppend = new List<object>(nextCount - currentCount);
            for (var index = currentCount; index < nextCount; index++)
            {
                itemsToAppend.Add(nextItems[index]);
            }

            if (itemsToAppend.Count > 0)
            {
                _visibleItems.AddRange(itemsToAppend);
            }

            return;
        }

        for (var index = 0; index < nextCount; index++)
        {
            if (!ReferenceEquals(_visibleItems[index], nextItems[index]))
            {
                _visibleItems.ReplaceRange(nextItems);
                return;
            }
        }

        // Shrink by removing tail items only to avoid a full list rebind/reset.
        for (var index = currentCount - 1; index >= nextCount; index--)
        {
            _visibleItems.RemoveAt(index);
        }
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

