namespace mashin.Converters;

public class RemoveBlankLinesConverter : IValueConverter
{
    public object? Convert(object? value, Type targetType, object? parameter, System.Globalization.CultureInfo culture)
    {
        if (value is not string text)
        {
            return value;
        }

        var lines = text.Split(new[] { "\r\n", "\r", "\n" }, StringSplitOptions.None);
        var nonEmptyLines = lines.Where(line => !string.IsNullOrWhiteSpace(line));
        
        return string.Join("\n", nonEmptyLines);
    }

    public object? ConvertBack(object? value, Type targetType, object? parameter, System.Globalization.CultureInfo culture)
    {
        throw new NotImplementedException();
    }
}