namespace mashin.Views.Desktop.Behaviors;

/// <summary>
/// Animates a label as a horizontal marquee only while it is hovered,
/// and only when the full text is wider than the visible label area.
/// </summary>
public sealed class HoverMarqueeBehavior : Behavior<Label>
{
    #region Constants

    const double MeasureSafetyPadding = 24;
    const double EndRevealPadding = 8;

    #endregion

    #region Bindable Properties

    public static readonly BindableProperty PixelsPerSecondProperty =
        BindableProperty.Create(nameof(PixelsPerSecond), typeof(double), typeof(HoverMarqueeBehavior), 48d);

    public static readonly BindableProperty PauseMillisecondsProperty =
        BindableProperty.Create(nameof(PauseMilliseconds), typeof(int), typeof(HoverMarqueeBehavior), 500);

    #endregion

    #region Fields

    Label? associatedObject;
    PointerGestureRecognizer? pointerGesture;
    Layout? clippedParent;
    CancellationTokenSource? animationCancellation;

    bool isHovering;
    bool isAnimating;
    bool hasCapturedTextLayout;
    bool originalParentClipState;
    LineBreakMode originalLineBreakMode;
    int originalMaxLines;
    double originalWidthRequest;
    LayoutOptions originalHorizontalOptions;
    double hoverViewportWidth;
    double hoverContentWidth;

    #endregion

    #region Public API

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

    #endregion

    #region Lifecycle

    protected override void OnAttachedTo(Label bindable)
    {
        base.OnAttachedTo(bindable);

        associatedObject = bindable;

        pointerGesture = new PointerGestureRecognizer();
        pointerGesture.PointerEntered += OnPointerEntered;
        pointerGesture.PointerExited += OnPointerExited;
        bindable.GestureRecognizers.Add(pointerGesture);
    }

    protected override void OnDetachingFrom(Label bindable)
    {
        base.OnDetachingFrom(bindable);

        if (pointerGesture != null)
        {
            pointerGesture.PointerEntered -= OnPointerEntered;
            pointerGesture.PointerExited -= OnPointerExited;
            bindable.GestureRecognizers.Remove(pointerGesture);
            pointerGesture = null;
        }

        StopAnimation(resetPosition: true);

        RestoreParentClipping();

        RestoreTextLayout();

        associatedObject = null;
    }

    #endregion

    #region Parent Clipping

