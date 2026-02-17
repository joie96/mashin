using System.Diagnostics;

namespace mashin.Views.Desktop.Behaviors;

public sealed class LayoutTraceBehavior : Behavior<VisualElement>
{
    public static readonly BindableProperty TagProperty =
        BindableProperty.Create(nameof(Tag), typeof(string), typeof(LayoutTraceBehavior), defaultValue: "");

    public static readonly BindableProperty MinIntervalMsProperty =
        BindableProperty.Create(nameof(MinIntervalMs), typeof(int), typeof(LayoutTraceBehavior), defaultValue: 100);

    private long _lastLogTicks;
    private double _lastWidth = double.NaN;
    private double _lastHeight = double.NaN;
    private long _lastMeasureWindowTicks;
    private int _measureRepeatCount;
    private bool _measureStackLogged;

    public string Tag
    {
        get => (string)GetValue(TagProperty);
        set => SetValue(TagProperty, value);
    }

    public int MinIntervalMs
    {
        get => (int)GetValue(MinIntervalMsProperty);
        set => SetValue(MinIntervalMsProperty, value);
    }

    protected override void OnAttachedTo(VisualElement bindable)
    {
        base.OnAttachedTo(bindable);
        bindable.SizeChanged += OnSizeChanged;
        bindable.MeasureInvalidated += OnMeasureInvalidated;
    }

    protected override void OnDetachingFrom(VisualElement bindable)
    {
        bindable.SizeChanged -= OnSizeChanged;
        bindable.MeasureInvalidated -= OnMeasureInvalidated;
        base.OnDetachingFrom(bindable);
    }

    private void OnSizeChanged(object? sender, EventArgs e)
    {
#if DEBUG
        if (sender is not VisualElement element)
        {
            return;
        }

        if (!ShouldLog())
        {
            return;
        }

        if (IsSameSize(element.Width, element.Height))
        {
            return;
        }

        _lastWidth = element.Width;
        _lastHeight = element.Height;
        Debug.WriteLine($"LayoutTrace {Tag} SizeChanged: {element.Width:0.##}x{element.Height:0.##}");
#endif
    }

    private void OnMeasureInvalidated(object? sender, EventArgs e)
    {
#if DEBUG
        if (sender is not VisualElement element)
        {
            return;
        }

        TrackMeasureRepeat();

        if (!ShouldLog())
        {
            return;
        }

        Debug.WriteLine($"LayoutTrace {Tag} MeasureInvalidated: {element.Width:0.##}x{element.Height:0.##}");
#endif
    }

    private void TrackMeasureRepeat()
    {
        var now = Stopwatch.GetTimestamp();
        var elapsedMs = (now - _lastMeasureWindowTicks) * 1000.0 / Stopwatch.Frequency;
        if (elapsedMs > 500)
        {
            _lastMeasureWindowTicks = now;
            _measureRepeatCount = 0;
            _measureStackLogged = false;
        }

        _measureRepeatCount++;
        if (!_measureStackLogged && _measureRepeatCount >= 20)
        {
            _measureStackLogged = true;
            //Debug.WriteLine($"LayoutTrace {Tag} MeasureInvalidated stack:\n{Environment.StackTrace}");
        }
    }

    private bool ShouldLog()
    {
        var now = Stopwatch.GetTimestamp();
        var elapsedMs = (now - _lastLogTicks) * 1000.0 / Stopwatch.Frequency;
        if (elapsedMs < MinIntervalMs)
        {
            return false;
        }

        _lastLogTicks = now;
        return true;
    }

    private bool IsSameSize(double width, double height)
    {
        var deltaWidth = Math.Abs(width - _lastWidth);
        var deltaHeight = Math.Abs(height - _lastHeight);
        return deltaWidth < 0.5 && deltaHeight < 0.5;
    }
}
