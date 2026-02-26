using System.Windows.Input;

namespace mashin.Views.Desktop.Behaviors;

/// <summary>
/// Executes commands when a Slider drag operation starts/completes, passing the current value.
/// </summary>
public class SliderDragCompletedBehavior : Behavior<Slider>
{
    private Slider? _slider;

    public static readonly BindableProperty CommandProperty =
        BindableProperty.Create(nameof(Command), typeof(ICommand), typeof(SliderDragCompletedBehavior));

    public static readonly BindableProperty StartedCommandProperty =
        BindableProperty.Create(nameof(StartedCommand), typeof(ICommand), typeof(SliderDragCompletedBehavior));

    public ICommand? Command
    {
        get => (ICommand?)GetValue(CommandProperty);
        set => SetValue(CommandProperty, value);
    }

    public ICommand? StartedCommand
    {
        get => (ICommand?)GetValue(StartedCommandProperty);
        set => SetValue(StartedCommandProperty, value);
    }

    protected override void OnAttachedTo(Slider slider)
    {
        base.OnAttachedTo(slider);
        _slider = slider;
        BindingContext = slider.BindingContext;
        slider.BindingContextChanged += OnSliderBindingContextChanged;
        slider.DragStarted += OnDragStarted;
        slider.DragCompleted += OnDragCompleted;
    }

    protected override void OnDetachingFrom(Slider slider)
    {
        base.OnDetachingFrom(slider);
        slider.BindingContextChanged -= OnSliderBindingContextChanged;
        slider.DragStarted -= OnDragStarted;
        slider.DragCompleted -= OnDragCompleted;
        _slider = null;
    }

    private void OnSliderBindingContextChanged(object? sender, EventArgs e)
    {
        if (_slider != null)
        {
            BindingContext = _slider.BindingContext;
        }
    }

    private void OnDragCompleted(object? sender, EventArgs e)
    {
        if (sender is Slider slider && Command?.CanExecute(slider.Value) == true)
        {
            Command.Execute(slider.Value);
        }
    }

    private void OnDragStarted(object? sender, EventArgs e)
    {
        if (sender is Slider slider && StartedCommand?.CanExecute(slider.Value) == true)
        {
            StartedCommand.Execute(slider.Value);
        }
    }
}
