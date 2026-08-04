using mashin.Collections;
using mashin.Models;
using mashin.Services;
using Microsoft.Maui.ApplicationModel;
using System.Collections;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Linq;
using System.Windows.Input;

namespace mashin.Views.Desktop.Controls;

public partial class TableView : ContentView
{
    private static readonly BindableProperty LastAnimatedContextProperty =
        BindableProperty.CreateAttached("LastAnimatedContext", typeof(object), typeof(TableView), defaultValue: null);

    #region Bindable properties

    public static readonly BindableProperty ItemsSourceProperty =
        BindableProperty.Create(nameof(ItemsSource), typeof(IEnumerable<object>), typeof(TableView), propertyChanged: OnItemsSourceChanged);

    public static readonly BindableProperty SelectedItemsProperty =
    BindableProperty.Create(nameof(SelectedItems), typeof(IList), typeof(TableView), defaultBindingMode: BindingMode.TwoWay);

    public static readonly BindableProperty IsAllSelectedProperty =
        BindableProperty.Create(nameof(IsAllSelected), typeof(bool), typeof(TableView), defaultValue: false);

    public static readonly BindableProperty PrimaryInfoTappedCommandProperty =
        BindableProperty.Create(nameof(PrimaryInfoTappedCommand), typeof(ICommand), typeof(TableView));

    public static readonly BindableProperty SecondaryInfoTappedCommandProperty =
        BindableProperty.Create(nameof(SecondaryInfoTappedCommand), typeof(ICommand), typeof(TableView));

    public static readonly BindableProperty UserDataServiceProperty =
        BindableProperty.Create(nameof(UserDataService), typeof(UserDataService), typeof(TableView));

