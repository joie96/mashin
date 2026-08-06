using System.Globalization;
using Microsoft.Maui.Controls;

namespace mashin.Converters;

/// <summary>
/// Compares two strings and returns true when both are equal (case-insensitive).
/// </summary>
public class StringEqualsConverter : IMultiValueConverter
{
    public object Convert(object[] values, Type targetType, object parameter, CultureInfo culture)
    {
        if (values.Length < 2)
        {
            return false;
        }

        var left = values[0] as string;
        var right = values[1] as string;

        if (string.IsNullOrWhiteSpace(left) || string.IsNullOrWhiteSpace(right))
        {
            return false;
        }

        return string.Equals(left.Trim(), right.Trim(), StringComparison.OrdinalIgnoreCase);
    }

    public object[] ConvertBack(object value, Type[] targetTypes, object parameter, CultureInfo culture)
    {
        if (targetTypes == null || targetTypes.Length == 0)
        {
            return Array.Empty<object>();
        }

        // This converter is used for display comparisons only.
        // Ignore reverse flow and keep source bindings unchanged.
        return targetTypes.Select(_ => Binding.DoNothing).ToArray();
    }
}

/// <summary>
/// Returns true when the input string is null, empty, or whitespace.
/// Pass converter parameter "invert" to invert the result.
/// </summary>
public class StringNullOrWhiteSpaceToBoolConverter : IValueConverter
{
    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        var isNullOrWhiteSpace = string.IsNullOrWhiteSpace(value as string);
        var invert = parameter is string text
            && string.Equals(text, "invert", StringComparison.OrdinalIgnoreCase);

        return invert ? !isNullOrWhiteSpace : isNullOrWhiteSpace;
    }

    public object ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        throw new NotImplementedException();
    }
}
