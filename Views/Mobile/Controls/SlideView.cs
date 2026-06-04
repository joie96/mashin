using mashin.Models;
using mashin.Services;
using mashin.Collections;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Linq;
using System.Windows.Input;

namespace mashin.Views.Mobile.Controls;

public partial class SlideView : ContentView
{
    #region Fields

    private const int PageSize = 10;

    private bool _isSynchronizingSize;
    private readonly HashSet<MediaItem> _suppressNextTap = new();
    private readonly HashSet<INotifyPropertyChanged> _observedItems = new();
    private readonly ObservableRangeCollection<object> _visibleItems = new();
    private INotifyCollectionChanged? _observedCollection;
    private int _loadedItemCount = PageSize;
    private bool _hasMoreItems;
    private bool _hasSelection;

    #endregion

    #region Bindable properties

    public static readonly BindableProperty ItemsSourceProperty =
        BindableProperty.Create(
            nameof(ItemsSource),
            typeof(IEnumerable<object>),
            typeof(SlideView),
            propertyChanged: OnItemsSourceChanged);

    public static readonly BindableProperty MediaActionsProperty =
        BindableProperty.Create(nameof(MediaActions), typeof(IMediaItemActions), typeof(SlideView));

    public static readonly BindableProperty PrimaryInfoTappedCommandProperty =
        BindableProperty.Create(nameof(PrimaryInfoTappedCommand), typeof(ICommand), typeof(SlideView));

    public static readonly BindableProperty SecondaryInfoTappedCommandProperty =
        BindableProperty.Create(nameof(SecondaryInfoTappedCommand), typeof(ICommand), typeof(SlideView));

