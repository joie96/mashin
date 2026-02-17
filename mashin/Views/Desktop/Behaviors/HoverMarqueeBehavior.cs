namespace mashin.Views.Desktop.Behaviors;

/// <summary>
/// Animates a label as a horizontal marquee only while it is hovered,
/// and only when the full text is wider than the visible label area.
/// </summary>
public sealed class HoverMarqueeBehavior : Behavior<Label>
{
    #region Constants

    const string TranslationAnimationName = "HoverMarqueeBehavior_Translate";

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

    bool isHovering;
    bool isAnimating;
    int animationVersion;
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
        CaptureTextLayout();
        TryApplyParentClipping();
        ApplyMarqueeTextLayout();
        TryStartAnimation();
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

    void TryStartAnimation()
    {
        if (isAnimating || associatedObject == null)
        {
            return;
        }

        var overflow = GetOverflowWidth();
        if (overflow <= 1)
        {
            return;
        }

        isAnimating = true;
        StartForwardPhase();
    }

    void StartForwardPhase()
    {
        if (!CanContinueAnimation(out var overflow))
        {
            EndAnimation(resetPosition: true);
            return;
        }

        var speed = Math.Max(8d, PixelsPerSecond);
        var durationMilliseconds = (uint)Math.Max(300d, overflow / speed * 1000d);

        ScheduleOrRun(() =>
        {
            if (associatedObject == null)
            {
                EndAnimation(resetPosition: true);
                return;
            }

            associatedObject.Animate(
                name: TranslationAnimationName,
                callback: value => associatedObject.TranslationX = value,
                start: associatedObject.TranslationX,
                end: -overflow,
                length: durationMilliseconds,
                easing: Easing.Linear,
                finished: (_, canceled) =>
                {
                    if (canceled || !isHovering)
                    {
                        EndAnimation(resetPosition: true);
                        return;
                    }

                    StartReturnPhase();
                });
        });
    }

    void StartReturnPhase()
    {
        ScheduleOrRun(() =>
        {
            if (associatedObject == null)
            {
                EndAnimation(resetPosition: true);
                return;
            }

            associatedObject.Animate(
                name: TranslationAnimationName,
                callback: value => associatedObject.TranslationX = value,
                start: associatedObject.TranslationX,
                end: 0,
                length: 220,
                easing: Easing.CubicOut,
                finished: (_, canceled) =>
                {
                    if (canceled || !isHovering)
                    {
                        EndAnimation(resetPosition: true);
                        return;
                    }

                    if (CanContinueAnimation(out _))
                    {
                        StartForwardPhase();
                        return;
                    }

                    EndAnimation(resetPosition: false);
                });
        });
    }

    bool CanContinueAnimation(out double overflow)
    {
        overflow = GetOverflowWidth();
        return associatedObject != null && isHovering && overflow > 1;
    }

    void ScheduleOrRun(Action action)
    {
        if (associatedObject == null)
        {
            return;
        }

        if (PauseMilliseconds <= 0)
        {
            action();
            return;
        }

        var version = animationVersion;
        associatedObject.Dispatcher.DispatchDelayed(TimeSpan.FromMilliseconds(PauseMilliseconds), () =>
        {
            if (version != animationVersion || !isHovering || associatedObject == null)
            {
                return;
            }

            action();
        });
    }

    void StopAnimation(bool resetPosition)
    {
        animationVersion++;
        isAnimating = false;

        if (associatedObject == null)
        {
            return;
        }

        associatedObject.AbortAnimation(TranslationAnimationName);
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

        var measured = label.Measure(double.PositiveInfinity, label.Height);
        return Math.Max(0, measured.Width - label.Width);
    }

    void ApplyMarqueeTextLayout()
    {
        if (associatedObject == null)
        {
            return;
        }

        var label = associatedObject;

        hoverViewportWidth = label.Width;
        label.LineBreakMode = LineBreakMode.NoWrap;
        label.MaxLines = 1;

        if (hoverViewportWidth <= 0)
        {
            return;
        }

        hoverContentWidth = Math.Ceiling(MeasureFullTextWidth(label)) + 12;

        if (hoverContentWidth <= hoverViewportWidth + 1)
        {
            return;
        }

        label.WidthRequest = hoverContentWidth;
        label.HorizontalOptions = LayoutOptions.Start;
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
        if (string.IsNullOrWhiteSpace(label.Text))
        {
            return 0;
        }

        var measured = label.Measure(double.PositiveInfinity, double.PositiveInfinity);
        return measured.Width;
    }

    #endregion
}