    public static readonly BindableProperty ShowContextMenuAtAnchorCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtAnchorCommand), typeof(ICommand), typeof(TableView));

    public static readonly BindableProperty ShowContextMenuAtPositionCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtPositionCommand), typeof(ICommand), typeof(TableView));

    public static readonly BindableProperty PlaybackContextItemProperty =
        BindableProperty.Create(nameof(PlaybackContextItem), typeof(object), typeof(TableView), propertyChanged: OnPlaybackContextItemChanged);

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

    public static readonly BindableProperty PageSizeProperty =
        BindableProperty.Create(nameof(PageSize), typeof(int), typeof(TableView), defaultValue: 10, propertyChanged: OnPageSizeChanged);

    public static readonly BindableProperty CanGoPrevProperty =
        BindableProperty.Create(nameof(CanGoPrev), typeof(bool), typeof(TableView), false);

    public static readonly BindableProperty CanGoNextProperty =
        BindableProperty.Create(nameof(CanGoNext), typeof(bool), typeof(TableView), false);

    public static readonly BindableProperty CanToggleExpandProperty =
        BindableProperty.Create(nameof(CanToggleExpand), typeof(bool), typeof(TableView), false);

    public static readonly BindableProperty IsExpandedProperty =
        BindableProperty.Create(nameof(IsExpanded), typeof(bool), typeof(TableView), false);

    public static readonly BindableProperty CurrentPageProperty =
        BindableProperty.Create(nameof(CurrentPage), typeof(int), typeof(TableView), 1);

    public static readonly BindableProperty TotalPagesProperty =
        BindableProperty.Create(nameof(TotalPages), typeof(int), typeof(TableView), 1);

    #endregion

    #region Fields

    private readonly ObservableRangeCollection<object> _selectedItems = new();
    private readonly ObservableRangeCollection<object> _headerItems = new();
    private readonly ObservableRangeCollection<object> _visibleItems = new();
    private INotifyCollectionChanged? _itemsSourceCollection;
    private List<object> _allItems = new();
    private IKeyboardService? _keyboardService;
    private PlaybackService? _playbackService;
    private QueueItem? _currentQueueItem;
    private Track? _currentTrackMediaItem;
    private int? _anchorIndex;
    private int _pageIndex;
    private bool _isExpanded;
    private bool _isCheckboxClick;
    private bool _isUnloaded;

    #endregion

    #region Properties

    public IEnumerable<object>? ItemsSource
    {
        get => (IEnumerable<object>?)GetValue(ItemsSourceProperty);
        set => SetValue(ItemsSourceProperty, value);
    }

    public IList SelectedItems
    {
        get => (IList)GetValue(SelectedItemsProperty);
        private set => SetValue(SelectedItemsProperty, value);
    }

    public bool IsAllSelected
    {
        get => (bool)GetValue(IsAllSelectedProperty);
        private set => SetValue(IsAllSelectedProperty, value);
    }

    public ICommand? PrimaryInfoTappedCommand
    {
        get => (ICommand?)GetValue(PrimaryInfoTappedCommandProperty);
        set => SetValue(PrimaryInfoTappedCommandProperty, value);
    }

    public ICommand? SecondaryInfoTappedCommand
    {
        get => (ICommand?)GetValue(SecondaryInfoTappedCommandProperty);
        set => SetValue(SecondaryInfoTappedCommandProperty, value);
    }

    public UserDataService? UserDataService
    {
        get => (UserDataService?)GetValue(UserDataServiceProperty);
        set => SetValue(UserDataServiceProperty, value);
    }

    public ICommand? ShowContextMenuAtAnchorCommand
    {
        get => (ICommand?)GetValue(ShowContextMenuAtAnchorCommandProperty);
        set => SetValue(ShowContextMenuAtAnchorCommandProperty, value);
    }

    public ICommand? ShowContextMenuAtPositionCommand
    {
        get => (ICommand?)GetValue(ShowContextMenuAtPositionCommandProperty);
        set => SetValue(ShowContextMenuAtPositionCommandProperty, value);
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

    public ObservableRangeCollection<object> HeaderItems => _headerItems;

    public ObservableRangeCollection<object> VisibleItems => _visibleItems;

    public int PageSize
    {
        get => (int)GetValue(PageSizeProperty);
        set => SetValue(PageSizeProperty, value);
    }

    public bool CanGoPrev
    {
        get => (bool)GetValue(CanGoPrevProperty);
        private set => SetValue(CanGoPrevProperty, value);
    }

    public bool CanGoNext
    {
        get => (bool)GetValue(CanGoNextProperty);
        private set => SetValue(CanGoNextProperty, value);
    }

    public bool CanToggleExpand
    {
        get => (bool)GetValue(CanToggleExpandProperty);
        private set => SetValue(CanToggleExpandProperty, value);
    }

    public bool IsExpanded
    {
        get => (bool)GetValue(IsExpandedProperty);
        private set => SetValue(IsExpandedProperty, value);
    }

    public int CurrentPage
    {
        get => (int)GetValue(CurrentPageProperty);
        private set => SetValue(CurrentPageProperty, value);
    }

    public int TotalPages
    {
        get => (int)GetValue(TotalPagesProperty);
        private set => SetValue(TotalPagesProperty, value);
    }

    public ICommand PrevPageCommand { get; }
    public ICommand NextPageCommand { get; }
    public ICommand ToggleExpandCommand { get; }

    private bool HasExpandableItems => PageSize > 0 && _allItems.Count > PageSize;

    private int ComputedTotalPages => PageSize <= 0
        ? 1
        : Math.Max(1, (int)Math.Ceiling((double)_allItems.Count / PageSize));

    private bool CanExpandFromCurrentView => !_isExpanded && _allItems.Count > _visibleItems.Count;

    #endregion

    #region Construction

    public TableView()
    {
        InitializeComponent();

        PrevPageCommand = new Command(async () => await GoToPreviousPageAsync(), () => CanGoPrev);
        NextPageCommand = new Command(async () => await GoToNextPageAsync(), () => CanGoNext);
        ToggleExpandCommand = new Command(async () => await ToggleExpandedAsync(), () => CanToggleExpand);

        SelectedItems = _selectedItems;
        UpdateHeaderItems(PlaybackContextItem);
        SyncVisibleItems();
        UpdateHeaderSelectionState();
        UpdateNavigationState();
    }

    protected override void OnHandlerChanged()
    {
        base.OnHandlerChanged();
    }

    private void OnTableViewLoaded(object? sender, EventArgs e)
    {
        _isUnloaded = false;

        // KeyboardService
        if (_keyboardService == null)
        {
            var mauiContext = Handler?.MauiContext
                ?? Microsoft.Maui.Controls.Application.Current?.Handler?.MauiContext;

            _keyboardService = mauiContext?.Services.GetService<IKeyboardService>();

            if (_keyboardService != null)
            {
                _keyboardService.KeyActionDetected += OnKeyActionDetected;
            }
        }

        AttachPlaybackStateSource();
        UpdateFavoriteStateForVisibleItems();

    }

    private void OnTableViewUnloaded(object? sender, EventArgs e)
    {
        _isUnloaded = true;
        
        // Cleanup
        if (_keyboardService != null)
        {
            _keyboardService.KeyActionDetected -= OnKeyActionDetected;
            _keyboardService = null;
        }

        DetachPlaybackStateSource();
        DetachItemsSourceCollection();

        PrimaryInfoTappedCommand = null;
        SecondaryInfoTappedCommand = null;
        ShowContextMenuAtAnchorCommand = null;
        ShowContextMenuAtPositionCommand = null;
        UserDataService = null;
        PlaybackContextItem = null;

        if (collectionView != null)
        {
            collectionView.ItemsSource = null;
            collectionView.BindingContext = null;
        }

        ItemsSource = null;
        BindingContext = null;

        // Clear collections
        _selectedItems.Clear();
        _visibleItems.Clear();
        _allItems.Clear();

    }

    #endregion

    #region Property Changed Handlers

    private static void OnItemsSourceChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not TableView view)
        {
            return;
        }

        if (ReferenceEquals(oldValue, newValue))
        {
            return;
        }

        view.ApplyItemsSource(newValue as IEnumerable<object>);
    }

    private static void OnPlaybackContextItemChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not TableView view)
        {
            return;
        }

        view.UpdateHeaderItems(newValue);
    }

    private static void OnPageSizeChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not TableView view)
        {
            return;
        }

        view._pageIndex = 0;
        view.SyncVisibleItems();
        view.UpdateNavigationState();
    }

    private void ApplyItemsSource(IEnumerable<object>? items)
    {
        AttachItemsSourceCollection(items);

        _allItems = items?.ToList() ?? new List<object>();
        _pageIndex = 0;
        _isExpanded = false;
        _anchorIndex = null;

        ClearAllSelections();

        SyncVisibleItems();
        SyncSelectionAndHeaderState(clickedItem: null);
        UpdateNavigationState();
        UpdateFavoriteStateForVisibleItems();
    }

    private void AttachItemsSourceCollection(IEnumerable<object>? items)
    {
        DetachItemsSourceCollection();

        if (items is INotifyCollectionChanged notifyCollection)
        {
            _itemsSourceCollection = notifyCollection;
            _itemsSourceCollection.CollectionChanged += OnItemsSourceCollectionChanged;
        }
    }

    private void DetachItemsSourceCollection()
    {
        if (_itemsSourceCollection != null)
        {
            _itemsSourceCollection.CollectionChanged -= OnItemsSourceCollectionChanged;
            _itemsSourceCollection = null;
        }
    }

    private void OnItemsSourceCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        if (!MainThread.IsMainThread)
        {
            MainThread.BeginInvokeOnMainThread(() => OnItemsSourceCollectionChanged(sender, e));
            return;
        }

        // Prefer incremental updates to preserve row containers and avoid unnecessary image rebinds.
        if (TryApplyIncrementalCollectionChange(e))
        {
            SyncSelectionAndHeaderState(clickedItem: null);
            UpdateNavigationState();
            UpdateFavoriteStateForVisibleItems();
            return;
        }

        _allItems = ItemsSource?.ToList() ?? new List<object>();

        if (_pageIndex >= ComputedTotalPages)
        {
            _pageIndex = Math.Max(0, ComputedTotalPages - 1);
        }

        SyncVisibleItems();
        SyncSelectionAndHeaderState(clickedItem: null);
        UpdateNavigationState();
        UpdateFavoriteStateForVisibleItems();
    }

    private bool TryApplyIncrementalCollectionChange(NotifyCollectionChangedEventArgs e)
    {
        // Incremental updates are only safe when the visible list mirrors all items.
        if (!IsShowingAllItems())
        {
            return false;
        }

        switch (e.Action)
        {
            case NotifyCollectionChangedAction.Add:
                if (e.NewItems == null || e.NewItems.Count == 0)
                {
                    return true;
                }

                // Insert at source index so virtualization can move only affected rows.
                var addIndex = e.NewStartingIndex >= 0 ? e.NewStartingIndex : _allItems.Count;
                addIndex = Math.Clamp(addIndex, 0, _allItems.Count);

                for (var i = 0; i < e.NewItems.Count; i++)
                {
                    var item = e.NewItems[i];
                    _allItems.Insert(addIndex + i, item!);
                    _visibleItems.Insert(Math.Clamp(addIndex + i, 0, _visibleItems.Count), item!);
                }

                return true;

            case NotifyCollectionChangedAction.Remove:
                if (e.OldItems == null || e.OldItems.Count == 0)
                {
                    return true;
                }

                // Remove by event index if available; otherwise fallback to identity-based removal.
                if (e.OldStartingIndex >= 0)
                {
                    for (var i = 0; i < e.OldItems.Count; i++)
                    {
                        if (e.OldStartingIndex < _allItems.Count)
                        {
                            _allItems.RemoveAt(e.OldStartingIndex);
                        }

                        if (e.OldStartingIndex < _visibleItems.Count)
                        {
                            _visibleItems.RemoveAt(e.OldStartingIndex);
                        }
                    }
                }
                else
                {
                    foreach (var oldItem in e.OldItems)
                    {
                        _allItems.Remove(oldItem!);
                        _visibleItems.Remove(oldItem!);
                    }
                }

                return true;

            case NotifyCollectionChangedAction.Move:
                if (e.OldStartingIndex < 0 || e.NewStartingIndex < 0 || e.OldItems == null || e.OldItems.Count == 0)
                {
                    return false;
                }

                // Single-item move is the common queue reorder case and keeps UI churn minimal.
                if (e.OldItems.Count == 1)
                {
                    if (e.OldStartingIndex < _allItems.Count && e.NewStartingIndex < _allItems.Count)
                    {
                        var moved = _allItems[e.OldStartingIndex];
                        _allItems.RemoveAt(e.OldStartingIndex);
                        _allItems.Insert(Math.Clamp(e.NewStartingIndex, 0, _allItems.Count), moved);
                    }

                    if (e.OldStartingIndex < _visibleItems.Count && e.NewStartingIndex < _visibleItems.Count)
                    {
                        _visibleItems.Move(e.OldStartingIndex, e.NewStartingIndex);
                    }

                    return true;
                }

                return false;

            case NotifyCollectionChangedAction.Replace:
                if (e.NewItems == null || e.NewItems.Count == 0)
                {
                    return true;
                }

                // Replace in place to preserve list shape and selection state.
                var replaceIndex = e.NewStartingIndex;
                if (replaceIndex < 0)
                {
                    return false;
                }

                for (var i = 0; i < e.NewItems.Count; i++)
                {
                    var index = replaceIndex + i;
                    if (index < 0 || index >= _allItems.Count || index >= _visibleItems.Count)
                    {
                        return false;
                    }

                    _allItems[index] = e.NewItems[i]!;
                    _visibleItems[index] = e.NewItems[i]!;
                }

                return true;

            case NotifyCollectionChangedAction.Reset:
            default:
                // Reset or unsupported patterns fall back to full resync in caller.
                return false;
        }
    }

    private bool IsShowingAllItems()
    {
        return _isExpanded || PageSize <= 0 || _allItems.Count <= PageSize;
    }

    #endregion

    #region Paging & Expansion

    public async Task GoToPreviousPageAsync()
    {
        if (_isExpanded || _pageIndex <= 0)
        {
            return;
        }

        _pageIndex--;
        SyncVisibleItems();
        UpdateNavigationState();

        await Task.CompletedTask;
    }

    public async Task GoToNextPageAsync()
    {
        if (_isExpanded || _pageIndex >= ComputedTotalPages - 1)
        {
            return;
        }

        _pageIndex++;
        SyncVisibleItems();
        UpdateNavigationState();

        await Task.CompletedTask;
    }

    public async Task ToggleExpandedAsync()
    {
        if (!_isExpanded && !CanExpandFromCurrentView)
        {
            return;
        }

        _isExpanded = !_isExpanded;

        if (_isExpanded)
        {
            // Expand dynamically in page-sized chunks from top to bottom.
            // If page 0 is currently shown and still in sync, keep it and append the missing pages.
            if (_pageIndex == 0
                && PageSize > 0
                && _visibleItems.Count == Math.Min(PageSize, _allItems.Count)
                && _visibleItems.SequenceEqual(_allItems.Take(Math.Min(PageSize, _allItems.Count))))
            {
                await AppendExpandedPagesAsync(startPageIndex: 1);
            }
            else
            {
                // Different current page: start from page 0 so expanded mode contains the full ordered list.
                if (PageSize <= 0)
                {
                    SyncVisibleItems();
                }
                else
                {
                    _visibleItems.ReplaceRange(_allItems.Take(PageSize));
                    await AppendExpandedPagesAsync(startPageIndex: 1);
                }
            }
        }
        else
        {
            // Collapse: return to first page.
            _pageIndex = 0;
            SyncVisibleItems();
        }

        UpdateNavigationState();
    }

    private async Task AppendExpandedPagesAsync(int startPageIndex)
    {
        if (PageSize <= 0)
        {
            return;
        }

        for (var pageIndex = startPageIndex; pageIndex < ComputedTotalPages; pageIndex++)
        {
            if (!_isExpanded)
            {
                return;
            }

            // Append exactly one page per step (last page may be smaller naturally).
            var pageItems = _allItems.Skip(pageIndex * PageSize).Take(PageSize).ToList();
            if (pageItems.Count > 0)
            {
                _visibleItems.AddRange(pageItems);
            }

            await Task.Delay(50);
        }
    }

    private void SyncVisibleItems()
    {
        IEnumerable<object> source;

        if (_allItems.Count == 0)
        {
            source = Array.Empty<object>();
        }
        else if (_isExpanded || PageSize <= 0)
        {
            source = _allItems;
        }
        else
        {
            var start = _pageIndex * PageSize;
            source = _allItems.Skip(start).Take(PageSize);
        }

        _visibleItems.ReplaceRange(source);
    }

    private void UpdateNavigationState()
    {
        var canGoPrev = !_isExpanded && _pageIndex > 0;
        var canGoNext = !_isExpanded && _pageIndex < ComputedTotalPages - 1;
        var showExpansion = CanExpandFromCurrentView || _isExpanded;

        CanGoPrev = canGoPrev;
        CanGoNext = canGoNext;
        CanToggleExpand = showExpansion;
        IsExpanded = _isExpanded;
        TotalPages = ComputedTotalPages;
        CurrentPage = Math.Min(ComputedTotalPages, Math.Max(1, _pageIndex + 1));

        if (PrevPageCommand is Command prevCmd)
        {
            prevCmd.ChangeCanExecute();
        }

        if (NextPageCommand is Command nextCmd)
        {
            nextCmd.ChangeCanExecute();
        }

        if (ToggleExpandCommand is Command expandCmd)
        {
            expandCmd.ChangeCanExecute();
        }
    }

    #endregion

    #region Item Lifecycle & Animation

    private async void OnItemBindingContextChanged(object? sender, EventArgs e)
    {
        if (_isUnloaded || sender is not VisualElement element)
        {
            return;
        }

        if (element.BindingContext == null)
        {
            return;
        }

        if (!ShouldAnimateForCurrentContext(element))
        {
            return;
        }

        await AnimateItemEntryAsync(element);
    }

    private async void OnItemLoaded(object? sender, EventArgs e)
    {
        if (_isUnloaded || sender is not VisualElement element)
        {
            return;
        }

        if (element.BindingContext == null)
        {
            return;
        }

        if (!ShouldAnimateForCurrentContext(element))
        {
            return;
        }

        await AnimateItemEntryAsync(element);
    }

    private void OnItemUnloaded(object? sender, EventArgs e)
    {
        if (sender is not VisualElement element)
        {
            return;
        }

        element.SetValue(LastAnimatedContextProperty, null);
    }

    private static bool ShouldAnimateForCurrentContext(VisualElement element)
    {
        var context = element.BindingContext;
        var lastContext = element.GetValue(LastAnimatedContextProperty);

        if (ReferenceEquals(lastContext, context))
        {
            return false;
        }

        element.SetValue(LastAnimatedContextProperty, context);
        return true;
    }

    private static async Task AnimateItemEntryAsync(VisualElement element)
    {
        element.Opacity = 0;
        await element.FadeToAsync(1, 500, Easing.CubicOut);
    }

    #endregion

    #region Keyboard Actions

    private void OnKeyActionDetected(object? sender, KeyActionEventArgs e)
    {
        if (!Services.FocusManager.HasFocus(this))
        {
            return;
        }

        switch (e.Action)
        {
            case KeyAction.CtrlA:
                if (IsAllSelected)
                {
                    ClearAllSelections();
                    SyncSelectionAndHeaderState(null);
                }
                else
                {
                    SelectAll();
                }
                break;

            case KeyAction.Escape:
                if (_selectedItems.Count > 0)
                {
                    ClearAllSelections();
                    SyncSelectionAndHeaderState(null);
                }
                break;
        }
    }

    #endregion

    #region UI event handlers

    private void OnCustomCheckBoxPointerPressed(object? sender, PointerEventArgs e)
    {
        if (sender is not Border border)
        {
            return;
        }

        var item = GetMediaItem(border.BindingContext);
        if (item == null)
        {
            return;
        }

        Services.FocusManager.SetFocus(this);

        _isCheckboxClick = true;
        ToggleSelection(item);
        SyncSelectionAndHeaderState(item);
    }

    private void OnHeaderCustomCheckBoxPointerPressed(object? sender, PointerEventArgs e)
    {
        var targetState = !IsAllSelected;

        foreach (var item in EnumerateSelectableItems())
        {
            item.IsSelected = targetState;
        }

        _anchorIndex = targetState ? 0 : null;

        SyncSelectionAndHeaderState(clickedItem: null);
    }

    private void OnRowPrimaryPointerPressed(object? sender, PointerEventArgs e)
    {
        if (_isCheckboxClick)
        {
            _isCheckboxClick = false;
            return;
        }

        if (sender is not BindableObject bindable)
        {
            return;
        }

        var item = GetMediaItem(bindable.BindingContext);
        if (item == null)
        {
            return;
        }

        Services.FocusManager.SetFocus(this);

        var isCtrl = _keyboardService?.IsControlPressed ?? false;
        var isShift = _keyboardService?.IsShiftPressed ?? false;

        if (isShift)
        {
            SelectRangeTo(item, keepExistingSelection: true);
        }
        else if (isCtrl)
        {
            ToggleSelection(item);
        }
        else
        {
            SelectSingle(item);
        }

        SyncSelectionAndHeaderState(item);
    }

    private void OnRowSecondaryPointerPressed(object? sender, PointerEventArgs e)
    {
        if (sender is not BindableObject bindable)
        {
            return;
        }

        var item = GetMediaItem(bindable.BindingContext);
        if (item == null)
        {
            return;
        }

        Services.FocusManager.SetFocus(this);

        if (!item.IsSelected)
        {
            SelectSingle(item);
            SyncSelectionAndHeaderState(item);
        }

        var position = e.GetPosition(null);
        ShowContextMenuAtPositionCommand?.Execute(position);
    }

    private async void OnRowDoubleTapped(object? sender, Microsoft.Maui.Controls.TappedEventArgs e)
    {
        if (sender is not Border border || _playbackService == null)
        {
            return;
        }

        var playbackService = _playbackService;

        var item = GetMediaItem(border.BindingContext);
        if (item == null)
        {
            return;
        }

        SelectSingle(item);
        SyncSelectionAndHeaderState(item);

        // if queue, play the queue index via playback service
        var itemIndex = GetIndexOf(item);

        if (PlaybackContextItem is IEnumerable<QueueItem> && itemIndex is >= 0 && _playbackService != null)
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

    private async void OnPlayOverlayClicked(object? sender, Microsoft.Maui.Controls.TappedEventArgs e)
    {
        if (sender is not Border playOverlay || _playbackService == null)
        {
            return;
        }

        var playbackService = _playbackService;

        var item = GetMediaItem(playOverlay.BindingContext);
        if (item == null)
        {
            return;
        }

        SelectSingle(item);
        SyncSelectionAndHeaderState(item);

        // if queue, play the queue index via playback service
        var itemIndex = GetIndexOf(item);

        if (PlaybackContextItem is IEnumerable<QueueItem> && itemIndex is >= 0 && _playbackService != null)
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

        await playbackService.PlayMediaAsync(new List<MediaItem> { item });
    }

    private async void OnFavoriteIconTapped(object? sender, Microsoft.Maui.Controls.TappedEventArgs e)
    {
        if (sender is not Label label)
        {
            return;
        }

        var item = GetMediaItem(label.BindingContext);
        if (item == null)
        {
            return;
        }

        if (item.Favorite)
        {
            await RemoveFromFavoritesAsync(item);
        }
        else
        {
            await AddToFavoritesAsync(item);
        }

        if (_currentTrackMediaItem != null && IsSameTrack(item, _currentTrackMediaItem) && _currentTrackMediaItem.Favorite != item.Favorite)
        {
            _currentTrackMediaItem.Favorite = item.Favorite;
        }

        UpdateFavoriteStateForVisibleItems();
    }

    private Task AddToFavoritesAsync(MediaItem item)
    {
        return SetFavoriteAsync(item, true);
    }

    private Task RemoveFromFavoritesAsync(MediaItem item)
    {
        return SetFavoriteAsync(item, false);
    }

    private async Task SetFavoriteAsync(MediaItem item, bool isFavorite)
    {
        var dataService = UserDataService;
        if (dataService == null)
        {
            return;
        }

        await dataService.SetFavoriteAsync(new[] { item }, isFavorite);
    }

    #endregion

    #region Context Menu

    private void OnMoreIconTapped(object? sender, Microsoft.Maui.Controls.TappedEventArgs e)
    {
        if (sender is not Label label)
        {
            return;
        }

        var selectable = GetMediaItem(label.BindingContext);
        if (selectable != null)
        {
            SelectSingle(selectable);
            SyncSelectionAndHeaderState(selectable);
        }

        ShowContextMenuAtAnchorCommand?.Execute(label);
    }

    private void OnHeaderMoreIconTapped(object? sender, Microsoft.Maui.Controls.TappedEventArgs e)
    {
        if (sender is not Label label)
        {
            return;
        }

        ShowContextMenuAtAnchorCommand?.Execute(label);
    }

    #endregion

    #region Selection operations (UI Logic)

    private void SelectSingle(MediaItem selected)
    {
        var currentlySelected = _selectedItems.OfType<MediaItem>().ToList();

        foreach (var item in currentlySelected)
        {
            if (!ReferenceEquals(item, selected))
            {
                item.IsSelected = false;
            }
        }

        selected.IsSelected = true;
        _anchorIndex = GetIndexOf(selected);
    }

    private void ClearAllSelections()
    {
        foreach (var item in EnumerateSelectableItems())
        {
            item.IsSelected = false;
        }
    }

    private void ToggleSelection(MediaItem item)
    {
        item.IsSelected = !item.IsSelected;
        _anchorIndex = GetIndexOf(item);
    }

    private void SelectRangeTo(MediaItem target, bool keepExistingSelection)
    {
        var targetIndex = GetIndexOf(target);
        if (targetIndex is null)
        {
            return;
        }

        _anchorIndex ??= targetIndex;

        if (!keepExistingSelection)
        {
            ClearAllSelections();
        }

        var start = Math.Min(_anchorIndex.Value, targetIndex.Value);
        var end = Math.Max(_anchorIndex.Value, targetIndex.Value);

        var i = 0;
        foreach (var item in EnumerateSelectableItems())
        {
            if (i >= start && i <= end)
            {
                item.IsSelected = true;
            }
            i++;
        }
    }

    private void SelectAll()
    {
        foreach (var item in EnumerateSelectableItems())
        {
            item.IsSelected = true;
        }

        _anchorIndex = 0;
        SyncSelectionAndHeaderState(clickedItem: null);
    }

    #endregion

    #region Selection state synchronization

    private void SyncSelectionAndHeaderState(object? clickedItem)
    {
        RebuildSelectedItems();
        UpdateHeaderSelectionState();
    }

    private void RebuildSelectedItems()
    {
        _selectedItems.ReplaceRange(EnumerateSelectableItems().Where(item => item.IsSelected));
    }

    private void UpdateHeaderSelectionState()
    {
        var total = 0;
        var selected = 0;

        foreach (var item in EnumerateSelectableItems())
        {
            total++;
            if (item.IsSelected)
            {
                selected++;
            }
        }

        var newValue = total > 0 && selected == total;
        IsAllSelected = newValue;
    }

    #endregion

    #region Playing state & favorite tracking

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

        if (_playbackService == null)
        {
            _playbackService = mauiContext.Services.GetService<PlaybackService>();
            if (_playbackService != null)
            {
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
        }
    }

    private void DetachPlaybackStateSource()
    {
        if (_playbackService != null)
        {
            _playbackService.PropertyChanged -= OnPlaybackServicePropertyChanged;

            if (_currentTrackMediaItem != null)
            {
                _currentTrackMediaItem.PropertyChanged -= OnCurrentTrackPropertyChanged;
            }
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

    #endregion

    #region Helpers

    private void UpdateHeaderItems(object? context)
    {
        _headerItems.Replace(context ?? new Playlist());
    }

    private int? GetIndexOf(MediaItem target)
    {
        var i = 0;
        foreach (var item in EnumerateSelectableItems())
        {
            if (ReferenceEquals(item, target))
            {
                return i;
            }
            i++;
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
        var source = ItemsSource;
        if (source == null)
        {
            yield break;
        }

        foreach (var item in source)
        {
            if (item is MediaItem selectable)
            {
                yield return selectable;
                continue;
            }

            if (item is QueueItem queueItem && queueItem.MediaItem != null)
            {
                yield return queueItem.MediaItem;
            }
        }
    }

    private static MediaItem? GetMediaItem(object? context)
    {
        if (context is MediaItem mediaItem)
        {
            return mediaItem;
        }

        return context is QueueItem queueItem ? queueItem.MediaItem : null;
    }

    #endregion
}

#region TemplateSelector
public sealed class TableViewContentTemplateSelector : DataTemplateSelector
{
    public DataTemplate? TrackTemplate { get; set; }
    public DataTemplate? QueueItemTemplate { get; set; }
    public DataTemplate? SkeletonTemplate { get; set; }

    protected override DataTemplate OnSelectTemplate(object item, BindableObject container)
    {
        if (item is TableViewSkeleton && SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        if (item is QueueItem && QueueItemTemplate != null)
        {
            return QueueItemTemplate;
        }

        if (item is Track && TrackTemplate != null)
        {
            return TrackTemplate;
        }

        throw new InvalidOperationException("TableViewTemplateSelector requires TrackTemplate, QueueItemTemplate, or SkeletonTemplate.");
    }
}

public sealed class TableViewHeaderTemplateSelector : DataTemplateSelector
{
    public DataTemplate? TrackHeaderTemplate { get; set; }
    public DataTemplate? QueueHeaderTemplate { get; set; }
    public DataTemplate? SkeletonHeaderTemplate { get; set; }

    protected override DataTemplate OnSelectTemplate(object item, BindableObject container)
    {
       if ((item is Playlist || item is Album || item is Artist) && TrackHeaderTemplate != null)
        {
            return TrackHeaderTemplate;
        }

        if (item is IEnumerable<QueueItem> && QueueHeaderTemplate != null)
        {
            return QueueHeaderTemplate;
        }    

        throw new InvalidOperationException("TableViewHeaderTemplateSelector requires a header template.");
    }
}
#endregion
