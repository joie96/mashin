using mashin.Collections;
using mashin.Models;
using mashin.Services;
using Microsoft.Maui.Layouts;
using System.Collections;
using System.Collections.Generic;
using System.Collections.Specialized;
using System.Windows.Input;


namespace mashin.Views.Desktop.Controls;

public partial class RowView : ContentView
{
    #region Bindable properties

    public static readonly BindableProperty ItemsSourceProperty =
        BindableProperty.Create(nameof(ItemsSource), typeof(IEnumerable<object>), typeof(RowView), propertyChanged: OnItemsSourceChanged);

    public static readonly BindableProperty MediaActionsProperty =
        BindableProperty.Create(nameof(MediaActions), typeof(IMediaItemActions), typeof(RowView));

    public static readonly BindableProperty CurrentTrackUriProperty =
        BindableProperty.Create(nameof(CurrentTrackUri), typeof(string), typeof(RowView));

    public static readonly BindableProperty ItemCornerRadiusProperty =
        BindableProperty.Create(nameof(ItemCornerRadius), typeof(float), typeof(RowView), defaultValue: 8f);

    public static readonly BindableProperty ItemWidthProperty = 
        BindableProperty.Create(nameof(ItemWidth), typeof(double), typeof(RowView), MinItemWidth);

    public static readonly BindableProperty PrimaryInfoTappedCommandProperty =
        BindableProperty.Create(nameof(PrimaryInfoTappedCommand), typeof(ICommand), typeof(RowView));

    public static readonly BindableProperty SecondaryInfoTappedCommandProperty =
        BindableProperty.Create(nameof(SecondaryInfoTappedCommand), typeof(ICommand), typeof(RowView));

