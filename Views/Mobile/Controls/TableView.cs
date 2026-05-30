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

    private readonly HashSet<MediaItem> _suppressNextTap = new();
    private readonly HashSet<INotifyPropertyChanged> _observedItems = new();
    private INotifyCollectionChanged? _observedCollection;

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

    public bool HasSelection => HasAnySelectedItems();

    #endregion

    #region Selection state synchronization

    private static void OnItemsSourceChanged(BindableObject bindable, object? oldValue, object? newValue)
    {
        if (bindable is not TableView tableView)
        {
            return;
        }

        tableView.DetachSelectionObservers();
        tableView.AttachSelectionObservers(newValue as IEnumerable<object>);
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
        if (sender is not BindableObject { BindingContext: MediaItem mediaItem })
        {
            return;
        }

        ExecuteShortPress(mediaItem);
    }

    private void OnRowLongPressCompleted(object? sender, EventArgs e)
    {
        if (sender is not BindableObject { BindingContext: MediaItem mediaItem })
        {
            return;
        }

        mediaItem.IsSelected = true;
        UpdateSelectionIndicator();

        if (sender is View anchorView)
        {
            var contextMenuCommand = ShowContextMenuAtAnchorCommand;
            if (contextMenuCommand?.CanExecute(anchorView) == true)
            {
                contextMenuCommand.Execute(anchorView);
            }
        }

        _suppressNextTap.Add(mediaItem);
    }

    private void OnRowTapped(object? sender, TappedEventArgs e)
    {
        if (sender is not BindableObject { BindingContext: MediaItem mediaItem })
        {
            return;
        }

        ExecuteShortPress(mediaItem);
    }

    private void ExecuteShortPress(MediaItem mediaItem)
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

        var command = ShortPressCommand;
        if (command?.CanExecute(mediaItem) == true)
        {
            command.Execute(mediaItem);
        }
    }

    private void OnMoreButtonTapped(object? sender, TappedEventArgs e)
    {
        if (sender is not View anchorView || sender is not BindableObject { BindingContext: MediaItem mediaItem })
        {
            return;
        }

        // If nothing is selected yet, scope the context menu actions to the tapped row.
        if (!HasAnySelectedItems())
        {
            mediaItem.IsSelected = true;
            UpdateSelectionIndicator();
        }

        var contextMenuCommand = ShowContextMenuAtAnchorCommand;
        if (contextMenuCommand?.CanExecute(anchorView) == true)
        {
            contextMenuCommand.Execute(anchorView);
        }
    }

    #endregion

    #region Helpers

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

    private void UpdateSelectionIndicator()
    {
        var overlayService = ResolveOverlayService();
        if (overlayService == null)
        {
            return;
        }

        if (HasAnySelectedItems())
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

        if (SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        throw new InvalidOperationException("MobileTableViewTemplateSelector requires PlaylistTemplate, TrackTemplate, or SkeletonTemplate.");
    }
}
