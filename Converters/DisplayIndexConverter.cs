using System.Globalization;

namespace mashin.Converters;

/// <summary>
/// Converts a zero-based index to a one-based value for UI display.
/// </summary>
public class DisplayIndexConverter : IValueConverter
{
    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        if (value == null)
        {
            return string.Empty;
        }

        if (value is int intValue)
        {
            return intValue + 1;
        }

        if (int.TryParse(value.ToString(), out var parsed))
        {
            return parsed + 1;
        }

        return string.Empty;
    }

    public object ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        throw new NotImplementedException("DisplayIndexConverter does not support two-way binding.");
    }
}
