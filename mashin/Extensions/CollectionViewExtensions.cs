namespace mashin.Extensions;

/// <summary>
/// Provides extension methods for <see cref="CollectionView"/> controls.
/// </summary>
public static class CollectionViewExtensions
{
    /// <summary>
    /// Smoothly scrolls the CollectionView to the specified pixel position with animated easing.
    /// The position is automatically clamped to valid scroll range.
    /// Currently only implemented for Windows platform.
    /// </summary>
    /// <param name="collectionView">The CollectionView to scroll.</param>
    /// <param name="targetX">The target horizontal pixel position.</param>
    /// <param name="duration">The duration of the scroll animation in milliseconds. Default is 300.</param>
    /// <param name="ct">A cancellation token to cancel the scroll operation.</param>
    /// <exception cref="NotImplementedException">Thrown on non-Windows platforms.</exception>
    public static async Task ScrollToPixelSmoothAsync(
        this CollectionView collectionView,
        double targetX,
        int duration = 300,
        CancellationToken ct = default)
    {
#if WINDOWS
        if (collectionView?.Handler?.PlatformView == null)
        {
            return;
        }

        if (collectionView.Handler.PlatformView is not Microsoft.UI.Xaml.Controls.ListViewBase listView)
        {
            return;
        }

        var scrollViewer = FindScrollViewer(listView);
        if (scrollViewer == null)
        {
            return;
        }

        var clampedX = Math.Clamp(targetX, 0, scrollViewer.ScrollableWidth);

        try
        {
            await AnimateScrollAsync(scrollViewer, clampedX, duration, ct);
        }
        catch (TaskCanceledException)
        {
            // Jump directly to target position without animation
            scrollViewer.ChangeView(clampedX, null, null, disableAnimation: true);
        }
        catch (OperationCanceledException)
        {
            // Jump directly to target position without animation
            scrollViewer.ChangeView(clampedX, null, null, disableAnimation: true);
        }
#else
        throw new NotImplementedException("Smooth scrolling is currently only implemented for Windows. Other platforms coming soon.");
#endif
    }

    public static double ClampHorizontalTargetX(this CollectionView collectionView, double targetX)
    {
#if WINDOWS
        if (collectionView?.Handler?.PlatformView is Microsoft.UI.Xaml.Controls.ListViewBase listView)
        {
            var scrollViewer = FindScrollViewer(listView);
            if (scrollViewer != null)
            {
                return Math.Clamp(targetX, 0, scrollViewer.ScrollableWidth);
            }
        }
#endif

        return Math.Max(0, targetX);
    }

#if WINDOWS
    private static Microsoft.UI.Xaml.Controls.ScrollViewer? FindScrollViewer(Microsoft.UI.Xaml.DependencyObject parent)
    {
        var childCount = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChildrenCount(parent);

        for (int i = 0; i < childCount; i++)
        {
            var child = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChild(parent, i);

            if (child is Microsoft.UI.Xaml.Controls.ScrollViewer scrollViewer)
            {
                return scrollViewer;
            }

            var result = FindScrollViewer(child);
            if (result != null)
            {
                return result;
            }
        }

        return null;
    }

    private static async Task AnimateScrollAsync(Microsoft.UI.Xaml.Controls.ScrollViewer scrollViewer, double targetX, int duration, CancellationToken ct)
    {
        var startX = scrollViewer.HorizontalOffset;
        var distance = targetX - startX;
        var steps = 30;
        var stepDelay = duration / steps;

        for (int i = 0; i <= steps; i++)
        {
            ct.ThrowIfCancellationRequested();

            var progress = (double)i / steps;
            var easedProgress = EaseInOutQuad(progress);
            var currentX = startX + (distance * easedProgress);

            scrollViewer.ChangeView(currentX, null, null, disableAnimation: true);
            await Task.Delay(stepDelay, ct);
        }
    }

    private static double EaseInOutQuad(double t)
    {
        return t < 0.5 ? 2 * t * t : 1 - Math.Pow(-2 * t + 2, 2) / 2;
    }
        private static double EaseOutCubic(double t)
    {
        return 1 - Math.Pow(1 - t, 3);
    }
#endif
}