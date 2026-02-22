using mashin.Models;
using mashin.Services;
using System.Collections;
using System.Collections.ObjectModel;
using System.Diagnostics;
using mashin.Extensions;
using System.Windows.Input;


namespace mashin.Views.Desktop.Controls;

public partial class RowView : ContentView
{
    #region Bindable properties

    public static readonly BindableProperty ItemsSourceProperty =
        BindableProperty.Create(nameof(ItemsSource), typeof(IEnumerable<object>), typeof(RowView), propertyChanged: OnItemsSourceChanged);

    public static readonly BindableProperty SelectedItemsProperty =
        BindableProperty.Create(nameof(SelectedItems), typeof(IList), typeof(RowView), defaultBindingMode: BindingMode.TwoWay);

    public static readonly BindableProperty PrimaryInfoTappedCommandProperty =
        BindableProperty.Create(nameof(PrimaryInfoTappedCommand), typeof(ICommand), typeof(RowView));

    public static readonly BindableProperty SecondaryInfoTappedCommandProperty =
        BindableProperty.Create(nameof(SecondaryInfoTappedCommand), typeof(ICommand), typeof(RowView));

    public static readonly BindableProperty MediaActionsProperty =
        BindableProperty.Create(nameof(MediaActions), typeof(IMediaItemActions), typeof(RowView));

    public static readonly BindableProperty ShowContextMenuAtAnchorCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtAnchorCommand), typeof(ICommand), typeof(RowView));

    public static readonly BindableProperty ShowContextMenuAtPositionCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtPositionCommand), typeof(ICommand), typeof(RowView));

    public static readonly BindableProperty IsAllSelectedProperty =
    BindableProperty.Create(nameof(IsAllSelected), typeof(bool), typeof(RowView), defaultValue: false);

    public static readonly BindableProperty ItemCornerRadiusProperty =
        BindableProperty.Create(nameof(ItemCornerRadius), typeof(float), typeof(RowView), defaultValue: 8f);


    #endregion

    #region Fields

    private readonly ObservableCollection<object> _selectedItems = new();
    private IKeyboardService? _keyboardService;
    private int? _anchorIndex;
    private bool _isCheckboxClick;
    private double _currentScrollPosition = 0;
    private const double ScrollPixelStep = 780;
    private CancellationTokenSource? _scrollCts;

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

    #endregion

    #region Construction

    public RowView()
    {
        InitializeComponent();
        
        SelectedItems = _selectedItems;
    }

    protected override void OnHandlerChanged()
    {
        base.OnHandlerChanged();
    }

    private void OnRowViewLoaded(object? sender, EventArgs e)
    {
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

    }

    private void OnRowViewUnloaded(object? sender, EventArgs e)
    {
        // Cleanup
        if (_keyboardService != null)
        {
            _keyboardService.KeyActionDetected -= OnKeyActionDetected;
            _keyboardService = null;
        }

        _scrollCts?.Cancel();
        _scrollCts?.Dispose();
        _scrollCts = null;

        PrimaryInfoTappedCommand = null;
        SecondaryInfoTappedCommand = null;
        ShowContextMenuAtAnchorCommand = null;
        ShowContextMenuAtPositionCommand = null;
        MediaActions = null;

        if (collectionView != null)
        {
            collectionView.ItemsSource = null;
            collectionView.BindingContext = null;
            collectionView.Handler?.DisconnectHandler();
        }

        ItemsSource = null;
        BindingContext = null;

        // Collections bereinigen
        _selectedItems.Clear();
    }

    #endregion

    #region Property Changed Handlers

    private static void OnItemsSourceChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not RowView view)
        {
            return;
        }

        view._anchorIndex = null;
        view._currentScrollPosition = 0;
        view.ClearAllSelections();
        view.SyncSelectionState();
    }

    #endregion

    #region Scroll Operations

    private void OnCollectionViewScrolled(object? sender, ItemsViewScrolledEventArgs e)
    {
    }

    private void OnItemLoaded(object? sender, EventArgs e)
    {
        if (sender is BindableObject bindable && bindable.BindingContext is MediaItem item)
        {
            //Debug.WriteLine($"RowView item loaded: {item.Name} ({item.MediaType})");
        }
    }

    private void OnItemUnloaded(object? sender, EventArgs e)
    {
        if (sender is BindableObject bindable && bindable.BindingContext is MediaItem item)
        {
            //Debug.WriteLine($"RowView item unloaded: {item.Name} ({item.MediaType})");
        }
    }

    private async void OnScrollLeftClicked(object? sender, EventArgs e)
    {
        _scrollCts?.Cancel();
        _scrollCts = new CancellationTokenSource();

        var targetPosition = Math.Max(0, _currentScrollPosition - ScrollPixelStep);
        _currentScrollPosition = targetPosition;

        await collectionView.ScrollToPixelSmoothAsync(
            targetPosition,
            duration: 50,
            ct: _scrollCts.Token);  
    }

    private async void OnScrollRightClicked(object? sender, EventArgs e)
    {
        _scrollCts?.Cancel();
        _scrollCts = new CancellationTokenSource();

        var targetPosition = collectionView.ClampHorizontalTargetX(_currentScrollPosition + ScrollPixelStep);
        _currentScrollPosition = targetPosition;
        
        await collectionView.ScrollToPixelSmoothAsync(
            targetPosition,
            duration: 50,
            ct: _scrollCts.Token);
  
    }

    public void ResetScrollPosition()
    {
        _currentScrollPosition = 0;
        collectionView?.ScrollTo(0, animate: false);
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
    public DataTemplate? SkeletonTemplate { get; set; }

    protected override DataTemplate OnSelectTemplate(object item, BindableObject container)
    {
        if (item is RowViewSkeleton && SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        if (AlbumTemplate != null)
        {
            return AlbumTemplate;
        }

        if (SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        throw new InvalidOperationException("RowViewTemplateSelector requires AlbumTemplate or SkeletonTemplate.");
    }
}
#endregion