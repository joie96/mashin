using Microsoft.Maui.Controls.Shapes;

namespace mashin.Views.Desktop.Controls;

public partial class SkeletonLoader : Border
{
    public static readonly BindableProperty CornerRadiusProperty =
        BindableProperty.Create(nameof(CornerRadius), typeof(CornerRadius), typeof(SkeletonLoader), new CornerRadius(4),
            propertyChanged: OnCornerRadiusChanged);

    public CornerRadius CornerRadius
    {
        get => (CornerRadius)GetValue(CornerRadiusProperty);
        set => SetValue(CornerRadiusProperty, value);
    }

    public SkeletonLoader()
    {
        InitializeComponent();
        UpdateCornerRadius(CornerRadius);
    }

    private static void OnCornerRadiusChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not SkeletonLoader loader || newValue is not CornerRadius radius)
        {
            return;
        }

        loader.UpdateCornerRadius(radius);
    }

    private void UpdateCornerRadius(CornerRadius radius)
    {
        StrokeShape = new RoundRectangle { CornerRadius = radius };
    }
}