using System.Globalization;
using mashin.Models;

namespace mashin.Converters;

/// <summary>
/// Adds or updates the proxy image size on top of a base ImageUri.
/// Allowed proxy sizes: 0, 80, 160, 256, 512, 1024.
/// </summary>
public class ImageUriSizeConverter : IValueConverter
{
    public object? Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        if (value is not string imageUri || string.IsNullOrWhiteSpace(imageUri))
        {
            return value;
        }

        if (parameter is not string sizeText || !int.TryParse(sizeText, NumberStyles.Integer, CultureInfo.InvariantCulture, out var requestedSize))
        {
            return imageUri;
        }

        var normalizedSize = MediaItemImage.MapToAllowedSize(requestedSize);

        if (!Uri.TryCreate(imageUri, UriKind.Absolute, out var parsedUri)
            || imageUri.IndexOf("/imageproxy/", StringComparison.OrdinalIgnoreCase) < 0)
        {
            return imageUri;
        }

        var queryMap = ParseQuery(parsedUri.Query);
        if (normalizedSize > 0)
        {
            queryMap["size"] = normalizedSize.ToString(CultureInfo.InvariantCulture);
        }
        else
        {
            queryMap.Remove("size");
        }

        var rebuiltBase = imageUri.Split('?', 2)[0];
        var rebuiltQuery = BuildQuery(queryMap);
        return string.IsNullOrEmpty(rebuiltQuery) ? rebuiltBase : string.Concat(rebuiltBase, "?", rebuiltQuery);
    }

    public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        throw new NotImplementedException("ImageUriSizeConverter does not support two-way binding.");
    }

    private static Dictionary<string, string> ParseQuery(string query)
    {
        var result = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        if (string.IsNullOrWhiteSpace(query))
        {
            return result;
        }

        var trimmedQuery = query.TrimStart('?');
        if (trimmedQuery.Length == 0)
        {
            return result;
        }

        var parts = trimmedQuery.Split('&', StringSplitOptions.RemoveEmptyEntries);
        foreach (var part in parts)
        {
            var separatorIndex = part.IndexOf('=');
            if (separatorIndex <= 0)
            {
                continue;
            }

            var key = Uri.UnescapeDataString(part[..separatorIndex]);
            var rawValue = part[(separatorIndex + 1)..];
            var queryValue = Uri.UnescapeDataString(rawValue);
            result[key] = queryValue;
        }

        return result;
    }

    private static string BuildQuery(Dictionary<string, string> queryMap)
    {
        if (queryMap.Count == 0)
        {
            return string.Empty;
        }

        return string.Join("&", queryMap.Select(kvp => string.Concat(Uri.EscapeDataString(kvp.Key), "=", Uri.EscapeDataString(kvp.Value))));
    }
}