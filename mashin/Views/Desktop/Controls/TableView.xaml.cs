using mashin.Models;
using mashin.Services;
using mashin.ViewModels;
using mashin.Views.Desktop;
using Microsoft.Maui.ApplicationModel;
using System.Collections;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.Windows.Input;

namespace mashin.Views.Desktop.Controls;

public partial class TableView : ContentView
{
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

    public static readonly BindableProperty MediaActionsProperty =
        BindableProperty.Create(nameof(MediaActions), typeof(IMediaItemActions), typeof(TableView));

    public static readonly BindableProperty ShowContextMenuAtAnchorCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtAnchorCommand), typeof(ICommand), typeof(TableView));

    public static readonly BindableProperty ShowContextMenuAtPositionCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtPositionCommand), typeof(ICommand), typeof(TableView));

    public static readonly BindableProperty PlaybackContextItemProperty =
        BindableProperty.Create(nameof(PlaybackContextItem), typeof(object), typeof(TableView));

    #endregion

    #region Fields

    private readonly ObservableCollection<object> _selectedItems = new();
    private IKeyboardService? _keyboardService;
    private MainViewModel? _mainViewModel;
    private Track? _currentTrack;
    private int? _anchorIndex;
    private bool _isCheckboxClick;

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

    public object? PlaybackContextItem
    {
        get => GetValue(PlaybackContextItemProperty);
        set => SetValue(PlaybackContextItemProperty, value);
    }

    #endregion

    #region Construction

    public TableView()
    {
        InitializeComponent();

        SelectedItems = _selectedItems;
        UpdateHeaderSelectionState();
    }

    protected override void OnHandlerChanged()
    {
        base.OnHandlerChanged();
    }

    private void OnTableViewLoaded(object? sender, EventArgs e)
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

        AttachPlaybackStateSource();
        UpdatePlayingStateForVisibleItems();

    }

    private void OnTableViewUnloaded(object? sender, EventArgs e)
    {
        
        // Cleanup
        if (_keyboardService != null)
        {
            _keyboardService.KeyActionDetected -= OnKeyActionDetected;
            _keyboardService = null;
        }

        DetachPlaybackStateSource();
        ResetPlayingStateForVisibleItems();

        PrimaryInfoTappedCommand = null;
        SecondaryInfoTappedCommand = null;
        ShowContextMenuAtAnchorCommand = null;
        ShowContextMenuAtPositionCommand = null;
        MediaActions = null;
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

    }

    #endregion

    #region Property Changed Handlers

    private static void OnItemsSourceChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not TableView view)
        {
            return;
        }

        view._anchorIndex = null;
        view.ClearAllSelections();
        view.SyncSelectionAndHeaderState(clickedItem: null);
        view.UpdatePlayingStateForVisibleItems();
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
        if (sender is not Border border || border.BindingContext is not MediaItem item)
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

        SyncSelectionAndHeaderState(item);
    }

    private void OnRowSecondaryPointerPressed(object? sender, PointerEventArgs e)
    {
        if (sender is not BindableObject bindable || bindable.BindingContext is not MediaItem item)
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
        if (sender is not Border border || border.BindingContext is not MediaItem item || MediaActions == null)
        {
            return;
        }

        SelectSingle(item);
        SyncSelectionAndHeaderState(item);

        // Context queue: play item index
        if (PlaybackContextItem is PlayerQueue queueContext)
        {
            if (!string.IsNullOrWhiteSpace(queueContext.QueueId))
            {
                var zeroBasedIndex = item is Track queueTrack && queueTrack.Index > 0
                    ? queueTrack.Index - 1
                    : (GetIndexOf(item) ?? -1);

                if (zeroBasedIndex >= 0)
                {
                    await MediaActions.PlayIndexAsync(queueContext.QueueId, zeroBasedIndex);
                    return;
                }
            }
        }

        // Context album, playlist, etc.: Play the container and start at clicked item if context is available.
        if (PlaybackContextItem != null && !(PlaybackContextItem is PlayerQueue)) 
        {
            await MediaActions.PlayMediaAsync(PlaybackContextItem, item);
            return;
        }

        // Fallback: Play clicked item immediately and append following visible items.
        await MediaActions.PlayMediaAsync(item);

        var itemsToQueue = GetItemsAfterIndex(item);
        if (itemsToQueue.Count > 0)
        {
            await MediaActions.PlayMediaNextAsync(itemsToQueue);
        }
    }

    private async void OnPlayOverlayClicked(object? sender, Microsoft.Maui.Controls.TappedEventArgs e)
    {
        if (sender is not Border playOverlay || MediaActions == null)
        {
            return;
        }

        // The Border inherits the BindingContext from the DataTemplate
        if (playOverlay.BindingContext is not MediaItem item)
        {
            return;
        }

        SelectSingle(item);
        SyncSelectionAndHeaderState(item);

        // Context queue: play item index
        if (PlaybackContextItem is PlayerQueue queueContext)
        {
            if (!string.IsNullOrWhiteSpace(queueContext.QueueId))
            {
                var zeroBasedIndex = item is Track queueTrack && queueTrack.Index > 0
                    ? queueTrack.Index - 1
                    : (GetIndexOf(item) ?? -1);

                if (zeroBasedIndex >= 0)
                {
                    await MediaActions.PlayIndexAsync(queueContext.QueueId, zeroBasedIndex);
                    return;
                }
            }
        }

        // Context album, playlist, etc.: Play the container and start at clicked item if context is available.
        if (PlaybackContextItem != null && !(PlaybackContextItem is PlayerQueue)) 
        {
            await MediaActions.PlayMediaAsync(PlaybackContextItem, item);
            return;
        }

        // Fallback: Play clicked item immediately and append following visible items.
        await MediaActions.PlayMediaAsync(item);

        var itemsToQueue = GetItemsAfterIndex(item);
        if (itemsToQueue.Count > 0)
        {
            await MediaActions.PlayMediaNextAsync(itemsToQueue);
        }
    }

    private async void OnFavoriteIconTapped(object? sender, Microsoft.Maui.Controls.TappedEventArgs e)
    {
        if (sender is not Label label || label.BindingContext is not MediaItem item || MediaActions == null)
        {
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

    #endregion

    #region Context Menu

    private void OnMoreIconTapped(object? sender, Microsoft.Maui.Controls.TappedEventArgs e)
    {
        if (sender is not Label label)
        {
            return;
        }

        if (label.BindingContext is MediaItem selectable)
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

    #region Playing state tracking

    private void AttachPlaybackStateSource()
    {
        if (_mainViewModel != null)
        {
            return;
        }

        var mainPage = Application.Current?.Windows.FirstOrDefault()?.Page as MainPage;
        if (mainPage?.BindingContext is not MainViewModel vm)
        {
            return;
        }

        _mainViewModel = vm;
        _currentTrack = vm.CurrentTrack;
        _mainViewModel.CurrentTrackChanged += OnCurrentTrackChanged;
    }

    private void DetachPlaybackStateSource()
    {
        if (_mainViewModel == null)
        {
            return;
        }

        _mainViewModel.CurrentTrackChanged -= OnCurrentTrackChanged;
        _mainViewModel = null;
        _currentTrack = null;
    }

    private void OnCurrentTrackChanged(object? sender, Track? track)
    {
        _currentTrack = track;

        if (MainThread.IsMainThread)
        {
            UpdatePlayingStateForVisibleItems();
            return;
        }

        MainThread.BeginInvokeOnMainThread(UpdatePlayingStateForVisibleItems);
    }

    private void UpdatePlayingStateForVisibleItems()
    {
        var activeTrack = _currentTrack;

        foreach (var item in EnumerateSelectableItems())
        {
            var isPlaying = activeTrack != null && IsSameTrack(item, activeTrack);
            if (item.IsPlaying != isPlaying)
            {
                item.IsPlaying = isPlaying;
            }
        }
    }

    private void ResetPlayingStateForVisibleItems()
    {
        foreach (var item in EnumerateSelectableItems())
        {
            if (item.IsPlaying)
            {
                item.IsPlaying = false;
            }
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

    private List<MediaItem> GetItemsAfterIndex(MediaItem startItem)
    {
        var startIndex = GetIndexOf(startItem);
        if (startIndex is null)
        {
            return new List<MediaItem>();
        }

        var index = 0;
        return EnumerateSelectableItems()
            .Where(item => index++ > startIndex.Value)
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
            }
        }
    }

    #endregion
}

#region TemplateSelector
public sealed class TableViewTemplateSelector : DataTemplateSelector
{
    public DataTemplate? TrackTemplate { get; set; }
    public DataTemplate? SkeletonTemplate { get; set; }

    protected override DataTemplate OnSelectTemplate(object item, BindableObject container)
    {
        if (item is TableViewSkeleton && SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        if (TrackTemplate != null)
        {
            return TrackTemplate;
        }

        if (SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        throw new InvalidOperationException("TableViewTemplateSelector requires TrackTemplate or SkeletonTemplate.");
    }
}
#endregion