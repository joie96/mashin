namespace mashin.Views.Desktop.Controls;

/// <summary>
/// A label that scrolls overflowing text from right to left while hovered.
/// </summary>
public class MarqueeLabel : Label
{
    const uint ReturnDurationMs = 220;
    const double EndRevealPadding = 2;
    const double TailSafetyPadding = 4;

    PointerGestureRecognizer? pointerGesture;
    CancellationTokenSource? animationCancellation;
    Layout? clippedParent;

    bool originalParentClipState;
    bool isHovering;
    bool isAnimating;
    bool hasCapturedLayout;

    LineBreakMode originalLineBreakMode;
    int originalMaxLines;
    double originalWidthRequest;
    double originalMinimumWidthRequest;
    LayoutOptions originalHorizontalOptions;

    double contentWidth;
    double travelWidth;

    public static readonly BindableProperty PixelsPerSecondProperty =
        BindableProperty.Create(nameof(PixelsPerSecond), typeof(double), typeof(MarqueeLabel), 48d);

    public static readonly BindableProperty PauseMillisecondsProperty =
        BindableProperty.Create(nameof(PauseMilliseconds), typeof(int), typeof(MarqueeLabel), 500);

    public double PixelsPerSecond
    {
        get => (double)GetValue(PixelsPerSecondProperty);
        set => SetValue(PixelsPerSecondProperty, value);
    }

    public int PauseMilliseconds
    {
        get => (int)GetValue(PauseMillisecondsProperty);
        set => SetValue(PauseMillisecondsProperty, value);
    }

    public MarqueeLabel()
    {
        Loaded += OnLoaded;
        Unloaded += OnUnloaded;
    }

    void OnLoaded(object? sender, EventArgs e)
    {
        if (pointerGesture != null)
        {
            return;
        }

        pointerGesture = new PointerGestureRecognizer();
        pointerGesture.PointerEntered += OnPointerEntered;
        pointerGesture.PointerExited += OnPointerExited;
        GestureRecognizers.Add(pointerGesture);
    }

    void OnUnloaded(object? sender, EventArgs e)
    {
        StopAndReset();

        if (pointerGesture == null)
        {
            return;
        }

        pointerGesture.PointerEntered -= OnPointerEntered;
        pointerGesture.PointerExited -= OnPointerExited;
        GestureRecognizers.Remove(pointerGesture);
        pointerGesture = null;
    }

    void OnPointerEntered(object? sender, PointerEventArgs e)
    {
        if (isHovering)
        {
            return;
        }

        isHovering = true;
        _ = StartMarqueeAsync();
    }

    void OnPointerExited(object? sender, PointerEventArgs e)
    {
        if (!isHovering)
        {
            return;
        }

        isHovering = false;
        StopAndReset();
    }

    async Task StartMarqueeAsync()
    {
        if (!isHovering)
        {
            return;
        }

        await Task.Yield();

        if (!isHovering)
        {
            return;
        }

        CaptureLayout();

        if (!TryConfigureMarquee())
        {
            RestoreLayout();
            return;
        }

        ApplyParentClipping();
        StartAnimationLoop();
    }

    bool TryConfigureMarquee()
    {
        if (Width <= 0)
        {
            return false;
        }

        LineBreakMode = LineBreakMode.NoWrap;
        MaxLines = 1;

        contentWidth = SnapUpToDevicePixels(MeasureFullTextWidth() + TailSafetyPadding);
        if (contentWidth <= Width + 1)
        {
            return false;
        }

        travelWidth = SnapDownToDevicePixels(Math.Max(0, contentWidth - Width - EndRevealPadding));
        if (travelWidth <= 1)
        {
            return false;
        }

        MinimumWidthRequest = contentWidth;
        WidthRequest = contentWidth;
        HorizontalOptions = LayoutOptions.Start;
        return true;
    }

    void StartAnimationLoop()
    {
        if (isAnimating)
        {
            return;
        }

        isAnimating = true;
        animationCancellation?.Cancel();
        animationCancellation?.Dispose();
        animationCancellation = new CancellationTokenSource();
        _ = RunAnimationLoopAsync(animationCancellation.Token);
    }

