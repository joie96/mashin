using System.Windows.Input;

namespace mashin.Views.Desktop.Behaviors;

/// <summary>
/// Executes a command when a Slider drag operation completes, passing the current value.
/// </summary>
public class SliderDragCompletedBehavior : Behavior<Slider>
{
    public static readonly BindableProperty CommandProperty =
        BindableProperty.Create(nameof(Command), typeof(ICommand), typeof(SliderDragCompletedBehavior));

    public ICommand? Command
    {
        get => (ICommand?)GetValue(CommandProperty);
        set => SetValue(CommandProperty, value);
    }

    protected override void OnAttachedTo(Slider slider)
    {
        base.OnAttachedTo(slider);
        slider.DragCompleted += OnDragCompleted;
    }

    protected override void OnDetachingFrom(Slider slider)
    {
        base.OnDetachingFrom(slider);
        slider.DragCompleted -= OnDragCompleted;
    }

    private void OnDragCompleted(object? sender, EventArgs e)
    {
        if (sender is Slider slider && Command?.CanExecute(slider.Value) == true)
        {
            Command.Execute(slider.Value);
        }
    }
}
