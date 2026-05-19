using mashin.Models;
using System.Windows.Input;

namespace mashin.Views.Mobile.Controls;

public partial class TableView : ContentView
{
    #region Fields

    private const int LongPressDurationMilliseconds = 420;
    private readonly Dictionary<Playlist, CancellationTokenSource> _pendingLongPresses = new();
    private readonly HashSet<Playlist> _suppressNextTap = new();

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
        if (sender is not BindableObject { BindingContext: Playlist playlist })
        {
            return;
        }

        CancelPendingLongPress(playlist);

        var cts = new CancellationTokenSource();
        _pendingLongPresses[playlist] = cts;

        _ = DetectLongPressAsync(playlist, cts.Token);
    }

    private void OnRowPointerReleased(object? sender, PointerEventArgs e)
    {
        if (sender is not BindableObject { BindingContext: Playlist playlist })
        {
            return;
        }

        CancelPendingLongPress(playlist);
    }

    private void OnRowPointerExited(object? sender, PointerEventArgs e)
    {
        if (sender is not BindableObject { BindingContext: Playlist playlist })
        {
            return;
        }

        CancelPendingLongPress(playlist);
    }

    private async Task DetectLongPressAsync(Playlist playlist, CancellationToken token)
    {
        try
        {
            await Task.Delay(LongPressDurationMilliseconds, token);

            Dispatcher.Dispatch(() =>
            {
                var longPressCommand = LongPressCommand;
                if (longPressCommand?.CanExecute(playlist) == true)
                {
                    longPressCommand.Execute(playlist);
                }
                else
                {
                    playlist.IsSelected = !playlist.IsSelected;
                }

                _suppressNextTap.Add(playlist);
            });
        }
        catch (TaskCanceledException)
        {
            // Ignored: row was released/exited before long-press threshold.
        }
    }

    private void OnRowTapped(object? sender, TappedEventArgs e)
    {
        if (sender is not BindableObject { BindingContext: Playlist playlist })
        {
            return;
        }

        if (_suppressNextTap.Remove(playlist))
        {
            return;
        }

        var command = ShortPressCommand;
        if (command?.CanExecute(playlist) == true)
        {
            command.Execute(playlist);
        }
    }

    #endregion

    #region Helpers

    private void CancelPendingLongPress(Playlist playlist)
    {
        if (!_pendingLongPresses.TryGetValue(playlist, out var cts))
        {
            return;
        }

        _pendingLongPresses.Remove(playlist);
        cts.Cancel();
        cts.Dispose();
    }

    #endregion
}

public sealed class MobileTableViewTemplateSelector : DataTemplateSelector
{
    public DataTemplate? PlaylistTemplate { get; set; }
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

        if (PlaylistTemplate != null)
        {
            return PlaylistTemplate;
        }

        if (SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        throw new InvalidOperationException("MobileTableViewTemplateSelector requires PlaylistTemplate or SkeletonTemplate.");
    }
}
