using System.Runtime.CompilerServices;

namespace mashin.Views.Desktop.Behaviors;

/// <summary>
/// Applies selection styling to a Border and its child Labels using attached properties.
/// </summary>
public static class SelectionBehavior
{
    sealed class BorderState
    {
        public bool IsLoadedHooked { get; set; }
        public bool IsApplyQueued { get; set; }
        public bool HasLastTextColor { get; set; }
        public Color? LastTextColor { get; set; }
    }

    static readonly ConditionalWeakTable<Border, BorderState> BorderStates = new();

    public static readonly BindableProperty IsSelectedBindingProperty =
        BindableProperty.CreateAttached(
            "IsSelectedBinding",
            typeof(bool),
            typeof(SelectionBehavior),
            false,
            propertyChanged: OnChanged);

    public static readonly BindableProperty BaseBackgroundColorProperty =
        BindableProperty.CreateAttached(
            "BaseBackgroundColor",
            typeof(Color),
            typeof(SelectionBehavior),
            null,
            propertyChanged: OnChanged);

    public static readonly BindableProperty BaseTextColorProperty =
        BindableProperty.CreateAttached(
            "BaseTextColor",
            typeof(Color),
            typeof(SelectionBehavior),
            null,
            propertyChanged: OnChanged);

    public static readonly BindableProperty SelectionBackgroundColorProperty =
        BindableProperty.CreateAttached(
            "SelectionBackgroundColor",
            typeof(Color),
            typeof(SelectionBehavior),
            null,
            propertyChanged: OnChanged);

    public static readonly BindableProperty SelectionTextColorProperty =
        BindableProperty.CreateAttached(
            "SelectionTextColor",
            typeof(Color),
            typeof(SelectionBehavior),
            null,
            propertyChanged: OnChanged);

    public static bool GetIsSelectedBinding(BindableObject view)
        => (bool)view.GetValue(IsSelectedBindingProperty);

    public static void SetIsSelectedBinding(BindableObject view, bool value)
        => view.SetValue(IsSelectedBindingProperty, value);

    public static Color? GetBaseBackgroundColor(BindableObject view)
        => (Color?)view.GetValue(BaseBackgroundColorProperty);

    public static void SetBaseBackgroundColor(BindableObject view, Color? value)
        => view.SetValue(BaseBackgroundColorProperty, value);

    public static Color? GetBaseTextColor(BindableObject view)
        => (Color?)view.GetValue(BaseTextColorProperty);

    public static void SetBaseTextColor(BindableObject view, Color? value)
        => view.SetValue(BaseTextColorProperty, value);

    public static Color? GetSelectionBackgroundColor(BindableObject view)
        => (Color?)view.GetValue(SelectionBackgroundColorProperty);

    public static void SetSelectionBackgroundColor(BindableObject view, Color? value)
        => view.SetValue(SelectionBackgroundColorProperty, value);

    public static Color? GetSelectionTextColor(BindableObject view)
        => (Color?)view.GetValue(SelectionTextColorProperty);

    public static void SetSelectionTextColor(BindableObject view, Color? value)
        => view.SetValue(SelectionTextColorProperty, value);

    static void OnChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not Border border)
        {
            return;
        }

        QueueApply(border);
    }

    static void QueueApply(Border border)
    {
        var state = BorderStates.GetOrCreateValue(border);

        if (!border.IsLoaded)
        {
            if (!state.IsLoadedHooked)
            {
                state.IsLoadedHooked = true;
                border.Loaded += OnBorderLoaded;
                border.Unloaded += OnBorderUnloaded;
            }

            return;
        }

        if (state.IsApplyQueued)
        {
            return;
        }

        state.IsApplyQueued = true;
        border.Dispatcher.Dispatch(() =>
        {
            var currentState = BorderStates.GetOrCreateValue(border);
            currentState.IsApplyQueued = false;
            Apply(border, currentState);
        });
    }

    static void OnBorderLoaded(object? sender, EventArgs e)
    {
        if (sender is not Border border)
        {
            return;
        }

        QueueApply(border);
    }

    static void OnBorderUnloaded(object? sender, EventArgs e)
    {
        if (sender is not Border border)
        {
            return;
        }

        if (!BorderStates.TryGetValue(border, out var state))
        {
            return;
        }

        state.IsApplyQueued = false;
        state.HasLastTextColor = false;
    }

    static void Apply(Border border, BorderState state)
    {
        var isSelected = GetIsSelectedBinding(border);

        // If BaseBackgroundColor is not specified, default to Transparent.
        var baseBackground = GetBaseBackgroundColor(border) ?? Colors.Transparent;
        var baseText = GetBaseTextColor(border);

        var selectionBackground = GetSelectionBackgroundColor(border);
        var selectionText = GetSelectionTextColor(border);

        Color? targetTextColor = null;

        if (isSelected)
        {
            if (selectionBackground != null)
            {
                border.BackgroundColor = selectionBackground;
            }

            if (selectionText != null)
            {
                targetTextColor = selectionText;
            }
        }
        else
        {
            border.BackgroundColor = baseBackground;

            if (baseText != null)
            {
                targetTextColor = baseText;
            }
        }

        if (targetTextColor == null)
        {
            state.HasLastTextColor = false;
            state.LastTextColor = null;
            return;
        }

        if (state.HasLastTextColor && state.LastTextColor == targetTextColor)
        {
            return;
        }

        state.HasLastTextColor = true;
        state.LastTextColor = targetTextColor;
        SetLabelsTextColor(border, targetTextColor);
    }

    static void SetLabelsTextColor(Border border, Color color)
    {
        foreach (var label in EnumerateLabels(border))
        {
            label.TextColor = color;
        }
    }

    static IEnumerable<Label> EnumerateLabels(Border border)
    {
        if (border.Content is not VisualElement root)
        {
            yield break;
        }

        foreach (var label in EnumerateLabelsRecursive(root))
        {
            yield return label;
        }
    }

    static IEnumerable<Label> EnumerateLabelsRecursive(VisualElement element)
    {
        if (element is Label label)
        {
            yield return label;
            yield break;
        }

        if (element is Border border && border.Content is VisualElement borderContent)
        {
            foreach (var nested in EnumerateLabelsRecursive(borderContent))
            {
                yield return nested;
            }
        }

        if (element is ContentView contentView && contentView.Content is VisualElement content)
        {
            foreach (var nested in EnumerateLabelsRecursive(content))
            {
                yield return nested;
            }
        }

        if (element is Layout layout)
        {
            foreach (var child in layout.Children)
            {
                if (child is VisualElement ve)
                {
                    foreach (var nested in EnumerateLabelsRecursive(ve))
                    {
                        yield return nested;
                    }
                }
            }
        }
    }
}