using System.Globalization;
using FFImageLoading.Work;

namespace mashin.Converters;

/// <summary>
/// Assigns high image loading priority to the first few indexed media items.
/// </summary>
public class MediaItemLoadingPriorityConverter : IValueConverter
{
    public int HighPriorityCount { get; set; } = 3;

    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        var highPriorityCount = ResolveHighPriorityCount(parameter);
        if (highPriorityCount <= 0)
        {
            return LoadingPriority.Normal;
        }

        if (TryGetIndex(value, out var index) && index >= 0 && index < highPriorityCount)
        {
            return LoadingPriority.High;
        }

        return LoadingPriority.Normal;
    }

    public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        throw new NotImplementedException("MediaItemLoadingPriorityConverter does not support two-way binding.");
    }

    private int ResolveHighPriorityCount(object? parameter)
    {
        if (parameter is string parameterText && int.TryParse(parameterText, out var parsedCount))
        {
            return parsedCount;
        }

        return HighPriorityCount;
    }

    private static bool TryGetIndex(object? value, out int index)
    {
        index = -1;
        if (value == null)
        {
            return false;
        }

        var indexProperty = value.GetType().GetProperty("Index");
        if (indexProperty?.PropertyType != typeof(int))
        {
            return false;
        }

        var rawValue = indexProperty.GetValue(value);
        if (rawValue is not int resolvedIndex)
        {
            return false;
        }

        index = resolvedIndex;
        return true;
    }
}