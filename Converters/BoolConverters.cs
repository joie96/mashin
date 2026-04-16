using System.Globalization;
using mashin.Services;

namespace mashin.Converters;

/// <summary>
/// Inverts a boolean value (true → false, false → true).
/// </summary>
public class InvertedBoolConverter : IValueConverter
{
    public object? Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        return value is bool boolValue && !boolValue;
    }

    public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        return value is bool boolValue && !boolValue;
    }
}

/// <summary>
/// Converts a boolean to opacity.
/// Default: true → 1.0, false → 0.5
/// With parameter "0": true → 1.0, false → 0.0
/// </summary>
public class BoolToOpacityConverter : IValueConverter
{
    public object? Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        if (value is not bool boolValue)
            return 0.5;

        if (!boolValue)
        {
            // Check if parameter specifies the false opacity value
            if (parameter is string paramStr && double.TryParse(paramStr, out double falseOpacity))
            {
                return falseOpacity;
            }
            return 0.5; // Default false opacity
        }

        return 1.0; // True opacity
    }

    /// <exception cref="NotImplementedException">Two-way binding not supported.</exception>
    public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        throw new NotImplementedException("BoolToOpacityConverter does not support two-way binding.");
    }
}

/// <summary>
/// Converts an integer to boolean (0 → true, >0 → false).
/// Useful for showing placeholders when collections are empty.
/// </summary>
public class IntToBoolConverter : IValueConverter
{
    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        if (value is int count)
        {
            return count == 0;
        }
        return true;
    }

    public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        throw new NotImplementedException("IntToBoolConverter does not support two-way binding.");
    }
}

public class PlayStateToBoolConverter : IValueConverter
{
    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        if (value is not PlayerPlayState playState)
        {
            return false;
        }

        var mode = (parameter as string)?.Trim().ToLowerInvariant();
        return mode switch
        {
            "playing" => playState.State is PlayerPlaybackState.Playing or PlayerPlaybackState.Seeking,
            "not-playing" => playState.State is not (PlayerPlaybackState.Playing or PlayerPlaybackState.Seeking),
            "buffering" => playState.State == PlayerPlaybackState.Buffering,
            "seeking" => playState.State == PlayerPlaybackState.Seeking,
            "show-play-icon" => playState.State is not (PlayerPlaybackState.Playing or PlayerPlaybackState.Seeking or PlayerPlaybackState.Buffering),
            "show-pause-icon" => playState.State is PlayerPlaybackState.Playing or PlayerPlaybackState.Seeking,
            _ => false,
        };
    }

    public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        throw new NotImplementedException("PlayStateToBoolConverter does not support two-way binding.");
    }
}