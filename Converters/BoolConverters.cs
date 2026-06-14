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
        if (value is not PlaybackState playState)
        {
            return false;
        }

        if (parameter is not string stateParameter || string.IsNullOrWhiteSpace(stateParameter))
        {
            return false;
        }

        var token = stateParameter.Trim().ToLowerInvariant();

        return token switch
        {
            "unknown" => playState == PlaybackState.Unknown,
            "stopped" => playState == PlaybackState.Idle,
            "paused" => playState is PlaybackState.Paused or PlaybackState.Idle or PlaybackState.Unknown,
            "buffering" => playState == PlaybackState.Buffering,
            "playing" => playState == PlaybackState.Playing,
            "seeking" => playState == PlaybackState.Buffering,
            _ => false,
        };
    }

    public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        throw new NotImplementedException("PlayStateToBoolConverter does not support two-way binding.");
    }
}