    public static readonly BindableProperty ShowContextMenuAtAnchorCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtAnchorCommand), typeof(ICommand), typeof(RowView));

    public static readonly BindableProperty ShowContextMenuAtPositionCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtPositionCommand), typeof(ICommand), typeof(RowView));

    public static readonly BindableProperty SelectedItemsProperty =
        BindableProperty.Create(nameof(SelectedItems), typeof(IList), typeof(RowView), defaultBindingMode: BindingMode.TwoWay);

    public static readonly BindableProperty IsAllSelectedProperty =
    BindableProperty.Create(nameof(IsAllSelected), typeof(bool), typeof(RowView), defaultValue: false);

    public static readonly BindableProperty CanGoPrevProperty =
        BindableProperty.Create(nameof(CanGoPrev), typeof(bool), typeof(RowView), false);

    public static readonly BindableProperty CanGoNextProperty =
        BindableProperty.Create(nameof(CanGoNext), typeof(bool), typeof(RowView), false);

    public static readonly BindableProperty CanToggleExpandProperty =
        BindableProperty.Create(nameof(CanToggleExpand), typeof(bool), typeof(RowView), false);

    public static readonly BindableProperty IsExpandedProperty =
        BindableProperty.Create(nameof(IsExpanded), typeof(bool), typeof(RowView), false);

    public static readonly BindableProperty CurrentPageProperty =
        BindableProperty.Create(nameof(CurrentPage), typeof(int), typeof(RowView), 1);

    public static readonly BindableProperty TotalPagesProperty =
        BindableProperty.Create(nameof(TotalPages), typeof(int), typeof(RowView), 1);

    #endregion

    #region Fields

    private const double MinItemWidth = 125;
    private const double ItemSpacing = 12;


    private readonly ObservableRangeCollection<object> _selectedItems = new();
    private readonly ObservableRangeCollection<object> _visibleItemsPrimary = new();
    private readonly ObservableRangeCollection<object> _visibleItemsSecondary = new();
    private readonly HashSet<object> _pageVerticalInItems = new(ReferenceEqualityComparer.Instance);
    private List<object> _allItems = new();
    private INotifyCollectionChanged? _itemsSourceCollection;
    private IKeyboardService? _keyboardService;
    private IQueueSyncService? _queueSyncService;
    private int? _anchorIndex;
    private bool _isCheckboxClick;
    private int _pageIndex;
    private int _itemsPerPage = 1;
    private bool _isExpanded;
    private bool _isPageAnimating;
    private bool _isPrimaryHostActive = true;
    private double _lastItemsHostWidth = double.NaN;
    private bool _isUnloaded;
    private bool _hasMeasuredHostWidth;
    private bool _hasPendingPagedSync;

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

    public IMediaItemActions? MediaActions
    {
        get => (IMediaItemActions?)GetValue(MediaActionsProperty);
        set => SetValue(MediaActionsProperty, value);
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

    public bool IsAllSelected
    {
        get => (bool)GetValue(IsAllSelectedProperty);
        private set => SetValue(IsAllSelectedProperty, value);
    }

    public float ItemCornerRadius
    {
        get => (float)GetValue(ItemCornerRadiusProperty);
        set => SetValue(ItemCornerRadiusProperty, value);
    }

    public double ItemWidth
    {
        get => (double)GetValue(ItemWidthProperty);
        private set => SetValue(ItemWidthProperty, value);
    }

    public string? CurrentTrackUri
    {
        get => (string?)GetValue(CurrentTrackUriProperty);
        private set => SetValue(CurrentTrackUriProperty, value);
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

    private bool CanExpandFromCurrentView => !_isExpanded && _allItems.Count > ActiveVisibleItems.Count;

    #endregion

    #region Construction

    private ObservableRangeCollection<object> ActiveVisibleItems => _isPrimaryHostActive ? _visibleItemsPrimary : _visibleItemsSecondary;
    private ObservableRangeCollection<object> InactiveVisibleItems => _isPrimaryHostActive ? _visibleItemsSecondary : _visibleItemsPrimary;
    private Grid ActiveItemsLayer => _isPrimaryHostActive ? ItemsHostPrimaryLayer : ItemsHostSecondaryLayer;
    private Grid InactiveItemsLayer => _isPrimaryHostActive ? ItemsHostSecondaryLayer : ItemsHostPrimaryLayer;
    private ScrollView ActiveItemsScroll => _isPrimaryHostActive ? ItemsScrollPrimary : ItemsScrollSecondary;
    private ScrollView InactiveItemsScroll => _isPrimaryHostActive ? ItemsScrollSecondary : ItemsScrollPrimary;
    private FlexLayout ActiveItemsFlex => _isPrimaryHostActive ? ItemsFlexPrimary : ItemsFlexSecondary;
    private FlexLayout InactiveItemsFlex => _isPrimaryHostActive ? ItemsFlexSecondary : ItemsFlexPrimary;

    public RowView()
    {
        InitializeComponent();

        PrevPageCommand = new Command(async () => await GoToPreviousPageAsync(), () => CanGoPrev);
        NextPageCommand = new Command(async () => await GoToNextPageAsync(), () => CanGoNext);
        ToggleExpandCommand = new Command(ToggleExpanded, () => CanToggleExpand);

        SelectedItems = _selectedItems;
        BindableLayout.SetItemsSource(ItemsFlexPrimary, _visibleItemsPrimary);
        BindableLayout.SetItemsSource(ItemsFlexSecondary, _visibleItemsSecondary);
        EnsureSingleActiveHostVisible();
        UpdateNavigationState();
    }

    protected override void OnHandlerChanged()
    {
        base.OnHandlerChanged();
    }

    private void OnRowViewLoaded(object? sender, EventArgs e)
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

    }

    private void OnRowViewUnloaded(object? sender, EventArgs e)
    {
        _isUnloaded = true;
        _hasPendingPagedSync = false;
        _hasMeasuredHostWidth = false;
        _lastItemsHostWidth = double.NaN;

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
        MediaActions = null;

        BindableLayout.SetItemsSource(ItemsFlexPrimary, null);
        BindableLayout.SetItemsSource(ItemsFlexSecondary, null);

        ItemsSource = null;
        BindingContext = null;

        // Collections bereinigen
        _selectedItems.Clear();
    }

    #endregion

    #region Playback state tracking

    private void AttachPlaybackStateSource()
    {
        if (_queueSyncService != null)
        {
            return;
        }

        var mauiContext = Handler?.MauiContext
            ?? Microsoft.Maui.Controls.Application.Current?.Handler?.MauiContext;
        if (mauiContext == null)
        {
            return;
        }

        _queueSyncService = mauiContext.Services.GetService<IQueueSyncService>();
        if (_queueSyncService == null)
        {
            return;
        }

        _queueSyncService.CurrentTrackUpdated += OnCurrentTrackUpdated;
        SetCurrentTrackUri(_queueSyncService.CurrentTrack?.Uri);
    }

    private void DetachPlaybackStateSource()
    {
        if (_queueSyncService == null)
        {
            return;
        }

        _queueSyncService.CurrentTrackUpdated -= OnCurrentTrackUpdated;
        _queueSyncService = null;
        SetCurrentTrackUri(null);
    }

    private void OnCurrentTrackUpdated(object? sender, EventArgs e)
    {
        var currentTrackUri = _queueSyncService?.CurrentTrack?.Uri;

        if (MainThread.IsMainThread)
        {
            SetCurrentTrackUri(currentTrackUri);
            return;
        }

        MainThread.BeginInvokeOnMainThread(() => SetCurrentTrackUri(currentTrackUri));
    }

    private void SetCurrentTrackUri(string? uri)
    {
        if (string.Equals(CurrentTrackUri, uri, StringComparison.OrdinalIgnoreCase))
        {
            return;
        }

        CurrentTrackUri = uri;
    }

    #endregion

    #region Property Changed Handlers

    private static void OnItemsSourceChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not RowView view)
        {
            return;
        }

        view.ApplyItemsSource(newValue as IEnumerable<object>);
    }

    #endregion

    #region ItemSource handling
    private void ApplyItemsSource(IEnumerable<object>? items)
    {
        AttachItemsSourceCollection(items);
        _allItems = items?.Cast<object>().ToList() ?? new List<object>();
        _pageIndex = 0;
        _anchorIndex = null;
        ClearAllSelections();
        SyncSelectionState();
        OnExpandedChanged();
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
        if (_isUnloaded || Handler == null)
        {
            return;
        }

        if (!MainThread.IsMainThread)
        {
            MainThread.BeginInvokeOnMainThread(() => OnItemsSourceCollectionChanged(sender, e));
            return;
        }

        _allItems = ItemsSource?.Cast<object>().ToList() ?? new List<object>();
        OnExpandedChanged();
    }

    #endregion

    #region Paging & Expansion

    private void OnItemsHostSizeChanged(object? sender, EventArgs e)
    {
        if (sender is not VisualElement element)
        {
            return;
        }

        // Get current itemost host width
        var width = element.Width;
        if (double.IsNaN(width) || double.IsInfinity(width) || width <= 0)
        {
            return;
        }

        _hasMeasuredHostWidth = true;

        // If width changed and currently expanded, collapse to reduce items count for resizing
        var widthChanged = !double.IsNaN(_lastItemsHostWidth) && Math.Abs(width - _lastItemsHostWidth) > 0.1;
        _lastItemsHostWidth = width;

        if (_isExpanded && widthChanged)
        {
            _isExpanded = false;
            OnExpandedChanged();
        }

        // Calculate items per page and item width based on new item hostwidth
        var perPage = Math.Max(1, (int)Math.Floor(width / (MinItemWidth + ItemSpacing)));
        var newItemWidth = Math.Max(MinItemWidth, Math.Floor((width / perPage) - ItemSpacing));
        var itemsPerPageChanged = perPage != _itemsPerPage;
        var itemWidthChanged = Math.Abs(newItemWidth - ItemWidth) > 0.1;

        if (itemsPerPageChanged)
        {
            _itemsPerPage = perPage;
        }

        if (itemWidthChanged)
        {
            ItemWidth = newItemWidth;
        }

        // Paged mode: keep current page in sync when host size changes
        if (!_isExpanded && (itemsPerPageChanged || itemWidthChanged || _hasPendingPagedSync))
        {
            _hasPendingPagedSync = false;
            SyncPageItems(ActiveVisibleItems, _pageIndex);
            InactiveVisibleItems.Clear();
            UpdateNavigationState();
        }
    }

    public async Task GoToPreviousPageAsync()
    {
        await SetPageAsync(_pageIndex - 1);
    }

    public async Task GoToNextPageAsync()
    {
        await SetPageAsync(_pageIndex + 1);
    }

    public void ToggleExpanded()
    {
        if (!_isExpanded && !CanExpandFromCurrentView)
        {
            return;
        }

        _isExpanded = !_isExpanded;
        OnExpandedChanged();
    }

    private void OnExpandedChanged()
    {
        _pageIndex = 0;

        // Set specific layout fpr expansion or paging
        UpdateLayout();

        // Clear inactive itemshost and item animation hashset
        InactiveVisibleItems.Clear();
        _pageVerticalInItems.Clear();

        if (!_isExpanded && !_hasMeasuredHostWidth && _allItems.Count > 1)
        {
            _hasPendingPagedSync = true;
            ActiveVisibleItems.Clear();
            return;
        }

        // Load page items for page 0
        SyncPageItems(ActiveVisibleItems, _pageIndex);

        // Refresh nav visibility after visible-items changed (e.g. collapse from expanded view)
        UpdateNavigationState();

        // Load Data expanded data if needed
        if (_isExpanded)
        {   
            _ = LoadExpandedItemsAsync();
        }
    }

    private async Task SetPageAsync(int pageIndex)
    {
        if (_isExpanded || _allItems.Count == 0)
        {
            return;
        }

        var totalPages = Math.Max(1, (int)Math.Ceiling((double)_allItems.Count / _itemsPerPage));
        var nextIndex = Math.Clamp(pageIndex, 0, totalPages - 1);
        if (nextIndex == _pageIndex)
        {
            return;
        }

        if (_isPageAnimating)
        {
            return;
        }

        _isPageAnimating = true;
        try
        {
            var direction = nextIndex > _pageIndex ? -1 : 1;
            var offset = 50;
            var outX = offset * direction;
            var inX = -outX;

            var targetPageItems = GetPageItems(nextIndex);
            if (targetPageItems.Count == 0)
            {
                return;
            }

            SetPagedLayout(ActiveItemsFlex, ActiveItemsScroll);
            SetPagedLayout(InactiveItemsFlex, InactiveItemsScroll);

            InactiveVisibleItems.Clear();
            InactiveVisibleItems.AddRange(targetPageItems);

            var activeLayer = ActiveItemsLayer;
            var incomingLayer = InactiveItemsLayer;

            incomingLayer.IsVisible = true;
            incomingLayer.InputTransparent = true;
            incomingLayer.Opacity = 0;
            incomingLayer.TranslationX = inX;

            await Task.WhenAll(
                activeLayer.TranslateToAsync(outX, 0, 250, Easing.Linear),
                activeLayer.FadeToAsync(0, 250, Easing.CubicIn));

            await Task.WhenAll(
                incomingLayer.TranslateToAsync(0, 0, 250, Easing.Linear),
                incomingLayer.FadeToAsync(1, 250, Easing.CubicOut));

            _pageIndex = nextIndex;
            _isPrimaryHostActive = !_isPrimaryHostActive;
            EnsureSingleActiveHostVisible();

            InactiveVisibleItems.Clear();

            UpdateNavigationState();
        }
        finally
        {
            _isPageAnimating = false;
        }
    }

    private void UpdateLayout()
    {
        if (_isExpanded)
        {
            // Set flex layout for expanded mode
            EnsureSingleActiveHostVisible();
            SetExpandedLayout(ActiveItemsFlex, ActiveItemsScroll);
        }
        else
        {
            // Set flex layout for paged mode
            EnsureSingleActiveHostVisible();
            SetPagedLayout(ActiveItemsFlex, ActiveItemsScroll);
            SetPagedLayout(InactiveItemsFlex, InactiveItemsScroll);
        }

        UpdateNavigationState();
    }

    private async Task LoadExpandedItemsAsync()
    {
        _pageVerticalInItems.Clear();

        var remainingItems = _allItems.ToList();
        if (_pageIndex == 0)
        {
            remainingItems = _allItems.Skip(_itemsPerPage).ToList();
        }
        else
        {
            ActiveVisibleItems.Clear();
        }

        foreach (var batch in remainingItems.Chunk(_itemsPerPage))
        {
            if (!_isExpanded)
            {
                return;
            }

            _pageVerticalInItems.UnionWith(batch);
            ActiveVisibleItems.AddRange(batch);

            await Task.Yield();
            await Task.Delay(250);
        }

        if (_isExpanded)
        {
            InactiveVisibleItems.Clear();
        }
    }

    private void UpdateNavigationState()
    {
        var canGoPrev = !_isExpanded && _pageIndex > 0;
        var canGoNext = !_isExpanded && _allItems.Count > (_pageIndex + 1) * _itemsPerPage;
        var showExpansion = CanExpandFromCurrentView || _isExpanded;
        var totalPages = Math.Max(1, (int)Math.Ceiling((double)Math.Max(1, _allItems.Count) / _itemsPerPage));

        CanGoPrev = canGoPrev;
        CanGoNext = canGoNext;
        CanToggleExpand = showExpansion;
        IsExpanded = _isExpanded;
        TotalPages = totalPages;
        CurrentPage = Math.Min(totalPages, Math.Max(1, _pageIndex + 1));

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

    #region Paging Helpers

    private void EnsureSingleActiveHostVisible()
    {
        var activeLayer = ActiveItemsLayer;
        var inactiveLayer = InactiveItemsLayer;

        activeLayer.IsVisible = true;
        activeLayer.InputTransparent = false;
        activeLayer.Opacity = 1;
        activeLayer.TranslationX = 0;

        inactiveLayer.IsVisible = false;
        inactiveLayer.InputTransparent = true;
        inactiveLayer.Opacity = 0;
        inactiveLayer.TranslationX = 0;
    }

    private static void SetPagedLayout(FlexLayout flexLayout, ScrollView scrollView)
    {
        flexLayout.Direction = FlexDirection.Row;
        flexLayout.Wrap = FlexWrap.NoWrap;
        scrollView.Orientation = ScrollOrientation.Horizontal;
    }

    private static void SetExpandedLayout(FlexLayout flexLayout, ScrollView scrollView)
    {
        flexLayout.Direction = FlexDirection.Row;
        flexLayout.Wrap = FlexWrap.Wrap;
        scrollView.Orientation = ScrollOrientation.Vertical;
    }

    // Sync target collection with page items for given page index (adding/removing/replacing items as needed if eg. page size changes)
    private void SyncPageItems(ObservableRangeCollection<object> target, int pageIndex)
    {
        var items = GetPageItems(pageIndex);

        if (items.Count == 0)
        {
            target.Clear();
            return;
        }

        if (target.Count == 0 || !ReferenceEquals(target[0], items[0]))
        {
            target.Clear();
            target.AddRange(items);
            return;
        }

        if (target.Count > items.Count)
        {
            for (var i = target.Count - 1; i >= items.Count; i--)
            {
                target.RemoveAt(i);
            }
        }
        else if (target.Count < items.Count)
        {
            target.AddRange(items.Skip(target.Count));
        }
    }

    private List<object> GetPageItems(int pageIndex)
    {
        var pageStart = pageIndex * _itemsPerPage;

        if (_allItems.Count == 0 || pageStart >= _allItems.Count)
        {
            return new List<object>();
        }

        var pageEnd = Math.Min(pageStart + _itemsPerPage, _allItems.Count);
        return _allItems.Skip(pageStart).Take(pageEnd - pageStart).ToList();
    }

    #endregion

    #region Item Lifecycle

    private async void OnItemLoaded(object? sender, EventArgs e)
    {
        if (sender is not VisualElement element)
        {
            return;
        }

        var item = element.BindingContext;
        if (item == null)
        {
            return;
        }

        // Page extension animation
        if (_pageVerticalInItems.Remove(item))
        {
            element.Opacity = 0;
            element.TranslationY = -50;
            await Task.WhenAll(
                element.FadeToAsync(1, 200, Easing.CubicIn),
                element.TranslateToAsync(0, 0, 200, Easing.Linear));
            return;
        }

        //Default animation
        element.Opacity = 0;
        await element.FadeToAsync(1, 100, Easing.CubicOut);
    }

    private void OnItemUnloaded(object? sender, EventArgs e)
    {
        if (sender is not VisualElement)
        {
            return;
        }
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
                    SyncSelectionState();
                }
                break;
        }
    }

    #endregion

    #region UI event handlers

    private void OnCustomCheckBoxPointerPressed(object? sender, PointerEventArgs e)
    {
        if (sender is not Border border || border.BindingContext is not MediaItem item)
        {
            return;
        }

        Services.FocusManager.SetFocus(this);

        _isCheckboxClick = true;
        ToggleSelection(item);
        SyncSelectionState();
    }

    private void OnGridItemPrimaryPointerPressed(object? sender, PointerEventArgs e)
    {
        if (_isCheckboxClick)
        {
            _isCheckboxClick = false;
            return;
        }

        if (sender is not BindableObject bindable || bindable.BindingContext is not MediaItem item)
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

        SyncSelectionState();
    }

    private void OnGridItemSecondaryPointerPressed(object? sender, PointerEventArgs e)
    {
        if (sender is not BindableObject bindable || bindable.BindingContext is not MediaItem item)
        {
            return;
        }

        Services.FocusManager.SetFocus(this);

        if (!item.IsSelected)
        {
            SelectSingle(item);
            SyncSelectionState();
        }

        var position = e.GetPosition(null);
        ShowContextMenuAtPositionCommand?.Execute(position);
        return;
    }

    private async void OnGridItemDoubleTapped(object? sender, TappedEventArgs e)
    {
        if (sender is not Border border || border.BindingContext is not MediaItem item || MediaActions == null)
        {
            return;
        }

        // Select the item
        SelectSingle(item);
        SyncSelectionState();

        // Play immediately
        await MediaActions.PlayMediaAsync(item);
    }

    private async void OnPlayOverlayClicked(object? sender, TappedEventArgs e)
    {
        if (sender is not Border playOverlay || MediaActions == null)
        {
            return;
        }

        if (playOverlay.BindingContext is not MediaItem item)
        {
            return;
        }

        SelectSingle(item);
        SyncSelectionState();

        await MediaActions.PlayMediaAsync(item);
    }

    private async void OnFavoriteIconTapped(object? sender, TappedEventArgs e)
    {
        if (sender is not Label label || label.BindingContext is not MediaItem item)
        {
            return;
        }

        if (MediaActions == null)
        {
            item.Favorite = !item.Favorite;
            return;
        }

        if (item.Favorite)
        {
            await MediaActions.RemoveFromFavoritesAsync(item);
        }
        else
        {
            await MediaActions.AddToFavoritesAsync(item);
        }
    }

    private void OnMoreIconTapped(object? sender, TappedEventArgs e)
    {
        if (sender is not Label label)
        {
            return;
        }

        if (label.BindingContext is MediaItem selectable)
        {
            SelectSingle(selectable);
            SyncSelectionState();
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
        SyncSelectionState();
    }

    #endregion

    #region Selection state synchronization

    private void SyncSelectionState()
    {
        RebuildSelectedItems();
        UpdateHeaderSelectionState();
    }

    private void RebuildSelectedItems()
    {
        _selectedItems.Clear();

        foreach (var item in EnumerateSelectableItems())
        {
            if (item.IsSelected)
            {
                _selectedItems.Add(item);
            }
        }
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

    #region Helpers

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
            }
        }
    }

    #endregion



}

#region Template Selector
public sealed class RowViewTemplateSelector : DataTemplateSelector
{
    public DataTemplate? AlbumTemplate { get; set; }
    public DataTemplate? TrackTemplate { get; set; }
    public DataTemplate? ArtistTemplate { get; set; }
    public DataTemplate? PlaylistTemplate { get; set; }
    public DataTemplate? SkeletonTemplate { get; set; }

    protected override DataTemplate OnSelectTemplate(object item, BindableObject container)
    {
        if (item is RowViewSkeleton && SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        if (item is Artist && ArtistTemplate != null)
        {
            return ArtistTemplate;
        }

        if (item is Track && TrackTemplate != null)
        {
            return TrackTemplate;
        }

        if (item is Playlist && PlaylistTemplate != null)
        {
            return PlaylistTemplate;
        }

        if (AlbumTemplate != null)
        {
            return AlbumTemplate;
        }

        if (SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        throw new InvalidOperationException("RowViewTemplateSelector requires AlbumTemplate, TrackTemplate, ArtistTemplate, PlaylistTemplate or SkeletonTemplate.");
    }
}
#endregion