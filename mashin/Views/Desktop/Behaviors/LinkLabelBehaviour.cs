using System.Windows.Input;

namespace mashin.Views.Desktop.Behaviors;

/// <summary>
/// Adds link-like behavior to a Label, including hover styling and command execution on tap.
/// </summary>
public sealed class LinkLabelBehavior : Behavior<Label>
{
    public static readonly BindableProperty CommandProperty =
        BindableProperty.Create(nameof(Command), typeof(ICommand), typeof(LinkLabelBehavior));

    public static readonly BindableProperty CommandParameterProperty =
        BindableProperty.Create(nameof(CommandParameter), typeof(object), typeof(LinkLabelBehavior));

    public static readonly BindableProperty HoverTextColorProperty =
        BindableProperty.Create(nameof(HoverTextColor), typeof(Color), typeof(LinkLabelBehavior));

    public ICommand? Command
    {
        get => (ICommand?)GetValue(CommandProperty);
        set => SetValue(CommandProperty, value);
    }

    public object? CommandParameter
    {
        get => GetValue(CommandParameterProperty);
        set => SetValue(CommandParameterProperty, value);
    }

    public Color? HoverTextColor
    {
        get => (Color?)GetValue(HoverTextColorProperty);
        set => SetValue(HoverTextColorProperty, value);
    }

    Label? associatedObject;
    TapGestureRecognizer? tap;
    PointerGestureRecognizer? pointer;

    bool isHovering;
    Color? originalTextColor;
    TextDecorations originalTextDecorations;

    protected override void OnAttachedTo(Label bindable)
    {
        base.OnAttachedTo(bindable);

        associatedObject = bindable;
        BindingContext = bindable.BindingContext;
        bindable.BindingContextChanged += OnAssociatedObjectBindingContextChanged;

        tap = new TapGestureRecognizer();
        tap.Tapped += OnTapped;
        bindable.GestureRecognizers.Add(tap);

        pointer = new PointerGestureRecognizer();
        pointer.PointerEntered += OnPointerEntered;
        pointer.PointerExited += OnPointerExited;
        bindable.GestureRecognizers.Add(pointer);
    }

    protected override void OnDetachingFrom(Label bindable)
    {
        base.OnDetachingFrom(bindable);

        bindable.BindingContextChanged -= OnAssociatedObjectBindingContextChanged;

        if (tap != null)
        {
            tap.Tapped -= OnTapped;
            bindable.GestureRecognizers.Remove(tap);
            tap = null;
        }

        if (pointer != null)
        {
            pointer.PointerEntered -= OnPointerEntered;
            pointer.PointerExited -= OnPointerExited;
            bindable.GestureRecognizers.Remove(pointer);
            pointer = null;
        }

        associatedObject = null;
        BindingContext = null;
        originalTextColor = null;
    }

    void OnAssociatedObjectBindingContextChanged(object? sender, EventArgs e)
    {
        if (associatedObject != null)
        {
            BindingContext = associatedObject.BindingContext;
        }
    }

    void OnPointerEntered(object? sender, PointerEventArgs e)
    {
        if (associatedObject == null || isHovering)
        {
            return;
        }

        isHovering = true;
        originalTextDecorations = associatedObject.TextDecorations;
        associatedObject.TextDecorations = TextDecorations.Underline;

        var hoverTextColor = HoverTextColor;
        if (hoverTextColor == null)
        {
            return;
        }

        originalTextColor ??= associatedObject.TextColor;
        associatedObject.TextColor = hoverTextColor;
    }

    void OnPointerExited(object? sender, PointerEventArgs e)
    {
        if (associatedObject == null || !isHovering)
        {
            return;
        }

        isHovering = false;
        associatedObject.TextDecorations = originalTextDecorations;

        if (originalTextColor != null)
        {
            associatedObject.TextColor = originalTextColor;
            originalTextColor = null;
        }
    }

    void OnTapped(object? sender, TappedEventArgs e)
    {
        var command = Command;
        if (command == null)
        {
            return;
        }

        var parameter = IsSet(CommandParameterProperty)
            ? CommandParameter
            : associatedObject?.BindingContext;

        if (command.CanExecute(parameter))
        {
            command.Execute(parameter);
        }
    }
}