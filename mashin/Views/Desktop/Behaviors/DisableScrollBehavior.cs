using System;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.Maui.Controls;
using Microsoft.Maui.Platform;

namespace mashin.Views.Desktop.Behaviors;

#region Windows
#if WINDOWS
/// <summary>
/// Disables horizontal and/or vertical scrolling on a CollectionView by configuring its native ScrollViewer.
/// </summary>
public class DisableScrollBehavior : Behavior<CollectionView>
{
    public static readonly BindableProperty DisableHorizontalScrollProperty =
        BindableProperty.Create(
            nameof(DisableHorizontalScroll),
            typeof(bool),
            typeof(DisableScrollBehavior),
            defaultValue: true);

    public static readonly BindableProperty DisableVerticalScrollProperty =
        BindableProperty.Create(
            nameof(DisableVerticalScroll),
            typeof(bool),
            typeof(DisableScrollBehavior),
            defaultValue: true);

    public bool DisableHorizontalScroll
    {
        get => (bool)GetValue(DisableHorizontalScrollProperty);
        set => SetValue(DisableHorizontalScrollProperty, value);
    }

    public bool DisableVerticalScroll
    {
        get => (bool)GetValue(DisableVerticalScrollProperty);
        set => SetValue(DisableVerticalScrollProperty, value);
    }

    private CollectionView? _collectionView;
    private Microsoft.UI.Xaml.Controls.ScrollViewer? _scrollViewer;
    private CancellationTokenSource? _applyCts;

    protected override void OnAttachedTo(CollectionView bindable)
    {
        base.OnAttachedTo(bindable);
        _collectionView = bindable;
        bindable.HandlerChanged += OnHandlerChanged;
        bindable.Loaded += OnLoaded;
        bindable.SizeChanged += OnSizeChanged;
        bindable.PropertyChanged += OnCollectionViewPropertyChanged;
    }

    protected override void OnDetachingFrom(CollectionView bindable)
    {
        bindable.HandlerChanged -= OnHandlerChanged;
        bindable.Loaded -= OnLoaded;
        bindable.SizeChanged -= OnSizeChanged;
        bindable.PropertyChanged -= OnCollectionViewPropertyChanged;

        _applyCts?.Cancel();
        _applyCts?.Dispose();
        _applyCts = null;

        _collectionView = null;
        _scrollViewer = null;
        base.OnDetachingFrom(bindable);
    }

    private void OnHandlerChanged(object? sender, EventArgs e)
    {
        QueueApplyScrollDisabling();
    }

    private void OnLoaded(object? sender, EventArgs e)
    {
        QueueApplyScrollDisabling();
    }

    private void OnSizeChanged(object? sender, EventArgs e)
    {
        QueueApplyScrollDisabling();
    }

    private void OnCollectionViewPropertyChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(ItemsView.ItemsSource))
        {
            QueueApplyScrollDisabling();
        }
    }

    private void QueueApplyScrollDisabling()
    {
        _applyCts?.Cancel();
        _applyCts?.Dispose();
        _applyCts = new CancellationTokenSource();

        _ = TryApplyScrollDisablingAsync(_applyCts.Token);
    }

    private async Task TryApplyScrollDisablingAsync(CancellationToken ct)
    {
        const int maxAttempts = 6;
        var delay = 80;

        for (var attempt = 0; attempt < maxAttempts; attempt++)
        {
            if (ct.IsCancellationRequested)
            {
                return;
            }

            if (ApplyScrollDisabling())
            {
                return;
            }

            try
            {
                await Task.Delay(delay, ct);
            }
            catch (TaskCanceledException)
            {
                return;
            }

            delay = Math.Min(delay * 2, 500);
        }
    }

    private bool ApplyScrollDisabling()
    {
        if (_collectionView?.Handler?.PlatformView is not Microsoft.UI.Xaml.FrameworkElement platformView)
            return false;

        // Versuche den ScrollViewer zu finden
        var scrollViewer = FindScrollViewer(platformView);
        if (scrollViewer != null)
        {
            _scrollViewer = scrollViewer;
            ConfigureScrollViewer(scrollViewer);
            return true;
        }

        return false;
    }

    private Microsoft.UI.Xaml.Controls.ScrollViewer? FindScrollViewer(Microsoft.UI.Xaml.DependencyObject element)
    {
        if (element is Microsoft.UI.Xaml.Controls.ScrollViewer scrollViewer)
            return scrollViewer;

        var childCount = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChildrenCount(element);
        for (int i = 0; i < childCount; i++)
        {
            var child = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChild(element, i);
            var result = FindScrollViewer(child);
            if (result != null)
                return result;
        }

        return null;
    }

    private void ConfigureScrollViewer(Microsoft.UI.Xaml.Controls.ScrollViewer scrollViewer)
    {
        if (DisableHorizontalScroll)
        {
            scrollViewer.HorizontalScrollMode = Microsoft.UI.Xaml.Controls.ScrollMode.Disabled;
            scrollViewer.HorizontalScrollBarVisibility = Microsoft.UI.Xaml.Controls.ScrollBarVisibility.Hidden;
        }

        if (DisableVerticalScroll)
        {
            scrollViewer.VerticalScrollMode = Microsoft.UI.Xaml.Controls.ScrollMode.Disabled;
            scrollViewer.VerticalScrollBarVisibility = Microsoft.UI.Xaml.Controls.ScrollBarVisibility.Hidden;
        }

    }

}
#endif
#endregion

#region Unsupported platforms
#if !WINDOWS
public class DisableScrollBehavior : Behavior<CollectionView>
{
    public static readonly BindableProperty DisableHorizontalScrollProperty =
        BindableProperty.Create(
            nameof(DisableHorizontalScroll),
            typeof(bool),
            typeof(DisableScrollBehavior),
            defaultValue: true);

    public static readonly BindableProperty DisableVerticalScrollProperty =
        BindableProperty.Create(
            nameof(DisableVerticalScroll),
            typeof(bool),
            typeof(DisableScrollBehavior),
            defaultValue: true);

    public bool DisableHorizontalScroll
    {
        get => (bool)GetValue(DisableHorizontalScrollProperty);
        set => SetValue(DisableHorizontalScrollProperty, value);
    }

    public bool DisableVerticalScroll
    {
        get => (bool)GetValue(DisableVerticalScrollProperty);
        set => SetValue(DisableVerticalScrollProperty, value);
    }

    protected override void OnAttachedTo(CollectionView bindable)
    {
        throw new NotImplementedException("DisableScrollBehavior is currently only implemented for Windows.");
    }
}
#endif
#endregion