    void TryApplyParentClipping()
    {
        if (associatedObject == null)
        {
            return;
        }

        if (associatedObject.Parent is not Layout parentLayout)
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

    #endregion

    #region Hover Handling

    void OnPointerEntered(object? sender, PointerEventArgs e)
    {
        if (isHovering)
        {
            return;
        }

        isHovering = true;
        _ = StartHoverMarqueeAsync();
    }

    void OnPointerExited(object? sender, PointerEventArgs e)
    {
        if (!isHovering)
        {
            return;
        }

        isHovering = false;
        StopAnimation(resetPosition: true);
        RestoreTextLayout();
        RestoreParentClipping();
    }

    #endregion

    #region Animation

    async Task StartHoverMarqueeAsync()
    {
        if (!isHovering || associatedObject == null)
        {
            return;
        }

        await Task.Yield();

        if (!isHovering || associatedObject == null)
        {
            return;
        }

        CaptureTextLayout();
        TryApplyParentClipping();

        if (!TryApplyMarqueeTextLayout())
        {
            RestoreTextLayout();
            RestoreParentClipping();
            return;
        }

        StartAnimationLoop();
    }

    void StartAnimationLoop()
    {
        if (isAnimating || associatedObject == null)
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
                if (!CanContinueAnimation(out var overflow))
                {
                    break;
                }

                var label = associatedObject;
                if (label == null)
                {
                    break;
                }

                var speed = Math.Max(8d, PixelsPerSecond);
                var totalTravel = overflow + EndRevealPadding;
                var durationMilliseconds = (uint)Math.Max(300d, totalTravel / speed * 1000d);

                if (PauseMilliseconds > 0)
                {
                    await Task.Delay(PauseMilliseconds, cancellationToken);
                }

                if (cancellationToken.IsCancellationRequested || !isHovering)
                {
                    break;
                }

                var forwardCanceled = await label.TranslateTo(-totalTravel, 0, durationMilliseconds, Easing.Linear);
                if (forwardCanceled || cancellationToken.IsCancellationRequested || !isHovering)
                {
                    break;
                }

                if (PauseMilliseconds > 0)
                {
                    await Task.Delay(PauseMilliseconds, cancellationToken);
                }

                if (cancellationToken.IsCancellationRequested || !isHovering)
                {
                    break;
                }

                var returnCanceled = await label.TranslateTo(0, 0, 220, Easing.CubicOut);
                if (returnCanceled)
                {
                    break;
                }
            }
        }
        catch (TaskCanceledException)
        {
        }
        finally
        {
            EndAnimation(resetPosition: !isHovering);
        }
    }

    bool CanContinueAnimation(out double overflow)
    {
        overflow = GetOverflowWidth();
        return associatedObject != null && isHovering && overflow > 1;
    }

    void StopAnimation(bool resetPosition)
    {
        animationCancellation?.Cancel();
        animationCancellation?.Dispose();
        animationCancellation = null;
        isAnimating = false;

        if (associatedObject == null)
        {
            return;
        }

        associatedObject.CancelAnimations();
        if (resetPosition)
        {
            associatedObject.TranslationX = 0;
        }
    }

    void EndAnimation(bool resetPosition)
    {
        isAnimating = false;
        StopAnimation(resetPosition);
    }

    #endregion

    #region Layout and Measurement

    void CaptureTextLayout()
    {
        if (associatedObject == null)
        {
            return;
        }

        originalLineBreakMode = associatedObject.LineBreakMode;
        originalMaxLines = associatedObject.MaxLines;
        originalWidthRequest = associatedObject.WidthRequest;
        originalHorizontalOptions = associatedObject.HorizontalOptions;
        hasCapturedTextLayout = true;
    }

    double GetOverflowWidth()
    {
        if (associatedObject == null)
        {
            return 0;
        }

        if (isHovering && hoverViewportWidth > 0 && hoverContentWidth > 0)
        {
            return Math.Max(0, hoverContentWidth - hoverViewportWidth);
        }

        var label = associatedObject;
        if (label.Width <= 0 || label.Height <= 0 || string.IsNullOrWhiteSpace(label.Text))
        {
            return 0;
        }

        var measured = label.Measure(double.PositiveInfinity, double.PositiveInfinity);
        return Math.Max(0, measured.Width - label.Width);
    }

    bool TryApplyMarqueeTextLayout()
    {
        if (associatedObject == null)
        {
            return false;
        }

        var label = associatedObject;

        hoverViewportWidth = label.Width;
        if (hoverViewportWidth <= 0)
        {
            return false;
        }

        label.LineBreakMode = LineBreakMode.NoWrap;
        label.MaxLines = 1;

        var contentWidth = MeasureFullTextWidth(label);
        hoverContentWidth = Math.Ceiling(contentWidth) + GetSafetyPadding(label);

        if (hoverContentWidth <= hoverViewportWidth + 1)
        {
            return false;
        }

        label.WidthRequest = hoverContentWidth;
        label.HorizontalOptions = LayoutOptions.Start;
        return true;
    }

    void RestoreTextLayout()
    {
        if (associatedObject == null || !hasCapturedTextLayout)
        {
            return;
        }

        associatedObject.LineBreakMode = originalLineBreakMode;
        associatedObject.MaxLines = originalMaxLines;
        associatedObject.WidthRequest = originalWidthRequest;
        associatedObject.HorizontalOptions = originalHorizontalOptions;
        hasCapturedTextLayout = false;
        hoverViewportWidth = 0;
        hoverContentWidth = 0;
    }

    static double MeasureFullTextWidth(Label label)
    {
        if (string.IsNullOrWhiteSpace(label.Text) && label.FormattedText == null)
        {
            return 0;
        }

        var originalWidthRequest = label.WidthRequest;
        var originalHorizontalOptions = label.HorizontalOptions;

        label.WidthRequest = -1;
        label.HorizontalOptions = LayoutOptions.Start;
        label.InvalidateMeasure();

        var measured = label.Measure(double.PositiveInfinity, double.PositiveInfinity).Width;

        label.WidthRequest = originalWidthRequest;
        label.HorizontalOptions = originalHorizontalOptions;

        return measured;
    }

    static double GetSafetyPadding(Label label)
    {
        var characterPadding = Math.Max(0, label.CharacterSpacing) * 2;
        var fontPadding = Math.Ceiling(Math.Max(0, label.FontSize));
        return Math.Max(MeasureSafetyPadding, fontPadding + characterPadding + 8);
    }

    #endregion
}