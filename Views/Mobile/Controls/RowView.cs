using mashin.Collections;
using mashin.Models;
using mashin.Services;
using Microsoft.Extensions.DependencyInjection;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Linq;
using System.Windows.Input;

namespace mashin.Views.Mobile.Controls;

public partial class RowView : ContentView
{
    #region Fields

    private const int PageSize = 10;

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
            typeof(RowView),
            propertyChanged: OnItemsSourceChanged);

    public static readonly BindableProperty MediaActionsProperty =
        BindableProperty.Create(nameof(MediaActions), typeof(IMediaItemActions), typeof(RowView));

    public static readonly BindableProperty PrimaryInfoTappedCommandProperty =
        BindableProperty.Create(nameof(PrimaryInfoTappedCommand), typeof(ICommand), typeof(RowView));

    public static readonly BindableProperty SecondaryInfoTappedCommandProperty =
        BindableProperty.Create(nameof(SecondaryInfoTappedCommand), typeof(ICommand), typeof(RowView));

    public static readonly BindableProperty ShowContextMenuAtAnchorCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtAnchorCommand), typeof(ICommand), typeof(RowView));

    public static readonly BindableProperty ShowContextMenuAtPositionCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtPositionCommand), typeof(ICommand), typeof(RowView));

    public static readonly BindableProperty CoverSizeProperty =
        BindableProperty.Create(nameof(CoverSize), typeof(double), typeof(RowView), 145d);

    public static readonly BindableProperty ItemCornerRadiusProperty =
        BindableProperty.Create(nameof(ItemCornerRadius), typeof(float), typeof(RowView), 8f);

    public static readonly BindableProperty ItemWidthProperty =
        BindableProperty.Create(nameof(ItemWidth), typeof(double), typeof(RowView), 145d);

    public static readonly BindableProperty ItemHeightProperty =
        BindableProperty.Create(nameof(ItemHeight), typeof(double), typeof(RowView), 206d);

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

    public RowView()
    {
        InitializeComponent();
    }

    #endregion

    #region Selection state synchronization

    private static void OnItemsSourceChanged(BindableObject bindable, object? oldValue, object? newValue)
    {
        if (bindable is not RowView rowView)
        {
            return;
        }

        if (!ReferenceEquals(oldValue, newValue))
        {
            rowView._loadedItemCount = PageSize;
        }

        rowView.DetachSelectionObservers();
        rowView.AttachSelectionObservers(newValue as IEnumerable<object>);
        rowView.RefreshVisibleItems();
        rowView.UpdateSelectionIndicator();
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

    private void OnRowTouchCompleted(object? sender, EventArgs e)
    {
        if (sender is not BindableObject { BindingContext: MediaItem mediaItem })
        {
            return;
        }

        EnsureSelectionOwnership();

        ExecuteShortPress(mediaItem);
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

    private void OnRowTapped(object? sender, TappedEventArgs e)
    {
        if (sender is not BindableObject { BindingContext: MediaItem mediaItem })
        {
            return;
        }

        EnsureSelectionOwnership();

        ExecuteShortPress(mediaItem);
    }

    private void ExecuteShortPress(MediaItem mediaItem)
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

        var command = PrimaryInfoTappedCommand;
        var parameter = GetPrimaryNavigationParameter(mediaItem);
        if (command?.CanExecute(parameter) == true)
        {
            command.Execute(parameter);
        }
    }

    private void OnMoreButtonTapped(object? sender, TappedEventArgs e)
    {
        if (sender is not View anchorView || sender is not BindableObject { BindingContext: MediaItem })
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

    private async void OnPlayOverlayClicked(object? sender, TappedEventArgs e)
    {
        if (sender is not BindableObject { BindingContext: MediaItem item })
        {
            return;
        }

        var playbackService = ResolvePlaybackService();
        if (playbackService == null)
        {
            return;
        }

        await playbackService.PlayMediaAsync(new List<MediaItem> { item });
    }

    #endregion

    #region Helpers

    private static IPlaybackService? ResolvePlaybackService()
    {
        var services = Application.Current?.Handler?.MauiContext?.Services;
        if (services == null)
        {
            return null;
        }

        return services.GetService<IPlaybackService>();
    }

    private static object GetPrimaryNavigationParameter(MediaItem mediaItem)
    {
        if (mediaItem is Track track)
        {
            return track.Album ?? mediaItem;
        }

        return mediaItem;
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
        if (mashin.Services.FocusManager.GetFocusedControl<RowView>(out var focusedRow)
            && focusedRow != null
            && !ReferenceEquals(focusedRow, this)
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

public sealed class MobileRowViewTemplateSelector : DataTemplateSelector
{
    #region Templates

    public DataTemplate? AlbumTemplate { get; set; }
    public DataTemplate? TrackTemplate { get; set; }
    public DataTemplate? ArtistTemplate { get; set; }
    public DataTemplate? PlaylistTemplate { get; set; }
    public DataTemplate? SkeletonTemplate { get; set; }

    #endregion

    #region Template Selection

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

        throw new InvalidOperationException("MobileRowViewTemplateSelector requires AlbumTemplate, TrackTemplate, ArtistTemplate, PlaylistTemplate or SkeletonTemplate.");
    }

    #endregion
}