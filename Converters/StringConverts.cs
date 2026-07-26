using System.Globalization;
using mashin.Models;

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
        throw new NotImplementedException();
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

/// <summary>
/// Builds the playlist secondary line text (track count and total duration) for list rows.
/// </summary>
public class PlaylistMetadataTextConverter : IValueConverter
{
    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        if (value is not Playlist playlist)
        {
            return string.Empty;
        }

        var tracksCount = Math.Max(0, playlist.TracksCount);
        var totalDurationSeconds = Math.Max(0, playlist.TotalDurationSeconds);

        var titlesText = tracksCount == 1 ? "1 Titel" : $"{tracksCount} Titel";
        var durationText = FormatTotalDuration(totalDurationSeconds);

        return $"{titlesText} • {durationText}";
    }

    public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        throw new NotImplementedException();
    }

    private static string FormatTotalDuration(int totalSeconds)
    {
        if (totalSeconds <= 0)
        {
            return "0m";
        }

        var timeSpan = TimeSpan.FromSeconds(totalSeconds);
        var totalHours = (int)timeSpan.TotalHours;

        if (totalHours > 0)
        {
            return $"{totalHours}h {timeSpan.Minutes}m";
        }

        return $"{Math.Max(1, timeSpan.Minutes)}m";
    }
}