    async Task RunAnimationLoopAsync(CancellationToken cancellationToken)
    {
        try
        {
            while (!cancellationToken.IsCancellationRequested)
            {
                if (!isHovering || travelWidth <= 1)
                {
                    break;
                }

                var speed = Math.Max(8d, PixelsPerSecond);
                var durationMs = (uint)Math.Max(300d, travelWidth / speed * 1000d);

                if (PauseMilliseconds > 0)
                {
                    await Task.Delay(PauseMilliseconds, cancellationToken);
                }

                if (!isHovering || cancellationToken.IsCancellationRequested)
                {
                    break;
                }

                await this.TranslateToAsync(-travelWidth, 0, durationMs, Easing.Linear);

                if (!isHovering || cancellationToken.IsCancellationRequested)
                {
                    break;
                }

                if (PauseMilliseconds > 0)
                {
                    await Task.Delay(PauseMilliseconds, cancellationToken);
                }

                if (!isHovering || cancellationToken.IsCancellationRequested)
                {
                    break;
                }

                await this.TranslateToAsync(0, 0, ReturnDurationMs, Easing.CubicOut);
            }
        }
        catch (TaskCanceledException)
        {
        }
        finally
        {
            isAnimating = false;
            if (!isHovering)
            {
                TranslationX = 0;
            }
        }
    }

    void StopAndReset()
    {
        animationCancellation?.Cancel();
        animationCancellation?.Dispose();
        animationCancellation = null;

        isAnimating = false;
        Microsoft.Maui.Controls.ViewExtensions.CancelAnimations(this);
        TranslationX = 0;

        RestoreParentClipping();
        RestoreLayout();
    }

    void CaptureLayout()
    {
        originalLineBreakMode = LineBreakMode;
        originalMaxLines = MaxLines;
        originalWidthRequest = WidthRequest;
        originalMinimumWidthRequest = MinimumWidthRequest;
        originalHorizontalOptions = HorizontalOptions;
        hasCapturedLayout = true;
    }

    void RestoreLayout()
    {
        if (!hasCapturedLayout)
        {
            return;
        }

        LineBreakMode = originalLineBreakMode;
        MaxLines = originalMaxLines;
        WidthRequest = originalWidthRequest;
        MinimumWidthRequest = originalMinimumWidthRequest;
        HorizontalOptions = originalHorizontalOptions;

        contentWidth = 0;
        travelWidth = 0;
        hasCapturedLayout = false;
    }

    double MeasureFullTextWidth()
    {
        if (string.IsNullOrWhiteSpace(Text) && FormattedText == null)
        {
            return 0;
        }

        var previousLineBreak = LineBreakMode;
        var previousMaxLines = MaxLines;
        var previousWidthRequest = WidthRequest;
        var previousMinimumWidthRequest = MinimumWidthRequest;
        var previousHorizontal = HorizontalOptions;

        LineBreakMode = LineBreakMode.NoWrap;
        MaxLines = 1;
        WidthRequest = -1;
        MinimumWidthRequest = -1;
        HorizontalOptions = LayoutOptions.Start;
        InvalidateMeasure();

        var measured = Measure(double.PositiveInfinity, double.PositiveInfinity).Width;

        LineBreakMode = previousLineBreak;
        MaxLines = previousMaxLines;
        WidthRequest = previousWidthRequest;
        MinimumWidthRequest = previousMinimumWidthRequest;
        HorizontalOptions = previousHorizontal;

        return measured;
    }

    void ApplyParentClipping()
    {
        if (Parent is not Layout parentLayout)
        {
            return;
        }

        if (ReferenceEquals(clippedParent, parentLayout))
        {
            return;
        }

        RestoreParentClipping();
        clippedParent = parentLayout;
        originalParentClipState = parentLayout.IsClippedToBounds;
        parentLayout.IsClippedToBounds = true;
    }

    void RestoreParentClipping()
    {
        if (clippedParent == null)
        {
            return;
        }

        clippedParent.IsClippedToBounds = originalParentClipState;
        clippedParent = null;
    }

    static double SnapUpToDevicePixels(double value)
    {
        if (value <= 0)
        {
            return 0;
        }

        var density = DeviceDisplay.MainDisplayInfo.Density;
        return density <= 0 ? Math.Ceiling(value) : Math.Ceiling(value * density) / density;
    }

    static double SnapDownToDevicePixels(double value)
    {
        if (value <= 0)
        {
            return 0;
        }

        var density = DeviceDisplay.MainDisplayInfo.Density;
        return density <= 0 ? Math.Floor(value) : Math.Floor(value * density) / density;
    }
}