    public static readonly BindableProperty ShowContextMenuAtAnchorCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtAnchorCommand), typeof(ICommand), typeof(SlideView));

    public static readonly BindableProperty ShowContextMenuAtPositionCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtPositionCommand), typeof(ICommand), typeof(SlideView));

    public static readonly BindableProperty CoverSizeProperty =
        BindableProperty.Create(
            nameof(CoverSize),
            typeof(double),
            typeof(SlideView),
            145d,
            propertyChanged: OnCoverSizeChanged);

    public static readonly BindableProperty ItemCornerRadiusProperty =
        BindableProperty.Create(nameof(ItemCornerRadius), typeof(float), typeof(SlideView), 8f);

    public static readonly BindableProperty ItemWidthProperty =
        BindableProperty.Create(nameof(ItemWidth), typeof(double), typeof(SlideView), 320d);

    public static readonly BindableProperty ItemHeightProperty =
        BindableProperty.Create(
            nameof(ItemHeight),
            typeof(double),
            typeof(SlideView),
            145d,
            propertyChanged: OnItemHeightChanged);

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

    public ICommand? SecondaryInfoTappedCommand
    {
        get => (ICommand?)GetValue(SecondaryInfoTappedCommandProperty);
        set => SetValue(SecondaryInfoTappedCommandProperty, value);
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

    public double CoverSize
    {
        get => (double)GetValue(CoverSizeProperty);
        set => SetValue(CoverSizeProperty, value);
    }

    public float ItemCornerRadius
    {
        get => (float)GetValue(ItemCornerRadiusProperty);
        set => SetValue(ItemCornerRadiusProperty, value);
    }

    public double ItemWidth
    {
        get => (double)GetValue(ItemWidthProperty);
        set => SetValue(ItemWidthProperty, value);
    }

    public double ItemHeight
    {
        get => (double)GetValue(ItemHeightProperty);
        set => SetValue(ItemHeightProperty, value);
    }

    public bool HasSelection => _hasSelection;

    public bool HasMoreItems => _hasMoreItems;

    public IReadOnlyList<object> VisibleItems => _visibleItems;

    #endregion

    #region Construction

    public SlideView()
    {
        InitializeComponent();
        ItemsCollectionView.ItemsSource = _visibleItems;
        RefreshVisibleItems();
    }

    #endregion

    #region Selection state synchronization

    private static void OnItemsSourceChanged(BindableObject bindable, object? oldValue, object? newValue)
    {
        if (bindable is not SlideView slideView)
        {
            return;
        }

        if (!ReferenceEquals(oldValue, newValue))
        {
            slideView._loadedItemCount = PageSize;
        }

        slideView.DetachSelectionObservers();
        slideView.AttachSelectionObservers(newValue as IEnumerable<object>);
        slideView.RefreshVisibleItems();
        slideView.UpdateSelectionIndicator();
    }

    private static void OnCoverSizeChanged(BindableObject bindable, object? oldValue, object? newValue)
    {
        if (bindable is not SlideView slideView || newValue is not double coverSize)
        {
            return;
        }

        slideView.SynchronizeSizes(sourceIsCoverSize: true, value: coverSize);
    }

    private static void OnItemHeightChanged(BindableObject bindable, object? oldValue, object? newValue)
    {
        if (bindable is not SlideView slideView || newValue is not double itemHeight)
        {
            return;
        }

        slideView.SynchronizeSizes(sourceIsCoverSize: false, value: itemHeight);
    }

    private void SynchronizeSizes(bool sourceIsCoverSize, double value)
    {
        if (_isSynchronizingSize)
        {
            return;
        }

        var normalizedValue = Math.Max(1d, value);
        _isSynchronizingSize = true;
        try
        {
            if (sourceIsCoverSize)
            {
                if (Math.Abs(ItemHeight - normalizedValue) > 0.001d)
                {
                    ItemHeight = normalizedValue;
                }
            }
            else
            {
                if (Math.Abs(CoverSize - normalizedValue) > 0.001d)
                {
                    CoverSize = normalizedValue;
                }
            }
        }
        finally
        {
            _isSynchronizingSize = false;
        }
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

        foreach (var item in items.OfType<INotifyPropertyChanged>())
        {
            if (_observedItems.Add(item))
            {
                item.PropertyChanged += OnObservedItemPropertyChanged;
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
        if (e.OldItems != null)
        {
            foreach (var oldItem in e.OldItems.OfType<INotifyPropertyChanged>())
            {
                if (_observedItems.Remove(oldItem))
                {
                    oldItem.PropertyChanged -= OnObservedItemPropertyChanged;
                }
            }
        }

        if (e.NewItems != null)
        {
            foreach (var newItem in e.NewItems.OfType<INotifyPropertyChanged>())
            {
                if (_observedItems.Add(newItem))
                {
                    newItem.PropertyChanged += OnObservedItemPropertyChanged;
                }
            }
        }

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

    #region UI Events

    private async void OnRowTouchCompleted(object? sender, EventArgs e)
    {
        if (sender is not BindableObject { BindingContext: MediaItem mediaItem })
        {
            return;
        }

        EnsureSelectionOwnership();

        await ExecuteShortPressAsync(mediaItem);
    }

    private void OnRowLongPressCompleted(object? sender, EventArgs e)
    {
        if (sender is not BindableObject { BindingContext: MediaItem mediaItem })
        {
            return;
        }

        EnsureSelectionOwnership();

        mediaItem.IsSelected = !mediaItem.IsSelected;
        UpdateSelectionIndicator();

        _suppressNextTap.Add(mediaItem);
    }

    private async void OnRowTapped(object? sender, TappedEventArgs e)
    {
        if (sender is not BindableObject { BindingContext: MediaItem mediaItem })
        {
            return;
        }

        EnsureSelectionOwnership();

        await ExecuteShortPressAsync(mediaItem);
    }

    private async void OnPlayOverlayClicked(object? sender, TappedEventArgs e)
    {
        if (sender is not BindableObject { BindingContext: MediaItem item } || MediaActions == null)
        {
            return;
        }

        await MediaActions.PlayMediaAsync(item);
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

    private static object GetPrimaryNavigationParameter(MediaItem mediaItem)
    {
        if (mediaItem is Track track)
        {
            return track.Album ?? mediaItem;
        }

        return mediaItem;
    }

    private async Task ExecuteShortPressAsync(MediaItem mediaItem)
    {
        if (_suppressNextTap.Remove(mediaItem))
        {
            return;
        }

        // If at least one item is selected, keep taps in selection mode until all are deselected.
        if (HasAnySelectedItems())
        {
            mediaItem.IsSelected = !mediaItem.IsSelected;
            UpdateSelectionIndicator();
            return;
        }

        if (mediaItem is Track && MediaActions != null)
        {
            await MediaActions.PlayMediaAsync(mediaItem);
            return;
        }

        var command = PrimaryInfoTappedCommand;
        var parameter = GetPrimaryNavigationParameter(mediaItem);
        if (command?.CanExecute(parameter) == true)
        {
            command.Execute(parameter);
        }
    }

    public void SelectAllItems()
    {
        if (ItemsSource == null)
        {
            return;
        }

        foreach (var item in ItemsSource.OfType<MediaItem>())
        {
            item.IsSelected = true;
        }

        UpdateSelectionIndicator();
    }

    public void ClearSelection()
    {
        if (ItemsSource == null)
        {
            return;
        }

        foreach (var item in ItemsSource.OfType<MediaItem>())
        {
            item.IsSelected = false;
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
        if (mashin.Services.FocusManager.GetFocusedControl<SlideView>(out var focusedSlide)
            && focusedSlide != null
            && !ReferenceEquals(focusedSlide, this)
            && focusedSlide.HasSelection)
        {
            focusedSlide.ClearSelection();
        }

        if (mashin.Services.FocusManager.GetFocusedControl<RowView>(out var focusedRow)
            && focusedRow != null
            && focusedRow.HasSelection)
        {
            focusedRow.ClearSelection();
        }

        if (mashin.Services.FocusManager.GetFocusedControl<TableView>(out var focusedTable)
            && focusedTable != null
            && focusedTable.HasSelection)
        {
            focusedTable.ClearSelection();
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
        var nextItems = sourceItems.Take(visibleCount).ToList();

        _visibleItems.ReplaceRange(nextItems);
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
        var nextCount = Math.Min(currentCount + PageSize, totalCount);
        if (nextCount > currentCount)
        {
            _visibleItems.AddRange(sourceItems.Skip(currentCount).Take(nextCount - currentCount));
        }

        _loadedItemCount = nextCount;
        UpdateHasMoreItems(nextCount < totalCount);
    }

    #endregion

    private bool HasAnySelectedItems()
    {
        if (ItemsSource == null)
        {
            return false;
        }

        return ItemsSource.OfType<MediaItem>().Any(item => item.IsSelected);
    }

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

public sealed class SlideViewTemplateSelector : DataTemplateSelector
{
    public DataTemplate? TrackTemplate { get; set; }
    public DataTemplate? SkeletonTemplate { get; set; }

    protected override DataTemplate OnSelectTemplate(object item, BindableObject container)
    {
        if (item is SkeletonItem && SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        if (item is Track && TrackTemplate != null)
        {
            return TrackTemplate;
        }

        if (TrackTemplate != null)
        {
            return TrackTemplate;
        }

        if (SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        throw new InvalidOperationException("SlideViewTemplateSelector requires TrackTemplate or SkeletonTemplate.");
    }
}
