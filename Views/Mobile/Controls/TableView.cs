using mashin.Models;
using System.Linq;
using System.Windows.Input;

namespace mashin.Views.Mobile.Controls;

public partial class TableView : ContentView
{
    #region Fields

    private const int LongPressDurationMilliseconds = 420;
    private readonly Dictionary<MediaItem, CancellationTokenSource> _pendingLongPresses = new();
    private readonly HashSet<MediaItem> _suppressNextTap = new();

    #endregion

    #region Bindable properties

    public static readonly BindableProperty ItemsSourceProperty =
        BindableProperty.Create(nameof(ItemsSource), typeof(IEnumerable<object>), typeof(TableView));

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

    #endregion

    #region Input handling

    private void OnRowPointerPressed(object? sender, PointerEventArgs e)
    {
        if (sender is not BindableObject { BindingContext: MediaItem mediaItem })
        {
            return;
        }

        var anchorView = sender as View;

        CancelPendingLongPress(mediaItem);

        var cts = new CancellationTokenSource();
        _pendingLongPresses[mediaItem] = cts;

        _ = DetectLongPressAsync(mediaItem, anchorView, cts.Token);
    }

    private void OnRowPointerReleased(object? sender, PointerEventArgs e)
    {
        if (sender is not BindableObject { BindingContext: MediaItem mediaItem })
        {
            return;
        }

        CancelPendingLongPress(mediaItem);
    }

    private void OnRowPointerExited(object? sender, PointerEventArgs e)
    {
        if (sender is not BindableObject { BindingContext: MediaItem mediaItem })
        {
            return;
        }

        CancelPendingLongPress(mediaItem);
    }

    private async Task DetectLongPressAsync(MediaItem mediaItem, View? anchorView, CancellationToken token)
    {
        try
        {
            await Task.Delay(LongPressDurationMilliseconds, token);

            Dispatcher.Dispatch(() =>
            {
                // Long-press opens the context menu only when this specific row is selected.
                if (mediaItem.IsSelected)
                {
                    var contextMenuCommand = ShowContextMenuAtAnchorCommand;
                    if (anchorView != null && contextMenuCommand?.CanExecute(anchorView) == true)
                    {
                        contextMenuCommand.Execute(anchorView);
                        _suppressNextTap.Add(mediaItem);
                        return;
                    }
                }

                var longPressCommand = LongPressCommand;
                if (longPressCommand?.CanExecute(mediaItem) == true)
                {
                    longPressCommand.Execute(mediaItem);
                }
                else
                {
                    mediaItem.IsSelected = !mediaItem.IsSelected;
                }

                _suppressNextTap.Add(mediaItem);
            });
        }
        catch (TaskCanceledException)
        {
            // Ignored: row was released/exited before long-press threshold.
        }
    }

    private void OnRowTapped(object? sender, TappedEventArgs e)
    {
        if (sender is not BindableObject { BindingContext: MediaItem mediaItem })
        {
            return;
        }

        if (_suppressNextTap.Remove(mediaItem))
        {
            return;
        }

        // If at least one row is selected, keep taps in selection mode until all are deselected.
        if (HasAnySelectedItems())
        {
            mediaItem.IsSelected = !mediaItem.IsSelected;
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
        }

        var contextMenuCommand = ShowContextMenuAtAnchorCommand;
        if (contextMenuCommand?.CanExecute(anchorView) == true)
        {
            contextMenuCommand.Execute(anchorView);
        }
    }

    #endregion

    #region Helpers

    private void CancelPendingLongPress(MediaItem mediaItem)
    {
        if (!_pendingLongPresses.TryGetValue(mediaItem, out var cts))
        {
            return;
        }

        _pendingLongPresses.Remove(mediaItem);
        cts.Cancel();
        cts.Dispose();
    }

    private bool HasAnySelectedItems()
    {
        if (ItemsSource == null)
        {
            return false;
        }

        return ItemsSource.OfType<MediaItem>().Any(item => item.IsSelected);
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
