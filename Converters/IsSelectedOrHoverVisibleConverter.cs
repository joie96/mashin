using System.Globalization;
using Microsoft.Maui.Controls;

namespace mashin.Converters;

/// <summary>
/// Determines the visibility of the selection checkbox of a rowview item by combining selection state and hover overlay visibility.
/// </summary>
public class IsSelectedOrHoverVisibleConverter : IMultiValueConverter
{
    public object Convert(object[] values, Type targetType, object parameter, CultureInfo culture)
    {
        // Expect two bindings: [0] => IsSelected (bool), [1] => HoverOverlay.Opacity (double/float).
        if (values.Length < 2)
            return false;

        // Checkbox must stay visible while the item itself is selected.
        var isSelected = values[0] is bool b && b;

        // When not selected, we only show the checkbox while the hover overlay is visible.
        var opacity = values[1] switch
        {
            double d => d,
            float f => f,
            _ => 0d
        };

        return isSelected || opacity > 0.0;
    }

    public object[] ConvertBack(object value, Type[] targetTypes, object parameter, CultureInfo culture) =>
        throw new NotImplementedException();
}