namespace mashin.Views.Desktop.Behaviors;

/// <summary>
/// Provides attached properties to add hover effects (background, scale, overlay, and text color) to MAUI views.
/// </summary>
public static class HoverBehavior
{
    static readonly BindableProperty BackgroundHoverRecognizerProperty =
        BindableProperty.CreateAttached("BackgroundHoverRecognizer", typeof(PointerGestureRecognizer), typeof(HoverBehavior), null);

    static readonly BindableProperty ScaleHoverRecognizerProperty =
        BindableProperty.CreateAttached("ScaleHoverRecognizer", typeof(PointerGestureRecognizer), typeof(HoverBehavior), null);

    static readonly BindableProperty OverlayHoverRecognizerProperty =
        BindableProperty.CreateAttached("OverlayHoverRecognizer", typeof(PointerGestureRecognizer), typeof(HoverBehavior), null);

    static readonly BindableProperty OverlayBaseOpacityProperty =
        BindableProperty.CreateAttached("OverlayBaseOpacity", typeof(double), typeof(HoverBehavior), 1d);

    static readonly BindableProperty OverlayBaseInputTransparentProperty =
        BindableProperty.CreateAttached("OverlayBaseInputTransparent", typeof(bool), typeof(HoverBehavior), false);

    static readonly BindableProperty TextHoverRecognizerProperty =
        BindableProperty.CreateAttached("TextHoverRecognizer", typeof(PointerGestureRecognizer), typeof(HoverBehavior), null);

    #region Background hover (Border)

    public static readonly BindableProperty HoverBackgroundColorProperty =
        BindableProperty.CreateAttached(
            "HoverBackgroundColor",
            typeof(Color),
            typeof(HoverBehavior),
            null,
            propertyChanged: OnHoverBackgroundColorChanged);

    public static readonly BindableProperty BaseBackgroundColorProperty =
        BindableProperty.CreateAttached(
            "BaseBackgroundColor",
            typeof(Color),
            typeof(HoverBehavior),
            null);

    public static Color? GetHoverBackgroundColor(BindableObject view)
        => (Color?)view.GetValue(HoverBackgroundColorProperty);

    public static void SetHoverBackgroundColor(BindableObject view, Color? value)
        => view.SetValue(HoverBackgroundColorProperty, value);

    public static Color? GetBaseBackgroundColor(BindableObject view)
        => (Color?)view.GetValue(BaseBackgroundColorProperty);

    public static void SetBaseBackgroundColor(BindableObject view, Color? value)
        => view.SetValue(BaseBackgroundColorProperty, value);

    static void OnHoverBackgroundColorChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not Border border)
        {
            return;
        }

        var pointerGesture = (PointerGestureRecognizer?)border.GetValue(BackgroundHoverRecognizerProperty);

        if (newValue is not Color)
        {
            if (pointerGesture != null)
            {
                border.GestureRecognizers.Remove(pointerGesture);
                border.ClearValue(BackgroundHoverRecognizerProperty);
            }

            return;
        }

        if (pointerGesture != null)
        {
            return;
        }

        pointerGesture = new PointerGestureRecognizer();
        pointerGesture.PointerEntered += (_, _) =>
        {
            if (GetHoverBackgroundColor(border) is Color hoverBackground)
            {
                border.BackgroundColor = hoverBackground;
            }
        };
        pointerGesture.PointerExited += (_, _) =>
        {
            if (GetBaseBackgroundColor(border) is Color baseBackground)
            {
                border.BackgroundColor = baseBackground;
                return;
            }

            border.ClearValue(Border.BackgroundColorProperty);
        };

        border.GestureRecognizers.Add(pointerGesture);
        border.SetValue(BackgroundHoverRecognizerProperty, pointerGesture);
    }

    #endregion

    #region Scale hover (View)

    public static readonly BindableProperty EnableScaleHoverProperty =
        BindableProperty.CreateAttached(
            "EnableScaleHover",
            typeof(bool),
            typeof(HoverBehavior),
            false,
            propertyChanged: OnEnableScaleHoverChanged);

    public static bool GetEnableScaleHover(BindableObject view)
        => (bool)view.GetValue(EnableScaleHoverProperty);

    public static void SetEnableScaleHover(BindableObject view, bool value)
        => view.SetValue(EnableScaleHoverProperty, value);

    static void OnEnableScaleHoverChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not View view || newValue is not bool enabled)
        {
            return;
        }

        var pointerGesture = (PointerGestureRecognizer?)view.GetValue(ScaleHoverRecognizerProperty);
        if (!enabled)
        {
            if (pointerGesture != null)
            {
                view.GestureRecognizers.Remove(pointerGesture);
                view.ClearValue(ScaleHoverRecognizerProperty);
            }

            return;
        }

        if (pointerGesture != null)
        {
            return;
        }

        var originalScale = view.Scale;

        pointerGesture = new PointerGestureRecognizer();
        pointerGesture.PointerEntered += (_, _) =>
        {
            view.Scale = 1.1;
        };
        pointerGesture.PointerExited += (_, _) =>
        {
            view.Scale = originalScale;
        };

        view.GestureRecognizers.Add(pointerGesture);
        view.SetValue(ScaleHoverRecognizerProperty, pointerGesture);
    }

    #endregion

    #region Show overlay on hover

    public static readonly BindableProperty ShowOnHoverTargetProperty =
        BindableProperty.CreateAttached(
            "ShowOnHoverTarget",
            typeof(VisualElement),
            typeof(HoverBehavior),
            null,
            propertyChanged: OnShowOnHoverTargetChanged);

    public static VisualElement? GetShowOnHoverTarget(BindableObject view)
        => (VisualElement?)view.GetValue(ShowOnHoverTargetProperty);

    public static void SetShowOnHoverTarget(BindableObject view, VisualElement? value)
        => view.SetValue(ShowOnHoverTargetProperty, value);

    static void OnShowOnHoverTargetChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not View host)
        {
            return;
        }

        if (oldValue is VisualElement oldTarget)
        {
            var oldBaseOpacity = (double)host.GetValue(OverlayBaseOpacityProperty);
            var oldBaseInputTransparent = (bool)host.GetValue(OverlayBaseInputTransparentProperty);
            oldTarget.Opacity = oldBaseOpacity;
            oldTarget.InputTransparent = oldBaseInputTransparent;
        }

        var pointerGesture = (PointerGestureRecognizer?)host.GetValue(OverlayHoverRecognizerProperty);

        if (newValue is not VisualElement target)
        {
            if (pointerGesture != null)
            {
                host.GestureRecognizers.Remove(pointerGesture);
                host.ClearValue(OverlayHoverRecognizerProperty);
            }

            return;
        }

        host.SetValue(OverlayBaseOpacityProperty, target.Opacity);
        host.SetValue(OverlayBaseInputTransparentProperty, target.InputTransparent);

        target.Opacity = 0;
        target.InputTransparent = true;

        if (pointerGesture != null)
        {
            return;
        }

        pointerGesture = new PointerGestureRecognizer();
        pointerGesture.PointerEntered += (_, _) =>
        {
            if (GetShowOnHoverTarget(host) is not VisualElement hoverTarget)
            {
                return;
            }

            var baseOpacity = (double)host.GetValue(OverlayBaseOpacityProperty);
            hoverTarget.Opacity = baseOpacity == 0 ? 1 : baseOpacity;
            hoverTarget.InputTransparent = false;
        };
        pointerGesture.PointerExited += (_, _) =>
        {
            if (GetShowOnHoverTarget(host) is not VisualElement hoverTarget)
            {
                return;
            }

            hoverTarget.Opacity = 0;
            hoverTarget.InputTransparent = true;
        };

        host.GestureRecognizers.Add(pointerGesture);
        host.SetValue(OverlayHoverRecognizerProperty, pointerGesture);
    }

    #endregion

    #region TextColor hover (Label)

    public static readonly BindableProperty HoverTextColorProperty =
        BindableProperty.CreateAttached(
            "HoverTextColor",
            typeof(Color),
            typeof(HoverBehavior),
            null,
            propertyChanged: OnHoverTextColorChanged);

    public static readonly BindableProperty BaseTextColorProperty =
        BindableProperty.CreateAttached(
            "BaseTextColor",
            typeof(Color),
            typeof(HoverBehavior),
            null);

    public static Color? GetHoverTextColor(BindableObject view)
        => (Color?)view.GetValue(HoverTextColorProperty);

    public static void SetHoverTextColor(BindableObject view, Color? value)
        => view.SetValue(HoverTextColorProperty, value);

    public static Color? GetBaseTextColor(BindableObject view)
        => (Color?)view.GetValue(BaseTextColorProperty);

    public static void SetBaseTextColor(BindableObject view, Color? value)
        => view.SetValue(BaseTextColorProperty, value);

    static void OnHoverTextColorChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not Label label)
        {
            return;
        }

        var pointerGesture = (PointerGestureRecognizer?)label.GetValue(TextHoverRecognizerProperty);
        if (newValue is not Color)
        {
            if (pointerGesture != null)
            {
                label.GestureRecognizers.Remove(pointerGesture);
                label.ClearValue(TextHoverRecognizerProperty);
            }

            return;
        }

        if (pointerGesture != null)
        {
            return;
        }

        pointerGesture = new PointerGestureRecognizer();
        pointerGesture.PointerEntered += (_, _) =>
        {
            if (GetHoverTextColor(label) is Color hoverText)
            {
                label.TextColor = hoverText;
            }
        };
        pointerGesture.PointerExited += (_, _) =>
        {
            if (GetBaseTextColor(label) is Color baseText)
            {
                label.TextColor = baseText;
                return;
            }

            label.ClearValue(Label.TextColorProperty);
        };

        label.GestureRecognizers.Add(pointerGesture);
        label.SetValue(TextHoverRecognizerProperty, pointerGesture);
    }

    #endregion
}